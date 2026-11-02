// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `RenderOutcome` enum that `WebRender::render` returns.
//!
//! A render either completes synchronously (the common case for static
//! components) or returns a future that must be awaited (the async-fn case).
//! The enum lets the sync path bypass all async machinery.

use std::future::Future;

use crate::shared::write_buf::WriteError;

/// Outcome of a `WebRender::render` call.
///
/// - `Done(result)` — the render completed synchronously. No future was
///   constructed; no heap allocation happened.
/// - `Pending(future)` — the render is async and must be `.await`ed.
///
/// The `F` type parameter is the concrete future type for `Pending`. Sync
/// components use `Ready<Result<(), WriteError>>` as a placeholder that's
/// never actually constructed (they only return `Done`). Async components
/// usually return a compiler-generated `async move` future, keeping the
/// hot path allocation-free under static dispatch.
pub enum RenderOutcome<F> {
    Done(Result<(), WriteError>),
    Pending(F),
}

impl<F> RenderOutcome<F> {
    /// Sync success shortcut.
    pub const fn done_ok() -> Self {
        RenderOutcome::Done(Ok(()))
    }

    /// Sync error shortcut.
    pub const fn done_err(e: WriteError) -> Self {
        RenderOutcome::Done(Err(e))
    }

    /// Wrap a future as a pending outcome.
    pub const fn pending(fut: F) -> Self {
        RenderOutcome::Pending(fut)
    }
}

impl<F: Future<Output = Result<(), WriteError>>> RenderOutcome<F> {
    /// Drive this outcome to completion, awaiting the future if pending.
    ///
    /// Used by the async root driver (`render_to_vec_async` etc.). For
    /// the sync fast path in `TwoSync`, components call their children's
    /// `WebRenderSync::render` directly and never reach this method.
    pub fn resolve(self) -> impl Future<Output = Result<(), WriteError>> {
        match self {
            RenderOutcome::Done(result) => Resolve::Done {
                result: Some(result),
            },
            RenderOutcome::Pending(future) => Resolve::Pending { future },
        }
    }
}

// An async `match self { Pending(future) => future.await, ... }` keeps room for
// both the original outcome and the extracted future in its state machine.
// Nested renderers then double their future size at each resolve boundary.
// Project the existing future in place instead: no boxing or duplicate state.
pin_project_lite::pin_project! {
    #[project = ResolveProj]
    enum Resolve<F> {
        Done { result: Option<Result<(), WriteError>> },
        Pending { #[pin] future: F },
    }
}

impl<F: Future<Output = Result<(), WriteError>>> Future for Resolve<F> {
    type Output = Result<(), WriteError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project() {
            ResolveProj::Done { result } => std::task::Poll::Ready(
                result
                    .take()
                    .expect("render outcome polled after completion"),
            ),
            ResolveProj::Pending { future } => future.poll(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nest(
        future: impl Future<Output = Result<(), WriteError>>,
    ) -> impl Future<Output = Result<(), WriteError>> {
        RenderOutcome::Pending(future).resolve()
    }

    #[tokio::test]
    async fn nested_resolution_keeps_future_size_linear() {
        let data = [7_u8; 4096];
        let leaf = async move {
            tokio::task::yield_now().await;
            assert!(std::hint::black_box(&data).iter().all(|byte| *byte == 7));
            Ok(())
        };
        let leaf_size = std::mem::size_of_val(&leaf);
        let nested = nest(nest(nest(nest(leaf))));
        assert!(
            std::mem::size_of_val(&nested)
                <= leaf_size + 4 * std::mem::size_of::<Result<(), WriteError>>(),
            "resolving a child must not duplicate its future's storage"
        );
        nested.await.unwrap();
    }

    #[tokio::test]
    async fn resolution_preserves_done_pending_and_errors() {
        let done: RenderOutcome<std::future::Ready<Result<(), WriteError>>> =
            RenderOutcome::Done(Ok(()));
        done.resolve().await.unwrap();
        let failed: RenderOutcome<std::future::Ready<Result<(), WriteError>>> =
            RenderOutcome::Done(Err(WriteError::OutOfCapacity));
        assert!(matches!(
            failed.resolve().await,
            Err(WriteError::OutOfCapacity)
        ));
        let pending = RenderOutcome::Pending(async {
            tokio::task::yield_now().await;
            Err(WriteError::OutOfCapacity)
        });
        assert!(matches!(
            pending.resolve().await,
            Err(WriteError::OutOfCapacity)
        ));
    }
}
