// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Complex Control Flow SSR Benchmark
//!
//! Unlike the focused render/memcpy floor, this benchmark exercises real-world
//! template complexity:
//! - Deeply nested conditionals (5+ levels)
//! - Pattern matching on enum variants → different DOM shapes
//! - Computed dynamic class composition
//! - Inline filter/map/sort chains
//! - Format/transform function calls
//! - Conditional rendering with fallbacks
//!
//! It serves as an internal regression fixture for Proa's full-language template
//! lowering rather than a cross-framework comparison.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proa_bench::complex_page;

fn bench_complex_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_control_flow");

    // Proa with buffer reuse
    group.bench_function("proa_complex_reuse", |b| {
        let mut buffer = Vec::with_capacity(64 * 1024);
        b.iter(|| {
            complex_page::render_into(&mut buffer);
            black_box(buffer.len())
        });
    });

    // Proa with fresh allocation
    group.bench_function("proa_complex_fresh", |b| {
        b.iter(|| {
            let html = complex_page::render();
            black_box(html.len())
        });
    });

    // Proa with RenderBuffer
    group.bench_function("proa_complex_chained", |b| {
        let mut buf = proa_core::RenderBuffer::new();
        b.iter(|| {
            complex_page::render_into_chained(&mut buf);
            black_box(buf.page_count())
        });
    });

    group.finish();
}

criterion_group!(benches, bench_complex_render);
criterion_main!(benches);
