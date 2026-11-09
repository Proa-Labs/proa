// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `FrameworkBuilder` and `RouteContextLayer` start without a React island
//! manifest. The stylesheet hook and an explicit manifest compose in either
//! order, and the base path still resolves the stylesheet URL.

use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::Request, routing::get, Router};
use proa_framework_axum::{
    css_asset_path, BasePath, CssAsset, FrameworkBuilder, IslandManifest, ManifestAssets,
    RouteContextLayer,
};
use tower::ServiceExt;

const STYLESHEET: &str = "/static/app.css";

fn css_router() -> Router {
    Router::new().route(
        "/css",
        get(|| async { css_asset_path().unwrap_or_else(|| "none".to_owned()) }),
    )
}

async fn css_path(app: Router) -> String {
    let response = app
        .oneshot(Request::get("/css").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

fn manifest_with_css(path: &str) -> IslandManifest {
    IslandManifest {
        assets: ManifestAssets {
            css: Some(CssAsset {
                path: path.to_owned(),
                size: None,
            }),
        },
        ..IslandManifest::default()
    }
}

#[tokio::test]
async fn builder_starts_without_a_stylesheet() {
    let app = FrameworkBuilder::new()
        .with_dynamic_routes(css_router())
        .build();
    assert_eq!(css_path(app).await, "none");
}

#[tokio::test]
async fn builder_css_asset_survives_an_explicit_manifest_in_either_order() {
    let css_first = FrameworkBuilder::new()
        .with_css_asset(STYLESHEET)
        .with_island_manifest(IslandManifest::default())
        .with_dynamic_routes(css_router())
        .build();
    assert_eq!(css_path(css_first).await, STYLESHEET);

    let manifest_first = FrameworkBuilder::new()
        .with_island_manifest(IslandManifest::default())
        .with_css_asset(STYLESHEET)
        .with_dynamic_routes(css_router())
        .build();
    assert_eq!(css_path(manifest_first).await, STYLESHEET);
}

#[tokio::test]
async fn builder_manifest_stylesheet_wins_over_an_earlier_css_asset() {
    let app = FrameworkBuilder::new()
        .with_css_asset("/static/old.css")
        .with_island_manifest(manifest_with_css(STYLESHEET))
        .with_dynamic_routes(css_router())
        .build();
    assert_eq!(css_path(app).await, STYLESHEET);
}

#[tokio::test]
async fn builder_css_asset_follows_the_base_path() {
    let app = FrameworkBuilder::new()
        .with_css_asset(STYLESHEET)
        .with_base_path(BasePath::new("/apps/demo").unwrap())
        .with_dynamic_routes(css_router())
        .build();
    assert_eq!(css_path(app).await, "/apps/demo/static/app.css");
}

#[tokio::test]
async fn layer_css_asset_and_manifest_compose_in_either_order() {
    let metadata = || Arc::new(HashMap::new());

    let css_first = RouteContextLayer::new(metadata())
        .with_css_asset(STYLESHEET)
        .with_island_manifest(IslandManifest::default())
        .with_base_path(BasePath::new("/mount").unwrap());
    assert_eq!(
        css_path(css_router().layer(css_first)).await,
        "/mount/static/app.css"
    );

    let base_path_first = RouteContextLayer::new(metadata())
        .with_base_path(BasePath::new("/mount").unwrap())
        .with_island_manifest(IslandManifest::default())
        .with_css_asset(STYLESHEET);
    assert_eq!(
        css_path(css_router().layer(base_path_first)).await,
        "/mount/static/app.css"
    );

    let bare = RouteContextLayer::new(metadata());
    assert_eq!(css_path(css_router().layer(bare)).await, "none");
}
