// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::path::{normalize_slug, path_to_slug};

#[derive(Debug, Clone, Copy)]
pub struct Docs {
    pub config: DocsConfig,
    pub nav_title: &'static str,
    pub nav_icon: Option<&'static str>,
    pub pages: &'static [DocPage],
    pub nav: &'static [NavItem],
    pub search: Option<SearchArtifacts>,
}

impl Docs {
    pub fn find_page(&self, slug: &str) -> Option<&'static DocPage> {
        let normalized = normalize_slug(slug);
        self.pages.iter().find(|page| page.slug == normalized)
    }

    pub fn first_page(&self) -> Option<&'static DocPage> {
        self.pages.first()
    }

    pub fn page_for_path(&self, path: &str) -> Option<&'static DocPage> {
        let slug = path_to_slug(path, self.config.base_path);
        self.find_page(slug)
    }

    pub fn nav_items_for_path(&self, path: &str) -> &'static [NavItem] {
        self.active_nav_root_item(path)
            .map_or(self.nav, |root| root.children)
    }

    pub fn active_nav_root(&self, path: &str) -> NavRoot {
        self.active_nav_root_item(path).map_or(
            NavRoot {
                title: self.nav_title,
                icon: self.nav_icon,
                href: self.config.base_path,
                items: self.nav,
            },
            |root| NavRoot {
                title: root.title,
                icon: root.icon,
                href: root.first_page_path().unwrap_or(self.config.base_path),
                items: root.children,
            },
        )
    }

    pub fn has_nav_roots(&self) -> bool {
        self.nav.iter().any(NavItem::contains_root)
    }

    fn active_nav_root_item(&self, path: &str) -> Option<&'static NavItem> {
        self.nav
            .iter()
            .find_map(|item| item.find_root_for_path(path))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DocsConfig {
    pub base_path: &'static str,
    pub site_name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DocPage {
    pub slug: &'static str,
    pub route_path: &'static str,
    pub markdown_path: &'static str,
    pub title: &'static str,
    pub description: Option<&'static str>,
    pub frontmatter: &'static Frontmatter,
    pub article_html: &'static str,
    pub markdown: &'static str,
    pub toc: &'static [TocItem],
    pub prev: Option<PageLink>,
    pub next: Option<PageLink>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Frontmatter {
    pub title: Option<&'static str>,
    pub description: Option<&'static str>,
    pub icon: Option<&'static str>,
    pub slug: Option<&'static str>,
    pub sidebar_title: Option<&'static str>,
    pub nav_order: Option<i32>,
    pub hidden: bool,
    pub draft: bool,
    pub search: Option<bool>,
    pub toc: Option<bool>,
    pub last_modified: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct TocItem {
    pub depth: u8,
    pub id: &'static str,
    pub title: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct PageLink {
    pub title: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct NavItem {
    pub kind: NavItemKind,
    pub title: &'static str,
    pub icon: Option<&'static str>,
    pub path: Option<&'static str>,
    pub children: &'static [NavItem],
}

impl NavItem {
    pub const fn is_root(&self) -> bool {
        matches!(self.kind, NavItemKind::Root)
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.path == Some(path) || self.children.iter().any(|child| child.contains_path(path))
    }

    pub fn first_page_path(&self) -> Option<&'static str> {
        self.path
            .or_else(|| self.children.iter().find_map(NavItem::first_page_path))
    }

    pub(crate) fn contains_root(&self) -> bool {
        self.is_root() || self.children.iter().any(NavItem::contains_root)
    }

    pub(crate) fn find_root_for_path(&'static self, path: &str) -> Option<&'static NavItem> {
        if let Some(root) = self
            .children
            .iter()
            .find_map(|child| child.find_root_for_path(path))
        {
            return Some(root);
        }
        if self.is_root() && self.contains_path(path) {
            return Some(self);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavItemKind {
    Page,
    Group,
    Root,
    Separator,
}

impl NavItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Group => "group",
            Self::Root => "root",
            Self::Separator => "separator",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NavRoot {
    pub title: &'static str,
    pub icon: Option<&'static str>,
    pub href: &'static str,
    pub items: &'static [NavItem],
}

#[derive(Debug, Clone, Copy)]
pub struct SearchArtifacts {
    pub manifest_path: &'static str,
    pub manifest_json: &'static str,
    pub documents_path: &'static str,
    pub documents_json: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchDocument {
    pub id: &'static str,
    pub url: &'static str,
    pub title: &'static str,
    pub section: Option<&'static str>,
    pub content: &'static str,
}
