// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compare request-time static defaults with the precomputed implementation.
//! Run scripts/bench_metadata_static.py to exercise both revisions.
#![allow(unsafe_code)]

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use proa_core::{SendWebContext, WebContext};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Duration,
};
use tower::ServiceExt;
mod api {
    include!("../fixtures/metadata_static_current.rs.txt");
}

thread_local! { static COUNT: Cell<Option<(usize, usize)>> = const { Cell::new(None) }; }
struct Allocator;
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn allocation(bytes: usize) {
    let _ = COUNT.try_with(|c| {
        if let Some((n, b)) = c.get() {
            c.set(Some((n + 1, b + bytes)));
        }
    });
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        allocation(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        allocation(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        allocation(n);
        unsafe { System.realloc(p, l, n) }
    }
}
fn allocations(run: impl FnOnce()) -> (usize, usize) {
    COUNT.with(|c| c.set(Some((0, 0))));
    run();
    COUNT.with(|c| c.replace(None).unwrap())
}
async fn document(app: axum::Router) -> bytes::Bytes {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/pricing")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
}
fn future_size() -> usize {
    let future = async {
        let mut cx = WebContext::new();
        std::future::pending::<()>().await;
        black_box(&mut cx);
    };
    std::mem::size_of_val(&future)
}
fn bench(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let app = api::app();
    let output = runtime.block_on(document(app.clone()));
    let prepared = api::Prepared::new();
    let mut cx = WebContext::new();
    prepared.emit(&mut cx);
    let head = cx.into_output().into_vec();
    assert_eq!(
        &output[..],
        [
            b"<html><head>".as_slice(),
            head.as_slice(),
            b"</head><body>Pricing</body></html>"
        ]
        .concat()
    );
    let http_allocations = allocations(|| {
        black_box(runtime.block_on(document(app.clone())));
    });
    let mut cx = WebContext::new();
    let head_allocations = allocations(|| prepared.emit(&mut cx));
    eprintln!("static metadata: bytes={} HTTP allocations={:?}; head allocations={:?}; context={} send={} owned pending future={}", output.len(), http_allocations, head_allocations, std::mem::size_of::<WebContext>(), std::mem::size_of::<SendWebContext>(), future_size());
    let mut group = c.benchmark_group("static_metadata");
    group.sample_size(100);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.bench_function("http_document", |b| {
        b.iter(|| black_box(runtime.block_on(document(app.clone()))))
    });
    // Old path reuses a fully resolved immutable snapshot prepared at startup;
    // new path reuses immutable declarations and applies them per request.
    group.bench_function("head_only", |b| {
        b.iter_batched(
            || WebContext::new(),
            |mut cx| {
                prepared.emit(&mut cx);
                black_box(cx)
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}
criterion_group!(benches, bench);
criterion_main!(benches);
