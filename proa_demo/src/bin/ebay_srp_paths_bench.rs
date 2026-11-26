// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Microbenchmark: ChainedBumpWriter vs Vec vs ZeroCopyBuf for the eBay
//! SRP listings render. All three render the same markup; only the write
//! buffer changes.
//!
//! Run:
//!   cargo run --release -p proa_demo --bin ebay_srp_paths_bench

use std::time::Instant;

use proa_core::RenderBuffer;
use proa_demo::ebay_srp::{
    default_listings, render_listings_into, render_listings_into_chained,
    render_listings_into_zerocopy,
};

const ITERS: usize = 5_000;
const WARMUP: usize = 500;
const LISTINGS: usize = 60;

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let r = (pct / 100.0) * (sorted.len() - 1) as f64;
    let lo = r.floor() as usize;
    let hi = r.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = r - lo as f64;
        ((sorted[lo] as f64) + (sorted[hi] - sorted[lo]) as f64 * frac) as u64
    }
}

fn report(name: &str, samples: &mut [u64], output_bytes: usize, extra: &str) {
    samples.sort_unstable();
    let mean = samples.iter().sum::<u64>() as f64 / samples.len() as f64;
    let p50 = percentile(samples, 50.0);
    let p99 = percentile(samples, 99.0);
    let min = *samples.first().unwrap();
    let max = *samples.last().unwrap();
    let to_us = |ns: u64| ns as f64 / 1_000.0;
    println!(
        "  {name:<32}  mean {:>7.2} µs  min {:>7.2}  p50 {:>7.2}  p99 {:>7.2}  max {:>7.2}  bytes {:>7}  {extra}",
        mean / 1_000.0,
        to_us(min),
        to_us(p50),
        to_us(p99),
        to_us(max),
        output_bytes
    );
}

fn main() {
    let listings = default_listings(LISTINGS);
    println!("=== eBay SRP render paths ({LISTINGS} listings, {ITERS} iters)\n");

    // Path A — Vec<u8> (the simple "render to a String" path)
    {
        let mut buf = Vec::with_capacity(1024 * 1024);
        for _ in 0..WARMUP {
            render_listings_into(listings, &mut buf);
        }
        let mut samples = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let t = Instant::now();
            render_listings_into(listings, &mut buf);
            samples.push(t.elapsed().as_nanos() as u64);
            std::hint::black_box(buf.len());
        }
        let bytes = buf.len();
        report("Vec<u8> (reused)", &mut samples, bytes, "");
    }

    // Path B — RenderBuffer (4 KiB pages)
    {
        let mut buf = RenderBuffer::new();
        for _ in 0..WARMUP {
            render_listings_into_chained(listings, &mut buf);
        }
        let mut samples = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let t = Instant::now();
            render_listings_into_chained(listings, &mut buf);
            samples.push(t.elapsed().as_nanos() as u64);
        }
        let pages = buf.page_count();
        let bytes: usize = buf.slices().map(|s| s.len()).sum();
        report(
            "ChainedBumpWriter<4KB>",
            &mut samples,
            bytes,
            &format!("pages={pages}"),
        );
    }

    // Path C — ZeroCopyBuf (scatter-gather; statics are pointers, not copies)
    {
        // Warmup with a fresh ZC buf each time (it's not designed to be cleared
        // and reused — the static refs hold; the owned buffer would need reset).
        for _ in 0..WARMUP {
            let zc = render_listings_into_zerocopy(listings);
            std::hint::black_box(zc.parts.len());
        }
        let mut samples = Vec::with_capacity(ITERS);
        let mut last_parts = 0usize;
        let mut last_owned = 0usize;
        let mut last_total = 0usize;
        for _ in 0..ITERS {
            let t = Instant::now();
            let zc = render_listings_into_zerocopy(listings);
            samples.push(t.elapsed().as_nanos() as u64);
            last_parts = zc.parts.len();
            last_owned = zc.owned.len();
            last_total = zc
                .parts
                .iter()
                .map(|p| match p {
                    proa_core::Part::Static(s) => s.len(),
                    proa_core::Part::Owned { len, .. } => *len as usize,
                })
                .sum();
            std::hint::black_box(zc.parts.len());
        }
        report(
            "ZeroCopyBuf",
            &mut samples,
            last_total,
            &format!("parts={last_parts}  owned_bytes={last_owned}"),
        );
    }

    // Path D — ZeroCopyBuf + flatten_into a reusable Vec (apples-to-apples
    // with the Vec path, since the final response body is a Vec anyway).
    {
        let zc_template = render_listings_into_zerocopy(listings);
        let mut flat = Vec::with_capacity(zc_template.owned.len() + 16 * 1024);
        for _ in 0..WARMUP {
            let zc = render_listings_into_zerocopy(listings);
            zc.flatten_into(&mut flat);
        }
        let mut samples = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let t = Instant::now();
            let zc = render_listings_into_zerocopy(listings);
            zc.flatten_into(&mut flat);
            samples.push(t.elapsed().as_nanos() as u64);
            std::hint::black_box(flat.len());
        }
        report("ZeroCopyBuf → flatten", &mut samples, flat.len(), "");
    }
}
