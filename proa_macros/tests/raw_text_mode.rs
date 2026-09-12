// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for raw-text content-mode masking on the macro path.
//!
//! `<script>` and `<style>` are raw-text elements. A dynamic `{expr}` child of
//! `<script>` must be escaped so it cannot break out of the element with a
//! `</script>` sequence (or transition the tokenizer via `<script`/`<!--`).
//! These tests confirm `html_sync!` switches the runtime content mode so the
//! sentinel masking in `proa_core::shared::escape` actually applies.

use proa_core::{render::DataLoader, Ctx, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

fn render(f: impl FnOnce(&mut Ctx)) -> String {
    let mut cx = Ctx::new();
    f(&mut cx);
    String::from_utf8(cx.into_output().into_vec()).unwrap()
}

#[test]
fn dynamic_script_child_cannot_close_the_element() {
    // A value that tries to break out of the <script>.
    let payload = "</script><img src=x onerror=alert(1)>";
    let html = render(|cx| {
        html_sync! {
            <script>{payload}</script>
        }
        .render(cx)
        .unwrap();
    });
    // The literal closing sequence must not survive: it is masked to `<\/script`
    // (and the trailing `>` is escaped), so the element cannot be closed early.
    assert!(
        !html.contains("</script><img"),
        "raw </script> breakout survived: {html}"
    );
    assert!(
        html.contains(r"<\/script"),
        "expected masked close tag: {html}"
    );
}

#[test]
fn dynamic_script_child_masks_nested_open_and_comment() {
    let payload = "<script>x<!--y";
    let html = render(|cx| {
        html_sync! {
            <script>{payload}</script>
        }
        .render(cx)
        .unwrap();
    });
    // `<script` (open) and `<!--` (comment) transitions are masked.
    assert!(
        html.contains(r"<\script"),
        "expected masked open tag: {html}"
    );
    assert!(
        html.contains(r"<\!--"),
        "expected masked comment open: {html}"
    );
}

#[test]
fn dynamic_style_child_cannot_close_the_element() {
    let payload = "</style><script>alert(1)</script>";
    let html = render(|cx| {
        html_sync! {
            <style>{payload}</style>
        }
        .render(cx)
        .unwrap();
    });
    assert!(
        !html.contains("</style><script>"),
        "raw </style> breakout survived: {html}"
    );
    assert!(
        html.contains(r"<\/style"),
        "expected masked close tag: {html}"
    );
}

#[test]
fn mode_restored_after_script_element() {
    // After the <script> closes, a following text node must use normal HTML
    // text escaping again (`<` -> `&lt;`), proving the mode was restored.
    let inside = "a";
    let after = "<b>";
    let html = render(|cx| {
        html_sync! {
            <div>
                <script>{inside}</script>
                <span>{after}</span>
            </div>
        }
        .render(cx)
        .unwrap();
    });
    assert!(
        html.contains("&lt;b&gt;"),
        "mode not restored after script: {html}"
    );
}

struct FailingChild;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for FailingChild {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Err(WriteError::InvalidAttribute("synthetic failure"))
    }
}

#[test]
fn mode_restored_when_script_child_returns_error() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let err = html_sync! {
        <script>{FailingChild}</script>
    }
    .render(&mut cx);
    assert!(err.is_err());

    let sentinel = "</script>";
    html_sync! {
        <span>{sentinel}</span>
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.ends_with("<span>&lt;/script&gt;</span>"),
        "mode leaked after failed script child render: {html}"
    );
}

#[test]
fn static_only_script_passes_through_without_masking() {
    // A purely static script body skips the content-mode switch (no dynamic
    // children), so no sentinel-masking artifacts appear and the developer's
    // literal JS is emitted as-is.
    let html = render(|cx| {
        html_sync! {
            <script>"var x = 1; start();"</script>
        }
        .render(cx)
        .unwrap();
    });
    assert_eq!(html, "<script>var x = 1; start();</script>");
}
