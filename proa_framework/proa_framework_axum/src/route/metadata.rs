// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::ctx::Ctx;

use proa_framework_core::{
    css_asset_path as css_asset_path_from_manifest, render_css_link as render_css_link_with_path,
    render_metadata_head as render_resolved_metadata_head, resolved_metadata_from_computed,
};
pub use proa_framework_core::{
    CascadeOpenGraph, CascadeTwitter, CascadeValue, ComputedRouteMeta, Metadata, MetadataRef,
    OpenGraph, OpenGraphImage, OpenGraphMedia, ResolvedMetadata, ResolvedOpenGraphImage,
    ResolvedOpenGraphMedia, ResolvedTwitter, TitleOnly, TwitterCard,
};

/// Resolves and returns the current metadata from the route context.
/// This is an O(1) lookup into the precomputed metadata table when possible,
/// with a fallback to resolving the current metadata stack on demand.
pub fn resolved_metadata() -> Option<ResolvedMetadata> {
    use super::context::ROUTE_CONTEXT;

    ROUTE_CONTEXT
        .try_with(|ctx| {
            let ctx = ctx.borrow();
            if let Some(path) = ctx.current_path() {
                if let Some(computed) = ctx.get_computed_metadata(path) {
                    return Some(resolved_metadata_from_computed(&computed));
                }
            }
            None
        })
        .ok()
        .flatten()
        .or_else(|| {
            ROUTE_CONTEXT
                .try_with(|ctx| {
                    let mut ctx = ctx.borrow_mut();
                    ctx.resolve_metadata()
                })
                .ok()
                .flatten()
        })
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
pub fn render_css_link(cx: &mut Ctx) {
    if let Some(css_path) = css_asset_path() {
        render_css_link_with_path(cx, &css_path);
    }
}

/// Renders metadata tags into the HTML head using the current route snapshot.
pub fn render_metadata_head(cx: &mut Ctx) {
    if let Some(metadata) = resolved_metadata() {
        render_resolved_metadata_head(cx, &metadata);
    }
}
