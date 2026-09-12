// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::{
    shared::write_buf::{ChainedBumpWriter, WriteBuf, WriteError},
    Priority, RenderArtifacts,
};

pub type PendingBoundaryFuture =
    Pin<Box<dyn Future<Output = Result<RenderedBoundary, WriteError>> + 'static>>;

/// Deferred boundary future whose complete state is safe to move between
/// executor threads between polls.
pub type SendPendingBoundaryFuture =
    Pin<Box<dyn Future<Output = Result<SendRenderedBoundary, WriteError>> + Send + 'static>>;

/// Successfully rendered output for one out-of-order boundary.
///
/// The HTML and response artifacts that belong to it travel together so a
/// stream driver can publish exact React/rsjs deltas only after the boundary
/// commits. Failed or cancelled futures never produce this value, which keeps
/// speculative registrations off the wire.
pub struct RenderedBoundary<B = crate::RenderBuffer, N = PendingBoundary> {
    pub html: B,
    pub artifacts: RenderArtifacts,
    /// Boundaries discovered while rendering this detached branch.
    ///
    /// They remain speculative until their parent is published. A stream
    /// driver admits them only after emitting this boundary, which prevents a
    /// nested update from arriving before its placeholder exists in the DOM.
    pub nested: Vec<N>,
}

/// Send-capable counterpart carried by [`SendPendingBoundaryFuture`].
pub type SendRenderedBoundary<B = crate::RenderBuffer> = RenderedBoundary<B, SendPendingBoundary>;

impl<B, N> RenderedBoundary<B, N> {
    pub fn new(html: B, artifacts: RenderArtifacts) -> Self {
        Self {
            html,
            artifacts,
            nested: Vec::new(),
        }
    }

    pub fn with_nested(html: B, artifacts: RenderArtifacts, nested: Vec<N>) -> Self {
        Self {
            html,
            artifacts,
            nested,
        }
    }
}

impl RenderedBoundary {
    pub fn html(html: Vec<u8>) -> Self {
        let mut buffer = crate::RenderBuffer::new();
        crate::WriteBuf::extend(&mut buffer, &html)
            .expect("RenderBuffer accepts an unbounded boundary payload");
        Self::new(buffer, RenderArtifacts::default())
    }
}

impl From<Vec<u8>> for RenderedBoundary {
    fn from(html: Vec<u8>) -> Self {
        Self::html(html)
    }
}

/// Deferred boundary scheduled during streaming shell render.
pub struct PendingBoundary {
    pub id: u64,
    pub future: PendingBoundaryFuture,
    /// Scheduling priority used by out-of-order stream drivers when more
    /// than one boundary is ready in the same poll turn.
    pub priority: Priority,
    /// Maximum time an out-of-order driver should keep polling this boundary.
    ///
    /// The duration starts when the driver admits the boundary after the
    /// shell render. Drivers without a clock can inspect this metadata and
    /// apply their own deadline policy.
    pub timeout: Option<Duration>,
}

/// Send-capable counterpart to [`PendingBoundary`].
///
/// The separate type is the compiler proof carried by the direct streaming
/// path. A local boundary cannot be relabeled or inserted here at runtime.
pub struct SendPendingBoundary {
    pub id: u64,
    pub future: SendPendingBoundaryFuture,
    pub priority: Priority,
    pub timeout: Option<Duration>,
}

impl SendPendingBoundary {
    pub fn new(id: u64, future: SendPendingBoundaryFuture) -> Self {
        Self {
            id,
            future,
            priority: Priority::Critical,
            timeout: None,
        }
    }

    pub fn with_options(
        id: u64,
        future: SendPendingBoundaryFuture,
        priority: Priority,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            id,
            future,
            priority,
            timeout,
        }
    }
}

impl PendingBoundary {
    /// Construct a boundary with the historical defaults: critical priority
    /// and no timeout.
    pub fn new(id: u64, future: PendingBoundaryFuture) -> Self {
        Self {
            id,
            future,
            priority: Priority::Critical,
            timeout: None,
        }
    }

    /// Construct a boundary with explicit out-of-order scheduling options.
    pub fn with_options(
        id: u64,
        future: PendingBoundaryFuture,
        priority: Priority,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            id,
            future,
            priority,
            timeout,
        }
    }
}

/// Request-scoped recorder for out-of-order streaming boundaries.
///
/// This lives in `proa_core` so `WebContext` and components like `Suspense`
/// can schedule deferred work without depending on `proa_stream`.
pub trait StreamBoundaryRecorder {
    fn next_boundary_id(&self) -> u64;
    fn push_boundary(&self, boundary: PendingBoundary);
}

/// Request-scoped recorder for boundaries in a compiler-proved `Send` graph.
pub trait SendStreamBoundaryRecorder: Send + Sync {
    fn next_boundary_id(&self) -> u64;
    fn push_boundary(&self, boundary: SendPendingBoundary);
}

/// Borrowed view over a flushed streaming chunk.
///
/// This is intentionally transport-facing: implementations expose ordered byte
/// slices without forcing the caller to materialize a contiguous `Vec<u8>`.
/// Sinks that need owned bytes can flatten through the default
/// [`StreamChunkFlusher::flush_chunk_view`] implementation.
pub trait StreamChunkView {
    /// Total bytes across all slices in the view.
    fn total_len(&self) -> usize;

    /// Visit each slice in order.
    fn for_each_slice(
        &self,
        f: &mut dyn FnMut(&[u8]) -> Result<(), WriteError>,
    ) -> Result<(), WriteError>;
}

impl<const N: usize> StreamChunkView for ChainedBumpWriter<N> {
    #[inline]
    fn total_len(&self) -> usize {
        self.len()
    }

    fn for_each_slice(
        &self,
        f: &mut dyn FnMut(&[u8]) -> Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        for slice in self.slices() {
            if !slice.is_empty() {
                f(slice)?;
            }
        }
        Ok(())
    }
}

/// Request-scoped sink for in-order streamed HTML chunks.
///
/// This lets components like `Suspense` flush the bytes currently buffered in
/// `WebContext<::proa_core::NoLoader, Vec<u8>>` without depending on the framework's response body
/// implementation details.
pub trait StreamChunkFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError>;

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut out = Vec::with_capacity(chunk.total_len());
        chunk.for_each_slice(&mut |slice| {
            out.extend_from_slice(slice);
            Ok(())
        })?;
        self.flush_chunk(out)
    }
}

/// Send-safe chunk sink used by in-order direct streams.
pub trait SendStreamChunkFlusher: Send + Sync {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError>;

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut out = Vec::with_capacity(chunk.total_len());
        chunk.for_each_slice(&mut |slice| {
            out.extend_from_slice(slice);
            Ok(())
        })?;
        self.flush_chunk(out)
    }
}
