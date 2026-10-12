// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Tooltip, TooltipContent, TooltipTrigger};
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Tooltip renders only its trigger content inline — the hover text is dropped.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, Trigger: MdRenderSync<Loader, B>, Content>
    MdRenderSync<Loader, B> for Tooltip<Trigger, Content>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.content);
        let prev = cx.md_set_inline(true);
        self.trigger.render_md(cx)?;
        cx.md_set_inline(prev);
        Ok(())
    }
}

/// TooltipTrigger renders as just its label text — the button chrome is meaningless in markdown.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TooltipTrigger<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        let prev = cx.md_set_inline(true);
        self.children.render_md(cx)?;
        cx.md_set_inline(prev);
        Ok(())
    }
}

/// TooltipContent is hover-only supplementary text — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B>
    for TooltipContent<C>
{
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.children);
        Ok(())
    }
}
