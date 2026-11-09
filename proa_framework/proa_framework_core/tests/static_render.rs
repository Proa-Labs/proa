// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `RouteResponse::render_to_bytes` renders a complete managed document with
//! no HTTP server, no island manifest, and no async runtime. Every test here is
//! a plain `#[test]` on purpose: nothing may require tokio.

use proa_core::{
    ComponentRole, DataLoader, HeadConfig, RenderOutcome, SyncHeadOperation, WebContext, WebRender,
    WebRenderSync, WriteError,
};
use proa_framework_core::RouteResponse;
use proa_macros::{html, html_sync};
use std::future::Future;

struct Page;

impl<L: DataLoader> WebRenderSync<L> for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl SyncHeadOperation {
        cx.head_mut().title("Static page")
    }

    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        WebRenderSync::render(html_sync! {
            <main>
                <h1>"Rendered offline"</h1>
            </main>
        }, cx)
    }
}

struct Layout {
    page: Page,
}

impl<L: DataLoader> WebRenderSync<L> for Layout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl SyncHeadOperation {
        cx.head_mut().description("Written straight to disk.")
    }

    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let head = cx.head();
        let document = html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>{head}</head>
                <body>{self.page}</body>
            </html>
        };
        WebRenderSync::render(document, cx)
    }
}

struct AsyncLayout {
    page: Page,
}

impl<L: DataLoader> WebRender<L> for AsyncLayout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.head_mut().description("Driven on the calling thread.")
    }

    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        let document = html! {
            <!DOCTYPE html>
            <html lang="en">
                <head>{head}</head>
                <body>{proa_core::to_async(self.page)}</body>
            </html>
        };
        WebRender::render(document, cx)
    }
}

struct Plain;

impl<L: DataLoader> WebRenderSync<L> for Plain {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let document = html_sync! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>"Manual"</title>
                </head>
                <body>"No managed head"</body>
            </html>
        };
        WebRenderSync::render(document, cx)
    }
}

fn assert_document(html: &str, description: &str) {
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("<title>Static page</title>"), "{html}");
    assert!(html.contains(description), "{html}");
    assert!(html.contains("<h1>Rendered offline</h1>"), "{html}");
    assert!(html.trim_end().ends_with("</html>"), "{html}");
}

#[test]
fn sync_managed_document_renders_without_a_runtime() {
    let html = RouteResponse::ssr(Layout { page: Page })
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render sync document");
    assert_document(&html, "Written straight to disk.");
}

#[test]
fn render_to_bytes_and_render_to_string_agree() {
    let bytes = RouteResponse::ssr(Layout { page: Page })
        .with_head(HeadConfig::default())
        .render_to_bytes()
        .expect("render bytes");
    let string = RouteResponse::ssr(Layout { page: Page })
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render string");
    assert_eq!(bytes.as_ref(), string.as_bytes());
}

#[test]
fn buffered_async_plan_is_driven_on_the_calling_thread() {
    let html = RouteResponse::ssr_async(AsyncLayout { page: Page })
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render async document");
    assert_document(&html, "Driven on the calling thread.");
}

#[test]
fn streamed_plan_collects_into_one_document() {
    let html = RouteResponse::ssr_stream(AsyncLayout { page: Page })
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render streamed document");
    assert_document(&html, "Driven on the calling thread.");
}

#[test]
fn manual_document_renders_without_head_configuration() {
    let html = RouteResponse::ssr(Plain)
        .render_to_string()
        .expect("render manual document");
    assert!(html.contains("<title>Manual</title>"), "{html}");
    assert!(html.contains("No managed head"), "{html}");
}

#[test]
fn page_role_without_head_configuration_is_rejected() {
    let error = RouteResponse::ssr(Layout { page: Page })
        .render_to_bytes()
        .expect_err("a Page needs a managed document");
    assert!(error.to_string().contains("managed head"), "{error}");
}
