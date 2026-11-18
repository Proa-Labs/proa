// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Startup resolution and serving for the build tool's public-file catalog.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use axum::{
    body::Body,
    extract::{Path as RequestPath, State},
    http::{header, HeaderValue, Method, Request, Response, StatusCode},
    routing::get,
    Router,
};
use bytes::Bytes;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use proa_assets::{AssetCatalog, AssetError, CachePolicy, OUTPUT_ENV, PUBLIC_ROUTE};
use proa_core::{BasePath, PublicAssetUrls};
use thiserror::Error;
use tower::ServiceExt;
use tower_http::services::ServeFile;

const PATH_ENCODING: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
const MAX_REWRITE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum PublicAssetError {
    #[error(transparent)]
    Catalog(#[from] AssetError),
    #[error("could not prepare public asset `{path}`: {reason}")]
    Preparation { path: String, reason: String },
}

/// Load once at startup and attach to the framework builder to serve files
/// and resolve `cx.url("public/...")` in templates.
///
/// The router serves only active manifest entries under `/_proa/assets`.
/// A base path changes outgoing URLs for a prefix-stripping proxy; incoming
/// routes stay rooted at `/`, matching `FrameworkBuilder::with_base_path`.
#[derive(Clone)]
pub struct PublicAssets(Arc<PreparedCatalog>);

struct PreparedCatalog {
    _catalog: AssetCatalog,
    urls: PublicAssetUrls,
    routes: BTreeMap<String, PreparedFile>,
    cache_key: String,
}

#[derive(Clone)]
struct PreparedFile {
    source: Source,
    etag: HeaderValue,
    content_type: HeaderValue,
    immutable: bool,
}

#[derive(Clone)]
enum Source {
    File(ServeFile),
    Rewritten(Bytes),
}

impl PublicAssets {
    /// An explicitly configured missing/corrupt output fails startup.
    pub fn load(output: impl AsRef<Path>, base_path: &BasePath) -> Result<Self, PublicAssetError> {
        Self::from_catalog(AssetCatalog::load(output)?, base_path)
    }

    /// The CLI sets this runtime-only path after a successful build/publication.
    /// `None` means the application has not opted into a configured public dir.
    pub fn from_env(base_path: &BasePath) -> Result<Option<Self>, PublicAssetError> {
        std::env::var_os(OUTPUT_ENV)
            .map(|path| Self::load(path, base_path))
            .transpose()
    }

    pub fn from_catalog(
        catalog: AssetCatalog,
        base_path: &BasePath,
    ) -> Result<Self, PublicAssetError> {
        let mut urls = BTreeMap::new();
        let mut routes = BTreeMap::new();
        let mut rewrite_bytes = 0;
        for (logical, entry) in &catalog.manifest().assets {
            let fail = |reason: String| PublicAssetError::Preparation {
                path: logical.clone(),
                reason,
            };
            let mime: mime::Mime = entry
                .content_type
                .parse()
                .map_err(|e: mime::FromStrError| fail(e.to_string()))?;
            let path = catalog.object_path(logical).expect("manifest entry");
            let extension = logical
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let rewrite = !base_path.as_str().is_empty()
                && entry.cache_policy == CachePolicy::Revalidate
                && matches!(extension.as_str(), "js" | "mjs" | "css");
            let (source, digest) = if rewrite {
                rewrite_bytes += entry.byte_length;
                if rewrite_bytes > MAX_REWRITE_BYTES {
                    return Err(fail(
                        "base-path rewriting exceeds the 32 MiB startup budget".into(),
                    ));
                }
                let input = std::fs::read_to_string(&path).map_err(|e| fail(e.to_string()))?;
                let output = if extension == "css" {
                    crate::asset_urls::rewrite_css(&input, base_path)
                } else {
                    crate::asset_urls::rewrite_js(&input, base_path)
                }
                .map_err(fail)?;
                let digest = proa_assets::sha256(output.as_bytes());
                (Source::Rewritten(Bytes::from(output)), digest)
            } else {
                (
                    Source::File(ServeFile::new_with_mime(path, &mime)),
                    entry.sha256.clone(),
                )
            };
            let file = PreparedFile {
                source,
                etag: format!("\"{digest}\"").parse().expect("SHA-256 header"),
                content_type: entry
                    .content_type
                    .parse()
                    .map_err(|e: axum::http::header::InvalidHeaderValue| fail(e.to_string()))?,
                immutable: false,
            };
            routes
                .entry(logical.clone())
                .or_insert_with(|| file.clone());
            let mut preferred = file;
            preferred.immutable = entry.cache_policy == CachePolicy::Immutable;
            // Manifest validation rejects different bytes claiming one route.
            // Prefer the immutable policy when a generated name is also logical.
            routes
                .entry(entry.public_path.clone())
                .and_modify(|existing| {
                    if preferred.immutable {
                        *existing = preferred.clone();
                    }
                })
                .or_insert(preferred);
            let encoded = utf8_percent_encode(&entry.public_path, PATH_ENCODING);
            urls.insert(
                logical.clone(),
                Arc::from(format!("{}{PUBLIC_ROUTE}/{encoded}", base_path.as_str())),
            );
        }
        let cache_key = proa_assets::sha256(
            format!("{}\n{}", catalog.generation(), base_path.as_str()).as_bytes(),
        );
        Ok(Self(Arc::new(PreparedCatalog {
            _catalog: catalog,
            urls: PublicAssetUrls::new(urls),
            routes,
            cache_key,
        })))
    }

    pub fn url(&self, logical: &str) -> Option<&str> {
        self.0.urls.get(logical)
    }

    /// Share the prepared URL table with a manually constructed render context.
    pub fn render_urls(&self) -> PublicAssetUrls {
        self.0.urls.clone()
    }

    /// Include this in application caches that hold HTML containing asset URLs.
    /// It incorporates both catalog bytes and the startup delivery base path.
    pub fn cache_key(&self) -> &str {
        &self.0.cache_key
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route(&format!("{PUBLIC_ROUTE}/*path"), get(serve))
            .with_state(self.clone())
    }
}

async fn serve(
    State(assets): State<PublicAssets>,
    RequestPath(path): RequestPath<String>,
    mut request: Request<Body>,
) -> Response<Body> {
    let Some(file) = assets.0.routes.get(&path) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let matches = request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .any(|value| {
            value.to_str().is_ok_and(|value| {
                value.split(',').any(|tag| {
                    let tag = tag.trim();
                    tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == file.etag
                })
            })
        });
    let mut response = if matches {
        empty(StatusCode::NOT_MODIFIED)
    } else {
        // Digest validators are authoritative, including edits that preserve
        // mtime. ServeFile still owns streaming, HEAD, and byte-range parsing.
        request.headers_mut().remove(header::IF_NONE_MATCH);
        request.headers_mut().remove(header::IF_MODIFIED_SINCE);
        if let Some(if_range) = request.headers_mut().remove(header::IF_RANGE) {
            if if_range != file.etag {
                request.headers_mut().remove(header::RANGE);
            }
        }
        match &file.source {
            Source::File(service) => service
                .clone()
                .oneshot(request)
                .await
                .unwrap()
                .map(Body::new),
            Source::Rewritten(bytes) => {
                // A server may ignore Range and send the complete representation.
                // Rewritten JS/CSS uses that behavior; media stays on ServeFile.
                let body = if request.method() == Method::HEAD {
                    Body::empty()
                } else {
                    Body::from(bytes.clone())
                };
                let mut response = Response::new(body);
                response
                    .headers_mut()
                    .insert(header::CONTENT_LENGTH, bytes.len().into());
                response
            }
        }
    };
    if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
        let headers = response.headers_mut();
        headers.insert(header::ETAG, file.etag.clone());
        headers.insert(header::CONTENT_TYPE, file.content_type.clone());
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(if file.immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            }),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.remove(header::LAST_MODIFIED);
    }
    response
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests;
