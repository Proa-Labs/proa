// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Micro-benchmark to identify closure overhead sources

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proa_core::{el, text, Attr, AttrKey, Tag, WebContext, WebRenderSync};

fn bench_overhead_sources(c: &mut Criterion) {
    let mut group = c.benchmark_group("overhead_sources");

    // Baseline: just extend bytes directly
    group.bench_function("baseline_extend", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            buffer.extend_from_slice(b"<div class=\"test\">");
            buffer.extend_from_slice(b"Hello World");
            buffer.extend_from_slice(b"</div>");
            black_box(buffer.len())
        });
    });

    // Test Tag::as_str() overhead
    group.bench_function("tag_as_str", |b| {
        let mut buffer = Vec::with_capacity(1024);
        let tag = Tag::Div;
        b.iter(|| {
            buffer.clear();
            buffer.extend_from_slice(b"<");
            buffer.extend_from_slice(tag.as_str().as_bytes());
            buffer.extend_from_slice(b">");
            buffer.extend_from_slice(b"Hello");
            buffer.extend_from_slice(b"</");
            buffer.extend_from_slice(tag.as_str().as_bytes());
            buffer.extend_from_slice(b">");
            black_box(buffer.len())
        });
    });

    // Test single el() with no attrs, no children
    group.bench_function("single_el_empty", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(&mut buffer);
            el(Tag::Div, [], |_cx| Ok(())).render(&mut cx).unwrap();
            black_box(buffer.len())
        });
    });

    // Test single el() with 1 attr
    group.bench_function("single_el_1attr", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(&mut buffer);
            el(
                Tag::Div,
                [Attr::Str {
                    k: AttrKey::Class,
                    v: "test",
                }],
                |_cx| Ok(()),
            )
            .render(&mut cx)
            .unwrap();
            black_box(buffer.len())
        });
    });

    // Test single el() with text child
    group.bench_function("single_el_with_text", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(&mut buffer);
            el(
                Tag::Div,
                [Attr::Str {
                    k: AttrKey::Class,
                    v: "test",
                }],
                |cx| text("Hello World").render(cx),
            )
            .render(&mut cx)
            .unwrap();
            black_box(buffer.len())
        });
    });

    // Test nested el()
    group.bench_function("nested_el_2_levels", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(&mut buffer);
            el(Tag::Div, [], |cx| {
                el(Tag::Span, [], |cx| text("Hello").render(cx)).render(cx)
            })
            .render(&mut cx)
            .unwrap();
            black_box(buffer.len())
        });
    });

    // Test nested el() 5 levels deep
    group.bench_function("nested_el_5_levels", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(&mut buffer);
            el(Tag::Div, [], |cx| {
                el(Tag::Div, [], |cx| {
                    el(Tag::Div, [], |cx| {
                        el(Tag::Div, [], |cx| {
                            el(Tag::Span, [], |cx| text("Hello").render(cx)).render(cx)
                        })
                        .render(cx)
                    })
                    .render(cx)
                })
                .render(cx)
            })
            .render(&mut cx)
            .unwrap();
            black_box(buffer.len())
        });
    });

    // Test raw version equivalent to nested_el_5_levels
    group.bench_function("raw_5_levels", |b| {
        let mut buffer = Vec::with_capacity(1024);
        b.iter(|| {
            buffer.clear();
            buffer.extend_from_slice(b"<div>");
            buffer.extend_from_slice(b"<div>");
            buffer.extend_from_slice(b"<div>");
            buffer.extend_from_slice(b"<div>");
            buffer.extend_from_slice(b"<span>");
            buffer.extend_from_slice(b"Hello");
            buffer.extend_from_slice(b"</span>");
            buffer.extend_from_slice(b"</div>");
            buffer.extend_from_slice(b"</div>");
            buffer.extend_from_slice(b"</div>");
            buffer.extend_from_slice(b"</div>");
            black_box(buffer.len())
        });
    });

    group.finish();
}

criterion_group!(benches, bench_overhead_sources);
criterion_main!(benches);
