// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::discover::Resolver;
use super::*;
use proa_dsl::content::{ContentPart, OmitContent, OmitSelector};
use proa_dsl::md::{ExprLocation, MdNodeKind, MdRoot, MdTemplateInput, TemplateExpr, TplNode};
use proa_dsl::parse::{Attribute, HtmlRoot, Node};
use quote::ToTokens;
use syn::{spanned::Spanned, Expr, ImplItemFn, ItemImpl, Pat, Stmt};

type Env = BTreeMap<String, Value>;
#[derive(Clone, Default)]
struct Value {
    parts: Vec<Contribution>,
    boundaries: Vec<Boundary>,
    deps: BTreeSet<String>,
    symbol: Option<String>,
    predicate: Option<String>,
    template: bool,
    elements: Vec<Value>,
    fields: BTreeMap<String, Value>,
    /// A proven Proa metadata handle; mutations here do not write the body.
    metadata: bool,
    /// A deferred callback/future; handing it to opaque code may execute writes.
    callback: bool,
}
impl Value {
    fn append(&mut self, other: Value) {
        self.parts.extend(other.parts);
        self.boundaries.extend(other.boundaries);
        self.deps.extend(other.deps);
    }
    fn guarded(mut self, guards: &[Presence]) -> Self {
        for part in &mut self.parts {
            let mut p = guards.to_vec();
            p.extend(part.presence.clone());
            // Implicit Option presence and an explicit `if let Some(...)`
            // can prove the same guard. Keep one obligation, in source order.
            let mut seen = BTreeSet::new();
            p.retain(|guard| seen.insert(guard.expression.clone()));
            part.presence = p;
        }
        self
    }
}
struct Analyzer<'a> {
    path: &'a Path,
    target: &'a TargetDescriptor,
    resolver: &'a Resolver,
    context: String,
    proa_context: bool,
    optional_inputs: BTreeSet<String>,
    omissions: Vec<Omission>,
    bindings: BTreeMap<String, Vec<ContentKey>>,
    bindings_initialized: bool,
    sink_range: Option<SourceRange>,
}
fn bare(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
            Some(p.path.segments[0].ident.to_string())
        }
        Expr::Paren(p) => bare(&p.expr),
        Expr::Reference(r) => bare(&r.expr),
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => bare(&u.expr),
        _ => None,
    }
}
fn syntax<T: ToTokens>(t: &T) -> String {
    t.to_token_stream().to_string()
}
fn has_early_exit(block: &syn::Block) -> bool {
    struct Exit(bool);
    impl<'ast> syn::visit::Visit<'ast> for Exit {
        fn visit_expr_return(&mut self, _: &'ast syn::ExprReturn) {
            self.0 = true;
        }
        fn visit_expr_break(&mut self, _: &'ast syn::ExprBreak) {
            self.0 = true;
        }
        fn visit_expr_continue(&mut self, _: &'ast syn::ExprContinue) {
            self.0 = true;
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
        fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}
        fn visit_item(&mut self, _: &'ast syn::Item) {}
    }
    let mut visitor = Exit(false);
    syn::visit::Visit::visit_block(&mut visitor, block);
    visitor.0
}
fn predicate_expression(expr: &Expr, env: &Env) -> Option<String> {
    match expr {
        Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            if name == "self" {
                Some("self".into())
            } else {
                env.get(&name).and_then(|v| {
                    if v.symbol
                        .as_deref()
                        .is_some_and(|symbol| symbol.starts_with("@context"))
                    {
                        None
                    } else {
                        v.predicate.clone().or_else(|| v.symbol.clone())
                    }
                })
            }
        }
        Expr::Field(field) => Some(format!(
            "{}.{}",
            predicate_expression(&field.base, env)?,
            syntax(&field.member)
        )),
        Expr::Lit(literal) => Some(syntax(&literal.lit)),
        Expr::Binary(binary) => Some(format!(
            "({} {} {})",
            predicate_expression(&binary.left, env)?,
            syntax(&binary.op),
            predicate_expression(&binary.right, env)?
        )),
        Expr::Unary(unary) => Some(format!(
            "{}({})",
            syntax(&unary.op),
            predicate_expression(&unary.expr, env)?
        )),
        Expr::Paren(paren) => predicate_expression(&paren.expr, env),
        Expr::Reference(reference) => predicate_expression(&reference.expr, env),
        Expr::MethodCall(method)
            if method.args.is_empty()
                && matches!(
                    method.method.to_string().as_str(),
                    "is_empty" | "is_some" | "is_none" | "len" | "as_ref" | "as_str" | "as_deref"
                ) =>
        {
            Some(format!(
                "{}.{}()",
                predicate_expression(&method.receiver, env)?,
                method.method
            ))
        }
        _ => None,
    }
}
fn bind(pat: &Pat, value: Value, env: &mut Env) {
    match pat {
        Pat::Ident(p) => {
            env.insert(p.ident.to_string(), value.clone());
            if let Some((_, pattern)) = &p.subpat {
                bind(pattern, value, env);
            }
        }
        Pat::Type(p) => bind(&p.pat, value, env),
        Pat::Reference(p) => bind(&p.pat, value, env),
        Pat::Struct(p) => {
            for field in &p.fields {
                let member = syntax(&field.member);
                let mut v = if let Some(field) = value.fields.get(&member) {
                    field.clone()
                } else if value.symbol.is_some() {
                    value.clone()
                } else {
                    Value {
                        boundaries: vec![Boundary {
                            may_hide_content: true,
                            message: "destructured field has unresolved provenance".into(),
                            range: span_range(Path::new(""), field.span()),
                        }],
                        ..Value::default()
                    }
                };
                if value.symbol.is_some() {
                    v.symbol = value.symbol.as_ref().map(|s| format!("{s}.{member}"));
                }
                if let Some(symbol) = &v.symbol {
                    v.deps = BTreeSet::from([symbol.clone()]);
                    v.parts.clear();
                    v.predicate = None;
                }
                bind(&field.pat, v, env);
            }
        }
        Pat::Tuple(p) => {
            for (index, pat) in p.elems.iter().enumerate() {
                let mut v = value.elements.get(index).cloned().unwrap_or_else(|| {
                    if value.symbol.is_some() {
                        value.clone()
                    } else {
                        Value {
                            boundaries: vec![Boundary {
                                may_hide_content: true,
                                message: "destructured tuple has unresolved provenance".into(),
                                range: span_range(Path::new(""), pat.span()),
                            }],
                            ..Value::default()
                        }
                    }
                });
                if value.symbol.is_some() {
                    v.symbol = value.symbol.as_ref().map(|s| format!("{s}.{index}"));
                }
                if let Some(symbol) = &v.symbol {
                    v.deps = BTreeSet::from([symbol.clone()]);
                    v.parts.clear();
                    v.predicate = None;
                }
                bind(pat, v, env);
            }
        }
        _ => {
            struct Bindings(Vec<String>);
            impl<'ast> syn::visit::Visit<'ast> for Bindings {
                fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
                    self.0.push(pat.ident.to_string());
                    syn::visit::visit_pat_ident(self, pat);
                }
            }
            let mut names = Bindings(Vec::new());
            syn::visit::Visit::visit_pat(&mut names, pat);
            for name in names.0 {
                env.insert(
                    name,
                    Value {
                        boundaries: vec![Boundary {
                            may_hide_content: true,
                            message: "this binding pattern's provenance is not analyzed".into(),
                            range: span_range(Path::new(""), pat.span()),
                        }],
                        ..Value::default()
                    },
                );
            }
        }
    }
}
impl Analyzer<'_> {
    fn range(&self, span: Span, override_range: Option<&SourceRange>) -> SourceRange {
        override_range
            .cloned()
            .unwrap_or_else(|| span_range(self.path, span))
    }
    fn boundary(
        &self,
        expr: &Expr,
        message: impl Into<String>,
        override_range: Option<&SourceRange>,
    ) -> Value {
        let message = message.into();
        Value {
            boundaries: vec![Boundary {
                may_hide_content: !message.starts_with("unnamed static"),
                message,
                range: self.range(expr.span(), override_range),
            }],
            ..Value::default()
        }
    }
    fn part(&self, key: ContentKey, expr: &Expr, override_range: Option<&SourceRange>) -> Value {
        Value {
            deps: key.inputs(),
            parts: vec![Contribution {
                key,
                presence: Vec::new(),
                range: self.range(expr.span(), override_range),
            }],
            ..Value::default()
        }
    }
    fn option_variant(&self, path: &syn::Path, env: &Env, variant: &str) -> bool {
        let name = self.resolver.path(path);
        (name == format!("core::option::Option::{variant}")
            || name == format!("std::option::Option::{variant}"))
            || (path.segments.len() == 1
                && path.is_ident(variant)
                && name == format!("{}::{variant}", self.resolver.module)
                && !env.contains_key(variant)
                && !self.resolver.functions.contains(&name)
                && !self.resolver.constants.contains_key(&name)
                && self.resolver.known_type(path).is_none())
    }

    fn bind_condition(
        &mut self,
        binding: &syn::ExprLet,
        env: &Env,
        inner: &mut Env,
        location: Option<&SourceRange>,
    ) {
        let value = self.expr(&binding.expr, env, location);
        if let Pat::TupleStruct(pattern) = binding.pat.as_ref() {
            if self.option_variant(&pattern.path, env, "Some") && pattern.elems.len() == 1 {
                bind(&pattern.elems[0], value, inner);
                return;
            }
        }
        bind(&binding.pat, value, inner);
    }

    fn type_identity(&self, ty: &syn::TypePath) -> String {
        if ty.qself.is_some() {
            return syntax(ty);
        }
        let mut identity = self.resolver.path(&ty.path);
        for segment in &ty.path.segments {
            if let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments {
                let arguments = arguments
                    .args
                    .iter()
                    .map(|argument| match argument {
                        syn::GenericArgument::Type(syn::Type::Path(ty)) => self.type_identity(ty),
                        _ => syntax(argument),
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                identity.push_str(&format!("<{arguments}>"));
            }
        }
        identity
    }

    fn input_identity(&self, expr: &Expr, env: &Env) -> Option<String> {
        if let Some(identity) = predicate_expression(expr, env) {
            return Some(identity);
        }
        match expr {
            Expr::Call(call) => {
                let Expr::Path(function) = call.func.as_ref() else {
                    return None;
                };
                if bare(&call.func).is_some_and(|name| env.contains_key(&name)) {
                    return None;
                }
                let args = call
                    .args
                    .iter()
                    .map(|arg| self.input_identity(arg, env))
                    .collect::<Option<Vec<_>>>()?;
                Some(format!(
                    "{}({})",
                    self.resolver.path(&function.path),
                    args.join(",")
                ))
            }
            _ => None,
        }
    }

    fn static_markup(
        &mut self,
        expr: &Expr,
        env: &Env,
        location: Option<&SourceRange>,
    ) -> Option<Value> {
        let text = match expr {
            Expr::Lit(literal) => {
                let syn::Lit::Str(text) = &literal.lit else {
                    return None;
                };
                text.value()
            }
            Expr::Path(path) if !bare(expr).is_some_and(|name| env.contains_key(&name)) => self
                .resolver
                .constants
                .get(&self.resolver.path(&path.path))?
                .clone()?,
            _ => return None,
        };
        let root = syn::parse2::<HtmlRoot>(text.parse().ok()?).ok()?;
        // Rust-template expressions and bare text are not a valid model of raw
        // HTML. Only inspect fully static markup; otherwise retain the boundary.
        fn static_nodes(nodes: &[Node]) -> bool {
            nodes.iter().all(|node| match node {
                Node::Element(element) => {
                    !element
                        .attrs
                        .iter()
                        .any(|attr| matches!(attr, Attribute::Dynamic { .. }))
                        && static_nodes(&element.children)
                }
                Node::Fragment(children) => static_nodes(children),
                Node::Doctype(_) => true,
                _ => false,
            })
        }
        static_nodes(&root.nodes).then(|| self.html(&root.nodes, env, location))
    }

    fn literal(&self, text: &str, range: SourceRange) -> Value {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.chars().any(char::is_alphanumeric) {
            return Value::default();
        }
        Value {
            parts: vec![Contribution {
                key: ContentKey::Literal(text),
                presence: Vec::new(),
                range,
            }],
            ..Value::default()
        }
    }

    fn markdown_text(&self, text: &str, range: SourceRange) -> Value {
        use pulldown_cmark::{Event, Parser, Tag, TagEnd};
        let mut value = Value::default();
        let mut paragraph = String::new();
        for event in Parser::new(text) {
            match event {
                Event::Text(text) | Event::Code(text) => paragraph.push_str(&text),
                Event::SoftBreak | Event::HardBreak => paragraph.push(' '),
                Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                    value.append(self.literal(&dest_url, range.clone()));
                }
                Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::CodeBlock
                    | TagEnd::Item
                    | TagEnd::TableCell,
                ) => {
                    value.append(self.literal(&paragraph, range.clone()));
                    paragraph.clear();
                }
                _ => {}
            }
        }
        value.append(self.literal(&paragraph, range));
        value
    }

    fn predicate(
        &mut self,
        expr: &Expr,
        env: &Env,
        negated: bool,
        override_range: Option<&SourceRange>,
    ) -> Presence {
        if let Expr::Let(condition) = expr {
            if let Pat::TupleStruct(pattern) = condition.pat.as_ref() {
                if self.option_variant(&pattern.path, env, "Some") && pattern.elems.len() == 1 {
                    if let Some(symbol) = self.expr(&condition.expr, env, override_range).symbol {
                        return Presence {
                            expression: if negated {
                                format!("!({symbol}.is_some())")
                            } else {
                                format!("{symbol}.is_some()")
                            },
                            range: self.range(expr.span(), override_range),
                        };
                    }
                }
            }
        }
        // Rewrite only proven local input aliases. Never resolve a local by
        // looking up later bindings with the same spelling.
        let expression = predicate_expression(expr, env).unwrap_or_else(|| {
            format!(
                "@unchecked:{}:{}:{}:{}",
                self.path.display(),
                self.range(expr.span(), override_range).line,
                self.range(expr.span(), override_range).column,
                syntax(expr)
            )
        });
        Presence {
            expression: if negated {
                format!("!({expression})")
            } else {
                expression
            },
            range: self.range(expr.span(), override_range),
        }
    }
    fn expr(&mut self, expr: &Expr, env: &Env, override_range: Option<&SourceRange>) -> Value {
        match expr {
            Expr::Paren(p) => self.expr(&p.expr, env, override_range),
            Expr::Group(g) => self.expr(&g.expr, env, override_range),
            Expr::Reference(r) => self.expr(&r.expr, env, override_range),
            Expr::Try(t) => self.expr(&t.expr, env, override_range),
            Expr::Path(p) => {
                if self.option_variant(&p.path, env, "None") {
                    return Value::default();
                }
                if let Some(name) = bare(expr) {
                    if name == "self" {
                        return Value {
                            deps: BTreeSet::from(["self".into()]),
                            symbol: Some("self".into()),
                            ..Value::default()
                        };
                    }
                    if let Some(v) = env.get(&name) {
                        let mut v = v.clone();
                        if !v.template {
                            for part in &mut v.parts {
                                part.range = self.range(expr.span(), override_range);
                            }
                        }
                        if v.parts.is_empty() {
                            if let Some(s) = &v.symbol {
                                let mut output =
                                    self.part(ContentKey::Value(s.clone()), expr, override_range);
                                output.symbol = Some(s.clone());
                                return output;
                            }
                        }
                        return v;
                    }
                }
                let path = self.resolver.path(&p.path);
                if self.resolver.unit_variants.contains(&path) {
                    let symbol = format!("@variant:{path}");
                    let mut value =
                        self.part(ContentKey::Value(symbol.clone()), expr, override_range);
                    value.symbol = Some(symbol);
                    return value;
                }
                if let Some(ty) = self.resolver.known_type(&p.path) {
                    return self.part(
                        ContentKey::Child {
                            ty,
                            inputs: BTreeSet::new(),
                        },
                        expr,
                        override_range,
                    );
                }
                self.boundary(
                    expr,
                    format!(
                        "the value `{}` has no resolved input provenance",
                        syntax(expr)
                    ),
                    override_range,
                )
            }
            Expr::Field(f) => {
                let base = self.expr(&f.base, env, override_range);
                let projected = match &f.member {
                    syn::Member::Named(name) => base.fields.get(&name.to_string()),
                    syn::Member::Unnamed(index) => base.elements.get(index.index as usize),
                };
                if let Some(projected) = projected {
                    let mut value = projected.clone();
                    for part in &mut value.parts {
                        part.range = self.range(expr.span(), override_range);
                    }
                    return value;
                }
                if let Some(symbol) = base.symbol {
                    if symbol.starts_with("@context") {
                        return self.boundary(
                            expr,
                            "output-context state is not shared component content",
                            override_range,
                        );
                    }
                    let field = format!("{symbol}.{}", syntax(&f.member));
                    let mut value =
                        self.part(ContentKey::Value(field.clone()), expr, override_range);
                    value.symbol = Some(field.clone());
                    if self.optional_inputs.contains(&field) {
                        for part in &mut value.parts {
                            part.presence.push(Presence {
                                expression: format!("{field}.is_some()"),
                                range: self.range(expr.span(), override_range),
                            });
                        }
                    }
                    value
                } else {
                    self.boundary(
                        expr,
                        "a field access crosses a transformed or unresolved value",
                        override_range,
                    )
                }
            }
            Expr::Macro(m) => self.template(m, env, override_range),
            Expr::Call(c) => {
                if let Expr::Path(function) = c.func.as_ref() {
                    let path = self.resolver.path(&function.path);
                    if path == "proa_core::StaticRaw" && c.args.len() == 1 {
                        if let Some(value) = self.static_markup(&c.args[0], env, override_range) {
                            return value;
                        }
                    }
                    if c.args.len() == 1 && self.option_variant(&function.path, env, "Some") {
                        return self.expr(&c.args[0], env, override_range);
                    }
                    if self.target.adapters.contains(&path) && c.args.len() == 1 {
                        return self.expr(&c.args[0], env, override_range);
                    }
                    let mut child = function.path.clone();
                    if child.segments.last().is_some_and(|s| s.ident == "new") {
                        child.segments.pop();
                    }
                    if let Some(ty) = self.resolver.known_type(&child) {
                        let mut deps = BTreeSet::new();
                        let mut boundaries = Vec::new();
                        for (index, arg) in c.args.iter().enumerate() {
                            let v = self.expr(arg, env, override_range);
                            deps.insert(format!(
                                "@arg:{index}={}",
                                v.deps.iter().cloned().collect::<Vec<_>>().join("|")
                            ));
                            deps.extend(v.deps);
                            boundaries
                                .extend(v.boundaries.into_iter().filter(|b| b.may_hide_content));
                        }
                        let mut v =
                            self.part(ContentKey::Child { ty, inputs: deps }, expr, override_range);
                        v.boundaries = boundaries;
                        return v;
                    }
                }
                let mut v = self.boundary(
                    expr,
                    format!(
                        "helper `{}` produces output that is not analyzed",
                        syntax(&c.func)
                    ),
                    override_range,
                );
                for arg in &c.args {
                    v.deps.extend(self.expr(arg, env, override_range).deps);
                }
                v
            }
            Expr::Struct(s) => {
                if let Some(ty) = self.resolver.known_type(&s.path) {
                    let mut deps = BTreeSet::new();
                    let mut boundaries = Vec::new();
                    let mut fields = BTreeMap::new();
                    for field in &s.fields {
                        let member = syntax(&field.member);
                        fields.insert(member.clone(), self.expr(&field.expr, env, override_range));
                        if member == "class" || member == "style" || member.starts_with("on_") {
                            continue;
                        }
                        let v = self.expr(&field.expr, env, override_range);
                        deps.insert(format!(
                            "@field:{}={}",
                            syntax(&field.member),
                            v.deps.iter().cloned().collect::<Vec<_>>().join("|")
                        ));
                        deps.extend(v.deps);
                        boundaries.extend(v.boundaries.into_iter().filter(|b| b.may_hide_content));
                    }
                    if let Some(rest) = &s.rest {
                        let v = self.expr(rest, env, override_range);
                        deps.extend(v.deps);
                        boundaries.extend(v.boundaries);
                    }
                    let mut v =
                        self.part(ContentKey::Child { ty, inputs: deps }, expr, override_range);
                    v.boundaries = boundaries;
                    v.fields = fields;
                    v
                } else {
                    self.boundary(
                        expr,
                        "this constructed output type is unresolved",
                        override_range,
                    )
                }
            }
            Expr::MethodCall(m) => {
                if self.metadata_receiver(expr, env)
                    || (self.proa_context
                        && self.output_arg(&m.receiver, env)
                        && matches!(
                            m.method.to_string().as_str(),
                            "md_set_inline" | "md_push_heading_offset" | "md_pop_heading_offset"
                        ))
                {
                    let mut value = self.boundary(
                        expr,
                        "context metadata needs an explicit content mapping when rendered",
                        override_range,
                    );
                    value.metadata = true;
                    return value;
                }
                if self.proa_context
                    && self.output_arg(&m.receiver, env)
                    && m.method == "load_typed"
                {
                    if let Some(key) = m
                        .args
                        .iter()
                        .map(|arg| self.input_identity(arg, env))
                        .collect::<Option<Vec<_>>>()
                    {
                        let types = m
                            .turbofish
                            .as_ref()
                            .map(|args| {
                                args.args
                                    .iter()
                                    .map(|arg| match arg {
                                        syn::GenericArgument::Type(syn::Type::Path(ty)) => {
                                            self.type_identity(ty)
                                        }
                                        _ => syntax(arg),
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let symbol = format!("@load:typed:{types:?}:{key:?}");
                        return Value {
                            symbol: Some(symbol.clone()),
                            deps: BTreeSet::from([symbol]),
                            ..Value::default()
                        };
                    }
                }
                let receiver = self.expr(&m.receiver, env, override_range);
                if m.method == "map_or"
                    && m.args.len() == 2
                    && matches!(&m.args[0], Expr::Lit(lit) if matches!(&lit.lit, syn::Lit::Str(text) if text.value().is_empty()))
                    && matches!(&m.args[1], Expr::Path(path) if self.resolver.path(&path.path) == "proa_core::MdCodeContent::md_code_content")
                {
                    return receiver;
                }

                if m.args.is_empty()
                    && matches!(
                        m.method.to_string().as_str(),
                        "as_ref"
                            | "as_deref"
                            | "as_str"
                            | "clone"
                            | "copied"
                            | "cloned"
                            | "iter"
                            | "into_iter"
                            | "unwrap_or_default"
                    )
                {
                    return receiver;
                }
                if m.args.is_empty() && m.method == "len" {
                    if let Some(symbol) = receiver.symbol {
                        return self.part(
                            ContentKey::Value(format!("{symbol}.len()")),
                            expr,
                            override_range,
                        );
                    }
                }
                let mut v = self.boundary(
                    expr,
                    format!(
                        "method `{}` transforms output without a content mapping",
                        m.method
                    ),
                    override_range,
                );
                v.deps = receiver.deps;
                v
            }
            Expr::Lit(l) => {
                let meaningful = match &l.lit {
                    syn::Lit::Str(text) => text.value().chars().any(char::is_alphanumeric),
                    _ => true,
                };
                let mut value = if meaningful {
                    match &l.lit {
                        syn::Lit::Str(text) => {
                            self.literal(&text.value(), self.range(expr.span(), override_range))
                        }
                        _ => self.part(ContentKey::Literal(syntax(&l.lit)), expr, override_range),
                    }
                } else {
                    Value::default()
                };
                value.deps.insert(match &l.lit {
                    syn::Lit::Str(text) => format!("@literal:{:?}", text.value()),
                    _ => format!("@literal:{}", syntax(&l.lit)),
                });
                value
            }
            Expr::If(i) => {
                let condition = self.predicate(&i.cond, env, false, override_range);
                let mut branch_env = env.clone();
                if let Expr::Let(binding) = i.cond.as_ref() {
                    self.bind_condition(binding, env, &mut branch_env, override_range);
                }
                let mut v = self
                    .value_block(&i.then_branch, &branch_env, override_range)
                    .guarded(&[condition]);
                if let Some((_, other)) = &i.else_branch {
                    let condition = self.predicate(&i.cond, env, true, override_range);
                    v.append(self.expr(other, env, override_range).guarded(&[condition]));
                }
                v
            }
            Expr::Block(b) => self.value_block(&b.block, env, override_range),
            Expr::ForLoop(f) => self.loop_value(&f.pat, &f.expr, &f.body, env, override_range),
            Expr::Array(a) => {
                let mut v = Value::default();
                for element in &a.elems {
                    v.append(self.expr(element, env, override_range));
                }
                v
            }
            Expr::Tuple(tuple) => {
                let mut output = Value::default();
                for (index, expr) in tuple.elems.iter().enumerate() {
                    let mut value = self.expr(expr, env, override_range);
                    value.predicate = predicate_expression(expr, env);
                    output.deps.insert(format!(
                        "@element:{index}={}",
                        value.deps.iter().cloned().collect::<Vec<_>>().join("|")
                    ));
                    output.elements.push(value.clone());
                    output.append(value);
                }
                output
            }
            Expr::Closure(_) | Expr::Async(_) => {
                let mut value = self.boundary(
                    expr,
                    "a deferred callback requires a known invocation/iteration model",
                    override_range,
                );
                value.callback = true;
                value
            }
            Expr::Await(a) => {
                let mut v = self.expr(&a.base, env, override_range);
                if !v
                    .symbol
                    .as_deref()
                    .is_some_and(|symbol| symbol.starts_with("@load:"))
                {
                    v.boundaries.push(Boundary {
                        may_hide_content: true,
                        message: "awaited output needs an explicit named content mapping".into(),
                        range: self.range(expr.span(), override_range),
                    });
                }
                v
            }
            _ => self.boundary(
                expr,
                format!(
                    "this output expression (`{}`) is outside the supported analysis",
                    syntax(expr)
                ),
                override_range,
            ),
        }
    }
    fn local(&mut self, local: &syn::Local, env: &mut Env, override_range: Option<&SourceRange>) {
        if let Some(init) = &local.init {
            let mut value = self.expr(&init.expr, env, override_range);
            value.predicate = predicate_expression(&init.expr, env);
            bind(&local.pat, value.clone(), env);
            for value in env.values_mut() {
                for boundary in &mut value.boundaries {
                    if boundary.range.path.as_os_str().is_empty() {
                        boundary.range = self.range(local.span(), override_range);
                    }
                }
            }
            if init.diverge.is_some() {
                let mut value = value;
                value.boundaries.push(Boundary {
                    may_hide_content: true,
                    message: "let-else control flow is not analyzed".into(),
                    range: self.range(local.span(), override_range),
                });
                bind(&local.pat, value, env);
            }
        }
    }
    fn value_block(
        &mut self,
        block: &syn::Block,
        env: &Env,
        override_range: Option<&SourceRange>,
    ) -> Value {
        let mut env = env.clone();
        let mut result = Value::default();
        for stmt in &block.stmts {
            match stmt {
                Stmt::Local(local) => self.local(local, &mut env, override_range),
                Stmt::Expr(expr, None) => result = self.expr(expr, &env, override_range),
                Stmt::Expr(Expr::Assign(a), _) => {
                    if let Some(name) = bare(&a.left) {
                        env.insert(
                            name,
                            self.boundary(
                                expr_from_assign(a),
                                "assigned local has unresolved control flow",
                                override_range,
                            ),
                        );
                    }
                }
                Stmt::Expr(Expr::Return(r), _) => {
                    if let Some(value) = &r.expr {
                        result.append(self.expr(value, &env, override_range));
                        result.boundaries.push(Boundary {
                            may_hide_content: true,
                            message: "early return inside template output is not fully analyzed"
                                .into(),
                            range: self.range(r.span(), override_range),
                        });
                    }
                }
                // Side-effect statements may influence output, but unused values
                // and uncalled closures never contribute content.
                Stmt::Macro(m) if m.semi_token.is_none() => {
                    result = self.template(
                        &syn::ExprMacro {
                            attrs: m.attrs.clone(),
                            mac: m.mac.clone(),
                        },
                        &env,
                        override_range,
                    );
                }
                Stmt::Expr(_, Some(_)) | Stmt::Macro(_) | Stmt::Item(_) => {}
            }
        }
        if has_early_exit(block) {
            result.boundaries.push(Boundary {
                may_hide_content: true,
                message: "early exit inside output changes presence/iteration reachability".into(),
                range: span_range(self.path, block.span()),
            });
        }
        result
    }
    fn loop_value(
        &mut self,
        pat: &Pat,
        iter: &Expr,
        body: &syn::Block,
        env: &Env,
        override_range: Option<&SourceRange>,
    ) -> Value {
        let iterator = self.expr(iter, env, override_range);
        let mut child = env.clone();
        let Some(symbol) = iterator.symbol else {
            return self.boundary(
                iter,
                "iteration source cannot be resolved to a shared collection",
                override_range,
            );
        };
        let element = format!("{symbol}[]");
        bind(
            pat,
            Value {
                symbol: Some(element.clone()),
                deps: BTreeSet::from([element]),
                ..Value::default()
            },
            &mut child,
        );
        self.value_block(body, &child, override_range)
            .guarded(&[Presence {
                expression: format!("for {symbol}"),
                range: self.range(iter.span(), override_range),
            }])
    }
    fn named_value(
        &mut self,
        role: &str,
        expr: &Expr,
        env: &Env,
        location: Option<&SourceRange>,
    ) -> Value {
        if let Expr::If(condition) = expr {
            let guard = self.predicate(&condition.cond, env, false, location);
            let mut value = self
                .named_block(role, &condition.then_branch, env, location)
                .guarded(&[guard]);
            if let Some((_, otherwise)) = &condition.else_branch {
                let guard = self.predicate(&condition.cond, env, true, location);
                value.append(
                    self.named_value(role, otherwise, env, location)
                        .guarded(&[guard]),
                );
            }
            return value;
        }
        if let Expr::Block(block) = expr {
            return self.named_block(role, &block.block, env, location);
        }
        if matches!(expr,Expr::Tuple(tuple) if tuple.elems.is_empty()) {
            return Value::default();
        }
        let mut value = self.expr(expr, env, location);
        if value.parts.is_empty() {
            return self.part(ContentKey::Role(role.into()), expr, location);
        }
        for part in &mut value.parts {
            part.key = ContentKey::Role(role.into());
        }
        value.boundaries.clear();
        value.deps.clear();
        value.symbol = None;
        value.template = false;
        value
    }
    fn named_block(
        &mut self,
        role: &str,
        block: &syn::Block,
        env: &Env,
        location: Option<&SourceRange>,
    ) -> Value {
        let mut env = env.clone();
        let mut output = Value::default();
        for statement in &block.stmts {
            match statement {
                Stmt::Local(local) => self.local(local, &mut env, location),
                Stmt::Expr(expr, None) => output = self.named_value(role, expr, &env, location),
                Stmt::Macro(mac) if mac.semi_token.is_none() => {
                    output = self.named_value(
                        role,
                        &Expr::Macro(syn::ExprMacro {
                            attrs: mac.attrs.clone(),
                            mac: mac.mac.clone(),
                        }),
                        &env,
                        location,
                    )
                }
                _ => {}
            }
        }
        output
    }
    fn template(
        &mut self,
        expr: &syn::ExprMacro,
        env: &Env,
        override_range: Option<&SourceRange>,
    ) -> Value {
        let path = self.resolver.path(&expr.mac.path);
        if path == "proa_macros::content_part" {
            return match syn::parse2::<ContentPart>(expr.mac.tokens.clone()) {
                Ok(part) => self.named_value(&part.role.value(), &part.value, env, override_range),
                Err(err) => {
                    self.boundary(&Expr::Macro(expr.clone()), err.to_string(), override_range)
                }
            };
        }
        if !self.target.templates.contains(&path) {
            return self.boundary(
                &Expr::Macro(expr.clone()),
                format!("macro `{path}` is not a registered output template"),
                override_range,
            );
        }
        let mut output = match self.target.syntax {
            TemplateSyntax::Html => match syn::parse2::<HtmlRoot>(expr.mac.tokens.clone()) {
                Ok(root) => self.html(&root.nodes, env, override_range),
                Err(err) => self.boundary(
                    &Expr::Macro(expr.clone()),
                    format!("HTML template could not be analyzed: {err}"),
                    override_range,
                ),
            },
            TemplateSyntax::Custom(parser) => match parser(expr.mac.tokens.clone()) {
                Ok(nodes) => self.md(&nodes, env, &self.range(expr.span(), override_range)),
                Err(error) => self.boundary(
                    &Expr::Macro(expr.clone()),
                    format!(
                        "{} template could not be analyzed: {error}",
                        self.target.display_name
                    ),
                    override_range,
                ),
            },
            TemplateSyntax::Markdown => {
                if let Ok(input) = syn::parse2::<MdTemplateInput>(expr.mac.tokens.clone()) {
                    match input.semantic_nodes() {
                        Ok(nodes) => {
                            self.md(&nodes, env, &self.range(input.lit.span(), override_range))
                        }
                        Err(err) => self.boundary(
                            &Expr::Macro(expr.clone()),
                            format!("Markdown template could not be analyzed: {err}"),
                            override_range,
                        ),
                    }
                } else {
                    match syn::parse2::<MdRoot>(expr.mac.tokens.clone()) {
                        Ok(root) => {
                            let mut v = Value::default();
                            for node in &root.nodes {
                                match &node.kind {
                                    MdNodeKind::Block(e) => {
                                        v.append(self.expr(e, env, override_range))
                                    }
                                    MdNodeKind::Text { value, .. } => {
                                        v.append(self.markdown_text(
                                            &value.value(),
                                            self.range(value.span(), override_range),
                                        ));
                                    }
                                }
                            }
                            v
                        }
                        Err(err) => self.boundary(
                            &Expr::Macro(expr.clone()),
                            format!("Markdown template could not be analyzed: {err}"),
                            override_range,
                        ),
                    }
                }
            }
        };
        output.template = true;
        output
    }
    fn html(&mut self, nodes: &[Node], env: &Env, override_range: Option<&SourceRange>) -> Value {
        let mut v = Value::default();
        for node in nodes {
            match node {
                Node::Block(e) => v.append(self.expr(e, env, override_range)),
                Node::Fragment(nodes) => v.append(self.html(nodes, env, override_range)),
                Node::Element(e) => {
                    for attr in &e.attrs {
                        if let Attribute::Static { name, value } = attr {
                            if matches!(
                                name.as_markup_str(),
                                "alt"
                                    | "title"
                                    | "value"
                                    | "placeholder"
                                    | "href"
                                    | "src"
                                    | "aria-label"
                                    | "aria-description"
                                    | "aria-valuetext"
                            ) && !value.value().is_empty()
                            {
                                v.append(self.literal(
                                    &value.value(),
                                    self.range(value.span(), override_range),
                                ));
                            }
                        }
                        if let Attribute::Dynamic { name, value } = attr {
                            if matches!(
                                name.as_markup_str(),
                                "alt"
                                    | "title"
                                    | "value"
                                    | "placeholder"
                                    | "href"
                                    | "src"
                                    | "aria-label"
                                    | "aria-description"
                                    | "aria-valuetext"
                            ) {
                                v.append(self.expr(value, env, override_range));
                            }
                        }
                    }
                    v.append(self.html(&e.children, env, override_range));
                }
                Node::Text(t) => {
                    v.append(self.literal(&t.value(), self.range(t.span(), override_range)));
                }
                Node::Doctype(_) => {}
            }
        }
        v
    }
    fn template_range(&self, expr: &TemplateExpr) -> SourceRange {
        match &expr.location {
            ExprLocation::Rust(span) => span_range(self.path, *span),
            ExprLocation::Literal { span, range } => {
                let literal = span_range(self.path, *span);
                // Use authored token bytes, not decoded template bytes. This
                // keeps Unicode, cooked escapes and dedent locations truthful.
                let token = span.source_text().unwrap_or_default();
                let point = |offset: usize| {
                    let prefix = token.get(..offset).unwrap_or("");
                    let newline = prefix.chars().filter(|c| *c == '\n').count();
                    if newline == 0 {
                        (literal.line, literal.column + prefix.chars().count())
                    } else {
                        (
                            literal.line + newline,
                            prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
                        )
                    }
                };
                if token.is_empty() {
                    return literal;
                }
                let (line, column) = point(range.start);
                let (end_line, end_column) = point(range.end);
                SourceRange {
                    path: self.path.into(),
                    line,
                    column,
                    end_line,
                    end_column,
                }
            }
            ExprLocation::Decoded(_) => span_range(self.path, expr.expr.span()),
        }
    }
    fn md(&mut self, nodes: &[TplNode], env: &Env, fallback: &SourceRange) -> Value {
        let mut result = Value::default();
        for node in nodes {
            match node {
                TplNode::Text(text) => {
                    result.append(self.markdown_text(text, fallback.clone()));
                }
                TplNode::Expr(e) => {
                    let range = self.template_range(e);
                    result.append(self.expr(&e.expr, env, Some(&range)));
                }
                TplNode::If { branches } => {
                    let mut previous = Vec::new();
                    for branch in branches {
                        let mut guards = previous.clone();
                        if let Some(cond) = &branch.cond {
                            let range = self.template_range(cond);
                            guards.push(self.predicate(&cond.expr, env, false, Some(&range)));
                            previous.push(self.predicate(&cond.expr, env, true, Some(&range)));
                        }
                        result.append(self.md(&branch.body, env, fallback).guarded(&guards));
                    }
                }
                TplNode::For { pat, iter, body } => {
                    let range = self.template_range(iter);
                    let value = self.expr(&iter.expr, env, Some(&range));
                    if let Some(symbol) = value.symbol {
                        let mut child = env.clone();
                        let element = format!("{symbol}[]");
                        bind(
                            pat,
                            Value {
                                symbol: Some(element.clone()),
                                deps: BTreeSet::from([element]),
                                ..Value::default()
                            },
                            &mut child,
                        );
                        result.append(self.md(body, &child, fallback).guarded(&[Presence {
                            expression: format!("for {symbol}"),
                            range,
                        }]));
                    } else {
                        result.boundaries.push(Boundary {
                            may_hide_content: true,
                            message: "Markdown iteration source cannot be resolved".into(),
                            range,
                        });
                    }
                }
            }
        }
        result
    }
    fn marker(&mut self, mac: &syn::Macro, env: &Env, guards: &[Presence]) {
        let range = span_range(self.path, mac.span());
        let marker = match syn::parse2::<OmitContent>(mac.tokens.clone()) {
            Ok(m) => m,
            Err(err) => {
                self.omissions.push(Omission {
                    selector: OmissionKey::Unknown(err.to_string()),
                    from: None,
                    range,
                    presence: guards.into(),
                });
                return;
            }
        };
        let selector = match marker.selector {
            OmitSelector::Value(e) => {
                let value = self.expr(&e, env, None);
                if !value.boundaries.is_empty() {
                    OmissionKey::Unknown(format!("unresolved omitted value `{}`", syntax(&e)))
                } else if value
                    .parts
                    .iter()
                    .any(|p| matches!(p.key, ContentKey::Child { .. } | ContentKey::Role(_)))
                {
                    OmissionKey::Contributions(value.parts.iter().map(|p| p.key.clone()).collect())
                } else {
                    OmissionKey::Values(value.deps)
                }
            }
            OmitSelector::Child(ty) => {
                if let syn::Type::Path(path) = &ty {
                    let generic_arguments = path.path.segments.iter().any(|segment| match &segment
                        .arguments
                    {
                        syn::PathArguments::AngleBracketed(args) => args
                            .args
                            .iter()
                            .any(|arg| !matches!(arg, syn::GenericArgument::Lifetime(_))),
                        syn::PathArguments::Parenthesized(_) => true,
                        syn::PathArguments::None => false,
                    });
                    if generic_arguments {
                        OmissionKey::Unknown("the child's concrete generic instantiation cannot be resolved; omit an existing child local or give it a content_part! name".into())
                    } else {
                        self.resolver
                        .known_type(&path.path)
                        .map(|ty| {
                            if self.resolver.generic_children.contains(&ty) {
                                OmissionKey::Unknown("the child's concrete generic instantiation cannot be resolved; omit an existing child local or give it a content_part! name".into())
                            } else { OmissionKey::Child(ty) }
                        })
                        .unwrap_or_else(|| {
                            OmissionKey::Unknown(format!("unresolved child `{}`", syntax(&ty)))
                        })
                    }
                } else {
                    OmissionKey::Unknown(format!("unresolved child type `{}`", syntax(&ty)))
                }
            }
            OmitSelector::Role(r) => OmissionKey::Role(r.value()),
        };
        self.omissions.push(Omission {
            selector,
            from: marker.from.map(|s| s.value()),
            range,
            presence: guards.into(),
        });
    }
    fn statements(&mut self, block: &syn::Block, env: &Env, guards: &[Presence]) -> Value {
        let mut env = env.clone();
        let mut result = Value::default();
        for stmt in &block.stmts {
            match stmt {
                Stmt::Local(local) => {
                    if let Some(init) = &local.init {
                        result.append(self.sinks(&init.expr, &mut env, guards));
                    }
                    self.local(local, &mut env, None);
                }
                Stmt::Macro(m) if self.resolver.path(&m.mac.path) == "proa_macros::omit" => {
                    self.marker(&m.mac, &env, guards)
                }
                Stmt::Macro(mac)
                    if self.resolver.path(&mac.mac.path) == "proa_macros::content_part" =>
                {
                    result.append(self.sinks(
                        &Expr::Macro(syn::ExprMacro {
                            attrs: mac.attrs.clone(),
                            mac: mac.mac.clone(),
                        }),
                        &mut env,
                        guards,
                    ))
                }
                Stmt::Expr(e, _) => result.append(self.sinks(e, &mut env, guards)),
                Stmt::Item(_) => {}
                Stmt::Macro(m) => {
                    result.boundaries.push(Boundary {
                        may_hide_content: true,
                        message: format!(
                            "statement macro `{}` may affect rendered output",
                            syntax(&m.mac.path)
                        ),
                        range: span_range(self.path, m.span()),
                    });
                }
            }
        }
        result
    }
    fn record_bindings(&mut self, env: &Env, expr: &Expr) {
        self.sink_range
            .get_or_insert_with(|| span_range(self.path, expr.span()));
        let current: BTreeMap<_, _> = env
            .iter()
            .filter(|(name, _)| name.as_str() != "self" && *name != &self.context)
            .map(|(name, value)| {
                (
                    name.clone(),
                    value
                        .parts
                        .iter()
                        .map(|p| p.key.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        if self.bindings_initialized {
            self.bindings
                .retain(|name, keys| current.get(name) == Some(keys));
        } else {
            self.bindings = current;
            self.bindings_initialized = true;
        }
    }
    fn output_arg(&self, expr: &Expr, env: &Env) -> bool {
        bare(expr).is_some_and(|name| {
            env.get(&name).and_then(|v| v.symbol.as_deref()) == Some("@context")
        })
    }
    fn callback_arg(&self, expr: &Expr, env: &Env) -> bool {
        match expr {
            Expr::Reference(reference) => self.callback_arg(&reference.expr, env),
            Expr::Paren(paren) => self.callback_arg(&paren.expr, env),
            Expr::Group(group) => self.callback_arg(&group.expr, env),
            Expr::Closure(_) | Expr::Async(_) => true,
            Expr::Path(_) => bare(expr)
                .and_then(|name| env.get(&name))
                .is_some_and(|value| value.callback),
            _ => false,
        }
    }
    fn metadata_receiver(&self, expr: &Expr, env: &Env) -> bool {
        match expr {
            Expr::Paren(p) => self.metadata_receiver(&p.expr, env),
            Expr::Reference(r) => self.metadata_receiver(&r.expr, env),
            Expr::Path(_) => bare(expr)
                .and_then(|name| env.get(&name))
                .is_some_and(|value| value.metadata),
            Expr::MethodCall(m) if m.args.is_empty() => {
                let name = m.method.to_string();
                (self.proa_context
                    && self.output_arg(&m.receiver, env)
                    && matches!(
                        name.as_str(),
                        "request"
                            | "request_mut"
                            | "response"
                            | "response_mut"
                            | "head"
                            | "head_mut"
                            | "cache_trace"
                            | "cache_trace_mut"
                            | "telemetry"
                            | "telemetry_mut"
                            | "locals"
                            | "locals_mut"
                    ))
                    || (self.metadata_receiver(&m.receiver, env)
                        && matches!(
                            name.as_str(),
                            "headers"
                                | "headers_mut"
                                | "locals"
                                | "locals_mut"
                                | "clone"
                                | "as_ref"
                                | "as_deref"
                        ))
            }
            _ => false,
        }
    }
    fn context_preparation(&self, method: &syn::ExprMethodCall, env: &Env) -> bool {
        self.proa_context
            && self.output_arg(&method.receiver, env)
            && matches!(
                method.method.to_string().as_str(),
                "loader"
                    | "render_mode"
                    | "set_render_mode"
                    | "with_render_mode"
                    | "request"
                    | "request_mut"
                    | "set_request"
                    | "with_request"
                    | "response"
                    | "response_mut"
                    | "set_response"
                    | "with_response"
                    | "head"
                    | "head_mut"
                    | "set_head"
                    | "with_head"
                    | "cache_trace"
                    | "cache_trace_mut"
                    | "set_cache_trace"
                    | "with_cache_trace"
                    | "telemetry"
                    | "telemetry_mut"
                    | "set_telemetry"
                    | "with_telemetry"
                    | "locals"
                    | "locals_mut"
                    | "set_locals"
                    | "with_locals"
                    | "csp_nonce"
                    | "set_csp_nonce"
                    | "with_csp_nonce"
                    | "request_location"
                    | "set_loader_dyn"
                    | "with_loader_dyn"
                    | "load"
                    | "load_with"
                    | "load_request"
                    | "load_bytes"
                    | "load_bytes_request"
                    | "load_json"
                    | "load_json_with"
                    | "load_json_request"
                    | "load_typed"
                    | "load_typed_request"
                    | "md_set_inline"
                    | "md_push_heading_offset"
                    | "md_pop_heading_offset"
                    | "md_blank_line"
                    | "md_block_boundary"
                    | "md_line_boundary"
                    | "md_hr"
            )
    }
    fn sinks(&mut self, expr: &Expr, env: &mut Env, guards: &[Presence]) -> Value {
        match expr {
            Expr::MethodCall(m)
                if self.target.methods.contains(&m.method.to_string())
                    && m.args.len() == 1
                    && self.output_arg(&m.args[0], env) =>
            {
                self.record_bindings(env, &m.receiver);
                self.expr(&m.receiver, env, None).guarded(guards)
            }
            Expr::MethodCall(method)
                if self.target.methods.contains(&method.method.to_string())
                    && method.args.len() == 1 =>
            {
                self.boundary(
                    expr,
                    "render call's output context provenance could not be resolved",
                    None,
                )
            }
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    if self
                        .target
                        .future_sinks
                        .contains(&self.resolver.path(&p.path))
                        && c.args.len() == 1
                    {
                        if let Expr::Async(future) = &c.args[0] {
                            return self.statements(&future.block, env, guards);
                        }
                        return self.boundary(expr,"render outcome's future is opaque; expose its render path or name the content",None);
                    }
                    let mut path = p.path.clone();
                    let method = path
                        .segments
                        .pop()
                        .map(|s| s.into_value().ident.to_string());
                    let trait_name = self.resolver.path(&path);
                    if method.is_some_and(|m| self.target.methods.contains(&m))
                        && self.target.traits.contains(&trait_name)
                        && c.args.len() == 2
                        && self.output_arg(&c.args[1], env)
                    {
                        self.record_bindings(env, &c.args[0]);
                        return self.expr(&c.args[0], env, None).guarded(guards);
                    }
                }
                if c.args.iter().any(|a| self.output_arg(a, env)) {
                    return self.boundary(
                        expr,
                        "a helper receives the output context; its writes are not analyzed",
                        None,
                    );
                }
                if !matches!(c.func.as_ref(), Expr::Path(_))
                    || bare(&c.func).is_some_and(|name| env.contains_key(&name))
                {
                    return self.boundary(
                        expr,
                        "a dynamic callable may write through captured context; its invocation is not analyzed",
                        None,
                    );
                }
                if c.args.iter().any(|a| self.callback_arg(a, env)) {
                    return self.boundary(
                        expr,
                        "helper receives a future or closure whose output reachability is unknown",
                        None,
                    );
                }
                let mut v = Value::default();
                for arg in &c.args {
                    v.append(self.sinks(arg, env, guards));
                }
                v
            }
            Expr::Try(t) => self.sinks(&t.expr, env, guards),
            Expr::Await(a) => self.sinks(&a.base, env, guards),
            Expr::Paren(p) => self.sinks(&p.expr, env, guards),
            Expr::Group(g) => self.sinks(&g.expr, env, guards),
            Expr::Reference(reference) => self.sinks(&reference.expr, env, guards),
            Expr::Block(b) => self.statements(&b.block, env, guards),
            Expr::If(i) => {
                let mut yes = guards.to_vec();
                yes.push(self.predicate(&i.cond, env, false, None));
                let mut branch_env = env.clone();
                if let Expr::Let(binding) = i.cond.as_ref() {
                    self.bind_condition(binding, env, &mut branch_env, None);
                }
                let mut v = self.statements(&i.then_branch, &branch_env, &yes);
                if let Some((_, other)) = &i.else_branch {
                    let mut no = guards.to_vec();
                    no.push(self.predicate(&i.cond, env, true, None));
                    v.append(self.sinks(other, env, &no));
                }
                v
            }
            Expr::ForLoop(f) => {
                let iterator = self.expr(&f.expr, env, None);
                if let Some(symbol) = iterator.symbol {
                    let mut child = env.clone();
                    let element = format!("{symbol}[]");
                    bind(
                        &f.pat,
                        Value {
                            symbol: Some(element.clone()),
                            deps: BTreeSet::from([element]),
                            ..Value::default()
                        },
                        &mut child,
                    );
                    let mut presence = guards.to_vec();
                    presence.push(Presence {
                        expression: format!("for {symbol}"),
                        range: span_range(self.path, f.expr.span()),
                    });
                    self.statements(&f.body, &child, &presence)
                } else {
                    self.boundary(
                        expr,
                        "imperative render loop has unresolved element provenance",
                        None,
                    )
                }
            }
            Expr::Return(r) => {
                let mut v = r
                    .expr
                    .as_ref()
                    .map(|e| self.sinks(e, env, guards))
                    .unwrap_or_default();
                v.boundaries.push(Boundary {
                    may_hide_content: true,
                    message:
                        "early return changes output reachability; coverage is not established"
                            .into(),
                    range: span_range(self.path, r.span()),
                });
                v
            }
            Expr::Break(_) | Expr::Continue(_) => {
                self.boundary(expr, "loop exit changes rendered element coverage", None)
            }
            Expr::Binary(binary)
                if matches!(
                    binary.op,
                    syn::BinOp::AddAssign(_)
                        | syn::BinOp::SubAssign(_)
                        | syn::BinOp::MulAssign(_)
                        | syn::BinOp::DivAssign(_)
                        | syn::BinOp::RemAssign(_)
                        | syn::BinOp::BitXorAssign(_)
                        | syn::BinOp::BitAndAssign(_)
                        | syn::BinOp::BitOrAssign(_)
                        | syn::BinOp::ShlAssign(_)
                        | syn::BinOp::ShrAssign(_)
                ) =>
            {
                if let Some(name) = bare(&binary.left) {
                    env.insert(
                        name,
                        self.boundary(expr, "compound assignment changes local provenance", None),
                    );
                }
                self.boundary(expr, "compound assignment changes input provenance", None)
            }
            Expr::Assign(a) => {
                if let Some(name) = bare(&a.left) {
                    env.insert(
                        name,
                        self.boundary(
                            &a.right,
                            "assigned local requires control-flow analysis",
                            None,
                        ),
                    );
                } else {
                    return self.boundary(
                        expr,
                        "assignment may change shared input provenance",
                        None,
                    );
                }
                Value::default()
            }
            Expr::Macro(mac)
                if self.resolver.path(&mac.mac.path) == "proa_macros::content_part" =>
            {
                match syn::parse2::<ContentPart>(mac.mac.tokens.clone()) {
                    Ok(part) => {
                        let mut value = self.sinks(&part.value, env, guards);
                        if value.parts.is_empty() && value.boundaries.is_empty() {
                            return value;
                        }
                        if value.parts.is_empty() {
                            value = self
                                .part(ContentKey::Role(part.role.value()), expr, None)
                                .guarded(guards);
                        } else {
                            for contribution in &mut value.parts {
                                contribution.key = ContentKey::Role(part.role.value());
                            }
                        }
                        value.boundaries.clear();
                        value
                    }
                    Err(error) => self.boundary(expr, error.to_string(), None),
                }
            }
            Expr::Macro(m) if self.resolver.path(&m.mac.path) == "proa_macros::omit" => {
                self.marker(&m.mac, env, guards);
                Value::default()
            }
            Expr::MethodCall(m)
                if self.proa_context
                    && self.output_arg(&m.receiver, env)
                    && m.method == "write_md_inline_code"
                    && m.args.len() == 1 =>
            {
                self.expr(&m.args[0], env, None).guarded(guards)
            }
            Expr::MethodCall(m)
                if self.proa_context
                    && self.output_arg(&m.receiver, env)
                    && m.method == "md_heading_with"
                    && m.args.len() == 2 =>
            {
                let mut value = self.sinks(&m.args[0], env, guards);
                if let Expr::Closure(callback) = &m.args[1] {
                    if callback.inputs.len() == 1 {
                        let mut inner = env.clone();
                        bind(
                            &callback.inputs[0],
                            Value {
                                symbol: Some("@context".into()),
                                ..Value::default()
                            },
                            &mut inner,
                        );
                        value.append(self.sinks(&callback.body, &mut inner, guards));
                        return value;
                    }
                }
                value.append(self.boundary(
                    expr,
                    "Markdown heading callback cannot be resolved",
                    None,
                ));
                value
            }
            Expr::MethodCall(m) => {
                // Context/loader usage is preparation, not an obligation.
                // This applies only to proven WebContext APIs and metadata
                // handles, never arbitrary methods with familiar spellings.
                if (self.context_preparation(m, env) || self.metadata_receiver(&m.receiver, env))
                    && !m.args.iter().any(|arg| self.output_arg(arg, env))
                {
                    let mut value = self.sinks(&m.receiver, env, guards);
                    for arg in &m.args {
                        if self.callback_arg(arg, env) {
                            value.append(self.boundary(arg,
                                "preparation receives a callback whose output reachability is unknown", None));
                        } else {
                            value.append(self.sinks(arg, env, guards));
                        }
                    }
                    return value;
                }
                if matches!(
                    m.method.to_string().as_str(),
                    "push"
                        | "pop"
                        | "insert"
                        | "remove"
                        | "clear"
                        | "set"
                        | "update"
                        | "extend"
                        | "retain"
                        | "sort"
                        | "reverse"
                ) {
                    if let Some(name) = bare(&m.receiver) {
                        env.insert(
                            name,
                            self.boundary(expr, "mutation changes local provenance", None),
                        );
                    }
                    return self.boundary(
                        expr,
                        "mutation of render inputs is outside the supported analysis",
                        None,
                    );
                }
                if self.output_arg(&m.receiver, env)
                    || m.args.iter().any(|a| self.output_arg(a, env))
                {
                    self.boundary(expr,"direct context writes or unregistered output methods need a named content mapping",None)
                } else {
                    let mut value = self.sinks(&m.receiver, env, guards);
                    for arg in &m.args {
                        if self.callback_arg(arg, env) {
                            value.append(self.boundary(
                                arg,
                                "method receives a callback whose output reachability is unknown",
                                None,
                            ));
                        } else {
                            value.append(self.sinks(arg, env, guards));
                        }
                    }
                    value
                }
            }
            Expr::Match(_) | Expr::While(_) | Expr::Loop(_) | Expr::Unsafe(_) => self.boundary(
                expr,
                "render control flow is outside the supported subset",
                None,
            ),
            _ => Value::default(),
        }
    }
}
// Assignment does not need to be rebuilt merely to record its RHS boundary.
fn expr_from_assign(assign: &syn::ExprAssign) -> &Expr {
    &assign.right
}

pub(super) fn summarize(
    path: &Path,
    source: &str,
    item: &ItemImpl,
    method: &ImplItemFn,
    component: &str,
    target: &TargetDescriptor,
    resolver: &Resolver,
    skip: bool,
) -> ViewSummary {
    let context_input = method
        .sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            syn::FnArg::Typed(t) => match t.pat.as_ref() {
                Pat::Ident(i) => Some((i.ident.to_string(), t.ty.as_ref())),
                _ => None,
            },
            _ => None,
        })
        .last();
    let proa_context = context_input.as_ref().is_some_and(|(_, ty)| {
        let mut ty = *ty;
        while let syn::Type::Reference(reference) = ty {
            ty = &reference.elem;
        }
        matches!(ty, syn::Type::Path(path) if path.qself.is_none()
            && resolver.path(&path.path) == "proa_core::WebContext")
    });
    let context = context_input
        .map(|(name, _)| name)
        .unwrap_or_else(|| "cx".into());
    let mut self_type = item.self_ty.as_ref();
    while let syn::Type::Reference(reference) = self_type {
        self_type = &reference.elem;
    }
    let optional_inputs = if let syn::Type::Path(ty) = self_type {
        resolver
            .known_type(&ty.path)
            .and_then(|name| resolver.optional_fields.get(&name))
            .into_iter()
            .flatten()
            .map(|field| format!("self.{field}"))
            .collect()
    } else {
        BTreeSet::new()
    };
    let mut analyzer = Analyzer {
        path,
        target,
        resolver,
        context,
        proa_context,
        optional_inputs,
        omissions: Vec::new(),
        bindings: BTreeMap::new(),
        bindings_initialized: false,
        sink_range: None,
    };
    let mut env = Env::new();
    env.insert(
        "self".into(),
        Value {
            symbol: Some("self".into()),
            deps: BTreeSet::from(["self".into()]),
            ..Value::default()
        },
    );
    env.insert(
        analyzer.context.clone(),
        Value {
            symbol: Some("@context".into()),
            ..Value::default()
        },
    );
    let output = analyzer.statements(&method.block, &env, &[]);
    let mut entry = span_range(path, method.block.brace_token.span.open());
    entry.column += 1;
    entry.end_column = entry.column;
    entry.end_line = entry.line;
    // Self values seen anywhere (including unused definitions) can supply
    // syntactically valid omission spellings for this same component type.
    let mut self_keys = output
        .parts
        .iter()
        .map(|c| c.key.clone())
        .collect::<Vec<_>>();
    for keys in analyzer.bindings.values() {
        self_keys.extend(keys.clone());
    }
    let mut ty = item.self_ty.as_ref();
    if let syn::Type::Reference(r) = ty {
        ty = &r.elem;
    }
    if let syn::Type::Path(path) = ty {
        if let Some(name) = resolver.known_type(&path.path) {
            if let Some(fields) = resolver.fields.get(&name) {
                self_keys.extend(
                    fields
                        .iter()
                        .map(|field| ContentKey::Value(format!("self.{field}"))),
                );
            }
        }
    }
    analyzer.bindings.insert("self".into(), self_keys);
    let _ = item;
    ViewSummary {
        component: component.into(),
        target: target.name.clone(),
        target_label: target.display_name.clone(),
        source: source.into(),
        entry,
        output: analyzer.sink_range,
        contributions: output.parts,
        boundaries: output.boundaries,
        omissions: analyzer.omissions,
        skip,
        bindings: analyzer.bindings,
        child_selectors: resolver.child_selectors.clone(),
    }
}
