// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed serializers for discovery documents containing deployment-specific data.
//! Literal-only JSON schemas live in `schemas` and `openapi` as `const_json!`.

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
    encode(&Catalog {
        spec_version: "1.0",
        host: CatalogHost {
            display_name: "Proa Labs, Inc.",
        },
        entries: [
            CatalogEntry {
                identifier: "urn:air:proa.so:mcp:docs",
                display_name: "Proa documentation MCP",
                media_type: "application/mcp-server-card+json",
                url: SiteUrl {
                    origin,
                    path: "/mcp/server-card",
                },
            },
            CatalogEntry {
                identifier: "urn:air:proa.so:api:docs",
                display_name: "Proa documentation API",
                media_type: "application/vnd.oai.openapi+json",
                url: SiteUrl {
                    origin,
                    path: "/openapi.json",
                },
            },
            CatalogEntry {
                identifier: "urn:air:proa.so:skills:authoring",
                display_name: "Proa authoring skills",
                media_type: "application/json",
                url: SiteUrl {
                    origin,
                    path: "/.well-known/agent-skills/index.json",
                },
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
    encode(&ApiCatalog {
        linkset: (
            CatalogLinks {
                anchor: SiteUrl {
                    origin,
                    path: "/.well-known/api-catalog",
                },
                item: [Link {
                    href: SiteUrl {
                        origin,
                        path: "/api/v1",
                    },
                    media_type: "application/json",
                }],
            },
            ApiLinks {
                anchor: SiteUrl {
                    origin,
                    path: "/api/v1",
                },
                service_desc: [Link {
                    href: SiteUrl {
                        origin,
                        path: "/openapi.json",
                    },
                    media_type: "application/vnd.oai.openapi+json",
                }],
                service_doc: [Link {
                    href: SiteUrl {
                        origin,
                        path: "/docs/api",
                    },
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
                url: format!("{origin}/.well-known/agent-skills/{name}/SKILL.md"),
                digest: skill_digest(body),
            })
            .collect(),
    })
}

#[derive(Serialize)]
struct SiteMetadata<'a> {
    format: &'static str,
    version: &'static str,
    status: &'static str,
    site: Site<'a>,
    interfaces: [Interface; 4],
    authentication: Authentication,
}

#[derive(Serialize)]
struct Site<'a> {
    name: &'static str,
    description: &'static str,
    homepage: SiteUrl<'a>,
}

#[derive(Serialize)]
struct Interface {
    kind: &'static str,
    url: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<&'static str>,
}

#[derive(Serialize)]
struct Authentication {
    required: bool,
}

pub fn site(origin: &str) -> String {
    encode(&SiteMetadata {
        format: "proa-site",
        version: "0.1",
        status: "experimental",
        site: Site {
            name: "Proa",
            description: "A Rust web framework for compiled, composable HTML and Markdown rendering with RSJS browser islands.",
            homepage: SiteUrl { origin, path: "/" },
        },
        interfaces: [
            Interface {
                kind: "agent-instructions",
                url: "/llms.txt",
                media_type: Some("text/plain"),
                transport: None,
            },
            Interface {
                kind: "documentation-corpus",
                url: "/llms-full.txt",
                media_type: Some("text/plain"),
                transport: None,
            },
            Interface {
                kind: "openapi",
                url: "/openapi.json",
                media_type: Some("application/vnd.oai.openapi+json"),
                transport: None,
            },
            Interface {
                kind: "mcp",
                url: "/mcp",
                media_type: None,
                transport: Some("streamable-http"),
            },
        ],
        authentication: Authentication { required: false },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerCard<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    name: &'static str,
    title: &'static str,
    version: &'static str,
    description: &'static str,
    website_url: SiteUrl<'a>,
    remotes: [Remote<'a>; 1],
}

#[derive(Serialize)]
struct Remote<'a> {
    #[serde(rename = "type")]
    transport: &'static str,
    url: SiteUrl<'a>,
}

pub fn server_card(origin: &str) -> String {
    encode(&ServerCard {
        schema: "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json",
        name: mcp::SERVER_NAME,
        title: mcp::SERVER_TITLE,
        version: mcp::SERVER_VERSION,
        description: mcp::SERVER_DESCRIPTION,
        website_url: SiteUrl {
            origin,
            path: "/docs/api",
        },
        remotes: [Remote {
            transport: "streamable-http",
            url: SiteUrl {
                origin,
                path: "/mcp",
            },
        }],
    })
}

#[derive(Serialize)]
struct StructuredData<'a> {
    #[serde(rename = "@context")]
    context: &'static str,
    #[serde(rename = "@graph")]
    graph: (
        Organization<'a>,
        Website<'a>,
        Application<'a>,
        SourceCode<'a>,
    ),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Organization<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    #[serde(rename = "@id")]
    id: SiteUrl<'a>,
    name: &'static str,
    url: SiteUrl<'a>,
    description: &'static str,
    logo: SiteUrl<'a>,
    same_as: [&'static str; 1],
    address: Address,
    contact_point: [Contact; 2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Address {
    #[serde(rename = "@type")]
    kind: &'static str,
    address_country: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Contact {
    #[serde(rename = "@type")]
    kind: &'static str,
    contact_type: &'static str,
    email: &'static str,
}

#[derive(Serialize)]
struct Identity<'a> {
    #[serde(rename = "@id")]
    id: SiteUrl<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Website<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    #[serde(rename = "@id")]
    id: SiteUrl<'a>,
    name: &'static str,
    url: SiteUrl<'a>,
    description: &'static str,
    publisher: Identity<'a>,
    in_language: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Application<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    #[serde(rename = "@id")]
    id: SiteUrl<'a>,
    name: &'static str,
    url: SiteUrl<'a>,
    description: &'static str,
    application_category: &'static str,
    operating_system: &'static str,
    publisher: Identity<'a>,
    same_as: [&'static str; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceCode<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    name: &'static str,
    code_repository: &'static str,
    programming_language: &'static str,
    target_product: Identity<'a>,
    license: &'static str,
}

pub fn structured_data(origin: &str) -> String {
    let document = StructuredData {
        context: "https://schema.org",
        graph: (
            Organization {
                kind: "Organization",
                id: SiteUrl { origin, path: "/#organization" },
                name: "Proa Labs, Inc.",
                url: SiteUrl { origin, path: "/" },
                description: "Publisher of Proa, a Rust web framework for composable, multi-target rendering.",
                logo: SiteUrl { origin, path: "/static/apple-touch-icon.png" },
                same_as: ["https://github.com/proa-labs"],
                address: Address { kind: "PostalAddress", address_country: "US" },
                contact_point: [
                    Contact { kind: "ContactPoint", contact_type: "privacy", email: "privacy@proa.so" },
                    Contact { kind: "ContactPoint", contact_type: "licensing", email: "legal@proa.so" },
                ],
            },
            Website {
                kind: "WebSite",
                id: SiteUrl { origin, path: "/#website" },
                name: "Proa",
                url: SiteUrl { origin, path: "/" },
                description: "Proa Rust framework documentation, examples, and developer tools.",
                publisher: Identity { id: SiteUrl { origin, path: "/#organization" } },
                in_language: "en",
            },
            Application {
                kind: "SoftwareApplication",
                id: SiteUrl { origin, path: "/#proa" },
                name: "Proa",
                url: SiteUrl { origin, path: "/" },
                description: "A Rust web framework with compiled HTML templates, streaming SSR, typed Markdown, and RSJS browser islands.",
                application_category: "DeveloperApplication",
                operating_system: "Linux, macOS, Windows",
                publisher: Identity { id: SiteUrl { origin, path: "/#organization" } },
                same_as: ["https://github.com/proa-labs/proa"],
            },
            SourceCode {
                kind: "SoftwareSourceCode",
                name: "Proa source code",
                code_repository: "https://github.com/proa-labs/proa",
                programming_language: "Rust",
                target_product: Identity { id: SiteUrl { origin, path: "/#proa" } },
                license: "https://github.com/proa-labs/proa/blob/main/LICENSING.md",
            },
        ),
    };
    // JSON escaping alone does not protect the surrounding HTML script element.
    encode(&document)
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_urls_are_json_escaped_and_script_safe() {
        let origin = "https://preview.example/\"</script>&";
        let card: serde_json::Value = serde_json::from_str(&server_card(origin)).unwrap();
        assert_eq!(card["remotes"][0]["url"], format!("{origin}/mcp"));
        let data = structured_data(origin);
        assert!(!data.contains(['<', '>', '&']));
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(parsed["@graph"][0]["url"], format!("{origin}/"));
    }
}
