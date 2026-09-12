// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use proa_core::{
    NoLoader, RenderBuffer, RenderMode, StreamChunkFlusher, StreamChunkView, WebContext, WriteBuf,
    WriteError,
};

struct ThreadCountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

fn record_allocation() {
    COUNTING.with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|allocations| allocations.set(allocations.get() + 1));
        }
    });
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
    COUNTING.with(|counting| counting.set(false));
    ALLOCATIONS.with(|allocations| allocations.set(0));
    COUNTING.with(|counting| counting.set(true));
}

fn stop_counting() -> usize {
    COUNTING.with(|counting| counting.set(false));
    ALLOCATIONS.with(Cell::get)
}

struct CapturingFlusher {
    bytes: RefCell<Vec<u8>>,
    logical_flushes: Cell<usize>,
    fail: Cell<bool>,
}

impl CapturingFlusher {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: RefCell::new(Vec::with_capacity(capacity)),
            logical_flushes: Cell::new(0),
            fail: Cell::new(false),
        }
    }
}

impl StreamChunkFlusher for CapturingFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.logical_flushes.set(self.logical_flushes.get() + 1);
        if self.fail.get() {
            return Err(WriteError::StreamClosed);
        }
        self.bytes.borrow_mut().extend_from_slice(&chunk);
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        self.logical_flushes.set(self.logical_flushes.get() + 1);
        if self.fail.get() {
            return Err(WriteError::StreamClosed);
        }
        chunk.for_each_slice(&mut |slice| {
            self.bytes.borrow_mut().extend_from_slice(slice);
            Ok(())
        })
    }
}

#[test]
fn render_buffer_view_preserves_multi_page_order_and_skips_empty_slices() {
    let mut buffer = RenderBuffer::new();
    let payload = (0..9_000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    buffer.extend(&payload).unwrap();

    let mut slices = Vec::new();
    StreamChunkView::for_each_slice(&buffer, &mut |slice| {
        slices.push(slice.to_vec());
        Ok(())
    })
    .unwrap();

    assert_eq!(StreamChunkView::total_len(&buffer), payload.len());
    assert_eq!(slices.len(), 3);
    assert_eq!(slices.concat(), payload);

    buffer.clear();
    let mut visits = 0;
    StreamChunkView::for_each_slice(&buffer, &mut |_| {
        visits += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(visits, 0);
}

#[test]
fn failed_render_buffer_flush_discards_attempted_chunk_once() {
    let sink = Rc::new(CapturingFlusher::with_capacity(0));
    sink.fail.set(true);
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut cx: WebContext<NoLoader, RenderBuffer> = WebContext::with_buffer(RenderBuffer::new())
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    cx.out.extend(b"attempted").unwrap();

    assert_eq!(cx.flush_stream_chunk(), Err(WriteError::StreamClosed));
    assert!(cx.out.is_empty());
    assert_eq!(sink.logical_flushes.get(), 1);
    assert_eq!(cx.response().stream_flush_generation(), 1);
}

#[test]
fn empty_render_buffer_flush_keeps_response_metadata_mutable() {
    let sink = Rc::new(CapturingFlusher::with_capacity(0));
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut cx: WebContext<NoLoader, RenderBuffer> = WebContext::with_buffer(RenderBuffer::new())
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);

    cx.flush_stream_chunk().unwrap();

    assert_eq!(sink.logical_flushes.get(), 0);
    assert_eq!(cx.response().stream_flush_generation(), 0);
    assert!(cx.response_mut().set_status(http::StatusCode::CREATED));
}

#[test]
fn warmed_render_buffer_write_and_flush_allocates_nothing() {
    let payload = vec![b'x'; 9_000];
    let sink = Rc::new(CapturingFlusher::with_capacity(payload.len()));
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut cx: WebContext<NoLoader, RenderBuffer> = WebContext::with_buffer(RenderBuffer::new())
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);

    // Warm the buffer pages, response side channel, and destination storage.
    cx.out.extend(&payload).unwrap();
    cx.flush_stream_chunk().unwrap();
    sink.bytes.borrow_mut().clear();

    start_counting();
    cx.out.extend(&payload).unwrap();
    cx.flush_stream_chunk().unwrap();
    let allocations = stop_counting();

    assert_eq!(allocations, 0);
    assert_eq!(sink.bytes.borrow().as_slice(), payload);
    assert!(cx.out.is_empty());
    assert_eq!(sink.logical_flushes.get(), 2);
}
