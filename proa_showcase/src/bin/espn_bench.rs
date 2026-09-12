// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::WebContext;
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

fn main() {
    let warmup = env_usize("BENCH_WARMUP", 1_000);
    let iterations = env_usize("BENCH_ITERATIONS", 10_000);

    let sample = proa_showcase::espn::render();
    assert!(sample.contains("data-rsjs-island-root"));

    for _ in 0..warmup {
        std::hint::black_box(proa_showcase::espn::render());
    }
    let mut fresh = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let html = proa_showcase::espn::render();
        fresh.push(start.elapsed().as_nanos() as f64 / 1_000.0);
        std::hint::black_box(html);
    }

    let mut output = Vec::with_capacity(sample.len() + 1024);
    for _ in 0..warmup {
        output.clear();
        let mut cx = WebContext::with_buffer(&mut output);
        proa_showcase::espn::render_into(&mut cx, &proa_showcase::espn::GAME).unwrap();
        std::hint::black_box(output.len());
    }
    let mut reused = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        output.clear();
        let mut cx = WebContext::with_buffer(&mut output);
        let start = Instant::now();
        proa_showcase::espn::render_into(&mut cx, &proa_showcase::espn::GAME).unwrap();
        reused.push(start.elapsed().as_nanos() as f64 / 1_000.0);
        std::hint::black_box(output.len());
    }

    println!("ESPN_PROA_BENCH warmup={warmup} iterations={iterations}");
    print_stats("proa_render_fresh", &mut fresh, sample.len());
    print_stats("proa_render_reused_vec", &mut reused, output.len());
}
