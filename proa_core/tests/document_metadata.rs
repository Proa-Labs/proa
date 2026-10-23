// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    DataLoader, Metadata, NoLoader, OpenGraphImage, RenderBuffer, SendWebContext, WebContext,
    WebRender, WebRenderSend, WebRenderSendSync, WebRenderSync, WriteBuf,
};
use proa_macros::{html, html_send, html_sync};

fn inline_sync<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>) {
    WebRenderSync::render(
        html_sync! {
            <head>{cx.metadata.head()}</head>
        },
        cx,
    )
    .unwrap();
}
async fn inline_async<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>) {
    html! {
        <head>{cx.metadata.head()}</head>
    }
    .render(cx)
    .resolve()
    .await
    .unwrap();
}

#[test]
fn exact_inline_expression_supports_typed_loaders_and_writers() {
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().title(String::from("<& title")));
    inline_sync(&mut cx);
    assert_eq!(
        cx.into_output().into_string(),
        "<head><title>&lt;&amp; title</title></head>"
    );
    let mut out = RenderBuffer::new();
    let mut cx = WebContext::<NoLoader, _>::with_buffer(&mut out);
    inline_sync(&mut cx);
    assert!(cx.metadata.is_closed());
}

#[tokio::test]
async fn exact_inline_expression_supports_async_and_send() {
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().title("async"));
    inline_async(&mut cx).await;
    assert_eq!(
        cx.into_output().into_string(),
        "<head><title>async</title></head>"
    );
    let mut cx = SendWebContext::new();
    cx.metadata(Metadata::new().title("send"));
    WebRenderSendSync::render_send_sync(cx.metadata.head(), &mut cx).unwrap();
    assert!(cx.metadata.is_closed());
    let mut cx = SendWebContext::new();
    cx.metadata(Metadata::new().title("send async"));
    html_send! { <head>{cx.metadata.head()}</head> }
        .render_send(&mut cx)
        .resolve()
        .await
        .unwrap();
    assert!(cx.metadata.is_closed());
}

#[test]
fn handle_reads_at_render_time_and_emission_closes_even_when_empty() {
    let mut cx = WebContext::new();
    let head = cx.metadata.head();
    cx.metadata(Metadata::new().title("later"));
    WebRenderSync::render(head, &mut cx).unwrap();
    cx.metadata(Metadata::new().title("too late"));
    WebRenderSync::render(cx.metadata.head(), &mut cx).unwrap();
    assert_eq!(cx.metadata.rejections(), 2);
    assert_eq!(cx.into_output().into_string(), "<title>later</title>");
    let mut cx = WebContext::new();
    inline_sync(&mut cx);
    cx.metadata(Metadata::new().title("too late"));
    assert_eq!(cx.metadata.rejections(), 1);
    assert_eq!(cx.into_output().into_string(), "<head></head>");
}

#[test]
fn scopes_merge_fields_replace_lists_and_apply_only_ancestor_templates() {
    let mut cx = WebContext::new();
    cx.metadata(
        Metadata::new()
            .title("Store")
            .title_template("%s | Store")
            .description("inherited")
            .open_graph(|og| {
                og.site_name("Store");
                og.image("https://example.com/default.png");
            }),
    );
    cx.metadata.__next_route_scope();
    cx.metadata(
        Metadata::new()
            .title("Section")
            .title_template("%s | Section"),
    );
    cx.metadata.__next_route_scope();
    cx.metadata(
        Metadata::new()
            .title("First")
            .clear_description()
            .open_graph(|og| {
                og.image("https://example.com/a.png");
                og.image("https://example.com/b.png");
            }),
    );
    cx.metadata(Metadata::new().title("Last").open_graph(|og| {
        og.image(
            OpenGraphImage::new(String::from("https://example.com/final.png"))
                .with_dimensions(1200, 630)
                .with_alt(String::from("a\"<b")),
        );
    }));
    inline_sync(&mut cx);
    let output = cx.into_output().into_string();
    assert!(output.contains("<title>Last | Section</title>"));
    assert!(output.contains("og:site_name\" content=\"Store"));
    assert!(!output.contains("description"));
    assert!(!output.contains("default.png"));
    assert!(!output.contains("/a.png"));
    assert!(!output.contains("/b.png"));
    assert!(output.contains("content=\"1200\""));
    assert!(output.contains("content=\"a&quot;&lt;b\""));
    assert!(!output.contains("og:title"));
}

#[test]
fn same_scope_default_is_unformatted_and_clear_stops_template_inheritance() {
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().title("Store").title_template("%s | Store"));
    inline_sync(&mut cx);
    assert_eq!(
        cx.into_output().into_string(),
        "<head><title>Store</title></head>"
    );
    for absolute in [false, true] {
        let mut cx = WebContext::new();
        cx.metadata(Metadata::new().title_template("%s | Store"));
        cx.metadata.__next_route_scope();
        cx.metadata(Metadata::new().clear_title_template());
        if !absolute {
            cx.metadata.__next_route_scope();
        }
        cx.metadata(if absolute {
            Metadata::new().absolute_title("Page")
        } else {
            Metadata::new().title("Page")
        });
        inline_sync(&mut cx);
        assert_eq!(
            cx.into_output().into_string(),
            "<head><title>Page</title></head>"
        );
    }
}

#[test]
fn scoped_writers_preserve_state_detached_and_fallback_contexts_are_denied() {
    let mut cx = WebContext::new();
    let mut out = RenderBuffer::new();
    cx.with_output(&mut out, |child| {
        child.metadata(Metadata::new().title("shared"))
    });
    cx.__without_document_metadata(|fallback| {
        fallback.metadata(Metadata::new().title("fallback"));
        WebRenderSync::render(fallback.metadata.head(), fallback).unwrap();
    });
    assert_eq!(cx.metadata.rejections(), 2);
    let parts = cx.context_parts();
    let mut child = WebContext::with_loader_buffer_and_context_parts(
        RenderBuffer::new(),
        cx.loader().clone(),
        parts,
    );
    child.metadata(Metadata::new().title("detached"));
    WebRenderSync::render(child.metadata.head(), &mut child).unwrap();
    assert_eq!(child.metadata.rejections(), 2);
    assert!(child.into_output().is_empty());
    inline_sync(&mut cx);
    assert_eq!(
        cx.into_output().into_string(),
        "<head><title>shared</title></head>"
    );
}

#[test]
fn manual_tags_coexist_and_clears_only_affect_managed_fields() {
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().description("managed"));
    WebRenderSync::render(
        html_sync! {
            <head>
                <meta name="description" content="manual" />
                {cx.metadata.head()}
            </head>
        },
        &mut cx,
    )
    .unwrap();
    let output = cx.into_output().into_string();
    assert_eq!(output.matches("name=\"description\"").count(), 2);
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().description("managed"));
    cx.metadata(Metadata::new().clear_description());
    WebRenderSync::render(
        html_sync! {
            <head>
                <meta name="description" content="manual" />
                {cx.metadata.head()}
            </head>
        },
        &mut cx,
    )
    .unwrap();
    assert_eq!(
        cx.into_output()
            .into_string()
            .matches("name=\"description\"")
            .count(),
        1
    );
}

#[test]
fn writer_failure_keeps_metadata_closed_and_fresh_documents_have_fresh_state() {
    use proa_core::shared::write_buf::WriteCursor;
    use proa_core::WriteError;
    let mut storage = [0u8; 8];
    let mut cx = WebContext::with_buffer(WriteCursor::new(&mut storage));
    cx.metadata(Metadata::new().title("longer than capacity"));
    assert_eq!(
        WebRenderSync::render(cx.metadata.head(), &mut cx),
        Err(WriteError::OutOfCapacity)
    );
    assert!(cx.metadata.is_closed());
    cx.metadata(Metadata::new().clear_title());
    assert_eq!(cx.metadata.rejections(), 1);
    assert!(WebRenderSync::render(cx.metadata.head(), &mut cx).is_ok());
    assert_eq!(cx.metadata.rejections(), 2);
    let mut fresh = WebContext::new();
    inline_sync(&mut fresh);
    assert_eq!(fresh.into_output().into_string(), "<head></head>");
}

#[test]
fn caught_inline_errors_keep_prior_declarations_and_scoped_state_survives_unwind() {
    use proa_core::WriteError;
    fn declare_then_fail(cx: &mut WebContext) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("before failure"));
        Err(WriteError::OutOfCapacity)
    }
    let mut cx = WebContext::new();
    let result = declare_then_fail(&mut cx);
    assert!(result.is_err());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.__with_erased_loader(|setup| {
            setup.metadata(Metadata::new().description("setup survives"));
            setup.request_mut().headers_mut().insert(
                http::HeaderName::from_static("x-setup"),
                http::HeaderValue::from_static("yes"),
            );
            panic!("setup interrupted");
        });
    }));
    assert!(result.is_err());
    assert_eq!(cx.request().headers()["x-setup"], "yes");
    inline_sync(&mut cx);
    let html = cx.into_output().into_string();
    assert!(html.contains("<title>before failure</title>"));
    assert!(html.contains("content=\"setup survives\""));
}

#[tokio::test]
async fn cancellation_and_unwind_restore_metadata_eligibility() {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let mut cx = WebContext::new();
    {
        let mut operation = pin!(cx.__without_document_metadata_async(async |_cx| {
            std::future::pending::<()>().await;
        }));
        let mut task = Context::from_waker(Waker::noop());
        assert!(matches!(operation.as_mut().poll(&mut task), Poll::Pending));
    }
    cx.metadata(Metadata::new().title("after cancellation"));
    inline_sync(&mut cx);
    assert_eq!(cx.metadata.rejections(), 0);
    let mut cx = SendWebContext::new();
    {
        let mut operation = pin!(cx.__without_document_metadata_async(async |_cx| {
            std::future::pending::<()>().await;
        }));
        let mut task = Context::from_waker(Waker::noop());
        assert!(matches!(operation.as_mut().poll(&mut task), Poll::Pending));
    }
    cx.metadata(Metadata::new().title("after cancellation"));
    cx.metadata.head().render_send_sync(&mut cx).unwrap();
    assert_eq!(cx.metadata.rejections(), 0);
    assert_eq!(
        cx.into_output().into_string(),
        "<title>after cancellation</title>"
    );
}

#[test]
fn images_keep_order_and_subproperties_until_explicitly_cleared() {
    let mut cx = WebContext::new();
    cx.metadata(Metadata::new().open_graph(|og| {
        og.site_name("retained");
        og.image(OpenGraphImage::new("https://example.com/first").with_alt("first alt"));
        og.image(OpenGraphImage::new("https://example.com/second").with_alt("second alt"));
    }));
    WebRenderSync::render(cx.metadata.head(), &mut cx).unwrap();
    let output = cx.into_output().into_string();
    let first = output
        .find("content=\"https://example.com/first\"")
        .unwrap();
    let first_alt = output.find("content=\"first alt\"").unwrap();
    let second = output
        .find("content=\"https://example.com/second\"")
        .unwrap();
    let second_alt = output.find("content=\"second alt\"").unwrap();
    assert!(first < first_alt && first_alt < second && second < second_alt);
    let mut cx = WebContext::new();
    cx.metadata(
        Metadata::new()
            .title_template("%s | inherited")
            .title("default")
            .open_graph(|og| {
                og.site_name("retained");
                og.image("https://example.com/remove");
            }),
    );
    cx.metadata(Metadata::new().clear_title().open_graph(|og| {
        og.clear_images();
    }));
    cx.metadata.__next_route_scope();
    cx.metadata(Metadata::new().title("child"));
    WebRenderSync::render(cx.metadata.head(), &mut cx).unwrap();
    let output = cx.into_output().into_string();
    assert!(output.contains("<title>child | inherited</title>"));
    assert!(!output.contains("og:image"));
    assert!(output.contains("og:site_name\" content=\"retained\""));
}

#[test]
fn emitting_head_writes_bytes_without_flushing_or_changing_http_headers() {
    use std::{cell::Cell, rc::Rc};
    struct Sink(Rc<Cell<usize>>);
    impl proa_core::StreamChunkFlusher for Sink {
        fn flush_chunk(&self, _: Vec<u8>) -> Result<(), proa_core::WriteError> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let flushes = Rc::new(Cell::new(0));
    let mut cx = WebContext::new();
    cx.set_render_mode(proa_core::RenderMode::ChunkedInOrder);
    cx.set_stream_chunk_flusher(Rc::new(Sink(flushes.clone())));
    cx.response_mut().mark_streaming_started();
    cx.metadata(Metadata::new().title("immediate"));
    WebRenderSync::render(cx.metadata.head(), &mut cx).unwrap();
    assert_eq!(cx.out.len(), "<title>immediate</title>".len());
    assert!(cx.metadata.is_closed());
    assert_eq!(flushes.get(), 0);
    assert!(!cx.response_mut().insert_header(
        http::HeaderName::from_static("x-late"),
        http::HeaderValue::from_static("no")
    ));
    cx.flush_stream_chunk().unwrap();
    assert_eq!(flushes.get(), 1);
}

#[test]
fn send_layout_adapter_preserves_live_output_request_response_and_metadata() {
    let mut cx = SendWebContext::new();
    cx.out.extend_static(b"before").unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.__with_erased_loader(|setup| {
            setup.out.extend_static(b" setup ").unwrap();
            setup.metadata(Metadata::new().title("from setup"));
            setup.request_mut().headers_mut().insert(
                http::HeaderName::from_static("x-request"),
                http::HeaderValue::from_static("yes"),
            );
            setup.response_mut().insert_header(
                http::HeaderName::from_static("x-response"),
                http::HeaderValue::from_static("yes"),
            );
            panic!("setup interrupted");
        });
    }));
    assert!(result.is_err());
    assert_eq!(cx.request().headers()["x-request"], "yes");
    assert_eq!(cx.response().headers()["x-response"], "yes");
    cx.metadata.head().render_send_sync(&mut cx).unwrap();
    assert_eq!(
        cx.into_output().into_string(),
        "before setup <title>from setup</title>"
    );
}
