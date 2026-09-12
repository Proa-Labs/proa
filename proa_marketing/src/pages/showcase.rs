// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Redirect;

pub async fn handler() -> Redirect {
    Redirect::permanent("/ecosystem/apps#reference-showcases")
}
