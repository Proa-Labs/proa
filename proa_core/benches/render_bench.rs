// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use proa_core::{
    el, render_into, text, void, Attr, AttrKey, Ctx, Render, RenderBuffer, Tag, WriteError,
};

#[derive(Clone, Copy)]
struct SamplePage;

impl Render for SamplePage {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        el(
            Tag::Main,
            [Attr::Str {
                k: AttrKey::Class,
                v: "layout",
            }],
            |cx| {
                el(
                    Tag::Header,
                    [Attr::Str {
                        k: AttrKey::Class,
                        v: "hero",
                    }],
                    |cx| {
                        el(
                            Tag::H1,
                            [Attr::Str {
                                k: AttrKey::Class,
                                v: "title",
                            }],
                            |cx| text("Fast SSR in Rust").render(cx),
                        )
                        .render(cx)?;
                        el(
                            Tag::P,
                            [Attr::Str {
                                k: AttrKey::Class,
                                v: "subtitle",
                            }],
                            |cx| text("Macro-free CPS renderer demo").render(cx),
                        )
                        .render(cx)
                    },
                )
                .render(cx)?;

                el(
                    Tag::Section,
                    [Attr::Str {
                        k: AttrKey::Class,
                        v: "cards",
                    }],
                    |cx| {
                        for idx in 0..8 {
                            el(
                                Tag::Div,
                                [
                                    Attr::Str {
                                        k: AttrKey::Class,
                                        v: "card",
                                    },
                                    Attr::Data {
                                        name: "data-id",
                                        v: if idx % 2 == 0 { "even" } else { "odd" },
                                    },
                                ],
                                |cx| {
                                    el(Tag::H1, [], |cx| {
                                        text(if idx % 2 == 0 { "Speed" } else { "Correctness" })
                                            .render(cx)
                                    })
                                    .render(cx)?;
                                    el(Tag::P, [], |cx| text("Benchmark body").render(cx))
                                        .render(cx)
                                },
                            )
                            .render(cx)?;
                        }
                        Ok(())
                    },
                )
                .render(cx)?;

                void(
                    Tag::Img,
                    [
                        Attr::Str {
                            k: AttrKey::Src,
                            v: "/static/speed.png",
                        },
                        Attr::Str {
                            k: AttrKey::Alt,
                            v: "Speed chart",
                        },
                    ],
                )
                .render(cx)?;

                el(
                    Tag::Div,
                    [Attr::Str {
                        k: AttrKey::Class,
                        v: "frame",
                    }],
                    |cx| text("Loading...").render(cx),
                )
                .render(cx)
            },
        )
        .render(cx)
    }
}

#[derive(Clone, Copy)]
struct LargePage;

impl Render for LargePage {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        el(
            Tag::Main,
            [Attr::Str {
                k: AttrKey::Class,
                v: "layout-heavy",
            }],
            |cx| {
                for section in 0..12 {
                    el(
                        Tag::Section,
                        [
                            Attr::Str {
                                k: AttrKey::Class,
                                v: "section",
                            },
                            Attr::Data {
                                name: "data-section",
                                v: if section % 2 == 0 { "even" } else { "odd" },
                            },
                        ],
                        |cx| {
                            for card in 0..48 {
                                el(
                                    Tag::Div,
                                    [
                                        Attr::Str {
                                            k: AttrKey::Class,
                                            v: "card-heavy",
                                        },
                                        Attr::Data {
                                            name: "data-card",
                                            v: if card % 3 == 0 {
                                                "divisible-by-3"
                                            } else {
                                                "other"
                                            },
                                        },
                                    ],
                                    |cx| {
                                        el(
                                            Tag::H1,
                                            [],
                                            |cx| {
                                                text(if card % 2 == 0 {
                                                    "Latency Distribution"
                                                } else {
                                                    "Cache Pressure"
                                                })
                                                .render(cx)
                                            },
                                        )
                                        .render(cx)?;
                                        el(
                                            Tag::P,
                                            [],
                                            |cx| {
                                                text("Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                                                    Vivamus lacinia odio vitae vestibulum vestibulum.")
                                                .render(cx)
                                            },
                                        )
                                        .render(cx)?;
                                        el(
                                            Tag::Ul,
                                            [],
                                            |cx| {
                                                for point in 0..5 {
                                                    el(Tag::Li, [], |cx| {
                                                        text(match point {
                                                            0 => "µs p50 under load, warm cache.",
                                                            1 => "p99 within budget.",
                                                            2 => "Zero heap churn per render.",
                                                            3 => "Arena reset in O(1).",
                                                            _ => "Branch-poor critical path.",
                                                        })
                                                        .render(cx)
                                                    })
                                                    .render(cx)?;
                                                }
                                                Ok(())
                                            },
                                        )
                                        .render(cx)
                                    },
                                )
                                .render(cx)?;
                            }
                            Ok(())
                        },
                    )
                    .render(cx)?;
                }

                el(Tag::Div, [], |cx| text("Loading charts...").render(cx)).render(cx)
            },
        )
        .render(cx)
    }
}

fn bench_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("render");
    let configs = [("sample", false), ("large", true)];

    for (name, large) in configs {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new(name, name), &large, |b, large| {
            b.iter_with_large_drop(|| {
                let mut buffer = RenderBuffer::new();
                if *large {
                    render_into(&mut buffer, LargePage);
                } else {
                    render_into(&mut buffer, SamplePage);
                }
                buffer
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
