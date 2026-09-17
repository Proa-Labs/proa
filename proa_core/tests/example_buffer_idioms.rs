// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Pins the buffer idioms shown in the authoring templates and READMEs:
//! examples default to `RenderBuffer` as the `WriteBuf`.
//!
//! If one of these tests breaks, the documented idiom broke with it.
//! Update together: CLAUDE.md, docs/agent/components.md,
//! .claude/skills/test-component/SKILL.md,
//! proa_framework/proa_framework_axum/README.md, and
//! proa_framework/proa_framework_core/README.md.

use proa_core::text::text;
use proa_core::{DataLoader, RenderBuffer, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

struct ExamplePage;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ExamplePage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <main class="container">
                <h1>"Welcome to Proa"</h1>
                {text("Hi")}
            </main>
        }
        .render(cx)
    }
}

/// The unit-test template from CLAUDE.md and the test-component skill.
#[test]
fn template_test_idiom_renders() {
    let mut out = RenderBuffer::new();
    let mut cx = WebContext::with_buffer(&mut out);
    ExamplePage.render(&mut cx).unwrap();
    let _rendered = out.into_string();
}

/// The minimal-adapter example from proa_framework_core/README.md:
/// staged response snapshot plus scatter-gather slices.
#[test]
fn adapter_idiom_staged_channel_and_slices() {
    let mut buf = RenderBuffer::new();
    let mut cx = WebContext::with_buffer(&mut buf);
    ExamplePage.render(&mut cx).unwrap();
    let staged = cx.response().clone();

    let status = staged.status().map(|s| s.as_u16()).unwrap_or(200);
    assert_eq!(status, 200);
    let _headers: Vec<(String, Vec<u8>)> = staged
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
        .collect();

    let gathered: usize = buf.slices().map(<[u8]>::len).sum();
    assert_eq!(gathered, buf.to_vec().len());
}

/// The criterion-bench idiom from the test-component skill: render_into
/// clears and renders, recycling pages across iterations.
#[test]
fn bench_idiom_render_into_reuses_buffer() {
    let mut buf = RenderBuffer::new();
    buf.render_into(ExamplePage);
    let first = buf.to_vec();
    buf.render_into(ExamplePage);
    assert_eq!(first, buf.to_vec());
}
