// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Standalone HTML document rendering without an HTTP adapter.

use bytes::Bytes;
use proa_core::{HeadConfig, WebRenderSync};

use crate::{
    finalize_route_response_sync, FrameworkBody, FrameworkError, FrameworkRequestContext,
    IslandManifest, RouteFinalizeMode, RouteResponse,
};

/// Render a synchronous managed-head document to UTF-8 HTML bytes.
///
/// Use this for static HTML exports without constructing an HTTP response,
/// request context, or empty island manifest. The normal framework finalizer
/// runs layout/Page head hooks, checks the single Page and head outlet, and
/// appends any generated RSJS bootstrap. The returned bytes can be passed
/// directly to [`std::fs::write`]; this function performs no file or network I/O.
///
/// The document must contain one `ComponentRole::PAGE` and one `cx.head()`
/// outlet. Configure inherited metadata and trusted URL policies with `head`.
/// External scripts and generated RSJS assets are still references, not
/// embedded files. Documents requiring an external React island manifest or
/// request-specific configuration should use [`RouteResponse`] instead.
pub fn render_sync_html_document(
    component: impl WebRenderSync + Send + 'static,
    head: HeadConfig,
) -> Result<Bytes, FrameworkError> {
    let manifest = IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    };
    let response = finalize_route_response_sync(
        RouteResponse::ssr(component).with_head(head),
        &manifest,
        &FrameworkRequestContext::default(),
        RouteFinalizeMode::Document,
    )?;
    match response.body {
        FrameworkBody::Full(bytes) => Ok(bytes),
        FrameworkBody::BufferedPages(buffer) => Ok(Bytes::from(buffer.into_vec())),
        FrameworkBody::Managed(_) | FrameworkBody::Stream(_) => Err(FrameworkError::Invariant(
            "synchronous HTML document produced a streaming body",
        )),
    }
}
