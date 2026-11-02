// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, MdRender, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
use proa_macros::{html, html_sync, md, md_sync};
use std::{future::Future, rc::Rc};

struct Delayed<'a>(&'a str);
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Delayed<'_> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            tokio::task::yield_now().await;
            WebRender::render(self.0, cx).resolve().await
        })
    }
}
impl<L: DataLoader, B: WriteBuf> MdRender<L, B> for Delayed<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            tokio::task::yield_now().await;
            cx.write_md_text(self.0)
        })
    }
}

#[tokio::test]
async fn async_formats_borrow_local_strings_across_pending_children() {
    let owned = String::from("<em>*borrowed*</em>");
    let borrowed = owned.as_str();
    let html_output = proa_core::render_to_string_async(html! {
        <p>{Delayed(borrowed)}{borrowed}</p>
    })
    .await;
    let md_output =
        proa_core::render_md_to_string_async(md! { "{Delayed(borrowed)}{borrowed}" }).await;
    assert_eq!(
        html_output,
        "<p>&lt;em&gt;*borrowed*&lt;/em&gt;&lt;em&gt;*borrowed*&lt;/em&gt;</p>"
    );
    assert_eq!(
        md_output,
        "\\<em\\>\\*borrowed\\*\\</em\\>\\<em\\>\\*borrowed\\*\\</em\\>"
    );
    // Rendering borrowed the input; it remains available afterwards.
    assert_eq!(owned, "<em>*borrowed*</em>");
}

#[tokio::test]
async fn nested_async_formats_borrow_loop_locals() {
    let values = [String::from("a"), String::from("b")];
    let html_output = proa_core::render_to_string_async(html! {
        {for value in &values {
            let local = value.repeat(2);
            html! { <p>{Delayed(&local)}</p> }
        }}
    })
    .await;
    let md_output = proa_core::render_md_to_string_async(md! { r###"{for value in &values {
        let local = value.repeat(2);
        md! { "{Delayed(&local)}\n" }
    }}"### })
    .await;
    assert_eq!(html_output, "<p>aa</p><p>bb</p>");
    assert_eq!(md_output, "aa\nbb\n");
}

#[tokio::test]
async fn local_async_formats_allow_non_send_captures() {
    let value = Rc::new(String::from("local"));
    assert_eq!(
        proa_core::render_to_string_async(html! { {Delayed(value.as_str())} }).await,
        "local"
    );
    assert_eq!(
        proa_core::render_md_to_string_async(md! { "{Delayed(value.as_str())}" }).await,
        "local"
    );
}

#[test]
fn sync_formats_share_block_attribute_and_jump_lowering() {
    let html_output = proa_core::render_to_string(html_sync! {
        {#[cfg(any())] if true { "hidden" }}
        {match true {
            #[cfg(any())] true => "hidden",
            _ => { for value in ["1", "2", "3"] {
                if value == "3" { break } else { value }
            } },
        }}
        {'outer: for value in ["3", "4"] {
            if value == "4" { break 'outer } else { value }
        }}
    });
    let md_output = proa_core::render_to_string(
        md_sync! { r###"{#[cfg(any())] if true { "hidden" }}{match true {
        #[cfg(any())] true => "hidden",
        _ => { for value in ["1", "2", "3"] {
            if value == "3" { break } else { value }
        } },
    }}{'outer: for value in ["3", "4"] {
        if value == "4" { break 'outer } else { value }
    }}"### },
    );
    assert_eq!(html_output, "123");
    assert_eq!(md_output, html_output);
}

#[tokio::test]
async fn async_formats_share_block_attribute_and_jump_lowering() {
    let html_output = proa_core::render_to_string_async(html! {
        {#[cfg(any())] if true { "hidden" }}
        {match true {
            #[cfg(any())] true => "hidden",
            _ => { for value in ["1", "2", "3"] {
                if value == "3" { break } else { value }
            } },
        }}
        {'outer: for value in ["3", "4"] {
            if value == "4" { break 'outer } else { value }
        }}
    })
    .await;
    let md_output = proa_core::render_md_to_string_async(
        md! { r###"{#[cfg(any())] if true { "hidden" }}{match true {
        #[cfg(any())] true => "hidden",
        _ => { for value in ["1", "2", "3"] {
            if value == "3" { break } else { value }
        } },
    }}{'outer: for value in ["3", "4"] {
        if value == "4" { break 'outer } else { value }
    }}"### },
    )
    .await;
    assert_eq!(html_output, "123");
    assert_eq!(md_output, html_output);
}

#[tokio::test]
async fn markdown_backtick_runs_respect_escapes_and_matching_lengths() {
    let value = "Ada";
    let sync =
        proa_core::render_to_string(md_sync! { r#"a `` {value} ` end; escaped \`{value}`"# });
    let asynchronous =
        proa_core::render_md_to_string_async(md! { r#"a `` {value} ` end; escaped \`{value}`"# })
            .await;
    assert_eq!(sync, r"a `` Ada ` end; escaped \`Ada`");
    assert_eq!(asynchronous, sync);
}

#[test]
fn rust_string_in_slot_and_literal_braces_are_distinct() {
    assert_eq!(
        proa_core::render_to_string(md_sync! { r#"{"Hello {name}"}"# }),
        "Hello {name}"
    );
    assert_eq!(
        proa_core::render_to_string(md_sync! { r#"{{"Hello {{name}}"}}"# }),
        r#"{"Hello {name}"}"#
    );
}

#[tokio::test]
async fn typed_async_formats_keep_borrowed_writer_support() {
    let input = String::from("<&*");
    let mut bytes = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut bytes);
        WebRender::render(
            html! {
                <p>{Delayed(&input)}</p>
            },
            &mut cx,
        )
        .resolve()
        .await
        .unwrap();
        MdRender::render_md(md! { "{Delayed(&input)}" }, &mut cx)
            .resolve()
            .await
            .unwrap();
    }
    assert_eq!(bytes, br"<p>&lt;&amp;*</p>\<\&\*");
}
