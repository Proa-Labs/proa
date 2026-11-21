// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Generalizable asset-URL lints across HTML templates, Markdown templates,
//! render/head/markdown methods, local/const bindings, struct fields, and
//! constructor calls.
//!
//! Instead of relying solely on a fixed list of field names, this pass infers
//! which struct fields and constructor parameters flow into URL sinks without
//! passing through `.url(...)` (and which ones *do* pass through `.url(...)`),
//! then flags raw `public/…`, hardcoded `/_proa/assets/…`, and root-relative
//! paths matching files in a project's configured `public/` catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldRole {
    ResolvedUrl { srcset: bool },
    LogicalPath,
}

impl FieldRole {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::LogicalPath, _) | (_, Self::LogicalPath) => Self::LogicalPath,
            (Self::ResolvedUrl { srcset: a }, Self::ResolvedUrl { srcset: b }) => {
                Self::ResolvedUrl { srcset: a || b }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FieldKey {
    type_name: String,
    field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ParamKey {
    type_name: Option<String>,
    func: String,
    index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ValueSlot {
    Field(FieldKey),
    Param(ParamKey),
}

#[derive(Default)]
struct UrlSchema {
    /// Struct field type names (unwrapping &, Option, Vec, slices, Box, Arc).
    struct_field_types: BTreeMap<FieldKey, String>,
    /// File-level const/static item element type names (e.g. `const CARDS: &[Card]` -> `Card`).
    global_types: BTreeMap<String, String>,
    /// Crate-wide const/static string literal bindings (e.g. `pub const LOGO: &str = "public/..."`).
    global_str_literals: BTreeMap<String, String>,
    /// Inferred URL roles for struct fields and function/constructor parameters.
    roles: BTreeMap<ValueSlot, FieldRole>,
    /// Directed flow edges: initializing `from` flows its value into `to`.
    flow_edges: BTreeSet<(ValueSlot, ValueSlot)>,
}

impl UrlSchema {
    fn with_builtin_seeds() -> Self {
        let mut schema = Self::default();
        for (type_name, func, index) in [
            ("HeadMedia", "new", 0),
            ("HeadMedia", "secure_url", 0),
            ("HeadIcon", "new", 0),
            ("HeadAlternate", "new", 0),
        ] {
            schema.record_role(
                ValueSlot::Param(ParamKey {
                    type_name: Some(type_name.to_string()),
                    func: func.to_string(),
                    index,
                }),
                FieldRole::ResolvedUrl { srcset: false },
            );
        }
        schema
    }

    fn record_role(&mut self, slot: ValueSlot, role: FieldRole) {
        self.roles
            .entry(slot)
            .and_modify(|existing| *existing = existing.merge(role))
            .or_insert(role);
    }

    fn propagate(&mut self) {
        for _ in 0..8 {
            let mut changed = false;
            let edges: Vec<_> = self.flow_edges.iter().cloned().collect();
            for (from, to) in edges {
                let Some(&target_role) = self.roles.get(&to) else {
                    continue;
                };
                let next = match self.roles.get(&from) {
                    Some(&existing) => existing.merge(target_role),
                    None => target_role,
                };
                if self.roles.get(&from).copied() != Some(next) {
                    self.roles.insert(from, next);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn field_role(&self, type_name: &str, field: &str) -> Option<FieldRole> {
        self.roles
            .get(&ValueSlot::Field(FieldKey {
                type_name: type_name.to_string(),
                field: field.to_string(),
            }))
            .copied()
    }

    fn param_role(&self, type_name: Option<&str>, func: &str, index: usize) -> Option<FieldRole> {
        self.roles
            .get(&ValueSlot::Param(ParamKey {
                type_name: type_name.map(ToString::to_string),
                func: func.to_string(),
                index,
            }))
            .copied()
    }
}

#[derive(Debug, Clone)]
struct PublicCatalog {
    prefix: String,
    files: BTreeSet<String>,
}

impl PublicCatalog {
    fn resolve_for_path(
        path: &Path,
        cache: &mut BTreeMap<PathBuf, Option<PublicCatalog>>,
    ) -> Option<PublicCatalog> {
        let project_root = find_proa_config_root(path)?;
        if let Some(cached) = cache.get(&project_root) {
            return cached.clone();
        }
        let loaded = Self::load(&project_root);
        cache.insert(project_root, loaded.clone());
        loaded
    }

    fn load(project_root: &Path) -> Option<Self> {
        let config_path = project_root.join("proa.config.rs");
        let configured = parse_public_dir_from_config(&config_path)?;
        let relative = configured.trim_matches('/');
        if relative.is_empty() || relative.split('/').any(|seg| seg == "..") {
            return None;
        }
        let dir = project_root.join(relative);
        if !dir.is_dir() {
            return None;
        }
        let mut files = BTreeSet::new();
        collect_public_files(&dir, &dir, 0, &mut files);
        (!files.is_empty()).then(|| Self {
            prefix: relative.to_string(),
            files,
        })
    }

    fn logical_path_for_root_relative(&self, url_path: &str) -> Option<String> {
        let stripped = url_path.strip_prefix('/')?;
        if stripped.is_empty() || stripped.starts_with('/') {
            return None;
        }
        self.files
            .contains(stripped)
            .then(|| format!("{}/{stripped}", self.prefix))
    }
}

fn find_proa_config_root(path: &Path) -> Option<PathBuf> {
    let abs = content::absolute(path);
    let mut current = abs.parent()?;
    loop {
        if current.join("proa.config.rs").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Read `paths.public` from `proa.config.rs` statically without executing it.
/// Returns `None` when public assets are explicitly disabled (`public: None`),
/// otherwise `Some("public")` by default or `Some("<dir>")` when configured.
fn parse_public_dir_from_config(config_path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(config_path).ok()?;
    let file = syn::parse_file(&source).ok()?;

    struct ConfigPublicFinder {
        found: Option<Option<String>>,
    }
    impl<'ast> Visit<'ast> for ConfigPublicFinder {
        fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
            if self.found.is_some() {
                return;
            }
            let is_paths_config = expr
                .path
                .segments
                .last()
                .is_some_and(|seg| seg.ident == "PathsConfig");
            if is_paths_config {
                for field in &expr.fields {
                    if let syn::Member::Named(name) = &field.member {
                        if name == "public" {
                            self.found = Some(extract_option_str_literal(&field.expr));
                            return;
                        }
                    }
                }
            }
            visit::visit_expr_struct(self, expr);
        }
    }

    let mut finder = ConfigPublicFinder { found: None };
    finder.visit_file(&file);
    match finder.found {
        Some(None) => None,
        Some(Some(custom)) => Some(custom),
        None => Some("public".to_string()),
    }
}

fn extract_option_str_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|seg| seg.ident == "None") =>
        {
            None
        }
        Expr::Call(call) if call.args.len() == 1 => {
            if let Expr::Path(path) = call.func.as_ref() {
                if path
                    .path
                    .segments
                    .last()
                    .is_some_and(|seg| seg.ident == "Some")
                {
                    return extract_str_literal(&call.args[0]);
                }
            }
            None
        }
        Expr::Paren(e) => extract_option_str_literal(&e.expr),
        Expr::Group(e) => extract_option_str_literal(&e.expr),
        _ => None,
    }
}

fn extract_str_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(lit), ..
        }) => Some(lit.value()),
        Expr::Paren(e) => extract_str_literal(&e.expr),
        Expr::Group(e) => extract_str_literal(&e.expr),
        Expr::Reference(e) => extract_str_literal(&e.expr),
        Expr::MethodCall(call)
            if call.args.is_empty()
                && matches!(
                    call.method.to_string().as_str(),
                    "into" | "to_owned" | "to_string"
                ) =>
        {
            extract_str_literal(&call.receiver)
        }
        Expr::Call(call) if call.args.len() == 1 => {
            if let Expr::Path(path) = call.func.as_ref() {
                let segs = &path.path.segments;
                if segs.len() == 2
                    && matches!(segs[0].ident.to_string().as_str(), "String" | "PathBuf")
                    && segs[1].ident == "from"
                {
                    return extract_str_literal(&call.args[0]);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_public_files(root: &Path, dir: &Path, depth: usize, out: &mut BTreeSet<String>) {
    const MAX_DEPTH: usize = 8;
    const MAX_FILES: usize = 4096;
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_public_files(root, &path, depth + 1, out);
        } else if file_type.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                let normalized = rel.to_string_lossy().replace('\\', "/");
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
        }
    }
}

pub(super) fn lint_sources(
    sources: &[(PathBuf, String)],
    reporting: &BTreeSet<PathBuf>,
    include_tests: bool,
) -> Vec<Diagnostic> {
    let parsed_files: Vec<(&Path, &str, Option<File>)> = sources
        .iter()
        .map(|(path, source)| {
            (
                path.as_path(),
                source.as_str(),
                syn::parse_file(source).ok(),
            )
        })
        .collect();

    let mut schema = UrlSchema::with_builtin_seeds();
    for (path, _, parsed) in &parsed_files {
        if is_config_file(path) || (!include_tests && is_test_path(path)) {
            continue;
        }
        if let Some(file) = parsed {
            collect_type_index(file, &mut schema);
        }
    }
    for (path, source, parsed) in &parsed_files {
        if is_config_file(path) || (!include_tests && is_test_path(path)) {
            continue;
        }
        let test_ranges = if include_tests {
            Vec::new()
        } else {
            cfg_test_module_line_ranges(source)
        };
        if let Some(file) = parsed {
            infer_roles_in_file(source, file, &test_ranges, &mut schema);
        }
    }
    schema.propagate();

    let mut catalog_cache: BTreeMap<PathBuf, Option<PublicCatalog>> = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for (path, source, parsed) in &parsed_files {
        if !reporting.contains(&content::absolute(path)) {
            continue;
        }
        if is_config_file(path) || (!include_tests && is_test_path(path)) {
            continue;
        }
        let catalog = PublicCatalog::resolve_for_path(path, &mut catalog_cache);
        lint_single_file(
            path,
            source,
            parsed.as_ref(),
            include_tests,
            &schema,
            catalog.as_ref(),
            &mut diagnostics,
        );
    }

    dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

fn is_config_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("proa.config.rs")
}

// ---------------------------------------------------------------------------
// Pass 1: Type indexing & sink-driven URL role inference
// ---------------------------------------------------------------------------

fn collect_type_index(file: &File, schema: &mut UrlSchema) {
    for item in &file.items {
        collect_item_types(item, schema);
    }
}

fn collect_item_types(item: &Item, schema: &mut UrlSchema) {
    match item {
        Item::Struct(item_struct) => {
            let type_name = item_struct.ident.to_string();
            for field in &item_struct.fields {
                if let Some(ident) = &field.ident {
                    if let Some(inner) = extract_inner_type_ident(&field.ty) {
                        schema.struct_field_types.insert(
                            FieldKey {
                                type_name: type_name.clone(),
                                field: ident.to_string(),
                            },
                            inner,
                        );
                    }
                }
            }
        }
        Item::Const(item_const) => {
            if let Some(inner) = extract_inner_type_ident(&item_const.ty) {
                schema
                    .global_types
                    .insert(item_const.ident.to_string(), inner);
            }
            if let Some(value) = extract_str_literal(&item_const.expr) {
                schema
                    .global_str_literals
                    .insert(item_const.ident.to_string(), value);
            }
        }
        Item::Static(item_static) => {
            if let Some(inner) = extract_inner_type_ident(&item_static.ty) {
                schema
                    .global_types
                    .insert(item_static.ident.to_string(), inner);
            }
            if let Some(value) = extract_str_literal(&item_static.expr) {
                schema
                    .global_str_literals
                    .insert(item_static.ident.to_string(), value);
            }
        }
        Item::Mod(item_mod) => {
            if let Some((_, items)) = &item_mod.content {
                for child in items {
                    collect_item_types(child, schema);
                }
            }
        }
        _ => {}
    }
}

fn extract_inner_type_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(ty) => extract_inner_type_ident(&ty.elem),
        Type::Slice(ty) => extract_inner_type_ident(&ty.elem),
        Type::Array(ty) => extract_inner_type_ident(&ty.elem),
        Type::Paren(ty) => extract_inner_type_ident(&ty.elem),
        Type::Group(ty) => extract_inner_type_ident(&ty.elem),
        Type::Path(ty) => {
            let last = ty.path.segments.last()?;
            let name = last.ident.to_string();
            if matches!(
                name.as_str(),
                "Option" | "Vec" | "Box" | "Arc" | "Rc" | "Cow" | "Result"
            ) {
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    for arg in &args.args {
                        if let GenericArgument::Type(inner) = arg {
                            if let Some(found) = extract_inner_type_ident(inner) {
                                return Some(found);
                            }
                        }
                    }
                }
                return None;
            }
            if matches!(
                name.as_str(),
                "str"
                    | "String"
                    | "bool"
                    | "char"
                    | "usize"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "f32"
                    | "f64"
            ) {
                return None;
            }
            Some(name)
        }
        _ => None,
    }
}

fn infer_roles_in_file(
    source: &str,
    file: &File,
    ignored_ranges: &[RangeInclusive<usize>],
    schema: &mut UrlSchema,
) {
    let macros = find_macros(source);
    let mut macro_map: BTreeMap<usize, &MacroInvocation> = BTreeMap::new();
    for inv in &macros {
        if !line_in_ranges(inv.name_line, ignored_ranges) {
            macro_map.entry(inv.name_line).or_insert(inv);
        }
    }

    for item in &file.items {
        infer_roles_in_item(source, item, ignored_ranges, &macro_map, schema);
    }
}

fn infer_roles_in_item(
    source: &str,
    item: &Item,
    ignored_ranges: &[RangeInclusive<usize>],
    macro_map: &BTreeMap<usize, &MacroInvocation>,
    schema: &mut UrlSchema,
) {
    match item {
        Item::Impl(item_impl) => {
            if line_in_ranges(item_impl.impl_token.span.start().line, ignored_ranges) {
                return;
            }
            let self_type = extract_inner_type_ident(&item_impl.self_ty);
            let is_render_impl = item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                is_render_trait(path) || is_markdown_render_trait(path)
            });
            for impl_item in &item_impl.items {
                let ImplItem::Fn(method) = impl_item else {
                    continue;
                };
                if line_in_ranges(method.sig.ident.span().start().line, ignored_ranges) {
                    continue;
                }
                let method_name = method.sig.ident.to_string();
                let mut infer = RoleInferer::new(
                    source,
                    self_type.as_deref(),
                    Some(&method_name),
                    is_render_impl,
                    macro_map,
                    schema,
                );
                infer.bind_signature(&method.sig);
                infer.visit_block(&method.block);
            }
        }
        Item::Fn(item_fn) => {
            if line_in_ranges(item_fn.sig.ident.span().start().line, ignored_ranges) {
                return;
            }
            let fn_name = item_fn.sig.ident.to_string();
            let mut infer =
                RoleInferer::new(source, None, Some(&fn_name), false, macro_map, schema);
            infer.bind_signature(&item_fn.sig);
            infer.visit_block(&item_fn.block);
        }
        Item::Mod(item_mod) => {
            if let Some((_, items)) = &item_mod.content {
                for child in items {
                    infer_roles_in_item(source, child, ignored_ranges, macro_map, schema);
                }
            }
        }
        _ => {}
    }
}

struct RoleInferer<'a> {
    source: &'a str,
    self_type: Option<&'a str>,
    func_name: Option<&'a str>,
    in_render_impl: bool,
    macro_map: &'a BTreeMap<usize, &'a MacroInvocation>,
    schema: &'a mut UrlSchema,
    /// Local variable -> known struct type name.
    var_types: BTreeMap<String, String>,
    /// Local variable -> exact upstream ValueSlot (from destructuring, params, or `let x = self.f`).
    var_slots: BTreeMap<String, ValueSlot>,
}

impl<'a> RoleInferer<'a> {
    fn new(
        source: &'a str,
        self_type: Option<&'a str>,
        func_name: Option<&'a str>,
        in_render_impl: bool,
        macro_map: &'a BTreeMap<usize, &'a MacroInvocation>,
        schema: &'a mut UrlSchema,
    ) -> Self {
        let mut var_types = BTreeMap::new();
        if let Some(ty) = self_type {
            var_types.insert("self".to_string(), ty.to_string());
        }
        Self {
            source,
            self_type,
            func_name,
            in_render_impl,
            macro_map,
            schema,
            var_types,
            var_slots: BTreeMap::new(),
        }
    }

    fn bind_signature(&mut self, sig: &syn::Signature) {
        let mut param_index = 0usize;
        for input in &sig.inputs {
            match input {
                syn::FnArg::Receiver(_) => {}
                syn::FnArg::Typed(pat_type) => {
                    if let Some(ident) = simple_pat_ident(&pat_type.pat) {
                        if let Some(ty_name) = extract_inner_type_ident(&pat_type.ty) {
                            self.var_types.insert(ident.clone(), ty_name);
                        }
                        if let Some(func) = self.func_name {
                            self.var_slots.insert(
                                ident,
                                ValueSlot::Param(ParamKey {
                                    type_name: self.self_type.map(ToString::to_string),
                                    func: func.to_string(),
                                    index: param_index,
                                }),
                            );
                        }
                    }
                    param_index += 1;
                }
            }
        }
    }

    fn resolve_expr_slot(&self, expr: &Expr) -> Option<ValueSlot> {
        match expr {
            Expr::Paren(e) => self.resolve_expr_slot(&e.expr),
            Expr::Group(e) => self.resolve_expr_slot(&e.expr),
            Expr::Reference(e) => self.resolve_expr_slot(&e.expr),
            Expr::Block(e) => match e.block.stmts.as_slice() {
                [syn::Stmt::Expr(inner, None)] => self.resolve_expr_slot(inner),
                _ => None,
            },
            Expr::Field(field_expr) => {
                let syn::Member::Named(member) = &field_expr.member else {
                    return None;
                };
                let base_ty = self.resolve_expr_type(&field_expr.base)?;
                Some(ValueSlot::Field(FieldKey {
                    type_name: base_ty,
                    field: member.to_string(),
                }))
            }
            Expr::Path(path_expr) => {
                let ident = path_expr.path.get_ident()?.to_string();
                self.var_slots.get(&ident).cloned()
            }
            Expr::MethodCall(call)
                if matches!(
                    call.method.to_string().as_str(),
                    "as_str"
                        | "as_deref"
                        | "as_ref"
                        | "clone"
                        | "into"
                        | "to_owned"
                        | "to_string"
                        | "unwrap_or"
                        | "unwrap_or_default"
                ) =>
            {
                self.resolve_expr_slot(&call.receiver)
            }
            Expr::Call(call) if call.args.len() == 1 => {
                if let Expr::Path(path) = call.func.as_ref() {
                    if path
                        .path
                        .segments
                        .last()
                        .is_some_and(|seg| seg.ident == "Some" || seg.ident == "from")
                    {
                        return self.resolve_expr_slot(&call.args[0]);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn resolve_expr_type(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Paren(e) => self.resolve_expr_type(&e.expr),
            Expr::Group(e) => self.resolve_expr_type(&e.expr),
            Expr::Reference(e) => self.resolve_expr_type(&e.expr),
            Expr::Path(path_expr) => {
                let ident = path_expr.path.get_ident()?.to_string();
                self.var_types
                    .get(&ident)
                    .or_else(|| self.schema.global_types.get(&ident))
                    .cloned()
            }
            Expr::Field(field_expr) => {
                let syn::Member::Named(member) = &field_expr.member else {
                    return None;
                };
                let base_ty = self.resolve_expr_type(&field_expr.base)?;
                self.schema
                    .struct_field_types
                    .get(&FieldKey {
                        type_name: base_ty,
                        field: member.to_string(),
                    })
                    .cloned()
            }
            Expr::Struct(struct_expr) => struct_expr_type_name(struct_expr, self.self_type),
            Expr::MethodCall(call)
                if matches!(
                    call.method.to_string().as_str(),
                    "iter"
                        | "into_iter"
                        | "as_slice"
                        | "as_ref"
                        | "as_deref"
                        | "clone"
                        | "enumerate"
                ) =>
            {
                self.resolve_expr_type(&call.receiver)
            }
            _ => None,
        }
    }

    fn record_url_sink_expr(&mut self, expr: &Expr, srcset: bool) {
        if let Some(slot) = self.resolve_expr_slot(expr) {
            self.schema
                .record_role(slot, FieldRole::ResolvedUrl { srcset });
            return;
        }
        match expr {
            Expr::If(e) => {
                if let Some(syn::Stmt::Expr(then_val, None)) = e.then_branch.stmts.last() {
                    self.record_url_sink_expr(then_val, srcset);
                }
                if let Some((_, else_val)) = &e.else_branch {
                    self.record_url_sink_expr(else_val, srcset);
                }
            }
            Expr::Match(e) => {
                for arm in &e.arms {
                    self.record_url_sink_expr(&arm.body, srcset);
                }
            }
            _ => {}
        }
    }

    fn visit_html_nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    let meta_url_content = is_meta_url_element(element);
                    for attr in &element.attrs {
                        let name = attr_name(attr).as_markup_str();
                        let srcset = matches!(name, "srcset" | "imagesrcset");
                        let is_url = srcset
                            || proa_dsl::consolidate::is_url_attr_name(name)
                            || (meta_url_content && name == "content");
                        if let Attribute::Dynamic { value, .. } = attr {
                            if is_url {
                                self.record_url_sink_expr(value, srcset);
                            }
                            self.visit_expr(value);
                        }
                    }
                    self.visit_html_nodes(&element.children);
                }
                Node::Fragment(children) => self.visit_html_nodes(children),
                Node::Block(expr) => self.visit_expr(expr),
                _ => {}
            }
        }
    }

    fn visit_md_tokens(&mut self, macro_name: &str, tokens: proc_macro2::TokenStream) {
        let Ok(input) = proa_dsl::md::parse_template_input(macro_name, tokens) else {
            return;
        };
        let Ok(nodes) = input.parse_nodes() else {
            return;
        };
        let contexts = proa_dsl::md::slot_contexts(&nodes);
        let mut slot_index = 0usize;
        for node in &nodes {
            if let proa_dsl::md::TplNode::Expr(expr) = node {
                let is_url = contexts
                    .get(slot_index)
                    .is_some_and(|ctx| ctx.url_destination);
                slot_index += 1;
                if is_url {
                    self.record_url_sink_expr(expr, false);
                }
                self.visit_expr(expr);
            }
        }
    }
}

impl<'ast> Visit<'ast> for RoleInferer<'_> {
    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(init) = &local.init {
            if let Some(ident) = simple_pat_ident(&local.pat) {
                if let Some(slot) = self.resolve_expr_slot(&init.expr) {
                    self.var_slots.insert(ident.clone(), slot);
                }
                if let Some(ty) = pat_explicit_type(&local.pat)
                    .and_then(extract_inner_type_ident)
                    .or_else(|| self.resolve_expr_type(&init.expr))
                {
                    self.var_types.insert(ident, ty);
                }
            } else if let Pat::Struct(pat_struct) = &local.pat {
                let struct_ty = path_last_ident(&pat_struct.path)
                    .map(|name| {
                        if name == "Self" {
                            self.self_type.unwrap_or(&name).to_string()
                        } else {
                            name
                        }
                    })
                    .or_else(|| self.resolve_expr_type(&init.expr));
                if let Some(ty_name) = struct_ty {
                    for field_pat in &pat_struct.fields {
                        if let syn::Member::Named(member) = &field_pat.member {
                            if let Some(binding) = simple_pat_ident(&field_pat.pat) {
                                self.var_slots.insert(
                                    binding.clone(),
                                    ValueSlot::Field(FieldKey {
                                        type_name: ty_name.clone(),
                                        field: member.to_string(),
                                    }),
                                );
                                if let Some(field_ty) = self
                                    .schema
                                    .struct_field_types
                                    .get(&FieldKey {
                                        type_name: ty_name.clone(),
                                        field: member.to_string(),
                                    })
                                    .cloned()
                                {
                                    self.var_types.insert(binding, field_ty);
                                }
                            }
                        }
                    }
                }
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        if let Some(elem_ty) = self.resolve_expr_type(&expr.expr) {
            if let Some(ident) = simple_pat_ident(&expr.pat) {
                self.var_types.insert(ident, elem_ty);
            } else if let Pat::Tuple(tuple) = expr.pat.as_ref() {
                // `for (_, item) in items.iter().enumerate()`
                if let Some(last_pat) = tuple.elems.last() {
                    if let Some(ident) = simple_pat_ident(last_pat) {
                        self.var_types.insert(ident, elem_ty);
                    }
                }
            }
        }
        visit::visit_expr_for_loop(self, expr);
    }

    fn visit_expr_if(&mut self, expr: &'ast syn::ExprIf) {
        if let Expr::Let(expr_let) = expr.cond.as_ref() {
            if let Some(binding) = unwrap_some_pat_ident(&expr_let.pat) {
                if let Some(slot) = self.resolve_expr_slot(&expr_let.expr) {
                    self.var_slots.insert(binding.clone(), slot);
                }
                if let Some(ty) = self.resolve_expr_type(&expr_let.expr) {
                    self.var_types.insert(binding, ty);
                }
            }
        }
        visit::visit_expr_if(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if call.method == "url" && call.args.len() == 1 {
            if let Some(slot) = self.resolve_expr_slot(&call.args[0]) {
                self.schema.record_role(slot, FieldRole::LogicalPath);
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        if let Some(target_type) = struct_expr_type_name(expr, self.self_type) {
            for field in &expr.fields {
                let syn::Member::Named(member) = &field.member else {
                    continue;
                };
                if let Some(source_slot) = self.resolve_expr_slot(&field.expr) {
                    let target_slot = ValueSlot::Field(FieldKey {
                        type_name: target_type.clone(),
                        field: member.to_string(),
                    });
                    if source_slot != target_slot {
                        self.schema.flow_edges.insert((source_slot, target_slot));
                    }
                }
            }
        }
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some((target_type, func_name)) = call_target_signature(call, self.self_type) {
            for (index, arg) in call.args.iter().enumerate() {
                if let Some(source_slot) = self.resolve_expr_slot(arg) {
                    let target_slot = ValueSlot::Param(ParamKey {
                        type_name: target_type.clone(),
                        func: func_name.clone(),
                        index,
                    });
                    if source_slot != target_slot {
                        self.schema.flow_edges.insert((source_slot, target_slot));
                    }
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let Some(name) = mac.path.segments.last().map(|seg| seg.ident.to_string()) else {
            return;
        };
        match name.as_str() {
            "html" | "html_sync" => {
                if let Ok(root) = syn::parse2::<HtmlRoot>(mac.tokens.clone()) {
                    self.visit_html_nodes(&root.nodes);
                } else if let Some(inv) = self.macro_map.get(&mac.path.span().start().line) {
                    if let Ok(root) = parse_invocation(self.source, inv) {
                        self.visit_html_nodes(&root.nodes);
                    }
                }
            }
            "md" | "md_sync" => {
                self.visit_md_tokens(&name, mac.tokens.clone());
            }
            _ => {}
        }
        let _ = self.in_render_impl;
    }
}

// ---------------------------------------------------------------------------
// Pass 2: Reporting diagnostics on each reported file
// ---------------------------------------------------------------------------

fn lint_single_file(
    path: &Path,
    source: &str,
    parsed_file: Option<&File>,
    include_tests: bool,
    schema: &UrlSchema,
    catalog: Option<&PublicCatalog>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let display = display_path(path);
    let test_ranges = if include_tests {
        Vec::new()
    } else {
        cfg_test_module_line_ranges(source)
    };
    let mut suppressions = SuppressionComments::new(source);
    if let Some(file) = parsed_file {
        suppressions.attach_item_scopes(file);
    }

    // Collect file-level const/static string literals (`const LOGO: &str = "public/..."`)
    // so `<img src={LOGO} />` is resolved without false positives on `cx.url(LOGO)`.
    let mut file_str_bindings: BTreeMap<String, String> = BTreeMap::new();
    if let Some(file) = parsed_file {
        collect_str_constants(file, &mut file_str_bindings);
    }

    // 1. Walk parsed AST for all items (handles handlers, consts, statics,
    //    render/render_head/render_md, helper functions, and nested macros).
    if let Some(file) = parsed_file {
        let mut checker = FileAssetChecker {
            path: &display,
            source,
            suppressions: &suppressions,
            ignored_ranges: &test_ranges,
            schema,
            catalog,
            file_str_bindings: &file_str_bindings,
            local_str_bindings: BTreeMap::new(),
            self_type: None,
            in_render_context: false,
            diagnostics,
        };
        checker.visit_file(file);
    } else {
        // Fallback for files with transient Rust syntax errors (e.g., live editor typing):
        // still lint parseable `html!`/`html_sync!`/`md!`/`md_sync!` macro invocations.
        for inv in find_macros(source) {
            if line_in_ranges(inv.name_line, &test_ranges) {
                continue;
            }
            match inv.dialect {
                MacroDialect::Html => {
                    if let Ok(root) = parse_invocation(source, &inv) {
                        let mut checker = MacroAssetChecker {
                            path: &display,
                            origin: SpanOrigin::Macro(&inv),
                            fixed_pos: None,
                            suppressions: &suppressions,
                            schema,
                            catalog,
                            file_str_bindings: &file_str_bindings,
                            local_str_bindings: &BTreeMap::new(),
                            self_type: None,
                            in_render_context: true,
                            diagnostics,
                        };
                        checker.visit_nodes(&root.nodes);
                    }
                }
                MacroDialect::Md => {
                    if let Ok(tokens) = inv.body(source).parse::<proc_macro2::TokenStream>() {
                        let mut checker = MacroAssetChecker {
                            path: &display,
                            origin: SpanOrigin::Macro(&inv),
                            fixed_pos: Some((inv.name_line, inv.name_column + 1)),
                            suppressions: &suppressions,
                            schema,
                            catalog,
                            file_str_bindings: &file_str_bindings,
                            local_str_bindings: &BTreeMap::new(),
                            self_type: None,
                            in_render_context: true,
                            diagnostics,
                        };
                        checker.lint_markdown_macro(
                            &inv.name,
                            tokens,
                            inv.name_line,
                            inv.name_column + 1,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn collect_str_constants(file: &File, out: &mut BTreeMap<String, String>) {
    for item in &file.items {
        match item {
            Item::Const(item_const) => {
                if let Some(value) = extract_str_literal(&item_const.expr) {
                    out.insert(item_const.ident.to_string(), value);
                }
            }
            Item::Static(item_static) => {
                if let Some(value) = extract_str_literal(&item_static.expr) {
                    out.insert(item_static.ident.to_string(), value);
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, items)) = &item_mod.content {
                    for child in items {
                        if let Item::Const(c) = child {
                            if let Some(value) = extract_str_literal(&c.expr) {
                                out.insert(c.ident.to_string(), value);
                            }
                        } else if let Item::Static(s) = child {
                            if let Some(value) = extract_str_literal(&s.expr) {
                                out.insert(s.ident.to_string(), value);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

struct FileAssetChecker<'a> {
    path: &'a Path,
    source: &'a str,
    suppressions: &'a SuppressionComments,
    ignored_ranges: &'a [RangeInclusive<usize>],
    schema: &'a UrlSchema,
    catalog: Option<&'a PublicCatalog>,
    file_str_bindings: &'a BTreeMap<String, String>,
    local_str_bindings: BTreeMap<String, String>,
    self_type: Option<String>,
    in_render_context: bool,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> FileAssetChecker<'a> {
    fn as_macro_checker(&mut self) -> MacroAssetChecker<'_> {
        MacroAssetChecker {
            path: self.path,
            origin: SpanOrigin::File,
            fixed_pos: None,
            suppressions: self.suppressions,
            schema: self.schema,
            catalog: self.catalog,
            file_str_bindings: self.file_str_bindings,
            local_str_bindings: &self.local_str_bindings,
            self_type: self.self_type.as_deref(),
            in_render_context: self.in_render_context,
            diagnostics: self.diagnostics,
        }
    }
}

impl<'ast> Visit<'ast> for FileAssetChecker<'_> {
    fn visit_item_impl(&mut self, item_impl: &'ast ItemImpl) {
        if line_in_ranges(item_impl.impl_token.span.start().line, self.ignored_ranges) {
            return;
        }
        let prev_self = self
            .self_type
            .replace(extract_inner_type_ident(&item_impl.self_ty).unwrap_or_default());
        let is_render_impl = item_impl
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| is_render_trait(path) || is_markdown_render_trait(path));
        for item in &item_impl.items {
            match item {
                ImplItem::Fn(method) => {
                    if line_in_ranges(method.sig.ident.span().start().line, self.ignored_ranges) {
                        continue;
                    }
                    let prev_render = self.in_render_context;
                    let prev_locals = std::mem::take(&mut self.local_str_bindings);
                    let method_name = method.sig.ident.to_string();
                    self.in_render_context = is_render_impl
                        && matches!(
                            method_name.as_str(),
                            "render" | "render_send" | "render_head" | "render_md"
                        );
                    self.visit_block(&method.block);
                    self.in_render_context = prev_render;
                    self.local_str_bindings = prev_locals;
                }
                other => visit::visit_impl_item(self, other),
            }
        }
        self.self_type = prev_self;
    }

    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        if line_in_ranges(item_fn.sig.ident.span().start().line, self.ignored_ranges) {
            return;
        }
        let prev_locals = std::mem::take(&mut self.local_str_bindings);
        let prev_render = self.in_render_context;
        self.in_render_context = false;
        visit::visit_item_fn(self, item_fn);
        self.in_render_context = prev_render;
        self.local_str_bindings = prev_locals;
    }

    fn visit_item_mod(&mut self, item_mod: &'ast syn::ItemMod) {
        if line_in_ranges(item_mod.ident.span().start().line, self.ignored_ranges) {
            return;
        }
        visit::visit_item_mod(self, item_mod);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(init) = &local.init {
            if let Some(ident) = simple_pat_ident(&local.pat) {
                if let Some(value) = extract_str_literal(&init.expr) {
                    self.local_str_bindings.insert(ident, value);
                } else {
                    self.local_str_bindings.remove(&ident);
                }
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        if line_in_ranges(expr.path.span().start().line, self.ignored_ranges) {
            return;
        }
        self.as_macro_checker().check_struct_fields(expr);
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if line_in_ranges(call.span().start().line, self.ignored_ranges) {
            return;
        }
        self.as_macro_checker().check_call_args(call);
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if line_in_ranges(call.method.span().start().line, self.ignored_ranges) {
            return;
        }
        self.as_macro_checker().check_method_call(call);
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let start = mac.path.span().start();
        if line_in_ranges(start.line, self.ignored_ranges) {
            return;
        }
        let Some(name) = mac.path.segments.last().map(|seg| seg.ident.to_string()) else {
            return;
        };
        match name.as_str() {
            "html" | "html_sync" => {
                if let Ok(root) = syn::parse2::<HtmlRoot>(mac.tokens.clone()) {
                    let mut checker = MacroAssetChecker {
                        path: self.path,
                        origin: SpanOrigin::File,
                        fixed_pos: None,
                        suppressions: self.suppressions,
                        schema: self.schema,
                        catalog: self.catalog,
                        file_str_bindings: self.file_str_bindings,
                        local_str_bindings: &self.local_str_bindings,
                        self_type: self.self_type.as_deref(),
                        in_render_context: true,
                        diagnostics: self.diagnostics,
                    };
                    checker.visit_nodes(&root.nodes);
                }
            }
            "md" | "md_sync" => {
                let mut checker = MacroAssetChecker {
                    path: self.path,
                    origin: SpanOrigin::File,
                    fixed_pos: Some((start.line, start.column + 1)),
                    suppressions: self.suppressions,
                    schema: self.schema,
                    catalog: self.catalog,
                    file_str_bindings: self.file_str_bindings,
                    local_str_bindings: &self.local_str_bindings,
                    self_type: self.self_type.as_deref(),
                    in_render_context: true,
                    diagnostics: self.diagnostics,
                };
                checker.lint_markdown_macro(
                    &name,
                    mac.tokens.clone(),
                    start.line,
                    start.column + 1,
                );
            }
            _ => {
                let _ = self.source;
            }
        }
    }
}

struct MacroAssetChecker<'a> {
    path: &'a Path,
    origin: SpanOrigin<'a>,
    fixed_pos: Option<(usize, usize)>,
    suppressions: &'a SuppressionComments,
    schema: &'a UrlSchema,
    catalog: Option<&'a PublicCatalog>,
    file_str_bindings: &'a BTreeMap<String, String>,
    local_str_bindings: &'a BTreeMap<String, String>,
    self_type: Option<&'a str>,
    in_render_context: bool,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl MacroAssetChecker<'_> {
    fn visit_nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    let meta_url_content = is_meta_url_element(element);
                    for attr in &element.attrs {
                        let name = attr_name(attr).as_markup_str();
                        let srcset = matches!(name, "srcset" | "imagesrcset");
                        let is_url = srcset
                            || proa_dsl::consolidate::is_url_attr_name(name)
                            || (meta_url_content && name == "content");
                        match attr {
                            Attribute::Static { value, .. } if is_url => {
                                self.check_str(value.span(), &value.value(), srcset);
                            }
                            Attribute::Dynamic { value, .. } => {
                                if is_url {
                                    self.url_expr(value, srcset);
                                }
                                self.visit_expr(value);
                            }
                            _ => {}
                        }
                    }
                    self.visit_nodes(&element.children);
                }
                Node::Fragment(children) => self.visit_nodes(children),
                Node::Block(expr) => self.visit_expr(expr),
                _ => {}
            }
        }
    }

    fn lint_markdown_macro(
        &mut self,
        macro_name: &str,
        tokens: proc_macro2::TokenStream,
        macro_line: usize,
        macro_column: usize,
    ) {
        let Ok(input) = proa_dsl::md::parse_template_input(macro_name, tokens) else {
            return;
        };
        let Ok(nodes) = input.parse_nodes() else {
            return;
        };

        // Check static Markdown link/image destinations (e.g. `![Brand](public/brand.svg)`).
        for dest in proa_dsl::md::static_link_destinations(&nodes) {
            if let Some(kind) = asset_url(&dest, self.catalog) {
                let (code, message, help) = diagnostic_for_asset_url(&kind, false);
                push_source_diag_with_severity(
                    self.path,
                    self.diagnostics,
                    self.suppressions,
                    macro_line,
                    macro_column,
                    Severity::Warning,
                    code,
                    message,
                    &help,
                );
            }
        }

        // Check dynamic slots inside Markdown templates (including named arguments).
        let mut named_bindings = self.local_str_bindings.clone();
        for arg in &input.args {
            if let Some(value) = extract_str_literal(&arg.value) {
                named_bindings.insert(arg.name.to_string(), value);
            }
        }
        let mut slot_checker = MacroAssetChecker {
            path: self.path,
            origin: self.origin,
            fixed_pos: self.fixed_pos,
            suppressions: self.suppressions,
            schema: self.schema,
            catalog: self.catalog,
            file_str_bindings: self.file_str_bindings,
            local_str_bindings: &named_bindings,
            self_type: self.self_type,
            in_render_context: self.in_render_context,
            diagnostics: self.diagnostics,
        };
        let contexts = proa_dsl::md::slot_contexts(&nodes);
        let mut slot_index = 0usize;
        for node in &nodes {
            if let proa_dsl::md::TplNode::Expr(expr) = node {
                let is_url = contexts
                    .get(slot_index)
                    .is_some_and(|ctx| ctx.url_destination);
                slot_index += 1;
                if is_url {
                    slot_checker.url_expr(expr, false);
                }
                slot_checker.visit_expr(expr);
            }
        }
        for arg in &input.args {
            slot_checker.visit_expr(&arg.value);
        }
    }

    fn check_struct_fields(&mut self, expr: &syn::ExprStruct) {
        let type_name = struct_expr_type_name(expr, self.self_type);
        for field in &expr.fields {
            let syn::Member::Named(name) = &field.member else {
                continue;
            };
            let field_name = name.to_string();
            let inferred = type_name
                .as_deref()
                .and_then(|ty| self.schema.field_role(ty, &field_name));
            match inferred {
                Some(FieldRole::LogicalPath) => {
                    // Component resolves this field via `cx.url(...)`; logical paths are valid.
                }
                Some(FieldRole::ResolvedUrl { srcset }) => {
                    self.url_expr(&field.expr, srcset);
                }
                None if self.in_render_context => {
                    if let Some(srcset) = heuristic_url_field(&field_name) {
                        self.url_expr(&field.expr, srcset);
                    }
                }
                None => {}
            }
        }
    }

    fn check_call_args(&mut self, call: &ExprCall) {
        let Some((type_name, func_name)) = call_target_signature(call, self.self_type) else {
            return;
        };
        for (index, arg) in call.args.iter().enumerate() {
            if let Some(FieldRole::ResolvedUrl { srcset }) =
                self.schema
                    .param_role(type_name.as_deref(), &func_name, index)
            {
                self.url_expr(arg, srcset);
            }
        }
    }

    fn check_method_call(&mut self, call: &ExprMethodCall) {
        if call.method == "url" && call.args.len() == 1 {
            if let Some((span, text)) = self.resolve_string_arg(&call.args[0]) {
                if text.starts_with("/public/") || text.starts_with("./public/") {
                    self.warn(
                        span,
                        "asset/logical-path",
                        "managed asset paths start with `public/`",
                        "remove the leading slash or dot-slash to resolve the path in the asset catalog",
                    );
                } else if matches!(asset_url(&text, None), Some(AssetUrl::Generated)) {
                    self.check_str(span, &text, false);
                }
            }
            return;
        }
        if call.method == "secure_url" && call.args.len() == 1 {
            self.url_expr(&call.args[0], false);
        }
    }

    fn lookup_str_binding(&self, path: &SynPath) -> Option<&str> {
        if let Some(ident) = path.get_ident() {
            let key = ident.to_string();
            return self
                .local_str_bindings
                .get(&key)
                .or_else(|| self.file_str_bindings.get(&key))
                .or_else(|| self.schema.global_str_literals.get(&key))
                .map(String::as_str);
        }
        // Multi-segment constant paths like `crate::assets::LOGO`.
        let last = path.segments.last()?.ident.to_string();
        if last.chars().all(|c| !c.is_ascii_lowercase()) {
            self.schema
                .global_str_literals
                .get(&last)
                .map(String::as_str)
        } else {
            None
        }
    }

    fn resolve_string_arg(&self, expr: &Expr) -> Option<(proc_macro2::Span, String)> {
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: Lit::Str(value),
                ..
            }) => Some((value.span(), value.value())),
            Expr::Path(path_expr) => {
                let value = self.lookup_str_binding(&path_expr.path)?;
                Some((path_expr.span(), value.to_string()))
            }
            Expr::Paren(e) => self.resolve_string_arg(&e.expr),
            Expr::Group(e) => self.resolve_string_arg(&e.expr),
            Expr::Reference(e) => self.resolve_string_arg(&e.expr),
            _ => None,
        }
    }

    fn check_str(&mut self, span: proc_macro2::Span, text: &str, srcset: bool) {
        let kind = if srcset {
            srcset_urls(text).find_map(|candidate| asset_url(candidate, self.catalog))
        } else {
            asset_url(text, self.catalog)
        };
        let Some(kind) = kind else { return };
        let (code, message, help) = diagnostic_for_asset_url(&kind, srcset);
        self.warn(span, code, message, &help);
    }

    fn url_expr(&mut self, expr: &Expr, srcset: bool) {
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: Lit::Str(value),
                ..
            }) => self.check_str(value.span(), &value.value(), srcset),
            Expr::Path(path_expr) => {
                if let Some(value) = self.lookup_str_binding(&path_expr.path) {
                    let owned = value.to_string();
                    self.check_str(path_expr.span(), &owned, srcset);
                }
            }
            Expr::Paren(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Group(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Reference(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Block(expr) => {
                if let Some(syn::Stmt::Expr(value, None)) = expr.block.stmts.last() {
                    self.url_expr(value, srcset);
                }
            }
            Expr::If(expr) => {
                if let Some(syn::Stmt::Expr(value, None)) = expr.then_branch.stmts.last() {
                    self.url_expr(value, srcset);
                }
                if let Some((_, else_branch)) = &expr.else_branch {
                    self.url_expr(else_branch, srcset);
                }
            }
            Expr::Match(expr) => {
                for arm in &expr.arms {
                    self.url_expr(&arm.body, srcset);
                }
            }
            Expr::MethodCall(call)
                if call.args.is_empty()
                    && matches!(
                        call.method.to_string().as_str(),
                        "into" | "to_owned" | "to_string" | "as_str" | "as_deref" | "clone"
                    ) =>
            {
                self.url_expr(&call.receiver, srcset);
            }
            Expr::Call(call) if call.args.len() == 1 => {
                if let Expr::Path(path) = call.func.as_ref() {
                    let segments = &path.path.segments;
                    if segments.last().is_some_and(|last| last.ident == "Some")
                        || (segments.len() == 2
                            && matches!(segments[0].ident.to_string().as_str(), "String" | "Cow")
                            && matches!(
                                segments[1].ident.to_string().as_str(),
                                "from" | "Borrowed"
                            ))
                    {
                        self.url_expr(&call.args[0], srcset);
                    }
                }
            }
            _ => {}
        }
    }

    fn warn(&mut self, span: proc_macro2::Span, code: &'static str, message: &str, help: &str) {
        if let Some((line, column)) = self.fixed_pos {
            push_source_diag_with_severity(
                self.path,
                self.diagnostics,
                self.suppressions,
                line,
                column,
                Severity::Warning,
                code,
                message,
                help,
            );
        } else {
            push_expr_diag(
                self.path,
                self.origin,
                self.diagnostics,
                self.suppressions,
                span,
                code,
                message,
                help,
            );
        }
    }
}

impl<'ast> Visit<'ast> for MacroAssetChecker<'_> {
    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        self.check_method_call(call);
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        self.check_struct_fields(expr);
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        self.check_call_args(call);
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let Some(name) = mac.path.segments.last().map(|seg| seg.ident.to_string()) else {
            return;
        };
        match name.as_str() {
            "html" | "html_sync" => {
                if let Ok(root) = syn::parse2::<HtmlRoot>(mac.tokens.clone()) {
                    self.visit_nodes(&root.nodes);
                }
            }
            "md" | "md_sync" => {
                let prev_fixed = self.fixed_pos;
                if prev_fixed.is_none() {
                    let start = mac.path.span().start();
                    self.fixed_pos = Some((start.line, start.column + 1));
                }
                let (line, col) = self.fixed_pos.unwrap_or((1, 1));
                self.lint_markdown_macro(&name, mac.tokens.clone(), line, col);
                self.fixed_pos = prev_fixed;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_meta_url_element(element: &Element) -> bool {
    if !element.name.as_markup_str().eq_ignore_ascii_case("meta") {
        return false;
    }
    element.attrs.iter().any(|attr| {
        let name = attr_name(attr).as_markup_str();
        if !matches!(name, "property" | "name") {
            return false;
        }
        let Attribute::Static { value, .. } = attr else {
            return false;
        };
        let key = value.value();
        let key = key.trim().to_ascii_lowercase();
        key.ends_with(":image")
            || key.ends_with(":image:url")
            || key.ends_with(":image:secure_url")
            || key.ends_with(":url")
            || key.ends_with(":audio")
            || key.ends_with(":video")
            || key == "msapplication-tileimage"
    })
}

fn heuristic_url_field(name: &str) -> Option<bool> {
    if matches!(name, "srcset" | "src_set")
        || name.ends_with("_srcset")
        || name.ends_with("_src_set")
    {
        Some(true)
    } else if matches!(name, "src" | "href" | "poster" | "url")
        || name.ends_with("_url")
        || name.ends_with("_src")
        || name.ends_with("_href")
        || name.ends_with("_poster")
    {
        Some(false)
    } else {
        None
    }
}

fn struct_expr_type_name(expr: &syn::ExprStruct, self_type: Option<&str>) -> Option<String> {
    let last = path_last_ident(&expr.path)?;
    if last == "Self" {
        self_type.filter(|s| !s.is_empty()).map(ToString::to_string)
    } else {
        Some(last)
    }
}

fn call_target_signature(
    call: &ExprCall,
    self_type: Option<&str>,
) -> Option<(Option<String>, String)> {
    let Expr::Path(path_expr) = call.func.as_ref() else {
        return None;
    };
    let segs: Vec<String> = path_expr
        .path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect();
    match segs.as_slice() {
        [func] => Some((None, func.clone())),
        [.., type_seg, func] => {
            let resolved_type = if type_seg == "Self" {
                self_type.filter(|s| !s.is_empty()).map(ToString::to_string)
            } else if type_seg.chars().next().is_some_and(char::is_uppercase) {
                Some(type_seg.clone())
            } else {
                None
            };
            Some((resolved_type, func.clone()))
        }
        _ => None,
    }
}

fn path_last_ident(path: &SynPath) -> Option<String> {
    path.segments.last().map(|seg| seg.ident.to_string())
}

fn simple_pat_ident(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(ident) => Some(ident.ident.to_string()),
        Pat::Type(pat_type) => simple_pat_ident(&pat_type.pat),
        Pat::Reference(pat_ref) => simple_pat_ident(&pat_ref.pat),
        Pat::Paren(pat_paren) => simple_pat_ident(&pat_paren.pat),
        _ => None,
    }
}

fn pat_explicit_type(pat: &Pat) -> Option<&Type> {
    match pat {
        Pat::Type(pat_type) => Some(pat_type.ty.as_ref()),
        Pat::Paren(pat_paren) => pat_explicit_type(&pat_paren.pat),
        _ => None,
    }
}

fn unwrap_some_pat_ident(pat: &Pat) -> Option<String> {
    match pat {
        Pat::TupleStruct(ts)
            if ts
                .path
                .segments
                .last()
                .is_some_and(|seg| seg.ident == "Some")
                && ts.elems.len() == 1 =>
        {
            simple_pat_ident(&ts.elems[0])
        }
        Pat::Paren(p) => unwrap_some_pat_ident(&p.pat),
        _ => None,
    }
}

#[derive(PartialEq, Eq)]
enum AssetUrl {
    Logical,
    Generated,
    PublicFile { logical_path: String },
}

fn diagnostic_for_asset_url(kind: &AssetUrl, srcset: bool) -> (&'static str, &'static str, String) {
    match kind {
        AssetUrl::Logical => (
            "asset/public-url",
            "public asset path bypasses URL resolution",
            if srcset {
                "resolve each managed candidate with `cx.url(\"public/…\")` before composing the srcset"
                    .to_string()
            } else {
                "pass the logical `public/…` path to `cx.url(...)` so it follows the active asset catalog"
                    .to_string()
            },
        ),
        AssetUrl::Generated => (
            "asset/generated-url",
            "generated asset URL is hard-coded",
            if srcset {
                "resolve each managed candidate with `cx.url(\"public/…\")` before composing the srcset"
                    .to_string()
            } else {
                "pass the logical `public/…` path to `cx.url(...)` so it follows the active asset catalog"
                    .to_string()
            },
        ),
        AssetUrl::PublicFile { logical_path } => (
            "asset/public-url",
            "root-relative path matches a file in `public/` and bypasses URL resolution",
            if srcset {
                format!(
                    "resolve each managed candidate with `cx.url(\"{logical_path}\")` before composing the srcset"
                )
            } else {
                format!(
                    "use `cx.url(\"{logical_path}\")` so it resolves to the fingerprinted asset URL"
                )
            },
        ),
    }
}

fn asset_url(value: &str, catalog: Option<&PublicCatalog>) -> Option<AssetUrl> {
    let value = value.trim();
    let path = value.split(['?', '#']).next()?;
    if ["public/", "./public/", "/public/"].iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty())
    }) {
        Some(AssetUrl::Logical)
    } else if path.starts_with('/') && !path.starts_with("//") && path.contains("/_proa/assets/") {
        Some(AssetUrl::Generated)
    } else if let Some(logical_path) =
        catalog.and_then(|cat| cat.logical_path_for_root_relative(path))
    {
        Some(AssetUrl::PublicFile { logical_path })
    } else {
        None
    }
}

fn srcset_urls(mut input: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        input = input.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
        let end = input
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(input.len());
        let (url, rest) = input.split_at(end);
        input = if url.ends_with(',') {
            rest
        } else {
            rest.split_once(',').map_or("", |(_, rest)| rest)
        };
        (!url.is_empty()).then(|| url.trim_end_matches(','))
    })
}
