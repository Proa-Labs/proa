// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Linked RSJS assets with deployment-specific runtime imports.

use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use proa_core::BasePath;

/// Build unprefixed routes for the linked RSJS runtime and generated modules.
///
/// A reverse proxy strips the public `base_path` before forwarding requests.
/// Generated imports use that public prefix. All source rewriting happens once
/// here, at startup, never by buffering a streamed HTML response. Dependency
/// imports between generated modules remain relative to their module URL.
///
/// Use this for custom Axum routers with `RouteContextLayer::with_base_path`,
/// or let `FrameworkBuilder::with_rsjs_assets` install these routes.
pub fn rsjs_assets_router(base_path: &BasePath) -> Router {
    let runtime_url = base_path.url("/rsjs-runtime.js").to_string();
    let query_url = base_path.url("/rsjs-runtime-query.js").to_string();
    let rewrite = |source: &'static str| {
        if base_path.as_str().is_empty() {
            Bytes::from_static(source.as_bytes())
        } else {
            Bytes::from(
                crate::asset_urls::rewrite_js(
                    &rsjs::rewrite_runtime_import_specifiers(source, &runtime_url, &query_url),
                    base_path,
                )
                .expect("valid linked RSJS module"),
            )
        }
    };
    let manifest = rsjs::manifest();
    let mut assets = HashMap::new();
    for entry in &manifest.islands {
        assets.insert(format!("{}.js", entry.asset_id), rewrite(entry.js_source));
    }
    for entry in &manifest.client_modules {
        let source = rsjs::client_module_js_source(entry);
        let source = if base_path.as_str().is_empty() {
            Bytes::from(source)
        } else {
            Bytes::from(
                crate::asset_urls::rewrite_js(
                    &rsjs::rewrite_runtime_import_specifiers(&source, &runtime_url, &query_url),
                    base_path,
                )
                .expect("valid linked RSJS client module"),
            )
        };
        assets.insert(rsjs::client_module_filename(entry.module_path), source);
    }
    for entry in &manifest.client_classes {
        assets.insert(
            rsjs::client_class_filename(entry.module_path),
            rewrite(entry.js_source),
        );
    }
    for entry in &manifest.component_fragments {
        assets.insert(
            rsjs::manifest::component_fragment_filename(entry.asset_id),
            rewrite(entry.js_source),
        );
    }
    let assets = Arc::new(assets);
    let configure_runtime = |source: &'static str| {
        if base_path.as_str().is_empty() {
            Bytes::from_static(source.as_bytes())
        } else {
            Bytes::from(rsjs::runtime_assets::with_base_path(source, base_path).into_owned())
        }
    };
    let runtime = configure_runtime(rsjs::runtime_assets::CORE_JS);
    let query = configure_runtime(rsjs::runtime_assets::QUERY_JS);
    Router::new()
        .route(
            "/rsjs-runtime.js",
            get(move || async move { javascript(runtime) }),
        )
        .route(
            "/rsjs-runtime-query.js",
            get(move || async move { javascript(query) }),
        )
        .route(
            "/rsjs/island/:file",
            get(move |Path(file): Path<String>| {
                let assets = assets.clone();
                async move {
                    match assets.get(&file) {
                        Some(source) => javascript(source.clone()),
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }),
        )
}

fn javascript(source: Bytes) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        source,
    )
        .into_response()
}
