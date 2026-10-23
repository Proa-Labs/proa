// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{router_from_spec, Layout, Route, RouteResponse};
use crate::manifest::IslandManifest;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get,
};
use proa_core::text::{text, StaticRaw};
use proa_core::{ctx::Ctx, DynLoaderHandle, Metadata, WebContext, WebRenderSync, WriteError};
use std::sync::Arc;
use tower::ServiceExt;

struct MetadataLayout;
impl Layout for MetadataLayout {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(
            Metadata::new()
                .title("Site")
                .title_template("%s | Site")
                .description("Base Description")
                .open_graph(|og| {
                    og.site_name("Site");
                }),
        );
        // Setup uses the live request/response, not a copied metadata context.
        if !cx.response().streaming_started() {
            cx.response_mut().insert_header(
                http::HeaderName::from_static("x-layout"),
                http::HeaderValue::from_static("setup"),
            );
        }
        Ok(())
    }
}
struct Section;
impl Layout for Section {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title_template("%s | Section"));
        Ok(())
    }
}
struct MetadataPage;
impl WebRenderSync for MetadataPage {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        if let Some(slug) = cx.param("slug") {
            let variant = cx.request().headers()["x-preview"].to_str().unwrap();
            let query = cx.request().location().unwrap().query().unwrap();
            cx.metadata(
                Metadata::new()
                    .title(format!("{slug} {variant}"))
                    .description(query.to_owned()),
            );
        }
        StaticRaw("<!DOCTYPE html><html><head>").render(cx)?;
        super::render_metadata_head(cx)?;
        StaticRaw("</head><body>").render(cx)?;
        text("content").render(cx)?;
        StaticRaw("</body></html>").render(cx)
    }
}
fn manifest() -> IslandManifest {
    IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    }
}

#[tokio::test]
async fn route_layouts_use_live_context_in_order_and_page_declarations_override_legacy_title() {
    for stream in [false, true] {
        let page = if stream {
            Route::page(
                "blog/:slug",
                "legacy",
                get(|| async { RouteResponse::ssr_stream(proa_core::to_async(MetadataPage)) }),
            )
        } else {
            Route::page(
                "blog/:slug",
                "legacy",
                get(|| async { RouteResponse::ssr(MetadataPage) }),
            )
        };
        let (router, plans) = router_from_spec(vec![Route::layout(
            "/",
            &MetadataLayout,
            vec![Route::layout("", &Section, vec![page])],
        )]);
        let app = crate::FrameworkBuilder::new(manifest())
            .with_dynamic_routes_and_setups(router, plans)
            .build();
        let fetch = |slug: &'static str| {
            let app = app.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::builder()
                            .uri(format!("/blog/{slug}?q=A&B"))
                            .header("x-preview", slug)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                if !stream {
                    assert_eq!(response.headers()["x-layout"], "setup");
                }
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let html = std::str::from_utf8(&bytes).unwrap();
                assert!(
                    html.contains(&format!("<title>{slug} {slug} | Section</title>")),
                    "{html}"
                );
                assert!(html.contains("content=\"q=A&amp;B\""));
                assert!(html.contains("og:site_name\" content=\"Site\""));
                assert!(!html.contains("legacy"));
            }
        };
        tokio::join!(fetch("first"), fetch("second"));
    }
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
    assert!(html.contains("<title>Child title | Site</title>"));
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
