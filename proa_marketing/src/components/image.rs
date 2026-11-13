// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::static_assets::image_url;
use crate::utils::document::Url;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{html_sync, md_sync};

/// Shared image renderer. Local sources get build-time content hashes, while
/// external sources retain their original URL. The alt attribute is always emitted,
/// including an empty value for decorative images.
#[derive(Default)]
pub(crate) struct Image<'a> {
    pub src: &'a str,
    pub alt: &'a str,
    pub width: &'a str,
    pub height: &'a str,
    pub class: Option<&'static str>,
    pub loading: Option<&'a str>,
    pub decoding: Option<&'a str>,
    pub fetchpriority: Option<&'a str>,
    /// Comma-separated local URLs with width/density descriptors.
    pub srcset: Option<&'a str>,
    pub sizes: Option<&'a str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Image<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <img
                data-image
                src={image_url(self.src)}
                alt={self.alt}
                width={self.width}
                height={self.height}
                class={self.class}
                loading={self.loading}
                decoding={self.decoding}
                fetchpriority={self.fetchpriority}
                srcset={LocalSrcset(self.srcset.unwrap_or(""))}
                sizes={self.sizes}
            />
        }
        .render(cx)
    }
}

// Local srcset URLs have no embedded commas. Write each candidate directly to
// the attribute buffer; keep descriptors and escaping without building a String.
struct LocalSrcset<'a>(&'a str);

impl AttrValue for LocalSrcset<'_> {
    fn should_render(&self) -> bool {
        !self.0.is_empty()
    }
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        for (index, candidate) in self.0.split(',').enumerate() {
            if index != 0 {
                cx.out.extend_static_str(", ")?;
            }
            let candidate = candidate.trim();
            let (url, descriptor) = candidate
                .split_once(char::is_whitespace)
                .unwrap_or((candidate, ""));
            image_url(url).render_attr_value(cx)?;
            if !descriptor.is_empty() {
                cx.out.extend_static_str(" ")?;
                descriptor.trim().render_attr_value(cx)?;
            }
        }
        Ok(())
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Image<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let url = image_url(self.src);
        md_sync! { "![{self.alt}]({Url(url)})" }.render_md(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responsive_sources_share_content_hashes_and_keep_descriptors() {
        let src = "/static/images/airbnb-preview-exterior-420.webp";
        let large = "/static/images/airbnb-preview-exterior-720.webp";
        let output = proa_core::render_to_string(Image {
            src, alt: "A < B & C", width: "420", height: "327",
            srcset: Some("/static/images/airbnb-preview-exterior-420.webp 420w, /static/images/airbnb-preview-exterior-720.webp 720w"),
            sizes: Some("(max-width: 640px) 58vw, 420px"), ..Default::default()
        });
        assert!(output.contains(&format!("src=\"{}\"", image_url(src))));
        assert!(output.contains(&format!(
            "srcset=\"{} 420w, {} 720w\"",
            image_url(src),
            image_url(large)
        )));
        assert!(output.contains("alt=\"A &lt; B &amp; C\""));
        assert!(!output.contains("loading="));
        assert!(output.contains("sizes=\"(max-width: 640px) 58vw, 420px\""));
    }

    #[test]
    fn external_images_keep_their_url_and_are_escaped() {
        let url = "https://example.com/photo.jpg?a=1&b=2";
        assert_eq!(image_url(url), url);
        let output = proa_core::render_to_string(Image {
            src: url,
            alt: "",
            ..Default::default()
        });
        assert!(output.contains("src=\"https://example.com/photo.jpg?a=1&amp;b=2\""));
    }
}
