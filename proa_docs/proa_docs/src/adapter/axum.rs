// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, Response, StatusCode},
};

use crate::{render_with_base_path, Docs};

pub fn response(docs: &'static Docs, path: &str) -> Response<Body> {
    response_with_base_path(docs, path, &proa_core::BasePath::default())
}

/// Respond to an unprefixed route while composing shell URLs under `base_path`.
pub fn response_with_base_path(
    docs: &'static Docs,
    path: &str,
    base_path: &proa_core::BasePath,
) -> Response<Body> {
    let docs_response = render_with_base_path(docs, path, base_path);
    Response::builder()
        .status(StatusCode::from_u16(docs_response.status).unwrap_or(StatusCode::OK))
        .header(CONTENT_TYPE, docs_response.content_type)
        .body(Body::from(docs_response.body.into_owned()))
        .expect("valid docs response")
}
