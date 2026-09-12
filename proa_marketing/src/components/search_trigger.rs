// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{ClassList, DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use super::icons::SearchIcon;

const SEARCH_TRIGGER_CLASS: &str = "search-trigger";

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchTrigger {
    pub class: Option<&'static str>,
}

impl SearchTrigger {
    pub const fn new() -> Self {
        Self { class: None }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SearchTrigger {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new()
            .add(SEARCH_TRIGGER_CLASS)
            .add_opt(self.class);

        html_sync! {
            <button
                type="button"
                class={classes}
                data-component="search-trigger"
                data-search-open="true"
                aria-label="Search Proa (Cmd+K)"
                aria-haspopup="dialog"
            >
                {SearchIcon {
                    size: 16,
                    class: Some("search-trigger-icon"),
                }}
                <span class="search-trigger-label">"Search"</span>
                <span class="search-trigger-kbds">
                    <kbd>"⌘"</kbd>
                    <kbd>"K"</kbd>
                </span>
            </button>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchIconButton {
    pub aria_label: &'static str,
    pub class: Option<&'static str>,
}

impl SearchIconButton {
    pub const fn new(aria_label: &'static str) -> Self {
        Self {
            aria_label,
            class: None,
        }
    }

    pub const fn with_class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SearchIconButton {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <button
                type="button"
                class={self.class}
                data-component="search-icon-button"
                data-search-open="true"
                aria-label={self.aria_label}
                aria-haspopup="dialog"
            >
                {SearchIcon::new(19)}
            </button>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchIconButton, SearchTrigger};
    use proa_core::{RenderBuffer, WebContext, WebRenderSync};

    #[test]
    fn full_trigger_renders_behavior_keyboard_and_custom_class_contracts() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        SearchTrigger {
            class: Some("docs-sidebar-search"),
        }
        .render(&mut cx)
        .unwrap();

        let html = output.into_string();
        assert!(html.contains(r#"data-component="search-trigger""#));
        assert!(html.contains(r#"data-search-open="true""#));
        assert!(html.contains(r#"aria-haspopup="dialog""#));
        assert!(html.contains("search-trigger docs-sidebar-search"));
        assert_eq!(html.matches("<kbd>").count(), 2);
    }

    #[test]
    fn icon_button_renders_caller_accessibility_and_class_contracts() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        SearchIconButton {
            aria_label: "Search docs",
            class: Some("docs-mobile-icon-button"),
        }
        .render(&mut cx)
        .unwrap();

        let html = output.into_string();
        assert!(html.contains(r#"data-component="search-icon-button""#));
        assert!(html.contains(r#"aria-label="Search docs""#));
        assert!(html.contains(r#"class="docs-mobile-icon-button""#));
        assert_eq!(html.matches("<svg").count(), 1);
    }
}
