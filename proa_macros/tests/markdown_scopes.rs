// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    DataLoader, MdRenderSync, MeteredWriteBuf, OutputMeter, StreamingZeroCopyBuf, WebContext,
    WriteBuf, WriteError, ZeroCopyBuf,
};
use proa_macros::{md, md_sync};
use std::{
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    task::{Context, Poll, Waker},
};

fn context() -> WebContext<proa_core::NoLoader, Vec<u8>> {
    WebContext::with_buffer(Vec::new())
}

#[test]
fn every_raw_delimiter_selects_the_same_block_mode() {
    macro_rules! check {
        ($template:expr) => {{
            let mut cx = context();
            $template.render_md(&mut cx).unwrap();
            assert!(!cx.md_inline());
            assert_eq!(cx.into_output(), b"## Heading\n\n");
        }};
    }
    check!(md_sync! { r"
        ## Heading
        " });
    check!(md_sync! { r#"
        ## Heading
        "# });
    check!(md_sync! { r##"
        ## Heading
        "## });
    check!(md_sync! { r###"
        ## Heading
        "### });
    check!(md_sync! { r####"
        ## Heading
        "#### });
    check!(md_sync! { r#####"
        ## Heading
        "##### });
}

#[test]
fn block_boundaries_only_add_missing_line_breaks() {
    for prefix in ["", "Intro", "Intro\n", "Intro\n\n", "Intro\n\n\n"] {
        let mut cx = context();
        // Authored whitespace must survive the inline scope verbatim.
        proa_core::md::TrustedMarkdown(prefix)
            .render_md(&mut cx)
            .unwrap();
        md_sync! { r#"
            ## Heading
            "#
        }
        .render_md(&mut cx)
        .unwrap();
        let expected = match prefix {
            "" => "## Heading\n\n",
            "Intro\n\n\n" => "Intro\n\n\n## Heading\n\n",
            _ => "Intro\n\n## Heading\n\n",
        };
        assert_eq!(cx.into_output(), expected.as_bytes());
    }

    let mut cx = context();
    md_sync! { r#"
        First
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    md_sync! { r#"
        Second
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"First\n\nSecond\n\n");
}

#[test]
fn inline_templates_preserve_sentences_and_authored_newlines() {
    let mut cx = context();
    let name = "Ada";
    md_sync! { "Hello " }.render_md(&mut cx).unwrap();
    md_sync! { "**{name}**" }.render_md(&mut cx).unwrap();
    md_sync! { ", welcome" }.render_md(&mut cx).unwrap();
    assert_eq!(cx.into_output(), b"Hello **Ada**, welcome");

    let mut cx = context();
    for item in ["Alpha", "Beta"] {
        md_sync! { "- {item}\n" }.render_md(&mut cx).unwrap();
    }
    assert_eq!(cx.into_output(), b"- Alpha\n- Beta\n");

    let mut cx = context();
    md_sync! { "First\n\n\nSecond" }.render_md(&mut cx).unwrap();
    assert_eq!(cx.into_output(), b"First\n\n\nSecond");
}

struct Mode(bool);
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Mode {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        assert_eq!(cx.md_inline(), self.0);
        Ok(())
    }
}

#[test]
fn scope_is_entered_only_when_rendered_and_restores_the_enclosing_mode() {
    let mut cx = context();
    let status = md_sync! { "**Ready**{Mode(true)}" };
    let block = md_sync! { r#"
        {Mode(false)}Status: {status}{Mode(false)}
        "#
    };
    assert!(!cx.md_inline());
    assert!(cx.out.is_empty());
    block.render_md(&mut cx).unwrap();
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Status: **Ready**\n\n");
}

#[test]
fn single_line_raw_and_multiline_cooked_literals_are_inline() {
    let mut cx = context();
    md_sync! { r"first{Mode(true)}" }
        .render_md(&mut cx)
        .unwrap();
    md_sync! { r###" second{Mode(true)}"### }
        .render_md(&mut cx)
        .unwrap();
    md_sync! { "third{Mode(true)}\nfourth\n" }
        .render_md(&mut cx)
        .unwrap();
    assert_eq!(cx.into_output(), b"first secondthird\nfourth\n");
}

#[test]
fn closure_renderers_built_without_the_macro_inherit_the_callers_mode() {
    let fragment = |expected_inline: bool| {
        proa_core::OptimizedMd::new(move |cx: &mut WebContext<proa_core::NoLoader, Vec<u8>>| {
            assert_eq!(cx.md_inline(), expected_inline);
            cx.write_md_prepared_static(b"**")
        })
    };
    let mut cx = context();
    fragment(false).render_md(&mut cx).unwrap();
    cx.md_set_inline(true);
    fragment(true).render_md(&mut cx).unwrap();
    assert!(cx.md_inline());
    assert_eq!(cx.into_output(), b"****");
}

#[test]
fn dynamic_prose_stays_escaped_and_other_formatting_is_inherited() {
    let mut cx = context();
    cx.md_push_list_depth();
    let label = "A * B";
    md_sync! { "{label}" }.render_md(&mut cx).unwrap();
    assert_eq!(cx.md_list_depth(), 1);
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), br"A \* B");
}

struct Fail(bool);

#[test]
fn generated_inline_context_bindings_do_not_capture_authored_names() {
    let __proa_md_inline_scope = "caller";
    let mut cx = context();
    md_sync! { "[{__proa_md_inline_scope}](/target)" }
        .render_md(&mut cx)
        .unwrap();
    assert_eq!(cx.into_output(), b"[caller](/target)");
}

#[test]
fn static_scoped_templates_keep_the_immediate_async_path() {
    let mut cx = context();
    assert!(matches!(
        proa_core::MdRender::render_md(md! { "Inline" }, &mut cx),
        proa_core::RenderOutcome::Done(Ok(()))
    ));
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Inline");
}
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Fail {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if self.0 {
            cx.write_md_prepared_static(b"Partial")?;
        }
        Err(WriteError::render("test child failed"))
    }
}

#[test]
fn catchable_errors_restore_mode_without_rolling_back_output() {
    for partial in [true, false] {
        for initial_inline in [true, false] {
            let mut cx = context();
            cx.md_set_inline(initial_inline);
            let result = md_sync! { "{Fail(partial)}" }.render_md(&mut cx);
            assert_eq!(result, Err(WriteError::render("test child failed")));
            assert_eq!(cx.md_inline(), initial_inline);
            cx.md_set_inline(false);
            md_sync! { r#"
                Fallback
                "#
            }
            .render_md(&mut cx)
            .unwrap();
            assert_eq!(
                cx.into_output(),
                if partial {
                    b"Partial\n\nFallback\n\n".as_slice()
                } else {
                    b"Fallback\n\n"
                }
            );
        }
    }
}

#[test]
fn failing_blocks_keep_the_leading_boundary_but_do_not_finish_the_block() {
    let mut cx = context();
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    let result = md_sync! { r#"
        {Fail(true)}
        "#
    }
    .render_md(&mut cx);
    assert!(result.is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.out, b"Intro\n\nPartial");
    md_sync! { r#"
        Fallback
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"Intro\n\nPartial\n\nFallback\n\n");
}

struct Recover;
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Recover {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        assert!(!cx.md_inline());
        assert!(md_sync! { "{Fail(true)}" }.render_md(cx).is_err());
        assert!(!cx.md_inline());
        cx.write_md_prepared_static(b" recovered")
    }
}

struct Panic;
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Panic {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.write_md_prepared_static(b"Partial")?;
        panic!("test child panicked")
    }
}

#[test]
fn nested_error_and_panic_scopes_restore_each_parent() {
    let mut cx = context();
    md_sync! { r#"
        {Recover}
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Partial recovered\n\n");

    for initial_inline in [false, true] {
        let mut cx = context();
        cx.md_set_inline(initial_inline);
        assert!(catch_unwind(AssertUnwindSafe(|| {
            md_sync! { "{Panic}" }.render_md(&mut cx)
        }))
        .is_err());
        assert_eq!(cx.md_inline(), initial_inline);
        assert_eq!(cx.into_output(), b"Partial");
    }

    let mut cx = context();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        md_sync! { r###"
            {md_sync! { "{Panic}" }}
            "### }
        .render_md(&mut cx)
    }))
    .is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Partial");
}

struct Block;
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Block {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r#"
            ## Heading
            "#
        }
        .render_md(cx)
    }
}

#[test]
fn typed_block_children_are_rejected_before_writing_their_boundary_or_body() {
    let mut cx = context();
    let block = md_sync! { r#"
        ## Heading
        "#
    };
    let error = md_sync! { "Label: {block}" }
        .render_md(&mut cx)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("block Markdown template inside an inline scope"));
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Label: ");

    let mut cx = context();
    cx.md_set_inline(true);
    assert!(Block.render_md(&mut cx).is_err());
    assert!(cx.md_inline());
    assert!(cx.out.is_empty());

    let mut cx = context();
    assert!(cx
        .md_link_with("/target", |cx| Block.render_md(cx))
        .is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"[");

    let mut cx = context();
    assert!(md_sync! { "| {Block} |\n" }.render_md(&mut cx).is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"| ");

    let mut cx = context();
    assert!(md_sync! { r#"
        [{Block}](/target)
        "# }
    .render_md(&mut cx)
    .is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"[");

    let mut cx = context();
    assert!(md_sync! { r#"
        | Title |
        | --- |
        | {Block} |
        "# }
    .render_md(&mut cx)
    .is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"| Title |\n| --- |\n| ");
}

#[test]
fn strikethrough_rejects_typed_blocks_and_accepts_inline_children() {
    let mut cx = context();
    let result = md_sync! { r#"
        ~~{Block}~~
        "#
    }
    .render_md(&mut cx);
    assert!(result.is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"~~");

    let mut cx = context();
    let status = md_sync! { "**Ready**" };
    md_sync! { r#"
        ~~{status}{Mode(true)}~~
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"~~**Ready**~~\n\n");
}

struct ForceBlock;

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for ForceBlock {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.md_set_inline(false);
        Block.render_md(cx)
    }
}

#[test]
fn a_manual_mode_toggle_cannot_override_an_active_inline_scope() {
    let mut cx = context();
    assert!(md_sync! { "{ForceBlock}" }.render_md(&mut cx).is_err());
    assert!(!cx.md_inline());
    assert!(cx.out.is_empty());
    Block.render_md(&mut cx).unwrap();
    assert_eq!(cx.into_output(), b"## Heading\n\n");
}

#[test]
fn link_label_helpers_restore_mode_on_unwinding() {
    let mut cx = context();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        cx.md_link_with("/target", |cx| Panic.render_md(cx))
    }))
    .is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"[Partial");
}

#[test]
fn document_rows_and_conditionals_preserve_all_authored_whitespace() {
    for in_stock in [true, false] {
        let title = "Widget";
        let features = ["Fast", "Lightweight"];
        let mut cx = context();
        let summary = md_sync! { r###"
            # {title}

            ## Features

            {for feature in features.iter() {
                md_sync! { "- {feature}\n" }
            }}

            {if in_stock {
                md_sync! { "**In stock**" }
            } else {
                md_sync! { "Backordered" }
            }}
            "###
        };
        assert!(cx.out.is_empty());
        assert!(!cx.md_inline());
        summary.render_md(&mut cx).unwrap();
        assert_eq!(
            cx.into_output(),
            if in_stock {
                b"# Widget\n\n## Features\n\n- Fast\n- Lightweight\n\n**In stock**\n\n".as_slice()
            } else {
                b"# Widget\n\n## Features\n\n- Fast\n- Lightweight\n\nBackordered\n\n"
            }
        );
    }
}

#[test]
fn repeated_nested_blocks_deduplicate_only_automatic_boundaries() {
    let entries = [
        ("Shipping", "Ships tomorrow"),
        ("Returns", "Returns accepted"),
    ];
    let mut cx = context();
    md_sync! { r###"
        {for (question, answer) in entries {
            md_sync! { r#"
                ### {question}

                {answer}

                "# }
        }}
        "###
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(
        cx.into_output(),
        b"### Shipping\n\nShips tomorrow\n\n### Returns\n\nReturns accepted\n\n"
    );
}

fn writer_contract<B: WriteBuf>(out: B) -> B {
    let mut cx = WebContext::with_buffer(out);
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    md_sync! { r#"
        First
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    md_sync! { r#"
        Second
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert!(!cx.md_inline());
    cx.into_output()
}

#[test]
fn contiguous_segmented_bounded_and_metered_writers_share_boundaries() {
    let expected = b"Intro\n\nFirst\n\nSecond\n\n";
    assert_eq!(writer_contract(Vec::new()), expected);
    assert_eq!(
        writer_contract(ZeroCopyBuf::with_capacity(8, 64)).to_vec(),
        expected
    );
    assert_eq!(
        writer_contract(StreamingZeroCopyBuf::new()).to_vec(),
        expected
    );
    let mut bytes = [0; 64];
    assert_eq!(
        writer_contract(proa_core::stack::WriteCursor::new(&mut bytes)).as_slice(),
        expected
    );
    let meter = OutputMeter::new(expected.len() as u64);
    assert_eq!(
        writer_contract(MeteredWriteBuf::new(Vec::new(), meter.borrow())).as_slice(),
        expected
    );
    assert_eq!(meter.remaining(), 0);
}

#[test]
fn capacity_failures_restore_mode_and_keep_partial_escaping_state() {
    let mut bytes = [0; 3];
    let mut cx = WebContext::with_buffer(proa_core::stack::WriteCursor::new(&mut bytes));
    let label = "A * B";
    assert_eq!(
        md_sync! { "{label}" }.render_md(&mut cx),
        Err(WriteError::OutOfCapacity)
    );
    assert!(!cx.md_inline());
    assert_eq!(cx.out.as_slice(), b"A \\");

    let meter = OutputMeter::new(3);
    let mut cx = WebContext::with_buffer(MeteredWriteBuf::new(Vec::new(), meter.borrow()));
    assert_eq!(
        md_sync! { "{label}" }.render_md(&mut cx),
        Err(WriteError::OutputLimitExceeded {
            requested: 1,
            remaining: 0
        })
    );
    assert!(!cx.md_inline());
    // Transfer the actual suffix to an unrestricted writer to exercise recovery.
    let state = cx.md_state();
    let bytes = cx.into_output().to_vec();
    let mut cx = WebContext::with_buffer(bytes).with_md_state(state);
    md_sync! { r#"
        Fallback
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"A \\\n\nFallback\n\n");
}

#[test]
fn leading_and_trailing_boundary_failures_preserve_the_original_error() {
    for budget in [0, 1] {
        let meter = OutputMeter::new(budget);
        let mut out = Vec::new();
        out.extend_from_slice(b"Intro");
        let mut cx = WebContext::with_buffer(MeteredWriteBuf::new(out, meter.borrow()));
        // The prepared write is how all Markdown writers establish suffix state.
        cx.write_md_prepared_static(b"").unwrap();
        // Seed a matching state without charging this writer for the prefix.
        let mut seed = context();
        seed.write_md_prepared_static(b"Intro").unwrap();
        cx.set_md_state(seed.md_state());
        let result = md_sync! { r#"
            Body
            "#
        }
        .render_md(&mut cx);
        assert_eq!(
            result,
            Err(WriteError::OutputLimitExceeded {
                requested: 2,
                remaining: budget
            })
        );
        assert!(!cx.md_inline());
        assert_eq!(cx.out.as_slice(), b"Intro");
    }
    let meter = OutputMeter::new(5);
    let mut cx = WebContext::with_buffer(MeteredWriteBuf::new(Vec::new(), meter.borrow()));
    let result = md_sync! { r#"
        Body
        "#
    }
    .render_md(&mut cx);
    assert_eq!(
        result,
        Err(WriteError::OutputLimitExceeded {
            requested: 1,
            remaining: 0
        })
    );
    assert!(!cx.md_inline());
    assert_eq!(cx.out.as_slice(), b"Body\n");
}

#[test]
fn empty_and_whitespace_only_templates_have_explicit_byte_contracts() {
    let mut cx = context();
    md_sync! { "" }.render_md(&mut cx).unwrap();
    md_sync! { r"" }.render_md(&mut cx).unwrap();
    md_sync! { r"
    " }
    .render_md(&mut cx)
    .unwrap();
    // Whitespace-only literals keep their closing-line indentation as content,
    // matching the existing parser rather than introducing a dedent change.
    assert_eq!(cx.into_output(), b"\n    \n\n");

    let mut cx = context();
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    md_sync! { r#"
        {for _ in std::iter::empty::<()>() { "unused" }}
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"Intro\n\n");

    let mut cx = context();
    md_sync! { r#"{for _ in std::iter::empty::<()>() {
        "unused"
    }}"# }
    .render_md(&mut cx)
    .unwrap();
    assert!(cx.out.is_empty());
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    md_sync! { r#"{for _ in std::iter::empty::<()>() {
        "unused"
    }}"# }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"Intro\n\n");
}

struct PendingPartial;
impl<L: DataLoader, B: WriteBuf> proa_core::MdRender<L, B> for PendingPartial {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::Pending(async move {
            cx.write_md_prepared_static(b"Partial")?;
            std::future::pending().await
        })
    }
}

#[test]
fn async_scope_restores_mode_on_cancellation_without_a_trailing_boundary() {
    for inline in [false, true] {
        let mut cx = context();
        cx.md_set_inline(inline);
        let mut future =
            Box::pin(proa_core::MdRender::render_md(md! { "{PendingPartial}" }, &mut cx).resolve());
        assert!(future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        drop(future);
        assert_eq!(cx.md_inline(), inline);
        assert_eq!(cx.into_output(), b"Partial");
    }
    let mut cx = context();
    let mut future = Box::pin(
        proa_core::MdRender::render_md(
            md! { r#"
                {PendingPartial}
                "#
            },
            &mut cx,
        )
        .resolve(),
    );
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    drop(future);
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Partial");

    let mut cx = context();
    let mut future = Box::pin(
        proa_core::MdRender::render_md(
            md! { r###"
                {md! { "{PendingPartial}" }}
                "###
            },
            &mut cx,
        )
        .resolve(),
    );
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    drop(future);
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Partial");
}

#[test]
fn dropping_an_unpolled_async_template_does_not_enter_its_scope() {
    let mut cx = context();
    let template = md! { "{PendingPartial}" };
    assert!(!cx.md_inline());
    assert!(cx.out.is_empty());
    drop(proa_core::MdRender::render_md(template, &mut cx));
    assert!(!cx.md_inline());
    assert!(cx.out.is_empty());
}

struct AsyncFailure(bool);
impl<L: DataLoader, B: WriteBuf> proa_core::MdRender<L, B> for AsyncFailure {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::Pending(async move {
            cx.write_md_prepared_static(b"Partial")?;
            tokio::task::yield_now().await;
            if self.0 {
                panic!("async child panicked");
            }
            Err(WriteError::render("async child failed"))
        })
    }
}

#[test]
fn async_errors_and_unwinding_restore_nested_scopes() {
    for panic in [false, true] {
        let mut cx = context();
        let mut future = Box::pin(
            proa_core::MdRender::render_md(
                md! { r###"
                    {md! { "{AsyncFailure(panic)}" }}
                    "###
                },
                &mut cx,
            )
            .resolve(),
        );
        let mut task = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut task).is_pending());
        let result = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut task)));
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.unwrap(),
                Poll::Ready(Err(WriteError::render("async child failed")))
            );
        }
        drop(future);
        assert!(!cx.md_inline());
        assert_eq!(cx.into_output(), b"Partial");
    }
}

#[tokio::test]
async fn async_templates_share_the_sync_literal_mode_contract() {
    let mut cx = context();
    proa_core::MdRender::render_md(md! { "Intro" }, &mut cx)
        .resolve()
        .await
        .unwrap();
    let label = "A * B";
    proa_core::MdRender::render_md(
        md! { r#"
            {label}
            "#
        },
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Intro\n\nA \\* B\n\n");
}

#[tokio::test]
async fn async_link_labels_reject_blocks_after_suspension_and_restore_mode() {
    let mut cx = context();
    let block = md! { r#"
        ## Heading
        "#
    };
    let result = proa_core::MdRender::render_md(
        md! { r###"
            [{if { tokio::task::yield_now().await; true } { block }}](/target)
            "###
        },
        &mut cx,
    )
    .resolve()
    .await;
    assert!(result.is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"[");
}

#[tokio::test]
async fn async_strikethrough_rejects_typed_blocks_after_suspension() {
    let mut cx = context();
    let block = proa_core::to_md_async(Block);
    let result = proa_core::MdRender::render_md(
        md! { r#"
            ~~{if { tokio::task::yield_now().await; true } { block }}~~
            "#
        },
        &mut cx,
    )
    .resolve()
    .await;
    assert!(result.is_err());
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"~~");
}

struct BridgedBlock;
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for BridgedBlock {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_core::WebRenderSync::render(
            md_sync! { r#"
                ## Heading
                "#
            },
            cx,
        )
    }
}

#[test]
fn web_bridged_blocks_obey_inline_scopes_and_keep_boundaries_continuous() {
    // The inline rule applies to a block reached through the Web-trait bridge.
    let mut cx = context();
    let result = md_sync! { "Label: [{BridgedBlock}](/target) end" }.render_md(&mut cx);
    assert_eq!(
        result,
        Err(WriteError::render(
            "cannot render a block Markdown template inside an inline scope; use an ordinary-string inline template"
        ))
    );
    assert!(!cx.md_inline());
    assert_eq!(cx.into_output(), b"Label: [");

    // Suffix tracking carries into the bridge (one boundary before the block)
    // and back out (no duplicate boundary after it).
    let mut cx = context();
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    proa_core::WebRenderSync::render(
        md_sync! { r#"
            ## Heading
            "#
        },
        &mut cx,
    )
    .unwrap();
    md_sync! { r#"
        Next
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"Intro\n\n## Heading\n\nNext\n\n");

    // The async bridge shares the same scope.
    let mut cx = context();
    md_sync! { "Intro" }.render_md(&mut cx).unwrap();
    let mut future = Box::pin(
        proa_core::WebRender::render(
            md! { r#"
                ## Heading
                "#
            },
            &mut cx,
        )
        .resolve(),
    );
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    );
    drop(future);
    md_sync! { r#"
        Next
        "#
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(cx.into_output(), b"Intro\n\n## Heading\n\nNext\n\n");
}

#[test]
fn line_breaks_after_block_position_slots_are_boundaries() {
    struct Quote;
    impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Quote {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            md_sync! { r#"
                > quoted
                "#
            }
            .render_md(cx)
        }
    }
    fn render(template: impl MdRenderSync<proa_core::NoLoader, Vec<u8>>) -> Vec<u8> {
        let mut cx = context();
        template.render_md(&mut cx).unwrap();
        cx.into_output()
    }

    // A block child on its own line keeps exactly one blank line after it,
    // whatever follows: the end of the template, prose, or another child.
    assert_eq!(
        render(md_sync! { r###"
            {Quote}
            "###
        }),
        b"> quoted\n\n"
    );
    assert_eq!(
        render(md_sync! { r###"
            {Quote}

            After
            "###
        }),
        b"> quoted\n\nAfter\n\n"
    );
    assert_eq!(
        render(md_sync! { r###"
            {Quote}

            {Quote}
            "###
        }),
        b"> quoted\n\n> quoted\n\n"
    );

    // Row templates on consecutive lines stay one list.
    let (a, b) = ("a", "b");
    assert_eq!(
        render(md_sync! { r###"
            {md_sync! { "- {a}\n" }}
            {md_sync! { "- {b}\n" }}
            "###
        }),
        b"- a\n- b\n\n"
    );

    // Inline text still receives its authored break and blank line.
    let name = "Ada";
    assert_eq!(
        render(md_sync! { r###"
            {name}
            next

            {name}
            "###
        }),
        b"Ada\nnext\n\nAda\n\n"
    );

    // An absent value leaves no stray blank lines between paragraphs.
    let none: Option<&str> = None;
    assert_eq!(
        render(md_sync! { r###"
            Intro

            {none}

            Next
            "###
        }),
        b"Intro\n\nNext\n\n"
    );

    // Further authored blank lines stay literal, as does every break inside
    // an inline template.
    assert_eq!(
        render(md_sync! { r###"
            {name}


            Next
            "###
        }),
        b"Ada\n\n\nNext\n\n"
    );
    let row = md_sync! { "- a\n" };
    assert_eq!(render(md_sync! { "{row}\n" }), b"- a\n\n");
}

#[test]
fn deeply_nested_block_children_add_no_extra_blank_lines() {
    struct Leaf;
    impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Leaf {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            md_sync! { r"
                > deep
                " }
            .render_md(cx)
        }
    }
    // A block template wrapping one child on its own line.
    struct Wrap<C>(C);
    impl<L: DataLoader, B: WriteBuf, C: MdRenderSync<L, B>> MdRenderSync<L, B> for Wrap<C> {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let child = self.0;
            md_sync! { r#"
                {child}
                "# }
            .render_md(cx)
        }
    }
    // Heading, child, prose: the common section shape.
    struct Section<C>(&'static str, C);
    impl<L: DataLoader, B: WriteBuf, C: MdRenderSync<L, B>> MdRenderSync<L, B> for Section<C> {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let (title, child) = (self.0, self.1);
            md_sync! { r#"
                ## {title}

                {child}

                after {title}
                "# }
            .render_md(cx)
        }
    }
    // An imperative wrapper: boundary, children, boundary.
    struct Card<C>(C);
    impl<L: DataLoader, B: WriteBuf, C: MdRenderSync<L, B>> MdRenderSync<L, B> for Card<C> {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.md_block_boundary()?;
            self.0.render_md(cx)?;
            cx.md_block_boundary()
        }
    }
    // Rows, a nested block child, rows again.
    struct Mixed<C>(C);
    impl<L: DataLoader, B: WriteBuf, C: MdRenderSync<L, B>> MdRenderSync<L, B> for Mixed<C> {
        fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let child = self.0;
            let rows = ["a", "b"];
            md_sync! { r#"
                {for row in rows { md_sync! { "- {row}\n" } }}

                {child}

                {for row in rows { md_sync! { "- {row}\n" } }}
                "# }
            .render_md(cx)
        }
    }
    fn render(template: impl MdRenderSync<proa_core::NoLoader, Vec<u8>>) -> String {
        let mut cx = context();
        template.render_md(&mut cx).unwrap();
        let out = String::from_utf8(cx.into_output()).unwrap();
        assert!(!out.contains("\n\n\n"), "extra blank line in {out:?}");
        out
    }

    assert_eq!(render(Wrap(Wrap(Wrap(Wrap(Leaf))))), "> deep\n\n");
    assert_eq!(render(Card(Card(Card(Leaf)))), "> deep\n\n");
    assert_eq!(
        render(Section("one", Section("two", Section("three", Leaf)))),
        "## one\n\n## two\n\n## three\n\n> deep\n\nafter three\n\nafter two\n\nafter one\n\n"
    );
    assert_eq!(
        render(Card(Section("s", Card(Wrap(Leaf))))),
        "## s\n\n> deep\n\nafter s\n\n"
    );
    assert_eq!(
        render(Mixed(Mixed(Leaf))),
        "- a\n- b\n\n- a\n- b\n\n> deep\n\n- a\n- b\n\n- a\n- b\n\n"
    );
    assert_eq!(render(Wrap(Wrap(md_sync! { "**bold**" }))), "**bold**\n\n");
    let items = [1, 2];
    assert_eq!(
        render(md_sync! { r###"
            {for _ in items {
                Section("outer", md_sync! { r#"
                    {for _ in items {
                        Wrap(Section("inner", Leaf))
                    }}
                    "# })
            }}
            {Wrap(Leaf)}
            {Wrap(Leaf)}
            "###
        }),
        "## outer\n\n## inner\n\n> deep\n\nafter inner\n\n## inner\n\n> deep\n\nafter inner\n\nafter outer\n\n\
         ## outer\n\n## inner\n\n> deep\n\nafter inner\n\n## inner\n\n> deep\n\nafter inner\n\nafter outer\n\n\
         > deep\n\n> deep\n\n"
    );
}
