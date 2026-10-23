// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end page benchmark for request-scoped `WebContext` state.
//!
//! The page matches the standard Proa SSR fixture shape: four sections,
//! eight cards per section, and five feature rows per card. Request metadata
//! is read independently by the root, every section, and every card, for 37
//! `cx.request()` call sites per page render.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::future::{ready, Future};
use std::rc::Rc;
use std::time::Duration;

use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, Uri};
use criterion::{black_box, criterion_group, Criterion, Throughput};
use proa_bench::{CARDS_PER_SECTION, ITEMS_PER_CARD, NUM_SECTIONS};
use proa_core::{
    render_in_context_async, DataLoader, RenderBuffer, RenderMode, RenderOutcome, RequestContext,
    RequestLocation, ResponseContext, RouteParam, RouteParams, SafeText, StreamBoundaryRecorder,
    SyncToAsync, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
use proa_stream::{drain_pending_boundaries_with_artifacts, StreamContext, Suspense};

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
        let _ = ALLOCATED_BYTES.try_with(|count| count.set(count.get() + bytes));
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

fn count_allocations(run: impl FnOnce()) -> usize {
    start_counting_allocations();
    run();
    stop_counting_allocations()
}

fn start_counting_allocations() {
    ALLOCATIONS.with(|count| count.set(0));
    ALLOCATED_BYTES.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
}

fn stop_counting_allocations() -> usize {
    TRACKING.with(|tracking| tracking.set(false));
    ALLOCATIONS.with(Cell::get)
}

fn allocated_bytes() -> usize {
    ALLOCATED_BYTES.with(Cell::get)
}

#[derive(Clone, Copy)]
struct RequestAwarePage;

impl<L, B> WebRenderSync<L, B> for RequestAwarePage
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let method = if cx.request().method() == Some(&Method::GET) {
            "get"
        } else {
            "other"
        };

        html_sync! {
            <html lang="en" data-request-method={method}>
                <head>
                    <title>"SSR Benchmark Page"</title>
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                </head>
                <body>
                    <header class="hero-section">
                        <h1 class="title">"SSR Framework Benchmark"</h1>
                        <p class="subtitle">"Request-aware compiled rendering"</p>
                    </header>
                    <main class="main-content">{RequestAwareSectionList}</main>
                    <footer class="footer">
                        <p>"Rendered by the SSR benchmark fixture"</p>
                    </footer>
                </body>
            </html>
        }
        .render(cx)
    }
}

#[derive(Clone, Copy)]
struct RequestAwareSectionList;

impl<L, B> WebRenderSync<L, B> for RequestAwareSectionList
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for section_idx in 0..NUM_SECTIONS {
            RequestAwareSection { idx: section_idx }.render(cx)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RequestAwareSection {
    idx: usize,
}

impl<L, B> WebRenderSync<L, B> for RequestAwareSection
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let section_class = if self.idx.is_multiple_of(2) {
            "even"
        } else {
            "odd"
        };
        let request_marker = if cx.request().headers().contains_key("x-proa-benchmark") {
            "benchmark"
        } else {
            "ordinary"
        };
        let title = SafeText::from_trusted(match self.idx {
            0 => "Performance Metrics",
            1 => "Rendering Speed",
            2 => "Memory Efficiency",
            _ => "Framework Comparison",
        });

        html_sync! {
            <section
                class="card-section"
                data-section={section_class}
                data-request={request_marker}
            >
                <h2 class="section-title">{title}</h2>
                <div class="card-grid">{RequestAwareCardGrid}</div>
            </section>
        }
        .render(cx)
    }
}

#[derive(Clone, Copy)]
struct RequestAwareCardGrid;

impl<L, B> WebRenderSync<L, B> for RequestAwareCardGrid
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for card_idx in 0..CARDS_PER_SECTION {
            RequestAwareCard { idx: card_idx }.render(cx)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RequestAwareCard {
    idx: usize,
}

impl<L, B> WebRenderSync<L, B> for RequestAwareCard
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let card_class = if self.idx.is_multiple_of(2) {
            "even"
        } else {
            "odd"
        };
        let tenant = if cx.request().param("tenant") == Some("acme") {
            "known"
        } else {
            "unknown"
        };
        let title = SafeText::from_trusted(match self.idx % 4 {
            0 => "Latency Analysis",
            1 => "Throughput Metrics",
            2 => "Memory Usage",
            _ => "CPU Utilization",
        });

        html_sync! {
            <article class="card" data-card-id={card_class} data-tenant={tenant}>
                <div class="card-header">
                    <img
                        src="/images/benchmark.png"
                        alt="Benchmark chart"
                        class="card-image"
                    />
                </div>
                <div class="card-body">
                    <h3 class="card-title">{title}</h3>
                    <p class="card-description">
                        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
                    </p>
                    <ul class="feature-list">{RequestAwareFeatureList}</ul>
                    <button class="btn-primary" type="button">"View Details"</button>
                </div>
            </article>
        }
        .render(cx)
    }
}

#[derive(Clone, Copy)]
struct RequestAwareFeatureList;

impl<L, B> WebRenderSync<L, B> for RequestAwareFeatureList
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item_idx in 0..ITEMS_PER_CARD {
            RequestAwareFeature { idx: item_idx }.render(cx)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RequestAwareFeature {
    idx: usize,
}

#[derive(Clone, Copy)]
struct RequestAwareOutOfOrderPage;

impl<L> proa_core::WebRender<L, RenderBuffer> for RequestAwareOutOfOrderPage
where
    L: DataLoader + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let shell_open = if cx.request().method() == Some(&Method::GET) {
                b"<html lang=\"en\" data-request-method=\"get\"><head><title>SSR Benchmark Page</title><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head><body><header class=\"hero-section\"><h1 class=\"title\">SSR Framework Benchmark</h1><p class=\"subtitle\">Request-aware compiled rendering</p></header><main class=\"main-content\">".as_slice()
            } else {
                b"<html lang=\"en\" data-request-method=\"other\"><head><title>SSR Benchmark Page</title><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head><body><header class=\"hero-section\"><h1 class=\"title\">SSR Framework Benchmark</h1><p class=\"subtitle\">Request-aware compiled rendering</p></header><main class=\"main-content\">".as_slice()
            };
            cx.out.extend_static(shell_open)?;

            for section_idx in 0..NUM_SECTIONS {
                proa_core::render_child(
                    &mut *cx,
                    Suspense::new(
                        SyncToAsync(RequestAwareSection { idx: section_idx }),
                        RequestAwareSectionFallback,
                    ),
                )
                .await?;
            }

            cx.out.extend_static(
                b"</main><footer class=\"footer\"><p>Rendered by the SSR benchmark fixture</p></footer></body></html>",
            )
        })
    }
}

#[derive(Clone, Copy)]
struct RequestAwareSectionFallback;

impl<L, B> WebRenderSync<L, B> for RequestAwareSectionFallback
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class="card-section pending" aria-busy="true">
                <h2 class="section-title">"Loading section"</h2>
                <div class="card-grid"></div>
            </section>
        }
        .render(cx)
    }
}

impl<L, B> WebRenderSync<L, B> for RequestAwareFeature
where
    B: WriteBuf,
    L: DataLoader,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let text = SafeText::from_trusted(match self.idx {
            0 => "Sub-millisecond response times",
            1 => "Zero-copy rendering pipeline",
            2 => "SIMD-accelerated escaping",
            3 => "Pre-allocated buffers",
            _ => "Minimal heap allocations",
        });
        html_sync! {
            <li class="feature-item">{text}</li>
        }
        .render(cx)
    }
}

fn benchmark_request() -> RequestContext {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-proa-benchmark"),
        HeaderValue::from_static("context-parts"),
    );
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static("proa-benchmark/1.0"),
    );
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static("session=benchmark; theme=dark"),
    );

    RequestContext::new()
        .with_method(Method::GET)
        .with_location(RequestLocation::new(Uri::from_static("/bench/acme?page=1")))
        .with_route_params(RouteParams::from_vec(vec![RouteParam::new(
            "tenant", "acme",
        )]))
        .with_headers(headers)
        .with_client_ip("127.0.0.1")
        .with_original_pathname("/bench/acme")
}

fn render_fresh_request(request: &RequestContext, buffer: RenderBuffer) -> (RenderBuffer, usize) {
    let mut cx = WebContext::with_buffer(buffer);
    cx.set_request(request.clone());
    RequestAwarePage
        .render(&mut cx)
        .expect("request-aware benchmark page should render");
    let len = cx.out.len();
    (cx.into_output(), len)
}

fn render_out_of_order_request(
    runtime: &tokio::runtime::Runtime,
    request: &RequestContext,
    buffer: RenderBuffer,
) -> (RenderBuffer, usize, usize) {
    runtime.block_on(async {
        let stream = StreamContext::new(RenderMode::ChunkedOutOfOrder);
        let recorder: Rc<dyn StreamBoundaryRecorder> = Rc::new(stream.clone());
        let mut cx = WebContext::with_buffer(buffer)
            .with_render_mode(RenderMode::ChunkedOutOfOrder)
            .with_stream_recorder(recorder);
        cx.set_request(request.clone());
        let mut response = ResponseContext::new();
        response.mark_streaming_started();
        cx.set_response(response);
        render_in_context_async(&mut cx, RequestAwareOutOfOrderPage)
            .await
            .expect("request-aware out-of-order page should render its shell");
        let shell_len = cx.out.len();
        let _ = cx.rsjs_shared_state_seed_table_json();
        let buffer = cx.into_output();

        let boundary_bytes = Cell::new(0usize);
        let boundary_count = Cell::new(0usize);
        drain_pending_boundaries_with_artifacts(&stream, |chunk| {
            boundary_count.set(boundary_count.get() + 1);
            boundary_bytes.set(boundary_bytes.get() + chunk.bytes.len());
            ready(true)
        })
        .await;

        (
            buffer,
            shell_len + boundary_bytes.get(),
            boundary_count.get(),
        )
    })
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn verify_contract() -> usize {
    let request = benchmark_request();
    let (mut buffer, output_size) = render_fresh_request(&request, RenderBuffer::new());
    let output = buffer.to_vec();

    assert_eq!(
        count_occurrences(&output, b"data-request-method=\"get\""),
        1
    );
    assert_eq!(
        count_occurrences(&output, b"data-request=\"benchmark\""),
        NUM_SECTIONS
    );
    assert_eq!(
        count_occurrences(&output, b"data-tenant=\"known\""),
        NUM_SECTIONS * CARDS_PER_SECTION
    );

    buffer.clear();
    let framework_allocations = count_allocations(|| {
        let owned = std::mem::take(&mut buffer);
        let (returned, len) = render_fresh_request(&request, owned);
        buffer = returned;
        black_box(len);
    });

    let mut reused = WebContext::with_buffer(buffer);
    reused.set_request(request.clone());
    reused.out.clear();
    RequestAwarePage
        .render(&mut reused)
        .expect("request-aware benchmark page should warm its output buffer");
    let reused_allocations = count_allocations(|| {
        reused.out.clear();
        RequestAwarePage
            .render(&mut reused)
            .expect("request-aware benchmark page should rerender");
        black_box(reused.out.len());
    });

    assert_eq!(
        reused_allocations, 0,
        "a warmed full-page render with request reads must not allocate"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("out-of-order benchmark runtime should build");
    let (mut out_of_order_buffer, streamed_size, boundary_count) =
        render_out_of_order_request(&runtime, &request, RenderBuffer::new());
    assert_eq!(boundary_count, NUM_SECTIONS);
    assert!(streamed_size > output_size);
    out_of_order_buffer.clear();
    let out_of_order_allocations = count_allocations(|| {
        let owned = std::mem::take(&mut out_of_order_buffer);
        let (returned, len, boundaries) = render_out_of_order_request(&runtime, &request, owned);
        out_of_order_buffer = returned;
        black_box((len, boundaries));
    });
    let out_of_order_allocated_bytes = allocated_bytes();

    eprintln!(
        "context-parts full page: {output_size} bytes, 37 request reads, \
         fresh-request allocations={framework_allocations}, \
         reused-context render allocations={reused_allocations}; \
         out-of-order page: {streamed_size} bytes, {boundary_count} boundaries, \
         allocations={out_of_order_allocations}, \
         allocated bytes={out_of_order_allocated_bytes}",
    );
    black_box(out_of_order_buffer.len());
    output_size
}

fn bench_context_parts_page(c: &mut Criterion) {
    let request = benchmark_request();
    let (mut reusable_buffer, output_size) = render_fresh_request(&request, RenderBuffer::new());

    let mut group = c.benchmark_group("context_parts_full_page");
    group.sample_size(100);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.throughput(Throughput::Bytes(output_size as u64));

    group.bench_function("framework_request_render", |b| {
        b.iter(|| {
            reusable_buffer.clear();
            let owned = std::mem::take(&mut reusable_buffer);
            let (returned, len) = render_fresh_request(black_box(&request), owned);
            reusable_buffer = returned;
            black_box(len);
        });
    });

    group.bench_function("reused_context_render", |b| {
        let mut cx = WebContext::with_buffer(RenderBuffer::new());
        cx.set_request(request.clone());
        RequestAwarePage
            .render(&mut cx)
            .expect("request-aware benchmark page should warm its output buffer");

        b.iter(|| {
            cx.out.clear();
            RequestAwarePage
                .render(&mut cx)
                .expect("request-aware benchmark page should rerender");
            black_box(cx.out.len());
        });
    });

    group.finish();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("out-of-order benchmark runtime should build");
    let (mut streaming_buffer, streamed_size, boundary_count) =
        render_out_of_order_request(&runtime, &request, RenderBuffer::new());
    assert_eq!(boundary_count, NUM_SECTIONS);

    let mut streaming_group = c.benchmark_group("context_parts_out_of_order_page");
    streaming_group.sample_size(100);
    streaming_group.warm_up_time(Duration::from_secs(2));
    streaming_group.measurement_time(Duration::from_secs(5));
    streaming_group.throughput(Throughput::Bytes(streamed_size as u64));
    streaming_group.bench_function("four_suspense_boundaries", |b| {
        b.iter(|| {
            streaming_buffer.clear();
            let owned = std::mem::take(&mut streaming_buffer);
            let (returned, len, boundaries) =
                render_out_of_order_request(&runtime, black_box(&request), owned);
            streaming_buffer = returned;
            black_box((len, boundaries));
        });
    });
    streaming_group.finish();
}

fn bench_document_metadata(c: &mut Criterion) {
    use proa_core::{DocumentMetadata, Metadata, OpenGraphImage, SendWebContext};
    eprintln!(
        "metadata context sizes: local={} send={} facade={} parts={}",
        std::mem::size_of::<WebContext>(),
        std::mem::size_of::<SendWebContext>(),
        std::mem::size_of::<DocumentMetadata>(),
        std::mem::size_of::<proa_core::WebContextParts>()
    );
    let mut bytes = Vec::with_capacity(4096);
    let mut cx = WebContext::with_buffer(&mut bytes);
    let unused = count_allocations(|| {
        cx.metadata(Metadata::new()).unwrap();
    });
    assert_eq!(unused, 0);
    let static_declaration = count_allocations(|| {
        cx.metadata(
            Metadata::new()
                .title("Store")
                .title_template("%s | Store")
                .open_graph(|og| {
                    og.site_name("Store");
                }),
        )
        .unwrap();
        cx.__next_metadata_route_scope().unwrap();
        cx.metadata(
            Metadata::new()
                .title("Pricing")
                .canonical("https://example.com/pricing"),
        )
        .unwrap();
    });
    let static_emission = count_allocations(|| {
        cx.metadata.head().render(&mut cx).unwrap();
    });
    assert_eq!(static_declaration, 1);
    assert_eq!(static_emission, 0);
    drop(cx);
    let mut cx = WebContext::with_buffer(&mut bytes);
    let dynamic = count_allocations(|| {
        cx.metadata(
            Metadata::new()
                .title(String::from("A dynamic title"))
                .open_graph(|og| {
                    og.image(
                        OpenGraphImage::new(String::from("https://example.com/front.png"))
                            .with_alt(String::from("Front")),
                    );
                    og.image(String::from("https://example.com/back.png"));
                }),
        )
        .unwrap();
    });
    let dynamic_emission = count_allocations(|| {
        cx.metadata.head().render(&mut cx).unwrap();
    });
    assert_eq!(dynamic_emission, 0);
    eprintln!("metadata allocations (preallocated writer): empty={unused}; static layout+page declarations={static_declaration}, emission={static_emission}; dynamic title+2 images declarations={dynamic}, emission={dynamic_emission}; bytes={}", cx.out.len());
    let mut group = c.benchmark_group("document_metadata");
    group.sample_size(100);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.bench_function("static_layout_and_page", |b| {
        b.iter_batched(
            || WebContext::with_buffer(Vec::with_capacity(4096)),
            |mut cx| {
                cx.metadata(
                    Metadata::new()
                        .title("Store")
                        .title_template("%s | Store")
                        .open_graph(|og| {
                            og.site_name("Store");
                        }),
                )
                .unwrap();
                cx.__next_metadata_route_scope().unwrap();
                cx.metadata(
                    Metadata::new()
                        .title("Pricing")
                        .canonical("https://example.com/pricing"),
                )
                .unwrap();
                cx.metadata.head().render(&mut cx).unwrap();
                black_box(cx.out.len());
                cx
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("owned_title_and_two_images", |b| {
        b.iter_batched(
            || WebContext::with_buffer(Vec::with_capacity(4096)),
            |mut cx| {
                cx.metadata(
                    Metadata::new()
                        .title(String::from("A dynamic title"))
                        .open_graph(|og| {
                            og.image(
                                OpenGraphImage::new(String::from("https://example.com/front.png"))
                                    .with_alt(String::from("Front")),
                            );
                            og.image(String::from("https://example.com/back.png"));
                        }),
                )
                .unwrap();
                cx.metadata.head().render(&mut cx).unwrap();
                black_box(cx.out.len());
                cx
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, bench_context_parts_page, bench_document_metadata);

fn main() {
    verify_contract();
    benches();
    Criterion::default().configure_from_args().final_summary();
}
