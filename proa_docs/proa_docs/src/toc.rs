// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{shared::attr_value::AttrValue, DataLoader, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

use crate::model::TocItem;

pub struct DocsToc<'a> {
    pub items: &'a [TocItem],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsToc<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.items.is_empty() {
            html_sync! {
                <nav>
                    <div class="proa-docs-toc-title">"On this page"</div>
                    <p class="proa-docs-toc-empty">"No sections"</p>
                </nav>
            }
            .render(cx)
        } else {
            html_sync! {
                <nav>
                    <div class="proa-docs-toc-title">"On this page"</div>
                    <ul>
                        {for item in self.items {
                            TocListItem { item }
                        }}
                    </ul>
                </nav>
            }
            .render(cx)
        }
    }
}

struct TocListItem<'a> {
    item: &'a TocItem,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TocListItem<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let class = toc_item_class(self.item.depth);
        html_sync! {
            <li class={class}>{TocLink { item: self.item }}</li>
        }
        .render(cx)
    }
}

struct TocLink<'a> {
    item: &'a TocItem,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TocLink<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let href = TocHref(self.item.id);
        let link = html_sync! {
            <a href={href}>{self.item.title}</a>
        };
        link.render(cx)
    }
}

fn toc_item_class(depth: u8) -> &'static str {
    match depth {
        1 => "proa-docs-toc-item proa-docs-toc-depth-1",
        2 => "proa-docs-toc-item proa-docs-toc-depth-2",
        3 => "proa-docs-toc-item proa-docs-toc-depth-3",
        4 => "proa-docs-toc-item proa-docs-toc-depth-4",
        5 => "proa-docs-toc-item proa-docs-toc-depth-5",
        _ => "proa-docs-toc-item proa-docs-toc-depth-6",
    }
}

struct TocHref<'a>(&'a str);

impl AttrValue for TocHref<'_> {
    fn should_render(&self) -> bool {
        true
    }

    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::WriteError> {
        "#".render_attr_value(cx)?;
        self.0.render_attr_value(cx)
    }
}
