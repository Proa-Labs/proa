// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Per-Suspense-boundary cost as a function of boundary count N.
//!
//! Measures the in-order streaming render path with a synthetic page
//! containing exactly N Suspense boundaries, each with an
//! immediate-resolve sync child. Varying N (1, 2, 5, 10, 20) lets us
//! see the cold-vs-warm cost split:
//!
//! - N = 1 measures fixed boundary cost.
//! - N = 20 shows the steady-state per-boundary slope.
//!
//! Criterion reports total render time per N. Divide by N (in your
//! head) to get per-boundary cost.
//!
//! All children resolve synchronously via `SyncToAsync`, so every
//! microsecond measured is structural overhead (allocation,
//! `Box::pin`, channel send, `WebContext` rebuild) — none of it is
//! waiting on actual I/O. This isolates the sink architecture's
//! cost model from anything happening in user code.

use std::cell::Cell;
use std::rc::Rc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use proa_core::md::MdContext;
use proa_core::{
    render_in_context_async, render_md_in_context_async, to_md_async, MdRender, MdRenderSync,
    NoLoader, RenderBuffer, RenderMode, RenderOutcome, StreamChunkFlusher, SyncToAsync, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html_sync, md_sync};
use proa_stream::Suspense;

/// Smallest possible Suspense child: writes one tiny static span.
/// Sync, infallible, and generic over buffer + loader.
struct TinyChild;

impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TinyChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <span>"x"</span>
            },
            cx,
        )
    }
}

/// Suspense fallback: empty (no fallback HTML). Bytes still flow through
/// the in-order pre-flush; the Suspense in-order branch flushes the
/// parent buffer regardless of fallback content.
struct TinyFallback;

impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TinyFallback {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// Synthetic page with exactly `boundary_count` production `Suspense` nodes
/// rendered sequentially through the reused standard buffer.
#[derive(Clone, Copy)]
struct ScalingPage {
    boundary_count: usize,
}

impl WebRender<NoLoader, RenderBuffer> for ScalingPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let n = self.boundary_count;
        RenderOutcome::pending(async move {
            for _ in 0..n {
                let outcome = Suspense::new(SyncToAsync(TinyChild), TinyFallback).render(&mut *cx);
                match outcome {
                    RenderOutcome::Done(res) => res?,
                    RenderOutcome::Pending(fut) => fut.await?,
                }
            }
            Ok(())
        })
    }
}

struct TinyMarkdownChild;

impl<B: WriteBuf> MdRenderSync<B> for TinyMarkdownChild {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        MdRenderSync::render_md(
            md_sync! { r###"
                x
                "###
            },
            cx,
        )
    }
}

#[derive(Clone, Copy)]
struct MarkdownScalingPage {
    boundary_count: usize,
}

impl MdRender<RenderBuffer> for MarkdownScalingPage {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            for _ in 0..self.boundary_count {
                proa_core::render_md_child(cx, Suspense::new(to_md_async(TinyMarkdownChild), ()))
                    .await?;
            }
            Ok(())
        })
    }
}

/// Test sink: counts shipped bytes and recycles pages back into a real
/// pool. Mirrors the production `ChannelSink` page-recycling behavior so
/// the bench measures the warm-pool steady state.
struct CountingSink {
    flushed_len: Cell<usize>,
}

impl CountingSink {
    fn new() -> Self {
        Self {
            flushed_len: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.flushed_len.set(0);
    }
}

fn render_html_scaling<R>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    flusher: Rc<dyn StreamChunkFlusher>,
    sink: &CountingSink,
    root: R,
) -> usize
where
    R: WebRender<NoLoader, RenderBuffer>,
{
    let buffer = std::mem::take(out);
    let (total_bytes, buffer) = runtime.block_on(async move {
        let mut cx = WebContext::with_buffer(buffer)
            .with_render_mode(RenderMode::ChunkedInOrder)
            .with_stream_chunk_flusher(flusher);
        render_in_context_async(&mut cx, root)
            .await
            .expect("scaling page render should succeed");
        let total_bytes = cx.out.len() + sink.flushed_len.get();
        (total_bytes, cx.into_output())
    });
    *out = buffer;
    total_bytes
}

impl StreamChunkFlusher for CountingSink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.flushed_len
            .set(self.flushed_len.get().saturating_add(chunk.len()));
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn proa_core::StreamChunkView) -> Result<(), WriteError> {
        self.flushed_len
            .set(self.flushed_len.get().saturating_add(chunk.total_len()));
        Ok(())
    }
}

fn bench_suspense_boundary_scaling(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("failed to build tokio runtime for boundary-scaling bench");

    let mut group = c.benchmark_group("suspense_boundary_scaling");

    for &n in &[1usize, 2, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("production_suspense", n), &n, |b, &n| {
            let mut out = RenderBuffer::new();
            let sink = Rc::new(CountingSink::new());
            let flusher_dyn: Rc<dyn StreamChunkFlusher> = sink.clone();

            b.iter(|| {
                out.clear();
                sink.reset();
                let total_bytes = render_html_scaling(
                    &runtime,
                    &mut out,
                    flusher_dyn.clone(),
                    &sink,
                    ScalingPage { boundary_count: n },
                );
                black_box(total_bytes)
            });
        });
    }

    group.finish();
}

fn bench_markdown_suspense_boundary_scaling(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("failed to build tokio runtime for Markdown boundary-scaling bench");
    let mut group = c.benchmark_group("markdown_suspense_boundary_scaling");

    for &n in &[1usize, 2, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("production_suspense", n), &n, |b, &n| {
            let sink = Rc::new(CountingSink::new());
            let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
            let mut out = RenderBuffer::new();

            b.iter(|| {
                out.clear();
                sink.reset();
                let total_bytes = runtime.block_on(async {
                    let mut cx = MdContext::with_buffer(&mut out)
                        .with_render_mode(RenderMode::ChunkedInOrder)
                        .with_stream_chunk_flusher(Rc::clone(&flusher));
                    render_md_in_context_async(&mut cx, MarkdownScalingPage { boundary_count: n })
                        .await
                        .expect("Markdown scaling page should render");
                    out.len() + sink.flushed_len.get()
                });
                black_box(total_bytes)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_suspense_boundary_scaling,
    bench_markdown_suspense_boundary_scaling
);
criterion_main!(benches);
