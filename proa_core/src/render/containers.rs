// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Composite containers for sync-oriented HTML macro output.
//!
//! `html_sync!` emits `TwoSync` for two-child compositions. It is bounded on
//! `WebRenderSync` children, so it composes with the current Proa sync
//! component library at zero overhead.
//!
//! Mixed sync/async composition (`TwoMixed`) is deferred to Phase B of
//! the streaming plan, where it will live alongside the `Suspense`
//! primitive. Keeping `TwoMixed` out of proa_core for now avoids the
//! coherence problems around automatic sync-to-async lifting and a direct
//! `impl WebRender for TwoMixed`. We only ship `TwoSync` in `proa_core`;
//! richer async container shapes will be reintroduced behind an explicit
//! adapter strategy once streaming work lands.
//!
//! N-ary variants (`ThreeSync`, `FourSync`, etc., or a const-generic
//! `NSync<const N: usize>` equivalent) will be added when `html_sync!` needs
//! them in Phase B.

use crate::render::web_render::WebRenderSync;
use crate::render::DataLoader;
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::web::WebContext;

/// Sync composite: bounded on `WebRenderSync` children. The fast path.
///
/// Calls each child's `WebRenderSync::render` directly. No enum
/// dispatch, no future construction, no allocation. Zero overhead.
///
pub struct TwoSync<A, B> {
    pub first: A,
    pub second: B,
}

impl<A, B, Buf, L> WebRenderSync<L, Buf> for TwoSync<A, B>
where
    A: WebRenderSync<L, Buf>,
    B: WebRenderSync<L, Buf>,
    Buf: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, Buf>) -> Result<(), WriteError> {
        self.first.render(&mut *cx)?;
        self.second.render(&mut *cx)?;
        Ok(())
    }
}
