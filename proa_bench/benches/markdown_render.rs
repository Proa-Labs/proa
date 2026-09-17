// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Markdown root setup, warmed rendering, and repeated HTML bridge costs.
//! Run with `--save-baseline before-context-merge` before migrating the API.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

use criterion::{black_box, criterion_group, Criterion};
use proa_core::{DataLoader, RenderBuffer, WebContext, WebRenderSync, WriteBuf, WriteError};

struct CountingAllocator;
thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocated(bytes: usize) {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|counts| {
            let (n, size) = counts.get();
            counts.set((n + 1, size + bytes));
        });
    }
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocated(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn report_allocations(name: &str, render: impl FnOnce()) {
    COUNTS.with(|c| c.set((0, 0)));
    TRACKING.with(|t| t.set(true));
    render();
    TRACKING.with(|t| t.set(false));
    let (count, bytes) = COUNTS.with(Cell::get);
    eprintln!("markdown allocations {name}: {count} allocations, {bytes} bytes");
}

struct HtmlBridge;
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HtmlBridge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // proa-perf-ok: fixed trusted markup isolates the context bridge cost in this benchmark
        cx.out.extend_static(b"<span>bridge</span>\n")
    }
}

fn document<B: WriteBuf>(cx: &mut WebContext<::proa_core::NoLoader, B>, paragraphs: usize) {
    for _ in 0..paragraphs {
        cx.md_heading(2, black_box("Dynamic *title*")).unwrap();
        cx.write_md_text(black_box("Dynamic [content] with <escaping> & more."))
            .unwrap();
        cx.md_block_boundary().unwrap();
    }
}

fn bridges<B: WriteBuf>(cx: &mut WebContext<::proa_core::NoLoader, B>) {
    for _ in 0..32 {
        cx.render_html_in_md(HtmlBridge).unwrap();
        cx.md_block_boundary().unwrap();
    }
}

fn bench_writer<B: WriteBuf + Default>(c: &mut Criterion, name: &str) {
    let mut group = c.benchmark_group(format!("markdown/{name}"));
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));
    for (size, paragraphs) in [("small", 2), ("multipage", 128)] {
        report_allocations(&format!("{name}/{size}/root"), || {
            let mut out = B::default();
            document(&mut WebContext::with_buffer(&mut out), paragraphs);
            black_box(out.len());
        });
        group.bench_function(format!("{size}/root_sync"), |b| {
            b.iter(|| {
                let mut out = B::default();
                document(&mut WebContext::with_buffer(&mut out), paragraphs);
                black_box(out.len());
            });
        });
        group.bench_function(format!("{size}/root_async"), |b| {
            b.iter(|| {
                futures::executor::block_on(async {
                    let mut out = B::default();
                    let mut cx = WebContext::with_buffer(&mut out);
                    std::future::ready(()).await;
                    document(&mut cx, paragraphs);
                    black_box(out.len());
                })
            });
        });
        let mut out = B::default();
        let mut cx = WebContext::with_buffer(&mut out);
        let initial = cx.md_state();
        document(&mut cx, paragraphs);
        let mut render = || {
            cx.out.clear();
            cx.set_md_state(initial);
            document(&mut cx, paragraphs);
            black_box(cx.out.len());
        };
        report_allocations(&format!("{name}/{size}/reused"), &mut render);
        group.bench_function(format!("{size}/reused_sync"), |b| b.iter(&mut render));
    }
    let mut out = B::default();
    let mut cx = WebContext::with_buffer(&mut out);
    let initial = cx.md_state();
    bridges(&mut cx);
    let mut render = || {
        cx.out.clear();
        cx.set_md_state(initial);
        bridges(&mut cx);
        black_box(cx.out.len());
    };
    report_allocations(&format!("{name}/32_bridges/reused"), &mut render);
    group.bench_function("32_bridges/reused", |b| b.iter(&mut render));
    group.finish();
}

fn markdown(c: &mut Criterion) {
    bench_writer::<Vec<u8>>(c, "vec");
    bench_writer::<RenderBuffer>(c, "render_buffer");
}
criterion_group!(benches, markdown);
fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
