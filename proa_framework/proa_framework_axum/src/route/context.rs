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
use proa_core::{CspNonce, RouteParam, RouteParams};
use proa_framework_core::{
    extract_route_params, ComputedRouteMeta, FrameworkRouteSnapshot, MetadataRef, ResolvedMetadata,
};
use tower::layer::Layer;
use tower::Service;

use crate::manifest::IslandManifest;

use super::local_executor::{LocalStreamExecutor, LocalStreamExecutorConfig};
use super::response::prepare_deferred_framework_stream;

tokio::task_local! {
    pub(super) static ROUTE_CONTEXT: RefCell<RouteContext>;
}

/// Internal route context holding the manifest and request metadata.
#[doc(hidden)]
#[derive(Clone)]
pub struct RouteContext {
    snapshot: FrameworkRouteSnapshot,
    request_method: Option<http::Method>,
    request_headers: http::HeaderMap,
    csp_nonce: Option<CspNonce>,
    base_path: proa_core::BasePath,
    head: Option<proa_core::HeadConfig>,
}

impl RouteContext {
    pub(super) fn new(
        manifest: Arc<IslandManifest>,
        computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        Self {
            snapshot: FrameworkRouteSnapshot::new(manifest, computed_metadata),
            request_method: None,
            request_headers: http::HeaderMap::new(),
            csp_nonce: None,
            base_path: proa_core::BasePath::default(),
            head: None,
        }
    }

    pub(super) fn head_config(&self) -> Option<&proa_core::HeadConfig> {
        self.head.as_ref()
    }

    pub(super) fn base_path(&self) -> &proa_core::BasePath {
        &self.base_path
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

    pub(super) fn set_current_request_parts(
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

    pub(super) fn current_route_params(&self) -> &RouteParams {
        self.snapshot.current_route_params()
    }

    pub(super) fn swap_route_params(&mut self, route_params: RouteParams) -> RouteParams {
        self.snapshot.swap_route_params(route_params)
    }

    pub(super) fn restore_route_params(&mut self, route_params: RouteParams) {
        self.snapshot.restore_route_params(route_params);
    }

    pub(super) fn manifest_arc(&self) -> Arc<IslandManifest> {
        self.snapshot.manifest_arc()
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

/// Helper type for managing metadata stacks.
#[derive(Clone)]
pub(super) struct Stack<T>(Arc<[T]>);

impl<T: Copy> Stack<T> {
    pub(super) fn from_slice(items: &[T]) -> Self {
        if items.is_empty() {
            Self(Arc::from([]))
        } else {
            let owned: Vec<T> = items.to_vec();
            Self(Arc::from(owned.into_boxed_slice()))
        }
    }

    pub(super) fn arc(&self) -> Arc<[T]> {
        self.0.clone()
    }
}

/// Tower layer for managing endpoint-specific metadata and route params.
#[derive(Clone)]
pub(super) struct EndpointContextLayer {
    metadata: Stack<MetadataRef>,
    route_pattern: Arc<str>,
}

impl EndpointContextLayer {
    pub(super) fn new(metadata: Stack<MetadataRef>, route_pattern: Arc<str>) -> Self {
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
    metadata: Stack<MetadataRef>,
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
        let uri = req.uri().clone();
        let (previous_metadata, previous_resolved, previous_route_params) =
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
                // Prefer Axum's decoded captures, including nested and wildcard
                // parameters. Keep pattern extraction for non-router adapters.
                let route_params = if ctx.current_route_params().is_empty() {
                    extract_route_params(self.route_pattern.as_ref(), uri.path())
                } else {
                    ctx.current_route_params().clone()
                };
                ctx.set_current_uri(uri);
                let (previous_metadata, previous_resolved) = ctx.swap_metadata(self.metadata.arc());
                let previous_route_params = ctx.swap_route_params(route_params);
                (previous_metadata, previous_resolved, previous_route_params)
            });

        EndpointContextFuture {
            future: self.inner.call(req),
            previous_metadata,
            previous_resolved,
            previous_route_params,
            restored: false,
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
    previous_metadata: Arc<[MetadataRef]>,
    previous_resolved: Option<ResolvedMetadata>,
    previous_route_params: RouteParams,
    restored: bool,
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
                if !*this.restored {
                    restore_context(
                        this.previous_metadata.clone(),
                        this.previous_resolved.clone(),
                        this.previous_route_params.clone(),
                    );
                    *this.restored = true;
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
        if !*this.restored {
            restore_context(
                this.previous_metadata.clone(),
                this.previous_resolved.clone(),
                this.previous_route_params.clone(),
            );
        }
    }
}

fn restore_context(
    previous_metadata: Arc<[MetadataRef]>,
    previous_resolved: Option<ResolvedMetadata>,
    previous_route_params: RouteParams,
) {
    ROUTE_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.restore_metadata(previous_metadata, previous_resolved);
        ctx.restore_route_params(previous_route_params);
    });
}

/// Tower layer for establishing the route context for each request
#[derive(Clone)]
pub struct RouteContextLayer {
    manifest: Arc<IslandManifest>,
    unprefixed_manifest: Arc<IslandManifest>,
    computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    local_stream_executor: LocalStreamExecutor,
    base_path: proa_core::BasePath,
    head: Option<proa_core::HeadConfig>,
}

impl RouteContextLayer {
    pub fn new(
        manifest: Arc<IslandManifest>,
        computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    ) -> Self {
        Self {
            unprefixed_manifest: manifest.clone(),
            manifest,
            computed_metadata,
            local_stream_executor: LocalStreamExecutor::new(LocalStreamExecutorConfig::default()),
            base_path: proa_core::BasePath::default(),
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
        self.manifest = if base_path.as_str().is_empty() {
            self.unprefixed_manifest.clone()
        } else {
            Arc::new(
                self.unprefixed_manifest
                    .as_ref()
                    .clone()
                    .with_base_path(&base_path),
            )
        };
        self.base_path = base_path;
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
            manifest: self.manifest.clone(),
            computed_metadata: self.computed_metadata.clone(),
            local_stream_executor: self.local_stream_executor.clone(),
            base_path: self.base_path.clone(),
            head: self.head.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RouteContextService<S> {
    inner: S,
    manifest: Arc<IslandManifest>,
    computed_metadata: Arc<std::collections::HashMap<String, ComputedRouteMeta>>,
    local_stream_executor: LocalStreamExecutor,
    base_path: proa_core::BasePath,
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

        let mut context = RouteContext::new(self.manifest.clone(), self.computed_metadata.clone());
        context.head = self.head.clone();
        context.set_is_client_request(is_client_request);
        context.set_current_request_parts(current_method, current_headers, current_uri);
        context.set_csp_nonce(csp_nonce);
        context.base_path = self.base_path.clone();

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
    let pattern = pattern.as_str();
    let has_named = pattern.split('/').any(|part| part.starts_with(':'));
    let has_wildcard = pattern.split('/').any(|part| part.starts_with('*'));
    if !has_named && !has_wildcard {
        return true;
    }
    if has_wildcard {
        return false;
    }
    let current = req.uri().path();
    let path = match req
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path())
    {
        None => current,
        Some(original) if original == current => current,
        Some(original) if original.ends_with(current) => original,
        Some(_) => return false,
    };
    if path.contains('%') {
        return false;
    }
    let params = extract_route_params(pattern, path);
    if params.is_empty() {
        return false;
    }
    context.swap_route_params(params);
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
        let manifest = IslandManifest {
            islands: Default::default(),
            assets: Default::default(),
            import_map: Default::default(),
            ssr_bundle: Default::default(),
        };
        let layer = RouteContextLayer::new(Arc::new(manifest), Arc::new(Default::default()));
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
