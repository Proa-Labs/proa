// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A directory of public interfaces, not their API implementation.
//!
//! - `catalog`: ARD clients find the OpenAPI document, MCP card, and skills.
//! - `api_catalog`: RFC 9727 clients find the API and its documentation.
//! - `skills`: clients discover authoring instructions and verify their digests.
//! - `site`: Proa's experimental overview of its machine-readable interfaces.
//! - `server_card`: MCP clients discover the transport and server identity.
//!
//! The endpoint module caches these documents once per deployment. Typed Serde
//! models escape preview origins correctly. REST/tool schemas come from the
//! request/response types; HTML JSON-LD belongs to `layout::structured_data`.

use crate::data::{routes, site};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{endpoints::mcp, utils::document::SiteUrl};

fn encode(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("serialize a typed discovery document")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Catalog<'a> {
    spec_version: &'static str,
    host: CatalogHost,
    entries: [CatalogEntry<'a>; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogHost {
    display_name: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry<'a> {
    identifier: &'static str,
    display_name: &'static str,
    #[serde(rename = "type")]
    media_type: &'static str,
    url: SiteUrl<'a>,
}

pub fn catalog(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    encode(&Catalog {
        spec_version: "1.0",
        host: CatalogHost {
            display_name: site::ORGANIZATION,
        },
        entries: [
            CatalogEntry {
                identifier: "urn:air:proa.so:mcp:docs",
                display_name: "Proa documentation MCP",
                media_type: "application/mcp-server-card+json",
                url: url(routes::MCP_CARD),
            },
            CatalogEntry {
                identifier: "urn:air:proa.so:api:docs",
                display_name: "Proa documentation API",
                media_type: "application/vnd.oai.openapi+json",
                url: url(routes::OPENAPI),
            },
            CatalogEntry {
                identifier: "urn:air:proa.so:skills:authoring",
                display_name: "Proa authoring skills",
                media_type: "application/json",
                url: url(routes::SKILLS_INDEX),
            },
        ],
    })
}

#[derive(Serialize)]
struct ApiCatalog<'a> {
    linkset: (CatalogLinks<'a>, ApiLinks<'a>),
}

#[derive(Serialize)]
struct CatalogLinks<'a> {
    anchor: SiteUrl<'a>,
    item: [Link<'a>; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct ApiLinks<'a> {
    anchor: SiteUrl<'a>,
    service_desc: [Link<'a>; 1],
    service_doc: [Link<'a>; 1],
}

#[derive(Serialize)]
struct Link<'a> {
    href: SiteUrl<'a>,
    #[serde(rename = "type")]
    media_type: &'static str,
}

pub fn api_catalog(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    encode(&ApiCatalog {
        linkset: (
            CatalogLinks {
                anchor: url(routes::API_CATALOG),
                item: [Link {
                    href: url(routes::API_V1),
                    media_type: "application/json",
                }],
            },
            ApiLinks {
                anchor: url(routes::API_V1),
                service_desc: [Link {
                    href: url(routes::OPENAPI),
                    media_type: "application/vnd.oai.openapi+json",
                }],
                service_doc: [Link {
                    href: url(routes::API_GUIDE),
                    media_type: "text/html",
                }],
            },
        ),
    })
}

#[derive(Serialize)]
struct SkillsIndex<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    skills: Vec<SkillEntry<'a>>,
}

#[derive(Serialize)]
struct SkillEntry<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    url: String,
    digest: String,
}

fn skill_digest(body: &str) -> String {
    use std::fmt::Write;
    let mut digest = String::with_capacity(71);
    digest.push_str("sha256:");
    for byte in Sha256::digest(body.as_bytes()) {
        write!(digest, "{byte:02x}").expect("write digest to a String");
    }
    digest
}

pub fn skills(origin: &str, sources: &[(&str, &str, &str)]) -> String {
    encode(&SkillsIndex {
        schema: "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
        skills: sources
            .iter()
            .map(|(name, description, body)| SkillEntry {
                name,
                description,
                kind: "skill-md",
                url: format!("{origin}{}/{name}/SKILL.md", routes::SKILLS_ROOT),
                digest: skill_digest(body),
            })
            .collect(),
    })
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SiteMetadata<'a> {
    format: &'static str,
    version: &'static str,
    status: &'static str,
    site: Site<'a>,
    interfaces: [Interface; 4],
    authentication: Authentication,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Site<'a> {
    name: &'static str,
    description: &'static str,
    #[schema(value_type = String, format = "uri")]
    homepage: SiteUrl<'a>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Interface {
    kind: &'static str,
    url: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<&'static str>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Authentication {
    required: bool,
}

pub fn site(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    encode(&SiteMetadata {
        format: "proa-site",
        version: "0.1",
        status: "experimental",
        site: Site {
            name: site::NAME,
            description: "A Rust web framework for compiled, composable HTML and Markdown rendering with RSJS browser islands.",
            homepage: url(routes::HOME),
        },
        interfaces: [
            Interface {
                kind: "agent-instructions",
                url: routes::AGENT_INDEX,
                media_type: Some("text/plain"),
                transport: None,
            },
            Interface {
                kind: "documentation-corpus",
                url: routes::DOCS_CORPUS,
                media_type: Some("text/plain"),
                transport: None,
            },
            Interface {
                kind: "openapi",
                url: routes::OPENAPI,
                media_type: Some("application/vnd.oai.openapi+json"),
                transport: None,
            },
            Interface {
                kind: "mcp",
                url: routes::MCP,
                media_type: None,
                transport: Some("streamable-http"),
            },
        ],
        authentication: Authentication { required: false },
    })
}

#[derive(Serialize)]
struct ServerCard<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    #[serde(flatten)]
    identity: rmcp::model::Implementation,
    remotes: [Remote<'a>; 1],
}

#[derive(Serialize)]
struct Remote<'a> {
    #[serde(rename = "type")]
    transport: &'static str,
    url: SiteUrl<'a>,
}

pub fn server_card(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    encode(&ServerCard {
        schema: "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json",
        identity: mcp::server_info(origin),
        remotes: [Remote {
            transport: "streamable-http",
            url: url(routes::MCP),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_urls_are_json_escaped() {
        let origin = "https://preview.example/\"</script>&";
        let card: serde_json::Value = serde_json::from_str(&server_card(origin)).unwrap();
        assert_eq!(card["remotes"][0]["url"], format!("{origin}/mcp"));
    }
}
