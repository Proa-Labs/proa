// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{app::AppState, auth, endpoints, pages, static_assets};
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use proa_framework_axum::{router_from_spec, IslandManifest, Route, RouteContextLayer};
use std::{collections::HashMap, sync::Arc};
pub fn router(state: AppState) -> Router {
    let (router, metadata) = router_from_spec(vec![
        // proa:route-specs:start
        Route::endpoint("/", get(pages::home::handler)),
        Route::endpoint("/issues", get(pages::issues)),
        Route::endpoint("/issues/:id", get(pages::issue)),
        Route::endpoint("/issues/:id/resolve", post(pages::resolve)),
        Route::endpoint("/runbooks/:id", get(pages::runbook)),
        Route::endpoint("/connections", get(pages::connections)),
        Route::endpoint("/connections/revoke", post(pages::revoke)),
        Route::endpoint("/connect", get(pages::connect)),
        Route::endpoint("/lab", get(pages::lab)),
        // proa:route-specs:end
    ]);
    let manifest = Arc::new(IslandManifest {
        islands: HashMap::new(),
        assets: Default::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    });
    let mcp = Router::new()
        .nest_service("/mcp", crate::mcp::service(state.clone()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::mcp::authenticate,
        ));
    router
        .route("/healthz", get(endpoints::health::live))
        .route("/readyz", get(endpoints::health::ready))
        // proa:explicit-routes:start
        .route("/auth/login", get(auth::browser::login))
        .route("/auth/callback", get(auth::browser::callback))
        .route("/auth/logout", post(auth::browser::logout))
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(auth::provider::resource),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(auth::provider::resource),
        )
        // proa:explicit-routes:end
        .merge(mcp)
        .nest_service("/static", static_assets::service())
        .with_state(state.clone())
        .layer(RouteContextLayer::new(manifest, metadata))
        .layer(DefaultBodyLimit::max(16384))
        .layer(middleware::from_fn_with_state(
            state.config.app_origin.clone(),
            boundary,
        ))
}
pub fn issuer(state: AppState) -> Router {
    let expected = state.config.issuer.clone();
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(auth::provider::metadata),
        )
        .route(
            "/.well-known/openid-configuration",
            get(auth::provider::metadata),
        )
        .route("/jwks.json", get(auth::provider::jwks))
        .route("/register", post(auth::provider::register))
        .route("/authorize", get(auth::provider::authorize))
        .route("/authorize/login", post(auth::provider::login))
        .route("/consent", post(auth::provider::consent))
        .route("/token", post(auth::provider::token))
        .route("/revoke", post(auth::provider::revoke))
        .nest_service("/static", static_assets::service())
        .with_state(state)
        .layer(DefaultBodyLimit::max(16384))
        .layer(middleware::from_fn_with_state(expected, boundary))
}
async fn boundary(
    State(origin): State<String>,
    request: Request,
    next: middleware::Next,
) -> Response {
    let host = origin.strip_prefix("https://").unwrap_or("");
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| request.uri().authority().map(|a| a.as_str()))
        != Some(host)
    {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v != origin)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut r = next.run(request).await;
    r.headers_mut().entry("content-security-policy").or_insert("default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    r.headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    r.headers_mut()
        // `no-referrer` makes native form POSTs send Origin: null, failing CSRF checks.
        .insert("referrer-policy", "same-origin".parse().unwrap());
    r
}
