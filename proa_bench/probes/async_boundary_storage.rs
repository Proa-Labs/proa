// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Native/manual contexts only. Compare identical source against recorded baseline
//! and current integration core rlibs. Measures storage/allocation, not latency.
use proa_core::{ErrorBoundary, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    future::{poll_fn, Future},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

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
#[inline(never)]
fn run(cx: &mut WebContext, pending: bool, error: bool) -> (usize, usize) {
    let outcome = ErrorBoundary::new(Child { pending, error }, "fallback").render(cx);
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
fn main() {
    for (name, pending, error) in [
        ("ready", false, false),
        ("pending", true, false),
        ("recovered", true, true),
    ] {
        let mut cx = WebContext::new();
        for _ in 0..8 {
            run(&mut cx, pending, error);
            cx.output_mut().clear();
        }
        let calls = CALLS.load(Ordering::Relaxed);
        let bytes = BYTES.load(Ordering::Relaxed);
        let (size, polls) = run(black_box(&mut cx), black_box(pending), black_box(error));
        let calls = CALLS.load(Ordering::Relaxed) - calls;
        let bytes = BYTES.load(Ordering::Relaxed) - bytes;
        let out = cx.output().to_vec();
        assert_eq!(
            out,
            if error {
                b"fallback".as_slice()
            } else {
                b"child".as_slice()
            }
        );
        println!("case={name} outcome_bytes={size} allocations={calls} allocated_bytes={bytes} polls={polls} output_bytes={}", out.len());
    }
}
