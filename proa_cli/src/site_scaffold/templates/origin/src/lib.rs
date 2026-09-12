// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod api;
mod app;
mod assets;
pub mod components;
mod layouts;
pub mod loader;
pub mod model;

mod __proa_fonts_generated {
    include!(concat!(env!("OUT_DIR"), "/proa_fonts.rs"));
}

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use proa_framework_axum::{
    explicit_markdown, negotiate, router_from_spec, IslandManifest, MarkdownAlternate, Route,
    RouteContextLayer, RouteResponse,
};
use proa_stream::DataResolver;
use serde::Deserialize;
use tower_http::{services::ServeDir, trace::TraceLayer};

use api::RipeClient;
use app::AppState;
use components::{AtlasMd, AtlasPage};
use layouts::RootDocument;
use loader::AtlasLoader;
use model::Dossier;

pub use components::markdown_string;

/// Render the atlas document for one lookup through the given loader, exactly
/// as the `/` route serves it. The golden SSR test injects a fixture loader
/// here so the output is deterministic.
pub async fn render_atlas_page<L>(raw_query: Option<&str>, loader: Arc<L>) -> String
where
    L: proa_core::DataLoader + Send + Sync + 'static,
{
    use proa_core::WebRender;
    let resolver = DataResolver::request_scope(loader);
    let mut cx = proa_core::WebContext::with_loader(resolver);
    RootDocument {
        content: AtlasPage::resolve(raw_query),
    }
    .render(&mut cx)
    .resolve()
    .await
    .expect("render the atlas document");
    cx.into_output().into_string()
}

#[derive(Debug, Deserialize)]
struct Lookup {
    q: Option<String>,
}

/// The request's Host header, for rendering copyable absolute URLs.
fn request_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

pub fn app() -> Router {
    // Measure one full document render from canned data (pure CPU) so the
    // receipt can report this server's own share of a lookup separately from
    // upstream latency. Inside that render the receipt itself reads the
    // still-unset cell and prints a placeholder, so there is no recursion.
    // The render future is not Send, so it gets its own thread and runtime.
    std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build the render-measurement runtime");
        runtime.block_on(async {
            let started = std::time::Instant::now();
            let _ = render_atlas_page(None, Arc::new(loader::FixtureLoader::github())).await;
            components::receipt::record_page_render(started.elapsed());
        });
    });
    let client = RipeClient::new().expect("construct RIPEstat client");
    let state = AppState {
        loader: Arc::new(AtlasLoader::new(client)),
    };
    let (router, metadata) = router_from_spec(vec![Route::endpoint("/", get(index))]);
    let manifest = Arc::new(IslandManifest {
        islands: HashMap::new(),
        assets: Default::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    });
    router
        .route("/index.md", get(markdown))
        .route("/site/:file", get(site_markdown))
        .route("/lab/rows", get(lab_rows))
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/rsjs-runtime.js", get(assets::rsjs_runtime))
        .route("/rsjs-runtime-query.js", get(assets::rsjs_query_runtime))
        .route("/rsjs/island/:file", get(assets::rsjs_asset))
        .nest_service(
            "/static",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/public")),
        )
        .nest_service(
            "/fonts",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/public/fonts")),
        )
        .with_state(state)
        .layer(RouteContextLayer::new(manifest, metadata))
        .layer(TraceLayer::new_for_http())
}

/// Markdown routes fetch through the same TTL cache as the page's loader,
/// so `/site/<host>.md` and the HTML dossier share one upstream fan-out and
/// one freshness window.
async fn fetch_dossier(state: &AppState, query: &str) -> Option<Arc<Dossier>> {
    state.loader.cached_dossier(query).await.ok().map(Arc::new)
}

async fn index(
    State(state): State<AppState>,
    Query(query): Query<Lookup>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let host = request_host(&headers);
    let page = AtlasPage::resolve(query.q.as_deref()).with_host(host.clone());
    let lookup = page.query.clone();
    let resolver = DataResolver::request_scope(state.loader.clone());
    let html = RouteResponse::ssr_stream_with_loader(RootDocument { content: page }, resolver);
    // The markdown value must be sync-renderable inside negotiate, so its
    // dossier is prefetched, but only when the Accept header can actually
    // select markdown; the HTML path stays loader-driven.
    let wants_markdown = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("markdown"));
    let dossier = if wants_markdown {
        fetch_dossier(&state, &lookup).await
    } else {
        None
    };
    negotiate(
        &headers,
        html,
        AtlasMd {
            query: lookup,
            host,
            dossier,
        },
        MarkdownAlternate::new("/index.md"),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn markdown(
    State(state): State<AppState>,
    Query(query): Query<Lookup>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let page = AtlasPage::resolve(query.q.as_deref());
    let dossier = fetch_dossier(&state, &page.query).await;
    explicit_markdown(AtlasMd {
        query: page.query,
        host: request_host(&headers),
        dossier,
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// The overlay advertises `GET /site/<host>.md`; serve exactly that.
async fn site_markdown(
    State(state): State<AppState>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let host = file.strip_suffix(".md").ok_or(StatusCode::NOT_FOUND)?;
    let page = AtlasPage::resolve(Some(host));
    let (dossier, upstream_down) = match state.loader.cached_dossier(&page.query).await {
        Ok(dossier) => (Some(Arc::new(dossier)), false),
        Err(error) => (None, !error.is_query_problem()),
    };
    if dossier.is_none() && model::site_for(host).is_none() {
        // A name that does not resolve is 404; an unreachable upstream is
        // 503, so agents can tell "does not exist" from "try again".
        return Err(if upstream_down {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NOT_FOUND
        });
    }
    explicit_markdown(AtlasMd {
        query: page.query,
        host: request_host(&headers),
        dossier,
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct LabRowCount {
    n: Option<usize>,
}

/// The lab's server half: render the same synthetic rows the browser builds
/// and report the measured wall-clock time, so the "server render" tile is an
/// observation rather than a model.
async fn lab_rows(Query(query): Query<LabRowCount>) -> Json<serde_json::Value> {
    let count = query.n.unwrap_or(1_000).clamp(1, 200_000);
    let started = std::time::Instant::now();
    let mut cx = proa_core::WebContext::new();
    proa_core::WebRenderSync::render(components::LabRows { count }, &mut cx)
        .expect("render lab rows");
    // The clock stops before the buffer-to-String copy: the tile claims
    // render time, so only rendering is inside the window. The profile
    // travels along because a debug dev build is an order of magnitude
    // slower than the release binary this demo deploys as.
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    let bytes = cx.into_output().into_string().len();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    Json(serde_json::json!({ "rows": count, "millis": millis, "bytes": bytes, "profile": profile }))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn ready() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ready" }))
}
