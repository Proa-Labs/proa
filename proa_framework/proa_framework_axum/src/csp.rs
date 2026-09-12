// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Per-request CSP nonce generation and a strict `Content-Security-Policy`.
//!
//! Proa can already stamp a validated [`CspNonce`] onto every script tag it
//! emits (rsjs seed state, React island / import-map / bootstrap tags, the
//! streaming bootstrap, and every out-of-order boundary swap). What it cannot
//! do on its own is mint the per-response
//! random value and write the matching header, because that is HTTP-level
//! work outside the render path.
//!
//! [`CspNonceLayer`] closes that gap: it generates a 128-bit nonce per
//! request, inserts it into the request extensions where
//! [`RouteContextLayer`](crate::route::RouteContextLayer) picks it up, and
//! writes the assembled policy onto the response.
//!
//! ```rust,ignore
//! FrameworkBuilder::new(manifest)
//!     .with_strict_csp()
//!     .build();
//! ```
//!
//! That yields
//! `default-src 'self'; script-src 'nonce-…' 'strict-dynamic'; …` with no
//! script `'unsafe-inline'`. Raw `<script>` tags written by hand still need
//! their own `nonce` attribute. Components can read the request nonce with
//! [`WebContext::csp_nonce`](proa_core::WebContext::csp_nonce).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use axum::http::{header, HeaderName, HeaderValue, Request, Response};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine as _;
use bytes::Bytes;
use pin_project::pin_project;
use rand::RngCore as _;
use tower::{Layer, Service};

use proa_core::CspNonce;

/// Nonce entropy in bytes. CSP recommends at least 128 bits per response.
const NONCE_BYTES: usize = 16;

/// The `script-src` sources Proa always emits ahead of any application
/// additions: the request nonce, then `'strict-dynamic'` so scripts loaded
/// by an already-trusted script inherit trust instead of needing a
/// host allowlist.
const SCRIPT_SRC_PREFIX: &str = "'nonce-";
const SCRIPT_SRC_SUFFIX: &str = "' 'strict-dynamic'";

/// Non-script directives applied unless overridden with
/// [`CspPolicy::with_directive`].
///
/// `style-src` keeps `'unsafe-inline'` because inline `style` attributes are
/// common in real templates and, unlike inline script, cannot execute code.
/// Tighten it once your app has moved those styles into classes.
const DEFAULT_DIRECTIVES: &[(&str, &str)] = &[
    ("default-src", "'self'"),
    ("base-uri", "'self'"),
    ("object-src", "'none'"),
    ("frame-ancestors", "'none'"),
    ("style-src", "'self' 'unsafe-inline'"),
];

/// A strict, nonce-based Content Security Policy.
///
/// The `script-src` directive is owned by this type: it always begins with
/// the request nonce and `'strict-dynamic'`, and extra sources go through
/// [`allow_script_source`](Self::allow_script_source). Every other directive
/// can be replaced with [`with_directive`](Self::with_directive).
#[derive(Debug, Clone)]
pub struct CspPolicy {
    directives: BTreeMap<Cow<'static, str>, String>,
    /// Directives the application set explicitly, used to keep builder-level
    /// consistency warnings from firing on Proa's own defaults.
    explicit: BTreeSet<Cow<'static, str>>,
    script_src_extra: Vec<String>,
    report_only: bool,
}

impl Default for CspPolicy {
    fn default() -> Self {
        Self::strict()
    }
}

impl CspPolicy {
    /// The recommended strict policy: same-origin default, nonce plus
    /// `'strict-dynamic'` for scripts, no plugins, no framing, and a locked
    /// `base-uri`.
    pub fn strict() -> Self {
        Self {
            directives: DEFAULT_DIRECTIVES
                .iter()
                .map(|(name, value)| (Cow::Borrowed(*name), (*value).to_owned()))
                .collect(),
            explicit: BTreeSet::new(),
            script_src_extra: Vec::new(),
            report_only: false,
        }
    }

    /// Set or replace a directive, e.g.
    /// `with_directive("img-src", "'self' data: https://cdn.example")`.
    ///
    /// `script-src` cannot be set this way: replacing it would drop the
    /// nonce and silently break hydration. Use
    /// [`allow_script_source`](Self::allow_script_source) instead. Passing
    /// `"script-src"` logs a warning and leaves the policy unchanged.
    pub fn with_directive(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<String>,
    ) -> Self {
        let name = name.into();
        if name.eq_ignore_ascii_case("script-src") {
            tracing::warn!(
                "CspPolicy::with_directive cannot set script-src; the nonce and \
                 'strict-dynamic' are managed by the layer. Use \
                 CspPolicy::allow_script_source to add sources."
            );
            return self;
        }
        self.explicit.insert(name.clone());
        self.directives.insert(name, value.into());
        self
    }

    /// Remove a directive from the policy entirely.
    pub fn without_directive(mut self, name: &str) -> Self {
        self.directives.remove(name);
        self.explicit.remove(name);
        self
    }

    /// Append an extra `script-src` source after the nonce and
    /// `'strict-dynamic'`.
    ///
    /// With `'strict-dynamic'` in play, modern browsers ignore host
    /// allowlists in `script-src`, so this is mainly for keyword sources
    /// (`'wasm-unsafe-eval'`) or as a fallback for browsers without
    /// `'strict-dynamic'` support.
    pub fn allow_script_source(mut self, source: impl Into<String>) -> Self {
        self.script_src_extra.push(source.into());
        self
    }

    /// Send `Content-Security-Policy-Report-Only` instead of the enforcing
    /// header. Use this to observe violations before switching a live app
    /// over to enforcement.
    pub fn report_only(mut self, report_only: bool) -> Self {
        self.report_only = report_only;
        self
    }

    /// Whether this policy is observation-only. A report-only policy writes a
    /// different header, so it cannot collide with a configured static CSP
    /// and it does not actually block framing.
    pub(crate) fn is_report_only(&self) -> bool {
        self.report_only
    }

    /// Whether `frame-ancestors` still holds Proa's restrictive default.
    /// The builder uses this to flag a policy that would keep an app from
    /// being framed even though `X-Frame-Options` was deliberately relaxed.
    pub(crate) fn frame_ancestors_is_default(&self) -> bool {
        !self.explicit.contains("frame-ancestors")
            && self.directives.contains_key("frame-ancestors")
    }

    /// Split the assembled policy around the nonce value so per-request work
    /// is one exact-capacity allocation.
    fn resolve(self) -> ResolvedCsp {
        let header_name = if self.report_only {
            HeaderName::from_static("content-security-policy-report-only")
        } else {
            header::CONTENT_SECURITY_POLICY
        };

        // proa-perf-ok(scope): one-time layer construction, not a render path.
        let mut prefix = String::new();
        let mut suffix = String::new();

        // `script-src` sorts among the other directives so the emitted
        // policy stays stable and readable; everything before it lands in
        // the prefix, everything after in the suffix.
        let mut before_script = true;
        for (name, value) in &self.directives {
            if before_script && name.as_ref() > "script-src" {
                before_script = false;
            }
            let out = if before_script {
                &mut prefix
            } else {
                &mut suffix
            };
            if !out.is_empty() {
                out.push_str("; ");
            }
            out.push_str(name);
            out.push(' ');
            out.push_str(value);
        }

        if !prefix.is_empty() {
            prefix.push_str("; ");
        }
        prefix.push_str("script-src ");
        prefix.push_str(SCRIPT_SRC_PREFIX);

        let mut tail = String::from(SCRIPT_SRC_SUFFIX);
        for source in &self.script_src_extra {
            tail.push(' ');
            tail.push_str(source);
        }
        if !suffix.is_empty() {
            tail.push_str("; ");
            tail.push_str(&suffix);
        }

        ResolvedCsp {
            header_name,
            prefix: prefix.into_boxed_str(),
            suffix: tail.into_boxed_str(),
            valid: true,
        }
        .validated()
    }
}

/// Header name plus the two halves of the policy string, resolved once.
#[derive(Debug)]
struct ResolvedCsp {
    header_name: HeaderName,
    prefix: Box<str>,
    suffix: Box<str>,
    /// Cleared when the assembled policy is not a legal header value, so a
    /// malformed directive drops the header instead of panicking per request.
    valid: bool,
}

impl ResolvedCsp {
    /// Prove at construction that the policy forms a valid header value, so
    /// the per-request path cannot fail on a value the app cannot fix.
    fn validated(mut self) -> Self {
        let sample = CspNonce::new("c2FtcGxlLW5vbmNlLXZhbHVl").expect("sample nonce is base64");
        if self.header_value(&sample).is_none() {
            tracing::warn!(
                "assembled Content-Security-Policy contains characters that are \
                 illegal in a header value; the header will not be sent"
            );
            self.valid = false;
        }
        self
    }

    fn header_value(&self, nonce: &CspNonce) -> Option<HeaderValue> {
        let nonce = nonce.as_str();
        // proa-perf-ok: one exact-capacity allocation per response; a header
        // value must own its bytes, and `from_maybe_shared` avoids a second copy.
        let mut policy = String::with_capacity(self.prefix.len() + nonce.len() + self.suffix.len());
        policy.push_str(&self.prefix);
        policy.push_str(nonce);
        policy.push_str(&self.suffix);
        HeaderValue::from_maybe_shared(Bytes::from(policy)).ok()
    }
}

/// Generate a fresh 128-bit nonce for one response.
///
/// `rand::rng()` is a cryptographically secure, OS-seeded, automatically
/// reseeded generator; a predictable nonce would let an injected script
/// name it and defeat the policy.
fn fresh_nonce() -> CspNonce {
    let mut random = [0_u8; NONCE_BYTES];
    rand::rng().fill_bytes(&mut random);
    // proa-perf-ok: one small allocation per response, off the render path.
    CspNonce::new(STANDARD_NO_PAD.encode(random)).expect("base64 output is a valid CSP nonce")
}

/// Tower layer that mints a per-request [`CspNonce`] and writes the matching
/// `Content-Security-Policy` header.
///
/// The nonce goes into the request extensions before the inner service runs,
/// so it must sit outside
/// [`RouteContextLayer`](crate::route::RouteContextLayer);
/// [`FrameworkBuilder::with_strict_csp`](crate::FrameworkBuilder::with_strict_csp)
/// places it correctly. The header is only written when the response does
/// not already carry one, so a handler that sets its own policy wins.
#[derive(Debug, Clone)]
pub struct CspNonceLayer {
    resolved: Arc<ResolvedCsp>,
}

impl CspNonceLayer {
    pub fn new(policy: CspPolicy) -> Self {
        Self {
            resolved: Arc::new(policy.resolve()),
        }
    }

    /// Shorthand for [`CspPolicy::strict`].
    pub fn strict() -> Self {
        Self::new(CspPolicy::strict())
    }
}

impl Default for CspNonceLayer {
    fn default() -> Self {
        Self::strict()
    }
}

impl<S> Layer<S> for CspNonceLayer {
    type Service = CspNonceService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CspNonceService {
            inner,
            resolved: Arc::clone(&self.resolved),
        }
    }
}

/// Tower service produced by [`CspNonceLayer`].
#[derive(Debug, Clone)]
pub struct CspNonceService<S> {
    inner: S,
    resolved: Arc<ResolvedCsp>,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for CspNonceService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = CspNonceFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let nonce = fresh_nonce();
        let header = self
            .resolved
            .valid
            .then(|| self.resolved.header_value(&nonce))
            .flatten()
            .map(|value| (self.resolved.header_name.clone(), value));
        // The render path reads this back through `RouteContext`, which
        // installs it on every nested and deferred render context.
        req.extensions_mut().insert(nonce);

        CspNonceFuture {
            inner: self.inner.call(req),
            header,
        }
    }
}

/// Future returned by [`CspNonceService`].
#[pin_project]
pub struct CspNonceFuture<F> {
    #[pin]
    inner: F,
    header: Option<(HeaderName, HeaderValue)>,
}

impl<F, ResBody, E> Future for CspNonceFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut result = ready!(this.inner.poll(cx));
        if let Ok(response) = &mut result {
            if let Some((name, value)) = this.header.take() {
                let headers = response.headers_mut();
                if !headers.contains_key(&name) {
                    headers.insert(name, value);
                }
            }
        }
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use std::convert::Infallible;
    use tower::{service_fn, ServiceExt};

    fn policy_string(policy: CspPolicy) -> String {
        let resolved = policy.resolve();
        let nonce = CspNonce::new("dGVzdC1ub25jZQ").unwrap();
        resolved
            .header_value(&nonce)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn strict_policy_carries_the_nonce_and_strict_dynamic() {
        assert_eq!(
            policy_string(CspPolicy::strict()),
            "base-uri 'self'; default-src 'self'; frame-ancestors 'none'; \
             object-src 'none'; script-src 'nonce-dGVzdC1ub25jZQ' 'strict-dynamic'; \
             style-src 'self' 'unsafe-inline'"
        );
    }

    #[test]
    fn strict_policy_never_allows_unsafe_inline_script() {
        let policy = policy_string(CspPolicy::strict());
        let script_src = policy
            .split("; ")
            .find(|directive| directive.starts_with("script-src "))
            .unwrap();
        assert!(!script_src.contains("'unsafe-inline'"));
        assert!(!script_src.contains("'unsafe-eval'"));
    }

    #[test]
    fn directives_can_be_overridden_added_and_removed() {
        let policy = policy_string(
            CspPolicy::strict()
                .with_directive("style-src", "'self'")
                .with_directive("img-src", "'self' data:")
                .without_directive("frame-ancestors"),
        );
        assert!(policy.contains("style-src 'self'"));
        assert!(!policy.contains("'unsafe-inline'"));
        assert!(policy.contains("img-src 'self' data:"));
        assert!(!policy.contains("frame-ancestors"));
    }

    #[test]
    fn extra_script_sources_follow_the_nonce() {
        let policy = policy_string(CspPolicy::strict().allow_script_source("'wasm-unsafe-eval'"));
        assert!(policy
            .contains("script-src 'nonce-dGVzdC1ub25jZQ' 'strict-dynamic' 'wasm-unsafe-eval';"));
    }

    #[test]
    fn script_src_cannot_be_replaced_wholesale() {
        let policy = policy_string(CspPolicy::strict().with_directive("script-src", "'self'"));
        assert!(policy.contains("script-src 'nonce-dGVzdC1ub25jZQ' 'strict-dynamic'"));
    }

    #[test]
    fn a_directive_sorting_after_script_src_still_renders() {
        // `style-src` and anything later lands in the suffix half of the
        // split; a lost separator there would corrupt the policy.
        let policy = policy_string(CspPolicy::strict().with_directive("worker-src", "'self'"));
        assert!(policy.contains("'strict-dynamic'; style-src"));
        assert!(policy.ends_with("worker-src 'self'"));
    }

    #[test]
    fn report_only_uses_the_report_only_header_name() {
        let resolved = CspPolicy::strict().report_only(true).resolve();
        assert_eq!(resolved.header_name, "content-security-policy-report-only");
        assert!(resolved.valid);
    }

    #[test]
    fn a_malformed_directive_drops_the_header_instead_of_panicking() {
        let resolved = CspPolicy::strict()
            .with_directive("img-src", "bad\nvalue")
            .resolve();
        assert!(!resolved.valid);
    }

    #[test]
    fn frame_ancestors_default_is_distinguishable_from_an_explicit_value() {
        assert!(CspPolicy::strict().frame_ancestors_is_default());
        assert!(!CspPolicy::strict()
            .with_directive("frame-ancestors", "https://embedder.example")
            .frame_ancestors_is_default());
        assert!(!CspPolicy::strict()
            .without_directive("frame-ancestors")
            .frame_ancestors_is_default());
    }

    #[test]
    fn generated_nonces_are_unique_and_carry_128_bits() {
        let first = fresh_nonce();
        let second = fresh_nonce();
        assert_ne!(first.as_str(), second.as_str());
        // 16 bytes, base64 without padding.
        assert_eq!(first.as_str().len(), 22);
    }

    async fn call_layer<F>(layer: CspNonceLayer, handler: F) -> Response<Body>
    where
        F: Fn(Request<Body>) -> Response<Body> + Clone + Send + 'static,
    {
        let service = layer.layer(service_fn(move |req: Request<Body>| {
            let handler = handler.clone();
            async move { Ok::<_, Infallible>(handler(req)) }
        }));
        service
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_response_header_matches_the_nonce_in_the_request_extensions() {
        let response = call_layer(CspNonceLayer::strict(), |req| {
            let nonce = req
                .extensions()
                .get::<CspNonce>()
                .expect("layer installs the request nonce");
            Response::new(Body::from(nonce.as_str().to_owned()))
        })
        .await;

        let header = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let body = axum::body::to_bytes(response.into_body(), 64)
            .await
            .unwrap();
        let seen_by_handler = std::str::from_utf8(&body).unwrap();

        assert!(header.contains(&format!("'nonce-{seen_by_handler}'")));
    }

    #[tokio::test]
    async fn a_handler_authored_policy_is_not_overwritten() {
        let response = call_layer(CspNonceLayer::strict(), |_req| {
            let mut response = Response::new(Body::empty());
            response.headers_mut().insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'none'"),
            );
            response
        })
        .await;

        assert_eq!(
            response.headers().get(header::CONTENT_SECURITY_POLICY),
            Some(&HeaderValue::from_static("default-src 'none'"))
        );
    }
}
