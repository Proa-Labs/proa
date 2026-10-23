// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WriteBuf};
use proa_framework_core::{
    css_asset_path as css_asset_path_from_manifest, render_css_link as render_css_link_with_path,
};
pub use proa_framework_core::{
    render_metadata_head, render_metadata_head_send, Metadata, OpenGraph, OpenGraphImage,
    RouteSetupPlan,
};
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
