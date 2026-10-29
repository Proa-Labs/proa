// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Public ownership proof for IndexNow search-engine notifications.
//! This is deliberately a public verification file, not an API credential.

pub async fn key() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        include_str!("../../indexnow-key.txt"),
    )
}
