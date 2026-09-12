// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use include_dir::Dir;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;
use tracing::debug;

/// Infer a MIME type from a file extension. Falls back to
/// `application/octet-stream` for unknown extensions so browsers don't
/// MIME-sniff and possibly execute content as HTML/JS.
fn content_type_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// A simple static asset service backed by an embedded directory.
pub struct EmbeddedStaticService {
    dir: Dir<'static>,
}

impl EmbeddedStaticService {
    pub const fn new(dir: Dir<'static>) -> Self {
        Self { dir }
    }
}

impl Service<Request<Body>> for EmbeddedStaticService {
    type Response = Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let path = req.uri().path().trim_start_matches('/');
        let file = if path.is_empty() { "index.html" } else { path };
        let response = match self.dir.get_file(file) {
            Some(f) => {
                debug!("serving embedded static file: {}", file);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, content_type_for(file))
                    .header("X-Content-Type-Options", "nosniff")
                    .body(Body::from(f.contents().to_vec()))
                    .unwrap()
            }
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        };
        Box::pin(async move { Ok(response) })
    }
}
