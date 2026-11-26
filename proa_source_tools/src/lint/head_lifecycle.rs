// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Managed-head lifecycle mistakes that dispatch always rejects at runtime:
//! checked head setters in a body, and metadata roles rendered inside
//! `Suspense` or inside the selected Page.
//!
//! Precision comes before recall. A rendered component is matched to a role
//! only when its path resolves, in the module that writes it, to the module
//! that defines the package's only type with that name, and every render impl
//! of that type declares the same literal `PAGE` or `LAYOUT` role. A name
//! bound by a function body, a glob import, the prelude, an external crate,
//! or a re-export is never resolved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
            scopes: vec![HashMap::new()],
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

/// Tracks which locals hold a `HeadWriter`, following Rust's lexical scopes.
/// Every binding form shadows outer names: `let` (bound after its
/// initializer), `if let` / `while let`, match arms, `for` patterns, and
/// closure parameters. Only `let name = <cx>.head_mut()` binds a writer.
/// Blocks, closures, arms, and template slots each close their own scope.
struct HeadMutationVisitor {
    /// Innermost scope last; each maps a bound name to whether it is a writer.
    scopes: Vec<HashMap<String, bool>>,
    found: Vec<(Span, String)>,
}

impl HeadMutationVisitor {
    fn scoped(&mut self, visit: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        visit(self);
        self.scopes.pop();
    }

    fn bind(&mut self, name: String, writer: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, writer);
        }
    }

    /// Bind every name a pattern introduces as a non-writer.
    fn bind_pattern(&mut self, pat: &Pat) {
        for name in pattern_bindings(pat) {
            self.bind(name, false);
        }
    }

    fn is_writer_name(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .copied()
            .unwrap_or(false)
    }

    fn is_head_writer(&self, expr: &Expr) -> bool {
        match expr {
            Expr::MethodCall(_) => is_head_mut_call(expr),
            Expr::Path(path) => path
                .path
                .get_ident()
                .is_some_and(|ident| self.is_writer_name(&ident.to_string())),
            Expr::Paren(inner) => self.is_head_writer(&inner.expr),
            _ => false,
        }
    }

    /// Each slot is evaluated in its own closure, so its bindings end with it.
    fn visit_template(&mut self, mac: &syn::Macro) {
        visit_template_exprs(mac, |expr| self.scoped(|visitor| visitor.visit_expr(expr)));
    }
}

fn is_head_mut_call(expr: &Expr) -> bool {
    matches!(expr, Expr::MethodCall(call) if call.method == "head_mut" && call.args.is_empty())
}

/// Whether a `let` initializer evaluates to `<cx>.head_mut()`, directly or
/// as the tail of a block.
fn yields_head_writer(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(inner) => yields_head_writer(&inner.expr),
        Expr::Block(block) if block.label.is_none() => match block.block.stmts.last() {
            Some(syn::Stmt::Expr(tail, None)) => yields_head_writer(tail),
            _ => false,
        },
        _ => is_head_mut_call(expr),
    }
}

/// The name of a plain `name` / `mut name` / `name: T` binding.
fn simple_binding(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(pat) if pat.by_ref.is_none() && pat.subpat.is_none() => {
            Some(pat.ident.to_string())
        }
        Pat::Type(pat) => simple_binding(&pat.pat),
        _ => None,
    }
}

/// Every name a pattern may bind. Identifiers inside pattern macros count as
/// bindings, so an opaque pattern can only hide a writer.
fn pattern_bindings(pat: &Pat) -> Vec<String> {
    struct Bindings(Vec<String>);
    impl<'ast> Visit<'ast> for Bindings {
        fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
            self.0.push(node.ident.to_string());
            visit::visit_pat_ident(self, node);
        }
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            self.0.extend(token_idents(node.tokens.clone()));
        }
        // Names inside an arm guard or a const pattern (closure parameters,
        // block locals) are scoped to that expression and never bind in the
        // arm.
        fn visit_expr(&mut self, _: &'ast Expr) {}
    }
    let mut bindings = Bindings(Vec::new());
    bindings.visit_pat(pat);
    bindings.0
}

fn token_idents(tokens: proc_macro2::TokenStream) -> Vec<String> {
    let mut idents = Vec::new();
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Ident(ident) => idents.push(ident.to_string()),
            proc_macro2::TokenTree::Group(group) => idents.extend(token_idents(group.stream())),
            _ => {}
        }
    }
    idents
}

impl<'ast> Visit<'ast> for HeadMutationVisitor {
    fn visit_block(&mut self, node: &'ast Block) {
        self.scoped(|visitor| visit::visit_block(visitor, node));
    }

    fn visit_local(&mut self, local: &'ast Local) {
        // The initializer and `else` block run before the new name exists.
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        let writer = local
            .init
            .as_ref()
            .is_some_and(|init| init.diverge.is_none() && yields_head_writer(&init.expr));
        match simple_binding(&local.pat) {
            Some(name) if writer => self.bind(name, true),
            _ => self.bind_pattern(&local.pat),
        }
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        self.scoped(|visitor| {
            for input in &node.inputs {
                visitor.bind_pattern(input);
            }
            visitor.visit_expr(&node.body);
        });
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        self.visit_expr(&node.expr);
        for arm in &node.arms {
            self.scoped(|visitor| {
                visitor.bind_pattern(&arm.pat);
                // syn keeps an arm's guard inside its pattern.
                if let Pat::Guard(guarded) = &arm.pat {
                    visitor.visit_expr(&guarded.guard);
                }
                visitor.visit_expr(&arm.body);
            });
        }
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.visit_expr(&node.expr);
        self.scoped(|visitor| {
            visitor.bind_pattern(&node.pat);
            visitor.visit_block(&node.body);
        });
    }

    // `if let` / `while let` bindings reach the condition's remainder and the
    // body, not an `else` branch.
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        self.scoped(|visitor| {
            visitor.visit_expr(&node.cond);
            visitor.visit_block(&node.then_branch);
        });
        if let Some((_, otherwise)) = &node.else_branch {
            self.visit_expr(otherwise);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.scoped(|visitor| {
            visitor.visit_expr(&node.cond);
            visitor.visit_block(&node.body);
        });
    }

    fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
        self.visit_expr(&node.expr);
        self.bind_pattern(&node.pat);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let setter = node.method.to_string();
        if CHECKED_HEAD_SETTERS.contains(&setter.as_str()) && self.is_head_writer(&node.receiver) {
            self.found.push((node.span(), setter));
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.visit_template(&node.mac);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        if is_html_macro(&node.mac) {
            self.visit_template(&node.mac);
            return;
        }
        // A statement macro can expand to `let $name = ..` for a name passed
        // to it, so any writer it mentions may be shadowed from here on.
        for name in token_idents(node.mac.tokens.clone()) {
            if self.is_writer_name(&name) {
                self.bind(name, false);
            }
        }
    }

    // Nested items have their own bodies and roles.
    fn visit_item(&mut self, _: &'ast Item) {}
}

/// Visit every Rust expression in an `html!`/`html_sync!` template: slots and
/// dynamic attribute values, including nested elements.
fn visit_template_exprs(mac: &syn::Macro, mut visit: impl FnMut(&Expr)) {
    if !is_html_macro(mac) {
        return;
    }
    let Ok(root) = syn::parse2::<HtmlRoot>(mac.tokens.clone()) else {
        return;
    };
    fn walk(nodes: &[Node], visit: &mut impl FnMut(&Expr)) {
        for node in nodes {
            match node {
                Node::Element(el) => {
                    for attr in &el.attrs {
                        if let Attribute::Dynamic { value, .. } = attr {
                            visit(value);
                        }
                    }
                    walk(&el.children, visit);
                }
                Node::Fragment(children) => walk(children, visit),
                Node::Block(expr) => visit(expr),
                Node::Text(_) | Node::Doctype(_) | Node::XmlDeclaration(_) => {}
            }
        }
    }
    walk(&root.nodes, &mut visit);
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

/// A module: the file holding its items, plus the inline `mod` path inside it.
type Location = (usize, Vec<String>);

#[derive(Default)]
struct PackageIndex {
    /// Every type definition by name; `None` locates a block-local item.
    definitions: BTreeMap<String, Vec<Option<Location>>>,
    roles: BTreeMap<String, Vec<Declaration>>,
}

impl PackageIndex {
    /// The metadata role of the package's only type named `name`, and where
    /// that type is defined.
    fn metadata_role(&self, name: &str) -> Option<(&Declaration, &Location)> {
        let [Some(location)] = self.definitions.get(name)?.as_slice() else {
            return None;
        };
        let declarations = self.roles.get(name)?;
        let first = declarations.first()?;
        let agreed = declarations.iter().all(|d| d.role == first.role);
        (agreed && matches!(first.role, Role::Page | Role::Layout)).then_some((first, location))
    }
}

struct IndexVisitor<'a> {
    file: usize,
    module: Vec<String>,
    blocks: usize,
    index: &'a mut PackageIndex,
}

impl IndexVisitor<'_> {
    fn define(&mut self, name: String, attrs: &[SynAttribute]) {
        let location = (self.blocks == 0).then(|| (self.file, self.module.clone()));
        self.index
            .definitions
            .entry(name.clone())
            .or_default()
            .push(location);
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
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if node.content.is_some() {
            self.module.push(node.ident.to_string());
            visit::visit_item_mod(self, node);
            self.module.pop();
        }
    }
    fn visit_block(&mut self, node: &'ast Block) {
        self.blocks += 1;
        visit::visit_block(self, node);
        self.blocks -= 1;
    }
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

/// The names one module binds directly. Rust modules do not inherit their
/// parent's names, so resolution never looks outward.
#[derive(Default)]
struct ModuleScope {
    defined: BTreeSet<String>,
    /// Alias -> (`use` path prefix, original name); `None` when the alias is
    /// bound twice.
    imports: BTreeMap<String, Option<(Vec<String>, String)>>,
    glob: bool,
    children: BTreeSet<String>,
}

impl ModuleScope {
    fn import(&mut self, alias: String, mut prefix: Vec<String>, original: String) {
        // `use a::b::{self}` and `use a::b::{self as c}` import module `b`.
        let (alias, original) = if original == "self" {
            let Some(module) = prefix.pop() else { return };
            let alias = if alias == "self" {
                module.clone()
            } else {
                alias
            };
            (alias, module)
        } else {
            (alias, original)
        };
        if alias == "_" {
            return;
        }
        self.imports
            .entry(alias)
            .and_modify(|target| *target = None)
            .or_insert(Some((prefix, original)));
    }

    fn add_use(&mut self, tree: &syn::UseTree, prefix: Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut prefix = prefix;
                prefix.push(path.ident.to_string());
                self.add_use(&path.tree, prefix);
            }
            syn::UseTree::Name(name) => {
                self.import(name.ident.to_string(), prefix, name.ident.to_string());
            }
            syn::UseTree::Rename(rename) => {
                self.import(rename.rename.to_string(), prefix, rename.ident.to_string());
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use(tree, prefix.clone());
                }
            }
            syn::UseTree::Glob(_) => self.glob = true,
        }
    }
}

fn module_scopes(items: &[Item], path: Vec<String>, out: &mut BTreeMap<Vec<String>, ModuleScope>) {
    let mut scope = ModuleScope::default();
    for item in items {
        if let Some(ident) = type_item_ident(item) {
            scope.defined.insert(ident.to_string());
        }
        match item {
            Item::Use(item) => scope.add_use(&item.tree, Vec::new()),
            Item::Mod(item) => {
                scope.children.insert(item.ident.to_string());
                if let Some((_, items)) = &item.content {
                    let mut child = path.clone();
                    child.push(item.ident.to_string());
                    module_scopes(items, child, out);
                }
            }
            _ => {}
        }
    }
    out.insert(path, scope);
}

fn type_item_ident(item: &Item) -> Option<&syn::Ident> {
    match item {
        Item::Struct(item) => Some(&item.ident),
        Item::Enum(item) => Some(&item.ident),
        Item::Union(item) => Some(&item.ident),
        Item::Type(item) => Some(&item.ident),
        _ => None,
    }
}

/// Names a function body binds anywhere inside it, plus the generic
/// parameters of the impl and function around it. Any of them, or a glob
/// import, may shadow the enclosing module, so resolution gives up on them.
#[derive(Default)]
struct BlockShadow {
    names: BTreeSet<String>,
    glob: bool,
}

impl BlockShadow {
    fn of(generics: &[&syn::Generics], block: &Block) -> Self {
        struct Collector(BlockShadow);
        impl<'ast> Visit<'ast> for Collector {
            // Type and const parameters, on the enclosing items and on any
            // item nested in the body.
            fn visit_generic_param(&mut self, param: &'ast syn::GenericParam) {
                match param {
                    syn::GenericParam::Type(param) => {
                        self.0.names.insert(param.ident.to_string());
                    }
                    syn::GenericParam::Const(param) => {
                        self.0.names.insert(param.ident.to_string());
                    }
                    syn::GenericParam::Lifetime(_) => {}
                }
                visit::visit_generic_param(self, param);
            }
            fn visit_item(&mut self, item: &'ast Item) {
                let ident = match item {
                    Item::Mod(item) => Some(&item.ident),
                    Item::Const(item) => Some(&item.ident),
                    Item::Static(item) => Some(&item.ident),
                    Item::Fn(item) => Some(&item.sig.ident),
                    item => type_item_ident(item),
                };
                if let Some(ident) = ident {
                    self.0.names.insert(ident.to_string());
                }
                if let Item::Use(item) = item {
                    let mut scope = ModuleScope::default();
                    scope.add_use(&item.tree, Vec::new());
                    self.0.glob |= scope.glob;
                    self.0.names.extend(scope.imports.into_keys());
                }
                visit::visit_item(self, item);
            }
        }
        let mut collector = Collector(Self::default());
        for generics in generics {
            collector.visit_generics(generics);
        }
        collector.visit_block(block);
        collector.0
    }
}

/// Which crate a file's `crate::` paths name. Files under `src/` (outside
/// `src/bin/`) share the package's library or main binary module tree; any
/// other file is treated as its own crate root.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CrateRoot {
    Src,
    File(usize),
}

/// Resolves rendered component paths to the module that defines them.
struct Resolver<'a> {
    files: &'a [PreparedFile<'a>],
    by_path: &'a BTreeMap<PathBuf, usize>,
}

impl Resolver<'_> {
    /// The crate-relative module path of an inline module in `file`.
    fn module_path(&self, file: usize, inline: &[String]) -> (CrateRoot, Vec<String>) {
        let prepared = &self.files[file];
        let mut path = prepared.module.clone();
        path.extend(inline.iter().cloned());
        (prepared.root, path)
    }

    /// The absolute module named by `prefix` inside module `inline` of `file`.
    fn resolve_prefix(
        &self,
        file: usize,
        inline: &[String],
        scope: &ModuleScope,
        prefix: &[String],
        depth: usize,
    ) -> Option<(CrateRoot, Vec<String>)> {
        let (first, rest) = prefix.split_first()?;
        let (root, current) = self.module_path(file, inline);
        let (root, mut path) = match first.as_str() {
            "crate" => (root, Vec::new()),
            "self" => (root, current),
            "super" => {
                let mut parent = current;
                parent.pop()?;
                (root, parent)
            }
            name if scope.children.contains(name) => {
                let mut child = current;
                child.push(name.to_owned());
                (root, child)
            }
            name => {
                // A module imported by `use`; bounded against import cycles.
                let Some(Some((target, original))) = scope.imports.get(name) else {
                    return None;
                };
                if depth > 8 {
                    return None;
                }
                let mut full = target.clone();
                full.push(original.clone());
                self.resolve_prefix(file, inline, scope, &full, depth + 1)?
            }
        };
        for segment in rest {
            match segment.as_str() {
                "super" => {
                    path.pop()?;
                }
                "self" | "crate" => return None,
                name => path.push(name.to_owned()),
            }
        }
        Some((root, path))
    }

    /// The file and inline path holding the items of a module.
    fn locate(&self, from: usize, root: CrateRoot, path: Vec<String>) -> Option<Location> {
        let src = match root {
            CrateRoot::File(file) => return Some((file, path)),
            CrateRoot::Src => self.files[from].package.join("src"),
        };
        for split in (1..=path.len()).rev() {
            let relative: PathBuf = path[..split].iter().collect();
            for candidate in [
                src.join(&relative).with_extension("rs"),
                src.join(&relative).join("mod.rs"),
            ] {
                if let Some(&file) = self.by_path.get(&candidate) {
                    return Some((file, path[split..].to_vec()));
                }
            }
        }
        // Both a library and a binary root: `crate` is ambiguous unless the
        // path is written in one of them.
        let roots: Vec<usize> = ["lib.rs", "main.rs"]
            .iter()
            .filter_map(|name| self.by_path.get(&src.join(name)).copied())
            .collect();
        let root = match roots.as_slice() {
            _ if roots.contains(&from) => from,
            [only] => *only,
            _ => return None,
        };
        Some((root, path))
    }

    /// Where the type a rendered path names is defined, and its name there.
    fn resolve(
        &self,
        file: usize,
        scopes: &BTreeMap<Vec<String>, ModuleScope>,
        finding: &Finding,
    ) -> Option<(Location, String)> {
        let path = &finding.path;
        if path.leading_colon.is_some() {
            return None;
        }
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let (last, prefix) = segments.split_last()?;
        let first = segments.first()?;
        if finding.shadow.glob || finding.shadow.names.contains(first) {
            return None;
        }
        let scope = scopes.get(&finding.module)?;
        if prefix.is_empty() {
            if scope.defined.contains(last) {
                if scope.imports.contains_key(last) {
                    return None;
                }
                return Some(((file, finding.module.clone()), last.clone()));
            }
            // A glob import or the prelude may supply anything else.
            let Some(Some((target, original))) = scope.imports.get(last) else {
                return None;
            };
            let (root, module) = self.resolve_prefix(file, &finding.module, scope, target, 0)?;
            return Some((self.locate(file, root, module)?, original.clone()));
        }
        let (root, module) = self.resolve_prefix(file, &finding.module, scope, prefix, 0)?;
        Some((self.locate(file, root, module)?, last.clone()))
    }
}

/// Component expressions a template slot, `render_child`, or `Suspense::new`
/// argument renders, following `proa_macros::render_control::lower`: only a
/// block's tail renders, and control flow directs it into the selected branch
/// or loop body. html templates nested in a slot are visited separately.
fn rendered_slot<'a>(expr: &'a Expr, out: &mut Vec<(&'a SynPath, Span)>) {
    match expr {
        Expr::If(node) => {
            rendered_tail(&node.then_branch, out);
            if let Some((_, otherwise)) = &node.else_branch {
                rendered_slot(otherwise, out);
            }
        }
        Expr::Match(node) => {
            for arm in &node.arms {
                rendered_slot(&arm.body, out);
            }
        }
        Expr::ForLoop(node) => rendered_tail(&node.body, out),
        Expr::While(node) => rendered_tail(&node.body, out),
        // A breaking loop also renders its break value, which stays opaque.
        Expr::Loop(node) => rendered_tail(&node.body, out),
        // A labeled block renders its break value instead of its tail.
        Expr::Block(node) if node.label.is_none() => rendered_tail(&node.block, out),
        Expr::Block(_) | Expr::Break(_) | Expr::Continue(_) | Expr::Return(_) => {}
        _ => rendered_components(expr, out),
    }
}

fn rendered_tail<'a>(block: &'a Block, out: &mut Vec<(&'a SynPath, Span)>) {
    if let Some(syn::Stmt::Expr(tail, None)) = block.stmts.last() {
        rendered_slot(tail, out);
    }
}

/// The component a rendered value constructs. Wrappers that forward their
/// child's role are unwrapped.
fn rendered_components<'a>(expr: &'a Expr, out: &mut Vec<(&'a SynPath, Span)>) {
    match expr {
        Expr::Paren(inner) => rendered_slot(&inner.expr, out),
        Expr::Group(inner) => rendered_slot(&inner.expr, out),
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
                rendered_slot(&call.args[0], out);
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
    /// The inline module holding the usage, within its file.
    module: Vec<String>,
    shadow: Rc<BlockShadow>,
}

/// Collects components rendered inside `Suspense::new(..)` arguments
/// anywhere in a file, and inside the body of a `PAGE` impl.
struct UsageVisitor<'a> {
    page: Option<String>,
    module: &'a [String],
    shadow: Rc<BlockShadow>,
    suspense_depth: usize,
    findings: &'a mut Vec<Finding>,
}

impl UsageVisitor<'_> {
    fn record(&mut self, expr: &Expr) {
        if self.suspense_depth == 0 && self.page.is_none() {
            return;
        }
        let mut found = Vec::new();
        rendered_slot(expr, &mut found);
        for (path, span) in found {
            self.findings.push(Finding {
                page: (self.suspense_depth == 0)
                    .then(|| self.page.clone())
                    .flatten(),
                path: component_path(path),
                span,
                module: self.module.to_vec(),
                shadow: Rc::clone(&self.shadow),
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

impl<'ast> Visit<'ast> for UsageVisitor<'_> {
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
    root: CrateRoot,
    /// The file's module path within its crate.
    module: Vec<String>,
    test_ranges: Vec<RangeInclusive<usize>>,
}

/// A file's crate and module path from its location under `src/`.
fn crate_position(absolute: &Path, package: &Path, index: usize) -> (CrateRoot, Vec<String>) {
    let own = (CrateRoot::File(index), Vec::new());
    let Ok(relative) = absolute.strip_prefix(package.join("src")) else {
        return own;
    };
    let mut module: Vec<String> = relative
        .iter()
        .filter_map(|part| part.to_str().map(str::to_owned))
        .collect();
    if module.first().is_some_and(|first| first == "bin") {
        return own;
    }
    match module
        .pop()
        .as_deref()
        .and_then(|file| file.strip_suffix(".rs"))
    {
        Some("lib" | "main") if module.is_empty() => {}
        Some("mod") => {}
        Some(stem) => module.push(stem.to_owned()),
        None => return own,
    }
    (CrateRoot::Src, module)
}

pub(super) fn lint_parsed(
    sources: &[(PathBuf, String)],
    parsed: &[Option<&File>],
    reporting: &BTreeSet<PathBuf>,
    include_tests: bool,
) -> Vec<Diagnostic> {
    let mut package_by_dir: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    let mut package_of = |absolute: &Path| {
        let dir = absolute.parent().unwrap_or(absolute).to_path_buf();
        package_by_dir
            .entry(dir.clone())
            .or_insert_with(|| {
                super::asset_urls::package_scope(&dir).unwrap_or(absolute.to_path_buf())
            })
            .clone()
    };
    // An unparsed file could define a same-named type, so its package's
    // index would be incomplete.
    let mut incomplete = BTreeSet::new();
    let mut files: Vec<PreparedFile<'_>> = Vec::new();
    let mut by_path = BTreeMap::new();
    for ((path, source), file) in sources.iter().zip(parsed) {
        if !include_tests && is_test_path(path) {
            continue;
        }
        let absolute = content::absolute(path);
        let package = package_of(&absolute);
        let Some(file) = *file else {
            incomplete.insert(package);
            continue;
        };
        let index = files.len();
        let (root, module) = crate_position(&absolute, &package, index);
        by_path.insert(absolute, index);
        files.push(PreparedFile {
            path,
            source,
            file,
            package,
            root,
            module,
            test_ranges: if include_tests {
                Vec::new()
            } else {
                cfg_test_module_line_ranges(source)
            },
        });
    }

    let mut packages: BTreeMap<&Path, PackageIndex> = BTreeMap::new();
    for (index, prepared) in files.iter().enumerate() {
        IndexVisitor {
            file: index,
            module: Vec::new(),
            blocks: 0,
            index: packages.entry(&prepared.package).or_default(),
        }
        .visit_file(prepared.file);
    }

    let resolver = Resolver {
        files: &files,
        by_path: &by_path,
    };
    let mut diagnostics = Vec::new();
    for (index, prepared) in files.iter().enumerate() {
        if !reporting.contains(&content::absolute(prepared.path))
            || incomplete.contains(&prepared.package)
        {
            continue;
        }
        let Some(package) = packages.get(prepared.package.as_path()) else {
            continue;
        };
        let mut scopes = BTreeMap::new();
        module_scopes(&prepared.file.items, Vec::new(), &mut scopes);
        let mut findings = Vec::new();
        collect_usages(&prepared.file.items, &[], &mut findings);
        for finding in findings {
            let start = finding.span.start();
            if line_in_ranges(start.line, &prepared.test_ranges) {
                continue;
            }
            let Some((location, name)) = resolver.resolve(index, &scopes, &finding) else {
                continue;
            };
            let Some((declaration, defined)) = package.metadata_role(&name) else {
                continue;
            };
            if *defined != location {
                continue;
            }
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

/// Visit every function body; a `PAGE` impl's body is a Page site.
fn collect_usages(items: &[Item], module: &[String], findings: &mut Vec<Finding>) {
    let visit = |generics: &[&syn::Generics],
                 block: &Block,
                 page: Option<String>,
                 findings: &mut Vec<Finding>| {
        UsageVisitor {
            page,
            module,
            shadow: Rc::new(BlockShadow::of(generics, block)),
            suspense_depth: 0,
            findings,
        }
        .visit_block(block);
    };
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
                    let site = page
                        .as_ref()
                        .filter(|(_, body)| method.sig.ident == *body)
                        .map(|(name, _)| name.clone());
                    visit(
                        &[&item_impl.generics, &method.sig.generics],
                        &method.block,
                        site,
                        findings,
                    );
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, items)) = &item_mod.content {
                    let mut child = module.to_vec();
                    child.push(item_mod.ident.to_string());
                    collect_usages(items, &child, findings);
                }
            }
            Item::Fn(item_fn) => visit(&[&item_fn.sig.generics], &item_fn.block, None, findings),
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
