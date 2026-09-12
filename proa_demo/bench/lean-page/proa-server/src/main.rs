// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Lean proa benchmark server (port 5501).
//!
//! Serves a stripped-to-the-bone page consisting of:
//!   - HTML doctype + `<head>` with ~200 bytes of inline CSS for grid layout
//!   - N `WatchHeart` rsjs islands (default 60, configurable via `?n=N`)
//!   - the rsjs runtime + the per-island JS for the `WatchHeart` asset only
//!
//! No analytics, no fonts, no images, no third-party scripts. The point is
//! that the framework-shipped JS dominates the byte budget so the
//! per-engine wins/losses are visible (proa rsjs island vs Marko hydrate
//! bundle vs React 19 client bundle).
//!
//! Routes:
//!   - GET /          → lean page with N WatchHeart toggles
//!   - GET /health    → {"status":"ok","engine":"proa-lean"}
//!   - GET /rsjs-runtime.js → shared rsjs runtime (same source as the playground)
//!   - GET /rsjs/island/:asset_id → generated rsjs JS
//!     (only WatchHeart actually loaded in this benchmark)

use axum::{
    extract::{Path, Query},
    http::header,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use proa_core::{WebContext, WebRenderSync};
use proa_demo::ebay_srp_islands::WatchHeart;
use std::collections::HashMap;

const PORT: u16 = 5501;

const PAGE_CSS: &str = "*{box-sizing:border-box}body{margin:0;padding:16px;font:14px system-ui,sans-serif;background:#fafafa}.lean-bench{display:grid;grid-template-columns:repeat(auto-fill,minmax(72px,1fr));gap:8px;max-width:960px;margin:0 auto}.lean-bench>*{display:flex;align-items:center;justify-content:center;background:#fff;border:1px solid #e5e5e5;border-radius:6px;padding:12px;min-height:48px}button.s-card__watchheart-click{background:transparent;border:0;cursor:pointer;font-size:24px;line-height:1;padding:4px;color:#c00}button.s-card__watchheart-click .s-card__watchheart-icon{display:inline-flex}svg.icon{width:1em;height:1em}";

// Hand-rolled SVG sprite definitions for the heart icons. The
// WatchHeart island renders `<use href="#icon-heart-16">` and
// `<use href="#icon-heart-filled-16">`, so a tiny inline `<svg>` symbol
// definition block has to be in the page.
const SPRITE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" style="display:none" aria-hidden="true"><symbol id="icon-heart-16" viewBox="0 0 16 16"><path fill="none" stroke="currentColor" stroke-width="1.5" d="M8 13.5s-5-3.2-5-7a2.7 2.7 0 0 1 5-1.5A2.7 2.7 0 0 1 13 6.5c0 3.8-5 7-5 7z"/></symbol><symbol id="icon-heart-filled-16" viewBox="0 0 16 16"><path fill="currentColor" d="M8 13.5s-5-3.2-5-7a2.7 2.7 0 0 1 5-1.5A2.7 2.7 0 0 1 13 6.5c0 3.8-5 7-5 7z"/></symbol></svg>"##;

fn render_lean_page(n: usize) -> String {
    let n = n.clamp(1, 1000);
    // Render N WatchHearts. Each render returns a self-contained
    // `<rsjs-island>...<seed-script></rsjs-island>` blob, so we just
    // concatenate them inside the `<div class="lean-bench">` grid.
    let mut heart_bytes = Vec::with_capacity(n * 800);
    let artifacts = {
        let mut cx = WebContext::with_buffer(&mut heart_bytes);
        for _ in 0..n {
            WatchHeart.render(&mut cx).expect("WatchHeart renders");
        }
        cx.rsjs_artifacts_snapshot()
    };
    let hearts = String::from_utf8(heart_bytes).expect("WatchHeart rendered valid UTF-8");
    let plan = rsjs::manifest()
        .page_runtime_plan(artifacts.rendered_islands())
        .expect("rendered WatchHeart island must exist in the linked rsjs manifest");
    let mut script_bytes = Vec::new();
    plan.write_html(&mut script_bytes, None)
        .expect("lean page rsjs runtime plan should render");
    let scripts =
        String::from_utf8(script_bytes).expect("rsjs runtime plan should render valid UTF-8");

    // proa-perf-ok(scope): this benchmark intentionally hand-assembles its fixed
    // page shell around Proa-rendered islands and shared-planner output.
    format!(
        "<!DOCTYPE html>\
<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>proa lean bench (n={n})</title>\
<style>{PAGE_CSS}</style></head>\
<body>{SPRITE}<main class=\"lean-bench\">{hearts}</main>{scripts}</body></html>"
    )
}

async fn lean_page(Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let n = q.get("n").and_then(|s| s.parse().ok()).unwrap_or(60);
    Html(render_lean_page(n))
}

async fn health() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"status":"ok","engine":"proa-lean"}"#,
    )
}

async fn serve_rsjs_runtime() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        rsjs::runtime_assets::CORE_JS,
    )
}

async fn serve_rsjs_runtime_query() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        rsjs::runtime_assets::QUERY_JS,
    )
}

async fn serve_rsjs_island(Path(asset_id): Path<String>) -> impl IntoResponse {
    let stem = asset_id.strip_suffix(".js").unwrap_or(&asset_id);
    let m = rsjs::manifest();
    if let Some(entry) = m.islands.iter().find(|i| i.asset_id == stem) {
        return (
            axum::http::StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            )],
            entry.js_source.to_string(),
        )
            .into_response();
    }

    if let Some(entry) = m.client_modules.iter().find(|module| {
        rsjs::client_module_filename(module.module_path)
            .strip_suffix(".js")
            .unwrap_or(module.module_path)
            == stem
    }) {
        return (
            axum::http::StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            )],
            rsjs::client_module_js_source(entry),
        )
            .into_response();
    }

    if let Some(fragment_asset_id) = stem.strip_prefix("component_") {
        if let Some(entry) = m
            .component_fragments
            .iter()
            .find(|fragment| fragment.asset_id == fragment_asset_id)
        {
            return (
                axum::http::StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    "application/javascript; charset=utf-8",
                )],
                entry.js_source.to_string(),
            )
                .into_response();
        }
    }

    (axum::http::StatusCode::NOT_FOUND, "unknown rsjs asset").into_response()
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(lean_page))
        .route("/health", get(health))
        .route("/rsjs-runtime.js", get(serve_rsjs_runtime))
        .route("/rsjs-runtime-query.js", get(serve_rsjs_runtime_query))
        .route("/rsjs/island/:asset_id", get(serve_rsjs_island));

    let addr = format!("0.0.0.0:{PORT}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("proa-lean-server: bind {addr} failed: {e}"));
    println!("proa-lean-server listening on :{PORT}");
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lean_scripts_scope_query_runtime_to_selected_islands() {
        let html = render_lean_page(3);

        assert!(
            html.contains(r#"<script type="module" src="/rsjs-runtime.js"></script>"#),
            "lean page should load the base rsjs runtime, got:\n{html}"
        );
        // Plain-cargo rendering serves the island module directly
        // (`/rsjs/island/watchheart_<hash>.js`); the linked pipeline serves
        // a shared component fragment plus an owner module
        // (`/rsjs/island/component_watchheart_<hash>.js`). Either spelling
        // proves the WatchHeart island's JS is wired into the page.
        let watchheart_asset_refs = html.matches(r#""/rsjs/island/watchheart_"#).count()
            + html
                .matches(r#""/rsjs/island/component_watchheart_"#)
                .count();
        assert!(
            watchheart_asset_refs > 0,
            "lean page should load the WatchHeart island, got:\n{html}"
        );
        assert!(
            !html.contains("/rsjs-runtime-query.js"),
            "WatchHeart does not use queries or mutations, so the lean benchmark should not load the query runtime, got:\n{html}"
        );
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        // All three rendered islands must share a single island module
        // script (dedupe), in both plain and linked serving modes.
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs/island/"#)
                .count(),
            1,
            "expected exactly one island module script tag, got:\n{html}"
        );
        // And exactly one WatchHeart preload, regardless of which asset
        // spelling the serving mode uses.
        let watchheart_preloads = html
            .matches(r#"<link rel="modulepreload" href="/rsjs/island/watchheart_"#)
            .count()
            + html
                .matches(r#"<link rel="modulepreload" href="/rsjs/island/component_watchheart_"#)
                .count();
        assert_eq!(
            watchheart_preloads, 1,
            "expected exactly one WatchHeart module preload, got:\n{html}"
        );
    }
}
