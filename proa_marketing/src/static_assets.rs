// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, path::PathBuf, sync::LazyLock};

use anyhow::Context;
use axum::{
    extract::Path,
    http::{header::CACHE_CONTROL, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use proa_assets::build::OutputStore;
use proa_core::{BasePath, RequestContext};
use proa_framework_axum::PublicAssets;

static ASSETS: LazyLock<anyhow::Result<PublicAssets>> = LazyLock::new(|| {
    let base_path = BasePath::default();
    // The CLI names the catalog it published, and a release build carries one
    // beside the executable. The site holds one fixed generation either way.
    if let Some(assets) =
        PublicAssets::from_env(&base_path).context("load marketing public assets")?
    {
        return Ok(assets.current());
    }
    // Workspace-root invocations and plain Cargo tests can start without a
    // CLI-published catalog. Publish from the same files the app serves.
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    let directory = target.join("marketing-public-assets");
    OutputStore::open(&directory)?
        .publish(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public"))
        .context("publish marketing public assets")?;
    PublicAssets::load(&directory, &base_path)
        .with_context(|| format!("load marketing public assets from {}", directory.display()))
});

pub(super) fn load() -> anyhow::Result<&'static PublicAssets> {
    ASSETS
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error:#}"))
}

pub(super) fn get() -> &'static PublicAssets {
    load().expect("the marketing public asset catalog must be available")
}

pub(super) fn request() -> RequestContext {
    let mut request = RequestContext::new();
    request.set_public_asset_urls(Some(get().render_urls()));
    request
}

pub(super) async fn legacy_url(Path(path): Path<String>) -> Response {
    let Some(url) = get().url(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ([(CACHE_CONTROL, "no-cache")], Redirect::temporary(url)).into_response()
}
