// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use proa_core::{ctx::Ctx, WebRenderSync};

// Test strings with varying escape densities
const NO_ESCAPES: &str = "Hello world this is a normal string with no special characters";
const FEW_ESCAPES: &str = "Hello <world> this & that";
const MANY_ESCAPES: &str = "<>&<>&<>&<>&<>&<>&<>&<>&<>&<>&";
const LONG_NO_ESCAPES: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam.";

fn bench_escape_text_normal(c: &mut Criterion) {
    let mut group = c.benchmark_group("escape_text_normal");

    for (name, input) in [
        ("no_escapes", NO_ESCAPES),
        ("few_escapes", FEW_ESCAPES),
        ("many_escapes", MANY_ESCAPES),
        ("long_no_escapes", LONG_NO_ESCAPES),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &input, |b, &s| {
            let mut buffer = Vec::with_capacity(s.len() * 2);

            b.iter(|| {
                buffer.clear();
                // Access the private escape function through public API
                let mut cx = Ctx::with_buffer(&mut buffer);
                proa_core::text::text(s).render(&mut cx).unwrap();
                black_box(buffer.len());
            });
        });
    }

    group.finish();
}

fn bench_escape_attr_value(c: &mut Criterion) {
    let mut group = c.benchmark_group("escape_attr_value");

    const ATTR_NO_ESCAPES: &str = "my-class-name";
    const ATTR_WITH_QUOTES: &str = "my\"class'name";
    const ATTR_WITH_HTML: &str = "my<class>&name";

    for (name, input) in [
        ("no_escapes", ATTR_NO_ESCAPES),
        ("with_quotes", ATTR_WITH_QUOTES),
        ("with_html", ATTR_WITH_HTML),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &input, |b, &s| {
            use proa_core::{el, Attr, AttrKey, Tag};
            let mut buffer = Vec::with_capacity(256);

            b.iter(|| {
                buffer.clear();
                let mut cx = Ctx::with_buffer(&mut buffer);
                el(
                    Tag::Div,
                    [Attr::Str {
                        k: AttrKey::Class,
                        v: s,
                    }],
                    |_| Ok(()),
                )
                .render(&mut cx)
                .unwrap();
                black_box(buffer.len());
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_escape_text_normal, bench_escape_attr_value);
criterion_main!(benches);
