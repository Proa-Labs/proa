// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderValue, Request, StatusCode},
    routing::get,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use http_body_util::BodyExt as _;
use proa_core::{
    await_component, static_html, to_async, DataLoader, ErrorBoundary, LoadError, LoadKey,
    LoadRequest, LoadValue, NoLoader, Priority, RenderBuffer, RenderMode, RenderOutcome,
    SendWebContext, WebContext, WebRender, WebRenderSend, WebRenderSync, WriteBuf, WriteError,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{
    router_from_spec, CspNonce, LocalStreamExecutorConfig, Route, RouteContextLayer, RouteResponse,
};
use proa_macros::{html, html_send, html_sync};
use proa_stream::Suspense;
use tower::ServiceExt;
use tower_http::set_header::SetResponseHeaderLayer;

fn decoded_boundary_payloads(document: &str) -> Vec<String> {
    const TEMPLATE_OPEN: &str = "<template id=\"proa-tpl-";
    const ENCODING_END: &str = "\" data-proa-encoding=\"base64-v1\">";
    const TEMPLATE_CLOSE: &str = "</template>";

    let mut document = document;
    let mut payloads = Vec::new();
    while let Some(template_at) = document.find(TEMPLATE_OPEN) {
        let template = &document[template_at..];
        let payload_at = template
            .find(ENCODING_END)
            .expect("boundary template must declare its transport encoding")
            + ENCODING_END.len();
        let payload_end = template[payload_at..]
            .find(TEMPLATE_CLOSE)
            .expect("boundary template must close before its swap script");
        let encoded = &template[payload_at..payload_at + payload_end];
        payloads.push(
            String::from_utf8(STANDARD.decode(encoded).expect("valid boundary base64"))
                .expect("rendered boundary must be UTF-8"),
        );
        document = &template[payload_at + payload_end + TEMPLATE_CLOSE.len()..];
    }
    payloads
}

fn empty_manifest() -> Arc<IslandManifest> {
    Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    })
}

struct DelayedSection {
    label: &'static str,
    delay_ms: u64,
}

struct LoadedSection {
    label: &'static str,
    load_key: &'static str,
}

struct ReadySection {
    label: &'static str,
}

struct ImmediateAsyncSection {
    label: &'static str,
}

struct NeverSection;

struct FailingSection;

struct StreamingResponseContextProbe;

struct SendDelayedSection;

struct NestedLocalParent;

struct SendNestedParent;

struct SendNestedReady;

struct NeverWithDrop {
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

struct DropSignal(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl WebRenderSend for SendDelayedSection {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            cx.out.extend_static(b"<section>send ready</section>")
        })
    }
}

impl WebRender<NoLoader, RenderBuffer> for NestedLocalParent {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        WebRender::render(
            html! {
                <article>
                    {Suspense::new(
                        to_async(ReadySection { label: "nested local ready" }),
                        html_sync! { <p>"nested local fallback"</p> },
                    )}
                </article>
            },
            cx,
        )
    }
}

impl WebRenderSend for SendNestedReady {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            cx.out.extend_static(b"<strong>nested send ready</strong>"),
        )
    }
}

impl WebRenderSend for SendNestedParent {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        WebRenderSend::render_send(
            html_send! {
                <article>
                    {Suspense::new(
                        SendNestedReady,
                        html_send! { <p>"nested send fallback"</p> },
                    )}
                </article>
            },
            cx,
        )
    }
}

impl WebRender<NoLoader, RenderBuffer> for NeverWithDrop {
    fn render(
        self,
        _cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _signal = DropSignal(self.dropped);
            std::future::pending().await
        })
    }
}

struct SendStreamingPage;

impl WebRenderSend for SendStreamingPage {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        WebRenderSend::render_send(
            html_send! {
                <main>
                    {Suspense::new(
                        SendDelayedSection,
                        html_send! { <p>"send fallback"</p> },
                    )}
                </main>
            },
            cx,
        )
    }
}

#[derive(Default)]
struct ConcurrencyTrackingLoader {
    active: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl<L: ::proa_core::DataLoader, B: proa_core::WriteBuf> WebRenderSync<L, B> for ReadySection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section class="ready">{self.label}</section>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for FailingSection {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            WriteBuf::extend(&mut cx.out, b"<section class=\"partial\">partial</section>")?;
            Err(WriteError::render("section failed"))
        })
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for StreamingResponseContextProbe {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let can_mutate = if cx.response().can_mutate_headers() {
            "yes"
        } else {
            "no"
        };
        let status_set = if cx.response_mut().set_status(StatusCode::CREATED) {
            "yes"
        } else {
            "no"
        };
        let header_set = if cx.response_mut().insert_header(
            http::HeaderName::from_static("x-stream-response-context"),
            http::HeaderValue::from_static("set"),
        ) {
            "yes"
        } else {
            "no"
        };
        let res = WebRenderSync::render(
            html_sync! {
                <section
                    data-can-mutate={can_mutate}
                    data-status-set={status_set}
                    data-header-set={header_set}
                >
                    "stream response context"
                </section>
            },
            cx,
        );
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(res)
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for ImmediateAsyncSection
where
    ReadySection: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            WebRenderSync::render(ReadySection { label: self.label }, cx)
        })
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for NeverSection {
    fn render(
        self,
        _cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(std::future::pending())
    }
}

impl DataLoader for ConcurrencyTrackingLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let delay_ms = match request.key().kind() {
            proa_core::LoadKeyKind::Named(value) if value == "slow" => 40,
            proa_core::LoadKeyKind::Named(value) if value == "fast" => 10,
            _ => 5,
        };

        let calls = Arc::clone(&self.calls);
        let active = Arc::clone(&self.active);
        let max_active = Arc::clone(&self.max_active);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
            let mut observed = max_active.load(Ordering::SeqCst);
            while cur > observed {
                match max_active.compare_exchange(observed, cur, Ordering::SeqCst, Ordering::SeqCst)
                {
                    Ok(_) => break,
                    Err(next) => observed = next,
                }
            }

            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(LoadValue::bytes(bytes::Bytes::from_static(b"null")))
        }
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for DelayedSection
where
    ReadySection: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let label = self.label;
        let delay_ms = self.delay_ms;
        await_component(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            to_async(ReadySection { label })
        })
        .render(cx)
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for LoadedSection
where
    ReadySection: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let label = self.label;
        let load_key = self.load_key;
        RenderOutcome::pending(async move {
            let (): () = cx.load_json(LoadKey::named("panel", load_key)).await?;
            proa_core::render_child(cx, to_async(ReadySection { label })).await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_flushes_shell_and_resolved_boundaries() {
    let routes = vec![Route::endpoint(
        "/stream",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        DelayedSection {
                            label: "slow ready",
                            delay_ms: 40,
                        },
                        html_sync! { <p class="fallback">"loading slow"</p> },
                    )}
                    {Suspense::new(
                        DelayedSection {
                            label: "fast ready",
                            delay_ms: 5,
                        },
                        html_sync! { <p class="fallback">"loading fast"</p> },
                    )}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-content-type-options")
            .and_then(|h| h.to_str().ok()),
        Some("nosniff")
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("<h1>stream</h1>"));
    assert!(html.contains("loading slow"));
    assert!(html.contains("loading fast"));
    assert!(html.contains("<!--proa:start:"));
    assert!(html.contains("<!--proa:end:"));
    assert!(html.contains("<template id=\"proa-tpl-"));
    assert!(html.contains("$proaBatch(["));

    let slow_fallback = html.find("loading slow").unwrap();
    let fast_fallback = html.find("loading fast").unwrap();
    let payloads = decoded_boundary_payloads(&html);

    assert!(slow_fallback < fast_fallback);
    assert_eq!(
        payloads.len(),
        2,
        "expected one payload per boundary: {html}"
    );
    assert!(
        payloads[0].contains("fast ready") && payloads[1].contains("slow ready"),
        "faster boundary should flush before slower boundary: {payloads:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_stream_bypasses_an_unavailable_local_executor() {
    let routes = vec![Route::endpoint(
        "/send-stream",
        get(|| async { RouteResponse::ssr_stream_send(SendStreamingPage) }),
    )];
    let (router, computed_metadata) = router_from_spec(routes);
    let layer = RouteContextLayer::new(empty_manifest(), computed_metadata)
        .with_local_stream_executor_config(LocalStreamExecutorConfig {
            worker_threads: 0,
            ..LocalStreamExecutorConfig::default()
        });

    let response = router
        .layer(layer)
        .oneshot(
            Request::builder()
                .uri("/send-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("send fallback"));
    assert_eq!(
        decoded_boundary_payloads(&html),
        ["<section>send ready</section>"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_local_boundaries_are_admitted_only_after_the_parent_is_published() {
    let routes = vec![Route::endpoint(
        "/nested-local",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    {Suspense::new(
                        NestedLocalParent,
                        html_sync! { <p>"outer local fallback"</p> },
                    )}
                </main>
            })
        }),
    )];
    let (router, metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(empty_manifest(), metadata));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/nested-local")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body();

    let shell = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let parent = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let nested = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(body.frame().await.is_none());

    let shell = String::from_utf8(shell.to_vec()).unwrap();
    let parent = String::from_utf8(parent.to_vec()).unwrap();
    let nested = String::from_utf8(nested.to_vec()).unwrap();
    assert!(shell.contains("outer local fallback"));
    assert!(!shell.contains("nested local fallback"));
    assert_eq!(decoded_boundary_payloads(&parent).len(), 1);
    assert!(decoded_boundary_payloads(&parent)[0].contains("nested local fallback"));
    assert!(decoded_boundary_payloads(&nested)[0].contains("nested local ready"));
    assert!(parent.contains(">$proaBatch([1])"));
    assert!(nested.contains(">$proaBatch([2])"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_send_boundaries_preserve_parent_before_child_wire_order() {
    let routes = vec![Route::endpoint(
        "/nested-send",
        get(|| async {
            RouteResponse::ssr_stream_send(html_send! {
                <main>
                    {Suspense::new(
                        SendNestedParent,
                        html_send! { <p>"outer send fallback"</p> },
                    )}
                </main>
            })
        }),
    )];
    let (router, metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(empty_manifest(), metadata));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/nested-send")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body();

    let shell = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let parent = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let nested = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(body.frame().await.is_none());

    let shell = String::from_utf8(shell.to_vec()).unwrap();
    let parent = String::from_utf8(parent.to_vec()).unwrap();
    let nested = String::from_utf8(nested.to_vec()).unwrap();
    assert!(shell.contains("outer send fallback"));
    assert!(!shell.contains("nested send fallback"));
    assert!(decoded_boundary_payloads(&parent)[0].contains("nested send fallback"));
    assert!(decoded_boundary_payloads(&nested)[0].contains("nested send ready"));
    assert!(parent.contains(">$proaBatch([1])"));
    assert!(nested.contains(">$proaBatch([2])"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_priority_orders_boundaries_ready_in_the_same_turn() {
    let routes = vec![Route::endpoint(
        "/stream-priority",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    {Suspense {
                        child: ImmediateAsyncSection {
                            label: "resolved defer",
                        },
                        fallback: html_sync! { <p>"fallback defer"</p> },
                        priority: Priority::Defer,
                        timeout: None,
                    }}
                    {Suspense {
                        child: ImmediateAsyncSection {
                            label: "resolved critical",
                        },
                        fallback: html_sync! { <p>"fallback critical"</p> },
                        priority: Priority::Critical,
                        timeout: None,
                    }}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-priority")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let payloads = decoded_boundary_payloads(&html);
    assert!(
        payloads[0].contains("resolved critical") && payloads[1].contains("resolved defer"),
        "critical must win the ready-turn tie-break: {payloads:?}"
    );
    assert_eq!(
        html.matches(">$proaBatch([").count(),
        1,
        "same-poll completions must share one browser application call: {html}"
    );
    assert!(html.contains(">$proaBatch([2,1])"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_timeout_cancels_a_never_resolving_boundary() {
    let routes = vec![Route::endpoint(
        "/stream-timeout",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    {Suspense {
                        child: NeverSection,
                        fallback: html_sync! { <p>"timeout fallback"</p> },
                        priority: Priority::Optional,
                        timeout: Some(Duration::from_millis(5)),
                    }}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-timeout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), usize::MAX),
    )
    .await
    .expect("a timed boundary must not hold the response open")
    .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("timeout fallback"));
    assert!(html.contains("<!--proa:timeout:"));
    assert!(!html.contains("<template id=\"proa-tpl-"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_boundary_timeout_progresses_while_client_output_is_backpressured() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let routes = vec![Route::endpoint(
        "/stream-timeout-backpressured",
        get({
            let dropped = Arc::clone(&dropped);
            move || {
                let dropped = Arc::clone(&dropped);
                async move {
                    RouteResponse::ssr_stream(html! {
                        <main>
                            {Suspense::new(
                                ImmediateAsyncSection { label: "first ready" },
                                html_sync! { <p>"first fallback"</p> },
                            )}
                            {Suspense::new(
                                DelayedSection { label: "second ready", delay_ms: 5 },
                                html_sync! { <p>"second fallback"</p> },
                            )}
                            {Suspense {
                                child: NeverWithDrop { dropped },
                                fallback: html_sync! { <p>"stalled fallback"</p> },
                                priority: Priority::Optional,
                                timeout: Some(Duration::from_millis(25)),
                            }}
                        </main>
                    })
                }
            }
        }),
    )];
    let (router, metadata) = router_from_spec(routes);
    let app = router.layer(
        RouteContextLayer::new(empty_manifest(), metadata).with_local_stream_executor_config(
            LocalStreamExecutorConfig {
                worker_threads: 1,
                max_in_flight: 4,
                response_buffer_bytes: 1,
                response_buffer_chunks: 1,
                cooperative_yield_interval: 1,
            },
        ),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-timeout-backpressured")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Keep the body alive but deliberately do not poll it. The first ready
    // boundary fills the one-chunk/one-byte transport budget; the second then
    // blocks publication. Timeout supervision must still drop the stalled
    // child independently of that client-side backpressure.
    let body = response.into_body();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !dropped.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("timed boundary must be cancelled while response output is blocked");
    drop(body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_order_stream_documents_priority_and_timeout_as_source_order_no_ops() {
    let routes = vec![Route::endpoint(
        "/stream-in-order-options",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    {Suspense {
                        child: ImmediateAsyncSection {
                            label: "in-order child",
                        },
                        fallback: html_sync! { <p>"unused fallback"</p> },
                        priority: Priority::Defer,
                        timeout: Some(Duration::ZERO),
                    }}
                </main>
            })
            .with_render_mode(RenderMode::ChunkedInOrder)
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-in-order-options")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(
        html,
        "<main><section class=\"ready\">in-order child</section></main>"
    );
    assert!(!html.contains("proa:timeout"));
    assert!(!html.contains("unused fallback"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_without_pending_boundaries_skips_runtime_script() {
    let routes = vec![Route::endpoint(
        "/plain",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <p>"plain"</p>
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/plain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(html, "<main><p>plain</p></main>");
    assert!(!html.contains("$proaBatch(["));
    assert!(!html.contains("proa-tpl-"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_csp_nonce_covers_stream_runtime_and_boundary_swaps() {
    const NONCE: &str = "c3RyaWN0LWNzcC10ZXN0";
    const POLICY: &str = "default-src 'self'; script-src 'self' 'nonce-c3RyaWN0LWNzcC10ZXN0'";

    let routes = vec![Route::endpoint(
        "/strict-csp",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    {Suspense::new(
                        DelayedSection {
                            label: "nonced",
                            delay_ms: 1,
                        },
                        html_sync! { <p>"loading"</p> },
                    )}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router
        .layer(RouteContextLayer::new(manifest, computed_metadata))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(POLICY),
        ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/strict-csp")
                .extension(CspNonce::new(NONCE).unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|value| value.to_str().ok()),
        Some(POLICY)
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let script_count = html.matches("<script").count();
    assert!(
        script_count >= 2,
        "expected runtime and boundary scripts: {html}"
    );
    assert_eq!(
        html.matches(&format!(r#"nonce="{NONCE}""#)).count(),
        script_count,
        "every script must carry the request nonce: {html}"
    );
    assert!(html.contains(&format!(r#"<script nonce="{NONCE}">(function()"#)));
    assert!(html.contains(&format!(r#"<script nonce="{NONCE}">$proaBatch(["#)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_marks_response_context_committed_before_render() {
    let routes = vec![
        Route::endpoint(
            "/stream-response-context",
            get(|| async { RouteResponse::ssr_stream(StreamingResponseContextProbe) }),
        ),
        Route::endpoint(
            "/stream-response-context-in-order",
            get(|| async {
                RouteResponse::ssr_stream(StreamingResponseContextProbe)
                    .with_render_mode(RenderMode::ChunkedInOrder)
            }),
        ),
    ];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    for path in [
        "/stream-response-context",
        "/stream-response-context-in-order",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("x-stream-response-context")
            .is_none());
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            String::from_utf8(body.to_vec()).unwrap(),
            "<section data-can-mutate=\"no\" data-status-set=\"no\" data-header-set=\"no\">stream response context</section>"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_in_order_emits_pure_html_chunks() {
    let routes = vec![Route::endpoint(
        "/stream-in-order",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        DelayedSection {
                            label: "slow ready",
                            delay_ms: 25,
                        },
                        html_sync! { <p class="fallback">"loading slow"</p> },
                    )}
                    {Suspense::new(
                        DelayedSection {
                            label: "fast ready",
                            delay_ms: 5,
                        },
                        html_sync! { <p class="fallback">"loading fast"</p> },
                    )}
                </main>
            })
            .with_render_mode(RenderMode::ChunkedInOrder)
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-in-order")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body();
    let mut chunks = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        chunks.push(String::from_utf8(data.to_vec()).unwrap());
    }

    assert!(
        chunks.len() >= 3,
        "in-order mode should flush multiple pure HTML chunks"
    );
    assert!(chunks[0].contains("<h1>stream</h1>"));
    assert!(!chunks[0].contains("slow ready"));

    let html = chunks.concat();
    assert_eq!(
        html,
        "<main><h1>stream</h1><section class=\"ready\">slow ready</section><section class=\"ready\">fast ready</section></main>"
    );
    assert!(!html.contains("$proaBatch(["));
    assert!(!html.contains("proa-tpl-"));
    assert!(!html.contains("loading slow"));
    assert!(!html.contains("loading fast"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_out_of_order_error_boundary_replaces_failed_boundary() {
    let routes = vec![Route::endpoint(
        "/stream-error-boundary",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {ErrorBoundary::new(
                        Suspense::new(
                            FailingSection,
                            html_sync! { <p class="fallback">"loading failed section"</p> },
                        ),
                        static_html(b"<section class=\"error\">error fallback</section>"),
                    )}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-error-boundary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let payloads = decoded_boundary_payloads(&html);

    assert!(html.contains("loading failed section"));
    assert_eq!(payloads.len(), 1);
    assert!(payloads[0].contains("error fallback"));
    assert!(html.contains("<template id=\"proa-tpl-"));
    assert!(!payloads[0].contains("partial"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_in_order_error_boundary_emits_fallback_and_continues() {
    let routes = vec![Route::endpoint(
        "/stream-error-boundary-in-order",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {ErrorBoundary::new(
                        Suspense::new(
                            FailingSection,
                            html_sync! { <p class="fallback">"loading failed section"</p> },
                        ),
                        static_html(b"<section class=\"error\">error fallback</section>"),
                    )}
                    <p>"after"</p>
                </main>
            })
            .with_render_mode(RenderMode::ChunkedInOrder)
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-error-boundary-in-order")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(
        html,
        "<main><h1>stream</h1><section class=\"error\">error fallback</section><p>after</p></main>"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_out_of_order_without_error_boundary_blanks_failed_boundary() {
    let routes = vec![Route::endpoint(
        "/stream-error-no-boundary",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        FailingSection,
                        html_sync! { <p class="fallback">"loading failed section"</p> },
                    )}
                </main>
            })
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-error-no-boundary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let payloads = decoded_boundary_payloads(&html);

    assert!(html.contains("loading failed section"));
    assert_eq!(payloads, [""]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_in_order_without_error_boundary_returns_clean_500() {
    let routes = vec![Route::endpoint(
        "/stream-error-no-boundary-in-order",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        FailingSection,
                        html_sync! { <p class="fallback">"loading failed section"</p> },
                    )}
                    <p>"after"</p>
                </main>
            })
            .with_render_mode(RenderMode::ChunkedInOrder)
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-error-no-boundary-in-order")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(html, "render error");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_in_order_preloads_known_loader_keys() {
    let loader = Arc::new(ConcurrencyTrackingLoader::default());
    let routes = vec![Route::endpoint(
        "/stream-in-order-preloaded",
        get({
            let loader = Arc::clone(&loader);
            move || {
                let loader = Arc::clone(&loader);
                async move {
                    // Typed-L path: wrap in `PreparedLoader<L>` for in-order
                    // warm-up and a `DataResolver<L>` for dedupe. Both stay
                    // monomorphized on the concrete loader type.
                    let preloaded = std::sync::Arc::new(proa_stream::PreparedLoader::new_typed(
                        std::sync::Arc::new(proa_stream::DataResolver::new_typed(loader)),
                        [
                            LoadKey::named("panel", "slow"),
                            LoadKey::named("panel", "fast"),
                        ],
                    ));
                    RouteResponse::ssr_stream_with_loader(
                        html! {
                            <main>
                                <h1>"stream"</h1>
                                {Suspense::new(
                                    LoadedSection {
                                        label: "slow ready",
                                        load_key: "slow",
                                    },
                                    html_sync! { <p class="fallback">"loading slow"</p> },
                                )}
                                {Suspense::new(
                                    LoadedSection {
                                        label: "fast ready",
                                        load_key: "fast",
                                    },
                                    html_sync! { <p class="fallback">"loading fast"</p> },
                                )}
                            </main>
                        },
                        preloaded,
                    )
                    .with_render_mode(RenderMode::ChunkedInOrder)
                }
            }
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-in-order-preloaded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(
        html,
        "<main><h1>stream</h1><section class=\"ready\">slow ready</section><section class=\"ready\">fast ready</section></main>"
    );
    assert!(!html.contains("$proaBatch(["));
    assert!(!html.contains("proa-tpl-"));
    assert!(!html.contains("loading slow"));
    assert!(!html.contains("loading fast"));
    assert!(
        loader.max_active.load(Ordering::SeqCst) >= 2,
        "preloaded in-order keys should overlap instead of serializing"
    );
    assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_out_of_order_dedupes_duplicate_loader_keys() {
    let loader = Arc::new(ConcurrencyTrackingLoader::default());
    let routes = vec![Route::endpoint(
        "/stream-shared",
        get({
            let loader = Arc::clone(&loader);
            move || {
                let loader = Arc::clone(&loader);
                async move {
                    let typed_loader =
                        std::sync::Arc::new(proa_stream::DataResolver::new_typed(loader));
                    RouteResponse::ssr_stream_with_loader(
                        html! {
                            <main>
                                {Suspense::new(
                                    LoadedSection {
                                        label: "first ready",
                                        load_key: "shared",
                                    },
                                    html_sync! { <p class="fallback">"loading first"</p> },
                                )}
                                {Suspense::new(
                                    LoadedSection {
                                        label: "second ready",
                                        load_key: "shared",
                                    },
                                    html_sync! { <p class="fallback">"loading second"</p> },
                                )}
                            </main>
                        },
                        typed_loader,
                    )
                }
            }
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-shared")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let payloads = decoded_boundary_payloads(&html);

    assert!(payloads
        .iter()
        .any(|payload| payload.contains("first ready")));
    assert!(payloads
        .iter()
        .any(|payload| payload.contains("second ready")));
    assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_stream_in_order_preload_and_render_share_one_loader_call() {
    let loader = Arc::new(ConcurrencyTrackingLoader::default());
    let routes = vec![Route::endpoint(
        "/stream-shared-in-order",
        get({
            let loader = Arc::clone(&loader);
            move || {
                let loader = Arc::clone(&loader);
                async move {
                    let preloaded = std::sync::Arc::new(proa_stream::PreparedLoader::new_typed(
                        std::sync::Arc::new(proa_stream::DataResolver::new_typed(loader)),
                        [LoadKey::named("panel", "shared")],
                    ));
                    RouteResponse::ssr_stream_with_loader(
                        html! {
                            <main>
                                {Suspense::new(
                                    LoadedSection {
                                        label: "first ready",
                                        load_key: "shared",
                                    },
                                    html_sync! { <p class="fallback">"loading first"</p> },
                                )}
                                {Suspense::new(
                                    LoadedSection {
                                        label: "second ready",
                                        load_key: "shared",
                                    },
                                    html_sync! { <p class="fallback">"loading second"</p> },
                                )}
                            </main>
                        },
                        preloaded,
                    )
                    .with_render_mode(RenderMode::ChunkedInOrder)
                }
            }
        }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream-shared-in-order")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(
        html,
        "<main><section class=\"ready\">first ready</section><section class=\"ready\">second ready</section></main>"
    );
    assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
}
