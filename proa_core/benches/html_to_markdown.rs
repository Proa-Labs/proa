// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compare direct compiled Markdown with converting the same HTML content.
//! Run: cargo bench -p proa_core --features html-to-markdown --locked --bench html_to_markdown
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Duration,
};

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proa_core::{to_markdown, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{html_sync, md_sync};
use pulldown_cmark::{html, Options, Parser};

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
            let (count, size) = counts.get();
            counts.set((count + 1, size + bytes));
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
    COUNTS.with(|counts| counts.set((0, 0)));
    TRACKING.with(|tracking| tracking.set(true));
    render();
    TRACKING.with(|tracking| tracking.set(false));
    let (count, bytes) = COUNTS.with(Cell::get);
    eprintln!("allocations {name}: {count} calls, {bytes} requested bytes");
}

#[derive(Clone, Copy)]
enum Fixture<'a> {
    Paragraph(&'a str),
    Card {
        title: &'a str,
        body: &'a str,
    },
    Document {
        title: &'a str,
        paragraphs: &'a [String],
    },
}

impl WebRenderSync for Fixture<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        match self {
            Self::Paragraph(label) => html_sync! {
                <p>
                    "Built for "
                    <strong>{label}</strong>
                </p>
            }
            .render(cx),
            Self::Card { title, body } => html_sync! {
                <article class="rounded-lg border p-4">
                    <h2>{title}</h2>
                    <p>{body}</p>
                    <a href="/products/proa">"Learn more"</a>
                </article>
            }
            .render(cx),
            Self::Document { title, paragraphs } => {
                html_sync! {
                    <h1>{title}</h1>
                }
                .render(cx)?;
                for paragraph in paragraphs {
                    html_sync! {
                        <p>{paragraph.as_str()}</p>
                    }
                    .render(cx)?;
                }
                Ok(())
            }
        }
    }
}

impl MdRenderSync for Fixture<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        match self {
            Self::Paragraph(label) => md_sync! { r###"Built for **{label}**"### }.render_md(cx),
            Self::Card { title, body } => md_sync! { r###"
                ## {title}

                {body}

                [Learn more](/products/proa)
                "###
            }
            .render_md(cx),
            Self::Document { title, paragraphs } => {
                md_sync! { r###"# {title}"### }.render_md(cx)?;
                for paragraph in paragraphs {
                    cx.md_block_boundary()?;
                    md_sync! { r###"{paragraph.as_str()}"### }.render_md(cx)?;
                }
                Ok(())
            }
        }
    }
}

#[inline]
fn render(fixture: Fixture<'_>, convert: bool, cx: &mut WebContext) {
    if convert {
        to_markdown(fixture).render_md(cx).unwrap();
    } else {
        fixture.render_md(cx).unwrap();
    }
    black_box(&cx.out);
}

fn canonical_html(markdown: &str) -> String {
    let mut output = String::new();
    html::push_html(
        &mut output,
        Parser::new_ext(markdown, Options::ENABLE_TABLES),
    );
    output
}

// proa-perf-ok(scope): benchmark setup and allocation reporting intentionally own fixture/output data
fn benchmark(c: &mut Criterion) {
    let paragraphs: Vec<_> = (0..128)
        .map(|index| format!("Section {index}: compiled templates reuse static markup while rendering dynamic values. Literal *stars*, [brackets], <tags>, and &copy; remain ordinary text."))
        .collect();
    for (name, fixture) in [
        ("paragraph", Fixture::Paragraph("fast rendering")),
        (
            "card",
            Fixture::Card {
                title: "Proa performance",
                body: "Compiled templates for Rust, with fast rendering and reusable components.",
            },
        ),
        (
            "document_128",
            Fixture::Document {
                title: "Rendering overview",
                paragraphs: &paragraphs,
            },
        ),
    ] {
        // Validate outside timing, also initializing the converter's LazyLock.
        let mut direct = WebContext::new();
        render(fixture, false, &mut direct);
        let direct = direct.into_output().into_string();
        let mut converted = WebContext::new();
        render(fixture, true, &mut converted);
        let converted = converted.into_output().into_string();
        assert_eq!(
            canonical_html(&direct),
            canonical_html(&converted),
            "{name}: Markdown semantics drifted"
        );
        eprintln!(
            "fixture {name}: direct {} bytes, converted {} bytes",
            direct.len(),
            converted.len()
        );

        let mut group = c.benchmark_group(format!("html_to_markdown/{name}"));
        group.sample_size(50);
        group.warm_up_time(Duration::from_millis(300));
        group.measurement_time(Duration::from_secs(1));
        for (method, convert) in [("direct", false), ("converted", true)] {
            let root_name = format!("{name}/{method}/fresh");
            report_allocations(&root_name, || {
                render(fixture, convert, &mut WebContext::new())
            });
            group.bench_function(format!("{method}/fresh"), |b| {
                b.iter(|| render(black_box(fixture), convert, &mut WebContext::new()));
            });

            let mut cx = WebContext::new();
            let initial = cx.md_state();
            render(fixture, convert, &mut cx);
            let mut reused = || {
                cx.out.clear();
                cx.set_md_state(initial);
                render(black_box(fixture), convert, &mut cx);
            };
            report_allocations(&format!("{name}/{method}/reused"), &mut reused);
            group.bench_function(format!("{method}/reused"), |b| b.iter(&mut reused));
        }
        group.finish();
    }
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
