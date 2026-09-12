// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

use crate::model::{DocPage, PageLink};

pub struct DocsPager<'a> {
    pub page: &'a DocPage,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for DocsPager<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <nav class="proa-docs-pager" aria-label="Docs pagination">
                {PagerLink {
                    link: self.page.prev,
                    class: "proa-docs-pager-prev",
                    label: "Previous",
                }}
                {PagerLink {
                    link: self.page.next,
                    class: "proa-docs-pager-next",
                    label: "Next",
                }}
            </nav>
        }
        .render(cx)
    }
}

struct PagerLink {
    link: Option<PageLink>,
    class: &'static str,
    label: &'static str,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for PagerLink {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if let Some(link) = self.link {
            html_sync! {
                <a class={self.class} href={link.path}>
                    <span>{self.label}</span>
                    <strong>{link.title}</strong>
                </a>
            }
            .render(cx)
        } else {
            Ok(())
        }
    }
}
