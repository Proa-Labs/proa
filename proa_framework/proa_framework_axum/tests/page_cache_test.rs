// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    convert::Infallible,
    future::Future,
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
use proa_core::{
    ComponentRole, DataLoader, HeadConfig, MdRenderSync, RenderOutcome, WebContext, WebRender,
    WriteBuf, WriteError,
};
use proa_framework_axum::{
    explicit_markdown, router_from_spec, FrameworkBuilder, PageCache, PageCacheLayer, Route,
    RouteResponse,
};
use proa_macros::{html, html_send, html_sync, md_sync};
use proa_stream::Suspense;
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
    let app = FrameworkBuilder::new().with_dynamic_routes(routes).build();

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
async fn isr_releases_the_lock_when_stream_preparation_stalls() {
    let generations = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let routes = Router::new()
        .route(
            "/",
            get({
                let generations = generations.clone();
                let started = started.clone();
                move || {
                    let generation = generations.fetch_add(1, Ordering::SeqCst);
                    let started = started.clone();
                    async move {
                        if generation == 1 {
                            return RouteResponse::ssr_stream_send(html_send! {
                                {proa_core::await_send_component(async move {
                                    started.notify_one();
                                    std::future::pending::<()>().await;
                                    html_send! { <p>"unreachable"</p> }
                                })}
                            });
                        }
                        RouteResponse::ssr(generation)
                    }
                }
            }),
        )
        .route_layer(
            PageCacheLayer::new("build")
                .revalidate(Duration::from_secs(1))
                .with_timeout(Duration::from_millis(50)),
        );
    let app = FrameworkBuilder::new().with_dynamic_routes(routes).build();

    assert_eq!(
        body(app.clone().oneshot(request("/")).await.unwrap()).await,
        "0"
    );
    tokio::time::advance(Duration::from_secs(2)).await;

    let stale = app.clone().oneshot(request("/")).await.unwrap();
    assert_eq!(cache_status(&stale), "STALE");
    assert_eq!(body(stale).await, "0");
    started.notified().await;
    tokio::time::advance(Duration::from_millis(100)).await;

    let mut fresh = request("/");
    fresh
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    let response = tokio::time::timeout(Duration::from_secs(1), app.oneshot(fresh))
        .await
        .expect("stalled refresh must release the cache lock")
        .unwrap();
    assert_eq!(cache_status(&response), "MISS");
    assert_eq!(body(response).await, "2");
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

/// Resolves after the streamed shell, so a stream shows its fallback first.
struct Slow;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Slow {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            WriteBuf::extend_str(&mut cx.out, "<p>resolved</p>")
        })
    }
}

/// A buffered page that chooses its own cache policy while rendering.
struct SelfCachingPage;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for SelfCachingPage {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            assert!(cx.response_mut().insert_header(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=30"),
            ));
            WriteBuf::extend_str(&mut cx.out, "<p>page</p>")
        })
    }
}

/// A managed-head page whose head hook chooses the cache policy.
struct HeadPolicyPage;

impl WebRender for HeadPolicyPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        assert!(cx.response_mut().insert_header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=86400"),
        ));
        Ok(())
    }

    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <html>
                <head>{cx.head()}</head>
                <body></body>
            </html>
        }
        .render(cx)
    }
}

struct Notes;

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Notes {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # Notes
            "###
        }
        .render_md(cx)
    }
}

fn streamed_page() -> RouteResponse {
    RouteResponse::ssr_stream(html! {
        <main>
            {Suspense::new(Slow, html_sync! {
                <p>"loading"</p>
            })}
        </main>
    })
}

fn cache_control(response: &Response) -> Option<&str> {
    response
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|value| value.to_str().unwrap())
}

async fn assert_cache_control(app: &Router, cases: &[(&str, Option<&str>)]) {
    for (uri, expected) in cases {
        let response = app.clone().oneshot(request(uri)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(cache_control(&response), *expected, "{uri}");
        body(response).await;
    }
}

#[tokio::test]
async fn framework_documents_without_a_policy_are_private_no_store() {
    let routes = Router::new()
        .route(
            "/",
            get(|| async { RouteResponse::ssr(proa_core::text::text("page")) }),
        )
        .route("/async", get(|| async { RouteResponse::ssr_async(Slow) }))
        .route("/stream", get(|| async { streamed_page() }))
        .route(
            "/index.md",
            get(|| async { explicit_markdown(Notes).unwrap() }),
        )
        .route(
            "/own",
            get(|| async {
                (
                    [(header::CACHE_CONTROL, "public, max-age=60")],
                    RouteResponse::ssr_async(Slow),
                )
            }),
        )
        .route(
            "/page",
            get(|| async { RouteResponse::ssr_async(SelfCachingPage) }),
        )
        .route(
            "/edge",
            get(|| async {
                (
                    [("cdn-cache-control", "max-age=60")],
                    RouteResponse::ssr(proa_core::text::text("page")),
                )
            }),
        )
        .route("/plain", get(|| async { "ok" }));
    let app = FrameworkBuilder::new().with_dynamic_routes(routes).build();
    assert_cache_control(
        &app,
        &[
            ("/", Some("private, no-store")),
            ("/async", Some("private, no-store")),
            ("/stream", Some("private, no-store")),
            ("/index.md", Some("private, no-store")),
            ("/own", Some("public, max-age=60")),
            ("/page", Some("public, max-age=30")),
            ("/edge", None),
            ("/plain", None),
        ],
    )
    .await;
}

#[tokio::test]
async fn head_hook_cache_policies_survive_the_default() {
    let routes = Router::new()
        .route(
            "/async",
            get(|| async { RouteResponse::ssr_async(HeadPolicyPage) }),
        )
        .route(
            "/stream",
            get(|| async { RouteResponse::ssr_stream(HeadPolicyPage) }),
        );
    let app = FrameworkBuilder::new()
        .with_head(HeadConfig::default())
        .with_dynamic_routes(routes)
        .build();
    assert_cache_control(
        &app,
        &[
            ("/async", Some("public, max-age=86400")),
            ("/stream", Some("public, max-age=86400")),
        ],
    )
    .await;
}

fn page_cached_app(route: Route<()>) -> Router {
    let route = route.with_page_cache(PageCache::new().revalidate(Duration::from_secs(60)));
    let (routes, metadata) = router_from_spec(vec![route]);
    FrameworkBuilder::new()
        .with_dynamic_routes_and_metadata(routes, metadata)
        .build()
}

fn assert_cdn_policy(response: &Response) {
    assert_eq!(
        cache_control(response),
        Some("public, max-age=0, s-maxage=60, stale-while-revalidate=86400")
    );
    assert_eq!(
        response.headers()["cdn-cache-control"],
        "public, max-age=60, stale-while-revalidate=86400"
    );
}

async fn page_cached_stream_renders_in_order_for_the_cdn() {
    let app = page_cached_app(Route::endpoint("/", get(|| async { streamed_page() })));
    let response = app.clone().oneshot(request("/")).await.unwrap();
    assert_cdn_policy(&response);
    let html = body(response).await;
    assert!(html.contains("<p>resolved</p>"), "{html}");
    assert!(!html.contains("loading"), "{html}");
    assert!(!html.contains("proa-tpl-"), "{html}");

    // A request the CDN must not store keeps streaming.
    let mut private = request("/");
    private
        .headers_mut()
        .insert(header::COOKIE, HeaderValue::from_static("session=secret"));
    let response = app.oneshot(private).await.unwrap();
    assert_eq!(cache_control(&response), Some("private, no-store"));
    let html = body(response).await;
    assert!(
        html.contains("loading") && html.contains("proa-tpl-"),
        "{html}"
    );
}

#[tokio::test]
async fn page_cached_stream_renders_in_order_for_the_cdn_on_a_current_thread_runtime() {
    page_cached_stream_renders_in_order_for_the_cdn().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_cached_stream_renders_in_order_for_the_cdn_on_a_multi_thread_runtime() {
    page_cached_stream_renders_in_order_for_the_cdn().await;
}

#[tokio::test]
async fn page_cache_policy_replaces_the_page_policy_on_a_deferred_render() {
    let app = page_cached_app(Route::endpoint(
        "/",
        get(|| async { RouteResponse::ssr_async(SelfCachingPage) }),
    ));
    let response = app.oneshot(request("/")).await.unwrap();
    assert_cdn_policy(&response);
    assert_eq!(body(response).await, "<p>page</p>");
}

#[tokio::test]
async fn origin_page_cache_stores_a_streamed_document() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let routes = Router::new()
        .route(
            "/",
            get(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async { streamed_page() }
            }),
        )
        .route_layer(PageCacheLayer::new("build"));
    let app = FrameworkBuilder::new().with_dynamic_routes(routes).build();
    for expected in ["MISS", "HIT"] {
        let response = app.clone().oneshot(request("/")).await.unwrap();
        assert_eq!(response.headers()["x-proa-page-cache"], expected);
        assert_eq!(cache_control(&response), Some("public, no-cache"));
        let html = body(response).await;
        assert!(
            html.contains("<p>resolved</p>") && !html.contains("loading"),
            "{html}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Resolves after `.0` milliseconds.
struct Sleeps(u64);

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Sleeps {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(Duration::from_millis(self.0)).await;
            WriteBuf::extend_str(&mut cx.out, "<p>resolved</p>")
        })
    }
}

/// A shell, then a Suspense boundary that resolves after `ms`.
fn slow_streamed_page(ms: u64) -> RouteResponse {
    RouteResponse::ssr_stream(html! {
        <main>
            {Suspense::new(Sleeps(ms), html_sync! {
                <p>"loading"</p>
            })}
        </main>
    })
}

const STREAMED_PAGE: &str = "<main><p>resolved</p></main>";

fn cache_status(response: &Response) -> &str {
    response.headers()["x-proa-page-cache"].to_str().unwrap()
}

/// A route under an origin `PageCacheLayer`, counting handler calls.
fn origin_cached(
    layer: PageCacheLayer,
    page: fn() -> axum::response::Response,
) -> (Router, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let routes = Router::new()
        .route(
            "/",
            get(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async move { page() }
            }),
        )
        .route_layer(layer);
    (
        FrameworkBuilder::new().with_dynamic_routes(routes).build(),
        calls,
    )
}

async fn an_outer_page_cache_stores_framework_documents() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (sync, streamed) = (calls.clone(), calls.clone());
    let routes = Router::new()
        .route(
            "/",
            get(move || {
                sync.fetch_add(1, Ordering::SeqCst);
                async { RouteResponse::ssr(proa_core::text::text("page")) }
            }),
        )
        .route(
            "/stream",
            get(move || {
                streamed.fetch_add(1, Ordering::SeqCst);
                async { streamed_page() }
            }),
        );
    let app = FrameworkBuilder::new()
        .with_dynamic_routes(routes)
        .build()
        .layer(PageCacheLayer::new("build"));
    for (uri, page) in [("/", "page"), ("/stream", STREAMED_PAGE)] {
        for expected in ["MISS", "HIT"] {
            let response = app.clone().oneshot(request(uri)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(cache_status(&response), expected, "{uri}");
            assert_eq!(cache_control(&response), Some("public, no-cache"), "{uri}");
            assert_eq!(body(response).await, page, "{uri}");
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // A request the cache passes still gets the framework default.
    let mut private = request("/");
    private
        .headers_mut()
        .insert(header::COOKIE, HeaderValue::from_static("session=secret"));
    let response = app.oneshot(private).await.unwrap();
    assert_eq!(cache_status(&response), "BYPASS");
    assert_eq!(cache_control(&response), Some("private, no-store"));
}

#[tokio::test]
async fn an_outer_page_cache_stores_framework_documents_on_a_current_thread_runtime() {
    an_outer_page_cache_stores_framework_documents().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_outer_page_cache_stores_framework_documents_on_a_multi_thread_runtime() {
    an_outer_page_cache_stores_framework_documents().await;
}

async fn streams_outlast_the_page_cache_timeout() {
    // The boundary resolves long after the cache stops waiting for it.
    let layer = PageCacheLayer::new("build").with_timeout(Duration::from_millis(50));
    let (app, calls) = origin_cached(layer, || slow_streamed_page(300).into_response());
    for _ in 0..2 {
        let started = std::time::Instant::now();
        let response = app.clone().oneshot(request("/")).await.unwrap();
        let head = started.elapsed();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(cache_status(&response), "BYPASS");
        assert_eq!(cache_control(&response), Some("private, no-store"));
        assert!(head < Duration::from_millis(250), "head after {head:?}");
        assert_eq!(body(response).await, STREAMED_PAGE);
    }
    // A page that outlasts the timeout is not stored.
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn streams_outlast_the_page_cache_timeout_on_a_current_thread_runtime() {
    streams_outlast_the_page_cache_timeout().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_outlast_the_page_cache_timeout_on_a_multi_thread_runtime() {
    streams_outlast_the_page_cache_timeout().await;
}

async fn cdn_cached_streams_outlast_the_request_timeout() {
    let route = Route::endpoint("/", get(|| async { slow_streamed_page(300) }))
        .with_page_cache(PageCache::new().revalidate(Duration::from_secs(60)));
    let (routes, metadata) = router_from_spec(vec![route]);
    let app = FrameworkBuilder::new()
        .with_request_timeout(Duration::from_millis(100))
        .with_dynamic_routes_and_metadata(routes, metadata)
        .build();
    let response = app.oneshot(request("/")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_cdn_policy(&response);
    // The CDN stores the complete page; there is no fallback to replace.
    assert_eq!(body(response).await, STREAMED_PAGE);
}

#[tokio::test]
async fn cdn_cached_streams_outlast_the_request_timeout_on_a_current_thread_runtime() {
    cdn_cached_streams_outlast_the_request_timeout().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cdn_cached_streams_outlast_the_request_timeout_on_a_multi_thread_runtime() {
    cdn_cached_streams_outlast_the_request_timeout().await;
}

/// A managed-head page whose head hook keeps every cache out.
struct PrivateHeadPage;

impl WebRender for PrivateHeadPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        assert!(cx.response_mut().insert_header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        ));
        Ok(())
    }

    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <html>
                <head>{cx.head()}</head>
                <body>{Suspense::new(Sleeps(5), html_sync! { <p>"loading"</p> })}</body>
            </html>
        }
        .render(cx)
    }
}

#[tokio::test]
async fn cdn_policy_reads_a_streamed_head_hook_before_deciding() {
    let route = Route::endpoint(
        "/",
        get(|| async { RouteResponse::ssr_stream(PrivateHeadPage) }),
    )
    .with_page_cache(PageCache::new());
    let (routes, metadata) = router_from_spec(vec![route]);
    let app = FrameworkBuilder::new()
        .with_head(HeadConfig::default())
        .with_dynamic_routes_and_metadata(routes, metadata)
        .build();
    let response = app.oneshot(request("/")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for name in ["cache-control", "cdn-cache-control", "surrogate-control"] {
        assert_eq!(response.headers()[name], "private, no-store", "{name}");
    }
    assert!(body(response).await.contains("<p>resolved</p>"));
}

async fn oversized_streams_are_delivered_and_not_stored() {
    let layer = PageCacheLayer::new("build").with_limits(8, 16);
    let (app, calls) = origin_cached(layer, || slow_streamed_page(5).into_response());
    for _ in 0..2 {
        let response = app.clone().oneshot(request("/")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(cache_status(&response), "BYPASS");
        assert_eq!(body(response).await, STREAMED_PAGE);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // Within the limit, the same page is stored once complete.
    let (app, calls) = origin_cached(PageCacheLayer::new("build"), || {
        slow_streamed_page(5).into_response()
    });
    for expected in ["MISS", "HIT"] {
        let response = app.clone().oneshot(request("/")).await.unwrap();
        assert_eq!(cache_status(&response), expected);
        assert_eq!(body(response).await, STREAMED_PAGE);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn oversized_streams_are_delivered_and_not_stored_on_a_current_thread_runtime() {
    oversized_streams_are_delivered_and_not_stored().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_streams_are_delivered_and_not_stored_on_a_multi_thread_runtime() {
    oversized_streams_are_delivered_and_not_stored().await;
}

/// Five concurrent requests: the slowest response head, and the time until
/// every body has arrived.
async fn five_at_once(app: &Router) -> (Duration, Duration) {
    let started = std::time::Instant::now();
    let requests = (0..5).map(|_| {
        let app = app.clone();
        tokio::spawn(async move {
            let response = app.oneshot(request("/")).await.unwrap();
            let head = started.elapsed();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(cache_status(&response), "BYPASS");
            assert_eq!(body(response).await, STREAMED_PAGE);
            head
        })
    });
    let heads = futures::future::join_all(requests).await;
    let slowest_head = heads.into_iter().map(Result::unwrap).max().unwrap();
    (slowest_head, started.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncacheable_streams_do_not_wait_for_each_other() {
    // Serialized behind the miss lock, five 200 ms renders take 1 s.
    let layer = PageCacheLayer::new("build").with_limits(8, 16);
    let (app, _) = origin_cached(layer, || slow_streamed_page(200).into_response());
    let (_, all) = five_at_once(&app).await;
    assert!(all < Duration::from_millis(600), "oversized: {all:?}");

    // A head the cache cannot store bypasses at once and streams.
    let (app, _) = origin_cached(PageCacheLayer::new("build"), || {
        ([(header::SET_COOKIE, "visit=1")], slow_streamed_page(200)).into_response()
    });
    let (head, all) = five_at_once(&app).await;
    assert!(
        head < Duration::from_millis(150),
        "Set-Cookie head: {head:?}"
    );
    assert!(all < Duration::from_millis(600), "Set-Cookie: {all:?}");
}
