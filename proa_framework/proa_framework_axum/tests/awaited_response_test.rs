// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Middleware between a handler and `RouteContextLayer` sees the rendered
//! page when the handler awaits its `RouteResponse`.
//!
//! Each scenario once broke because those layers saw an empty placeholder:
//! a nested Router's `Content-Length: 0`, an inner `if_not_present` header
//! beating the page's own, page-cache header removals, inner middleware that
//! no longer wrapped the render, and a pool worker joining itself on drop.
//! Awaited renders cover all of them. Where an un-awaited response must keep
//! working, the current-thread placeholder and the multi-thread in-place
//! render are checked too.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderName, HeaderValue, Request, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use proa_core::render::RenderOutcome;
use proa_core::{SendWebContext, WebContext, WebRender, WebRenderSend, WriteBuf, WriteError};
use proa_framework_axum::{
    router_from_spec, PageCache, PageCacheLayer, Route, RouteContextLayer, RouteResponse,
};
use tower::{Layer as _, ServiceBuilder, ServiceExt};
use tower_http::set_header::SetResponseHeaderLayer;

tokio::task_local! {
    /// Set by middleware around the handler.
    static REQUEST_TAG: u32;
}

fn request_tag() -> u32 {
    REQUEST_TAG.try_with(|tag| *tag).unwrap_or(0)
}

const SURROGATE_CONTROL: HeaderName = HeaderName::from_static("surrogate-control");

/// Chooses its own cache policy after an await and reports the middleware
/// task-local it can see.
struct PolicyPage;

fn stage_policy(response: &mut proa_core::ResponseContext) {
    response.insert_header(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.insert_header(SURROGATE_CONTROL, HeaderValue::from_static("max-age=600"));
}

impl WebRender for PolicyPage {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            stage_policy(cx.response_mut());
            WriteBuf::extend_str(&mut cx.out, &format!("<p>tag={}</p>", request_tag()))
        })
    }
}

impl WebRenderSend for PolicyPage {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            stage_policy(cx.response_mut());
            cx.output_mut()
                .extend_str(&format!("<p>tag={}</p>", request_tag()))
        })
    }
}

/// Fails after an await.
struct Fails;

impl WebRender for Fails {
    fn render(
        self,
        _: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async {
            tokio::task::yield_now().await;
            Err(WriteError::render("private diagnostic"))
        })
    }
}

impl WebRenderSend for Fails {
    fn render_send(
        self,
        _: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async {
            tokio::task::yield_now().await;
            Err(WriteError::render("private diagnostic"))
        })
    }
}

/// Takes longer than the short timeouts below.
struct Slow;

impl WebRender for Slow {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            WriteBuf::extend_str(&mut cx.out, "<p>slow</p>")
        })
    }
}

impl WebRenderSend for Slow {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cx.output_mut().extend_str("<p>slow</p>")
        })
    }
}

/// `/local` and `/send` await the page; `/unawaited` returns it.
fn pages<P>(page: fn() -> P) -> Router
where
    P: WebRender + WebRenderSend + Send + 'static,
{
    Router::new()
        .route(
            "/local",
            get(move || async move { RouteResponse::ssr_async(page()).await }),
        )
        .route(
            "/send",
            get(move || async move { RouteResponse::ssr_async_send(page()).await }),
        )
        .route(
            "/unawaited",
            get(move || async move { RouteResponse::ssr_async(page()) }),
        )
}

const AWAITED: [&str; 2] = ["/local", "/send"];
const ALL: [&str; 3] = ["/local", "/send", "/unawaited"];

fn context() -> RouteContextLayer {
    RouteContextLayer::new(Default::default())
}

fn request(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

async fn fetch<S>(service: S, uri: &str) -> Response
where
    S: tower::Service<Request<Body>, Response = Response> + Send,
    S::Future: Send,
    S::Error: std::fmt::Debug,
{
    tokio::time::timeout(Duration::from_secs(5), service.oneshot(request(uri)))
        .await
        .expect("response must not hang")
        .unwrap()
}

/// The body, after checking that any declared length matches it.
async fn framed_body(response: Response) -> String {
    let declared = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .map(|value| value.to_str().unwrap().parse::<usize>().unwrap());
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    if let Some(declared) = declared {
        assert_eq!(declared, body.len(), "Content-Length must match the body");
    }
    String::from_utf8(body.to_vec()).unwrap()
}

// 1. A Router mounted as a service stamps `Content-Length` from the body it
//    sees. It must describe the page, never an empty placeholder.

async fn nested_routers_frame_the_rendered_body() {
    let mounts: [(&str, Router, &str); 2] = [
        (
            "nest_service",
            Router::new()
                .nest_service("/n", pages(|| PolicyPage))
                .layer(context()),
            "/n",
        ),
        (
            "fallback_service",
            Router::new()
                .fallback_service(pages(|| PolicyPage))
                .layer(context()),
            "",
        ),
    ];
    for (mount, app, prefix) in mounts {
        for uri in ALL {
            let response = fetch(app.clone(), &format!("{prefix}{uri}")).await;
            assert_eq!(response.status(), StatusCode::OK, "{mount} {uri}");
            assert_eq!(framed_body(response).await, "<p>tag=0</p>", "{mount} {uri}");
        }
    }
    let wrapped = context().layer(pages(|| PolicyPage));
    let built = ServiceBuilder::new()
        .layer(context())
        .service(pages(|| PolicyPage));
    for uri in ALL {
        let response = fetch(wrapped.clone(), uri).await;
        assert_eq!(framed_body(response).await, "<p>tag=0</p>", "layer {uri}");
        let response = fetch(built.clone(), uri).await;
        assert_eq!(framed_body(response).await, "<p>tag=0</p>", "builder {uri}");
    }
    // A nested stream's preparation response is empty as well.
    let streamed = Router::new()
        .nest_service(
            "/n",
            Router::new().route(
                "/",
                get(|| async { RouteResponse::ssr_stream(PolicyPage).await }),
            ),
        )
        .layer(context());
    let response = fetch(streamed, "/n").await;
    assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
    assert_eq!(framed_body(response).await, "<p>tag=0</p>");
}

#[tokio::test]
async fn nested_routers_frame_the_rendered_body_on_a_current_thread_runtime() {
    nested_routers_frame_the_rendered_body().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_routers_frame_the_rendered_body_on_a_multi_thread_runtime() {
    nested_routers_frame_the_rendered_body().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_nested_router_delivers_whole_pages() {
    let app = Router::new()
        .nest_service(
            "/n",
            pages(|| PolicyPage).route(
                "/stream",
                get(|| async { RouteResponse::ssr_stream(PolicyPage) }),
            ),
        )
        .layer(context());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // One client reuses a keep-alive connection, which a wrong length desyncs.
    let client = reqwest::Client::new();
    for _ in 0..2 {
        for uri in ["/n/local", "/n/send", "/n/unawaited", "/n/stream"] {
            let response = client
                .get(format!("http://{address}{uri}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.text().await.unwrap(), "<p>tag=0</p>", "{uri}");
        }
    }
    server.abort();
}

// 2. Middleware that sets a header only when it is absent sees the page's
//    own header and leaves it alone.

async fn inner_if_not_present_headers_yield_to_the_page() {
    let app = pages(|| PolicyPage)
        .route(
            "/default",
            get(|| async { RouteResponse::ssr(proa_core::text::text("page")).await }),
        )
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=600"),
        ))
        .layer(context());
    for uri in AWAITED {
        let response = fetch(app.clone(), uri).await;
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store",
            "{uri}"
        );
    }
    // A page without a policy still gets the middleware's.
    let response = fetch(app, "/default").await;
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=600"
    );
}

#[tokio::test]
async fn inner_if_not_present_headers_yield_to_the_page_on_a_current_thread_runtime() {
    inner_if_not_present_headers_yield_to_the_page().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_if_not_present_headers_yield_to_the_page_on_a_multi_thread_runtime() {
    inner_if_not_present_headers_yield_to_the_page().await;
    // Rendered in place, an un-awaited page is finished here too.
    let app = pages(|| PolicyPage)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=600"),
        ))
        .layer(context());
    let response = fetch(app, "/unawaited").await;
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
}

// 3. A page cache that bypasses a request removes downstream freshness
//    headers from the page, not from a placeholder.

async fn page_cache_bypass_strips_the_pages_freshness_headers() {
    let route = |path: &'static str, awaited: bool| {
        Route::endpoint(
            path,
            get(move || async move {
                if awaited {
                    RouteResponse::ssr_async(PolicyPage).await
                } else {
                    axum::response::IntoResponse::into_response(RouteResponse::ssr_async(
                        PolicyPage,
                    ))
                }
            }),
        )
    };
    let (router, metadata) = router_from_spec(vec![
        route("/origin/local", true).with_page_cache(PageCacheLayer::new("build")),
        route("/origin/unawaited", false).with_page_cache(PageCacheLayer::new("build")),
        route("/cdn/local", true).with_page_cache(PageCache::new()),
        route("/cdn/unawaited", false).with_page_cache(PageCache::new()),
    ]);
    let app = router.layer(RouteContextLayer::new(metadata));
    for uri in [
        "/origin/local",
        "/origin/unawaited",
        "/cdn/local",
        "/cdn/unawaited",
    ] {
        for cookie in [true, false] {
            let mut request = request(uri);
            if cookie {
                request
                    .headers_mut()
                    .insert(header::COOKIE, HeaderValue::from_static("session=abc"));
            }
            let response = app.clone().oneshot(request).await.unwrap();
            let headers = response.headers();
            assert_eq!(
                headers[header::CACHE_CONTROL],
                "private, no-store",
                "{uri} cookie={cookie}"
            );
            assert_ne!(
                headers.get(&SURROGATE_CONTROL).map(HeaderValue::as_bytes),
                Some(&b"max-age=600"[..]),
                "{uri} cookie={cookie}"
            );
            if uri.starts_with("/origin") {
                assert_eq!(headers["x-proa-page-cache"], "BYPASS", "{uri}");
            }
        }
    }
}

#[tokio::test]
async fn page_cache_bypass_strips_the_pages_freshness_headers_on_a_current_thread_runtime() {
    page_cache_bypass_strips_the_pages_freshness_headers().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_cache_bypass_strips_the_pages_freshness_headers_on_a_multi_thread_runtime() {
    page_cache_bypass_strips_the_pages_freshness_headers().await;
}

// 4. Middleware inside `RouteContextLayer` wraps the awaited render: it sees
//    the final status, including a render error, its timeouts bound the
//    render, and a `Send` render sees its task-locals.

async fn inner_middleware_wraps_the_awaited_render() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let app = pages(|| PolicyPage)
        .nest("/fails", pages(|| Fails))
        .layer(tower::util::MapResponseLayer::new(
            move |response: Response| {
                record.lock().unwrap().push(response.status());
                response
            },
        ))
        .layer(axum::middleware::from_fn(
            |request: Request<Body>, next: axum::middleware::Next| async move {
                REQUEST_TAG.scope(7, next.run(request)).await
            },
        ))
        .layer(context());
    for uri in AWAITED {
        let response = fetch(app.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let body = framed_body(response).await;
        if uri == "/send" {
            assert_eq!(
                body, "<p>tag=7</p>",
                "a Send render runs inside the middleware"
            );
        }
        let response = fetch(app.clone(), &format!("/fails{uri}")).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [
            StatusCode::OK,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::OK,
            StatusCode::INTERNAL_SERVER_ERROR,
        ]
    );

    #[allow(deprecated)] // `TimeoutLayer::new` is the only constructor in the locked tower-http.
    let timeout = tower_http::timeout::TimeoutLayer::new(Duration::from_millis(50));
    let app = pages(|| Slow).layer(timeout).layer(context());
    for uri in AWAITED {
        let response = fetch(app.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT, "{uri}");
    }
}

#[tokio::test]
async fn inner_middleware_wraps_the_awaited_render_on_a_current_thread_runtime() {
    inner_middleware_wraps_the_awaited_render().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_middleware_wraps_the_awaited_render_on_a_multi_thread_runtime() {
    inner_middleware_wraps_the_awaited_render().await;
}

// 5. Dropping the app while a local render is in flight releases the pool
//    without a worker joining itself.

static POOL_PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record_pool_panics() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            if thread
                .name()
                .is_some_and(|name| name.starts_with("proa-stream-"))
            {
                POOL_PANICS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(info.to_string());
            }
            previous(info);
        }));
    });
}

/// `uris` render on the pool: awaited local renders, and on a
/// current-thread runtime un-awaited ones too.
async fn dropping_the_app_mid_render_releases_the_pool(uris: &[&str]) {
    record_pool_panics();
    for uri in uris {
        let app = pages(|| Slow).layer(context());
        let cancelled =
            tokio::time::timeout(Duration::from_millis(20), app.clone().oneshot(request(uri)))
                .await;
        assert!(cancelled.is_err(), "{uri} should still be rendering");
        drop(app);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let panics = POOL_PANICS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(panics.is_empty(), "a pool worker panicked: {panics:?}");
}

#[tokio::test]
async fn dropping_the_app_mid_render_releases_the_pool_on_a_current_thread_runtime() {
    dropping_the_app_mid_render_releases_the_pool(&["/local", "/unawaited"]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_app_mid_render_releases_the_pool_on_a_multi_thread_runtime() {
    dropping_the_app_mid_render_releases_the_pool(&["/local"]).await;
}
