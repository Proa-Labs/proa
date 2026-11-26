// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use axum::http::header;
use proa_docs::{DocPage, Docs};

use crate::pages::docs;

/// Read the complete public Proa documentation corpus.
///
/// Returns all public documentation in one response. Use /docs/llms.txt to
/// find a focused page when the entire corpus is unnecessary.
#[utoipa::path(get, path = routes::DOCS_CORPUS, operation_id = "getDocumentationCorpus",
    responses((status = 200, description = "Complete public documentation corpus.", body = String, content_type = "text/plain")))]
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
