// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared HTML/Markdown representation negotiation for typed pages.

use axum::body::Body;
use axum::http::{
    header::{ACCEPT, CONTENT_TYPE, LINK, VARY},
    HeaderMap, HeaderValue, Response,
};
use axum::response::IntoResponse;
use proa_core::{md::MdContext, DataLoader, MdRenderSync, WriteError};

use super::RouteResponse;

/// Metadata for the stable Markdown representation of a canonical page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownAlternate {
    path: &'static str,
}

impl MarkdownAlternate {
    pub const fn new(path: &'static str) -> Self {
        Self { path }
    }

    pub const fn path(self) -> &'static str {
        self.path
    }
}

/// A canonical page response after content negotiation.
///
/// The HTML variant preserves Proa's typed loader and streaming response. The
/// Markdown variant is rendered directly from the page's `MdRenderSync`
/// implementation and deliberately bypasses the HTML document shell.
pub enum RepresentationResponse<L: DataLoader> {
    Html {
        response: RouteResponse<L>,
        alternate: MarkdownAlternate,
    },
    Markdown {
        body: Vec<u8>,
        canonical: bool,
    },
}

impl<L: DataLoader + 'static> IntoResponse for RepresentationResponse<L> {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Html {
                response,
                alternate,
            } => {
                let mut response = response.into_response();
                response.headers_mut().insert(VARY, accept_vary());
                response
                    .headers_mut()
                    .insert(LINK, alternate_link(alternate));
                response
            }
            Self::Markdown { body, canonical } => {
                let mut response = Response::new(Body::from(body));
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/markdown; charset=utf-8"),
                );
                if canonical {
                    response.headers_mut().insert(VARY, accept_vary());
                }
                response
            }
        }
    }
}

/// Negotiate a canonical page between browser HTML and semantic Markdown.
///
/// Markdown wins only when the client explicitly gives `text/markdown` a
/// positive quality greater than the quality applicable to HTML. Missing,
/// wildcard-only, invalid, and equal-preference requests stay on HTML.
pub fn negotiate<L, M>(
    headers: &HeaderMap,
    html: RouteResponse<L>,
    markdown: M,
    alternate: MarkdownAlternate,
) -> Result<RepresentationResponse<L>, WriteError>
where
    L: DataLoader,
    M: MdRenderSync<Vec<u8>>,
{
    if prefers_markdown(headers) {
        Ok(RepresentationResponse::Markdown {
            body: render_markdown(markdown)?,
            canonical: true,
        })
    } else {
        Ok(RepresentationResponse::Html {
            response: html,
            alternate,
        })
    }
}

/// Render the explicit stable `.md` representation without content
/// negotiation or an HTML document shell.
pub fn explicit_markdown<M>(
    markdown: M,
) -> Result<RepresentationResponse<proa_core::NoLoader>, WriteError>
where
    M: MdRenderSync<Vec<u8>>,
{
    Ok(RepresentationResponse::Markdown {
        body: render_markdown(markdown)?,
        canonical: false,
    })
}

fn render_markdown<M>(markdown: M) -> Result<Vec<u8>, WriteError>
where
    M: MdRenderSync<Vec<u8>>,
{
    let mut body = Vec::new();
    markdown.render_md(&mut MdContext::new(&mut body))?;
    Ok(body)
}

fn prefers_markdown(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(ACCEPT).and_then(|value| value.to_str().ok()) else {
        return false;
    };

    let ranges: Vec<MediaRange<'_>> = value.split(',').filter_map(MediaRange::parse).collect();
    let has_explicit_markdown = ranges.iter().any(|range| {
        range.ty.eq_ignore_ascii_case("text")
            && range.subtype.eq_ignore_ascii_case("markdown")
            && range.quality > 0
    });
    if !has_explicit_markdown {
        return false;
    }

    let markdown = quality_for(&ranges, "text", "markdown");
    let html = quality_for(&ranges, "text", "html");
    markdown > 0 && markdown > html
}

#[derive(Debug, Clone, Copy)]
struct MediaRange<'a> {
    ty: &'a str,
    subtype: &'a str,
    quality: u16,
}

impl<'a> MediaRange<'a> {
    fn parse(raw: &'a str) -> Option<Self> {
        let mut parts = raw.trim().split(';');
        let media = parts.next()?.trim();
        let (ty, subtype) = media.split_once('/')?;
        let ty = ty.trim();
        let subtype = subtype.trim();
        if ty.is_empty() || subtype.is_empty() {
            return None;
        }

        let mut quality = 1_000;
        for parameter in parts {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = parse_quality(value.trim())?;
            }
        }

        Some(Self {
            ty,
            subtype,
            quality,
        })
    }

    fn specificity(self, ty: &str, subtype: &str) -> Option<u8> {
        if self.ty == "*" && self.subtype == "*" {
            Some(0)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype == "*" {
            Some(1)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype.eq_ignore_ascii_case(subtype) {
            Some(2)
        } else {
            None
        }
    }
}

fn quality_for(ranges: &[MediaRange<'_>], ty: &str, subtype: &str) -> u16 {
    ranges
        .iter()
        .filter_map(|range| {
            range
                .specificity(ty, subtype)
                .map(|specificity| (specificity, range.quality))
        })
        .max_by_key(|(specificity, quality)| (*specificity, *quality))
        .map(|(_, quality)| quality)
        .unwrap_or(0)
}

fn parse_quality(raw: &str) -> Option<u16> {
    if raw == "0" {
        return Some(0);
    }
    if raw == "1" {
        return Some(1_000);
    }

    let (whole, fraction) = raw.split_once('.')?;
    if !matches!(whole, "0" | "1") || fraction.len() > 3 || fraction.is_empty() {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut scaled = fraction.parse::<u16>().ok()?;
    for _ in fraction.len()..3 {
        scaled *= 10;
    }
    if whole == "1" {
        (scaled == 0).then_some(1_000)
    } else {
        Some(scaled)
    }
}

fn accept_vary() -> HeaderValue {
    HeaderValue::from_static("Accept")
}

fn alternate_link(alternate: MarkdownAlternate) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "<{}>; rel=\"alternate\"; type=\"text/markdown\"",
        alternate.path()
    ))
    .expect("Markdown alternate paths must be valid header values")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{header, Request},
        routing::get,
        Router,
    };
    use http_body_util::BodyExt;
    use proa_core::{text::StaticRaw, WriteBuf, WriteError};
    use proa_macros::md_sync;
    use std::{collections::HashMap, sync::Arc};
    use tower::ServiceExt;

    use crate::{manifest::IslandManifest, route::context::RouteContextLayer};

    #[derive(Clone, Copy)]
    struct MarkdownPage;

    impl<B: WriteBuf> MdRenderSync<B> for MarkdownPage {
        fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
            md_sync! { r###"
                # Route dossier

                Semantic body.
                "###
            }
            .render_md(cx)
        }
    }

    fn headers(accept: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(ACCEPT, HeaderValue::from_str(accept).unwrap());
        }
        headers
    }

    #[test]
    fn negotiation_defaults_to_html_for_absent_wildcard_equal_and_zero_quality() {
        for accept in [
            None,
            Some("*/*"),
            Some("text/*"),
            Some("text/html, text/markdown"),
            Some("text/html;q=0.8, text/markdown;q=0.8"),
            Some("text/markdown;q=0, text/html;q=0.1"),
            Some("text/markdown;q=0.5, */*;q=1"),
        ] {
            assert!(!prefers_markdown(&headers(accept)), "{accept:?}");
        }
    }

    #[test]
    fn negotiation_selects_explicitly_preferred_markdown() {
        for accept in [
            "text/markdown",
            "text/html;q=0.4, text/markdown;q=0.9",
            "application/json;q=1, text/markdown;q=0.2",
        ] {
            assert!(prefers_markdown(&headers(Some(accept))), "{accept}");
        }
    }

    #[tokio::test]
    async fn canonical_html_advertises_markdown_and_varies_on_accept() {
        let app = Router::new()
            .route(
                "/",
                get(|| async {
                    negotiate(
                        &headers(None),
                        RouteResponse::ssr(StaticRaw("<!DOCTYPE html><p>HTML</p>")),
                        MarkdownPage,
                        MarkdownAlternate::new("/index.md"),
                    )
                    .unwrap()
                }),
            )
            .layer(RouteContextLayer::new(
                Arc::new(IslandManifest {
                    islands: Default::default(),
                    assets: Default::default(),
                    import_map: Default::default(),
                    ssr_bundle: Default::default(),
                }),
                Arc::new(HashMap::new()),
            ));
        let response = app.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(response.headers()[VARY], "Accept");
        assert_eq!(
            response.headers()[LINK],
            "</index.md>; rel=\"alternate\"; type=\"text/markdown\""
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn canonical_and_explicit_markdown_bodies_are_identical() {
        let canonical = negotiate(
            &headers(Some("text/markdown")),
            RouteResponse::ssr(StaticRaw("unused")),
            MarkdownPage,
            MarkdownAlternate::new("/index.md"),
        )
        .unwrap()
        .into_response();
        let explicit = explicit_markdown(MarkdownPage).unwrap().into_response();

        assert_eq!(canonical.headers()[VARY], "Accept");
        assert!(explicit.headers().get(VARY).is_none());
        assert_eq!(
            canonical.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            explicit.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert!(canonical.headers().get(header::LINK).is_none());

        let canonical = canonical.into_body().collect().await.unwrap().to_bytes();
        let explicit = explicit.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(canonical, explicit);
        assert_eq!(canonical, "# Route dossier\n\nSemantic body.\n");
    }
}
