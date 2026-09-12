// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

// ── Per-value benchmarks (fixed inputs, one at a time) ──

fn bench_u64_fixed(c: &mut Criterion) {
    let values: &[(u64, &str)] = &[
        (5, "01d"),
        (42, "02d"),
        (7890, "04d"),
        (12345, "05d"),
        (2345678, "07d"),
        (1_000_000_000, "10d"),
        (56789012345678, "14d"),
        (u64::MAX, "20d"),
    ];

    let mut group = c.benchmark_group("u64_fixed");
    group.warm_up_time(std::time::Duration::from_millis(500));
    group.measurement_time(std::time::Duration::from_secs(2));
    group.sample_size(50);

    for &(val, label) in values {
        group.bench_with_input(
            BenchmarkId::new("itoa_lut_write_u64", label),
            &val,
            |b, &v| {
                b.iter(|| {
                    let mut buf = [0u8; 20];
                    let n = itoa_lut::write_u64(black_box(v), &mut buf);
                    black_box(&buf[..n]);
                });
            },
        );
        group.bench_with_input(BenchmarkId::new("itoa_lut_buffer", label), &val, |b, &v| {
            b.iter(|| {
                let mut buf = itoa_lut::Buffer::new();
                black_box(buf.format(black_box(v)));
            });
        });
        group.bench_with_input(BenchmarkId::new("itoa", label), &val, |b, &v| {
            b.iter(|| {
                let mut buf = itoa::Buffer::new();
                black_box(buf.format(black_box(v)));
            });
        });
        group.bench_with_input(BenchmarkId::new("itoap", label), &val, |b, &v| {
            b.iter(|| {
                let mut buf = [0u8; 21];
                let n = itoap::write(&mut buf[..], black_box(v)).unwrap();
                black_box(&buf[..n]);
            });
        });
    }
    group.finish();
}

// ── Randomized throughput benchmarks (realistic workload) ──

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

fn random_values(seed: u64, count: usize) -> Vec<u64> {
    let mut x = seed;
    (0..count).map(|_| xorshift(&mut x)).collect()
}

fn random_values_clamped(seed: u64, count: usize, max: u64) -> Vec<u64> {
    let mut x = seed;
    (0..count).map(|_| xorshift(&mut x) % (max + 1)).collect()
}

macro_rules! fixed_int_bench {
    ($fn_name:ident, $group_name:literal, $ty:ty, $buf_len:expr, $write_fn:path, $values:expr) => {
        fn $fn_name(c: &mut Criterion) {
            let values: &[($ty, &str)] = $values;

            let mut group = c.benchmark_group($group_name);
            group.warm_up_time(std::time::Duration::from_millis(300));
            group.measurement_time(std::time::Duration::from_secs(1));
            group.sample_size(20);

            for &(val, label) in values {
                group.bench_with_input(BenchmarkId::new("itoa_lut_write", label), &val, |b, &v| {
                    b.iter(|| {
                        let mut buf = [0u8; $buf_len];
                        let n = $write_fn(black_box(v), &mut buf);
                        black_box(&buf[..n]);
                    });
                });
                group.bench_with_input(
                    BenchmarkId::new("itoa_lut_buffer", label),
                    &val,
                    |b, &v| {
                        b.iter(|| {
                            let mut buf = itoa_lut::Buffer::new();
                            black_box(buf.format(black_box(v)));
                        });
                    },
                );
                group.bench_with_input(BenchmarkId::new("itoa", label), &val, |b, &v| {
                    b.iter(|| {
                        let mut buf = itoa::Buffer::new();
                        black_box(buf.format(black_box(v)));
                    });
                });
                group.bench_with_input(BenchmarkId::new("itoap", label), &val, |b, &v| {
                    b.iter(|| {
                        let mut buf = [0u8; $buf_len];
                        let n = itoap::write(&mut buf[..], black_box(v)).unwrap();
                        black_box(&buf[..n]);
                    });
                });
            }
            group.finish();
        }
    };
}

fixed_int_bench!(
    bench_u8_fixed,
    "u8_fixed",
    u8,
    3,
    itoa_lut::write_u8,
    &[(0, "zero"), (5, "01d"), (42, "02d"), (u8::MAX, "03d_max")]
);

fixed_int_bench!(
    bench_i8_fixed,
    "i8_fixed",
    i8,
    4,
    itoa_lut::write_i8,
    &[
        (0, "zero"),
        (5, "pos_01d"),
        (-5, "neg_01d"),
        (42, "pos_02d"),
        (-42, "neg_02d"),
        (i8::MAX, "max"),
        (i8::MIN, "min"),
    ]
);

fixed_int_bench!(
    bench_u32_fixed,
    "u32_fixed",
    u32,
    10,
    itoa_lut::write_u32,
    &[
        (5, "01d"),
        (42, "02d"),
        (7890, "04d"),
        (12345, "05d"),
        (2_345_678, "07d"),
        (1_000_000_000, "10d"),
        (u32::MAX, "10d_max"),
    ]
);

fixed_int_bench!(
    bench_i32_fixed,
    "i32_fixed",
    i32,
    11,
    itoa_lut::write_i32,
    &[
        (5, "pos_01d"),
        (-5, "neg_01d"),
        (42, "pos_02d"),
        (-42, "neg_02d"),
        (12_345, "pos_05d"),
        (-12_345, "neg_05d"),
        (2_345_678, "pos_07d"),
        (-2_345_678, "neg_07d"),
        (i32::MAX, "max"),
        (i32::MIN, "min"),
    ]
);

fn bench_random_full(c: &mut Criterion) {
    let values = random_values(0xDEAD_BEEF_CAFE_BABE, 4096);

    let mut group = c.benchmark_group("random_full_range");
    group.warm_up_time(std::time::Duration::from_millis(500));
    group.measurement_time(std::time::Duration::from_secs(3));
    group.sample_size(50);

    group.bench_function("itoa_lut_write_u64", |b| {
        b.iter(|| {
            let mut buf = [0u8; 20];
            for &v in &values {
                let n = itoa_lut::write_u64(black_box(v), &mut buf);
                black_box(&buf[..n]);
            }
        });
    });

    group.bench_function("itoa_lut_buffer", |b| {
        b.iter(|| {
            let mut buf = itoa_lut::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoa", |b| {
        b.iter(|| {
            let mut buf = itoa::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoap", |b| {
        b.iter(|| {
            let mut buf = [0u8; 21];
            for &v in &values {
                let n = itoap::write(&mut buf[..], black_box(v)).unwrap();
                black_box(&buf[..n]);
            }
        });
    });

    group.finish();
}

fn bench_random_small(c: &mut Criterion) {
    let values = random_values_clamped(0x1234_5678_9ABC_DEF0, 4096, 999_999);

    let mut group = c.benchmark_group("random_small_1_6d");
    group.warm_up_time(std::time::Duration::from_millis(500));
    group.measurement_time(std::time::Duration::from_secs(3));
    group.sample_size(50);

    group.bench_function("itoa_lut_write_u64", |b| {
        b.iter(|| {
            let mut buf = [0u8; 20];
            for &v in &values {
                let n = itoa_lut::write_u64(black_box(v), &mut buf);
                black_box(&buf[..n]);
            }
        });
    });

    group.bench_function("itoa_lut_buffer", |b| {
        b.iter(|| {
            let mut buf = itoa_lut::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoa", |b| {
        b.iter(|| {
            let mut buf = itoa::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoap", |b| {
        b.iter(|| {
            let mut buf = [0u8; 7];
            for &v in &values {
                let n = itoap::write(&mut buf[..], black_box(v)).unwrap();
                black_box(&buf[..n]);
            }
        });
    });

    group.finish();
}

fn bench_random_large(c: &mut Criterion) {
    let values: Vec<u64> = random_values(0xFEED_FACE_DEAD_C0DE, 4096)
        .into_iter()
        .map(|v| v.max(1_000_000_000))
        .collect();

    let mut group = c.benchmark_group("random_large_10_20d");
    group.warm_up_time(std::time::Duration::from_millis(500));
    group.measurement_time(std::time::Duration::from_secs(3));
    group.sample_size(50);

    group.bench_function("itoa_lut_write_u64", |b| {
        b.iter(|| {
            let mut buf = [0u8; 20];
            for &v in &values {
                let n = itoa_lut::write_u64(black_box(v), &mut buf);
                black_box(&buf[..n]);
            }
        });
    });

    group.bench_function("itoa_lut_buffer", |b| {
        b.iter(|| {
            let mut buf = itoa_lut::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoa", |b| {
        b.iter(|| {
            let mut buf = itoa::Buffer::new();
            for &v in &values {
                black_box(buf.format(black_box(v)));
            }
        });
    });

    group.bench_function("itoap", |b| {
        b.iter(|| {
            let mut buf = [0u8; 21];
            for &v in &values {
                let n = itoap::write(&mut buf[..], black_box(v)).unwrap();
                black_box(&buf[..n]);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_u8_fixed,
    bench_i8_fixed,
    bench_u32_fixed,
    bench_i32_fixed,
    bench_u64_fixed,
    bench_random_full,
    bench_random_small,
    bench_random_large
);
criterion_main!(benches);
