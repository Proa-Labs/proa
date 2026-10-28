// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Search-engine metadata owned by the HTML document shell, not agent discovery.

use crate::data::{routes, site};

use crate::utils::{document::SiteUrl, site_url::public_site_url};
use serde::Serialize;
use std::{collections::HashMap, sync::OnceLock};

pub(super) fn document() -> &'static str {
    static DOCUMENT: OnceLock<String> = OnceLock::new();
    DOCUMENT.get_or_init(|| serialize(public_site_url()))
}

/// Immutable documentation metadata is serialized once, outside rendering.
pub(super) fn for_documentation(path: &str) -> Option<&'static str> {
    if path != routes::DOCS && !path.starts_with("/docs/") {
        return None;
    }
    static DOCUMENTS: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    DOCUMENTS
        .get_or_init(|| {
            crate::pages::docs::docs()
                .pages
                .iter()
                .filter(|page| !page.frontmatter.hidden)
                .map(|page| (page.route_path, documentation(public_site_url(), page)))
                .collect()
        })
        .get(path)
        .map(String::as_str)
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
        FaqPage<'a>,
        VideoObject<'a>,
        VideoObject<'a>,
        ChartCollection<'a>,
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
    same_as: [&'static str; 2],
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
    same_as: [&'static str; 2],
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FaqPage<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    #[serde(rename = "@id")]
    id: SiteUrl<'a>,
    main_entity: Vec<Question<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Question<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    name: &'a str,
    accepted_answer: Answer<'a>,
}

#[derive(Serialize)]
struct Answer<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VideoObject<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    name: &'static str,
    url: &'static str,
    thumbnail_url: &'static str,
    embed_url: &'static str,
    publisher: Identity<'a>,
}

fn video_object<'a>(origin: &'a str, video: &crate::pages::home::media::Video) -> VideoObject<'a> {
    VideoObject {
        kind: "VideoObject",
        name: video.name,
        url: video.url,
        thumbnail_url: video.thumbnail_url,
        embed_url: video.embed_url,
        publisher: Identity {
            id: SiteUrl {
                origin,
                path: ORGANIZATION_ID,
            },
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChartCollection<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    name: &'static str,
    item_list_order: &'static str,
    number_of_items: usize,
    item_list_element: [ChartImage<'a>; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChartImage<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    name: &'static str,
    description: &'static str,
    content_url: SiteUrl<'a>,
    encoding_format: &'static str,
}

#[derive(Serialize)]
struct Documentation<'a> {
    #[serde(rename = "@context")]
    context: &'static str,
    #[serde(rename = "@graph")]
    graph: (Article<'a>, Breadcrumbs<'a>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Article<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    headline: &'a str,
    description: &'a str,
    url: SiteUrl<'a>,
    main_entity_of_page: SiteUrl<'a>,
    in_language: &'static str,
    publisher: Identity<'a>,
    about: Identity<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Breadcrumbs<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    item_list_element: Vec<Breadcrumb<'a>>,
}

#[derive(Serialize)]
struct Breadcrumb<'a> {
    #[serde(rename = "@type")]
    kind: &'static str,
    position: usize,
    name: &'a str,
    item: SiteUrl<'a>,
}

fn documentation(origin: &str, page: &proa_docs::DocPage) -> String {
    let url = |path| SiteUrl { origin, path };
    let mut breadcrumbs = vec![Breadcrumb {
        kind: "ListItem",
        position: 1,
        name: site::NAME,
        item: url(routes::HOME),
    }];
    if page.route_path != routes::DOCS {
        breadcrumbs.push(Breadcrumb {
            kind: "ListItem",
            position: 2,
            name: "Documentation",
            item: url(routes::DOCS),
        });
    }
    breadcrumbs.push(Breadcrumb {
        kind: "ListItem",
        position: breadcrumbs.len() + 1,
        name: page.title,
        item: url(page.route_path),
    });
    script_safe(&Documentation {
        context: "https://schema.org",
        graph: (
            Article {
                kind: if page.route_path == routes::API_GUIDE {
                    "APIReference"
                } else {
                    "TechArticle"
                },
                headline: page.title,
                description: page.description.unwrap_or(""),
                url: url(page.route_path),
                main_entity_of_page: url(page.route_path),
                in_language: "en",
                publisher: Identity {
                    id: url(ORGANIZATION_ID),
                },
                about: Identity {
                    id: url(APPLICATION_ID),
                },
            },
            Breadcrumbs {
                kind: "BreadcrumbList",
                item_list_element: breadcrumbs,
            },
        ),
    })
}

fn serialize(origin: &str) -> String {
    let url = |path| SiteUrl { origin, path };
    let (chart_title, charts) = crate::pages::home::benchmark_charts();
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
                same_as: [site::GITHUB_ORGANIZATION_URL, site::YOUTUBE_URL],
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
                same_as: [site::GITHUB_URL, site::YOUTUBE_URL],
            },
            SourceCode {
                kind: "SoftwareSourceCode",
                name: const_format::concatcp!(site::NAME, " source code"),
                code_repository: site::GITHUB_URL,
                programming_language: "Rust",
                target_product: Identity { id: url(APPLICATION_ID) },
                license: site::LICENSE_URL,
            },
            FaqPage {
                kind: "FAQPage",
                id: url("/#faq"),
                main_entity: crate::pages::home::faq::FAQS.iter().map(|entry| Question {
                    kind: "Question",
                    name: entry.question,
                    accepted_answer: Answer { kind: "Answer", text: entry.answer },
                }).collect(),
            },
            video_object(origin, &crate::pages::home::media::INTRO),
            video_object(origin, &crate::pages::home::media::RENDER_TARGETS),
            ChartCollection {
                kind: "ItemList",
                name: chart_title,
                item_list_order: "https://schema.org/ItemListUnordered",
                number_of_items: charts.len(),
                item_list_element: charts.map(|chart| ChartImage {
                    kind: "ImageObject",
                    name: chart.title,
                    description: chart.description,
                    content_url: url(chart.url),
                    encoding_format: "image/svg+xml",
                }),
            },
        ),
    };
    script_safe(&document)
}

fn script_safe(document: &impl Serialize) -> String {
    // JSON escaping alone does not protect the surrounding HTML script element.
    serde_json::to_string(document)
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
