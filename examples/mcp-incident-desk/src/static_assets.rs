// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::Router;
use proa::axum::PublicAssets;
use tower_http::services::ServeDir;

pub fn mount<S>(router: Router<S>, assets: Option<&PublicAssets>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let router = router.nest_service("/static", service());
    match assets {
        Some(assets) => router.merge(assets.router().with_state(())),
        None => router,
    }
}

pub fn service() -> ServeDir {
    ServeDir::new("public").append_index_html_on_directories(false)
}
