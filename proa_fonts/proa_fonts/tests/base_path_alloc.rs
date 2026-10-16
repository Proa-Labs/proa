// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::stack::WriteCursor;
use proa_core::{BasePath, WebContext, WriteBuf};
use proa_fonts::{Font, FontFace};

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

const FONT: Font = Font {
    family: "Fixture",
    class_name: "fixture",
    variable: "--font-fixture",
    family_stack: "Fixture,sans-serif",
    faces: &[FontFace::woff2("/fonts/fixture.woff2", 400).preloaded()],
    css: "@font-face{font-family:Fixture;src:url(/fonts/fixture.woff2) format('woff2')}",
};

#[test]
fn mounted_font_head_writes_preload_and_css_without_allocations() {
    let mut backing = [0u8; 1024];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);
    cx.request_mut()
        .set_base_path(BasePath::new("/user/demo/proxy/3000").unwrap());

    start_counting();
    FONT.head::<_>(&mut cx).unwrap();
    let allocations = stop_counting();
    assert_eq!(allocations, 0);
    drop(cx);
    let html = std::str::from_utf8(cursor.as_slice()).unwrap();
    assert!(html.contains("href=\"/user/demo/proxy/3000/fonts/fixture.woff2\""));
    assert!(html.contains("src:url(/user/demo/proxy/3000/fonts/fixture.woff2)"));
}
