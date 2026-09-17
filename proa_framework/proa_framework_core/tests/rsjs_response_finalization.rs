// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Response-path integration coverage for automatic rsjs finalization.
//!
//! The components below deliberately use the low-level recording bridges
//! emitted by RSJS-analyzed components. The test only invokes normal `RouteResponse`
//! finalization; it never calls the page planner or appends runtime tags
//! itself.

use std::collections::HashMap;
use std::future::Future;
use std::task::Poll;
use std::time::Duration;

use futures::executor::block_on;
use futures::StreamExt;
use proa_core::{
    CspNonce, DataLoader, ErrorBoundary, NoLoader, RenderBuffer, RenderMode, RenderOutcome,
    WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_framework_core::{
    FrameworkBody, FrameworkRequestContext, IslandManifest, ManifestAssets, RouteResponse,
};
use proa_stream::Suspense;

const BASIC_ISLAND_ID: &str = "framework_auto_basic";
const QUERY_ISLAND_ID: &str = "framework_auto_query";
const CHILD_ISLAND_ID: &str = "framework_auto_child";
const LEAK_ISLAND_ID: &str = "framework_auto_leak";
const RECOVERY_ISLAND_ID: &str = "framework_auto_recovery";
const NONCE: &str = "ZnJhbWV3b3JrLWF1dG8tcnNqcw";

static BASIC_ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: BASIC_ISLAND_ID,
    struct_name: "FrameworkAutoBasic",
    js_source: "export function hydrate() {}",
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: false,
    uses_mutations: false,
};

static QUERY_ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: QUERY_ISLAND_ID,
    struct_name: "FrameworkAutoQuery",
    js_source: "export function hydrate() {}",
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: true,
    uses_mutations: false,
};

static CHILD_ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: CHILD_ISLAND_ID,
    struct_name: "FrameworkAutoChild",
    js_source: "export function hydrate() {}",
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: false,
    uses_mutations: false,
};

static LEAK_ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: LEAK_ISLAND_ID,
    struct_name: "FrameworkAutoLeak",
    js_source: "export function hydrate() {}",
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: false,
    uses_mutations: false,
};

static RECOVERY_ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: RECOVERY_ISLAND_ID,
    struct_name: "FrameworkAutoRecovery",
    js_source: "export function hydrate() {}",
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: false,
    uses_mutations: false,
};

#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static BASIC_ISLAND_REGISTRATION: &'static rsjs::IslandManifestEntry = &BASIC_ISLAND;

#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static QUERY_ISLAND_REGISTRATION: &'static rsjs::IslandManifestEntry = &QUERY_ISLAND;

#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static CHILD_ISLAND_REGISTRATION: &'static rsjs::IslandManifestEntry = &CHILD_ISLAND;

#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static LEAK_ISLAND_REGISTRATION: &'static rsjs::IslandManifestEntry = &LEAK_ISLAND;

#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static RECOVERY_ISLAND_REGISTRATION: &'static rsjs::IslandManifestEntry = &RECOVERY_ISLAND;

fn empty_react_manifest() -> IslandManifest {
    IslandManifest {
        islands: HashMap::new(),
        assets: ManifestAssets::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    }
}

fn finalize(route: RouteResponse<NoLoader>, request: &FrameworkRequestContext) -> String {
    let manifest = empty_react_manifest();
    let response = block_on(route.into_framework_response(&manifest, request))
        .expect("route response should finalize");
    let FrameworkBody::Full(bytes) = response.body else {
        panic!("buffered route should produce a full response body");
    };
    String::from_utf8(bytes.to_vec()).expect("framework response should be UTF-8")
}

fn finalize_stream(
    route: RouteResponse<NoLoader>,
    request: &FrameworkRequestContext,
) -> Vec<String> {
    block_on(finalize_stream_async(route, request))
}

async fn finalize_stream_async(
    route: RouteResponse<NoLoader>,
    request: &FrameworkRequestContext,
) -> Vec<String> {
    let manifest = empty_react_manifest();
    let response = route
        .into_framework_response(&manifest, request)
        .await
        .expect("streaming route response should finalize");
    let FrameworkBody::Stream(proa_framework_core::FrameworkStreamFactory::Local(factory)) =
        response.body
    else {
        panic!("streaming route should produce a stream factory");
    };

    let mut stream = factory();
    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.expect("streaming response should complete without transport errors");
        chunks.push(
            String::from_utf8(bytes.to_vec())
                .expect("every framework streaming chunk should be UTF-8"),
        );
    }
    chunks
}

fn assert_exactly_once(html: &str, needle: &str) {
    assert_eq!(
        html.matches(needle).count(),
        1,
        "expected exactly one `{needle}` in:\n{html}"
    );
}

fn assert_every_script_has_nonce(html: &str, nonce: &str) {
    let expected = format!(r#"nonce="{nonce}""#);
    let mut rest = html;
    let mut script_count = 0;
    while let Some(start) = rest.find("<script") {
        let script = &rest[start..];
        let end = script
            .find('>')
            .expect("generated script opening tag should be complete");
        let opening_tag = &script[..=end];
        assert!(
            opening_tag.contains(&expected),
            "script opening tag did not carry the request nonce: {opening_tag}"
        );
        script_count += 1;
        rest = &script[end + 1..];
    }
    assert!(script_count > 0, "fixture should emit at least one script");
}

struct ServerOnly;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerOnly {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_static(b"<main data-server-only></main>")
    }
}

struct BasicIsland;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BasicIsland {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // Repeated component instances must still produce one page asset graph.
        cx.rsjs_island_used(BASIC_ISLAND_ID);
        cx.rsjs_island_used(BASIC_ISLAND_ID);
        cx.out.extend_static(b"<main data-basic-island></main>")
    }
}

struct BasicDocument;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BasicDocument {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.rsjs_island_used(BASIC_ISLAND_ID);
        cx.out.extend_static(
            b"<!DOCTYPE html><html><body><main data-basic-document></main></body></html>",
        )
    }
}

struct QueryIsland;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for QueryIsland {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.rsjs_island_used(QUERY_ISLAND_ID);
        cx.rsjs_query_seed_json(
            r#"["framework","query"]"#,
            r#"{"key":["framework","query"],"data":{"ready":true},"data_updated_at":0}"#,
        );
        cx.out.extend_static(b"<main data-query-island></main>")
    }
}

struct AsyncIsland;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for AsyncIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            std::future::ready(()).await;
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.out.extend_static(b"<main data-async-island></main>")
        })
    }
}

async fn yield_once() {
    let mut yielded = false;
    futures::future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

struct SuccessfulDeferredChild;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for SuccessfulDeferredChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            yield_once().await;
            // The shell id is repeated here to verify page-wide dedupe across
            // the shell and a separately rendered boundary context.
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.rsjs_island_used(CHILD_ISLAND_ID);
            cx.rsjs_island_used(CHILD_ISLAND_ID);
            cx.out
                .extend_static(b"<section data-deferred-child></section>")
        })
    }
}

struct QueryDeferredChild;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for QueryDeferredChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            yield_once().await;
            cx.rsjs_island_used(QUERY_ISLAND_ID);
            cx.rsjs_query_seed_json(
                r#"["framework","streamed-query"]"#,
                r#"{"key":["framework","streamed-query"],"data":"ready","data_updated_at":0}"#,
            );
            cx.out
                .extend_static(b"<section data-streamed-query></section>")
        })
    }
}

#[derive(Clone, Copy)]
struct LoadingFallback;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LoadingFallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out
            .extend_static(b"<section data-loading-fallback></section>")
    }
}

#[derive(Clone, Copy)]
struct RecoveryFallback;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for RecoveryFallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.rsjs_island_used(RECOVERY_ISLAND_ID);
        cx.out
            .extend_static(b"<section data-recovery-fallback></section>")
    }
}

struct LeakingFailedChild;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for LeakingFailedChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(LEAK_ISLAND_ID);
            cx.out
                .extend_static(b"<section data-discarded></section>")?;
            yield_once().await;
            Err(WriteError::render("deferred child failed"))
        })
    }
}

struct TimedOutChild;

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for TimedOutChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(LEAK_ISLAND_ID);
            cx.out
                .extend_static(b"<section data-never-published></section>")?;
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}

struct SuccessfulStreamingPage;

impl WebRender<NoLoader, RenderBuffer> for SuccessfulStreamingPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.out.extend_static(b"<main data-stream-page>")?;
            WebRender::render(Suspense::new(SuccessfulDeferredChild, LoadingFallback), cx)
                .resolve()
                .await?;
            cx.out.extend_static(b"</main>")
        })
    }
}

struct RecoveredStreamingPage;

impl WebRender<NoLoader, RenderBuffer> for RecoveredStreamingPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.out.extend_static(b"<main data-recovered-stream-page>")?;
            WebRender::render(
                ErrorBoundary::new(
                    Suspense::new(LeakingFailedChild, LoadingFallback),
                    RecoveryFallback,
                ),
                cx,
            )
            .resolve()
            .await?;
            cx.out.extend_static(b"</main>")
        })
    }
}

struct QueryStreamingPage;

impl WebRender<NoLoader, RenderBuffer> for QueryStreamingPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.out.extend_static(b"<main data-query-stream-page>")?;
            WebRender::render(Suspense::new(QueryDeferredChild, LoadingFallback), cx)
                .resolve()
                .await?;
            cx.out.extend_static(b"</main>")
        })
    }
}

struct TimedOutStreamingPage;

impl WebRender<NoLoader, RenderBuffer> for TimedOutStreamingPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used(BASIC_ISLAND_ID);
            cx.out.extend_static(b"<main data-timeout-stream-page>")?;
            WebRender::render(
                Suspense {
                    child: TimedOutChild,
                    fallback: LoadingFallback,
                    priority: proa_core::Priority::Critical,
                    timeout: Some(Duration::from_millis(10)),
                },
                cx,
            )
            .resolve()
            .await?;
            cx.out.extend_static(b"</main>")
        })
    }
}

#[test]
fn zero_islands_emit_no_rsjs_tags() {
    let html = finalize(
        RouteResponse::ssr(ServerOnly),
        &FrameworkRequestContext::default(),
    );

    assert_eq!(html, "<main data-server-only></main>");
    assert!(!html.contains("<script"));
    assert!(!html.contains("modulepreload"));
    assert!(!html.contains("/rsjs"));
}

#[test]
fn sync_response_automatically_emits_one_deduplicated_runtime_plan() {
    let html = finalize(
        RouteResponse::ssr(BasicIsland),
        &FrameworkRequestContext::default(),
    );

    assert!(html.starts_with("<main data-basic-island></main>"));
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<link rel="modulepreload" href="/rsjs/island/framework_auto_basic.js">"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_basic.js"></script>"#,
    );
    assert!(!html.contains("/rsjs-runtime-query.js"));
}

#[test]
fn document_response_places_automatic_plan_before_body_close() {
    let html = finalize(
        RouteResponse::ssr(BasicDocument),
        &FrameworkRequestContext::default(),
    );

    let plan_start = html
        .find(r#"<script type="module" src="/rsjs-runtime.js"></script>"#)
        .expect("document response should contain the runtime plan");
    let body_close = html
        .rfind("</body>")
        .expect("document fixture should retain its body close");
    assert!(
        plan_start < body_close,
        "runtime plan must stay inside body"
    );
    assert!(html.ends_with("</body></html>"));
}

#[test]
fn query_runtime_and_seed_table_are_scoped_to_the_rendered_query_island() {
    let basic_html = finalize(
        RouteResponse::ssr(BasicIsland),
        &FrameworkRequestContext::default(),
    );
    assert!(!basic_html.contains("/rsjs-runtime-query.js"));
    assert!(!basic_html.contains("rsjs-query-seed-table"));

    let query_html = finalize(
        RouteResponse::ssr(QueryIsland),
        &FrameworkRequestContext::default(),
    );
    assert_exactly_once(
        &query_html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &query_html,
        r#"<script type="module" src="/rsjs-runtime-query.js"></script>"#,
    );
    assert_exactly_once(
        &query_html,
        r#"<script type="module" src="/rsjs/island/framework_auto_query.js"></script>"#,
    );
    assert_exactly_once(&query_html, r#"id="rsjs-query-seed-table""#);
}

#[test]
fn request_nonce_is_applied_to_every_generated_script() {
    let request = FrameworkRequestContext {
        csp_nonce: Some(CspNonce::new(NONCE).expect("fixture nonce should be valid")),
        ..FrameworkRequestContext::default()
    };
    let html = finalize(RouteResponse::ssr(QueryIsland), &request);

    assert_every_script_has_nonce(&html, NONCE);
}

#[test]
fn async_response_automatically_finalizes_rsjs_assets() {
    let html = finalize(
        RouteResponse::ssr_async(AsyncIsland),
        &FrameworkRequestContext::default(),
    );

    assert!(html.starts_with("<main data-async-island></main>"));
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_basic.js"></script>"#,
    );
}

#[test]
fn in_order_stream_finalizer_unions_shell_and_deferred_child_artifacts() {
    let chunks = finalize_stream(
        RouteResponse::ssr_stream(SuccessfulStreamingPage)
            .with_render_mode(RenderMode::ChunkedInOrder),
        &FrameworkRequestContext::default(),
    );
    let html = chunks.concat();

    assert!(
        chunks.len() >= 2,
        "a deferred in-order child should flush before final response artifacts"
    );
    assert!(html.contains("<section data-deferred-child></section>"));
    assert!(!html.contains("data-loading-fallback"));
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_basic.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_child.js"></script>"#,
    );
}

#[test]
fn out_of_order_stream_emits_exact_shell_and_boundary_asset_deltas() {
    let chunks = finalize_stream(
        RouteResponse::ssr_stream(SuccessfulStreamingPage),
        &FrameworkRequestContext::default(),
    );
    let shell = chunks
        .first()
        .expect("out-of-order stream should emit a shell");
    let boundary_chunks = chunks[1..].concat();
    let html = chunks.concat();

    assert!(shell.contains("data-loading-fallback"));
    assert!(shell.contains("/rsjs/island/framework_auto_basic.js"));
    assert!(!shell.contains("/rsjs/island/framework_auto_child.js"));
    assert!(boundary_chunks.contains("<template id=\"proa-tpl-"));
    assert!(boundary_chunks.contains("/rsjs/island/framework_auto_child.js"));
    assert!(!boundary_chunks.contains("/rsjs/island/framework_auto_basic.js"));
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<link rel="modulepreload" href="/rsjs/island/framework_auto_basic.js">"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_basic.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<link rel="modulepreload" href="/rsjs/island/framework_auto_child.js">"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_child.js"></script>"#,
    );
}

#[test]
fn out_of_order_query_boundary_emits_late_seed_and_query_runtime_delta() {
    let chunks = finalize_stream(
        RouteResponse::ssr_stream(QueryStreamingPage),
        &FrameworkRequestContext::default(),
    );
    let shell = chunks
        .first()
        .expect("out-of-order stream should emit a shell");
    let boundary_chunks = chunks[1..].concat();
    let html = chunks.concat();

    assert!(!shell.contains("/rsjs-runtime-query.js"));
    assert!(!shell.contains("rsjs-query-seed-table"));
    assert!(
        boundary_chunks.contains(r#"id="rsjs-query-seed-table" data-rsjs-seed-scope="fragment""#)
    );
    assert!(boundary_chunks.contains(r#"["framework","streamed-query"]"#));
    assert!(boundary_chunks.contains("/rsjs-runtime-query.js"));
    assert!(boundary_chunks.contains("/rsjs/island/framework_auto_query.js"));
    let seed_offset = boundary_chunks
        .find("rsjs-query-seed-table")
        .expect("streamed query seed table should be present");
    let swap_offset = boundary_chunks
        .find("<template id=\"proa-tpl-")
        .expect("boundary swap template should be present");
    assert!(
        seed_offset < swap_offset,
        "boundary metadata must be parsed before the swap dispatches proa:boundary-ready"
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime.js"></script>"#,
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs-runtime-query.js"></script>"#,
    );
    assert_exactly_once(&html, r#"id="rsjs-query-seed-table""#);
}

#[test]
fn recovered_out_of_order_boundary_discards_failed_branch_artifacts() {
    let chunks = finalize_stream(
        RouteResponse::ssr_stream(RecoveredStreamingPage),
        &FrameworkRequestContext::default(),
    );
    let shell = chunks
        .first()
        .expect("out-of-order stream should emit a shell");
    let boundary_chunks = chunks[1..].concat();
    let html = chunks.concat();

    assert!(shell.contains("data-loading-fallback"));
    assert!(!shell.contains("/rsjs/island/framework_auto_recovery.js"));
    assert!(boundary_chunks.contains("<template id=\"proa-tpl-"));
    assert!(boundary_chunks.contains("/rsjs/island/framework_auto_recovery.js"));
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_recovery.js"></script>"#,
    );
    assert!(
        !html.contains("/rsjs/island/framework_auto_leak.js"),
        "artifacts from the failed and discarded branch must not reach the response"
    );
}

#[tokio::test]
async fn timed_out_out_of_order_boundary_discards_cancelled_branch_artifacts() {
    let chunks = finalize_stream_async(
        RouteResponse::ssr_stream(TimedOutStreamingPage),
        &FrameworkRequestContext::default(),
    )
    .await;
    let html = chunks.concat();

    assert!(html.contains("data-loading-fallback"));
    assert!(!html.contains("data-never-published"));
    assert!(
        !html.contains("/rsjs/island/framework_auto_leak.js"),
        "artifacts from a cancelled boundary must not reach the response"
    );
    assert_exactly_once(
        &html,
        r#"<script type="module" src="/rsjs/island/framework_auto_basic.js"></script>"#,
    );
}
