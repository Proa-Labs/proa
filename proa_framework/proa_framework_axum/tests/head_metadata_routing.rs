// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dynamic captures must reach metadata and body through the ordinary Axum API.
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use proa_core::{
    ComponentRole, HeadConfig, HeadMetadata, HeadUrlPolicy, SyncHeadOperation, WebContext,
    WebRenderSync, WriteError,
};
use proa_framework_axum::{FrameworkBuilder, IslandManifest, Route, RouteResponse};
use proa_macros::html_sync;
use tower::ServiceExt;

struct Document;
impl WebRenderSync for Document {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        cx.head_mut().metadata(HeadMetadata {
            title: Some("Default".into()),
            title_template: Some("%s | Shop".into()),
            description: Some("Inherited description".into()),
            ..Default::default()
        })
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <html>
                <head>{cx.head()}</head>
                <body>{Page}</body>
            </html>
        }
        .render(cx)
    }
}
struct Page;
impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        let name = cx
            .param("id")
            .or_else(|| cx.param("slug"))
            .or_else(|| cx.param("path"))
            .ok_or_else(|| WriteError::render("missing route capture"))?
            .to_owned();
        cx.head_mut().metadata(HeadMetadata {
            title: Some(name.into()),
            ..Default::default()
        })
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let params = cx.route_params().cloned().unwrap_or_default();
        let name = params
            .get("id")
            .or_else(|| params.get("slug"))
            .or_else(|| params.get("path"))
            .unwrap();
        html_sync! {
            <main>
                <h1>{name}</h1>
            </main>
        }
        .render(cx)
    }
}
async fn page() -> RouteResponse {
    RouteResponse::ssr(Document)
}
async fn stream() -> RouteResponse {
    RouteResponse::ssr_stream(proa_core::to_async(Document))
}
fn app(spec: bool, streaming: bool) -> Router {
    let handler = if streaming { get(stream) } else { get(page) };
    let routes = if spec {
        Route::endpoint("/products/:id", handler.clone())
            .into_router()
            .merge(Route::endpoint("/blog/:slug", handler.clone()).into_router())
            .merge(Route::endpoint("/files/*path", handler.clone()).into_router())
    } else {
        Router::new()
            .route("/products/:id", handler.clone())
            .route("/blog/:slug", handler.clone())
            .route("/files/*path", handler.clone())
    };
    let routes = Router::new()
        .nest("/shop", routes)
        .route("/plain", get(|| async { "ok" }));
    let manifest = IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    };
    FrameworkBuilder::new(manifest)
        .with_head(
            HeadConfig::default()
                .with_urls(HeadUrlPolicy::new("https://shop.example", "/").unwrap()),
        )
        .with_dynamic_routes(routes)
        .build()
}
#[tokio::test]
async fn dynamic_titles_use_decoded_captures_and_inherit_templates() {
    for spec in [false, true] {
        for streaming in [false, true] {
            let app = app(spec, streaming);
            for (path, value) in [
                ("/products/42", "42"),
                ("/products/73", "73"),
                ("/blog/red%20%26%20blue", "red &amp; blue"),
                ("/blog/a%2Fb", "a/b"),
                ("/blog/caf%C3%A9", "café"),
                ("/files/a/b.txt", "a/b.txt"),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("/shop{path}?q=ignored"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    200,
                    "{path}; spec={spec}; streaming={streaming}"
                );
                let html = String::from_utf8(
                    to_bytes(response.into_body(), usize::MAX)
                        .await
                        .unwrap()
                        .to_vec(),
                )
                .unwrap();
                assert!(
                    html.contains(&format!("<title>{value} | Shop</title>")),
                    "{html}"
                );
                assert!(html.contains(&format!("<h1>{value}</h1>")), "{html}");
                assert!(
                    html.contains("<meta name=\"description\" content=\"Inherited description\">")
                );
                assert_eq!(html.matches("<title>").count(), 1);
            }
        }
    }
}
#[tokio::test]
async fn invalid_utf8_is_rejected_and_static_routes_need_no_captures() {
    let app = app(false, false);
    for (path, status) in [("/shop/blog/%FF", 400), ("/plain", 200)] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}
