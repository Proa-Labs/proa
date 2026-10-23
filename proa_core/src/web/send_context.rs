// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Send-safe HTML rendering context.
//!
//! `WebContext` deliberately supports request-local `Rc` state and therefore
//! remains the compatibility surface for local futures. This companion keeps
//! ordinary request state and mutable branch deltas owned, shares only
//! immutable request data plus the response-local id allocator, and uses
//! `Send + Sync` extension points for streaming. A future borrowing this
//! context can therefore carry a real compiler-checked `Send` proof.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use serde::de::DeserializeOwned;

use super::{
    CacheTrace, CspNonce, HeadContext, Locals, RenderArtifacts, RenderTelemetry, RequestContext,
    ResponseContext, RsjsRenderArtifacts, WebContext,
};
use crate::render::{
    DataLoader, LoadError, LoadHints, LoadKey, LoadRequest, NoLoader, RenderMode, RequestLocation,
    RouteParams, SendStreamBoundaryRecorder, SendStreamChunkFlusher,
};
use crate::shared::escape::EscapeDialect;
use crate::shared::mode::ContentMode;
use crate::{RenderBuffer, WriteBuf, WriteError};

/// Owned request/branch state used to fork a detached send-capable boundary.
///
/// Ordinary accumulators are cloned into the branch. Browser artifacts start
/// empty and travel back with the completed boundary, while the immutable
/// request and monotonic React id allocator retain response identity through
/// cheap `Arc` clones.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct SendWebContextParts {
    metadata_denied: bool,
    request: Arc<RequestContext>,
    response: ResponseContext,
    head: HeadContext,
    cache_trace: CacheTrace,
    telemetry: RenderTelemetry,
    locals: Locals,
    csp_nonce: Option<CspNonce>,
    artifacts: RenderArtifacts,
    next_react_island_id: Arc<AtomicU64>,
}

impl Default for SendWebContextParts {
    fn default() -> Self {
        Self {
            metadata_denied: false,
            request: Arc::new(RequestContext::new()),
            response: ResponseContext::new(),
            head: HeadContext::new(),
            cache_trace: CacheTrace::new(),
            telemetry: RenderTelemetry::new(),
            locals: Locals::new(),
            csp_nonce: None,
            artifacts: RenderArtifacts::default(),
            next_react_island_id: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl SendWebContextParts {
    fn fork_for_detached(&self) -> Self {
        Self {
            metadata_denied: true,
            request: Arc::clone(&self.request),
            response: self.response.clone(),
            head: self.head.clone(),
            cache_trace: self.cache_trace.clone(),
            telemetry: self.telemetry.clone(),
            locals: self.locals.clone(),
            csp_nonce: self.csp_nonce.clone(),
            artifacts: RenderArtifacts::default(),
            next_react_island_id: Arc::clone(&self.next_react_island_id),
        }
    }
}

/// HTML render context for futures that may migrate between executor threads.
///
/// Unlike [`WebContext`], this type contains no `Rc`, `RefCell`, or local dyn
/// callbacks. Detached boundary mutation is branch-owned and returned as a
/// [`RenderArtifacts`] value at commit time.
pub struct SendWebContext<L: DataLoader = NoLoader, B: WriteBuf + Send = RenderBuffer> {
    pub out: B,
    /// Managed metadata for this document.
    pub metadata: super::DocumentMetadata,
    pub(super) metadata_state: super::metadata::MetadataState,
    mode: ContentMode,
    escape_dialect: EscapeDialect,
    render_mode: RenderMode,
    parts: SendWebContextParts,
    loader: Arc<L>,
    stream_recorder: Option<Arc<dyn SendStreamBoundaryRecorder>>,
    stream_chunk_flusher: Option<Arc<dyn SendStreamChunkFlusher>>,
}

impl SendWebContext<NoLoader, RenderBuffer> {
    pub fn new() -> Self {
        Self::with_buffer(RenderBuffer::new())
    }
}

impl Default for SendWebContext<NoLoader, RenderBuffer> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: WriteBuf + Send> SendWebContext<NoLoader, B> {
    pub fn with_buffer(out: B) -> Self {
        Self::with_loader_buffer(out, Arc::new(NoLoader))
    }
}

impl<L: DataLoader, B: WriteBuf + Send> SendWebContext<L, B> {
    /// Declare metadata before emitting the managed head.
    #[track_caller]
    pub fn metadata(&mut self, value: super::Metadata) -> Result<(), super::MetadataError> {
        self.metadata_state.declare(value)
    }
    /// Rejected metadata operations in this render context.
    pub fn metadata_rejections(&self) -> usize {
        self.metadata_state.rejections()
    }
    /// Whether the managed head was already emitted.
    pub fn metadata_is_closed(&self) -> bool {
        self.metadata_state.is_closed()
    }
    /// The final document title, primarily for the existing navigation adapter.
    pub fn metadata_title(&self) -> Option<String> {
        self.metadata_state.resolved_title()
    }
    #[doc(hidden)]
    pub fn __next_metadata_route_scope(&mut self) -> Result<(), super::MetadataError> {
        self.metadata_state.next_route_scope()
    }
    #[doc(hidden)]
    pub fn __metadata_title_only(&mut self) -> Result<(), crate::WriteError> {
        self.metadata_state.enable_title_only().map_err(Into::into)
    }

    #[doc(hidden)]
    pub async fn __without_document_metadata_async<R>(
        &mut self,
        render: impl AsyncFnOnce(&mut Self) -> R,
    ) -> R {
        super::metadata::without_metadata_async(self, render).await
    }

    pub fn with_loader_buffer(out: B, loader: Arc<L>) -> Self {
        Self {
            metadata: super::DocumentMetadata,
            metadata_state: super::metadata::MetadataState::default(),
            out,
            mode: ContentMode::Html,
            escape_dialect: EscapeDialect::Html,
            render_mode: RenderMode::Static,
            parts: SendWebContextParts::default(),
            loader,
            stream_recorder: None,
            stream_chunk_flusher: None,
        }
    }

    #[doc(hidden)]
    pub fn with_loader_buffer_and_context_parts(
        out: B,
        loader: Arc<L>,
        parts: SendWebContextParts,
    ) -> Self {
        Self {
            metadata: super::DocumentMetadata,
            metadata_state: if parts.metadata_denied {
                super::metadata::MetadataState::denied()
            } else {
                super::metadata::MetadataState::default()
            },
            out,
            mode: ContentMode::Html,
            escape_dialect: EscapeDialect::Html,
            render_mode: RenderMode::Static,
            parts,
            loader,
            stream_recorder: None,
            stream_chunk_flusher: None,
        }
    }

    pub fn output(&self) -> &B {
        &self.out
    }

    pub fn output_mut(&mut self) -> &mut B {
        &mut self.out
    }

    pub fn into_output(self) -> B {
        self.out
    }

    /// Consume a detached context without cloning its committed artifacts.
    pub fn into_output_and_render_artifacts(self) -> (B, RenderArtifacts) {
        (self.out, self.parts.artifacts)
    }

    /// Consume a root context and return the state needed by response
    /// finalization without snapshot clones.
    pub fn into_output_render_artifacts_and_response(
        self,
    ) -> (B, RenderArtifacts, ResponseContext) {
        (self.out, self.parts.artifacts, self.parts.response)
    }

    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, render_mode: RenderMode) {
        self.render_mode = render_mode;
    }

    pub fn with_render_mode(mut self, render_mode: RenderMode) -> Self {
        self.set_render_mode(render_mode);
        self
    }

    pub fn loader(&self) -> &Arc<L> {
        &self.loader
    }

    pub fn request(&self) -> &RequestContext {
        &self.parts.request
    }

    pub fn request_mut(&mut self) -> &mut RequestContext {
        Arc::make_mut(&mut self.parts.request)
    }

    pub fn set_request(&mut self, request: RequestContext) {
        self.parts.request = Arc::new(request);
    }

    pub fn response(&self) -> &ResponseContext {
        &self.parts.response
    }

    pub fn response_mut(&mut self) -> &mut ResponseContext {
        &mut self.parts.response
    }

    pub fn head(&self) -> &HeadContext {
        &self.parts.head
    }

    pub fn head_mut(&mut self) -> &mut HeadContext {
        &mut self.parts.head
    }

    pub fn cache_trace(&self) -> &CacheTrace {
        &self.parts.cache_trace
    }

    pub fn cache_trace_mut(&mut self) -> &mut CacheTrace {
        &mut self.parts.cache_trace
    }

    pub fn telemetry(&self) -> &RenderTelemetry {
        &self.parts.telemetry
    }

    pub fn telemetry_mut(&mut self) -> &mut RenderTelemetry {
        &mut self.parts.telemetry
    }

    pub fn locals(&self) -> &Locals {
        &self.parts.locals
    }

    pub fn locals_mut(&mut self) -> &mut Locals {
        &mut self.parts.locals
    }

    pub fn csp_nonce(&self) -> Option<&CspNonce> {
        self.parts.csp_nonce.as_ref()
    }

    pub fn csp_nonce_cloned(&self) -> Option<CspNonce> {
        self.csp_nonce().cloned()
    }

    pub fn set_csp_nonce(&mut self, nonce: CspNonce) {
        self.parts.csp_nonce = Some(nonce);
    }

    pub fn request_location(&self) -> Option<&RequestLocation> {
        self.request().location()
    }

    pub fn set_request_location(&mut self, location: RequestLocation) {
        self.request_mut().set_location(location);
    }

    pub fn route_params(&self) -> Option<&RouteParams> {
        self.request().route_params()
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.request().param(key)
    }

    pub fn set_route_params(&mut self, route_params: RouteParams) {
        self.request_mut().set_route_params(route_params);
    }

    #[doc(hidden)]
    pub fn context_parts(&self) -> SendWebContextParts {
        self.parts.fork_for_detached()
    }

    pub fn stream_recorder(&self) -> Option<&Arc<dyn SendStreamBoundaryRecorder>> {
        self.stream_recorder.as_ref()
    }

    pub fn set_stream_recorder(&mut self, recorder: Arc<dyn SendStreamBoundaryRecorder>) {
        self.stream_recorder = Some(recorder);
    }

    pub fn stream_chunk_flusher(&self) -> Option<&Arc<dyn SendStreamChunkFlusher>> {
        self.stream_chunk_flusher.as_ref()
    }

    pub fn set_stream_chunk_flusher(&mut self, flusher: Arc<dyn SendStreamChunkFlusher>) {
        self.stream_chunk_flusher = Some(flusher);
    }

    pub fn rsjs_island_used(&mut self, asset_id: &'static str) {
        self.parts.artifacts.rsjs.record_island(asset_id);
    }

    #[doc(hidden)]
    pub fn rsjs_query_seed_json(&mut self, key_json: &str, state_json: &str) {
        self.parts
            .artifacts
            .rsjs
            .record_query_seed_json(key_json, state_json);
    }

    pub fn react_island_used(&mut self, name: &str) {
        self.parts.artifacts.record_react_island(name);
    }

    pub fn next_react_island_id(&self) -> u64 {
        self.parts
            .next_react_island_id
            .fetch_add(1, Ordering::Relaxed)
    }

    pub fn rsjs_artifacts_snapshot(&self) -> RsjsRenderArtifacts {
        self.parts.artifacts.rsjs.clone()
    }

    pub fn render_artifacts_snapshot(&self) -> RenderArtifacts {
        self.parts.artifacts.clone()
    }

    #[doc(hidden)]
    pub fn replace_render_artifacts(&mut self, artifacts: RenderArtifacts) {
        self.parts.artifacts = artifacts;
    }

    #[doc(hidden)]
    pub fn merge_render_artifacts(&mut self, artifacts: RenderArtifacts) {
        self.parts.artifacts.merge_committed(artifacts);
    }

    /// Run a context-independent synchronous renderer against this output.
    ///
    /// This is used by generated send templates for attributes and numeric
    /// formatting. The temporary context intentionally has no request state;
    /// stateful children must implement `WebRenderSend` and use this context
    /// directly rather than entering through this helper.
    #[doc(hidden)]
    pub fn __proa_with_stateless_context<R>(
        &mut self,
        render: impl FnOnce(&mut WebContext<NoLoader, &mut B>) -> R,
    ) -> R {
        let mut local = WebContext::with_stateless_buffer(&mut self.out);
        local.__proa_enter_mode(self.mode);
        local.__proa_enter_escape_dialect(self.escape_dialect);
        render(&mut local)
    }

    pub fn enter_mode(&mut self, mode: ContentMode) -> ContentMode {
        std::mem::replace(&mut self.mode, mode)
    }

    pub fn restore_mode(&mut self, previous: ContentMode) {
        self.mode = previous;
    }

    pub fn current_mode(&self) -> ContentMode {
        self.mode
    }

    #[doc(hidden)]
    pub fn __proa_enter_mode(&mut self, mode: ContentMode) -> ContentMode {
        self.enter_mode(mode)
    }

    #[doc(hidden)]
    pub fn __proa_restore_mode(&mut self, previous: ContentMode) {
        self.restore_mode(previous);
    }

    #[doc(hidden)]
    pub fn __proa_enter_escape_dialect(&mut self, dialect: EscapeDialect) -> EscapeDialect {
        std::mem::replace(&mut self.escape_dialect, dialect)
    }

    #[doc(hidden)]
    pub fn __proa_restore_escape_dialect(&mut self, previous: EscapeDialect) {
        self.escape_dialect = previous;
    }

    #[doc(hidden)]
    pub fn __proa_current_escape_dialect(&self) -> EscapeDialect {
        self.escape_dialect
    }

    #[deprecated(
        note = "use load_json for JSON bytes, load_bytes for raw bytes, or load_typed for typed values"
    )]
    pub async fn load<T>(&self, key: LoadKey) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json(key).await
    }

    pub async fn load_bytes(&self, key: LoadKey) -> Result<Bytes, LoadError> {
        self.load_bytes_request(LoadRequest::new(key)).await
    }

    pub async fn load_bytes_request(&self, request: LoadRequest) -> Result<Bytes, LoadError> {
        self.loader.load(&request).await?.into_bytes()
    }

    pub async fn load_json<T>(&self, key: LoadKey) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(LoadRequest::new(key)).await
    }

    pub async fn load_json_with<T>(&self, key: LoadKey, hints: LoadHints) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        self.load_json_request(LoadRequest::with_hints(key, hints))
            .await
    }

    pub async fn load_json_request<T>(&self, request: LoadRequest) -> Result<T, LoadError>
    where
        T: DeserializeOwned,
    {
        let bytes = self.load_bytes_request(request).await?;
        serde_json::from_slice(bytes.as_ref()).map_err(LoadError::from)
    }

    pub async fn load_typed<T>(&self, key: LoadKey) -> Result<Arc<T>, LoadError>
    where
        T: Send + Sync + 'static,
    {
        self.load_typed_request(LoadRequest::new(key)).await
    }

    pub async fn load_typed_request<T>(&self, request: LoadRequest) -> Result<Arc<T>, LoadError>
    where
        T: Send + Sync + 'static,
    {
        self.loader.load(&request).await?.downcast::<T>()
    }
}

impl<L: DataLoader> SendWebContext<L, RenderBuffer> {
    /// Flush the current in-order chunk to a send-safe synchronous sink and
    /// recycle the standard buffer pages.
    pub fn flush_stream_chunk(&mut self) -> Result<(), WriteError> {
        let Some(flusher) = self.stream_chunk_flusher.as_ref().cloned() else {
            return Ok(());
        };
        if self.out.is_empty() {
            return Ok(());
        }
        self.parts.response.mark_stream_chunk_flushed();
        let result = flusher.flush_chunk_view(&self.out);
        self.out.clear();
        result
    }
}

struct ErasedSendLoaderScope<'a, L: DataLoader> {
    parent: &'a mut SendWebContext<L>,
    child: SendWebContext<crate::DynLoaderHandle>,
}
impl<L: DataLoader> Drop for ErasedSendLoaderScope<'_, L> {
    fn drop(&mut self) {
        std::mem::swap(
            &mut self.parent.metadata_state,
            &mut self.child.metadata_state,
        );
        std::mem::swap(&mut self.parent.out, &mut self.child.out);
        std::mem::swap(&mut self.parent.mode, &mut self.child.mode);
        std::mem::swap(
            &mut self.parent.escape_dialect,
            &mut self.child.escape_dialect,
        );
        std::mem::swap(&mut self.parent.render_mode, &mut self.child.render_mode);
        std::mem::swap(&mut self.parent.parts, &mut self.child.parts);
        std::mem::swap(
            &mut self.parent.stream_recorder,
            &mut self.child.stream_recorder,
        );
        std::mem::swap(
            &mut self.parent.stream_chunk_flusher,
            &mut self.child.stream_chunk_flusher,
        );
    }
}
impl<L: DataLoader + 'static> SendWebContext<L> {
    #[doc(hidden)]
    pub fn __with_erased_loader<R>(
        &mut self,
        setup: impl FnOnce(&mut SendWebContext<crate::DynLoaderHandle>) -> R,
    ) -> R {
        // Every field is either swapped into the setup context below or
        // deliberately kept on the parent. A new field fails to compile here
        // until it is classified, so setup cannot silently lose live state.
        let SendWebContext {
            metadata: _,
            out: _,
            metadata_state: _,
            mode: _,
            escape_dialect: _,
            render_mode: _,
            parts: _,
            stream_recorder: _,
            stream_chunk_flusher: _,
            // Kept on the parent: setup has its own loader.
            loader: _,
        } = &*self;
        let loader: Arc<dyn crate::DynDataLoader> = self.loader.clone();
        // An inert placeholder shares immutable identity only; no request state
        // is cloned or lost when the real parts are moved into the setup context.
        let parts = SendWebContextParts {
            metadata_denied: true,
            request: Arc::clone(&self.parts.request),
            response: ResponseContext::new(),
            head: HeadContext::new(),
            cache_trace: CacheTrace::new(),
            telemetry: RenderTelemetry::new(),
            locals: Locals::new(),
            csp_nonce: None,
            artifacts: RenderArtifacts::default(),
            next_react_island_id: Arc::clone(&self.parts.next_react_island_id),
        };
        let mut child = SendWebContext {
            metadata: super::DocumentMetadata,
            metadata_state: super::metadata::MetadataState::default(),
            out: RenderBuffer::new(),
            mode: ContentMode::Html,
            escape_dialect: EscapeDialect::Html,
            render_mode: RenderMode::Static,
            parts,
            loader: Arc::new(crate::DynLoaderHandle::new(loader)),
            stream_recorder: None,
            stream_chunk_flusher: None,
        };
        std::mem::swap(&mut self.metadata_state, &mut child.metadata_state);
        std::mem::swap(&mut self.out, &mut child.out);
        std::mem::swap(&mut self.mode, &mut child.mode);
        std::mem::swap(&mut self.escape_dialect, &mut child.escape_dialect);
        std::mem::swap(&mut self.render_mode, &mut child.render_mode);
        std::mem::swap(&mut self.parts, &mut child.parts);
        std::mem::swap(&mut self.stream_recorder, &mut child.stream_recorder);
        std::mem::swap(
            &mut self.stream_chunk_flusher,
            &mut child.stream_chunk_flusher,
        );
        let mut scope = ErasedSendLoaderScope {
            parent: self,
            child,
        };
        setup(&mut scope.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn context_is_send_and_detached_artifacts_are_branch_owned() {
        assert_send::<SendWebContext>();

        let mut root = SendWebContext::new();
        root.react_island_used("Root");
        let parts = root.context_parts();
        let mut child = SendWebContext::with_loader_buffer_and_context_parts(
            RenderBuffer::new(),
            Arc::new(NoLoader),
            parts,
        );
        child.react_island_used("Child");

        assert_eq!(root.render_artifacts_snapshot().react_islands, ["Root"]);
        assert_eq!(child.render_artifacts_snapshot().react_islands, ["Child"]);
        assert_eq!(root.next_react_island_id(), 0);
        assert_eq!(child.next_react_island_id(), 1);
    }
}
