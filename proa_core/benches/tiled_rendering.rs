// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark: Standard (AoS) vs Tiled (blocked) loop rendering
//!
//! Investigates whether HPC-style loop tiling improves cache efficiency
//! for list rendering. The hypothesis: when rendering N items, each iteration
//! re-reads the same template bytes from .rodata. If the working set of the
//! template, output buffer head, and item data exceeds L1d, the template gets evicted
//! between iterations. Tiling keeps the template hot by processing multiple
//! items against the same fragment before moving to the next fragment.
//!
//! Apple Silicon (M-series) cache hierarchy:
//! L1d: 128KB (perf cores), 64KB (eff cores).
//! Cache line: 128 bytes.
//! L2: ~16MB shared.
//!
//! We test across template sizes and item counts to find the crossover point
//! where tiling beats standard rendering.

#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// ---------------------------------------------------------------------------
// Simulated component data
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Item {
    name: [u8; 32],
    price: [u8; 16],
    image_url: [u8; 64],
    description: [u8; 128],
    rating: [u8; 8],
    review_count: [u8; 8],
}

impl Item {
    fn new(idx: usize) -> Self {
        let mut name = [b' '; 32];
        let mut price = [b' '; 16];
        let mut image_url = [b' '; 64];
        let mut description = [b' '; 128];
        let mut rating = [b' '; 8];
        let mut review_count = [b' '; 8];

        // Fill with deterministic but varying content
        let tag = format!("item-{:04}", idx);
        let tag_bytes = tag.as_bytes();
        name[..tag_bytes.len().min(32)].copy_from_slice(&tag_bytes[..tag_bytes.len().min(32)]);

        let p = format!("${}.99", 10 + (idx % 990));
        price[..p.len().min(16)].copy_from_slice(&p.as_bytes()[..p.len().min(16)]);

        let url = format!("/img/product-{:04}.webp", idx);
        image_url[..url.len().min(64)].copy_from_slice(&url.as_bytes()[..url.len().min(64)]);

        let desc = format!(
            "Product {} is a high-quality item with excellent features and great value for your money.",
            idx
        );
        description[..desc.len().min(128)].copy_from_slice(&desc.as_bytes()[..desc.len().min(128)]);

        let r = format!("{}.{}", 3 + (idx % 2), idx % 10);
        rating[..r.len().min(8)].copy_from_slice(&r.as_bytes()[..r.len().min(8)]);

        let rc = format!("{}", 10 + idx % 500);
        review_count[..rc.len().min(8)].copy_from_slice(&rc.as_bytes()[..rc.len().min(8)]);

        Self {
            name,
            price,
            image_url,
            description,
            rating,
            review_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Template fragments — sized to stress different cache levels
// ---------------------------------------------------------------------------

/// Generate a static-like template fragment of the given size.
/// In real Proa code, these are `&'static [u8]` from compiled html! macros.
/// We use Box<[u8]> pinned once to simulate .rodata addresses.
struct TemplateSet {
    header: Box<[u8]>,
    after_name: Box<[u8]>,
    after_price: Box<[u8]>,
    after_image: Box<[u8]>,
    after_desc: Box<[u8]>,
    after_rating: Box<[u8]>,
    footer: Box<[u8]>,
}

impl TemplateSet {
    /// Create a template set where each fragment is approximately `frag_size` bytes.
    /// Total template per item = 7 * frag_size.
    fn new(frag_size: usize) -> Self {
        fn make_frag(tag: &str, size: usize) -> Box<[u8]> {
            let mut v = Vec::with_capacity(size);
            // Realistic HTML-ish content
            let prefix = format!("<div class=\"product-card-{tag} flex flex-col gap-2 rounded-xl border bg-card p-4 shadow-sm hover:shadow-md transition-shadow\">");
            v.extend_from_slice(prefix.as_bytes());
            // Pad to desired size with realistic HTML
            while v.len() < size {
                let padding = b"<span class=\"text-sm text-muted-foreground leading-relaxed\">";
                let remaining = size - v.len();
                v.extend_from_slice(&padding[..remaining.min(padding.len())]);
            }
            v.truncate(size);
            v.into_boxed_slice()
        }

        Self {
            header: make_frag("header", frag_size),
            after_name: make_frag("name", frag_size),
            after_price: make_frag("price", frag_size),
            after_image: make_frag("image", frag_size),
            after_desc: make_frag("desc", frag_size),
            after_rating: make_frag("rating", frag_size),
            footer: make_frag("footer", frag_size),
        }
    }

    /// Total template bytes per item render.
    fn per_item_bytes(&self) -> usize {
        self.header.len()
            + self.after_name.len()
            + self.after_price.len()
            + self.after_image.len()
            + self.after_desc.len()
            + self.after_rating.len()
            + self.footer.len()
    }

    /// Total output bytes per item (template + dynamic data).
    fn total_per_item(&self) -> usize {
        self.per_item_bytes() + 32 + 16 + 64 + 128 + 8 + 8 // Item field sizes
    }
}

// ---------------------------------------------------------------------------
// Standard (AoS) rendering — current Proa codegen pattern
// ---------------------------------------------------------------------------

#[inline(never)]
fn render_standard(items: &[Item], tmpl: &TemplateSet, buf: &mut Vec<u8>) {
    buf.clear();
    buf.reserve(items.len() * tmpl.total_per_item());

    for item in items {
        buf.extend_from_slice(&tmpl.header);
        buf.extend_from_slice(&item.name);
        buf.extend_from_slice(&tmpl.after_name);
        buf.extend_from_slice(&item.price);
        buf.extend_from_slice(&tmpl.after_price);
        buf.extend_from_slice(&item.image_url);
        buf.extend_from_slice(&tmpl.after_image);
        buf.extend_from_slice(&item.description);
        buf.extend_from_slice(&tmpl.after_desc);
        buf.extend_from_slice(&item.rating);
        buf.extend_from_slice(&tmpl.after_rating);
        buf.extend_from_slice(&item.review_count);
        buf.extend_from_slice(&tmpl.footer);
    }
}

// ---------------------------------------------------------------------------
// Tiled rendering — process items in blocks, fragment by fragment
// ---------------------------------------------------------------------------

/// Tiled approach using pre-computed offsets.
/// Since all items produce the same-sized output (fixed-length fields),
/// we can compute exact write positions and write out of order.
#[inline(never)]
fn render_tiled_precomputed(
    items: &[Item],
    tmpl: &TemplateSet,
    buf: &mut Vec<u8>,
    tile_size: usize,
) {
    let item_size = tmpl.total_per_item();
    let total = items.len() * item_size;
    buf.clear();
    buf.resize(total, 0);

    // Offsets within a single item's output
    let off_name = tmpl.header.len();
    let off_after_name = off_name + 32;
    let off_price = off_after_name + tmpl.after_name.len();
    let off_after_price = off_price + 16;
    let off_image = off_after_price + tmpl.after_price.len();
    let off_after_image = off_image + 64;
    let off_desc = off_after_image + tmpl.after_image.len();
    let off_after_desc = off_desc + 128;
    let off_rating = off_after_desc + tmpl.after_desc.len();
    let off_after_rating = off_rating + 8;
    let off_review = off_after_rating + tmpl.after_rating.len();
    let off_footer = off_review + 8;

    for chunk_start in (0..items.len()).step_by(tile_size) {
        let chunk_end = (chunk_start + tile_size).min(items.len());

        // Fragment 1: headers — template stays in L1 for all items in tile
        for i in chunk_start..chunk_end {
            let base = i * item_size;
            buf[base..base + tmpl.header.len()].copy_from_slice(&tmpl.header);
        }

        // Fragment 2: names — sequential data access
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_name;
            buf[base..base + 32].copy_from_slice(&items[i].name);
        }

        // Fragment 3: after_name template
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_name;
            buf[base..base + tmpl.after_name.len()].copy_from_slice(&tmpl.after_name);
        }

        // Fragment 4: prices
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_price;
            buf[base..base + 16].copy_from_slice(&items[i].price);
        }

        // Fragment 5: after_price template
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_price;
            buf[base..base + tmpl.after_price.len()].copy_from_slice(&tmpl.after_price);
        }

        // Fragment 6: image URLs
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_image;
            buf[base..base + 64].copy_from_slice(&items[i].image_url);
        }

        // Fragment 7: after_image template
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_image;
            buf[base..base + tmpl.after_image.len()].copy_from_slice(&tmpl.after_image);
        }

        // Fragment 8: descriptions
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_desc;
            buf[base..base + 128].copy_from_slice(&items[i].description);
        }

        // Fragment 9: after_desc template
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_desc;
            buf[base..base + tmpl.after_desc.len()].copy_from_slice(&tmpl.after_desc);
        }

        // Fragment 10: ratings
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_rating;
            buf[base..base + 8].copy_from_slice(&items[i].rating);
        }

        // Fragment 11: after_rating template
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_rating;
            buf[base..base + tmpl.after_rating.len()].copy_from_slice(&tmpl.after_rating);
        }

        // Fragment 12: review counts
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_review;
            buf[base..base + 8].copy_from_slice(&items[i].review_count);
        }

        // Fragment 13: footers
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_footer;
            buf[base..base + tmpl.footer.len()].copy_from_slice(&tmpl.footer);
        }
    }
}

/// Tiled approach using per-item buffers (simpler, but extra concatenation cost).
#[inline(never)]
fn render_tiled_multibuf(
    items: &[Item],
    tmpl: &TemplateSet,
    output: &mut Vec<u8>,
    tile_bufs: &mut Vec<Vec<u8>>,
    tile_size: usize,
) {
    let item_size = tmpl.total_per_item();

    // Ensure we have enough per-item buffers
    while tile_bufs.len() < tile_size {
        tile_bufs.push(Vec::with_capacity(item_size));
    }

    output.clear();
    output.reserve(items.len() * item_size);

    for chunk_start in (0..items.len()).step_by(tile_size) {
        let chunk_end = (chunk_start + tile_size).min(items.len());
        let chunk_len = chunk_end - chunk_start;

        // Clear tile buffers
        for buf in tile_bufs[..chunk_len].iter_mut() {
            buf.clear();
        }

        // Fragment 1: headers
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.header);
        }

        // Fragment 2: names
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].name);
        }

        // Fragment 3: after_name
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.after_name);
        }

        // Fragment 4: prices
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].price);
        }

        // Fragment 5: after_price
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.after_price);
        }

        // Fragment 6: image URLs
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].image_url);
        }

        // Fragment 7: after_image
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.after_image);
        }

        // Fragment 8: descriptions
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].description);
        }

        // Fragment 9: after_desc
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.after_desc);
        }

        // Fragment 10: ratings
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].rating);
        }

        // Fragment 11: after_rating
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.after_rating);
        }

        // Fragment 12: review counts
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&items[chunk_start + j].review_count);
        }

        // Fragment 13: footers
        for j in 0..chunk_len {
            tile_bufs[j].extend_from_slice(&tmpl.footer);
        }

        // Concatenate tile buffers into output
        for j in 0..chunk_len {
            output.extend_from_slice(&tile_bufs[j]);
        }
    }
}

/// Tiled approach using SoA (Structure of Arrays) data layout.
/// Items stored in columnar format for sequential field access.
#[inline(never)]
fn render_tiled_soa(
    names: &[[u8; 32]],
    prices: &[[u8; 16]],
    image_urls: &[[u8; 64]],
    descriptions: &[[u8; 128]],
    ratings: &[[u8; 8]],
    review_counts: &[[u8; 8]],
    tmpl: &TemplateSet,
    buf: &mut Vec<u8>,
    tile_size: usize,
) {
    let n = names.len();
    let item_size = tmpl.total_per_item();
    let total = n * item_size;
    buf.clear();
    buf.resize(total, 0);

    let off_name = tmpl.header.len();
    let off_after_name = off_name + 32;
    let off_price = off_after_name + tmpl.after_name.len();
    let off_after_price = off_price + 16;
    let off_image = off_after_price + tmpl.after_price.len();
    let off_after_image = off_image + 64;
    let off_desc = off_after_image + tmpl.after_image.len();
    let off_after_desc = off_desc + 128;
    let off_rating = off_after_desc + tmpl.after_desc.len();
    let off_after_rating = off_rating + 8;
    let off_review = off_after_rating + tmpl.after_rating.len();
    let off_footer = off_review + 8;

    for chunk_start in (0..n).step_by(tile_size) {
        let chunk_end = (chunk_start + tile_size).min(n);

        for i in chunk_start..chunk_end {
            let base = i * item_size;
            buf[base..base + tmpl.header.len()].copy_from_slice(&tmpl.header);
        }
        // SoA: names are contiguous in memory — sequential read
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_name;
            buf[base..base + 32].copy_from_slice(&names[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_name;
            buf[base..base + tmpl.after_name.len()].copy_from_slice(&tmpl.after_name);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_price;
            buf[base..base + 16].copy_from_slice(&prices[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_price;
            buf[base..base + tmpl.after_price.len()].copy_from_slice(&tmpl.after_price);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_image;
            buf[base..base + 64].copy_from_slice(&image_urls[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_image;
            buf[base..base + tmpl.after_image.len()].copy_from_slice(&tmpl.after_image);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_desc;
            buf[base..base + 128].copy_from_slice(&descriptions[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_desc;
            buf[base..base + tmpl.after_desc.len()].copy_from_slice(&tmpl.after_desc);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_rating;
            buf[base..base + 8].copy_from_slice(&ratings[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_after_rating;
            buf[base..base + tmpl.after_rating.len()].copy_from_slice(&tmpl.after_rating);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_review;
            buf[base..base + 8].copy_from_slice(&review_counts[i]);
        }
        for i in chunk_start..chunk_end {
            let base = i * item_size + off_footer;
            buf[base..base + tmpl.footer.len()].copy_from_slice(&tmpl.footer);
        }
    }
}

// ---------------------------------------------------------------------------
// Criterion benchmarks
// ---------------------------------------------------------------------------

fn criterion_benchmark(c: &mut Criterion) {
    // Template fragment sizes to test (bytes per fragment, 7 fragments per item)
    // 128B × 7 = 896B/item — fits L1 easily
    // 1KB × 7 = 7KB/item — moderate
    // 4KB × 7 = 28KB/item — starting to stress L1
    // 16KB × 7 = 112KB/item — exceeds L1d on efficiency cores
    let frag_sizes = [128, 1024, 4096, 16384];

    // Item counts
    let item_counts = [10, 50, 200, 1000];

    // Tile sizes to test
    let tile_sizes = [4, 8, 16, 32, 64];

    for &frag_size in &frag_sizes {
        let tmpl = TemplateSet::new(frag_size);
        let total_per_item = tmpl.total_per_item();

        for &n_items in &item_counts {
            let items: Vec<Item> = (0..n_items).map(Item::new).collect();
            let total_bytes = n_items * total_per_item;

            // SoA layout for the SoA benchmark
            let names: Vec<[u8; 32]> = items.iter().map(|i| i.name).collect();
            let prices: Vec<[u8; 16]> = items.iter().map(|i| i.price).collect();
            let image_urls: Vec<[u8; 64]> = items.iter().map(|i| i.image_url).collect();
            let descriptions: Vec<[u8; 128]> = items.iter().map(|i| i.description).collect();
            let ratings: Vec<[u8; 8]> = items.iter().map(|i| i.rating).collect();
            let review_counts: Vec<[u8; 8]> = items.iter().map(|i| i.review_count).collect();

            let group_name = format!("tiled_f{}b_n{}", frag_size, n_items);
            let mut group = c.benchmark_group(&group_name);
            group.throughput(Throughput::Bytes(total_bytes as u64));

            // Standard (AoS) rendering
            group.bench_function("standard", |b| {
                let mut buf = Vec::with_capacity(total_bytes);
                b.iter(|| {
                    render_standard(black_box(&items), black_box(&tmpl), &mut buf);
                    black_box(buf.len());
                })
            });

            // Tiled with precomputed offsets — sweep tile sizes
            for &tile in &tile_sizes {
                if tile > n_items {
                    continue;
                }
                group.bench_function(BenchmarkId::new("tiled_precomp", tile), |b| {
                    let mut buf = Vec::with_capacity(total_bytes);
                    b.iter(|| {
                        render_tiled_precomputed(
                            black_box(&items),
                            black_box(&tmpl),
                            &mut buf,
                            tile,
                        );
                        black_box(buf.len());
                    })
                });
            }

            // Tiled with multi-buffer — one tile size for comparison
            let best_tile = 16.min(n_items);
            group.bench_function(BenchmarkId::new("tiled_multibuf", best_tile), |b| {
                let mut output = Vec::with_capacity(total_bytes);
                let mut tile_bufs = Vec::new();
                b.iter(|| {
                    render_tiled_multibuf(
                        black_box(&items),
                        black_box(&tmpl),
                        &mut output,
                        &mut tile_bufs,
                        best_tile,
                    );
                    black_box(output.len());
                })
            });

            // SoA + tiled
            group.bench_function(BenchmarkId::new("tiled_soa", best_tile), |b| {
                let mut buf = Vec::with_capacity(total_bytes);
                b.iter(|| {
                    render_tiled_soa(
                        black_box(&names),
                        black_box(&prices),
                        black_box(&image_urls),
                        black_box(&descriptions),
                        black_box(&ratings),
                        black_box(&review_counts),
                        black_box(&tmpl),
                        &mut buf,
                        best_tile,
                    );
                    black_box(buf.len());
                })
            });

            group.finish();
        }
    }

    // Print working set analysis
    println!("\n=== Working set analysis ===");
    println!("Apple Silicon L1d: 128KB (perf) / 64KB (eff)");
    println!("Cache line: 128 bytes\n");
    for &frag_size in &frag_sizes {
        let tmpl = TemplateSet::new(frag_size);
        println!(
            "Fragment {}B: template/item = {}B, total/item = {}B",
            frag_size,
            tmpl.per_item_bytes(),
            tmpl.total_per_item()
        );
        println!(
            "  Working set for 50 items: {:.1}KB template re-reads",
            50.0 * tmpl.per_item_bytes() as f64 / 1024.0
        );
        println!(
            "  Template unique bytes: {:.1}KB (fits L1: {})",
            tmpl.per_item_bytes() as f64 / 1024.0,
            if tmpl.per_item_bytes() < 128 * 1024 {
                "YES"
            } else {
                "NO"
            }
        );
    }

    // Correctness check
    println!("\n=== Correctness verification ===");
    let tmpl = TemplateSet::new(256);
    let items: Vec<Item> = (0..20).map(Item::new).collect();
    let names: Vec<[u8; 32]> = items.iter().map(|i| i.name).collect();
    let prices: Vec<[u8; 16]> = items.iter().map(|i| i.price).collect();
    let image_urls: Vec<[u8; 64]> = items.iter().map(|i| i.image_url).collect();
    let descriptions: Vec<[u8; 128]> = items.iter().map(|i| i.description).collect();
    let ratings: Vec<[u8; 8]> = items.iter().map(|i| i.rating).collect();
    let review_counts: Vec<[u8; 8]> = items.iter().map(|i| i.review_count).collect();

    let mut std_buf = Vec::new();
    render_standard(&items, &tmpl, &mut std_buf);

    let mut tiled_buf = Vec::new();
    render_tiled_precomputed(&items, &tmpl, &mut tiled_buf, 8);
    assert_eq!(std_buf, tiled_buf, "precomputed tiled output mismatch");

    let mut mb_out = Vec::new();
    let mut mb_bufs = Vec::new();
    render_tiled_multibuf(&items, &tmpl, &mut mb_out, &mut mb_bufs, 8);
    assert_eq!(std_buf, mb_out, "multibuf tiled output mismatch");

    let mut soa_buf = Vec::new();
    render_tiled_soa(
        &names,
        &prices,
        &image_urls,
        &descriptions,
        &ratings,
        &review_counts,
        &tmpl,
        &mut soa_buf,
        8,
    );
    assert_eq!(std_buf, soa_buf, "SoA tiled output mismatch");

    println!("All approaches produce identical output. ✓");
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
