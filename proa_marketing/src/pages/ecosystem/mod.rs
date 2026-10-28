// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use axum::response::Html;

mod apps;
mod data;
mod developer_tools;
mod layout;
mod registry;

pub async fn handler() -> Html<String> {
    apps::render(routes::ECOSYSTEM)
}

pub async fn apps_handler() -> Html<String> {
    apps::render(routes::APPS)
}

pub async fn developer_tools_handler() -> Html<String> {
    developer_tools::render()
}

pub async fn registry_handler() -> Html<String> {
    registry::render()
}
