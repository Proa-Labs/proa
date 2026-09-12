// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::Body,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        Method, Request, StatusCode,
    },
    response::Response,
};
use http_body_util::BodyExt as _;
use proa_framework_axum::Readiness;
use tokio::sync::{Barrier, Notify};
use tower::ServiceExt as _;

const READY_BODY: &str = "ready\n";
const NOT_READY_BODY: &str = "not ready\n";

fn request(method: Method) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri("/readyz")
        .body(Body::empty())
        .expect("valid readiness request")
}

fn assert_stable_headers(response: &Response) {
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"no-store".parse().unwrap())
    );
    assert_eq!(
        response.headers().get(CONTENT_TYPE),
        Some(&"text/plain; charset=utf-8".parse().unwrap())
    );
}

async fn body_text(response: Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read readiness body")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("readiness body is UTF-8")
}

#[tokio::test]
async fn successful_probe_returns_stable_ready_response() {
    let app = Readiness::new("database", || async { Ok::<(), Infallible>(()) }).into_router();

    let response = app
        .oneshot(request(Method::GET))
        .await
        .expect("readiness response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_stable_headers(&response);
    assert_eq!(body_text(response).await, READY_BODY);
}

#[tokio::test]
async fn failed_probe_returns_503_without_leaking_internal_error() {
    const INTERNAL_ERROR: &str = "database password rejected by 10.0.0.7";
    let app = Readiness::new("database", || async { Err::<(), _>(INTERNAL_ERROR) }).into_router();

    let response = app
        .oneshot(request(Method::GET))
        .await
        .expect("readiness response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_stable_headers(&response);
    let body = body_text(response).await;
    assert_eq!(body, NOT_READY_BODY);
    assert!(!body.contains(INTERNAL_ERROR));
}

struct PendingProbe {
    started: Option<Arc<Notify>>,
    dropped: Arc<AtomicBool>,
}

impl Future for PendingProbe {
    type Output = Result<(), Infallible>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(started) = self.get_mut().started.take() {
            started.notify_one();
        }
        Poll::Pending
    }
}

impl Drop for PendingProbe {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn zero_timeout_returns_503_and_drops_pending_probe() {
    let dropped = Arc::new(AtomicBool::new(false));
    let app = Readiness::new("database", {
        let dropped = Arc::clone(&dropped);
        move || PendingProbe {
            started: None,
            dropped: Arc::clone(&dropped),
        }
    })
    .with_timeout(Duration::ZERO)
    .into_router();

    let response = app
        .oneshot(request(Method::GET))
        .await
        .expect("readiness response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_stable_headers(&response);
    assert_eq!(body_text(response).await, NOT_READY_BODY);
    assert!(
        dropped.load(Ordering::SeqCst),
        "timing out must cancel the pending dependency future"
    );
}

#[tokio::test]
async fn head_has_get_status_and_headers_without_a_body() {
    let app = Readiness::new("database", || async { Ok::<(), Infallible>(()) }).into_router();

    let response = app
        .oneshot(request(Method::HEAD))
        .await
        .expect("readiness response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_stable_headers(&response);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("read HEAD body")
        .to_bytes();
    assert!(body.is_empty(), "HEAD must not send the readiness body");
}

struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn concurrent_requests_run_independent_probes() {
    const REQUESTS: usize = 8;

    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(REQUESTS + 1));
    let app = Readiness::new("database", {
        let active = Arc::clone(&active);
        let max_active = Arc::clone(&max_active);
        let barrier = Arc::clone(&barrier);
        move || {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            let barrier = Arc::clone(&barrier);
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(current, Ordering::SeqCst);
                let _active = ActiveGuard(active);
                barrier.wait().await;
                Ok::<(), Infallible>(())
            }
        }
    })
    .into_router();

    let requests: Vec<_> = (0..REQUESTS)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                app.oneshot(request(Method::GET))
                    .await
                    .expect("readiness response")
            })
        })
        .collect();

    tokio::time::timeout(Duration::from_secs(1), barrier.wait())
        .await
        .expect("all probes should reach the barrier concurrently");

    for request in requests {
        let response = request.await.expect("request task");
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(max_active.load(Ordering::SeqCst), REQUESTS);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_later_request_recovers_after_dependency_failure() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let app = Readiness::new("database", {
        let attempts = Arc::clone(&attempts);
        move || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    Err("database is starting")
                } else {
                    Ok(())
                }
            }
        }
    })
    .into_router();

    let failed = app
        .clone()
        .oneshot(request(Method::GET))
        .await
        .expect("failed readiness response");
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_text(failed).await, NOT_READY_BODY);

    let recovered = app
        .oneshot(request(Method::GET))
        .await
        .expect("recovered readiness response");
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(body_text(recovered).await, READY_BODY);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_request_future_cancels_the_probe() {
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let app = Readiness::new("database", {
        let started = Arc::clone(&started);
        let dropped = Arc::clone(&dropped);
        move || PendingProbe {
            started: Some(Arc::clone(&started)),
            dropped: Arc::clone(&dropped),
        }
    })
    .with_timeout(Duration::from_secs(60))
    .into_router();

    let started_wait = started.notified();
    let request = tokio::spawn(async move {
        app.oneshot(request(Method::GET))
            .await
            .expect("readiness response")
    });

    tokio::time::timeout(Duration::from_secs(1), started_wait)
        .await
        .expect("probe should start");
    request.abort();
    let cancelled = request.await.expect_err("request task should be cancelled");
    assert!(cancelled.is_cancelled());
    assert!(
        dropped.load(Ordering::SeqCst),
        "dropping the request future must drop the dependency future"
    );
}
