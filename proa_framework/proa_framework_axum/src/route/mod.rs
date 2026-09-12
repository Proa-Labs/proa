// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Routing module for the Proa framework
//!
//! This module provides declarative routing with support for:
//! - Nested metadata layouts
//! - Metadata composition (title, description, Open Graph)
//! - Server-side rendering (SSR) with islands architecture
//! - Buffered and streaming async rendering

mod context;
mod layout;
mod local_executor;
mod metadata;
mod page_cache;
mod representation;
mod response;
mod spec;

#[cfg(test)]
mod tests;

// Public exports
pub use context::{RouteContextLayer, RouteContextService};
pub use layout::{Layout, LayoutRef};
pub use local_executor::LocalStreamExecutorConfig;
pub use metadata::{
    css_asset_path, render_css_link, render_metadata_head, resolved_metadata, CascadeOpenGraph,
    CascadeValue, ComputedRouteMeta, Metadata, MetadataRef, OpenGraph, OpenGraphImage,
    OpenGraphMedia, ResolvedMetadata, ResolvedOpenGraphImage, ResolvedOpenGraphMedia, TitleOnly,
};
pub use page_cache::{PageCacheLayer, PageCacheService};
pub use proa_framework_core::{ComputedOpenGraph, ResolvedOpenGraph};
pub use representation::{explicit_markdown, negotiate, MarkdownAlternate, RepresentationResponse};
pub use response::{
    drive_dyn_render, drive_dyn_send_render, DynRenderOutcome, DynSendRenderOutcome, DynWebRender,
    DynWebRenderSend, RouteResponse,
};
pub use spec::{router_from_spec, Route, RouteMode, RouteSpec, StaticPathsFn};

mod cdn_cache;
pub use cdn_cache::PageCache;
