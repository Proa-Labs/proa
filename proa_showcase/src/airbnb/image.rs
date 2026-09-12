// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Lightweight image helper for the Airbnb showcase.
//!
//! Provides native lazy loading, async decoding, and a fallback source for
//! broken remote images without touching the broader design-system surface.
use proa_core::{SafeAttr, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
const DEFAULT_FALLBACK_SRC: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1200 900'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop stop-color='%23f6f6f6'/%3E%3Cstop offset='1' stop-color='%23ececec'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='1200' height='900' fill='url(%23g)'/%3E%3Ccircle cx='910' cy='220' r='160' fill='%23ffffff' fill-opacity='.58'/%3E%3Ccircle cx='280' cy='640' r='220' fill='%23ffffff' fill-opacity='.44'/%3E%3Cpath d='M0 685c113-43 217-70 312-81 168-20 283 16 400 1 95-12 201-51 488-177v472H0z' fill='%23e3e3e3'/%3E%3Cpath d='M0 585c154-68 301-97 430-83 130 15 224 57 331 54 92-2 211-39 439-150v494H0z' fill='%23d8d8d8'/%3E%3Ctext x='50%25' y='50%25' text-anchor='middle' dominant-baseline='middle' fill='%23717171' font-family='system-ui,-apple-system,sans-serif' font-size='42'%3EImage unavailable%3C/text%3E%3C/svg%3E";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AirImageLoading {
    Eager,
    #[default]
    Lazy,
}
impl AirImageLoading {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eager => "eager",
            Self::Lazy => "lazy",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AirImageFetchPriority {
    High,
    #[default]
    Auto,
    Low,
}
impl AirImageFetchPriority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Auto => "auto",
            Self::Low => "low",
        }
    }
}
pub struct AirImage<'a> {
    pub id: Option<&'a str>,
    pub src: &'a str,
    pub alt: &'a str,
    pub class: Option<&'a str>,
    pub loading: AirImageLoading,
    pub fetch_priority: AirImageFetchPriority,
    pub fallback_src: Option<&'static str>,
}
impl<'a> AirImage<'a> {
    pub const fn new(src: &'a str, alt: &'a str) -> Self {
        Self {
            id: None,
            src,
            alt,
            class: None,
            loading: AirImageLoading::Lazy,
            fetch_priority: AirImageFetchPriority::Auto,
            fallback_src: None,
        }
    }
}
impl<'a> Default for AirImage<'a> {
    fn default() -> Self {
        Self::new("", "")
    }
}
impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for AirImage<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let class = self.class.unwrap_or("");
        let loading = SafeAttr(self.loading.as_str());
        let fetch_priority = SafeAttr(self.fetch_priority.as_str());
        let fallback_src = SafeAttr(self.fallback_src.unwrap_or(DEFAULT_FALLBACK_SRC));
        if let Some(id) = self.id {
            html_sync! {
                <img
                    id={id}
                    data-air-image
                    src={self.src}
                    alt={self.alt}
                    class={class}
                    loading={loading}
                    decoding="async"
                    fetchpriority={fetch_priority}
                    data-fallback-src={fallback_src}
                />
            }
            .render(cx)
        } else {
            html_sync! {
                <img
                    data-air-image
                    src={self.src}
                    alt={self.alt}
                    class={class}
                    loading={loading}
                    decoding="async"
                    fetchpriority={fetch_priority}
                    data-fallback-src={fallback_src}
                />
            }
            .render(cx)
        }
    }
}
