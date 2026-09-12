// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[allow(unused_imports)]
use proa_docs::include_docs;
use proa_docs::{
    nav_icon_svg, render, render_page_to_string, render_static, DocPage, Docs, DocsConfig, DocsNav,
    DocsNavItem, DocsPager, DocsResponse, DocsSectionMenu, DocsShell, DocsToc, Frontmatter,
    NavItem, NavItemKind, NavRoot, PageLink, SearchArtifacts, SearchDocument, TocItem, TypeTable,
    TypeTableParam, TypeTableRow,
};

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

static PAGE: DocPage = DocPage {
    slug: "",
    route_path: "/docs",
    markdown_path: "/docs/index.md",
    title: "Intro",
    description: Some("Intro docs"),
    frontmatter: &FRONTMATTER,
    article_html: "<h1>Intro</h1>",
    markdown: "# Intro",
    toc: &[],
    prev: None,
    next: Some(PageLink {
        title: "Next",
        path: "/docs/next",
    }),
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
    search: Some(SearchArtifacts {
        manifest_path: "/docs/search/manifest.json",
        manifest_json: "{}",
        documents_path: "/docs/search/documents.json",
        documents_json: "[]",
    }),
};

#[test]
fn crate_root_exports_remain_available() {
    assert_eq!(DOCS.find_page("").map(|page| page.title), Some("Intro"));
    assert_eq!(DOCS.first_page().map(|page| page.title), Some("Intro"));
    assert_eq!(
        DOCS.page_for_path("/docs").map(|page| page.title),
        Some("Intro")
    );

    let _ = DocsShell {
        docs: &DOCS,
        page: &PAGE,
    };
    let _ = DocsNav {
        items: NAV,
        active_path: "/docs",
    };
    let _ = DocsNavItem {
        item: &NAV[0],
        active_path: "/docs",
    };
    let _ = DocsSectionMenu {
        docs: &DOCS,
        active_path: "/docs",
    };
    let _ = DocsToc { items: &[] };
    let _ = DocsPager { page: &PAGE };

    let root: NavRoot = DOCS.active_nav_root("/docs");
    assert_eq!(root.title, "Docs");
    assert_eq!(DOCS.nav_items_for_path("/docs").len(), 1);
    assert!(nav_icon_svg("BookOpen").starts_with("<svg"));

    let page_response = render(&DOCS, "/docs");
    assert_eq!(page_response.status, 200);
    assert!(render_page_to_string(&DOCS, &PAGE).contains("<article"));
    assert_eq!(
        render_static(&DOCS, "/docs/index.md").map(|response| response.content_type),
        Some("text/markdown; charset=utf-8")
    );

    let responses = [
        DocsResponse::html(String::new()),
        DocsResponse::markdown(""),
        DocsResponse::json("{}"),
        DocsResponse::not_found(),
    ];
    assert_eq!(responses.len(), 4);
}

#[test]
fn generated_and_type_table_models_remain_public() {
    let param = TypeTableParam {
        name: "value",
        type_repr: "&str",
        description: Some("Input value"),
    };
    let row = TypeTableRow {
        name: "label",
        type_repr: "&str",
        description: Some("Visible label"),
        default: None,
        required: true,
        deprecated: false,
        href: None,
        parameters: &[param],
        returns: None,
    };
    let table = TypeTable {
        id: "props",
        name: "Props",
        rows: &[row],
    };
    assert_eq!(table.rows[0].parameters[0].name, "value");

    let toc = TocItem {
        depth: 2,
        id: "install",
        title: "Install",
    };
    let search = SearchDocument {
        id: "intro",
        url: "/docs",
        title: "Intro",
        section: None,
        content: "Welcome",
    };
    assert_eq!(toc.id, "install");
    assert_eq!(search.url, "/docs");
}

#[cfg(feature = "axum")]
#[test]
fn axum_adapter_remains_feature_gated_and_public() {
    let response = proa_docs::adapter::axum::response(&DOCS, "/docs");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[test]
fn default_assets_are_exported_and_non_empty() {
    // A consumer adding only `proa_docs` must get a working theme.
    assert!(proa_docs::DEFAULT_STYLESHEET.len() > 10_000);
    assert!(proa_docs::DEFAULT_SCRIPT.len() > 1_000);
    let _ = proa_docs::DocsAssets::new();
    let _ = proa_docs::DocsAssets::style_only();
    let _ = proa_docs::DocsAssets::script_only();
}
