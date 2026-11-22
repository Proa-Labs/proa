// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `bool` and `char` children are render-path primitives: every HTML,
//! attribute, Markdown, and async-bridge path must write straight into the
//! output buffer without touching the heap.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::shared::attr_value::AttrValue;
use proa_core::stack::WriteCursor;
use proa_core::{MdRenderSync, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteBuf};

struct ThreadCountingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

#[inline]
fn record_allocation() {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn start_counting() {
    ALLOCATIONS.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
}

fn stop_counting() -> usize {
    TRACKING.with(|tracking| tracking.set(false));
    ALLOCATIONS.with(Cell::get)
}

#[test]
fn bool_and_char_render_paths_allocate_nothing() {
    let mut backing = [0u8; 64];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);

    start_counting();
    WebRenderSync::render(true, &mut cx).unwrap();
    WebRenderSync::render('<', &mut cx).unwrap();
    WebRenderSync::render('😀', &mut cx).unwrap();
    MdRenderSync::render_md(false, &mut cx).unwrap();
    MdRenderSync::render_md('*', &mut cx).unwrap();
    '"'.render_attr_value(&mut cx).unwrap();
    // The async surface completes immediately instead of building a future.
    let char_done = matches!(WebRender::render('é', &mut cx), RenderOutcome::Done(Ok(())));
    let bool_done = matches!(
        WebRender::render(true, &mut cx),
        RenderOutcome::Done(Ok(()))
    );
    let int_done = matches!(
        WebRender::render(7_u8, &mut cx),
        RenderOutcome::Done(Ok(()))
    );
    let float_done = matches!(
        WebRender::render(-0.5_f64, &mut cx),
        RenderOutcome::Done(Ok(()))
    );
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    assert_eq!([char_done, bool_done, int_done, float_done], [true; 4]);
    drop(cx);
    assert_eq!(
        std::str::from_utf8(cursor.as_slice()).unwrap(),
        "true&lt;😀false\\*&quot;étrue7-0.5"
    );
}
