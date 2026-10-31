// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use proa_core::md::MdState;
use proa_core::{
    DataLoader, MdRender, PendingBoundary, Priority, RenderBuffer, RenderMode, RenderOutcome,
    RenderedBoundary, SendPendingBoundary, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSync, WriteBuf, WriteError,
};
use tracing::error;

use crate::context::{LocalBoundaryBranch, SendBoundaryBranch};

/// Explicit fallback boundary for streamed rendering.
///
/// `priority` and `timeout` apply to out-of-order HTML streaming. Every
/// boundary starts concurrently; priority deterministically orders boundaries
/// that become ready in the same driver turn, and timeout cancels a stalled
/// boundary while leaving its shell fallback in place.
///
/// Buffered/static HTML, in-order HTML, and Markdown always preserve document
/// order and render the child inline. They intentionally ignore both
/// scheduling options: reordering would violate their wire contract, while
/// cancelling an in-order child after it has flushed could leave partial
/// markup on the wire.
pub struct Suspense<C, Fb> {
    pub child: C,
    pub fallback: Fb,
    pub priority: Priority,
    pub timeout: Option<Duration>,
}

impl<C, Fb> Suspense<C, Fb> {
    pub fn new(child: C, fallback: Fb) -> Self {
        Self {
            child,
            fallback,
            priority: Priority::Critical,
            timeout: None,
        }
    }
}

/// Discards only the currently-unflushed suffix when a same-context in-order
/// boundary is cancelled, unwinds, or returns before its final successful
/// flush. Nested successful flushes have already cleared the buffer and remain
/// committed on the wire.
struct ClearRenderBufferOnDrop {
    out: *mut RenderBuffer,
    armed: bool,
}

impl ClearRenderBufferOnDrop {
    fn new(out: &mut RenderBuffer) -> Self {
        Self {
            out: std::ptr::from_mut(out),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ClearRenderBufferOnDrop {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: the guard is created inside the boundary future from
            // that future's exclusive context borrow before the awaited child
            // future. The child future is dropped first during cancellation,
            // so no outstanding child access remains when cleanup runs.
            unsafe { (*self.out).clear() };
        }
    }
}

/// Restores the Markdown-local state that the old disposable child context
/// isolated when a same-context boundary does not complete successfully.
struct RestoreMarkdownBoundaryOnDrop<L: DataLoader> {
    cx: *mut WebContext<L, RenderBuffer>,
    state: MdState,
    armed: bool,
}

impl<L: DataLoader> RestoreMarkdownBoundaryOnDrop<L> {
    fn new(cx: &mut WebContext<L, RenderBuffer>) -> Self {
        Self {
            cx: std::ptr::from_mut(cx),
            state: cx.md_state(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<L: DataLoader> Drop for RestoreMarkdownBoundaryOnDrop<L> {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: this guard is created before the awaited child future,
            // which is dropped first on cancellation/unwind. The boundary
            // owns the exclusive context borrow for the guard's lifetime.
            unsafe {
                (*self.cx).out.clear();
                (*self.cx).set_md_state(self.state);
            }
        }
    }
}

/// Standard-writer `Suspense` implementation.
///
/// In-order rendering flushes and reuses the parent `RenderBuffer` on the same
/// context after managed head publication (or immediately in manual mode).
/// Before publication, use buffered rendering: a held flush does not empty the
/// writer, so streaming cleanup must not clear the retained document prefix.
/// Out-of-order rendering gives each detached boundary its own owned
/// `RenderBuffer` and preserves its pages through transport framing.
// These expand at the existing return points; there is no extra future or
// stored optional guard on metadata-free boundary paths.
macro_rules! boundary_return {
    ($cx:expr, $result:expr) => {{
        let result = $result;
        if let Some(document) = $cx.head_state().document() {
            document.leave_boundary();
        }
        return result;
    }};
}
macro_rules! boundary_try {
    ($cx:expr, $result:expr) => {{
        match $result {
            Ok(value) => value,
            Err(error) => boundary_return!($cx, Err(error)),
        }
    }};
}

impl<C, Fb, L> WebRender<L, RenderBuffer> for Suspense<C, Fb>
where
    L: DataLoader + 'static,
    C: WebRender<L, RenderBuffer> + 'static,
    Fb: WebRenderSync<L, RenderBuffer>,
{
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let Self {
            child,
            fallback,
            priority,
            timeout,
        } = self;

        if let Some(document) = cx.head_state().document() {
            document.enter_boundary();
        }
        if matches!(cx.render_mode(), RenderMode::ChunkedOutOfOrder) {
            if let Some(recorder) = cx.stream_recorder().cloned() {
                let boundary_id = recorder.next_boundary_id();
                let context_parts = cx.context_parts();
                let nested_stream = Rc::new(LocalBoundaryBranch::new(Rc::clone(&recorder)));
                let child_stream: Rc<dyn proa_core::StreamBoundaryRecorder> =
                    Rc::clone(&nested_stream) as Rc<dyn proa_core::StreamBoundaryRecorder>;
                let error_fallback = cx.current_boundary_error_fallback();
                let error_fallback_stack = cx.boundary_error_fallback_stack();
                let loader: Arc<L> = Arc::clone(cx.loader());
                recorder.push_boundary(PendingBoundary::with_options(
                    boundary_id,
                    Box::pin(async move {
                        let mut child_cx =
                            WebContext::<L, RenderBuffer>::with_loader_buffer_and_context_parts(
                                RenderBuffer::new(),
                                loader,
                                context_parts,
                            )
                            .with_render_mode(RenderMode::ChunkedOutOfOrder);
                        child_cx.set_stream_recorder(child_stream);
                        child_cx.set_boundary_error_fallback_stack(error_fallback_stack);
                        let artifact_branch = child_cx.rsjs_artifact_branch();
                        let result = proa_core::render_in_context_async(&mut child_cx, child).await;
                        if let Some(head) = child_cx.head_state().document() {
                            head.check_terminal()?;
                        }
                        let artifacts = match result {
                            Ok(()) => artifact_branch.commit_render_artifacts(&child_cx),
                            Err(err) if err.is_boundary_catchable() => {
                                let Some(error_fallback) = error_fallback.as_ref() else {
                                    return Err(err);
                                };
                                child_cx.out.clear();
                                artifact_branch.rollback(&child_cx);
                                error!(
                                    ?err,
                                    mode = RenderMode::ChunkedOutOfOrder.as_str(),
                                    "streaming boundary recovered with error fallback"
                                );
                                let fallback_branch = child_cx.rsjs_artifact_branch();
                                error_fallback.render_buffer(&mut child_cx)?;
                                if let Some(head) = child_cx.head_state().document() {
                                    head.check_terminal()?;
                                }
                                fallback_branch.commit_render_artifacts(&child_cx)
                            }
                            Err(err) => return Err(err),
                        };
                        let out = child_cx.into_output();
                        let nested = nested_stream.take_boundaries();
                        Ok(RenderedBoundary::with_nested(out, artifacts, nested))
                    }),
                    priority,
                    timeout,
                ));

                let result = write_boundary_comment(cx, b"start", boundary_id)
                    .and_then(|()| proa_core::head_dispatch::sync(fallback, cx))
                    .and_then(|()| write_boundary_comment(cx, b"end", boundary_id));
                if let Some(document) = cx.head_state().document() {
                    document.leave_boundary();
                }
                return RenderOutcome::<
                    std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<(), WriteError>> + '_>,
                    >,
                >::Done(result);
            }
        }

        if matches!(cx.render_mode(), RenderMode::ChunkedInOrder)
            && cx.stream_chunk_flusher().is_some()
            && cx.head_state().document().is_none_or(|d| d.is_published())
        {
            let pre_flush = cx.flush_stream_chunk();
            let future: std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), WriteError>> + '_>,
            > = Box::pin(async move {
                boundary_try!(cx, pre_flush);
                // Declared before the child future so cancellation drops the
                // child borrow before this guard clears its unshipped bytes.
                let mut output_guard = ClearRenderBufferOnDrop::new(&mut cx.out);
                let error_fallback = cx.current_boundary_error_fallback();
                let seed_checkpoint = cx.rsjs_shared_state_seed_checkpoint();
                let artifact_checkpoint = cx.rsjs_artifact_checkpoint();
                match proa_core::render_in_context_async(cx, child).await {
                    Ok(()) => {
                        cx.rsjs_shared_state_commit_seeds(seed_checkpoint);
                        cx.rsjs_artifact_commit(artifact_checkpoint);
                    }
                    Err(err) if err.is_boundary_catchable() => {
                        let Some(error_fallback) = error_fallback.as_ref() else {
                            cx.rsjs_shared_state_commit_seeds(seed_checkpoint);
                            cx.rsjs_artifact_commit(artifact_checkpoint);
                            error!(
                                ?err,
                                mode = RenderMode::ChunkedInOrder.as_str(),
                                "streaming boundary failed without error boundary"
                            );
                            boundary_return!(cx, Err(err));
                        };
                        cx.out.clear();
                        cx.rsjs_shared_state_rollback_seeds(seed_checkpoint);
                        cx.rsjs_artifact_rollback(artifact_checkpoint);
                        error!(
                            ?err,
                            mode = RenderMode::ChunkedInOrder.as_str(),
                            "streaming boundary recovered with error fallback"
                        );
                        boundary_try!(cx, error_fallback.render_buffer(cx));
                    }
                    Err(err) => {
                        cx.rsjs_shared_state_commit_seeds(seed_checkpoint);
                        cx.rsjs_artifact_commit(artifact_checkpoint);
                        boundary_return!(cx, Err(err));
                    }
                }
                let result = cx.flush_stream_chunk();
                output_guard.disarm();
                if let Some(document) = cx.head_state().document() {
                    document.leave_boundary();
                }
                result
            });
            return RenderOutcome::Pending(future);
        }

        let _ = fallback;
        // Do not wait for head readiness here. The selected Page/outlet may be
        // a later sibling; keep rendering so discovery can reach it. An outer
        // ErrorBoundary owns buffered rollback to its checkpoint on failure.
        RenderOutcome::Pending(Box::pin(async move {
            let result = proa_core::render_child(cx, child).await;
            if let Some(document) = cx.head_state().document() {
                document.leave_boundary();
            }
            result
        }))
    }
}

/// Compiler-proved `Send` Suspense path.
///
/// Detached children own their context and artifact delta. The only shared
/// mutable object is the send-safe boundary registry, and a boundary future is
/// inserted only through `SendPendingBoundary`.
impl<C, Fb, L> WebRenderSend<L, RenderBuffer> for Suspense<C, Fb>
where
    L: DataLoader + 'static,
    C: WebRenderSend<L, RenderBuffer> + 'static,
    Fb: WebRenderSend<L, RenderBuffer> + 'static,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            if let Some(document) = cx.head_state().document() {
                document.enter_boundary();
            }
            let Self {
                child,
                fallback,
                priority,
                timeout,
            } = self;

            if matches!(cx.render_mode(), RenderMode::ChunkedOutOfOrder) {
                if let Some(recorder) = cx.stream_recorder().cloned() {
                    let boundary_id = recorder.next_boundary_id();
                    let context_parts = cx.context_parts();
                    let nested_stream = Arc::new(SendBoundaryBranch::new(Arc::clone(&recorder)));
                    let child_stream: Arc<dyn proa_core::SendStreamBoundaryRecorder> =
                        nested_stream.clone();
                    let loader = Arc::clone(cx.loader());
                    recorder.push_boundary(SendPendingBoundary::with_options(
                        boundary_id,
                        Box::pin(async move {
                            let mut child_cx =
                                SendWebContext::with_loader_buffer_and_context_parts(
                                    RenderBuffer::new(),
                                    loader,
                                    context_parts,
                                )
                                .with_render_mode(RenderMode::ChunkedOutOfOrder);
                            child_cx.set_stream_recorder(child_stream);
                            proa_core::render_in_send_context(&mut child_cx, child).await?;
                            if let Some(head) = child_cx.head_state().document() {
                                head.check_terminal()?;
                            }
                            let (html, artifacts) = child_cx.into_output_and_render_artifacts();
                            let nested = nested_stream.take_boundaries();
                            Ok(RenderedBoundary::with_nested(html, artifacts, nested))
                        }),
                        priority,
                        timeout,
                    ));

                    boundary_try!(cx, write_send_boundary_comment(cx, b"start", boundary_id));
                    boundary_try!(cx, proa_core::render_send_child(cx, fallback).await);
                    boundary_return!(cx, write_send_boundary_comment(cx, b"end", boundary_id));
                }
            }

            let _ = fallback;
            if matches!(cx.render_mode(), RenderMode::ChunkedInOrder)
                && cx.stream_chunk_flusher().is_some()
                // Sealing alone is insufficient: an outlet may still be ahead.
                && cx.head_state().document().is_none_or(|d| d.is_published())
            {
                let pre_flush = cx.flush_stream_chunk();
                boundary_try!(cx, pre_flush);
                if let Err(error) = proa_core::render_send_child(cx, child).await {
                    cx.out.clear();
                    boundary_return!(cx, Err(error));
                }
                boundary_return!(cx, cx.flush_stream_chunk());
            }

            let result = proa_core::render_send_child(cx, child).await;
            if let Some(document) = cx.head_state().document() {
                document.leave_boundary();
            }
            result
        })
    }
}

impl<Loader: ::proa_core::DataLoader, C, Fb> MdRender<Loader, RenderBuffer> for Suspense<C, Fb>
where
    C: MdRender<Loader, RenderBuffer>,
{
    fn render_md(
        self,
        cx: &mut WebContext<Loader, RenderBuffer>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let Self {
            child,
            fallback,
            priority: _priority,
            timeout: _timeout,
        } = self;
        let _ = fallback;

        // Flush eagerly, matching HTML's in-order contract. One concrete
        // future captures both the child and context borrow without forcing
        // either lifetime to outlive the other through an erased future.
        let pre_flush = if matches!(cx.render_mode(), RenderMode::ChunkedInOrder)
            && cx.stream_chunk_flusher().is_some()
            // Markdown may share a managed HTML context and its retained writer.
            && cx.head_state().document().is_none_or(|d| d.is_published())
        {
            Some(cx.flush_stream_chunk())
        } else {
            None
        };
        RenderOutcome::pending(async move {
            if let Some(result) = pre_flush {
                result?;
                let mut restore = RestoreMarkdownBoundaryOnDrop::new(cx);
                proa_core::render_md_in_context_async(cx, child).await?;
                cx.flush_stream_chunk()?;
                restore.disarm();
                Ok(())
            } else {
                proa_core::render_md_child(cx, child).await
            }
        })
    }
}

// proa-perf-ok(scope): boundary markers are low-level streaming protocol comments
fn write_boundary_comment<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    marker: &'static [u8],
    boundary_id: u64,
) -> Result<(), WriteError> {
    WriteBuf::extend_static(&mut cx.out, b"<!--proa:")?;
    WriteBuf::extend_static(&mut cx.out, marker)?;
    WriteBuf::extend_static(&mut cx.out, b":")?;
    WebRenderSync::render(boundary_id, cx)?;
    WriteBuf::extend_static(&mut cx.out, b"-->")
}

fn write_send_boundary_comment<B: WriteBuf + Send, L: DataLoader>(
    cx: &mut SendWebContext<L, B>,
    marker: &'static [u8],
    boundary_id: u64,
) -> Result<(), WriteError> {
    WriteBuf::extend_static(&mut cx.out, b"<!--proa:")?;
    WriteBuf::extend_static(&mut cx.out, marker)?;
    WriteBuf::extend_static(&mut cx.out, b":")?;
    cx.__proa_with_stateless_context(|local| WebRenderSync::render(boundary_id, local))?;
    WriteBuf::extend_static(&mut cx.out, b"-->")
}
