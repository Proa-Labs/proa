// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use axum::body::to_bytes;
use proa_assets::build::OutputStore;

struct Fixture {
    temp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("public")).unwrap();
        Self { temp }
    }

    fn write(&self, name: &str, value: &[u8]) {
        let path = self.temp.path().join("public").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }

    fn load(&self, base: &str) -> PublicAssets {
        let output = self.temp.path().join("output");
        OutputStore::open(&output)
            .unwrap()
            .publish(self.temp.path().join("public"))
            .unwrap();
        PublicAssets::load(output, &BasePath::new(base).unwrap()).unwrap()
    }
}

async fn get_asset(assets: &PublicAssets, url: &str) -> Response<Body> {
    assets
        .router()
        .oneshot(Request::builder().uri(url).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn builder_mounts_the_published_catalog_from_the_environment() {
    use crate::{FrameworkBuildError, FrameworkBuilder, RouteResponse};
    use proa_core::{WebContext, WebRenderSync, WriteError};
    use proa_macros::html_sync;

    const EXPECTED: &str = "PROA_TEST_EXPECTED_PUBLIC_URL";
    const BASE: &str = "/preview";

    struct Logo;

    impl WebRenderSync for Logo {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            html_sync! {
                <img src={cx.url("public/img/logo.png")} alt="Logo" />
            }
            .render(cx)
        }
    }

    if let Ok(expected) = std::env::var(EXPECTED) {
        let built = FrameworkBuilder::new()
            .with_dynamic_routes(
                Router::new().route("/", get(|| async { RouteResponse::ssr(Logo) })),
            )
            .with_base_path(BasePath::new(BASE).unwrap())
            .try_build();
        if expected == "invalid" {
            assert!(matches!(built, Err(FrameworkBuildError::PublicAssets(_))));
            return;
        }
        let app = built.unwrap();
        let page = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let html = to_bytes(page.into_body(), 1024).await.unwrap();
        assert!(std::str::from_utf8(&html).unwrap().contains(&expected));
        let asset = app
            .oneshot(
                Request::builder()
                    .uri(expected.strip_prefix(BASE).unwrap())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(to_bytes(asset.into_body(), 1024).await.unwrap(), "logo");
        return;
    }

    // Process isolation keeps test environment changes away from other builders.
    let f = Fixture::new();
    f.write("img/logo.png", b"logo");
    let assets = f.load(BASE);
    for (directory, expected) in [
        (
            f.temp.path().join("output"),
            assets.url("img/logo.png").unwrap(),
        ),
        (f.temp.path().join("missing-output"), "invalid"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "public_assets::tests::builder_mounts_the_published_catalog_from_the_environment",
            ])
            .env(OUTPUT_ENV, directory)
            .env(EXPECTED, expected)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&child.stderr)
        );
    }
}

#[tokio::test]
async fn files_keep_digest_validators_head_and_range_semantics() {
    let f = Fixture::new();
    f.write("movie.mp4", b"0123456789");
    let assets = f.load("");
    let url = assets.url("movie.mp4").unwrap();
    let response = get_asset(&assets, url).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let etag = response.headers()[header::ETAG].clone();
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "0123456789"
    );

    let head = assets
        .router()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri(url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
    assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
    let range = assets
        .router()
        .oneshot(
            Request::builder()
                .uri(url)
                .header(header::RANGE, "bytes=2-5")
                .header(header::IF_RANGE, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(to_bytes(range.into_body(), 1024).await.unwrap(), "2345");
    let stale_range = assets
        .router()
        .oneshot(
            Request::builder()
                .uri(url)
                .header(header::RANGE, "bytes=2-5")
                .header(header::IF_RANGE, "\"old\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale_range.status(), StatusCode::OK);
    let conditional = assets
        .router()
        .oneshot(
            Request::builder()
                .uri(url)
                .header(
                    header::IF_NONE_MATCH,
                    format!("\"other\", W/{}", etag.to_str().unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
    assert!(to_bytes(conditional.into_body(), 1024)
        .await
        .unwrap()
        .is_empty());
    let alias = get_asset(&assets, "/_proa/assets/movie.mp4").await;
    assert_eq!(alias.headers()[header::CACHE_CONTROL], "no-cache");
}

#[tokio::test]
async fn only_manifest_routes_are_public_and_names_are_encoded_once() {
    let f = Fixture::new();
    f.write("images/a %25?#é.png", b"image");
    let assets = f.load("");
    let url = assets.url("images/a %25?#é.png").unwrap();
    assert!(url.contains("%20%2525%3F%23%C3%A9"));
    let response = get_asset(&assets, url).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), "image");
    for private in [
        "manifest.json",
        "fingerprints.json",
        ".readers.lock",
        "staging/attempt/0",
        "images/a.deadbeef.png",
        "../manifest.json",
        "%2e%2e/manifest.json",
    ] {
        assert_eq!(
            get_asset(&assets, &format!("/_proa/assets/{private}"))
                .await
                .status(),
            StatusCode::NOT_FOUND,
            "{private}"
        );
    }
}

#[tokio::test]
async fn stable_js_and_css_rewrite_once_and_validate_the_delivered_bytes() {
    let f = Fixture::new();
    // Parser fixtures, not application-authored browser behavior.
    f.write("main.js", b"import './dependency.js'; fetch('/api/data');");
    f.write("styles.css", b"a { background: url('/images/a.png'); }");
    let root = f.load("");
    let mounted = f.load("/apps/demo");
    assert_ne!(root.cache_key(), mounted.cache_key());
    assert_eq!(
        mounted.url("main.js"),
        Some("/apps/demo/_proa/assets/main.js")
    );
    for (path, rewritten) in [
        ("main.js", "/apps/demo/api/data"),
        ("styles.css", "/apps/demo/images/a.png"),
    ] {
        let url = format!("/_proa/assets/{path}");
        let original = get_asset(&root, &url).await;
        let original_etag = original.headers()[header::ETAG].clone();
        let response = get_asset(&mounted, &url).await;
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let etag = response.headers()[header::ETAG].clone();
        assert_ne!(etag, original_etag);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(std::str::from_utf8(&bytes).unwrap().contains(rewritten));
        assert_eq!(etag, format!("\"{}\"", proa_assets::sha256(&bytes)));
        let head = mounted
            .router()
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri(&url)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            head.headers()[header::CONTENT_LENGTH],
            bytes.len().to_string()
        );
        assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn request_urls_and_shared_page_cache_follow_the_active_catalog() {
    use crate::{route::PageCacheLayer, FrameworkBuilder, Route, RouteResponse};
    use proa_core::{WebContext, WebRenderSync, WriteError};
    use proa_macros::html_sync;

    struct ImageContent;

    impl WebRenderSync for ImageContent {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            let url = cx.url("public/image.png");
            html_sync! {
                <link rel="preload" as="image" href={url} />
                <picture>
                    <source srcset={url} />
                    <img src={url} srcset={url} alt="Image" />
                </picture>
                <video poster={url} />
                <img src="public/image.png" alt="Relative URL" />
            }
            .render(cx)
        }
    }

    async fn page(app: &Router, expected_cache_status: &str) -> String {
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, 4096).await.unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert_eq!(parts.status, StatusCode::OK, "{body}");
        assert_eq!(parts.headers["x-proa-page-cache"], expected_cache_status);
        body.to_owned()
    }

    let f = Fixture::new();
    let base = BasePath::new("/shop").unwrap();
    let cache = PageCacheLayer::new("shared");
    let app = |assets| {
        let routes = Route::page(
            "/",
            "Image",
            get(|| async { RouteResponse::ssr(ImageContent) }),
        )
        .with_page_cache(cache.clone())
        .into_router();
        FrameworkBuilder::new()
            .with_base_path(base.clone())
            .with_rsjs_assets()
            .with_public_assets(assets)
            .with_dynamic_routes(routes)
            .build()
    };
    f.write("image.png", b"old");
    let old = f.load(base.as_str());
    let old_app = app(old.clone());
    f.write("image.png", b"new");
    let new = f.load(base.as_str());
    let new_app = app(new.clone());

    assert_ne!(old.cache_key(), new.cache_key());
    let old_url = old.url("image.png").unwrap();
    let new_url = new.url("image.png").unwrap();
    assert_ne!(old_url, new_url);
    assert!(old_url.starts_with("/shop/_proa/assets/"));
    let before = page(&old_app, "MISS").await;
    assert_eq!(page(&old_app, "HIT").await, before);
    let after = page(&new_app, "MISS").await;
    assert_eq!(page(&new_app, "HIT").await, after);
    assert_eq!(before.matches(old_url).count(), 5);
    assert_eq!(after.matches(new_url).count(), 5);
    assert!(!after.contains(old_url));
    assert!(after.contains(r#"src="public/image.png""#));

    let old_path = old_url.strip_prefix(base.as_str()).unwrap();
    let new_path = new_url.strip_prefix(base.as_str()).unwrap();
    let response = get_asset(&old, old_path).await;
    assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), "old");
    let response = get_asset(&new, new_path).await;
    assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), "new");
    assert_eq!(
        get_asset(&new, old_path).await.status(),
        StatusCode::NOT_FOUND
    );
}
