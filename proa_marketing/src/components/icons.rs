// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

#[derive(Debug, Clone, Copy)]
pub struct SearchIcon {
    pub size: u8,
    pub class: Option<&'static str>,
}

impl SearchIcon {
    pub const fn new(size: u8) -> Self {
        Self { size, class: None }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SearchIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                data-component="search-icon"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                width={self.size}
                height={self.size}
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class={self.class}
                aria-hidden="true"
            >
                <path d="m21 21-4.34-4.34" />
                <circle cx="11" cy="11" r="8" />
            </svg>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PanelLeftIcon {
    pub size: u8,
    pub class: Option<&'static str>,
}

impl PanelLeftIcon {
    pub const fn new(size: u8) -> Self {
        Self { size, class: None }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for PanelLeftIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                data-component="panel-left-icon"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                width={self.size}
                height={self.size}
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class={self.class}
                aria-hidden="true"
            >
                <rect width="18" height="18" x="3" y="3" rx="2" />
                <path d="M9 3v18" />
            </svg>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CopyIcon {
    pub size: u8,
    pub class: Option<&'static str>,
}

impl CopyIcon {
    pub const fn new(size: u8) -> Self {
        Self { size, class: None }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for CopyIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                data-component="copy-icon"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                width={self.size}
                height={self.size}
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class={self.class}
                aria-hidden="true"
            >
                <rect width="14" height="14" x="8" y="8" rx="2" />
                <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
            </svg>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExternalLinkIcon {
    pub size: u8,
    pub class: Option<&'static str>,
}

impl ExternalLinkIcon {
    pub const fn new(size: u8) -> Self {
        Self { size, class: None }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ExternalLinkIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                data-component="external-link-icon"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                width={self.size}
                height={self.size}
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class={self.class}
                aria-hidden="true"
            >
                <path d="M15 3h6v6" />
                <path d="M10 14 21 3" />
                <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            </svg>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{CopyIcon, ExternalLinkIcon, PanelLeftIcon, SearchIcon};
    use proa_core::{RenderBuffer, WebContext, WebRenderSync};
    use proa_macros::html_sync;

    #[test]
    fn icons_render_stable_hooks_sizes_and_optional_classes() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        html_sync! {
            {SearchIcon { size: 16, class: Some("search-trigger-icon") }}
            {PanelLeftIcon::new(19)}
            {CopyIcon::new(14)}
            {ExternalLinkIcon::new(14)}
        }
        .render(&mut cx)
        .unwrap();

        let html = output.into_string();
        assert_eq!(html.matches("<svg").count(), 4);
        assert!(html.contains(r#"data-component="search-icon""#));
        assert!(html.contains(r#"data-component="panel-left-icon""#));
        assert!(html.contains(r#"data-component="copy-icon""#));
        assert!(html.contains(r#"data-component="external-link-icon""#));
        assert!(html.contains(r#"width="16" height="16""#));
        assert!(html.contains(r#"class="search-trigger-icon""#));
        assert_eq!(html.matches(r#"aria-hidden="true""#).count(), 4);
    }
}
