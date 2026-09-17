// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_docs::{DocPage, Docs, DocsPager, DocsToc};
use proa_macros::html_sync;

use crate::components::{
    brand::ProaLogoMark,
    icons::PanelLeftIcon,
    landing_shell::{MarketingNav, MarketingNavActive, MarketingNavVariant},
    search_trigger::SearchIconButton,
    theme_toggle::ThemeToggle,
};

use super::{
    article::{DocsArticleContent, DocsPageDescription},
    sidebar::DocsSidebar,
};

pub(crate) struct DocsPageView {
    pub(crate) docs: &'static Docs,
    pub(crate) page: &'static DocPage,
    pub(crate) label: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsPageView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <input
                type="checkbox"
                id="docs-mobile-nav-toggle"
                class="docs-mobile-nav-state"
                aria-hidden="true"
            />
            <header class="docs-mobile-topbar" aria-label="Docs mobile header">
                <a class="docs-mobile-logo" href="/" aria-label="Proa home">
                    <span aria-hidden="true">
                        {ProaLogoMark { class: "docs-sidebar-logo-svg" }}
                    </span>
                </a>
                <div class="docs-mobile-actions">
                    {SearchIconButton::new("Search docs").with_class("docs-mobile-icon-button")}
                    {ThemeToggle {
                        class: Some("docs-mobile-icon-button"),
                    }}
                    <label
                        for="docs-mobile-nav-toggle"
                        class="docs-mobile-icon-button docs-mobile-nav-button"
                        aria-label="Open docs navigation"
                    >
                        {PanelLeftIcon::new(19)}
                    </label>
                </div>
            </header>
            <div class="docs-mobile-drawer">
                <label
                    for="docs-mobile-nav-toggle"
                    class="docs-mobile-backdrop"
                    aria-label="Close docs navigation"
                ></label>
                <aside class="docs-mobile-drawer-panel docs-sidebar" aria-label="Docs navigation">
                    {DocsSidebar {
                        docs: self.docs,
                        page: self.page,
                    }}
                </aside>
            </div>
            <button type="button" class="docs-mobile-tocbar" aria-label="Current docs section">
                <span class="docs-mobile-progress" aria-hidden="true"></span>
                <span>{self.page.title}</span>
                <span class="docs-mobile-chevron" aria-hidden="true">"v"</span>
            </button>
            <main
                id="main"
                class="docs-layout"
                data-component="docs-page"
                data-docs-collection={self.label}
                tabindex="-1"
            >
                <aside class="docs-sidebar" aria-label="Docs sidebar">
                    {DocsSidebar {
                        docs: self.docs,
                        page: self.page,
                    }}
                </aside>
                {DocsHeader { active_path: self.page.route_path }}
                <article class="docs-article prose" data-full="false">
                    <p class="docs-kicker">{self.label}</p>
                    <h1 class="docs-page-title">{self.page.title}</h1>
                    {DocsPageDescription { description: self.page.description }}
                    {DocsArticleContent { page: self.page }}
                    <div class="not-prose">{DocsPager { page: self.page }}</div>
                </article>
                <aside class="docs-toc" aria-label="On this page">
                    <div class="docs-toc-title">"On this page"</div>
                    <div class="docs-toc-nav">{DocsToc { items: self.page.toc }}</div>
                </aside>
            </main>
        }
        .render(cx)
    }
}

struct DocsHeader {
    active_path: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsHeader {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <header data-component="docs-header" class="docs-header" aria-label="Site navigation">
                <div class="docs-header-inner">
                    {MarketingNav {
                        variant: MarketingNavVariant::Docs,
                        active: MarketingNavActive::for_path(self.active_path),
                    }}
                </div>
            </header>
        }
        .render(cx)
    }
}
