// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

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
    utils::{
        document::{render_markdown, SiteUrl},
        site_url::public_site_url,
    },
};

/// Public metadata can be read by browser discovery clients. The MCP transport
/// is registered separately and keeps its strict Host and Origin validation.
pub fn router() -> Router {
    Router::new()
        .route("/.well-known/proa-site.json", get(site_metadata))
        .route("/openapi.json", get(openapi))
        .route("/llms.txt", get(index))
        .route("/.well-known/llms.txt", get(index))
        .route("/docs/llms.txt", get(docs_index))
        .route("/index.md", get(home))
        .route("/auth.md", get(auth))
        .route("/.well-known/ard.json", get(catalog))
        .route("/.well-known/ai-catalog.json", get(catalog))
        .route("/.well-known/api-catalog", get(api_catalog))
        .route("/.well-known/agent-skills/index.json", get(skills))
        .route("/.well-known/agent-skills/:name/SKILL.md", get(skill))
        .route("/mcp/server-card", get(server_card))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::HEAD])
                .allow_headers([header::CONTENT_TYPE, header::IF_NONE_MATCH])
                .expose_headers([header::ETAG, header::LINK])
                .max_age(Duration::from_secs(300)),
        )
}

pub const DISCOVERY_LINKS: &str = concat!(
    "</.well-known/proa-site.json>; rel=\"service-meta\"; type=\"application/json\", ",
    "</openapi.json>; rel=\"service-desc\"; type=\"application/vnd.oai.openapi+json\", ",
    "</.well-known/ard.json>; rel=\"ard\"; type=\"application/ai-catalog+json\", ",
    "</.well-known/api-catalog>; rel=\"api-catalog\"; type=\"application/linkset+json\", ",
    "</index.md>; rel=\"alternate\"; type=\"text/markdown\", ",
    "</llms.txt>; rel=\"describedby\"; type=\"text/plain\""
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
];

struct Documents {
    auth: String,
    docs_index: String,
    catalog: String,
    api_catalog: String,
    skills: String,
    site: String,
    server_card: String,
    openapi: String,
    json_ld: String,
}

static DOCUMENTS: OnceLock<Documents> = OnceLock::new();

fn openapi_document(origin: &str) -> serde_json::Value {
    #[derive(serde::Serialize)]
    struct Server<'a> {
        url: &'a str,
    }

    // The literal is checked and encoded by const_json! during compilation.
    // Only deployment-specific fields are supplied at startup.
    let mut document: serde_json::Value =
        serde_json::from_str(crate::agent::openapi::DOCUMENT).expect("compiler-validated OpenAPI");
    document["servers"] = serde_json::to_value([Server { url: origin }]).unwrap();
    document["externalDocs"]["url"] = serde_json::to_value(SiteUrl {
        origin,
        path: "/docs/api",
    })
    .unwrap();
    document["info"]["contact"]["url"] = serde_json::to_value(SiteUrl {
        origin,
        path: "/contact",
    })
    .unwrap();
    document
}

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
            }),
            catalog: discovery::catalog(origin),
            api_catalog: discovery::api_catalog(origin),
            skills: discovery::skills(origin, SKILLS),
            site: discovery::site(origin),
            server_card: discovery::server_card(origin),
            openapi: openapi_document(origin).to_string(),
            json_ld: discovery::structured_data(origin),
        }
    })
}

pub fn structured_data() -> &'static str {
    &documents().json_ld
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

pub async fn index() -> Response {
    text_response(home::markdown(), "text/plain; charset=utf-8")
}
pub async fn docs_index() -> Response {
    text_response(&documents().docs_index, "text/plain; charset=utf-8")
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
        axum::http::HeaderValue::from_static(
            "</.well-known/api-catalog>; rel=\"api-catalog\"; type=\"application/linkset+json\"",
        ),
    );
    response
}
pub async fn skills() -> Response {
    text_response(&documents().skills, "application/json; charset=utf-8")
}
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

pub async fn skill(Path(name): Path<String>) -> Response {
    match SKILLS.iter().find(|(skill, _, _)| *skill == name) {
        Some((_, _, body)) => text_response(body, "text/markdown; charset=utf-8"),
        None => super::not_found::markdown(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn openapi_document_uses_the_deployment_origin_for_local_links() {
        let origin = "https://pr-54.example.test";
        let document = super::openapi_document(origin);
        assert_eq!(document["servers"][0]["url"], origin);
        assert_eq!(
            document["externalDocs"]["url"],
            format!("{origin}/docs/api")
        );
        assert_eq!(
            document["info"]["contact"]["url"],
            format!("{origin}/contact")
        );
    }
}
