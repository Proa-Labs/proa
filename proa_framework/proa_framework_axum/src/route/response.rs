// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures::{stream, FutureExt as _, StreamExt as _};
use proa_core::{
    DataLoader, DynDataLoader, DynLoaderHandle, LoadKey, LoadRequest, NoLoader, RenderBuffer,
    RenderMode, RequestContext, RequestLocation,
};
use proa_framework_core::{
    finalize_route_response_deferred, finalize_route_response_stream, finalize_route_response_sync,
    FrameworkBody, FrameworkError, FrameworkRequestContext, FrameworkResponse, FrameworkSendStream,
    FrameworkStreamFactory, RouteFinalization, RouteFinalizeMode,
    RouteResponse as CoreRouteResponse,
};
use tracing::error;

use super::context::{RouteContext, ROUTE_CONTEXT};
use super::local_executor::{LocalStreamExecutor, LocalStreamPrepareError};
pub use proa_framework_core::{
    drive_dyn_render, drive_dyn_send_render, DynRenderOutcome, DynSendRenderOutcome, DynWebRender,
    DynWebRenderSend,
};

/// Axum adapter for the framework-independent route render plan.
///
/// Defaults to `NoLoader` so handlers that don't install a loader keep
/// writing `-> RouteResponse` unchanged. Handlers that install a typed
/// loader through the typed or dyn loader constructors should declare their return type as
/// `-> impl IntoResponse`, which lets axum infer the concrete `L` without
/// the caller having to spell it out.
pub struct RouteResponse<L: DataLoader = NoLoader>(CoreRouteResponse<L>);

impl RouteResponse<NoLoader> {
    pub fn ssr<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSync<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr(component))
    }

    /// Construct a buffered async route response with `L = NoLoader`.
    ///
    /// Rendering completes before Axum emits the response, so root errors and
    /// response metadata are known before body delivery begins. The render
    /// runs on the local render executor after the handler returns, so serve
    /// it through `FrameworkBuilder` or `RouteContextLayer`. Use this when
    /// the component never calls `cx.load_json(...)`. For routes
    /// that install a loader, use
    /// [`ssr_async_with_loader`](RouteResponse::<L>::ssr_async_with_loader)
    /// (which fixes `L` to the supplied loader's concrete type) or the
    /// dyn escape hatch
    /// [`ssr_async_with_loader_dyn`](RouteResponse::<DynLoaderHandle>::ssr_async_with_loader_dyn).
    pub fn ssr_async<T>(component: T) -> Self
    where
        T: proa_core::WebRender<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async(component))
    }

    /// Construct a streamed async route response with `L = NoLoader`.
    ///
    /// Use this when the component never calls `cx.load_json(...)`. For routes
    /// that install a loader, see
    /// [`ssr_stream_with_loader`](RouteResponse::<L>::ssr_stream_with_loader).
    pub fn ssr_stream<T>(component: T) -> Self
    where
        T: proa_core::WebRender<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream(component))
    }

    /// Construct a buffered response with a compiler-proved `Send` render graph.
    pub fn ssr_async_send<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSend<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async_send(component))
    }

    /// Construct a direct Axum stream with no local-executor dispatch.
    pub fn ssr_stream_send<T>(component: T) -> Self
    where
        T: proa_core::WebRenderSend<NoLoader, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream_send(component))
    }
}

impl<L: DataLoader + 'static> RouteResponse<L> {
    /// Construct a typed-loader buffered async route response.
    ///
    /// `L` is inferred from `loader`'s type. The component must implement
    /// `WebRender<L>`; `cx.load_json(...)` calls inside it become
    /// direct, monomorphized calls on `L`.
    pub fn ssr_async_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRender<L, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async_with_loader(component, loader))
    }

    /// Construct a typed-loader streamed async route response.
    pub fn ssr_stream_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRender<L, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream_with_loader(component, loader))
    }

    pub fn ssr_async_send_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRenderSend<L, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async_send_with_loader(
            component, loader,
        ))
    }

    pub fn ssr_stream_send_with_loader<T>(component: T, loader: Arc<L>) -> Self
    where
        T: proa_core::WebRenderSend<L, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream_send_with_loader(
            component, loader,
        ))
    }

    /// Enable the request-local document head lifecycle.
    pub fn with_head(self, config: proa_core::HeadConfig) -> Self {
        Self(self.0.with_head(config))
    }

    /// Override app defaults for a manually authored document.
    pub fn without_head(self) -> Self {
        Self(self.0.without_head())
    }

    /// Install a typed loader on the response.
    ///
    /// `L` is the loader's concrete type. `cx.load_json(...)` calls inside the
    /// component become direct, monomorphized calls on `L` rather than going
    /// through a `Box::pin` at the dyn boundary.
    pub fn with_loader(self, loader: Arc<L>) -> Self {
        Self(self.0.with_loader(loader))
    }

    /// Select both render semantics and response delivery for an async plan.
    ///
    /// Chunked modes stream; static and agent modes buffer.
    pub fn with_render_mode(self, render_mode: RenderMode) -> Self {
        Self(self.0.with_render_mode(render_mode))
    }

    /// Render this response to a complete HTML document without an HTTP
    /// server or an async runtime.
    ///
    /// Forwards to [`proa_framework_core::RouteResponse::render_to_bytes`],
    /// which documents how async plans, status codes, and runtimes are
    /// handled. No route context is installed here: the base path is the
    /// origin root, route parameters are absent, and `render_metadata_head`
    /// resolves to nothing. For a page that reads any of those, send a request
    /// through the built router with `tower::ServiceExt::oneshot` instead.
    pub fn render_to_bytes(self) -> Result<bytes::Bytes, FrameworkError> {
        self.0.render_to_bytes()
    }

    /// Render a complete document using the catalog's public URLs and base path.
    /// This performs the same buffered render as [`render_to_bytes`](Self::render_to_bytes).
    ///
    /// A page with RSJS islands links their modules at the canonical
    /// `/rsjs-runtime.js` paths here. When
    /// [`RsjsAssets`](crate::RsjsAssets) serves them, use
    /// [`render_to_bytes_with_assets`](Self::render_to_bytes_with_assets) so
    /// the page links the names it publishes.
    pub fn render_to_bytes_with_public_assets(
        self,
        assets: &crate::PublicAssets,
    ) -> Result<bytes::Bytes, FrameworkError> {
        self.render_to_bytes_with_assets(assets, None)
    }

    /// [`render_to_bytes_with_public_assets`](Self::render_to_bytes_with_public_assets),
    /// linking RSJS modules by the names `rsjs` publishes them under.
    pub fn render_to_bytes_with_assets(
        self,
        assets: &crate::PublicAssets,
        rsjs: Option<&crate::RsjsAssets>,
    ) -> Result<bytes::Bytes, FrameworkError> {
        let mut context = FrameworkRequestContext::default();
        context.request.set_base_path(assets.base_path().clone());
        context
            .request
            .set_public_asset_urls(Some(assets.render_urls()));
        context
            .request
            .set_asset_prefix(assets.asset_prefix().map(crate::AssetPrefix::shared));
        context
            .request
            .set_framework_assets(rsjs.map(|rsjs| rsjs.published().clone()));
        self.0.render_to_bytes_with_context(&context)
    }

    /// [`render_to_bytes`](Self::render_to_bytes) as an owned UTF-8 `String`.
    pub fn render_to_string(self) -> Result<String, FrameworkError> {
        self.0.render_to_string()
    }
}

impl<L: DataLoader> From<CoreRouteResponse<L>> for RouteResponse<L> {
    fn from(response: CoreRouteResponse<L>) -> Self {
        Self(response)
    }
}

impl RouteResponse<DynLoaderHandle> {
    /// Construct an async route response with a type-erased loader.
    ///
    /// This is the dyn-boundary escape hatch constructor: `L` is fixed to
    /// `DynLoaderHandle`, so `cx.load_json(...)` calls inside the component pay
    /// one `Box::pin` at the dyn boundary. Use only when the loader's
    /// concrete type is genuinely not statically known at the route's
    /// construction site; prefer
    /// [`ssr_async_with_loader`](RouteResponse::<L>::ssr_async_with_loader)
    /// for the monomorphized fast path.
    pub fn ssr_async_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRender<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async_with_loader_dyn(
            component, loader,
        ))
    }

    /// Construct a streaming route response with a type-erased loader.
    pub fn ssr_stream_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRender<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream_with_loader_dyn(
            component, loader,
        ))
    }

    pub fn ssr_async_send_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRenderSend<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_async_send_with_loader_dyn(
            component, loader,
        ))
    }

    pub fn ssr_stream_send_with_loader_dyn<T>(component: T, loader: Arc<dyn DynDataLoader>) -> Self
    where
        T: proa_core::WebRenderSend<DynLoaderHandle, RenderBuffer> + Send + 'static,
    {
        Self(CoreRouteResponse::ssr_stream_send_with_loader_dyn(
            component, loader,
        ))
    }

    /// Replace the loader on an existing type-erased response.
    ///
    /// The response must already use `L = DynLoaderHandle`; this method does
    /// not change its loader type. Every `cx.load_json(...)` inside the component
    /// pays one `Box::pin` at the dyn boundary.
    ///
    /// Use [`with_loader`](Self::with_loader) on a typed `L` for the
    /// monomorphized fast path; use this only when the loader's concrete
    /// type is genuinely not statically known at the route's construction
    /// site.
    ///
    /// The supplied loader is wrapped with a request-scoped `DataResolver`
    /// for per-request dedupe.
    pub fn with_loader_dyn(self, loader: Arc<dyn DynDataLoader>) -> Self {
        Self(self.0.with_loader_dyn(loader))
    }

    /// Install in-order preload keys. Only meaningful on the dyn path; the
    /// keys are warmed by a `PreparedLoader<dyn>` wrapper.
    pub fn with_preload_keys<I>(self, preload_keys: I) -> Self
    where
        I: IntoIterator<Item = LoadKey>,
    {
        Self(self.0.with_preload_keys(preload_keys))
    }

    /// Install in-order preload requests. Use this when preload hints need to
    /// reach the underlying loader.
    pub fn with_preload_requests<I>(self, preload_requests: I) -> Self
    where
        I: IntoIterator<Item = LoadRequest>,
    {
        Self(self.0.with_preload_requests(preload_requests))
    }
}

pub(super) fn framework_response_to_axum(response: FrameworkResponse) -> Response {
    let FrameworkResponse {
        status,
        headers,
        body,
    } = response;
    let mut axum_response = match body {
        FrameworkBody::Managed(factory) => {
            let (factory, head) = proa_framework_core::managed_response::adapt(factory);
            let mut response = Response::builder()
                .status(status)
                .body(Body::empty())
                .unwrap();
            response
                .extensions_mut()
                .insert(DeferredFrameworkStream::new(factory));
            response.extensions_mut().insert(ManagedPreparedHead(head));
            response
        }
        FrameworkBody::Full(bytes) => Response::builder()
            .status(status)
            .body(Body::from(bytes))
            .expect("framework full response should build"),
        FrameworkBody::BufferedPages(buffer) => Response::builder()
            .status(status)
            .body(Body::from_stream(stream::iter(
                buffer.into_pages().map(Ok::<_, std::convert::Infallible>),
            )))
            .expect("framework buffered pages response should build"),
        FrameworkBody::Stream(factory) => {
            let mut response = Response::builder()
                .status(status)
                .body(Body::empty())
                .expect("framework deferred stream response should build");
            response
                .extensions_mut()
                .insert(DeferredFrameworkStream::new(factory));
            response
        }
    };
    *axum_response.headers_mut() = headers;
    axum_response
}

#[derive(Clone)]
struct ManagedPreparedHead(proa_framework_core::managed_response::PreparedHead);

/// Response extension overriding the managed-head preparation deadline (default: 30 seconds).
/// Install before the outer `RouteContextLayer` prepares the response. This limits
/// time to publication; it does not change a body stream's lifetime.
#[derive(Clone, Copy)]
pub struct ManagedPreparationTimeout(pub std::time::Duration);

#[derive(Clone)]
pub(super) struct DeferredFrameworkStream {
    factory: Arc<Mutex<Option<FrameworkStreamFactory>>>,
}

impl DeferredFrameworkStream {
    fn new(factory: FrameworkStreamFactory) -> Self {
        Self {
            factory: Arc::new(Mutex::new(Some(factory))),
        }
    }

    fn take(&self) -> Option<FrameworkStreamFactory> {
        self.factory
            .lock()
            .expect("deferred stream mutex poisoned")
            .take()
    }
}

enum StreamPrepareError {
    Overloaded,
    Unavailable(Arc<str>),
    FirstItem(proa_framework_core::FrameworkError),
    TimedOut,
}

pub(super) async fn prepare_deferred_framework_stream(
    response: Response,
    executor: LocalStreamExecutor,
) -> Response {
    let mut response = finish_deferred_render(response).await;
    let Some(deferred) = response
        .extensions_mut()
        .remove::<DeferredFrameworkStream>()
    else {
        return response;
    };
    let Some(factory) = deferred.take() else {
        error!("framework stream factory was already consumed");
        return framework_response_to_axum(FrameworkResponse::plain_text(
            StatusCode::INTERNAL_SERVER_ERROR,
            "render error",
        ));
    };

    let readiness = response
        .extensions()
        .get::<ManagedPreparedHead>()
        .map(|h| h.0.clone());
    let prepared = if readiness.is_some() {
        let deadline = response
            .extensions()
            .get::<ManagedPreparationTimeout>()
            .map_or(std::time::Duration::from_secs(30), |v| v.0);
        match tokio::time::timeout(deadline, prepare_stream(factory, &executor, readiness)).await {
            Ok(result) => result,
            Err(_) => Err(StreamPrepareError::TimedOut),
        }
    } else {
        prepare_stream(factory, &executor, readiness).await
    };
    match prepared {
        Ok(stream) => {
            if let Some(prepared) = response.extensions_mut().remove::<ManagedPreparedHead>() {
                let Some(decisions) = prepared.0.get() else {
                    return framework_response_to_axum(FrameworkResponse::plain_text(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "missing prepared response head",
                    ));
                };
                if let Some(status) = decisions.status() {
                    *response.status_mut() = status;
                }
                // A deferred head must not weaken a cache bypass already applied
                // by middleware before preparation. Vary and cookies accumulate.
                let cache_headers = [
                    "cache-control",
                    "cdn-cache-control",
                    "cloudflare-cdn-cache-control",
                    "vercel-cdn-cache-control",
                    "surrogate-control",
                ];
                // Targeted CDN headers can take precedence over Cache-Control.
                // A bypass in any family member also constrains newly added
                // policies, even when middleware did not set that exact name.
                let restricted_cache = cache_headers.iter().any(|name| {
                    response.headers().get_all(*name).iter().any(|value| {
                        value.to_str().map_or(true, |value| {
                            value.split(',').any(|directive| {
                                let name = directive.split('=').next().unwrap_or("").trim();
                                ["private", "no-store", "no-cache"]
                                    .iter()
                                    .any(|restricted| name.eq_ignore_ascii_case(restricted))
                            })
                        })
                    })
                });
                for name in decisions.headers().keys() {
                    if name != http::header::SET_COOKIE && name != http::header::VARY {
                        response.headers_mut().remove(name);
                    }
                }
                for (name, value) in decisions.headers() {
                    response.headers_mut().append(name.clone(), value.clone());
                }
                for name in cache_headers {
                    if restricted_cache && decisions.headers().contains_key(name) {
                        response
                            .headers_mut()
                            .insert(name, http::HeaderValue::from_static("private, no-store"));
                    }
                }
                // The final streamed representation has no known byte length.
                // Framing belongs to the HTTP transport, never a head provider.
                response.headers_mut().remove(http::header::CONTENT_LENGTH);
                response
                    .headers_mut()
                    .remove(http::header::TRANSFER_ENCODING);
                // These statuses cannot carry the generated document body.
                if response.status() == StatusCode::NO_CONTENT
                    || response.status() == StatusCode::NOT_MODIFIED
                    || response.status() == StatusCode::RESET_CONTENT
                {
                    drop(stream);
                    *response.body_mut() = Body::empty();
                    return response;
                }
                if response.status().is_informational() {
                    drop(stream);
                    return framework_response_to_axum(FrameworkResponse::plain_text(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "invalid document status",
                    ));
                }
            }
            *response.body_mut() = Body::from_stream(stream);
            response
        }
        Err(StreamPrepareError::TimedOut) => {
            error!("managed head preparation timed out before response headers");
            framework_response_to_axum(FrameworkResponse::plain_text(
                StatusCode::GATEWAY_TIMEOUT,
                "render timeout",
            ))
        }
        Err(StreamPrepareError::Overloaded) => {
            error!("local stream executor admission limit reached");
            framework_response_to_axum(FrameworkResponse::plain_text(
                StatusCode::SERVICE_UNAVAILABLE,
                "server busy",
            ))
        }
        Err(StreamPrepareError::Unavailable(reason)) => {
            error!(%reason, "local stream executor unavailable");
            framework_response_to_axum(FrameworkResponse::plain_text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "render error",
            ))
        }
        Err(StreamPrepareError::FirstItem(err)) => {
            error!(?err, "framework stream failed before response headers");
            framework_response_to_axum(FrameworkResponse::plain_text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "render error",
            ))
        }
    }
}

async fn prepare_stream(
    factory: FrameworkStreamFactory,
    executor: &LocalStreamExecutor,
    readiness: Option<proa_framework_core::managed_response::PreparedHead>,
) -> Result<FrameworkSendStream, StreamPrepareError> {
    match factory {
        FrameworkStreamFactory::Send(make) => {
            let mut tail = make();
            match proa_framework_core::managed_response::first_or_ready(
                &mut tail,
                readiness.as_ref(),
            )
            .await
            {
                None => Ok(tail),
                Some(Err(error)) => Err(StreamPrepareError::FirstItem(error)),
                Some(Ok(first)) => Ok(Box::pin(
                    stream::once(std::future::ready(Ok(first))).chain(tail),
                )),
            }
        }
        FrameworkStreamFactory::Local(make) => executor
            .prepare_with_head(make, readiness)
            .await
            .map_err(|error| match error {
                LocalStreamPrepareError::Overloaded => StreamPrepareError::Overloaded,
                LocalStreamPrepareError::Unavailable(reason) => {
                    StreamPrepareError::Unavailable(reason)
                }
                LocalStreamPrepareError::FirstItem(error) => StreamPrepareError::FirstItem(error),
            }),
    }
}

fn framework_request_context(ctx: &mut RouteContext) -> FrameworkRequestContext {
    let request_location = ctx.current_uri().cloned().map(RequestLocation::from);
    let route_params = Some(ctx.current_route_params().clone());
    let mut request = RequestContext::new().with_base_path(ctx.base_path().clone());
    request.set_public_asset_urls(ctx.public_asset_urls());
    request.set_framework_assets(ctx.framework_assets());
    request.set_asset_prefix(ctx.asset_prefix());
    if let Some(method) = ctx.current_method().cloned() {
        request.set_method(method);
    }
    request.set_headers(ctx.current_headers().clone());
    if let Some(location) = request_location.clone() {
        request.set_original_pathname(location.path());
        request.set_location(location);
    }
    if let Some(params) = route_params.clone() {
        request.set_route_params(params);
    }
    FrameworkRequestContext {
        head: ctx.head_config().cloned().map(Into::into),
        request,
        request_location,
        route_params,
        render_mode: RenderMode::Static,
        csp_nonce: ctx.csp_nonce().cloned(),
    }
}

pub(super) fn current_framework_request_context() -> FrameworkRequestContext {
    ROUTE_CONTEXT
        .try_with(|ctx| framework_request_context(&mut ctx.borrow_mut()))
        .unwrap_or_default()
}

impl<L: DataLoader + 'static> IntoResponse for RouteResponse<L> {
    fn into_response(self) -> Response {
        let Ok((is_client_request, page_title, request)) = ROUTE_CONTEXT.try_with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            let is_client = ctx.is_client_request();
            let request = framework_request_context(&mut ctx);
            let page_title = if is_client {
                ctx.resolve_metadata().and_then(|meta| meta.title)
            } else {
                None
            };
            (is_client, page_title, request)
        }) else {
            return invalid_route_context();
        };

        let mode = if is_client_request {
            RouteFinalizeMode::ClientPayload { page_title }
        } else {
            RouteFinalizeMode::Document
        };
        let response = self.0;
        let finalized = if response.is_synchronous() {
            finalize_route_response_sync(response, &request, mode)
        } else if matches!(mode, RouteFinalizeMode::Document) && response.is_streaming() {
            finalize_route_response_stream(response, &request)
        } else {
            return defer(finalize_route_response_deferred(response, request, mode));
        };
        match finalized {
            Ok(response) => framework_response_to_axum(response),
            Err(err) => {
                error!(?err, "route finalization failed");
                render_error()
            }
        }
    }
}

fn invalid_route_context() -> Response {
    error!("RouteResponse used outside RouteContextLayer, which FrameworkBuilder installs");
    (StatusCode::INTERNAL_SERVER_ERROR, "invalid route context").into_response()
}

fn render_error() -> Response {
    framework_response_to_axum(FrameworkResponse::plain_text(
        StatusCode::INTERNAL_SERVER_ERROR,
        "render error",
    ))
}

/// A buffered render that `into_response` could not await.
#[derive(Clone)]
pub(super) struct DeferredRender(Arc<Mutex<Option<PendingRender>>>);

struct PendingRender {
    finalization: RouteFinalization,
    route: RouteContext,
}

/// Return a placeholder whose render runs at the next framework boundary:
/// a page cache, or else `RouteContextLayer`, which always awaits it before
/// returning response headers. Without that layer this is an HTTP 500.
pub(super) fn defer(finalization: RouteFinalization) -> Response {
    let Ok(route) = ROUTE_CONTEXT.try_with(|ctx| ctx.borrow().clone()) else {
        return invalid_route_context();
    };
    let mut response = Response::new(Body::empty());
    response
        .extensions_mut()
        .insert(DeferredRender(Arc::new(Mutex::new(Some(PendingRender {
            finalization,
            route,
        })))));
    response
}

/// Finish a render deferred by [`defer`], then reapply what handler wrappers
/// and middleware set on the placeholder.
pub(super) async fn finish_deferred_render(mut response: Response) -> Response {
    let Some(deferred) = response.extensions_mut().remove::<DeferredRender>() else {
        return response;
    };
    let pending = deferred
        .0
        .lock()
        .expect("deferred render mutex poisoned")
        .take();
    let rendered = match pending {
        Some(PendingRender {
            finalization,
            route,
        }) => render_pending(finalization, route).await,
        None => {
            error!("deferred route render was already consumed");
            render_error()
        }
    };
    apply_placeholder(response, rendered)
}

async fn render_pending(finalization: RouteFinalization, route: RouteContext) -> Response {
    // The render sees the route context, endpoint metadata included, from
    // when the handler returned.
    let result = match finalization {
        RouteFinalization::Send(render) => ROUTE_CONTEXT.scope(RefCell::new(route), render).await,
        RouteFinalization::Local(make) => {
            let Some(executor) = route.local_stream_executor().cloned() else {
                error!("deferred local render has no executor");
                return render_error();
            };
            let run = executor.run(Box::new(move || {
                ROUTE_CONTEXT
                    .scope(RefCell::new(route), make())
                    .boxed_local()
            }));
            match run.await {
                Ok(result) => result,
                Err(LocalStreamPrepareError::Overloaded) => {
                    error!("local stream executor admission limit reached");
                    return framework_response_to_axum(FrameworkResponse::plain_text(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "server busy",
                    ));
                }
                Err(LocalStreamPrepareError::Unavailable(reason)) => {
                    error!(%reason, "local stream executor unavailable");
                    return render_error();
                }
                Err(LocalStreamPrepareError::FirstItem(err)) => Err(err),
            }
        }
    };
    match result {
        Ok(response) => framework_response_to_axum(response),
        Err(err) => {
            error!(?err, "route finalization failed");
            render_error()
        }
    }
}

/// Wrappers and middleware edited the placeholder after `into_response`, as
/// they once edited the rendered response. Keep that precedence: a status
/// other than the placeholder's 200 and each header name win, while `Vary`,
/// `Link`, and `Set-Cookie` accumulate.
fn apply_placeholder(placeholder: Response, mut rendered: Response) -> Response {
    let (parts, _) = placeholder.into_parts();
    if parts.status != StatusCode::OK {
        *rendered.status_mut() = parts.status;
    }
    let headers = rendered.headers_mut();
    let mut current = None;
    for (name, value) in parts.headers {
        if let Some(name) = name {
            if name != http::header::VARY
                && name != http::header::LINK
                && name != http::header::SET_COOKIE
            {
                headers.remove(&name);
            }
            current = Some(name);
        }
        if let Some(name) = &current {
            headers.append(name, value);
        }
    }
    rendered.extensions_mut().extend(parts.extensions);
    rendered
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use http_body_util::BodyExt as _;

    use super::*;
    use crate::route::LocalStreamExecutorConfig;

    fn unavailable_local_executor() -> LocalStreamExecutor {
        LocalStreamExecutor::new(LocalStreamExecutorConfig {
            worker_threads: 0,
            ..LocalStreamExecutorConfig::default()
        })
    }

    #[tokio::test]
    async fn send_stream_first_error_becomes_http_500() {
        let framework = FrameworkResponse::send_stream(|| -> FrameworkSendStream {
            Box::pin(stream::iter([Err(
                proa_framework_core::FrameworkError::Invariant("first item failed"),
            )]))
        });
        let deferred = framework_response_to_axum(framework);
        let response =
            prepare_deferred_framework_stream(deferred, unavailable_local_executor()).await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            "render error"
        );
    }

    #[tokio::test]
    async fn send_stream_chains_first_item_and_preserves_later_body_error() {
        let framework = FrameworkResponse::send_stream(|| -> FrameworkSendStream {
            Box::pin(stream::iter([
                Ok(bytes::Bytes::from_static(b"first")),
                Err(proa_framework_core::FrameworkError::Invariant(
                    "later item failed",
                )),
            ]))
        });
        let deferred = framework_response_to_axum(framework);
        let response =
            prepare_deferred_framework_stream(deferred, unavailable_local_executor()).await;

        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let first = body
            .frame()
            .await
            .expect("first frame")
            .expect("first frame succeeds")
            .into_data()
            .expect("data frame");
        assert_eq!(first, "first");
        let error = body
            .frame()
            .await
            .expect("later error frame")
            .expect_err("later stream error must reach the body");
        assert!(error.to_string().contains("later item failed"));
    }

    #[tokio::test]
    async fn send_stream_clean_eof_keeps_status_and_empty_body() {
        let framework =
            FrameworkResponse::send_stream(|| -> FrameworkSendStream { Box::pin(stream::empty()) });
        let deferred = framework_response_to_axum(framework);
        let response =
            prepare_deferred_framework_stream(deferred, unavailable_local_executor()).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
    }
}

#[cfg(test)]
#[path = "managed_response_tests.rs"]
mod managed_response_tests;
