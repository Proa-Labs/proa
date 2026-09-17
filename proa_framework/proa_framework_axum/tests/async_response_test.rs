// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for buffered async `RouteResponse` rendering.
//!
//! Drives an async `WebRender` component through the framework's `IntoResponse`
//! impl end-to-end: build an axum router, install `RouteContextLayer`, fire a
//! `oneshot` request, and assert the rendered body contains the bytes the
//! async component produced (including content written *after* a real
//! `.await`). This proves the `block_in_place` + `Handle::block_on` bridge
//! inside `into_http_response` drives the future to completion before returning
//! the HTTP response.
//!
//! We explicitly use `#[tokio::test(flavor = "multi_thread")]` so that
//! `tokio::task::block_in_place` is actually available (it panics on the
//! current-thread runtime). Axum's default runtime is multi-threaded, so this
//! matches real-world usage.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get,
};
use proa_core::render::RenderOutcome;
use proa_core::{
    await_component, render_to_string_async, to_async, DataLoader, LoadError, LoadKey, LoadRequest,
    LoadValue, RenderBuffer, WebContext, WebRender, WriteBuf, WriteError,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{router_from_spec, Route, RouteContextLayer, RouteResponse};
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
            WriteBuf::extend(&mut cx.out, b"<p>hello ")?;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            WriteBuf::extend(&mut cx.out, self.name.as_bytes())?;
            WriteBuf::extend(&mut cx.out, b"</p>")?;
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

fn empty_manifest() -> Arc<IslandManifest> {
    Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_async_renders_async_component() {
    let routes = vec![Route::endpoint(
        "/hello",
        get(|| async { RouteResponse::ssr_async(AsyncGreeting { name: "world" }) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_async_sets_default_security_headers() {
    let routes = vec![Route::endpoint(
        "/hello",
        get(|| async { RouteResponse::ssr_async(AsyncGreeting { name: "security" }) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_render_installs_request_location_on_context() {
    let routes = vec![Route::endpoint(
        "/dashboard",
        get(|| async { RouteResponse::ssr(LocationEcho) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_render_installs_route_params_on_context() {
    let routes = vec![Route::endpoint(
        "/products/:id",
        get(|| async { RouteResponse::ssr(RouteParamEcho) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_render_installs_request_headers_on_context() {
    let routes = vec![Route::endpoint(
        "/request",
        get(|| async { RouteResponse::ssr(RequestEcho) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_render_applies_staged_response_context() {
    let routes = vec![Route::endpoint(
        "/response",
        get(|| async { RouteResponse::ssr(ResponseContextEcho) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
        cx.out.extend(b"<p>sync ")?;
        cx.out.extend(self.name.as_bytes())?;
        cx.out.extend(b"</p>")?;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssr_async_accepts_sync_component_via_adapter() {
    let routes = vec![Route::endpoint(
        "/sync",
        get(|| async { RouteResponse::ssr_async(to_async(SyncGreeting { name: "adapter" })) }),
    )];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(Request::builder().uri("/sync").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), b"<p>sync adapter</p>");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
                    // `DynLoaderHandle` path. `cx.load(...)` calls inside the
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

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

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
