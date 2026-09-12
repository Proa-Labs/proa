// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Tooltip, TooltipContent, TooltipTrigger};
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Tooltip renders only its trigger content inline — the hover text is dropped.
impl<B: WriteBuf, Trigger: MdRenderSync<B>, Content> MdRenderSync<B> for Tooltip<Trigger, Content> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        let prev = cx.set_inline(true);
        self.trigger.render_md(cx)?;
        cx.set_inline(prev);
        Ok(())
    }
}

/// TooltipTrigger renders as just its label text — the button chrome is meaningless in markdown.
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for TooltipTrigger<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        let prev = cx.set_inline(true);
        self.children.render_md(cx)?;
        cx.set_inline(prev);
        Ok(())
    }
}

/// TooltipContent is hover-only supplementary text — renders nothing.
impl<B: WriteBuf, C> MdRenderSync<B> for TooltipContent<C> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
