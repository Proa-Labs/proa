// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use std::{sync::OnceLock, time::Duration};

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

struct Documents {
    auth: String,
    docs_index: String,
    framework_index: String,
    rsjs_index: String,
    ecosystem_index: String,
    catalog: String,
    api_catalog: String,
    skills: String,
    site: String,
    server_card: String,
    openapi: String,
    site_openapi: String,
}

static DOCUMENTS: OnceLock<Documents> = OnceLock::new();

// The response/cache boundary owns the finished documents. Renderers compose
// typed children; serialization and rendering happen once per deployment.
fn documents() -> &'static Documents {
    DOCUMENTS.get_or_init(|| {
        let origin = public_site_url();
        Documents {
            auth: render_markdown(Authentication { origin }),
            docs_index: render_markdown(DocumentationIndex {
                origin,
                pages: docs::docs().pages,
                section: None,
            }),
            framework_index: render_markdown(DocumentationIndex {
                origin,
                pages: docs::docs().pages,
                section: Some(("Framework", routes::FRAMEWORK_DOCS)),
            }),
            rsjs_index: render_markdown(DocumentationIndex {
                origin,
                pages: docs::docs().pages,
                section: Some(("RSJS", routes::RSJS_DOCS)),
            }),
            ecosystem_index: render_markdown(crate::pages::ecosystem::EcosystemIndex),
            catalog: discovery::catalog(origin),
            api_catalog: discovery::api_catalog(origin),
            skills: discovery::skills(origin, SKILLS),
            site: discovery::site(origin),
            server_card: discovery::server_card(origin),
            openapi: crate::agent::openapi::document(origin)
                .to_json()
                .expect("serialize generated OpenAPI"),
            site_openapi: crate::agent::openapi::website_document(origin)
                .to_json()
                .expect("serialize generated website OpenAPI"),
        }
    })
}

pub fn text_response(body: &'static str, content_type: &'static str) -> Response {
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
    text_response(&documents().docs_index, "text/plain; charset=utf-8")
}
pub async fn framework_index() -> Response {
    text_response(&documents().framework_index, "text/plain; charset=utf-8")
}
pub async fn rsjs_index() -> Response {
    text_response(&documents().rsjs_index, "text/plain; charset=utf-8")
}
pub async fn ecosystem_index() -> Response {
    text_response(&documents().ecosystem_index, "text/plain; charset=utf-8")
}
pub async fn auth() -> Response {
    text_response(&documents().auth, "text/markdown; charset=utf-8")
}
pub async fn home() -> Response {
    text_response(home::markdown(), "text/markdown; charset=utf-8")
}
pub async fn catalog() -> Response {
    text_response(&documents().catalog, "application/ai-catalog+json")
}
pub async fn api_catalog() -> Response {
    let mut response = text_response(
        &documents().api_catalog,
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
    text_response(&documents().skills, "application/json; charset=utf-8")
}
/// Describe the public machine-readable interfaces.
///
/// Proa-specific experimental metadata links to the documentation corpus,
/// OpenAPI definition, and MCP transport. No authentication is required.
#[utoipa::path(get, path = routes::SITE_METADATA, operation_id = "getProaSiteMetadata",
    responses((status = 200, description = "Public interface metadata.", body = discovery::SiteMetadata)))]
pub async fn site_metadata() -> Response {
    text_response(&documents().site, "application/json; charset=utf-8")
}
pub async fn server_card() -> Response {
    text_response(
        &documents().server_card,
        "application/mcp-server-card+json; charset=utf-8",
    )
}
pub async fn openapi() -> Response {
    text_response(
        &documents().openapi,
        "application/vnd.oai.openapi+json; charset=utf-8",
    )
}

pub async fn site_openapi() -> Response {
    text_response(
        &documents().site_openapi,
        "application/vnd.oai.openapi+json; charset=utf-8",
    )
}

pub async fn skill(Path(name): Path<String>) -> Response {
    match SKILLS.iter().find(|(skill, _, _)| *skill == name) {
        Some((_, _, body)) => text_response(body, "text/markdown; charset=utf-8"),
        None => super::not_found::markdown(),
    }
}
