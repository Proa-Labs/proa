// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{
    rejection::RawPathParamsRejection, FromRequestParts, MatchedPath, OriginalUri, RawPathParams,
};
use axum::response::{IntoResponse, Response};
use pin_project::{pin_project, pinned_drop};
use proa_core::{CspNonce, PublicAssetUrls, RouteParam, RouteParams};
use proa_framework_core::{
    extract_route_params, visit_route_params, ComputedRouteMeta, FrameworkRouteSnapshot,
    MetadataRef, ResolvedMetadata,
};
use tower::layer::Layer;
use tower::Service;

use super::cdn_cache::{finish_route_document, MayStore};
use super::local_executor::{LocalStreamExecutor, LocalStreamExecutorConfig};

tokio::task_local! {
    pub(super) static ROUTE_CONTEXT: RefCell<RouteContext>;
}

/// Internal route context holding the request metadata.
#[doc(hidden)]
#[derive(Clone)]
pub struct RouteContext {
    snapshot: FrameworkRouteSnapshot<AxumRouteParams>,
    request_method: Option<http::Method>,
    request_headers: http::HeaderMap,
    csp_nonce: Option<CspNonce>,
    base_path: proa_core::BasePath,
    public_asset_urls: Option<PublicAssetUrls>,
    framework_assets: Option<proa_core::FrameworkAssets>,
    asset_prefix: Option<Arc<str>>,
    head: Option<proa_core::HeadConfig>,
    /// Runs local buffered renders. See [`RouteContext::detach`].
    local_stream_executor: Option<LocalStreamExecutor>,
    /// A page cache may store this request's document.
    may_store: bool,
}

#[derive(Clone)]
enum AxumRouteParams {
    Ready(RouteParams),
    Matched(MatchedParams),
}

#[derive(Clone)]
struct MatchedParams {
    pattern: MatchedPath,
    path: http::uri::PathAndQuery,
    ranges: CaptureRanges,
}

impl MatchedParams {
    fn collect(&self) -> RouteParams {
        self.ranges
            .as_slice()
            .iter()
            .map(|capture| {
                RouteParam::new(
                    capture.name.slice(self.pattern.as_str()),
                    capture.value.slice(self.path.path()),
                )
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct CaptureRange {
    name: CaptureSpan,
    value: CaptureSpan,
}

#[derive(Clone, Copy)]
struct CaptureSpan {
    start: u32,
    end: u32,
}

impl CaptureSpan {
    fn from_substr(source: &str, capture: &str) -> Self {
        let range = source
            .substr_range(capture)
            .expect("capture is part of its source");
        Self {
            start: range.start as u32,
            end: range.end as u32,
        }
    }

    fn slice(self, source: &str) -> &str {
        &source[self.start as usize..self.end as usize]
    }
}

// A single named segment needs no heap storage until a renderer reads it.
#[derive(Clone)]
enum CaptureRanges {
    One(CaptureRange),
    Many(Vec<CaptureRange>),
}

impl CaptureRanges {
    fn as_slice(&self) -> &[CaptureRange] {
        match self {
            Self::One(capture) => std::slice::from_ref(capture),
            Self::Many(captures) => captures,
        }
    }

    fn for_path(pattern: &str, path: &str) -> Option<Self> {
        // Keep a single capture inline in the space already required for a Vec.
        // Defer to Axum if either source cannot be indexed with 32-bit offsets.
        if u32::try_from(pattern.len()).is_err()
            || u32::try_from(path.len()).is_err()
            || path.contains('%')
            || pattern.contains(':')
        {
            return None;
        }
        let mut captures = None;
        let matched = visit_route_params(pattern, path, |name, value| {
            let capture = CaptureRange {
                name: CaptureSpan::from_substr(pattern, name),
                value: CaptureSpan::from_substr(path, value),
            };
            match &mut captures {
                None => captures = Some(Self::One(capture)),
                Some(Self::One(first)) => captures = Some(Self::Many(vec![*first, capture])),
                Some(Self::Many(captures)) => captures.push(capture),
            }
        });
        if matched {
            captures
        } else {
            None
        }
    }
}

impl AxumRouteParams {
    fn is_empty(&self) -> bool {
        matches!(self, Self::Ready(params) if params.is_empty())
    }

    fn get(&mut self) -> &RouteParams {
        match self {
            Self::Ready(params) => params,
            Self::Matched(matched) => {
                *self = Self::Ready(matched.collect());
                self.get()
            }
        }
    }
}

impl RouteContext {
    pub(super) fn new(
        computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        Self {
            snapshot: FrameworkRouteSnapshot::with_route_params(
                computed_metadata,
                AxumRouteParams::Ready(RouteParams::default()),
            ),
            request_method: None,
            request_headers: http::HeaderMap::new(),
            csp_nonce: None,
            base_path: proa_core::BasePath::default(),
            public_asset_urls: None,
            framework_assets: None,
            asset_prefix: None,
            head: None,
            local_stream_executor: None,
            may_store: false,
        }
    }

    pub(super) fn head_config(&self) -> Option<&proa_core::HeadConfig> {
        self.head.as_ref()
    }

    /// A copy for a render that runs outside the handler, and the executor
    /// that runs it.
    ///
    /// The copy holds no executor handle. A copy moved onto a pool worker
    /// could otherwise drop the pool's last handle there.
    pub(super) fn detach(&self) -> (Self, Option<LocalStreamExecutor>) {
        let mut route = self.clone();
        let executor = route.local_stream_executor.take();
        (route, executor)
    }

    pub(super) fn local_stream_executor(&self) -> Option<LocalStreamExecutor> {
        self.local_stream_executor.clone()
    }

    pub(super) fn set_may_store(&mut self) {
        self.may_store = true;
    }

    pub(super) fn may_store(&self) -> bool {
        self.may_store
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
        headers: http::HeaderMap,
        uri: http::Uri,
    ) {
        self.request_method = Some(method);
        self.request_headers = headers;
        self.snapshot.set_current_uri(uri);
    }

    pub(super) fn current_method(&self) -> Option<&http::Method> {
        self.request_method.as_ref()
    }

    pub(super) fn current_headers(&self) -> &http::HeaderMap {
        &self.request_headers
    }

    pub(super) fn set_csp_nonce(&mut self, nonce: Option<CspNonce>) {
        self.csp_nonce = nonce;
    }

    pub(super) fn csp_nonce(&self) -> Option<&CspNonce> {
        self.csp_nonce.as_ref()
    }

    pub(super) fn current_route_params(&mut self) -> &RouteParams {
        self.snapshot.current_route_params_mut().get()
    }

    pub(super) fn set_route_params(&mut self, route_params: RouteParams) {
        self.snapshot
            .restore_route_params(AxumRouteParams::Ready(route_params));
    }

    // An outer router may install this layer before it matches the endpoint.
    // Only an initially empty parameter collection may be filled here.
    fn fill_missing_route_params(&mut self, pattern: &str, path: &str) -> bool {
        if !self.snapshot.current_route_params().is_empty() || !pattern.contains(['{', ':']) {
            return false;
        }
        let params = extract_route_params(pattern, path);
        if params.is_empty() {
            return false;
        }
        self.set_route_params(params);
        true
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
            let clear_params = ctx.fill_missing_route_params(&self.route_pattern, uri.path());
            ctx.set_current_uri(uri.clone());
            let metadata = if self.metadata.is_empty() && !ctx.snapshot.has_metadata() {
                None
            } else {
                let (providers, resolved) = ctx.swap_metadata(self.metadata.clone());
                Some(PreviousMetadata {
                    providers,
                    resolved,
                })
            };
            (clear_params || metadata.is_some()).then_some(PreviousContext {
                metadata,
                clear_params,
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
    resolved: Option<ResolvedMetadata>,
}

struct PreviousContext {
    metadata: Option<PreviousMetadata>,
    clear_params: bool,
}

impl PreviousContext {
    fn restore(self) {
        ROUTE_CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            if let Some(metadata) = self.metadata {
                ctx.restore_metadata(metadata.providers, metadata.resolved);
            }
            if self.clear_params {
                ctx.set_route_params(RouteParams::default());
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

    /// Configure the bounded executor used only for genuinely local renders.
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
        let current_headers = req.headers().clone();
        let current_uri = req.uri().clone();
        let csp_nonce = req.extensions().get::<CspNonce>().cloned();
        // A page cache outside this layer may store the document.
        let probe = matches!(req.uri().path(), "/healthz" | "/readyz");
        let outer_cache = req.extensions().get::<MayStore>().is_some();

        let mut context = RouteContext::new(self.computed_metadata.clone());
        context.may_store = outer_cache;
        context.head = self.head.clone();
        context.set_is_client_request(is_client_request);
        context.set_current_request_parts(current_method, current_headers, current_uri);
        context.set_csp_nonce(csp_nonce);
        context.base_path = self.base_path.clone();
        context.framework_assets = self.framework_assets.clone();
        context.local_stream_executor = Some(self.local_stream_executor.clone());
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
            let future = self.inner.call(req);
            let scoped = ROUTE_CONTEXT.scope(RefCell::new(context), future);
            let local_stream_executor = self.local_stream_executor.clone();
            return Box::pin(async move {
                let response = scoped.await?;
                Ok(
                    finish_route_document(response, local_stream_executor, outer_cache, probe)
                        .await,
                )
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
                Ok(params) => params
                    .iter()
                    .map(|(name, value)| RouteParam::new(name, value))
                    .collect(),
                Err(RawPathParamsRejection::MissingPathParams(_)) => RouteParams::default(),
                Err(rejection) => return Ok(rejection.into_response()),
            };
            context.set_route_params(params);
            let request = axum::http::Request::from_parts(parts, body);
            let response = ROUTE_CONTEXT
                .scope(
                    RefCell::new(context),
                    async move { inner.call(request).await },
                )
                .await?;
            Ok(finish_route_document(response, local_stream_executor, outer_cache, probe).await)
        })
    }
}

/// Keep the ready service for plain segments. Axum handles decoding, wildcards,
/// and URI rewrites. Store byte ranges from the match so a renderer can copy
/// just the captures without walking the pattern a second time.
fn prepare_ascii_params(req: &http::Request<Body>, context: &mut RouteContext) -> bool {
    let Some(pattern) = req.extensions().get::<MatchedPath>() else {
        return true;
    };
    if !pattern.as_str().contains('{') {
        return true;
    }
    let current = req.uri();
    let uri = match req.extensions().get::<OriginalUri>() {
        None => current,
        Some(original) if original.0.path() == current.path() => current,
        Some(original) if original.0.path().ends_with(current.path()) => &original.0,
        Some(_) => return false,
    };
    let Some(path) = uri.path_and_query() else {
        return false;
    };
    let Some(ranges) = CaptureRanges::for_path(pattern.as_str(), path.path()) else {
        return false;
    };
    context
        .snapshot
        .restore_route_params(AxumRouteParams::Matched(MatchedParams {
            pattern: pattern.clone(),
            path: path.clone(),
            ranges,
        }));
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
mod tests;
