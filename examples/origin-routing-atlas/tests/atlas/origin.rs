// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_is_local_and_machine_readable() {
    let response = proa_origin_routing_atlas::app()
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
    let response = proa_origin_routing_atlas::app()
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
    let response = proa_origin_routing_atlas::app()
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
