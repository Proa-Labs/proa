// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

#[derive(Debug, Clone, Copy)]
pub struct TypeTable<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub rows: &'a [TypeTableRow<'a>],
}

#[derive(Debug, Clone, Copy)]
pub struct TypeTableRow<'a> {
    pub name: &'a str,
    pub type_repr: &'a str,
    pub description: Option<&'a str>,
    pub default: Option<&'a str>,
    pub required: bool,
    pub deprecated: bool,
    pub href: Option<&'a str>,
    pub parameters: &'a [TypeTableParam<'a>],
    pub returns: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct TypeTableParam<'a> {
    pub name: &'a str,
    pub type_repr: &'a str,
    pub description: Option<&'a str>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TypeTable<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <div id={self.id} class="proa-docs-type-table" data-proa-docs-type-table>
                <div class="proa-docs-type-table-title">{self.name}</div>
                <table>
                    <thead>
                        <tr>
                            <th>"Prop"</th>
                            <th>"Type"</th>
                            <th>"Description"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {for row in self.rows {
                            TypeTableRowView { row }.render(cx)?;
                        }}
                    </tbody>
                </table>
            </div>
        }
        .render(cx)
    }
}

struct TypeTableRowView<'a> {
    row: &'a TypeTableRow<'a>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TypeTableRowView<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <tr>
                <td>
                    <code>{self.row.name}</code>
                    {TypeBadge {
                        class: "proa-docs-type-required",
                        label: "required",
                        show: self.row.required,
                    }}
                    {TypeBadge {
                        class: "proa-docs-type-deprecated",
                        label: "deprecated",
                        show: self.row.deprecated,
                    }}
                </td>
                <td>{TypeRepr { row: self.row }}</td>
                <td>
                    {OptionalSpan { value: self.row.description, class: None, prefix: None }}
                    {OptionalSpan {
                        value: self.row.default,
                        class: Some("proa-docs-type-default"),
                        prefix: Some("Default: "),
                    }}
                    {OptionalSpan {
                        value: self.row.returns,
                        class: Some("proa-docs-type-returns"),
                        prefix: Some("Returns: "),
                    }}
                </td>
            </tr>
        }
        .render(cx)
    }
}

struct TypeBadge<'a> {
    class: &'static str,
    label: &'a str,
    show: bool,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TypeBadge<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.show {
            let badge = html_sync! {
                <span class={self.class}>{self.label}</span>
            };
            badge.render(cx)
        } else {
            Ok(())
        }
    }
}

struct TypeRepr<'a> {
    row: &'a TypeTableRow<'a>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TypeRepr<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if let Some(href) = self.row.href {
            let type_repr = html_sync! {
                <a href={href}>
                    <code>{self.row.type_repr}</code>
                </a>
            };
            type_repr.render(cx)
        } else {
            let type_repr = html_sync! {
                <code>{self.row.type_repr}</code>
            };
            type_repr.render(cx)
        }
    }
}

struct OptionalSpan<'a> {
    value: Option<&'a str>,
    class: Option<&'static str>,
    prefix: Option<&'static str>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for OptionalSpan<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let Some(value) = self.value else {
            return Ok(());
        };
        let class = self.class;
        let prefix = self.prefix.unwrap_or("");
        html_sync! {
            <span class={class}>
                {prefix}
                <code>{value}</code>
            </span>
        }
        .render(cx)
    }
}
