// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Table component implementation
//!
//! Renders a responsive data table with header, body, footer, and caption sections.
use super::recipe::{TableRecipeProps, TABLE_RECIPE};
use super::TABLE_ANATOMY;
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Table component - a responsive data table wrapped in an overflow container
pub struct Table<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The table content
    pub children: Option<C>,
}
impl Table<()> {
    pub const fn new() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<C> Default for Table<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for Table<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = TABLE_RECIPE.resolve(TableRecipeProps);
        html_sync! {
            <div
                data-scope={TABLE_ANATOMY.scope()}
                data-part="container"
                data-slot="table-container"
                class={TABLE_RECIPE.container}
            >
                <table
                    data-table
                    data-scope={TABLE_ANATOMY.scope()}
                    data-part="root"
                    data-slot="table"
                    class={proa_macros::text!("{} {}", style, self.class)}
                >
                    {self.children}
                </table>
            </div>
        }
        .render(cx)
    }
}
/// TableHeader component - groups header rows
pub struct TableHeader<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The header content
    pub children: Option<C>,
}
impl<C> Default for TableHeader<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableHeader<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <thead
                data-table-header
                data-scope={TABLE_ANATOMY.scope()}
                data-part="header"
                data-slot="table-header"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.header, self.class)}
            >
                {self.children}
            </thead>
        }
        .render(cx)
    }
}
/// TableBody component - groups body rows
pub struct TableBody<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The body content
    pub children: Option<C>,
}
impl<C> Default for TableBody<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableBody<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <tbody
                data-table-body
                data-scope={TABLE_ANATOMY.scope()}
                data-part="body"
                data-slot="table-body"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.body, self.class)}
            >
                {self.children}
            </tbody>
        }
        .render(cx)
    }
}
/// TableFooter component - groups footer rows
pub struct TableFooter<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The footer content
    pub children: Option<C>,
}
impl<C> Default for TableFooter<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableFooter<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <tfoot
                data-table-footer
                data-scope={TABLE_ANATOMY.scope()}
                data-part="footer"
                data-slot="table-footer"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.footer, self.class)}
            >
                {self.children}
            </tfoot>
        }
        .render(cx)
    }
}
/// TableRow component - a single table row
pub struct TableRow<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The row content
    pub children: Option<C>,
}
impl<C> Default for TableRow<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableRow<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <tr
                data-table-row
                data-scope={TABLE_ANATOMY.scope()}
                data-part="row"
                data-slot="table-row"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.row, self.class)}
            >
                {self.children}
            </tr>
        }
        .render(cx)
    }
}
/// TableHead component - a header cell
pub struct TableHead<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The header cell content
    pub children: Option<C>,
}
impl<C> Default for TableHead<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableHead<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <th
                data-table-head
                data-scope={TABLE_ANATOMY.scope()}
                data-part="head"
                data-slot="table-head"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.head, self.class)}
            >
                {self.children}
            </th>
        }
        .render(cx)
    }
}
/// TableCell component - a data cell
pub struct TableCell<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The cell content
    pub children: Option<C>,
}
impl<C> Default for TableCell<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableCell<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <td
                data-table-cell
                data-scope={TABLE_ANATOMY.scope()}
                data-part="cell"
                data-slot="table-cell"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.cell, self.class)}
            >
                {self.children}
            </td>
        }
        .render(cx)
    }
}
/// TableCaption component - a table caption
pub struct TableCaption<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The caption content
    pub children: Option<C>,
}
impl<C> Default for TableCaption<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for TableCaption<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <caption
                data-table-caption
                data-scope={TABLE_ANATOMY.scope()}
                data-part="caption"
                data-slot="table-caption"
                class={proa_macros::text!("{} {}", TABLE_RECIPE.caption, self.class)}
            >
                {self.children}
            </caption>
        }
        .render(cx)
    }
}
