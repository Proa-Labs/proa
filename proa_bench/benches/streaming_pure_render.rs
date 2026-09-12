// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use proa_bench::streaming_marketplace::{
    MarketplaceBufferedPage, MarketplaceLoadedSyncPage, MarketplaceStreamPage,
};
use proa_core::{
    render_in_context_async, DataLoader, DynDataLoader, DynLoaderHandle, LoadError, LoadRequest,
    LoadValue, RenderBuffer, RenderMode, StreamChunkFlusher, StreamChunkView, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError, ZeroCopyBuf,
};

#[derive(Debug, Default)]
struct ImmediateLoader;

impl DataLoader for ImmediateLoader {
    async fn load(&self, _request: &LoadRequest) -> Result<LoadValue, LoadError> {
        Ok(LoadValue::bytes(Bytes::from_static(b"null")))
    }
}

#[derive(Default)]
struct CountingFlusher {
    flushed_len: Cell<usize>,
}

impl CountingFlusher {
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

impl StreamChunkFlusher for CountingFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.flushed_len
            .set(self.flushed_len.get().saturating_add(chunk.len()));
        Ok(())
    }
}

struct DirectCountingFlusher {
    flushed_len: Cell<usize>,
}

#[derive(Default)]
struct CollectingFlusher {
    bytes: RefCell<Vec<u8>>,
}

impl StreamChunkFlusher for CollectingFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.bytes.borrow_mut().extend_from_slice(&chunk);
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        chunk.for_each_slice(&mut |slice| {
            self.bytes.borrow_mut().extend_from_slice(slice);
            Ok(())
        })
    }
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

fn render_dyn<R>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    loader: Arc<dyn DynDataLoader>,
    flusher: Option<Rc<dyn StreamChunkFlusher>>,
    root: R,
) -> Result<(), WriteError>
where
    R: WebRender<DynLoaderHandle, RenderBuffer>,
{
    let buffer = std::mem::take(out);
    let (result, buffer) = runtime.block_on(async move {
        let mut cx = WebContext::with_dyn_loader_buffer(buffer, loader);
        if let Some(flusher) = flusher {
            cx = cx
                .with_render_mode(RenderMode::ChunkedInOrder)
                .with_stream_chunk_flusher(flusher);
        }
        let result = render_in_context_async(&mut cx, root).await;
        (result, cx.into_output())
    });
    *out = buffer;
    result
}

fn bench_marketplace_pure_render(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("failed to build tokio runtime for pure render bench");
    let loader: Arc<dyn DynDataLoader> = Arc::new(ImmediateLoader);

    let output_size = {
        let mut out = RenderBuffer::new();
        render_dyn(
            &runtime,
            &mut out,
            loader.clone(),
            None,
            MarketplaceBufferedPage,
        )
        .expect("buffered marketplace render should succeed");
        out.len()
    };

    let expected = {
        let mut cx = WebContext::new();
        MarketplaceLoadedSyncPage
            .render(&mut cx)
            .expect("sync marketplace parity render should succeed");
        let out = cx.into_output();
        out.to_vec()
    };

    let streamed = {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(CollectingFlusher::default());
        let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
        render_dyn(
            &runtime,
            &mut out,
            loader.clone(),
            Some(flusher_dyn),
            MarketplaceStreamPage,
        )
        .expect("production marketplace parity render should succeed");
        let mut bytes = flusher.bytes.borrow().clone();
        for slice in out.slices() {
            bytes.extend_from_slice(slice);
        }
        bytes
    };

    assert_eq!(streamed, expected);

    let mut group = c.benchmark_group("marketplace_pure_render");
    group.throughput(Throughput::Bytes(output_size as u64));

    group.bench_function("sync_chained_complete", |b| {
        let mut buf = RenderBuffer::new();
        b.iter(|| {
            buf.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buf));
            MarketplaceLoadedSyncPage
                .render(&mut cx)
                .expect("sync chained marketplace render should succeed");
            buf = cx.into_output();
            black_box(buf.len())
        });
    });

    group.bench_function("sync_zero_copy_complete", |b| {
        let mut buf = ZeroCopyBuf::with_capacity(4096, output_size);
        b.iter(|| {
            buf.clear();
            {
                let mut cx = WebContext::with_buffer(&mut buf);
                MarketplaceLoadedSyncPage
                    .render(&mut cx)
                    .expect("sync zero-copy marketplace render should succeed");
            }
            black_box(buf.len())
        });
    });

    group.bench_function("buffered_async_complete", |b| {
        let mut out = RenderBuffer::new();

        b.iter(|| {
            out.clear();
            render_dyn(
                &runtime,
                &mut out,
                loader.clone(),
                None,
                MarketplaceBufferedPage,
            )
            .expect("buffered marketplace render should succeed");
            black_box(out.len())
        });
    });

    group.bench_function("chunked_in_order_production_flattening_complete", |b| {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(CountingFlusher::new());
        let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();

        b.iter(|| {
            out.clear();
            flusher.reset();
            render_dyn(
                &runtime,
                &mut out,
                loader.clone(),
                Some(flusher_dyn.clone()),
                MarketplaceStreamPage,
            )
            .expect("in-order marketplace render should succeed");
            let total_len = out.len() + flusher.flushed_len();
            black_box(total_len)
        });
    });

    group.bench_function("chunked_in_order_production_direct_complete", |b| {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(DirectCountingFlusher::new());
        let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();

        b.iter(|| {
            out.clear();
            flusher.reset();
            render_dyn(
                &runtime,
                &mut out,
                loader.clone(),
                Some(flusher_dyn.clone()),
                MarketplaceStreamPage,
            )
            .expect("in-order marketplace render should succeed");
            let total_len = out.len() + flusher.flushed_len();
            black_box(total_len)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_marketplace_pure_render);
criterion_main!(benches);
