// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Default security response headers applied by the framework builder.
//!
//! `proa_framework_core` already stamps `X-Content-Type-Options`,
//! `X-Frame-Options`, and `Referrer-Policy` onto SSR responses at
//! construction time. This layer is the HTTP-level counterpart: it covers
//! every response that leaves the built router (static files, proxied
//! apps, plain axum handlers, error responses from other middleware) and
//! carries the opt-in headers that need deployment knowledge
//! (`Strict-Transport-Security`, `Content-Security-Policy`) plus the
//! ability to relax or remove `X-Frame-Options` for embeddable apps.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use axum::http::{header, HeaderValue, Request, Response};
use pin_project::pin_project;
use tower::{Layer, Service};

/// `X-Frame-Options` policy applied by [`SecurityHeadersConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameOptions {
    /// `X-Frame-Options: DENY` (the default): pages may never be framed.
    #[default]
    Deny,
    /// `X-Frame-Options: SAMEORIGIN`: same-origin pages may frame the app.
    SameOrigin,
    /// Remove the header entirely, for apps that are embedded by other
    /// origins. Prefer pairing this with a `Content-Security-Policy`
    /// `frame-ancestors` directive that names the allowed embedders.
    Disabled,
}

/// Security header policy for the built router.
///
/// Defaults: `X-Frame-Options: DENY`, `Referrer-Policy:
/// strict-origin-when-cross-origin`, and `X-Content-Type-Options:
/// nosniff` on every response that does not already carry them. HSTS and
/// CSP are off by default: HSTS is only correct when the deployment
/// terminates TLS, and a CSP must be authored per app.
#[derive(Debug, Clone, Default)]
pub struct SecurityHeadersConfig {
    /// Clickjacking defense. See [`FrameOptions`].
    pub frame_options: FrameOptions,
    /// When set, emit `Strict-Transport-Security: max-age=<secs>`.
    /// Off by default because it requires TLS awareness.
    pub hsts_max_age_secs: Option<u64>,
    /// When set, emit this exact string as the `Content-Security-Policy`
    /// value. Off by default.
    pub content_security_policy: Option<String>,
}

impl SecurityHeadersConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `X-Frame-Options` policy.
    pub fn with_frame_options(mut self, frame_options: FrameOptions) -> Self {
        self.frame_options = frame_options;
        self
    }

    /// Enable `Strict-Transport-Security` with the given `max-age` in
    /// seconds. Only do this when the deployment serves over TLS.
    pub fn with_hsts_max_age_secs(mut self, max_age_secs: u64) -> Self {
        self.hsts_max_age_secs = Some(max_age_secs);
        self
    }

    /// Enable `Content-Security-Policy` with the given full policy
    /// string, e.g. `"default-src 'self'"`.
    pub fn with_content_security_policy(mut self, policy: impl Into<String>) -> Self {
        self.content_security_policy = Some(policy.into());
        self
    }
}

/// Header values resolved once at layer construction so the per-response
/// work is a handful of header-map probes with zero allocation.
#[derive(Debug)]
struct ResolvedSecurityHeaders {
    frame_options: FrameOptions,
    hsts: Option<HeaderValue>,
    csp: Option<HeaderValue>,
}

impl ResolvedSecurityHeaders {
    fn from_config(config: SecurityHeadersConfig) -> Self {
        let hsts = config.hsts_max_age_secs.and_then(|secs| {
            // proa-perf-ok: one-time layer construction, not a render path.
            HeaderValue::from_str(&format!("max-age={secs}")).ok()
        });
        let csp = config.content_security_policy.and_then(|policy| {
            match HeaderValue::from_str(&policy) {
                Ok(value) => Some(value),
                Err(_) => {
                    tracing::warn!(
                        "configured Content-Security-Policy contains invalid header \
                         characters; the header will not be sent"
                    );
                    None
                }
            }
        });
        Self {
            frame_options: config.frame_options,
            hsts,
            csp,
        }
    }

    fn apply<B>(&self, response: &mut Response<B>) {
        let headers = response.headers_mut();
        if !headers.contains_key(header::X_CONTENT_TYPE_OPTIONS) {
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
        }
        match self.frame_options {
            // The SSR response constructors already default to DENY; only
            // fill the gap for responses that did not pass through them.
            FrameOptions::Deny => {
                if !headers.contains_key(header::X_FRAME_OPTIONS) {
                    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
                }
            }
            // Overrides the core default of DENY, so insert unconditionally.
            FrameOptions::SameOrigin => {
                headers.insert(
                    header::X_FRAME_OPTIONS,
                    HeaderValue::from_static("SAMEORIGIN"),
                );
            }
            FrameOptions::Disabled => {
                headers.remove(header::X_FRAME_OPTIONS);
            }
        }
        if !headers.contains_key(header::REFERRER_POLICY) {
            headers.insert(
                header::REFERRER_POLICY,
                HeaderValue::from_static("strict-origin-when-cross-origin"),
            );
        }
        if let Some(hsts) = &self.hsts {
            if !headers.contains_key(header::STRICT_TRANSPORT_SECURITY) {
                headers.insert(header::STRICT_TRANSPORT_SECURITY, hsts.clone());
            }
        }
        if let Some(csp) = &self.csp {
            if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
                headers.insert(header::CONTENT_SECURITY_POLICY, csp.clone());
            }
        }
    }
}

/// Tower layer that applies a [`SecurityHeadersConfig`] to every response.
#[derive(Debug, Clone)]
pub struct SecurityHeadersLayer {
    resolved: Arc<ResolvedSecurityHeaders>,
}

impl SecurityHeadersLayer {
    pub fn new(config: SecurityHeadersConfig) -> Self {
        Self {
            resolved: Arc::new(ResolvedSecurityHeaders::from_config(config)),
        }
    }
}

impl Default for SecurityHeadersLayer {
    fn default() -> Self {
        Self::new(SecurityHeadersConfig::default())
    }
}

impl<S> Layer<S> for SecurityHeadersLayer {
    type Service = SecurityHeaders<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SecurityHeaders {
            inner,
            resolved: Arc::clone(&self.resolved),
        }
    }
}

/// Tower service produced by [`SecurityHeadersLayer`].
#[derive(Debug, Clone)]
pub struct SecurityHeaders<S> {
    inner: S,
    resolved: Arc<ResolvedSecurityHeaders>,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for SecurityHeaders<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = SecurityHeadersFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        SecurityHeadersFuture {
            inner: self.inner.call(req),
            resolved: Arc::clone(&self.resolved),
        }
    }
}

/// Future returned by [`SecurityHeaders`].
#[pin_project]
pub struct SecurityHeadersFuture<F> {
    #[pin]
    inner: F,
    resolved: Arc<ResolvedSecurityHeaders>,
}

impl<F, ResBody, E> Future for SecurityHeadersFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut result = ready!(this.inner.poll(cx));
        if let Ok(response) = &mut result {
            this.resolved.apply(response);
        }
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn apply(config: SecurityHeadersConfig, response: &mut Response<Body>) {
        ResolvedSecurityHeaders::from_config(config).apply(response);
    }

    #[test]
    fn defaults_add_frame_options_referrer_policy_and_nosniff() {
        let mut response = Response::new(Body::empty());
        apply(SecurityHeadersConfig::default(), &mut response);

        let headers = response.headers();
        assert_eq!(headers.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
        assert_eq!(
            headers.get(header::REFERRER_POLICY).unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );
        assert!(headers.get(header::STRICT_TRANSPORT_SECURITY).is_none());
        assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_none());
    }

    #[test]
    fn existing_headers_are_not_clobbered() {
        let mut response = Response::new(Body::empty());
        response.headers_mut().insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
        apply(SecurityHeadersConfig::default(), &mut response);

        assert_eq!(
            response.headers().get(header::REFERRER_POLICY).unwrap(),
            "no-referrer"
        );
    }

    #[test]
    fn same_origin_overrides_an_existing_deny() {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        apply(
            SecurityHeadersConfig::default().with_frame_options(FrameOptions::SameOrigin),
            &mut response,
        );

        assert_eq!(
            response.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "SAMEORIGIN"
        );
    }

    #[test]
    fn disabled_frame_options_removes_the_header() {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        apply(
            SecurityHeadersConfig::default().with_frame_options(FrameOptions::Disabled),
            &mut response,
        );

        assert!(response.headers().get(header::X_FRAME_OPTIONS).is_none());
    }

    #[test]
    fn hsts_and_csp_are_emitted_when_configured() {
        let mut response = Response::new(Body::empty());
        apply(
            SecurityHeadersConfig::default()
                .with_hsts_max_age_secs(63072000)
                .with_content_security_policy("default-src 'self'"),
            &mut response,
        );

        let headers = response.headers();
        assert_eq!(
            headers.get(header::STRICT_TRANSPORT_SECURITY).unwrap(),
            "max-age=63072000"
        );
        assert_eq!(
            headers.get(header::CONTENT_SECURITY_POLICY).unwrap(),
            "default-src 'self'"
        );
    }

    #[test]
    fn invalid_csp_is_dropped_instead_of_panicking() {
        let mut response = Response::new(Body::empty());
        apply(
            SecurityHeadersConfig::default().with_content_security_policy("bad\npolicy"),
            &mut response,
        );

        assert!(response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_none());
    }
}
