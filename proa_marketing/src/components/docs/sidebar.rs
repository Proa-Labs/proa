// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_docs::{DocPage, Docs, DocsNav, DocsSectionMenu};
use proa_macros::html_sync;

use crate::components::{
    brand::ProaLogoMark, search_trigger::SearchTrigger, theme_toggle::ThemeToggle,
};

pub(super) struct DocsSidebar {
    pub(super) docs: &'static Docs,
    pub(super) page: &'static DocPage,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsSidebar {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        ReadingSidebar {
            title: self.docs.config.site_name,
            section: Some(DocsSectionMenu {
                docs: self.docs,
                active_path: self.page.route_path,
            }),
            content: DocsNav {
                items: self.docs.nav_items_for_path(self.page.route_path),
                active_path: self.page.route_path,
            },
        }
        .render(cx)
    }
}

pub(crate) struct ReadingSidebar<C, S = ()> {
    pub(crate) title: &'static str,
    pub(crate) section: Option<S>,
    pub(crate) content: C,
}

impl<L: DataLoader, B: WriteBuf, C: WebRenderSync<L, B>, S: WebRenderSync<L, B>> WebRenderSync<L, B>
    for ReadingSidebar<C, S>
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="docs-sidebar" class="docs-sidebar-inner">
                <div class="docs-sidebar-header">
                    <a href={routes::HOME} class="docs-sidebar-brand" aria-label="Proa home">
                        <span class="docs-sidebar-mark" aria-hidden="true">
                            {ProaLogoMark { class: "docs-sidebar-logo-svg" }}
                        </span>
                        <span class="docs-sidebar-brand-copy">
                            <strong>{self.title}</strong>
                        </span>
                    </a>
                    {SearchTrigger {
                        class: Some("docs-sidebar-search"),
                    }}
                    {self.section}
                </div>
                <div class="docs-sidebar-content">{self.content}</div>
                {ReadingSidebarFooter}
            </div>
        }
        .render(cx)
    }
}

pub(crate) struct ReadingSidebarFooter;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ReadingSidebarFooter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="docs-sidebar-footer" data-component="reading-sidebar-footer">
                <a href={routes::HOME} class="docs-sidebar-footer-link">"Back to site"</a>
                {ThemeToggle::new()}
            </div>
        }
        .render(cx)
    }
}
