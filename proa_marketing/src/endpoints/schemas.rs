// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Canonical JSON Schemas referenced by generated Proa project files.

use axum::{
    http::header::{CACHE_CONTROL, CONTENT_TYPE},
    response::{IntoResponse, Response},
};

const CONFIG: &str = include_str!("../../../proa_cli/schemas/proa.config.json");
const LOCK: &str = include_str!("../../../proa_cli/schemas/proa.lock.json");
const UI_MANIFEST: &str = include_str!("../../../proa_cli/schemas/ui-component-manifest-v1.json");

fn response(schema: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, "application/schema+json; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        schema,
    )
        .into_response()
}

pub async fn project_config() -> Response {
    response(CONFIG)
}

pub async fn component_lock() -> Response {
    response(LOCK)
}

pub async fn ui_component_manifest() -> Response {
    response(UI_MANIFEST)
}
