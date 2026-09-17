// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sync-render coverage benchmarks.
//!
//! Complements the focused `render_memcpy_floor`, `button_comparison`, and
//! `complex_comparison` suites. These pages are deliberately minimal so each
//! benchmark isolates a single cost axis:
//!
//! 1. Large page (1000-row table, ~10k node tree)
//! 2. Deeply nested tree (depth 20)
//! 3. Widely-branching tree (fan-out 100)
//! 4. Static-heavy page (pure literals, no interpolation)
//! 5. Dynamic-heavy page (every element has `{expr}` interpolation)
//!
//! Each group compares three paths on the same workload:
//!
//! - `sync_direct` — current `WebRenderSync` hot path
//! - `async_done_direct` — `to_async(...)` + direct `RenderOutcome::Done` match
//! - `async_entry` — public `render_into_async(...)` root driver
//!
//! Output sizes are printed at the start of each group so callers can
//! sanity-check that the pages are doing non-trivial work.
use criterion::measurement::WallTime;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkGroup, Criterion};
use futures::executor::block_on;
use proa_core::{
    render_into_async, to_async, RenderOutcome, SafeText, WebContext, WebRenderSync, WriteBuf,
};
use proa_macros::html_sync;
const TABLE_ROWS: usize = 1000;
struct LargeTablePage;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for LargeTablePage {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <html lang="en">
                <head>
                    <title>"Large Table Benchmark"</title>
                </head>
                <body>
                    <table class="data-grid">
                        <thead>
                            <tr>
                                <th>"#"</th>
                                <th>"Name"</th>
                                <th>"Status"</th>
                                <th>"Price"</th>
                                <th>"Qty"</th>
                            </tr>
                        </thead>
                        <tbody>{LargeTableRows}</tbody>
                    </table>
                </body>
            </html>
        }
        .render(cx)
    }
}
struct LargeTableRows;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for LargeTableRows {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for idx in 0..TABLE_ROWS {
            LargeTableRow { idx }.render(cx)?;
        }
        Ok(())
    }
}
struct LargeTableRow {
    idx: usize,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for LargeTableRow {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let row_class = if self.idx.is_multiple_of(2) {
            "even"
        } else {
            "odd"
        };
        let status = SafeText::from_trusted(match self.idx % 3 {
            0 => "active",
            1 => "pending",
            _ => "archived",
        });
        let name = SafeText::from_trusted(match self.idx % 4 {
            0 => "Widget Alpha",
            1 => "Widget Beta",
            2 => "Widget Gamma",
            _ => "Widget Delta",
        });
        html_sync! {
            <tr class={row_class}>
                <td>{self.idx}</td>
                <td>{name}</td>
                <td>{status}</td>
                <td>"$19.99"</td>
                <td>{self.idx % 99}</td>
            </tr>
        }
        .render(cx)
    }
}
struct Depth20;
struct Depth19;
struct Depth18;
struct Depth17;
struct Depth16;
struct Depth15;
struct Depth14;
struct Depth13;
struct Depth12;
struct Depth11;
struct Depth10;
struct Depth9;
struct Depth8;
struct Depth7;
struct Depth6;
struct Depth5;
struct Depth4;
struct Depth3;
struct Depth2;
struct Depth1;
macro_rules! depth_impl {
    ($name:ident, $child:ident) => {
        impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for $name {
            #[inline(always)]
            fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
                html_sync! { < div class = "d" > { $child } </ div > }.render(cx)
            }
        }
    };
}
depth_impl!(Depth20, Depth19);
depth_impl!(Depth19, Depth18);
depth_impl!(Depth18, Depth17);
depth_impl!(Depth17, Depth16);
depth_impl!(Depth16, Depth15);
depth_impl!(Depth15, Depth14);
depth_impl!(Depth14, Depth13);
depth_impl!(Depth13, Depth12);
depth_impl!(Depth12, Depth11);
depth_impl!(Depth11, Depth10);
depth_impl!(Depth10, Depth9);
depth_impl!(Depth9, Depth8);
depth_impl!(Depth8, Depth7);
depth_impl!(Depth7, Depth6);
depth_impl!(Depth6, Depth5);
depth_impl!(Depth5, Depth4);
depth_impl!(Depth4, Depth3);
depth_impl!(Depth3, Depth2);
depth_impl!(Depth2, Depth1);
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Depth1 {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <span class="leaf">"leaf"</span>
        }
        .render(cx)
    }
}
const FANOUT: usize = 100;
struct FanoutPage;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for FanoutPage {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <ul class="fanout">{FanoutChildren}</ul>
        }
        .render(cx)
    }
}
struct FanoutChildren;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for FanoutChildren {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for _ in 0..FANOUT {
            html_sync! {
                <li class="child">
                    <span>"item"</span>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}
struct StaticHeavyPage;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticHeavyPage {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <html lang="en">
                <head>
                    <title>"Static Page"</title>
                    <meta
                        name="viewport"
                        content="width=device-width, initial-scale=1"
                    />
                    <link
                        rel="stylesheet"
                        href="/style.css"
                    />
                </head>
                <body class="page-body">
                    <header class="hero">
                        <h1 class="title">"Welcome"</h1>
                        <p class="lede">"A static page with no interpolation."</p>
                    </header>
                    <nav class="top-nav">
                        <ul>
                            <li>
                                <a href="/a">"Alpha"</a>
                            </li>
                            <li>
                                <a href="/b">"Beta"</a>
                            </li>
                            <li>
                                <a href="/c">"Gamma"</a>
                            </li>
                            <li>
                                <a href="/d">"Delta"</a>
                            </li>
                            <li>
                                <a href="/e">"Epsilon"</a>
                            </li>
                        </ul>
                    </nav>
                    <main class="content">
                        <section class="hero-section">
                            <h2>"Section One"</h2>
                            <p>
                                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
                            </p>
                            <p>
                                "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat."
                            </p>
                        </section>
                        <section class="hero-section">
                            <h2>"Section Two"</h2>
                            <p>
                                "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur."
                            </p>
                            <p>
                                "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum."
                            </p>
                        </section>
                        <section class="hero-section">
                            <h2>"Section Three"</h2>
                            <p>
                                "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium."
                            </p>
                            <p>
                                "Totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo."
                            </p>
                        </section>
                        <section class="hero-section">
                            <h2>"Section Four"</h2>
                            <p>
                                "Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur magni dolores."
                            </p>
                            <p>
                                "Neque porro quisquam est, qui dolorem ipsum quia dolor sit amet, consectetur, adipisci velit, sed quia non numquam eius."
                            </p>
                        </section>
                    </main>
                    <footer class="site-footer">
                        <p>"Static page footer."</p>
                    </footer>
                </body>
            </html>
        }
            .render(cx)
    }
}
const DYNAMIC_ITEMS: usize = 100;
struct DynamicHeavyPage;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DynamicHeavyPage {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <ul class="dynamic">{DynamicItems}</ul>
        }
        .render(cx)
    }
}
struct DynamicItems;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DynamicItems {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for idx in 0..DYNAMIC_ITEMS {
            DynamicItem { idx }.render(cx)?;
        }
        Ok(())
    }
}
struct DynamicItem {
    idx: usize,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DynamicItem {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let kind: &str = match self.idx % 3 {
            0 => "primary",
            1 => "secondary",
            _ => "tertiary",
        };
        let label = SafeText::from_trusted(match self.idx % 4 {
            0 => "Alpha",
            1 => "Beta",
            2 => "Gamma",
            _ => "Delta",
        });
        let href_base: &str = match self.idx % 2 {
            0 => "/left",
            _ => "/right",
        };
        html_sync! {
            <li class={kind} data-idx={self.idx}>
                <a
                    href={href_base}
                    data-rank={self.idx}
                >
                    <span class="label">{label}</span>
                    <span class="value">{self.idx}</span>
                </a>
            </li>
        }
        .render(cx)
    }
}
fn output_size<R, Make>(reserve: usize, make: Make) -> usize
where
    R: WebRenderSync<::proa_core::NoLoader, proa_core::RenderBuffer>,
    Make: Fn() -> R,
{
    let _ = reserve;
    let mut cx = WebContext::new();
    make().render(&mut cx).unwrap();
    let probe = cx.into_output();
    probe.len()
}
fn bench_paths<R, Make>(group: &mut BenchmarkGroup<'_, WallTime>, reserve: usize, make: Make)
where
    R: WebRenderSync<::proa_core::NoLoader, proa_core::RenderBuffer>,
    Make: Fn() -> R + Copy,
{
    let _ = reserve;
    group.bench_function("sync_direct", |b| {
        let mut buf = proa_core::RenderBuffer::new();
        b.iter(|| {
            buf.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buf));
            make().render(&mut cx).unwrap();
            buf = cx.into_output();
            black_box(buf.len())
        });
    });
    group.bench_function("async_done_direct", |b| {
        let mut buf = proa_core::RenderBuffer::new();
        b.iter(|| {
            buf.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buf));
            {
                let outcome = proa_core::WebRender::render(to_async(make()), &mut cx);
                match outcome {
                    RenderOutcome::Done(res) => res.unwrap(),
                    RenderOutcome::Pending(_) => {
                        unreachable!("sync adapter unexpectedly returned Pending")
                    }
                }
            }
            buf = cx.into_output();
            black_box(buf.len())
        });
    });
    group.bench_function("async_entry", |b| {
        let mut buf = proa_core::RenderBuffer::new();
        b.iter(|| {
            block_on(render_into_async(&mut buf, to_async(make())));
            black_box(buf.len())
        });
    });
}
fn bench_large_table(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_table_1k");
    let output_size = output_size(256 * 1024, || LargeTablePage);
    eprintln!(
        "large_table_1k output: {} bytes ({:.1} KB)",
        output_size,
        output_size as f64 / 1024.0
    );
    bench_paths(&mut group, 256 * 1024, || LargeTablePage);
    group.finish();
}
fn bench_deep_nest(c: &mut Criterion) {
    let mut group = c.benchmark_group("deep_nest_20");
    let output_size = output_size(4096, || Depth20);
    eprintln!("deep_nest_20 output: {} bytes", output_size);
    bench_paths(&mut group, 4096, || Depth20);
    group.finish();
}
fn bench_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("fanout_100");
    let output_size = output_size(8 * 1024, || FanoutPage);
    eprintln!("fanout_100 output: {} bytes", output_size);
    bench_paths(&mut group, 8 * 1024, || FanoutPage);
    group.finish();
}
fn bench_static_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("static_heavy");
    let output_size = output_size(8 * 1024, || StaticHeavyPage);
    eprintln!("static_heavy output: {} bytes", output_size);
    bench_paths(&mut group, 8 * 1024, || StaticHeavyPage);
    group.finish();
}
fn bench_dynamic_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_heavy");
    let output_size = output_size(32 * 1024, || DynamicHeavyPage);
    eprintln!("dynamic_heavy output: {} bytes", output_size);
    bench_paths(&mut group, 32 * 1024, || DynamicHeavyPage);
    group.finish();
}
criterion_group!(
    benches,
    bench_large_table,
    bench_deep_nest,
    bench_fanout,
    bench_static_heavy,
    bench_dynamic_heavy,
);
criterion_main!(benches);
