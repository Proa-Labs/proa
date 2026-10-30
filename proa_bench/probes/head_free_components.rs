// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Same source is compiled against main and the candidate. No managed API calls.
use proa_core::{
    RenderOutcome, SendWebContext, WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, html_sync};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    future::{poll_fn, Future, Ready},
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
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
struct Leaf(u64);
impl WebRender for Leaf {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<_>>::Done(WebRenderSync::render(
            html_sync! {
                <span>{self.0}</span>
            },
            cx,
        ))
    }
}
struct Pending(u64);
impl WebRender for Pending {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let mut waited = false;
            poll_fn(|_| {
                if std::mem::replace(&mut waited, true) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            Leaf(self.0).render(cx).resolve().await
        })
    }
}
struct Page(u64);
// proa-perf-ok(scope): identical async composition benchmark against baseline and candidate
impl WebRender for Page {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            html! {
                <main>{for n in 0..32 { Leaf(self.0 + n) }}</main>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
fn drive<F: Future<Output = Result<(), WriteError>>>(outcome: RenderOutcome<F>) -> (usize, usize) {
    let size = std::mem::size_of_val(&outcome);
    let mut future = std::pin::pin!(black_box(outcome).resolve());
    let mut context = Context::from_waker(Waker::noop());
    for polls in 1..10 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            result.unwrap();
            return (size, polls);
        }
    }
    panic!("stalled");
}
#[inline(never)]
fn run(cx: &mut WebContext, case: usize, n: u64) -> (usize, usize) {
    match case {
        0 => drive(Leaf(n).render(cx)),
        1 => drive(
            html! {
                <div>{Leaf(n)}</div>
            }
            .render(cx),
        ),
        2 => drive(
            html! {
                <div>{Pending(n)}</div>
            }
            .render(cx),
        ),
        _ => drive(Page(n).render(cx)),
    }
}
fn main() {
    let timing = std::env::args().any(|a| a == "--timing");
    if !timing {
        println!(
            "sizes context={} send_context={} error={}",
            std::mem::size_of::<WebContext>(),
            std::mem::size_of::<SendWebContext>(),
            std::mem::size_of::<WriteError>()
        );
    }
    for case in 0..4 {
        let mut cx = WebContext::new();
        for n in 0..100 {
            run(&mut cx, case, n);
            cx.output_mut().clear();
        }
        if timing {
            let started = std::time::Instant::now();
            for n in 0..3_000_000 {
                black_box(run(black_box(&mut cx), black_box(case), black_box(n)));
                cx.output_mut().clear();
            }
            println!(
                "case={case} ns_per_render={:.3}",
                started.elapsed().as_nanos() as f64 / 3_000_000.0
            );
        } else {
            let count = CALLS.load(Ordering::Relaxed);
            let bytes = BYTES.load(Ordering::Relaxed);
            let (size, polls) = run(black_box(&mut cx), black_box(case), black_box(42));
            let calls = CALLS.load(Ordering::Relaxed) - count;
            let bytes = BYTES.load(Ordering::Relaxed) - bytes;
            println!("case={case} outcome_bytes={size} allocations={calls} allocated_bytes={bytes} polls={polls} output_bytes={}",cx.output().len());
        }
    }
}
