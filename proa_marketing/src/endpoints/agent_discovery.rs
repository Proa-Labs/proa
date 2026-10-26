// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use axum::{
    extract::Path,
    http::header,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{pages::docs, utils::site_url::public_site_url};

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
            "item":[{"href":url("/openapi.json"),"type":"application/vnd.oai.openapi+json"}]
        }, {
            "anchor":url("/api/v1"),
            "service-desc":[{"href":url("/openapi.json"),"type":"application/vnd.oai.openapi+json"}],
            "service-doc":[{"href":url("/docs/api"),"type":"text/html"}]
        }]}).to_string();
        let skills = json!({"skills": SKILLS.iter().map(|(name, description, _)| json!({
            "name":name, "description":description,
            "url":url(&format!("/.well-known/agent-skills/{name}/SKILL.md")),
            "files":["SKILL.md"]
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
            "name":"so.proa/docs", "title":"Proa documentation", "version":"1.0.0",
            "description":"Search and read official Proa Rust framework documentation.",
            "websiteUrl":url("/docs/api"),
            "remotes":[{"type":"streamable-http","url":url("/mcp")}]
        }).to_string();
        let mut openapi: serde_json::Value = serde_json::from_str(include_str!("../agent/openapi.json")).expect("valid OpenAPI");
        openapi["servers"] = json!([{"url":origin}]);
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
    text_response(
        &documents().api_catalog,
        "application/linkset+json;profile=\"https://www.rfc-editor.org/info/rfc9727\"",
    )
}
pub async fn skills() -> Response {
    text_response(&documents().skills, "application/json; charset=utf-8")
}
pub async fn site_metadata() -> Response {
    text_response(&documents().site, "application/json; charset=utf-8")
}
pub async fn server_card() -> Response {
    text_response(&documents().server_card, "application/json; charset=utf-8")
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
