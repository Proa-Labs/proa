// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeSet,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

use crate::{
    config::{Config, DocsSource, LinkValidationMode, SearchConfig},
    frontmatter::{self, ParsedFrontmatter},
    highlight_cache::HighlightCache,
    link_validation::{
        validate_references, LinkDiagnostic, LinkGraph, LinkGraphPage, LinkValidationReport,
    },
    markdown::{render_markdown, render_markdown_with_cache, TocEntry},
    rust_emit, search,
};

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("missing OUT_DIR environment variable")]
    MissingOutDir,
    #[error("frontmatter error in {path}: {message}")]
    Frontmatter { path: PathBuf, message: String },
    #[error("metadata error in {path}: {message}")]
    Metadata { path: PathBuf, message: String },
    #[error("MDX lowering error in {path}: {message}")]
    MdxLowering { path: PathBuf, message: String },
    #[error("search JSON error: {0}")]
    SearchJson(#[from] serde_json::Error),
    #[error("link validation failed: {0} broken link(s)")]
    LinkValidation(usize),
    #[error("TypeScript docgen is not configured but {0} type table request(s) were found")]
    TypeScriptDocgenMissing(usize),
    #[error("unknown nav icon `{0}`; add it to proa_docs::icons or fix the spelling")]
    UnknownNavIcon(String),
}

pub fn compile(config: Config) {
    try_compile(config).expect("compile Proa docs");
}

pub fn compile_docs(source: DocsSource) {
    compile(Config::new().source(source));
}

pub fn try_compile(config: Config) -> Result<(), CompileError> {
    let out_dir = env::var_os("OUT_DIR").ok_or(CompileError::MissingOutDir)?;
    try_compile_to(config, Path::new(&out_dir))
}

pub fn try_compile_to(config: Config, out_dir: &Path) -> Result<(), CompileError> {
    for collection in config.collections() {
        let _ = collection.root();
        let _ = collection.collection_kind();
    }
    for plugin in config.plugins() {
        match plugin.kind() {
            crate::config::PluginKind::JsonSchema | crate::config::PluginKind::LastModified => {}
        }
    }
    let highlight_cache = config
        .sources()
        .iter()
        .any(|source| source.markdown.code_highlight.enabled)
        .then(|| HighlightCache::new(&out_dir.join(".proa-docs-highlight-cache")))
        .flatten();
    for source in config.sources() {
        compile_source(
            source,
            highlight_cache.as_ref(),
            &out_dir.join("proa_docs").join(&source.out_dir_name),
        )?;
    }
    Ok(())
}

pub fn validate_links_config(config: Config) -> Result<LinkValidationReport, CompileError> {
    let mut report = LinkValidationReport::default();
    for source in config.sources() {
        let compiled = compile_source_model(source)?;
        let source_report = validate_references(
            &compiled.link_graph,
            &compiled.link_references,
            &source.link_validation,
        );
        report.diagnostics.extend(source_report.diagnostics);
    }
    Ok(report)
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledSource {
    pub nav_title: String,
    pub nav_icon: Option<String>,
    pub pages: Vec<CompiledPage>,
    pub nav: Vec<CompiledNavItem>,
    pub link_graph: LinkGraph,
    pub link_references: Vec<PageLinkReferences>,
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledNavItem {
    Page {
        title: String,
        icon: Option<String>,
        path: String,
    },
    Group {
        title: String,
        icon: Option<String>,
        root: bool,
        children: Vec<CompiledNavItem>,
    },
    Separator {
        title: String,
        icon: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PageLinkReferences {
    pub source_path: String,
    pub route_path: String,
    pub hrefs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledPage {
    pub source_path: PathBuf,
    pub slug: String,
    pub route_path: String,
    pub markdown_path: String,
    pub title: String,
    pub description: Option<String>,
    pub sidebar_title: Option<String>,
    pub frontmatter: ResolvedFrontmatter,
    pub article_html: String,
    pub markdown: String,
    pub toc: Vec<TocEntry>,
    pub hashes: Vec<String>,
    pub search_text: String,
    pub html_include_path: String,
    pub markdown_include_path: String,
}

impl CompiledPage {
    pub(crate) fn sidebar_title(&self) -> &str {
        self.sidebar_title.as_deref().unwrap_or(&self.title)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedFrontmatter {
    pub title: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub slug: Option<String>,
    pub sidebar_title: Option<String>,
    pub nav_order: Option<i32>,
    pub hidden: bool,
    pub draft: bool,
    pub search: Option<bool>,
    pub toc: Option<bool>,
    pub last_modified: Option<String>,
    /// Extra search terms the page canonically documents (identifiers, API
    /// names, synonyms). Emitted to the search documents only.
    pub keywords: Vec<String>,
}

fn compile_source(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
    out_dir: &Path,
) -> Result<(), CompileError> {
    let compiled = compile_source_model_with_cache(source, highlight_cache)?;

    if matches!(source.link_validation.mode, LinkValidationMode::Error) {
        let report = validate_references(
            &compiled.link_graph,
            &compiled.link_references,
            &source.link_validation,
        );
        if !report.is_empty() {
            print_link_diagnostics(&report.diagnostics);
            return Err(CompileError::LinkValidation(report.diagnostics.len()));
        }
    }

    if out_dir.exists() {
        fs::remove_dir_all(out_dir).map_err(|source| CompileError::Io {
            path: out_dir.to_path_buf(),
            source,
        })?;
    }
    fs::create_dir_all(out_dir).map_err(|source| CompileError::Io {
        path: out_dir.to_path_buf(),
        source,
    })?;

    for page in &compiled.pages {
        write_generated_file(out_dir, &page.html_include_path, &page.article_html)?;
        write_generated_file(out_dir, &page.markdown_include_path, &page.markdown)?;
    }

    let search_paths = if matches!(source.search, SearchConfig::Disabled) {
        None
    } else {
        let (manifest_json, documents_json) =
            search::build_search_json(&compiled.pages, &compiled.nav, &source.base_path)?;
        write_generated_file(out_dir, "search/manifest.json", &manifest_json)?;
        write_generated_file(out_dir, "search/documents.json", &documents_json)?;
        Some(("search/manifest.json", "search/documents.json"))
    };

    let generated = rust_emit::emit_docs(
        &source.site_name,
        &source.base_path,
        &compiled.nav_title,
        compiled.nav_icon.as_deref(),
        &compiled.pages,
        &compiled.nav,
        search_paths,
    );
    write_generated_file(out_dir, "docs.generated.rs", &generated)
}

fn compile_source_model(source: &DocsSource) -> Result<CompiledSource, CompileError> {
    compile_source_model_with_cache(source, None)
}

fn compile_source_model_with_cache(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
) -> Result<CompiledSource, CompileError> {
    let root = &source.root;
    let mut pages = Vec::new();
    let mut link_references = Vec::new();
    let mut type_table_request_count = 0usize;
    let mut seen = BTreeSet::new();
    let nav;
    let nav_title;
    let nav_icon;

    if root.join("meta.json").is_file() {
        let compiled_nav = compile_folder_nav(
            source,
            highlight_cache,
            Path::new(""),
            &mut pages,
            &mut link_references,
            &mut type_table_request_count,
            &mut seen,
        )?;
        nav_title = compiled_nav.title;
        nav_icon = compiled_nav.icon;
        nav = compiled_nav.items;
        append_unlisted_pages(
            source,
            highlight_cache,
            &mut pages,
            &mut link_references,
            &mut type_table_request_count,
            &mut seen,
        )?;
    } else {
        nav_title = source.site_name.clone();
        nav_icon = None;
        let mut files = Vec::new();
        collect_markdown_files(root, root, &mut files)?;
        files.sort();
        for rel_path in files {
            if seen.insert(rel_path.clone()) {
                if let Some((page, refs, type_count)) =
                    compile_page_rel(source, highlight_cache, &rel_path)?
                {
                    type_table_request_count += type_count;
                    link_references.push(refs);
                    pages.push(page);
                }
            }
        }
        pages.sort_by(|a, b| {
            a.frontmatter
                .nav_order
                .cmp(&b.frontmatter.nav_order)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        nav = pages
            .iter()
            .filter(|page| !page.frontmatter.hidden)
            .map(|page| CompiledNavItem::Page {
                title: page.sidebar_title().to_string(),
                icon: page.frontmatter.icon.clone(),
                path: page.route_path.clone(),
            })
            .collect();
    }

    if type_table_request_count > 0
        && source.markdown.typescript.enabled
        && source.markdown.typescript.command.is_none()
    {
        return Err(CompileError::TypeScriptDocgenMissing(
            type_table_request_count,
        ));
    }

    let link_graph = LinkGraph {
        pages: pages
            .iter()
            .map(|page| LinkGraphPage {
                source_path: page.source_path.display().to_string(),
                route_path: page.route_path.clone(),
                hashes: page.hashes.clone(),
            })
            .collect(),
        assets: Vec::new(),
        redirects: Vec::new(),
    };

    Ok(CompiledSource {
        nav_title,
        nav_icon,
        pages,
        nav,
        link_graph,
        link_references,
    })
}

#[derive(Debug, Deserialize)]
struct Meta {
    title: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    root: bool,
    #[serde(default)]
    pages: Vec<MetaEntry>,
}

struct CompiledFolderNav {
    title: String,
    icon: Option<String>,
    items: Vec<CompiledNavItem>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MetaEntry {
    Item(String),
    /// Object form of a separator, carrying an optional nav icon:
    /// `{ "separator": "Data", "icon": "Database" }`. The `"---Data---"`
    /// string form stays supported and compiles to the same node without
    /// an icon.
    Separator {
        separator: String,
        #[serde(default)]
        icon: Option<String>,
    },
    Group {
        title: String,
        pages: Vec<MetaEntry>,
    },
}

fn compile_folder_nav(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
    rel_dir: &Path,
    pages: &mut Vec<CompiledPage>,
    link_references: &mut Vec<PageLinkReferences>,
    type_table_request_count: &mut usize,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<CompiledFolderNav, CompileError> {
    let meta = read_meta(&source.root.join(rel_dir).join("meta.json"))?;
    let title = meta
        .title
        .clone()
        .unwrap_or_else(|| source.site_name.clone());
    let items = compile_meta_entries(
        source,
        highlight_cache,
        rel_dir,
        &meta.pages,
        pages,
        link_references,
        type_table_request_count,
        seen,
    )?;
    Ok(CompiledFolderNav {
        title,
        icon: meta.icon,
        items,
    })
}

fn compile_meta_entries(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
    rel_dir: &Path,
    entries: &[MetaEntry],
    pages: &mut Vec<CompiledPage>,
    link_references: &mut Vec<PageLinkReferences>,
    type_table_request_count: &mut usize,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<Vec<CompiledNavItem>, CompileError> {
    let mut nav = Vec::new();
    for entry in entries {
        match entry {
            MetaEntry::Separator { separator, icon } => {
                validate_nav_icon(icon.as_deref())?;
                nav.push(CompiledNavItem::Separator {
                    title: separator.clone(),
                    icon: icon.clone(),
                });
                continue;
            }
            MetaEntry::Item(item) => {
                if let Some(title) = separator_title(item) {
                    nav.push(CompiledNavItem::Separator { title, icon: None });
                    continue;
                }

                let candidate_dir = rel_dir.join(item);
                if source.root.join(&candidate_dir).is_dir() {
                    let meta = read_meta(&source.root.join(&candidate_dir).join("meta.json"))?;
                    let title = meta.title.clone().unwrap_or_else(|| title_from_slug(item));
                    let icon = meta.icon.clone();
                    let children = compile_meta_entries(
                        source,
                        highlight_cache,
                        &candidate_dir,
                        &meta.pages,
                        pages,
                        link_references,
                        type_table_request_count,
                        seen,
                    )?;
                    nav.push(CompiledNavItem::Group {
                        title,
                        icon,
                        root: meta.root,
                        children,
                    });
                    continue;
                }

                let Some(rel_path) = resolve_markdown_rel_path(&source.root, rel_dir, item) else {
                    return Err(CompileError::Metadata {
                        path: source.root.join(rel_dir).join("meta.json"),
                        message: format!(
                            "entry `{item}` did not resolve to a folder or markdown file"
                        ),
                    });
                };
                if seen.insert(rel_path.clone()) {
                    if let Some((page, refs, type_count)) =
                        compile_page_rel(source, highlight_cache, &rel_path)?
                    {
                        *type_table_request_count += type_count;
                        if !page.frontmatter.hidden {
                            nav.push(CompiledNavItem::Page {
                                title: page.sidebar_title().to_string(),
                                icon: page.frontmatter.icon.clone(),
                                path: page.route_path.clone(),
                            });
                        }
                        link_references.push(refs);
                        pages.push(page);
                    }
                }
            }
            MetaEntry::Group {
                title,
                pages: group_pages,
            } => {
                let children = compile_meta_entries(
                    source,
                    highlight_cache,
                    rel_dir,
                    group_pages,
                    pages,
                    link_references,
                    type_table_request_count,
                    seen,
                )?;
                nav.push(CompiledNavItem::Group {
                    title: title.clone(),
                    icon: None,
                    root: false,
                    children,
                });
            }
        }
    }
    Ok(nav)
}

fn append_unlisted_pages(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
    pages: &mut Vec<CompiledPage>,
    link_references: &mut Vec<PageLinkReferences>,
    type_table_request_count: &mut usize,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), CompileError> {
    let mut files = Vec::new();
    collect_markdown_files(&source.root, &source.root, &mut files)?;
    files.sort();
    for rel_path in files {
        if seen.insert(rel_path.clone()) {
            if let Some((page, refs, type_count)) =
                compile_page_rel(source, highlight_cache, &rel_path)?
            {
                *type_table_request_count += type_count;
                link_references.push(refs);
                pages.push(page);
            }
        }
    }
    Ok(())
}

fn compile_page_rel(
    source: &DocsSource,
    highlight_cache: Option<&HighlightCache>,
    rel_path: &Path,
) -> Result<Option<(CompiledPage, PageLinkReferences, usize)>, CompileError> {
    let source_path = source.root.join(rel_path);
    let raw = fs::read_to_string(&source_path).map_err(|source| CompileError::Io {
        path: source_path.clone(),
        source,
    })?;
    let processed = if matches!(
        source_path.extension().and_then(|ext| ext.to_str()),
        Some("mdx")
    ) && source.markdown.mdx.enabled
    {
        crate::mdx::lower_mdx(&raw, &source.markdown.mdx).map_err(|message| {
            CompileError::MdxLowering {
                path: source_path.clone(),
                message,
            }
        })?
    } else {
        raw.clone()
    };
    let (frontmatter, markdown) =
        frontmatter::split_frontmatter(&processed).map_err(|message| {
            CompileError::Frontmatter {
                path: source_path.clone(),
                message,
            }
        })?;
    frontmatter::validate_schema(&source.schema, &frontmatter).map_err(|message| {
        CompileError::Frontmatter {
            path: source_path.clone(),
            message,
        }
    })?;

    let resolved_frontmatter = resolve_frontmatter(&frontmatter);
    if resolved_frontmatter.draft && !source.include_drafts {
        return Ok(None);
    }

    let slug = resolved_frontmatter
        .slug
        .clone()
        .unwrap_or_else(|| slug_from_rel_path(rel_path));
    let route_path = route_path(&source.base_path, &slug);
    let markdown_path = markdown_route_path(&source.base_path, &slug);
    let rendered = match highlight_cache {
        Some(cache) => {
            render_markdown_with_cache(markdown, &source_path, &source.markdown, Some(cache))
        }
        None => render_markdown(markdown, &source_path, &source.markdown),
    };
    let type_count = rendered.type_table_requests.len();

    let title = resolved_frontmatter
        .title
        .clone()
        .or_else(|| infer_title(markdown))
        .unwrap_or_else(|| title_from_slug(&slug));
    let hashes = rendered.hashes;
    let description = resolved_frontmatter
        .description
        .clone()
        .or_else(|| infer_description(markdown));
    let include_stem = if slug.is_empty() {
        "pages/index".to_string()
    } else {
        format!("pages/{slug}")
    };

    let refs = PageLinkReferences {
        source_path: source_path.display().to_string(),
        route_path: route_path.clone(),
        hrefs: rendered.links.into_iter().map(|link| link.href).collect(),
    };

    let page = CompiledPage {
        source_path,
        slug,
        route_path,
        markdown_path,
        title,
        description,
        sidebar_title: resolved_frontmatter.sidebar_title.clone(),
        frontmatter: resolved_frontmatter,
        article_html: rendered.html,
        markdown: processed,
        toc: rendered.toc,
        hashes,
        search_text: rendered.search_text,
        html_include_path: format!("{include_stem}.html"),
        markdown_include_path: format!("{include_stem}.md"),
    };

    Ok(Some((page, refs, type_count)))
}

fn read_meta(path: &Path) -> Result<Meta, CompileError> {
    let raw = fs::read_to_string(path).map_err(|source| CompileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|error| CompileError::Metadata {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn resolve_markdown_rel_path(root: &Path, rel_dir: &Path, item: &str) -> Option<PathBuf> {
    let path = rel_dir.join(item);
    if path.extension().is_some() {
        return root.join(&path).is_file().then_some(path);
    }

    for extension in ["md", "mdx"] {
        let candidate = path.with_extension(extension);
        if root.join(&candidate).is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Reject an icon name the renderer cannot draw.
///
/// `nav_icon_svg` falls back to a document glyph for anything unknown, so a
/// typo in frontmatter used to render a plausible-looking wrong icon and
/// stay invisible. Failing the build makes the typo loud.
fn validate_nav_icon(icon: Option<&str>) -> Result<(), CompileError> {
    let Some(icon) = icon else {
        return Ok(());
    };
    if !KNOWN_NAV_ICONS.contains(&icon) {
        return Err(CompileError::UnknownNavIcon(icon.to_string()));
    }
    Ok(())
}

/// Icon names `proa_docs::icons::nav_icon_svg` can draw.
///
/// Duplicated here rather than imported: this is a build-time crate and
/// depending on the runtime crate would pull the whole render stack into
/// every consumer's build graph. `known_nav_icons_match_the_renderer` reads
/// the renderer source and fails if the two lists drift.
const KNOWN_NAV_ICONS: &[&str] = &[
    "ArrowRightLeft",
    "BookOpen",
    "Bot",
    "Braces",
    "Brain",
    "Cpu",
    "Database",
    "FileText",
    "Layers",
    "LifeBuoy",
    "Palette",
    "Rocket",
    "Route",
    "Settings",
    "ShieldCheck",
    "Terminal",
    "Zap",
];

fn separator_title(item: &str) -> Option<String> {
    item.strip_prefix("---")
        .and_then(|rest| rest.strip_suffix("---"))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
}

fn collect_markdown_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), CompileError> {
    let entries = fs::read_dir(dir).map_err(|source| CompileError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CompileError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(root, &path, files)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("md" | "mdx")
        ) {
            files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

fn resolve_frontmatter(frontmatter: &ParsedFrontmatter) -> ResolvedFrontmatter {
    ResolvedFrontmatter {
        title: frontmatter.get_str("title").map(ToOwned::to_owned),
        description: frontmatter.get_str("description").map(ToOwned::to_owned),
        icon: frontmatter.get_str("icon").map(ToOwned::to_owned),
        slug: frontmatter.get_str("slug").map(normalize_slug_value),
        sidebar_title: frontmatter.get_str("sidebar_title").map(ToOwned::to_owned),
        nav_order: frontmatter
            .get_i64("nav_order")
            .and_then(|value| i32::try_from(value).ok()),
        hidden: frontmatter.get_bool("hidden").unwrap_or(false),
        draft: frontmatter.get_bool("draft").unwrap_or(false),
        search: frontmatter.get_bool("search"),
        toc: frontmatter.get_bool("toc"),
        last_modified: frontmatter.get_str("last_modified").map(ToOwned::to_owned),
        keywords: frontmatter.get_string_list("keywords"),
    }
}

fn slug_from_rel_path(rel_path: &Path) -> String {
    let without_ext = rel_path.with_extension("");
    let parts = without_ext
        .iter()
        .filter_map(|part| part.to_str())
        .collect::<Vec<_>>();
    if parts.last() == Some(&"index") {
        parts[..parts.len().saturating_sub(1)].join("/")
    } else {
        parts.join("/")
    }
}

fn normalize_slug_value(value: &str) -> String {
    value.trim_matches('/').to_string()
}

fn route_path(base_path: &str, slug: &str) -> String {
    let base = base_path.trim_end_matches('/');
    if slug.is_empty() {
        if base.is_empty() {
            "/".to_string()
        } else {
            base.to_string()
        }
    } else if base.is_empty() {
        format!("/{slug}")
    } else {
        format!("{base}/{slug}")
    }
}

fn markdown_route_path(base_path: &str, slug: &str) -> String {
    let base = base_path.trim_end_matches('/');
    if slug.is_empty() {
        format!("{base}/index.md")
    } else {
        format!("{base}/{slug}.md")
    }
}

fn infer_title(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn infer_description(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with("---")
        {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn title_from_slug(slug: &str) -> String {
    slug.rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("Overview")
        .replace('-', " ")
}

fn write_generated_file(out_dir: &Path, rel: &str, contents: &str) -> Result<(), CompileError> {
    let path = out_dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CompileError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, contents).map_err(|source| CompileError::Io { path, source })
}

fn print_link_diagnostics(diagnostics: &[LinkDiagnostic]) {
    for diagnostic in diagnostics {
        println!(
            "cargo:warning=docs link validation: {}: {} ({})",
            diagnostic.source_path, diagnostic.message, diagnostic.href
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CodeHighlight, DocsSource, MarkdownPipeline, MdxConfig, MermaidConfig};

    #[test]
    fn renders_tabs_with_fenced_children_and_keeps_the_markdown_authorable() {
        // The whole point of a container component: children keep the normal
        // pipeline (highlighting, copy button) while the tags become chrome.
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.mdx"),
            "---\ntitle: Install\n---\n\n<Tabs items={['macOS and Linux', 'Windows']}>\n\n<Tab value=\"macOS and Linux\">\n\n```bash\ncurl -fsSL https://proa.so/install.sh | sh\n```\n\n</Tab>\n\n<Tab value=\"Windows\">\n\n```powershell\nirm https://proa.so/install.ps1 | iex\n```\n\n</Tab>\n\n</Tabs>\n",
        )
        .unwrap();
        let out_dir = tempfile::tempdir().unwrap();

        let markdown = MarkdownPipeline::default()
            .code_highlighting(CodeHighlight::new())
            .mdx(MdxConfig::proa());

        try_compile_to(
            Config::new().source(
                DocsSource::new(source_dir.path())
                    .site_name("Example")
                    .markdown(markdown),
            ),
            out_dir.path(),
        )
        .unwrap();

        let html =
            fs::read_to_string(out_dir.path().join("proa_docs/docs/pages/index.html")).unwrap();
        // Chrome replaced the tags rather than escaping them onto the page.
        assert!(!html.contains("&lt;Tabs"), "raw tag leaked: {html}");
        assert!(!html.contains("&lt;Tab "), "raw tag leaked: {html}");
        assert!(html.contains("class=\"proa-docs-tabs\""));
        assert!(html.contains(">macOS and Linux</label>"));
        assert!(html.contains(">Windows</label>"));
        // Children still went through the code-block renderer.
        assert!(html.contains("data-lang=\"bash\""));
        assert!(html.contains("data-lang=\"powershell\""));
        assert!(html.contains("data-code-copy"));
    }

    #[test]
    fn compiles_mdx_filenames_into_code_block_headers() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.mdx"),
            "---\ntitle: Example\n---\n\n```rust filename=\"src/components/hero card.rs\" {1}\nfn hero() {}\n```\n",
        )
        .unwrap();
        let out_dir = tempfile::tempdir().unwrap();

        let markdown = MarkdownPipeline::default()
            .code_highlighting(CodeHighlight::new())
            .mdx(MdxConfig::proa());

        try_compile_to(
            Config::new().source(
                DocsSource::new(source_dir.path())
                    .site_name("Example")
                    .markdown(markdown),
            ),
            out_dir.path(),
        )
        .unwrap();

        let html =
            fs::read_to_string(out_dir.path().join("proa_docs/docs/pages/index.html")).unwrap();
        assert!(html.contains("data-filename=\"src/components/hero card.rs\""));
        assert!(html.contains("data-code-filename>src/components/hero card.rs</span>"));
        assert!(html.contains("data-code-header"));
        assert!(html.contains("data-highlighted-line"));
    }

    #[test]
    fn compiles_docs_to_generated_files() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.md"),
            "---\ntitle: Intro\ndescription: Start here\n---\n# Intro\n\n## Install\n\nHello",
        )
        .unwrap();
        let out_dir = tempfile::tempdir().unwrap();

        try_compile_to(
            Config::new().source(DocsSource::new(source_dir.path()).site_name("Example")),
            out_dir.path(),
        )
        .unwrap();

        let generated = fs::read_to_string(
            out_dir
                .path()
                .join("proa_docs")
                .join("docs")
                .join("docs.generated.rs"),
        )
        .unwrap();
        assert!(generated.contains("proa_docs::Docs"));
        assert!(generated.contains("Intro"));
        assert!(out_dir
            .path()
            .join("proa_docs/docs/pages/index.html")
            .exists());
    }

    #[test]
    fn compiles_highlighted_code_into_the_generated_html_only() {
        let source_dir = tempfile::tempdir().unwrap();
        let markdown = "# Intro\n\n```rust\npub fn answer() -> u64 { 42 }\n```\n";
        fs::write(source_dir.path().join("index.md"), markdown).unwrap();
        let out_dir = tempfile::tempdir().unwrap();

        try_compile_to(
            Config::new().source(
                DocsSource::new(source_dir.path())
                    .markdown(MarkdownPipeline::default().code_highlighting(CodeHighlight::new())),
            ),
            out_dir.path(),
        )
        .unwrap();

        let pages = out_dir.path().join("proa_docs/docs/pages");
        let generated_html = fs::read_to_string(pages.join("index.html")).unwrap();
        let generated_markdown = fs::read_to_string(pages.join("index.md")).unwrap();
        assert!(generated_html.contains("class=\"language-rust\""));
        assert!(generated_html.contains("class=\"hl-keyword\""));
        assert_eq!(generated_markdown, markdown);
        assert!(!generated_markdown.contains("hl-keyword"));
    }

    #[test]
    fn mermaid_config_emits_diagram_shell() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.md"),
            "# Intro\n\n```mermaid\ngraph TD; A-->B;\n```",
        )
        .unwrap();
        let model = compile_source_model(
            &DocsSource::new(source_dir.path())
                .markdown(crate::config::MarkdownPipeline::default().mermaid(MermaidConfig::new())),
        )
        .unwrap();
        assert!(model.pages[0]
            .article_html
            .contains("data-proa-docs-mermaid"));
    }

    #[test]
    fn leading_article_h1_is_preserved_in_article_and_toc() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.md"),
            "---\ntitle: Requirements\n---\n\n# Requirements\n\n## Rust\n\nInstall Rust.",
        )
        .unwrap();
        let model = compile_source_model(&DocsSource::new(source_dir.path())).unwrap();
        let page = &model.pages[0];

        assert!(page
            .article_html
            .contains("<h1 id=\"requirements\">Requirements</h1>"));
        assert_eq!(page.toc[0].depth, 2);
        assert_eq!(page.toc[0].id, "rust");
        assert!(page.hashes.iter().any(|hash| hash == "requirements"));
    }

    #[test]
    fn leading_article_h1_hash_still_validates() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.md"),
            "---\ntitle: Requirements\n---\n\n# Requirements\n\n[Top](#requirements)\n\n## Rust",
        )
        .unwrap();
        let report = crate::validate_links(
            Config::new().source(
                DocsSource::new(source_dir.path())
                    .link_validation(crate::config::LinkValidation::strict()),
            ),
        )
        .unwrap();
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    }

    #[test]
    fn validates_broken_internal_links() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("index.md"),
            "# Intro\n\n[Missing](./missing)",
        )
        .unwrap();
        let report = crate::validate_links(
            Config::new().source(
                DocsSource::new(source_dir.path())
                    .link_validation(crate::config::LinkValidation::strict()),
            ),
        )
        .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert!(report.diagnostics[0]
            .message
            .contains("unknown internal route"));
    }

    #[test]
    fn meta_json_controls_order_and_nested_nav() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(source_dir.path().join("reference")).unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"title":"Docs","pages":["index","reference","about"]}"#,
        )
        .unwrap();
        fs::write(
            source_dir.path().join("reference/meta.json"),
            r#"{"title":"Reference","icon":"BookOpen","root":true,"pages":["api"]}"#,
        )
        .unwrap();
        fs::write(source_dir.path().join("index.md"), "# Intro\n").unwrap();
        fs::write(source_dir.path().join("about.md"), "# About\n").unwrap();
        fs::write(source_dir.path().join("reference/api.md"), "# API\n").unwrap();

        let model = compile_source_model(&DocsSource::new(source_dir.path())).unwrap();
        assert_eq!(model.nav_title, "Docs");
        assert_eq!(model.nav_icon, None);
        assert_eq!(
            model
                .pages
                .iter()
                .map(|page| page.slug.as_str())
                .collect::<Vec<_>>(),
            ["", "reference/api", "about"]
        );
        match &model.nav[1] {
            CompiledNavItem::Group {
                title,
                icon,
                root,
                children,
            } => {
                assert_eq!(title, "Reference");
                assert_eq!(icon.as_deref(), Some("BookOpen"));
                assert!(*root);
                assert_eq!(children.len(), 1);
            }
            other => panic!("expected reference group, got {other:?}"),
        }
    }

    #[test]
    fn hidden_pages_compile_without_appearing_in_navigation() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"title":"Docs","pages":["index","internal"]}"#,
        )
        .unwrap();
        fs::write(source_dir.path().join("index.md"), "# Intro\n").unwrap();
        fs::write(
            source_dir.path().join("internal.md"),
            "---\nhidden: true\n---\n# Internal\n",
        )
        .unwrap();

        let model = compile_source_model(&DocsSource::new(source_dir.path())).unwrap();

        assert_eq!(model.pages.len(), 2);
        assert!(model.pages.iter().any(|page| page.slug == "internal"));
        assert_eq!(model.nav.len(), 1);
        assert!(matches!(
            &model.nav[0],
            CompiledNavItem::Page { path, .. } if path == "/docs"
        ));
    }

    #[test]
    fn meta_json_resolves_extensionless_mdx_pages_when_lowering_is_enabled() {
        let source_dir = tempfile::tempdir().unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"title":"Docs","pages":["index","guide"]}"#,
        )
        .unwrap();
        fs::write(source_dir.path().join("index.md"), "# Intro\n").unwrap();
        fs::write(
            source_dir.path().join("guide.mdx"),
            "# Guide\n\n<Cards>\n  <Card title=\"API\" href=\"/docs/api\" />\n</Cards>\n",
        )
        .unwrap();

        let model = compile_source_model(
            &DocsSource::new(source_dir.path())
                .markdown(crate::config::MarkdownPipeline::default().mdx(MdxConfig::proa())),
        )
        .unwrap();

        let guide = model
            .pages
            .iter()
            .find(|page| page.slug == "guide")
            .expect("guide page");
        assert_eq!(guide.route_path, "/docs/guide");
        assert!(guide.markdown.contains("- [API](/docs/api)"));
        assert!(!guide.markdown.contains("<Cards>"));
    }
}

#[cfg(test)]
mod nav_icon_tests {
    use super::{validate_nav_icon, KNOWN_NAV_ICONS};

    /// The renderer's match arms are the source of truth. If someone adds an
    /// icon there without adding it here, authors get a build error for a
    /// name that actually works; the reverse ships a silent wrong glyph.
    #[test]
    fn known_nav_icons_match_the_renderer() {
        // Read at run time instead of with `include_str!`. The sibling crate's
        // source sits outside this crate's published archive, so a compile-time
        // include would leave the packaged crate unable to build its tests. In
        // an archive the sibling is simply absent and the drift check is moot.
        let sibling = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../proa_docs");
        if !sibling.is_dir() {
            return;
        }
        let icons = sibling.join("src/icons.rs");
        let source = std::fs::read_to_string(&icons)
            .unwrap_or_else(|error| panic!("read {}: {error}", icons.display()));
        let body = source
            .split_once("pub fn nav_icon_svg")
            .expect("nav_icon_svg exists")
            .1
            .split_once("_ => FILE_TEXT_ICON")
            .expect("fallback arm exists")
            .0;

        let mut rendered: Vec<&str> = body
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line.strip_prefix('"')?;
                let (name, _) = rest.split_once('"')?;
                Some(name)
            })
            .collect();
        rendered.sort_unstable();

        let mut known = KNOWN_NAV_ICONS.to_vec();
        known.sort_unstable();

        assert_eq!(
            rendered, known,
            "nav icon lists drifted between proa_docs and proa_docs_build"
        );
    }

    #[test]
    fn an_unknown_icon_fails_the_build() {
        assert!(validate_nav_icon(Some("Database")).is_ok());
        assert!(validate_nav_icon(None).is_ok());
        assert!(validate_nav_icon(Some("Bot")).is_ok());
        // The old silent-fallback case: a plausible typo.
        assert!(validate_nav_icon(Some("DataBase")).is_err());
        assert!(validate_nav_icon(Some("NotAnIcon")).is_err());
    }
}

#[cfg(test)]
mod highlight_cache_tests {
    use super::*;

    fn generated_files(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = std::collections::BTreeMap::new();
        walk(root, root, &mut files);
        files
    }

    fn assert_cached_output_matches_uncached(
        source: &DocsSource,
        cache: &HighlightCache,
        output: &Path,
    ) {
        compile_source(source, Some(cache), output).unwrap();
        let uncached = tempfile::tempdir().unwrap();
        compile_source(source, None, uncached.path()).unwrap();
        assert_eq!(generated_files(output), generated_files(uncached.path()));
    }

    #[test]
    fn cached_highlighting_keeps_page_metadata_navigation_and_search_fresh() {
        let source_dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = HighlightCache::new(cache_dir.path()).unwrap();
        let index = source_dir.path().join("index.md");
        let markdown = "---\ntitle: Original\nslug: old\n---\n\n# Intro\n\n```rust filename=\"before.rs\" {1}\nlet sample = \"<script> & text\";\nlet next = 2;\n```\n\n```unknown\n<unsafe>&text\n```\n";
        fs::write(&index, markdown).unwrap();
        fs::write(source_dir.path().join("guide.md"), "# Guide\n").unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"title":"Original","pages":["index","guide"]}"#,
        )
        .unwrap();
        let source = DocsSource::new(source_dir.path()).markdown(
            crate::config::MarkdownPipeline::default()
                .code_highlighting(crate::config::CodeHighlight::new()),
        );
        assert_cached_output_matches_uncached(&source, &cache, output.path());
        assert_cached_output_matches_uncached(&source, &cache, output.path());

        // Code bytes stay identical while all surrounding page/fence metadata changes.
        fs::write(
            &index,
            markdown
                .replace("Original", "Changed")
                .replace("slug: old", "slug: renamed")
                .replace("before.rs", "after.rs")
                .replace("{1}", "{2}"),
        )
        .unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"title":"Changed","pages":["guide","index"]}"#,
        )
        .unwrap();
        let source = source.base_path("/manual");
        assert_cached_output_matches_uncached(&source, &cache, output.path());
        assert!(!output.path().join("pages/old.html").exists());

        fs::remove_file(source_dir.path().join("guide.md")).unwrap();
        fs::write(
            source_dir.path().join("meta.json"),
            r#"{"pages":["index"]}"#,
        )
        .unwrap();
        assert_cached_output_matches_uncached(&source, &cache, output.path());
        assert!(!output.path().join("pages/guide.html").exists());
    }

    #[test]
    fn cached_fences_do_not_hide_new_broken_links() {
        let source_dir = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = HighlightCache::new(cache_dir.path()).unwrap();
        let index = source_dir.path().join("index.md");
        let markdown = "# Intro\n\n```rust\nlet sample = 3;\n```\n";
        fs::write(&index, markdown).unwrap();
        let source = DocsSource::new(source_dir.path())
            .markdown(
                crate::config::MarkdownPipeline::default()
                    .code_highlighting(crate::config::CodeHighlight::new()),
            )
            .link_validation(crate::config::LinkValidation::strict());
        compile_source(&source, Some(&cache), output.path()).unwrap();
        fs::write(&index, format!("{markdown}\n[Missing](/docs/missing)\n")).unwrap();
        for cache in [Some(&cache), None] {
            assert!(matches!(
                compile_source(&source, cache, output.path()),
                Err(CompileError::LinkValidation(1))
            ));
        }
    }
}
