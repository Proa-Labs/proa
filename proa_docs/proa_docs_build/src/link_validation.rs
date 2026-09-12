// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::{
    compiler::{CompileError, PageLinkReferences},
    config::{Config, LinkValidation, LinkValidationMode, RelativePathMode},
};

#[derive(Debug, Clone, Default)]
pub struct LinkGraph {
    pub pages: Vec<LinkGraphPage>,
    pub assets: Vec<String>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct LinkGraphPage {
    pub source_path: String,
    pub route_path: String,
    pub hashes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default)]
pub struct LinkValidationReport {
    pub diagnostics: Vec<LinkDiagnostic>,
}

impl LinkValidationReport {
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct LinkDiagnostic {
    pub source_path: String,
    pub href: String,
    pub message: String,
}

pub fn validate_links(config: Config) -> Result<LinkValidationReport, CompileError> {
    crate::compiler::validate_links_config(config)
}

pub(crate) fn validate_references(
    graph: &LinkGraph,
    references: &[PageLinkReferences],
    config: &LinkValidation,
) -> LinkValidationReport {
    if matches!(config.mode, LinkValidationMode::Off) {
        return LinkValidationReport::default();
    }

    let routes = graph
        .pages
        .iter()
        .map(|page| normalize_path(&page.route_path).to_string())
        .collect::<BTreeSet<_>>();
    let mut hashes = BTreeSet::new();
    for page in &graph.pages {
        for hash in &page.hashes {
            hashes.insert((normalize_path(&page.route_path).to_string(), hash.clone()));
        }
    }

    let mut report = LinkValidationReport::default();
    for page_refs in references {
        let page_route = normalize_path(&page_refs.route_path);
        for href in &page_refs.hrefs {
            validate_href(
                &mut report,
                config,
                &routes,
                &hashes,
                &page_refs.source_path,
                page_route,
                href,
            );
        }
    }
    report
}

fn validate_href(
    report: &mut LinkValidationReport,
    config: &LinkValidation,
    routes: &BTreeSet<String>,
    hashes: &BTreeSet<(String, String)>,
    source_path: &str,
    current_route: &str,
    href: &str,
) {
    if href.is_empty()
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("http://")
        || href.starts_with("https://")
    {
        return;
    }

    if let Some(hash) = href.strip_prefix('#') {
        if config.check_hashes && !hashes.contains(&(current_route.to_string(), hash.to_string())) {
            report.diagnostics.push(LinkDiagnostic {
                source_path: source_path.to_string(),
                href: href.to_string(),
                message: format!("unknown hash `#{hash}` on current page"),
            });
        }
        return;
    }

    let (path, hash) = split_hash(href);
    let resolved = if path.starts_with('/') {
        normalize_owned(path)
    } else if matches!(config.check_relative_paths, RelativePathMode::AsUrl) {
        resolve_relative_url(current_route, path)
    } else {
        return;
    };

    if config.check_internal && !routes.contains(resolved.as_str()) {
        report.diagnostics.push(LinkDiagnostic {
            source_path: source_path.to_string(),
            href: href.to_string(),
            message: format!("unknown internal route `{resolved}`"),
        });
        return;
    }

    if config.check_hashes {
        if let Some(hash) = hash {
            if !hashes.contains(&(resolved.clone(), hash.to_string())) {
                report.diagnostics.push(LinkDiagnostic {
                    source_path: source_path.to_string(),
                    href: href.to_string(),
                    message: format!("unknown hash `#{hash}` on `{resolved}`"),
                });
            }
        }
    }
}

fn split_hash(href: &str) -> (&str, Option<&str>) {
    href.split_once('#')
        .map_or((href, None), |(path, hash)| (path, Some(hash)))
}

fn normalize_path(path: &str) -> &str {
    let without_index = path.strip_suffix("/index.html").unwrap_or(path);
    if without_index != "/" {
        without_index.trim_end_matches('/')
    } else {
        without_index
    }
}

fn normalize_owned(path: &str) -> String {
    normalize_path(path).to_string()
}

fn resolve_relative_url(current_route: &str, href: &str) -> String {
    let mut parts = current_route
        .trim_end_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if !current_route.ends_with('/') {
        parts.pop();
    }
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}
