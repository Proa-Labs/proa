// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use axum::{body::to_bytes, http::Method};
use bytes::Bytes;
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
    // A hashed file has one URL. Its logical name is not a second one.
    for alias in [
        "/_proa/immutable/movie.mp4",
        "/_proa/mutable/movie.mp4",
        "/_proa/assets/movie.mp4",
    ] {
        assert_eq!(
            get_asset(&assets, alias).await.status(),
            StatusCode::NOT_FOUND,
            "{alias}"
        );
    }
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
        for directory in ["immutable/", "mutable/", ""] {
            assert_eq!(
                get_asset(&assets, &format!("/_proa/{directory}{private}"))
                    .await
                    .status(),
                StatusCode::NOT_FOUND,
                "{directory}{private}"
            );
        }
    }
}

#[tokio::test]
async fn custom_router_and_buffered_render_share_catalog_urls_with_suffixes() {
    use crate::{RouteContextLayer, RouteResponse};
    use axum::Extension;
    use proa_core::{WebContext, WebRenderSync, WriteError};
    use proa_macros::html_sync;

    struct Icon;

    impl WebRenderSync for Icon {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            let fragment = cx.url("public/icons.svg#home");
            let query_and_fragment = cx.url("public/icons.svg?v=2&theme=dark#home");
            let home = cx.url("/products");
            html_sync! {
                <svg><use href={fragment}></use></svg>
                <img src={query_and_fragment} alt="" />
                <a href={home}>"Products"</a>
            }
            .render(cx)
        }
    }

    let f = Fixture::new();
    f.write("icons.svg", b"sprite");
    let assets = f.load("/shop");
    let url = assets.url("icons.svg").unwrap();
    let buffered = RouteResponse::ssr(Icon)
        .render_to_bytes_with_public_assets(&assets)
        .unwrap();
    let html = std::str::from_utf8(&buffered).unwrap();
    assert!(html.contains(&format!(r#"href="{url}#home""#)), "{html}");
    assert!(
        html.contains(&format!(r#"src="{url}?v=2&amp;theme=dark#home""#)),
        "{html}"
    );
    assert!(html.contains(r#"href="/shop/products""#), "{html}");

    let key = assets.cache_key().to_owned();
    let app = Router::new()
        .route(
            "/",
            get(move |Extension(assets): Extension<PublicAssets>| {
                let key = key.clone();
                async move {
                    assert_eq!(assets.cache_key(), key);
                    RouteResponse::ssr(Icon)
                }
            }),
        )
        .merge(assets.router())
        .layer(RouteContextLayer::new(Default::default()).with_public_assets(assets.clone()));
    let page = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(to_bytes(page.into_body(), 1024).await.unwrap(), buffered);
    let asset = app
        .oneshot(
            Request::builder()
                .uri(format!("{}?v=2", url.strip_prefix("/shop").unwrap()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(to_bytes(asset.into_body(), 1024).await.unwrap(), "sprite");
}

#[tokio::test]
async fn files_are_served_as_built_under_any_base_path() {
    let f = Fixture::new();
    // Parser fixtures, not application-authored browser behavior.
    let script = b"import './dependency.js'; fetch('/api/data');";
    f.write("main.js", script);
    f.write("images/a.png", b"image");
    f.write("styles.css", b"a { background: url('images/a.png'); }");
    let root = f.load("");
    let mounted = f.load("/apps/demo");
    // The key covers the URLs a page holds, which carry the base path.
    assert_ne!(root.cache_key(), mounted.cache_key());
    assert_eq!(
        mounted.url("main.js"),
        Some("/apps/demo/_proa/mutable/main.js")
    );
    for logical in ["main.js", "styles.css"] {
        let url = root.url(logical).unwrap();
        assert_eq!(
            mounted.url(logical).unwrap().strip_prefix("/apps/demo"),
            Some(url)
        );
        let original = get_asset(&root, url).await;
        let response = get_asset(&mounted, url).await;
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG].clone();
        assert_eq!(etag, original.headers()[header::ETAG]);
        let cache = response.headers()[header::CACHE_CONTROL].clone();
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(
            bytes,
            to_bytes(original.into_body(), 1024).await.unwrap(),
            "{logical}"
        );
        assert_eq!(etag, format!("\"{}\"", proa_assets::sha256(&bytes)));
        if logical == "main.js" {
            assert_eq!(cache, "public, no-cache");
            assert_eq!(bytes.as_ref(), script);
        } else {
            // The stylesheet is hashed, and reaches the image by a path
            // relative to itself, so no host or mount appears in its bytes.
            assert_eq!(cache, "public, max-age=31536000, immutable");
            let image = root.url("images/a.png").unwrap();
            let image = image.strip_prefix("/_proa/immutable/").unwrap();
            assert_eq!(
                std::str::from_utf8(&bytes).unwrap(),
                format!("a {{ background: url(\"{image}\"); }}")
            );
        }
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
    assert!(old_url.starts_with("/shop/_proa/immutable/"));
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

#[tokio::test]
async fn extensionless_files_named_css_or_js_are_not_rewritten_with_base_path() {
    let f = Fixture::new();
    f.write("css", &[0xff, 0xfe, 0x00, 0x01]);
    f.write("nested/js", &[0x80, 0x81, 0x82]);
    let assets = f.load("/shop");
    for (logical, expected) in [
        ("css", &[0xff_u8, 0xfe, 0x00, 0x01][..]),
        ("nested/js", &[0x80_u8, 0x81, 0x82][..]),
    ] {
        let url = assets.url(logical).expect(logical);
        let path = url.strip_prefix("/shop").unwrap();
        let response = get_asset(&assets, path).await;
        assert_eq!(response.status(), StatusCode::OK, "{logical}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{logical}"
        );
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
            expected,
            "{logical}"
        );
    }
}

/// A `proa dev` style output: publish, then follow republication in place.
struct LiveFixture {
    f: Fixture,
}

impl LiveFixture {
    fn new() -> Self {
        Self { f: Fixture::new() }
    }

    fn output(&self) -> std::path::PathBuf {
        self.f.temp.path().join("output")
    }

    fn publish(&self) {
        OutputStore::open(self.output())
            .unwrap()
            .publish(self.f.temp.path().join("public"))
            .unwrap();
    }

    fn remove(&self, name: &str) {
        std::fs::remove_file(self.f.temp.path().join("public").join(name)).unwrap();
    }

    fn load_live(&self, base: &str) -> PublicAssets {
        PublicAssets::load_live(self.output(), &BasePath::new(base).unwrap()).unwrap()
    }

    /// Age the manifest past the racy window so its stamp alone is trusted.
    fn settle_manifest(&self) {
        let manifest = std::fs::File::options()
            .write(true)
            .open(self.output().join(MANIFEST_FILE))
            .unwrap();
        manifest
            .set_modified(SystemTime::now() - RACY_MANIFEST_WINDOW * 5)
            .unwrap();
    }
}

async fn body(assets: &PublicAssets, url: &str) -> (StatusCode, Bytes) {
    let response = get_asset(assets, url).await;
    let status = response.status();
    (status, to_bytes(response.into_body(), 4096).await.unwrap())
}

#[tokio::test]
async fn live_catalog_follows_republication_and_pins_each_request_to_one_generation() {
    let live = LiveFixture::new();
    live.f.write("favicon.svg", b"<svg>A</svg>");
    live.f.write("data.json", b"{\"a\":1}");
    live.f.write("old-name.png", b"png");
    live.publish();

    let assets = live.load_live("");
    assert!(OutputStore::open(live.output())
        .unwrap()
        .has_live_reader()
        .unwrap());
    let before = assets.current();
    let first_url = before.url("favicon.svg").unwrap().to_owned();
    assert_eq!(
        body(&assets, &first_url).await,
        (StatusCode::OK, Bytes::from_static(b"<svg>A</svg>"))
    );
    let data = body(&assets, "/_proa/mutable/data.json").await;
    assert_eq!(data.1, "{\"a\":1}");

    // A same-length edit keeps the manifest size and can keep its timestamp.
    // A stable-URL file changes bytes only. A rename drops one logical path.
    live.f.write("favicon.svg", b"<svg>B</svg>");
    live.f.write("data.json", b"{\"a\":2}");
    live.remove("old-name.png");
    live.f.write("new-name.png", b"png");
    live.publish();

    let after = assets.current();
    let second_url = after.url("favicon.svg").unwrap().to_owned();
    assert_ne!(second_url, first_url);
    assert_ne!(after.cache_key(), before.cache_key());
    // The long-lived handle follows; a handle taken earlier does not move.
    assert_eq!(
        assets.render_urls().get("favicon.svg"),
        Some(second_url.as_str())
    );
    assert_eq!(before.url("favicon.svg"), Some(first_url.as_str()));
    assert_eq!(
        before.render_urls().get("favicon.svg"),
        Some(first_url.as_str())
    );
    assert_eq!(before.current().cache_key(), before.cache_key());

    assert_eq!(
        body(&assets, &second_url).await,
        (StatusCode::OK, Bytes::from_static(b"<svg>B</svg>"))
    );
    // The active router exposes only the active manifest.
    assert_eq!(body(&assets, &first_url).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        body(&assets, "/_proa/mutable/data.json").await.1,
        "{\"a\":2}"
    );
    assert_eq!(after.url("old-name.png"), None);
    assert!(after.url("new-name.png").is_some());
    // The earlier generation still serves its own files to a request that
    // started before the publication.
    assert_eq!(
        body(&before, &first_url).await,
        (StatusCode::OK, Bytes::from_static(b"<svg>A</svg>"))
    );
}

#[tokio::test]
async fn live_catalog_trusts_a_settled_stamp_and_still_sees_the_next_publication() {
    let live = LiveFixture::new();
    live.f.write("logo.svg", b"one");
    live.publish();
    let assets = live.load_live("");
    let watcher = assets.live.as_ref().unwrap();

    // Fresh manifest: the stamp is not trusted, bytes are compared instead,
    // and an identical republication is not mistaken for a new catalog.
    let first = watcher.current();
    assert!(watcher.state.read().unwrap().settled.is_none());
    live.publish();
    assert!(Arc::ptr_eq(&first, &watcher.current()));

    live.settle_manifest();
    assert!(Arc::ptr_eq(&first, &watcher.current()));
    assert!(watcher.state.read().unwrap().settled.is_some());
    // Settled: answered from metadata, without rereading the manifest.
    assert!(Arc::ptr_eq(&first, &watcher.current()));

    live.f.write("logo.svg", b"two");
    live.publish();
    let second = assets.current();
    assert_ne!(second.url("logo.svg"), first.urls.get("logo.svg"));
    assert_eq!(
        body(&assets, second.url("logo.svg").unwrap()).await.1,
        "two"
    );
}

#[tokio::test]
async fn live_catalog_keeps_serving_through_a_broken_manifest_and_recovers() {
    let live = LiveFixture::new();
    live.f.write("logo.svg", b"one");
    live.publish();
    let assets = live.load_live("");
    let url = assets.current().url("logo.svg").unwrap().to_owned();
    let manifest = live.output().join(MANIFEST_FILE);
    let valid = std::fs::read(&manifest).unwrap();

    std::fs::write(&manifest, b"{ not json").unwrap();
    assert_eq!(body(&assets, &url).await.1, "one");
    // The broken generation is attempted once, not once per request.
    let rejected = assets
        .live
        .as_ref()
        .unwrap()
        .state
        .read()
        .unwrap()
        .rejected
        .clone();
    assert_eq!(rejected, Some(proa_assets::sha256(b"{ not json")));
    assert_eq!(body(&assets, &url).await.1, "one");

    std::fs::remove_file(&manifest).unwrap();
    assert_eq!(body(&assets, &url).await.1, "one");

    std::fs::write(&manifest, valid).unwrap();
    live.f.write("logo.svg", b"two");
    live.publish();
    let next = assets.current();
    assert_ne!(next.url("logo.svg"), Some(url.as_str()));
    assert_eq!(body(&assets, next.url("logo.svg").unwrap()).await.1, "two");
}

#[tokio::test]
async fn concurrent_requests_during_publication_all_land_on_a_complete_catalog() {
    let live = LiveFixture::new();
    live.f.write("logo.svg", b"0");
    live.publish();
    let assets = live.load_live("");
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let assets = assets.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut seen = 0_usize;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let current = assets.current();
                    // Every handle is internally consistent: its URL exists in
                    // its own route table.
                    let url = current.url("logo.svg").expect("active logical path");
                    let route = url.strip_prefix("/_proa/").unwrap();
                    assert!(current.catalog.routes.contains_key(route));
                    seen += 1;
                }
                seen
            })
        })
        .collect();
    for generation in 1..=5 {
        live.f.write("logo.svg", generation.to_string().as_bytes());
        live.publish();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for reader in readers {
        assert!(reader.join().unwrap() > 0);
    }
    let last = assets.current();
    assert_eq!(body(&assets, last.url("logo.svg").unwrap()).await.1, "5");
}

#[tokio::test]
async fn live_page_cache_and_request_extension_follow_one_generation() {
    use crate::{route::PageCacheLayer, FrameworkBuilder, Route, RouteResponse};
    use axum::Extension;
    use proa_core::{WebContext, WebRenderSync, WriteError};
    use proa_macros::html_sync;

    struct Image;

    impl WebRenderSync for Image {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            html_sync! {
                <img src={cx.url("public/image.png")} alt="Image" />
            }
            .render(cx)
        }
    }

    async fn page(app: &Router) -> (String, String, String) {
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, 4096).await.unwrap();
        (
            parts.headers["x-proa-page-cache"]
                .to_str()
                .unwrap()
                .to_owned(),
            parts.headers["x-test-asset-key"]
                .to_str()
                .unwrap()
                .to_owned(),
            String::from_utf8(body.to_vec()).unwrap(),
        )
    }

    let live = LiveFixture::new();
    live.f.write("image.png", b"old");
    live.publish();
    let assets = live.load_live("");
    let routes = Route::page(
        "/",
        "Image",
        get(|Extension(assets): Extension<PublicAssets>| async move {
            // What a handler reads from the extension describes the same
            // catalog its page renders with.
            let key = assets.cache_key().to_owned();
            let url = assets.url("image.png").unwrap().to_owned();
            let mut response =
                axum::response::IntoResponse::into_response(RouteResponse::ssr(Image));
            response
                .headers_mut()
                .insert("x-test-asset-key", key.parse().unwrap());
            response
                .headers_mut()
                .insert("x-test-asset-url", url.parse().unwrap());
            response
        }),
    )
    .with_page_cache(PageCacheLayer::new("live"))
    .into_router();
    let app = FrameworkBuilder::new()
        .with_public_assets(assets.clone())
        .with_dynamic_routes(routes)
        .build();

    let old = assets.current();
    let (status, key, html) = page(&app).await;
    assert_eq!(status, "MISS");
    assert_eq!(key, old.cache_key());
    assert!(html.contains(old.url("image.png").unwrap()), "{html}");
    assert_eq!(page(&app).await.0, "HIT");

    live.f.write("image.png", b"new");
    live.publish();
    let new = assets.current();
    let (status, key, html) = page(&app).await;
    assert_eq!(
        status, "MISS",
        "a republished catalog must not reuse cached HTML"
    );
    assert_eq!(key, new.cache_key());
    assert!(html.contains(new.url("image.png").unwrap()), "{html}");
    assert!(!html.contains(old.url("image.png").unwrap()), "{html}");
    assert_eq!(page(&app).await.0, "HIT");
}

#[tokio::test]
async fn live_loading_degrades_to_a_fixed_catalog_without_writer_support() {
    let live = LiveFixture::new();
    live.f.write("logo.svg", b"one");
    live.publish();
    // An output prepared by a CLI that always restarts the server.
    std::fs::remove_file(live.output().join(".live-readers.lock")).unwrap();
    let assets = live.load_live("/shop");
    assert!(assets.live.is_none());
    assert_eq!(assets.current().cache_key(), assets.cache_key());
    let url = assets.url("logo.svg").unwrap();
    assert_eq!(
        body(&assets, url.strip_prefix("/shop").unwrap()).await.1,
        "one"
    );

    // A fixed catalog never claims it can follow publication.
    let fixed = live.f.load("");
    let writer = OutputStore::open(live.output()).unwrap();
    assert!(!writer.has_live_reader().unwrap());
    drop(fixed);
}

#[tokio::test]
async fn managed_head_resolves_catalog_urls_once_under_a_deployment_mount() {
    use crate::{FrameworkBuilder, RouteResponse};
    use proa_core::{
        ComponentRole, HeadConfig, HeadUrlPolicy, SyncHeadOperation, WebContext, WebRenderSync,
        WriteError,
    };
    use proa_macros::html_sync;

    struct Page;

    impl WebRenderSync for Page {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
            let card = cx.url("public/og/card.png?v=2");
            let icon = cx.url("public/favicon.svg");
            let canonical = cx.url("/jacket");
            let mut head = cx.head_mut();
            head.title("Jacket")?;
            head.og_image(card)?;
            head.icon(icon)?;
            head.canonical(canonical)
        }
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            html_sync! {
                <html>
                    <head>{cx.head()}</head>
                    <body>
                        <img src={cx.url("public/og/card.png")} alt="Card" />
                    </body>
                </html>
            }
            .render(cx)
        }
    }

    for mount in ["", "/shop"] {
        let f = Fixture::new();
        f.write("og/card.png", b"card");
        f.write("favicon.svg", b"icon");
        let assets = f.load(mount);
        let mount_arg = if mount.is_empty() { "/" } else { mount };
        let app = FrameworkBuilder::new()
            .with_base_path(BasePath::new(mount).unwrap())
            .with_public_assets(assets.clone())
            .with_head(
                HeadConfig::default()
                    .with_urls(HeadUrlPolicy::new("https://shop.example", mount_arg).unwrap()),
            )
            .with_dynamic_routes(
                Router::new().route("/", get(|| async { RouteResponse::ssr(Page) })),
            )
            .build();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let html = to_bytes(response.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&html).unwrap();

        // The head's absolute URL is the page's own URL on the public origin:
        // the same path the body links to, with the mount exactly once.
        let card = assets.url("og/card.png").unwrap();
        let icon = assets.url("favicon.svg").unwrap();
        assert!(
            card.starts_with(&format!("{mount}/_proa/immutable/")),
            "{card}"
        );
        assert!(html.contains(&format!(r#"src="{card}""#)), "{html}");
        for expected in [
            format!(r#"property="og:image" content="https://shop.example{card}?v=2""#),
            format!(r#"rel="icon" href="https://shop.example{icon}""#),
            format!(r#"rel="canonical" href="https://shop.example{mount}/jacket""#),
        ] {
            assert!(
                html.contains(&expected),
                "{mount:?}: missing {expected} in {html}"
            );
        }
    }
}

#[test]
fn a_catalog_bundled_beside_the_executable_is_found_without_configuration() {
    let f = Fixture::new();
    f.write("img/logo.png", b"logo");
    let executable = f.temp.path().join("dist/app");
    assert_eq!(bundled_catalog(&executable), None);

    let output = f.temp.path().join("dist/app.assets");
    // A directory Proa did not write, such as the application's own data
    // directory or a pre-created mount point, is not a bundle.
    std::fs::create_dir_all(&output).unwrap();
    assert_eq!(bundled_catalog(&executable), None);
    std::fs::write(output.join("data.json"), "{}").unwrap();
    assert_eq!(bundled_catalog(&executable), None);
    std::fs::remove_file(output.join("data.json")).unwrap();
    OutputStore::open(&output)
        .unwrap()
        .publish(f.temp.path().join("public"))
        .unwrap();
    assert_eq!(bundled_catalog(&executable), Some(output.clone()));
    let assets = PublicAssets::load(&output, &BasePath::new("").unwrap()).unwrap();
    assert!(assets.url("img/logo.png").is_some());
    drop(assets);

    // Another executable in the same directory has no claim on it.
    assert_eq!(bundled_catalog(&f.temp.path().join("dist/other")), None);

    // A bundle that lost its manifest in transit is still found, so loading
    // it fails startup instead of serving without assets.
    let base_path = BasePath::new("").unwrap();
    drop(PublicAssets::load(&output, &base_path).unwrap());
    std::fs::remove_file(output.join("manifest.json")).unwrap();
    assert_eq!(bundled_catalog(&executable), Some(output.clone()));
    assert!(PublicAssets::load(&output, &base_path).is_err());
}

#[tokio::test]
async fn icons_browsers_request_on_their_own_are_answered_at_the_root() {
    let f = Fixture::new();
    f.write("favicon.ico", b"icon");
    f.write("apple-touch-icon.png", b"touch");
    f.write("robots.txt", b"not a root file");
    let assets = f.load("/shop");
    let own = Router::new().route("/apple-touch-icon.png", get(|| async { "the app's own" }));
    let app = assets.serve_root_files(own.merge(assets.router()));
    let get = |path: &'static str, method: Method| {
        app.clone().oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
    };

    let icon = get("/favicon.ico", Method::GET).await.unwrap();
    assert_eq!(icon.status(), StatusCode::OK);
    assert_eq!(icon.headers()[header::CACHE_CONTROL], "public, no-cache");
    assert_eq!(icon.headers()[header::CONTENT_TYPE], "image/x-icon");
    let etag = icon.headers()[header::ETAG].clone();
    assert_eq!(to_bytes(icon.into_body(), 1024).await.unwrap(), "icon");
    let head = get("/favicon.ico", Method::HEAD).await.unwrap();
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "4");
    // It is the same file the catalog serves under its hashed name.
    let hashed = assets
        .url("favicon.ico")
        .unwrap()
        .strip_prefix("/shop")
        .unwrap();
    let hashed = get_asset(&assets, hashed).await;
    assert_eq!(hashed.headers()[header::ETAG], etag);
    assert_eq!(
        hashed.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    // The application's own route at such a path is left alone, and no other
    // public file gains a root URL.
    let own = get("/apple-touch-icon.png", Method::GET).await.unwrap();
    assert_eq!(
        to_bytes(own.into_body(), 1024).await.unwrap(),
        "the app's own"
    );
    for absent in ["/robots.txt", "/apple-touch-icon-precomposed.png"] {
        assert_eq!(
            get(absent, Method::GET).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        get("/favicon.ico", Method::POST).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_bundle_serves_the_same_urls_as_a_store() {
    let f = Fixture::new();
    f.write("images/a.png", b"image");
    f.write("styles.css", b"a { background: url(images/a.png) }");
    f.write("data.json", b"{}");
    let store = f.load("");
    let bundle = f.temp.path().join("app.assets");
    proa_assets::build::export(&bundle, Some(&f.temp.path().join("public")), &[]).unwrap();
    let bundled = PublicAssets::load(&bundle, &BasePath::default()).unwrap();
    for logical in ["images/a.png", "styles.css", "data.json"] {
        let url = store.url(logical).unwrap();
        assert_eq!(bundled.url(logical), Some(url));
        let (expected, served) = (get_asset(&store, url).await, get_asset(&bundled, url).await);
        assert_eq!(served.status(), StatusCode::OK, "{logical}");
        for name in [header::CACHE_CONTROL, header::ETAG, header::CONTENT_TYPE] {
            assert_eq!(served.headers()[&name], expected.headers()[&name]);
        }
        assert_eq!(
            to_bytes(served.into_body(), 1024).await.unwrap(),
            to_bytes(expected.into_body(), 1024).await.unwrap()
        );
    }
}

#[test]
fn an_asset_prefix_is_a_url_or_a_root_path_and_nothing_that_could_break_markup() {
    let hosted = AssetPrefix::new("https://cdn.example.com/site/").unwrap();
    assert_eq!(hosted.as_str(), "https://cdn.example.com/site");
    assert_eq!(hosted.origin(), Some("https://cdn.example.com"));
    let with_port = AssetPrefix::new("http://localhost:9000").unwrap();
    assert_eq!(with_port.origin(), Some("http://localhost:9000"));
    // A path on this origin, such as a proxy route to a bucket, needs no
    // entry in a content security policy.
    let path = AssetPrefix::new("/static-host").unwrap();
    assert_eq!((path.as_str(), path.origin()), ("/static-host", None));
    for invalid in [
        "",
        "/",
        "//cdn.example.com",
        "/cdn/../x",
        "/cdn/./x",
        "/cdn//x",
        "cdn.example.com",
        "ftp://cdn.example.com",
        "https:cdn.example.com",
        "https:/cdn.example.com",
        "https://user:secret@cdn.example.com",
        "https://cdn.example.com/?v=1",
        "https://cdn.example.com/#x",
        "https://cdn.example.com/a b",
        "https://cdn.example.com/\"><script>",
        "https://cdn.example.com; script-src *",
    ] {
        assert!(AssetPrefix::new(invalid).is_err(), "{invalid}");
    }
}

#[tokio::test]
async fn hosted_files_are_linked_from_the_asset_host_and_keyed_by_it() {
    let f = Fixture::new();
    f.write("images/a b.png", b"image");
    let local = f.load("/shop");
    let prefix = AssetPrefix::new("https://cdn.example.com/site").unwrap();
    let hosted = PublicAssets::load_hosted(
        f.temp.path().join("output"),
        &BasePath::new("/shop").unwrap(),
        Some(&prefix),
    )
    .unwrap();
    let url = hosted.url("images/a b.png").unwrap();
    assert_eq!(
        url.strip_prefix("https://cdn.example.com/site"),
        local.url("images/a b.png").unwrap().strip_prefix("/shop")
    );
    assert!(url.contains("/_proa/immutable/images/a%20b."));
    // A cached page holds these URLs, so the host is part of its key.
    assert_ne!(hosted.cache_key(), local.cache_key());
    // The files are the same either way, and this server still has them.
    let path = url.strip_prefix("https://cdn.example.com/site").unwrap();
    assert_eq!(get_asset(&hosted, path).await.status(), StatusCode::OK);
}
