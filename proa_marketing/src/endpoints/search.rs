// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use std::sync::OnceLock;

use axum::extract::Query;
use axum::http::header;
use axum::Json;
use serde::Serialize;

use crate::pages::docs;

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    #[serde(default)]
    #[param(default = "")]
    q: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = LegacySearchResponse)]
pub(super) struct SearchResponse {
    query: String,
    has_results: bool,
    first_url: &'static str,
    results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[schema(as = LegacySearchResult)]
struct SearchResult {
    title: &'static str,
    url: &'static str,
    description: &'static str,
    collection: &'static str,
}

#[derive(Debug)]
struct SearchRecord {
    result: SearchResult,
    page: &'static proa_docs::DocPage,
    haystack: String,
}

static SEARCH_INDEX: OnceLock<Vec<SearchRecord>> = OnceLock::new();

/// Legacy browser search. Prefer /api/v1/search for paginated agent reads.
///
/// Returns at most eight matches. Empty queries return no results.
#[utoipa::path(get, path = routes::LEGACY_SEARCH, operation_id = "searchProaDocumentation",
    params(SearchQuery), responses(super::api::ApiError,
        (status = 200, description = "Up to eight search matches.", body = SearchResponse),
        (status = 400, description = "Malformed query parameters.", body = String, content_type = "text/plain"),
        (status = 405, description = "Unsupported HTTP method.", headers(("Allow" = String, description = "GET,HEAD,POST")))
    ))]
pub async fn handler(Query(query): Query<SearchQuery>) -> impl axum::response::IntoResponse {
    search_response(query.q)
}

/// Legacy browser search accepting a JSON string.
///
/// Returns the same eight-result format as GET /api/search. The body is a
/// JSON string, not an object containing a query field.
#[utoipa::path(post, path = routes::LEGACY_SEARCH, operation_id = "searchProaDocumentationPost",
    request_body(content = String, content_type = "application/json"), responses(super::api::ApiError,
        (status = 200, description = "Up to eight search matches.", body = SearchResponse),
        (status = 400, description = "Malformed JSON.", body = String, content_type = "text/plain"),
        (status = 405, description = "Unsupported HTTP method.", headers(("Allow" = String, description = "GET,HEAD,POST"))),
        (status = 413, description = "Request body too large.", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected application/json.", body = String, content_type = "text/plain"),
        (status = 422, description = "Expected a JSON string.", body = String, content_type = "text/plain")
    ))]
pub async fn post_handler(Json(query): Json<String>) -> impl axum::response::IntoResponse {
    search_response(query)
}

fn search_response(query: String) -> impl axum::response::IntoResponse {
    let needle = query.trim().to_ascii_lowercase();
    let mut results = Vec::new();

    if !needle.is_empty() {
        collect_matches(&mut results, &needle);
    }

    let has_results = !results.is_empty();
    let first_url = results.first().map(|result| result.url).unwrap_or("");
    let body = serde_json::to_string(&SearchResponse {
        query,
        has_results,
        first_url,
        results,
    })
    .expect("search response serializes");

    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
}

fn collect_matches(results: &mut Vec<SearchResult>, needle: &str) {
    for record in search_index() {
        if record.haystack.contains(needle) {
            results.push(record.result);
            if results.len() >= 8 {
                break;
            }
        }
    }
}

fn search_index() -> &'static [SearchRecord] {
    SEARCH_INDEX.get_or_init(|| {
        let mut records = Vec::new();
        collect_documents(&mut records, "Docs", docs::docs());
        records
    })
}

pub(super) fn matching_pages(
    needle: &str,
) -> impl Iterator<Item = &'static proa_docs::DocPage> + '_ {
    search_index()
        .iter()
        .filter(move |record| record.haystack.contains(needle))
        .map(|record| record.page)
}

fn collect_documents(
    records: &mut Vec<SearchRecord>,
    collection: &'static str,
    docs: &'static proa_docs::Docs,
) {
    for page in docs.pages {
        if page.frontmatter.hidden || page.frontmatter.search == Some(false) {
            continue;
        }
        let mut haystack = String::with_capacity(
            page.title.len()
                + page.description.map(str::len).unwrap_or(0)
                + page.markdown.len()
                + 2,
        );
        push_lowercase_segment(&mut haystack, page.title);
        if let Some(description) = page.description {
            push_lowercase_segment(&mut haystack, description);
        }
        push_lowercase_segment(&mut haystack, page.markdown);

        records.push(SearchRecord {
            page,
            result: SearchResult {
                title: page.title,
                url: page.route_path,
                description: page.description.unwrap_or(page.route_path),
                collection,
            },
            haystack,
        });
    }
}

fn push_lowercase_segment(out: &mut String, value: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    for ch in value.chars() {
        out.push(ch.to_ascii_lowercase());
    }
}
