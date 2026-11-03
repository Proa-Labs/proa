// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, MdRender, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
use proa_macros::{html, html_sync, md, md_sync, xml, xml_sync};
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

#[test]
fn sync_formats_stream_loop_bodies_and_render_break_values_once() {
    macro_rules! fragment {
        ($template:ident) => {
            $template! {
                {{
                    let mut count = 0;
                    'rows: loop {
                        if count == 2 { break 'rows "end&"; }
                        count += 1;
                        "row&"
                    }
                }}
            }
        };
    }
    let html = proa_core::render_to_string(fragment!(html_sync));
    let xml = proa_core::render_to_string(fragment!(xml_sync));
    let markdown = proa_core::render_to_string(fragment!(md_sync));
    assert_eq!(html, "row&amp;row&amp;end&amp;");
    assert_eq!(xml, html);
    assert_eq!(markdown, r"row\&row\&end\&");
    assert_eq!(
        proa_core::render_to_string(md_sync! { r#"{loop { break "done"; }}"# }),
        "done"
    );
}

#[tokio::test]
async fn async_formats_await_loop_break_values() {
    macro_rules! fragment {
        ($template:ident) => {
            $template! {
                {{
                    let mut count = 0;
                    'rows: loop {
                        if count == 2 { break 'rows Delayed("end&"); }
                        count += 1;
                        Delayed("row&")
                    }
                }}
            }
        };
    }
    let html = proa_core::render_to_string_async(fragment!(html)).await;
    let xml = proa_core::render_to_string_async(fragment!(xml)).await;
    let markdown = proa_core::render_md_to_string_async(fragment!(md)).await;
    assert_eq!(html, "row&amp;row&amp;end&amp;");
    assert_eq!(xml, html);
    assert_eq!(markdown, r"row\&row\&end\&");
}

fn returned_html(title: String) -> impl WebRender {
    html! {
        <p>{Delayed(title.as_str())}</p>
    }
}

fn returned_xml(title: String) -> impl WebRender {
    xml! {
        <title>{Delayed(title.as_str())}</title>
    }
}

fn returned_markdown<L: DataLoader, B: WriteBuf>(title: String) -> impl MdRender<L, B> {
    md! { "# {Delayed(title.as_str())}" }
}

#[tokio::test]
async fn returned_async_templates_own_inputs_across_pending_children() {
    let input = "<&*";
    let html = returned_html(input.to_owned());
    let xml = returned_xml(input.to_owned());
    let markdown = returned_markdown(input.to_owned());

    assert_eq!(
        proa_core::render_to_string_async(html).await,
        "<p>&lt;&amp;*</p>"
    );
    assert_eq!(
        proa_core::render_to_string_async(xml).await,
        "<title>&lt;&amp;*</title>"
    );
    assert_eq!(
        proa_core::render_md_to_string_async(markdown).await,
        "# \\<\\&\\*"
    );
}

#[test]
fn sync_formats_render_labeled_block_results_once() {
    for early in [true, false] {
        let first = "<&*";
        let fallback = "other&*";
        let html = proa_core::render_to_string(html_sync! {
            {'pick: {
                if early { break 'pick first; }
                fallback
            }}
        });
        let xml = proa_core::render_to_string(xml_sync! {
            {'pick: {
                if early { break 'pick first; }
                fallback
            }}
        });
        let markdown = proa_core::render_to_string(md_sync! { r#"{'pick: {
            if early { break 'pick first; }
            fallback
        }}"# });

        assert_eq!(html, if early { "&lt;&amp;*" } else { "other&amp;*" });
        assert_eq!(xml, html);
        assert_eq!(markdown, if early { r"\<\&\*" } else { r"other\&\*" });
    }
}

#[tokio::test]
async fn async_formats_render_labeled_block_results_once() {
    for early in [true, false] {
        let first = "<&*";
        let fallback = "other&*";
        let html = proa_core::render_to_string_async(html! {
            {'pick: {
                if early { break 'pick Delayed(first); }
                Delayed(fallback)
            }}
        })
        .await;
        let xml = proa_core::render_to_string_async(xml! {
            {'pick: {
                if early { break 'pick Delayed(first); }
                Delayed(fallback)
            }}
        })
        .await;
        let markdown = proa_core::render_md_to_string_async(md! { r#"{'pick: {
            if early { break 'pick Delayed(first); }
            Delayed(fallback)
        }}"# })
        .await;

        assert_eq!(html, if early { "&lt;&amp;*" } else { "other&amp;*" });
        assert_eq!(xml, html);
        assert_eq!(markdown, if early { r"\<\&\*" } else { r"other\&\*" });
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
    let values = &values;
    let html_output = proa_core::render_to_string_async(html! {
        {for value in values {
            let local = value.repeat(2);
            let borrowed = local.as_str();
            html! { <p>{Delayed(borrowed)}</p> }
        }}
    })
    .await;
    let md_output = proa_core::render_md_to_string_async(md! { r###"{for value in values {
        let local = value.repeat(2);
        let borrowed = local.as_str();
        md! { "{Delayed(borrowed)}\n" }
    }}"### })
    .await;
    assert_eq!(html_output, "<p>aa</p><p>bb</p>");
    assert_eq!(md_output, "aa\nbb\n");
}

#[tokio::test]
async fn local_async_formats_allow_non_send_captures() {
    let value = Rc::new(String::from("local"));
    let value = &value;
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
    let input = input.as_str();
    let mut bytes = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut bytes);
        WebRender::render(
            html! {
                <p>{Delayed(input)}</p>
            },
            &mut cx,
        )
        .resolve()
        .await
        .unwrap();
        MdRender::render_md(md! { "{Delayed(input)}" }, &mut cx)
            .resolve()
            .await
            .unwrap();
    }
    assert_eq!(bytes, br"<p>&lt;&amp;*</p>\<\&\*");
}

#[test]
#[deny(dependency_on_unit_never_type_fallback)]
#[allow(unreachable_code)]
fn diverging_loops_do_not_require_a_renderable_never_type() {
    use proa_core::WebRenderSync;
    fn fail() -> Result<(), WriteError> {
        Err(WriteError::OutOfCapacity)
    }
    macro_rules! check {
        ($template:ident) => {{
            let mut cx = WebContext::<proa_core::NoLoader>::new();
            let result = WebRenderSync::render(
                $template! { {loop {
                    loop { break; }
                    fail()?;
                }} },
                &mut cx,
            );
            assert_eq!(result, Err(WriteError::OutOfCapacity));
            let mut cx = WebContext::<proa_core::NoLoader>::new();
            let result = WebRenderSync::render(
                $template! { {loop { return Err(WriteError::OutOfCapacity); }} },
                &mut cx,
            );
            assert_eq!(result, Err(WriteError::OutOfCapacity));
        }};
    }
    check!(html_sync);
    check!(xml_sync);
    check!(md_sync);
}

#[tokio::test]
#[deny(dependency_on_unit_never_type_fallback)]
#[allow(unreachable_code)]
async fn diverging_loops_stream_nested_templates_before_propagating_errors() {
    use proa_core::WebRenderSync;
    fn next_row(count: &mut usize) -> Result<(), WriteError> {
        *count += 1;
        if *count > 1 {
            return Err(WriteError::OutOfCapacity);
        }
        Ok(())
    }
    macro_rules! fragment {
        ($template:ident, $child:ident) => {
            $template! { {{
                let mut count = 0;
                loop {
                    next_row(&mut count)?;
                    ::proa_macros::$child! { "row" }
                }
            }} }
        };
    }
    macro_rules! sync {
        ($template:ident) => {{
            let mut cx = WebContext::<proa_core::NoLoader>::new();
            let result = WebRenderSync::render(fragment!($template, $template), &mut cx);
            assert_eq!(result, Err(WriteError::OutOfCapacity));
            assert_eq!(cx.into_output().into_string(), "row");
        }};
    }
    sync!(html_sync);
    sync!(xml_sync);
    sync!(md_sync);
    let mut cx = WebContext::<proa_core::NoLoader>::new();
    let result = WebRender::render(fragment!(html, html), &mut cx)
        .resolve()
        .await;
    assert_eq!(result, Err(WriteError::OutOfCapacity));
    assert_eq!(cx.into_output().into_string(), "row");
    let mut cx = WebContext::<proa_core::NoLoader>::new();
    let result = WebRender::render(fragment!(xml, xml), &mut cx)
        .resolve()
        .await;
    assert_eq!(result, Err(WriteError::OutOfCapacity));
    assert_eq!(cx.into_output().into_string(), "row");
    let mut cx = WebContext::<proa_core::NoLoader>::new();
    let result = MdRender::render_md(fragment!(md, md), &mut cx)
        .resolve()
        .await;
    assert_eq!(result, Err(WriteError::OutOfCapacity));
    assert_eq!(cx.into_output().into_string(), "row");
}

#[tokio::test]
#[allow(unreachable_code)]
async fn loop_exit_values_respect_iterator_scope_and_macro_expansion() {
    macro_rules! leave {
        () => {
            break "macro&"
        };
    }
    macro_rules! fragment {
        ($template:ident) => {
            $template! {
                {loop {
                    for _ in { break "iterator&"; [0] } {}
                }}
                {loop { leave!(); }}
            }
        };
    }
    let expected = "iterator&amp;macro&amp;";
    assert_eq!(proa_core::render_to_string(fragment!(html_sync)), expected);
    assert_eq!(proa_core::render_to_string(fragment!(xml_sync)), expected);
    assert_eq!(
        proa_core::render_to_string_async(fragment!(html)).await,
        expected
    );
    assert_eq!(
        proa_core::render_to_string_async(fragment!(xml)).await,
        expected
    );
    let expected = "iterator\\&\n\nmacro\\&";
    assert_eq!(proa_core::render_to_string(fragment!(md_sync)), expected);
    assert_eq!(
        proa_core::render_md_to_string_async(fragment!(md)).await,
        expected
    );
}

#[tokio::test]
async fn user_macros_with_render_macro_names_keep_loop_break_values() {
    macro_rules! check {
        ($name:ident) => {{
            macro_rules! $name {
                () => {
                    break "done&"
                };
            }
            let expected = "done&amp;";
            assert_eq!(
                proa_core::render_to_string(::proa_macros::html_sync! { {loop { $name!(); }} }),
                expected
            );
            assert_eq!(
                proa_core::render_to_string(::proa_macros::xml_sync! { {loop { $name!(); }} }),
                expected
            );
            assert_eq!(
                proa_core::render_to_string_async(::proa_macros::html! { {loop { $name!(); }} })
                    .await,
                expected
            );
            assert_eq!(
                proa_core::render_to_string_async(::proa_macros::xml! { {loop { $name!(); }} })
                    .await,
                expected
            );
            let expected = r"done\&";
            assert_eq!(
                proa_core::render_to_string(::proa_macros::md_sync! { {loop { $name!(); }} }),
                expected
            );
            assert_eq!(
                proa_core::render_md_to_string_async(::proa_macros::md! { {loop { $name!(); }} })
                    .await,
                expected
            );
        }};
    }
    check!(md);
    check!(md_sync);
    check!(html);
    check!(html_sync);
    check!(xml);
    check!(xml_sync);
}
