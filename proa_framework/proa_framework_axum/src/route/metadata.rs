// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WriteBuf};

use super::context::RouteContext;

use proa_framework_core::{
    compute_metadata_stack, css_asset_path as css_asset_path_from_manifest,
    render_css_link as render_css_link_with_path,
    render_metadata_head as render_resolved_metadata_head, resolved_metadata_from_computed,
};
pub use proa_framework_core::{
    CascadeOpenGraph, CascadeTwitter, CascadeValue, ComputedRouteMeta, Metadata, MetadataRef,
    OpenGraph, OpenGraphImage, OpenGraphMedia, RequestMetadata, ResolvedMetadata,
    ResolvedOpenGraphImage, ResolvedOpenGraphMedia, ResolvedTwitter, TitleOnly, TwitterCard,
};

/// Resolves and returns the current metadata from the route context.
/// This is an O(1) lookup into the precomputed metadata table when possible,
/// with a fallback to resolving the current metadata stack on demand.
pub fn resolved_metadata() -> Option<ResolvedMetadata> {
    use super::context::ROUTE_CONTEXT;

    ROUTE_CONTEXT
        .try_with(|ctx| route_metadata(&mut ctx.borrow_mut()))
        .ok()
        .flatten()
}

/// Resolves the metadata for the request `ctx` is handling: the precomputed
/// entry for its path, or the endpoint's provider stack for paths that are
/// not in the precomputed map, such as dynamic `:param` routes.
pub(super) fn route_metadata(ctx: &mut RouteContext) -> Option<ResolvedMetadata> {
    if let Some(path) = ctx.current_path() {
        if let Some(computed) = ctx.get_computed_metadata(path) {
            return Some(resolved_metadata_from_computed(&computed));
        }
    }
    ctx.resolve_metadata()
}

/// Like [`route_metadata`], but keeps the templates so per-request
/// [`RequestMetadata`] can still be formatted by the route's layouts.
pub(super) fn route_computed_metadata(ctx: &RouteContext) -> Option<ComputedRouteMeta> {
    if let Some(path) = ctx.current_path() {
        if let Some(computed) = ctx.get_computed_metadata(path) {
            return Some(computed);
        }
    }
    compute_metadata_stack(ctx.metadata_stack())
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

/// Renders metadata tags into the HTML head for the current request.
///
/// A `RouteResponse` resolves the route's metadata before rendering and
/// stores it in the render context, so this works in renders that run after
/// the handler returns, such as streamed documents. Outside a
/// `RouteResponse` render it falls back to the installed route context.
pub fn render_metadata_head<L: DataLoader, B: WriteBuf>(cx: &mut WebContext<L, B>) {
    if let Some(metadata) = cx.locals().get::<ResolvedMetadata>() {
        render_resolved_metadata_head(cx, &metadata);
    } else if let Some(metadata) = resolved_metadata() {
        render_resolved_metadata_head(cx, &metadata);
    }
}
