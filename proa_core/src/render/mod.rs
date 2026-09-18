// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Multi-target rendering infrastructure for Proa
//!
//! This module provides the foundation for rendering components to multiple targets:
//! - **Web**: HTML for browsers (see [`crate::web`])
//! - **Email**: HTML with inline styles and table layouts (see [`crate::email`])
//! - **PDF**: Planned for future implementation
//!
//! ## Architecture
//!
//! Each target has its own directory at the crate root:
//! - Context type (e.g., `WebContext`, `EmailContext`)
//! - Target-specific rendering utilities
//! - Trait implementations for components
//!
//! Common code shared between targets (especially web/email) lives in [`crate::shared`].

pub mod containers;
pub mod dyn_web_render;
pub mod email_render;
pub mod error_boundary;
pub mod load_error;
pub mod load_key;
pub mod load_request;
pub mod load_value;
pub mod loader;
pub mod md_render;
pub mod mode;
pub mod outcome;
pub mod priority;
pub mod request_location;
pub mod route_params;
pub mod send_web_render;
pub mod sink;
pub mod standard_loader;
pub mod stream;
pub mod web_render;

// Re-export primary types for convenience
pub use crate::email::EmailContext;
pub use crate::web::{
    CacheDependency, CacheTrace, HeadContext, HeadEntry, Locals, RenderArtifacts, RenderTelemetry,
    RequestContext, ResponseContext, RsjsArtifactBranch, RsjsArtifactCheckpoint,
    RsjsLinkedProviderOccurrenceKey, RsjsLinkedProviderTagGuard, RsjsQuerySeedArtifact,
    RsjsRenderArtifacts, RsjsSharedStateScopeGuard, RsjsSharedStateSeedArtifact, SendWebContext,
    SendWebContextParts, TelemetryEvent, WebContext, WebContextParts,
};
pub use containers::TwoSync;
pub use dyn_web_render::{
    boxed_render, boxed_sync_render, drive_dyn_render, BoxedWebRender, BoxedWebRenderSync,
    DynRenderOutcome, DynWebRender, DynWebRenderSync,
};
pub use email_render::EmailRender;
pub use error_boundary::{BoundaryErrorFallback, BoundaryErrorFallbackHandle, ErrorBoundary};
pub use load_error::LoadError;
pub use load_key::{LoadKey, LoadKeyKind};
pub use load_request::{CacheHint, CacheMode, LoadHints, LoadRequest};
pub use load_value::LoadValue;
pub use loader::{DataLoader, DynDataLoader, DynLoaderHandle, LoadFuture, NoLoader};
pub use md_render::{
    AwaitMdComponent, MdRender, MdRenderAsyncExt, MdRenderSync, MdRenderSyncExt, MdSyncToAsync,
};
pub use mode::RenderMode;
pub use outcome::RenderOutcome;
pub use priority::Priority;
pub use request_location::RequestLocation;
pub use route_params::{RouteParam, RouteParams};
pub use send_web_render::{
    drive_dyn_send_render, render_in_send_context, render_send_child, AwaitSendComponent,
    DynSendRenderOutcome, DynWebRenderSend, SendSyncToAsync, StatelessSyncToSend, WebRenderSend,
    WebRenderSendSync,
};
#[allow(deprecated)]
pub use sink::{
    InOrderPagePool, InOrderSinkWriter, MemorySink, PagePool, SinkCheckpoint, SinkPage, SinkWriter,
    IN_ORDER_SINK_PAGE_SIZE,
};
pub use standard_loader::StandardDataLoader;
pub use stream::{
    PendingBoundary, RenderedBoundary, SendPendingBoundary, SendPendingBoundaryFuture,
    SendRenderedBoundary, SendStreamBoundaryRecorder, SendStreamChunkFlusher,
    StreamBoundaryRecorder, StreamChunkFlusher, StreamChunkView,
};
pub use web_render::{AwaitComponent, SyncToAsync, WebRender, WebRenderSync};
