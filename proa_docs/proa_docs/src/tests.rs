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

#[test]
fn mounted_shell_prefixes_navigation_sections_and_pager_but_preserves_fragments_and_external_links()
{
    use proa_core::WebRenderSync;
    let page = DocPage {
        route_path: "/docs/core/render-traits",
        article_html: r###"<a href="/docs/other">Other</a><img src="/images/test.svg"><a href="#install">Part</a><a href="https://example.com/out">External</a><code>&lt;a href="/example"&gt;</code>"###,
        prev: Some(PageLink {
            title: "Previous",
            path: "/docs/guides/quickstart",
        }),
        next: Some(PageLink {
            title: "External",
            path: "https://example.com/next",
        }),
        ..PAGE
    };
    let mut cx = proa_core::WebContext::new();
    cx.request_mut()
        .set_base_path(proa_core::BasePath::new("/apps/demo").unwrap());
    DocsShell {
        docs: &SECTION_DOCS,
        page: &page,
    }
    .render(&mut cx)
    .unwrap();
    let html = cx.into_output().into_string();
    for path in [
        "/docs",
        "/docs/guides/quickstart",
        "/docs/core/render-traits",
        "/docs/cli",
    ] {
        assert!(
            html.contains(&format!("href=\"/apps/demo{path}\"")),
            "{path}: {html}"
        );
    }
    assert!(html.contains("href=\"#install\""));
    assert!(html.contains("href=\"https://example.com/next\""));
    assert!(html.contains("data-active=\"true\""));
    assert!(html.contains("href=\"/apps/demo/docs/other\""));
    assert!(html.contains("src=\"/apps/demo/images/test.svg\""));
    assert!(html.contains("href=\"https://example.com/out\""));
    assert!(html.contains("&lt;a href=\"/example\"&gt;"));
    assert!(!html.contains("href=\"/docs"));
}

#[test]
fn mounted_docs_response_keeps_local_route_lookup_and_markdown_artifacts() {
    let base_path = proa_core::BasePath::new("/proxy/123").unwrap();
    let html = render_with_base_path(&DOCS, "/docs", &base_path);
    assert_eq!(html.status, 200);
    assert!(html.body.contains("href=\"/proxy/123/docs\""));
    let markdown = render_with_base_path(&DOCS, "/docs/index.md", &base_path);
    assert_eq!(markdown.content_type, "text/markdown; charset=utf-8");
    assert_eq!(markdown.body, PAGE.markdown);
    assert_eq!(
        render(&DOCS, "/docs").body,
        render_with_base_path(&DOCS, "/docs", &proa_core::BasePath::default()).body
    );
}

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

#[test]
fn mounted_search_urls_follow_the_mount_and_manifest_hash_matches_the_documents() {
    use base16ct::HexDisplay;
    use sha2::{Digest, Sha256};
    static SEARCH_DOCS: Docs = Docs {
        search: Some(SearchArtifacts {
            manifest_path: "/docs/_search/manifest.json",
            manifest_json: r#"{"version":1,"basePath":"/docs","documents":"/docs/_search/documents.json","hash":"build-hash"}"#,
            documents_path: "/docs/_search/documents.json",
            documents_json: r#"{"version":1,"documents":[{"url":"/docs/start","content":"/docs is ordinary text"},{"url":"https://example.com/out"}]}"#,
        }),
        ..DOCS
    };
    let base = proa_core::BasePath::new("/apps/demo").unwrap();
    let manifest = render_with_base_path(&SEARCH_DOCS, "/docs/_search/manifest.json", &base);
    let documents = render_with_base_path(&SEARCH_DOCS, "/docs/_search/documents.json", &base);
    let manifest: serde_json::Value = serde_json::from_str(&manifest.body).unwrap();
    let rows: serde_json::Value = serde_json::from_str(&documents.body).unwrap();
    assert_eq!(manifest["basePath"], "/apps/demo/docs");
    assert_eq!(
        manifest["documents"],
        "/apps/demo/docs/_search/documents.json"
    );
    assert_eq!(
        manifest["hash"],
        format!(
            "sha256-{:x}",
            HexDisplay(&Sha256::digest(documents.body.as_bytes()))
        )
    );
    assert_eq!(rows["documents"][0]["url"], "/apps/demo/docs/start");
    assert_eq!(rows["documents"][0]["content"], "/docs is ordinary text");
    assert_eq!(rows["documents"][1]["url"], "https://example.com/out");
    let root = render(&SEARCH_DOCS, "/docs/_search/documents.json");
    assert_eq!(root.body, SEARCH_DOCS.search.unwrap().documents_json);
}
