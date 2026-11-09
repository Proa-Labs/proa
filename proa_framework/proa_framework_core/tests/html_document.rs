// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{cell::Cell, rc::Rc};

use proa_core::{
    ComponentRole, HeadConfig, HeadMetadata, SyncHeadOperation, WebContext, WebRenderSync,
    WriteError,
};
use proa_framework_core::{render_sync_html_document, FrameworkError, WebContextDocumentExt};
use proa_macros::html_sync;

struct Page {
    metadata: HeadMetadata,
    body: &'static str,
    fails: bool,
}

impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        cx.head_mut().metadata(self.metadata.clone())
    }

    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        if self.fails {
            return Err(WriteError::render("document body failed"));
        }
        html_sync! {
            <main>{self.body}</main>
        }
        .render(cx)
    }
}

struct Layout<C> {
    page: C,
    outlets: usize,
}

impl<C: WebRenderSync> WebRenderSync for Layout<C> {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        cx.head_mut().metadata(HeadMetadata {
            title: "Layout".into(),
            ..Default::default()
        })
    }

    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>{for _ in 0..self.outlets { cx.head() }}</head>
                <body>{self.page}</body>
            </html>
        }
        .render(cx)
    }
}

#[test]
fn standalone_document_runs_hooks_and_preserves_head_prefix_and_body_tail() {
    let title = "Export <one> & two";
    let description = "Inherited export description";
    let body = "Rendered after metadata publication";
    let config = HeadConfig::default()
        .with_defaults(|head| head.description(description))
        .unwrap();
    let bytes = render_sync_html_document(
        Layout {
            page: Page {
                metadata: HeadMetadata {
                    title: title.into(),
                    ..Default::default()
                },
                body,
                fails: false,
            },
            outlets: 1,
        },
        config,
    )
    .unwrap();
    let html = std::str::from_utf8(&bytes).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert_eq!(html.matches("<title>").count(), 1);
    assert!(html.contains("<title>Export &lt;one&gt; &amp; two</title>"));
    assert!(html.contains(description));
    assert!(html.contains(body));
    assert!(html.find("<title>").unwrap() < html.find("</head>").unwrap());
    assert!(html.ends_with("</main></body></html>"));
}

#[test]
fn standalone_document_rejects_missing_page_or_invalid_outlet_count() {
    let missing_page = render_sync_html_document(
        Layout {
            page: (),
            outlets: 1,
        },
        HeadConfig::default(),
    );
    assert!(matches!(missing_page, Err(FrameworkError::Render(_))));
    for outlets in [0, 2] {
        let result = render_sync_html_document(
            Layout {
                page: Page {
                    metadata: HeadMetadata::default(),
                    body: "",
                    fails: false,
                },
                outlets,
            },
            HeadConfig::default(),
        );
        assert!(matches!(result, Err(FrameworkError::Render(_))));
    }
}

#[test]
fn standalone_document_does_not_return_partial_html_after_body_failure() {
    let mut cx = WebContext::new();
    let error = cx
        .render(Layout {
            page: Page {
                metadata: HeadMetadata::default(),
                body: "",
                fails: true,
            },
            outlets: 1,
        })
        .unwrap_err();
    assert!(error.to_string().contains("document body failed"));
    assert!(cx.finish().is_err());
}

#[test]
fn document_context_renders_borrowed_non_send_components_immediately() {
    struct LocalPage<'a> {
        body: &'a str,
        renders: Rc<Cell<usize>>,
    }

    impl WebRenderSync for LocalPage<'_> {
        const ROLE: ComponentRole = ComponentRole::PAGE;

        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            self.renders.set(self.renders.get() + 1);
            html_sync! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>{cx.head()}</head>
                    <body>{self.body}</body>
                </html>
            }
            .render(cx)
        }
    }

    let body = String::from("Borrowed <body>");
    let renders = Rc::new(Cell::new(0));
    let head = HeadConfig::default()
        .with_defaults(|head| head.title("Configured default"))
        .unwrap();
    let mut cx = WebContext::new();
    cx.set_document_head(head).unwrap();
    cx.render(LocalPage {
        body: &body,
        renders: renders.clone(),
    })
    .unwrap();
    assert_eq!(renders.get(), 1, "render must run before finish");
    let bytes = cx.finish().unwrap();
    let html = std::str::from_utf8(&bytes).unwrap();
    assert!(html.contains("<title>Configured default</title>"));
    assert!(html.contains("Borrowed &lt;body&gt;"));
}

#[test]
fn web_context_rejects_invalid_document_lifecycles() {
    assert!(WebContext::new().finish().is_err());

    let document = || Layout {
        page: Page {
            metadata: HeadMetadata::default(),
            body: "",
            fails: false,
        },
        outlets: 1,
    };
    let mut cx = WebContext::new();
    cx.render(document()).unwrap();
    assert!(cx.render(document()).is_err());
    assert!(cx.finish().is_err());

    let mut cx = WebContext::new();
    cx.render(document()).unwrap();
    let fork = WebContext::with_loader_buffer_and_context_parts(
        proa_core::RenderBuffer::new(),
        std::sync::Arc::new(proa_core::NoLoader),
        cx.context_parts(),
    );
    assert!(fork.finish().is_err());
    assert!(cx.finish().is_err());

    let mut cx = WebContext::new();
    html_sync! {
        <p>"Fragment"</p>
    }
    .render(&mut cx)
    .unwrap();
    assert!(cx.render(document()).is_err());
    assert!(cx.finish().is_err());
}
