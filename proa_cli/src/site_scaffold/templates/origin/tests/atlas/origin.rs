// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use __CRATE_IDENT__ as atlas;

#[tokio::test]
async fn mounted_documents_and_assets_keep_their_public_prefix() {
    for mount in ["/apps/demo", "/user/a%40b/proxy/3000"] {
        let base_path = proa_core::BasePath::new(mount).unwrap();
        let html = atlas::render_atlas_page_with_base_path(
            None,
            std::sync::Arc::new(atlas::loader::FixtureLoader::github()),
            base_path.clone(),
        )
        .await;
        for path in [
            "/static/favicon.svg",
            "/static/styles.css",
            "/static/proa/proa-ui/proa-free.css",
            "/static/proa/proa-ui/proa-free.js",
            "/static/origin.js",
            "/static/proa-logo.svg",
            "/site/github.com.md",
        ] {
            assert!(
                html.contains(&format!("\"{mount}{path}")),
                "missing {mount}{path}"
            );
        }
        assert!(html.contains(&format!("action=\"{mount}/\"")));
        assert!(html.contains(&format!("href=\"{mount}/?q=")));
        assert!(html.contains(&format!("src:url({mount}/fonts/")));
        assert!(html.contains(&format!("{mount}/?q=github.com")));
        for root in [
            "href=\"/static/",
            "src=\"/static/",
            "href=\"/fonts/",
            "src:url(/fonts/",
            "href=\"/site/",
        ] {
            assert!(!html.contains(root), "unprefixed URL: {root}");
        }

        let app = atlas::app_with_base_path(base_path.clone());
        let font = html
            .split(&format!("href=\"{mount}/fonts/"))
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let font = format!("/fonts/{font}");
        for path in [
            "/static/favicon.svg",
            "/static/styles.css",
            "/static/origin.js",
            "/rsjs-runtime.js",
            "/rsjs-runtime-query.js",
            font.as_str(),
        ] {
            let public = base_path.url(path).to_string();
            let forwarded = public.strip_prefix(mount).unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(forwarded)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{public}");
        }
        let island = rsjs::manifest().islands.first().unwrap().asset_id;
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/rsjs/island/{island}.js"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let source = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(source.contains(&format!("{mount}/rsjs-runtime.js")));
    }
}

#[tokio::test]
async fn health_endpoint_is_local_and_machine_readable() {
    let response = __CRATE_IDENT__::app()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"status":"ok"}"#);
}

#[tokio::test]
async fn site_markdown_route_serves_the_dossier() {
    let response = __CRATE_IDENT__::app()
        .oneshot(
            Request::builder()
                .uri("/site/cloudflare.com.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/markdown"));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body.contains("`cloudflare.com` — network dossier"),
        "unexpected body head: {:.300}",
        body
    );
    assert!(body.contains("anything unusual"));
    assert!(body.contains("lookup_site(hostname)"));
    assert!(body.contains("Accept: text/markdown"));
}

#[tokio::test]
async fn bundled_browser_runtime_drives_the_feeds_and_lab() {
    let response = __CRATE_IDENT__::app()
        .oneshot(
            Request::builder()
                .uri("/static/origin.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("origin:flood"));
    assert!(body.contains("data-flood-table"));
    assert!(body.contains("origin:filter"));
}
