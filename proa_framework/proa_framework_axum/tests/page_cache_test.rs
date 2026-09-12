// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderName, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use proa_framework_axum::{
    FrameworkBuilder, IslandManifest, ManifestAssets, PageCacheLayer, RouteResponse,
};
use tokio::sync::Notify;
use tower::{service_fn, Layer, ServiceExt};

fn request(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn counter_app(layer: PageCacheLayer, calls: Arc<AtomicUsize>) -> Router {
    Router::new()
        .route(
            "/",
            get(move || {
                let calls = calls.clone();
                async move { calls.fetch_add(1, Ordering::SeqCst).to_string() }
            }),
        )
        .route_layer(layer)
}

#[tokio::test]
async fn cache_hits_skip_handler_and_revalidate_without_a_body() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = counter_app(PageCacheLayer::new("build-a"), calls.clone());
    let first = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(first.headers()["x-proa-page-cache"], "MISS");
    assert_eq!(first.headers()[header::CACHE_CONTROL], "public, no-cache");
    let etag = first.headers()[header::ETAG].clone();
    assert_eq!(body(first).await, "0");
    let hit = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(hit.headers()["x-proa-page-cache"], "HIT");
    assert_eq!(body(hit).await, "0");

    for validator in [
        etag.to_str().unwrap().to_owned(),
        format!("\"other\", W/{}", etag.to_str().unwrap()),
        "*".into(),
    ] {
        let mut req = request("/");
        req.headers_mut()
            .insert(header::IF_NONE_MATCH, validator.parse().unwrap());
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()[header::ETAG], etag);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, no-cache"
        );
        assert!(response.headers().contains_key(header::VARY));
        assert_eq!(body(response).await, "");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn new_build_cannot_validate_the_previous_builds_page() {
    let old = counter_app(
        PageCacheLayer::new("old-build"),
        Arc::new(AtomicUsize::new(0)),
    );
    let old_response = old.oneshot(request("/")).await.unwrap();
    let old_etag = old_response.headers()[header::ETAG].clone();
    let new_calls = Arc::new(AtomicUsize::new(0));
    let new = counter_app(PageCacheLayer::new("new-build"), new_calls.clone());
    let mut req = request("/");
    req.headers_mut()
        .insert(header::IF_NONE_MATCH, old_etag.clone());
    let response = new.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_ne!(response.headers()[header::ETAG], old_etag);
    assert_eq!(new_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn hosts_queries_and_representations_have_separate_entries() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = counter_app(PageCacheLayer::new("build"), calls.clone());
    for (url, host, accept, client) in [
        ("/?q=1", "a.example", "text/html", ""),
        ("/?q=2", "a.example", "text/html", ""),
        ("/?q=1", "b.example", "text/html", ""),
        ("/?q=1", "a.example", "text/markdown", ""),
        ("/?q=1", "a.example", "text/html", "1"),
    ] {
        for expected in ["MISS", "HIT"] {
            let req = Request::builder()
                .uri(url)
                .header("host", host)
                .header("accept", accept)
                .header("x-proa-client", client)
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.headers()["x-proa-page-cache"], expected);
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn private_requests_bypass_existing_public_entries() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = counter_app(PageCacheLayer::new("build"), calls.clone());
    app.clone().oneshot(request("/")).await.unwrap();
    for (name, value) in [
        ("cookie", "session=secret"),
        ("authorization", "Bearer secret"),
        ("cache-control", "no-store"),
        ("range", "bytes=0-1"),
    ] {
        let mut req = request("/");
        req.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.headers()["x-proa-page-cache"], "BYPASS");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
    }
    let mut req = request("/");
    req.extensions_mut()
        .insert(proa_core::CspNonce::new("dGVzdA").unwrap());
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().headers()["x-proa-page-cache"],
        "BYPASS"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(body(app.oneshot(request("/")).await.unwrap()).await, "0");
}

#[tokio::test]
async fn unsafe_responses_and_unknown_vary_fields_are_not_stored() {
    for (name, value) in [
        ("cache-control", "PRIVATE, max-age=60"),
        ("cache-control", "no-store"),
        ("cloudflare-cdn-cache-control", "no-store"),
        ("set-cookie", "session=secret"),
        ("vary", "*"),
        ("vary", "x-theme"),
        ("content-security-policy", "script-src 'nonce-dGVzdA'"),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let service = PageCacheLayer::new("build").layer(service_fn(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async move {
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(name, value)
                        .body(Body::from("private"))
                        .unwrap(),
                )
            }
        }));
        for _ in 0..2 {
            assert_eq!(
                service
                    .clone()
                    .oneshot(request("/"))
                    .await
                    .unwrap()
                    .headers()["x-proa-page-cache"],
                "BYPASS"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2, "{name}: {value}");
    }
}

#[tokio::test]
async fn registered_vary_fields_are_partitioned_and_cacheable() {
    let layer = PageCacheLayer::new("build").vary_on(HeaderName::from_static("x-theme"));
    let service = layer.layer(service_fn(|req: Request<Body>| async move {
        Ok::<_, Infallible>(
            Response::builder()
                .header("vary", "x-theme")
                .body(Body::from(
                    req.headers()["x-theme"].to_str().unwrap().to_owned(),
                ))
                .unwrap(),
        )
    }));
    for theme in ["light", "dark", "light"] {
        let req = Request::builder()
            .uri("/")
            .header("x-theme", theme)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            body(service.clone().oneshot(req).await.unwrap()).await,
            theme
        );
    }
}

#[tokio::test]
async fn head_hits_have_no_body_and_head_misses_never_seed_get() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = counter_app(PageCacheLayer::new("build"), calls.clone());
    let mut head = request("/");
    *head.method_mut() = Method::HEAD;
    assert_eq!(
        app.clone().oneshot(head).await.unwrap().headers()["x-proa-page-cache"],
        "BYPASS"
    );
    let get = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(get.headers()["x-proa-page-cache"], "MISS");
    assert_eq!(body(get).await, "1");
    let mut head = request("/");
    *head.method_mut() = Method::HEAD;
    let response = app.oneshot(head).await.unwrap();
    assert_eq!(response.headers()["x-proa-page-cache"], "HIT");
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "1");
    assert_eq!(body(response).await, "");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn simultaneous_misses_render_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let service = PageCacheLayer::new("build").layer(service_fn(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        async {
            tokio::task::yield_now().await;
            Ok::<_, Infallible>(Response::new(Body::from("complete")))
        }
    }));
    let responses =
        futures::future::join_all((0..16).map(|_| service.clone().oneshot(request("/")))).await;
    for response in responses {
        assert_eq!(body(response.unwrap()).await, "complete");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn isr_serves_stale_collapses_refreshes_and_retains_route_context() {
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let count = calls.clone();
    let signal = started.clone();
    let gate = release.clone();
    let routes = Router::new()
        .route(
            "/",
            get(move || {
                let count = count.clone();
                let signal = signal.clone();
                let gate = gate.clone();
                async move {
                    let generation = count.fetch_add(1, Ordering::SeqCst);
                    if generation > 0 {
                        signal.notify_one();
                        gate.notified().await;
                    }
                    RouteResponse::ssr(proa_core::text::text(if generation == 0 {
                        "initial"
                    } else {
                        "regenerated"
                    }))
                }
            }),
        )
        .route_layer(PageCacheLayer::new("build").revalidate(Duration::from_secs(3600)));
    let app = FrameworkBuilder::new(IslandManifest {
        islands: HashMap::new(),
        assets: ManifestAssets::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    })
    .with_dynamic_routes(routes)
    .build();

    let first = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = body(first).await;
    tokio::time::advance(Duration::from_secs(3601)).await;
    let stale = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(stale.headers()["x-proa-page-cache"], "STALE");
    assert_eq!(body(stale).await, first_body);
    started.notified().await;
    for _ in 0..8 {
        assert_eq!(
            app.clone().oneshot(request("/")).await.unwrap().headers()["x-proa-page-cache"],
            "STALE"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    release.notify_one();
    tokio::task::yield_now().await;
    let refreshed = app.oneshot(request("/")).await.unwrap();
    assert_eq!(refreshed.status(), StatusCode::OK);
    assert_eq!(refreshed.headers()["x-proa-page-cache"], "HIT");
    assert_ne!(body(refreshed).await, first_body);
}

#[tokio::test(start_paused = true)]
async fn failed_refresh_keeps_stale_until_the_stale_window_expires() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let service = PageCacheLayer::new("build")
        .revalidate(Duration::from_secs(10))
        .stale_while_revalidate(Duration::from_secs(20))
        .layer(service_fn(move |_| {
            let generation = count.fetch_add(1, Ordering::SeqCst);
            async move {
                Ok::<_, Infallible>(if generation == 0 {
                    "initial".into_response()
                } else {
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                })
            }
        }));
    let initial = body(service.clone().oneshot(request("/")).await.unwrap()).await;
    tokio::time::advance(Duration::from_secs(11)).await;
    assert_eq!(
        body(service.clone().oneshot(request("/")).await.unwrap()).await,
        initial
    );
    tokio::task::yield_now().await;
    assert_eq!(
        body(service.clone().oneshot(request("/")).await.unwrap()).await,
        initial
    );
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(20)).await;
    assert_eq!(
        service.oneshot(request("/")).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn limits_evict_old_entries_and_pass_oversized_bodies_through() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = counter_app(
        PageCacheLayer::new("build").with_limits(1, 100),
        calls.clone(),
    );
    for uri in ["/?a", "/?b", "/?a"] {
        assert_eq!(
            app.clone().oneshot(request(uri)).await.unwrap().headers()["x-proa-page-cache"],
            "MISS"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let app = counter_app(
        PageCacheLayer::new("build").with_limits(1, 0),
        calls.clone(),
    );
    for expected in ["3", "4"] {
        let response = app.clone().oneshot(request("/")).await.unwrap();
        assert_eq!(response.headers()["x-proa-page-cache"], "BYPASS");
        assert_eq!(body(response).await, expected);
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_releases_the_miss_lock_for_later_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let service = PageCacheLayer::new("build")
        .with_timeout(Duration::from_secs(1))
        .layer(service_fn(move |_| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    std::future::pending::<()>().await;
                }
                Ok::<_, Infallible>(Response::new(Body::from("recovered")))
            }
        }));
    assert_eq!(
        service
            .clone()
            .oneshot(request("/"))
            .await
            .unwrap()
            .status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        body(service.oneshot(request("/")).await.unwrap()).await,
        "recovered"
    );
}

#[tokio::test]
async fn streaming_bodies_bypass_without_losing_chunks() {
    let service = PageCacheLayer::new("build").layer(service_fn(|_| async {
        let chunks = futures::stream::iter([Ok::<_, Infallible>("first"), Ok("second")]);
        Ok::<_, Infallible>(Response::new(Body::from_stream(chunks)))
    }));
    let response = service.oneshot(request("/")).await.unwrap();
    assert_eq!(response.headers()["x-proa-page-cache"], "BYPASS");
    assert_eq!(body(response).await, "firstsecond");
}

#[tokio::test]
async fn downstream_freshness_headers_cannot_hide_a_deployment() {
    let service = PageCacheLayer::new("build").layer(service_fn(|_| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("cache-control", "public, max-age=31536000")
                .header("cloudflare-cdn-cache-control", "max-age=31536000")
                .header("surrogate-control", "max-age=31536000")
                .body(Body::from("page"))
                .unwrap(),
        )
    }));
    for _ in 0..2 {
        let response = service.clone().oneshot(request("/")).await.unwrap();
        for name in [
            "cache-control",
            "cdn-cache-control",
            "cloudflare-cdn-cache-control",
            "vercel-cdn-cache-control",
        ] {
            assert_eq!(
                response.headers()[name],
                HeaderValue::from_static("public, no-cache")
            );
        }
        assert!(!response.headers().contains_key("surrogate-control"));
    }
}
