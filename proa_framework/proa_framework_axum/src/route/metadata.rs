// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::ctx::Ctx;

use proa_framework_core::{
    render_metadata_head as render_resolved_metadata_head, resolved_metadata_from_computed,
};
pub use proa_framework_core::{
    CascadeOpenGraph, CascadeValue, ComputedRouteMeta, Metadata, MetadataRef, OpenGraph,
    OpenGraphImage, OpenGraphMedia, ResolvedMetadata, ResolvedOpenGraphImage,
    ResolvedOpenGraphMedia, TitleOnly,
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

/// Renders metadata tags into the HTML head using the current route snapshot.
pub fn render_metadata_head(cx: &mut Ctx) {
    if let Some(metadata) = resolved_metadata() {
        render_resolved_metadata_head(cx, &metadata);
    }
}
