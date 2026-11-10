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
        html_sync! {
            <div data-component="docs-sidebar" class="docs-sidebar-inner">
                <div class="docs-sidebar-header">
                    <a href={routes::HOME} class="docs-sidebar-brand" aria-label="Proa home">
                        <span class="docs-sidebar-mark" aria-hidden="true">
                            {ProaLogoMark { class: "docs-sidebar-logo-svg" }}
                        </span>
                        <span class="docs-sidebar-brand-copy">
                            <strong>{self.docs.config.site_name}</strong>
                        </span>
                    </a>
                    {SearchTrigger {
                        class: Some("docs-sidebar-search"),
                    }}
                    {DocsSectionMenu {
                        docs: self.docs,
                        active_path: self.page.route_path,
                    }}
                </div>
                <div class="docs-sidebar-content">
                    {DocsNav {
                        items: self.docs.nav_items_for_path(self.page.route_path),
                        active_path: self.page.route_path,
                    }}
                </div>
                <div class="docs-sidebar-footer">
                    <a href={routes::HOME} class="docs-sidebar-footer-link">"Back to site"</a>
                    {ThemeToggle::new()}
                </div>
            </div>
        }
        .render(cx)
    }
}
