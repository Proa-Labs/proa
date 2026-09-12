// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use crate::app::AppState;

#[derive(Serialize)]
pub struct StatusBody {
    status: &'static str,
}

pub async fn live() -> Json<StatusBody> {
    Json(StatusBody { status: "ok" })
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(1) => (StatusCode::OK, Json(StatusBody { status: "ready" })),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusBody {
                status: "unavailable",
            }),
        ),
    }
}
