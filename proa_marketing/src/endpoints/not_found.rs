// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::utils::http::accepts_markdown;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

pub fn markdown() -> Response {
    (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::VARY, "Accept")],
     "# Page not found\n\nThis URL does not identify a public Proa page. Start with the [documentation](/docs), [agent index](/llms.txt), or [sitemap](/sitemap.xml).\n").into_response()
}

pub async fn handler(uri: axum::http::Uri, headers: HeaderMap) -> Response {
    if uri.path().starts_with("/api/") {
        super::api::ApiError::not_found().into_response()
    } else if accepts_markdown(&headers) {
        markdown()
    } else {
        (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::VARY, "Accept")],
         "<!doctype html><html lang=\"en\"><head><title>Page not found - Proa</title><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head><body><main><h1>Page not found</h1><p>This URL does not identify a public Proa page. Visit the <a href=\"/docs\">documentation</a> or <a href=\"/\">homepage</a>.</p></main></body></html>").into_response()
    }
}
