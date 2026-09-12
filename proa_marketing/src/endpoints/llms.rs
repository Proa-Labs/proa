// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;
use proa_docs::{DocPage, Docs};

use crate::pages::docs;

pub async fn site_handler() -> impl axum::response::IntoResponse {
    let collections = [("Docs", docs::docs())];
    llms_full_text(&collections)
}

fn llms_full_text(collections: &[(&str, &'static Docs)]) -> impl axum::response::IntoResponse {
    let mut body = String::new();

    body.push_str("# Proa documentation corpus\n\n");
    body.push_str(
        "This plain text endpoint concatenates the Markdown source used to build the Proa site.\n",
    );

    for (label, docs) in collections {
        for page in docs.pages {
            if page.frontmatter.hidden {
                continue;
            }
            push_page(&mut body, label, page);
        }
    }

    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
}

fn push_page(body: &mut String, collection: &str, page: &DocPage) {
    body.push_str("\n\n---\n\n");
    body.push_str("# ");
    body.push_str(collection);
    body.push_str(": ");
    body.push_str(page.title);
    body.push_str("\n\nURL: ");
    body.push_str(page.route_path);
    body.push('\n');
    if let Some(description) = page.description {
        body.push_str("Description: ");
        body.push_str(description);
        body.push('\n');
    }
    body.push('\n');
    body.push_str(page.markdown.trim());
    body.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn site_llms_text_contains_documentation_markdown() {
        let response = site_handler().await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        // `/docs` is the quickstart itself now, so the corpus heading carries
        // that title while the URL stays the docs root.
        assert!(body.contains("# Docs: Quickstart"));
        assert!(body.contains("URL: /docs"));
        assert!(!body.contains("# Blog:"));
        assert!(body.contains("Proa documentation corpus"));
    }

    #[tokio::test]
    async fn site_llms_text_includes_visible_guide_pages() {
        let response = site_handler().await.into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("# Docs: Islands"));
        assert!(body.contains("URL: /docs/guides/islands"));
        assert!(body.contains("URL: /docs/rsjs"));
    }
}
