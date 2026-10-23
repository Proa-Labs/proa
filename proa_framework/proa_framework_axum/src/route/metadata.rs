// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WriteBuf};
use std::sync::Arc;

use proa_framework_core::{
    css_asset_path as css_asset_path_from_manifest, render_css_link as render_css_link_with_path,
    render_metadata_head as render_resolved_metadata_head,
};
pub use proa_framework_core::{
    local_metadata, metadata_from_context, CascadeOpenGraph, CascadeValue, ComputedRouteMeta,
    DynMetadata, LocalMetadata, LocalMetadataProvider, Metadata, MetadataBase, MetadataContext,
    MetadataError, MetadataPatch, MetadataRef, MetadataResult, MetadataText, OpenGraph,
    OpenGraphImage, OpenGraphMedia, OpenGraphPatch, ResolvedMetadata, ResolvedOpenGraphImage,
    ResolvedOpenGraphMedia, TitleOnly,
};

/// Resolves and returns the current metadata from the route context.
/// Compatibility accessor for immediate providers without data loading.
/// Inside components prefer `metadata_from_context(cx)`, which also supports
/// async providers and deferred streaming without regenerating metadata.
pub fn resolved_metadata() -> Option<ResolvedMetadata> {
    use super::context::ROUTE_CONTEXT;
    ROUTE_CONTEXT
        .try_with(|ctx| {
            let ctx = ctx.borrow();
            let request = Arc::new(super::response::framework_request_context(&ctx).request);
            request
                .extensions()
                .get::<ComputedRouteMeta>()?
                .resolve_sync(Arc::clone(&request), Arc::new(proa_core::NoLoader))
                .ok()
        })
        .ok()
        .flatten()
}

/// Returns the CSS asset path from the manifest, if available.
pub fn css_asset_path() -> Option<String> {
    use super::context::ROUTE_CONTEXT;

    ROUTE_CONTEXT
        .try_with(|ctx| {
            let ctx = ctx.borrow();
            let manifest = ctx.manifest_arc();
            css_asset_path_from_manifest(manifest.as_ref()).map(str::to_owned)
        })
        .ok()
        .flatten()
}

/// Renders a CSS link tag if a CSS asset is available in the manifest.
pub fn render_css_link<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>) {
    if let Some(css_path) = css_asset_path() {
        render_css_link_with_path(cx, &css_path);
    }
}

/// Renders metadata tags into the HTML head using the current route snapshot.
pub fn render_metadata_head<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>) {
    if let Some(metadata) = cx
        .request()
        .extensions()
        .get::<Arc<ResolvedMetadata>>()
        .cloned()
    {
        render_resolved_metadata_head(cx, &metadata);
    } else if let Some(metadata) = resolved_metadata() {
        render_resolved_metadata_head(cx, &metadata);
    }
}

/// Render metadata for compiler-proved Send trees without task-local state.
pub fn render_metadata_head_send<L: DataLoader, B: WriteBuf + Send>(
    cx: &mut proa_core::SendWebContext<L, B>,
) {
    if let Some(metadata) = cx
        .request()
        .extensions()
        .get::<Arc<ResolvedMetadata>>()
        .cloned()
    {
        proa_framework_core::render_metadata_head_send(cx, &metadata);
    }
}
