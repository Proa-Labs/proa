// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::stack::WriteCursor;
use proa_core::{BasePath, SendWebContext, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

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
fn context_urls_render_without_intermediate_allocations() {
    let base_path = BasePath::new("/user/demo/proxy/3000").unwrap();
    let mut backing = [0u8; 512];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);
    cx.request_mut().set_base_path(base_path);

    start_counting();
    let url = cx.url("/static/styles.css?v=1&mode=dark");
    html_sync! {
        <link rel="stylesheet" href={url} />
    }
    .render(&mut cx)
    .unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(cursor.as_slice(), br#"<link rel="stylesheet" href="/user/demo/proxy/3000/static/styles.css?v=1&amp;mode=dark">"#);
}

#[test]
fn send_context_urls_share_the_prefix_without_intermediate_allocations() {
    let mut cx = SendWebContext::new();
    cx.request_mut()
        .set_base_path(BasePath::new("/apps/demo").unwrap());
    let mut backing = [0u8; 128];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut output = WebContext::with_buffer(&mut cursor);

    start_counting();
    let url = cx.url("/products?q=1&sort=asc");
    // The URL does not retain a context borrow or depend on its later state.
    cx.request_mut().set_base_path(BasePath::default());
    html_sync! {
        <a href={url}>"Products"</a>
    }
    .render(&mut output)
    .unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(output);
    assert_eq!(
        cursor.as_slice(),
        br#"<a href="/apps/demo/products?q=1&amp;sort=asc">Products</a>"#
    );
}

#[test]
fn compiled_article_url_composition_does_not_allocate() {
    let mut backing = [0u8; 512];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);
    cx.request_mut()
        .set_base_path(BasePath::new("/apps/demo").unwrap());
    start_counting();
    proa_core::BasePathHtml(r#"<a href="/docs">Docs</a><img src='/image.svg'>"#)
        .render(&mut cx)
        .unwrap();
    let allocations = stop_counting();
    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(
        cursor.as_slice(),
        br#"<a href="/apps/demo/docs">Docs</a><img src='/apps/demo/image.svg'>"#
    );
}
