// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Linked RSJS assets with deployment-specific runtime imports.

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, LegalComment};
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use proa_core::BasePath;

#[derive(Debug, thiserror::Error)]
#[error("could not prepare RSJS JavaScript `{asset}`: {reason}")]
pub struct RsjsAssetsError {
    asset: String,
    reason: String,
}

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
    try_rsjs_assets_router(base_path).expect("could not prepare linked RSJS assets")
}

/// Prepare browser JavaScript using the setting selected by `cargo proa`.
pub fn try_rsjs_assets_router(base_path: &BasePath) -> Result<Router, RsjsAssetsError> {
    prepare_router(
        base_path,
        matches!(option_env!("PROA_RSJS_MINIFY"), Some("1")),
    )
}

fn prepare_router(base_path: &BasePath, minify: bool) -> Result<Router, RsjsAssetsError> {
    let runtime_url = base_path.url("/rsjs-runtime.js").to_string();
    let query_url = base_path.url("/rsjs-runtime-query.js").to_string();
    let finalize = |name: &str, source: Cow<'static, str>| {
        if minify {
            return minify_module(&source)
                .map(Bytes::from)
                .map_err(|reason| RsjsAssetsError {
                    asset: name.into(),
                    reason,
                });
        }
        Ok(match source {
            Cow::Borrowed(source) => Bytes::from_static(source.as_bytes()),
            Cow::Owned(source) => Bytes::from(source),
        })
    };
    let module = |name: &str, source: Cow<'static, str>| {
        let source =
            if base_path.as_str().is_empty() {
                source
            } else {
                let imports =
                    rsjs::rewrite_runtime_import_specifiers(&source, &runtime_url, &query_url);
                Cow::Owned(crate::asset_urls::rewrite_js(&imports, base_path).map_err(
                    |reason| RsjsAssetsError {
                        asset: name.into(),
                        reason,
                    },
                )?)
            };
        finalize(name, source)
    };

    let manifest = rsjs::manifest();
    let mut assets = HashMap::new();
    for entry in &manifest.islands {
        let key = format!("{}.js", entry.asset_id);
        let source = module(&key, Cow::Borrowed(entry.js_source))?;
        assets.insert(key, source);
    }
    for entry in &manifest.client_modules {
        let key = rsjs::client_module_filename(entry.module_path);
        let source = module(&key, Cow::Owned(rsjs::client_module_js_source(entry)))?;
        assets.insert(key, source);
    }
    for entry in &manifest.client_classes {
        let key = rsjs::client_class_filename(entry.module_path);
        let source = module(&key, Cow::Borrowed(entry.js_source))?;
        assets.insert(key, source);
    }
    for entry in &manifest.component_fragments {
        let key = rsjs::manifest::component_fragment_filename(entry.asset_id);
        let source = module(&key, Cow::Borrowed(entry.js_source))?;
        assets.insert(key, source);
    }
    let assets = Arc::new(assets);
    let runtime = finalize(
        "rsjs-runtime.js",
        rsjs::runtime_assets::with_base_path(rsjs::runtime_assets::CORE_JS, base_path),
    )?;
    let query = finalize(
        "rsjs-runtime-query.js",
        rsjs::runtime_assets::with_base_path(rsjs::runtime_assets::QUERY_JS, base_path),
    )?;
    Ok(Router::new()
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
        ))
}

fn minify_module(source: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let mut parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }

    // Keep diagnostic names and explicit debugger/console statements. The
    // current Oxc mangler's keep-names pass also panics on runtime patterns.
    Minifier::new(MinifierOptions {
        compress: Some(CompressOptions::safest()),
        mangle: None,
    })
    .build(&allocator, &mut parsed.program);
    let codegen = CodegenOptions {
        legal_comments: LegalComment::Inline,
        ..CodegenOptions::minify()
    };
    Ok(Codegen::new()
        .with_options(codegen)
        .build(&parsed.program)
        .code)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt;

    static ISLAND: rsjs::IslandManifestEntry = rsjs::IslandManifestEntry {
        asset_id: "minify_test",
        struct_name: "MinifyTest",
        js_source: r#"
            import { signal } from "/rsjs-runtime.js";
            import { mount } from "./child.js";

            export function hydrate(root) {
                const currentValue = signal(root.dataset.initialValue);
                mount(root.querySelector("[data-rsjs-bind]"), currentValue);
            }
        "#,
        is_async: false,
        hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    #[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
    #[linkme(crate = rsjs::linkme)]
    static REGISTER_ISLAND: &'static rsjs::IslandManifestEntry = &ISLAND;

    async fn source(router: &Router, path: &str) -> String {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn minifies_served_rsjs_and_preserves_exports_and_import_urls() {
        let prefix = BasePath::new("/preview").unwrap();
        let original = prepare_router(&prefix, false).unwrap();
        let minified = prepare_router(&prefix, true).unwrap();

        for (path, export) in [
            ("/rsjs-runtime.js", "app_url"),
            ("/rsjs-runtime-query.js", "app_url"),
            ("/rsjs/island/minify_test.js", "hydrate"),
        ] {
            let plain = source(&original, path).await;
            let compact = source(&minified, path).await;
            assert!(compact.len() < plain.len(), "{path}");
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &compact, SourceType::mjs()).parse();
            assert!(parsed.errors.is_empty(), "{path}: {:?}", parsed.errors);
            assert!(
                compact.contains(&format!("export function {export}(")),
                "{path}"
            );
            assert!(compact.contains("/preview"), "{path}");
            if path.ends_with("minify_test.js") {
                assert!(compact.contains("/preview/rsjs-runtime.js"));
                assert!(compact.contains("./child.js"));
                assert!(compact.contains("[data-rsjs-bind]"));
            }
        }
    }
}
