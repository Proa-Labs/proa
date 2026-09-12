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
    pub async fn resolve(self) -> Result<(), WriteError> {
        match self {
            RenderOutcome::Done(r) => r,
            RenderOutcome::Pending(fut) => fut.await,
        }
    }
}
