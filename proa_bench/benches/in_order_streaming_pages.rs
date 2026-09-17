// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Full-page benchmarks for production in-order `Suspense` rendering.
//!
//! Every page has three rows:
//! - the former borrowed-context synchronous `RenderBuffer` baseline;
//! - direct synchronous rendering with the owned-context `RenderBuffer`;
//! - production in-order `Suspense` using the standard `RenderBuffer`.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion, Throughput,
};
use proa_bench::complex_page::ComplexPage;
use proa_bench::proa_page::SafeTextBenchmarkPage;
use proa_bench::streaming_marketplace::MarketplaceStreamPage;
use proa_core::{
    render_in_context_async, DataLoader, DynDataLoader, DynLoaderHandle, LoadError, LoadRequest,
    LoadValue, NoLoader, RenderBuffer, RenderMode, RenderOutcome, StreamChunkFlusher,
    StreamChunkView, SyncToAsync, WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_stream::Suspense;

const SHELL_OPEN: &[u8] = b"<main data-in-order-streaming-bench>";
const SHELL_CLOSE: &[u8] = b"</main>";

struct ThreadCountingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

#[inline]
fn record_allocation(bytes: usize) {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        let _ = ALLOCATED_BYTES.try_with(|total| total.set(total.get().saturating_add(bytes)));
    }
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy, Debug)]
struct AllocationStats {
    calls: usize,
    bytes: usize,
}

fn start_counting() {
    ALLOCATIONS.with(|count| count.set(0));
    ALLOCATED_BYTES.with(|total| total.set(0));
    TRACKING.with(|tracking| tracking.set(true));
}

fn stop_counting() -> AllocationStats {
    TRACKING.with(|tracking| tracking.set(false));
    AllocationStats {
        calls: ALLOCATIONS.with(Cell::get),
        bytes: ALLOCATED_BYTES.with(Cell::get),
    }
}

trait PageCase: 'static {
    const NAME: &'static str;

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}

struct CaseRender<C>(PhantomData<fn() -> C>);

impl<C> CaseRender<C> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<C: PageCase, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CaseRender<C> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        C::render(cx)
    }
}

struct SyncEnvelope<C>(PhantomData<fn() -> C>);

impl<C> SyncEnvelope<C> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<C: PageCase, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SyncEnvelope<C> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_static(SHELL_OPEN)?;
        C::render(cx)?;
        cx.out.extend_static(SHELL_CLOSE)
    }
}

struct SuspenseEnvelope<C>(PhantomData<fn() -> C>);

impl<C> SuspenseEnvelope<C> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<C, L> WebRender<L, RenderBuffer> for SuspenseEnvelope<C>
where
    C: PageCase,
    L: DataLoader + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(SHELL_OPEN)?;
            proa_core::render_child(
                cx,
                Suspense::new(SyncToAsync(CaseRender::<C>::new()), EmptyFallback),
            )
            .await?;
            cx.out.extend_static(SHELL_CLOSE)
        })
    }
}

#[derive(Clone, Copy)]
struct EmptyFallback;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EmptyFallback {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

struct SafeTextCase;

impl PageCase for SafeTextCase {
    const NAME: &'static str = "proa_bench_safe_text";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        SafeTextBenchmarkPage.render(cx)
    }
}

struct ComplexCase;

impl PageCase for ComplexCase {
    const NAME: &'static str = "proa_bench_complex";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        ComplexPage.render(cx)
    }
}

struct AirbnbHomeCase;

impl PageCase for AirbnbHomeCase {
    const NAME: &'static str = "showcase_airbnb_home";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = proa_showcase::airbnb::home::HomeData::default_la();
        proa_showcase::airbnb::home::render_into(cx, &data);
        Ok(())
    }
}

struct AirbnbListingCase;

impl PageCase for AirbnbListingCase {
    const NAME: &'static str = "showcase_airbnb_listing";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = proa_showcase::airbnb::listing::ListingData::malibu();
        proa_showcase::airbnb::listing::render_into(cx, &data, None)
    }
}

struct NikeCatalogCase;

impl PageCase for NikeCatalogCase {
    const NAME: &'static str = "showcase_nike_catalog";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = proa_showcase::nike::catalog::default_data();
        proa_showcase::nike::catalog::render_into(cx, &data);
        Ok(())
    }
}

struct NikeProductCase;

impl PageCase for NikeProductCase {
    const NAME: &'static str = "showcase_nike_product";

    fn render<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = proa_showcase::nike::product::default_data();
        proa_showcase::nike::product::render_into(cx, &data);
        Ok(())
    }
}

struct DirectFlusher {
    bytes: Cell<usize>,
    logical_chunks: Cell<usize>,
    slices: Cell<usize>,
}

struct FlatteningFlusher {
    bytes: Cell<usize>,
    logical_chunks: Cell<usize>,
}

struct PerSliceCopyFlusher {
    bytes: Cell<usize>,
    logical_chunks: Cell<usize>,
    slices: Cell<usize>,
}

impl PerSliceCopyFlusher {
    fn new() -> Self {
        Self {
            bytes: Cell::new(0),
            logical_chunks: Cell::new(0),
            slices: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.bytes.set(0);
        self.logical_chunks.set(0);
        self.slices.set(0);
    }
}

impl StreamChunkFlusher for PerSliceCopyFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        if !chunk.is_empty() {
            self.bytes.set(self.bytes.get().saturating_add(chunk.len()));
            self.logical_chunks
                .set(self.logical_chunks.get().saturating_add(1));
            self.slices.set(self.slices.get().saturating_add(1));
        }
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut copied_bytes = 0usize;
        let mut slices = 0usize;
        chunk.for_each_slice(&mut |slice| {
            if !slice.is_empty() {
                let owned = Bytes::copy_from_slice(slice);
                copied_bytes = copied_bytes.saturating_add(black_box(owned.len()));
                slices = slices.saturating_add(1);
                black_box(owned);
            }
            Ok(())
        })?;
        if copied_bytes != 0 {
            self.bytes
                .set(self.bytes.get().saturating_add(copied_bytes));
            self.logical_chunks
                .set(self.logical_chunks.get().saturating_add(1));
            self.slices.set(self.slices.get().saturating_add(slices));
        }
        Ok(())
    }
}

impl FlatteningFlusher {
    fn new() -> Self {
        Self {
            bytes: Cell::new(0),
            logical_chunks: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.bytes.set(0);
        self.logical_chunks.set(0);
    }
}

impl StreamChunkFlusher for FlatteningFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        if !chunk.is_empty() {
            self.bytes.set(self.bytes.get().saturating_add(chunk.len()));
            self.logical_chunks
                .set(self.logical_chunks.get().saturating_add(1));
        }
        Ok(())
    }
}

impl DirectFlusher {
    fn new() -> Self {
        Self {
            bytes: Cell::new(0),
            logical_chunks: Cell::new(0),
            slices: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.bytes.set(0);
        self.logical_chunks.set(0);
        self.slices.set(0);
    }
}

impl StreamChunkFlusher for DirectFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        if !chunk.is_empty() {
            self.bytes.set(self.bytes.get().saturating_add(chunk.len()));
            self.logical_chunks
                .set(self.logical_chunks.get().saturating_add(1));
            self.slices.set(self.slices.get().saturating_add(1));
        }
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut bytes = 0usize;
        let mut slices = 0usize;
        chunk.for_each_slice(&mut |slice| {
            if !slice.is_empty() {
                bytes = bytes.saturating_add(slice.len());
                slices = slices.saturating_add(1);
            }
            Ok(())
        })?;
        if bytes != 0 {
            self.bytes.set(self.bytes.get().saturating_add(bytes));
            self.logical_chunks
                .set(self.logical_chunks.get().saturating_add(1));
            self.slices.set(self.slices.get().saturating_add(slices));
        }
        Ok(())
    }
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

fn render_sync<C: PageCase>(out: &mut RenderBuffer) -> usize {
    proa_core::render_into(out, SyncEnvelope::<C>::new());
    out.len()
}

fn render_sync_borrowed<C: PageCase>(out: &mut RenderBuffer) -> usize {
    out.clear();
    let mut cx = WebContext::with_buffer(&mut *out);
    SyncEnvelope::<C>::new()
        .render(&mut cx)
        .expect("borrowed sync page should render");
    out.len()
}

fn render_streamed<R>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    flusher: Rc<dyn StreamChunkFlusher>,
    root: R,
) -> Result<(), WriteError>
where
    R: WebRender<NoLoader, RenderBuffer>,
{
    let buffer = std::mem::take(out);
    let (result, buffer) = runtime.block_on(async move {
        let mut cx = WebContext::with_buffer(buffer)
            .with_render_mode(RenderMode::ChunkedInOrder)
            .with_stream_chunk_flusher(flusher);
        let result = render_in_context_async(&mut cx, root).await;
        (result, cx.into_output())
    });
    *out = buffer;
    result
}

fn render_streamed_with_loader<R>(
    runtime: &tokio::runtime::Runtime,
    loader: Arc<dyn DynDataLoader>,
    out: &mut RenderBuffer,
    flusher: Rc<dyn StreamChunkFlusher>,
    root: R,
) -> Result<(), WriteError>
where
    R: WebRender<DynLoaderHandle, RenderBuffer>,
{
    let buffer = std::mem::take(out);
    let (result, buffer) = runtime.block_on(async move {
        let mut cx = WebContext::with_dyn_loader_buffer(buffer, loader)
            .with_render_mode(RenderMode::ChunkedInOrder)
            .with_stream_chunk_flusher(flusher);
        let result = render_in_context_async(&mut cx, root).await;
        (result, cx.into_output())
    });
    *out = buffer;
    result
}

fn render_suspense<C: PageCase>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    flusher: &Rc<DirectFlusher>,
) -> usize {
    out.clear();
    flusher.reset();
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed(runtime, out, flusher_dyn, SuspenseEnvelope::<C>::new())
        .expect("production Suspense page should render");
    out.len().saturating_add(flusher.bytes.get())
}

fn render_suspense_per_slice<C: PageCase>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    flusher: &Rc<PerSliceCopyFlusher>,
) -> usize {
    out.clear();
    flusher.reset();
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed(runtime, out, flusher_dyn, SuspenseEnvelope::<C>::new())
        .expect("production per-slice page should render");
    out.len().saturating_add(flusher.bytes.get())
}

fn render_suspense_coalesced<C: PageCase>(
    runtime: &tokio::runtime::Runtime,
    out: &mut RenderBuffer,
    flusher: &Rc<FlatteningFlusher>,
) -> usize {
    out.clear();
    flusher.reset();
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed(runtime, out, flusher_dyn, SuspenseEnvelope::<C>::new())
        .expect("production coalesced page should render");
    out.len().saturating_add(flusher.bytes.get())
}

fn collect_suspense<C: PageCase>(runtime: &tokio::runtime::Runtime) -> Vec<u8> {
    let mut out = RenderBuffer::new();
    let flusher = Rc::new(CollectingFlusher::default());
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed(runtime, &mut out, flusher_dyn, SuspenseEnvelope::<C>::new())
        .expect("production Suspense parity page should render");
    let mut bytes = flusher.bytes.borrow().clone();
    for slice in out.slices() {
        bytes.extend_from_slice(slice);
    }
    bytes
}

fn preflight<C: PageCase>(runtime: &tokio::runtime::Runtime) -> usize {
    let mut sync_out = RenderBuffer::new();
    let expected_len = render_sync::<C>(&mut sync_out);
    let expected = sync_out.to_vec();
    assert_eq!(collect_suspense::<C>(runtime), expected);

    render_sync::<C>(&mut sync_out);
    start_counting();
    let direct_len = render_sync::<C>(&mut sync_out);
    let direct_alloc = stop_counting();

    let suspense_flusher = Rc::new(DirectFlusher::new());
    let mut suspense_out = RenderBuffer::new();
    render_suspense::<C>(runtime, &mut suspense_out, &suspense_flusher);
    start_counting();
    let suspense_len = render_suspense::<C>(runtime, &mut suspense_out, &suspense_flusher);
    let suspense_alloc = stop_counting();

    assert_eq!(direct_len, expected_len);
    assert_eq!(suspense_len, expected_len);

    eprintln!(
        "IN_ORDER_STREAMING_PREFLIGHT page={} bytes={} direct_allocs={} direct_alloc_bytes={} \
suspense_allocs={} suspense_alloc_bytes={} suspense_chunks={} suspense_slices={}",
        C::NAME,
        expected_len,
        direct_alloc.calls,
        direct_alloc.bytes,
        suspense_alloc.calls,
        suspense_alloc.bytes,
        suspense_flusher.logical_chunks.get(),
        suspense_flusher.slices.get(),
    );

    expected_len
}

#[derive(Debug, Default)]
struct ImmediateLoader;

impl DataLoader for ImmediateLoader {
    async fn load(&self, _request: &LoadRequest) -> Result<LoadValue, LoadError> {
        Ok(LoadValue::bytes(Bytes::from_static(b"null")))
    }
}

fn render_marketplace_direct(
    runtime: &tokio::runtime::Runtime,
    loader: &Arc<dyn DynDataLoader>,
    out: &mut RenderBuffer,
    flusher: &Rc<DirectFlusher>,
) -> usize {
    out.clear();
    flusher.reset();
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed_with_loader(
        runtime,
        loader.clone(),
        out,
        flusher_dyn,
        MarketplaceStreamPage,
    )
    .expect("marketplace allocation preflight should render");
    out.len().saturating_add(flusher.bytes.get())
}

fn render_marketplace_flattening(
    runtime: &tokio::runtime::Runtime,
    loader: &Arc<dyn DynDataLoader>,
    out: &mut RenderBuffer,
    flusher: &Rc<FlatteningFlusher>,
) -> usize {
    out.clear();
    flusher.reset();
    let flusher_dyn: Rc<dyn StreamChunkFlusher> = flusher.clone();
    render_streamed_with_loader(
        runtime,
        loader.clone(),
        out,
        flusher_dyn,
        MarketplaceStreamPage,
    )
    .expect("flattening allocation preflight should render");
    out.len().saturating_add(flusher.bytes.get())
}

fn preflight_marketplace_allocations(runtime: &tokio::runtime::Runtime) {
    let loader: Arc<dyn DynDataLoader> = Arc::new(ImmediateLoader);

    let direct_flusher = Rc::new(DirectFlusher::new());
    let mut direct_out = RenderBuffer::new();
    let expected_len =
        render_marketplace_direct(runtime, &loader, &mut direct_out, &direct_flusher);
    start_counting();
    let direct_len = render_marketplace_direct(runtime, &loader, &mut direct_out, &direct_flusher);
    let direct_alloc = stop_counting();

    let flattening_flusher = Rc::new(FlatteningFlusher::new());
    let mut flattening_out = RenderBuffer::new();
    render_marketplace_flattening(runtime, &loader, &mut flattening_out, &flattening_flusher);
    start_counting();
    let flattening_len =
        render_marketplace_flattening(runtime, &loader, &mut flattening_out, &flattening_flusher);
    let flattening_alloc = stop_counting();

    assert_eq!(direct_len, expected_len);
    assert_eq!(flattening_len, expected_len);

    eprintln!(
        "IN_ORDER_STREAMING_MARKETPLACE_ALLOC bytes={} direct_allocs={} \
direct_alloc_bytes={} flattening_allocs={} flattening_alloc_bytes={} \
direct_chunks={} direct_slices={}",
        expected_len,
        direct_alloc.calls,
        direct_alloc.bytes,
        flattening_alloc.calls,
        flattening_alloc.bytes,
        direct_flusher.logical_chunks.get(),
        direct_flusher.slices.get(),
    );
}

fn bench_case<C: PageCase>(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
) {
    let output_size = preflight::<C>(runtime);
    group.throughput(Throughput::Bytes(output_size as u64));

    group.bench_function(
        BenchmarkId::new(C::NAME, "borrowed_render_buffer_baseline"),
        |b| {
            let mut out = RenderBuffer::new();
            render_sync_borrowed::<C>(&mut out);
            b.iter(|| black_box(render_sync_borrowed::<C>(&mut out)));
        },
    );

    group.bench_function(BenchmarkId::new(C::NAME, "sync_render_buffer"), |b| {
        let mut out = RenderBuffer::new();
        render_sync::<C>(&mut out);
        b.iter(|| black_box(render_sync::<C>(&mut out)));
    });

    group.bench_function(BenchmarkId::new(C::NAME, "production_suspense"), |b| {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(DirectFlusher::new());
        render_suspense::<C>(runtime, &mut out, &flusher);
        b.iter(|| black_box(render_suspense::<C>(runtime, &mut out, &flusher)));
    });
}

fn bench_transport_case<C: PageCase>(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
) {
    let per_slice_flusher = Rc::new(PerSliceCopyFlusher::new());
    let mut per_slice_out = RenderBuffer::new();
    let expected_len =
        render_suspense_per_slice::<C>(runtime, &mut per_slice_out, &per_slice_flusher);
    start_counting();
    let per_slice_len =
        render_suspense_per_slice::<C>(runtime, &mut per_slice_out, &per_slice_flusher);
    let per_slice_alloc = stop_counting();

    let coalesced_flusher = Rc::new(FlatteningFlusher::new());
    let mut coalesced_out = RenderBuffer::new();
    render_suspense_coalesced::<C>(runtime, &mut coalesced_out, &coalesced_flusher);
    start_counting();
    let coalesced_len =
        render_suspense_coalesced::<C>(runtime, &mut coalesced_out, &coalesced_flusher);
    let coalesced_alloc = stop_counting();

    assert_eq!(per_slice_len, expected_len);
    assert_eq!(coalesced_len, expected_len);

    eprintln!(
        "IN_ORDER_STREAMING_TRANSPORT_PREFLIGHT page={} bytes={} \
per_slice_allocs={} per_slice_alloc_bytes={} per_slice_frames={} \
coalesced_allocs={} coalesced_alloc_bytes={} coalesced_chunks={}",
        C::NAME,
        expected_len,
        per_slice_alloc.calls,
        per_slice_alloc.bytes,
        per_slice_flusher.slices.get(),
        coalesced_alloc.calls,
        coalesced_alloc.bytes,
        coalesced_flusher.logical_chunks.get(),
    );

    group.throughput(Throughput::Bytes(expected_len as u64));
    group.bench_function(BenchmarkId::new(C::NAME, "per_slice_copy"), |b| {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(PerSliceCopyFlusher::new());
        render_suspense_per_slice::<C>(runtime, &mut out, &flusher);
        b.iter(|| black_box(render_suspense_per_slice::<C>(runtime, &mut out, &flusher)));
    });
    group.bench_function(BenchmarkId::new(C::NAME, "coalesced_copy"), |b| {
        let mut out = RenderBuffer::new();
        let flusher = Rc::new(FlatteningFlusher::new());
        render_suspense_coalesced::<C>(runtime, &mut out, &flusher);
        b.iter(|| black_box(render_suspense_coalesced::<C>(runtime, &mut out, &flusher)));
    });
}

fn bench_in_order_streaming_pages(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("benchmark runtime should build");
    let mut group = c.benchmark_group("in_order_streaming_pages");
    group.sample_size(60);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    bench_case::<SafeTextCase>(&mut group, &runtime);
    bench_case::<ComplexCase>(&mut group, &runtime);
    bench_case::<AirbnbHomeCase>(&mut group, &runtime);
    bench_case::<AirbnbListingCase>(&mut group, &runtime);
    bench_case::<NikeCatalogCase>(&mut group, &runtime);
    bench_case::<NikeProductCase>(&mut group, &runtime);

    group.finish();
    preflight_marketplace_allocations(&runtime);

    let mut transport_group = c.benchmark_group("in_order_streaming_transport");
    transport_group.sample_size(60);
    transport_group.warm_up_time(Duration::from_secs(1));
    transport_group.measurement_time(Duration::from_secs(3));
    bench_transport_case::<AirbnbHomeCase>(&mut transport_group, &runtime);
    bench_transport_case::<NikeCatalogCase>(&mut transport_group, &runtime);
    transport_group.finish();
}

criterion_group!(benches, bench_in_order_streaming_pages);
criterion_main!(benches);
