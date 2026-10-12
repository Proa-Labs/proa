// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Button;
use crate::components::spinner::Spinner;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Button renders as just its label text — the visual chrome is meaningless in markdown.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Button<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(Spinner, "Loading spinners are visual interaction chrome.");
        let prev = cx.md_set_inline(true);
        self.children.render_md(cx)?;
        cx.md_set_inline(prev);
        Ok(())
    }
}
