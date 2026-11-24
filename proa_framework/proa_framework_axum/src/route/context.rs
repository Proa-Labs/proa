// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{
    rejection::RawPathParamsRejection, FromRequestParts, MatchedPath, OriginalUri, RawPathParams,
};
use axum::response::{IntoResponse, Response};
use pin_project::{pin_project, pinned_drop};
use proa_core::{CspNonce, PublicAssetUrls, RouteParam, RouteParams};
use proa_framework_core::{
    extract_route_params, has_named_route_params, ComputedRouteMeta, FrameworkRouteSnapshot,
    MetadataRef, ResolvedMetadata,
};
use tower::layer::Layer;
use tower::Service;

use super::local_executor::{LocalStreamExecutor, LocalStreamExecutorConfig};
use super::response::prepare_deferred_framework_stream;

tokio::task_local! {
    pub(super) static ROUTE_CONTEXT: RefCell<RouteContext>;
}

/// Internal route context holding the request metadata.
#[doc(hidden)]
#[derive(Clone)]
pub struct RouteContext {
    snapshot: FrameworkRouteSnapshot,
    request_method: Option<http::Method>,
    request_headers: CapturedHeaders,
    matched_params: Option<MatchedParams>,
    csp_nonce: Option<CspNonce>,
    base_path: proa_core::BasePath,
    public_asset_urls: Option<PublicAssetUrls>,
    framework_assets: Option<proa_core::FrameworkAssets>,
    asset_prefix: Option<Arc<str>>,
    head: Option<proa_core::HeadConfig>,
}

#[derive(Clone)]
struct MatchedParams {
    pattern: MatchedPath,
    uri: http::Uri,
    parsed: OnceLock<RouteParams>,
}

// A render may inspect any original header, including repeated values, after
// middleware has changed the request. Avoid a second hash table on small requests.
#[derive(Clone)]
enum CapturedHeaders {
    Inline(arrayvec::ArrayVec<(http::HeaderName, http::HeaderValue), 4>),
    Map(http::HeaderMap),
}

impl CapturedHeaders {
    fn capture(headers: &http::HeaderMap) -> Self {
        if headers.len() <= 4 {
            Self::Inline(
                headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        } else {
            Self::Map(headers.clone())
        }
    }

    fn to_header_map(&self) -> http::HeaderMap {
        match self {
            Self::Inline(entries) => {
                let mut headers = http::HeaderMap::with_capacity(entries.len());
                for (name, value) in entries {
                    headers.append(name, value.clone());
                }
                headers
            }
            Self::Map(headers) => headers.clone(),
        }
    }
}

impl RouteContext {
    pub(super) fn new(
        computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        Self {
            snapshot: FrameworkRouteSnapshot::new(computed_metadata),
            request_method: None,
            request_headers: CapturedHeaders::Inline(Default::default()),
            matched_params: None,
            csp_nonce: None,
            base_path: proa_core::BasePath::default(),
            public_asset_urls: None,
            framework_assets: None,
            asset_prefix: None,
            head: None,
        }
    }

    pub(super) fn head_config(&self) -> Option<&proa_core::HeadConfig> {
        self.head.as_ref()
    }

    pub(super) fn base_path(&self) -> &proa_core::BasePath {
        &self.base_path
    }

    pub(super) fn public_asset_urls(&self) -> Option<PublicAssetUrls> {
        self.public_asset_urls.clone()
    }

    pub(super) fn framework_assets(&self) -> Option<proa_core::FrameworkAssets> {
        self.framework_assets.clone()
    }

    pub(super) fn asset_prefix(&self) -> Option<Arc<str>> {
        self.asset_prefix.clone()
    }

    pub(super) fn set_is_client_request(&mut self, is_client: bool) {
        self.snapshot.set_is_client_request(is_client);
    }

    pub(super) fn is_client_request(&self) -> bool {
        self.snapshot.is_client_request()
    }

    pub(super) fn set_current_uri(&mut self, uri: http::Uri) {
        self.snapshot.set_current_uri(uri);
    }

    pub(super) fn get_computed_metadata(&self, path: &str) -> Option<ComputedRouteMeta> {
        self.snapshot.get_computed_metadata(path)
    }

    pub(super) fn current_path(&self) -> Option<&str> {
        self.snapshot.current_path()
    }

    pub(super) fn current_uri(&self) -> Option<&http::Uri> {
        self.snapshot.current_uri()
    }

    fn set_current_request_parts(
        &mut self,
        method: http::Method,
        headers: CapturedHeaders,
        uri: http::Uri,
    ) {
        self.request_method = Some(method);
        self.request_headers = headers;
        self.snapshot.set_current_uri(uri);
    }

    pub(super) fn current_method(&self) -> Option<&http::Method> {
        self.request_method.as_ref()
    }

    pub(super) fn current_headers(&self) -> http::HeaderMap {
        self.request_headers.to_header_map()
    }

    pub(super) fn set_csp_nonce(&mut self, nonce: Option<CspNonce>) {
        self.csp_nonce = nonce;
    }

    pub(super) fn csp_nonce(&self) -> Option<&CspNonce> {
        self.csp_nonce.as_ref()
    }

    pub(super) fn current_route_params(&self) -> &RouteParams {
        if let Some(params) = &self.matched_params {
            params
                .parsed
                .get_or_init(|| extract_route_params(params.pattern.as_str(), params.uri.path()))
        } else {
            self.snapshot.current_route_params()
        }
    }

    pub(super) fn swap_route_params(&mut self, route_params: RouteParams) -> RouteParams {
        let previous = self.snapshot.swap_route_params(route_params);
        self.matched_params.take().map_or(previous, |params| {
            params
                .parsed
                .into_inner()
                .unwrap_or_else(|| extract_route_params(params.pattern.as_str(), params.uri.path()))
        })
    }

    pub(super) fn restore_route_params(&mut self, route_params: RouteParams) {
        self.snapshot.restore_route_params(route_params);
    }

    pub(super) fn swap_metadata(
        &mut self,
        metadata: Arc<[MetadataRef]>,
    ) -> (Arc<[MetadataRef]>, Option<ResolvedMetadata>) {
        self.snapshot.swap_metadata(metadata)
    }

    pub(super) fn restore_metadata(
        &mut self,
        metadata: Arc<[MetadataRef]>,
        resolved: Option<ResolvedMetadata>,
    ) {
        self.snapshot.restore_metadata(metadata, resolved);
    }

    pub(super) fn resolve_metadata(&mut self) -> Option<ResolvedMetadata> {
        self.snapshot.resolve_metadata()
    }
}

/// Tower layer for managing endpoint-specific metadata and route params.
#[derive(Clone)]
pub(super) struct EndpointContextLayer {
    metadata: Arc<[MetadataRef]>,
    route_pattern: Arc<str>,
}

impl EndpointContextLayer {
    pub(super) fn new(metadata: Arc<[MetadataRef]>, route_pattern: Arc<str>) -> Self {
        Self {
            metadata,
            route_pattern,
        }
    }
}

impl<S> Layer<S> for EndpointContextLayer {
    type Service = EndpointContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EndpointContextService {
            inner,
            metadata: self.metadata.clone(),
            route_pattern: self.route_pattern.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct EndpointContextService<S> {
    inner: S,
    metadata: Arc<[MetadataRef]>,
    route_pattern: Arc<str>,
}

impl<S> Service<axum::http::Request<Body>> for EndpointContextService<S>
where
    S: Service<axum::http::Request<Body>, Response = Response> + Clone,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = EndpointContextFuture<S::Future, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: axum::http::Request<Body>) -> Self::Future {
        let previous = ROUTE_CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            let uri = req.uri();
            // The outer layer already owns Axum's captures on normal routes.
            // A custom, outer router can reach this endpoint before matching.
            let route_params = if ctx.matched_params.is_none()
                && ctx.snapshot.current_route_params().is_empty()
                && self.route_pattern.contains(['{', ':'])
            {
                let params = extract_route_params(&self.route_pattern, uri.path());
                (!params.is_empty()).then(|| ctx.swap_route_params(params))
            } else {
                None
            };
            ctx.set_current_uri(uri.clone());
            if self.metadata.is_empty() && !ctx.snapshot.has_metadata() {
                return route_params.map(|params| PreviousContext {
                    metadata: None,
                    route_params: Some(params),
                });
            }

            let (metadata, resolved) = ctx.swap_metadata(self.metadata.clone());
            Some(PreviousContext {
                metadata: Some(PreviousMetadata {
                    providers: metadata,
                    resolved: resolved.map(Box::new),
                }),
                route_params,
            })
        });
        EndpointContextFuture {
            future: self.inner.call(req),
            previous,
            _marker: PhantomData,
        }
    }
}

#[pin_project(PinnedDrop)]
pub(super) struct EndpointContextFuture<F, E>
where
    F: Future<Output = Result<Response, E>>,
{
    #[pin]
    future: F,
    previous: Option<PreviousContext>,
    _marker: PhantomData<E>,
}

impl<F, E> Future for EndpointContextFuture<F, E>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = Result<Response, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.future.poll(cx) {
            Poll::Ready(result) => {
                if let Some(previous) = this.previous.take() {
                    previous.restore();
                }
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[pinned_drop]
impl<F, E> PinnedDrop for EndpointContextFuture<F, E>
where
    F: Future<Output = Result<Response, E>>,
{
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        if let Some(previous) = this.previous.take() {
            previous.restore();
        }
    }
}

struct PreviousMetadata {
    providers: Arc<[MetadataRef]>,
    resolved: Option<Box<ResolvedMetadata>>,
}

struct PreviousContext {
    metadata: Option<PreviousMetadata>,
    route_params: Option<RouteParams>,
}

impl PreviousContext {
    fn restore(self) {
        ROUTE_CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            if let Some(metadata) = self.metadata {
                ctx.restore_metadata(metadata.providers, metadata.resolved.map(|value| *value));
            }
            if let Some(params) = self.route_params {
                ctx.restore_route_params(params);
            }
        });
    }
}

/// Tower layer for establishing the route context for each request
#[derive(Clone)]
pub struct RouteContextLayer {
    computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    local_stream_executor: LocalStreamExecutor,
    base_path: proa_core::BasePath,
    public_assets: Option<crate::PublicAssets>,
    framework_assets: Option<proa_core::FrameworkAssets>,
    head: Option<proa_core::HeadConfig>,
}

impl RouteContextLayer {
    /// Create a layer with no managed head.
    pub fn new(
        computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        Self {
            computed_metadata,
            local_stream_executor: LocalStreamExecutor::new(LocalStreamExecutorConfig::default()),
            base_path: proa_core::BasePath::default(),
            public_assets: None,
            framework_assets: None,
            head: None,
        }
    }

    /// Enable managed head rendering for the document responses in this layer.
    pub fn with_head(mut self, head: proa_core::HeadConfig) -> Self {
        self.head = Some(head);
        self
    }

    /// Set the public mount prefix without changing incoming routes.
    pub fn with_base_path(mut self, base_path: proa_core::BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    /// Use this catalog and its base path when rendering through a custom router.
    /// Merge [`PublicAssets::router`](crate::PublicAssets::router) to serve the URLs.
    /// The layer also provides the catalog as an Axum request extension.
    pub fn with_public_assets(mut self, assets: crate::PublicAssets) -> Self {
        self.base_path = assets.base_path().clone();
        self.public_assets = Some(assets);
        self
    }

    /// Link the RSJS modules by the names `assets` publishes them under.
    /// Merge [`RsjsAssets::router`](crate::RsjsAssets::router) to serve them.
    pub fn with_rsjs_assets(mut self, assets: &crate::RsjsAssets) -> Self {
        self.framework_assets = Some(assets.published().clone());
        self
    }

    /// Configure the bounded executor used only for genuinely local streams.
    pub fn with_local_stream_executor_config(mut self, config: LocalStreamExecutorConfig) -> Self {
        self.local_stream_executor = LocalStreamExecutor::new(config);
        self
    }
}

impl<S> Layer<S> for RouteContextLayer {
    type Service = RouteContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RouteContextService {
            inner,
            computed_metadata: self.computed_metadata.clone(),
            local_stream_executor: self.local_stream_executor.clone(),
            base_path: self.base_path.clone(),
            public_assets: self.public_assets.clone(),
            framework_assets: self.framework_assets.clone(),
            head: self.head.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RouteContextService<S> {
    inner: S,
    computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    local_stream_executor: LocalStreamExecutor,
    base_path: proa_core::BasePath,
    public_assets: Option<crate::PublicAssets>,
    framework_assets: Option<proa_core::FrameworkAssets>,
    head: Option<proa_core::HeadConfig>,
}

impl<S> Service<axum::http::Request<Body>> for RouteContextService<S>
where
    S: Service<axum::http::Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<Body>) -> Self::Future {
        req.extensions_mut().insert(self.base_path.clone());
        // Check if this is a client-side navigation request
        let is_client_request = req
            .headers()
            .get("X-Proa-Client")
            .and_then(|v| v.to_str().ok())
            .is_some();
        let current_method = req.method().clone();
        let current_headers = CapturedHeaders::capture(req.headers());
        let current_uri = req.uri().clone();
        let csp_nonce = req.extensions().get::<CspNonce>().cloned();

        let mut context = RouteContext::new(self.computed_metadata.clone());
        context.head = self.head.clone();
        context.set_is_client_request(is_client_request);
        context.set_current_request_parts(current_method, current_headers, current_uri);
        context.set_csp_nonce(csp_nonce);
        context.base_path = self.base_path.clone();
        context.framework_assets = self.framework_assets.clone();
        if let Some(assets) = &self.public_assets {
            // One generation per request: the URLs rendered into the page and
            // the cache key a later layer reads from the extension must agree,
            // even when `proa dev` republishes between the two.
            let assets = assets.current();
            context.public_asset_urls = Some(assets.render_urls());
            context.asset_prefix = assets.asset_prefix().map(crate::AssetPrefix::shared);
            req.extensions_mut().insert(assets);
        }

        if prepare_ascii_params(&req, &mut context) {
            // Static and ordinary named paths keep the original ready-service
            // path: no extractor future, service clone, or replacement params.
            let future = self.inner.call(req);
            let scoped = ROUTE_CONTEXT.scope(RefCell::new(context), future);
            let local_stream_executor = self.local_stream_executor.clone();
            return Box::pin(async move {
                let response = scoped.await?;
                Ok(prepare_deferred_framework_stream(response, local_stream_executor).await)
            });
        }

        // Move the ready service into the future; its clone may need poll_ready
        // before use on the next request.
        let replacement = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, replacement);
        let local_stream_executor = self.local_stream_executor.clone();
        Box::pin(async move {
            let (mut parts, body) = req.into_parts();
            let params = match RawPathParams::from_request_parts(&mut parts, &()).await {
                Ok(params) => RouteParams::from_vec(
                    params
                        .iter()
                        .map(|(name, value)| {
                            RouteParam::new(
                                name.to_owned().into_boxed_str(),
                                value.to_owned().into_boxed_str(),
                            )
                        })
                        .collect(),
                ),
                Err(RawPathParamsRejection::MissingPathParams(_)) => RouteParams::default(),
                Err(rejection) => return Ok(rejection.into_response()),
            };
            context.swap_route_params(params);
            let request = axum::http::Request::from_parts(parts, body);
            let response = ROUTE_CONTEXT
                .scope(
                    RefCell::new(context),
                    async move { inner.call(request).await },
                )
                .await?;
            Ok(prepare_deferred_framework_stream(response, local_stream_executor).await)
        })
    }
}

/// Match unescaped named segments synchronously. Axum still owns decoding and
/// wildcard semantics on the cold path below. A nested router strips its prefix
/// from the current URI while `MatchedPath` keeps the full pattern, so the full
/// path is read from `OriginalUri` when the current path is its tail. Any other
/// divergence between the two means a rewrite happened between them; only
/// Axum's own captures are authoritative then, so that request also takes the
/// cold path.
fn prepare_ascii_params(req: &axum::http::Request<Body>, context: &mut RouteContext) -> bool {
    let Some(pattern) = req.extensions().get::<MatchedPath>() else {
        return true;
    };
    let pattern_str = pattern.as_str();
    if !pattern_str.contains('{') {
        return true;
    }
    if pattern_str.contains("{*") || pattern_str.contains("{{") || pattern_str.contains("}}") {
        return false;
    }
    let current = req.uri();
    let uri = match req.extensions().get::<OriginalUri>() {
        None => current,
        Some(original) if original.0.path() == current.path() => current,
        Some(original) if original.0.path().ends_with(current.path()) => &original.0,
        Some(_) => return false,
    };
    if uri.path().contains('%') || !has_named_route_params(pattern_str, uri.path()) {
        return false;
    }
    context.matched_params = Some(MatchedParams {
        pattern: pattern.clone(),
        uri: uri.clone(),
        parsed: OnceLock::new(),
    });
    true
}

/// Compose a public URL for an application-local path in the current request.
/// Outside a framework request, the prefix defaults to the origin root.
/// Prefer `cx.url(path)` inside components, which also works in
/// suspended boundaries after the HTTP handler has completed.
pub fn app_url(path: &str) -> proa_core::AppUrl<'_> {
    ROUTE_CONTEXT
        .try_with(|ctx| ctx.borrow().base_path().url(path))
        .unwrap_or_else(|_| proa_core::BasePath::default().url(path))
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use std::{
        convert::Infallible,
        future::{ready, Ready},
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;

    #[test]
    fn header_capture_preserves_repeated_sensitive_values_after_request_mutation() {
        for extra in [0, 2, 3, 8] {
            let mut original = http::HeaderMap::new();
            let mut secret = http::HeaderValue::from_static("second");
            secret.set_sensitive(true);
            original.append("x-test", http::HeaderValue::from_static("first"));
            original.append("x-test", secret);
            for _ in 0..extra {
                original.append("x-extra", http::HeaderValue::from_static("value"));
            }
            let captured = CapturedHeaders::capture(&original);
            original.clear();
            let headers = captured.to_header_map();
            let values: Vec<_> = headers.get_all("x-test").iter().collect();
            assert_eq!(values.len(), 2);
            assert_eq!(values[0], "first");
            assert_eq!(values[1], "second");
            assert!(values[1].is_sensitive());
            assert_eq!(headers.get_all("x-extra").iter().count(), extra);
        }
    }

    #[tokio::test]
    async fn named_params_are_materialized_on_first_read_inside_the_endpoint() {
        async fn handler() {
            ROUTE_CONTEXT.with(|context| {
                let context = context.borrow();
                let captures = context.matched_params.as_ref().unwrap();
                assert!(captures.parsed.get().is_none());
                assert_eq!(context.current_route_params().get("album"), Some("sea"));
                assert_eq!(context.current_route_params().get("id"), Some("07"));
                assert!(std::ptr::eq(
                    captures.parsed.get().unwrap(),
                    context.current_route_params()
                ));
            });
        }
        let (router, metadata) = crate::router_from_spec(vec![crate::Route::endpoint(
            "/albums/{album}/{id}",
            axum::routing::get(handler),
        )]);
        let response = router
            .layer(RouteContextLayer::new(metadata))
            .oneshot(
                http::Request::get("/albums/sea/07")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);
    }

    struct ReadyService(Arc<AtomicUsize>);
    impl Clone for ReadyService {
        fn clone(&self) -> Self {
            self.0.fetch_add(1, Ordering::Relaxed);
            Self(self.0.clone())
        }
    }
    impl Service<axum::http::Request<Body>> for ReadyService {
        type Response = Response;
        type Error = Infallible;
        type Future = Ready<Result<Response, Infallible>>;
        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, _: axum::http::Request<Body>) -> Self::Future {
            ready(Ok(Response::new(Body::from("ok"))))
        }
    }

    #[tokio::test]
    async fn static_context_calls_the_ready_service_without_cloning_it() {
        let clones = Arc::new(AtomicUsize::new(0));
        let layer = RouteContextLayer::new(Arc::new(Default::default()));
        let mut service = layer.layer(ReadyService(clones.clone()));
        for _ in 0..3 {
            service
                .ready()
                .await
                .unwrap()
                .call(axum::http::Request::new(Body::empty()))
                .await
                .unwrap();
        }
        assert_eq!(clones.load(Ordering::Relaxed), 0);
    }
}
