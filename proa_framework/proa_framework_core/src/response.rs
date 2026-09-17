// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use async_stream::try_stream;
use bytes::Bytes;
use futures::channel::mpsc;
use futures::future::{select, BoxFuture, Either, FutureExt, LocalBoxFuture};
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use proa_core::ctx::Ctx;
use proa_core::web::WebContext;
use proa_core::{
    drive_dyn_render, drive_dyn_send_render, CspNonce, DataLoader, DynDataLoader, DynLoaderHandle,
    DynWebRender, DynWebRenderSend, DynWebRenderSync, LoadKey, LoadRequest, NoLoader, RenderBuffer,
    RenderMode, RequestContext, RequestLocation, ResponseContext, RouteParams,
    SendStreamChunkFlusher, SendWebContext, StreamChunkFlusher, WriteError,
};
use proa_react::ReactBootstrapConfig;
#[cfg(not(feature = "rt-tokio"))]
use proa_stream::driver::out_of_order::{
    OutOfOrderBoundaryDriver as RuntimeOutOfOrderBoundaryDriver,
    SendOutOfOrderBoundaryDriver as RuntimeSendOutOfOrderBoundaryDriver,
};
#[cfg(feature = "rt-tokio")]
use proa_stream::driver::{RuntimeOutOfOrderBoundaryDriver, RuntimeSendOutOfOrderBoundaryDriver};
use proa_stream::{
    driver::out_of_order::{
        append_boundary_batch_script_with_nonce, append_boundary_buffer_template,
        append_boundary_template, append_boundary_timeout_chunk, BoundaryEvent,
    },
    write_inline_runtime_tag, DataResolver, PreparedLoader, SendStreamContext, StreamContext,
};
use thiserror::Error;

use crate::bootstrap::{
    render_bootstrap_for_islands, render_bootstrap_for_islands_with_import_map,
};
use crate::manifest::IslandManifest;

/// A response stream that may retain request-local, non-`Send` render state.
///
/// This local type preserves the core `WebRender` contract; it does not mean
/// that every render stream must be `!Send`. A local executor can poll this
/// stream directly. An adapter whose body API requires `Send` must either use
/// a distinct send-capable stream path when the complete render graph supports
/// it or bridge this local stream onto an appropriate local executor.
///
/// The currently shipped factory below erases that capability distinction.
/// Treat that as an adapter limitation, not a reason to impose `Send` on all
/// component futures. The canonical decision is recorded in this crate's
/// README under “`Send` policy and runtime portability.”
pub type FrameworkLocalStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, FrameworkError>> + 'static>>;

/// A response stream whose complete active future graph is compiler-proved
/// safe to migrate between executor threads.
pub type FrameworkSendStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, FrameworkError>> + Send + 'static>>;

/// Transferable unopened factory for a pinned local response stream.
pub type FrameworkLocalStreamFactory = Box<dyn FnOnce() -> FrameworkLocalStream + Send + 'static>;

/// Transferable unopened factory for a `Send` response stream.
pub type FrameworkSendStreamFactory = Box<dyn FnOnce() -> FrameworkSendStream + Send + 'static>;

/// Execution capability nested inside the streamed HTTP body shape.
///
/// Adapters poll `Send` streams directly and place `Local` streams on a
/// pinned local executor. The distinction is carried by Rust's type system;
/// no runtime flag can upgrade a local stream into the direct path.
pub enum FrameworkStreamFactory {
    Send(FrameworkSendStreamFactory),
    Local(FrameworkLocalStreamFactory),
}

impl std::fmt::Debug for FrameworkStreamFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Send(_) => f.write_str("Send(<stream-factory>)"),
            Self::Local(_) => f.write_str("Local(<stream-factory>)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsyncDelivery {
    BufferedStatic,
    BufferedAgent,
    StreamInOrder,
    StreamOutOfOrder,
}

impl AsyncDelivery {
    const fn from_render_mode(render_mode: RenderMode) -> Self {
        match render_mode {
            RenderMode::ChunkedInOrder => Self::StreamInOrder,
            RenderMode::ChunkedOutOfOrder => Self::StreamOutOfOrder,
            RenderMode::Static => Self::BufferedStatic,
            RenderMode::Agent => Self::BufferedAgent,
        }
    }

    const fn render_mode(self) -> RenderMode {
        match self {
            Self::BufferedStatic => RenderMode::Static,
            Self::BufferedAgent => RenderMode::Agent,
            Self::StreamInOrder => RenderMode::ChunkedInOrder,
            Self::StreamOutOfOrder => RenderMode::ChunkedOutOfOrder,
        }
    }

    const fn is_streamed(self) -> bool {
        matches!(self, Self::StreamInOrder | Self::StreamOutOfOrder)
    }
}

enum RoutePlan<L: DataLoader> {
    Sync(Box<dyn DynWebRenderSync<NoLoader, RenderBuffer> + Send>),
    Async {
        component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
        loader: Arc<L>,
        delivery: AsyncDelivery,
        preload_keys: Vec<LoadKey>,
    },
    SendAsync {
        component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
        loader: Arc<L>,
        delivery: AsyncDelivery,
        preload_keys: Vec<LoadKey>,
    },
}

/// Portable, opaque route render plan.
///
/// Generic over `L: DataLoader` so the loader's concrete type is preserved
/// end-to-end through the route pipeline. `cx.load(...)` calls inside the
/// component become direct, monomorphized calls on `L` rather than going
/// through a `Box::pin` at the dyn boundary.
///
/// Defaults to `NoLoader` so handlers that don't use `cx.load(...)` keep
/// writing `-> RouteResponse` unchanged. Handlers that do install a loader
/// should declare their return type as `-> impl IntoResponse`, which lets
/// axum infer the concrete `L` without the caller having to spell it out.
/// Ready-made HTTP bodies and browser-only applications deliberately live
/// outside this type: a `RouteResponse` always means "render this component."
pub struct RouteResponse<L: DataLoader = NoLoader> {
    plan: RoutePlan<L>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
}

impl RouteResponse<DynLoaderHandle> {
    /// Construct a buffered async response with a type-erased loader.
    pub fn ssr_async_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRender<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        let wrapped = build_dyn_request_loader_handle(Some(loader));
        Self::ssr_async_with_loader(component, wrapped)
    }

    /// Construct a streamed async response with a type-erased loader.
    pub fn ssr_stream_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRender<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        let wrapped = build_dyn_request_loader_handle(Some(loader));
        Self::ssr_stream_with_loader(component, wrapped)
    }

    /// Construct a buffered compiler-proved `Send` response with a type-erased loader.
    pub fn ssr_async_send_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRenderSend<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        let wrapped = build_dyn_request_loader_handle(Some(loader));
        Self::ssr_async_send_with_loader(component, wrapped)
    }

    /// Construct a directly pollable streamed response with a type-erased loader.
    pub fn ssr_stream_send_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRenderSend<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        let wrapped = build_dyn_request_loader_handle(Some(loader));
        Self::ssr_stream_send_with_loader(component, wrapped)
    }

    /// Replace the loader on an existing type-erased response.
    ///
    /// The response must already use `L = DynLoaderHandle`; this method does
    /// not change its loader type. Every `cx.load(...)` inside the component
    /// pays one `Box::pin` at the dyn boundary.
    ///
    /// Use [`with_loader`](Self::with_loader) on a typed `L` for the
    /// monomorphized fast path; use this only when the loader's concrete
    /// type is genuinely not statically known at the route's construction
    /// site.
    ///
    /// The supplied loader is wrapped with a request-scoped
    /// [`DataResolver`] for per-request dedupe.
    pub fn with_loader_dyn(self, loader: Arc<dyn DynDataLoader>) -> Self {
        let wrapped = build_dyn_request_loader_handle(Some(loader));
        self.with_loader(wrapped)
    }

    /// Install in-order preload keys for this route's loader.
    ///
    /// Only meaningful on the dyn path: the keys are warmed up by a
    /// `PreparedLoader<dyn DynDataLoader>` wrapper that sits between the
    /// request `DataResolver` and the application loader. On the typed-L
    /// path this method is intentionally not exposed; callers should pass a
    /// pre-wrapped `PreparedLoader<MyLoader>` to `with_loader(...)` instead.
    pub fn with_preload_keys<I>(self, preload_keys: I) -> Self
    where
        I: IntoIterator<Item = LoadKey>,
    {
        self.with_preload_requests(preload_keys.into_iter().map(LoadRequest::from))
    }

    /// Install full in-order preload requests for this route's loader.
    ///
    /// Prefer this over [`with_preload_keys`](Self::with_preload_keys) when
    /// the preload needs advisory hints. The prepared load forwards the exact
    /// cloned [`LoadRequest`] to the inner loader instead of recreating a
    /// key-only request.
    pub fn with_preload_requests<I>(mut self, preload_requests: I) -> Self
    where
        I: IntoIterator<Item = LoadRequest>,
    {
        let requests: Vec<LoadRequest> = preload_requests.into_iter().collect();
        if requests.is_empty() {
            return self;
        }
        let (loader, delivery, preload_keys) = match &mut self.plan {
            RoutePlan::Async {
                loader,
                delivery,
                preload_keys,
                ..
            }
            | RoutePlan::SendAsync {
                loader,
                delivery,
                preload_keys,
                ..
            } => (loader, delivery, preload_keys),
            RoutePlan::Sync(_) => return self,
        };
        if !delivery.is_streamed() {
            return self;
        }
        let keys: Vec<LoadKey> = requests
            .iter()
            .map(|request| request.key().clone())
            .collect();
        preload_keys.extend(keys.iter().cloned());
        // Rewrap the stored loader with `PreparedLoader<dyn>` so the in-order
        // driver actually warms up the keys without us having to special-case
        // the typed-L path.
        let inner_dyn: Arc<dyn DynDataLoader> = Arc::clone(loader.inner());
        let prepared: Arc<dyn DynDataLoader> =
            Arc::new(PreparedLoader::new_requests(inner_dyn, requests));
        *loader = Arc::new(DynLoaderHandle::new(prepared));
        self
    }
}

impl RouteResponse<NoLoader> {
    pub fn ssr<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSync<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::Sync(Box::new(component)),
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a buffered async-rendered route response with `L = NoLoader`.
    ///
    /// Rendering completes before the response is emitted, so root errors and
    /// response metadata are known before body delivery begins. Use this when
    /// the component never calls `cx.load(...)`. To install a
    /// typed loader, use [`ssr_async_with_loader`](RouteResponse::<L>::ssr_async_with_loader)
    /// or the type-changing builder [`with_loader`](RouteResponse::<L>::with_loader).
    pub fn ssr_async<T>(component: T) -> Self
    where
        T: proa_core::WebRender<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::Async {
                component: Box::new(component),
                loader: Arc::new(NoLoader),
                delivery: AsyncDelivery::BufferedStatic,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a streamed async route response with `L = NoLoader`.
    pub fn ssr_stream<T>(component: T) -> Self
    where
        T: proa_core::WebRender<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::Async {
                component: Box::new(component),
                loader: Arc::new(NoLoader),
                delivery: AsyncDelivery::StreamOutOfOrder,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a buffered route whose active render future is `Send`.
    pub fn ssr_async_send<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSend<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::SendAsync {
                component: Box::new(component),
                loader: Arc::new(NoLoader),
                delivery: AsyncDelivery::BufferedStatic,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a streamed route that Axum can poll directly.
    pub fn ssr_stream_send<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSend<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::SendAsync {
                component: Box::new(component),
                loader: Arc::new(NoLoader),
                delivery: AsyncDelivery::StreamOutOfOrder,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }
}

impl<L: DataLoader + 'static> RouteResponse<L> {
    /// Construct a buffered async-rendered route response with a typed loader.
    ///
    /// `L` is inferred from `loader`'s type. The component must implement
    /// `WebRender<L, RenderBuffer>`; `cx.load(...)` calls inside it become
    /// direct, monomorphized calls on `L`.
    pub fn ssr_async_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRender<L, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::Async {
                component: Box::new(component),
                loader,
                delivery: AsyncDelivery::BufferedStatic,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a typed-loader streamed async route response.
    pub fn ssr_stream_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRender<L, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::Async {
                component: Box::new(component),
                loader,
                delivery: AsyncDelivery::StreamOutOfOrder,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a buffered send-capable route with a typed loader.
    pub fn ssr_async_send_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRenderSend<L, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::SendAsync {
                component: Box::new(component),
                loader,
                delivery: AsyncDelivery::BufferedStatic,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    /// Construct a directly pollable stream with a typed loader.
    pub fn ssr_stream_send_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRenderSend<L, RenderBuffer> + Send + 'static,
    {
        Self {
            plan: RoutePlan::SendAsync {
                component: Box::new(component),
                loader,
                delivery: AsyncDelivery::StreamOutOfOrder,
                preload_keys: Vec::new(),
            },
            islands: Vec::new(),
            bootstrap: ReactBootstrapConfig::default(),
        }
    }

    pub fn with_islands<I, S>(mut self, islands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.islands.extend(islands.into_iter().map(Into::into));
        self
    }

    pub fn with_bootstrap(mut self, config: ReactBootstrapConfig<'static>) -> Self {
        self.bootstrap = config;
        self
    }

    /// Install a typed loader on the response.
    ///
    /// `L` is the loader's concrete type, which lets `cx.load(...)` calls
    /// monomorphize to a direct call on the loader rather than going through
    /// a `Box::pin` at the dyn boundary.
    pub fn with_loader(mut self, loader: Arc<L>) -> Self {
        match &mut self.plan {
            RoutePlan::Async { loader: target, .. }
            | RoutePlan::SendAsync { loader: target, .. } => *target = loader,
            RoutePlan::Sync(_) => {}
        }
        self
    }

    /// Select the route's render and delivery mode.
    ///
    /// Static and agent modes produce a buffered response. Chunked modes
    /// produce a streamed response. Mapping both concerns together prevents
    /// impossible states such as a "stream" route configured for static
    /// rendering.
    pub fn with_render_mode(mut self, render_mode: RenderMode) -> Self {
        match &mut self.plan {
            RoutePlan::Async { delivery, .. } | RoutePlan::SendAsync { delivery, .. } => {
                *delivery = AsyncDelivery::from_render_mode(render_mode);
            }
            RoutePlan::Sync(_) => {}
        }
        self
    }

    /// Whether finalization can complete without polling an async renderer.
    pub fn is_synchronous(&self) -> bool {
        matches!(self.plan, RoutePlan::Sync(_))
    }

    /// Whether this async render plan produces a streamed document response.
    pub fn is_streaming(&self) -> bool {
        matches!(
            self.plan,
            RoutePlan::Async {
                delivery: AsyncDelivery::StreamInOrder | AsyncDelivery::StreamOutOfOrder,
                ..
            } | RoutePlan::SendAsync {
                delivery: AsyncDelivery::StreamInOrder | AsyncDelivery::StreamOutOfOrder,
                ..
            }
        )
    }

    /// Finalize a route already known to produce a streamed document.
    ///
    /// This operation only packages an unopened stream factory and never
    /// polls render work, so adapters can call it synchronously and perform
    /// their first-item gate at an async service boundary.
    pub fn into_stream_framework_response(
        self,
        manifest: &IslandManifest,
        request: &FrameworkRequestContext,
    ) -> Result<FrameworkResponse, FrameworkError> {
        let Self {
            plan,
            islands,
            bootstrap,
        } = self;
        match plan {
            RoutePlan::Sync(_) => Err(FrameworkError::Invariant(
                "non-async route passed to stream finalization",
            )),
            RoutePlan::Async {
                component,
                loader,
                delivery,
                preload_keys,
            } => {
                if !delivery.is_streamed() {
                    return Err(FrameworkError::Invariant(
                        "buffered route passed to stream finalization",
                    ));
                }
                let mut request = request.clone();
                request.render_mode = delivery.render_mode();
                Ok(render_stream_html_response(
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    &request,
                    loader,
                    preload_keys,
                ))
            }
            RoutePlan::SendAsync {
                component,
                loader,
                delivery,
                preload_keys,
            } => {
                if !delivery.is_streamed() {
                    return Err(FrameworkError::Invariant(
                        "buffered route passed to stream finalization",
                    ));
                }
                let mut request = request.clone();
                request.render_mode = delivery.render_mode();
                Ok(render_send_stream_html_response(
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    &request,
                    loader,
                    preload_keys,
                ))
            }
        }
    }

    pub async fn into_framework_response(
        self,
        manifest: &IslandManifest,
        request: &FrameworkRequestContext,
    ) -> Result<FrameworkResponse, FrameworkError> {
        let Self {
            plan,
            islands,
            bootstrap,
        } = self;
        match plan {
            RoutePlan::Sync(component) => {
                render_sync_html_response(component, islands, bootstrap, manifest, request)
            }
            RoutePlan::Async {
                component,
                loader,
                delivery,
                preload_keys,
            } => {
                let mut request = request.clone();
                request.render_mode = delivery.render_mode();
                if delivery.is_streamed() {
                    Ok(render_stream_html_response(
                        component,
                        islands,
                        bootstrap,
                        manifest,
                        &request,
                        loader,
                        preload_keys,
                    ))
                } else {
                    render_async_html_response(
                        component, islands, bootstrap, manifest, &request, loader,
                    )
                    .await
                }
            }
            RoutePlan::SendAsync {
                component,
                loader,
                delivery,
                preload_keys,
            } => {
                let mut request = request.clone();
                request.render_mode = delivery.render_mode();
                if delivery.is_streamed() {
                    Ok(render_send_stream_html_response(
                        component,
                        islands,
                        bootstrap,
                        manifest,
                        &request,
                        loader,
                        preload_keys,
                    ))
                } else {
                    render_send_html_response(
                        component, islands, bootstrap, manifest, &request, loader,
                    )
                    .await
                }
            }
        }
    }

    pub async fn into_client_payload_framework_response(
        self,
        page_title: Option<String>,
        _manifest: &IslandManifest,
        request: &FrameworkRequestContext,
    ) -> FrameworkResponse {
        let Self { plan, .. } = self;
        let html = match plan {
            RoutePlan::Sync(component) => match render_sync_component_to_html(component, request) {
                Ok(html) => html,
                Err(err) => {
                    return framework_error_response(err);
                }
            },
            RoutePlan::Async {
                component, loader, ..
            } => {
                let mut request = request.clone();
                request.render_mode = RenderMode::Static;
                match render_async_component_to_html(
                    component,
                    &request,
                    loader,
                    RenderMode::Static,
                )
                .await
                {
                    Ok(html) => html,
                    Err(err) => return framework_error_response(err),
                }
            }
            RoutePlan::SendAsync {
                component, loader, ..
            } => {
                let mut request = request.clone();
                request.render_mode = RenderMode::Static;
                match render_send_component_to_html(component, &request, loader, RenderMode::Static)
                    .await
                {
                    Ok(html) => html,
                    Err(err) => return framework_error_response(err),
                }
            }
        };
        encode_client_payload(html, page_title)
    }

    fn into_sync_framework_response(
        self,
        manifest: &IslandManifest,
        request: &FrameworkRequestContext,
        mode: RouteFinalizeMode,
    ) -> Result<FrameworkResponse, FrameworkError> {
        let Self {
            plan,
            islands,
            bootstrap,
        } = self;
        let RoutePlan::Sync(component) = plan else {
            return Err(FrameworkError::Invariant(
                "async route passed to synchronous finalization",
            ));
        };
        match mode {
            RouteFinalizeMode::Document => {
                render_sync_html_response(component, islands, bootstrap, manifest, request)
            }
            RouteFinalizeMode::ClientPayload { page_title } => {
                match render_sync_component_to_html(component, request) {
                    Ok(html) => Ok(encode_client_payload(html, page_title)),
                    Err(err) => Ok(framework_error_response(err)),
                }
            }
        }
    }
}

fn encode_client_payload(html: String, page_title: Option<String>) -> FrameworkResponse {
    let mut payload = serde_json::json!({
        "html": html,
    });

    if let Some(title) = page_title {
        payload["title"] = serde_json::json!(title);
    }

    match serde_json::to_vec(&payload) {
        Ok(bytes) => FrameworkResponse::json(Bytes::from(bytes)),
        Err(_) => {
            FrameworkResponse::plain_text(StatusCode::INTERNAL_SERVER_ERROR, "encoding error")
        }
    }
}

#[derive(Debug, Clone)]
pub enum RouteFinalizeMode {
    Document,
    ClientPayload { page_title: Option<String> },
}

pub async fn finalize_route_response<L: DataLoader + 'static>(
    response: RouteResponse<L>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    mode: RouteFinalizeMode,
) -> Result<FrameworkResponse, FrameworkError> {
    if response.is_synchronous() {
        return finalize_route_response_sync(response, manifest, request, mode);
    }
    match mode {
        RouteFinalizeMode::Document => response.into_framework_response(manifest, request).await,
        RouteFinalizeMode::ClientPayload { page_title } => Ok(response
            .into_client_payload_framework_response(page_title, manifest, request)
            .await),
    }
}

/// Finalize a route known to contain a synchronous render plan.
///
/// HTTP adapters can use this after [`RouteResponse::is_synchronous`] to keep
/// the sync fast path independent of an async executor.
pub fn finalize_route_response_sync<L: DataLoader + 'static>(
    response: RouteResponse<L>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    mode: RouteFinalizeMode,
) -> Result<FrameworkResponse, FrameworkError> {
    response.into_sync_framework_response(manifest, request, mode)
}

/// Finalize a streamed document without polling its local render future.
pub fn finalize_route_response_stream<L: DataLoader + 'static>(
    response: RouteResponse<L>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
) -> Result<FrameworkResponse, FrameworkError> {
    response.into_stream_framework_response(manifest, request)
}

/// Per-request metadata (location, route params, render mode).
///
/// The loader is *not* stored here: it's a property of the
/// [`RouteResponse`] render plan, and gets passed alongside this context wherever the
/// async render path needs both. Keeping the loader off this struct means
/// the struct itself is loader-agnostic, so the framework can build a single
/// request context up front without already knowing the route's `L`.
#[derive(Clone, Debug, Default)]
pub struct FrameworkRequestContext {
    pub request: RequestContext,
    pub request_location: Option<RequestLocation>,
    pub route_params: Option<RouteParams>,
    pub render_mode: RenderMode,
    /// Optional per-request nonce applied to every Proa-emitted script tag.
    pub csp_nonce: Option<CspNonce>,
}

/// The body half of a `FrameworkResponse`.
///
/// `Full` is the buffered fast path: the entire HTML is already a `Bytes`
/// blob and the host integration can hand it to its transport directly.
/// `Stream` is the streaming path; its nested factory preserves whether the
/// produced stream is directly pollable or requires a pinned local executor.
pub enum FrameworkBody {
    Full(Bytes),
    /// A fully rendered body whose owned pages can transfer to the transport
    /// without flattening. This is independent of deferred streaming setup.
    BufferedPages(RenderBuffer),
    Stream(FrameworkStreamFactory),
}

impl std::fmt::Debug for FrameworkBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(bytes) => f
                .debug_tuple("Full")
                .field(&format!("{} bytes", bytes.len()))
                .finish(),
            Self::Stream(factory) => f.debug_tuple("Stream").field(factory).finish(),
            Self::BufferedPages(buffer) => f
                .debug_tuple("BufferedPages")
                .field(&proa_core::WriteBuf::len(buffer))
                .finish(),
        }
    }
}

#[derive(Debug)]
pub struct FrameworkResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: FrameworkBody,
}

impl FrameworkResponse {
    pub fn html(body: impl Into<Bytes>) -> Self {
        let mut headers = HeaderMap::new();
        add_default_security_headers(&mut headers);
        Self {
            status: StatusCode::OK,
            headers,
            body: FrameworkBody::Full(body.into()),
        }
    }

    pub fn stream<F>(make_stream: F) -> Self
    where
        F: FnOnce() -> FrameworkLocalStream + Send + 'static,
    {
        let mut headers = HeaderMap::new();
        add_default_security_headers(&mut headers);
        Self {
            status: StatusCode::OK,
            headers,
            body: FrameworkBody::Stream(FrameworkStreamFactory::Local(Box::new(make_stream))),
        }
    }

    /// Construct a streamed response that an async adapter may poll directly.
    pub fn send_stream<F>(make_stream: F) -> Self
    where
        F: FnOnce() -> FrameworkSendStream + Send + 'static,
    {
        let mut headers = HeaderMap::new();
        add_default_security_headers(&mut headers);
        Self {
            status: StatusCode::OK,
            headers,
            body: FrameworkBody::Stream(FrameworkStreamFactory::Send(Box::new(make_stream))),
        }
    }

    pub fn plain_text(status: StatusCode, body: impl Into<Bytes>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        if !headers.contains_key("x-content-type-options") {
            headers.insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
        }
        Self {
            status,
            headers,
            body: FrameworkBody::Full(body.into()),
        }
    }

    pub fn json(body: impl Into<Bytes>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        if !headers.contains_key("x-content-type-options") {
            headers.insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
        }
        Self {
            status: StatusCode::OK,
            headers,
            body: FrameworkBody::Full(body.into()),
        }
    }
}

pub(crate) fn merge_response_context(response: &mut FrameworkResponse, context: &ResponseContext) {
    if context.is_empty() {
        return;
    }

    if let Some(status) = context.status() {
        response.status = status;
    }

    let headers = context.headers();
    // Replace defaults once per header name, then retain every supplied value.
    // Cookies accumulate alongside those already installed by the framework.
    for name in headers.keys() {
        if name != header::SET_COOKIE {
            response.headers.remove(name);
        }
    }
    for (name, value) in headers.iter() {
        // proa-perf-ok: response finalization transfers borrowed metadata into the owned HTTP map
        response.headers.append(name.clone(), value.clone());
    }
}

#[derive(Debug, Error)]
pub enum FrameworkError {
    #[error("render failed: {0}")]
    Render(#[from] WriteError),
    #[error("rsjs runtime planning failed: {0}")]
    RsjsPlan(#[from] rsjs::PageRuntimePlanError),
    #[error("bootstrap failed: {0}")]
    Bootstrap(#[from] anyhow::Error),
    #[error("rendered HTML was not valid UTF-8: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error("framework invariant violated: {0}")]
    Invariant(&'static str),
}

/// Inject the baseline security headers Proa ships by default on every SSR
/// response. Callers are free to override any of these downstream (this only
/// sets a header if it isn't already present).
///
/// Defaults: `X-Content-Type-Options: nosniff` (no MIME sniffing),
/// `X-Frame-Options: DENY` (clickjacking defense), and `Referrer-Policy:
/// strict-origin-when-cross-origin` (no full-URL leakage to other
/// origins). Apps that must be embeddable can relax or remove
/// `X-Frame-Options` through the framework builder's security headers
/// config (`proa_framework_axum::SecurityHeadersConfig`), which runs after
/// this function at the HTTP layer.
pub fn add_default_security_headers(headers: &mut HeaderMap) {
    if !headers.contains_key("x-content-type-options") {
        headers.insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
    }
    if !headers.contains_key("x-frame-options") {
        headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    }
    if !headers.contains_key("referrer-policy") {
        headers.insert(
            "referrer-policy",
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        );
    }
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
    }
}

pub fn render_sync_html_response(
    component: Box<dyn DynWebRenderSync<NoLoader, RenderBuffer> + Send>,
    mut islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
) -> Result<FrameworkResponse, FrameworkError> {
    let (render_artifacts, shared_seed_table_json, response_context, buffer) = {
        let mut ctx = Ctx::new();
        install_request_context(&mut ctx, request);
        component.render_sync_dyn(&mut ctx)?;
        let state = (
            ctx.render_artifacts_snapshot(),
            ctx.rsjs_shared_state_seed_table_json(),
            ctx.response().clone(),
        );
        (state.0, state.1, state.2, ctx.into_output())
    };

    islands.extend(render_artifacts.react_islands);
    let rsjs_artifacts = render_artifacts.rsjs;
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    let response_tail_start = buffer.len();
    append_rsjs_query_seed_table_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_table_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    render_bootstrap_if_needed(
        &mut buffer,
        manifest,
        islands,
        &bootstrap,
        request.csp_nonce.as_ref(),
    )?;
    move_appended_response_tail_before_body(&mut buffer, response_tail_start);
    validate_html_utf8(&buffer)?;
    let mut response = FrameworkResponse::html(Bytes::from(buffer));
    merge_response_context(&mut response, &response_context);
    Ok(response)
}

pub async fn render_async_html_response<L: DataLoader + 'static>(
    component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
    mut islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
) -> Result<FrameworkResponse, FrameworkError> {
    let (render_artifacts, shared_seed_table_json, response_context, buffer) = {
        let mut ctx =
            WebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
        ctx.set_render_mode(request.render_mode);
        install_request_context(&mut ctx, request);
        drive_dyn_render(component.render_dyn(&mut ctx)).await?;
        let state = (
            ctx.render_artifacts_snapshot(),
            ctx.rsjs_shared_state_seed_table_json(),
            ctx.response().clone(),
        );
        (state.0, state.1, state.2, ctx.into_output())
    };

    islands.extend(render_artifacts.react_islands);
    let rsjs_artifacts = render_artifacts.rsjs;
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    let response_tail_start = buffer.len();
    append_rsjs_query_seed_table_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_table_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    render_bootstrap_if_needed(
        &mut buffer,
        manifest,
        islands,
        &bootstrap,
        request.csp_nonce.as_ref(),
    )?;
    move_appended_response_tail_before_body(&mut buffer, response_tail_start);
    validate_html_utf8(&buffer)?;
    let mut response = FrameworkResponse::html(Bytes::from(buffer));
    merge_response_context(&mut response, &response_context);
    Ok(response)
}

pub async fn render_send_html_response<L: DataLoader + 'static>(
    component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
    mut islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
) -> Result<FrameworkResponse, FrameworkError> {
    let (render_artifacts, response_context, buffer) = {
        let mut ctx =
            SendWebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
        ctx.set_render_mode(request.render_mode);
        install_send_request_context(&mut ctx, request);
        drive_dyn_send_render(component.render_send_dyn(&mut ctx)).await?;
        let (buffer, artifacts, response) = ctx.into_output_render_artifacts_and_response();
        (artifacts, response, buffer)
    };

    islands.extend(render_artifacts.react_islands);
    let rsjs_artifacts = render_artifacts.rsjs;
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    let shared_seed_table_json = rsjs_artifacts.shared_state_seed_table_json();
    let response_tail_start = buffer.len();
    append_rsjs_query_seed_table_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_table_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    render_bootstrap_if_needed(
        &mut buffer,
        manifest,
        islands,
        &bootstrap,
        request.csp_nonce.as_ref(),
    )?;
    move_appended_response_tail_before_body(&mut buffer, response_tail_start);
    validate_html_utf8(&buffer)?;
    let mut response = FrameworkResponse::html(Bytes::from(buffer));
    merge_response_context(&mut response, &response_context);
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
pub fn render_stream_html_response<L>(
    component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
    preload_keys: Vec<LoadKey>,
) -> FrameworkResponse
where
    L: DataLoader + 'static,
{
    let manifest = manifest.clone();
    let request = request.clone();

    FrameworkResponse::stream(move || match request.render_mode {
        RenderMode::ChunkedInOrder => {
            let stream: FrameworkLocalStream = Box::pin(
                InOrderStreamDriver {
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    request,
                    loader,
                    preload_keys,
                }
                .into_stream(),
            );
            stream
        }
        _ => {
            let stream: FrameworkLocalStream = Box::pin(
                OutOfOrderStreamDriver {
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    request,
                    loader,
                }
                .into_stream(),
            );
            stream
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub fn render_send_stream_html_response<L>(
    component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: &IslandManifest,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
    preload_keys: Vec<LoadKey>,
) -> FrameworkResponse
where
    L: DataLoader + 'static,
{
    let manifest = manifest.clone();
    let request = request.clone();

    FrameworkResponse::send_stream(move || match request.render_mode {
        RenderMode::ChunkedInOrder => {
            let stream: FrameworkSendStream = Box::pin(
                SendInOrderStreamDriver {
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    request,
                    loader,
                    preload_keys,
                }
                .into_stream(),
            );
            stream
        }
        _ => {
            let stream: FrameworkSendStream = Box::pin(
                SendOutOfOrderStreamDriver {
                    component,
                    islands,
                    bootstrap,
                    manifest,
                    request,
                    loader,
                }
                .into_stream(),
            );
            stream
        }
    })
}

fn render_sync_component_to_html(
    component: Box<dyn DynWebRenderSync<NoLoader, RenderBuffer> + Send>,
    request: &FrameworkRequestContext,
) -> Result<String, FrameworkError> {
    let (rsjs_artifacts, shared_seed_table_json, buffer) = {
        let mut ctx = Ctx::new();
        install_request_context(&mut ctx, request);
        component.render_sync_dyn(&mut ctx)?;
        let state = (
            ctx.rsjs_artifacts_snapshot(),
            ctx.rsjs_shared_state_seed_table_json(),
        );
        (state.0, state.1, ctx.into_output())
    };
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    append_rsjs_query_seed_fragment_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_fragment_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    Ok(std::str::from_utf8(&buffer)?.to_owned())
}

async fn render_async_component_to_html<L: DataLoader + 'static>(
    component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
    render_mode: RenderMode,
) -> Result<String, FrameworkError> {
    let (rsjs_artifacts, shared_seed_table_json, buffer) = {
        let mut ctx =
            WebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
        ctx.set_render_mode(render_mode);
        install_request_context(&mut ctx, request);
        drive_dyn_render(component.render_dyn(&mut ctx)).await?;
        let state = (
            ctx.rsjs_artifacts_snapshot(),
            ctx.rsjs_shared_state_seed_table_json(),
        );
        (state.0, state.1, ctx.into_output())
    };
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    append_rsjs_query_seed_fragment_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_fragment_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    Ok(std::str::from_utf8(&buffer)?.to_owned())
}

async fn render_send_component_to_html<L: DataLoader + 'static>(
    component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
    request: &FrameworkRequestContext,
    loader: Arc<L>,
    render_mode: RenderMode,
) -> Result<String, FrameworkError> {
    let (rsjs_artifacts, buffer) = {
        let mut ctx =
            SendWebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
        ctx.set_render_mode(render_mode);
        install_send_request_context(&mut ctx, request);
        drive_dyn_send_render(component.render_send_dyn(&mut ctx)).await?;
        let (buffer, artifacts) = ctx.into_output_and_render_artifacts();
        (artifacts.rsjs, buffer)
    };
    let mut buffer = buffer.into_vec();
    let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
    let shared_seed_table_json = rsjs_artifacts.shared_state_seed_table_json();
    append_rsjs_query_seed_fragment_if_needed(
        &mut buffer,
        query_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_shared_state_seed_fragment_if_needed(
        &mut buffer,
        shared_seed_table_json.as_deref(),
        request.csp_nonce.as_ref(),
    )?;
    append_rsjs_runtime_plan(
        &mut buffer,
        rsjs_artifacts.rendered_islands(),
        request.csp_nonce.as_ref(),
    )?;
    Ok(std::str::from_utf8(&buffer)?.to_owned())
}

fn framework_error_response(err: FrameworkError) -> FrameworkResponse {
    match err {
        FrameworkError::Render(_)
        | FrameworkError::RsjsPlan(_)
        | FrameworkError::Bootstrap(_)
        | FrameworkError::Invariant(_) => {
            FrameworkResponse::plain_text(StatusCode::INTERNAL_SERVER_ERROR, "render error")
        }
        FrameworkError::Encoding(_) => {
            FrameworkResponse::plain_text(StatusCode::INTERNAL_SERVER_ERROR, "encoding error")
        }
    }
}

pub struct OutOfOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: IslandManifest,
    request: FrameworkRequestContext,
    loader: Arc<L>,
}

impl<L> OutOfOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    pub fn into_stream(self) -> impl Stream<Item = Result<Bytes, FrameworkError>> + 'static {
        try_stream! {
            let Self {
                component,
                mut islands,
                bootstrap,
                manifest,
                request,
                loader,
            } = self;

            let stream = StreamContext::new(request.render_mode);
            let recorder: Rc<dyn proa_core::StreamBoundaryRecorder> = Rc::new(stream.clone());
            let mut rsjs_emission = rsjs::PageRuntimeEmissionState::new();
            let mut react_emission = ReactBootstrapEmissionState::default();

            let (render_artifacts, shared_seed_table_json, shell) = {
                let mut ctx =
                    WebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
                ctx.set_render_mode(request.render_mode);
                install_request_context(&mut ctx, &request);
                ctx.response_mut().mark_streaming_started();
                ctx.set_stream_recorder(Rc::clone(&recorder));
                drive_dyn_render(component.render_dyn(&mut ctx)).await?;
                let state = (
                    ctx.render_artifacts_snapshot(),
                    ctx.rsjs_shared_state_seed_table_json(),
                );
                (state.0, state.1, ctx.into_output())
            };

            islands.extend(render_artifacts.react_islands);
            let rsjs_artifacts = render_artifacts.rsjs;
            let mut shell = shell.into_vec();
            let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
            let response_tail_start = shell.len();
            append_rsjs_query_seed_table_if_needed(
                &mut shell,
                query_seed_table_json.as_deref(),
                request.csp_nonce.as_ref(),
            )?;
            append_rsjs_shared_state_seed_table_if_needed(
                &mut shell,
                shared_seed_table_json.as_deref(),
                request.csp_nonce.as_ref(),
            )?;
            append_rsjs_runtime_plan_delta(
                &mut shell,
                rsjs_artifacts.rendered_islands(),
                &mut rsjs_emission,
                request.csp_nonce.as_ref(),
            )?;
            render_bootstrap_delta_if_needed(
                &mut shell,
                &manifest,
                islands,
                &bootstrap,
                &mut react_emission,
                request.csp_nonce.as_ref(),
            )?;
            if stream.has_pending_boundaries() {
                write_inline_runtime_tag(&mut shell, request.csp_nonce.as_ref())?;
            }
            move_appended_response_tail_before_body(&mut shell, response_tail_start);
            validate_html_utf8(&shell)?;
            yield Bytes::from(shell);

            let mut boundary_driver = RuntimeOutOfOrderBoundaryDriver::new();
            let mut pending_boundaries = Vec::new();
            loop {
                stream.drain_boundaries_into(&mut pending_boundaries);
                boundary_driver.extend(pending_boundaries.drain(..), Instant::now());
                let events = boundary_driver.next_events().await;
                if events.is_empty() {
                    break;
                }

                let mut chunk = Vec::new();
                let mut swap_ids = Vec::with_capacity(events.len());
                for event in events {
                    match event {
                        BoundaryEvent::Finished {
                            id,
                            rendered: Ok(rendered),
                        } => {
                            let proa_core::RenderedBoundary {
                                html,
                                artifacts,
                                nested,
                            } = rendered;
                            let query_seed_table_json = artifacts.rsjs.query_seed_table_json();
                            append_rsjs_query_seed_fragment_if_needed(
                                &mut chunk,
                                query_seed_table_json.as_deref(),
                                request.csp_nonce.as_ref(),
                            )?;
                            let shared_seed_table_json =
                                artifacts.rsjs.shared_state_seed_table_json();
                            append_rsjs_shared_state_seed_fragment_if_needed(
                                &mut chunk,
                                shared_seed_table_json.as_deref(),
                                request.csp_nonce.as_ref(),
                            )?;
                            append_rsjs_runtime_plan_delta(
                                &mut chunk,
                                artifacts.rsjs.rendered_islands(),
                                &mut rsjs_emission,
                                request.csp_nonce.as_ref(),
                            )?;
                            render_bootstrap_delta_if_needed(
                                &mut chunk,
                                &manifest,
                                artifacts.react_islands,
                                &bootstrap,
                                &mut react_emission,
                                request.csp_nonce.as_ref(),
                            )?;
                            // Metadata and every template precede the one
                            // application script for this readiness epoch.
                            append_boundary_buffer_template(&mut chunk, id, &html);
                            swap_ids.push(id);
                            pending_boundaries.extend(nested);
                        }
                        BoundaryEvent::Finished {
                            id,
                            rendered: Err(err),
                        } => {
                            tracing::error!(
                                boundary_id = id,
                                ?err,
                                "uncaught streaming boundary failed; blanking boundary"
                            );
                            append_boundary_template(&mut chunk, id, &[]);
                            swap_ids.push(id);
                        }
                        BoundaryEvent::TimedOut { id } => {
                            tracing::error!(
                                boundary_id = id,
                                "streaming boundary timed out; cancelling and preserving fallback"
                            );
                            append_boundary_timeout_chunk(&mut chunk, id);
                        }
                    }
                }
                append_boundary_batch_script_with_nonce(
                    &mut chunk,
                    &swap_ids,
                    request.csp_nonce.as_ref(),
                );
                yield Bytes::from(chunk);
            }
        }
    }
}

pub struct SendOutOfOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: IslandManifest,
    request: FrameworkRequestContext,
    loader: Arc<L>,
}

impl<L> SendOutOfOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    pub fn into_stream(self) -> impl Stream<Item = Result<Bytes, FrameworkError>> + Send + 'static {
        try_stream! {
            let Self {
                component,
                mut islands,
                bootstrap,
                manifest,
                request,
                loader,
            } = self;

            let stream = SendStreamContext::new(request.render_mode);
            let recorder: Arc<dyn proa_core::SendStreamBoundaryRecorder> = Arc::new(stream.clone());
            let mut rsjs_emission = rsjs::PageRuntimeEmissionState::new();
            let mut react_emission = ReactBootstrapEmissionState::default();

            let (render_artifacts, shell) = {
                let mut ctx = SendWebContext::<L, RenderBuffer>::with_loader_buffer(
                    RenderBuffer::new(),
                    loader,
                );
                ctx.set_render_mode(request.render_mode);
                install_send_request_context(&mut ctx, &request);
                ctx.response_mut().mark_streaming_started();
                ctx.set_stream_recorder(Arc::clone(&recorder));
                drive_dyn_send_render(component.render_send_dyn(&mut ctx)).await?;
                let (output, artifacts) = ctx.into_output_and_render_artifacts();
                (artifacts, output)
            };

            islands.extend(render_artifacts.react_islands);
            let rsjs_artifacts = render_artifacts.rsjs;
            let mut shell = shell.into_vec();
            let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
            let shared_seed_table_json = rsjs_artifacts.shared_state_seed_table_json();
            let response_tail_start = shell.len();
            append_rsjs_query_seed_table_if_needed(
                &mut shell,
                query_seed_table_json.as_deref(),
                request.csp_nonce.as_ref(),
            )?;
            append_rsjs_shared_state_seed_table_if_needed(
                &mut shell,
                shared_seed_table_json.as_deref(),
                request.csp_nonce.as_ref(),
            )?;
            append_rsjs_runtime_plan_delta(
                &mut shell,
                rsjs_artifacts.rendered_islands(),
                &mut rsjs_emission,
                request.csp_nonce.as_ref(),
            )?;
            render_bootstrap_delta_if_needed(
                &mut shell,
                &manifest,
                islands,
                &bootstrap,
                &mut react_emission,
                request.csp_nonce.as_ref(),
            )?;
            if stream.has_pending_boundaries() {
                write_inline_runtime_tag(&mut shell, request.csp_nonce.as_ref())?;
            }
            move_appended_response_tail_before_body(&mut shell, response_tail_start);
            validate_html_utf8(&shell)?;
            yield Bytes::from(shell);

            let mut boundary_driver = RuntimeSendOutOfOrderBoundaryDriver::new();
            let mut pending_boundaries = Vec::new();
            loop {
                stream.drain_boundaries_into(&mut pending_boundaries);
                boundary_driver.extend(pending_boundaries.drain(..), Instant::now());
                let events = boundary_driver.next_events().await;
                if events.is_empty() {
                    break;
                }

                let mut chunk = Vec::new();
                let mut swap_ids = Vec::with_capacity(events.len());
                for event in events {
                    match event {
                        BoundaryEvent::Finished {
                            id,
                            rendered: Ok(rendered),
                        } => {
                            let proa_core::RenderedBoundary {
                                html,
                                artifacts,
                                nested,
                            } = rendered;
                            let query_seed_table_json = artifacts.rsjs.query_seed_table_json();
                            append_rsjs_query_seed_fragment_if_needed(
                                &mut chunk,
                                query_seed_table_json.as_deref(),
                                request.csp_nonce.as_ref(),
                            )?;
                            let shared_seed_table_json =
                                artifacts.rsjs.shared_state_seed_table_json();
                            append_rsjs_shared_state_seed_fragment_if_needed(
                                &mut chunk,
                                shared_seed_table_json.as_deref(),
                                request.csp_nonce.as_ref(),
                            )?;
                            append_rsjs_runtime_plan_delta(
                                &mut chunk,
                                artifacts.rsjs.rendered_islands(),
                                &mut rsjs_emission,
                                request.csp_nonce.as_ref(),
                            )?;
                            render_bootstrap_delta_if_needed(
                                &mut chunk,
                                &manifest,
                                artifacts.react_islands,
                                &bootstrap,
                                &mut react_emission,
                                request.csp_nonce.as_ref(),
                            )?;
                            append_boundary_buffer_template(&mut chunk, id, &html);
                            swap_ids.push(id);
                            pending_boundaries.extend(nested);
                        }
                        BoundaryEvent::Finished {
                            id,
                            rendered: Err(err),
                        } => {
                            tracing::error!(
                                boundary_id = id,
                                ?err,
                                "uncaught send streaming boundary failed; blanking boundary"
                            );
                            append_boundary_template(&mut chunk, id, &[]);
                            swap_ids.push(id);
                        }
                        BoundaryEvent::TimedOut { id } => {
                            tracing::error!(
                                boundary_id = id,
                                "send streaming boundary timed out; preserving fallback"
                            );
                            append_boundary_timeout_chunk(&mut chunk, id);
                        }
                    }
                }
                append_boundary_batch_script_with_nonce(
                    &mut chunk,
                    &swap_ids,
                    request.csp_nonce.as_ref(),
                );
                yield Bytes::from(chunk);
            }
        }
    }
}

pub struct InOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    component: Box<dyn DynWebRender<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: IslandManifest,
    request: FrameworkRequestContext,
    loader: Arc<L>,
    preload_keys: Vec<LoadKey>,
}

impl<L> InOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    pub fn into_stream(self) -> impl Stream<Item = Result<Bytes, FrameworkError>> + 'static {
        try_stream! {
            let Self {
                component,
                mut islands,
                bootstrap,
                manifest,
                request,
                loader,
                preload_keys,
            } = self;

            // Preload keys are wired in by `with_loader_dyn`/`with_preload_keys`
            // on the dyn path (it rewraps the loader with `PreparedLoader<dyn>`
            // ahead of time). On the typed-L path the field is informational
            // only; the loader itself is responsible for in-order warm-up.
            let _ = preload_keys;
            // Backpressure note: this channel is intentionally unbounded. The
            // producer (`render_fut`) and the consumer (the `select` loop below)
            // are sub-futures of the SAME task: `ChannelSink::flush_chunk`
            // is a sync `StreamChunkFlusher` method invoked from deep
            // inside render code, so it cannot `.await` channel capacity,
            // and blocking the thread would stall the very loop that
            // drains the receiver (deadlock). The queue is bounded in
            // practice by the bytes one render flushes while the outer
            // stream is not being polled, i.e. at most one response body,
            // which matches the buffered (non-streaming) path's memory
            // profile. Cross-task backpressure against slow clients is
            // applied one level up by the bounded axum bridge channel in
            // the bounded local executor in `proa_framework_axum`.
            let (tx, mut rx) = mpsc::unbounded::<Result<Bytes, FrameworkError>>();

            let mut render_fut: LocalBoxFuture<'static, Result<(), FrameworkError>> = async move {
                let sink: Rc<ChannelSink<FRAMEWORK_SINK_PAGE_SIZE>> =
                    Rc::new(ChannelSink::new(tx.clone()));
                let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();

                let (render_artifacts, shared_seed_table_json, shell) = {
                    let mut ctx =
                        WebContext::<L, RenderBuffer>::with_loader_buffer(RenderBuffer::new(), loader);
                    ctx.set_render_mode(request.render_mode);
                    install_request_context(&mut ctx, &request);
                    ctx.response_mut().mark_streaming_started();
                    ctx.set_stream_chunk_flusher(flusher);
                    drive_dyn_render(component.render_dyn(&mut ctx)).await?;
                    let state = (
                        ctx.render_artifacts_snapshot(),
                        ctx.rsjs_shared_state_seed_table_json(),
                    );
                    (state.0, state.1, ctx.into_output())
                };

                islands.extend(render_artifacts.react_islands);
                let rsjs_artifacts = render_artifacts.rsjs;
                let mut shell = shell.into_vec();
                let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
                let response_tail_start = shell.len();
                append_rsjs_query_seed_table_if_needed(
                    &mut shell,
                    query_seed_table_json.as_deref(),
                    request.csp_nonce.as_ref(),
                )?;
                append_rsjs_shared_state_seed_table_if_needed(
                    &mut shell,
                    shared_seed_table_json.as_deref(),
                    request.csp_nonce.as_ref(),
                )?;
                append_rsjs_runtime_plan(
                    &mut shell,
                    rsjs_artifacts.rendered_islands(),
                    request.csp_nonce.as_ref(),
                )?;
                render_bootstrap_if_needed(
                    &mut shell,
                    &manifest,
                    islands,
                    &bootstrap,
                    request.csp_nonce.as_ref(),
                )?;
                move_appended_response_tail_before_body(&mut shell, response_tail_start);
                if !shell.is_empty() {
                    validate_html_utf8(&shell)?;
                    tx.unbounded_send(Ok(Bytes::from(shell)))
                        .map_err(|_| FrameworkError::Render(WriteError::StreamClosed))?;
                }
                Ok(())
            }
            .boxed_local();

            loop {
                let next_chunk = rx.next();
                futures::pin_mut!(next_chunk);

                match select(render_fut, next_chunk).await {
                    Either::Left((render_result, _)) => {
                        render_result?;
                        while let Some(item) = rx.next().await {
                            yield item?;
                        }
                        break;
                    }
                    Either::Right((maybe_item, next_render_fut)) => {
                        render_fut = next_render_fut;
                        match maybe_item {
                            Some(item) => yield item?,
                            None => {
                                render_fut.await?;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct SendInOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    component: Box<dyn DynWebRenderSend<L, RenderBuffer> + Send>,
    islands: Vec<String>,
    bootstrap: ReactBootstrapConfig<'static>,
    manifest: IslandManifest,
    request: FrameworkRequestContext,
    loader: Arc<L>,
    preload_keys: Vec<LoadKey>,
}

impl<L> SendInOrderStreamDriver<L>
where
    L: DataLoader + 'static,
{
    pub fn into_stream(self) -> impl Stream<Item = Result<Bytes, FrameworkError>> + Send + 'static {
        try_stream! {
            let Self {
                component,
                mut islands,
                bootstrap,
                manifest,
                request,
                loader,
                preload_keys,
            } = self;
            let _ = preload_keys;
            let (tx, mut rx) = mpsc::unbounded::<Result<Bytes, FrameworkError>>();

            let mut render_fut: BoxFuture<'static, Result<(), FrameworkError>> = async move {
                let sink: Arc<ChannelSink<FRAMEWORK_SINK_PAGE_SIZE>> =
                    Arc::new(ChannelSink::new(tx.clone()));
                let flusher: Arc<dyn SendStreamChunkFlusher> = sink;

                let (render_artifacts, shell) = {
                    let mut ctx = SendWebContext::<L, RenderBuffer>::with_loader_buffer(
                        RenderBuffer::new(),
                        loader,
                    );
                    ctx.set_render_mode(request.render_mode);
                    install_send_request_context(&mut ctx, &request);
                    ctx.response_mut().mark_streaming_started();
                    ctx.set_stream_chunk_flusher(flusher);
                    drive_dyn_send_render(component.render_send_dyn(&mut ctx)).await?;
                    let (output, artifacts) = ctx.into_output_and_render_artifacts();
                    (artifacts, output)
                };

                islands.extend(render_artifacts.react_islands);
                let rsjs_artifacts = render_artifacts.rsjs;
                let mut shell = shell.into_vec();
                let query_seed_table_json = rsjs_artifacts.query_seed_table_json();
                let shared_seed_table_json = rsjs_artifacts.shared_state_seed_table_json();
                let response_tail_start = shell.len();
                append_rsjs_query_seed_table_if_needed(
                    &mut shell,
                    query_seed_table_json.as_deref(),
                    request.csp_nonce.as_ref(),
                )?;
                append_rsjs_shared_state_seed_table_if_needed(
                    &mut shell,
                    shared_seed_table_json.as_deref(),
                    request.csp_nonce.as_ref(),
                )?;
                append_rsjs_runtime_plan(
                    &mut shell,
                    rsjs_artifacts.rendered_islands(),
                    request.csp_nonce.as_ref(),
                )?;
                render_bootstrap_if_needed(
                    &mut shell,
                    &manifest,
                    islands,
                    &bootstrap,
                    request.csp_nonce.as_ref(),
                )?;
                move_appended_response_tail_before_body(&mut shell, response_tail_start);
                if !shell.is_empty() {
                    validate_html_utf8(&shell)?;
                    tx.unbounded_send(Ok(Bytes::from(shell)))
                        .map_err(|_| FrameworkError::Render(WriteError::StreamClosed))?;
                }
                Ok(())
            }
            .boxed();

            loop {
                let next_chunk = rx.next();
                futures::pin_mut!(next_chunk);

                match select(render_fut, next_chunk).await {
                    Either::Left((render_result, _)) => {
                        render_result?;
                        while let Some(item) = rx.next().await {
                            yield item?;
                        }
                        break;
                    }
                    Either::Right((maybe_item, next_render_fut)) => {
                        render_fut = next_render_fut;
                        match maybe_item {
                            Some(item) => yield item?,
                            None => {
                                render_fut.await?;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Logical page size retained on the transport type for focused tests.
/// Rendering owns a standard [`RenderBuffer`] and no longer needs a second
/// page pool.
pub(crate) const FRAMEWORK_SINK_PAGE_SIZE: usize = 4096;

/// Streaming transport that ships flushed render-buffer chunks through an
/// mpsc channel.
pub(crate) struct ChannelSink<const N: usize> {
    tx: mpsc::UnboundedSender<Result<Bytes, FrameworkError>>,
    _page_size: std::marker::PhantomData<[u8; N]>,
}

impl<const N: usize> ChannelSink<N> {
    pub(crate) fn new(tx: mpsc::UnboundedSender<Result<Bytes, FrameworkError>>) -> Self {
        Self::with_max_idle(tx, 8)
    }

    pub(crate) fn with_max_idle(
        tx: mpsc::UnboundedSender<Result<Bytes, FrameworkError>>,
        _max_idle: usize,
    ) -> Self {
        Self {
            tx,
            _page_size: std::marker::PhantomData,
        }
    }

    fn send_bytes(&self, bytes: Bytes) -> Result<(), WriteError> {
        self.tx
            .unbounded_send(Ok(bytes))
            .map_err(|_| WriteError::StreamClosed)
    }
}

impl<const N: usize> StreamChunkFlusher for ChannelSink<N> {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.send_bytes(Bytes::from(chunk))
    }
}

impl<const N: usize> SendStreamChunkFlusher for ChannelSink<N> {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.send_bytes(Bytes::from(chunk))
    }
}

pub(crate) fn install_request_context<B: proa_core::WriteBuf, L: DataLoader>(
    ctx: &mut WebContext<L, B>,
    request: &FrameworkRequestContext,
) {
    ctx.set_request(request.request.clone());
    if let Some(request_location) = request.request_location.clone() {
        ctx.set_request_location(request_location);
    }
    if let Some(route_params) = request.route_params.clone() {
        ctx.set_route_params(route_params);
    }
    if let Some(nonce) = request.csp_nonce.clone() {
        ctx.set_csp_nonce(nonce);
    }
}

fn install_send_request_context<B: proa_core::WriteBuf + Send, L: DataLoader>(
    ctx: &mut SendWebContext<L, B>,
    request: &FrameworkRequestContext,
) {
    ctx.set_request(request.request.clone());
    if let Some(request_location) = request.request_location.clone() {
        ctx.set_request_location(request_location);
    }
    if let Some(route_params) = request.route_params.clone() {
        ctx.set_route_params(route_params);
    }
    if let Some(nonce) = request.csp_nonce.clone() {
        ctx.set_csp_nonce(nonce);
    }
}

fn append_rsjs_query_seed_table_if_needed<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    append_rsjs_query_seed_table_with_scope(buffer, seed_table_json, nonce, false)
}

fn append_rsjs_query_seed_fragment_if_needed<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    append_rsjs_query_seed_table_with_scope(buffer, seed_table_json, nonce, true)
}

fn append_rsjs_query_seed_table_with_scope<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
    fragment: bool,
) -> Result<(), FrameworkError> {
    let Some(seed_table_json) = seed_table_json else {
        return Ok(());
    };

    proa_core::shared::write_buf::WriteBuf::extend_static(
        buffer,
        br#"<script type="application/json" id="rsjs-query-seed-table""#,
    )
    .map_err(FrameworkError::Render)?;
    if fragment {
        proa_core::shared::write_buf::WriteBuf::extend_static(
            buffer,
            br#" data-rsjs-seed-scope="fragment""#,
        )
        .map_err(FrameworkError::Render)?;
    }
    if let Some(nonce) = nonce {
        nonce
            .write_html_attr(buffer)
            .map_err(FrameworkError::Render)?;
    }
    proa_core::shared::write_buf::WriteBuf::extend_static(buffer, b">")
        .map_err(FrameworkError::Render)?;
    proa_core::shared::write_buf::WriteBuf::extend(buffer, seed_table_json.as_bytes())
        .map_err(FrameworkError::Render)?;
    proa_core::shared::write_buf::WriteBuf::extend_static(buffer, b"</script>")
        .map_err(FrameworkError::Render)?;
    Ok(())
}

fn append_rsjs_shared_state_seed_table_if_needed<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    append_rsjs_shared_state_seed_table_with_scope(buffer, seed_table_json, nonce, false)
}

fn append_rsjs_shared_state_seed_fragment_if_needed<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    append_rsjs_shared_state_seed_table_with_scope(buffer, seed_table_json, nonce, true)
}

fn append_rsjs_shared_state_seed_table_with_scope<B: proa_core::WriteBuf>(
    buffer: &mut B,
    seed_table_json: Option<&str>,
    nonce: Option<&CspNonce>,
    fragment: bool,
) -> Result<(), FrameworkError> {
    let Some(seed_table_json) = seed_table_json else {
        return Ok(());
    };

    proa_core::shared::write_buf::WriteBuf::extend_static(
        buffer,
        br#"<script type="application/json" id="rsjs-shared-state-seed-table""#,
    )
    .map_err(FrameworkError::Render)?;
    if fragment {
        proa_core::shared::write_buf::WriteBuf::extend_static(
            buffer,
            br#" data-rsjs-seed-scope="fragment""#,
        )
        .map_err(FrameworkError::Render)?;
    }
    if let Some(nonce) = nonce {
        nonce
            .write_html_attr(buffer)
            .map_err(FrameworkError::Render)?;
    }
    proa_core::shared::write_buf::WriteBuf::extend_static(buffer, b">")
        .map_err(FrameworkError::Render)?;
    proa_core::shared::write_buf::WriteBuf::extend(buffer, seed_table_json.as_bytes())
        .map_err(FrameworkError::Render)?;
    proa_core::shared::write_buf::WriteBuf::extend_static(buffer, b"</script>")
        .map_err(FrameworkError::Render)?;
    Ok(())
}

#[doc(hidden)]
pub fn append_rsjs_runtime_plan<I, S, B>(
    buffer: &mut B,
    rendered_islands: I,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    B: proa_core::WriteBuf,
{
    let mut emission = rsjs::PageRuntimeEmissionState::new();
    append_rsjs_runtime_plan_delta(buffer, rendered_islands, &mut emission, nonce)
}

#[doc(hidden)]
pub fn append_rsjs_runtime_plan_delta<I, S, B>(
    buffer: &mut B,
    rendered_islands: I,
    emission: &mut rsjs::PageRuntimeEmissionState,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    B: proa_core::WriteBuf,
{
    let manifest = rsjs::manifest();
    let plan = manifest.page_runtime_plan(rendered_islands)?;
    plan.write_html_delta(buffer, emission, nonce)?;
    Ok(())
}

/// Append the React-island bootstrap script to `buffer` when at least one
/// island was rendered during the request.
///
/// Called by every framework render path (sync, async, in-order streaming,
/// out-of-order streaming) after the main component tree has been written.
/// The bootstrap is what wires browser-side React/JS to the rendered
/// island markers; pages with zero islands skip it to avoid emitting an
/// empty `<script>` tag.
///
/// The function appends bytes to whatever `buffer` already contains, so it
/// must run *after* the main render and *before* the buffer is shipped to
/// the response body.
///
/// # Buffer type
///
/// Currently takes `&mut Vec<u8>` because every caller renders the shell
/// into a `Vec<u8>` (root buffer) or a transient `Vec<u8>` used for one
/// boundary's payload. If a caller ever renders the shell into a different
/// `WriteBuf` (e.g., a sink-backed `SinkWriter`), this signature would
/// need to be generalized to `<B: WriteBuf>`. The body is already
/// generic-friendly: it constructs a `Ctx::with_buffer(buffer)`
/// which works for any `WriteBuf` implementor.
///
/// # Returns
///
/// `Ok(())` if no islands needed bootstrap or if the bootstrap was
/// successfully written. `Err(FrameworkError)` if the bootstrap render
/// itself failed (e.g., serialization of the manifest entries).
fn render_bootstrap_if_needed(
    buffer: &mut Vec<u8>,
    manifest: &IslandManifest,
    islands: Vec<String>,
    bootstrap: &ReactBootstrapConfig<'_>,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    if islands.is_empty() {
        return Ok(());
    }

    let mut ctx = Ctx::new();
    if let Some(nonce) = nonce {
        ctx.set_csp_nonce(nonce.clone());
    }
    render_bootstrap_for_islands(
        &mut ctx,
        manifest,
        islands.iter().map(|name| name.as_str()),
        bootstrap,
    )?;
    let bootstrap = ctx.into_output().into_vec();
    proa_core::WriteBuf::extend(buffer, &bootstrap)?;
    Ok(())
}

#[derive(Default)]
struct ReactBootstrapEmissionState {
    islands: BTreeSet<String>,
    runtime_emitted: bool,
}

/// Emit bootstrap metadata only for React island names not already published
/// by this streaming response.
fn render_bootstrap_delta_if_needed(
    buffer: &mut Vec<u8>,
    manifest: &IslandManifest,
    islands: Vec<String>,
    bootstrap: &ReactBootstrapConfig<'_>,
    emission: &mut ReactBootstrapEmissionState,
    nonce: Option<&CspNonce>,
) -> Result<(), FrameworkError> {
    let mut delta = Vec::new();
    for island in islands {
        if emission.islands.insert(island.clone()) {
            delta.push(island);
        }
    }
    if delta.is_empty() {
        return Ok(());
    }

    let mut ctx = Ctx::new();
    if let Some(nonce) = nonce {
        ctx.set_csp_nonce(nonce.clone());
    }
    render_bootstrap_for_islands_with_import_map(
        &mut ctx,
        manifest,
        delta.iter().map(String::as_str),
        bootstrap,
        !emission.runtime_emitted,
    )?;
    emission.runtime_emitted = true;
    let rendered = ctx.into_output().into_vec();
    proa_core::WriteBuf::extend(buffer, &rendered)?;
    Ok(())
}

/// Move a tail appended after the component render to immediately before a
/// complete document's final `</body>`.
///
/// Components are also allowed to render fragments, including trusted raw
/// script or comment content that happens to contain the bytes `</body>`.
/// Requiring the structural terminal suffix `</body></html>` (with optional
/// ASCII whitespace between/after the tags) prevents those fragments from
/// being mistaken for documents. `rotate_right` moves only the short document
/// suffix plus the generated tail and does not copy the rendered body.
fn move_appended_response_tail_before_body(buffer: &mut [u8], tail_start: usize) {
    const BODY_CLOSE: &[u8] = b"</body>";
    const HTML_CLOSE: &[u8] = b"</html>";

    if tail_start >= buffer.len() || tail_start < BODY_CLOSE.len() + HTML_CLOSE.len() {
        return;
    }

    let rendered = &buffer[..tail_start];
    let trim_ascii_whitespace = |mut end: usize| {
        while end > 0 && rendered[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        end
    };
    let rendered_end = trim_ascii_whitespace(rendered.len());
    if !rendered[..rendered_end].ends_with(HTML_CLOSE) {
        return;
    }
    let before_html = trim_ascii_whitespace(rendered_end - HTML_CLOSE.len());
    if !rendered[..before_html].ends_with(BODY_CLOSE) {
        return;
    }
    let body_close_start = before_html - BODY_CLOSE.len();
    let tail_len = buffer.len() - tail_start;
    buffer[body_close_start..].rotate_right(tail_len);
}

/// Defensive check that the bytes about to leave the framework are valid
/// UTF-8. Run on every assembled response body before it is shipped.
///
/// In practice this should never fail: the macros (`html!`, `html_sync!`)
/// HTML-escape user input via `SafeText` / `SafeAttr`, and all literal
/// byte slices written via `WriteBuf::extend_static` come from `&'static
/// str` (which is UTF-8 by construction in Rust). The only realistic way
/// to produce invalid UTF-8 is a custom component that calls
/// `WriteBuf::extend(&[non_utf8_bytes])` directly. This validation
/// catches that bug class at the response boundary instead of letting
/// invalid bytes reach the client.
///
/// Cost: `std::str::from_utf8` is `O(n)` over the response body. For a
/// 16 KB shell this is sub-microsecond and dwarfed by the cost of
/// rendering, so the check is always-on for non-streaming responses.
/// Streaming paths that ship per-page (e.g., a sink-backed shell) can't
/// run this exact check because UTF-8 multi-byte sequences may straddle
/// page boundaries; those paths must rely on macro guarantees instead.
fn validate_html_utf8(bytes: &[u8]) -> Result<(), FrameworkError> {
    std::str::from_utf8(bytes)?;
    Ok(())
}

/// Build the typed `Arc<L>` that the framework installs on the request
/// `WebContext`.
///
/// Wrap a typed loader in a `DataResolver` for request-scoped dedupe via the
/// dyn boundary. Used by the explicit `DynLoaderHandle` opt-in path; the
/// typed-L path skips the wrapper.
pub fn build_dyn_request_loader_handle(
    loader: Option<Arc<dyn DynDataLoader>>,
) -> Arc<DynLoaderHandle> {
    let inner: Arc<dyn DynDataLoader> = match loader {
        Some(upstream) => Arc::new(DataResolver::new(upstream)) as Arc<dyn DynDataLoader>,
        None => Arc::new(NoLoader) as Arc<dyn DynDataLoader>,
    };
    Arc::new(DynLoaderHandle::new(inner))
}

/// Variant of [`build_dyn_request_loader_handle`] that adds an in-order
/// `PreparedLoader` wrapper for known preload keys.
pub fn build_dyn_in_order_loader_handle(
    loader: Option<Arc<dyn DynDataLoader>>,
    preload_keys: Vec<LoadKey>,
) -> Arc<DynLoaderHandle> {
    build_dyn_in_order_loader_handle_requests(
        loader,
        preload_keys.into_iter().map(LoadRequest::from).collect(),
    )
}

/// Variant of [`build_dyn_in_order_loader_handle`] that preserves request
/// hints for prepared loads.
pub fn build_dyn_in_order_loader_handle_requests(
    loader: Option<Arc<dyn DynDataLoader>>,
    preload_requests: Vec<LoadRequest>,
) -> Arc<DynLoaderHandle> {
    let upstream: Arc<dyn DynDataLoader> = match loader {
        Some(upstream) => Arc::new(DataResolver::new(upstream)) as Arc<dyn DynDataLoader>,
        None => Arc::new(NoLoader) as Arc<dyn DynDataLoader>,
    };
    let inner: Arc<dyn DynDataLoader> = if preload_requests.is_empty() {
        upstream
    } else {
        Arc::new(PreparedLoader::new_requests(upstream, preload_requests)) as Arc<dyn DynDataLoader>
    };
    Arc::new(DynLoaderHandle::new(inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HydrationStrategyConfig, IslandEntry, ManifestAssets};
    use proa_core::{RenderBuffer, RenderOutcome, StreamChunkView, WriteBuf};
    use proa_macros::html_sync;
    use std::collections::HashMap;
    use std::future::Future;

    struct MigratingSendPage;

    struct MigratingRequestChild;

    impl proa_core::WebRenderSend for MigratingRequestChild {
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::pending(async move {
                let path = cx
                    .request()
                    .original_pathname()
                    .expect("request path must survive the detached fork")
                    .to_owned();
                cx.out.extend(path.as_bytes())
            })
        }
    }

    impl proa_core::WebRenderSend for MigratingSendPage {
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::pending(async move {
                let path = cx
                    .request()
                    .original_pathname()
                    .expect("request path must be installed")
                    .to_owned();
                cx.out.extend(path.as_bytes())?;
                proa_core::render_send_child(
                    cx,
                    proa_stream::Suspense::new(
                        MigratingRequestChild,
                        proa_core::static_html(b"fallback"),
                    ),
                )
                .await
            })
        }
    }

    #[test]
    fn send_stream_moves_between_threads_without_losing_request_state() {
        let mut installed_request = RequestContext::new();
        installed_request.set_original_pathname("/thread-migrate");
        let request = FrameworkRequestContext {
            request: installed_request,
            render_mode: RenderMode::ChunkedOutOfOrder,
            ..FrameworkRequestContext::default()
        };
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let response = render_send_stream_html_response(
            Box::new(MigratingSendPage),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
            Vec::new(),
        );
        let FrameworkBody::Stream(FrameworkStreamFactory::Send(factory)) = response.body else {
            panic!("expected compiler-proved send stream");
        };

        let (stream, shell) = std::thread::spawn(move || {
            let mut stream = factory();
            let shell = futures::executor::block_on(stream.next())
                .expect("shell item")
                .expect("shell render");
            (stream, shell)
        })
        .join()
        .unwrap();
        let tail = std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let mut stream = stream;
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    bytes.extend_from_slice(&chunk.expect("boundary render"));
                }
                bytes
            })
        })
        .join()
        .unwrap();

        assert!(std::str::from_utf8(&shell)
            .unwrap()
            .starts_with("/thread-migrate"));
        // The detached child writes only the request path, whose base64-v1
        // payload is stable and proves the fork retained request identity.
        assert!(std::str::from_utf8(&tail)
            .unwrap()
            .contains("L3RocmVhZC1taWdyYXRl"));
    }

    #[test]
    fn channel_sink_coalesces_one_logical_render_buffer_flush() {
        let (tx, mut rx) = mpsc::unbounded::<Result<Bytes, FrameworkError>>();
        let sink = ChannelSink::<FRAMEWORK_SINK_PAGE_SIZE>::new(tx);
        let mut buffer = RenderBuffer::new();
        let expected = vec![b'x'; 9_000];
        buffer.extend(&expected).unwrap();

        StreamChunkFlusher::flush_chunk_view(&sink, &buffer).unwrap();
        drop(sink);

        let chunks = futures::executor::block_on(async move {
            let mut chunks = Vec::new();
            while let Some(chunk) = rx.next().await {
                chunks.push(chunk.unwrap());
            }
            chunks
        });
        assert_eq!(chunks, [Bytes::from(expected)]);
        assert_eq!(StreamChunkView::total_len(&buffer), 9_000);
    }

    #[test]
    fn response_tail_moves_only_for_a_terminal_document_suffix() {
        let mut document = b"<html><body><main>ok</main></body> \n</html>\n".to_vec();
        let tail_start = document.len();
        document.extend_from_slice(b"<script>runtime()</script>");

        move_appended_response_tail_before_body(&mut document, tail_start);

        assert_eq!(
            document,
            b"<html><body><main>ok</main><script>runtime()</script></body> \n</html>\n"
        );
    }

    #[test]
    fn response_tail_does_not_treat_raw_body_bytes_in_a_fragment_as_a_document() {
        let mut fragment = b"<script>const marker = '</body>';</script>".to_vec();
        let tail_start = fragment.len();
        fragment.extend_from_slice(b"<script>runtime()</script>");

        move_appended_response_tail_before_body(&mut fragment, tail_start);

        assert_eq!(
            fragment,
            b"<script>const marker = '</body>';</script><script>runtime()</script>"
        );
    }

    #[test]
    fn html_response_sets_default_security_headers() {
        let response = FrameworkResponse::html(Bytes::from_static(b"<main>ok</main>"));

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            response.headers.get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers.get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(response.headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(
            response.headers.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        match response.body {
            FrameworkBody::Full(bytes) => assert_eq!(bytes, Bytes::from_static(b"<main>ok</main>")),
            FrameworkBody::Stream(_) | FrameworkBody::BufferedPages(_) => {
                panic!("expected full body")
            }
        }
    }

    #[test]
    fn html_finalization_preserves_repeated_response_headers() {
        struct HeaderPage;
        impl proa_core::WebRenderSync for HeaderPage {
            fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
                let response = cx.response_mut();
                response.set_status(StatusCode::CREATED);
                for value in ["</one>; rel=preload", "</two>; rel=preload"] {
                    response.append_header(header::LINK, HeaderValue::from_static(value));
                }
                for value in ["Origin", "Accept-Language"] {
                    response.append_header(header::VARY, HeaderValue::from_static(value));
                }
                for value in ["strict-origin", "no-referrer"] {
                    response
                        .append_header(header::REFERRER_POLICY, HeaderValue::from_static(value));
                }
                for value in ["a=1", "b=2"] {
                    response.append_set_cookie(HeaderValue::from_static(value));
                }
                html_sync! { <main /> }.render(cx)
            }
        }

        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let request = FrameworkRequestContext::default();
        for plan in [
            RouteResponse::ssr(HeaderPage),
            RouteResponse::ssr_async(proa_core::to_async(HeaderPage)),
        ] {
            let response =
                futures::executor::block_on(plan.into_framework_response(&manifest, &request))
                    .unwrap();
            assert_eq!(response.status, StatusCode::CREATED);
            assert_eq!(
                response.headers[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            for (name, expected) in [
                (header::LINK, ["</one>; rel=preload", "</two>; rel=preload"]),
                (header::VARY, ["Origin", "Accept-Language"]),
                (header::REFERRER_POLICY, ["strict-origin", "no-referrer"]),
                (header::SET_COOKIE, ["a=1", "b=2"]),
            ] {
                let actual: Vec<_> = response.headers.get_all(&name).iter().collect();
                assert_eq!(actual, expected, "{name}");
            }
        }
    }

    #[test]
    fn default_security_headers_do_not_clobber_existing_values() {
        let mut headers = HeaderMap::new();
        headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
        headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));

        add_default_security_headers(&mut headers);

        assert_eq!(headers.get("x-frame-options").unwrap(), "SAMEORIGIN");
        assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    }

    struct QuerySeedComponent;

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRenderSync<L, B> for QuerySeedComponent {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.rsjs_query_seed_json(
                r#"["stock","AAPL"]"#,
                r#"{"key":["stock","AAPL"],"data":{"value":172.5},"data_updated_at":0}"#,
            );
            proa_core::shared::write_buf::WriteBuf::extend_static(&mut cx.out, b"<main>ok</main>")
        }
    }

    #[test]
    fn render_mode_selects_async_delivery_instead_of_creating_mixed_states() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let request = FrameworkRequestContext::default();

        let streamed = futures::executor::block_on(
            RouteResponse::ssr_async(proa_core::to_async(QuerySeedComponent))
                .with_render_mode(RenderMode::ChunkedInOrder)
                .into_framework_response(&manifest, &request),
        )
        .unwrap();
        assert!(matches!(streamed.body, FrameworkBody::Stream(_)));

        let buffered = futures::executor::block_on(
            RouteResponse::ssr_stream(proa_core::to_async(QuerySeedComponent))
                .with_render_mode(RenderMode::Static)
                .into_framework_response(&manifest, &request),
        )
        .unwrap();
        assert!(matches!(buffered.body, FrameworkBody::Full(_)));
    }

    #[test]
    fn sync_response_appends_rsjs_query_seed_table_before_bootstrap() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let response = render_sync_html_response(
            Box::new(QuerySeedComponent),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &FrameworkRequestContext::default(),
        )
        .unwrap();

        let FrameworkBody::Full(bytes) = response.body else {
            panic!("expected full body");
        };
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(
            html.contains(r#"<script type="application/json" id="rsjs-query-seed-table">{"queries":[{"key":["stock","AAPL"],"state":{"key":["stock","AAPL"],"data":{"value":172.5},"data_updated_at":0},"rank":"{\"key\":[\"stock\",\"AAPL\"],\"data\":{\"value\":172.5},\"data_updated_at\":0}"}]}</script>"#),
            "expected page-level query seed table, got:\n{html}"
        );
    }

    struct SharedSeedComponent {
        acquire: bool,
    }

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRenderSync<L, B> for SharedSeedComponent {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            struct WorkbenchSeed;
            let _scope = cx.rsjs_shared_state_enter(
                "test.framework-workbench",
                "sha256:framework-test",
                "WorkbenchSeed",
                &[],
                std::rc::Rc::new(WorkbenchSeed),
                r#"{"button_bg":"green"}"#.to_string(), // proa-perf-ok: test seed must own its JSON
            )?;
            if self.acquire {
                cx.rsjs_shared_state_acquire::<WorkbenchSeed>(
                    "test.framework-workbench",
                    "sha256:framework-test",
                    "WorkbenchSeed",
                    "FrameworkWorkbenchConsumer",
                )?;
            }
            html_sync! {
                <main>"ok"</main>
            }
            .render(cx)
        }
    }

    #[test]
    fn sync_response_appends_shared_state_seed_table_before_bootstrap() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let response = render_sync_html_response(
            Box::new(SharedSeedComponent { acquire: true }),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &FrameworkRequestContext::default(),
        )
        .unwrap();

        let FrameworkBody::Full(bytes) = response.body else {
            panic!("expected full body");
        };
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(
            html.contains(
                r#"<script type="application/json" id="rsjs-shared-state-seed-table">{"abi_version":1,"stores":[{"scope_id":0,"type_key":"test.framework-workbench","schema_hash":"sha256:framework-test","rust_type":"WorkbenchSeed","state":{"button_bg":"green"}}]}</script>"#
            ),
            "expected page-level shared-state seed table, got:\n{html}"
        );
    }

    #[test]
    fn buffered_response_paths_publish_only_acquired_shared_state() {
        fn body(response: FrameworkResponse) -> String {
            let FrameworkBody::Full(bytes) = response.body else {
                panic!("expected a buffered response");
            };
            String::from_utf8(bytes.to_vec()).unwrap()
        }

        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let request = FrameworkRequestContext::default();

        let sync_unused = render_sync_html_response(
            Box::new(SharedSeedComponent { acquire: false }),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
        )
        .unwrap();
        assert!(!body(sync_unused).contains("rsjs-shared-state-seed-table"));

        let async_used = futures::executor::block_on(render_async_html_response(
            Box::new(proa_core::SyncToAsync(SharedSeedComponent {
                acquire: true,
            })),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
        ))
        .unwrap();
        assert!(body(async_used).contains(r#""type_key":"test.framework-workbench""#));

        let async_unused = futures::executor::block_on(render_async_html_response(
            Box::new(proa_core::SyncToAsync(SharedSeedComponent {
                acquire: false,
            })),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
        ))
        .unwrap();
        assert!(!body(async_unused).contains("rsjs-shared-state-seed-table"));
    }

    #[test]
    fn rsjs_seed_tables_use_the_request_nonce() {
        let nonce = CspNonce::new("ZnJhbWV3b3JrLW5vbmNl").unwrap();
        let mut buffer = Vec::new();

        append_rsjs_query_seed_table_if_needed(
            &mut buffer,
            Some(r#"{"queries":[]}"#),
            Some(&nonce),
        )
        .unwrap();
        append_rsjs_shared_state_seed_table_if_needed(
            &mut buffer,
            Some(r#"{"abi_version":1,"stores":[]}"#),
            Some(&nonce),
        )
        .unwrap();

        let html = String::from_utf8(buffer).unwrap();
        assert!(html.contains(
            r#"<script type="application/json" id="rsjs-query-seed-table" nonce="ZnJhbWV3b3JrLW5vbmNl">"#
        ));
        assert!(html.contains(
            r#"<script type="application/json" id="rsjs-shared-state-seed-table" nonce="ZnJhbWV3b3JrLW5vbmNl">"#
        ));
        assert_eq!(html.matches(r#"nonce="ZnJhbWV3b3JrLW5vbmNl""#).count(), 2);
    }

    #[test]
    fn bootstrap_finalizer_skips_empty_island_set() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let mut buffer = b"<main>server only</main>".to_vec();

        render_bootstrap_if_needed(
            &mut buffer,
            &manifest,
            Vec::new(),
            &ReactBootstrapConfig::default(),
            None,
        )
        .expect("empty island set should not render bootstrap");

        assert_eq!(buffer, b"<main>server only</main>");
    }

    #[test]
    fn bootstrap_finalizer_errors_for_unknown_rendered_island() {
        let mut islands = HashMap::new();
        islands.insert(
            "known".to_string(),
            IslandEntry {
                module_path: "/rsjs/island/known.js".to_string(),
                export: None,
                strategy: HydrationStrategyConfig::Immediate,
                module_preloads: Vec::new(),
                providers: Vec::new(),
                hydrate_fn: None,
            },
        );
        let manifest = IslandManifest {
            islands,
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let mut buffer = b"<main>server only</main>".to_vec();

        let err = render_bootstrap_if_needed(
            &mut buffer,
            &manifest,
            vec!["missing".to_string()],
            &ReactBootstrapConfig::default(),
            None,
        )
        .expect_err("unknown rendered island should fail bootstrap");

        assert!(
            err.to_string().contains("No island named 'missing'"),
            "unexpected bootstrap error: {err}"
        );
    }

    struct YieldOnce(bool);

    impl std::future::Future for YieldOnce {
        type Output = ();

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if self.0 {
                std::task::Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }
    }

    struct DeferredArtifactChild {
        react: &'static str,
        rsjs: &'static str,
    }

    impl proa_core::WebRender<NoLoader, RenderBuffer> for DeferredArtifactChild {
        fn render(
            self,
            cx: &mut WebContext<NoLoader, RenderBuffer>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                YieldOnce(false).await;
                cx.rsjs_query_seed_json(self.rsjs, r#"{"data":"owned"}"#);
                proa_react::Island::new(self.react).render(cx)
            })
        }
    }

    struct DeferredArtifactPage {
        shell_react: &'static str,
        react: &'static str,
        rsjs: &'static str,
    }

    impl proa_core::WebRender<NoLoader, RenderBuffer> for DeferredArtifactPage {
        fn render(
            self,
            cx: &mut WebContext<NoLoader, RenderBuffer>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                YieldOnce(false).await;
                proa_react::Island::new(self.shell_react).render(cx)?;
                proa_core::WebRender::render(
                    proa_stream::Suspense::new(
                        DeferredArtifactChild {
                            react: self.react,
                            rsjs: self.rsjs,
                        },
                        StreamFallback,
                    ),
                    cx,
                )
                .resolve()
                .await
            })
        }
    }

    fn react_test_manifest() -> IslandManifest {
        let mut islands = HashMap::new();
        for name in ["RequestARoot", "RequestA", "RequestBRoot", "RequestB"] {
            islands.insert(
                name.to_owned(),
                IslandEntry {
                    module_path: format!("/react/{name}.js"),
                    export: None,
                    strategy: HydrationStrategyConfig::Immediate,
                    module_preloads: Vec::new(),
                    providers: Vec::new(),
                    hydrate_fn: None,
                },
            );
        }
        IslandManifest {
            islands,
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        }
    }

    #[test]
    fn interleaved_streams_keep_react_ids_and_artifacts_request_owned() {
        let manifest = react_test_manifest();
        let request = FrameworkRequestContext {
            render_mode: RenderMode::ChunkedOutOfOrder,
            ..FrameworkRequestContext::default()
        };
        let make_response = |shell_react, react, rsjs| {
            render_stream_html_response(
                Box::new(DeferredArtifactPage {
                    shell_react,
                    react,
                    rsjs,
                }),
                Vec::new(),
                ReactBootstrapConfig::default(),
                &manifest,
                &request,
                Arc::new(NoLoader),
                Vec::new(),
            )
        };

        let FrameworkBody::Stream(FrameworkStreamFactory::Local(first_factory)) =
            make_response("RequestARoot", "RequestA", r#"["rsjs_request_a"]"#).body
        else {
            panic!("expected first streaming body");
        };
        let FrameworkBody::Stream(FrameworkStreamFactory::Local(second_factory)) =
            make_response("RequestBRoot", "RequestB", r#"["rsjs_request_b"]"#).body
        else {
            panic!("expected second streaming body");
        };

        let (first, second) = futures::executor::block_on(async move {
            let collect = |mut stream: FrameworkLocalStream| async move {
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    bytes.extend_from_slice(&chunk.expect("stream should render"));
                }
                String::from_utf8(bytes).unwrap()
            };
            futures::join!(collect(first_factory()), collect(second_factory()))
        });

        assert!(first.contains(r#"id="RequestARoot-0""#));
        assert!(first.contains("/react/RequestARoot.js"));
        assert!(first.contains("/react/RequestA.js"));
        assert!(first.contains("rsjs_request_a"));
        assert!(!first.contains("RequestB"));
        assert!(!first.contains("rsjs_request_b"));

        assert!(second.contains(r#"id="RequestBRoot-0""#));
        assert!(second.contains("/react/RequestBRoot.js"));
        assert!(second.contains("/react/RequestB.js"));
        assert!(second.contains("rsjs_request_b"));
        assert!(!second.contains("RequestA"));
        assert!(!second.contains("rsjs_request_a"));
    }

    /// Sync fallback rendered while an out-of-order boundary is pending.
    struct StreamFallback;

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRenderSync<L, B> for StreamFallback {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_core::shared::write_buf::WriteBuf::extend_static(&mut cx.out, b"<p>loading</p>")
        }
    }

    /// Deferred boundary child that tries to enter a shared-state provider
    /// after the shell has published the seed table. Records the resulting
    /// error message so the test can assert on the exact diagnostic.
    struct LateProviderBoundaryChild {
        witness: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRender<L, B>
        for LateProviderBoundaryChild
    {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let result = cx.rsjs_shared_state_enter(
                    "test.deferred-panel",
                    "sha256:deferred",
                    "DeferredPanelState",
                    &[],
                    std::rc::Rc::new(2u8),
                    r#"{"deferred":true}"#.to_string(), // proa-perf-ok: test seed must own its JSON
                );
                match result {
                    Ok(_guard) => {
                        self.witness
                            .lock()
                            .unwrap()
                            .replace("late provider unexpectedly succeeded".to_owned());
                        proa_core::shared::write_buf::WriteBuf::extend_static(
                            &mut cx.out,
                            b"<p>late</p>",
                        )
                    }
                    Err(err) => {
                        self.witness.lock().unwrap().replace(err.to_string());
                        Err(err)
                    }
                }
            })
        }
    }

    /// Streamed page: one provider in the shell, one (invalid) provider
    /// below a deferred out-of-order Suspense boundary.
    struct LateProviderStreamPage {
        witness: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl<L: DataLoader + 'static> proa_core::WebRender<L, RenderBuffer> for LateProviderStreamPage {
        fn render(
            self,
            cx: &mut WebContext<L, RenderBuffer>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let _shell_scope = cx.rsjs_shared_state_enter(
                    "test.shell-theme",
                    "sha256:shell",
                    "ShellThemeState",
                    &[],
                    std::rc::Rc::new(1u8),
                    r#"{"shell":true}"#.to_string(), // proa-perf-ok: test seed must own its JSON
                )?;
                cx.rsjs_shared_state_acquire::<u8>(
                    "test.shell-theme",
                    "sha256:shell",
                    "ShellThemeState",
                    "ShellThemeConsumer",
                )?;
                proa_core::shared::write_buf::WriteBuf::extend_static(
                    &mut cx.out,
                    b"<main>shell</main>",
                )?;
                let suspense = proa_stream::Suspense::new(
                    LateProviderBoundaryChild {
                        witness: self.witness,
                    },
                    StreamFallback,
                );
                proa_core::WebRender::render(suspense, cx).resolve().await
            })
        }
    }

    #[test]
    fn stream_response_surfaces_late_boundary_provider_safely() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let witness = std::sync::Arc::new(std::sync::Mutex::new(None));
        let request = FrameworkRequestContext {
            render_mode: RenderMode::ChunkedOutOfOrder,
            ..FrameworkRequestContext::default()
        };

        let response = render_stream_html_response(
            Box::new(LateProviderStreamPage {
                witness: std::sync::Arc::clone(&witness),
            }),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
            Vec::new(),
        );

        let FrameworkBody::Stream(FrameworkStreamFactory::Local(factory)) = response.body else {
            panic!("expected a streaming body");
        };
        let chunks: Vec<Bytes> = futures::executor::block_on(async {
            let mut stream = factory();
            let mut chunks = Vec::new();
            while let Some(chunk) = stream.next().await {
                chunks.push(chunk.expect("the stream must fail softly, not abort"));
            }
            chunks
        });

        // (a) The shell flush publishes the seed table first, carrying only
        // the shell provider's scope.
        let shell = std::str::from_utf8(&chunks[0]).unwrap();
        assert!(
            shell.contains(r#"<script type="application/json" id="rsjs-shared-state-seed-table">"#),
            "shell must carry the seed table, got:\n{shell}"
        );
        assert!(shell.contains(r#""type_key":"test.shell-theme""#));
        assert!(!shell.contains("test.deferred-panel"));

        // (b) The boundary's forked context observed the published flag and
        // failed with the streaming diagnostic.
        let message = witness
            .lock()
            .unwrap()
            .clone()
            .expect("the deferred boundary child must have run");
        assert!(
            message.contains("rsjs/shared-state-late-stream-provider"),
            "unexpected boundary error: {message}"
        );

        // (c) The failure surfaced safely: the stream completed, the failed
        // boundary was blanked, and hydration data was not corrupted (no
        // second seed table, no late scope anywhere in the body).
        assert!(
            chunks.len() >= 2,
            "expected the shell plus a blanked boundary chunk"
        );
        let body: String = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).unwrap().to_owned())
            .collect();
        assert_eq!(body.matches("rsjs-shared-state-seed-table").count(), 1);
        assert!(!body.contains("test.deferred-panel"));
    }

    struct DeferredSharedConsumer;

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRender<L, B> for DeferredSharedConsumer {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                futures::future::ready(()).await;
                let (_, scope_id) = cx.rsjs_shared_state_acquire::<u8>(
                    "test.deferred-consumer",
                    "sha256:deferred-consumer",
                    "DeferredConsumerState",
                    "DeferredSharedConsumer",
                )?;
                assert_eq!(scope_id, 0);
                proa_core::shared::write_buf::WriteBuf::extend_static(
                    &mut cx.out,
                    b"<p data-deferred-shared-consumer>ready</p>",
                )
            })
        }
    }

    struct DeferredSharedPage;

    impl<L: DataLoader + 'static> proa_core::WebRender<L, RenderBuffer> for DeferredSharedPage {
        fn render(
            self,
            cx: &mut WebContext<L, RenderBuffer>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let _scope = cx.rsjs_shared_state_enter(
                    "test.deferred-consumer",
                    "sha256:deferred-consumer",
                    "DeferredConsumerState",
                    &[],
                    std::rc::Rc::new(1u8),
                    r#"{"mode":"current"}"#.to_string(), // proa-perf-ok: test seed must own its JSON
                )?;
                proa_core::WebRender::render(
                    proa_stream::Suspense::new(DeferredSharedConsumer, StreamFallback),
                    cx,
                )
                .resolve()
                .await
            })
        }
    }

    struct DeferredFailingSharedConsumer;

    impl<L: DataLoader, B: proa_core::WriteBuf> proa_core::WebRender<L, B>
        for DeferredFailingSharedConsumer
    {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                cx.rsjs_shared_state_acquire::<u8>(
                    "test.deferred-consumer",
                    "sha256:deferred-consumer",
                    "DeferredConsumerState",
                    "DeferredFailingSharedConsumer",
                )?;
                proa_core::shared::write_buf::WriteBuf::extend_static(
                    &mut cx.out,
                    b"<p>must be discarded</p>",
                )?;
                Err(WriteError::render("deferred consumer failed"))
            })
        }
    }

    struct DeferredFailingSharedPage;

    impl<L: DataLoader + 'static> proa_core::WebRender<L, RenderBuffer> for DeferredFailingSharedPage {
        fn render(
            self,
            cx: &mut WebContext<L, RenderBuffer>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let _scope = cx.rsjs_shared_state_enter(
                    "test.deferred-consumer",
                    "sha256:deferred-consumer",
                    "DeferredConsumerState",
                    &[],
                    std::rc::Rc::new(1u8),
                    r#"{"mode":"must-not-ship"}"#.to_string(), // proa-perf-ok: test seed must own its JSON
                )?;
                proa_core::WebRender::render(
                    proa_stream::Suspense::new(DeferredFailingSharedConsumer, StreamFallback),
                    cx,
                )
                .resolve()
                .await
            })
        }
    }

    #[test]
    fn out_of_order_boundary_publishes_newly_live_provider_seed_before_swap() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let request = FrameworkRequestContext {
            render_mode: RenderMode::ChunkedOutOfOrder,
            ..FrameworkRequestContext::default()
        };
        let response = render_stream_html_response(
            Box::new(DeferredSharedPage),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
            Vec::new(),
        );

        let FrameworkBody::Stream(FrameworkStreamFactory::Local(factory)) = response.body else {
            panic!("expected a streaming body");
        };
        let chunks: Vec<Bytes> = futures::executor::block_on(async {
            let mut stream = factory();
            let mut chunks = Vec::new();
            while let Some(chunk) = stream.next().await {
                chunks.push(chunk.expect("deferred consumer must complete"));
            }
            chunks
        });
        assert!(chunks.len() >= 2, "expected shell plus boundary");

        let shell = std::str::from_utf8(&chunks[0]).unwrap();
        assert!(
            !shell.contains("rsjs-shared-state-seed-table"),
            "an unused shell provider must not publish before its deferred consumer commits: {shell}"
        );

        let boundary = chunks
            .iter()
            .skip(1)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .find(|chunk| chunk.contains("data-rsjs-seed-scope=\"fragment\""))
            .expect("committed deferred consumer must publish a seed fragment");
        assert!(boundary.contains(r#""type_key":"test.deferred-consumer""#));
        assert!(boundary.contains(r#""scope_id":0"#));
        assert!(boundary.contains(r#""mode":"current""#));
        let seed_offset = boundary.find("data-rsjs-seed-scope=\"fragment\"").unwrap();
        let swap_offset = boundary.find("<template id=\"proa-tpl-").unwrap();
        assert!(
            seed_offset < swap_offset,
            "the seed must install before the boundary swap dispatches readiness: {boundary}"
        );
    }

    #[test]
    fn failed_out_of_order_consumer_does_not_publish_provider_seed() {
        let manifest = IslandManifest {
            islands: HashMap::new(),
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };
        let request = FrameworkRequestContext {
            render_mode: RenderMode::ChunkedOutOfOrder,
            ..FrameworkRequestContext::default()
        };
        let response = render_stream_html_response(
            Box::new(DeferredFailingSharedPage),
            Vec::new(),
            ReactBootstrapConfig::default(),
            &manifest,
            &request,
            Arc::new(NoLoader),
            Vec::new(),
        );
        let FrameworkBody::Stream(FrameworkStreamFactory::Local(factory)) = response.body else {
            panic!("expected a streaming body");
        };
        let chunks: Vec<Bytes> = futures::executor::block_on(async {
            let mut stream = factory();
            let mut chunks = Vec::new();
            while let Some(chunk) = stream.next().await {
                chunks.push(chunk.expect("failed boundary must be blanked safely"));
            }
            chunks
        });
        let body: String = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();
        assert!(!body.contains("rsjs-shared-state-seed-table"), "{body}");
        assert!(!body.contains("test.deferred-consumer"), "{body}");
        assert!(!body.contains("must-not-ship"), "{body}");
        assert!(!body.contains("must be discarded"), "{body}");
    }
}
