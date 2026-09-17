// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::stack::WriteCursor;
use proa_core::{DataLoader, OutputMeter, WebContext, WebRenderSync, WriteBuf, WriteError};

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
    // Initialize both thread-local cells before allocation tracking begins.
    ALLOCATIONS.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
}

fn stop_counting() -> usize {
    TRACKING.with(|tracking| tracking.set(false));
    ALLOCATIONS.with(Cell::get)
}

struct StackOnlyComponent;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StackOnlyComponent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_static(b"abc")?;
        cx.out.push(b'd')?;
        cx.out.extend_with(4, |spare| {
            spare[..2].copy_from_slice(b"ef");
            2
        })
    }
}

#[test]
fn self_scoped_web_meter_allocates_no_heap_state() {
    let meter = OutputMeter::new(8);
    let mut backing = [0u8; 8];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);

    start_counting();
    cx.with_metered_output(meter.borrow(), |child| StackOnlyComponent.render(child))
        .unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(cursor.as_slice(), b"abcdef");
    assert_eq!(meter.remaining(), 2);
}

#[test]
fn self_scoped_web_reservation_allocates_no_heap_and_does_not_double_debit() {
    let meter = OutputMeter::new(6);
    let mut backing = [0u8; 6];
    let mut cursor = WriteCursor::new(&mut backing);
    let mut cx = WebContext::with_buffer(&mut cursor);

    start_counting();
    cx.with_metered_output(meter.borrow(), |metered| {
        let reservation = metered.out.reserve_exact(6)?;
        metered.with_reserved_output(reservation, |reserved| StackOnlyComponent.render(reserved))
    })
    .unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    drop(cx);
    assert_eq!(cursor.as_slice(), b"abcdef");
    assert_eq!(meter.remaining(), 0);
}

#[test]
fn repeated_html_markdown_bridges_reuse_root_state_without_allocating() {
    let mut backing = [0u8; 256];
    let mut cx = WebContext::with_buffer(WriteCursor::new(&mut backing));
    start_counting();
    for _ in 0..32 {
        cx.render_html_in_md(StackOnlyComponent).unwrap();
        proa_core::WebRenderSync::render(
            proa_core::OptimizedMd::new(|child: &mut WebContext<_, _>| {
                child.write_md_prepared_static(b"\n")
            }),
            &mut cx,
        )
        .unwrap();
    }
    let allocations = stop_counting();
    assert_eq!(allocations, 0);
    assert_eq!(cx.out.len(), 32 * 7);
}
