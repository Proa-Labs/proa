// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for `FrameworkBuilder::with_strict_csp`: the nonce the
//! layer puts in the `Content-Security-Policy` header must be the same one
//! that lands on every script tag Proa emits, including the out-of-order
//! streaming bootstrap and its boundary-swap chunks. A mismatch there is
//! silent in tests but fatal in a browser, so these assert the pairing on a
//! fully built router rather than on the layer alone.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    routing::get,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use proa_core::{
    await_component, to_async, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteBuf,
    WriteError,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{
    router_from_spec, CspPolicy, FrameworkBuilder, Route, RouteResponse, SecurityHeadersConfig,
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

struct ReadySection;

impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ReadySection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section class="ready">"resolved"</section>
            },
            cx,
        )
    }
}

struct DelayedSection;

impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for DelayedSection
where
    ReadySection: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        await_component(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            to_async(ReadySection)
        })
        .render(cx)
    }
}

/// A streaming route: the response carries the inline streaming bootstrap
/// plus one boundary-swap script for the deferred section.
fn streaming_routes() -> Vec<Route<()>> {
    vec![Route::endpoint(
        "/stream",
        get(|| async {
            RouteResponse::ssr_stream(html! {
                <main>
                    <h1>"stream"</h1>
                    {Suspense::new(
                        DelayedSection,
                        html_sync! { <p class="fallback">"loading"</p> },
                    )}
                </main>
            })
        }),
    )]
}

/// Pull the `'nonce-…'` value out of a policy's `script-src` directive.
fn nonce_from_policy(policy: &str) -> &str {
    let start = policy
        .find("'nonce-")
        .expect("script-src carries a nonce source")
        + "'nonce-".len();
    let rest = &policy[start..];
    &rest[..rest.find('\'').expect("nonce source is quoted")]
}

/// Every `<script` start tag in the document, as a slice up to its `>`.
fn script_tags(html: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<script") {
        rest = &rest[at..];
        let end = rest.find('>').expect("script tag is closed");
        tags.push(&rest[..end]);
        rest = &rest[end..];
    }
    tags
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamed_script_tags_carry_the_nonce_from_the_response_header() {
    let app = builder_with_routes(streaming_routes())
        .with_strict_csp()
        .build();

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
    let policy = response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .expect("with_strict_csp sends the policy")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(policy.contains("'strict-dynamic'"), "policy: {policy}");
    assert!(
        !policy.contains("script-src 'self' 'unsafe-inline'"),
        "policy: {policy}"
    );
    let nonce = nonce_from_policy(&policy).to_owned();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    // The deferred boundary resolved into the opaque base64 transport, so
    // the bootstrap and at least one swap script are both in the document.
    let resolved_payload = STANDARD.encode(r#"<section class="ready">resolved</section>"#);
    assert!(
        html.contains(&format!(
            r#"data-proa-encoding="base64-v1">{resolved_payload}</template>"#
        )),
        "body: {html}"
    );
    let tags = script_tags(&html);
    assert!(
        tags.len() >= 2,
        "expected a bootstrap and a boundary swap script, got {}: {html}",
        tags.len()
    );
    for tag in tags {
        assert!(
            tag.contains(&format!("nonce=\"{nonce}\"")),
            "script tag is missing the response nonce: {tag}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_response_gets_a_distinct_nonce() {
    let app = builder_with_routes(streaming_routes())
        .with_strict_csp()
        .build();

    let mut seen = Vec::new();
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        seen.push(nonce_from_policy(&policy).to_owned());
    }

    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3, "nonces must not repeat across responses");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_csp_header_without_the_opt_in() {
    let app = builder_with_routes(streaming_routes()).build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn report_only_sends_the_report_only_header_and_nothing_else() {
    let app = builder_with_routes(streaming_routes())
        .with_csp_policy(CspPolicy::strict().report_only(true))
        .build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .is_none());
    assert!(response
        .headers()
        .get("content-security-policy-report-only")
        .is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_responses_from_outer_layers_still_carry_the_policy() {
    let routes = vec![Route::endpoint(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            "too late"
        }),
    )];

    let app = builder_with_routes(routes)
        .with_strict_csp()
        .with_request_timeout(Duration::from_millis(50))
        .build();

    let response = app
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_configured_static_policy_takes_precedence() {
    // Documented conflict resolution: the static policy is written by the
    // inner layer and the nonce layer does not overwrite it. The builder
    // warns at startup; this pins the behavior the warning describes.
    let app = builder_with_routes(streaming_routes())
        .with_strict_csp()
        .with_security_headers(
            SecurityHeadersConfig::default().with_content_security_policy("default-src 'self'"),
        )
        .build();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap(),
        "default-src 'self'"
    );
}
