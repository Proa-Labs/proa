// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! An isolated documentation API backed only by synthetic test fixtures.
//! Added for integration testing and Ora's sandbox/onboarding checks; public
//! production reads are already safe. Do not introduce credentials or writes.

use axum::{
    extract::Path,
    http::{header, HeaderValue},
    middleware,
    routing::get,
    Json, Router,
};
use proa_docs::{DocPage, Frontmatter};
use serde::Serialize;
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::ToSchema;

use super::{ApiError, Arc, Mutex, RequestBudget, Source};
use crate::data::routes;

const FRONTMATTER: Frontmatter = Frontmatter {
    title: None,
    description: None,
    icon: None,
    slug: None,
    sidebar_title: None,
    nav_order: None,
    hidden: false,
    draft: false,
    search: None,
    toc: None,
    last_modified: None,
};

const fn fixture(
    slug: &'static str,
    route_path: &'static str,
    markdown_path: &'static str,
    title: &'static str,
    markdown: &'static str,
) -> DocPage {
    DocPage {
        slug,
        route_path,
        markdown_path,
        title,
        markdown,
        description: Some("Synthetic fixture for documentation API integration tests."),
        frontmatter: &FRONTMATTER,
        article_html: "",
        toc: &[],
        prev: None,
        next: None,
    }
}

pub(super) const PAGES: &[DocPage] = &[
    fixture(
        "fixture-alpha",
        "/sandbox/docs/fixture-alpha",
        "/sandbox/docs/fixture-alpha.md",
        "Fixture alpha",
        include_str!("fixtures/alpha.md"),
    ),
    fixture(
        "fixture-pagination",
        "/sandbox/docs/fixture-pagination",
        "/sandbox/docs/fixture-pagination.md",
        "Fixture pagination",
        include_str!("fixtures/pagination.md"),
    ),
    fixture(
        "fixture-escaping",
        "/sandbox/docs/fixture-escaping",
        "/sandbox/docs/fixture-escaping.md",
        "Fixture escaping & Unicode",
        include_str!("fixtures/escaping.md"),
    ),
];

#[derive(Serialize, ToSchema)]
pub struct SandboxInfo {
    environment: &'static str,
    synthetic_data: bool,
    authentication_required: bool,
    api_base_url: String,
    openapi_url: String,
    example_url: String,
    documentation_url: String,
}

pub fn router() -> Router {
    let budget = Arc::new(Mutex::new(RequestBudget::default()));
    Router::new()
        .route("/", get(info))
        .route("/health", get(info))
        .route(routes::OPENAPI, get(openapi))
        .route(routes::SITE_METADATA, get(info))
        .route("/docs/:slug", get(markdown))
        .merge(super::router(Source::Sandbox))
        .layer(middleware::from_fn_with_state(budget, super::rate_limit))
        .layer(SetResponseHeaderLayer::overriding(
            "x-proa-environment".parse().unwrap(),
            HeaderValue::from_static("sandbox"),
        ))
}

pub async fn info() -> Json<SandboxInfo> {
    let origin = crate::utils::site_url::public_site_url();
    Json(SandboxInfo {
        environment: "sandbox",
        synthetic_data: true,
        authentication_required: false,
        api_base_url: format!("{origin}{}", routes::SANDBOX),
        openapi_url: format!("{origin}{}{}", routes::SANDBOX, routes::OPENAPI),
        example_url: format!(
            "{origin}{}{}?q=fixture&limit=1",
            routes::SANDBOX,
            routes::API_SEARCH
        ),
        documentation_url: format!("{origin}{}#sandbox", routes::API_GUIDE),
    })
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(crate::agent::openapi::sandbox_document(
        crate::utils::site_url::public_site_url(),
    ))
}

async fn markdown(Path(slug): Path<String>) -> Result<impl axum::response::IntoResponse, ApiError> {
    let path = format!("/sandbox/docs/{slug}");
    let page = super::page_from(Source::Sandbox, &path)?;
    Ok((
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        page.markdown,
    ))
}
