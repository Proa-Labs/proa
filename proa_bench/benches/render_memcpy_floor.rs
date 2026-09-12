// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stable Proa render regression pair used by the performance gate.
//!
//! Cross-framework comparisons live in `Proa-Labs/proa-benchmarks`; this
//! benchmark intentionally retains only the full-page render and the cache-hot
//! memcpy floor for the exact same bytes.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use proa_bench::proa_page;

/// Write the exact bytes used by the CI performance fixture when requested.
///
/// The report hashes this file rather than treating byte length as fixture
/// identity. Keeping the write outside Criterion's timed closures also means
/// observability cannot perturb the measurements.
fn performance_fixture() -> Vec<u8> {
    let fixture = proa_page::render_safetext_owned().into_bytes();
    if let Some(path) = std::env::var_os("PROA_PERF_FIXTURE_OUTPUT") {
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create performance fixture directory");
        }
        std::fs::write(path, &fixture).expect("write performance fixture bytes");
    }
    fixture
}

fn bench_render_memcpy_floor(c: &mut Criterion) {
    let source = performance_fixture();
    let mut rendered = vec![0_u8; source.len()];
    let rendered_len = proa_page::render_safetext_to_cursor(&mut rendered)
        .expect("performance fixture must fit its exact-size output buffer");
    assert_eq!(rendered_len, source.len());
    assert_eq!(&rendered[..rendered_len], source.as_slice());

    let mut group = c.benchmark_group("render_memcpy_floor");
    group.throughput(Throughput::Bytes(source.len() as u64));

    group.bench_function("proa_html_safetext_stack", |b| {
        b.iter(|| {
            let len = proa_page::render_safetext_to_cursor(black_box(&mut rendered))
                .expect("performance fixture must fit its exact-size output buffer");
            black_box(&rendered[..len]);
        });
    });

    group.bench_function("memcpy_page_floor", |b| {
        let mut destination = vec![0_u8; source.len()];
        b.iter(|| {
            destination.copy_from_slice(black_box(source.as_slice()));
            black_box(destination.as_slice());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_render_memcpy_floor);
criterion_main!(benches);
