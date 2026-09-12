// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::extract::Path;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub async fn rsjs_runtime() -> Response {
    static_js(rsjs::runtime_assets::CORE_JS)
}

pub async fn rsjs_query_runtime() -> Response {
    static_js(rsjs::runtime_assets::QUERY_JS)
}

pub async fn rsjs_asset(Path(file): Path<String>) -> Response {
    let manifest = rsjs::manifest();

    if let Some(asset_id) = file.strip_suffix(".js") {
        if let Some(island) = manifest
            .islands
            .iter()
            .find(|entry| entry.asset_id == asset_id)
        {
            return static_js(island.js_source);
        }

        if let Some(bundle) = manifest
            .client_modules
            .iter()
            .find(|entry| rsjs::client_module_filename(entry.module_path) == file)
        {
            return owned_js(rsjs::client_module_js_source(bundle));
        }

        if let Some(class) = manifest
            .client_classes
            .iter()
            .find(|entry| rsjs::client_class_filename(entry.module_path) == file)
        {
            return static_js(class.js_source);
        }

        if let Some(fragment) = manifest
            .component_fragments
            .iter()
            .find(|entry| rsjs::manifest::component_fragment_filename(entry.asset_id) == file)
        {
            return static_js(fragment.js_source);
        }
    }

    (StatusCode::NOT_FOUND, "unknown RSJS asset").into_response()
}

fn static_js(body: &'static str) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    response
}

fn owned_js(body: String) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    response
}
