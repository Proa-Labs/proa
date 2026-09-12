// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sweep the `extend_static` inline threshold in ZeroCopyBuf.
//!
//! Two workloads:
//! 1. **tiny_statics** — the scatter_gather profile (82% of statics ≤15B)
//! 2. **consolidated** — simulates html! macro output after static consolidation,
//!    where consecutive HTML is merged into large byte literals (256B–4KB)
//!    punctuated by short dynamic values
//!
//! The consolidated workload is what matters most — it's what Proa actually
//! generates. ZeroCopyBuf's value proposition is avoiding memcpy on those
//! large consolidated statics.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use proa_core::shared::write_buf::Part;

// ---------------------------------------------------------------------------
// ZeroCopyBuf clone with configurable threshold
// ---------------------------------------------------------------------------

struct ZcbWithThreshold {
    parts: Vec<Part>,
    owned: Vec<u8>,
    total_len: usize,
    threshold: usize,
}

impl ZcbWithThreshold {
    fn new(est_parts: usize, est_owned: usize, threshold: usize) -> Self {
        Self {
            parts: Vec::with_capacity(est_parts),
            owned: Vec::with_capacity(est_owned),
            total_len: 0,
            threshold,
        }
    }

    #[inline(always)]
    fn extend_owned(&mut self, bytes: &[u8]) {
        let start = self.owned.len() as u32;
        self.owned.extend_from_slice(bytes);
        if let Some(Part::Owned { len: prev_len, .. }) = self.parts.last_mut() {
            *prev_len += bytes.len() as u32;
        } else {
            self.parts.push(Part::Owned {
                start,
                len: bytes.len() as u32,
            });
        }
        self.total_len += bytes.len();
    }

    #[inline(always)]
    fn extend_static(&mut self, bytes: &'static [u8]) {
        if bytes.len() <= self.threshold {
            self.extend_owned(bytes);
        } else {
            self.parts.push(Part::Static(bytes));
            self.total_len += bytes.len();
        }
    }

    #[inline(always)]
    fn extend_dynamic(&mut self, bytes: &[u8]) {
        self.extend_owned(bytes);
    }

    fn flatten(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for part in &self.parts {
            match part {
                Part::Static(s) => out.extend_from_slice(s),
                Part::Owned { start, len } => {
                    let s = *start as usize;
                    let l = *len as usize;
                    out.extend_from_slice(&self.owned[s..s + l]);
                }
            }
        }
        out
    }

    fn flatten_into(&self, dst: &mut Vec<u8>) {
        dst.clear();
        dst.reserve(self.total_len);
        for part in &self.parts {
            match part {
                Part::Static(s) => dst.extend_from_slice(s),
                Part::Owned { start, len } => {
                    let s = *start as usize;
                    let l = *len as usize;
                    dst.extend_from_slice(&self.owned[s..s + l]);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WriteOp — models a single write call from codegen
// ---------------------------------------------------------------------------

enum WriteOp {
    Static(&'static [u8]),
    Dynamic(Vec<u8>),
}

// ---------------------------------------------------------------------------
// Workload: consolidated statics (realistic html! macro output)
//
// After static consolidation, the html! macro merges consecutive HTML into
// single byte literals. A typical component produces:
//   - 300-2000B consolidated static (opening tags, classes, attributes)
//   - 10-80B dynamic value (name, price, URL)
//   - 200-1500B consolidated static (mid-section HTML)
//   - 10-80B dynamic value
//   - 300-1000B consolidated static (closing tags)
//
// For a list of 50 product cards, this creates ~150 static writes (many large)
// and ~100 dynamic writes.
// ---------------------------------------------------------------------------

/// Generate a &'static [u8] of the given size with realistic HTML content.
/// Uses Box::leak to create 'static references (fine for benchmarks).
fn make_static(tag: &str, size: usize) -> &'static [u8] {
    let mut v = Vec::with_capacity(size);
    let patterns: &[&[u8]] = &[
        b"<div class=\"flex items-center justify-between gap-4 rounded-xl border border-border/50 bg-card p-6 shadow-sm transition-all hover:shadow-md hover:border-primary/20\">",
        b"<span class=\"text-sm font-medium leading-relaxed text-muted-foreground tracking-tight\">",
        b"<div data-slot=\"card-content\" class=\"relative flex w-full min-w-0 flex-col gap-3\">",
        b"<img loading=\"lazy\" decoding=\"async\" class=\"aspect-video w-full rounded-lg object-cover\" ",
        b"<button type=\"button\" class=\"inline-flex items-center justify-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground ring-offset-background transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring\">",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
        b"<nav aria-label=\"breadcrumb\" class=\"flex items-center space-x-2 text-sm text-muted-foreground\">",
        b"</div></div></section></main>",
    ];

    // Tag the fragment for identification
    let header = format!("<!-- {} -->", tag);
    v.extend_from_slice(header.as_bytes());

    let mut pat_idx = 0;
    while v.len() < size {
        let pat = patterns[pat_idx % patterns.len()];
        let remaining = size - v.len();
        v.extend_from_slice(&pat[..remaining.min(pat.len())]);
        pat_idx += 1;
    }
    v.truncate(size);
    Box::leak(v.into_boxed_slice())
}

fn build_consolidated_workload() -> (Vec<WriteOp>, usize) {
    let mut ops = Vec::with_capacity(500);
    let mut total = 0usize;

    // Page shell — large consolidated statics
    let page_header = make_static("page-header", 2048);
    ops.push(WriteOp::Static(page_header));
    total += page_header.len();

    // Dynamic: page title
    let title = b"Luxury Beachfront Properties - Malibu, CA".to_vec();
    ops.push(WriteOp::Dynamic(title.clone()));
    total += title.len();

    let after_title = make_static("after-title", 512);
    ops.push(WriteOp::Static(after_title));
    total += after_title.len();

    // Dynamic: subtitle
    let subtitle = b"243 stays available for your dates".to_vec();
    ops.push(WriteOp::Dynamic(subtitle.clone()));
    total += subtitle.len();

    let after_subtitle = make_static("after-subtitle", 384);
    ops.push(WriteOp::Static(after_subtitle));
    total += after_subtitle.len();

    // Render 50 product cards (like Airbnb listing grid)
    for card_idx in 0..50 {
        // Card opening — large consolidated static (all the wrapper divs, classes)
        let card_open = make_static(&format!("card-{}-open", card_idx), 800);
        ops.push(WriteOp::Static(card_open));
        total += card_open.len();

        // Dynamic: image URL
        let img_url = format!("/images/listing-{:04}.webp", card_idx);
        ops.push(WriteOp::Dynamic(img_url.as_bytes().to_vec()));
        total += img_url.len();

        // Mid-card static (image closing tag, title section opening)
        let mid1 = make_static(&format!("card-{}-mid1", card_idx), 480);
        ops.push(WriteOp::Static(mid1));
        total += mid1.len();

        // Dynamic: listing title
        let titles = [
            "Oceanfront Villa with Infinity Pool",
            "Modern Loft in Downtown Arts District",
            "Cozy Cabin in Big Bear Mountains",
            "Beachside Bungalow with Private Garden",
            "Penthouse Suite with City Views",
        ];
        let t = titles[card_idx % titles.len()].as_bytes().to_vec();
        ops.push(WriteOp::Dynamic(t.clone()));
        total += t.len();

        // Mid-card static (title closing, rating section)
        let mid2 = make_static(&format!("card-{}-mid2", card_idx), 360);
        ops.push(WriteOp::Static(mid2));
        total += mid2.len();

        // Dynamic: rating
        let rating = format!("★ {}.{}", 4 + (card_idx % 2), card_idx % 10);
        ops.push(WriteOp::Dynamic(rating.as_bytes().to_vec()));
        total += rating.len();

        // Mid-card static (rating closing, price section)
        let mid3 = make_static(&format!("card-{}-mid3", card_idx), 256);
        ops.push(WriteOp::Static(mid3));
        total += mid3.len();

        // Dynamic: price
        let price = format!("${}/night", 89 + card_idx * 23);
        ops.push(WriteOp::Dynamic(price.as_bytes().to_vec()));
        total += price.len();

        // Card closing — consolidated static
        let card_close = make_static(&format!("card-{}-close", card_idx), 320);
        ops.push(WriteOp::Static(card_close));
        total += card_close.len();
    }

    // Page footer
    let page_footer = make_static("page-footer", 1536);
    ops.push(WriteOp::Static(page_footer));
    total += page_footer.len();

    (ops, total)
}

// ---------------------------------------------------------------------------
// Baseline: plain Vec<u8> (no scatter-gather, just memcpy everything)
// ---------------------------------------------------------------------------

fn bench_vec_baseline(ops: &[WriteOp], total_size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(total_size);
    for op in ops {
        match op {
            WriteOp::Static(s) => buf.extend_from_slice(s),
            WriteOp::Dynamic(d) => buf.extend_from_slice(d),
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// Criterion benchmarks
// ---------------------------------------------------------------------------

fn print_workload_stats(name: &str, ops: &[WriteOp], total_size: usize) {
    let mut size_buckets = [0usize; 8];
    let mut static_count = 0;
    let mut static_bytes = 0usize;
    let mut dynamic_count = 0;
    let mut dynamic_bytes = 0usize;
    for op in ops {
        match op {
            WriteOp::Static(s) => {
                static_count += 1;
                static_bytes += s.len();
                let bucket = match s.len() {
                    0..=15 => 0,
                    16..=63 => 1,
                    64..=127 => 2,
                    128..=255 => 3,
                    256..=511 => 4,
                    512..=1023 => 5,
                    1024..=4095 => 6,
                    _ => 7,
                };
                size_buckets[bucket] += 1;
            }
            WriteOp::Dynamic(d) => {
                dynamic_count += 1;
                dynamic_bytes += d.len();
            }
        }
    }

    println!("\n=== {} ===", name);
    println!(
        "Total: {} ops, {:.1}KB ({} static [{:.1}KB], {} dynamic [{:.1}KB])",
        ops.len(),
        total_size as f64 / 1024.0,
        static_count,
        static_bytes as f64 / 1024.0,
        dynamic_count,
        dynamic_bytes as f64 / 1024.0,
    );
    println!("Static size distribution:");
    println!("  0-15B:     {:>4} ops", size_buckets[0]);
    println!("  16-63B:    {:>4} ops", size_buckets[1]);
    println!("  64-127B:   {:>4} ops", size_buckets[2]);
    println!("  128-255B:  {:>4} ops", size_buckets[3]);
    println!("  256-511B:  {:>4} ops", size_buckets[4]);
    println!("  512-1023B: {:>4} ops", size_buckets[5]);
    println!("  1-4KB:     {:>4} ops", size_buckets[6]);
    println!("  4KB+:      {:>4} ops", size_buckets[7]);
}

fn print_threshold_table(ops: &[WriteOp], total_size: usize, thresholds: &[usize]) {
    println!(
        "\n{:>6} | {:>6} | {:>8} | {:>10} | {:>12}",
        "thresh", "parts", "owned_B", "zero_copy%", "bytes_saved"
    );
    let baseline_owned = total_size; // Vec<u8> copies everything
    for &thresh in thresholds {
        let mut zcb = ZcbWithThreshold::new(ops.len(), 64 * 1024, thresh);
        for op in ops {
            match op {
                WriteOp::Static(s) => zcb.extend_static(s),
                WriteOp::Dynamic(d) => zcb.extend_dynamic(d),
            }
        }
        let static_bytes: usize = zcb
            .parts
            .iter()
            .map(|p| match p {
                Part::Static(s) => s.len(),
                _ => 0,
            })
            .sum();
        let saved = baseline_owned - zcb.owned.len();
        let label = if thresh == usize::MAX {
            "∞".to_string()
        } else {
            thresh.to_string()
        };
        println!(
            "{:>6} | {:>6} | {:>8} | {:>9.1}% | {:>10.1}KB",
            label,
            zcb.parts.len(),
            zcb.owned.len(),
            100.0 * static_bytes as f64 / total_size as f64,
            saved as f64 / 1024.0,
        );
    }
}

fn bench_workload(
    c: &mut Criterion,
    name: &str,
    ops: &[WriteOp],
    total_size: usize,
    thresholds: &[usize],
) {
    // Vec<u8> baseline
    {
        let mut group = c.benchmark_group(format!("{}_baseline", name));
        group.throughput(Throughput::Bytes(total_size as u64));
        group.bench_function("vec_u8", |b| {
            b.iter(|| black_box(bench_vec_baseline(black_box(ops), total_size).len()))
        });
        group.finish();
    }

    // Render (build ZCB)
    {
        let mut group = c.benchmark_group(format!("{}_render", name));
        group.throughput(Throughput::Bytes(total_size as u64));
        for &thresh in thresholds {
            let label = if thresh == usize::MAX {
                "inf".to_string()
            } else {
                format!("{}B", thresh)
            };
            group.bench_function(BenchmarkId::new("threshold", &label), |b| {
                b.iter(|| {
                    let mut zcb = ZcbWithThreshold::new(ops.len(), 64 * 1024, black_box(thresh));
                    for op in ops {
                        match op {
                            WriteOp::Static(s) => zcb.extend_static(s),
                            WriteOp::Dynamic(d) => zcb.extend_dynamic(d),
                        }
                    }
                    black_box(zcb.parts.len() + zcb.total_len)
                })
            });
        }
        group.finish();
    }

    // Flatten
    {
        let mut group = c.benchmark_group(format!("{}_flatten", name));
        group.throughput(Throughput::Bytes(total_size as u64));
        for &thresh in thresholds {
            let label = if thresh == usize::MAX {
                "inf".to_string()
            } else {
                format!("{}B", thresh)
            };
            let mut zcb = ZcbWithThreshold::new(ops.len(), 64 * 1024, thresh);
            for op in ops {
                match op {
                    WriteOp::Static(s) => zcb.extend_static(s),
                    WriteOp::Dynamic(d) => zcb.extend_dynamic(d),
                }
            }
            group.bench_function(BenchmarkId::new("threshold", &label), |b| {
                let mut dst = Vec::with_capacity(total_size);
                b.iter(|| {
                    zcb.flatten_into(&mut dst);
                    black_box(dst.len())
                })
            });
        }
        group.finish();
    }

    // Total (render + flatten)
    {
        let mut group = c.benchmark_group(format!("{}_total", name));
        group.throughput(Throughput::Bytes(total_size as u64));

        // Vec<u8> baseline in the same group for direct comparison
        group.bench_function("vec_u8", |b| {
            b.iter(|| black_box(bench_vec_baseline(black_box(ops), total_size).len()))
        });

        for &thresh in thresholds {
            let label = if thresh == usize::MAX {
                "inf".to_string()
            } else {
                format!("{}B", thresh)
            };
            group.bench_function(BenchmarkId::new("zcb", &label), |b| {
                let mut dst = Vec::with_capacity(total_size);
                b.iter(|| {
                    let mut zcb = ZcbWithThreshold::new(ops.len(), 64 * 1024, black_box(thresh));
                    for op in ops {
                        match op {
                            WriteOp::Static(s) => zcb.extend_static(s),
                            WriteOp::Dynamic(d) => zcb.extend_dynamic(d),
                        }
                    }
                    zcb.flatten_into(&mut dst);
                    black_box(dst.len())
                })
            });
        }
        group.finish();
    }

    // Correctness
    let reference = bench_vec_baseline(ops, total_size);
    for &thresh in thresholds {
        let mut zcb = ZcbWithThreshold::new(ops.len(), 64 * 1024, thresh);
        for op in ops {
            match op {
                WriteOp::Static(s) => zcb.extend_static(s),
                WriteOp::Dynamic(d) => zcb.extend_dynamic(d),
            }
        }
        assert_eq!(
            reference,
            zcb.flatten(),
            "threshold {} output mismatch",
            thresh
        );
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let thresholds = [0, 16, 64, 128, 256, 512, 1024, usize::MAX];

    // Workload: consolidated statics (realistic Proa output)
    let (cons_ops, cons_total) = build_consolidated_workload();
    print_workload_stats(
        "Consolidated (realistic html! output)",
        &cons_ops,
        cons_total,
    );
    print_threshold_table(&cons_ops, cons_total, &thresholds);
    bench_workload(c, "consolidated", &cons_ops, cons_total, &thresholds);

    println!("\nAll outputs verified identical. ✓");
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
