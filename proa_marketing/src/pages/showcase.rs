// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Redirect;

use crate::data::routes;

pub async fn handler() -> Redirect {
    Redirect::permanent(routes::SHOWCASES)
}
