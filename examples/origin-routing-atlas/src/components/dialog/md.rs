// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{
    Dialog, DialogButtonTrigger, DialogClose, DialogCloseButton, DialogContent, DialogDescription,
    DialogFooter, DialogHeader, DialogOverlay, DialogTitle, DialogTrigger,
};
use crate::components::_proa::md_support::block_boundary;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

/// Dialog renders its content as a plain block — the trigger is interaction chrome and is dropped.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, Trigger, Content: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Dialog<Trigger, Content>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.trigger);
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        self.content.render_md(cx)?;
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        Ok(())
    }
}

/// DialogTrigger is interaction chrome — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B> for DialogTrigger<C> {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.children);
        Ok(())
    }
}

/// DialogButtonTrigger is interaction chrome — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B>
    for DialogButtonTrigger<C>
{
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.children);
        Ok(())
    }
}

/// DialogOverlay is visual chrome — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for DialogOverlay {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

/// DialogContent renders its children as a plain block (close button and size are dropped).
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for DialogContent<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!("close-control");
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        self.children.render_md(cx)?;
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        Ok(())
    }
}

/// DialogHeader is a passthrough — its children (title, description) handle formatting.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for DialogHeader<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// DialogTitle renders as a bold line: `**title**`.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for DialogTitle<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        let prev = cx.md_set_inline(true);
        md_sync! { "**" { self.children } "**" }.render_md(cx)?;
        cx.md_set_inline(prev);
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        Ok(())
    }
}

/// DialogDescription renders as a plain paragraph.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for DialogDescription<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        self.children.render_md(cx)?;
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        Ok(())
    }
}

/// DialogFooter is a passthrough for its children (action buttons render as labels).
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for DialogFooter<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        self.children.render_md(cx)?;
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        Ok(())
    }
}

/// DialogClose is interaction chrome — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B> for DialogClose<C> {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.children);
        Ok(())
    }
}

/// DialogCloseButton is interaction chrome — renders nothing.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C> MdRenderSync<Loader, B>
    for DialogCloseButton<C>
{
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(self.children);
        Ok(())
    }
}
