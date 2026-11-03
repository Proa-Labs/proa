// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{BumpWriter, MdRenderSync, NoLoader, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;
use std::{
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    task::{Context, Waker},
};

#[test]
fn nested_scopes_restore_immediate_offsets_without_resetting_other_settings() {
    let mut cx = WebContext::<NoLoader, _>::with_buffer(Vec::new());
    cx.md_push_heading_offset(2);
    {
        let mut parent = cx.md_heading_scope(1);
        assert_eq!(parent.context().md_heading_offset(), 3);
        {
            let mut child = parent.context().md_heading_scope(255);
            assert_eq!(child.context().md_heading_offset(), 255);
            child.context().md_push_list_depth();
            md_sync! { "**Inline**" }
                .render_md(child.context())
                .unwrap();
            child.context().md_block_boundary().unwrap();
            child.context().md_heading(1, "Clamped").unwrap();
        }
        assert_eq!(parent.context().md_heading_offset(), 3);
        assert_eq!(parent.context().md_list_depth(), 1);
        assert!(!parent.context().md_inline());
    }
    assert_eq!(cx.md_heading_offset(), 2);
    assert_eq!(cx.md_list_depth(), 1);
    assert_eq!(cx.into_output(), b"**Inline**\n\n###### Clamped\n\n");
}

fn partial(cx: &mut WebContext<NoLoader, Vec<u8>>, panic: bool) -> Result<(), WriteError> {
    let mut scope = cx.md_heading_scope(1);
    assert_eq!(scope.context().md_heading_offset(), 3);
    scope.context().write_md_prepared_static(b"Partial")?;
    if panic {
        panic!("injected heading child panic");
    }
    Err(WriteError::render("injected heading child error"))
}

#[test]
fn errors_and_unwinding_keep_bytes_and_suffix_tracking() {
    for panic in [false, true] {
        let mut cx = WebContext::with_buffer(Vec::new());
        cx.md_push_heading_offset(2);
        let result = catch_unwind(AssertUnwindSafe(|| partial(&mut cx, panic)));
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.unwrap(),
                Err(WriteError::render("injected heading child error"))
            );
        }
        assert_eq!(cx.md_heading_offset(), 2);
        cx.md_block_boundary().unwrap();
        cx.md_heading(1, "Fallback").unwrap();
        assert_eq!(cx.into_output(), b"Partial\n\n### Fallback\n\n");
    }
}

#[test]
fn cancelled_awaiting_scope_restores_the_offset() {
    let mut cx = WebContext::<NoLoader, _>::with_buffer(Vec::new());
    cx.md_push_heading_offset(1);
    let mut future = Box::pin(async {
        let mut scope = cx.md_heading_scope(2);
        scope
            .context()
            .write_md_prepared_static(b"Partial")
            .unwrap();
        std::future::pending::<()>().await;
    });
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    drop(future);
    assert_eq!(cx.md_heading_offset(), 1);
    cx.md_block_boundary().unwrap();
    assert_eq!(cx.into_output(), b"Partial\n\n");
}

#[test]
fn capacity_failure_restores_offset_and_preserves_the_original_error() {
    let mut cx = WebContext::<NoLoader, _>::with_buffer(BumpWriter::<3>::new());
    let result = (|| {
        let mut scope = cx.md_heading_scope(1);
        scope.context().write_md_prepared_static(b"A")?;
        scope.context().write_md_prepared_static(b"Long")
    })();
    assert_eq!(result, Err(WriteError::OutOfCapacity));
    assert_eq!(cx.md_heading_offset(), 0);
    cx.md_block_boundary().unwrap();
    assert_eq!(cx.out.as_slice(), b"A\n\n");
}
