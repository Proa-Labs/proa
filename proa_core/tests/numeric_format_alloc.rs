// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::stack::WriteCursor;
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::text;

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
fn primitive_float_paths_allocate_nothing() {
    let mut backing = [0u8; 32];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);

    start_counting();
    12.345_f64.render(&mut cx).unwrap();
    cx.out.push(b' ').unwrap();
    text!("{:.2}", 12.345_f64).render(&mut cx).unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(cursor.as_slice(), b"12.345 12.35");
}

#[test]
fn formatted_primitive_integer_paths_allocate_nothing() {
    let mut backing = [0u8; 512];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);

    start_counting();
    text!(
        "{:02} {:03} {:+08} {:#010x} {:.2e} {:b} {:.100e}",
        7u8,
        42u16,
        -123i32,
        0xdead_u32,
        12_345u64,
        u128::MAX,
        u128::MAX,
    )
    .render(&mut cx)
    .unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(
        cursor.as_slice(),
        format!(
            "{:02} {:03} {:+08} {:#010x} {:.2e} {:b} {:.100e}",
            7u8,
            42u16,
            -123i32,
            0xdead_u32,
            12_345u64,
            u128::MAX,
            u128::MAX,
        )
        .as_bytes(),
    );
}
