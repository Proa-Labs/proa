// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    model::{DocPage, Docs},
    path::normalize_path,
    shell::DocsShell,
};
use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct DocsResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Cow<'static, str>,
}

impl DocsResponse {
    pub fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: Cow::Owned(body),
        }
    }

    pub fn markdown(body: &'static str) -> Self {
        Self {
            status: 200,
            content_type: "text/markdown; charset=utf-8",
            body: Cow::Borrowed(body),
        }
    }

    pub fn json(body: &'static str) -> Self {
        Self {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: Cow::Borrowed(body),
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: Cow::Borrowed("Not found"),
        }
    }
}

pub fn render_page_to_string(docs: &'static Docs, page: &'static DocPage) -> String {
    proa_core::render_to_string(DocsShell { docs, page })
}

pub fn render(docs: &'static Docs, path: &str) -> DocsResponse {
    if let Some(response) = render_static(docs, path) {
        return response;
    }
    let Some(page) = docs.page_for_path(path).or_else(|| docs.first_page()) else {
        return DocsResponse::not_found();
    };
    DocsResponse::html(render_page_to_string(docs, page))
}

pub fn render_static(docs: &'static Docs, path: &str) -> Option<DocsResponse> {
    let normalized = normalize_path(path);
    if let Some(search) = docs.search {
        if normalized == normalize_path(search.manifest_path) {
            return Some(DocsResponse::json(search.manifest_json));
        }
        if normalized == normalize_path(search.documents_path) {
            return Some(DocsResponse::json(search.documents_json));
        }
    }

    for page in docs.pages {
        if normalized == normalize_path(page.markdown_path) {
            return Some(DocsResponse::markdown(page.markdown));
        }
    }

    None
}
