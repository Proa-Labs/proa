// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn print_stats(name: &str, timings: &mut [f64], size: usize) {
    timings.sort_by(|a, b| a.total_cmp(b));
    let avg = timings.iter().sum::<f64>() / timings.len() as f64;
    println!("{name}");
    println!("  avg: {avg:.2} us");
    println!("  p50: {:.2} us", percentile(timings, 0.50));
    println!("  p99: {:.2} us", percentile(timings, 0.99));
    println!("  min: {:.2} us", timings[0]);
    println!("  max: {:.2} us", timings[timings.len() - 1]);
    println!("  bytes: {size}");
}

fn prime_fresh_owned_allocator(capacity: usize) {
    // Model a warm, long-lived server process explicitly. Without an
    // output-sized allocation before this phase, glibc can use a different
    // large-allocation path even after render warmups have completed.
    let seed = vec![0; capacity];
    std::hint::black_box(seed.as_slice());
}

fn main() {
    let rows = env_usize("BENCH_ROWS", 1_000);
    let default_iterations = if rows >= 10_000 {
        25
    } else if rows >= 1_000 {
        200
    } else {
        1_000
    };
    let warmup = env_usize("BENCH_WARMUP", default_iterations.min(100));
    let iterations = env_usize("BENCH_ITERATIONS", default_iterations);

    let sample = proa_showcase::sentry_otel::render_benchmark_rows(rows);
    assert!(sample
        .as_bytes()
        .windows(12)
        .any(|window| window == b"data-log-row"));
    let sample_len = sample.len();
    drop(sample);
    prime_fresh_owned_allocator(sample_len);

    for _ in 0..warmup {
        std::hint::black_box(proa_showcase::sentry_otel::render_benchmark_rows(rows));
    }

    let mut timings = Vec::with_capacity(iterations);
    let mut size = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        let output = std::hint::black_box(proa_showcase::sentry_otel::render_benchmark_rows(rows));
        timings.push(start.elapsed().as_nanos() as f64 / 1_000.0);
        size = output.len();
    }

    println!("SENTRY_OTEL_PROA_BENCH rows={rows} warmup={warmup} iterations={iterations}");
    print_stats("proa_render_fresh_owned", &mut timings, size);
}
