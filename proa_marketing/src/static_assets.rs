// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, path::PathBuf, sync::LazyLock};

use anyhow::Context;
use proa_assets::{build::OutputStore, OUTPUT_ENV};
use proa_core::{BasePath, RequestContext};
use proa_framework_axum::PublicAssets;

static ASSETS: LazyLock<anyhow::Result<PublicAssets>> = LazyLock::new(|| {
    let directory = if let Some(directory) = env::var_os(OUTPUT_ENV) {
        PathBuf::from(directory)
    } else {
        // Workspace-root invocations and plain Cargo tests can start without a
        // CLI-published catalog. Publish from the same files the app serves.
        let target = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
        let directory = target.join("marketing-public-assets");
        OutputStore::open(&directory)?
            .publish(source_directory())
            .context("publish marketing public assets")?;
        directory
    };
    PublicAssets::load(&directory, &BasePath::default())
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

pub(super) fn source_directory() -> PathBuf {
    env::var_os("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public"))
}
