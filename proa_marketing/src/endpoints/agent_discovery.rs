// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use std::time::Duration;

use axum::{
    extract::Path,
    http::{header, Method},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    agent::discovery,
    pages::{
        docs::{self, Authentication, DocumentationIndex},
        home,
    },
    utils::{document::render_markdown, site_url::public_site_url},
};

/// Public metadata can be read by browser discovery clients. The MCP transport
/// is registered separately and keeps its strict Host and Origin validation.
pub fn router() -> Router {
    Router::new()
        .route(routes::SITE_METADATA, get(site_metadata))
        .route(routes::OPENAPI, get(openapi))
        .route(routes::SITE_OPENAPI, get(site_openapi))
        .route(routes::AGENT_INDEX, get(index))
        .route(routes::WELL_KNOWN_AGENT_INDEX, get(index))
        .route(routes::DOCS_INDEX, get(docs_index))
        .route(routes::FRAMEWORK_INDEX, get(framework_index))
        .route(routes::RSJS_INDEX, get(rsjs_index))
        .route(routes::ECOSYSTEM_INDEX, get(ecosystem_index))
        .route(routes::HOME_MARKDOWN, get(home))
        .route(routes::AUTH, get(auth))
        .route(routes::ARD_CATALOG, get(catalog))
        .route(routes::AI_CATALOG, get(catalog))
        .route(routes::API_CATALOG, get(api_catalog))
        .route(routes::SKILLS_INDEX, get(skills))
        .route(routes::SKILL_PATTERN, get(skill))
        .route(routes::MCP_CARD, get(server_card))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::HEAD])
                .allow_headers([header::CONTENT_TYPE, header::IF_NONE_MATCH])
                .expose_headers([header::ETAG, header::LINK])
                .max_age(Duration::from_secs(300)),
        )
}

pub const DISCOVERY_LINKS: &str = const_format::formatcp!(
    "<{}>; rel=\"service-meta\"; type=\"application/json\", <{}>; rel=\"service-desc\"; type=\"application/vnd.oai.openapi+json\", <{}>; rel=\"ard\"; type=\"application/ai-catalog+json\", <{}>; rel=\"api-catalog\"; type=\"application/linkset+json\", <{}>; rel=\"alternate\"; type=\"text/markdown\", <{}>; rel=\"describedby\"; type=\"text/plain\"",
    routes::SITE_METADATA,
    routes::OPENAPI,
    routes::ARD_CATALOG,
    routes::API_CATALOG,
    routes::HOME_MARKDOWN,
    routes::AGENT_INDEX,
);

const SKILLS: &[(&str, &str, &str)] = &[
    (
        "proa-component",
        "Author typed, allocation-conscious Proa Rust render components.",
        include_str!("../../../.agents/skills/proa-component/SKILL.md"),
    ),
    (
        "proa-island",
        "Author analyzed RSJS components and browser islands in Rust.",
        include_str!("../../../.agents/skills/proa-island/SKILL.md"),
    ),
    (
        "proa-perf-review",
        "Review Proa render paths for avoidable allocations and compiler diagnostics.",
        include_str!("../../../.agents/skills/proa-perf-review/SKILL.md"),
    ),
    (
        "proa-docs-api",
        "Retrieve current Proa documentation through the public HTTP API or MCP server.",
        include_str!("../../../.agents/skills/proa-docs-api/SKILL.md"),
    ),
    (
        "proa-markdown",
        "Author typed Markdown and shared HTML/Markdown page content in Proa.",
        include_str!("../../../.agents/skills/proa-markdown/SKILL.md"),
    ),
];

pub fn text_response(body: impl IntoResponse, content_type: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
        .into_response()
}

/// Read the homepage Markdown and links to documentation tools.
///
/// Shares the homepage content with /index.md and negotiated Markdown at /.
#[utoipa::path(get, path = routes::AGENT_INDEX, operation_id = "getAgentIndex",
    responses((status = 200, description = "Homepage Markdown with documentation tool links.", body = String, content_type = "text/plain")))]
pub async fn index() -> Response {
    text_response(home::markdown(), "text/plain; charset=utf-8")
}
pub async fn docs_index() -> Response {
    text_response(
        render_markdown(DocumentationIndex {
            origin: public_site_url(),
            pages: docs::docs().pages,
            section: None,
        }),
        "text/plain; charset=utf-8",
    )
}
pub async fn framework_index() -> Response {
    text_response(
        render_markdown(DocumentationIndex {
            origin: public_site_url(),
            pages: docs::docs().pages,
            section: Some(("Framework", routes::FRAMEWORK_DOCS)),
        }),
        "text/plain; charset=utf-8",
    )
}
pub async fn rsjs_index() -> Response {
    text_response(
        render_markdown(DocumentationIndex {
            origin: public_site_url(),
            pages: docs::docs().pages,
            section: Some(("RSJS", routes::RSJS_DOCS)),
        }),
        "text/plain; charset=utf-8",
    )
}
pub async fn ecosystem_index() -> Response {
    text_response(
        render_markdown(crate::pages::ecosystem::EcosystemIndex),
        "text/plain; charset=utf-8",
    )
}
pub async fn auth() -> Response {
    text_response(
        render_markdown(Authentication {
            origin: public_site_url(),
        }),
        "text/markdown; charset=utf-8",
    )
}
pub async fn home() -> Response {
    text_response(home::markdown(), "text/markdown; charset=utf-8")
}
pub async fn catalog() -> Response {
    text_response(
        discovery::catalog(public_site_url()),
        "application/ai-catalog+json",
    )
}
pub async fn api_catalog() -> Response {
    let mut response = text_response(
        discovery::api_catalog(public_site_url()),
        "application/linkset+json;profile=\"https://www.rfc-editor.org/info/rfc9727\"",
    );
    response.headers_mut().insert(
        header::LINK,
        axum::http::HeaderValue::from_static(const_format::formatcp!(
            "<{}>; rel=\"api-catalog\"; type=\"application/linkset+json\"",
            routes::API_CATALOG
        )),
    );
    response
}
pub async fn skills() -> Response {
    text_response(
        discovery::skills(public_site_url(), SKILLS),
        "application/json; charset=utf-8",
    )
}
/// Describe the public machine-readable interfaces.
///
/// Proa-specific experimental metadata links to the documentation corpus,
/// OpenAPI definition, and MCP transport. No authentication is required.
#[utoipa::path(get, path = routes::SITE_METADATA, operation_id = "getProaSiteMetadata",
    responses((status = 200, description = "Public interface metadata.", body = discovery::SiteMetadata)))]
pub async fn site_metadata() -> Response {
    text_response(
        discovery::site(public_site_url()),
        "application/json; charset=utf-8",
    )
}
pub async fn server_card() -> Response {
    text_response(
        discovery::server_card(public_site_url()),
        "application/mcp-server-card+json; charset=utf-8",
    )
}
pub async fn openapi() -> Response {
    text_response(
        crate::agent::openapi::document(public_site_url())
            .to_json()
            .expect("serialize generated OpenAPI"),
        "application/vnd.oai.openapi+json; charset=utf-8",
    )
}

pub async fn site_openapi() -> Response {
    text_response(
        crate::agent::openapi::website_document(public_site_url())
            .to_json()
            .expect("serialize generated website OpenAPI"),
        "application/vnd.oai.openapi+json; charset=utf-8",
    )
}

pub async fn skill(Path(name): Path<String>) -> Response {
    match SKILLS.iter().find(|(skill, _, _)| *skill == name) {
        Some((_, _, body)) => text_response(*body, "text/markdown; charset=utf-8"),
        None => super::not_found::markdown(),
    }
}
