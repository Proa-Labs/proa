// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;

mod apps;
mod data;
mod developer_tools;
mod layout;
mod registry;

pub async fn handler() -> Html<String> {
    apps::render("/ecosystem")
}

pub async fn apps_handler() -> Html<String> {
    apps::render("/ecosystem/apps")
}

pub async fn developer_tools_handler() -> Html<String> {
    developer_tools::render()
}

pub async fn registry_handler() -> Html<String> {
    registry::render()
}
