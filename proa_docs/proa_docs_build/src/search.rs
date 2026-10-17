// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use base16ct::HexDisplay;
use serde::Serialize;
use sha2::{Digest, Sha256};

use std::collections::HashMap;

use crate::compiler::{CompiledNavItem, CompiledPage};

#[derive(Debug, Serialize)]
pub struct SearchManifest {
    pub version: u8,
    pub engine: &'static str,
    #[serde(rename = "basePath")]
    pub base_path: String,
    pub hash: String,
    pub documents: String,
}

#[derive(Debug, Serialize)]
pub struct SearchDocuments<'a> {
    pub version: u8,
    pub documents: Vec<SearchDocument<'a>>,
}

#[derive(Debug, Serialize)]
pub struct SearchDocument<'a> {
    pub id: &'a str,
    pub url: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub section: Option<&'a str>,
    pub content: &'a str,
    pub breadcrumbs: Vec<&'a str>,
    pub headings: Vec<SearchHeading<'a>>,
    pub keywords: Vec<&'a str>,
    pub rank: u16,
}

#[derive(Debug, Serialize)]
pub struct SearchHeading<'a> {
    pub id: &'a str,
    pub depth: u8,
    pub title: &'a str,
}

pub fn build_search_json(
    pages: &[CompiledPage],
    nav: &[CompiledNavItem],
    base_path: &str,
) -> Result<(String, String), serde_json::Error> {
    let sections = nav_sections(nav);
    let documents = SearchDocuments {
        version: 1,
        documents: pages
            .iter()
            .filter(|page| page.frontmatter.search.unwrap_or(true))
            .map(|page| SearchDocument {
                id: &page.slug,
                url: &page.route_path,
                title: &page.title,
                description: page.description.as_deref(),
                section: sections.get(page.route_path.as_str()).copied(),
                content: &page.search_text,
                breadcrumbs: vec![&page.title],
                headings: page
                    .toc
                    .iter()
                    .map(|item| SearchHeading {
                        id: &item.id,
                        depth: item.depth,
                        title: &item.title,
                    })
                    .collect(),
                keywords: page
                    .frontmatter
                    .keywords
                    .iter()
                    .map(String::as_str)
                    .collect(),
                rank: 100,
            })
            .collect(),
    };

    let documents_json = serde_json::to_string(&documents)?;
    let hash = content_hash(&documents_json);
    let manifest = SearchManifest {
        version: 1,
        engine: "orama",
        base_path: base_path.to_string(),
        hash,
        documents: format!("{}/_search/documents.json", base_path.trim_end_matches('/')),
    };
    let manifest_json = serde_json::to_string(&manifest)?;
    Ok((manifest_json, documents_json))
}

/// Map each navigated page path to the title of the nearest separator above
/// it. Pages inside a group inherit the separator in effect where the group
/// sits; pages before the first separator get no section.
fn nav_sections(nav: &[CompiledNavItem]) -> HashMap<&str, &str> {
    fn walk<'a>(
        items: &'a [CompiledNavItem],
        mut current: Option<&'a str>,
        out: &mut HashMap<&'a str, &'a str>,
    ) {
        for item in items {
            match item {
                CompiledNavItem::Separator { title, .. } => current = Some(title.as_str()),
                CompiledNavItem::Page { path, .. } => {
                    if let Some(section) = current {
                        out.entry(path.as_str()).or_insert(section);
                    }
                }
                CompiledNavItem::Group { children, .. } => walk(children, current, out),
            }
        }
    }

    let mut out = HashMap::new();
    walk(nav, None, &mut out);
    out
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256-{:x}", HexDisplay(&hasher.finalize()))
}
