// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get,
};
use proa_core::ctx::Ctx;
use proa_core::text::{text, StaticRaw};
use proa_core::WriteError;
use tower::ServiceExt;

use crate::manifest::IslandManifest;

use super::context::RouteContextLayer;
use super::layout::Layout;
use super::metadata::{
    render_metadata_head, CascadeOpenGraph, CascadeTwitter, CascadeValue, Metadata, OpenGraphImage,
    TwitterCard,
};
use super::response::RouteResponse;
use super::spec::{router_from_spec, Route};

struct PassthroughLayout;
impl Metadata for PassthroughLayout {}
impl Layout for PassthroughLayout {}

#[tokio::test]
async fn endpoint_runs_under_metadata_layout() {
    let routes = vec![Route::layout(
        "/",
        &PassthroughLayout,
        vec![Route::endpoint(
            "dashboard",
            get(|| async { RouteResponse::ssr(StaticRaw("ok")) }),
        )],
    )];

    let manifest = Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    });

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/dashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), b"ok");
}

struct BaseMetadata;
impl Metadata for BaseMetadata {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Base Title")
    }

    fn description(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Base Description")
    }

    fn open_graph(&self) -> CascadeOpenGraph {
        static IMAGES: [OpenGraphImage; 1] = [OpenGraphImage::new("https://example.com/base.png")];
        CascadeOpenGraph {
            title: CascadeValue::Set("Base OG Title"),
            description: CascadeValue::Set("Base OG Description"),
            images: CascadeValue::Set(&IMAGES),
            ..Default::default()
        }
    }

    fn twitter(&self) -> CascadeTwitter {
        CascadeTwitter {
            card: CascadeValue::Set(TwitterCard::SummaryLargeImage),
            site: CascadeValue::Set("@proa"),
            ..Default::default()
        }
    }
}

struct PageMetadata;
impl Metadata for PageMetadata {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Page Title")
    }

    fn open_graph(&self) -> CascadeOpenGraph {
        static IMAGES: [OpenGraphImage; 1] = [OpenGraphImage::new("https://example.com/page.png")
            .with_dimensions(1200, 630)
            .with_alt("Preview")];
        CascadeOpenGraph {
            title: CascadeValue::Set("Page OG Title"),
            images: CascadeValue::Set(&IMAGES),
            ..Default::default()
        }
    }
}

struct MetadataLayout;
impl Metadata for MetadataLayout {}
impl Layout for MetadataLayout {}

struct MetadataPage;
impl proa_core::WebRenderSync for MetadataPage {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        proa_core::WebRenderSync::render(StaticRaw("<!DOCTYPE html><html><head>"), cx)?;
        render_metadata_head(cx);
        proa_core::WebRenderSync::render(StaticRaw("</head><body>"), cx)?;
        proa_core::WebRenderSync::render(text("content"), cx)?;
        proa_core::WebRenderSync::render(StaticRaw("</body></html>"), cx)
    }
}

#[tokio::test]
async fn metadata_is_composed() {
    let routes = vec![Route::layout(
        "/",
        &MetadataLayout,
        vec![
            Route::endpoint("", get(|| async { RouteResponse::ssr(MetadataPage) }))
                .with_metadata(&PageMetadata),
        ],
    )
    .with_metadata(&BaseMetadata)];

    let manifest = Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    });

    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("<title>Page Title</title>"));
    assert!(html.contains("name=\"description\" content=\"Base Description\""));
    assert!(html.contains("property=\"og:title\" content=\"Page OG Title\""));
    assert!(html.contains("property=\"og:image\" content=\"https://example.com/base.png\""));
    assert!(html.contains("property=\"og:image\" content=\"https://example.com/page.png\""));
    assert!(html.contains("property=\"og:image:width\" content=\"1200\""));
    assert!(html.contains("property=\"og:image:alt\" content=\"Preview\""));
    assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
    assert!(html.contains("name=\"twitter:site\" content=\"@proa\""));
    assert!(html.contains("name=\"twitter:title\" content=\"Page OG Title\""));
    assert!(html.contains("name=\"twitter:description\" content=\"Base OG Description\""));
    assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/base.png\""));
}

#[tokio::test]
async fn page_owned_cache_survives_endpoint_replacement_and_router_merge() {
    use crate::{FrameworkBuilder, PageCacheLayer};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let cached_calls = Arc::new(AtomicUsize::new(0));
    let uncached_calls = Arc::new(AtomicUsize::new(0));
    let count = cached_calls.clone();
    let cached = Route::page("/cached", "Cached page", get(|| async { "replaced" }))
        .with_page_cache(PageCacheLayer::new("build"))
        .with_endpoint(get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { RouteResponse::ssr(MetadataPage) }
        }));
    let count = uncached_calls.clone();
    let uncached = Route::endpoint(
        "/uncached",
        get(move || {
            let n = count.fetch_add(1, Ordering::SeqCst);
            async move { n.to_string() }
        }),
    );
    let pages = axum::Router::new()
        .merge(cached.into_router())
        .merge(uncached.into_router());
    let app = FrameworkBuilder::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    })
    .with_dynamic_routes(pages)
    .build();

    for expected in ["MISS", "HIT"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/cached")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["x-proa-page-cache"], expected);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(std::str::from_utf8(&bytes)
            .unwrap()
            .contains("<title>Cached page</title>"));
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/uncached")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    assert_eq!(cached_calls.load(Ordering::SeqCst), 1);
    assert_eq!(uncached_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn nested_page_refresh_preserves_metadata_and_does_not_cache_siblings() {
    use crate::{FrameworkBuilder, PageCacheLayer};
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let child = Route::page(
        "cached",
        "Child title",
        get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { RouteResponse::ssr(MetadataPage) }
        }),
    )
    .with_page_cache(PageCacheLayer::new("build").revalidate(Duration::from_secs(10)));
    let sibling = Route::endpoint("sibling", get(|| async { "uncached" }));
    let parent = Route::layout("/group", &MetadataLayout, vec![child, sibling])
        .with_metadata(&BaseMetadata)
        .with_page_cache(PageCacheLayer::new("unused-layout-policy"));
    let app = FrameworkBuilder::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    })
    .with_dynamic_routes(axum::Router::new().merge(parent.into_router()))
    .build();
    let fetch = || {
        app.clone().oneshot(
            Request::builder()
                .uri("/group/cached")
                .body(Body::empty())
                .unwrap(),
        )
    };
    assert_eq!(
        fetch().await.unwrap().headers()["x-proa-page-cache"],
        "MISS"
    );
    tokio::time::advance(Duration::from_secs(11)).await;
    assert_eq!(
        fetch().await.unwrap().headers()["x-proa-page-cache"],
        "STALE"
    );
    tokio::task::yield_now().await;
    let refreshed = fetch().await.unwrap();
    assert_eq!(refreshed.headers()["x-proa-page-cache"], "HIT");
    let bytes = to_bytes(refreshed.into_body(), usize::MAX).await.unwrap();
    let html = std::str::from_utf8(&bytes).unwrap();
    assert!(html.contains("<title>Child title</title>"));
    assert!(html.contains("name=\"description\" content=\"Base Description\""));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let sibling = app
        .oneshot(
            Request::builder()
                .uri("/group/sibling")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!sibling.headers().contains_key("x-proa-page-cache"));
}
