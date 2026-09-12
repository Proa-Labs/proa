// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub(crate) sources: Vec<DocsSource>,
    pub(crate) collections: Vec<CollectionSource>,
    pub(crate) plugins: Vec<Plugin>,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn source(mut self, source: DocsSource) -> Self {
        self.sources.push(source);
        self
    }

    pub fn collection(mut self, collection: CollectionSource) -> Self {
        self.collections.push(collection);
        self
    }

    pub fn plugin(mut self, plugin: Plugin) -> Self {
        self.plugins.push(plugin);
        self
    }

    pub fn sources(&self) -> &[DocsSource] {
        &self.sources
    }

    pub fn collections(&self) -> &[CollectionSource] {
        &self.collections
    }

    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }
}

#[derive(Debug, Clone)]
pub struct DocsSource {
    pub(crate) root: PathBuf,
    pub(crate) out_dir_name: String,
    pub(crate) base_path: String,
    pub(crate) site_name: String,
    pub(crate) schema: PageSchema,
    pub(crate) meta_schema: MetaSchema,
    pub(crate) postprocess: Postprocess,
    pub(crate) markdown: MarkdownPipeline,
    pub(crate) search: SearchConfig,
    pub(crate) link_validation: LinkValidation,
    pub(crate) include_drafts: bool,
}

impl DocsSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            out_dir_name: "docs".to_string(),
            base_path: "/docs".to_string(),
            site_name: "Docs".to_string(),
            schema: PageSchema::default(),
            meta_schema: MetaSchema::default(),
            postprocess: Postprocess::default(),
            markdown: MarkdownPipeline::default(),
            search: SearchConfig::orama_client(),
            link_validation: LinkValidation::default(),
            include_drafts: false,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn out_dir_name(mut self, out_dir_name: impl Into<String>) -> Self {
        self.out_dir_name = out_dir_name.into();
        self
    }

    pub fn base_path(mut self, base_path: impl Into<String>) -> Self {
        self.base_path = normalize_base_path(&base_path.into());
        self
    }

    pub fn site_name(mut self, site_name: impl Into<String>) -> Self {
        self.site_name = site_name.into();
        self
    }

    pub fn schema(mut self, schema: PageSchema) -> Self {
        self.schema = schema;
        self
    }

    pub fn meta_schema(mut self, meta_schema: MetaSchema) -> Self {
        self.meta_schema = meta_schema;
        self
    }

    pub fn postprocess(mut self, postprocess: Postprocess) -> Self {
        self.postprocess = postprocess;
        self
    }

    pub fn markdown(mut self, markdown: MarkdownPipeline) -> Self {
        self.markdown = markdown;
        self
    }

    pub fn search(mut self, search: SearchConfig) -> Self {
        self.search = search;
        self
    }

    pub fn link_validation(mut self, link_validation: LinkValidation) -> Self {
        self.link_validation = link_validation;
        self
    }

    pub fn include_drafts(mut self, include_drafts: bool) -> Self {
        self.include_drafts = include_drafts;
        self
    }
}

fn normalize_base_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        let without_trailing = trimmed.trim_end_matches('/');
        if without_trailing.starts_with('/') {
            without_trailing.to_string()
        } else {
            format!("/{without_trailing}")
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageSchema {
    pub(crate) fields: Vec<SchemaField>,
}

impl PageSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn optional_string(mut self, name: impl Into<String>) -> Self {
        self.fields
            .push(SchemaField::new(name, SchemaType::String, false));
        self
    }

    pub fn required_string(mut self, name: impl Into<String>) -> Self {
        self.fields
            .push(SchemaField::new(name, SchemaType::String, true));
        self
    }

    pub fn bool_with_default(mut self, name: impl Into<String>, default: bool) -> Self {
        self.fields
            .push(SchemaField::new(name, SchemaType::Bool { default }, false));
        self
    }

    pub fn date(mut self, name: impl Into<String>) -> Self {
        self.fields
            .push(SchemaField::new(name, SchemaType::Date, false));
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct MetaSchema {
    pub(crate) fields: Vec<SchemaField>,
}

impl MetaSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn optional_string(mut self, name: impl Into<String>) -> Self {
        self.fields
            .push(SchemaField::new(name, SchemaType::String, false));
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SchemaField {
    pub name: String,
    pub ty: SchemaType,
    pub required: bool,
}

impl SchemaField {
    fn new(name: impl Into<String>, ty: SchemaType, required: bool) -> Self {
        Self {
            name: name.into(),
            ty,
            required,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SchemaType {
    String,
    Bool { default: bool },
    Date,
}

#[derive(Debug, Clone, Copy)]
pub struct Postprocess {
    pub include_processed_markdown: bool,
    pub extract_link_references: bool,
    pub export_element_ids: bool,
}

impl Default for Postprocess {
    fn default() -> Self {
        Self {
            include_processed_markdown: true,
            extract_link_references: true,
            export_element_ids: true,
        }
    }
}

impl Postprocess {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn include_processed_markdown(mut self, value: bool) -> Self {
        self.include_processed_markdown = value;
        self
    }

    pub fn extract_link_references(mut self, value: bool) -> Self {
        self.extract_link_references = value;
        self
    }

    pub fn export_element_ids(mut self, value: bool) -> Self {
        self.export_element_ids = value;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct MarkdownPipeline {
    pub code_highlight: CodeHighlight,
    pub math: MathMode,
    pub component_indexing: ComponentIndexing,
    pub mermaid: MermaidConfig,
    pub mdx: MdxConfig,
    pub typescript: crate::typescript::TypeScriptDocgen,
}

impl MarkdownPipeline {
    pub fn code_highlighting(mut self, code_highlight: CodeHighlight) -> Self {
        self.code_highlight = code_highlight;
        self
    }

    pub fn math(mut self, math: MathMode) -> Self {
        self.math = math;
        self
    }

    pub fn component_indexing(mut self, component_indexing: ComponentIndexing) -> Self {
        self.component_indexing = component_indexing;
        self
    }

    pub fn mermaid(mut self, mermaid: MermaidConfig) -> Self {
        self.mermaid = mermaid;
        self
    }

    pub fn mdx(mut self, mdx: MdxConfig) -> Self {
        self.mdx = mdx;
        self
    }

    pub fn typescript(mut self, typescript: crate::typescript::TypeScriptDocgen) -> Self {
        self.typescript = typescript;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodeHighlight {
    /// Highlight recognized code fences with Syntect during the docs build.
    ///
    /// Highlighted tokens use stable semantic `hl-*` classes so the site owns
    /// their colors. Unknown languages fall back to escaped plain code.
    pub enabled: bool,
}

impl CodeHighlight {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct MdxConfig {
    pub enabled: bool,
    pub strip_imports: bool,
    pub strip_comments: bool,
    pub reject_exports: bool,
    pub normalize_brace_escapes: bool,
    pub normalize_fence_metadata: bool,
    pub normalize_class_name: bool,
    pub normalize_style_objects: bool,
    pub remove_layout_divs: bool,
    pub lower_paragraph_tags: bool,
    pub cards: bool,
    pub files: bool,
    pub agent_terminal: bool,
    pub images: MdxImageConfig,
}

impl MdxConfig {
    pub fn new() -> Self {
        Self {
            enabled: true,
            strip_imports: true,
            strip_comments: true,
            reject_exports: true,
            normalize_brace_escapes: true,
            normalize_fence_metadata: true,
            normalize_class_name: true,
            normalize_style_objects: true,
            remove_layout_divs: false,
            lower_paragraph_tags: false,
            cards: false,
            files: false,
            agent_terminal: false,
            images: MdxImageConfig::disabled(),
        }
    }

    /// Preset used by the Proa marketing/docs site.
    ///
    /// It supports the small static helper subset this repository authors
    /// today: `<Cards>`, `<Files>`, `<AgentTerminal />`, layout `<div>` removal,
    /// simple paragraph/image lowering, className normalization, style-object
    /// normalization, and code-fence metadata normalization.
    pub fn proa() -> Self {
        Self::new()
            .remove_layout_divs(true)
            .lower_paragraph_tags(true)
            .cards(true)
            .files(true)
            .agent_terminal(true)
            .images(MdxImageConfig::new().rewrite_prefix("/images/", "/static/images/"))
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn strip_comments(mut self, value: bool) -> Self {
        self.strip_comments = value;
        self
    }

    pub fn strip_imports(mut self, value: bool) -> Self {
        self.strip_imports = value;
        self
    }

    pub fn reject_exports(mut self, value: bool) -> Self {
        self.reject_exports = value;
        self
    }

    pub fn normalize_brace_escapes(mut self, value: bool) -> Self {
        self.normalize_brace_escapes = value;
        self
    }

    /// Keep supported code-fence metadata (`filename="..."` and selected-line
    /// ranges) while removing metadata the Markdown renderer does not consume.
    pub fn normalize_fence_metadata(mut self, value: bool) -> Self {
        self.normalize_fence_metadata = value;
        self
    }

    pub fn normalize_class_name(mut self, value: bool) -> Self {
        self.normalize_class_name = value;
        self
    }

    pub fn normalize_style_objects(mut self, value: bool) -> Self {
        self.normalize_style_objects = value;
        self
    }

    pub fn remove_layout_divs(mut self, value: bool) -> Self {
        self.remove_layout_divs = value;
        self
    }

    pub fn lower_paragraph_tags(mut self, value: bool) -> Self {
        self.lower_paragraph_tags = value;
        self
    }

    pub fn cards(mut self, value: bool) -> Self {
        self.cards = value;
        self
    }

    pub fn files(mut self, value: bool) -> Self {
        self.files = value;
        self
    }

    pub fn agent_terminal(mut self, value: bool) -> Self {
        self.agent_terminal = value;
        self
    }

    pub fn images(mut self, images: MdxImageConfig) -> Self {
        self.images = images;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct MdxImageConfig {
    pub enabled: bool,
    pub default_alt: String,
    pub prefix_rewrites: Vec<(String, String)>,
}

impl MdxImageConfig {
    pub fn new() -> Self {
        Self {
            enabled: true,
            default_alt: "Image".to_string(),
            prefix_rewrites: Vec::new(),
        }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn default_alt(mut self, default_alt: impl Into<String>) -> Self {
        self.default_alt = default_alt.into();
        self
    }

    pub fn rewrite_prefix(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.prefix_rewrites.push((from.into(), to.into()));
        self
    }
}

/// Build-time math rendering for `$...$` and `$$...$$` TeX segments.
///
/// Enabling math changes how dollar signs parse across the whole docs
/// source: `pulldown-cmark` treats paired `$` delimiters as inline math.
/// Sites whose prose contains bare dollar amounts should keep this
/// disabled or escape those amounts as `\$`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MathMode {
    /// Dollar-delimited text passes through as literal markdown text.
    #[default]
    Disabled,
    /// Convert TeX to MathML while compiling the docs. The generated
    /// markup renders natively in every modern browser; no client
    /// JavaScript, stylesheet, or font is required.
    MathMl,
}

impl MathMode {
    pub fn is_enabled(self) -> bool {
        !matches!(self, MathMode::Disabled)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ComponentIndexing {
    pub(crate) preserve: Vec<String>,
    pub(crate) children_only_default: bool,
}

impl ComponentIndexing {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn preserve<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.preserve.extend(names.into_iter().map(Into::into));
        self
    }

    pub fn children_only_default(mut self, value: bool) -> Self {
        self.children_only_default = value;
        self
    }
}

#[derive(Debug, Clone)]
pub struct MermaidConfig {
    pub enabled: bool,
    pub code_fences: bool,
    pub renderer: MermaidRenderer,
    pub fallback: MermaidFallback,
}

impl Default for MermaidConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            code_fences: true,
            renderer: MermaidRenderer::Client,
            fallback: MermaidFallback::CodeBlock,
        }
    }
}

impl MermaidConfig {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    pub fn code_fences(mut self, value: bool) -> Self {
        self.code_fences = value;
        self
    }

    pub fn renderer(mut self, renderer: MermaidRenderer) -> Self {
        self.renderer = renderer;
        self
    }

    pub fn fallback(mut self, fallback: MermaidFallback) -> Self {
        self.fallback = fallback;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidRenderer {
    Client,
    StaticSvg,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidFallback {
    CodeBlock,
    None,
}

#[derive(Debug, Clone)]
pub enum SearchConfig {
    Disabled,
    OramaClient,
    StaticClient,
    Server,
    Custom(String),
}

impl SearchConfig {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn orama_client() -> Self {
        Self::OramaClient
    }

    pub fn static_client() -> Self {
        Self::StaticClient
    }
}

#[derive(Debug, Clone)]
pub struct LinkValidation {
    pub mode: LinkValidationMode,
    pub check_internal: bool,
    pub check_hashes: bool,
    pub check_relative_paths: RelativePathMode,
    pub check_external: ExternalLinkCheck,
    pub(crate) component_attrs: Vec<(String, String)>,
    pub(crate) static_roots: Vec<PathBuf>,
}

impl Default for LinkValidation {
    fn default() -> Self {
        Self {
            mode: LinkValidationMode::Warn,
            check_internal: true,
            check_hashes: true,
            check_relative_paths: RelativePathMode::AsUrl,
            check_external: ExternalLinkCheck::Disabled,
            component_attrs: Vec::new(),
            static_roots: Vec::new(),
        }
    }
}

impl LinkValidation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn strict() -> Self {
        Self {
            mode: LinkValidationMode::Error,
            ..Default::default()
        }
    }

    pub fn mode(mut self, mode: LinkValidationMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn check_internal(mut self, value: bool) -> Self {
        self.check_internal = value;
        self
    }

    pub fn check_hashes(mut self, value: bool) -> Self {
        self.check_hashes = value;
        self
    }

    pub fn check_relative_paths(mut self, mode: RelativePathMode) -> Self {
        self.check_relative_paths = mode;
        self
    }

    pub fn check_external(mut self, check: ExternalLinkCheck) -> Self {
        self.check_external = check;
        self
    }

    pub fn component_attr(mut self, component: impl Into<String>, attr: impl Into<String>) -> Self {
        self.component_attrs.push((component.into(), attr.into()));
        self
    }

    pub fn static_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.static_roots.push(root.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkValidationMode {
    Off,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativePathMode {
    Ignore,
    AsUrl,
    AsFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalLinkCheck {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone)]
pub struct CollectionSource {
    pub(crate) root: PathBuf,
    pub(crate) kind: CollectionKind,
    pub(crate) schema: PageSchema,
}

impl CollectionSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            kind: CollectionKind::Doc,
            schema: PageSchema::default(),
        }
    }

    pub fn kind(mut self, kind: CollectionKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn schema(mut self, schema: PageSchema) -> Self {
        self.schema = schema;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn collection_kind(&self) -> CollectionKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Doc,
    Data,
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub(crate) kind: PluginKind,
}

impl Plugin {
    pub(crate) fn kind(&self) -> &PluginKind {
        &self.kind
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PluginKind {
    JsonSchema,
    LastModified,
}

pub mod plugins {
    use super::{Plugin, PluginKind};

    pub fn json_schema() -> Plugin {
        Plugin {
            kind: PluginKind::JsonSchema,
        }
    }

    pub fn last_modified() -> Plugin {
        Plugin {
            kind: PluginKind::LastModified,
        }
    }
}
