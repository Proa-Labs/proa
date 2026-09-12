// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use proa_bench::proa_page::SafeTextBenchmarkPage;
use proa_core::{
    RenderBuffer, StreamingZeroCopyBuf, WebContext, WebRenderSync, WriteBuf, ZeroCopyBuf,
};

#[derive(Clone, Copy)]
enum Op {
    Static(&'static [u8]),
    Dynamic(&'static [u8]),
}

const MICRO_OPS: &[Op] = &[
    Op::Static(b"<div class=\""),
    Op::Dynamic(b"card-container"),
    Op::Static(b"\" data-kind=\""),
    Op::Dynamic(b"benchmark"),
    Op::Static(b"\">"),
    Op::Static(b"<h1>"),
    Op::Dynamic(b"Hello &amp; World"),
    Op::Static(b"</h1><p class=\""),
    Op::Dynamic(b"description"),
    Op::Static(b"\">"),
    Op::Dynamic(b"Lorem ipsum dolor sit amet, consectetur adipiscing elit."),
    Op::Static(b"</p></div>"),
];

fn bench_realistic_page_render(c: &mut Criterion) {
    let output_size = {
        let mut buf = Vec::with_capacity(32 * 1024);
        let mut cx = WebContext::with_buffer(&mut buf);
        SafeTextBenchmarkPage.render(&mut cx).unwrap();
        buf.len()
    };

    let mut group = c.benchmark_group("streaming_buffer_page_render");
    group.throughput(Throughput::Bytes(output_size as u64));

    group.bench_function("chained_default_render", |b| {
        let mut buf = RenderBuffer::new();
        b.iter(|| {
            buf.clear();
            {
                let mut cx = WebContext::with_buffer(&mut buf);
                SafeTextBenchmarkPage.render(&mut cx).unwrap();
            }
            black_box(buf.len())
        });
    });

    group.bench_function("zero_copy_render", |b| {
        let mut buf = ZeroCopyBuf::with_capacity(4096, 16 * 1024);
        b.iter(|| {
            buf.clear();
            {
                let mut cx = WebContext::with_buffer(&mut buf);
                SafeTextBenchmarkPage.render(&mut cx).unwrap();
            }
            black_box(buf.len())
        });
    });

    group.bench_function("streaming_zero_copy_render", |b| {
        let mut buf = StreamingZeroCopyBuf::with_capacity(4096, 16 * 1024);
        b.iter(|| {
            buf.clear();
            {
                let mut cx = WebContext::with_buffer(&mut buf);
                SafeTextBenchmarkPage.render(&mut cx).unwrap();
            }
            black_box(buf.len())
        });
    });

    group.bench_function("streaming_zero_copy_render_flush_all", |b| {
        let mut buf = StreamingZeroCopyBuf::with_capacity(4096, 16 * 1024);
        b.iter(|| {
            buf.clear();
            {
                let mut cx = WebContext::with_buffer(&mut buf);
                SafeTextBenchmarkPage.render(&mut cx).unwrap();
            }
            let chunk = buf.flush_all().expect("benchmark page should emit bytes");
            black_box(chunk.len())
        });
    });

    group.finish();
}

fn bench_mixed_buffer_ops(c: &mut Criterion) {
    const ITERATIONS: usize = 1000;
    let total_size: usize = MICRO_OPS
        .iter()
        .map(|op| match op {
            Op::Static(bytes) | Op::Dynamic(bytes) => bytes.len(),
        })
        .sum::<usize>()
        * ITERATIONS;

    let mut group = c.benchmark_group("streaming_buffer_mixed_ops");
    group.throughput(Throughput::Bytes(total_size as u64));

    group.bench_function("chained_4k_mixed_write", |b| {
        let mut buf = RenderBuffer::new();
        b.iter(|| {
            buf.clear();
            for _ in 0..ITERATIONS {
                for op in MICRO_OPS {
                    match op {
                        Op::Static(bytes) | Op::Dynamic(bytes) => {
                            buf.extend(bytes).unwrap();
                        }
                    }
                }
            }
            black_box(buf.len())
        });
    });

    group.bench_function("zero_copy_mixed_write", |b| {
        let mut buf = ZeroCopyBuf::with_capacity(MICRO_OPS.len() * ITERATIONS, total_size);
        b.iter(|| {
            buf.clear();
            for _ in 0..ITERATIONS {
                for op in MICRO_OPS {
                    match op {
                        Op::Static(bytes) => buf.extend_static(bytes).unwrap(),
                        Op::Dynamic(bytes) => buf.extend(bytes).unwrap(),
                    }
                }
            }
            black_box(buf.len())
        });
    });

    group.bench_function("streaming_zero_copy_mixed_write", |b| {
        let mut buf = StreamingZeroCopyBuf::with_capacity(MICRO_OPS.len() * ITERATIONS, 4096);
        b.iter(|| {
            buf.clear();
            for _ in 0..ITERATIONS {
                for op in MICRO_OPS {
                    match op {
                        Op::Static(bytes) => buf.extend_static(bytes).unwrap(),
                        Op::Dynamic(bytes) => buf.extend(bytes).unwrap(),
                    }
                }
            }
            black_box(buf.len())
        });
    });

    group.bench_function("streaming_zero_copy_mark_flush_per_record", |b| {
        let mut buf = StreamingZeroCopyBuf::with_capacity(MICRO_OPS.len() * 2, 512);
        b.iter(|| {
            buf.clear();
            let mut flushed = 0usize;
            for _ in 0..ITERATIONS {
                for op in MICRO_OPS {
                    match op {
                        Op::Static(bytes) => buf.extend_static(bytes).unwrap(),
                        Op::Dynamic(bytes) => buf.extend(bytes).unwrap(),
                    }
                }
                let mark = buf.mark();
                let chunk = buf.flush_through(mark).expect("record should flush");
                flushed += chunk.len();
            }
            black_box(flushed)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_realistic_page_render, bench_mixed_buffer_ops);
criterion_main!(benches);
