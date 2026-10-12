// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use quote::ToTokens;
use std::collections::HashMap;
use syn::{spanned::Spanned, ImplItem, Item, ItemImpl, Path as SynPath, Type, UseTree};

pub(in crate::lint) fn absolute(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    })
}
fn package(path: &Path) -> Option<PathBuf> {
    path.parent()?
        .ancestors()
        .find(|p| p.join("Cargo.toml").is_file())
        .map(Path::to_owned)
}
pub(super) fn load_sources(
    files: &[PathBuf],
    options: &ContentOptions,
) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let mut all: BTreeSet<_> = files.iter().map(|p| absolute(p)).collect();
    let roots: BTreeSet<_> = if let Some(root) = &options.root {
        BTreeSet::from([absolute(root)])
    } else {
        files.iter().filter_map(|p| package(&absolute(p))).collect()
    };
    for root in roots {
        all.extend(
            crate::source::find_rust_files(&[root])?
                .into_iter()
                .map(|p| absolute(&p)),
        );
    }
    all.into_iter()
        .map(|path| {
            std::fs::read_to_string(&path)
                .map(|source| (path.clone(), source))
                .map_err(|err| anyhow::anyhow!("failed to index {}: {err}", path.display()))
        })
        .collect()
}

#[derive(Clone)]
pub(super) struct Resolver {
    pub module: String,
    pub local_modules: BTreeSet<String>,
    pub imports: BTreeMap<String, String>,
    pub symbols: BTreeSet<String>,
    pub fields: BTreeMap<String, Vec<String>>,
    pub constants: BTreeMap<String, Option<String>>,
    pub functions: BTreeSet<String>,
    pub optional_fields: BTreeMap<String, BTreeSet<String>>,
    pub unit_variants: BTreeSet<String>,
    pub child_selectors: BTreeMap<String, String>,
    pub generic_children: BTreeSet<String>,
    pub glob: bool,
    pub extern_roots: BTreeSet<String>,
}
impl Resolver {
    pub fn path(&self, path: &SynPath) -> String {
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let Some(first) = segments.first() else {
            return String::new();
        };
        let rest = segments
            .iter()
            .skip(1)
            .cloned()
            .collect::<Vec<_>>()
            .join("::");
        let base = if path.leading_colon.is_some() {
            first.clone()
        } else if first == "crate" {
            "crate".into()
        } else if first == "self" {
            self.module.clone()
        } else if first == "super" {
            self.module
                .rsplit_once("::")
                .map_or("crate", |(p, _)| p)
                .to_owned()
        } else if let Some(import) = self.imports.get(first) {
            if let Some(external) = import.strip_prefix("::") {
                external.to_owned()
            } else if self
                .local_modules
                .contains(import.split("::").next().unwrap_or(""))
            {
                format!("{}::{import}", self.module)
            } else {
                import.clone()
            }
        } else if path.leading_colon.is_some()
            || (self.extern_roots.contains(first) && !self.local_modules.contains(first))
        {
            first.clone()
        } else {
            format!("{}::{first}", self.module)
        };
        if rest.is_empty() {
            base
        } else {
            format!("{base}::{rest}")
        }
    }
    pub fn known_type(&self, path: &SynPath) -> Option<String> {
        let full = self.path(path);
        self.symbols.contains(&full).then_some(full)
    }
}
#[derive(Clone)]
struct Context {
    path: PathBuf,
    source: String,
    crate_id: String,
    module: String,
    items: Vec<Item>,
    directory: PathBuf,
    imports: BTreeMap<String, String>,
    glob: bool,
    configuration: String,
    complete: bool,
}
#[derive(Clone)]
struct View {
    context: std::rc::Rc<Context>,
    resolver: Resolver,
    item: ItemImpl,
    method: syn::ImplItemFn,
    target: TargetDescriptor,
    identity: String,
    component: String,
    policy: ContentPolicy,
    policy_range: SourceRange,
    config: String,
}
fn tokens<T: ToTokens>(value: &T) -> String {
    value.to_token_stream().to_string()
}
fn attrs_cfg(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
        .map(tokens)
        .collect::<Vec<_>>()
        .join(" ")
}
fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs_cfg(attrs).contains("test")
}
fn imports(
    tree: &UseTree,
    prefix: &str,
    map: &mut BTreeMap<String, String>,
    glob: &mut bool,
    module: &str,
) {
    let resolve = |p: String| {
        if let Some(rest) = p.strip_prefix("self::") {
            format!("{module}::{rest}")
        } else if let Some(rest) = p.strip_prefix("super::") {
            format!(
                "{}::{rest}",
                module.rsplit_once("::").map_or("crate", |(p, _)| p)
            )
        } else {
            p
        }
    };
    match tree {
        UseTree::Path(p) => imports(
            &p.tree,
            &format!("{prefix}{}::", p.ident),
            map,
            glob,
            module,
        ),
        UseTree::Name(n) => {
            if n.ident == "self" {
                let p = prefix.trim_end_matches("::");
                map.insert(p.rsplit("::").next().unwrap_or(p).into(), resolve(p.into()));
            } else {
                map.insert(n.ident.to_string(), resolve(format!("{prefix}{}", n.ident)));
            }
        }
        UseTree::Rename(n) => {
            map.insert(
                n.rename.to_string(),
                resolve(format!("{prefix}{}", n.ident)),
            );
        }
        UseTree::Group(g) => {
            for t in &g.items {
                imports(t, prefix, map, glob, module)
            }
        }
        UseTree::Glob(_) => {
            *glob = true;
        }
    }
}
fn contexts(sources: &[(PathBuf, String)], include_tests: bool) -> Vec<Context> {
    let mut files = BTreeMap::new();
    for (path, source) in sources {
        files.insert(
            absolute(path),
            (source.clone(), syn::parse_file(source).ok()),
        );
    }
    let mut contexts = Vec::new();
    let mut visited = BTreeSet::new();
    let roots: Vec<_> = files
        .keys()
        .filter(|p| {
            matches!(
                p.file_name().and_then(|s| s.to_str()),
                Some("lib.rs" | "main.rs")
            ) || p
                .parent()
                .is_some_and(|d| d.file_name().is_some_and(|n| n == "bin"))
        })
        .cloned()
        .collect();
    fn descend(
        path: &Path,
        directory: PathBuf,
        module: String,
        crate_id: String,
        config: String,
        inherited: BTreeMap<String, String>,
        complete: bool,
        files: &BTreeMap<PathBuf, (String, Option<syn::File>)>,
        visited: &mut BTreeSet<PathBuf>,
        out: &mut Vec<Context>,
        include_tests: bool,
        inline: Option<Vec<Item>>,
    ) {
        let Some((source, parsed)) = files.get(path) else {
            return;
        };
        let items = if let Some(items) = inline {
            items
        } else {
            if !visited.insert(path.to_owned()) {
                return;
            }
            match parsed {
                Some(f) => f.items.clone(),
                None => return,
            }
        };
        let config = format!(
            "{config} {}",
            parsed
                .as_ref()
                .map(|f| attrs_cfg(&f.attrs))
                .unwrap_or_default()
        )
        .trim()
        .to_owned();
        let mut map = inherited;
        let mut glob = false;
        for item in &items {
            if let Item::Use(u) = item {
                imports(
                    &u.tree,
                    if u.leading_colon.is_some() { "::" } else { "" },
                    &mut map,
                    &mut glob,
                    &module,
                );
            }
        }
        out.push(Context {
            path: path.into(),
            source: source.clone(),
            module: module.clone(),
            crate_id: crate_id.clone(),
            items: items.clone(),
            directory: directory.clone(),
            imports: map.clone(),
            glob,
            configuration: config.clone(),
            complete,
        });
        for item in items {
            if let Item::Mod(m) = item {
                if !include_tests && is_test(&m.attrs) {
                    continue;
                }
                let child = format!("{module}::{}", m.ident);
                let cfg = format!("{config} {}", attrs_cfg(&m.attrs))
                    .trim()
                    .to_owned();
                // Child modules do not inherit parent imports, unless explicitly
                // imported with `super::...`; avoid inventing lexical visibility.
                if let Some((_, items)) = m.content {
                    descend(
                        path,
                        directory.join(m.ident.to_string()),
                        child,
                        crate_id.clone(),
                        cfg,
                        BTreeMap::new(),
                        complete,
                        files,
                        visited,
                        out,
                        include_tests,
                        Some(items),
                    );
                } else {
                    let explicit =
                        m.attrs
                            .iter()
                            .find(|a| a.path().is_ident("path"))
                            .and_then(|a| match &a.meta {
                                syn::Meta::NameValue(n) => match &n.value {
                                    syn::Expr::Lit(l) => match &l.lit {
                                        syn::Lit::Str(s) => Some(s.value()),
                                        _ => None,
                                    },
                                    _ => None,
                                },
                                _ => None,
                            });
                    let file = explicit
                        .as_ref()
                        .map(|s| path.parent().unwrap_or(Path::new(".")).join(s))
                        .unwrap_or_else(|| {
                            let direct = directory.join(format!("{}.rs", m.ident));
                            if files.contains_key(&absolute(&direct)) {
                                direct
                            } else {
                                directory.join(m.ident.to_string()).join("mod.rs")
                            }
                        });
                    let file = absolute(&file);
                    let child_directory = if file.file_name().is_some_and(|n| n == "mod.rs") {
                        file.parent().unwrap_or(Path::new(".")).to_owned()
                    } else {
                        file.with_extension("")
                    };
                    descend(
                        &file,
                        child_directory,
                        child,
                        crate_id.clone(),
                        cfg,
                        BTreeMap::new(),
                        complete && explicit.is_none(),
                        files,
                        visited,
                        out,
                        include_tests,
                        None,
                    );
                }
            }
        }
    }
    let mut reached = BTreeSet::new();
    for root in roots {
        if !include_tests && super::super::is_test_path(&root) {
            continue;
        }
        if let Some(package) = package(&root) {
            let src = package.join("src");
            if root != src.join("lib.rs")
                && root != src.join("main.rs")
                && root.parent() != Some(src.join("bin").as_path())
            {
                continue;
            }
        }
        let crate_id = format!(
            "{}:{}",
            package(&root)
                .unwrap_or_else(|| root.parent().unwrap_or(Path::new(".")).into())
                .display(),
            root.display()
        );
        visited.clear();
        descend(
            &root,
            root.parent().unwrap_or(Path::new(".")).into(),
            "crate".into(),
            crate_id,
            String::new(),
            BTreeMap::new(),
            true,
            &files,
            &mut visited,
            &mut contexts,
            include_tests,
            None,
        );
        reached.extend(visited.iter().cloned());
    }
    visited = reached;
    // A standalone snippet can be analyzed locally. Cross-file/missing-view
    // assertions require a discovered module root; detached files remain bounded.
    for path in files.keys() {
        if !visited.contains(path) && (include_tests || !super::super::is_test_path(path)) {
            descend(
                path,
                path.parent().unwrap_or(Path::new(".")).into(),
                "crate".into(),
                format!("detached:{}", path.display()),
                String::new(),
                BTreeMap::new(),
                false,
                &files,
                &mut visited,
                &mut contexts,
                include_tests,
                None,
            );
        }
    }
    let mut incomplete = BTreeSet::new();
    for context in &contexts {
        for item in &context.items {
            if matches!(item,Item::Macro(mac) if mac.ident.is_none()) {
                incomplete.insert(context.crate_id.clone());
            }
            if let Item::Mod(module) = item {
                if module.content.is_some() || (!include_tests && is_test(&module.attrs)) {
                    continue;
                }
                let direct = absolute(&context.directory.join(format!("{}.rs", module.ident)));
                let nested = absolute(
                    &context
                        .directory
                        .join(module.ident.to_string())
                        .join("mod.rs"),
                );
                if module
                    .attrs
                    .iter()
                    .any(|a| a.path().is_ident("path") || a.path().is_ident("cfg_attr"))
                    || ![direct, nested]
                        .iter()
                        .any(|p| files.get(p).is_some_and(|(_, parsed)| parsed.is_some()))
                {
                    incomplete.insert(context.crate_id.clone());
                }
            }
        }
    }
    for context in &mut contexts {
        if incomplete.contains(&context.crate_id) {
            context.complete = false;
        }
    }
    contexts
}
fn type_key(ty: &Type, resolver: &Resolver, generics: &syn::Generics) -> Option<String> {
    let declared: BTreeSet<_> = generics
        .params
        .iter()
        .map(|p| match p {
            syn::GenericParam::Type(t) => t.ident.to_string(),
            syn::GenericParam::Lifetime(l) => l.lifetime.ident.to_string(),
            syn::GenericParam::Const(c) => c.ident.to_string(),
        })
        .collect();
    let mut params = BTreeMap::new();
    fn normalized(
        stream: proc_macro2::TokenStream,
        declared: &BTreeSet<String>,
        params: &mut BTreeMap<String, String>,
    ) -> String {
        stream
            .into_iter()
            .map(|token| match token {
                proc_macro2::TokenTree::Ident(ident) if declared.contains(&ident.to_string()) => {
                    let index = params.len();
                    params
                        .entry(ident.to_string())
                        .or_insert_with(|| format!("P{index}"))
                        .clone()
                }
                proc_macro2::TokenTree::Group(group) => format!(
                    "{:?} {}",
                    group.delimiter(),
                    normalized(group.stream(), declared, params)
                ),
                token => token.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    match ty {
        Type::Path(p) if p.qself.is_none() => {
            let mut path = p.path.clone();
            let args = path.segments.last()?.arguments.clone();
            for s in &mut path.segments {
                s.arguments = syn::PathArguments::None;
            }
            let base = resolver.known_type(&path)?;
            Some(format!(
                "{base}{}",
                normalized(args.to_token_stream(), &declared, &mut params)
            ))
        }
        Type::Reference(r) => Some(format!(
            "&{}{}",
            if r.mutability.is_some() { "mut " } else { "" },
            type_key(&r.elem, resolver, generics)?
        )),
        _ => None,
    }
}
fn bound_key(item: &ItemImpl, resolver: &Resolver, options: &ContentOptions) -> String {
    let names: BTreeSet<_> = item
        .generics
        .params
        .iter()
        .map(|p| match p {
            syn::GenericParam::Type(t) => t.ident.to_string(),
            syn::GenericParam::Lifetime(l) => l.lifetime.ident.to_string(),
            syn::GenericParam::Const(c) => c.ident.to_string(),
        })
        .collect();
    fn names_in(
        stream: proc_macro2::TokenStream,
        names: &BTreeSet<String>,
        map: &mut BTreeMap<String, String>,
    ) {
        for token in stream {
            match token {
                proc_macro2::TokenTree::Ident(i) if names.contains(&i.to_string()) => {
                    let index = map.len();
                    map.entry(i.to_string())
                        .or_insert_with(|| format!("P{index}"));
                }
                proc_macro2::TokenTree::Group(g) => names_in(g.stream(), names, map),
                _ => {}
            }
        }
    }
    let mut params = BTreeMap::new();
    names_in(item.self_ty.to_token_stream(), &names, &mut params);
    fn normalized(stream: proc_macro2::TokenStream, params: &BTreeMap<String, String>) -> String {
        stream
            .into_iter()
            .map(|t| match t {
                proc_macro2::TokenTree::Ident(i) => params
                    .get(&i.to_string())
                    .cloned()
                    .unwrap_or_else(|| i.to_string()),
                proc_macro2::TokenTree::Group(g) => normalized(g.stream(), params),
                t => t.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    let mut constraints = BTreeSet::new();
    let mut add =
        |ty: &Type, bounds: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::Token![+]>| {
            let mut used = BTreeMap::new();
            names_in(ty.to_token_stream(), &names, &mut used);
            if used.keys().all(|name| !params.contains_key(name)) {
                return;
            }
            for bound in bounds {
                if let syn::TypeParamBound::Trait(t) = bound {
                    let path = resolver.path(&t.path);
                    if path == "proa_core::DataLoader"
                        || path == "proa_core::MdCodeContent"
                        || options
                            .targets
                            .iter()
                            .any(|target| target.traits.contains(&path))
                    {
                        continue;
                    }
                }
                constraints.insert(format!(
                    "{}:{}",
                    normalized(ty.to_token_stream(), &params),
                    normalized(bound.to_token_stream(), &params)
                ));
            }
        };
    for parameter in item.generics.type_params() {
        let ident = &parameter.ident;
        let ty: Type = syn::parse_quote!(#ident);
        add(&ty, &parameter.bounds);
    }
    let mut other_constraints = Vec::new();
    if let Some(clause) = &item.generics.where_clause {
        for predicate in &clause.predicates {
            if let syn::WherePredicate::Type(t) = predicate {
                add(&t.bounded_ty, &t.bounds);
            } else {
                other_constraints.push(normalized(predicate.to_token_stream(), &params));
            }
        }
    }
    constraints.extend(other_constraints);
    constraints.into_iter().collect::<Vec<_>>().join(";")
}
fn content_attr(attr: &syn::Attribute, resolver: &Resolver) -> bool {
    resolver.path(attr.path()) == "proa_macros::content"
}
fn policy(
    attrs: &[syn::Attribute],
    resolver: &Resolver,
    context: &Context,
    diagnostics: &mut Vec<Diagnostic>,
) -> (ContentPolicy, Option<SourceRange>) {
    let mut result = ContentPolicy::default();
    let mut range = None;
    for attr in attrs.iter().filter(|a| content_attr(a, resolver)) {
        range = Some(span_range(&context.path, attr.span()));
        match attr.parse_args::<ContentPolicy>() {
            Ok(value) => result = value,
            Err(err) => diagnostics.push(Diagnostic::new(
                &context.path,
                err.span().start().line,
                err.span().start().column + 1,
                Severity::Error,
                "render/content-policy-invalid",
                err.to_string(),
            )),
        }
    }
    (result, range)
}

pub(super) fn lint_sources(
    sources: &[(PathBuf, String)],
    reporting: &BTreeSet<PathBuf>,
    include_tests: bool,
    options: &ContentOptions,
) -> Vec<Diagnostic> {
    let contexts = contexts(sources, include_tests);
    let mut diagnostics = Vec::new();
    let mut symbols: HashMap<String, BTreeSet<String>> = HashMap::new();
    for context in &contexts {
        for item in &context.items {
            let name = match item {
                Item::Struct(s) => Some(&s.ident),
                Item::Enum(e) => Some(&e.ident),
                _ => None,
            };
            if let Some(name) = name {
                symbols
                    .entry(context.crate_id.clone())
                    .or_default()
                    .insert(format!("{}::{name}", context.module));
            }
        }
    }
    // A render impl is also evidence of a named component declared by an
    // unexpanded macro. Only accept lexical local names without glob imports,
    // generic receivers, type aliases, or prelude primitives. Imported aliases
    // and blanket/external impls remain unresolved instead of being guessed.
    for context in &contexts {
        let resolver = Resolver {
            module: context.module.clone(),
            local_modules: context
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Mod(module) => Some(module.ident.to_string()),
                    _ => None,
                })
                .collect(),
            imports: context.imports.clone(),
            symbols: BTreeSet::new(),
            fields: BTreeMap::new(),
            constants: BTreeMap::new(),
            functions: BTreeSet::new(),
            optional_fields: BTreeMap::new(),
            unit_variants: BTreeSet::new(),
            child_selectors: BTreeMap::new(),
            generic_children: BTreeSet::new(),
            glob: context.glob,
            extern_roots: [
                "proa_core".into(),
                "proa_macros".into(),
                "std".into(),
                "core".into(),
            ]
            .into(),
        };
        for item in &context.items {
            if let Item::Impl(item) = item {
                let Some((_, trait_path, _)) = &item.trait_ else {
                    let (_, range) = policy(&item.attrs, &resolver, &context, &mut diagnostics);
                    if let Some(range) = range {
                        diagnostics.push(Diagnostic::new(
                            &range.path,
                            range.line,
                            range.column,
                            Severity::Error,
                            "render/content-policy-invalid",
                            "content policy belongs on a registered render trait impl",
                        ));
                    }
                    continue;
                };
                if !options
                    .targets
                    .iter()
                    .any(|t| t.traits.contains(&resolver.path(trait_path)))
                {
                    continue;
                }
                let mut ty = item.self_ty.as_ref();
                if let Type::Reference(reference) = ty {
                    ty = &reference.elem;
                }
                let Type::Path(ty) = ty else {
                    continue;
                };
                if ty.qself.is_some() || ty.path.segments.len() != 1 || context.glob {
                    continue;
                }
                let name = ty.path.segments[0].ident.to_string();
                if context.imports.contains_key(&name)
                    || item.generics.type_params().any(|p| p.ident == name)
                    || context
                        .items
                        .iter()
                        .any(|i| matches!(i,Item::Type(alias) if alias.ident==name))
                    || [
                        "String", "Vec", "Option", "Result", "Box", "bool", "char", "str", "usize",
                        "isize", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64",
                        "i128", "f32", "f64",
                    ]
                    .contains(&name.as_str())
                {
                    continue;
                }
                symbols
                    .entry(context.crate_id.clone())
                    .or_default()
                    .insert(format!("{}::{name}", context.module));
            }
        }
    }
    // Resolve explicit local reexports to their underlying symbol; repeated
    // passes support ordinary `pub use module::Type` chains without globs.
    let mut aliases = HashMap::<String, BTreeMap<String, String>>::new();
    for _ in 0..contexts.len().min(16) {
        for context in &contexts {
            for item in &context.items {
                if let Item::Use(u) = item {
                    if matches!(u.vis, syn::Visibility::Public(_)) {
                        let mut map = BTreeMap::new();
                        let mut glob = false;
                        imports(
                            &u.tree,
                            if u.leading_colon.is_some() { "::" } else { "" },
                            &mut map,
                            &mut glob,
                            &context.module,
                        );
                        for (name, mut target) in map {
                            if !target.starts_with("crate::") && !target.starts_with("proa_") {
                                target = format!("{}::{target}", context.module);
                            }
                            if let Some(resolved) =
                                aliases.get(&context.crate_id).and_then(|a| a.get(&target))
                            {
                                target = resolved.clone();
                            }
                            if symbols
                                .get(&context.crate_id)
                                .is_some_and(|s| s.contains(&target))
                            {
                                aliases
                                    .entry(context.crate_id.clone())
                                    .or_default()
                                    .insert(format!("{}::{name}", context.module), target);
                            }
                        }
                    }
                }
            }
        }
    }
    // Only suggest a type that Rust can instantiate without guessing its type
    // or const arguments. Lifetimes can use '_; generic inference remains bounded.
    let mut child_selectors = HashMap::<String, BTreeMap<String, String>>::new();
    let mut generic_children = HashMap::<String, BTreeSet<String>>::new();
    for context in &contexts {
        for item in &context.items {
            let (ident, generics) = match item {
                Item::Struct(item) => (&item.ident, &item.generics),
                Item::Enum(item) => (&item.ident, &item.generics),
                _ => continue,
            };
            let path = format!("{}::{ident}", context.module);
            if generics.type_params().next().is_some() || generics.const_params().next().is_some() {
                generic_children
                    .entry(context.crate_id.clone())
                    .or_default()
                    .insert(path);
                continue;
            }
            let lifetimes = vec!["'_"; generics.lifetimes().count()];
            let ty = if lifetimes.is_empty() {
                path.clone()
            } else {
                format!("{path}<{}>", lifetimes.join(", "))
            };
            child_selectors
                .entry(context.crate_id.clone())
                .or_default()
                .insert(path, ty);
        }
    }
    let mut fields = HashMap::<String, BTreeMap<String, Vec<String>>>::new();
    for context in &contexts {
        for item in &context.items {
            if let Item::Struct(s) = item {
                fields.entry(context.crate_id.clone()).or_default().insert(
                    format!("{}::{}", context.module, s.ident),
                    s.fields
                        .iter()
                        .filter_map(|f| f.ident.as_ref().map(ToString::to_string))
                        .collect(),
                );
            }
        }
    }
    let mut peer_targets = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for context in &contexts {
        for item in &context.items {
            if let Item::Impl(item) = item {
                if let Some((_, trait_path, _)) = &item.trait_ {
                    let mut name = trait_path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    if trait_path.segments.len() == 1 {
                        if let Some(import) = context.imports.get(&name) {
                            name = import.clone();
                        }
                    }
                    if let Some(target) = options.targets.iter().find(|t| t.traits.contains(&name))
                    {
                        peer_targets
                            .entry((context.crate_id.clone(), tokens(&item.self_ty)))
                            .or_default()
                            .insert(target.name.clone());
                    }
                }
            }
        }
    }
    let mut constants = HashMap::<String, BTreeMap<String, Option<String>>>::new();
    let mut functions = HashMap::<String, BTreeSet<String>>::new();
    for context in &contexts {
        for item in &context.items {
            match item {
                Item::Const(item) => {
                    let unconditional = context.configuration.is_empty()
                        && !item.attrs.iter().any(|attr| {
                            attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr")
                        });
                    let text = match item.expr.as_ref() {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(text),
                            ..
                        }) if unconditional => Some(text.value()),
                        _ => None,
                    };
                    constants
                        .entry(context.crate_id.clone())
                        .or_default()
                        .entry(format!("{}::{}", context.module, item.ident))
                        .and_modify(|value| *value = None)
                        .or_insert(text);
                }
                Item::Fn(item) => {
                    functions
                        .entry(context.crate_id.clone())
                        .or_default()
                        .insert(format!("{}::{}", context.module, item.sig.ident));
                }
                _ => {}
            }
        }
    }
    let mut optional_fields = HashMap::<String, BTreeMap<String, BTreeSet<String>>>::new();
    for context in &contexts {
        let shadows_option = context.imports.get("Option").is_some_and(|path| {
            !matches!(
                path.as_str(),
                "core::option::Option" | "std::option::Option"
            )
        }) || context.items.iter().any(|item| match item {
            Item::Struct(item) => item.ident == "Option",
            Item::Enum(item) => item.ident == "Option",
            Item::Type(item) => item.ident == "Option",
            _ => false,
        });
        for item in &context.items {
            if let Item::Struct(item) = item {
                if !context.configuration.is_empty()
                    || item
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
                {
                    continue;
                }
                let fields = optional_fields
                    .entry(context.crate_id.clone())
                    .or_default()
                    .entry(format!("{}::{}", context.module, item.ident))
                    .or_default();
                for field in &item.fields {
                    if field
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
                    {
                        continue;
                    }
                    let Some(name) = &field.ident else {
                        continue;
                    };
                    let syn::Type::Path(ty) = &field.ty else {
                        continue;
                    };
                    let path = ty
                        .path
                        .segments
                        .iter()
                        .map(|segment| segment.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    if (!shadows_option && path == "Option")
                        || matches!(
                            path.as_str(),
                            "core::option::Option" | "std::option::Option"
                        )
                    {
                        fields.insert(name.to_string());
                    }
                }
            }
        }
    }
    let mut unit_variants = HashMap::<String, BTreeSet<String>>::new();
    for context in &contexts {
        for item in &context.items {
            if let Item::Enum(item) = item {
                for variant in &item.variants {
                    if matches!(variant.fields, syn::Fields::Unit) {
                        unit_variants
                            .entry(context.crate_id.clone())
                            .or_default()
                            .insert(format!(
                                "{}::{}::{}",
                                context.module, item.ident, variant.ident
                            ));
                    }
                }
            }
        }
    }
    let mut views = Vec::new();
    for context in contexts {
        // Views share the parsed file/module rather than cloning its entire AST.
        let context = std::rc::Rc::new(context);
        let mut resolver = Resolver {
            module: context.module.clone(),
            local_modules: context
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Mod(module) => Some(module.ident.to_string()),
                    _ => None,
                })
                .collect(),
            imports: context.imports.clone(),
            symbols: symbols.get(&context.crate_id).cloned().unwrap_or_default(),
            fields: fields.get(&context.crate_id).cloned().unwrap_or_default(),
            unit_variants: unit_variants
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            optional_fields: optional_fields
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            constants: constants
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            functions: functions
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            child_selectors: child_selectors
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            generic_children: generic_children
                .get(&context.crate_id)
                .cloned()
                .unwrap_or_default(),
            extern_roots: [
                "proa_core".to_owned(),
                "proa_macros".to_owned(),
                "std".to_owned(),
                "core".to_owned(),
            ]
            .into_iter()
            .chain(
                options
                    .targets
                    .iter()
                    .flat_map(|t| {
                        t.traits
                            .iter()
                            .chain(&t.templates)
                            .chain(&t.adapters)
                            .chain(&t.future_sinks)
                    })
                    .filter_map(|identity| identity.split("::").next().map(str::to_owned))
                    .filter(|name| name != "crate"),
            )
            .collect(),
            glob: context.glob,
        };
        for value in resolver.imports.values_mut() {
            if let Some(external) = value.strip_prefix("::") {
                *value = external.to_owned();
            } else if resolver
                .local_modules
                .contains(value.split("::").next().unwrap_or(""))
            {
                *value = format!("{}::{value}", context.module);
            }
            if let Some(target) = aliases.get(&context.crate_id).and_then(|a| a.get(value)) {
                *value = target.clone();
            }
        }
        for item in &context.items {
            if let Item::Fn(function) = item {
                let (_, range) = policy(&function.attrs, &resolver, &context, &mut diagnostics);
                if let Some(range) = range {
                    diagnostics.push(Diagnostic::new(
                        &range.path,
                        range.line,
                        range.column,
                        Severity::Error,
                        "render/content-policy-invalid",
                        "content policy belongs on a render impl or its entry method",
                    ));
                }
            }
            if let Item::Impl(item) = item {
                if !include_tests && is_test(&item.attrs) {
                    continue;
                }
                let Some((_, trait_path, _)) = &item.trait_ else {
                    let (_, range) = policy(&item.attrs, &resolver, &context, &mut diagnostics);
                    if let Some(range) = range {
                        diagnostics.push(Diagnostic::new(
                            &range.path,
                            range.line,
                            range.column,
                            Severity::Error,
                            "render/content-policy-invalid",
                            "content policy belongs on a registered render trait impl",
                        ));
                    }
                    continue;
                };
                let trait_name = resolver.path(trait_path);
                let target = options
                    .targets
                    .iter()
                    .find(|t| t.traits.contains(&trait_name));
                let (impl_policy, impl_range) =
                    policy(&item.attrs, &resolver, &context, &mut diagnostics);
                let Some(target) = target else {
                    if resolver.glob
                        && options.targets.iter().any(|t| {
                            t.traits.iter().any(|identity| {
                                identity.rsplit("::").next()
                                    == trait_path
                                        .segments
                                        .last()
                                        .map(|s| s.ident.to_string())
                                        .as_deref()
                            })
                        })
                    {
                        diagnostics.push(
                            Diagnostic::new(
                                &context.path,
                                item.span().start().line,
                                1,
                                Severity::Warning,
                                "render/content-coverage-unchecked",
                                "render trait identity is hidden by a glob import",
                            )
                            .with_help(
                                "import the render trait explicitly so the target can be resolved",
                            ),
                        );
                    }
                    if impl_range.is_some() {
                        diagnostics.push(
                            Diagnostic::new(
                                &context.path,
                                item.span().start().line,
                                1,
                                Severity::Warning,
                                "render/content-coverage-unchecked",
                                "content policy is attached to an unregistered render trait",
                            )
                            .with_help("register this target's trait identity and analyzer"),
                        );
                    }
                    continue;
                };
                for method in &item.items {
                    if let ImplItem::Fn(method) = method {
                        let (mut method_policy, method_range) =
                            policy(&method.attrs, &resolver, &context, &mut diagnostics);
                        if !target.methods.contains(&method.sig.ident.to_string()) {
                            if method_range.is_some() {
                                diagnostics.push(Diagnostic::new(
                                    &context.path,
                                    method.span().start().line,
                                    1,
                                    Severity::Error,
                                    "render/content-policy-invalid",
                                    "content policy belongs on a registered render entry method",
                                ));
                            }
                            continue;
                        }
                        if method_range.is_none() {
                            method_policy = impl_policy.clone();
                        } else {
                            method_policy.skip |= impl_policy.skip;
                            if method_policy.source.is_none() {
                                method_policy.source = impl_policy.source.clone();
                            }
                            if method_policy.from.is_none() {
                                method_policy.from = impl_policy.from.clone();
                            }
                            if method_policy.reason.is_none() {
                                method_policy.reason = impl_policy.reason.clone();
                            }
                        }
                        let Some(identity) = type_key(&item.self_ty, &resolver, &item.generics)
                            .or_else(|| {
                                method_policy
                                    .skip
                                    .then(|| format!("@exempt:{}", tokens(&item.self_ty)))
                            })
                        else {
                            if reporting.contains(&context.path)
                                && (impl_range.is_some()
                                    || method_range.is_some()
                                    || options.report_status
                                    || peer_targets
                                        .get(&(context.crate_id.clone(), tokens(&item.self_ty)))
                                        .is_some_and(|targets| targets.len() > 1))
                            {
                                diagnostics.push(Diagnostic::new(&context.path,item.span().start().line,1,Severity::Warning,"render/content-coverage-unchecked","render component identity is not resolved (blanket impl, alias, or imported type)").with_help("use a locally declared component or explicit registered source identity"));
                            }
                            continue;
                        };
                        views.push(View {
                            context: context.clone(),
                            resolver: resolver.clone(),
                            item: item.clone(),
                            method: method.clone(),
                            target: target.clone(),
                            component: item
                                .self_ty
                                .span()
                                .source_text()
                                .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                                .unwrap_or_else(|| tokens(&item.self_ty)),
                            identity,
                            policy: method_policy,
                            policy_range: method_range
                                .or_else(|| impl_range.clone())
                                .unwrap_or_else(|| span_range(&context.path, item.span())),
                            config: format!(
                                "{} {} {} {}",
                                context.configuration,
                                attrs_cfg(&item.attrs),
                                attrs_cfg(&method.attrs),
                                bound_key(item, &resolver, options)
                            )
                            .trim()
                            .into(),
                        });
                    }
                }
            }
        }
    }
    let summaries: Vec<_> = views
        .iter()
        .map(|view| {
            analyze::summarize(
                &view.context.path,
                &view.context.source,
                &view.item,
                &view.method,
                &view.component,
                &view.target,
                &view.resolver,
                view.policy.skip,
            )
        })
        .collect();
    for (target_index, target) in views.iter().enumerate() {
        if options.report_status && reporting.contains(&target.context.path) {
            let paired = views.iter().any(|other| {
                other.context.crate_id == target.context.crate_id
                    && other.identity == target.identity
                    && other.target.name != target.target.name
            });
            let status = if target.policy.skip {
                "exempt"
            } else if paired {
                "paired"
            } else {
                "unpaired"
            };
            diagnostics.push(diag(
                &summaries[target_index],
                target.policy_range.clone(),
                "render/content-status",
                format!(
                    "`{}`'s {} view is {status}",
                    target.component, target.target.display_name
                ),
                if target.policy.skip {
                    "coverage is deliberately disabled for this whole view"
                } else {
                    "render view discovered"
                },
            ));
        }
        if target.policy.skip {
            continue;
        }
        let requested_sources: Vec<_> = if let Some(from) = &target.policy.from {
            vec![from.value()]
        } else {
            options
                .comparisons
                .iter()
                .filter(|(_, to)| to == &target.target.name)
                .map(|(from, _)| from.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        for from in requested_sources {
            if !options.targets.iter().any(|t| t.name == from) {
                if reporting.contains(&target.context.path) {
                    diagnostics.push(diag(
                        &summaries[target_index],
                        target.policy_range.clone(),
                        "render/content-policy-invalid",
                        format!("unknown source render target `{from}`"),
                        "register this target before selecting it",
                    ));
                }
                continue;
            }

            let identity = if let Some(source) = &target.policy.source {
                type_key(source, &target.resolver, &target.item.generics)
            } else {
                Some(target.identity.clone())
            };
            let candidates: Vec<_> = views
                .iter()
                .enumerate()
                .filter(|(_, v)| {
                    v.context.crate_id == target.context.crate_id
                        && Some(&v.identity) == identity.as_ref()
                        && v.target.name == from
                })
                .collect();
            let report = reporting.contains(&target.context.path)
                || candidates
                    .iter()
                    .any(|(_, v)| reporting.contains(&v.context.path));
            if !report {
                continue;
            }
            if candidates.is_empty() {
                if target.context.complete {
                    for marker in &summaries[target_index].omissions {
                        if marker.from.as_deref().is_none_or(|name| name == from) {
                            diagnostics.push(diag(&summaries[target_index],marker.range.clone(),"render/content-omit-stale",format!("this omission has no {from} source view to acknowledge"),"no source contribution exists in the discovered module graph").with_help("remove this omission marker, or restore the declared source view"));
                        }
                    }
                }
                if target.policy.source.is_some() || target.policy.from.is_some() {
                    diagnostics.push(
                        diag(
                            &summaries[target_index],
                            target.policy_range.clone(),
                            "render/content-coverage-unchecked",
                            format!(
                                "the declared {from} source for `{}` could not be resolved",
                                target.component
                            ),
                            "no resolvable source view in the indexed module graph",
                        )
                        .with_help("check the source type, imported paths, and analysis root"),
                    );
                }
                continue;
            }
            let exact: Vec<_> = candidates
                .iter()
                .filter(|(_, v)| v.config == target.config)
                .collect();
            let signature = |summary: &ViewSummary| {
                summary
                    .contributions
                    .iter()
                    .map(|c| {
                        (
                            c.key.clone(),
                            c.presence
                                .iter()
                                .map(|p| p.expression.clone())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<BTreeSet<_>>()
            };
            let variants_agree = !exact.is_empty()
                && exact.len() == candidates.len()
                && exact
                    .iter()
                    .all(|(i, _)| signature(&summaries[*i]) == signature(&summaries[exact[0].0]));
            if !variants_agree {
                diagnostics.push(
                    diag(
                        &summaries[target_index],
                        target.policy_range.clone(),
                        "render/content-coverage-unchecked",
                        format!(
                            "`{}` has ambiguous or configuration-dependent {from} views",
                            target.component
                        ),
                        "the checker cannot select one comparable configuration",
                    )
                    .with_help(
                        "align cfg boundaries and output contributions across execution variants",
                    ),
                );
                continue;
            }
            let (source_index, source_view) = exact[0];
            // A field spelling cannot establish correspondence between
            // different data types. Explicit pairs must map semantic roles.
            let mut source_summary = summaries[*source_index].clone();
            for (index, _) in &exact {
                if *index != *source_index {
                    source_summary
                        .boundaries
                        .extend(summaries[*index].boundaries.clone());
                }
            }
            let mut target_summary = summaries[target_index].clone();
            if source_view.identity != target.identity {
                source_summary
                    .contributions
                    .retain(|c| matches!(c.key, ContentKey::Role(_)));
                target_summary
                    .contributions
                    .retain(|c| matches!(c.key, ContentKey::Role(_)));
                if source_summary.contributions.len()
                    != summaries[*source_index].contributions.len()
                    || target_summary.contributions.len()
                        != summaries[target_index].contributions.len()
                {
                    source_summary.boundaries.push(Boundary {may_hide_content: true, message:"separate component types require named semantic parts; field spellings do not establish correspondence".into(),range:source_summary.entry.clone()});
                }
            }
            diagnostics.extend(compare_views(&source_summary, &target_summary));
        }
        if reporting.contains(&target.context.path) {
            for omission in &summaries[target_index].omissions {
                if let Some(from) = &omission.from {
                    if !options.targets.iter().any(|t| &t.name == from) {
                        diagnostics.push(diag(
                            &summaries[target_index],
                            omission.range.clone(),
                            "render/content-policy-invalid",
                            format!("unknown omission source target `{from}`"),
                            "this target is not registered",
                        ));
                    }
                }
            }
        }
    }
    diagnostics.retain(|d| {
        reporting.contains(&absolute(&d.path))
            || d.labels
                .iter()
                .any(|l| reporting.contains(&absolute(&l.range.path)))
    });
    diagnostics
}
