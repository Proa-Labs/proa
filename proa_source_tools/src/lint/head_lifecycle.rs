// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Managed-head lifecycle mistakes that dispatch always rejects at runtime:
//! checked head setters in a body, and metadata roles rendered inside
//! `Suspense` or inside the selected Page.
//!
//! Precision comes before recall. A rendered component is matched to a role
//! only when its name resolves inside the package (defined in the same file,
//! imported through `crate`/`self`/`super`, or spelled with such a path) and
//! the package defines exactly one type with that name, whose render impls
//! all declare the same literal `PAGE` or `LAYOUT` role.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::Span;

use super::*;
use crate::diagnostic::{SourceLabel, SourceRange};

/// The render traits with a head hook: `(trait, hook, body)`.
const RENDER_TRAITS: [(&str, &str, &str); 4] = [
    ("WebRender", "render_head", "render"),
    ("WebRenderSync", "render_head", "render"),
    ("WebRenderSend", "render_head_send", "render_send"),
    (
        "WebRenderSendSync",
        "render_head_send_sync",
        "render_send_sync",
    ),
];

/// Checked `HeadWriter` setters. They require an open head hook in a managed
/// document, so they fail in every body. The legacy setters (`set_title`,
/// `push_meta`, `push_link`, `push_raw_html`) still serve manual documents
/// and are deliberately absent.
const CHECKED_HEAD_SETTERS: &[&str] = &[
    "absolute_title",
    "alternates",
    "application_name",
    "authors",
    "canonical",
    "clear_alternates",
    "clear_authors",
    "clear_canonical",
    "clear_description",
    "clear_googlebot",
    "clear_icons",
    "clear_manifest",
    "clear_referrer",
    "clear_robots",
    "clear_title",
    "clear_title_template",
    "clear_twitter",
    "clear_verification",
    "creator",
    "description",
    "googlebot",
    "icon",
    "icons",
    "keywords",
    "manifest",
    "meta",
    "metadata",
    "module",
    "og_image",
    "open_graph",
    "publisher",
    "referrer",
    "remove_meta",
    "remove_module",
    "remove_stylesheet",
    "robots",
    "stylesheet",
    "title",
    "title_template",
    "twitter",
    "verification",
];

fn render_trait(item_impl: &ItemImpl) -> Option<(&'static str, &'static str)> {
    let (trait_path, _) = item_impl.trait_.as_ref()?;
    let name = trait_path.segments.last()?.ident.to_string();
    RENDER_TRAITS
        .iter()
        .find(|(trait_name, _, _)| *trait_name == name)
        .map(|(_, hook, body)| (*hook, *body))
}

/// `render/head-mutation-in-body`: a checked setter reached through
/// `head_mut()` inside a render body.
pub(super) fn lint_head_mutation_in_body(
    path: &Path,
    item_impl: &ItemImpl,
    ignored_ranges: &[RangeInclusive<usize>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some((hook, body)) = render_trait(item_impl) else {
        return;
    };
    for item in &item_impl.items {
        let ImplItem::Fn(method) = item else { continue };
        if method.sig.ident != body {
            continue;
        }
        let mut visitor = HeadMutationVisitor {
            writers: HashSet::new(),
            found: Vec::new(),
        };
        visitor.visit_block(&method.block);
        for (span, setter) in visitor.found {
            let start = span.start();
            if line_in_ranges(start.line, ignored_ranges) {
                continue;
            }
            diagnostics.push(
                Diagnostic::new(
                    path,
                    start.line,
                    start.column + 1,
                    Severity::Error,
                    "render/head-mutation-in-body",
                    format!("`head_mut().{setter}(..)` in `{body}` always fails: head metadata is written only in `{hook}`"),
                )
                .with_note(format!("`{body}` runs after the head hook has closed, so checked setters return `HeadError::OutsideHook`, or `HeadError::Closed` once the Page has sealed the head"))
                .with_help(format!("move this call into `{hook}` on the layout or Page that owns the metadata, and declare `const ROLE: proa::ComponentRole = proa::ComponentRole::PAGE;` (or `LAYOUT` for an ancestor layout)")),
            );
        }
    }
}

struct HeadMutationVisitor {
    /// Locals bound to `<cx>.head_mut()`.
    writers: HashSet<String>,
    found: Vec<(Span, String)>,
}

impl HeadMutationVisitor {
    fn is_head_writer(&self, expr: &Expr) -> bool {
        match expr {
            Expr::MethodCall(call) => call.method == "head_mut" && call.args.is_empty(),
            Expr::Path(path) => path
                .path
                .get_ident()
                .is_some_and(|ident| self.writers.contains(&ident.to_string())),
            Expr::Paren(inner) => self.is_head_writer(&inner.expr),
            _ => false,
        }
    }
}

impl<'ast> Visit<'ast> for HeadMutationVisitor {
    fn visit_local(&mut self, local: &'ast Local) {
        if let (Pat::Ident(binding), Some(init)) = (&local.pat, &local.init) {
            let name = binding.ident.to_string();
            if matches!(init.expr.as_ref(), Expr::MethodCall(call) if call.method == "head_mut" && call.args.is_empty())
            {
                self.writers.insert(name);
            } else {
                self.writers.remove(&name);
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let setter = node.method.to_string();
        if CHECKED_HEAD_SETTERS.contains(&setter.as_str()) && self.is_head_writer(&node.receiver) {
            self.found.push((node.span(), setter));
        }
        visit::visit_expr_method_call(self, node);
    }

    // Nested items have their own bodies and roles.
    fn visit_item(&mut self, _: &'ast Item) {}
}

// ---------------------------------------------------------------------------
// Cross-file: metadata roles rendered where dispatch rejects them
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Component,
    Layout,
    Page,
    /// Forwarded (`T::ROLE`) or application constant; not evaluated.
    Unknown,
}

impl Role {
    fn of_impl(item_impl: &ItemImpl) -> Self {
        let expr = item_impl.items.iter().find_map(|item| match item {
            ImplItem::Const(item) if item.ident == "ROLE" => Some(&item.expr),
            _ => None,
        });
        expr.map_or(Self::Component, Self::of_expr)
    }

    fn of_expr(expr: &Expr) -> Self {
        match expr {
            Expr::Path(path) => match path.path.segments.last() {
                Some(seg) if seg.ident == "PAGE" => Self::Page,
                Some(seg) if seg.ident == "LAYOUT" => Self::Layout,
                Some(seg) if seg.ident == "COMPONENT" => Self::Component,
                _ => Self::Unknown,
            },
            Expr::Paren(expr) => Self::of_expr(&expr.expr),
            Expr::Group(expr) => Self::of_expr(&expr.expr),
            Expr::Block(expr) => match expr.block.stmts.as_slice() {
                [syn::Stmt::Expr(expr, None)] => Self::of_expr(expr),
                _ => Self::Unknown,
            },
            _ => Self::Unknown,
        }
    }

    fn constant(self) -> &'static str {
        match self {
            Self::Page => "PAGE",
            Self::Layout => "LAYOUT",
            Self::Component | Self::Unknown => "COMPONENT",
        }
    }
}

/// Where a metadata role was declared, for the secondary label.
#[derive(Clone)]
struct Declaration {
    role: Role,
    file: usize,
    span: Span,
}

#[derive(Default)]
struct PackageIndex {
    definitions: BTreeMap<String, usize>,
    roles: BTreeMap<String, Vec<Declaration>>,
}

impl PackageIndex {
    /// A metadata role, only when the name is unambiguous in the package.
    fn metadata_role(&self, name: &str) -> Option<&Declaration> {
        if self.definitions.get(name) != Some(&1) {
            return None;
        }
        let declarations = self.roles.get(name)?;
        let first = declarations.first()?;
        let agreed = declarations.iter().all(|d| d.role == first.role);
        (agreed && matches!(first.role, Role::Page | Role::Layout)).then_some(first)
    }
}

struct IndexVisitor<'a> {
    file: usize,
    index: &'a mut PackageIndex,
}

impl IndexVisitor<'_> {
    fn define(&mut self, name: String, attrs: &[SynAttribute]) {
        *self.index.definitions.entry(name.clone()).or_default() += 1;
        // A derived component impl is invisible here; record its role so a
        // same-named metadata type elsewhere in the package stays ambiguous.
        if attrs.iter().any(derives_component) {
            self.index.roles.entry(name).or_default().push(Declaration {
                role: Role::Component,
                file: self.file,
                span: Span::call_site(),
            });
        }
    }
}

fn derives_component(attr: &SynAttribute) -> bool {
    if !attr.path().is_ident("derive") {
        return false;
    }
    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        found |= meta
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "ProaComponent");
        Ok(())
    });
    found
}

impl<'ast> Visit<'ast> for IndexVisitor<'_> {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.define(node.ident.to_string(), &node.attrs);
        visit::visit_item_struct(self, node);
    }
    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.define(node.ident.to_string(), &node.attrs);
        visit::visit_item_enum(self, node);
    }
    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.define(node.ident.to_string(), &node.attrs);
        visit::visit_item_union(self, node);
    }
    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        self.define(node.ident.to_string(), &node.attrs);
        visit::visit_item_type(self, node);
    }
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        if render_trait(node).is_some() {
            if let Some(name) = type_path_last_ident(&node.self_ty) {
                let role = Role::of_impl(node);
                let span = node
                    .items
                    .iter()
                    .find_map(|item| match item {
                        ImplItem::Const(item) if item.ident == "ROLE" => Some(item.span()),
                        _ => None,
                    })
                    .unwrap_or_else(|| node.self_ty.span());
                self.index.roles.entry(name).or_default().push(Declaration {
                    role,
                    file: self.file,
                    span,
                });
            }
        }
        visit::visit_item_impl(self, node);
    }
}

/// Names a file can attribute to its own package.
#[derive(Default)]
struct FileScope {
    defined: BTreeSet<String>,
    /// Module names declared anywhere in the file.
    modules: BTreeSet<String>,
    /// Imported alias -> original name, through a package-local root.
    local_imports: BTreeMap<String, String>,
    /// Aliases imported through any other root, including external crates.
    foreign_imports: BTreeSet<String>,
}

struct Import {
    alias: String,
    original: String,
    root: String,
}

impl FileScope {
    fn new(file: &File) -> Self {
        struct Collector<'a> {
            scope: &'a mut FileScope,
            imports: Vec<Import>,
        }
        impl<'ast> Visit<'ast> for Collector<'_> {
            fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
                self.scope.defined.insert(node.ident.to_string());
            }
            fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
                self.scope.defined.insert(node.ident.to_string());
            }
            fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
                self.scope.modules.insert(node.ident.to_string());
                visit::visit_item_mod(self, node);
            }
            fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
                collect_use(&node.tree, None, &mut self.imports);
            }
        }
        let mut scope = Self::default();
        let mut collector = Collector {
            scope: &mut scope,
            imports: Vec::new(),
        };
        collector.visit_file(file);
        let imports = collector.imports;
        for import in imports {
            if is_local_root(&import.root, &scope.modules) {
                scope.local_imports.insert(import.alias, import.original);
            } else {
                scope.foreign_imports.insert(import.alias);
            }
        }
        scope
    }

    /// The package-local original name of a rendered component path.
    fn resolve(&self, path: &SynPath) -> Option<String> {
        if path.leading_colon.is_some() {
            return None;
        }
        let last = path.segments.last()?.ident.to_string();
        if path.segments.len() > 1 {
            let root = path.segments.first()?.ident.to_string();
            return is_local_root(&root, &self.modules).then_some(last);
        }
        if self.foreign_imports.contains(&last) {
            return None;
        }
        if self.defined.contains(&last) {
            return Some(last);
        }
        self.local_imports.get(&last).cloned()
    }
}

fn is_local_root(root: &str, modules: &BTreeSet<String>) -> bool {
    matches!(root, "crate" | "self" | "super") || modules.contains(root)
}

/// Flatten a `use` tree. A bare `use Name;` has no root and stays unresolved.
fn collect_use(tree: &syn::UseTree, root: Option<&str>, out: &mut Vec<Import>) {
    let mut push = |alias: &syn::Ident, original: &syn::Ident| {
        out.push(Import {
            alias: alias.to_string(),
            original: original.to_string(),
            root: root.unwrap_or_default().to_owned(),
        });
    };
    match tree {
        syn::UseTree::Path(path) => {
            let ident = path.ident.to_string();
            collect_use(&path.tree, Some(root.unwrap_or(&ident)), out);
        }
        syn::UseTree::Name(name) => push(&name.ident, &name.ident),
        syn::UseTree::Rename(rename) => push(&rename.rename, &rename.ident),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use(tree, root, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

/// Component expressions a template slot, `render_child`, or `Suspense::new`
/// argument renders. Wrappers that forward their child's role are unwrapped;
/// html templates are searched slot by slot.
fn rendered_components<'a>(expr: &'a Expr, out: &mut Vec<(&'a SynPath, Span)>) {
    match expr {
        Expr::Paren(inner) => rendered_components(&inner.expr, out),
        Expr::Group(inner) => rendered_components(&inner.expr, out),
        Expr::Struct(node) => out.push((&node.path, expr.span())),
        Expr::Path(node) if node.qself.is_none() && is_type_like(&node.path) => {
            out.push((&node.path, expr.span()));
        }
        Expr::Call(call) => {
            let Expr::Path(func) = call.func.as_ref() else {
                return;
            };
            let Some(last) = func.path.segments.last() else {
                return;
            };
            let name = last.ident.to_string();
            if matches!(
                name.as_str(),
                "to_async" | "to_send" | "SyncToAsync" | "SendSyncToAsync" | "Some"
            ) && call.args.len() == 1
            {
                rendered_components(&call.args[0], out);
            } else if matches!(name.as_str(), "new" | "default") && func.path.segments.len() >= 2 {
                // `Type::new(..)` names the type it constructs. `Suspense::new`
                // is a boundary rather than a metadata provider and has no
                // metadata role in the package index.
                out.push((&func.path, expr.span()));
            } else if func.path.segments.len() == 1 && is_type_like(&func.path) {
                out.push((&func.path, expr.span()));
            }
        }
        _ => {}
    }
}

fn is_type_like(path: &SynPath) -> bool {
    path.segments.last().is_some_and(|seg| {
        let name = seg.ident.to_string();
        let mut chars = name.chars();
        chars.next().is_some_and(char::is_uppercase) && chars.any(char::is_lowercase)
    })
}

/// The component name of a path recorded by [`rendered_components`]:
/// `Type::new` and `Type::default` name `Type`.
fn component_path(path: &SynPath) -> SynPath {
    let mut path = path.clone();
    let constructor = path.segments.len() >= 2
        && path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "new" || seg.ident == "default");
    if constructor {
        let keep = path.segments.len() - 1;
        path.segments = path.segments.iter().take(keep).cloned().collect();
    }
    path
}

fn is_html_macro(mac: &syn::Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "html" || seg.ident == "html_sync")
}

fn html_slot_exprs<'a>(nodes: &'a [Node], out: &mut Vec<&'a Expr>) {
    for node in nodes {
        match node {
            Node::Element(el) => html_slot_exprs(&el.children, out),
            Node::Fragment(children) => html_slot_exprs(children, out),
            Node::Block(expr) => out.push(expr),
            Node::Text(_) | Node::Doctype(_) | Node::XmlDeclaration(_) => {}
        }
    }
}

struct Finding {
    /// The enclosing Page, or `None` for a `Suspense` argument.
    page: Option<String>,
    path: SynPath,
    span: Span,
}

/// Collects components rendered inside `Suspense::new(..)` arguments
/// anywhere in a file, and inside the body of a `PAGE` impl.
struct UsageVisitor {
    page: Option<String>,
    suspense_depth: usize,
    findings: Vec<Finding>,
}

impl UsageVisitor {
    fn new(page: Option<String>) -> Self {
        Self {
            page,
            suspense_depth: 0,
            findings: Vec::new(),
        }
    }

    fn record(&mut self, expr: &Expr) {
        if self.suspense_depth == 0 && self.page.is_none() {
            return;
        }
        let mut found = Vec::new();
        rendered_components(expr, &mut found);
        for (path, span) in found {
            self.findings.push(Finding {
                page: (self.suspense_depth == 0)
                    .then(|| self.page.clone())
                    .flatten(),
                path: component_path(path),
                span,
            });
        }
    }

    fn visit_template(&mut self, mac: &syn::Macro) {
        let Ok(root) = syn::parse2::<HtmlRoot>(mac.tokens.clone()) else {
            return;
        };
        let mut slots = Vec::new();
        html_slot_exprs(&root.nodes, &mut slots);
        for expr in slots {
            self.record(expr);
            self.visit_expr(expr);
        }
    }
}

fn is_suspense_new(call: &ExprCall) -> bool {
    let Expr::Path(func) = call.func.as_ref() else {
        return false;
    };
    let segments: Vec<_> = func.path.segments.iter().collect();
    matches!(segments.as_slice(), [.., ty, method] if ty.ident == "Suspense" && method.ident == "new")
}

fn is_render_child(call: &ExprCall) -> bool {
    let Expr::Path(func) = call.func.as_ref() else {
        return false;
    };
    func.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "render_child" || seg.ident == "render_send_child")
}

impl<'ast> Visit<'ast> for UsageVisitor {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if is_suspense_new(node) {
            self.suspense_depth += 1;
            for arg in &node.args {
                self.record(arg);
                self.visit_expr(arg);
            }
            self.suspense_depth -= 1;
            return;
        }
        if is_render_child(node) && node.args.len() == 2 {
            self.record(&node.args[1]);
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if is_html_macro(&node.mac) {
            self.visit_template(&node.mac);
        }
        visit::visit_expr_macro(self, node);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        if is_html_macro(&node.mac) {
            self.visit_template(&node.mac);
        }
        visit::visit_stmt_macro(self, node);
    }
}

struct PreparedFile<'a> {
    path: &'a Path,
    source: &'a str,
    file: &'a File,
    package: PathBuf,
    test_ranges: Vec<RangeInclusive<usize>>,
}

pub(super) fn lint_parsed(
    sources: &[(PathBuf, String)],
    parsed: &[Option<&File>],
    reporting: &BTreeSet<PathBuf>,
    include_tests: bool,
) -> Vec<Diagnostic> {
    let mut package_by_dir: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    let files: Vec<PreparedFile<'_>> = sources
        .iter()
        .zip(parsed)
        .filter(|((path, _), _)| include_tests || !is_test_path(path))
        .filter_map(|((path, source), file)| {
            let file = (*file)?;
            let absolute = content::absolute(path);
            let dir = absolute.parent().unwrap_or(&absolute).to_path_buf();
            let package = package_by_dir
                .entry(dir.clone())
                .or_insert_with(|| {
                    super::asset_urls::package_scope(&dir).unwrap_or(absolute.clone())
                })
                .clone();
            Some(PreparedFile {
                path,
                source,
                file,
                package,
                test_ranges: if include_tests {
                    Vec::new()
                } else {
                    cfg_test_module_line_ranges(source)
                },
            })
        })
        .collect();

    let mut packages: BTreeMap<&Path, PackageIndex> = BTreeMap::new();
    for (index, prepared) in files.iter().enumerate() {
        let package = packages.entry(&prepared.package).or_default();
        IndexVisitor {
            file: index,
            index: package,
        }
        .visit_file(prepared.file);
    }

    let mut diagnostics = Vec::new();
    for prepared in &files {
        if !reporting.contains(&content::absolute(prepared.path)) {
            continue;
        }
        let Some(index) = packages.get(prepared.package.as_path()) else {
            continue;
        };
        let scope = FileScope::new(prepared.file);
        let mut visitor = UsageVisitor::new(None);
        visit_usage_items(&prepared.file.items, &mut visitor);
        for finding in visitor.findings {
            let start = finding.span.start();
            if line_in_ranges(start.line, &prepared.test_ranges) {
                continue;
            }
            let Some(name) = scope.resolve(&finding.path) else {
                continue;
            };
            let Some(declaration) = index.metadata_role(&name) else {
                continue;
            };
            diagnostics.push(role_diagnostic(
                prepared,
                &files[declaration.file],
                &finding,
                &name,
                declaration,
            ));
        }
    }
    dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

/// Visit every item; a `PAGE` impl's body is a Page site.
fn visit_usage_items(items: &[Item], visitor: &mut UsageVisitor) {
    for item in items {
        match item {
            Item::Impl(item_impl) => {
                let page = match (render_trait(item_impl), Role::of_impl(item_impl)) {
                    (Some((_, body)), Role::Page) => {
                        type_path_last_ident(&item_impl.self_ty).map(|name| (name, body))
                    }
                    _ => None,
                };
                for impl_item in &item_impl.items {
                    let ImplItem::Fn(method) = impl_item else {
                        continue;
                    };
                    match &page {
                        Some((name, body)) if method.sig.ident == *body => {
                            let mut page_visitor = UsageVisitor::new(Some(name.clone()));
                            page_visitor.visit_block(&method.block);
                            visitor.findings.extend(page_visitor.findings);
                        }
                        _ => visitor.visit_block(&method.block),
                    }
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, items)) = &item_mod.content {
                    visit_usage_items(items, visitor);
                }
            }
            Item::Fn(item_fn) => visitor.visit_block(&item_fn.block),
            _ => {}
        }
    }
}

fn role_diagnostic(
    prepared: &PreparedFile<'_>,
    declared_in: &PreparedFile<'_>,
    finding: &Finding,
    name: &str,
    declaration: &Declaration,
) -> Diagnostic {
    let start = finding.span.start();
    let role = declaration.role.constant();
    let display = display_path(prepared.path);
    let (code, message, note, help) = if let Some(page) = &finding.page {
        let help = match declaration.role {
            Role::Layout => format!("layouts belong above the selected Page: render `{page}` inside `{name}`, or give `{name}` the default `COMPONENT` role if it only renders markup"),
            _ => format!("a document selects exactly one Page: render `{name}` from its own route, or extract the shared body into a `COMPONENT`"),
        };
        (
            "render/head-role-in-page",
            format!("`{name}` declares `ComponentRole::{role}` but is rendered inside the Page `{page}`"),
            "the Page seals the document head before its body, so dispatch rejects any later layout or Page with `HeadError::Closed`".to_owned(),
            help,
        )
    } else {
        (
            "render/head-role-in-suspense",
            format!("`{name}` declares `ComponentRole::{role}` but is rendered inside `Suspense`"),
            "Suspense children and fallbacks are body-only, so dispatch rejects a layout or Page there with `HeadError::Excluded`".to_owned(),
            "render the Page and its layouts outside the boundary, and place `Suspense` inside the Page body around the slow content".to_owned(),
        )
    };
    let usage = SourceRange::from_span(&display, finding.span);
    let declared = SourceRange::from_span(&display_path(declared_in.path), declaration.span);
    Diagnostic::new(
        &display,
        start.line,
        start.column + 1,
        Severity::Error,
        code,
        message,
    )
    .with_label(SourceLabel::new(
        usage,
        prepared.source,
        "rendered here",
        true,
    ))
    .with_label(SourceLabel::new(
        declared,
        declared_in.source,
        format!("`{name}` declares `ComponentRole::{role}` here"),
        false,
    ))
    .with_note(note)
    .with_help(help)
}
