// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared Axum routes and asset handlers for the Proa showcase fixtures.
//!
//! `proa_showcase` owns the renderable page fixtures. This crate owns the web
//! boundary around them so servers can mount the same routes without copying
//! handler and static asset code.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// Canonical public showcase routes. Alias routes are intentionally omitted so
/// sitemaps can point at one stable URL per page.
pub const SHOWCASE_ROUTES: &[&str] = &[
    "/showcase/airbnb/proa",
    "/showcase/airbnb/proa/benchmark",
    "/showcase/airbnb/proa/search",
    "/showcase/airbnb/proa/listing",
    "/showcase/nike/proa",
    "/showcase/nike/proa/product",
    "/showcase/ebay/proa",
    "/showcase/espn/proa",
    "/showcase/sentry-otel/proa",
];

const SHORT_CACHE_CONTROL: &str = "public, max-age=3600";
const IMAGE_CACHE_CONTROL: &str = "public, max-age=86400";

#[derive(Clone, Debug)]
pub struct ShowcaseWebConfig {
    public_roots: Vec<PathBuf>,
}

impl ShowcaseWebConfig {
    pub fn new(public_root: impl Into<PathBuf>) -> Self {
        Self {
            public_roots: vec![public_root.into()],
        }
    }

    pub fn from_env() -> Self {
        let mut roots = Vec::new();
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let proa_root = manifest_dir.parent().unwrap_or_else(|| Path::new("."));

        push_env_path(&mut roots, "SHOWCASE_PUBLIC_DIR");
        push_env_path(&mut roots, "STATIC_DIR");
        push_root(&mut roots, manifest_dir.join("public"));
        push_root(&mut roots, proa_root.join("proa_marketing").join("public"));
        push_root(
            &mut roots,
            proa_root
                .join("proa_showcase")
                .join("src")
                .join("espn")
                .join("data"),
        );
        Self {
            public_roots: roots,
        }
    }

    pub fn public_roots(&self) -> &[PathBuf] {
        &self.public_roots
    }

    fn resolve_public_file(&self, subdir: &str, path: &str) -> Option<PathBuf> {
        for root in &self.public_roots {
            let base = root.join(subdir);
            let canonical_base = match base.canonicalize() {
                Ok(path) => path,
                Err(_) => continue,
            };
            let file_path = match base.join(path).canonicalize() {
                Ok(path) => path,
                Err(_) => continue,
            };

            if file_path.starts_with(&canonical_base) {
                return Some(file_path);
            }
        }

        None
    }
}

impl Default for ShowcaseWebConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Build the shared showcase router using environment/default asset roots.
pub fn router() -> Router {
    router_with_config(ShowcaseWebConfig::default())
}

/// Build the shared showcase router with an explicit asset configuration.
pub fn router_with_config(config: ShowcaseWebConfig) -> Router {
    Router::new()
        .route("/showcase/airbnb/proa", get(airbnb_home_parity))
        .route("/showcase/airbnb/proa/benchmark", get(airbnb_home_parity))
        .route("/showcase/airbnb/proa/search", get(airbnb_search))
        .route("/showcase/airbnb/proa/listing", get(airbnb_listing))
        .route("/showcase/nike/proa", get(nike_catalog))
        .route("/showcase/nike/proa/product", get(nike_product))
        .route("/showcase/ebay/proa", get(ebay_srp))
        .route("/showcase/espn/proa", get(espn_game))
        .route("/showcase/sentry-otel/proa", get(sentry_otel))
        .route("/showcase/airbnb", get(airbnb_home))
        .route("/showcase/airbnb/search", get(airbnb_search))
        .route("/showcase/airbnb/listing", get(airbnb_listing))
        .route("/showcase/nike", get(nike_catalog))
        .route("/showcase/nike/product", get(nike_product))
        .route("/showcase/ebay", get(ebay_srp))
        .route("/showcase/espn", get(espn_game))
        .route("/showcase/sentry-otel", get(sentry_otel))
        .route("/rsjs-runtime.js", get(serve_rsjs_runtime))
        .route("/rsjs-runtime-query.js", get(serve_rsjs_runtime_query))
        .route("/rsjs-runtime.:hash.js", get(serve_rsjs_runtime_hashed))
        .route(
            "/rsjs-runtime-query.:hash.js",
            get(serve_rsjs_runtime_query_hashed),
        )
        .route("/rsjs/island/:file", get(serve_rsjs_island))
        .route("/proa.css", get(serve_proa_css))
        .route("/proa.js", get(serve_proa_js))
        .route("/assets/*path", get(serve_asset))
        .route("/fonts/*path", get(serve_font))
        .route("/images/*path", get(serve_image))
        .with_state(config)
}

async fn airbnb_home() -> Html<String> {
    Html(proa_showcase::airbnb::home::render())
}

async fn airbnb_home_parity() -> Html<String> {
    Html(proa_showcase::airbnb::parity::render())
}

async fn airbnb_search() -> Html<String> {
    Html(proa_showcase::airbnb::search::render())
}

async fn airbnb_listing() -> Html<String> {
    Html(proa_showcase::airbnb::listing::render())
}

async fn nike_catalog() -> Html<String> {
    Html(proa_showcase::nike::catalog::render())
}

async fn nike_product() -> Html<String> {
    Html(proa_showcase::nike::product::render())
}

async fn ebay_srp(Query(query): Query<HashMap<String, String>>) -> Html<String> {
    let (n, q) = srp_query_params(&query);
    let listings = proa_showcase::ebay::srp::default_listings(n);
    Html(proa_showcase::ebay::srp_chrome_real::render_document(
        q,
        listings,
        proa_showcase::ebay::srp::ListingsList { listings },
        (),
    ))
}

async fn espn_game() -> Html<String> {
    Html(proa_showcase::espn::render())
}

async fn sentry_otel(Query(query): Query<HashMap<String, String>>) -> Html<String> {
    let rows = query
        .get("rows")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(proa_showcase::sentry_otel::DEFAULT_ROW_COUNT)
        .clamp(40, 10_000);
    let mut out = Vec::new();
    let mut cx = proa_core::WebContext::with_buffer(&mut out);
    proa_showcase::sentry_otel::render_into(&mut cx, rows).unwrap();
    Html(String::from_utf8(out).unwrap())
}

async fn serve_proa_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
        ],
        include_str!("../../proa_marketing/public/styles.css"),
    )
}

async fn serve_proa_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
        ],
        "// Showcase interactivity is served through the rsjs asset routes.\n",
    )
}

async fn serve_rsjs_runtime() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
        ],
        rsjs::runtime_assets::CORE_JS,
    )
}

async fn serve_rsjs_runtime_query() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
        ],
        rsjs::runtime_assets::QUERY_JS,
    )
}

async fn serve_rsjs_runtime_hashed(AxumPath(_hash): AxumPath<String>) -> impl IntoResponse {
    serve_rsjs_runtime().await
}

async fn serve_rsjs_runtime_query_hashed(AxumPath(_hash): AxumPath<String>) -> impl IntoResponse {
    serve_rsjs_runtime_query().await
}

async fn serve_rsjs_island(AxumPath(file): AxumPath<String>) -> Response {
    let manifest = rsjs::manifest();
    if let Some(bundle) = manifest
        .client_modules
        .iter()
        .find(|entry| rsjs::client_module_filename(entry.module_path) == file)
    {
        return javascript_response(rsjs::client_module_js_source(bundle));
    }
    if let Some(class) = manifest
        .client_classes
        .iter()
        .find(|entry| rsjs::client_class_filename(entry.module_path) == file)
    {
        return javascript_response(class.js_source);
    }

    let Some(asset_id) = rsjs_asset_id_from_file(&file) else {
        return (StatusCode::NOT_FOUND, "rsjs asset not found").into_response();
    };
    let fragment_asset_id = asset_id.strip_prefix("component_").unwrap_or(asset_id);

    let source = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id == asset_id)
        .map(|entry| entry.js_source)
        .or_else(|| {
            manifest
                .component_fragments
                .iter()
                .find(|entry| entry.asset_id == asset_id || entry.asset_id == fragment_asset_id)
                .map(|entry| entry.js_source)
        });

    match source {
        Some(source) => (
            StatusCode::OK,
            [
                (
                    header::CONTENT_TYPE,
                    "application/javascript; charset=utf-8",
                ),
                (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
            ],
            source,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "rsjs asset not found").into_response(),
    }
}

fn javascript_response(source: impl Into<axum::body::Body>) -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, SHORT_CACHE_CONTROL),
        ],
        source.into(),
    )
        .into_response()
}

async fn serve_asset(
    State(config): State<ShowcaseWebConfig>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    serve_public_file(config, "assets", path, SHORT_CACHE_CONTROL).await
}

async fn serve_font(
    State(config): State<ShowcaseWebConfig>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    serve_public_file(config, "fonts", path, IMAGE_CACHE_CONTROL).await
}

async fn serve_image(
    State(config): State<ShowcaseWebConfig>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    let file_path = config.resolve_public_file("images", &path).or_else(|| {
        fallback_image_path(&path).and_then(|path| config.resolve_public_file("images", path))
    });

    serve_resolved_file(file_path, IMAGE_CACHE_CONTROL).await
}

async fn serve_public_file(
    config: ShowcaseWebConfig,
    subdir: &'static str,
    path: String,
    cache_control: &'static str,
) -> Response {
    let file_path = config.resolve_public_file(subdir, &path);
    serve_resolved_file(file_path, cache_control).await
}

async fn serve_resolved_file(file_path: Option<PathBuf>, cache_control: &'static str) -> Response {
    let Some(file_path) = file_path else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };

    let content_type = content_type_for_path(&file_path);

    match tokio::fs::read(&file_path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, cache_control),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

fn fallback_image_path(path: &str) -> Option<&'static str> {
    let path = path.to_ascii_lowercase();

    if path.starts_with("airbnb/") {
        if path.contains("bedroom") || path.contains("avatar") || path.contains("host") {
            return Some("airbnb-preview-bedroom.webp");
        }
        if path.contains("deck") || path.contains("ocean") || path.contains("laurel") {
            return Some("airbnb-preview-deck.webp");
        }
        return Some("airbnb-preview-exterior.webp");
    }

    if path.starts_with("nike/") {
        const NIKE_FALLBACKS: [&str; 4] = [
            "nike/alphafly-3-side.jpg",
            "nike/alphafly-3-side-wide.jpg",
            "nike/alphafly-3-side-portrait.jpg",
            "nike/alphafly-3-glam.jpg",
        ];
        return Some(NIKE_FALLBACKS[stable_index(path.as_bytes(), NIKE_FALLBACKS.len())]);
    }

    None
}

fn stable_index(bytes: &[u8], len: usize) -> usize {
    let mut hash = 0usize;
    for byte in bytes {
        hash = hash.wrapping_mul(31).wrapping_add(*byte as usize);
    }
    hash % len
}

fn srp_query_params(query: &HashMap<String, String>) -> (usize, &str) {
    let n = query
        .get("n")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(60)
        .clamp(1, 1000);
    let q = query
        .get("q")
        .map(String::as_str)
        .unwrap_or(proa_showcase::ebay::srp_chrome_real::DEFAULT_QUERY);
    (n, q)
}

fn rsjs_asset_id_from_file(file: &str) -> Option<&str> {
    let stem = file.strip_suffix(".js")?;
    match stem.rsplit_once('.') {
        Some((asset_id, hash)) if is_short_hash(hash) => Some(asset_id),
        _ => Some(stem),
    }
}

fn is_short_hash(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn push_env_path(roots: &mut Vec<PathBuf>, var: &str) {
    if let Some(path) = env::var_os(var).map(PathBuf::from) {
        push_root(roots, path);
    }
}

fn push_root(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !roots.iter().any(|root| root == &path) {
        roots.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_routes_are_showcase_paths() {
        assert!(SHOWCASE_ROUTES
            .iter()
            .all(|route| route.starts_with("/showcase/")));
    }

    #[tokio::test]
    async fn canonical_airbnb_home_renders_the_parity_fixture() {
        let Html(html) = airbnb_home_parity().await;

        assert!(html.contains("data-airbnb-parity-app"));
        assert!(html.contains("data-parity-revision"));
    }

    #[test]
    fn missing_showcase_images_map_to_existing_marketing_fallbacks() {
        assert_eq!(
            fallback_image_path("airbnb/bedroom.jpg"),
            Some("airbnb-preview-bedroom.webp")
        );
        assert_eq!(
            fallback_image_path("airbnb/external/8db0ac0c.jpeg"),
            Some("airbnb-preview-exterior.webp")
        );
        assert!(fallback_image_path("nike/AIR+JORDAN+14+RETRO.png")
            .unwrap()
            .starts_with("nike/alphafly-3"));
        assert_eq!(fallback_image_path("blog/foo.png"), None);
    }
}
