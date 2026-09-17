// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

use crate::{
    model::{DocPage, Docs},
    navigation::DocsNav,
    pager::DocsPager,
    sections::DocsSectionMenu,
    toc::DocsToc,
};

pub struct DocsShell<'a> {
    pub docs: &'a Docs,
    pub page: &'a DocPage,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsShell<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>
                        {proa_core::raw_text_ok(self.page.title)}
                        " - "
                        {proa_core::raw_text_ok(self.docs.config.site_name)}
                    </title>
                    {MetaDescription { description: self.page.description }}
                </head>
                <body class="proa-docs-body">
                    <div class="proa-docs-layout" data-proa-docs>
                        <aside class="proa-docs-sidebar" aria-label="Docs navigation">
                            <a class="proa-docs-brand" href={self.docs.config.base_path}>
                                {self.docs.config.site_name}
                            </a>
                            {DocsSectionMenu {
                                docs: self.docs,
                                active_path: self.page.route_path,
                            }}
                            {DocsNav {
                                items: self.docs.nav_items_for_path(self.page.route_path),
                                active_path: self.page.route_path,
                            }}
                        </aside>
                        <main class="proa-docs-main">
                            <article class="proa-docs-article">
                                {StaticRaw(self.page.article_html)}
                            </article>
                            {DocsPager { page: self.page }}
                        </main>
                        <aside class="proa-docs-toc" aria-label="On this page">
                            {DocsToc { items: self.page.toc }}
                        </aside>
                    </div>
                </body>
            </html>
        }
        .render(cx)
    }
}

struct MetaDescription {
    description: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MetaDescription {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if let Some(description) = self.description {
            let meta = html_sync! {
                <meta name="description" content={description} />
            };
            meta.render(cx)
        } else {
            Ok(())
        }
    }
}
