// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

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

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for DocsSidebar {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="docs-sidebar" class="docs-sidebar-inner">
                <div class="docs-sidebar-header">
                    <a href="/" class="docs-sidebar-brand" aria-label="Proa home">
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
                    <a href="/" class="docs-sidebar-footer-link">"Back to site"</a>
                    {ThemeToggle::new()}
                </div>
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{RenderBuffer, WebContext};

    fn render_sidebar(slug: &str) -> String {
        let docs = crate::pages::docs::docs();
        let page = docs.find_page(slug).expect("docs page");
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        DocsSidebar { docs, page }.render(&mut cx).unwrap();
        output.into_string()
    }

    #[test]
    fn default_sidebar_shows_base_separators_and_hides_root_contents() {
        let html = render_sidebar("core/html-rendering");
        let nav = html
            .split_once(r#"<div class="docs-sidebar-content">"#)
            .expect("sidebar content")
            .1;

        // Base-sidebar separators render.
        assert!(nav.contains("Start here"));
        assert!(nav.contains("Rendering"));
        assert!(nav.contains("Interactivity (RSJS)"));
        assert!(nav.contains("Security"));
        assert!(nav.contains("Deploy"));
        assert!(nav.contains(r#"data-nav-kind="separator""#));

        // Worlds stay out of the base sidebar and appear only in the switcher.
        assert!(!nav.contains(r#"data-nav-kind="root""#));
        assert!(!nav.contains(r#"href="/docs/cli/new-site""#));
        assert!(!nav.contains(r#"href="/docs/migration/react""#));
        assert!(html.contains(r#"data-nav-root="CLI""#));
        assert!(html.contains(r#"data-nav-root="Migration""#));
        assert!(html.contains(r#"data-nav-root="FFI""#));
        assert!(html.contains(r#"data-nav-root="Proa Docs""#));

        // RSJS is now in the base sidebar, not a world.
        assert!(nav.contains(r#"href="/docs/rsjs/signals""#));
        assert!(!html.contains(r#"data-nav-root="RSJS""#));

        assert!(nav.contains(r#"href="/docs/core/html-rendering" data-active="true""#));
    }

    #[test]
    fn root_sidebar_uses_only_the_active_meta_folder_pages() {
        let html = render_sidebar("cli/new-site");
        let nav = html
            .split_once(r#"<div class="docs-sidebar-content">"#)
            .expect("sidebar content")
            .1;

        assert!(html.contains(r#"data-active="true" data-nav-root="CLI""#));
        assert!(nav.contains(r#"href="/docs/cli/new-site" data-active="true""#));
        assert!(!nav.contains("Rendering"));
        assert!(!nav.contains(r#"href="/docs/core/html-rendering""#));
    }

    /// Every separator must label at least one page. A separator whose only
    /// member was a root section renders as a dead heading -- the bug that
    /// left `---Interactive UI---` pointing at nothing.
    #[test]
    fn every_separator_renders_an_icon() {
        // Separators are the only nav rows with icons now, so a missing one
        // leaves a visual gap in the 12-group sidebar. Page rows stay
        // icon-free on purpose: mixed rows made label left-edges zigzag.
        let html = render_sidebar("core/html-rendering");
        let nav = html
            .split_once(r#"<div class="docs-sidebar-content">"#)
            .expect("sidebar content")
            .1;

        let separators = nav.matches(r#"data-nav-kind="separator""#).count();
        assert_eq!(
            separators, 12,
            "expected 12 base separators, got {separators}"
        );

        for chunk in nav.split(r#"<li data-nav-kind="separator""#).skip(1) {
            let row = chunk.split_once("</li>").map_or(chunk, |(row, _)| row);
            assert!(
                row.contains("proa-docs-nav-icon"),
                "a separator rendered without an icon: {row}"
            );
        }
    }

    #[test]
    fn page_rows_carry_no_icon() {
        // §3.2: mixed icon/no-icon rows zigzag because .proa-docs-nav-icon
        // reserves no space when absent.
        let html = render_sidebar("core/html-rendering");
        let nav = html
            .split_once(r#"<div class="docs-sidebar-content">"#)
            .expect("sidebar content")
            .1;

        // Bound each row at its own </li>; a wider window spills into the
        // next row, which may legitimately be an icon-bearing separator.
        for chunk in nav.split(r#"<li data-nav-kind="page""#).skip(1) {
            let row = chunk.split_once("</li>").map_or(chunk, |(row, _)| row);
            assert!(
                !row.contains("proa-docs-nav-icon"),
                "a page row rendered an icon: {row}"
            );
        }
    }

    #[test]
    fn no_separator_is_empty() {
        let html = render_sidebar("core/html-rendering");
        let nav = html
            .split_once(r#"<div class="docs-sidebar-content">"#)
            .expect("sidebar content")
            .1;

        let mut pending: Option<usize> = None;
        for (idx, chunk) in nav.split(r#"data-nav-kind=""#).enumerate().skip(1) {
            let kind = chunk.split('"').next().unwrap_or_default();
            match kind {
                "separator" => {
                    assert!(
                        pending.is_none(),
                        "separator #{} has no pages under it",
                        pending.unwrap_or(idx)
                    );
                    pending = Some(idx);
                }
                "page" | "group" => pending = None,
                _ => {}
            }
        }
        assert!(
            pending.is_none(),
            "the last separator has no pages under it"
        );
    }
}
