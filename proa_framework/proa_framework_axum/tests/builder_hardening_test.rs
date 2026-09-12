// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the security and DoS hardening that
//! `FrameworkBuilder::build` applies to the built router: cross-origin
//! protection (default on), default security response headers, the
//! request timeout (default on, streaming-safe), and the opt-in
//! concurrency limit and catch-panic layers.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, Request, StatusCode},
    routing::{get, post},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use proa_core::{
    await_component, to_async, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteBuf,
    WriteError,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{
    router_from_spec, FrameworkBuilder, Route, RouteResponse, SecurityHeadersConfig,
};
use proa_macros::{html, html_sync};
use proa_stream::Suspense;
use tower::ServiceExt;

fn empty_manifest() -> IslandManifest {
    IslandManifest {
        islands: HashMap::new(),
        assets: Default::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    }
}

fn builder_with_routes(routes: Vec<Route<()>>) -> FrameworkBuilder {
    let (router, computed_metadata) = router_from_spec(routes);
    FrameworkBuilder::new(empty_manifest())
        .with_dynamic_routes_and_metadata(router, computed_metadata)
}

fn assert_default_security_headers(headers: &HeaderMap) {
    assert_eq!(
        headers
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|h| h.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        headers
            .get(header::X_FRAME_OPTIONS)
            .and_then(|h| h.to_str().ok()),
        Some("DENY")
    );
    assert_eq!(
        headers
            .get(header::REFERRER_POLICY)
            .and_then(|h| h.to_str().ok()),
        Some("strict-origin-when-cross-origin")
    );
}

fn decoded_boundary_payloads(document: &str) -> Vec<String> {
    const TEMPLATE_OPEN: &str = "<template id=\"proa-tpl-";
    const ENCODING_END: &str = "\" data-proa-encoding=\"base64-v1\">";
    const TEMPLATE_CLOSE: &str = "</template>";

    let mut document = document;
    let mut payloads = Vec::new();
    while let Some(template_at) = document.find(TEMPLATE_OPEN) {
        let template = &document[template_at..];
        let payload_at = template
            .find(ENCODING_END)
            .expect("boundary template must declare its transport encoding")
            + ENCODING_END.len();
        let payload_end = template[payload_at..]
            .find(TEMPLATE_CLOSE)
            .expect("boundary template must close before its swap script");
        let encoded = &template[payload_at..payload_at + payload_end];
        payloads.push(
            String::from_utf8(STANDARD.decode(encoded).expect("valid boundary base64"))
                .expect("rendered boundary must be UTF-8"),
        );
        document = &template[payload_at + payload_end + TEMPLATE_CLOSE.len()..];
    }
    payloads
}

struct ReadySection {
    label: &'static str,
}

impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ReadySection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section class="ready">{self.label}</section>
            },
            cx,
        )
    }
}

struct DelayedSection {
    label: &'static str,
    delay_ms: u64,
}

impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for DelayedSection
where
    ReadySection: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let label = self.label;
        let delay_ms = self.delay_ms;
        await_component(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            to_async(ReadySection { label })
        })
        .render(cx)
    }
}

fn simple_routes() -> Vec<Route<()>> {
    vec![
        Route::endpoint("/hello", get(|| async { "hello" })),
        Route::endpoint("/submit", post(|| async { "submitted" })),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn built_router_streams_ssr_with_default_request_timeout() {
    // Defaults include the 30s request timeout; the streamed body must
    // still arrive in full because the timeout only bounds producing
    // the response head, not draining the body.
    let routes = vec![Route::endpoint(
        "/stream",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        DelayedSection { label: "slow ready", delay_ms: 40 },
                        html_sync! { <p class="fallback">"loading"</p> },
                    )}
                </main>
            })
        }),
    )];

    let app = builder_with_routes(routes).build();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_default_security_headers(response.headers());

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("<h1>stream</h1>"));
    assert!(html.contains("loading"));
    assert!(
        decoded_boundary_payloads(&html)
            .iter()
            .any(|payload| payload.contains("slow ready")),
        "delayed section should arrive in an encoded boundary payload: {html}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_timeout_returns_408_for_stalled_handlers() {
    let routes = vec![Route::endpoint(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            "too late"
        }),
    )];

    let app = builder_with_routes(routes)
        .with_request_timeout(Duration::from_millis(50))
        .build();

    let response = app
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_default_security_headers(response.headers());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_timeout_can_be_disabled() {
    let routes = vec![Route::endpoint(
        "/slowish",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            "made it"
        }),
    )];

    let app = builder_with_routes(routes)
        .without_request_timeout()
        .build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/slowish")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_origin_protection_is_on_by_default() {
    let app = builder_with_routes(simple_routes()).build();

    // GET always passes, even cross-site.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Browser cross-site POST is rejected.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/submit")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Rejections still carry the security headers.
    assert_eq!(
        response
            .headers()
            .get(header::X_FRAME_OPTIONS)
            .and_then(|h| h.to_str().ok()),
        Some("DENY")
    );

    // Non-browser POST (no Origin / Sec-Fetch-Site) passes.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/submit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_origin_protection_respects_trusted_origins_and_disable() {
    let app = builder_with_routes(simple_routes())
        .with_trusted_origin("https://app.example.com")
        .build();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/submit")
                .header("origin", "https://app.example.com")
                .header("host", "api.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let app = builder_with_routes(simple_routes())
        .with_cross_origin_protection(false)
        .build();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/submit")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn built_router_sets_default_security_headers() {
    let app = builder_with_routes(simple_routes()).build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let headers = response.headers();
    assert_eq!(headers.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
    assert_eq!(
        headers.get(header::REFERRER_POLICY).unwrap(),
        "strict-origin-when-cross-origin"
    );
    assert_eq!(
        headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert!(headers.get(header::STRICT_TRANSPORT_SECURITY).is_none());
    assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opt_in_hsts_and_csp_headers_are_emitted() {
    let app = builder_with_routes(simple_routes())
        .with_security_headers(
            SecurityHeadersConfig::new()
                .with_hsts_max_age_secs(63072000)
                .with_content_security_policy("default-src 'self'"),
        )
        .build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let headers = response.headers();
    assert_eq!(
        headers.get(header::STRICT_TRANSPORT_SECURITY).unwrap(),
        "max-age=63072000"
    );
    assert_eq!(
        headers.get(header::CONTENT_SECURITY_POLICY).unwrap(),
        "default-src 'self'"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frame_options_can_be_disabled_for_embeddable_apps() {
    use proa_framework_axum::FrameOptions;

    let app = builder_with_routes(simple_routes())
        .with_security_headers(
            SecurityHeadersConfig::new().with_frame_options(FrameOptions::Disabled),
        )
        .build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.headers().get(header::X_FRAME_OPTIONS).is_none());
}

async fn exploding_handler() -> &'static str {
    panic!("handler bug")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catch_panic_turns_handler_panics_into_500() {
    let routes = vec![Route::endpoint("/explode", get(exploding_handler))];

    let app = builder_with_routes(routes).with_catch_panic(true).build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/explode")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_default_security_headers(response.headers());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrency_limit_still_serves_sequential_requests() {
    let app = builder_with_routes(simple_routes())
        .with_concurrency_limit(1)
        .build();

    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/hello")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
