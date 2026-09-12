// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, Response, StatusCode},
};

use crate::{render, Docs};

pub fn response(docs: &'static Docs, path: &str) -> Response<Body> {
    let docs_response = render(docs, path);
    Response::builder()
        .status(StatusCode::from_u16(docs_response.status).unwrap_or(StatusCode::OK))
        .header(CONTENT_TYPE, docs_response.content_type)
        .body(Body::from(docs_response.body.into_owned()))
        .expect("valid docs response")
}
