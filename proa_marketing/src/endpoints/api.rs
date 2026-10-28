// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Public, read-only documentation operations shared by HTTP and MCP.

use crate::data::routes;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{rejection::QueryRejection, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use proa_docs::DocPage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{pages::docs, utils::site_url::public_site_url};

const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;
pub const REQUESTS_PER_MINUTE: u32 = 600;

#[derive(Debug, Deserialize, IntoParams, JsonSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct SearchInput {
    /// Phrase to find in document titles, descriptions, or Markdown.
    #[param(min_length = 1, max_length = 256)]
    #[schemars(length(min = 1, max = 256))]
    pub q: String,
    /// Maximum results per page; defaults to 10.
    #[param(minimum = 1, maximum = 50, default = 10, nullable = false)]
    #[schemars(range(min = 1, max = MAX_LIMIT))]
    pub limit: Option<usize>,
    /// next_cursor from a preceding search with the same q.
    #[param(nullable = false)]
    pub cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Pagination {
    /// Maximum results per page; defaults to 10.
    #[param(minimum = 1, maximum = 50, default = 10, nullable = false)]
    pub limit: Option<usize>,
    /// next_cursor from the preceding response. Restart after a deployment.
    #[param(nullable = false)]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams, JsonSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PageInput {
    /// Published route or Markdown path, for example /docs/core/html-rendering.
    pub path: String,
}

#[derive(Debug, Serialize, ToSchema, JsonSchema)]
pub struct DocumentSummary {
    pub title: &'static str,
    pub path: &'static str,
    pub markdown_url: &'static str,
    pub description: &'static str,
}

impl From<&'static DocPage> for DocumentSummary {
    fn from(page: &'static DocPage) -> Self {
        Self {
            title: page.title,
            path: page.route_path,
            markdown_url: page.markdown_path,
            description: page.description.unwrap_or(""),
        }
    }
}

#[derive(Debug, Serialize, ToSchema, JsonSchema)]
pub struct DocumentList {
    pub results: Vec<DocumentSummary>,
    pub total: usize,
    /// Opaque cursor for the next page, or null when the result set is complete.
    #[schema(required)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, JsonSchema)]
pub struct Document {
    #[serde(flatten)]
    pub summary: DocumentSummary,
    pub markdown: &'static str,
}

#[derive(Serialize, ToSchema)]
pub struct Problem<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    #[schema(value_type = String, format = "uri")]
    documentation: crate::utils::document::SiteUrl<'a>,
}

#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub detail: &'static str,
}

impl ApiError {
    pub fn invalid(detail: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            detail,
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            detail: const_format::concatcp!(
                "No public document or API route matches this request. Use ",
                routes::API_DOCS,
                " to list documents."
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(Problem {
                kind: "about:blank",
                title: self.status.canonical_reason().unwrap_or("Request failed"),
                status: self.status.as_u16(),
                code: self.code,
                detail: self.detail,
                documentation: crate::utils::document::SiteUrl {
                    origin: public_site_url(),
                    path: routes::API_GUIDE,
                },
            }),
        )
            .into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

pub fn search(input: SearchInput) -> Result<DocumentList, ApiError> {
    let q = input.q.trim();
    if q.is_empty() || q.chars().count() > 256 {
        return Err(ApiError::invalid(
            "q must contain between 1 and 256 non-blank characters.",
        ));
    }
    paginate(
        super::search::matching_pages(&q.to_ascii_lowercase()),
        Pagination {
            limit: input.limit,
            cursor: input.cursor,
        },
    )
}

pub fn list(pagination: Pagination) -> Result<DocumentList, ApiError> {
    paginate(
        docs::docs()
            .pages
            .iter()
            .filter(|page| !page.frontmatter.hidden),
        pagination,
    )
}

fn paginate(
    pages: impl Iterator<Item = &'static DocPage>,
    pagination: Pagination,
) -> Result<DocumentList, ApiError> {
    let limit = pagination.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::invalid("limit must be an integer from 1 to 50."));
    }
    let offset = match pagination.cursor.as_deref() {
        None => 0,
        Some(cursor) => cursor.parse::<usize>().map_err(|_| {
            ApiError::invalid("Invalid cursor. Use next_cursor from the preceding response.")
        })?,
    };
    let mut results = Vec::with_capacity(limit);
    let mut total = 0;
    for page in pages {
        if total >= offset && results.len() < limit {
            results.push(page.into());
        }
        total += 1;
    }
    if offset > total {
        return Err(ApiError::invalid(
            "The cursor is outside this result set. Start again without a cursor.",
        ));
    }
    let next = offset + results.len();
    Ok(DocumentList {
        results,
        total,
        next_cursor: (next < total).then(|| next.to_string()),
    })
}

pub fn page(path: &str) -> Result<&'static DocPage, ApiError> {
    // Only lookup known public paths. Never fetch a supplied URL or read a file.
    docs::docs()
        .pages
        .iter()
        .find(|page| {
            !page.frontmatter.hidden && (page.route_path == path || page.markdown_path == path)
        })
        .ok_or_else(ApiError::not_found)
}

pub fn get(input: PageInput) -> Result<Document, ApiError> {
    let page = page(&input.path)?;
    Ok(Document {
        summary: page.into(),
        markdown: page.markdown,
    })
}

fn query<T>(input: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    input.map(|Query(value)| value).map_err(|_| {
        ApiError::invalid(const_format::concatcp!(
            "Malformed query parameters. Check the endpoint parameters in ",
            routes::OPENAPI,
            "."
        ))
    })
}

/// Search public Proa documentation.
///
/// Match a case-insensitive phrase in searchable public pages. Continue with
/// next_cursor and the same query; restart pagination after a deployment.
#[utoipa::path(get, path = routes::API_SEARCH, operation_id = "searchDocs",
    params(SearchInput), responses((status = 200, description = "A page of public document summaries.", body = DocumentList), ApiError))]
pub async fn search_handler(
    input: Result<Query<SearchInput>, QueryRejection>,
) -> Result<Json<DocumentList>, ApiError> {
    search(query(input)?).map(Json)
}

/// List public documentation with stable paths and Markdown URLs.
///
/// Includes public pages excluded from search. Use next_cursor to continue
/// reading this deployment's registry. Hidden pages are never listed.
#[utoipa::path(get, path = routes::API_DOCS, operation_id = "listDocs",
    params(Pagination), responses((status = 200, description = "A page of public document summaries.", body = DocumentList), ApiError))]
pub async fn list_handler(
    input: Result<Query<Pagination>, QueryRejection>,
) -> Result<Json<DocumentList>, ApiError> {
    list(query(input)?).map(Json)
}

/// Read a complete public document as Markdown.
///
/// Accepts a published route or Markdown path from search or the document list.
/// Arbitrary URLs and filesystem paths are not fetched.
#[utoipa::path(get, path = routes::API_PAGE, operation_id = "getDoc",
    params(PageInput), responses((status = 200, description = "Document metadata and complete Markdown.", body = Document), ApiError))]
pub async fn page_handler(
    input: Result<Query<PageInput>, QueryRejection>,
) -> Result<Json<Document>, ApiError> {
    get(query(input)?).map(Json)
}

pub async fn not_found() -> ApiError {
    ApiError::not_found()
}

pub async fn method_not_allowed() -> ApiError {
    ApiError {
        status: StatusCode::METHOD_NOT_ALLOWED,
        code: "method_not_allowed",
        detail: const_format::concatcp!(
            "This documentation endpoint supports GET and HEAD. See ",
            routes::OPENAPI,
            " for supported operations."
        ),
    }
}

#[derive(Debug)]
pub struct RequestBudget {
    window: Instant,
    used: u32,
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            window: Instant::now(),
            used: 0,
        }
    }
}

impl RequestBudget {
    fn consume(&mut self, now: Instant) -> (bool, u32, u64) {
        if now.duration_since(self.window) >= Duration::from_secs(60) {
            self.window = now;
            self.used = 0;
        }
        let allowed = self.used < REQUESTS_PER_MINUTE;
        if allowed {
            self.used += 1;
        }
        (
            allowed,
            REQUESTS_PER_MINUTE - self.used,
            60 - now.duration_since(self.window).as_secs(),
        )
    }
}

/// An honest, bounded, per-process budget. Do not trust forwarded client IPs.
pub async fn rate_limit(
    State(budget): State<Arc<Mutex<RequestBudget>>>,
    request: Request,
    next: Next,
) -> Response {
    let (allowed, remaining, reset) = budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .consume(Instant::now());
    let mut response = if allowed {
        next.run(request).await
    } else {
        ApiError { status:StatusCode::TOO_MANY_REQUESTS, code:"rate_limited", detail:"The shared documentation request budget is exhausted. Retry after the Retry-After delay." }.into_response()
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "ratelimit-policy",
        HeaderValue::from_static("\"docs\";q=600;w=60"),
    );
    headers.insert(
        "ratelimit",
        format!("\"docs\";r={remaining};t={reset}").parse().unwrap(),
    );
    headers.insert("ratelimit-limit", HeaderValue::from_static("600"));
    headers.insert("ratelimit-remaining", remaining.into());
    headers.insert("ratelimit-reset", reset.into());
    if !allowed {
        headers.insert(header::RETRY_AFTER, reset.into());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_rejects_excess_requests_and_resets() {
        let mut budget = RequestBudget::default();
        let now = budget.window;
        for _ in 0..REQUESTS_PER_MINUTE {
            assert!(budget.consume(now).0);
        }
        assert_eq!(budget.consume(now), (false, 0, 60));
        assert_eq!(
            budget.consume(now + Duration::from_secs(60)),
            (true, REQUESTS_PER_MINUTE - 1, 60)
        );
    }

    #[test]
    fn pagination_visits_every_public_document_once() {
        let mut cursor = None;
        let mut paths = std::collections::HashSet::new();
        loop {
            let page = list(Pagination {
                limit: Some(7),
                cursor,
            })
            .unwrap();
            for doc in page.results {
                assert!(paths.insert(doc.path));
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                assert_eq!(paths.len(), page.total);
                break;
            }
        }
        assert_eq!(
            paths.len(),
            docs::docs()
                .pages
                .iter()
                .filter(|p| !p.frontmatter.hidden)
                .count()
        );
    }

    #[tokio::test]
    async fn exhausted_budget_returns_problem_and_retry_headers() {
        use axum::{body::Body, http::Request, middleware, routing::get, Router};
        use tower_test::ServiceExt;
        let state = Arc::new(Mutex::new(RequestBudget {
            window: Instant::now(),
            used: REQUESTS_PER_MINUTE,
        }));
        let app = Router::new()
            .route("/", get(|| async { "unreachable" }))
            .layer(middleware::from_fn_with_state(state, rate_limit));
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        assert_eq!(response.headers()["ratelimit-remaining"], "0");
        assert!(
            response.headers()[header::RETRY_AFTER]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
