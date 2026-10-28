// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Search-engine metadata owned by the HTML document shell, not agent discovery.

use crate::data::{routes, site};

use crate::utils::{document::SiteUrl, site_url::public_site_url};
use serde::Serialize;
use std::sync::OnceLock;

pub(super) fn document() -> &'static str {
    static DOCUMENT: OnceLock<String> = OnceLock::new();
    DOCUMENT.get_or_init(|| serialize(public_site_url()))
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

const ORGANIZATION_ID: &str = "/#organization";
const WEBSITE_ID: &str = "/#website";
const APPLICATION_ID: &str = "/#proa";

fn serialize(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    let document = StructuredData {
        context: "https://schema.org",
        graph: (
            Organization {
                kind: "Organization",
                id: url(ORGANIZATION_ID),
                name: site::ORGANIZATION,
                url: url(routes::HOME),
                description: "Publisher of Proa, a Rust web framework for composable, multi-target rendering.",
                logo: url("/static/apple-touch-icon.png"),
                same_as: [site::GITHUB_ORGANIZATION_URL],
                address: Address { kind: "PostalAddress", address_country: "US" },
                contact_point: [
                    Contact { kind: "ContactPoint", contact_type: "privacy", email: site::PRIVACY_EMAIL },
                    Contact { kind: "ContactPoint", contact_type: "licensing", email: site::LEGAL_EMAIL },
                ],
            },
            Website {
                kind: "WebSite",
                id: url(WEBSITE_ID),
                name: site::NAME,
                url: url(routes::HOME),
                description: "Proa Rust framework documentation, examples, and developer tools.",
                publisher: Identity { id: url(ORGANIZATION_ID) },
                in_language: "en",
            },
            Application {
                kind: "SoftwareApplication",
                id: url(APPLICATION_ID),
                name: site::NAME,
                url: url(routes::HOME),
                description: "A Rust web framework with compiled HTML templates, streaming SSR, typed Markdown, and RSJS browser islands.",
                application_category: "DeveloperApplication",
                operating_system: "Linux, macOS, Windows",
                publisher: Identity { id: url(ORGANIZATION_ID) },
                same_as: [site::GITHUB_URL],
            },
            SourceCode {
                kind: "SoftwareSourceCode",
                name: const_format::concatcp!(site::NAME, " source code"),
                code_repository: site::GITHUB_URL,
                programming_language: "Rust",
                target_product: Identity { id: url(APPLICATION_ID) },
                license: site::LICENSE_URL,
            },
        ),
    };
    // JSON escaping alone does not protect the surrounding HTML script element.
    serde_json::to_string(&document)
        .expect("serialize site structured data")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_urls_are_json_escaped_and_script_safe() {
        let origin = "https://preview.example/\"</script>&";
        let data = super::serialize(origin);
        assert!(!data.contains(['<', '>', '&']));
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(parsed["@graph"][0]["url"], format!("{origin}/"));
    }
}
