// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! L0-3 proof: the declarative `ProaComponent` derive versus an equivalent
//! explicitly authored generic `WebRenderSync` component.
//!
//! This is intentionally a narrow comparison. All rows:
//!
//! - render the same component shape and concrete child type;
//! - include the same optional attribute, base/custom classes, and escaping;
//! - render through the same monomorphized helper into a reused `WriteCursor`;
//! - exclude output allocation and `WebContext` construction from timing.
//!
//! A preflight runs before Criterion and rejects byte drift, layout drift, a
//! missing generic `WebRenderSync<NoLoader, WriteCursor>` implementation, or
//! any heap allocation made by any component's render path.
//!
//! The authored row intentionally uses the idiomatic `html_sync!` baseline
//! instead of copying the derive expansion. The derive is expected to match
//! its top-level consolidation for compile-time-known fragments: one write
//! each for `<article`, the optional ` id="` prefix, and `</article>`. A
//! regression that splits those fragments therefore remains visible here
//! instead of turning this into a tautological macro-vs-copy test.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::mem::{align_of, size_of};
use std::time::Duration;

use criterion::{black_box, criterion_group, BenchmarkId, Criterion, Throughput};
use proa_core::stack::WriteCursor;
use proa_core::{ClassList, DataLoader, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{html_sync, ProaComponent};

const BUFFER_SIZE: usize = 512;
const CARD_ID: &str = "profile&summary";
const CUSTOM_CLASS: &str = "bench-extra";
const LABEL: &str = "Ada & Lin <3";
const EXPECTED: &[u8] = b"<article class=\"facade-card facade-card--featured bench-extra\" \
id=\"profile&amp;summary\"><span class=\"facade-card__label\">Ada &amp; Lin \
&lt;3</span></article>";

struct ThreadCountingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

#[inline]
fn record_allocation() {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn start_counting() {
    // Initialize both TLS cells before tracking is enabled.
    ALLOCATIONS.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
}

fn stop_counting() -> usize {
    TRACKING.with(|tracking| tracking.set(false));
    ALLOCATIONS.with(Cell::get)
}

#[derive(Clone, Copy)]
struct Label(&'static str);

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Label {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <span class="facade-card__label">{self.0}</span>
        }
        .render(cx)
    }
}

#[derive(ProaComponent)]
#[component(tag = "article", children_builder)]
struct DerivedCard<C = ()> {
    #[component(attr, attr_name = "id")]
    id: Option<&'static str>,
    #[component(class)]
    class: Option<&'static str>,
    #[component(children)]
    children: Option<C>,
}

impl<C> DerivedCard<C> {
    #[inline(always)]
    fn classes(&self) -> ([&'static str; 2], usize) {
        (["facade-card", "facade-card--featured"], 2)
    }
}

struct AuthoredCard<C = ()> {
    id: Option<&'static str>,
    class: Option<&'static str>,
    children: Option<C>,
}

impl<C> AuthoredCard<C> {
    #[inline(always)]
    fn with_children<NewChild>(self, child: NewChild) -> AuthoredCard<NewChild> {
        AuthoredCard {
            id: self.id,
            class: self.class,
            children: Some(child),
        }
    }
}

impl<L, B, C> WebRenderSync<L, B> for AuthoredCard<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let classes = ClassList::<3>::new()
            .add("facade-card")
            .add("facade-card--featured")
            .add_opt(self.class);
        html_sync! {
            <article class={classes} id={self.id}>{self.children}</article>
        }
        .render(cx)
    }
}

#[inline(always)]
fn render_statically<B, R>(root: R, cx: &mut WebContext<NoLoader, B>) -> Result<(), WriteError>
where
    B: WriteBuf,
    R: WebRenderSync<NoLoader, B>,
{
    WebRenderSync::render(root, cx)
}

fn derived_card() -> DerivedCard<Label> {
    DerivedCard::<()> {
        id: Some(CARD_ID),
        class: Some(CUSTOM_CLASS),
        children: None,
    }
    .with_children(Label(LABEL))
}

fn authored_card() -> AuthoredCard<Label> {
    AuthoredCard::<()> {
        id: Some(CARD_ID),
        class: Some(CUSTOM_CLASS),
        children: None,
    }
    .with_children(Label(LABEL))
}

fn assert_stack_static_dispatch<R>()
where
    for<'buffer> R: WebRenderSync<NoLoader, WriteCursor<'buffer>>,
{
}

fn verify_contract() {
    assert_stack_static_dispatch::<DerivedCard<Label>>();
    assert_stack_static_dispatch::<AuthoredCard<Label>>();
    assert_eq!(
        size_of::<DerivedCard<Label>>(),
        size_of::<AuthoredCard<Label>>(),
        "the facade must not add per-component storage"
    );
    assert_eq!(
        align_of::<DerivedCard<Label>>(),
        align_of::<AuthoredCard<Label>>(),
        "the facade must not change component alignment"
    );

    let mut derived_backing = [0_u8; BUFFER_SIZE];
    let mut derived_cursor = WriteCursor::new(&mut derived_backing);
    let mut derived_cx = WebContext::with_buffer(&mut derived_cursor);
    start_counting();
    render_statically(derived_card(), &mut derived_cx)
        .expect("derived component preflight render should succeed");
    let derived_allocations = stop_counting();

    let mut authored_backing = [0_u8; BUFFER_SIZE];
    let mut authored_cursor = WriteCursor::new(&mut authored_backing);
    let mut authored_cx = WebContext::with_buffer(&mut authored_cursor);
    start_counting();
    render_statically(authored_card(), &mut authored_cx)
        .expect("authored component preflight render should succeed");
    let authored_allocations = stop_counting();

    assert_eq!(derived_cx.out.as_slice(), EXPECTED);
    assert_eq!(authored_cx.out.as_slice(), EXPECTED);
    assert_eq!(
        derived_cx.out.as_slice(),
        authored_cx.out.as_slice(),
        "facade and authored output must be byte-identical"
    );
    assert_eq!(
        derived_allocations, 0,
        "derived component render path allocated"
    );
    assert_eq!(
        authored_allocations, 0,
        "authored component render path allocated"
    );

    eprintln!(
        "component facade preflight: {} bytes, derived allocations: {}, \
authored allocations: {}, component size: {} bytes",
        EXPECTED.len(),
        derived_allocations,
        authored_allocations,
        size_of::<DerivedCard<Label>>()
    );
}

fn bench_component_facade(c: &mut Criterion) {
    let mut group = c.benchmark_group("l0_3_component_facade");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Bytes(EXPECTED.len() as u64));

    group.bench_function(BenchmarkId::new("derived", "stack_reused"), |b| {
        let mut backing = [0_u8; BUFFER_SIZE];
        let mut cursor = WriteCursor::new(&mut backing);
        let mut cx = WebContext::with_buffer(&mut cursor);

        b.iter(|| {
            cx.out.clear();
            let card = DerivedCard::<()> {
                id: Some(black_box(CARD_ID)),
                class: Some(black_box(CUSTOM_CLASS)),
                children: None,
            }
            .with_children(Label(black_box(LABEL)));
            render_statically(card, &mut cx).expect("derived benchmark render should succeed");
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function(BenchmarkId::new("authored", "stack_reused"), |b| {
        let mut backing = [0_u8; BUFFER_SIZE];
        let mut cursor = WriteCursor::new(&mut backing);
        let mut cx = WebContext::with_buffer(&mut cursor);

        b.iter(|| {
            cx.out.clear();
            let card = AuthoredCard::<()> {
                id: Some(black_box(CARD_ID)),
                class: Some(black_box(CUSTOM_CLASS)),
                children: None,
            }
            .with_children(Label(black_box(LABEL)));
            render_statically(card, &mut cx).expect("authored benchmark render should succeed");
            black_box(cx.out.as_slice());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_component_facade);

fn main() {
    verify_contract();
    benches();
    Criterion::default().configure_from_args().final_summary();
}
