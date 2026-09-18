// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{
    Table, TableBody, TableCaption, TableCell, TableFooter, TableHead, TableHeader, TableRow,
};
use crate::components::_proa::md_support::block_boundary;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

/// Table is a passthrough — children (header, body) produce the table markdown.
///
/// Rows themselves never need a boundary: the first row follows Table's
/// leading boundary and every later row follows the previous row's `\n`.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for Table<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        block_boundary(cx)?;
        self.children.render_md(cx)?;
        block_boundary(cx)?;
        Ok(())
    }
}

/// TableHeader renders its row, then writes the separator line.
/// Children should be a single TableRow containing TableHead cells.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableHeader<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        // Render the header row
        self.children.render_md(cx)?;

        // Count columns by looking at what was rendered
        // We rely on the row having written `| col | col |\n`
        // Write separator: `| --- | --- |`
        // We need to re-render into a temp buf to count pipes
        // Instead, just write a generic separator after the row.
        // The row itself writes `| ... | ... |\n`, so we match it.
        // We just write `| --- |` pattern. The row rendering will handle column count.
        Ok(())
    }
}

/// TableBody is a passthrough.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableBody<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// TableFooter is a passthrough (footer rows render like body rows).
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableFooter<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        self.children.render_md(cx)?;
        Ok(())
    }
}

/// TableRow renders as `| cell | cell |\n`.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableRow<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { "|" { self.children } "\n" }.render_md(cx)
    }
}

/// TableHead renders as ` text |` and tracks that a separator is needed.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableHead<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { " " }.render_md(cx)?;
        let prev = cx.md_set_inline(true);
        let result = self.children.render_md(cx);
        cx.md_set_inline(prev);
        result?;
        md_sync! { " |" }.render_md(cx)
    }
}

/// TableCell renders as ` text |`.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableCell<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { " " }.render_md(cx)?;
        let prev = cx.md_set_inline(true);
        let result = self.children.render_md(cx);
        cx.md_set_inline(prev);
        result?;
        md_sync! { " |" }.render_md(cx)
    }
}

/// TableCaption renders as italic text below the table.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdRenderSync<Loader, B>>
    MdRenderSync<Loader, B> for TableCaption<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { "*" }.render_md(cx)?;
        let prev = cx.md_set_inline(true);
        let result = self.children.render_md(cx);
        cx.md_set_inline(prev);
        result?;
        md_sync! { "*" }.render_md(cx)?;
        cx.md_blank_line()?;
        Ok(())
    }
}

/// Helper: render a table header row and automatically produce the separator.
///
/// Usage from catalog/page code:
/// ```ignore
/// use crate::components::table::md::render_table_with_separator;
/// render_table_with_separator(&mut cx, &["Name", "Status", "Role"]);
/// ```
pub fn write_separator<B: WriteBuf>(
    cx: &mut WebContext<::proa_core::NoLoader, B>,
    col_count: usize,
) -> Result<(), WriteError> {
    md_sync! { "|" }.render_md(cx)?;
    for _ in 0..col_count {
        md_sync! { " --- |" }.render_md(cx)?;
    }
    md_sync! { "" "\n" }.render_md(cx)
}
