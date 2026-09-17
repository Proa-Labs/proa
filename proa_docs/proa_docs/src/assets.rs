// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Default stylesheet and script for the components `proa_docs_build` emits.
//!
//! The build crate lowers authored components into HTML carrying stable class
//! and `data-` hooks. This module ships the CSS and JavaScript that make those
//! hooks behave, so a docs site works on a fresh install.
//!
//! Three ways to use them, in increasing order of control:
//!
//! 1. Render [`DocsAssets`] into `<head>`. Inlines both, no extra requests.
//! 2. Serve [`DEFAULT_STYLESHEET`] and [`DEFAULT_SCRIPT`] as static files and
//!    link them yourself, so they cache across pages.
//! 3. Ship neither, and write your own CSS against the documented markup
//!    contract. Every hook is stable and versioned.
//!
//! Theming does not require option 3. The stylesheet drives every colour from
//! 15 custom properties on `.proa-docs-body`; override them and the whole theme
//! follows.

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

/// Default theme for every component the build crate emits.
///
/// Plain CSS. No Tailwind, no `@apply`, no build step.
pub const DEFAULT_STYLESHEET: &str = include_str!("../assets/docs.css");

/// Default behaviour for the interactive components.
///
/// Wires code-block copy buttons and page-Markdown copy buttons. Tabs, the
/// code explorer, and accordions are CSS-only and need nothing here.
pub const DEFAULT_SCRIPT: &str = include_str!("../assets/docs.js");

/// Inline the default stylesheet and script into `<head>`.
///
/// ```ignore
/// html_sync! {
///     <head>
///         {DocsAssets::new()}
///     </head>
/// }
/// ```
///
/// Prefer serving the constants as static files on a site with many pages:
/// inlining costs ~35 KB per response and cannot be cached between navigations.
#[derive(Debug, Clone, Copy, Default)]
pub struct DocsAssets {
    style: bool,
    script: bool,
}

impl DocsAssets {
    /// Both the stylesheet and the script.
    pub const fn new() -> Self {
        Self {
            style: true,
            script: true,
        }
    }

    /// Stylesheet only. Use when you serve the script separately.
    pub const fn style_only() -> Self {
        Self {
            style: true,
            script: false,
        }
    }

    /// Script only. Use when you ship your own theme.
    pub const fn script_only() -> Self {
        Self {
            style: false,
            script: true,
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsAssets {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.style {
            html_sync! {
                <style data-proa-docs-style>{StaticRaw(DEFAULT_STYLESHEET)}</style>
            }
            .render(cx)?;
        }
        if self.script {
            html_sync! {
                <script data-proa-docs-script>{StaticRaw(DEFAULT_SCRIPT)}</script>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_defines_the_themeable_properties() {
        assert!(DEFAULT_STYLESHEET.contains(".proa-docs-body {"));
        assert!(DEFAULT_STYLESHEET.contains("--docs-fg:"));
        assert!(DEFAULT_STYLESHEET.contains(".dark .proa-docs-body {"));
    }

    #[test]
    fn stylesheet_covers_every_component() {
        for hook in [
            "proa-docs-callout",
            "proa-docs-steps",
            "proa-docs-accordion",
            "proa-docs-explorer",
            "proa-docs-demo",
            "data-code-block",
            "proa-docs-tabs-panel",
            "proa-docs-filetree",
            "data-highlighted-line",
        ] {
            assert!(
                DEFAULT_STYLESHEET.contains(hook),
                "default stylesheet is missing `{hook}`"
            );
        }
    }

    /// The stylesheet was extracted from the marketing app, whose shell uses
    /// `.docs-body` / `.docs-article.prose`. `DocsShell` emits
    /// `.proa-docs-body` / `.proa-docs-article`, so an app-scoped selector
    /// here is a rule that silently never matches for a consumer.
    #[test]
    fn stylesheet_only_targets_markup_this_crate_emits() {
        for app_class in [
            ".docs-body",
            ".docs-article",
            ".docs-layout",
            ".docs-sidebar",
            ".docs-toc",
            ".docs-mobile",
            ".prose",
        ] {
            assert!(
                !DEFAULT_STYLESHEET.contains(app_class),
                "default stylesheet targets app-owned `{app_class}`; \
                 scope it to the classes `DocsShell` emits instead"
            );
        }
        // The shell's own structural classes must be styled.
        for shell_class in [
            ".proa-docs-body",
            ".proa-docs-layout",
            ".proa-docs-sidebar",
            ".proa-docs-main",
            ".proa-docs-article",
            ".proa-docs-toc",
            ".proa-docs-pager",
            ".proa-docs-nav",
        ] {
            assert!(
                DEFAULT_STYLESHEET.contains(shell_class),
                "default stylesheet never styles `{shell_class}`"
            );
        }
    }

    #[test]
    fn script_only_targets_markup_this_crate_emits() {
        assert!(DEFAULT_SCRIPT.contains("data-code-copy"));
        assert!(DEFAULT_SCRIPT.contains("data-code-block"));
        assert!(DEFAULT_SCRIPT.contains("data-docs-markdown-src"));
        // App-owned classes must not leak into a shipped asset.
        assert!(!DEFAULT_SCRIPT.contains(".docs-sidebar-content"));
        assert!(!DEFAULT_SCRIPT.contains(".docs-article"));
    }
}
