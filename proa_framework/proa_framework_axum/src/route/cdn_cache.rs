// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Route-owned edge policies. Storage and regeneration belong to the CDN.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, HttpBody},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    response::Response,
    routing::MethodRouter,
};
use proa_core::{DataLoader, RenderMode};
use proa_framework_core::RouteResponse as CoreRouteResponse;
use tower::{Layer, Service};

use super::context::ROUTE_CONTEXT;
use super::local_executor::LocalStreamExecutor;
use super::page_cache::{bypass_request, PageCacheLayer};
use super::response::{
    finish_deferred_render, prepare_deferred_framework_stream, DeferredFrameworkStream,
};

/// The policy for documents no cache may store.
///
/// Dynamic documents carry no validator, so `no-cache` would keep a copy the
/// browser must download again before every reuse, while leaving possibly
/// personalized HTML on disk. `no-store` gives the same freshness without
/// storage, and matches what a page cache sends when it bypasses a request.
const PRIVATE_NO_STORE: &str = "private, no-store";

/// Marks a response rendered by the framework: a `RouteResponse` document or
/// client payload, or a Markdown representation.
#[derive(Clone, Copy)]
pub(super) struct FrameworkDocument;

/// Marks a request a page cache may store.
///
/// A page cache inside `RouteContextLayer` records it on the route context.
/// One outside inserts it into the request, and `RouteContextLayer` copies it
/// there. Either way, a streamed document renders in order for the cache.
#[derive(Clone, Copy)]
pub(super) struct MayStore;

/// Marks a framework document streamed in order for a page cache. Once its
/// body ends, its bytes equal the buffered render.
#[derive(Clone, Default)]
pub(super) struct CacheableStream(Option<LocalStreamExecutor>);

/// Send `Cache-Control: private, no-store` on a framework document when no
/// handler, layer, head hook, or page cache chose a cache policy.
///
/// Without one, RFC 9111 heuristic freshness and CDN defaults may store
/// dynamic output. Any cache policy header the application set is kept.
///
/// `RouteContextLayer` applies this after finishing any deferred render. A
/// page cache inside it sets its own policy first. A page cache outside it
/// marks the requests it may store, and their documents keep no default: the
/// cache would otherwise read the default as the page's own `private`
/// policy and never store it. That cache sets a policy on every response.
fn default_document_policy(mut response: Response, outer_cache: bool) -> Response {
    if response
        .extensions_mut()
        .remove::<FrameworkDocument>()
        .is_some()
        && !outer_cache
    {
        let headers = response.headers();
        let chosen = headers.contains_key(header::CACHE_CONTROL)
            || headers.contains_key(header::EXPIRES)
            || CDN_HEADERS.iter().any(|name| headers.contains_key(*name));
        if !chosen {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(PRIVATE_NO_STORE),
            );
        }
    }
    response
}

/// Finish a route response, or hand an unopened in-order stream to the
/// outer cache so its lookup deadline only covers the handler response.
pub(super) async fn finish_route_document(
    mut response: Response,
    executor: LocalStreamExecutor,
    outer_cache: bool,
) -> Response {
    if outer_cache
        && response
            .extensions()
            .get::<DeferredFrameworkStream>()
            .is_some()
    {
        if let Some(stream) = response.extensions_mut().get_mut::<CacheableStream>() {
            stream.0 = Some(executor);
            return default_document_policy(response, outer_cache);
        }
    }
    default_document_policy(
        prepare_deferred_framework_stream(response, executor).await,
        outer_cache,
    )
}

/// Record that a page cache may store this request's document.
pub(super) fn may_store(request: &mut Request<Body>) {
    request.extensions_mut().insert(MayStore);
    let _ = ROUTE_CONTEXT.try_with(|cx| cx.borrow_mut().set_may_store());
}

/// Stream a document in order when a page cache may store it, and say
/// whether it did.
///
/// A cache needs the complete document. In order, a stream's bytes equal
/// the buffered render, with no fallbacks or boundary scripts, while its
/// response head is still known before the body. The cache can then bypass
/// an uncacheable page without waiting for its body, and a slow page keeps
/// streaming instead of running into deadlines that end at the response
/// head. Requests the cache bypasses keep the route's render mode.
pub(super) fn stream_in_order_for_page_cache<L: DataLoader + 'static>(
    response: CoreRouteResponse<L>,
) -> (CoreRouteResponse<L>, bool) {
    let cached = ROUTE_CONTEXT
        .try_with(|cx| cx.borrow().may_store())
        .unwrap_or(false);
    if cached && response.is_streaming() {
        (response.with_render_mode(RenderMode::ChunkedInOrder), true)
    } else {
        (response, false)
    }
}

/// Prepare a stream rendered for a page cache, so the cache decides on the
/// final response head, managed head decisions included.
pub(super) async fn prepare_cacheable_stream(mut response: Response) -> Response {
    let extensions = response.extensions();
    if extensions.get::<CacheableStream>().is_none()
        || extensions.get::<DeferredFrameworkStream>().is_none()
    {
        return response;
    }
    let executor = response
        .extensions_mut()
        .get_mut::<CacheableStream>()
        .and_then(|stream| stream.0.take())
        .or_else(|| {
            ROUTE_CONTEXT
                .try_with(|cx| cx.borrow().local_stream_executor())
                .ok()
                .flatten()
        });
    match executor {
        Some(executor) => prepare_deferred_framework_stream(response, executor).await,
        None => response,
    }
}

/// Whether the response is a cacheable stream whose head is final.
pub(super) fn prepared_cacheable_stream(response: &Response) -> bool {
    response.extensions().get::<CacheableStream>().is_some()
        && response
            .extensions()
            .get::<DeferredFrameworkStream>()
            .is_none()
}

/// CDN caching policy for a public page.
///
/// Pass `PageCache::new()` to `Route::with_page_cache`. Add `.revalidate(...)`
/// for CDN-backed ISR. The CDN policy stores no HTML in the application process.
///
/// CDN caching requires HTML cache eligibility and correct request variation at
/// the CDN. Deployment invalidation belongs to the selected provider integration.
/// A new executable alone cannot invalidate a remote CDN cache.
///
/// A streamed `RouteResponse` on this route streams in order on requests the
/// CDN may store, so the CDN receives the complete document, without Suspense
/// fallbacks, once the stream ends. Requests the policy bypasses, such as
/// those with a `Cookie` or `Authorization` header, keep the route's mode.
#[derive(Clone)]
pub struct PageCache {
    kind: CacheKind,
}

#[derive(Clone)]
enum CacheKind {
    Cdn { ttl: u64, stale: u64 },
    Origin(PageCacheLayer),
}

impl PageCache {
    /// Declare a public page cache policy without selecting a CDN provider.
    ///
    /// No credentials, environment variables, or cache tags are required.
    /// Configure HTML caching and deployment invalidation at your CDN.
    /// The one-year TTL is a fallback, not automatic build invalidation.
    pub const fn new() -> Self {
        Self {
            kind: CacheKind::Cdn {
                ttl: 31_536_000,
                stale: 0,
            },
        }
    }

    /// Refresh on the first request after this interval, not on a timer.
    ///
    /// The CDN may serve the stale page for up to a day during regeneration.
    /// Panics if the interval is less than one second.
    pub fn revalidate(mut self, interval: Duration) -> Self {
        assert!(
            interval.as_secs() > 0,
            "page cache interval must be at least one second"
        );
        self.kind = match self.kind {
            CacheKind::Cdn { .. } => CacheKind::Cdn {
                ttl: interval.as_secs(),
                stale: 86_400,
            },
            CacheKind::Origin(cache) => CacheKind::Origin(cache.revalidate(interval)),
        };
        self
    }

    /// Override the stale-serving window after configuring revalidation.
    pub fn stale_while_revalidate(mut self, window: Duration) -> Self {
        self.kind = match self.kind {
            CacheKind::Cdn { ttl, .. } => CacheKind::Cdn {
                ttl,
                stale: window.as_secs(),
            },
            CacheKind::Origin(cache) => CacheKind::Origin(cache.stale_while_revalidate(window)),
        };
        self
    }

    pub(super) fn apply_to<S>(self, router: MethodRouter<S>) -> MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        match self.kind {
            CacheKind::Origin(cache) => router.layer(cache),
            CacheKind::Cdn { ttl, stale } => {
                let (shared, edge) = if stale == 0 {
                    (
                        format!("public, max-age=0, s-maxage={ttl}"),
                        format!("public, max-age={ttl}"),
                    )
                } else {
                    (
                        format!(
                            "public, max-age=0, s-maxage={ttl}, stale-while-revalidate={stale}"
                        ),
                        format!("public, max-age={ttl}, stale-while-revalidate={stale}"),
                    )
                };
                router.layer(CdnLayer {
                    shared_control: HeaderValue::from_str(&shared).expect("numeric cache policy"),
                    control: HeaderValue::from_str(&edge).expect("numeric cache policy"),
                })
            }
        }
    }
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new()
    }
}

// Preserve existing origin-cache callers without presenting origin storage as
// a second mode in the primary page-caching API documentation.
#[doc(hidden)]
impl From<PageCacheLayer> for PageCache {
    fn from(cache: PageCacheLayer) -> Self {
        Self {
            kind: CacheKind::Origin(cache),
        }
    }
}

#[derive(Clone)]
struct CdnLayer {
    shared_control: HeaderValue,
    control: HeaderValue,
}

impl<S> Layer<S> for CdnLayer {
    type Service = CdnService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CdnService {
            inner,
            policy: self.clone(),
        }
    }
}

#[derive(Clone)]
struct CdnService<S> {
    inner: S,
    policy: CdnLayer,
}

impl<S> Service<Request<Body>> for CdnService<S>
where
    S: Service<Request<Body>, Response = Response>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        let bypass = bypass_request(&request);
        if !bypass {
            may_store(&mut request);
        }
        let policy = self.policy.clone();
        let future = self.inner.call(request);
        Box::pin(async move {
            let response = finish_deferred_render(future.await?).await;
            let mut response = prepare_cacheable_stream(response).await;
            let cacheable = !bypass && public_html(&response);
            let headers = response.headers_mut();
            for name in ["surrogate-control", "expires", "age"] {
                headers.remove(name);
            }
            if cacheable {
                // Standard shared-cache TTL works without CDN-specific headers.
                headers.insert(header::CACHE_CONTROL, policy.shared_control);
                for name in CDN_HEADERS {
                    headers.insert(name, policy.control.clone());
                }
                headers.append(
                    header::VARY,
                    HeaderValue::from_static(
                        "Accept, Accept-Encoding, Accept-Language, X-Proa-Client",
                    ),
                );
            } else {
                headers.remove("cache-tag");
                headers.remove("vercel-cache-tag");
                let private = HeaderValue::from_static(PRIVATE_NO_STORE);
                headers.insert(header::CACHE_CONTROL, private.clone());
                for name in CDN_HEADERS {
                    headers.insert(name, private.clone());
                }
            }
            Ok(response)
        })
    }
}

const CDN_HEADERS: [&str; 4] = [
    "cdn-cache-control",
    "cloudflare-cdn-cache-control",
    "vercel-cdn-cache-control",
    "surrogate-control",
];

fn public_html(response: &Response) -> bool {
    let headers = response.headers();
    // Any other stream may carry Suspense fallbacks, or has no head here yet.
    response.status() == StatusCode::OK
        && (prepared_cacheable_stream(response)
            || response
                .body()
                .size_hint()
                .exact()
                .is_some_and(|size| size > 0))
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/html")
            })
        && !["set-cookie", "trailer", "content-range"]
            .iter()
            .any(|name| headers.contains_key(*name))
        && ![
            "cache-control",
            "cdn-cache-control",
            "cloudflare-cdn-cache-control",
            "vercel-cdn-cache-control",
            "surrogate-control",
        ]
        .iter()
        .any(|name| private_directive(headers, name))
        && !headers
            .get_all("content-security-policy")
            .iter()
            .any(|v| v.to_str().map_or(true, |v| v.contains("'nonce-")))
        && headers.get_all(header::VARY).iter().all(|v| {
            v.to_str().is_ok_and(|v| {
                v.split(',').all(|name| {
                    [
                        "accept",
                        "accept-encoding",
                        "accept-language",
                        "x-proa-client",
                    ]
                    .iter()
                    .any(|allowed| name.trim().eq_ignore_ascii_case(allowed))
                })
            })
        })
}

fn private_directive(headers: &HeaderMap, name: &str) -> bool {
    headers.get_all(name).iter().any(|v| {
        v.to_str().map_or(true, |v| {
            v.split(',').any(|directive| {
                let name = directive.split('=').next().unwrap_or("").trim();
                name.eq_ignore_ascii_case("private") || name.eq_ignore_ascii_case("no-store")
            })
        })
    })
}
