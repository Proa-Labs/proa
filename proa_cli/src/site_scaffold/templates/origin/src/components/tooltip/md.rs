// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Tooltip, TooltipContent, TooltipTrigger};
use proa::WebContext;
use proa::{MdRenderSync, WriteBuf, WriteError};

/// Tooltip renders only its trigger content inline — the hover text is dropped.
impl<Loader: ::proa::DataLoader, B: WriteBuf, Trigger: MdRenderSync<Loader, B>, Content>
    MdRenderSync<Loader, B> for Tooltip<Trigger, Content>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa::omit!(self.content);
        proa::md_sync! { "{self.trigger}" }.render_md(cx)
    }
}

/// TooltipTrigger renders as just its label text — the button chrome is meaningless in markdown.
impl<Loader: ::proa::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TooltipTrigger<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa::md_sync! { "{self.children}" }.render_md(cx)
    }
}

/// TooltipContent is hover-only supplementary text — renders nothing.
impl<Loader: ::proa::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B>
    for TooltipContent<C>
{
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa::omit!(self.children);
        Ok(())
    }
}
