// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Web (HTML) rendering target
//!
//! This module provides functionality for rendering components to HTML for web browsers.
//! It includes the rendering context and target-specific utilities.

pub mod base_path;
pub mod base_path_html;
pub mod context;
pub mod csp;
pub mod send_context;

pub use context::{
    CacheDependency, CacheTrace, HeadContext, HeadEntry, Locals, RenderArtifacts, RenderTelemetry,
    RequestContext, ResponseContext, RsjsArtifactBranch, RsjsArtifactCheckpoint,
    RsjsLinkedProviderOccurrenceKey, RsjsLinkedProviderTagGuard, RsjsQuerySeedArtifact,
    RsjsRenderArtifacts, RsjsSharedStateScopeGuard, RsjsSharedStateSeedArtifact,
    RsjsSharedStateSeedCheckpoint, StandardWebContext, TelemetryEvent, WebContext, WebContextParts,
    STANDARD_PAGE_SIZE,
};
pub use csp::{csp_nonce_attr, CspNonce, CspNonceAttr, InvalidCspNonce};
pub use send_context::{SendWebContext, SendWebContextParts};

pub use base_path::{AppUrl, BasePath, InvalidBasePath};
pub use base_path_html::BasePathHtml;

mod metadata;
pub use metadata::{DocumentMetadata, Metadata, OpenGraph, OpenGraphImage};
