// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures::{stream, StreamExt as _};
use proa_core::{
    DataLoader, DynDataLoader, DynLoaderHandle, LoadKey, LoadRequest, NoLoader, RenderBuffer,
    RenderMode, RequestContext, RequestLocation,
};
use proa_framework_core::{
    finalize_route_response, finalize_route_response_stream, finalize_route_response_sync,
    FrameworkBody, FrameworkRequestContext, FrameworkResponse, FrameworkSendStream,
    FrameworkStreamFactory, RouteFinalizeMode, RouteResponse as CoreRouteResponse,
};
use proa_react::ReactBootstrapConfig;
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
    /// response metadata are known before body delivery begins. Use this when
    /// the component never calls `cx.load(...)`. For routes
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
    /// Use this when the component never calls `cx.load(...)`. For routes
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
    /// `WebRender<L>`; `cx.load(...)` calls inside it become
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

    pub fn with_islands<I, S>(self, islands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(self.0.with_islands(islands))
    }

    pub fn with_bootstrap(self, config: ReactBootstrapConfig<'static>) -> Self {
        Self(self.0.with_bootstrap(config))
    }

    /// Install a typed loader on the response.
    ///
    /// `L` is the loader's concrete type. `cx.load(...)` calls inside the
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
    /// `DynLoaderHandle`, so `cx.load(...)` calls inside the component pay
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
    /// not change its loader type. Every `cx.load(...)` inside the component
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
}

pub(super) async fn prepare_deferred_framework_stream(
    mut response: Response,
    executor: LocalStreamExecutor,
) -> Response {
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

    match prepare_stream(factory, &executor).await {
        Ok(stream) => {
            *response.body_mut() = Body::from_stream(stream);
            response
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
) -> Result<FrameworkSendStream, StreamPrepareError> {
    match factory {
        FrameworkStreamFactory::Send(make) => {
            let mut tail = make();
            match tail.next().await {
                None => Ok(Box::pin(stream::empty())),
                Some(Err(error)) => Err(StreamPrepareError::FirstItem(error)),
                Some(Ok(first)) => Ok(Box::pin(
                    stream::once(std::future::ready(Ok(first))).chain(tail),
                )),
            }
        }
        FrameworkStreamFactory::Local(make) => {
            executor.prepare(make).await.map_err(|error| match error {
                LocalStreamPrepareError::Overloaded => StreamPrepareError::Overloaded,
                LocalStreamPrepareError::Unavailable(reason) => {
                    StreamPrepareError::Unavailable(reason)
                }
                LocalStreamPrepareError::FirstItem(error) => StreamPrepareError::FirstItem(error),
            })
        }
    }
}

fn framework_request_context(ctx: &RouteContext) -> FrameworkRequestContext {
    let request_location = ctx.current_uri().cloned().map(RequestLocation::from);
    let route_params = Some(ctx.current_route_params().clone());
    let mut request = RequestContext::new();
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
        request,
        request_location,
        route_params,
        render_mode: RenderMode::Static,
        csp_nonce: ctx.csp_nonce().cloned(),
    }
}

pub(super) fn current_framework_request_context() -> FrameworkRequestContext {
    ROUTE_CONTEXT
        .try_with(|ctx| framework_request_context(&ctx.borrow()))
        .unwrap_or_default()
}

impl<L: DataLoader + 'static> IntoResponse for RouteResponse<L> {
    fn into_response(self) -> Response {
        ROUTE_CONTEXT
            .try_with(|ctx| {
                let (manifest, is_client_request, page_title, request) = {
                    let mut ctx_ref = ctx.borrow_mut();
                    let manifest = ctx_ref.manifest_arc();
                    let is_client = ctx_ref.is_client_request();
                    let request = framework_request_context(&ctx_ref);
                    let page_title = if is_client {
                        ctx_ref.resolve_metadata().and_then(|meta| meta.title)
                    } else {
                        None
                    };
                    (manifest, is_client, page_title, request)
                };

                let mode = if is_client_request {
                    RouteFinalizeMode::ClientPayload { page_title }
                } else {
                    RouteFinalizeMode::Document
                };
                let response = self.0;
                let streamed_document =
                    matches!(&mode, RouteFinalizeMode::Document) && response.is_streaming();
                let finalized = if response.is_synchronous() {
                    finalize_route_response_sync(response, manifest.as_ref(), &request, mode)
                } else if streamed_document {
                    finalize_route_response_stream(response, manifest.as_ref(), &request)
                } else {
                    block_on_current_runtime(finalize_route_response::<L>(
                        response,
                        manifest.as_ref(),
                        &request,
                        mode,
                    ))
                };

                match finalized {
                    Ok(response) => framework_response_to_axum(response),
                    Err(err) => {
                        error!(?err, "route finalization failed");
                        framework_response_to_axum(FrameworkResponse::plain_text(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "render error",
                        ))
                    }
                }
            })
            .unwrap_or_else(|_| {
                error!("RouteResponse used outside of routing context");
                (StatusCode::INTERNAL_SERVER_ERROR, "invalid route context").into_response()
            })
    }
}

pub(super) fn block_on_current_runtime<F>(fut: F) -> F::Output
where
    F: Future,
{
    let handle = tokio::runtime::Handle::try_current().expect(
        "RouteResponse async finalization was invoked outside a tokio runtime; \
         axum's tokio runtime is required to drive the future",
    );
    match handle.runtime_flavor() {
        tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        _ => handle.block_on(fut),
    }
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
