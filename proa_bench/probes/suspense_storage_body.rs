// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Identical benchmark body, directly including baseline/candidate Suspense modules.
//! Native/manual contexts only. No timing claim and no production source edits.
use proa_core::{
    RenderMode, RenderOutcome, SendStreamChunkFlusher, SendWebContext, StreamChunkFlusher,
    StreamChunkView, WebContext, WebRender, WebRenderSend, WriteBuf, WriteError,
};
use proa_stream::Suspense;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    future::{poll_fn, Future},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};
use std::{rc::Rc, sync::Arc};

struct Counter;
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(n, Ordering::Relaxed);
        unsafe { System.realloc(p, layout, n) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

struct Child {
    pending: bool,
    error: bool,
}
impl WebRender for Child {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let mut pending = self.pending;
            poll_fn(|_| {
                if std::mem::take(&mut pending) {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            cx.output_mut().extend(b"child")?;
            if self.error {
                Err(WriteError::render("failure"))
            } else {
                Ok(())
            }
        })
    }
}

impl WebRenderSend for Child {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            let mut pending = self.pending;
            poll_fn(|_| {
                if std::mem::take(&mut pending) {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            cx.output_mut().extend(b"child")
        })
    }
}
#[derive(Default)]
struct Sink(AtomicUsize);
impl StreamChunkFlusher for Sink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.0.fetch_add(chunk.len(), Ordering::Relaxed);
        Ok(())
    }
    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        self.0.fetch_add(chunk.total_len(), Ordering::Relaxed);
        Ok(())
    }
}
impl SendStreamChunkFlusher for Sink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        StreamChunkFlusher::flush_chunk(self, chunk)
    }
    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        StreamChunkFlusher::flush_chunk_view(self, chunk)
    }
}
fn drive<F: Future<Output = Result<(), WriteError>>>(outcome: RenderOutcome<F>) -> (usize, usize) {
    let size = std::mem::size_of_val(&outcome);
    let mut future = std::pin::pin!(black_box(outcome).resolve());
    let mut task = Context::from_waker(Waker::noop());
    let mut polls = 0;
    loop {
        polls += 1;
        if let Poll::Ready(result) = future.as_mut().poll(&mut task) {
            result.unwrap();
            return (size, polls);
        }
        assert!(polls < 3);
    }
}
#[inline(never)]
fn local(cx: &mut WebContext, pending: bool) -> (usize, usize) {
    drive(
        Suspense::new(
            Child {
                pending,
                error: false,
            },
            (),
        )
        .render(cx),
    )
}
#[inline(never)]
fn send(cx: &mut SendWebContext, pending: bool) -> (usize, usize) {
    drive(
        Suspense::new(
            Child {
                pending,
                error: false,
            },
            (),
        )
        .render_send(cx),
    )
}
fn main() {
    for pending in [false, true] {
        for streaming in [false, true] {
            let sink = Rc::new(Sink::default());
            let mut cx = WebContext::new();
            if streaming {
                cx = cx
                    .with_render_mode(RenderMode::ChunkedInOrder)
                    .with_stream_chunk_flusher(sink.clone());
            }
            for _ in 0..8 {
                local(&mut cx, pending);
                cx.output_mut().clear();
            }
            sink.0.store(0, Ordering::Relaxed);
            let calls = CALLS.load(Ordering::Relaxed);
            let bytes = BYTES.load(Ordering::Relaxed);
            let (size, polls) = local(black_box(&mut cx), black_box(pending));
            let calls = CALLS.load(Ordering::Relaxed) - calls;
            let bytes = BYTES.load(Ordering::Relaxed) - bytes;
            assert_eq!(sink.0.load(Ordering::Relaxed) + cx.output().len(), 5);
            println!("case=local_{streaming}_{pending} outcome_bytes={size} allocations={calls} allocated_bytes={bytes} polls={polls}");
            let sink = Arc::new(Sink::default());
            let mut cx = SendWebContext::new();
            if streaming {
                cx = cx.with_render_mode(RenderMode::ChunkedInOrder);
                cx.set_stream_chunk_flusher(sink.clone());
            }
            for _ in 0..8 {
                send(&mut cx, pending);
                cx.output_mut().clear();
            }
            sink.0.store(0, Ordering::Relaxed);
            let calls = CALLS.load(Ordering::Relaxed);
            let bytes = BYTES.load(Ordering::Relaxed);
            let (size, polls) = send(black_box(&mut cx), black_box(pending));
            let calls = CALLS.load(Ordering::Relaxed) - calls;
            let bytes = BYTES.load(Ordering::Relaxed) - bytes;
            assert_eq!(sink.0.load(Ordering::Relaxed) + cx.output().len(), 5);
            println!("case=send_{streaming}_{pending} outcome_bytes={size} allocations={calls} allocated_bytes={bytes} polls={polls}");
        }
    }
}
