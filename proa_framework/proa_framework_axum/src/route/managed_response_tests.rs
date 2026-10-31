// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Tests the real Axum first-item gate and real local-stream executor with
//! production core/macros, managed drivers and the typed readiness protocol.
use super::*;
use crate::route::local_executor::LocalStreamExecutorConfig;
use futures::{
    channel::{mpsc, oneshot},
    future::{select, Either},
    StreamExt,
};
use http::HeaderValue;
use http_body_util::BodyExt;
use proa_core::{
    head_dispatch::ComponentRole, managed_head::ManagedHead, NoLoader, RenderBuffer, RenderOutcome,
    SendWebContext, WebContext, WebRender, WebRenderSend, WriteBuf, WriteError,
};
use proa_framework_core::{
    managed_response::{LocalEvents, ManagedEvent, ManagedFactory, SendEvents},
    FrameworkError,
};
use proa_macros::{html, html_send};
use std::{future::Future, rc::Rc, sync::Arc, time::Duration};

struct ProductPage {
    body: oneshot::Receiver<()>,
    fail_head: bool,
    fail_body: bool,
}
impl WebRender for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> impl Future<Output = Result<(), WriteError>> {
        let local = Rc::new(self.fail_head);
        async move {
            std::future::ready(()).await;
            if *local {
                return Err(WriteError::render("lookup failed"));
            }
            cx.head_mut().title("Product")?;
            assert!(cx.response_mut().set_status(StatusCode::CREATED));
            assert!(cx.response_mut().insert_header(
                http::header::CACHE_CONTROL,
                HeaderValue::from_static("private")
            ));
            assert!(cx
                .response_mut()
                .append_set_cookie(HeaderValue::from_static("page=1")));
            Ok(())
        }
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _ = self.body.await;
            if self.fail_body {
                return Err(WriteError::render("late body failure"));
            }
            assert!(!cx.response_mut().set_status(StatusCode::ACCEPTED));
            cx.output_mut().extend(b"PRODUCT_BODY")
        })
    }
}
impl WebRenderSend for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head_send(&self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        if self.fail_head {
            return Err(WriteError::render("lookup failed"));
        }
        cx.head_mut().title("Product")?;
        assert!(cx.response_mut().set_status(StatusCode::CREATED));
        assert!(cx.response_mut().insert_header(
            http::header::CACHE_CONTROL,
            HeaderValue::from_static("private")
        ));
        assert!(cx
            .response_mut()
            .append_set_cookie(HeaderValue::from_static("page=1")));
        Ok(())
    }
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            let _ = self.body.await;
            if self.fail_body {
                return Err(WriteError::render("late body failure"));
            }
            assert!(!cx.response_mut().set_status(StatusCode::ACCEPTED));
            cx.output_mut().extend(b"PRODUCT_BODY")
        })
    }
}
struct Document(ProductPage);
// proa-perf-ok(scope): async lifecycle fixture must exercise actual pending child dispatch
impl WebRender for Document {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        html! {
            <!DOCTYPE html>
            <html>
                <head>{head}</head>
                <body>{self.0}</body>
            </html>
        }
        .render(cx)
    }
}
impl WebRenderSend for Document {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        let head = cx.head();
        html_send! {<!DOCTYPE html><html><head>{head}</head><body>{self.0}</body></html>}
            .render_send(cx)
    }
}
// Same scheduling shape as the existing in-order driver. The out-of-order
// implementation must share this initial-shell pump instead of awaiting the
// complete root before its first yield; boundary batches follow shell closure.
macro_rules! pump {
    ($cx:ident,$page:ident,$tx:ident,$rx:ident,$root:expr)=>{
        async_stream::try_stream!{
            let mut render=Box::pin(async move{
                $root.await?;
                $cx.head_state().document().unwrap().final_check()?;
                for page in $cx.into_output().into_pages(){
                    $tx.unbounded_send(Ok(ManagedEvent::Chunk(page))).map_err(|_|WriteError::StreamClosed)?;
                }
                Ok::<_,WriteError>(())
            });
            loop{
                match select($rx.next(),render).await{
                    Either::Left((Some(event),remaining))=>{render=remaining;yield event?;}
                    Either::Left((None,remaining))=>{remaining.await?;break;}
                    Either::Right((result,_))=>{
                        result?;
                        while let Some(event)=$rx.next().await{yield event?;}
                        break;
                    }
                }
            }
        }
    }
}
fn make_factory(
    send: bool,
    body: oneshot::Receiver<()>,
    fail_head: bool,
    fail_body: bool,
) -> ManagedFactory {
    if send {
        ManagedFactory::Send(Box::new(move || {
            let (tx, mut rx) = mpsc::unbounded::<Result<ManagedEvent, FrameworkError>>();
            let publisher = tx.clone();
            let head = ManagedHead::new(
                1024 * 1024,
                Arc::new(move |response, prefix| {
                    publisher
                        .unbounded_send(Ok(ManagedEvent::Ready { response, prefix }))
                        .map_err(|_| WriteError::StreamClosed)
                }),
            );
            let mut cx = SendWebContext::<NoLoader, RenderBuffer>::new();
            cx.head_mut().manage(head);
            let page = Document(ProductPage {
                body,
                fail_head,
                fail_body,
            });
            Box::pin(pump!(
                cx,
                page,
                tx,
                rx,
                proa_core::render_send_child(&mut cx, page)
            )) as SendEvents
        }))
    } else {
        ManagedFactory::Local(Box::new(move || {
            let (tx, mut rx) = mpsc::unbounded::<Result<ManagedEvent, FrameworkError>>();
            let publisher = tx.clone();
            let head = ManagedHead::new(
                1024 * 1024,
                Arc::new(move |response, prefix| {
                    publisher
                        .unbounded_send(Ok(ManagedEvent::Ready { response, prefix }))
                        .map_err(|_| WriteError::StreamClosed)
                }),
            );
            let mut cx = WebContext::<NoLoader, RenderBuffer>::new();
            cx.head_mut().manage(head);
            let page = Document(ProductPage {
                body,
                fail_head,
                fail_body,
            });
            Box::pin(pump!(
                cx,
                page,
                tx,
                rx,
                proa_core::render_child(&mut cx, page)
            )) as LocalEvents
        }))
    }
}
fn executor() -> LocalStreamExecutor {
    LocalStreamExecutor::new(LocalStreamExecutorConfig::default())
}
fn response(factory: ManagedFactory) -> Response {
    let mut r = FrameworkResponse::html("");
    r.body = FrameworkBody::Managed(factory);
    r.headers
        .insert("x-middleware", HeaderValue::from_static("kept"));
    r.headers.append(
        http::header::SET_COOKIE,
        HeaderValue::from_static("middleware=1"),
    );
    framework_response_to_axum(r)
}
#[tokio::test]
async fn managed_http_ready_arrives_while_page_body_is_still_pending() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            prepare_deferred_framework_stream(
                response(make_factory(send, body, false, false)),
                executor(),
            ),
        )
        .await
        .expect("head must not wait for body");
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[http::header::CACHE_CONTROL], "private");
        assert_eq!(response.headers()["x-middleware"], "kept");
        assert_eq!(
            response
                .headers()
                .get_all(http::header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
        let mut stream = response.into_body().into_data_stream();
        let mut prefix = Vec::new();
        while !prefix.ends_with(b"<body>") {
            let bytes = tokio::time::timeout(Duration::from_secs(3), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            prefix.extend_from_slice(&bytes);
        }
        assert!(String::from_utf8(prefix.clone())
            .unwrap()
            .contains("<head><title>Product</title></head>"));
        assert!(!String::from_utf8(prefix).unwrap().contains("PRODUCT_BODY"));
        release.send(()).unwrap();
        let mut tail = Vec::new();
        while let Some(bytes) = stream.next().await {
            tail.extend_from_slice(&bytes.unwrap());
        }
        assert_eq!(
            String::from_utf8(tail).unwrap(),
            "PRODUCT_BODY</body></html>"
        );
    }
}
#[tokio::test]
async fn managed_hook_failure_is_500_before_any_document_bytes() {
    for send in [false, true] {
        let (_release, body) = oneshot::channel();
        let response = prepare_deferred_framework_stream(
            response(make_factory(send, body, true, false)),
            executor(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "render error"
        );
    }
}
#[tokio::test]
async fn managed_failure_after_commit_ends_body_without_replacing_status() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        let response = prepare_deferred_framework_stream(
            response(make_factory(send, body, false, true)),
            executor(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        release.send(()).unwrap();
        assert!(response.into_body().collect().await.is_err());
    }
}
#[tokio::test]
async fn managed_protocol_rejects_bytes_without_ready() {
    let factory = ManagedFactory::Send(Box::new(|| {
        Box::pin(futures::stream::iter([Ok(ManagedEvent::Chunk(
            bytes::Bytes::from_static(b"too early"),
        ))]))
    }));
    let response = prepare_deferred_framework_stream(response(factory), executor()).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn managed_protocol_rejects_missing_ready() {
    let factory = ManagedFactory::Send(Box::new(|| Box::pin(futures::stream::empty())));
    let response = prepare_deferred_framework_stream(response(factory), executor()).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn managed_protocol_rejects_duplicate_ready_after_commit() {
    let factory = ManagedFactory::Send(Box::new(|| {
        let event = || ManagedEvent::Ready {
            response: proa_core::ResponseContext::new(),
            prefix: vec![bytes::Bytes::from_static(b"<html>")],
        };
        Box::pin(futures::stream::iter([Ok(event()), Ok(event())]))
    }));
    let response = prepare_deferred_framework_stream(response(factory), executor()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.into_body().collect().await.is_err());
}

struct CancellationProbe(Arc<std::sync::atomic::AtomicBool>);
impl Drop for CancellationProbe {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
fn pending_factory(send: bool, cancelled: Arc<std::sync::atomic::AtomicBool>) -> ManagedFactory {
    if send {
        ManagedFactory::Send(Box::new(move || {
            Box::pin(async_stream::try_stream! {
                let _probe=CancellationProbe(cancelled);
                std::future::pending::<()>().await;
                yield ManagedEvent::Chunk(bytes::Bytes::new());
            })
        }))
    } else {
        ManagedFactory::Local(Box::new(move || {
            Box::pin(async_stream::try_stream! {
                let _probe=CancellationProbe(cancelled);
                std::future::pending::<()>().await;
                yield ManagedEvent::Chunk(bytes::Bytes::new());
            })
        }))
    }
}
#[tokio::test]
async fn preparation_deadline_cancels_pending_local_and_send_sources() {
    for send in [false, true] {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut initial = response(pending_factory(send, cancelled.clone()));
        initial
            .extensions_mut()
            .insert(ManagedPreparationTimeout(Duration::from_millis(50)));
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            prepare_deferred_framework_stream(initial, executor()),
        )
        .await
        .unwrap();
        assert_eq!(result.status(), StatusCode::GATEWAY_TIMEOUT);
        tokio::time::timeout(Duration::from_secs(3), async {
            while !cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("dropping preparation must cancel its source");
        assert_eq!(
            result.into_body().collect().await.unwrap().to_bytes(),
            "render timeout"
        );
    }
}
fn ready_factory(status: StatusCode) -> ManagedFactory {
    ManagedFactory::Send(Box::new(move || {
        let mut decisions = proa_core::ResponseContext::new();
        decisions.set_status(status);
        decisions.insert_header(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_static("999999"),
        );
        decisions.insert_header(
            http::header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        Box::pin(futures::stream::iter([Ok(ManagedEvent::Ready {
            response: decisions,
            prefix: vec![bytes::Bytes::from_static(b"<html>prepared</html>")],
        })]))
    }))
}
#[tokio::test]
async fn managed_response_does_not_trust_provider_framing_headers() {
    let mut initial = response(ready_factory(StatusCode::OK));
    initial
        .headers_mut()
        .insert(http::header::CONTENT_LENGTH, HeaderValue::from_static("1"));
    let result = prepare_deferred_framework_stream(initial, executor()).await;
    assert_eq!(result.status(), StatusCode::OK);
    assert!(!result.headers().contains_key(http::header::CONTENT_LENGTH));
    assert!(!result
        .headers()
        .contains_key(http::header::TRANSFER_ENCODING));
    assert_eq!(
        result.into_body().collect().await.unwrap().to_bytes(),
        "<html>prepared</html>"
    );
}
#[tokio::test]
async fn no_body_statuses_do_not_emit_the_prepared_document() {
    for status in [
        StatusCode::NO_CONTENT,
        StatusCode::NOT_MODIFIED,
        StatusCode::RESET_CONTENT,
    ] {
        let result =
            prepare_deferred_framework_stream(response(ready_factory(status)), executor()).await;
        assert_eq!(result.status(), status);
        assert!(!result.headers().contains_key(http::header::CONTENT_LENGTH));
        assert!(result
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
    }
}
#[tokio::test]
async fn informational_status_cannot_become_a_final_document_response() {
    let result = prepare_deferred_framework_stream(
        response(ready_factory(StatusCode::EARLY_HINTS)),
        executor(),
    )
    .await;
    assert_eq!(result.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        result.into_body().collect().await.unwrap().to_bytes(),
        "invalid document status"
    );
}
#[tokio::test]
async fn body_failure_in_ready_poll_discards_unsent_document() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        release.send(()).unwrap();
        let result = prepare_deferred_framework_stream(
            response(make_factory(send, body, false, true)),
            executor(),
        )
        .await;
        assert_eq!(result.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            result.into_body().collect().await.unwrap().to_bytes(),
            "render error"
        );
    }
}
#[tokio::test]
async fn axum_head_route_preserves_prepared_headers_and_suppresses_body() {
    use tower::ServiceExt;
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            prepare_deferred_framework_stream(response(ready_factory(StatusCode::OK)), executor())
                .await
        }),
    );
    let request = http::Request::builder()
        .method(http::Method::HEAD)
        .uri("/")
        .body(Body::empty())
        .unwrap();
    let result = app.oneshot(request).await.unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()["x-middleware"], "kept");
    assert!(result
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
}
#[tokio::test]
async fn redirect_not_found_and_forbidden_are_decided_before_document_bytes() {
    for status in [
        StatusCode::SEE_OTHER,
        StatusCode::NOT_FOUND,
        StatusCode::FORBIDDEN,
    ] {
        let result =
            prepare_deferred_framework_stream(response(ready_factory(status)), executor()).await;
        assert_eq!(result.status(), status);
        assert_eq!(
            result.into_body().collect().await.unwrap().to_bytes(),
            "<html>prepared</html>"
        );
    }
}

#[tokio::test]
async fn actual_route_drivers_publish_before_pending_body_in_every_delivery_mode() {
    for send in [false, true] {
        for mode in [
            proa_core::RenderMode::ChunkedInOrder,
            proa_core::RenderMode::ChunkedOutOfOrder,
        ] {
            let (release, body) = oneshot::channel();
            let page = Document(ProductPage {
                body,
                fail_head: false,
                fail_body: false,
            });
            let route = if send {
                CoreRouteResponse::ssr_stream_send(page)
            } else {
                CoreRouteResponse::ssr_stream(page)
            }
            .with_render_mode(mode)
            .with_head(proa_core::HeadConfig::default());
            let response = route
                .into_stream_framework_response(&empty_manifest(), &Default::default())
                .unwrap();
            let response = tokio::time::timeout(
                Duration::from_secs(2),
                prepare_deferred_framework_stream(framework_response_to_axum(response), executor()),
            )
            .await
            .expect("actual route must publish before body");
            assert_eq!(response.status(), StatusCode::CREATED);
            assert_eq!(response.headers()[http::header::CACHE_CONTROL], "private");
            let mut stream = response.into_body().into_data_stream();
            let mut prefix = Vec::new();
            while !prefix.ends_with(b"<body>") {
                prefix.extend_from_slice(
                    &tokio::time::timeout(Duration::from_secs(2), stream.next())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap(),
                );
            }
            assert!(String::from_utf8(prefix)
                .unwrap()
                .contains("<title>Product</title>"));
            release.send(()).unwrap();
            let mut tail = Vec::new();
            while let Some(chunk) = stream.next().await {
                tail.extend_from_slice(&chunk.unwrap());
            }
            assert_eq!(tail, b"PRODUCT_BODY</body></html>");
        }
    }
}

struct ImmediatePage(Arc<std::sync::atomic::AtomicBool>);
impl WebRender for ImmediatePage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Ready first")
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.output_mut().extend(b"immediate body"),
        )
    }
}
impl WebRenderSend for ImmediatePage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head_send(&self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Ready first")
    }
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.output_mut().extend(b"immediate body"),
        )
    }
}
struct ImmediateDocument<P>(P);
impl<P: WebRender + Send + 'static> WebRender for ImmediateDocument<P> {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        let document = html! {
            <html>
                <head>{head}</head>
                <body>{self.0}</body>
            </html>
        };
        document.render(cx)
    }
}
impl<P: WebRenderSend + 'static> WebRenderSend for ImmediateDocument<P> {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        let head = cx.head();
        html_send! {<html><head>{head}</head><body>{self.0}</body></html>}.render_send(cx)
    }
}

// This Page inherits application/layout metadata and never constructs a head hook.
struct InheritedPage(ImmediatePage);
// proa-perf-ok(scope): test the default async head strategy with an immediately ready body
impl WebRender for InheritedPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        self.0.render(cx)
    }
}
impl WebRenderSend for InheritedPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        self.0.render_send(cx)
    }
}

#[tokio::test]
async fn inherited_head_is_published_before_same_poll_body_work() {
    use std::sync::atomic::Ordering;
    for send in [false, true] {
        for mode in [
            proa_core::RenderMode::ChunkedInOrder,
            proa_core::RenderMode::ChunkedOutOfOrder,
        ] {
            let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let page = ImmediateDocument(InheritedPage(ImmediatePage(ran.clone())));
            let route = if send {
                CoreRouteResponse::ssr_stream_send(page)
            } else {
                CoreRouteResponse::ssr_stream(page)
            }
            .with_render_mode(mode)
            .with_head(
                proa_core::HeadConfig::default()
                    .with_defaults(|head| head.title("Inherited"))
                    .unwrap(),
            );
            let response = route
                .into_stream_framework_response(&empty_manifest(), &Default::default())
                .unwrap();
            let FrameworkBody::Managed(factory) = response.body else {
                panic!("expected managed stream")
            };
            let mut events: LocalEvents = match factory {
                ManagedFactory::Local(make) => make(),
                ManagedFactory::Send(make) => Box::pin(make()),
            };
            let Some(Ok(ManagedEvent::Ready { prefix, .. })) = events.next().await else {
                panic!("inherited head was not published first")
            };
            assert!(
                !ran.load(Ordering::SeqCst),
                "body ran before the inherited head was delivered (send={send}, mode={mode:?})"
            );
            let mut bytes: Vec<u8> = prefix.into_iter().flatten().collect();
            while let Some(event) = events.next().await {
                match event.unwrap() {
                    ManagedEvent::Chunk(page) => bytes.extend_from_slice(&page),
                    ManagedEvent::Ready { .. } => panic!("duplicate Ready"),
                }
            }
            assert!(ran.load(Ordering::SeqCst));
            assert_eq!(
                bytes,
                b"<html><head><title>Inherited</title></head><body>immediate body</body></html>"
            );
        }
    }
}
#[tokio::test]
async fn actual_route_publishes_before_same_poll_body_work() {
    use std::sync::atomic::Ordering;
    for send in [false, true] {
        for mode in [
            proa_core::RenderMode::ChunkedInOrder,
            proa_core::RenderMode::ChunkedOutOfOrder,
        ] {
            let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let page = ImmediateDocument(ImmediatePage(ran.clone()));
            let route = if send {
                CoreRouteResponse::ssr_stream_send(page)
            } else {
                CoreRouteResponse::ssr_stream(page)
            }
            .with_render_mode(mode)
            .with_head(proa_core::HeadConfig::default());
            let response = route
                .into_stream_framework_response(&empty_manifest(), &Default::default())
                .unwrap();
            let FrameworkBody::Managed(factory) = response.body else {
                panic!("expected managed stream")
            };
            let mut events: LocalEvents = match factory {
                ManagedFactory::Local(make) => make(),
                ManagedFactory::Send(make) => Box::pin(make()),
            };
            let Some(Ok(ManagedEvent::Ready { prefix, .. })) = events.next().await else {
                panic!("head was not published first")
            };
            assert!(
                !ran.load(Ordering::SeqCst),
                "body ran in the publication poll"
            );
            let mut bytes = Vec::new();
            for page in prefix {
                bytes.extend_from_slice(&page);
            }
            while let Some(event) = events.next().await {
                match event.unwrap() {
                    ManagedEvent::Chunk(page) => bytes.extend_from_slice(&page),
                    ManagedEvent::Ready { .. } => panic!("duplicate Ready"),
                }
            }
            assert!(ran.load(Ordering::SeqCst));
            assert_eq!(
                bytes,
                b"<html><head><title>Ready first</title></head><body>immediate body</body></html>"
            );
        }
    }
}

struct BodyWork(Arc<std::sync::atomic::AtomicBool>);
impl WebRender for BodyWork {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.output_mut().extend(b"body"),
        )
    }
}
impl WebRenderSend for BodyWork {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.output_mut().extend(b"body"),
        )
    }
}
struct PageWithOutlet(BodyWork);
// proa-perf-ok(scope): the async outlet must yield before the immediately ready body fixture
impl WebRender for PageWithOutlet {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Page outlet")
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        let document = html! {
            <html>
                <head>{head}</head>
                <body>{self.0}</body>
            </html>
        };
        document.render(cx)
    }
}
impl WebRenderSend for PageWithOutlet {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head_send(&self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Page outlet")
    }
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        let head = cx.head();
        html_send! {<html><head>{head}</head><body>{self.0}</body></html>}.render_send(cx)
    }
}
#[tokio::test]
async fn outlet_inside_page_publishes_before_following_body_child() {
    use std::sync::atomic::Ordering;
    for send in [false, true] {
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let page = PageWithOutlet(BodyWork(ran.clone()));
        let route = if send {
            CoreRouteResponse::ssr_stream_send(page)
        } else {
            CoreRouteResponse::ssr_stream(page)
        }
        .with_head(proa_core::HeadConfig::default());
        let response = route
            .into_stream_framework_response(&empty_manifest(), &Default::default())
            .unwrap();
        let FrameworkBody::Managed(factory) = response.body else {
            panic!("expected managed stream")
        };
        let mut events: LocalEvents = match factory {
            ManagedFactory::Local(make) => make(),
            ManagedFactory::Send(make) => Box::pin(make()),
        };
        assert!(matches!(
            events.next().await,
            Some(Ok(ManagedEvent::Ready { .. }))
        ));
        assert!(!ran.load(Ordering::SeqCst));
        while let Some(event) = events.next().await {
            event.unwrap();
        }
        assert!(ran.load(Ordering::SeqCst));
    }
}

struct EmptyHeadPage(oneshot::Receiver<()>);
impl WebRender for EmptyHeadPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            proa_core::WebRenderSync::render(cx.head(), cx)?;
            let _ = self.0.await;
            cx.output_mut().extend(b"body")
        })
    }
}
impl WebRenderSend for EmptyHeadPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            proa_core::WebRenderSendSync::render_send_sync(cx.head(), cx)?;
            let _ = self.0.await;
            cx.output_mut().extend(b"body")
        })
    }
}
#[tokio::test]
async fn empty_head_is_ready_without_a_synthetic_byte_or_ready_body() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        let route = if send {
            CoreRouteResponse::ssr_stream_send(EmptyHeadPage(body))
        } else {
            CoreRouteResponse::ssr_stream(EmptyHeadPage(body))
        }
        .with_head(proa_core::HeadConfig::default());
        let response = route
            .into_stream_framework_response(&empty_manifest(), &Default::default())
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            prepare_deferred_framework_stream(framework_response_to_axum(response), executor()),
        )
        .await
        .expect("empty Ready is still Ready");
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), stream.next())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(bytes, b"body");
    }
}

fn empty_manifest() -> proa_framework_core::IslandManifest {
    serde_json::from_str(r#"{"islands":{}}"#).unwrap()
}

struct BadDeferred;
impl WebRender for BadDeferred {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let _ignored = cx.head_mut().title("too late");
        RenderOutcome::<std::future::Ready<_>>::Done(cx.output_mut().extend(b"must not publish"))
    }
}
struct DeferredPage;
impl WebRender for DeferredPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <head>{cx.head()}</head>
            <body>{proa_stream::Suspense::new(BadDeferred, "loading")}</body>
        }
        .render(cx)
    }
}
#[tokio::test]
async fn ignored_deferred_metadata_error_terminates_the_actual_ooo_body() {
    let route = CoreRouteResponse::ssr_stream(DeferredPage)
        .with_render_mode(proa_core::RenderMode::ChunkedOutOfOrder)
        .with_head(proa_core::HeadConfig::default());
    let response = route
        .into_stream_framework_response(&empty_manifest(), &Default::default())
        .unwrap();
    let response =
        prepare_deferred_framework_stream(framework_response_to_axum(response), executor()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let mut bytes = Vec::new();
    let mut failed = false;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(
        failed,
        "detached failures must not disappear after initial root finalization"
    );
    assert!(!String::from_utf8_lossy(&bytes).contains("must not publish"));
}

#[tokio::test]
async fn buffered_local_and_send_use_the_same_head_values_and_status() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        release.send(()).unwrap();
        let page = Document(ProductPage {
            body,
            fail_head: false,
            fail_body: false,
        });
        let route = if send {
            CoreRouteResponse::ssr_async_send(page)
        } else {
            CoreRouteResponse::ssr_async(page)
        }
        .with_head(proa_core::HeadConfig::default());
        let response = route
            .into_framework_response(&empty_manifest(), &Default::default())
            .await
            .unwrap();
        assert_eq!(response.status, StatusCode::CREATED);
        let FrameworkBody::Full(bytes) = response.body else {
            panic!("expected buffered body")
        };
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("<head><title>Product</title></head><body>PRODUCT_BODY</body>"));
    }
}

#[tokio::test]
async fn client_payload_preserves_dynamic_title_status_cookies_and_cache_policy() {
    for send in [false, true] {
        let (release, body) = oneshot::channel();
        release.send(()).unwrap();
        let page = Document(ProductPage {
            body,
            fail_head: false,
            fail_body: false,
        });
        let route = if send {
            CoreRouteResponse::ssr_async_send(page)
        } else {
            CoreRouteResponse::ssr_async(page)
        }
        .with_head(proa_core::HeadConfig::default());
        let response = route
            .into_client_payload_framework_response(
                Some("stale static title".into()),
                &empty_manifest(),
                &Default::default(),
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED);
        assert_eq!(
            response.headers[http::header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(response.headers[http::header::CACHE_CONTROL], "private");
        assert_eq!(response.headers[http::header::SET_COOKIE], "page=1");
        let FrameworkBody::Full(bytes) = response.body else {
            panic!("expected JSON")
        };
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["title"], "Product");
        assert!(payload["html"]
            .as_str()
            .unwrap()
            .contains("<title>Product</title>"));
    }
}
struct SyncPage {
    clear: bool,
    status: StatusCode,
}
impl proa_core::WebRenderSync for SyncPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> impl proa_core::SyncHeadOperation {
        assert!(cx.response_mut().set_status(self.status));
        if self.clear {
            cx.head_mut().clear_title()
        } else {
            Ok(())
        }
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        proa_core::WebRenderSync::render(
            proa_macros::html_sync! {
                <head>{cx.head()}</head>
                <body>"body"</body>
            },
            cx,
        )
    }
}
#[tokio::test]
async fn sync_app_defaults_explicit_clear_and_manual_override() {
    let config = proa_core::HeadConfig::default()
        .with_defaults(|head| head.title("default title"))
        .unwrap();
    let request = proa_framework_core::FrameworkRequestContext {
        head: Some(config.into()),
        ..Default::default()
    };
    for clear in [false, true] {
        let route = CoreRouteResponse::ssr(SyncPage {
            clear,
            status: StatusCode::ACCEPTED,
        });
        let response = route
            .into_client_payload_framework_response(
                Some("legacy title".into()),
                &empty_manifest(),
                &request,
            )
            .await;
        assert_eq!(response.status, StatusCode::ACCEPTED);
        let FrameworkBody::Full(bytes) = response.body else {
            panic!("expected JSON")
        };
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if clear {
            assert!(payload.get("title").is_none());
            assert!(!payload["html"].as_str().unwrap().contains("<title>"));
        } else {
            assert_eq!(payload["title"], "default title");
        }
    }
    let response = CoreRouteResponse::ssr("manual")
        .without_head()
        .into_framework_response(&empty_manifest(), &request)
        .await
        .unwrap();
    let FrameworkBody::Full(bytes) = response.body else {
        panic!("expected buffered")
    };
    assert_eq!(&bytes[..], b"manual");
}
#[tokio::test]
async fn managed_buffered_and_client_paths_enforce_bodyless_status() {
    for status in [
        StatusCode::NO_CONTENT,
        StatusCode::RESET_CONTENT,
        StatusCode::NOT_MODIFIED,
    ] {
        for client in [false, true] {
            let route = CoreRouteResponse::ssr(SyncPage {
                clear: false,
                status,
            })
            .with_head(proa_core::HeadConfig::default());
            let response = if client {
                route
                    .into_client_payload_framework_response(
                        None,
                        &empty_manifest(),
                        &Default::default(),
                    )
                    .await
            } else {
                route
                    .into_framework_response(&empty_manifest(), &Default::default())
                    .await
                    .unwrap()
            };
            assert_eq!(response.status, status);
            let FrameworkBody::Full(bytes) = response.body else {
                panic!("expected buffered")
            };
            assert!(bytes.is_empty());
        }
    }
}
#[tokio::test]
async fn deferred_head_cannot_weaken_middleware_cache_bypass() {
    let (release, body) = oneshot::channel();
    release.send(()).unwrap();
    let route = CoreRouteResponse::ssr_stream(Document(ProductPage {
        body,
        fail_head: false,
        fail_body: false,
    }))
    .with_head(proa_core::HeadConfig::default());
    let response = route
        .into_stream_framework_response(&empty_manifest(), &Default::default())
        .unwrap();
    let mut response = framework_response_to_axum(response);
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        http::header::VARY,
        HeaderValue::from_static("Accept-Encoding"),
    );
    let response = prepare_deferred_framework_stream(response, executor()).await;
    assert_eq!(
        response.headers()[http::header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()[http::header::VARY], "Accept-Encoding");
}

struct SeedChild;
impl WebRender for SeedChild {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            cx.rsjs_query_seed_json(r#"["managed-stock"]"#, r#"{"data":42}"#);
            cx.output_mut().extend(b"deferred-result")
        })
    }
}
struct SeedPage;
impl WebRender for SeedPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <html>
                <head>{cx.head()}</head>
                <body>{proa_stream::Suspense::new(SeedChild, "loading")}</body>
            </html>
        }
        .render(cx)
    }
}
#[tokio::test]
async fn managed_ooo_keeps_rsjs_seed_before_boundary_swap_after_shell() {
    let route = CoreRouteResponse::ssr_stream(SeedPage)
        .with_render_mode(proa_core::RenderMode::ChunkedOutOfOrder)
        .with_head(proa_core::HeadConfig::default());
    let response = route
        .into_stream_framework_response(&empty_manifest(), &Default::default())
        .unwrap();
    let response =
        prepare_deferred_framework_stream(framework_response_to_axum(response), executor()).await;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let html = std::str::from_utf8(&bytes).unwrap();
    let shell_end = html.find("</html>").unwrap();
    let seed = html.find("managed-stock").unwrap();
    let swap = html.find("<template id=\"proa-tpl-").unwrap();
    assert!(shell_end < seed && seed < swap, "{html}");
    use base64::Engine as _;
    let encoded = html[swap..]
        .split_once('>')
        .unwrap()
        .1
        .split_once("</template>")
        .unwrap()
        .0;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap(),
        b"deferred-result"
    );
    assert_eq!(html.matches("managed-stock").count(), 1);
}

#[tokio::test]
async fn axum_context_layer_supplies_defaults_for_document_and_client_requests() {
    use tower::ServiceExt;
    let head = proa_core::HeadConfig::default()
        .with_defaults(|head| head.title("configured title"))
        .unwrap();
    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(|| async {
                RouteResponse::ssr(SyncPage {
                    clear: false,
                    status: StatusCode::ACCEPTED,
                })
            }),
        )
        .layer(
            crate::RouteContextLayer::new(Arc::new(empty_manifest()), Arc::new(Default::default()))
                .with_head(head),
        );
    for client in [false, true] {
        let mut request = http::Request::builder().uri("/");
        if client {
            request = request.header("X-Proa-Client", "true");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        if client {
            let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(payload["title"], "configured title");
        } else {
            assert!(std::str::from_utf8(&bytes)
                .unwrap()
                .contains("<head><title>configured title</title></head>"));
        }
    }
}
