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
use serde_json::json;
use sha2::{Digest, Sha256};
use tower_http::cors::{Any, CorsLayer};

use crate::{pages::docs, utils::site_url::public_site_url};

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
    home: String,
    index: String,
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

fn skill_digest(body: &str) -> String {
    use std::fmt::Write;
    let mut digest = String::with_capacity(71);
    digest.push_str("sha256:");
    for byte in Sha256::digest(body.as_bytes()) {
        write!(digest, "{byte:02x}").expect("write to a String");
    }
    digest
}

fn openapi_document(origin: &str) -> serde_json::Value {
    let mut document: serde_json::Value =
        serde_json::from_str(include_str!("../agent/openapi.json")).expect("valid OpenAPI");
    document["servers"] = json!([{"url":origin}]);
    document["externalDocs"]["url"] = json!(format!("{origin}/docs/api"));
    document["info"]["contact"]["url"] = json!(format!("{origin}/contact"));
    document
}

// proa-perf-ok(scope): immutable discovery documents serialize once per process, outside rendering
fn documents() -> &'static Documents {
    DOCUMENTS.get_or_init(|| {
        let origin = public_site_url();
        let url = |path: &str| format!("{origin}{path}");
        let expand = |source: &str| source.replace("{{ORIGIN}}", origin);
        let mut docs_index = String::from("# Proa documentation\n\n> Public documentation for the Proa Rust web framework. Fetch the linked Markdown for focused context.\n\n## Pages\n\n");
        for page in docs::docs().pages.iter().filter(|page| !page.frontmatter.hidden) {
            use std::fmt::Write;
            writeln!(docs_index, "- [{}]({}): {}", page.title, url(page.markdown_path), page.description.unwrap_or("Proa documentation")).unwrap();
        }
        let entries = vec![
            json!({"identifier":"urn:air:proa.so:mcp:docs", "displayName":"Proa documentation MCP", "type":"application/mcp-server-card+json", "url":url("/mcp/server-card")}),
            json!({"identifier":"urn:air:proa.so:api:docs", "displayName":"Proa documentation API", "type":"application/vnd.oai.openapi+json", "url":url("/openapi.json")}),
            json!({"identifier":"urn:air:proa.so:skills:authoring", "displayName":"Proa authoring skills", "type":"application/json", "url":url("/.well-known/agent-skills/index.json")}),
        ];
        let catalog = json!({"specVersion":"1.0", "host":{"displayName":"Proa Labs, Inc."}, "entries":entries}).to_string();
        let api_catalog = json!({"linkset":[{
            "anchor":url("/.well-known/api-catalog"),
            "item":[{"href":url("/api/v1"),"type":"application/json"}]
        }, {
            "anchor":url("/api/v1"),
            "service-desc":[{"href":url("/openapi.json"),"type":"application/vnd.oai.openapi+json"}],
            "service-doc":[{"href":url("/docs/api"),"type":"text/html"}]
        }]}).to_string();
        let skills = json!({"$schema":"https://schemas.agentskills.io/discovery/0.2.0/schema.json", "skills": SKILLS.iter().map(|(name, description, body)| json!({
            "name":name, "description":description,
            "type":"skill-md",
            "url":url(&format!("/.well-known/agent-skills/{name}/SKILL.md")),
            "digest":skill_digest(body)
        })).collect::<Vec<_>>()}).to_string();
        let site = json!({
            "format":"proa-site", "version":"0.1", "status":"experimental",
            "site":{"name":"Proa", "description":"A Rust web framework for compiled, composable HTML and Markdown rendering with RSJS browser islands.", "homepage":url("/")},
            "interfaces":[
                {"kind":"agent-instructions","url":"/llms.txt","media_type":"text/plain"},
                {"kind":"documentation-corpus","url":"/llms-full.txt","media_type":"text/plain"},
                {"kind":"openapi","url":"/openapi.json","media_type":"application/vnd.oai.openapi+json"},
                {"kind":"mcp","url":"/mcp","transport":"streamable-http"}
            ], "authentication":{"required":false}
        }).to_string();
        let server_card = json!({
            "$schema":"https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json",
            "name":super::mcp::SERVER_NAME, "title":super::mcp::SERVER_TITLE, "version":super::mcp::SERVER_VERSION,
            "description":super::mcp::SERVER_DESCRIPTION,
            "websiteUrl":url("/docs/api"),
            "remotes":[{"type":"streamable-http","url":url("/mcp")}]
        }).to_string();
        let openapi = openapi_document(origin);
        let json_ld = json!({"@context":"https://schema.org", "@graph":[
            {"@type":"Organization", "@id":url("/#organization"), "name":"Proa Labs, Inc.", "url":url("/"),
             "description":"Publisher of Proa, a Rust web framework for composable, multi-target rendering.",
             "logo":url("/static/apple-touch-icon.png"), "sameAs":["https://github.com/proa-labs"],
             "address":{"@type":"PostalAddress","addressCountry":"US"},
             "contactPoint":[{"@type":"ContactPoint","contactType":"privacy","email":"privacy@proa.so"},
                             {"@type":"ContactPoint","contactType":"licensing","email":"legal@proa.so"}]},
            {"@type":"WebSite", "@id":url("/#website"), "name":"Proa", "url":url("/"),
             "description":"Proa Rust framework documentation, examples, and developer tools.",
             "publisher":{"@id":url("/#organization")}, "inLanguage":"en"},
            {"@type":"SoftwareApplication", "@id":url("/#proa"), "name":"Proa", "url":url("/"),
             "description":"A Rust web framework with compiled HTML templates, streaming SSR, typed Markdown, and RSJS browser islands.",
             "applicationCategory":"DeveloperApplication", "operatingSystem":"Linux, macOS, Windows",
             "publisher":{"@id":url("/#organization")}, "sameAs":["https://github.com/proa-labs/proa"]},
            {"@type":"SoftwareSourceCode", "name":"Proa source code", "codeRepository":"https://github.com/proa-labs/proa",
             "programmingLanguage":"Rust", "targetProduct":{"@id":url("/#proa")},
             "license":"https://github.com/proa-labs/proa/blob/main/LICENSING.md"}
        ]}).to_string().replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026");
        Documents {
            home:expand(include_str!("../agent/home.md")), index:expand(include_str!("../agent/index.md")),
            auth:expand(include_str!("../agent/auth.md")), docs_index, catalog, api_catalog, skills,
            site, server_card, openapi:openapi.to_string(), json_ld,
        }
    })
}

pub fn home_markdown() -> &'static str {
    &documents().home
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
    text_response(&documents().index, "text/plain; charset=utf-8")
}
pub async fn docs_index() -> Response {
    text_response(&documents().docs_index, "text/plain; charset=utf-8")
}
pub async fn auth() -> Response {
    text_response(&documents().auth, "text/markdown; charset=utf-8")
}
pub async fn home() -> Response {
    text_response(home_markdown(), "text/markdown; charset=utf-8")
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
