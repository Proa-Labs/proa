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
use tower::{Layer, Service};

use super::page_cache::{bypass_request, PageCacheLayer};

/// CDN caching policy for a public page.
///
/// Pass `PageCache::new()` to `Route::with_page_cache`. Add `.revalidate(...)`
/// for CDN-backed ISR. The CDN policy stores no HTML in the application process.
///
/// CDN caching requires HTML cache eligibility and correct request variation at
/// the CDN. Deployment invalidation belongs to the selected provider integration.
/// A new executable alone cannot invalidate a remote CDN cache.
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

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let bypass = bypass_request(&request);
        let policy = self.policy.clone();
        let future = self.inner.call(request);
        Box::pin(async move {
            let mut response = future.await?;
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
                let private = HeaderValue::from_static("private, no-store");
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
    // Deferred framework streams have no buffered HTML here. Do not cache them.
    response.status() == StatusCode::OK
        && response
            .body()
            .size_hint()
            .exact()
            .is_some_and(|size| size > 0)
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
