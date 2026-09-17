// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle};
use crate::components::_proa::md_support::block_boundary;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Card pushes heading offset so nested cards get deeper headings.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Card<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        let prev = cx.md_push_heading_offset(1);
        self.children.render_md(cx)?;
        cx.md_pop_heading_offset(prev);
        block_boundary(cx)?;
        Ok(())
    }
}

/// CardHeader is a passthrough — its children (title, description) handle formatting.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardHeader<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// CardTitle renders as a heading at the current level.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardTitle<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        // `heading` adds the current offset itself, so the logical level is
        // constant: a top-level Card title is `##`, one nesting deeper `###`.
        cx.md_heading_with(1, |cx| self.children.render_md(cx))
    }
}

/// CardDescription renders as paragraph text.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardDescription<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

/// CardContent is a passthrough.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardContent<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

/// CardFooter is a passthrough.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for CardFooter<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}
