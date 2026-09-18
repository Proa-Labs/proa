// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server-rendered YouTube previews. The player is created only on activation.

use std::{
    fmt,
    future::{Future, Ready},
};

use proa_core::md::TrustedMarkdown;
use proa_core::shared::attr_value::display_attr;
use proa_core::{
    csp_nonce_attr, DataLoader, MdRender, MdRenderSync, RenderOutcome, StaticRaw, WebContext,
    WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html_sync, md_sync};

/// Optional default styles; serve as text/css when inline styles are disabled.
pub const YOUTUBE_CSS: &str = include_str!("youtube.css");

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum YouTubeError {
    #[error("YouTube video ID must contain 11 ASCII letters, digits, hyphens, or underscores")]
    InvalidVideoId,
    #[error("a non-empty video title is required for accessibility")]
    EmptyTitle,
    #[error("poster URL must be HTTPS or a root-relative path")]
    InvalidPosterUrl,
}

#[derive(Debug, Clone, Copy)]
pub struct YouTubeEmbed<'a> {
    video_id: &'a str,
    title: &'a str,
    poster: Option<&'a str>,
    start_seconds: u32,
    inline_styles: bool,
    pub class: Option<&'a str>,
}

impl<'a> YouTubeEmbed<'a> {
    pub fn new(video_id: &'a str, title: &'a str) -> Result<Self, YouTubeError> {
        if video_id.len() != 11
            || !video_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(YouTubeError::InvalidVideoId);
        }
        if title.trim().is_empty() {
            return Err(YouTubeError::EmptyTitle);
        }
        Ok(Self {
            video_id,
            title,
            poster: None,
            start_seconds: 0,
            inline_styles: true,
            class: None,
        })
    }

    /// A local poster avoids the default thumbnail request to i.ytimg.com.
    pub fn poster(mut self, url: &'a str) -> Result<Self, YouTubeError> {
        if !crate::script::valid_resource_url(url) {
            return Err(YouTubeError::InvalidPosterUrl);
        }
        self.poster = Some(url);
        Ok(self)
    }

    pub const fn start_at(mut self, seconds: u32) -> Self {
        self.start_seconds = seconds;
        self
    }

    /// Set false when supplying your own styles or serving YOUTUBE_CSS externally.
    pub const fn inline_styles(mut self, enabled: bool) -> Self {
        self.inline_styles = enabled;
        self
    }

    /// Add a class to the root figure for application-owned styling.
    pub const fn class(mut self, class: &'a str) -> Self {
        self.class = Some(class);
        self
    }
}

struct PosterUrl<'a> {
    video_id: &'a str,
    custom: Option<&'a str>,
}

impl fmt::Display for PosterUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.custom {
            Some(url) => f.write_str(url),
            None => write!(f, "https://i.ytimg.com/vi/{}/hqdefault.jpg", self.video_id),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for YouTubeEmbed<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nonce = cx.csp_nonce_cloned();
        let poster = PosterUrl {
            video_id: self.video_id,
            custom: self.poster,
        };
        html_sync! {
            <figure data-proa-youtube class={self.class}>
                {if self.inline_styles {
                    html_sync! {
                        <style nonce={csp_nonce_attr(nonce.as_ref())}>{StaticRaw(YOUTUBE_CSS)}</style>
                    }
                }}
                <div data-proa-youtube-frame>
                    <button
                        type="button"
                        data-proa-youtube-play
                        data-src={display_attr(format_args!("https://www.youtube-nocookie.com/embed/{}?autoplay=1&playsinline=1&start={}", self.video_id, self.start_seconds))}
                        data-title={self.title}
                        aria-label={display_attr(format_args!("Play {}", self.title))}
                    >
                        <img
                            src={display_attr(poster)}
                            alt=""
                            width="480"
                            height="360"
                            loading="lazy"
                            decoding="async"
                        />
                        <span data-proa-youtube-play-icon aria-hidden="true">"▶"</span>
                    </button>
                </div>
                <script data-proa-youtube-bootstrap nonce={csp_nonce_attr(nonce.as_ref())}>
                    {StaticRaw(include_str!("youtube.js"))}
                </script>
                <figcaption>
                    <a href={display_attr(format_args!("https://www.youtube.com/watch?v={}", self.video_id))}>
                        {self.title}
                    </a>
                </figcaption>
            </figure>
        }.render(cx)
    }
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for YouTubeEmbed<'_> {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        // The constructor validates URL-safe IDs; the title uses Markdown escaping.
        let video_id = TrustedMarkdown(self.video_id);
        MdRenderSync::render_md(
            md_sync! { r###"[{self.title}](https://www.youtube.com/watch?v={video_id})"### },
            cx,
        )
    }
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRender<Loader, B> for YouTubeEmbed<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(self, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_video_configuration() {
        for id in ["", "short", "abcdefghij/", "abcdefghij\"", "../../video"] {
            assert!(YouTubeEmbed::new(id, "Video").is_err());
        }
        assert!(YouTubeEmbed::new("M7lc1UVf-VE", " ").is_err());
        assert!(YouTubeEmbed::new("M7lc1UVf-VE", "Video")
            .unwrap()
            .poster("data:image/svg+xml,evil")
            .is_err());
    }

    #[test]
    fn renders_a_preview_with_a_safe_fallback_and_no_player() {
        let mut cx =
            WebContext::new().with_csp_nonce(proa_core::CspNonce::new("YWJjZA==").unwrap());
        YouTubeEmbed::new("M7lc1UVf-VE", "A <video> & \"title\"")
            .unwrap()
            .start_at(42)
            .class("my-video")
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(!html.contains("<iframe"));
        assert!(html.contains("A &lt;video&gt; &amp;"));
        assert!(html.contains("&amp;start=42"));
        assert!(html.contains(r#"type="button""#));
        assert!(html.contains(r#"class="my-video""#));
        assert!(html.contains(r#"href="https://www.youtube.com/watch?v=M7lc1UVf-VE""#));
        assert!(html.contains(r#"loading="lazy""#));
        assert_eq!(html.matches(r#"nonce="YWJjZA==""#).count(), 2);
    }

    #[test]
    fn supports_local_posters_and_external_styles() {
        let mut cx = WebContext::new();
        YouTubeEmbed::new("M7lc1UVf-VE", "Video")
            .unwrap()
            .poster("/video/poster.webp")
            .unwrap()
            .inline_styles(false)
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(!html.contains("<style"));
        assert!(!html.contains("i.ytimg.com"));
        assert!(html.contains(r#"src="/video/poster.webp""#));
    }

    #[test]
    fn markdown_renders_an_escaped_title_and_video_link() {
        for (video_id, expected) in [
            (
                "M7lc1UVf-VE",
                "[Title \\[one\\]](https://www.youtube.com/watch?v=M7lc1UVf-VE)",
            ),
            (
                "AbCd_eF-g12",
                "[Title \\[one\\]](https://www.youtube.com/watch?v=AbCd_eF-g12)",
            ),
        ] {
            for asynchronous in [false, true] {
                let video = YouTubeEmbed::new(video_id, "Title [one]").unwrap();
                let mut out = Vec::new();
                let mut cx = WebContext::with_buffer(&mut out);
                if asynchronous {
                    assert!(matches!(
                        MdRender::render_md(video, &mut cx),
                        RenderOutcome::Done(Ok(()))
                    ));
                } else {
                    MdRenderSync::render_md(video, &mut cx).unwrap();
                }
                assert_eq!(String::from_utf8(out).unwrap(), expected);
            }
        }
    }
}
