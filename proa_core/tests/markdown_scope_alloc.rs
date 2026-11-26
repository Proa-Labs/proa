// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use proa_core::{BumpWriter, MdRenderSync, WebContext};
use proa_macros::md_sync;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct CountAlloc;
thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[test]
fn literal_mode_scopes_and_rejection_do_not_allocate() {
    let mut cx = WebContext::with_buffer(BumpWriter::<256>::new());
    COUNT.with(|count| count.set(Some(0)));
    let label = "A * B";
    let result = md_sync! { r###"
        # {label}

        {md_sync! { "**Inline**" }}
        "###
    }
    .render_md(&mut cx);
    let block = md_sync! { r#"
        Rejected
        "#
    };
    let rejected = md_sync! { "{block}" }.render_md(&mut cx);
    let allocations = COUNT.with(|count| count.replace(None).unwrap());
    assert!(result.is_ok());
    assert!(rejected.is_err());
    assert!(!cx.md_inline());
    assert_eq!(allocations, 0);
}
