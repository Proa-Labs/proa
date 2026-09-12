// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark: Vec<u8> extend vs scatter-gather (IoSlice) vs FastWriter
//!
//! Simulates the real SSR rendering workload:
//! - ~9,352 write calls for ~180KB output
//! - 82% of writes are ≤15 bytes (static HTML fragments)
//! - Dynamic attributes follow a 3-call pattern: prefix + value + suffix
//! - Mix of static (&'static [u8]) and dynamic (short escaped strings)

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use proa_core::shared::write_buf::{Part, ZeroCopyBuf};
use proa_core::WriteBuf;
use std::io::{IoSlice, Write};

// ---------------------------------------------------------------------------
// Workload generation — build a realistic sequence of writes
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum WriteOp {
    /// Static byte slice (lives in binary, zero-copy for scatter)
    Static(&'static [u8]),
    /// Dynamic content (simulates escaped attribute values, text nodes)
    Dynamic(Vec<u8>),
}

/// Build a workload matching the observed SSR profile:
/// ~9,352 ops, ~180KB total, 82% static ≤15 bytes
fn build_workload() -> (Vec<WriteOp>, usize) {
    let mut ops = Vec::with_capacity(10_000);
    let mut total = 0usize;

    // Static fragments (tags, attribute names, quotes, closing tags)
    const STATIC_FRAGS: &[&[u8]] = &[
        b"<div ",
        b"class=\"",
        b"\"",
        b">",
        b"</div>",
        b"<span ",
        b"</span>",
        b"<img ",
        b"src=\"",
        b" alt=\"",
        b"\" ",
        b"/>",
        b"<a href=\"",
        b"</a>",
        b"<li>",
        b"</li>",
        b"<ul>",
        b"</ul>",
        b"\n",
        b"  ",
        b"    ",
        b"<p>",
        b"</p>",
        b" data-slot=\"",
        b" data-state=\"",
        b"expanded",
        b"collapsed",
        b"<button ",
        b"type=\"button\"",
        b"</button>",
        b"<header ",
        b"</header>",
        b"<nav ",
        b"</nav>",
        b" role=\"",
        b"menu",
        b"menuitem",
    ];

    // Dynamic values (class lists, URLs, text content)
    const DYN_VALUES: &[&str] = &[
        "flex items-center gap-2 px-4",
        "text-sm font-medium text-foreground",
        "peer/menu-button group/menu-button flex w-full items-center gap-2 overflow-hidden rounded-md p-2",
        "/images/hero.webp",
        "A beautiful sunset over mountains",
        "sidebar-menu-item",
        "Build Your Application",
        "Data Fetching",
        "relative flex w-full min-w-0 flex-col p-2",
        "truncate font-semibold",
        "m@example.com",
        "Acme Inc",
        "#",
        "open",
        "closed",
        "true",
        "false",
    ];

    // Simulate 49 "card" renders (like the airbnb listing bench)
    // Each card has ~190 write ops = ~9,310 total
    for card_idx in 0..49 {
        // Card wrapper: <div class="...">
        ops.push(WriteOp::Static(b"<div "));
        ops.push(WriteOp::Static(b"class=\""));
        ops.push(WriteOp::Dynamic(
            DYN_VALUES[card_idx % DYN_VALUES.len()].as_bytes().to_vec(),
        ));
        ops.push(WriteOp::Static(b"\""));
        ops.push(WriteOp::Static(b">"));
        total += 5 + 7 + DYN_VALUES[card_idx % DYN_VALUES.len()].len() + 1 + 1;

        // Image: <img src="..." alt="..." class="..." loading="lazy" />
        ops.push(WriteOp::Static(b"<img "));
        ops.push(WriteOp::Static(b"src=\""));
        ops.push(WriteOp::Dynamic(b"/images/hero.webp".to_vec()));
        ops.push(WriteOp::Static(b"\" "));
        ops.push(WriteOp::Static(b" alt=\""));
        ops.push(WriteOp::Dynamic(b"A beautiful sunset".to_vec()));
        ops.push(WriteOp::Static(b"\" "));
        ops.push(WriteOp::Static(b"class=\""));
        ops.push(WriteOp::Dynamic(
            b"aspect-video rounded-xl object-cover".to_vec(),
        ));
        ops.push(WriteOp::Static(b"\" "));
        ops.push(WriteOp::Static(b"loading=\"lazy\""));
        ops.push(WriteOp::Static(b"/>"));
        total += 5 + 5 + 17 + 2 + 6 + 18 + 2 + 7 + 36 + 2 + 14 + 2;

        // Header with title
        ops.push(WriteOp::Static(b"<header "));
        ops.push(WriteOp::Static(b"class=\""));
        ops.push(WriteOp::Dynamic(b"flex items-center gap-2 px-4".to_vec()));
        ops.push(WriteOp::Static(b"\""));
        ops.push(WriteOp::Static(b">"));
        total += 8 + 7 + 28 + 1 + 1;

        // Inner elements (simulate ~30 more static/dynamic pairs per card)
        for inner in 0..30 {
            let frag = STATIC_FRAGS[inner % STATIC_FRAGS.len()];
            ops.push(WriteOp::Static(frag));
            total += frag.len();

            if inner % 3 == 0 {
                // Dynamic attribute value
                let val = DYN_VALUES[(card_idx * 7 + inner) % DYN_VALUES.len()];
                ops.push(WriteOp::Dynamic(val.as_bytes().to_vec()));
                total += val.len();
            }
        }

        // SVG icons (heart, star) — medium static chunks
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"><path d=\"m9 18 6-6-6-6\"/></svg>";
        ops.push(WriteOp::Static(svg));
        total += svg.len();

        ops.push(WriteOp::Static(svg));
        total += svg.len();

        // Close card
        ops.push(WriteOp::Static(b"</header>"));
        ops.push(WriteOp::Static(b"</div>"));
        total += 9 + 6;
    }

    (ops, total)
}

// ---------------------------------------------------------------------------
// Approach 1: Current — Vec<u8>::extend_from_slice per write
// ---------------------------------------------------------------------------

fn bench_vec_extend(ops: &[WriteOp]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256); // small initial, forces growth
    for op in ops {
        match op {
            WriteOp::Static(s) => buf.extend_from_slice(s),
            WriteOp::Dynamic(d) => buf.extend_from_slice(d),
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// Approach 1b: Vec<u8> with realistic pre-allocation
// ---------------------------------------------------------------------------

fn bench_vec_extend_prealloc(ops: &[WriteOp], total_size: usize) -> Vec<u8> {
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
// Approach 2: Scatter-gather — collect IoSlices, single write_vectored
// ---------------------------------------------------------------------------

fn bench_scatter_gather(ops: &[WriteOp]) -> Vec<u8> {
    let slices: Vec<IoSlice<'_>> = ops
        .iter()
        .map(|op| match op {
            WriteOp::Static(s) => IoSlice::new(s),
            WriteOp::Dynamic(d) => IoSlice::new(d),
        })
        .collect();

    // Simulate write_vectored to a buffer (like writing to socket)
    let total: usize = slices.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(total);
    // In real use this would be writev() to socket; here we simulate
    let written = out.write_vectored(&slices).unwrap();
    assert_eq!(written, total);
    out
}

// ---------------------------------------------------------------------------
// Approach 2b: Scatter-gather — collect only, no final copy
//              (measures just the collection cost; the writev is the OS's job)
// ---------------------------------------------------------------------------

fn bench_scatter_collect_only(ops: &[WriteOp]) -> usize {
    let slices: Vec<IoSlice<'_>> = ops
        .iter()
        .map(|op| match op {
            WriteOp::Static(s) => IoSlice::new(s),
            WriteOp::Dynamic(d) => IoSlice::new(d),
        })
        .collect();

    // Return total bytes to prevent optimization
    slices.iter().map(|s| s.len()).sum()
}

// ---------------------------------------------------------------------------
// Approach 2c: Userspace scatter-gather — collect slices, then single-pass
//              memcpy into one pre-sized Vec. No kernel involvement.
// ---------------------------------------------------------------------------

fn bench_userspace_scatter(ops: &[WriteOp]) -> Vec<u8> {
    // Phase 1: collect slice pointers (same as scatter_collect_only)
    let slices: Vec<&[u8]> = ops
        .iter()
        .map(|op| match op {
            WriteOp::Static(s) => *s,
            WriteOp::Dynamic(d) => d.as_slice(),
        })
        .collect();

    // Phase 2: single allocation, tight copy loop
    let total: usize = slices.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(total);
    for s in &slices {
        out.extend_from_slice(s);
    }
    out
}

// Same but with raw pointer copy to eliminate per-call len update
fn bench_userspace_scatter_unsafe(ops: &[WriteOp]) -> Vec<u8> {
    let slices: Vec<&[u8]> = ops
        .iter()
        .map(|op| match op {
            WriteOp::Static(s) => *s,
            WriteOp::Dynamic(d) => d.as_slice(),
        })
        .collect();

    let total: usize = slices.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(total);
    let mut ptr = out.as_mut_ptr();
    for s in &slices {
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), ptr, s.len());
            ptr = ptr.add(s.len());
        }
    }
    unsafe {
        out.set_len(total);
    }
    out
}

// ---------------------------------------------------------------------------
// Approach 5: Vec<Bytes> — the axum/hyper streaming body path
//             Static slices → Bytes::from_static (zero-copy pointer wrap)
//             Dynamic slices → Bytes::copy_from_slice (owned copy)
//             This is what render() would return for Body::from_stream()
// ---------------------------------------------------------------------------

fn bench_bytes_collect(ops: &[WriteOp]) -> Vec<Bytes> {
    let mut chunks = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            WriteOp::Static(s) => chunks.push(Bytes::from_static(s)),
            WriteOp::Dynamic(d) => chunks.push(Bytes::copy_from_slice(d)),
        }
    }
    chunks
}

// Same but coalesce consecutive small statics into one Bytes
// (reduces chunk count for hyper's writev without losing zero-copy on large statics)
fn bench_bytes_coalesced(ops: &[WriteOp]) -> Vec<Bytes> {
    let mut chunks = Vec::with_capacity(ops.len() / 2);
    let mut run_start: Option<*const u8> = None;
    let mut run_len: usize = 0;

    for op in ops {
        match op {
            WriteOp::Static(s) => {
                if let Some(start) = run_start {
                    // Check if this slice is contiguous with the current run
                    let expected_next = unsafe { start.add(run_len) };
                    if s.as_ptr() == expected_next {
                        run_len += s.len();
                        continue;
                    }
                    // Not contiguous — flush the run
                    chunks.push(Bytes::from_static(unsafe {
                        std::slice::from_raw_parts(start, run_len)
                    }));
                }
                run_start = Some(s.as_ptr());
                run_len = s.len();
            }
            WriteOp::Dynamic(d) => {
                // Flush any pending static run
                if let Some(start) = run_start.take() {
                    chunks.push(Bytes::from_static(unsafe {
                        std::slice::from_raw_parts(start, run_len)
                    }));
                    run_len = 0;
                }
                chunks.push(Bytes::copy_from_slice(d));
            }
        }
    }
    // Flush final run
    if let Some(start) = run_start {
        chunks.push(Bytes::from_static(unsafe {
            std::slice::from_raw_parts(start, run_len)
        }));
    }
    chunks
}

// Measure total bytes to verify correctness
fn bytes_total_len(chunks: &[Bytes]) -> usize {
    chunks.iter().map(|b| b.len()).sum()
}

// ---------------------------------------------------------------------------
// Approach 3: FastWriter — direct writes into Vec spare capacity
// ---------------------------------------------------------------------------

struct FastWriter {
    buf: Vec<u8>,
    cursor: usize,
}

impl FastWriter {
    fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
            cursor: 0,
        }
    }

    #[inline(always)]
    fn write(&mut self, data: &[u8]) {
        let spare = self.buf.capacity() - self.buf.len() - self.cursor;
        if data.len() > spare {
            unsafe {
                self.buf.set_len(self.buf.len() + self.cursor);
            }
            self.cursor = 0;
            self.buf.reserve(data.len().max(4096));
        }
        unsafe {
            let dst = self.buf.as_mut_ptr().add(self.buf.len() + self.cursor);
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        self.cursor += data.len();
    }

    fn finish(mut self) -> Vec<u8> {
        unsafe {
            self.buf.set_len(self.buf.len() + self.cursor);
        }
        self.buf
    }
}

fn bench_fast_writer(ops: &[WriteOp]) -> Vec<u8> {
    let mut w = FastWriter::with_capacity(256); // small initial
    for op in ops {
        match op {
            WriteOp::Static(s) => w.write(s),
            WriteOp::Dynamic(d) => w.write(d),
        }
    }
    w.finish()
}

fn bench_fast_writer_prealloc(ops: &[WriteOp], total_size: usize) -> Vec<u8> {
    let mut w = FastWriter::with_capacity(total_size);
    for op in ops {
        match op {
            WriteOp::Static(s) => w.write(s),
            WriteOp::Dynamic(d) => w.write(d),
        }
    }
    w.finish()
}

// ---------------------------------------------------------------------------
// Approach 4: WriteCursor — fixed stack buffer (existing proa_core type)
// ---------------------------------------------------------------------------

fn bench_write_cursor(ops: &[WriteOp], total_size: usize) -> Vec<u8> {
    // Allocate exact-size buffer on heap (simulating stack for large sizes)
    let mut backing = vec![0u8; total_size];
    let mut cursor = proa_core::shared::write_buf::WriteCursor::new(&mut backing);
    for op in ops {
        match op {
            WriteOp::Static(s) => {
                let _ = cursor.extend(s);
            }
            WriteOp::Dynamic(d) => {
                let _ = cursor.extend(d);
            }
        }
    }
    cursor.as_slice().to_vec()
}

// ---------------------------------------------------------------------------
// Approach 6: ScatteredBuf — WriteBuf impl that packs into N-byte chunks
//             This is the tile_http Body::Scattered integration path.
//             Parameterized by CHUNK_SIZE to find the optimal trade-off
//             between chunk count (IoSlice overhead) and allocation cost.
// ---------------------------------------------------------------------------

struct ScatteredBuf {
    chunk_size: usize,
    chunks: Vec<Vec<u8>>,
    current: Vec<u8>,
    total_len: usize,
}

impl ScatteredBuf {
    fn new(chunk_size: usize, capacity_hint: usize) -> Self {
        let est_chunks = capacity_hint / chunk_size + 1;
        Self {
            chunk_size,
            chunks: Vec::with_capacity(est_chunks),
            current: Vec::with_capacity(chunk_size),
            total_len: 0,
        }
    }

    fn into_chunks(mut self) -> (Vec<Vec<u8>>, usize) {
        if !self.current.is_empty() {
            self.chunks.push(self.current);
        }
        (self.chunks, self.total_len)
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        self.total_len += bytes.len();

        if self.current.len() + bytes.len() <= self.chunk_size {
            self.current.extend_from_slice(bytes);
            return;
        }

        let mut remaining = bytes;
        let space = self.chunk_size - self.current.len();
        if space > 0 && !self.current.is_empty() {
            self.current.extend_from_slice(&remaining[..space]);
            remaining = &remaining[space..];
        }
        if !self.current.is_empty() {
            self.chunks.push(std::mem::replace(
                &mut self.current,
                Vec::with_capacity(self.chunk_size),
            ));
        }

        while remaining.len() > self.chunk_size {
            let mut chunk = Vec::with_capacity(self.chunk_size);
            chunk.extend_from_slice(&remaining[..self.chunk_size]);
            self.chunks.push(chunk);
            remaining = &remaining[self.chunk_size..];
        }

        if !remaining.is_empty() {
            self.current.extend_from_slice(remaining);
        }
    }
}

impl WriteBuf for ScatteredBuf {
    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.total_len
    }

    fn reserve(
        &mut self,
        _additional: usize,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        Ok(())
    }

    #[inline(always)]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), proa_core::shared::write_buf::WriteError> {
        self.write(bytes);
        Ok(())
    }

    fn as_slice(&self) -> &[u8] {
        &[]
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for chunk in &self.chunks {
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&self.current);
        out
    }

    fn clear(&mut self) {
        self.chunks.clear();
        self.current.clear();
        self.total_len = 0;
    }
}

fn bench_scattered_buf(ops: &[WriteOp], total_size: usize) -> Vec<u8> {
    let mut buf = ScatteredBuf::new(1024, total_size);
    for op in ops {
        match op {
            WriteOp::Static(s) => buf.write(s),
            WriteOp::Dynamic(d) => buf.write(d),
        }
    }
    buf.to_vec()
}

fn bench_scattered_collect(ops: &[WriteOp], total_size: usize, chunk_size: usize) -> usize {
    let mut buf = ScatteredBuf::new(chunk_size, total_size);
    for op in ops {
        match op {
            WriteOp::Static(s) => buf.write(s),
            WriteOp::Dynamic(d) => buf.write(d),
        }
    }
    let (chunks, total) = buf.into_chunks();
    chunks.len() + total
}

// ---------------------------------------------------------------------------
// Approach 7: ZeroCopyBuf — WriteBuf with extend_static() override
//             Static slices → Part::Static (zero-copy pointer)
//             Dynamic slices → copied into shared owned buffer
//             This is the target design for tile_http scatter-gather.
//             (Implementation now lives in proa_core::shared::write_buf)
// ---------------------------------------------------------------------------

fn bench_zero_copy_buf(ops: &[WriteOp]) -> usize {
    let mut buf = ZeroCopyBuf::with_capacity(ops.len(), 64 * 1024);
    for op in ops {
        match op {
            WriteOp::Static(s) => {
                let _ = buf.extend_static(s);
            }
            WriteOp::Dynamic(d) => {
                let _ = buf.extend(d);
            }
        }
    }
    buf.part_count() + buf.len()
}

// ---------------------------------------------------------------------------
// Criterion benchmarks
// ---------------------------------------------------------------------------

fn criterion_benchmark(c: &mut Criterion) {
    let (ops, total_size) = build_workload();

    println!("\n=== Workload stats ===");
    println!("Total ops: {}", ops.len());
    println!(
        "Total bytes: {} ({:.1} KB)",
        total_size,
        total_size as f64 / 1024.0
    );

    let static_count = ops
        .iter()
        .filter(|op| matches!(op, WriteOp::Static(_)))
        .count();
    let dynamic_count = ops.len() - static_count;
    println!(
        "Static ops: {} ({:.1}%)",
        static_count,
        100.0 * static_count as f64 / ops.len() as f64
    );
    println!(
        "Dynamic ops: {} ({:.1}%)",
        dynamic_count,
        100.0 * dynamic_count as f64 / ops.len() as f64
    );

    let small_count = ops
        .iter()
        .filter(|op| {
            let len = match op {
                WriteOp::Static(s) => s.len(),
                WriteOp::Dynamic(d) => d.len(),
            };
            len <= 15
        })
        .count();
    println!(
        "Writes ≤15 bytes: {} ({:.1}%)",
        small_count,
        100.0 * small_count as f64 / ops.len() as f64
    );
    println!("========================\n");

    let mut group = c.benchmark_group("ssr_write_strategies");
    group.throughput(Throughput::Bytes(total_size as u64));

    // 1. Current approach: Vec extend (small initial alloc)
    group.bench_function("vec_extend", |b| {
        b.iter(|| black_box(bench_vec_extend(black_box(&ops))))
    });

    // 1b. Vec extend with pre-allocation
    group.bench_function("vec_extend_prealloc", |b| {
        b.iter(|| black_box(bench_vec_extend_prealloc(black_box(&ops), total_size)))
    });

    // 2. Scatter-gather (collect + write_vectored to Vec)
    group.bench_function("scatter_gather_full", |b| {
        b.iter(|| black_box(bench_scatter_gather(black_box(&ops))))
    });

    // 2b. Scatter: collect IoSlices only (no final copy)
    group.bench_function("scatter_collect_only", |b| {
        b.iter(|| black_box(bench_scatter_collect_only(black_box(&ops))))
    });

    // 2c. Userspace scatter-gather (collect + single-pass copy)
    group.bench_function("userspace_scatter", |b| {
        b.iter(|| black_box(bench_userspace_scatter(black_box(&ops))))
    });

    // 2d. Userspace scatter-gather (unsafe ptr copy, no per-call len update)
    group.bench_function("userspace_scatter_unsafe", |b| {
        b.iter(|| black_box(bench_userspace_scatter_unsafe(black_box(&ops))))
    });

    // 3. FastWriter (small initial alloc)
    group.bench_function("fast_writer", |b| {
        b.iter(|| black_box(bench_fast_writer(black_box(&ops))))
    });

    // 3b. FastWriter with pre-allocation
    group.bench_function("fast_writer_prealloc", |b| {
        b.iter(|| black_box(bench_fast_writer_prealloc(black_box(&ops), total_size)))
    });

    // 4. WriteCursor (fixed buffer, no growth)
    group.bench_function("write_cursor", |b| {
        b.iter(|| black_box(bench_write_cursor(black_box(&ops), total_size)))
    });

    // 5. Vec<Bytes> collect (the axum Body::from_stream path)
    group.bench_function("bytes_collect", |b| {
        b.iter(|| black_box(bench_bytes_collect(black_box(&ops))))
    });

    // 5b. Vec<Bytes> with consecutive static coalescing
    group.bench_function("bytes_coalesced", |b| {
        b.iter(|| black_box(bench_bytes_coalesced(black_box(&ops))))
    });

    // 6. ScatteredBuf: sweep chunk sizes to find the sweet spot
    for chunk_size in [512, 1024, 2048, 4096, 8192, 16384, 32768] {
        let label = format!("scattered_{}b", chunk_size);
        group.bench_function(&label, |b| {
            b.iter(|| {
                black_box(bench_scattered_collect(
                    black_box(&ops),
                    total_size,
                    chunk_size,
                ))
            })
        });
    }

    // 7. ZeroCopyBuf: extend_static for statics, shared owned buf for dynamics
    group.bench_function("zero_copy_buf", |b| {
        b.iter(|| black_box(bench_zero_copy_buf(black_box(&ops))))
    });

    group.finish();

    // Verify correctness — all approaches produce identical output
    let expected = bench_vec_extend(&ops);
    assert_eq!(expected, bench_vec_extend_prealloc(&ops, total_size));
    assert_eq!(expected, bench_scatter_gather(&ops));
    assert_eq!(expected, bench_fast_writer(&ops));
    assert_eq!(expected, bench_fast_writer_prealloc(&ops, total_size));
    assert_eq!(expected, bench_write_cursor(&ops, total_size));
    assert_eq!(expected, bench_userspace_scatter(&ops));
    assert_eq!(expected, bench_userspace_scatter_unsafe(&ops));
    assert_eq!(
        expected.len(),
        bench_scatter_collect_only(&ops),
        "scatter_collect_only byte count"
    );
    // Verify Bytes variants produce correct total size
    assert_eq!(
        expected.len(),
        bytes_total_len(&bench_bytes_collect(&ops)),
        "bytes_collect size"
    );
    assert_eq!(
        expected.len(),
        bytes_total_len(&bench_bytes_coalesced(&ops)),
        "bytes_coalesced size"
    );
    assert_eq!(expected, bench_scattered_buf(&ops, total_size));
    // Verify ZeroCopyBuf produces correct output
    {
        let mut zcb = ZeroCopyBuf::with_capacity(ops.len(), 64 * 1024);
        for op in &ops {
            match op {
                WriteOp::Static(s) => {
                    let _ = zcb.extend_static(s);
                }
                WriteOp::Dynamic(d) => {
                    let _ = WriteBuf::extend(&mut zcb, d);
                }
            }
        }
        assert_eq!(expected, zcb.to_vec(), "zero_copy_buf output");
        let static_parts = zcb
            .parts
            .iter()
            .filter(|p| matches!(p, Part::Static(_)))
            .count();
        let owned_parts = zcb
            .parts
            .iter()
            .filter(|p| matches!(p, Part::Owned { .. }))
            .count();
        println!(
            "ZeroCopyBuf: {} parts ({} static, {} owned), {} bytes owned data",
            zcb.part_count(),
            static_parts,
            owned_parts,
            zcb.owned.len()
        );
    }
    println!("All approaches produce identical output.");
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
