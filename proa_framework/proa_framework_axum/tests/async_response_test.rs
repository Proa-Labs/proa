// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for buffered async `RouteResponse` rendering.
//!
//! Drives async `WebRender` and `WebRenderSend` components through the
//! framework's `IntoResponse` impl end-to-end: build an axum router, install
//! `RouteContextLayer`, fire a `oneshot` request, and assert the rendered body
//! contains the bytes the component produced (including content written
//! *after* a real `.await`).
//!
//! Most tests use the default `#[tokio::test]` current-thread runtime, which
//! is what application tests reach for first: buffered finalization must
//! never block inside the handler's `into_response`.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderName, HeaderValue, Request, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use proa_core::render::RenderOutcome;
use proa_core::{
    await_component, render_to_string_async, to_async, DataLoader, LoadError, LoadKey, LoadRequest,
    LoadValue, RenderBuffer, SendWebContext, WebContext, WebRender, WebRenderSend, WriteBuf,
    WriteError,
};
use proa_framework_axum::{
    router_from_spec, PageCache, PageCacheLayer, Route, RouteContextLayer, RouteResponse,
};
use proa_macros::{html, html_sync};
use tower::ServiceExt;

/// An async component that writes an opening tag, yields to the runtime,
/// then writes a closing tag. The yield is load-bearing: if the framework
/// doesn't actually drive the future past the first `.await`, the closing
/// tag is never emitted and the test fails.
struct AsyncGreeting {
    name: &'static str,
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for AsyncGreeting {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            // Disambiguate: Vec<u8> has both std's Extend (infallible) and
            // our WriteBuf::extend (fallible). Call the trait form explicitly.
            WriteBuf::extend_str(&mut cx.out, "<p>hello ")?;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            WriteBuf::extend_str(&mut cx.out, self.name)?;
            WriteBuf::extend_str(&mut cx.out, "</p>")?;
            Ok(())
        })
    }
}

struct AsyncHtmlLeaf {
    label: &'static str,
}

struct SyncHtmlLeaf {
    label: &'static str,
}

struct LocationEcho;
struct RouteParamEcho;
struct RequestEcho;
struct ResponseContextEcho;

struct LoadedHtmlLeaf {
    label: &'static str,
    load_key: &'static str,
}

#[derive(Default)]
struct CountingLoader {
    calls: Arc<AtomicUsize>,
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for SyncHtmlLeaf {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_core::WebRenderSync::render(
            html_sync! {
                <span class="leaf">{self.label}</span>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for LocationEcho {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let location = cx
            .request_location()
            .expect("framework should install request location")
            .clone();
        let query = location.query().unwrap_or("");
        proa_core::WebRenderSync::render(
            html_sync! {
                <section data-path={location.path()} data-query={query}>{location.path()}</section>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for RouteParamEcho {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let id = cx.param("id").unwrap_or("").to_owned();
        proa_core::WebRenderSync::render(
            html_sync! {
                <section data-id={id.as_str()}>{id.as_str()}</section>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for RequestEcho {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let method = cx
            .request()
            .method()
            .map(http::Method::as_str)
            .unwrap_or("")
            .to_owned();
        let user_agent = cx.request().user_agent().unwrap_or("").to_owned();
        let theme = cx.request().cookie("theme").unwrap_or("").to_owned();
        proa_core::WebRenderSync::render(
            html_sync! {
                <section
                    data-method={method.as_str()}
                    data-agent={user_agent.as_str()}
                    data-theme={theme.as_str()}
                >
                    "request"
                </section>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B>
    for ResponseContextEcho
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.response_mut().set_status(StatusCode::CREATED);
        cx.response_mut().insert_header(
            http::HeaderName::from_static("x-render-context"),
            http::HeaderValue::from_static("set"),
        );
        proa_core::WebRenderSync::render(
            html_sync! {
                <p>"response"</p>
            },
            cx,
        )
    }
}

impl DataLoader for CountingLoader {
    fn load(
        &self,
        _request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let calls = Arc::clone(&self.calls);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            Ok(LoadValue::bytes(bytes::Bytes::from_static(b"null")))
        }
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for AsyncHtmlLeaf {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        await_component(async move {
            tokio::task::yield_now().await;
            to_async(SyncHtmlLeaf { label: self.label })
        })
        .render(cx)
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for LoadedHtmlLeaf {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let label = self.label;
        let load_key = self.load_key;
        RenderOutcome::pending(async move {
            let (): () = cx.load_json(LoadKey::named("panel", load_key)).await?;
            proa_core::render_child(cx, to_async(SyncHtmlLeaf { label })).await
        })
    }
}

#[tokio::test]
async fn ssr_async_renders_async_component() {
    let routes = vec![Route::endpoint(
        "/hello",
        get(|| async { RouteResponse::ssr_async(AsyncGreeting { name: "world" }) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    // Both pre-await and post-await bytes must be present; missing either
    // indicates the future was not driven to completion.
    assert_eq!(body.as_ref(), b"<p>hello world</p>");
}

#[tokio::test]
async fn ssr_async_sets_default_security_headers() {
    let routes = vec![Route::endpoint(
        "/hello",
        get(|| async { RouteResponse::ssr_async(AsyncGreeting { name: "security" }) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hello")
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
        Some("nosniff"),
        "buffered async SSR should emit the same default security headers as sync SSR"
    );
}

#[tokio::test]
async fn route_render_installs_request_location_on_context() {
    let routes = vec![Route::endpoint(
        "/dashboard",
        get(|| async { RouteResponse::ssr(LocationEcho) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/dashboard?tab=analytics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(body.to_vec()).unwrap(),
        "<section data-path=\"/dashboard\" data-query=\"tab=analytics\">/dashboard</section>"
    );
}

#[tokio::test]
async fn route_render_installs_route_params_on_context() {
    let routes = vec![Route::endpoint(
        "/products/{id}",
        get(|| async { RouteResponse::ssr(RouteParamEcho) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/products/42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(body.to_vec()).unwrap(),
        "<section data-id=\"42\">42</section>"
    );
}

#[tokio::test]
async fn route_render_installs_request_headers_on_context() {
    let routes = vec![Route::endpoint(
        "/request",
        get(|| async { RouteResponse::ssr(RequestEcho) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/request")
                .header(http::header::USER_AGENT, "proa-test")
                .header(http::header::COOKIE, "theme=dark; session=abc123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(body.to_vec()).unwrap(),
        "<section data-method=\"GET\" data-agent=\"proa-test\" data-theme=\"dark\">request</section>"
    );
}

#[tokio::test]
async fn route_render_applies_staged_response_context() {
    let routes = vec![Route::endpoint(
        "/response",
        get(|| async { RouteResponse::ssr(ResponseContextEcho) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/response")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response
            .headers()
            .get("x-render-context")
            .and_then(|value| value.to_str().ok()),
        Some("set")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(body.to_vec()).unwrap(), "<p>response</p>");
}

#[tokio::test]
async fn html_macro_supports_async_children_and_borrowed_locals() {
    let suffix = String::from("tail");
    let page = html! {
        <section>
            <h1>"async html"</h1>
            {to_async(SyncGreeting { name: "world" })}
            {AsyncHtmlLeaf { label: "child" }}
            {if true {
                html! { <em>{suffix.as_str()}</em> }
            }}
        </section>
    };

    let html = render_to_string_async(page).await;
    assert_eq!(
        html,
        "<section><h1>async html</h1><p>sync world</p><span class=\"leaf\">child</span><em>tail</em></section>"
    );
}

/// A sync component routed through `ssr_async` via the explicit adapter.
/// Exercises the `RenderOutcome::Done` fast path with no real future polling.
struct SyncGreeting {
    name: &'static str,
}

impl<L: ::proa_core::DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for SyncGreeting {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_str("<p>sync ")?;
        cx.out.extend_str(self.name)?;
        cx.out.extend_str("</p>")?;
        Ok(())
    }
}

#[tokio::test]
async fn ssr_async_accepts_sync_component_via_adapter() {
    let routes = vec![Route::endpoint(
        "/sync",
        get(|| async { RouteResponse::ssr_async(to_async(SyncGreeting { name: "adapter" })) }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(Request::builder().uri("/sync").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), b"<p>sync adapter</p>");
}

#[tokio::test]
async fn ssr_async_dedupes_duplicate_loader_keys_within_request() {
    let loader = Arc::new(CountingLoader::default());
    let routes = vec![Route::endpoint(
        "/dedupe",
        get({
            let loader = Arc::clone(&loader);
            move || {
                let loader = Arc::clone(&loader);
                async move {
                    // Wrap the typed loader in a typed `DataResolver<L>` so
                    // dedupe still works without going through the dyn
                    // `DynLoaderHandle` path. `cx.load_json(...)` calls inside the
                    // page monomorphize on `DataResolver<CountingLoader>`,
                    // not on `DynLoaderHandle`.
                    let typed_loader =
                        std::sync::Arc::new(proa_stream::DataResolver::new_typed(loader));
                    RouteResponse::ssr_async_with_loader(
                        html! {
                            <main>
                                {LoadedHtmlLeaf {
                                    label: "first",
                                    load_key: "shared",
                                }}
                                {LoadedHtmlLeaf {
                                    label: "second",
                                    load_key: "shared",
                                }}
                            </main>
                        },
                        typed_loader,
                    )
                }
            }
        }),
    )];

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/dedupe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        body.as_ref(),
        b"<main><span class=\"leaf\">first</span><span class=\"leaf\">second</span></main>"
    );
    assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
}

/// Writes around a real yield in both the local and the `Send` render graph.
struct Yielding(&'static str);

impl WebRender for Yielding {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            WriteBuf::extend_str(&mut cx.out, "<p>")?;
            tokio::task::yield_now().await;
            WriteBuf::extend_str(&mut cx.out, self.0)?;
            WriteBuf::extend_str(&mut cx.out, "</p>")
        })
    }
}

impl WebRenderSend for Yielding {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend_str("<p>")?;
            tokio::task::yield_now().await;
            cx.output_mut().extend_str(self.0)?;
            cx.output_mut().extend_str("</p>")
        })
    }
}

/// Stages status and headers only after an await.
struct Staged;

impl Staged {
    fn stage(response: &mut proa_core::ResponseContext) {
        response.set_status(StatusCode::CREATED);
        response.insert_header(
            HeaderName::from_static("x-render"),
            HeaderValue::from_static("async"),
        );
        response.append_header(header::VARY, HeaderValue::from_static("Origin"));
    }
}

impl WebRender for Staged {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            Self::stage(cx.response_mut());
            WriteBuf::extend_str(&mut cx.out, "<p>staged</p>")
        })
    }
}

impl WebRenderSend for Staged {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            Self::stage(cx.response_mut());
            cx.output_mut().extend_str("<p>staged</p>")
        })
    }
}

/// Fails after an await, once response finalization has started.
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

/// Reads the route's metadata after an await, as `render_metadata_head` does.
struct MetadataTitle;

impl WebRender for MetadataTitle {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            let title = proa_framework_axum::resolved_metadata()
                .and_then(|metadata| metadata.title)
                .unwrap_or_default();
            WriteBuf::extend_str(&mut cx.out, &title)
        })
    }
}

fn app(routes: Vec<Route<()>>) -> Router {
    let (router, computed_metadata) = router_from_spec(routes);
    router.layer(RouteContextLayer::new(computed_metadata))
}

async fn fetch(app: &Router, uri: &str) -> Response {
    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), app.clone().oneshot(request))
        .await
        .expect("buffered async finalization must not hang")
        .unwrap()
}

async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

/// Both buffered constructors, with and without a `Send` render graph.
fn local_and_send<P>(page: fn() -> P) -> Vec<Route<()>>
where
    P: WebRender + WebRenderSend + Send + 'static,
{
    vec![
        Route::endpoint(
            "/local",
            get(move || async move { RouteResponse::ssr_async(page()) }),
        ),
        Route::endpoint(
            "/send",
            get(move || async move { RouteResponse::ssr_async_send(page()) }),
        ),
    ]
}

#[tokio::test]
async fn buffered_async_preserves_page_status_and_headers() {
    let app = app(local_and_send(|| Staged));
    for uri in ["/local", "/send"] {
        let response = fetch(&app, uri).await;
        assert_eq!(response.status(), StatusCode::CREATED, "{uri}");
        assert_eq!(response.headers()["x-render"], "async", "{uri}");
        assert_eq!(response.headers()[header::VARY], "Origin", "{uri}");
        assert_eq!(
            response.headers()["x-content-type-options"],
            "nosniff",
            "{uri}"
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8",
            "{uri}"
        );
        assert_eq!(text(response).await, "<p>staged</p>", "{uri}");
    }
}

#[tokio::test]
async fn buffered_async_render_errors_become_http_500() {
    let app = app(local_and_send(|| Fails));
    for uri in ["/local", "/send"] {
        let response = fetch(&app, uri).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
        assert_eq!(text(response).await, "render error", "{uri}");
    }
}

#[tokio::test]
async fn buffered_async_client_payload_is_json() {
    let app = app(local_and_send(|| Staged));
    for uri in ["/local", "/send"] {
        let request = Request::builder()
            .uri(uri)
            .header("X-Proa-Client", "1")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED, "{uri}");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/json; charset=utf-8",
            "{uri}"
        );
        let payload: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
        assert_eq!(payload["html"], "<p>staged</p>", "{uri}");
    }
}

#[tokio::test]
async fn handler_wrappers_apply_over_the_rendered_page() {
    let app = app(vec![Route::endpoint(
        "/missing",
        get(|| async {
            (
                StatusCode::NOT_FOUND,
                [(header::CACHE_CONTROL, "no-store")],
                RouteResponse::ssr_async(Staged),
            )
        }),
    )]);
    let response = fetch(&app, "/missing").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["x-render"], "async");
    assert_eq!(text(response).await, "<p>staged</p>");
}

#[tokio::test]
async fn buffered_async_render_reads_its_route_context() {
    let app = app(vec![Route::page(
        "/about",
        "About us",
        get(|| async { RouteResponse::ssr_async(MetadataTitle) }),
    )]);
    let response = fetch(&app, "/about").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "About us");
}

#[tokio::test]
async fn origin_page_cache_stores_buffered_async_pages() {
    let calls = Arc::new(AtomicUsize::new(0));
    let route = |path: &'static str, send: bool| {
        let calls = Arc::clone(&calls);
        Route::endpoint(
            path,
            get(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if send {
                        RouteResponse::ssr_async_send(Yielding("cached"))
                    } else {
                        RouteResponse::ssr_async(Yielding("cached"))
                    }
                }
            }),
        )
        .with_page_cache(PageCacheLayer::new("build"))
    };
    let app = app(vec![route("/local", false), route("/send", true)]);
    for uri in ["/local", "/send"] {
        for expected in ["MISS", "HIT"] {
            let response = fetch(&app, uri).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.headers()["x-proa-page-cache"], expected, "{uri}");
            assert_eq!(text(response).await, "<p>cached</p>", "{uri}");
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cdn_page_cache_sees_the_rendered_html() {
    let app = app(local_and_send(|| Yielding("edge"))
        .into_iter()
        .map(|route| route.with_page_cache(PageCache::new()))
        .collect());
    for uri in ["/local", "/send"] {
        let response = fetch(&app, uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let policy = response.headers()[header::CACHE_CONTROL].to_str().unwrap();
        assert!(policy.contains("public"), "{uri}: {policy}");
        assert_eq!(text(response).await, "<p>edge</p>", "{uri}");
    }
}

#[tokio::test]
async fn buffered_async_outside_route_context_is_a_clear_error() {
    let router = Router::new()
        .route(
            "/local",
            get(|| async { RouteResponse::ssr_async(Yielding("unreachable")) }),
        )
        .route(
            "/send",
            get(|| async { RouteResponse::ssr_async_send(Yielding("unreachable")) }),
        );
    for uri in ["/local", "/send"] {
        let response = fetch(&router, uri).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
        assert_eq!(text(response).await, "invalid route context", "{uri}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn buffered_async_renders_on_a_multi_thread_runtime() {
    let app = app(local_and_send(|| Yielding("threads")));
    for uri in ["/local", "/send"] {
        let response = fetch(&app, uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(text(response).await, "<p>threads</p>", "{uri}");
    }
}

/// Writes the name of the thread that polls the render.
struct ThreadName;

fn thread_name() -> String {
    std::thread::current().name().unwrap_or("").to_owned()
}

impl WebRender for ThreadName {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            WriteBuf::extend_str(&mut cx.out, &thread_name())
        })
    }
}

impl WebRenderSend for ThreadName {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            cx.output_mut().extend_str(&thread_name())
        })
    }
}

#[tokio::test]
async fn send_renders_are_polled_directly_and_local_renders_stay_pinned() {
    let app = app(local_and_send(|| ThreadName));
    let local = text(fetch(&app, "/local").await).await;
    assert!(local.starts_with("proa-stream-"), "{local}");
    let send = text(fetch(&app, "/send").await).await;
    assert_eq!(send, thread_name());
}

/// Holds the render open so a burst is in flight at once.
struct Held;

impl WebRender for Held {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            WriteBuf::extend_str(&mut cx.out, "<p>held</p>")
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn buffered_async_bursts_queue_instead_of_failing() {
    // More concurrent pages than the default stream admission limit.
    let app = app(vec![Route::endpoint(
        "/held",
        get(|| async { RouteResponse::ssr_async(Held) }),
    )]);
    let responses = futures::future::join_all((0..1_500).map(|_| fetch(&app, "/held"))).await;
    for response in responses {
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn an_explicit_200_wrapper_wins_like_any_other_status() {
    let app = app(vec![
        Route::endpoint(
            "/local",
            get(|| async { (StatusCode::OK, RouteResponse::ssr_async(Staged)) }),
        ),
        Route::endpoint(
            "/send",
            get(|| async { (StatusCode::OK, RouteResponse::ssr_async_send(Staged)) }),
        ),
        Route::endpoint(
            "/failed",
            get(|| async { (StatusCode::OK, RouteResponse::ssr_async(Fails)) }),
        ),
    ]);
    for uri in ["/local", "/send"] {
        let response = fetch(&app, uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()["x-render"], "async", "{uri}");
        assert_eq!(text(response).await, "<p>staged</p>", "{uri}");
    }
    // As when the render ran inside `into_response`, the wrapper's status
    // also replaces the render error's 500.
    let response = fetch(&app, "/failed").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "render error");
}

#[tokio::test]
async fn middleware_outside_route_context_sees_only_final_statuses() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let mut routes = local_and_send(|| Staged);
    routes.push(Route::endpoint(
        "/plain",
        get(|| async { RouteResponse::ssr_async(Yielding("plain")) }),
    ));
    let app = app(routes).layer(tower::util::MapResponseLayer::new(
        move |response: Response| {
            record.lock().unwrap().push(response.status());
            response
        },
    ));
    for uri in ["/local", "/send", "/plain"] {
        let response = fetch(&app, uri).await;
        assert_ne!(response.status().as_u16(), 299, "{uri}");
        assert!(!text(response).await.is_empty(), "{uri}");
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [StatusCode::CREATED, StatusCode::CREATED, StatusCode::OK]
    );
}
