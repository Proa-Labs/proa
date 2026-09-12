// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::compiler::{CompiledNavItem, CompiledPage};

pub fn emit_docs(
    site_name: &str,
    base_path: &str,
    nav_title: &str,
    nav_icon: Option<&str>,
    pages: &[CompiledPage],
    nav: &[CompiledNavItem],
    search_paths: Option<(&str, &str)>,
) -> String {
    let mut out = String::new();
    out.push_str("proa_docs::Docs {\n");
    let _ = writeln!(
        out,
        "    config: proa_docs::DocsConfig {{ base_path: {base_path}, site_name: {site_name} }},",
        base_path = lit(base_path),
        site_name = lit(site_name)
    );
    let _ = writeln!(out, "    nav_title: {},", lit(nav_title));
    let _ = writeln!(out, "    nav_icon: {},", opt_lit(nav_icon));
    out.push_str("    pages: &[\n");
    for (index, page) in pages.iter().enumerate() {
        out.push_str("        proa_docs::DocPage {\n");
        let _ = writeln!(out, "            slug: {slug},", slug = lit(&page.slug));
        let _ = writeln!(
            out,
            "            route_path: {route_path},",
            route_path = lit(&page.route_path)
        );
        let _ = writeln!(
            out,
            "            markdown_path: {markdown_path},",
            markdown_path = lit(&page.markdown_path)
        );
        let _ = writeln!(out, "            title: {title},", title = lit(&page.title));
        let _ = writeln!(
            out,
            "            description: {description},",
            description = opt_lit(page.description.as_deref())
        );
        out.push_str("            frontmatter: &");
        emit_frontmatter(&mut out, page);
        out.push_str(",\n");
        let _ = writeln!(
            out,
            "            article_html: include_str!({path}),",
            path = lit(&page.html_include_path)
        );
        let _ = writeln!(
            out,
            "            markdown: include_str!({path}),",
            path = lit(&page.markdown_include_path)
        );
        out.push_str("            toc: &[\n");
        for item in &page.toc {
            let _ = writeln!(
                out,
                "                proa_docs::TocItem {{ depth: {depth}, id: {id}, title: {title} }},",
                depth = item.depth,
                id = lit(&item.id),
                title = lit(&item.title)
            );
        }
        out.push_str("            ],\n            prev: ");
        if let Some(prev) = pages[..index]
            .iter()
            .rev()
            .find(|page| !page.frontmatter.hidden)
        {
            emit_page_link(&mut out, prev);
        } else {
            out.push_str("None");
        }
        out.push_str(",\n            next: ");
        if let Some(next) = pages[index + 1..]
            .iter()
            .find(|page| !page.frontmatter.hidden)
        {
            emit_page_link(&mut out, next);
        } else {
            out.push_str("None");
        }
        out.push_str(",\n        },\n");
    }
    out.push_str("    ],\n");
    out.push_str("    nav: &[\n");
    for item in nav {
        emit_nav_item(&mut out, item, 2);
    }
    out.push_str("    ],\n    search: ");
    if let Some((manifest_json_path, documents_json_path)) = search_paths {
        out.push_str("Some(proa_docs::SearchArtifacts {\n");
        let _ = writeln!(
            out,
            "        manifest_path: {path},",
            path = lit(&format!(
                "{}/_search/manifest.json",
                base_path.trim_end_matches('/')
            ))
        );
        let _ = writeln!(
            out,
            "        manifest_json: include_str!({path}),",
            path = lit(manifest_json_path)
        );
        let _ = writeln!(
            out,
            "        documents_path: {path},",
            path = lit(&format!(
                "{}/_search/documents.json",
                base_path.trim_end_matches('/')
            ))
        );
        let _ = writeln!(
            out,
            "        documents_json: include_str!({path}),",
            path = lit(documents_json_path)
        );
        out.push_str("    })");
    } else {
        out.push_str("None");
    }
    out.push_str(",\n}\n");
    out
}

fn emit_nav_item(out: &mut String, item: &CompiledNavItem, indent: usize) {
    let pad = "    ".repeat(indent);
    match item {
        CompiledNavItem::Page { title, icon, path } => {
            let _ = writeln!(
                out,
                "{}proa_docs::NavItem {{ kind: proa_docs::NavItemKind::Page, title: {}, icon: {}, path: Some({}), children: &[] }},",
                pad,
                lit(title),
                opt_lit(icon.as_deref()),
                lit(path)
            );
        }
        CompiledNavItem::Group {
            title,
            icon,
            root,
            children,
        } => {
            let kind = if *root { "Root" } else { "Group" };
            let _ = writeln!(
                out,
                "{}proa_docs::NavItem {{ kind: proa_docs::NavItemKind::{}, title: {}, icon: {}, path: None, children: &[",
                pad,
                kind,
                lit(title),
                opt_lit(icon.as_deref())
            );
            for child in children {
                emit_nav_item(out, child, indent + 1);
            }
            let _ = writeln!(out, "{pad}] }},");
        }
        CompiledNavItem::Separator { title, icon } => {
            let _ = writeln!(
                out,
                "{}proa_docs::NavItem {{ kind: proa_docs::NavItemKind::Separator, title: {}, icon: {}, path: None, children: &[] }},",
                pad,
                lit(title),
                opt_lit(icon.as_deref())
            );
        }
    }
}

fn emit_frontmatter(out: &mut String, page: &CompiledPage) {
    let _ = write!(
        out,
        "proa_docs::Frontmatter {{ title: {title}, description: {description}, icon: {icon}, slug: {slug}, sidebar_title: {sidebar_title}, nav_order: ",
        title = opt_lit(page.frontmatter.title.as_deref()),
        description = opt_lit(page.frontmatter.description.as_deref()),
        icon = opt_lit(page.frontmatter.icon.as_deref()),
        slug = opt_lit(page.frontmatter.slug.as_deref()),
        sidebar_title = opt_lit(page.frontmatter.sidebar_title.as_deref())
    );
    if let Some(order) = page.frontmatter.nav_order {
        let _ = write!(out, "Some({order}");
        out.push(')');
    } else {
        out.push_str("None");
    }
    let _ = write!(
        out,
        ", hidden: {hidden}, draft: {draft}, search: ",
        hidden = bool_lit(page.frontmatter.hidden),
        draft = bool_lit(page.frontmatter.draft)
    );
    if let Some(search) = page.frontmatter.search {
        let _ = write!(out, "Some({value}", value = bool_lit(search));
        out.push(')');
    } else {
        out.push_str("None");
    }
    out.push_str(", toc: ");
    if let Some(toc) = page.frontmatter.toc {
        let _ = write!(out, "Some({value}", value = bool_lit(toc));
        out.push(')');
    } else {
        out.push_str("None");
    }
    let _ = write!(
        out,
        ", last_modified: {last_modified} }}",
        last_modified = opt_lit(page.frontmatter.last_modified.as_deref())
    );
}

fn emit_page_link(out: &mut String, page: &CompiledPage) {
    let _ = write!(
        out,
        "Some(proa_docs::PageLink {{ title: {title}, path: {path} }})",
        title = lit(&page.title),
        path = lit(&page.route_path)
    );
}

fn bool_lit(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

pub fn lit(value: &str) -> String {
    serde_json::to_string(value).expect("string literal serialization")
}

fn opt_lit(value: Option<&str>) -> String {
    value.map_or_else(
        || "None".to_string(),
        |value| format!("Some({})", lit(value)),
    )
}
