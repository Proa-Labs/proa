// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::response::Response;
use pin_project::{pin_project, pinned_drop};
use proa_core::{CspNonce, RouteParams};
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
        }
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

    pub(super) fn metadata_stack(&self) -> &[MetadataRef] {
        self.snapshot.metadata_stack()
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
        let route_params = extract_route_params(self.route_pattern.as_ref(), uri.path());

        let (previous_metadata, previous_resolved, previous_route_params) =
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
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
        }
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
}

impl<S> Service<axum::http::Request<Body>> for RouteContextService<S>
where
    S: Service<axum::http::Request<Body>, Response = Response> + Clone,
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
        context.set_is_client_request(is_client_request);
        context.set_current_request_parts(current_method, current_headers, current_uri);
        context.set_csp_nonce(csp_nonce);
        context.base_path = self.base_path.clone();

        let future = self.inner.call(req);
        let scoped = ROUTE_CONTEXT.scope(RefCell::new(context), future);
        let local_stream_executor = self.local_stream_executor.clone();
        Box::pin(async move {
            let response = scoped.await?;
            Ok(prepare_deferred_framework_stream(response, local_stream_executor).await)
        })
    }
}

/// Compose a public URL for an application-local path in the current request.
/// Outside a framework request, the prefix defaults to the origin root.
/// Prefer `cx.request().url(path)` inside components, which also works in
/// suspended boundaries after the HTTP handler has completed.
pub fn app_url(path: &str) -> proa_core::AppUrl<'_> {
    ROUTE_CONTEXT
        .try_with(|ctx| ctx.borrow().base_path().url(path))
        .unwrap_or_else(|_| proa_core::BasePath::default().url(path))
}
