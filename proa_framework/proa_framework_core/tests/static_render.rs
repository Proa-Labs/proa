// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `RouteResponse::render_to_bytes` renders a complete managed document with
//! no HTTP server, no island manifest, and no async runtime. Every test here is
//! a plain `#[test]` on purpose: nothing may require tokio.
//!
//! The fixtures take their copy as inputs so the assertions check behavior
//! (inputs reaching the output, escaping, head placement, delivery parity)
//! rather than pinning prose.

use proa_core::{
    ComponentRole, DataLoader, HeadConfig, RenderOutcome, SyncHeadOperation, WebContext, WebRender,
    WebRenderSync, WriteError,
};
use proa_framework_core::RouteResponse;
use proa_macros::{html, html_sync};
use std::future::Future;

const TITLE: &str = "Quarterly figures";
const DESCRIPTION: &str = "Three plots composed offline.";
const BODY: &str = "Rendered without a listener";

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
            <main>
                <h1>{self.title}</h1>
                <p>{self.body}</p>
            </main>
        };
        WebRenderSync::render(body, cx)
    }
}

struct Layout {
    description: &'static str,
    page: Page,
}

impl<L: DataLoader> WebRenderSync<L> for Layout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl SyncHeadOperation {
        cx.head_mut().description(self.description)
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
    description: &'static str,
    page: Page,
}

impl<L: DataLoader> WebRender<L> for AsyncLayout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.head_mut().description(self.description)
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

/// A manual document: no roles, no head hooks, its own `<title>`.
struct Plain {
    title: &'static str,
    body: &'static str,
}

impl<L: DataLoader> WebRenderSync<L> for Plain {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let document = html_sync! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>{self.title}</title>
                </head>
                <body>{self.body}</body>
            </html>
        };
        WebRenderSync::render(document, cx)
    }
}

fn layout() -> Layout {
    Layout {
        description: DESCRIPTION,
        page: Page {
            title: TITLE,
            body: BODY,
        },
    }
}

fn async_layout() -> AsyncLayout {
    AsyncLayout {
        description: DESCRIPTION,
        page: Page {
            title: TITLE,
            body: BODY,
        },
    }
}

/// The inputs must reach the document, in the right places, in the right order.
fn assert_managed_document(html: &str) {
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.trim_end().ends_with("</html>"), "{html}");
    let title_tag = format!("<title>{TITLE}</title>");
    let title_at = html
        .find(&title_tag)
        .unwrap_or_else(|| panic!("title missing: {html}"));
    let description_at = html
        .find(DESCRIPTION)
        .unwrap_or_else(|| panic!("description missing: {html}"));
    let body_at = html
        .find("<body>")
        .unwrap_or_else(|| panic!("body missing: {html}"));
    assert!(
        title_at < body_at,
        "head content must precede the body: {html}"
    );
    assert!(
        description_at < body_at,
        "head content must precede the body: {html}"
    );
    assert_eq!(html.matches("<title>").count(), 1, "{html}");
    assert!(html.contains(&format!("<h1>{TITLE}</h1>")), "{html}");
    assert!(html.contains(&format!("<p>{BODY}</p>")), "{html}");
}

#[test]
fn sync_managed_document_renders_without_a_runtime() {
    let html = RouteResponse::ssr(layout())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render sync document");
    assert_managed_document(&html);
}

#[test]
fn dynamic_body_text_is_escaped() {
    let html = RouteResponse::ssr(Layout {
        description: DESCRIPTION,
        page: Page {
            title: TITLE,
            body: "<script>alert(1)</script> & co",
        },
    })
    .with_head(HeadConfig::default())
    .render_to_string()
    .expect("render sync document");
    assert!(
        html.contains("<p>&lt;script&gt;alert(1)&lt;/script&gt; &amp; co</p>"),
        "{html}"
    );
    assert!(!html.contains("<script>alert"), "{html}");
}

#[test]
fn render_to_bytes_and_render_to_string_agree() {
    let bytes = RouteResponse::ssr(layout())
        .with_head(HeadConfig::default())
        .render_to_bytes()
        .expect("render bytes");
    let string = RouteResponse::ssr(layout())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render string");
    assert_eq!(bytes.as_ref(), string.as_bytes());
}

#[test]
fn buffered_async_plan_matches_the_sync_document() {
    let sync_html = RouteResponse::ssr(layout())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render sync document");
    let async_html = RouteResponse::ssr_async(async_layout())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render async document");
    assert_managed_document(&async_html);
    assert_eq!(async_html, sync_html);
}

#[test]
fn streamed_plan_collects_into_one_complete_document() {
    let html = RouteResponse::ssr_stream(async_layout())
        .with_head(HeadConfig::default())
        .render_to_string()
        .expect("render streamed document");
    assert_managed_document(&html);
}

#[test]
fn manual_document_renders_without_head_configuration() {
    let html = RouteResponse::ssr(Plain {
        title: TITLE,
        body: BODY,
    })
    .render_to_string()
    .expect("render manual document");
    assert!(html.contains(&format!("<title>{TITLE}</title>")), "{html}");
    assert!(html.contains(&format!("<body>{BODY}</body>")), "{html}");
}

#[test]
fn page_role_without_head_configuration_is_rejected() {
    let error = RouteResponse::ssr(layout())
        .render_to_bytes()
        .expect_err("a Page needs a managed document");
    assert!(error.to_string().contains("managed head"), "{error}");
}
