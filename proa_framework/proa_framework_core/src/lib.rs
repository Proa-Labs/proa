// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Framework-agnostic contracts shared by Proa server adapters.
//!
//! # Features
//!
//! - `rt-tokio` (default): forwarded to `proa_stream` for eager parallel
//!   preloads. Disable it for runtime-agnostic builds; this crate itself
//!   never references a runtime.

#![forbid(unsafe_code)]

pub mod bootstrap;
pub mod discovery;
pub mod manifest;
pub mod markdown;
pub mod metadata;
pub mod response;
pub mod route_snapshot;
pub mod route_spec;

pub use bootstrap::render_bootstrap_for_islands;
pub use discovery::{
    absolute_http_url, ChangeFreq, DiscoveryError, RobotsGroup, RobotsRule, RobotsTxt, Sitemap,
    SitemapFile, SitemapIndex, SitemapIndexEntry, SitemapPriority, SitemapUrl, MAX_SITEMAP_BYTES,
    MAX_SITEMAP_ENTRIES,
};
pub use manifest::{
    CssAsset, HydrationStrategyConfig, IslandEntry, IslandManifest, ManifestAssets, ProviderSpec,
};
pub use markdown::{render_sync_markdown_response, MarkdownResponse};
pub use metadata::{
    css_asset_path, render_css_link, render_metadata_head, resolve_metadata_stack,
    resolved_metadata_from_computed, CascadeOpenGraph, CascadeValue, ComputedOpenGraph,
    ComputedRouteMeta, Metadata, MetadataRef, OpenGraph, OpenGraphImage, OpenGraphMedia,
    ResolvedMetadata, ResolvedOpenGraph, ResolvedOpenGraphImage, ResolvedOpenGraphMedia, TitleOnly,
};
pub use response::{
    add_default_security_headers, build_dyn_in_order_loader_handle,
    build_dyn_request_loader_handle, finalize_route_response, finalize_route_response_stream,
    finalize_route_response_sync, render_async_html_response, render_send_html_response,
    render_send_stream_html_response, render_stream_html_response, render_sync_html_response,
    FrameworkBody, FrameworkError, FrameworkLocalStream, FrameworkLocalStreamFactory,
    FrameworkRequestContext, FrameworkResponse, FrameworkSendStream, FrameworkSendStreamFactory,
    FrameworkStreamFactory, InOrderStreamDriver, OutOfOrderStreamDriver, RouteFinalizeMode,
    RouteResponse, SendInOrderStreamDriver, SendOutOfOrderStreamDriver,
};

// Back-compat re-exports for the dyn-erasure render adapter, which now
// lives in `proa_core` alongside the static-dispatch `WebRender` trait.
// Existing call sites that import from `proa_framework_core` keep working.
pub use proa_core::{
    drive_dyn_render, drive_dyn_send_render, DynRenderOutcome, DynSendRenderOutcome, DynWebRender,
    DynWebRenderSend,
};
pub use route_snapshot::{extract_route_params, FrameworkRouteSnapshot};
pub use route_spec::{
    compute_metadata_map, resolve_route_path, resolve_route_specs, ResolvedRouteSpec, RouteMode,
    RouteSpec, StaticPathsFn,
};
