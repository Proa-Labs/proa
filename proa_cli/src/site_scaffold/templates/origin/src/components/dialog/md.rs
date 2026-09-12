// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{
    Dialog, DialogButtonTrigger, DialogClose, DialogCloseButton, DialogContent, DialogDescription,
    DialogFooter, DialogHeader, DialogOverlay, DialogTitle, DialogTrigger,
};
use crate::components::_proa::md_support::block_boundary;
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

/// Dialog renders its content as a plain block — the trigger is interaction chrome and is dropped.
impl<B: WriteBuf, Trigger, Content: MdRenderSync<B>> MdRenderSync<B> for Dialog<Trigger, Content> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        self.content.render_md(cx)?;
        block_boundary(cx)?;
        Ok(())
    }
}

/// DialogTrigger is interaction chrome — renders nothing.
impl<B: WriteBuf, C> MdRenderSync<B> for DialogTrigger<C> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// DialogButtonTrigger is interaction chrome — renders nothing.
impl<B: WriteBuf, C> MdRenderSync<B> for DialogButtonTrigger<C> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// DialogOverlay is visual chrome — renders nothing.
impl<B: WriteBuf> MdRenderSync<B> for DialogOverlay {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// DialogContent renders its children as a plain block (close button and size are dropped).
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for DialogContent<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        self.children.render_md(cx)?;
        block_boundary(cx)?;
        Ok(())
    }
}

/// DialogHeader is a passthrough — its children (title, description) handle formatting.
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for DialogHeader<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// DialogTitle renders as a bold line: `**title**`.
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for DialogTitle<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        let prev = cx.set_inline(true);
        md_sync! { "**" { self.children } "**" }.render_md(cx)?;
        cx.set_inline(prev);
        block_boundary(cx)?;
        Ok(())
    }
}

/// DialogDescription renders as a plain paragraph.
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for DialogDescription<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        self.children.render_md(cx)?;
        block_boundary(cx)?;
        Ok(())
    }
}

/// DialogFooter is a passthrough for its children (action buttons render as labels).
impl<B: WriteBuf, C: MdRenderSync<B>> MdRenderSync<B> for DialogFooter<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        self.children.render_md(cx)?;
        block_boundary(cx)?;
        Ok(())
    }
}

/// DialogClose is interaction chrome — renders nothing.
impl<B: WriteBuf, C> MdRenderSync<B> for DialogClose<C> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// DialogCloseButton is interaction chrome — renders nothing.
impl<B: WriteBuf, C> MdRenderSync<B> for DialogCloseButton<C> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
