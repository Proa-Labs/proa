// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Runtime primitives for generated Proa documentation sites.
//!
//! `proa_docs` is intentionally small at runtime. The build crate performs
//! Markdown parsing, frontmatter resolution, table-of-contents extraction,
//! search document generation, and optional docgen work ahead of time. This
//! crate owns the static data model and renders a default docs shell around
//! generated article HTML.
//!
//! It also ships the default theme. [`DocsAssets`] inlines the stylesheet and
//! script that style and wire every component `proa_docs_build` emits, so a
//! docs site works without writing CSS. Override the 15 custom properties on
//! `.proa-docs-body` to theme it, or ignore the defaults and style the documented
//! markup contract yourself.

#[cfg(feature = "axum")]
pub mod adapter;

mod assets;
mod icons;
mod model;
mod navigation;
mod pager;
mod path;
mod response;
mod sections;
mod shell;
mod toc;
mod type_table;

#[cfg(test)]
mod tests;

pub use assets::{DocsAssets, DEFAULT_SCRIPT, DEFAULT_STYLESHEET};
pub use icons::nav_icon_svg;
pub use model::{
    DocPage, Docs, DocsConfig, Frontmatter, NavItem, NavItemKind, NavRoot, PageLink,
    SearchArtifacts, SearchDocument, TocItem,
};
pub use navigation::{DocsNav, DocsNavItem};
pub use pager::DocsPager;
pub use response::{render, render_page_to_string, render_static, DocsResponse};
pub use sections::DocsSectionMenu;
pub use shell::DocsShell;
pub use toc::DocsToc;
pub use type_table::{TypeTable, TypeTableParam, TypeTableRow};

/// Include a generated docs registry emitted by `proa_docs_build`.
///
/// The argument is the source output directory name, usually `"docs"`.
#[macro_export]
macro_rules! include_docs {
    ($name:literal) => {
        include!(concat!(
            env!("OUT_DIR"),
            "/proa_docs/",
            $name,
            "/docs.generated.rs"
        ))
    };
}
