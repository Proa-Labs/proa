// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, ops::Range, sync::LazyLock};

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_docs::DocPage;
use proa_macros::html_sync;

use crate::components::icons::{CopyIcon, ExternalLinkIcon};

pub(super) struct DocsPageDescription {
    pub(super) description: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsPageDescription {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(description) = self.description {
            html_sync! {
                <p class="docs-page-description">{description}</p>
            }
            .render(cx)
        } else {
            html_sync! {}.render(cx)
        }
    }
}

pub(super) struct DocsArticleContent {
    pub(super) page: &'static DocPage,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsArticleContent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let html = self.page.article_html;
        let images = DOCS_IMAGES
            .get(self.page.route_path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if let Some(insert_at) = article_actions_insert_at(html) {
            html_sync! {
                {DocsArticleHtml { html, range: 0..insert_at, images }}
                {DocsArticleActions { page: self.page }}
                {DocsArticleHtml { html, range: insert_at..html.len(), images }}
            }
            .render(cx)
        } else {
            html_sync! {
                {DocsArticleActions { page: self.page }}
                {DocsArticleHtml { html, range: 0..html.len(), images }}
            }
            .render(cx)
        }
    }
}

fn article_actions_insert_at(html: &'static str) -> Option<usize> {
    let after_h1 = html.find("</h1>")? + "</h1>".len();
    let after_intro = match html[after_h1..].find("</p>") {
        Some(offset) => after_h1 + offset + "</p>".len(),
        None => after_h1,
    };
    Some(after_intro)
}

struct DocsArticleActions {
    page: &'static DocPage,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsArticleActions {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="docs-article-actions" class="docs-article-actions not-prose">
                <button
                    type="button"
                    class="docs-article-action"
                    data-docs-copy-markdown=""
                    data-docs-markdown-src={self.page.markdown_path}
                >
                    {CopyIcon::new(14)}
                    <span>"Copy Markdown"</span>
                </button>
                <a class="docs-article-action" href={self.page.markdown_path}>
                    {ExternalLinkIcon::new(14)}
                    <span>"Open Markdown"</span>
                </a>
            </div>
        }
        .render(cx)
    }
}

struct DocsImage {
    value: Range<usize>,
    logical_path: String,
}

// Compiled Markdown image tags have this source-attribute shape. Index the
// diagrams once so serving an article writes the compiled HTML in slices.
static DOCS_IMAGES: LazyLock<BTreeMap<&'static str, Vec<DocsImage>>> = LazyLock::new(|| {
    const IMAGE_PREFIX: &str = r#"<img src="/static/images/docs/"#;
    crate::pages::docs::docs()
        .pages
        .iter()
        .filter_map(|page| {
            let html = page.article_html;
            let images: Vec<_> = html
                .match_indices(IMAGE_PREFIX)
                .filter_map(|(offset, _)| {
                    let start = offset + r#"<img src=""#.len();
                    let end = start + html[start..].find('"')?;
                    let path = html[start..end].strip_prefix("/static/")?;
                    let path = path.split_once('?').map_or(path, |(path, _)| path);
                    Some(DocsImage {
                        value: start..end,
                        logical_path: format!("public/{path}"),
                    })
                })
                .collect();
            (!images.is_empty()).then_some((page.route_path, images))
        })
        .collect()
});

struct DocsArticleHtml {
    html: &'static str,
    range: Range<usize>,
    images: &'static [DocsImage],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsArticleHtml {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // The docs compiler supplies trusted markup with escaped text.
        let mut offset = self.range.start;
        for image in self
            .images
            .iter()
            .filter(|image| self.range.contains(&image.value.start))
        {
            cx.out
                .extend_static_str(&self.html[offset..image.value.start])?;
            cx.url(&image.logical_path).render_attr_value(cx)?;
            offset = image.value.end;
        }
        cx.out.extend_static_str(&self.html[offset..self.range.end])
    }
}
