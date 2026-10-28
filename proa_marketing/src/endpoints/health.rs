// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::{header, HeaderMap, HeaderValue};

const REVISION_HEADER: &str = "x-proa-revision";

/// Check whether the site process is healthy.
///
/// Returns plain text when the HTTP process can respond. No credentials are required.
#[utoipa::path(get, path = "/healthz", operation_id = "getHealth",
    responses((status = 200, description = "Healthy process.", body = String, content_type = "text/plain")))]
pub async fn healthz() -> impl axum::response::IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "ok")
}

pub async fn readyz() -> impl axum::response::IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(revision) = build_revision() {
        if let Some(value) = revision_header_value(&revision) {
            headers.insert(REVISION_HEADER, value);
        }
    }
    (headers, "ready")
}

fn build_revision() -> Option<String> {
    std::env::var("PROA_BUILD_REVISION")
        .ok()
        .filter(|revision| !revision.is_empty())
        .or_else(|| std::env::var("RAILWAY_GIT_COMMIT_SHA").ok())
}

fn revision_header_value(revision: &str) -> Option<HeaderValue> {
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    HeaderValue::from_bytes(revision.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::readyz;
    use axum::http::header;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn readiness_response_cannot_be_cached() {
        let response = readyz().await.into_response();
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
}
