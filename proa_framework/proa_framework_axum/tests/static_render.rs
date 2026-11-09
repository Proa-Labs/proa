// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The Axum `RouteResponse` wrapper forwards the runtime-free static renderers.
//! For a page that reads no request context (no route params, base path, or
//! `css_asset_path` / `render_metadata_head` helpers) the bytes they produce
//! are the bytes the router serves.

use axum::{body::Body, http::Request, routing::get, Router};
use proa_core::{
    ComponentRole, DataLoader, HeadConfig, SyncHeadOperation, WebContext, WebRenderSync, WriteError,
};
use proa_framework_axum::{FrameworkBuilder, RouteResponse};
use proa_macros::html_sync;
use tower::ServiceExt;

const TITLE: &str = "Served and exported";
const BODY: &str = "Same bytes either way";

struct Page {
    title: &'static str,
    body: &'static str,
}

impl<L: DataLoader> WebRenderSync<L> for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl SyncHeadOperation {
        cx.head_mut().title(self.title)
    }

    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let body = html_sync! {
            <main>{self.body}</main>
        };
        WebRenderSync::render(body, cx)
    }
}

struct Document {
    page: Page,
}

impl<L: DataLoader> WebRenderSync<L> for Document {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let head = cx.head();
        let document = html_sync! {
            <!DOCTYPE html>
            <html>
                <head>{head}</head>
                <body>{self.page}</body>
            </html>
        };
        WebRenderSync::render(document, cx)
    }
}

fn document() -> Document {
    Document {
        page: Page {
            title: TITLE,
            body: BODY,
        },
    }
}

/// Plain `#[test]` on purpose: the wrapper must not need a tokio runtime.
#[test]
fn axum_route_response_renders_without_a_server_or_runtime() {
    let html = RouteResponse::ssr(document())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render through the wrapper");
    assert!(html.contains(&format!("<title>{TITLE}</title>")), "{html}");
    assert!(html.contains(&format!("<main>{BODY}</main>")), "{html}");

    let bytes = RouteResponse::ssr(document())
        .with_head(HeadConfig::default())
        .render_to_bytes()
        .expect("render bytes through the wrapper");
    assert_eq!(bytes.as_ref(), html.as_bytes());
}

/// Holds for pages that read nothing from the route context; the wrapper's
/// docs point such pages at `oneshot` through the built router instead.
#[tokio::test]
async fn static_render_matches_the_document_the_router_serves() {
    let app = FrameworkBuilder::new()
        .with_head(HeadConfig::default())
        .with_dynamic_routes(
            Router::new().route("/", get(|| async { RouteResponse::ssr(document()) })),
        )
        .build();
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .expect("route the request");
    assert!(response.status().is_success(), "{:?}", response.status());
    let served = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the served body");

    let exported = RouteResponse::ssr(document())
        .with_head(HeadConfig::default())
        .render_to_bytes()
        .expect("render the same plan offline");

    assert_eq!(exported, served);
}
