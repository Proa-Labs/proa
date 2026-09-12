// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::shared::write_buf::{WriteBuf, WriteCursor, WriteError};
use proa_core::{
    RenderBuffer, RenderMode, StreamBoundaryRecorder, StreamChunkFlusher, StreamChunkView,
    WebContext,
};
use proa_demo::streaming_airbnb_home as streaming_demo;
use proa_showcase::{airbnb, nike};
use proa_stream::{driver::out_of_order::drain_pending_boundaries_with_artifacts, StreamContext};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

const WARMUP: usize = 1_000;
const ITERATIONS: usize = 10_000;

// ---------------------------------------------------------------------------
// Counting WriteBuf wrapper
// ---------------------------------------------------------------------------
struct CountingBuf {
    inner: Vec<u8>,
    extend_calls: usize,
    bytes_written: usize,
    size_histogram: [usize; 6],
}

impl CountingBuf {
    fn new(cap: usize) -> Self {
        Self {
            inner: Vec::with_capacity(cap),
            extend_calls: 0,
            bytes_written: 0,
            size_histogram: [0; 6],
        }
    }
    fn bucket(len: usize) -> usize {
        match len {
            0..=15 => 0,
            16..=63 => 1,
            64..=255 => 2,
            256..=1023 => 3,
            1024..=4095 => 4,
            _ => 5,
        }
    }
}

impl WriteBuf for CountingBuf {
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
    fn len(&self) -> usize {
        self.inner.len()
    }
    fn reserve(&mut self, n: usize) -> Result<(), WriteError> {
        self.inner.reserve(n);
        Ok(())
    }
    #[inline(always)]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.extend_calls += 1;
        self.bytes_written += bytes.len();
        self.size_histogram[Self::bucket(bytes.len())] += 1;
        self.inner.extend_from_slice(bytes);
        Ok(())
    }
    #[inline(always)]
    fn push(&mut self, byte: u8) -> Result<(), WriteError> {
        self.extend_calls += 1;
        self.bytes_written += 1;
        self.size_histogram[0] += 1;
        self.inner.push(byte);
        Ok(())
    }
    fn as_slice(&self) -> &[u8] {
        &self.inner
    }
    fn to_vec(&self) -> Vec<u8> {
        self.inner.clone()
    }
    fn clear(&mut self) {
        self.inner.clear();
    }
}

fn print_stats(name: &str, timings: &mut [f64], size: usize) {
    timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = timings.iter().sum::<f64>() / timings.len() as f64;
    let p50 = timings[timings.len() / 2];
    let p99 = timings[(timings.len() as f64 * 0.99) as usize];
    let min = timings[0];
    let throughput = (size as f64 / avg) / 1000.0;
    println!("  {name}");
    println!("  {:─<58}", "");
    println!("  avg: {avg:8.2} µs   p50: {p50:8.2} µs   p99: {p99:8.2} µs   min: {min:8.2} µs");
    println!("  {throughput:.1} GB/s   {:.1} KB", size as f64 / 1024.0);
    println!();
}

fn prime_fresh_owned_allocator(capacity: usize) {
    // Model a warm, long-lived server process explicitly. Without an
    // output-sized allocation before this phase, glibc can use a different
    // large-allocation path even after the render warmups have completed.
    let seed = vec![0; capacity];
    std::hint::black_box(seed.as_slice());
}

struct DirectCountingFlusher {
    flushed_len: Cell<usize>,
}

impl DirectCountingFlusher {
    fn new() -> Self {
        Self {
            flushed_len: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.flushed_len.set(0);
    }

    fn flushed_len(&self) -> usize {
        self.flushed_len.get()
    }
}

impl StreamChunkFlusher for DirectCountingFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.flushed_len
            .set(self.flushed_len.get().saturating_add(chunk.len()));
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        self.flushed_len
            .set(self.flushed_len.get().saturating_add(chunk.total_len()));
        Ok(())
    }
}

macro_rules! bench_page {
    ($name:expr, $mod:path, $data:expr, $prealloc:expr) => {{
        use $mod as page;
        let data = $data;

        // --- Phase 1: count extends ---
        {
            let mut buf = CountingBuf::new($prealloc);
            page::render_into(&mut WebContext::with_buffer(&mut buf), &data);
            println!("  {}:", $name);
            println!("    extend() calls: {}", buf.extend_calls);
            println!(
                "    bytes written:  {} ({:.1} KB)",
                buf.bytes_written,
                buf.bytes_written as f64 / 1024.0
            );
            println!(
                "    avg bytes/call: {:.0}",
                buf.bytes_written as f64 / buf.extend_calls as f64
            );
            let labels = [
                "0-15 B",
                "16-63 B",
                "64-255 B",
                "256-1023 B",
                "1K-4K B",
                "4K+ B",
            ];
            for (i, c) in buf.size_histogram.iter().enumerate() {
                if *c > 0 {
                    println!("      {:>10}: {:>5} calls", labels[i], c);
                }
            }
            println!();
        }

        // --- Phase 2: Vec allocating ---
        {
            prime_fresh_owned_allocator($prealloc);
            for _ in 0..WARMUP {
                std::hint::black_box(page::render());
            }
            let mut t = Vec::with_capacity(ITERATIONS);
            let mut sz = 0;
            for _ in 0..ITERATIONS {
                let s = Instant::now();
                let h = std::hint::black_box(page::render());
                t.push(s.elapsed().as_nanos() as f64 / 1000.0);
                sz = h.len();
            }
            print_stats(&format!("{} (Vec, fresh alloc)", $name), &mut t, sz);
        }

        // --- Phase 3: Vec pre-allocated ---
        {
            let mut buf: Vec<u8> = Vec::with_capacity($prealloc);
            for _ in 0..WARMUP {
                buf.clear();
                page::render_into(&mut WebContext::with_buffer(&mut buf), &data);
                std::hint::black_box(&buf);
            }
            let mut t = Vec::with_capacity(ITERATIONS);
            for _ in 0..ITERATIONS {
                buf.clear();
                let mut cx = WebContext::with_buffer(&mut buf);
                let s = Instant::now();
                page::render_into(&mut cx, &data);
                t.push(s.elapsed().as_nanos() as f64 / 1000.0);
                std::hint::black_box(&buf);
            }
            let sz = buf.len();
            print_stats(
                &format!("{} (Vec, pre-alloc {}K)", $name, $prealloc / 1024),
                &mut t,
                sz,
            );
        }

        // --- Phase 4: WriteCursor (fixed buffer) ---
        {
            let mut backing = vec![0u8; $prealloc];
            for _ in 0..WARMUP {
                let mut c = WriteCursor::new(&mut backing);
                page::render_into(&mut WebContext::with_buffer(&mut c), &data);
                std::hint::black_box(c.len());
            }
            let mut t = Vec::with_capacity(ITERATIONS);
            let mut sz = 0;
            for _ in 0..ITERATIONS {
                let mut c = WriteCursor::new(&mut backing);
                let mut cx = WebContext::with_buffer(&mut c);
                let s = Instant::now();
                page::render_into(&mut cx, &data);
                t.push(s.elapsed().as_nanos() as f64 / 1000.0);
                sz = c.len();
                std::hint::black_box(&backing[..sz]);
            }
            print_stats(
                &format!("{} (WriteCursor {}K)", $name, $prealloc / 1024),
                &mut t,
                sz,
            );
        }
    }};
}

fn bench_streaming_airbnb_home_variants() {
    let data = streaming_demo::DemoHomeData::default();
    let sync_output = streaming_demo::render_sync();
    let output_size = sync_output.len();

    println!("{}", "-".repeat(68));
    println!("  Airbnb Home Streaming Demo — Same Page Shape");
    println!("  Output size: {:.1} KB", output_size as f64 / 1024.0);
    println!();

    {
        let mut buf = RenderBuffer::new();
        for _ in 0..WARMUP {
            buf.clear();
            let mut cx = WebContext::with_buffer(&mut buf);
            streaming_demo::render_sync_into(&mut cx, data).expect("sync render should succeed");
            std::hint::black_box(buf.len());
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            buf.clear();
            let mut cx = WebContext::with_buffer(&mut buf);
            let s = Instant::now();
            streaming_demo::render_sync_into(&mut cx, data).expect("sync render should succeed");
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(buf.len());
        }
        print_stats("Sync (ChainedBumpWriter)", &mut t, buf.len());
    }

    {
        let mut buf = Vec::with_capacity(output_size);
        for _ in 0..WARMUP {
            buf.clear();
            let mut cx = WebContext::with_buffer(&mut buf);
            streaming_demo::render_sync_into(&mut cx, data).expect("sync render should succeed");
            std::hint::black_box(&buf);
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            buf.clear();
            let mut cx = WebContext::with_buffer(&mut buf);
            let s = Instant::now();
            streaming_demo::render_sync_into(&mut cx, data).expect("sync render should succeed");
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(&buf);
        }
        print_stats("Sync (Vec, pre-alloc)", &mut t, buf.len());
    }

    {
        let mut out = Vec::with_capacity(output_size);
        for _ in 0..WARMUP {
            out.clear();
            futures::executor::block_on(streaming_demo::render_buffered_async_into(&mut out, data))
                .expect("buffered async render should succeed");
            std::hint::black_box(&out);
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            out.clear();
            let s = Instant::now();
            futures::executor::block_on(streaming_demo::render_buffered_async_into(&mut out, data))
                .expect("buffered async render should succeed");
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(&out);
        }
        print_stats("Buffered async (Vec, immediate futures)", &mut t, out.len());
    }

    {
        let flusher = Rc::new(DirectCountingFlusher::new());
        let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
        let mut tail = Vec::with_capacity(output_size);
        for _ in 0..WARMUP {
            tail.clear();
            flusher.reset();
            futures::executor::block_on(streaming_demo::render_chunked_in_order_into(
                &mut tail,
                data,
                flusher_dyn.clone(),
            ))
            .expect("chunked in-order render should succeed");
            std::hint::black_box(tail.len() + flusher.flushed_len());
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            tail.clear();
            flusher.reset();
            let s = Instant::now();
            futures::executor::block_on(streaming_demo::render_chunked_in_order_into(
                &mut tail,
                data,
                flusher_dyn.clone(),
            ))
            .expect("chunked in-order render should succeed");
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(tail.len() + flusher.flushed_len());
        }
        print_stats(
            "Chunked in-order (direct sink, immediate futures)",
            &mut t,
            tail.len() + flusher.flushed_len(),
        );
    }

    {
        let mut shell = Vec::with_capacity(output_size);
        for _ in 0..WARMUP {
            shell.clear();
            let stream = Rc::new(StreamContext::new(RenderMode::ChunkedOutOfOrder));
            let recorder: Rc<dyn StreamBoundaryRecorder> = stream.clone();
            futures::executor::block_on(streaming_demo::render_chunked_out_of_order_into(
                &mut shell, data, recorder,
            ))
            .expect("chunked out-of-order shell render should succeed");
            std::hint::black_box(&shell);
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            shell.clear();
            let stream = Rc::new(StreamContext::new(RenderMode::ChunkedOutOfOrder));
            let recorder: Rc<dyn StreamBoundaryRecorder> = stream.clone();
            let s = Instant::now();
            futures::executor::block_on(streaming_demo::render_chunked_out_of_order_into(
                &mut shell, data, recorder,
            ))
            .expect("chunked out-of-order shell render should succeed");
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(&shell);
        }
        print_stats("Chunked out-of-order (shell only)", &mut t, shell.len());
    }

    {
        let mut shell = Vec::with_capacity(output_size);
        let total_len = Cell::new(0usize);
        for _ in 0..WARMUP {
            shell.clear();
            total_len.set(0);
            let stream = Rc::new(StreamContext::new(RenderMode::ChunkedOutOfOrder));
            let recorder: Rc<dyn StreamBoundaryRecorder> = stream.clone();
            futures::executor::block_on(async {
                streaming_demo::render_chunked_out_of_order_into(&mut shell, data, recorder)
                    .await
                    .expect("chunked out-of-order render should succeed");
                let shell_len = shell.len();
                drain_pending_boundaries_with_artifacts(&stream, |chunk| {
                    total_len.set(total_len.get().saturating_add(chunk.bytes.len()));
                    async { true }
                })
                .await;
                total_len.set(total_len.get().saturating_add(shell_len));
            });
            std::hint::black_box(total_len.get());
        }
        let mut t = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            shell.clear();
            total_len.set(0);
            let stream = Rc::new(StreamContext::new(RenderMode::ChunkedOutOfOrder));
            let recorder: Rc<dyn StreamBoundaryRecorder> = stream.clone();
            let s = Instant::now();
            futures::executor::block_on(async {
                streaming_demo::render_chunked_out_of_order_into(&mut shell, data, recorder)
                    .await
                    .expect("chunked out-of-order render should succeed");
                let shell_len = shell.len();
                drain_pending_boundaries_with_artifacts(&stream, |chunk| {
                    total_len.set(total_len.get().saturating_add(chunk.bytes.len()));
                    async { true }
                })
                .await;
                total_len.set(total_len.get().saturating_add(shell_len));
            });
            t.push(s.elapsed().as_nanos() as f64 / 1000.0);
            std::hint::black_box(total_len.get());
        }
        print_stats(
            "Chunked out-of-order (shell + deferred chunks)",
            &mut t,
            total_len.get(),
        );
    }
}

fn main() {
    println!("{}", "=".repeat(68));
    println!("  Proa SSR Benchmark — Native Profiling");
    println!("  Warmup: {WARMUP} | Bench: {ITERATIONS}");
    println!("{}", "=".repeat(68));
    println!();

    bench_page!(
        "Home",
        airbnb::home,
        airbnb::home::HomeData::default_la(),
        256 * 1024
    );
    println!("{}", "-".repeat(68));
    bench_page!(
        "Search",
        airbnb::search,
        airbnb::search::SearchData::default_malibu(),
        128 * 1024
    );
    println!("{}", "-".repeat(68));
    bench_page!(
        "Nike PDP",
        nike::product,
        nike::product::default_data(),
        128 * 1024
    );
    bench_streaming_airbnb_home_variants();

    println!("{}", "=".repeat(68));
    println!("  Done.");
    println!("{}", "=".repeat(68));
}
