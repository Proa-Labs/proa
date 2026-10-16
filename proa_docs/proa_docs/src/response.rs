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
    render_page_to_string_with_base_path(docs, page, &proa_core::BasePath::default())
}

pub fn render(docs: &'static Docs, path: &str) -> DocsResponse {
    render_with_base_path(docs, path, &proa_core::BasePath::default())
}

/// Render the docs shell with public URLs under the deployment mount.
/// Collection paths such as `/docs` remain unprefixed in the generated registry.
pub fn render_page_to_string_with_base_path(
    docs: &'static Docs,
    page: &'static DocPage,
    base_path: &proa_core::BasePath,
) -> String {
    use proa_core::WebRenderSync;
    let mut cx = proa_core::WebContext::new();
    // proa-perf-ok: share the immutable prefix when constructing the response context.
    cx.request_mut().set_base_path(base_path.clone());
    DocsShell { docs, page }
        .render(&mut cx)
        .expect("render docs shell");
    cx.into_output().into_string()
}

/// Serve an unprefixed docs request with a deployment-specific public mount.
/// Markdown and JSON artifact routes retain their original request paths.
pub fn render_with_base_path(
    docs: &'static Docs,
    path: &str,
    base_path: &proa_core::BasePath,
) -> DocsResponse {
    if !base_path.as_str().is_empty() {
        if let Some(search) = docs.search {
            let normalized = normalize_path(path);
            if normalized == normalize_path(search.manifest_path)
                || normalized == normalize_path(search.documents_path)
            {
                return mounted_search_response(search, &normalized, base_path);
            }
        }
    }
    if let Some(response) = render_static(docs, path) {
        return response;
    }
    let Some(page) = docs.page_for_path(path).or_else(|| docs.first_page()) else {
        return DocsResponse::not_found();
    };
    DocsResponse::html(render_page_to_string_with_base_path(docs, page, base_path))
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

// Search JSON is prepared at the artifact response boundary, outside rendering.
fn mounted_search_response(
    search: crate::SearchArtifacts,
    path: &str,
    base_path: &proa_core::BasePath,
) -> DocsResponse {
    use sha2::{Digest, Sha256};
    fn compose(value: &mut serde_json::Value, key: &str, base: &proa_core::BasePath) {
        if let Some(path) = value.get(key).and_then(serde_json::Value::as_str) {
            value[key] = serde_json::Value::String(base.url(path).to_string());
        }
    }
    let mut documents: serde_json::Value =
        serde_json::from_str(search.documents_json).expect("generated docs search JSON");
    if let Some(items) = documents
        .get_mut("documents")
        .and_then(serde_json::Value::as_array_mut)
    {
        for item in items {
            compose(item, "url", base_path);
        }
    }
    let documents = serde_json::to_string(&documents).expect("serialize mounted search documents");
    let body = if path == normalize_path(search.documents_path) {
        documents
    } else {
        let mut manifest: serde_json::Value =
            serde_json::from_str(search.manifest_json).expect("generated docs search manifest");
        compose(&mut manifest, "basePath", base_path);
        compose(&mut manifest, "documents", base_path);
        manifest["hash"] =
            serde_json::Value::String(format!("sha256-{:x}", Sha256::digest(documents.as_bytes())));
        serde_json::to_string(&manifest).expect("serialize mounted search manifest")
    };
    DocsResponse {
        status: 200,
        content_type: "application/json; charset=utf-8",
        body: Cow::Owned(body),
    }
}
