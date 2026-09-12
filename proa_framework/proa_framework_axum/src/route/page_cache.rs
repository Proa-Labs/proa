// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-scoped, bounded caching of public buffered route responses.
//!
//! This is an origin cache. Downstream caches must revalidate so deploying a
//! new executable cannot leave an old HTML response fresh at an unrelated CDN.

use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    io::{self, Read},
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body, Bytes, HttpBody},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tokio::{sync::Mutex as AsyncMutex, time::Instant};
use tower::{Layer, Service, ServiceExt};

use super::{context::ROUTE_CONTEXT, response::DeferredFrameworkStream};

type Key = [u8; 32];
const STATUS: &str = "x-proa-page-cache";
const LOCK_COUNT: usize = 64;

/// Cache selected public routes in memory, separately for each application build.
///
/// Construct once at startup and attach with [`super::Route::with_page_cache`].
/// It also supports ordinary Tower layering. Clones share bounded storage. A hit skips
/// the inner handler, loaders, and rendering; it clones reference-counted
/// response bytes rather than copying the HTML.
///
/// [`for_current_build`](Self::for_current_build) hashes the executable once
/// per process. [`new`](Self::new) accepts an explicit deployment identifier
/// when external assets or configuration also need to version the build.
/// Storage is process-local and starts empty on every restart.
///
/// Only opt in routes whose output is public. Authentication/authorization
/// middleware must run outside this layer. Cookie and Authorization requests,
/// per-request CSP nonces, errors, private/no-store responses, unknown Vary
/// fields, and streaming bodies bypass storage. Use buffered RouteResponse
/// constructors for cached pages.
///
/// Responses use `public, no-cache` downstream and a generated ETag. This
/// allows browser revalidation without rerendering and prevents an ordinary
/// CDN from retaining old HTML across deployment changes.
#[derive(Clone)]
pub struct PageCacheLayer {
    build: Key,
    cache: Arc<Cache>,
    revalidate: Option<Duration>,
    stale_window: Duration,
    timeout: Duration,
    max_entries: usize,
    max_body_bytes: usize,
    vary: Arc<Vec<HeaderName>>,
}

impl PageCacheLayer {
    /// Start a cache for a deployment identifier covering all page inputs.
    pub fn new(build_id: impl AsRef<[u8]>) -> Self {
        assert!(
            !build_id.as_ref().is_empty(),
            "page cache build ID must not be empty"
        );
        Self {
            build: Sha256::digest(build_id.as_ref()).into(),
            cache: Arc::new(Cache {
                pages: Mutex::new(HashMap::new()),
                locks: std::array::from_fn(|_| Arc::new(AsyncMutex::new(()))),
            }),
            revalidate: None,
            stale_window: Duration::from_secs(86400),
            timeout: Duration::from_secs(30),
            max_entries: 128,
            max_body_bytes: 256 * 1024,
            vary: Arc::new(vec![
                header::ACCEPT,
                header::ACCEPT_ENCODING,
                header::ACCEPT_LANGUAGE,
                HeaderName::from_static("x-proa-client"),
            ]),
        }
    }

    /// Identify the application by hashing its executable at startup.
    ///
    /// The fingerprint is computed once per process using a bounded buffer.
    /// Use `new(deployment_id)` instead if runtime configuration or external
    /// assets change independently of the executable. Construct before serving
    /// requests: reading the executable performs synchronous filesystem I/O.
    pub fn for_current_build() -> io::Result<Self> {
        static BUILD: OnceLock<Key> = OnceLock::new();
        let build = match BUILD.get() {
            Some(build) => *build,
            None => {
                let mut file = std::fs::File::open(std::env::current_exe()?)?;
                let mut hasher = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    let len = file.read(&mut buffer)?;
                    if len == 0 {
                        break;
                    }
                    hasher.update(&buffer[..len]);
                }
                *BUILD.get_or_init(|| hasher.finalize().into())
            }
        };
        Ok(Self::new(build))
    }

    /// Enable request-driven background regeneration after this interval.
    ///
    /// One request refreshes a stale entry while other requests reuse it.
    /// Failed refreshes retain the old entry. After the stale window expires,
    /// requests wait for regeneration and receive any origin error directly.
    pub fn revalidate(mut self, interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "page revalidation interval must be positive"
        );
        self.revalidate = Some(interval);
        self
    }

    /// Limit stale serving after the interval expires (default: one day).
    pub fn stale_while_revalidate(mut self, window: Duration) -> Self {
        self.stale_window = window;
        self
    }

    /// Bound each render/body collection, including background work (30s default).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        assert!(!timeout.is_zero(), "page cache timeout must be positive");
        self.timeout = timeout;
        self
    }

    /// Set entry and body-size limits (default: 128 entries of up to 256 KiB).
    ///
    /// Oversized responses bypass caching. Insertion evicts the oldest entry
    /// when full. Response headers are separately limited to 64 KiB per entry.
    pub fn with_limits(mut self, max_entries: usize, max_body_bytes: usize) -> Self {
        assert!(max_entries > 0, "page cache must allow at least one entry");
        self.max_entries = max_entries;
        self.max_body_bytes = max_body_bytes;
        self
    }

    /// Include another public representation selector in the cache key.
    ///
    /// Accept, Accept-Encoding, Accept-Language and X-Proa-Client are included
    /// by default. The URI (including query), authority, Host, forwarded host
    /// and forwarded scheme also partition entries. Responses with other Vary
    /// fields bypass storage until those fields are registered here.
    pub fn vary_on(mut self, header: HeaderName) -> Self {
        if !self.vary.contains(&header) {
            Arc::make_mut(&mut self.vary).push(header);
        }
        self
    }

    fn key(&self, request: &Request<Body>) -> Key {
        let mut hash = Sha256::new();
        hash.update(self.build);
        hash.update(
            self.revalidate
                .map(|v| v.as_nanos())
                .unwrap_or(u128::MAX)
                .to_le_bytes(),
        );
        hash.update(self.stale_window.as_nanos().to_le_bytes());
        hash_field(
            &mut hash,
            request.uri().scheme_str().unwrap_or("").as_bytes(),
        );
        hash_field(
            &mut hash,
            request
                .uri()
                .authority()
                .map(|a| a.as_str())
                .unwrap_or("")
                .as_bytes(),
        );
        hash_field(
            &mut hash,
            request
                .uri()
                .path_and_query()
                .map(|p| p.as_str())
                .unwrap_or("/")
                .as_bytes(),
        );
        for name in [
            header::HOST,
            HeaderName::from_static("x-forwarded-host"),
            HeaderName::from_static("x-forwarded-proto"),
        ]
        .iter()
        .chain(self.vary.iter())
        {
            hash_field(&mut hash, name.as_str().as_bytes());
            for value in request.headers().get_all(name) {
                hash_field(&mut hash, value.as_bytes());
            }
            // Separate an absent header from a present empty value.
            hash.update(u64::MAX.to_le_bytes());
        }
        hash.finalize().into()
    }

    fn get(&self, key: &Key) -> Option<Arc<CachedPage>> {
        self.cache.pages.lock().get(key).cloned()
    }

    fn fresh(&self, page: &CachedPage) -> bool {
        self.revalidate
            .is_none_or(|interval| page.created.elapsed() < interval)
    }

    fn may_serve_stale(&self, page: &CachedPage) -> bool {
        self.revalidate.is_some_and(|interval| {
            page.created.elapsed() < interval.saturating_add(self.stale_window)
        })
    }

    fn insert(&self, key: Key, page: Arc<CachedPage>) {
        let mut pages = self.cache.pages.lock();
        if !pages.contains_key(&key) && pages.len() >= self.max_entries {
            if let Some(oldest) = pages
                .iter()
                .min_by_key(|(_, page)| page.created)
                .map(|(key, _)| *key)
            {
                pages.remove(&oldest);
            }
        }
        pages.insert(key, page);
    }
}

impl<S> Layer<S> for PageCacheLayer {
    type Service = PageCacheService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PageCacheService {
            inner,
            layer: self.clone(),
        }
    }
}

/// Tower service produced by [`PageCacheLayer`].
#[derive(Clone)]
pub struct PageCacheService<S> {
    inner: S,
    layer: PageCacheLayer,
}

impl<S> Service<Request<Body>> for PageCacheService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Hits need no inner capacity. On misses, oneshot polls the cloned
        // inner service for readiness before calling it.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let layer = self.layer.clone();
        let inner = self.inner.clone();
        Box::pin(async move {
            if bypass_request(&request) {
                return inner.oneshot(request).await.map(bypass);
            }
            let key = layer.key(&request);
            let headers = request.headers().clone();
            let head = request.method() == Method::HEAD;
            if let Some(page) = layer.get(&key) {
                if layer.fresh(&page) {
                    return Ok(page.response(&headers, head, "HIT"));
                }
                if layer.may_serve_stale(&page)
                    && !has_directive(&headers, header::CACHE_CONTROL, "no-cache")
                    && !has_directive(&headers, header::CACHE_CONTROL, "max-age")
                    && !head
                {
                    let response = page.response(&headers, false, "STALE");
                    if let Ok(guard) = layer.cache.locks[key[0] as usize % LOCK_COUNT]
                        .clone()
                        .try_lock_owned()
                    {
                        // Tokio task locals do not propagate through spawn.
                        // Retain the manifest, route params and metadata for
                        // RouteResponse finalization in the refresh task.
                        let context = ROUTE_CONTEXT.try_with(|cx| cx.borrow().clone()).ok();
                        tokio::spawn(async move {
                            let _guard = guard;
                            let refresh = async {
                                if layer
                                    .get(&key)
                                    .is_some_and(|current| !Arc::ptr_eq(&current, &page))
                                {
                                    return;
                                }
                                if fetch(&layer, key, inner, request).await.is_err() {
                                    tracing::warn!("page cache background regeneration failed");
                                }
                            };
                            if let Some(context) = context {
                                ROUTE_CONTEXT.scope(RefCell::new(context), refresh).await;
                            } else {
                                refresh.await;
                            }
                        });
                    }
                    return Ok(response);
                }
            }
            // Never seed a GET entry from a HEAD response with an empty body.
            if head {
                return inner.oneshot(request).await.map(bypass);
            }
            // Fixed lock stripes bound coordination memory as well as stored
            // pages. Cancellation/panics release the guard automatically.
            let _guard = layer.cache.locks[key[0] as usize % LOCK_COUNT].lock().await;
            if let Some(page) = layer.get(&key).filter(|page| layer.fresh(page)) {
                return Ok(page.response(&headers, false, "HIT"));
            }
            let (response, page) = fetch(&layer, key, inner, request).await?;
            Ok(match page {
                Some(page) if matches_etag(&headers, &page.etag) => {
                    page.response(&headers, false, "MISS")
                }
                _ => response,
            })
        })
    }
}

struct Cache {
    pages: Mutex<HashMap<Key, Arc<CachedPage>>>,
    locks: [Arc<AsyncMutex<()>>; LOCK_COUNT],
}

struct CachedPage {
    headers: HeaderMap,
    body: Bytes,
    etag: HeaderValue,
    created: Instant,
}

impl CachedPage {
    fn response(&self, request_headers: &HeaderMap, head: bool, status: &'static str) -> Response {
        let not_modified = matches_etag(request_headers, &self.etag);
        let mut response = Response::new(if head || not_modified {
            Body::empty()
        } else {
            Body::from(self.body.clone())
        });
        *response.headers_mut() = self.headers.clone();
        response
            .headers_mut()
            .insert(STATUS, HeaderValue::from_static(status));
        if not_modified {
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            response.headers_mut().remove(header::CONTENT_LENGTH);
        }
        response
    }
}

async fn fetch<S>(
    layer: &PageCacheLayer,
    key: Key,
    inner: S,
    mut request: Request<Body>,
) -> Result<(Response, Option<Arc<CachedPage>>), S::Error>
where
    S: Service<Request<Body>, Response = Response> + Send,
    S::Future: Send,
{
    // The cache owns validators. An origin 304 cannot populate an empty entry.
    request.headers_mut().remove(header::IF_NONE_MATCH);
    request.headers_mut().remove(header::IF_MODIFIED_SINCE);
    let future = async {
        let response = inner.oneshot(request).await?;
        if !cacheable_response(layer, &response) {
            // Keep stale public content on transient server failures only.
            if !response.status().is_server_error() {
                layer.cache.pages.lock().remove(&key);
            }
            return Ok((bypass(response), None));
        }
        let (mut parts, body) = response.into_parts();
        let bytes = match to_bytes(body, layer.max_body_bytes).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, "page cache could not collect response body");
                return Ok((
                    bypass((StatusCode::BAD_GATEWAY, "page response failed").into_response()),
                    None,
                ));
            }
        };
        let mut hash = Sha256::new();
        hash.update(key);
        hash.update(&bytes);
        let etag = HeaderValue::from_str(&format!("\"{:x}\"", hash.finalize()))
            .expect("hex digest is a valid ETag");
        parts.headers.insert(header::ETAG, etag.clone());
        parts
            .headers
            .insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
        downstream_policy(&mut parts.headers, "public, no-cache");
        for name in layer.vary.iter() {
            if !parts.headers.get_all(header::VARY).iter().any(|value| {
                value.to_str().is_ok_and(|value| {
                    value
                        .split(',')
                        .any(|field| field.trim().eq_ignore_ascii_case(name.as_str()))
                })
            }) {
                parts.headers.append(
                    header::VARY,
                    HeaderValue::from_str(name.as_str()).expect("header name is a valid value"),
                );
            }
        }
        let page = Arc::new(CachedPage {
            headers: parts.headers.clone(),
            body: bytes.clone(),
            etag,
            created: Instant::now(),
        });
        layer.insert(key, page.clone());
        parts
            .headers
            .insert(STATUS, HeaderValue::from_static("MISS"));
        Ok((Response::from_parts(parts, Body::from(bytes)), Some(page)))
    };
    match tokio::time::timeout(layer.timeout, future).await {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!("page cache regeneration timed out");
            Ok((
                bypass(
                    (StatusCode::GATEWAY_TIMEOUT, "page regeneration timed out").into_response(),
                ),
                None,
            ))
        }
    }
}

pub(super) fn bypass_request(request: &Request<Body>) -> bool {
    !matches!(*request.method(), Method::GET | Method::HEAD)
        || !request.body().is_end_stream()
        || request.extensions().get::<proa_core::CspNonce>().is_some()
        || [
            header::AUTHORIZATION,
            header::COOKIE,
            header::RANGE,
            header::IF_MATCH,
            header::IF_UNMODIFIED_SINCE,
        ]
        .iter()
        .any(|name| request.headers().contains_key(name))
        || has_directive(request.headers(), header::CACHE_CONTROL, "no-store")
}

fn cacheable_response(layer: &PageCacheLayer, response: &Response) -> bool {
    let headers = response.headers();
    response.status() == StatusCode::OK
        && response
            .extensions()
            .get::<DeferredFrameworkStream>()
            .is_none()
        && response
            .body()
            .size_hint()
            .exact()
            .is_some_and(|len| len <= layer.max_body_bytes as u64)
        && !headers.contains_key(header::SET_COOKIE)
        && !headers.contains_key(header::TRAILER)
        && !headers.contains_key(header::CONTENT_RANGE)
        && headers
            .iter()
            .map(|(name, value)| name.as_str().len().saturating_add(value.len()))
            .sum::<usize>()
            <= 64 * 1024
        && [
            header::CACHE_CONTROL,
            HeaderName::from_static("cdn-cache-control"),
            HeaderName::from_static("cloudflare-cdn-cache-control"),
            HeaderName::from_static("vercel-cdn-cache-control"),
        ]
        .iter()
        .all(|name| {
            !has_directive(headers, name.clone(), "private")
                && !has_directive(headers, name.clone(), "no-store")
        })
        && [
            header::CONTENT_SECURITY_POLICY,
            header::CONTENT_SECURITY_POLICY_REPORT_ONLY,
        ]
        .iter()
        .all(|name| {
            headers
                .get_all(name)
                .iter()
                .all(|value| value.to_str().is_ok_and(|value| !value.contains("'nonce-")))
        })
        && headers.get_all(header::VARY).iter().all(|value| {
            value.to_str().is_ok_and(|value| {
                value.split(',').all(|field| {
                    let field = field.trim();
                    field.is_empty()
                        || layer
                            .vary
                            .iter()
                            .any(|name| field.eq_ignore_ascii_case(name.as_str()))
                })
            })
        })
}

fn downstream_policy(headers: &mut HeaderMap, policy: &'static str) {
    for name in [
        "cache-control",
        "cdn-cache-control",
        "cloudflare-cdn-cache-control",
        "vercel-cdn-cache-control",
    ] {
        headers.insert(name, HeaderValue::from_static(policy));
    }
    headers.remove("surrogate-control");
    headers.remove(header::EXPIRES);
    headers.remove(header::AGE);
}

fn bypass(mut response: Response) -> Response {
    downstream_policy(response.headers_mut(), "private, no-store");
    response
        .headers_mut()
        .insert(STATUS, HeaderValue::from_static("BYPASS"));
    response
}

fn has_directive(headers: &HeaderMap, name: HeaderName, directive: &str) -> bool {
    headers.get_all(name).iter().any(|value| {
        value.to_str().map_or(true, |value| {
            value.split(',').any(|part| {
                part.split('=')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case(directive)
            })
        })
    })
}

fn matches_etag(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    headers.get_all(header::IF_NONE_MATCH).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value.split(',').any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*"
                    || candidate.strip_prefix("W/").unwrap_or(candidate).as_bytes()
                        == etag.as_bytes()
            })
        })
    })
}

fn hash_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
