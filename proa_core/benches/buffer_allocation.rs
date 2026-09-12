// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use proa_core::{
    ctx::Ctx, el, text, Attr, AttrKey, DataLoader, Tag, WebContext, WebRenderSync, WriteBuf,
    WriteError,
};

/// A typical page component for realistic testing
#[derive(Clone, Copy)]
struct TypicalPage;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TypicalPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        el(
            Tag::Main,
            [Attr::Str {
                k: AttrKey::Class,
                v: "container",
            }],
            |cx| {
                el(Tag::Header, [], |cx| {
                    el(Tag::H1, [], |cx| text("Example Page Title").render(cx)).render(cx)?;
                    el(Tag::P, [Attr::Str { k: AttrKey::Class, v: "subtitle" }], |cx| {
                        text("This is a typical page with various elements").render(cx)
                    })
                    .render(cx)
                })
                .render(cx)?;

                el(
                    Tag::Section,
                    [Attr::Str {
                        k: AttrKey::Class,
                        v: "content",
                    }],
                    |cx| {
                        for i in 0..10 {
                            el(
                                Tag::Div,
                                [Attr::Str {
                                    k: AttrKey::Class,
                                    v: "card",
                                }],
                                |cx| {
                                    el(Tag::H2, [], |cx| {
                                        text(if i % 2 == 0 {
                                            "Feature Card"
                                        } else {
                                            "Product Card"
                                        })
                                        .render(cx)
                                    })
                                    .render(cx)?;
                                    el(Tag::P, [], |cx| {
                                        text("Lorem ipsum dolor sit amet, consectetur adipiscing elit.")
                                            .render(cx)
                                    })
                                    .render(cx)
                                },
                            )
                            .render(cx)?;
                        }
                        Ok(())
                    },
                )
                .render(cx)
            },
        )
        .render(cx)
    }
}

/// Benchmark: Buffer allocation strategies
fn bench_buffer_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_allocation_strategy");

    // Strategy 1: No pre-allocation (Vec::new())
    group.bench_function("no_prealloc", |b| {
        b.iter(|| {
            let mut buffer = Vec::new(); // Start with 0 capacity
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Strategy 2: Small pre-allocation (512 bytes)
    group.bench_function("prealloc_512", |b| {
        b.iter(|| {
            let mut buffer = Vec::with_capacity(512);
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Strategy 3: Medium pre-allocation (2KB)
    group.bench_function("prealloc_2k", |b| {
        b.iter(|| {
            let mut buffer = Vec::with_capacity(2048);
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Strategy 4: Large pre-allocation (8KB)
    group.bench_function("prealloc_8k", |b| {
        b.iter(|| {
            let mut buffer = Vec::with_capacity(8192);
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Strategy 5: Reused buffer with clear
    group.bench_function("reused_buffer", |b| {
        let mut buffer = Vec::with_capacity(2048);
        b.iter(|| {
            buffer.clear();
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    group.finish();
}

/// Benchmark: Pure writing overhead (no allocations)
fn bench_writing_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("writing_overhead");

    // First, render once to see how much data we produce
    let mut buffer = Vec::new();
    let mut cx = Ctx::with_buffer(&mut buffer);
    TypicalPage.render(&mut cx).unwrap();
    let typical_size = buffer.len();

    println!("Typical page size: {} bytes", typical_size);

    // Pre-allocate buffer large enough to avoid any reallocation
    group.bench_function("writing_only", |b| {
        let mut buffer = Vec::with_capacity(typical_size * 2);
        b.iter(|| {
            buffer.clear();
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    group.finish();
}

/// Benchmark: Raw Vec operations to isolate allocation overhead
fn bench_raw_vec_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("raw_vec_operations");

    // Typical page produces ~2KB of output, simulate that
    let test_data = b"<div class=\"test\">Hello World</div>";
    let iterations = 60; // Roughly 2KB total

    // No pre-allocation - maximum reallocation overhead
    group.bench_function("raw_vec_no_prealloc", |b| {
        b.iter(|| {
            let mut buffer = Vec::new();
            for _ in 0..iterations {
                buffer.extend_from_slice(test_data);
            }
            black_box(buffer.len());
        });
    });

    // With pre-allocation - minimal reallocation overhead
    group.bench_function("raw_vec_with_prealloc", |b| {
        b.iter(|| {
            let mut buffer = Vec::with_capacity(test_data.len() * iterations);
            for _ in 0..iterations {
                buffer.extend_from_slice(test_data);
            }
            black_box(buffer.len());
        });
    });

    // Reused buffer
    group.bench_function("raw_vec_reused", |b| {
        let mut buffer = Vec::with_capacity(test_data.len() * iterations);
        b.iter(|| {
            buffer.clear();
            for _ in 0..iterations {
                buffer.extend_from_slice(test_data);
            }
            black_box(buffer.len());
        });
    });

    group.finish();
}

/// Benchmark: Allocation overhead by measuring difference
fn bench_allocation_overhead_isolation(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_overhead_isolation");

    // Baseline: Just render with perfect pre-allocation (zero reallocs)
    group.bench_function("baseline_perfect_prealloc", |b| {
        // Determine exact size needed
        let mut sizing_buffer = Vec::new();
        let mut cx = Ctx::with_buffer(&mut sizing_buffer);
        TypicalPage.render(&mut cx).unwrap();
        let exact_size = sizing_buffer.len();

        b.iter(|| {
            let mut buffer = Vec::with_capacity(exact_size);
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Test: Render with no pre-allocation (maximum reallocs)
    group.bench_function("worst_case_no_prealloc", |b| {
        b.iter(|| {
            let mut buffer = Vec::new();
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    // Test: Render with tiny pre-allocation (some reallocs)
    group.bench_function("undersized_prealloc_128", |b| {
        b.iter(|| {
            let mut buffer = Vec::with_capacity(128);
            let mut cx = Ctx::with_buffer(&mut buffer);
            TypicalPage.render(&mut cx).unwrap();
            black_box(buffer.len());
        });
    });

    group.finish();
}

/// Benchmark: Different page sizes to see scaling
fn bench_size_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("size_scaling");

    for card_count in [5, 10, 50, 100].iter() {
        // With pre-allocation
        group.bench_with_input(
            BenchmarkId::new("with_prealloc", card_count),
            card_count,
            |b, &count| {
                let mut buffer = Vec::with_capacity(count * 200); // Estimate ~200 bytes per card
                b.iter(|| {
                    buffer.clear();
                    let mut cx = Ctx::with_buffer(&mut buffer);

                    // Render page with N cards
                    el(Tag::Main, [], |cx| {
                        for i in 0..count {
                            el(
                                Tag::Div,
                                [Attr::Str {
                                    k: AttrKey::Class,
                                    v: "card",
                                }],
                                |cx| {
                                    el(Tag::H2, [], |cx| text("Card Title").render(cx))
                                        .render(cx)?;
                                    el(Tag::P, [], |cx| {
                                        text(if i % 2 == 0 { "Even card" } else { "Odd card" })
                                            .render(cx)
                                    })
                                    .render(cx)
                                },
                            )
                            .render(cx)?;
                        }
                        Ok(())
                    })
                    .render(&mut cx)
                    .unwrap();

                    black_box(buffer.len());
                });
            },
        );

        // Without pre-allocation
        group.bench_with_input(
            BenchmarkId::new("no_prealloc", card_count),
            card_count,
            |b, &count| {
                b.iter(|| {
                    let mut buffer = Vec::new();
                    let mut cx = Ctx::with_buffer(&mut buffer);

                    // Render page with N cards
                    el(Tag::Main, [], |cx| {
                        for i in 0..count {
                            el(
                                Tag::Div,
                                [Attr::Str {
                                    k: AttrKey::Class,
                                    v: "card",
                                }],
                                |cx| {
                                    el(Tag::H2, [], |cx| text("Card Title").render(cx))
                                        .render(cx)?;
                                    el(Tag::P, [], |cx| {
                                        text(if i % 2 == 0 { "Even card" } else { "Odd card" })
                                            .render(cx)
                                    })
                                    .render(cx)
                                },
                            )
                            .render(cx)?;
                        }
                        Ok(())
                    })
                    .render(&mut cx)
                    .unwrap();

                    black_box(buffer.len());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_buffer_strategies,
    bench_writing_overhead,
    bench_raw_vec_operations,
    bench_allocation_overhead_isolation,
    bench_size_scaling
);
criterion_main!(benches);
