// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::Extension,
    http::{header, Request, StatusCode},
    response::Redirect,
    routing::get,
    Router,
};
use http_body_util::BodyExt;
use proa_core::{
    BasePath, DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError,
};
use proa_framework_axum::{
    app_url, router_from_spec, CssAsset, FrameworkBuilder, IslandManifest, ManifestAssets,
    MarkdownAlternate, Route, RouteResponse,
};
use proa_macros::{html, html_sync};
use proa_stream::Suspense;
use tower::ServiceExt;

const ISLAND: &str = "base_path_counter";
const LATE: &str = "base_path_late";
const SOURCE: &str = r#"import { signal } from "/rsjs-runtime.js";
import { rsjs } from "/rsjs-runtime-query.js";
import { mount } from "./component_base_path_fragment.js";
export function hydrate() {}"#;

static COUNTER: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: ISLAND,
    struct_name: "Counter",
    js_source: SOURCE,
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
    client_module_imports: &[],
    component_fragment_imports: &["base_path_fragment"],
    uses_queries: true,
    uses_mutations: false,
};
static DEFERRED: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
    asset_id: LATE,
    struct_name: "Late",
    js_source: SOURCE,
    is_async: false,
    hydration_strategy: rsjs::IslandHydrationStrategy::Visible,
    client_module_imports: &[],
    component_fragment_imports: &[],
    uses_queries: true,
    uses_mutations: false,
};
static FRAGMENT: rsjs::ComponentFragmentManifestEntry = rsjs::ComponentFragmentManifestEntry {
    asset_id: "base_path_fragment",
    struct_name: "Fragment",
    export_name: "mount",
    js_source: SOURCE,
};
static CLASS: rsjs::ClientClassManifestEntry = rsjs::ClientClassManifestEntry {
    module_path: "base_path::ClientClass",
    export_names: &["ClientClass"],
    js_source: SOURCE,
};
static MODULE: rsjs::ClientModuleManifestEntry = rsjs::ClientModuleManifestEntry {
    module_path: "base_path::helpers",
    helpers: &[],
};
#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static COUNTER_REG: &'static rsjs::IslandManifestEntry = &COUNTER;
#[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static DEFERRED_REG: &'static rsjs::IslandManifestEntry = &DEFERRED;
#[rsjs::linkme::distributed_slice(rsjs::COMPONENT_FRAGMENT_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static FRAGMENT_REG: &'static rsjs::ComponentFragmentManifestEntry = &FRAGMENT;
#[rsjs::linkme::distributed_slice(rsjs::CLIENT_CLASS_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static CLASS_REG: &'static rsjs::ClientClassManifestEntry = &CLASS;
#[rsjs::linkme::distributed_slice(rsjs::CLIENT_MODULE_REGISTRY)]
#[linkme(crate = rsjs::linkme)]
static MODULE_REG: &'static rsjs::ClientModuleManifestEntry = &MODULE;

fn manifest() -> IslandManifest {
    IslandManifest {
        islands: HashMap::new(),
        assets: ManifestAssets {
            css: Some(CssAsset {
                path: "/static/styles.css".into(),
                size: None,
            }),
        },
        import_map: HashMap::new(),
        ssr_bundle: None,
    }
}

struct Page;
impl<L: DataLoader> WebRenderSync<L> for Page {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let docs = cx.url("/docs/start?x=1&y=2");
        let submit = cx.url("/submit");
        let image = cx.url("https://cdn.example/image.png");
        cx.rsjs_island_used(ISLAND);
        WebRenderSync::render(
            html_sync! {
                <main>
                    <a href={docs}>"Docs"</a>
                    <form action={submit}></form>
                    <img src={image} />
                </main>
            },
            cx,
        )
    }
}

fn app(prefix: &str) -> Router {
    let (router, metadata) = router_from_spec(vec![
        Route::endpoint("/docs/start", get(|| async { RouteResponse::ssr(Page) })),
        Route::endpoint(
            "/redirect",
            get(|| async { Redirect::to(&app_url("/").to_string()) }),
        ),
        Route::endpoint(
            "/css",
            get(|| async { proa_framework_axum::css_asset_path().unwrap() }),
        ),
        Route::endpoint(
            "/alternate",
            get(|| async {
                proa_framework_axum::negotiate(
                    &axum::http::HeaderMap::new(),
                    RouteResponse::ssr(Page),
                    (),
                    MarkdownAlternate::new("/index.md"),
                )
                .unwrap()
            }),
        ),
        Route::endpoint(
            "/extension",
            get(|Extension(base): Extension<BasePath>| async move { base.url("/api").to_string() }),
        ),
    ]);
    FrameworkBuilder::new(manifest())
        .with_dynamic_routes_and_metadata(router, metadata)
        .with_static_service(
            Router::new()
                .route(
                    "/styles.css",
                    get(|| async {
                        (
                            [(header::CONTENT_TYPE, "text/css")],
                            "body{background:url(/image.svg)}",
                        )
                    }),
                )
                .route(
                    "/app.js",
                    get(|| async {
                        (
                            [(header::CONTENT_TYPE, "text/javascript")],
                            "fetch('/api/items'); const label='/ordinary';",
                        )
                    }),
                ),
        )
        .with_rsjs_assets()
        .with_base_path(BasePath::new(prefix).unwrap())
        .build()
}

async fn fetch(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn nested_page_urls_redirects_and_framework_links_follow_startup_prefix() {
    for prefix in ["", "/", "/apps/demo", "/apps/demo/"] {
        let app = app(prefix);
        let prefix = prefix.trim_end_matches('/');
        let (status, _, html) = fetch(&app, "/docs/start").await;
        assert_eq!(status, StatusCode::OK);
        for url in [
            "/docs/start?x=1&amp;y=2",
            "/submit",
            "/rsjs-runtime.js",
            "/rsjs-runtime-query.js",
            "/rsjs/island/base_path_counter.js",
            "/rsjs/island/component_base_path_fragment.js",
        ] {
            assert!(
                html.contains(&format!("\"{prefix}{url}\"")),
                "missing {url} in {html}"
            );
        }
        assert!(html.contains(r#"src="https://cdn.example/image.png""#));
        let (_, headers, _) = fetch(&app, "/redirect").await;
        assert_eq!(headers[header::LOCATION], format!("{prefix}/"));
        assert_eq!(
            fetch(&app, "/css").await.2,
            format!("{prefix}/static/styles.css")
        );
        assert_eq!(fetch(&app, "/static/styles.css").await.0, StatusCode::OK);
        assert_eq!(fetch(&app, "/extension").await.2, format!("{prefix}/api"));
        let (_, headers, _) = fetch(&app, "/alternate").await;
        assert_eq!(
            headers[header::LINK],
            format!("<{prefix}/index.md>; rel=\"alternate\"; type=\"text/markdown\"")
        );
    }
}

#[tokio::test]
async fn serves_all_linked_asset_kinds_with_public_runtime_imports() {
    let mounted_app = app("/apps/demo");
    for file in [
        "base_path_counter.js".to_string(),
        "component_base_path_fragment.js".to_string(),
        rsjs::client_class_filename(CLASS.module_path),
    ] {
        let (status, headers, source) = fetch(&mounted_app, &format!("/rsjs/island/{file}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "application/javascript; charset=utf-8"
        );
        assert!(source.contains(r#"from "/apps/demo/rsjs-runtime.js""#));
        assert!(source.contains(r#"from "/apps/demo/rsjs-runtime-query.js""#));
        assert!(source.contains(r#"from "./component_base_path_fragment.js""#));
        assert!(!source.contains(r#"from "/rsjs-runtime"#));
    }
    assert_eq!(
        fetch(
            &mounted_app,
            &format!(
                "/rsjs/island/{}",
                rsjs::client_module_filename(MODULE.module_path)
            )
        )
        .await
        .0,
        StatusCode::OK
    );
    for path in ["/rsjs-runtime.js", "/rsjs-runtime-query.js"] {
        let (status, _, source) = fetch(&mounted_app, path).await;
        assert_eq!(status, StatusCode::OK);
        assert!(source.contains(r#"const PROA_BASE_PATH = "/apps/demo";"#));
    }
    assert_eq!(
        fetch(&mounted_app, "/rsjs/island/missing.js").await.0,
        StatusCode::NOT_FOUND
    );
    // Separate apps can share a process without leaking each other's prefix.
    let other = app("/preview/123");
    assert!(fetch(&other, "/rsjs/island/base_path_counter.js")
        .await
        .2
        .contains(r#"from "/preview/123/rsjs-runtime.js""#));
}

struct LatePage(Arc<tokio::sync::Notify>);
impl WebRender for LatePage {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            self.0.notified().await;
            cx.rsjs_island_used(LATE);
            let late = cx.url("/late");
            html! {
                <a href={late}>"Ready"</a>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[tokio::test]
async fn streamed_shell_arrives_before_late_island_with_prefixed_deferred_loader() {
    let notify = Arc::new(tokio::sync::Notify::new());
    let handler_notify = notify.clone();
    let router = Router::new().route(
        "/stream",
        get(move || {
            let notify = handler_notify.clone();
            async move {
                RouteResponse::ssr_stream(html! {
                    <main>
                        {Suspense::new(LatePage(notify), html_sync! {
                            <p>"Loading"</p>
                        })}
                    </main>
                })
            }
        }),
    );
    let app = FrameworkBuilder::new(manifest())
        .with_base_path(BasePath::new("/apps/demo").unwrap())
        .with_dynamic_routes(router)
        .build();
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        app.oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let mut body = response.into_body();
    let first = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let shell = std::str::from_utf8(&first).unwrap();
    assert!(!shell.contains("base_path_late.js"));
    notify.notify_one();
    let remaining = tokio::time::timeout(Duration::from_secs(2), to_bytes(body, usize::MAX))
        .await
        .unwrap()
        .unwrap();
    let remaining = std::str::from_utf8(&remaining).unwrap();
    assert!(
        remaining.contains("/apps/demo/rsjs/island/base_path_late.js"),
        "{remaining}"
    );
    assert!(remaining.contains("/apps/demo/rsjs-runtime.js"));
    assert!(!remaining.contains(r#"src="/rsjs"#));
    // The child resumes after the HTTP handler's task-local context ends.
    // Its own URL helper must retain the captured request prefix too.
    use base64::Engine as _;
    let encoded = remaining
        .split_once("data-proa-encoding=\"base64-v1\">")
        .unwrap()
        .1
        .split_once("</template>")
        .unwrap()
        .0;
    let child = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    assert!(std::str::from_utf8(&child)
        .unwrap()
        .contains(r#"href="/apps/demo/late""#));
}

#[tokio::test]
async fn custom_layer_reconfiguration_resolves_manifest_from_original_paths() {
    for prefix in ["/preview/42", ""] {
        let layer = proa_framework_axum::RouteContextLayer::new(
            Arc::new(manifest()),
            Arc::new(HashMap::new()),
        )
        .with_base_path(BasePath::new("/old").unwrap())
        .with_base_path(BasePath::new(prefix).unwrap());
        let app = Router::new()
            .route(
                "/css",
                get(|| async { proa_framework_axum::css_asset_path().unwrap() }),
            )
            .layer(layer);
        assert_eq!(
            fetch(&app, "/css").await.2,
            format!("{prefix}/static/styles.css")
        );
    }
}

#[tokio::test]
async fn public_mount_requests_reach_assets_after_proxy_prefix_stripping() {
    // Axum nesting models the proxy's removal of the external mount prefix.
    let app = Router::new().nest("/apps/demo", app("/apps/demo"));
    let (status, _, html) = fetch(&app, "/apps/demo/docs/start").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"href="/apps/demo/docs/start?x=1&amp;y=2""#));
    for path in [
        "/apps/demo/static/styles.css",
        "/apps/demo/rsjs-runtime.js",
        "/apps/demo/rsjs-runtime-query.js",
        "/apps/demo/rsjs/island/base_path_counter.js",
        "/apps/demo/rsjs/island/component_base_path_fragment.js",
    ] {
        assert_eq!(fetch(&app, path).await.0, StatusCode::OK, "{path}");
    }
    assert_eq!(
        fetch(&app, "/rsjs-runtime.js").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn builder_composes_managed_css_and_js_and_retains_root_bytes() {
    let mounted = app("/apps/demo");
    let (_, _, css) = fetch(&mounted, "/static/styles.css").await;
    assert!(css.contains("url(\"/apps/demo/image.svg\")"), "{css}");
    let (_, _, js) = fetch(&mounted, "/static/app.js").await;
    assert!(js.contains("fetch(\"/apps/demo/api/items\")"), "{js}");
    assert!(js.contains("const label='/ordinary'"));
    let (_, _, root_css) = fetch(&app(""), "/static/styles.css").await;
    assert_eq!(root_css, "body{background:url(/image.svg)}");
    let (_, _, root_js) = fetch(&app(""), "/static/app.js").await;
    assert_eq!(root_js, "fetch('/api/items'); const label='/ordinary';");
    let (_, _, other) = fetch(&app("/preview/123"), "/static/styles.css").await;
    assert!(other.contains("/preview/123/image.svg"));
    assert!(!other.contains("/apps/demo"));
}
