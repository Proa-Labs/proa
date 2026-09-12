// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Button;
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Button renders as just its label text — the visual chrome is meaningless in markdown.
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for Button<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        let prev = cx.set_inline(true);
        self.children.render_md(cx)?;
        cx.set_inline(prev);
        Ok(())
    }
}
