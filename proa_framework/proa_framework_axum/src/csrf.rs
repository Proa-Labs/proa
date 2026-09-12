// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cross-origin request protection (CSRF defense).
//!
//! Implements the pattern Go 1.25 ships as `net/http`'s
//! `CrossOriginProtection`: instead of synchronizer tokens, rely on the
//! `Sec-Fetch-Site` metadata header (sent by every evergreen browser
//! by modern browsers) with an `Origin`-vs-`Host` fallback for older browsers.
//!
//! Decision procedure for each request:
//!
//! 1. Safe methods (GET, HEAD, OPTIONS) always pass.
//! 2. If `Sec-Fetch-Site` is present: allow `same-origin` and `none`,
//!    reject anything else (unless the `Origin` is explicitly trusted).
//! 3. Else, if `Origin` is present: allow when its host (and port) match
//!    the request `Host` header or the origin is explicitly trusted,
//!    otherwise reject.
//! 4. Else (neither header): allow. A request without both headers does
//!    not come from a browser, so CSRF does not apply.
//!
//! Rejections are `403 Forbidden` with a short generic body; the
//! offending origin is logged via `tracing::warn!`.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::http::{header, HeaderMap, HeaderName, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use pin_project::pin_project;
use tower::{Layer, Service};

const SEC_FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

/// Tower layer that applies [`CrossOriginProtection`] to a service.
///
/// Enabled with an empty trusted-origin set by default. The framework
/// builder installs this layer on every built router; use
/// [`FrameworkBuilder::with_cross_origin_protection`](crate::FrameworkBuilder::with_cross_origin_protection)
/// to turn it off and
/// [`FrameworkBuilder::with_trusted_origin`](crate::FrameworkBuilder::with_trusted_origin)
/// to allow specific cross-origin callers.
#[derive(Debug, Clone)]
pub struct CrossOriginProtectionLayer {
    config: Arc<CrossOriginConfig>,
}

#[derive(Debug)]
struct CrossOriginConfig {
    enabled: bool,
    /// Exact serialized origins (e.g. `https://app.example.com`) allowed
    /// to make cross-origin unsafe requests.
    trusted_origins: HashSet<String>,
}

impl CrossOriginProtectionLayer {
    /// Protection enabled, no extra trusted origins.
    pub fn new() -> Self {
        Self {
            config: Arc::new(CrossOriginConfig {
                enabled: true,
                trusted_origins: HashSet::new(),
            }),
        }
    }

    /// A layer that passes every request through unchanged.
    pub fn disabled() -> Self {
        Self::new().with_enabled(false)
    }

    /// Enable or disable the protection.
    pub fn with_enabled(self, enabled: bool) -> Self {
        let mut config = self.into_config();
        config.enabled = enabled;
        Self {
            config: Arc::new(config),
        }
    }

    /// Trust an additional origin for cross-origin unsafe requests.
    ///
    /// The value is matched exactly (byte for byte) against the `Origin`
    /// request header, so it must be a serialized origin such as
    /// `https://app.example.com` (scheme, host, optional port, no
    /// trailing slash, no path).
    pub fn with_trusted_origin(self, origin: impl Into<String>) -> Self {
        let mut config = self.into_config();
        config.trusted_origins.insert(origin.into());
        Self {
            config: Arc::new(config),
        }
    }

    fn into_config(self) -> CrossOriginConfig {
        match Arc::try_unwrap(self.config) {
            Ok(config) => config,
            Err(shared) => CrossOriginConfig {
                enabled: shared.enabled,
                trusted_origins: shared.trusted_origins.clone(),
            },
        }
    }
}

impl Default for CrossOriginProtectionLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for CrossOriginProtectionLayer {
    type Service = CrossOriginProtection<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CrossOriginProtection {
            inner,
            config: Arc::clone(&self.config),
        }
    }
}

/// Tower service produced by [`CrossOriginProtectionLayer`].
#[derive(Debug, Clone)]
pub struct CrossOriginProtection<S> {
    inner: S,
    config: Arc<CrossOriginConfig>,
}

impl<S, ReqBody> Service<Request<ReqBody>> for CrossOriginProtection<S>
where
    S: Service<Request<ReqBody>, Response = Response>,
{
    type Response = Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if !self.config.enabled {
            return ResponseFuture::Inner(self.inner.call(req));
        }

        match evaluate_cross_origin(req.method(), req.headers(), &self.config.trusted_origins) {
            CrossOriginVerdict::Allow => ResponseFuture::Inner(self.inner.call(req)),
            verdict @ (CrossOriginVerdict::RejectSecFetchSite
            | CrossOriginVerdict::RejectOriginMismatch) => {
                tracing::warn!(
                    method = %req.method(),
                    path = %req.uri().path(),
                    origin = header_str(req.headers(), &header::ORIGIN).unwrap_or("<none>"),
                    sec_fetch_site =
                        header_str(req.headers(), &SEC_FETCH_SITE).unwrap_or("<none>"),
                    ?verdict,
                    "rejected cross-origin request"
                );
                ResponseFuture::Rejected(Some(
                    (StatusCode::FORBIDDEN, "cross-origin request rejected").into_response(),
                ))
            }
        }
    }
}

/// Future returned by [`CrossOriginProtection`]. Avoids a per-request
/// `Box` by carrying either the inner service future or the prebuilt
/// rejection response.
#[pin_project(project = ResponseFutureProj)]
pub enum ResponseFuture<F> {
    Inner(#[pin] F),
    Rejected(Option<Response>),
}

impl<F, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = Result<Response, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ResponseFutureProj::Inner(future) => future.poll(cx),
            ResponseFutureProj::Rejected(response) => Poll::Ready(Ok(response
                .take()
                .expect("ResponseFuture polled after completion"))),
        }
    }
}

fn header_str<'h>(headers: &'h HeaderMap, name: &HeaderName) -> Option<&'h str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrossOriginVerdict {
    Allow,
    RejectSecFetchSite,
    RejectOriginMismatch,
}

/// Pure decision function (see the module docs for the procedure).
fn evaluate_cross_origin(
    method: &Method,
    headers: &HeaderMap,
    trusted_origins: &HashSet<String>,
) -> CrossOriginVerdict {
    // Safe methods are always allowed.
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return CrossOriginVerdict::Allow;
    }

    let origin = header_str(headers, &header::ORIGIN);
    let origin_is_trusted = origin.is_some_and(|origin| trusted_origins.contains(origin));

    match header_str(headers, &SEC_FETCH_SITE) {
        Some("same-origin") | Some("none") => return CrossOriginVerdict::Allow,
        Some(_) => {
            // `cross-site` or `same-site` (a sibling subdomain is still a
            // CSRF vector). Only an explicitly trusted Origin may pass.
            return if origin_is_trusted {
                CrossOriginVerdict::Allow
            } else {
                CrossOriginVerdict::RejectSecFetchSite
            };
        }
        None => {}
    }

    let Some(origin) = origin else {
        // Neither Sec-Fetch-Site nor Origin: not a browser request, so
        // cross-origin request forgery does not apply.
        return CrossOriginVerdict::Allow;
    };

    if origin_is_trusted {
        return CrossOriginVerdict::Allow;
    }

    if let Some(host) = header_str(headers, &header::HOST) {
        if origin_host_matches(origin, host) {
            // The Origin host matches the Host header. The Host header
            // carries no scheme, so an HTTP page posting to its HTTPS
            // twin passes here; modern browsers send Sec-Fetch-Site and
            // never reach this fallback, and HSTS closes the rest.
            return CrossOriginVerdict::Allow;
        }
    }

    CrossOriginVerdict::RejectOriginMismatch
}

/// Compare the `host[:port]` part of a serialized `Origin` header value
/// against the request `Host` header value. Returns false for opaque
/// (`null`) or malformed origins.
fn origin_host_matches(origin: &str, request_host: &str) -> bool {
    let Some((_scheme, host_port)) = origin.split_once("://") else {
        return false;
    };
    !host_port.is_empty()
        && !request_host.is_empty()
        && host_port.eq_ignore_ascii_case(request_host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn app(layer: CrossOriginProtectionLayer) -> Router {
        Router::new()
            .route("/", get(|| async { "ok" }).post(|| async { "ok" }))
            .layer(layer)
    }

    fn request(method: &str, headers: &[(&'static str, &'static str)]) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri("/");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn status_for(
        layer: CrossOriginProtectionLayer,
        method: &str,
        headers: &[(&'static str, &'static str)],
    ) -> StatusCode {
        app(layer)
            .oneshot(request(method, headers))
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn get_always_passes() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "GET",
            &[("sec-fetch-site", "cross-site")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn post_with_cross_site_fetch_site_is_rejected() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "POST",
            &[("sec-fetch-site", "cross-site")],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_with_same_origin_fetch_site_passes() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "POST",
            &[("sec-fetch-site", "same-origin")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn post_with_matching_origin_passes() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "POST",
            &[("origin", "http://example.com"), ("host", "example.com")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn post_with_mismatched_origin_is_rejected() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "POST",
            &[("origin", "https://evil.example"), ("host", "example.com")],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_without_origin_or_fetch_site_passes() {
        let status = status_for(
            CrossOriginProtectionLayer::new(),
            "POST",
            &[("host", "example.com")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn trusted_origin_passes() {
        let layer =
            CrossOriginProtectionLayer::new().with_trusted_origin("https://app.example.com");
        let status = status_for(
            layer.clone(),
            "POST",
            &[
                ("origin", "https://app.example.com"),
                ("host", "api.example.com"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // A trusted origin also overrides a cross-site Sec-Fetch-Site.
        let status = status_for(
            layer,
            "POST",
            &[
                ("origin", "https://app.example.com"),
                ("sec-fetch-site", "cross-site"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn disabled_layer_passes_everything() {
        let layer = CrossOriginProtectionLayer::disabled();
        let status = status_for(layer.clone(), "POST", &[("sec-fetch-site", "cross-site")]).await;
        assert_eq!(status, StatusCode::OK);

        let status = status_for(
            layer,
            "POST",
            &[("origin", "https://evil.example"), ("host", "example.com")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[test]
    fn same_site_fetch_site_is_rejected() {
        // Subdomains are a CSRF vector; same-site is not same-origin.
        let mut headers = HeaderMap::new();
        headers.insert(SEC_FETCH_SITE, "same-site".parse().unwrap());
        assert_eq!(
            evaluate_cross_origin(&Method::POST, &headers, &HashSet::new()),
            CrossOriginVerdict::RejectSecFetchSite
        );
    }

    #[test]
    fn null_origin_is_rejected() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "null".parse().unwrap());
        headers.insert(header::HOST, "example.com".parse().unwrap());
        assert_eq!(
            evaluate_cross_origin(&Method::POST, &headers, &HashSet::new()),
            CrossOriginVerdict::RejectOriginMismatch
        );
    }

    #[test]
    fn origin_host_comparison_includes_the_port() {
        assert!(origin_host_matches(
            "http://example.com:3000",
            "example.com:3000"
        ));
        assert!(!origin_host_matches(
            "http://example.com:3000",
            "example.com"
        ));
        assert!(!origin_host_matches(
            "http://example.com",
            "example.com:3000"
        ));
        assert!(origin_host_matches("http://EXAMPLE.com", "example.com"));
        assert!(!origin_host_matches("null", "example.com"));
    }
}
