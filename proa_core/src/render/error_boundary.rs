// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::rc::Rc;

use crate::render::loader::{DataLoader, NoLoader};
use crate::render::outcome::RenderOutcome;
use crate::render::web_render::{WebRender, WebRenderSync};
use crate::shared::write_buf::{CheckpointWriteBuf, RenderBuffer, WriteError};
use crate::web::{RsjsArtifactCheckpoint, RsjsSharedStateSeedCheckpoint, WebContext};

/// Scoped render error recovery.
///
/// `ErrorBoundary` catches catchable `WriteError`s from `child`, rewinds bytes
/// written since the boundary started, and renders `fallback` in their place.
/// In streaming modes, `Suspense` also captures the nearest fallback from the
/// context stack so deferred errors can recover after the boundary itself has
/// returned.
pub struct ErrorBoundary<C, Ef> {
    pub child: C,
    pub fallback: Ef,
}

impl<C, Ef> ErrorBoundary<C, Ef> {
    pub const fn new(child: C, fallback: Ef) -> Self {
        Self { child, fallback }
    }
}

/// Type-erased error fallback captured by streamed `Suspense` boundaries.
pub trait BoundaryErrorFallback<L: DataLoader>: 'static {
    fn render_buffer(&self, cx: &mut WebContext<L, RenderBuffer>) -> Result<(), WriteError>;
}

pub struct RenderBoundaryErrorFallback<Ef> {
    fallback: Ef,
}

impl<Ef> RenderBoundaryErrorFallback<Ef> {
    pub fn new(fallback: Ef) -> Self {
        Self { fallback }
    }
}

impl<Ef, L> BoundaryErrorFallback<L> for RenderBoundaryErrorFallback<Ef>
where
    Ef: Clone + WebRenderSync<L, RenderBuffer> + 'static,
    L: DataLoader + 'static,
{
    fn render_buffer(&self, cx: &mut WebContext<L, RenderBuffer>) -> Result<(), WriteError> {
        let _excluded = cx.head_state().document().map(|d| d.exclude());
        crate::head_dispatch::sync(self.fallback.clone(), cx)
    }
}

fn recover_or_propagate<B, L, Ef>(
    cx: &mut WebContext<L, B>,
    checkpoint: B::Checkpoint,
    artifact_checkpoint: RsjsArtifactCheckpoint,
    seed_checkpoint: RsjsSharedStateSeedCheckpoint,
    fallback: Ef,
    result: Result<(), WriteError>,
) -> Result<(), WriteError>
where
    B: CheckpointWriteBuf,
    L: DataLoader,
    Ef: WebRenderSync<L, B>,
{
    if let Some(document) = cx.head_state().document() {
        document.finish_recovery(seed_checkpoint.transaction_id(), &result)?;
    }
    match result {
        Ok(()) => {
            cx.rsjs_artifact_commit(artifact_checkpoint);
            cx.rsjs_shared_state_commit_seeds(seed_checkpoint);
            Ok(())
        }
        Err(err) if err.is_boundary_catchable() && cx.out.rewind_to(checkpoint) => {
            // The child's still-buffered bytes were just rewound, so the
            // rsjs artifacts and shared-state seeds registered by the
            // discarded subtree must not ship to the client.
            //
            // A successful `rewind_to` is not by itself proof that the
            // subtree's markup is gone: in streaming modes a nested
            // `Suspense` flushes the enclosing buffer mid-subtree, and on a
            // `Vec<u8>` shell that swaps in a fresh buffer, so the offset
            // recorded above describes a buffer that no longer exists and
            // rewinding to it succeeds while the earlier markup is already
            // on the wire. `rsjs_shared_state_rollback_seeds` compares the
            // stream flush generation captured in `seed_checkpoint` against
            // the live one and keeps the seeds in exactly that case, so
            // markup the browser can already see still hydrates.
            cx.rsjs_artifact_rollback(artifact_checkpoint);
            cx.rsjs_shared_state_rollback_seeds(seed_checkpoint);
            tracing::error!(?err, "error boundary recovered with fallback");
            let _excluded = cx.head_state().document().map(|d| d.exclude());
            crate::head_dispatch::sync(fallback, cx)
        }
        Err(err) => {
            // This boundary did not rewind. Merge its response artifacts into
            // the parent transaction so an outer boundary can still discard
            // the whole subtree if it catches the propagated error.
            cx.rsjs_artifact_commit(artifact_checkpoint);
            cx.rsjs_shared_state_commit_seeds(seed_checkpoint);
            Err(err)
        }
    }
}

impl<L, B, C, Ef> WebRenderSync<L, B> for ErrorBoundary<C, Ef>
where
    B: CheckpointWriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B>,
    Ef: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if Ef::ROLE.is_page() {
            if let Some(document) = cx.head_state().document() {
                return document.reject_component(
                    std::any::type_name::<Ef>(),
                    "error fallback",
                    crate::HeadError::Excluded,
                );
            }
        }
        let checkpoint = cx.out.checkpoint();
        let artifact_checkpoint = cx.rsjs_artifact_checkpoint();
        let seed_checkpoint = cx.rsjs_shared_state_seed_checkpoint();
        if let Some(document) = cx.head_state().document() {
            document.begin_recovery(seed_checkpoint.transaction_id())?;
        }
        let result = crate::head_dispatch::sync(self.child, cx);
        recover_or_propagate(
            cx,
            checkpoint,
            artifact_checkpoint,
            seed_checkpoint,
            self.fallback,
            result,
        )
    }
}

impl<L, B, C, Ef> WebRender<L, B> for ErrorBoundary<C, Ef>
where
    B: CheckpointWriteBuf,
    L: DataLoader + 'static,
    C: WebRender<L, B>,
    Ef: Clone + WebRenderSync<L, B> + WebRenderSync<L, RenderBuffer> + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        if <Ef as WebRenderSync<L, B>>::ROLE.is_page() {
            if let Some(document) = cx.head_state().document() {
                return RenderOutcome::Done(document.reject_component(
                    std::any::type_name::<Ef>(),
                    "error fallback",
                    crate::HeadError::Excluded,
                ));
            }
        }
        let checkpoint = cx.out.checkpoint();
        let artifact_checkpoint = cx.rsjs_artifact_checkpoint();
        let seed_checkpoint = cx.rsjs_shared_state_seed_checkpoint();
        if let Some(document) = cx.head_state().document() {
            if let Err(error) = document.begin_recovery(seed_checkpoint.transaction_id()) {
                return RenderOutcome::Done(Err(error));
            }
        }
        // proa-perf-ok: async boundaries need one owned fallback for deferred Suspense capture and one for local recovery
        let fallback_for_stack_value = self.fallback.clone();
        let fallback_for_stack: Rc<dyn BoundaryErrorFallback<L>> =
            Rc::new(RenderBoundaryErrorFallback::new(fallback_for_stack_value));
        let fallback_scope = cx.push_boundary_error_fallback(fallback_for_stack);

        RenderOutcome::Pending(Box::pin(async move {
            let result = crate::head_dispatch::begin(self.child, cx).resolve().await;
            drop(fallback_scope);
            recover_or_propagate(
                cx,
                checkpoint,
                artifact_checkpoint,
                seed_checkpoint,
                self.fallback,
                result,
            )
        }))
    }
}

impl<C, Ef> Default for ErrorBoundary<C, Ef>
where
    C: Default,
    Ef: Default,
{
    fn default() -> Self {
        Self {
            child: C::default(),
            fallback: Ef::default(),
        }
    }
}

impl<C, Ef> From<(C, Ef)> for ErrorBoundary<C, Ef> {
    fn from((child, fallback): (C, Ef)) -> Self {
        Self { child, fallback }
    }
}

pub type BoundaryErrorFallbackHandle<L = NoLoader> = Rc<dyn BoundaryErrorFallback<L>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::WebRenderSync;
    use crate::shared::html_node::static_html;
    use crate::shared::write_buf::WriteBuf;

    struct PartialThenErr {
        partial: &'static [u8],
        message: &'static str,
    }

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PartialThenErr {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.out.extend_static(self.partial)?;
            Err(WriteError::render(self.message))
        }
    }

    struct InnerThenOuterErr;

    impl<L: DataLoader, B: WriteBuf + CheckpointWriteBuf> WebRenderSync<L, B> for InnerThenOuterErr {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            WebRenderSync::render(
                ErrorBoundary::new(
                    PartialThenErr {
                        partial: b"<span>inner partial</span>",
                        message: "inner failed",
                    },
                    static_html(b"<span class=\"inner\">inner fallback</span>"),
                ),
                cx,
            )?;
            cx.out.extend_static(b"<span>outer partial</span>")?;
            Err(WriteError::render("outer failed"))
        }
    }

    struct FatalAfterPartial;

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FatalAfterPartial {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.rsjs_island_used("fatal-island");
            cx.out.extend_static(b"<span>partial</span>")?;
            Err(WriteError::OutOfCapacity)
        }
    }

    struct ArtifactThenErr {
        asset_id: &'static str,
        query_key: &'static str,
    }

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ArtifactThenErr {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.rsjs_island_used(self.asset_id);
            cx.rsjs_query_seed_json(self.query_key, r#"{"value":"doomed"}"#);
            cx.out.extend_static(b"<span>doomed</span>")?;
            Err(WriteError::render("artifact branch failed"))
        }
    }

    struct ArtifactFallback;

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ArtifactFallback {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.rsjs_island_used("fallback-island");
            cx.rsjs_query_seed_json(r#"["fallback"]"#, r#"{"value":"fallback"}"#);
            cx.out.extend_static(b"<span>fallback</span>")
        }
    }

    struct ArtifactThenFlushedErr;

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ArtifactThenFlushedErr {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.rsjs_island_used("already-shipped-island");
            cx.out.extend_static(b"<span>already shipped</span>")?;
            cx.response_mut().mark_stream_chunk_flushed();
            Err(WriteError::render("failed after flush"))
        }
    }

    struct PendingArtifact;

    impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for PendingArtifact {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            cx.rsjs_island_used("cancelled-async-island");
            RenderOutcome::Pending(std::future::pending())
        }
    }

    #[test]
    fn error_boundary_rewinds_partial_sync_output() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        WebRenderSync::render(
            ErrorBoundary::new(
                PartialThenErr {
                    partial: b"<span>partial</span>",
                    message: "component failed",
                },
                static_html(b"<span class=\"error\">failed</span>"),
            ),
            &mut cx,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<span class=\"error\">failed</span>"
        );
    }

    #[test]
    fn error_boundary_rolls_back_child_artifacts_before_rendering_fallback() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        cx.rsjs_island_used("outside-island");
        cx.rsjs_query_seed_json(r#"["outside"]"#, r#"{"value":"outside"}"#);

        WebRenderSync::render(
            ErrorBoundary::new(
                ArtifactThenErr {
                    asset_id: "doomed-island",
                    query_key: r#"["doomed"]"#,
                },
                ArtifactFallback,
            ),
            &mut cx,
        )
        .unwrap();

        let artifacts = cx.rsjs_artifacts_snapshot();
        assert_eq!(
            artifacts.rendered_islands().collect::<Vec<_>>(),
            ["fallback-island", "outside-island"]
        );
        assert_eq!(
            artifacts
                .query_seeds()
                .iter()
                .map(|seed| seed.key_json())
                .collect::<Vec<_>>(),
            [r#"["fallback"]"#, r#"["outside"]"#]
        );
        assert_eq!(out, b"<span>fallback</span>");
    }

    #[test]
    fn error_boundary_keeps_artifacts_when_child_markup_was_already_flushed() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        WebRenderSync::render(
            ErrorBoundary::new(ArtifactThenFlushedErr, ArtifactFallback),
            &mut cx,
        )
        .unwrap();

        assert_eq!(
            cx.rsjs_artifacts_snapshot()
                .rendered_islands()
                .collect::<Vec<_>>(),
            ["already-shipped-island", "fallback-island"]
        );
        assert_eq!(out, b"<span>fallback</span>");
    }

    #[tokio::test]
    async fn cancelling_async_error_boundary_rolls_back_open_artifacts() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            WebRender::render(
                ErrorBoundary::new(
                    PendingArtifact,
                    static_html(b"<span>unused fallback</span>"),
                ),
                &mut cx,
            )
            .resolve(),
        )
        .await;

        assert!(result.is_err());
        assert!(cx.rsjs_artifacts_snapshot().is_empty());
    }

    #[test]
    fn nested_error_boundary_uses_nearest_fallback() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        WebRenderSync::render(
            ErrorBoundary::new(
                ErrorBoundary::new(
                    PartialThenErr {
                        partial: b"<span>inner partial</span>",
                        message: "inner failed",
                    },
                    static_html(b"<span class=\"inner\">inner fallback</span>"),
                ),
                static_html(b"<span class=\"outer\">outer fallback</span>"),
            ),
            &mut cx,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<span class=\"inner\">inner fallback</span>"
        );
    }

    #[test]
    fn outer_error_boundary_catches_errors_outside_inner_boundary() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        WebRenderSync::render(
            ErrorBoundary::new(
                InnerThenOuterErr,
                static_html(b"<span class=\"outer\">outer fallback</span>"),
            ),
            &mut cx,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<span class=\"outer\">outer fallback</span>"
        );
    }

    #[test]
    fn error_boundary_propagates_systemic_errors() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let err = WebRenderSync::render(
            ErrorBoundary::new(
                FatalAfterPartial,
                static_html(b"<span class=\"error\">failed</span>"),
            ),
            &mut cx,
        )
        .unwrap_err();

        assert_eq!(err, WriteError::OutOfCapacity);
        assert_eq!(
            cx.rsjs_artifacts_snapshot()
                .rendered_islands()
                .collect::<Vec<_>>(),
            ["fatal-island"]
        );
        assert_eq!(String::from_utf8(out).unwrap(), "<span>partial</span>");
    }
}
