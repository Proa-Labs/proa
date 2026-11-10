// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Button;
use crate::components::spinner::Spinner;
use proa::WebContext;
use proa::{MdRenderSync, WriteBuf, WriteError};

/// Button renders as just its label text — the visual chrome is meaningless in markdown.
impl<Loader: ::proa::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Button<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa::omit!(Spinner, "Loading spinners are visual interaction chrome.");
        proa::md_sync! { "{self.children}" }.render_md(cx)
    }
}
