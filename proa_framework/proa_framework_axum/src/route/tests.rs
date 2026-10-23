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
    render_metadata_head, CascadeOpenGraph, CascadeValue, Metadata, OpenGraphImage,
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
    assert!(!html.contains("https://example.com/base.png"));
    assert!(html.contains("property=\"og:image\" content=\"https://example.com/page.png\""));
    assert!(html.contains("property=\"og:image:width\" content=\"1200\""));
    assert!(html.contains("property=\"og:image:alt\" content=\"Preview\""));
}

struct RequestDrivenMetadata;
impl Metadata for RequestDrivenMetadata {
    fn metadata(
        &self,
        cx: &dyn super::MetadataContext,
        _parent: &super::ResolvedMetadata,
    ) -> super::MetadataPatch {
        let slug = cx.param("slug").unwrap();
        let variant = cx.request().headers()["x-preview"].to_str().unwrap();
        let query = cx
            .search_params()
            .find(|(key, _)| key == "q")
            .unwrap()
            .1
            .into_owned();
        super::MetadataPatch {
            title: CascadeValue::Set(format!("{slug} {variant}").into()),
            description: CascadeValue::Set(query.into()),
            canonical: CascadeValue::Set(format!("/blog/{slug}").into()),
            ..Default::default()
        }
    }
}

struct AsyncRequestDrivenMetadata;
impl Metadata for AsyncRequestDrivenMetadata {
    async fn generate_metadata(
        &self,
        cx: &dyn super::MetadataContext,
        parent: &super::ResolvedMetadata,
    ) -> Result<super::MetadataPatch, super::MetadataError> {
        tokio::task::yield_now().await;
        Ok(RequestDrivenMetadata.metadata(cx, parent))
    }
}

fn request_metadata_app(stream: bool) -> axum::Router {
    let route = if stream {
        Route::page_with_metadata(
            "/blog/:slug",
            get(|| async { RouteResponse::ssr_stream(proa_core::to_async(MetadataPage)) }),
            &AsyncRequestDrivenMetadata,
        )
    } else {
        Route::page_with_metadata(
            "/blog/:slug",
            get(|| async { RouteResponse::ssr(MetadataPage) }),
            &RequestDrivenMetadata,
        )
    };
    let (router, plans) = router_from_spec(vec![route]);
    crate::FrameworkBuilder::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    })
    .with_dynamic_routes_and_metadata(router, plans)
    .with_metadata_base(super::MetadataBase::new("https://staging.example/app").unwrap())
    .build()
}

#[tokio::test]
async fn dynamic_route_metadata_reads_request_and_uses_deployment_or_middleware_base() {
    let app = request_metadata_app(false);
    for (slug, middleware_base) in [
        ("first", None),
        ("second", Some("https://preview.example/mount")),
    ] {
        let mut request = Request::builder()
            .uri(format!("/blog/{slug}?q=A%26B"))
            .header("x-preview", "draft")
            .body(Body::empty())
            .unwrap();
        if let Some(base) = middleware_base {
            request
                .extensions_mut()
                .insert(super::MetadataBase::new(base).unwrap());
        }
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(
            html.contains(&format!("<title>{slug} draft</title>")),
            "{html}"
        );
        assert!(html.contains("content=\"A&amp;B\""));
        let base = middleware_base.unwrap_or("https://staging.example/app");
        assert!(
            html.contains(&format!("href=\"{base}/blog/{slug}\"")),
            "{html}"
        );
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/blog/client?q=x")
                .header("x-preview", "published")
                .header("x-proa-client", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["title"], "client published");
}

#[tokio::test]
async fn streamed_metadata_survives_endpoint_context_teardown_and_concurrent_requests() {
    let app = request_metadata_app(true);
    let fetch = |slug: &'static str| {
        let app = app.clone();
        async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .uri(format!("/blog/{slug}?q={slug}"))
                        .header("x-preview", slug)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert!(
                html.contains(&format!("<title>{slug} {slug}</title>")),
                "{html}"
            );
            assert!(
                html.contains(&format!("href=\"https://staging.example/app/blog/{slug}\"")),
                "{html}"
            );
        }
    };
    tokio::join!(fetch("first"), fetch("second"));
}

struct LocalRequestDrivenMetadata;
impl super::LocalMetadata for LocalRequestDrivenMetadata {
    async fn generate_metadata(
        &self,
        cx: &dyn super::MetadataContext,
        parent: &super::ResolvedMetadata,
    ) -> Result<super::MetadataPatch, super::MetadataError> {
        let metadata = std::rc::Rc::new(RequestDrivenMetadata.metadata(cx, parent));
        tokio::task::yield_now().await;
        Ok(std::rc::Rc::try_unwrap(metadata).ok().unwrap())
    }
}

static LOCAL_METADATA_LAYOUT: super::LocalMetadataProvider<LocalRequestDrivenMetadata> =
    super::local_metadata(LocalRequestDrivenMetadata);

#[tokio::test]
async fn native_local_generators_work_as_layouts_and_fail_before_send_response_bytes() {
    let (router, plans) = router_from_spec(vec![Route::layout(
        "/blog",
        &LOCAL_METADATA_LAYOUT,
        vec![
            Route::endpoint(
                ":slug",
                get(|| async { RouteResponse::ssr_stream(proa_core::to_async(MetadataPage)) }),
            ),
            Route::endpoint("send", get(|| async { RouteResponse::ssr_stream_send(()) })),
        ],
    )]);
    let app = crate::FrameworkBuilder::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    })
    .with_dynamic_routes_and_metadata(router, plans)
    .with_metadata_base(super::MetadataBase::new("https://staging.example").unwrap())
    .build();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/blog/local?q=local")
                .header("x-preview", "draft")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("<title>local draft</title>"));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/blog/send?q=local")
                .header("x-preview", "draft")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), b"render error");
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
