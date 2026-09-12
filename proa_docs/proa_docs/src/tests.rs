// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

static FRONTMATTER: Frontmatter = Frontmatter {
    title: Some("Intro"),
    description: Some("Intro docs"),
    icon: None,
    slug: None,
    sidebar_title: None,
    nav_order: None,
    hidden: false,
    draft: false,
    search: None,
    toc: None,
    last_modified: None,
};

static TOC: &[TocItem] = &[TocItem {
    depth: 2,
    id: "install",
    title: "Install",
}];

static PAGE: DocPage = DocPage {
    slug: "",
    route_path: "/docs",
    markdown_path: "/docs/index.md",
    title: "Intro",
    description: Some("Intro docs"),
    frontmatter: &FRONTMATTER,
    article_html: "<h1>Intro</h1>",
    markdown: "# Intro",
    toc: TOC,
    prev: None,
    next: None,
};

static NAV: &[NavItem] = &[NavItem {
    kind: NavItemKind::Page,
    title: "Intro",
    icon: None,
    path: Some("/docs"),
    children: &[],
}];

static DOCS: Docs = Docs {
    config: DocsConfig {
        base_path: "/docs",
        site_name: "Docs",
    },
    nav_title: "Docs",
    nav_icon: None,
    pages: &[PAGE],
    nav: NAV,
    search: None,
};

static SECTION_START_CHILDREN: &[NavItem] = &[NavItem {
    kind: NavItemKind::Page,
    title: "Quickstart",
    icon: None,
    path: Some("/docs/guides/quickstart"),
    children: &[],
}];

static SECTION_CORE_CHILDREN: &[NavItem] = &[NavItem {
    kind: NavItemKind::Page,
    title: "Render Traits",
    icon: None,
    path: Some("/docs/core/render-traits"),
    children: &[],
}];

static SECTION_CLI_CHILDREN: &[NavItem] = &[
    NavItem {
        kind: NavItemKind::Page,
        title: "CLI",
        icon: None,
        path: Some("/docs/cli"),
        children: &[],
    },
    NavItem {
        kind: NavItemKind::Page,
        title: "Create a Site",
        icon: None,
        path: Some("/docs/cli/new-site"),
        children: &[],
    },
];

static SECTION_NAV: &[NavItem] = &[
    NavItem {
        kind: NavItemKind::Separator,
        title: "Start Here",
        icon: None,
        path: None,
        children: &[],
    },
    NavItem {
        kind: NavItemKind::Page,
        title: "Documentation",
        icon: None,
        path: Some("/docs"),
        children: &[],
    },
    NavItem {
        kind: NavItemKind::Root,
        title: "Guides",
        icon: Some("BookOpen"),
        path: None,
        children: SECTION_START_CHILDREN,
    },
    NavItem {
        kind: NavItemKind::Group,
        title: "Core APIs",
        icon: Some("Cpu"),
        path: None,
        children: SECTION_CORE_CHILDREN,
    },
    NavItem {
        kind: NavItemKind::Separator,
        title: "Tools and integrations",
        icon: None,
        path: None,
        children: &[],
    },
    NavItem {
        kind: NavItemKind::Group,
        title: "CLI",
        icon: Some("Terminal"),
        path: None,
        children: SECTION_CLI_CHILDREN,
    },
];

static SECTION_DOCS: Docs = Docs {
    config: DocsConfig {
        base_path: "/docs",
        site_name: "Docs",
    },
    nav_title: "Proa Docs",
    nav_icon: Some("BookOpen"),
    pages: &[PAGE],
    nav: SECTION_NAV,
    search: None,
};

fn nav_contains_path(items: &[NavItem], path: &str) -> bool {
    items.iter().any(|item| {
        item.path == Some(path)
            || (!item.children.is_empty() && nav_contains_path(item.children, path))
    })
}

#[test]
fn renders_page_shell() {
    let html = render_page_to_string(&DOCS, &PAGE);
    assert!(html.contains("<article"));
    assert!(html.contains("<h1>Intro</h1>"));
    assert!(html.contains("Intro - Docs"));
}

#[test]
fn serves_markdown_source() {
    let response = render(&DOCS, "/docs/index.md");
    assert_eq!(response.content_type, "text/markdown; charset=utf-8");
    assert_eq!(response.body, "# Intro");
}

#[test]
fn metadata_roots_scope_navigation_without_hiding_default_separators() {
    assert!(SECTION_DOCS.has_nav_roots());

    let default_items = SECTION_DOCS.nav_items_for_path("/docs/core/render-traits");
    assert!(nav_contains_path(default_items, "/docs"));
    assert!(nav_contains_path(default_items, "/docs/core/render-traits"));
    assert!(default_items.iter().any(|item| {
        item.kind == NavItemKind::Separator && item.title == "Tools and integrations"
    }));

    let guide_items = SECTION_DOCS.nav_items_for_path("/docs/guides/quickstart");
    assert_eq!(guide_items.len(), 1);
    assert!(nav_contains_path(guide_items, "/docs/guides/quickstart"));
    assert!(!nav_contains_path(guide_items, "/docs/cli"));

    let guide_root = SECTION_DOCS.active_nav_root("/docs/guides/quickstart");
    assert_eq!(guide_root.title, "Guides");
    assert_eq!(guide_root.icon, Some("BookOpen"));
    assert_eq!(guide_root.href, "/docs/guides/quickstart");

    let default_root = SECTION_DOCS.active_nav_root("/docs/cli/new-site");
    assert_eq!(default_root.title, "Proa Docs");
    assert_eq!(default_root.href, "/docs");
}
