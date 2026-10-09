// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn has_attr(attrs: &[SynAttribute], expected: &[&str]) -> bool {
    attrs.iter().any(|attr| {
        path_ends(attr.path(), expected)
            || attr
                .path()
                .segments
                .last()
                .is_some_and(|segment| expected.len() == 1 && segment.ident == expected[0])
    })
}

pub(crate) fn is_perf_suppressed(expr: &Expr) -> bool {
    matches!(expr, Expr::Call(call) if is_perf_suppressed_call(call))
}

pub(crate) fn is_perf_suppressed_call(call: &ExprCall) -> bool {
    if let Expr::Path(path) = call.func.as_ref() {
        path.path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "perf_ok")
    } else {
        false
    }
}

pub(crate) fn call_path_ends(call: &ExprCall, expected: &[&str]) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path_ends(&path.path, expected)
}

pub(crate) fn call_path_is_manual_escape(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };

    path.path
        .segments
        .iter()
        .any(|segment| segment.ident == "html_escape")
        || path_ends(&path.path, &["escape_text"])
        || path_ends(&path.path, &["escape_attr"])
        || path_ends(&path.path, &["encode_text"])
        || path_ends(&path.path, &["encode_double_quoted_attribute"])
}

pub(crate) fn call_path_is_raw_write(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };

    path_ends(&path.path, &["WriteBuf", "extend"])
        || path_ends(&path.path, &["WriteBuf", "extend_static"])
}

pub(crate) fn call_is_attr_stream_op(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };

    path_ends(&path.path, &["WriteBuf", "extend"])
        || path_ends(&path.path, &["WriteBuf", "extend_static"])
}

pub(crate) fn method_is_attr_stream_op(method: &ExprMethodCall) -> bool {
    if method.method == "render_attr_value" {
        return true;
    }
    matches!(
        method.method.to_string().as_str(),
        "extend" | "extend_static"
    ) && expr_is_output_buffer(&method.receiver)
}

pub(crate) fn attr_value_raw_segment_in_call(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path_ends(&path.path, &["WriteBuf", "extend"])
        && call.args.iter().any(expr_contains_as_bytes_call)
}

pub(crate) fn attr_value_raw_segment_in_method_call(method: &ExprMethodCall) -> bool {
    method.method == "extend"
        && expr_is_output_buffer(&method.receiver)
        && method.args.iter().any(expr_contains_as_bytes_call)
}

pub(crate) fn path_ends(path: &SynPath, expected: &[&str]) -> bool {
    path.segments.len() >= expected.len()
        && path
            .segments
            .iter()
            .skip(path.segments.len() - expected.len())
            .zip(expected)
            .all(|(seg, expected)| seg.ident == *expected)
}

pub(crate) fn path_last_ident(path: &SynPath) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// Idents that only appear when a template genuinely composes async work.
///
/// `to_async`/`to_md_async` are deliberately absent: they lift a *sync* child
/// into an async tree, so their presence is evidence the surrounding render
/// could be synchronous, not evidence that it must stay async.
pub(crate) const ASYNC_RENDER_MARKERS: &[&str] = &["await_component", "AwaitComponent", "Suspense"];

pub(crate) const MD_ASYNC_RENDER_MARKERS: &[&str] = &["await_md_component", "AwaitMdComponent"];

/// Scan raw macro tokens for an ident.
///
/// `syn::visit` stops at a macro boundary (`mac.tokens` is an opaque
/// `TokenStream`), so anything inside `html!`/`md!` is invisible to the normal
/// AST walk. Every async-detection predicate below has to descend manually or
/// it reports "no await" for a template whose slots await.
pub(crate) fn tokens_mention_ident(tokens: &proc_macro2::TokenStream, names: &[&str]) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Ident(ident) => names.iter().any(|name| ident == name),
        proc_macro2::TokenTree::Group(group) => tokens_mention_ident(&group.stream(), names),
        _ => false,
    })
}

/// `md!` template bodies are a single raw string literal, so slot expressions
/// never reach the token stream as idents. Peek inside literals for `.await`.
pub(crate) fn tokens_contain_await_in_literal(tokens: &proc_macro2::TokenStream) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Literal(literal) => literal.to_string().contains(".await"),
        proc_macro2::TokenTree::Group(group) => tokens_contain_await_in_literal(&group.stream()),
        _ => false,
    })
}

pub(crate) fn block_contains_await(block: &Block) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_await(&mut self, _node: &'ast syn::ExprAwait) {
            self.found = true;
        }

        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            if tokens_mention_ident(&node.tokens, &["await"])
                || tokens_contain_await_in_literal(&node.tokens)
            {
                self.found = true;
            }
            visit::visit_macro(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_block(block);
    finder.found
}

pub(crate) fn block_mentions_any_ident(block: &Block, names: &[&str]) -> bool {
    struct Finder<'a> {
        names: &'a [&'a str],
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder<'_> {
        fn visit_ident(&mut self, node: &'ast proc_macro2::Ident) {
            if self.names.iter().any(|name| node == name) {
                self.found = true;
            }
        }

        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            if tokens_mention_ident(&node.tokens, self.names) {
                self.found = true;
            }
            visit::visit_macro(self, node);
        }
    }

    let mut finder = Finder {
        names,
        found: false,
    };
    finder.visit_block(block);
    finder.found
}

pub(crate) fn block_contains_async_render_marker(block: &Block) -> bool {
    block_mentions_any_ident(block, ASYNC_RENDER_MARKERS)
}

pub(crate) fn block_contains_md_async_render_marker(block: &Block) -> bool {
    block_mentions_any_ident(block, MD_ASYNC_RENDER_MARKERS)
}

/// A bare unit-struct component child: `{StreamingPanels}`.
///
/// This is deliberately separate from [`expr_constructs_uppercase_component`].
/// `render/sync-child-in-async` already *presumes* that a call or struct-literal
/// child (`{Badge { label: "ok" }}`) is a sync component that merely forgot its
/// `to_async(..)` wrapper. A bare unit-struct path carries no such signal, and
/// in practice names a deferred/streaming component, so it is the one shape
/// that must block a "this could be synchronous" suggestion.
///
/// `SCREAMING_SNAKE_CASE` is excluded: `{MAX_ITEMS}` interpolates a const, and
/// treating it as an opaque child would silence both rules on any template that
/// uses one. That mis-sorts an all-caps component name such as `UI`, which is
/// the recoverable direction (the rule fires and can be suppressed) rather than
/// silent under-reporting.
pub(crate) fn expr_is_bare_uppercase_path(expr: &Expr) -> bool {
    match expr {
        Expr::Path(path) => {
            path.path.segments.len() == 1
                && path_last_ident(&path.path).is_some_and(|name| {
                    ident_starts_uppercase(&name) && name.chars().any(char::is_lowercase)
                })
        }
        Expr::Group(group) => expr_is_bare_uppercase_path(&group.expr),
        Expr::Paren(paren) => expr_is_bare_uppercase_path(&paren.expr),
        _ => false,
    }
}

/// Any component-like child: a call, a struct literal, or a bare path.
pub(crate) fn expr_references_uppercase_component(expr: &Expr) -> bool {
    expr_is_bare_uppercase_path(expr) || expr_constructs_uppercase_component(expr)
}

/// True when the body hands back a `RenderOutcome` produced somewhere else.
///
/// `RenderOutcome::Pending(<call>)` forwards an erased future (`AsyncHtml`,
/// `AsyncMd`), and a free call to another `render*` function forwards whatever
/// that function decides (`T::render_at(..)`). Neither body reveals whether the
/// delegate awaits, so neither can be advised to become synchronous. An inline
/// `RenderOutcome::pending(async move { .. })` is *not* forwarding: its body is
/// fully visible and is exactly what this rule is meant to catch.
///
/// Only free calls (`Expr::Call`) count. The method form `html! { .. }.render(cx)`
/// is an `ExprMethodCall` and stays reportable, which is the whole point.
pub(crate) fn block_forwards_foreign_render_outcome(block: &Block) -> bool {
    struct Finder {
        found: bool,
    }

    fn is_render_outcome_pending(path: &SynPath) -> bool {
        path_last_ident(path).is_some_and(|name| name == "Pending" || name == "pending")
            && path
                .segments
                .iter()
                .any(|segment| segment.ident == "RenderOutcome")
    }

    fn arg_is_inline_async(expr: &Expr) -> bool {
        match expr {
            Expr::Async(_) => true,
            Expr::Group(group) => arg_is_inline_async(&group.expr),
            Expr::Paren(paren) => arg_is_inline_async(&paren.expr),
            _ => false,
        }
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Expr::Path(path) = node.func.as_ref() {
                let forwards_pending = is_render_outcome_pending(&path.path)
                    && node.args.len() == 1
                    && !arg_is_inline_async(&node.args[0]);
                let forwards_render_fn =
                    path_last_ident(&path.path).is_some_and(|name| name.starts_with("render"));
                if forwards_pending || forwards_render_fn {
                    self.found = true;
                    return;
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_block(block);
    finder.found
}

/// True when the body completes through `RenderOutcome::Done`.
///
/// Types such as `SyncToAsync`, `BoxedWebRenderSync`, and the scalar leaf
/// impls implement the async trait purely to bridge an existing sync impl into
/// an async template. They already implement the sync trait, so advising
/// "implement the sync trait instead" is wrong.
pub(crate) fn block_completes_render_outcome_done(block: &Block) -> bool {
    struct Finder {
        found: bool,
    }

    fn path_is_render_outcome_done(path: &SynPath) -> bool {
        path_last_ident(path).as_deref() == Some("Done")
            && path
                .segments
                .iter()
                .any(|segment| segment.ident == "RenderOutcome")
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
            if path_is_render_outcome_done(&node.path) {
                self.found = true;
            }
            visit::visit_expr_path(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_block(block);
    finder.found
}

/// True when any generic parameter is bound by an async render trait.
///
/// Such an impl cannot be lowered to the sync trait no matter what its body
/// looks like: the child is only known to be `WebRender`/`MdRender`, so it may
/// await. Without this guard every generic container component in the
/// workspace trips `render/async-unneeded`.
pub(crate) fn generics_bind_async_render(generics: &syn::Generics, trait_names: &[&str]) -> bool {
    fn any_bound_matches<'a>(
        bounds: impl Iterator<Item = &'a syn::TypeParamBound>,
        trait_names: &[&str],
    ) -> bool {
        bounds.into_iter().any(|bound| match bound {
            syn::TypeParamBound::Trait(trait_bound) => path_last_ident(&trait_bound.path)
                .is_some_and(|name| trait_names.contains(&name.as_str())),
            _ => false,
        })
    }

    let inline = generics.params.iter().any(|param| match param {
        syn::GenericParam::Type(type_param) => {
            any_bound_matches(type_param.bounds.iter(), trait_names)
        }
        _ => false,
    });
    if inline {
        return true;
    }

    generics.where_clause.as_ref().is_some_and(|clause| {
        clause.predicates.iter().any(|predicate| match predicate {
            syn::WherePredicate::Type(pred) => any_bound_matches(pred.bounds.iter(), trait_names),
            _ => false,
        })
    })
}

pub(crate) fn expr_looks_like_large_render_data(expr: &Expr) -> bool {
    expr_leaf_ident(expr).is_some_and(|ident| {
        let lower = ident.to_ascii_lowercase();
        matches!(
            lower.as_str(),
            "audit"
                | "audits"
                | "data"
                | "events"
                | "items"
                | "metrics"
                | "model"
                | "models"
                | "origin"
                | "requests"
                | "results"
                | "rows"
                | "snapshot"
                | "snapshots"
                | "view"
                | "views"
        ) || lower.ends_with("_data")
            || lower.ends_with("_rows")
            || lower.ends_with("_snapshots")
            || lower.ends_with("_requests")
            || lower.ends_with("_results")
    })
}

pub(crate) fn expr_leaf_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => expr_path_single_ident(expr).or_else(|| path_last_ident(&path.path)),
        Expr::Field(field) => match &field.member {
            syn::Member::Named(member) => Some(member.to_string()),
            syn::Member::Unnamed(_) => None,
        },
        Expr::MethodCall(method) => expr_leaf_ident(&method.receiver),
        Expr::Group(group) => expr_leaf_ident(&group.expr),
        Expr::Paren(paren) => expr_leaf_ident(&paren.expr),
        Expr::Reference(reference) => expr_leaf_ident(&reference.expr),
        _ => None,
    }
}

pub(crate) fn expr_is_async_render_wrapper(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => {
            let Expr::Path(path) = call.func.as_ref() else {
                return false;
            };
            path_last_ident(&path.path).is_some_and(|name| {
                matches!(name.as_str(), "to_async" | "await_component" | "perf_ok")
            })
        }
        Expr::Group(group) => expr_is_async_render_wrapper(&group.expr),
        Expr::Paren(paren) => expr_is_async_render_wrapper(&paren.expr),
        _ => false,
    }
}

pub(crate) fn expr_constructs_uppercase_component(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => {
            let Expr::Path(path) = call.func.as_ref() else {
                return false;
            };
            path_last_ident(&path.path).is_some_and(|name| ident_starts_uppercase(&name))
        }
        Expr::Struct(expr_struct) => {
            path_last_ident(&expr_struct.path).is_some_and(|name| ident_starts_uppercase(&name))
        }
        Expr::Group(group) => expr_constructs_uppercase_component(&group.expr),
        Expr::Paren(paren) => expr_constructs_uppercase_component(&paren.expr),
        _ => false,
    }
}

pub(crate) fn ident_starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(crate) fn is_route_handler_fn(item_fn: &syn::ItemFn) -> bool {
    item_fn.sig.asyncness.is_some()
        && (fn_name_looks_route(&item_fn.sig.ident.to_string())
            || attrs_look_route(&item_fn.attrs)
            || inputs_include_axum_extractor(&item_fn.sig.inputs))
}

pub(crate) fn is_public_hot_route_fn(item_fn: &syn::ItemFn) -> bool {
    matches!(item_fn.vis, syn::Visibility::Public(_))
        && (matches!(
            item_fn.sig.ident.to_string().as_str(),
            "home" | "index" | "root" | "landing" | "dashboard"
        ) || attrs_look_public_root_route(&item_fn.attrs))
}

pub(crate) fn fn_name_looks_route(name: &str) -> bool {
    name.contains("route")
        || name.contains("handler")
        || name.ends_with("_page")
        || matches!(name, "home" | "index" | "root" | "landing" | "dashboard")
}

pub(crate) fn attrs_look_route(attrs: &[SynAttribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().segments.last().is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "route"
            )
        })
    })
}

pub(crate) fn attrs_look_public_root_route(attrs: &[SynAttribute]) -> bool {
    attrs
        .iter()
        .map(ToTokens::to_token_stream)
        .map(|tokens| tokens.to_string())
        .any(|tokens| tokens.contains("\"/\"") || tokens.contains("( \"/\" )"))
}

pub(crate) fn inputs_include_axum_extractor(inputs: &Punctuated<syn::FnArg, Token![,]>) -> bool {
    inputs.iter().any(|input| match input {
        syn::FnArg::Typed(pat_type) => {
            let tokens = pat_type.to_token_stream().to_string();
            tokens.contains("Extension")
                || tokens.contains("State")
                || tokens.contains("Path")
                || tokens.contains("Query")
        }
        syn::FnArg::Receiver(_) => false,
    })
}

pub(crate) fn ident_is_db_handle(name: &str) -> bool {
    matches!(name, "db" | "database")
        || name.ends_with("_db")
        || name.ends_with("Database")
        || name.ends_with("Db")
}

pub(crate) fn method_name_is_db_load(name: &str) -> bool {
    name.starts_with("find")
        || name.starts_with("get")
        || name.starts_with("latest")
        || name.starts_with("list")
        || name.starts_with("load")
        || name.starts_with("query")
        || name.starts_with("select")
        || name.contains("snapshot")
}

pub(crate) fn attr_name_is(name: &MarkupName, expected: &str) -> bool {
    name.as_markup_str() == expected || *name == expected
}

pub(crate) fn static_attr_value(el: &Element, expected: &str) -> Option<String> {
    el.attrs.iter().find_map(|attr| match attr {
        Attribute::Static { name, value } if attr_name_is(name, expected) => Some(value.value()),
        _ => None,
    })
}

pub(crate) fn bool_attr_present(el: &Element, expected: &str) -> bool {
    el.attrs
        .iter()
        .any(|attr| matches!(attr, Attribute::Bool { name } if attr_name_is(name, expected)))
}

pub(crate) fn macro_body_has_attr_spacing(body: &str) -> bool {
    let Ok(tokens) = TokenStream::from_str(body) else {
        return false;
    };
    let tokens: Vec<_> = tokens.into_iter().collect();
    let source = crate::source::SourceMap::new(body);
    let mut in_tag = false;

    for (idx, token) in tokens.iter().enumerate() {
        let proc_macro2::TokenTree::Punct(punct) = token else {
            continue;
        };

        match punct.as_char() {
            '<' if !in_tag => {
                in_tag = true;
                if tokens
                    .get(idx + 1)
                    .is_some_and(|next| token_source_has_gap(&source, token, next))
                {
                    return true;
                }
            }
            '>' if in_tag => {
                if idx
                    .checked_sub(1)
                    .and_then(|previous| tokens.get(previous))
                    .is_some_and(|previous| token_source_has_gap(&source, previous, token))
                {
                    return true;
                }
                in_tag = false;
            }
            '=' | '-' | ':' if in_tag => {
                let gap_before = idx
                    .checked_sub(1)
                    .and_then(|previous| tokens.get(previous))
                    .is_some_and(|previous| token_source_has_gap(&source, previous, token));
                let gap_after = tokens
                    .get(idx + 1)
                    .is_some_and(|next| token_source_has_gap(&source, token, next));
                if gap_before || gap_after {
                    return true;
                }
            }
            '/' if in_tag => {
                let previous_is_open = idx
                    .checked_sub(1)
                    .and_then(|previous| tokens.get(previous))
                    .is_some_and(|previous| token_is_punct(previous, '<'));
                if previous_is_open
                    && idx
                        .checked_sub(1)
                        .and_then(|previous| tokens.get(previous))
                        .is_some_and(|previous| token_source_has_gap(&source, previous, token))
                {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

fn token_is_punct(token: &proc_macro2::TokenTree, expected: char) -> bool {
    matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == expected)
}

fn token_source_has_gap(
    source: &crate::source::SourceMap<'_>,
    left: &proc_macro2::TokenTree,
    right: &proc_macro2::TokenTree,
) -> bool {
    let left_end = left.span().end();
    let right_start = right.span().start();
    let Some(left_end) = source.byte_for_line_column(left_end.line, left_end.column) else {
        return false;
    };
    let Some(right_start) = source.byte_for_line_column(right_start.line, right_start.column)
    else {
        return false;
    };
    right_start > left_end
}

pub(crate) fn macro_body_has_noncanonical_self_closing(body: &str) -> bool {
    body.contains("/>") && !body.contains(" />")
}

pub(crate) fn attr_name(attr: &Attribute) -> &MarkupName {
    match attr {
        Attribute::Static { name, .. }
        | Attribute::Dynamic { name, .. }
        | Attribute::Bool { name } => name,
    }
}

pub(crate) fn invalid_parent_child_pair(parent: &str, child: &str) -> bool {
    match parent {
        "p" => matches!(
            child,
            "div" | "section" | "article" | "main" | "header" | "footer" | "nav"
        ),
        "ul" | "ol" => !matches!(child, "li" | "script" | "template"),
        "table" => !matches!(
            child,
            "caption" | "colgroup" | "thead" | "tbody" | "tfoot" | "tr" | "script" | "template"
        ),
        "tr" => !matches!(child, "th" | "td" | "script" | "template"),
        _ => false,
    }
}

pub(crate) fn data_or_aria_name_is_valid(raw: &str) -> bool {
    let Some((prefix, suffix)) = raw.split_once('-') else {
        return false;
    };
    matches!(prefix, "data" | "aria")
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !suffix.contains("--")
        && !suffix.starts_with('-')
        && !suffix.ends_with('-')
}

pub(crate) fn event_attr_name_is_suspicious(raw: &str) -> bool {
    raw.starts_with("on_")
        || raw.starts_with("on-")
        || raw
            .strip_prefix("on")
            .is_some_and(|tail| tail.chars().next().is_some_and(char::is_uppercase))
}

pub(crate) fn expr_is_output_buffer(expr: &Expr) -> bool {
    match expr {
        Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(member) if member == "out")
        }
        Expr::Group(group) => expr_is_output_buffer(&group.expr),
        Expr::Paren(paren) => expr_is_output_buffer(&paren.expr),
        Expr::Reference(reference) => expr_is_output_buffer(&reference.expr),
        _ => false,
    }
}

pub(crate) fn expr_contains_as_bytes_call(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == "as_bytes" {
                self.found = true;
                return;
            }
            visit::visit_expr_method_call(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

pub(crate) fn type_path_last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Type::Group(group) => type_path_last_ident(&group.elem),
        Type::Paren(paren) => type_path_last_ident(&paren.elem),
        Type::Reference(reference) => type_path_last_ident(&reference.elem),
        _ => None,
    }
}

pub(crate) fn type_is_boxed_render_child(ty: &Type) -> bool {
    if type_is_render_trait_object(ty) {
        return true;
    }

    let Type::Path(type_path) = ty else {
        return false;
    };
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    if matches!(
        segment.ident.to_string().as_str(),
        "BoxedWebRender" | "BoxedWebRenderSync"
    ) {
        return true;
    }
    if segment.ident != "Box" {
        return false;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };

    args.args.iter().any(
        |arg| matches!(arg, GenericArgument::Type(inner) if type_is_render_trait_object(inner)),
    )
}

pub(crate) fn type_is_render_trait_object(ty: &Type) -> bool {
    let Type::TraitObject(object) = ty else {
        return false;
    };

    object.bounds.iter().any(|bound| {
        matches!(
            bound,
            syn::TypeParamBound::Trait(trait_bound) if is_render_trait(&trait_bound.path)
        )
    })
}

pub(crate) fn type_contains_owned_string(ty: &Type) -> bool {
    type_path_last_ident(ty).as_deref() == Some("String")
        || type_generic_args(ty).any(type_contains_owned_string)
}

pub(crate) fn type_contains_vec(ty: &Type) -> bool {
    type_path_last_ident(ty).as_deref() == Some("Vec")
        || type_generic_args(ty).any(type_contains_vec)
}

pub(crate) fn type_generic_args(ty: &Type) -> impl Iterator<Item = &Type> {
    let args = match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .and_then(|segment| match &segment.arguments {
                PathArguments::AngleBracketed(args) => Some(&args.args),
                _ => None,
            }),
        _ => None,
    };

    args.into_iter()
        .flat_map(|args| args.iter())
        .filter_map(|arg| {
            if let GenericArgument::Type(ty) = arg {
                Some(ty)
            } else {
                None
            }
        })
}

pub(crate) fn field_name_is_render_text(name: &str) -> bool {
    matches!(
        name,
        "child" | "children" | "content" | "description" | "heading" | "label" | "text" | "title"
    )
}

pub(crate) fn field_name_is_children(name: &str) -> bool {
    matches!(name, "child" | "children")
}

pub(crate) fn helper_name_is_string_display(name: &str) -> bool {
    name.starts_with("fmt_")
        || name.starts_with("format_")
        || name.starts_with("human_")
        || name.starts_with("short_")
        || name.ends_with("_display")
        || name.ends_with("_label")
        || name.ends_with("_text")
        || name.ends_with("_title")
}

pub(crate) fn helper_name_is_manual_escape(name: &str) -> bool {
    name == "esc"
        || name == "escape"
        || name == "sanitize"
        || name == "sanitize_html"
        || name == "html_escape"
        || name == "escape_html"
        || name.contains("escape")
        || name.contains("escaped")
        || name.contains("sanitize")
        || name.contains("sanitized")
        || name.contains("html_escape")
}

pub(crate) fn helper_name_is_html_string_builder(name: &str) -> bool {
    name.contains("escape")
        || name.contains("html")
        || name.contains("markup")
        || name.contains("raw_html")
}

pub(crate) fn block_contains_html_literal(block: &Block) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr(&mut self, node: &'ast Expr) {
            if expr_is_html_literal(node) {
                self.found = true;
                return;
            }
            visit::visit_expr(self, node);
        }

        fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
            if format_macro_template_looks_like_html(node) {
                self.found = true;
                return;
            }
            visit::visit_expr_macro(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_block(block);
    finder.found
}

pub(crate) fn block_has_manual_escape_helper_shape(block: &Block) -> bool {
    struct Finder {
        has_builder: bool,
        has_entity_literal: bool,
        has_escape_call: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if call_path_ends(node, &["String", "new"])
                || call_path_ends(node, &["String", "with_capacity"])
            {
                self.has_builder = true;
            }
            if call_path_is_manual_escape(node) {
                self.has_escape_call = true;
            }
            visit::visit_expr_call(self, node);
        }

        fn visit_lit(&mut self, node: &'ast Lit) {
            if lit_is_html_entity(node) {
                self.has_entity_literal = true;
            }
            visit::visit_lit(self, node);
        }
    }

    let mut finder = Finder {
        has_builder: false,
        has_entity_literal: false,
        has_escape_call: false,
    };
    finder.visit_block(block);

    finder.has_escape_call || (finder.has_builder && finder.has_entity_literal)
}

pub(crate) fn lit_is_html_entity(lit: &Lit) -> bool {
    match lit {
        Lit::Str(lit) => string_is_html_entity(&lit.value()),
        Lit::ByteStr(lit) => std::str::from_utf8(&lit.value()).is_ok_and(string_is_html_entity),
        _ => false,
    }
}

pub(crate) fn string_is_html_entity(value: &str) -> bool {
    matches!(
        value,
        "&amp;" | "&lt;" | "&gt;" | "&quot;" | "&#39;" | "&#x27;" | "&#x2F;" | "&#47;"
    )
}

pub(crate) fn expr_bool_literal(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Bool(value) => Some(value.value),
            _ => None,
        },
        Expr::Group(group) => expr_bool_literal(&group.expr),
        Expr::Paren(paren) => expr_bool_literal(&paren.expr),
        _ => None,
    }
}

pub(crate) fn block_returns_bool_literal(block: &Block, expected: bool) -> bool {
    block_single_expr(block).is_some_and(|expr| expr_bool_literal(expr) == Some(expected))
}

pub(crate) fn block_returns_ok_unit(block: &Block) -> bool {
    block_single_expr(block).is_some_and(expr_is_ok_unit)
}

pub(crate) fn block_single_expr(block: &Block) -> Option<&Expr> {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(expr, _)] => Some(expr),
        _ => None,
    }
}

pub(crate) fn expr_is_ok_unit(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => {
            let Expr::Path(func) = call.func.as_ref() else {
                return false;
            };
            path_last_ident(&func.path).as_deref() == Some("Ok")
                && matches!(call.args.first(), Some(Expr::Tuple(tuple)) if tuple.elems.is_empty())
        }
        Expr::Return(ret) => ret.expr.as_deref().is_some_and(expr_is_ok_unit),
        Expr::Group(group) => expr_is_ok_unit(&group.expr),
        Expr::Paren(paren) => expr_is_ok_unit(&paren.expr),
        _ => false,
    }
}

pub(crate) fn expr_string_literal(expr: &Expr) -> Option<&syn::LitStr> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(value) => Some(value),
            _ => None,
        },
        Expr::Group(group) => expr_string_literal(&group.expr),
        Expr::Paren(paren) => expr_string_literal(&paren.expr),
        _ => None,
    }
}

pub(crate) fn expr_is_format_macro(expr: &Expr) -> bool {
    matches!(expr, Expr::Macro(mac) if macro_path_ends(mac, "format"))
}

pub(crate) fn expr_is_vec_macro(expr: &Expr) -> bool {
    matches!(expr, Expr::Macro(mac) if macro_path_ends(mac, "vec"))
}

pub(crate) fn macro_path_ends(mac: &ExprMacro, expected: &str) -> bool {
    mac.mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == expected)
}

pub(crate) fn format_macro_uses_only_static_fragments(expr: &Expr) -> bool {
    let Expr::Macro(mac) = expr else {
        return false;
    };
    if !macro_path_ends(mac, "format") {
        return false;
    }

    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(args) = parser.parse2(mac.mac.tokens.clone()) else {
        return false;
    };
    if args.len() <= 1 {
        return false;
    }

    args.iter().skip(1).all(expr_is_static_string)
}

pub(crate) fn format_macro_template_contains(expr: &Expr, needle: &str) -> bool {
    let Expr::Macro(mac) = expr else {
        return false;
    };
    if !macro_path_ends(mac, "format") {
        return false;
    }

    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(args) = parser.parse2(mac.mac.tokens.clone()) else {
        return false;
    };
    args.first()
        .and_then(expr_string_literal)
        .is_some_and(|lit| lit.value().contains(needle))
}

pub(crate) fn format_macro_template_looks_like_html(mac: &ExprMacro) -> bool {
    if !macro_path_ends(mac, "format") {
        return false;
    }

    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(args) = parser.parse2(mac.mac.tokens.clone()) else {
        return false;
    };
    args.first()
        .and_then(expr_string_literal)
        .is_some_and(|lit| string_looks_like_html(&lit.value()))
}

pub(crate) fn expr_is_join_call(expr: &Expr) -> bool {
    matches!(expr, Expr::MethodCall(method) if method.method == "join")
}

pub(crate) fn expr_contains_vec_macro(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
            if macro_path_ends(node, "vec") {
                self.found = true;
                return;
            }
            visit::visit_expr_macro(self, node);
        }
    }

    if expr_is_vec_macro(expr) {
        return true;
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

pub(crate) fn expr_is_string_builder_init(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => {
            call_path_ends(call, &["String", "new"])
                || call_path_ends(call, &["String", "with_capacity"])
        }
        Expr::Group(group) => expr_is_string_builder_init(&group.expr),
        Expr::Paren(paren) => expr_is_string_builder_init(&paren.expr),
        _ => false,
    }
}

pub(crate) fn template_macro_dialect(node: &ExprMacro) -> Option<MacroDialect> {
    let name = node.mac.path.segments.last()?.ident.to_string();
    match name.as_str() {
        "html" | "html_sync" => Some(MacroDialect::Html),
        "xml" | "xml_sync" => Some(MacroDialect::Xml),
        _ => None,
    }
}

pub(crate) fn path_is_html_macro(path: &SynPath) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| matches!(segment.ident.to_string().as_str(), "html" | "html_sync"))
}

pub(crate) fn expr_tail_html_macro(expr: &Expr) -> Option<&ExprMacro> {
    match expr {
        Expr::Macro(mac) if path_is_html_macro(&mac.mac.path) => Some(mac),
        Expr::Macro(_) => None,
        Expr::MethodCall(method) => expr_tail_html_macro(&method.receiver),
        Expr::Await(await_expr) => expr_tail_html_macro(&await_expr.base),
        Expr::Paren(paren) => expr_tail_html_macro(&paren.expr),
        Expr::Group(group) => expr_tail_html_macro(&group.expr),
        Expr::Block(block) => block.block.stmts.last().and_then(|stmt| match stmt {
            syn::Stmt::Expr(expr, _) => expr_tail_html_macro(expr),
            _ => None,
        }),
        _ => None,
    }
}

pub(crate) fn for_loop_has_template_body(for_loop: &syn::ExprForLoop) -> bool {
    for_loop.body.stmts.last().is_some_and(|stmt| match stmt {
        syn::Stmt::Expr(expr, _) => expr_tail_html_macro(expr).is_some(),
        syn::Stmt::Macro(stmt) => path_is_html_macro(&stmt.mac.path),
        _ => false,
    })
}

pub(crate) fn attr_accepts_streamed_value(name: &MarkupName) -> bool {
    matches!(
        name.as_markup_str(),
        "href" | "src" | "title" | "alt" | "aria-label" | "d" | "style"
    ) || name.as_markup_str().starts_with("aria-")
}

pub(crate) fn attr_is_preformatted_label(name: &MarkupName) -> bool {
    matches!(name.as_markup_str(), "title" | "aria-label")
        || name.as_markup_str().starts_with("aria-")
}

pub(crate) fn expr_is_signal_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => call_path_ends(call, &["signal"]),
        Expr::MethodCall(method) if method.method == "into_signal" => true,
        _ => false,
    }
}

pub(crate) fn call_path_is_rsjs_client_primitive(call: &ExprCall) -> bool {
    rsjs_client_primitive_capture(call).is_some()
}

pub(crate) fn rsjs_client_primitive_capture(
    call: &ExprCall,
) -> Option<(proc_macro2::Span, String)> {
    if call_path_is_rsjs_opaque_boundary(call) {
        return None;
    }
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    let last = path.path.segments.last()?;
    let recognised = match last.ident.to_string().as_str() {
        // `shared` and `batch` follow the macro dialect's path discipline
        // (mirroring `is_rsjs_handler_constructor_call` in rsjs_macros): the
        // dialect only recognises a bare use-imported call (`batch(...)`) or
        // an `rsjs::`-prefixed two-segment path (`rsjs::shared::<T>()`).
        // Longer paths such as `pricing::batch(...)` are ordinary server
        // helpers and must not be treated as client primitives.
        "shared" | "batch" => path_is_rsjs_dialect_call(&path.path),
        "signal"
        | "memo"
        | "query"
        | "query_with_options"
        | "query_with_initial"
        | "mutation"
        | "mutation_with_options"
        | "node_ref"
        | "interval"
        | "handler"
        | "event_handler" => true,
        _ => false,
    };
    if recognised {
        Some((last.ident.span(), last.ident.to_string()))
    } else {
        None
    }
}

/// The rsjs macro dialect recognises a primitive constructor call only when
/// its path is a single segment (bare call after `use rsjs::...;`) or a
/// two-segment path whose first segment is `rsjs`, with or without a leading
/// `::`. This mirrors `is_rsjs_handler_constructor_call` in
/// `rsjs_macros/src/analyze/prelude.rs`; keeping the lint on the same path
/// discipline prevents server helpers like `pricing::batch(...)` from being
/// misclassified as client primitives.
pub(crate) fn path_is_rsjs_dialect_call(path: &syn::Path) -> bool {
    match path.segments.len() {
        1 => true,
        2 => path
            .segments
            .first()
            .map(|segment| segment.ident == "rsjs")
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn call_path_is_rsjs_shared_state(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "shared")
        && path_is_rsjs_dialect_call(&path.path)
}

pub(crate) fn call_path_is_rsjs_browser_helper(call: &ExprCall) -> bool {
    rsjs_browser_helper_capture(call).is_some()
}

pub(crate) fn rsjs_browser_helper_capture(call: &ExprCall) -> Option<(proc_macro2::Span, String)> {
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    let mut segments = path.path.segments.iter();
    let first = segments.next()?;
    if first.ident != "rsjs" {
        return None;
    }
    let second = segments.next()?;
    if matches!(
        second.ident.to_string().as_str(),
        "window"
            | "document"
            | "navigator"
            | "location"
            | "history"
            | "local_storage"
            | "session_storage"
            | "url"
            | "clipboard"
            | "webauthn"
    ) {
        Some((second.ident.span(), format!("rsjs::{}", second.ident)))
    } else {
        None
    }
}

pub(crate) fn expr_is_rsjs_event_handler_shape(expr: &Expr) -> bool {
    match expr {
        Expr::Closure(_) | Expr::Call(_) => true,
        Expr::MethodCall(method) => method.method == "call" || method.method == "call_if_some",
        Expr::Paren(paren) => expr_is_rsjs_event_handler_shape(&paren.expr),
        Expr::Group(group) => expr_is_rsjs_event_handler_shape(&group.expr),
        _ => false,
    }
}

pub(crate) fn expr_is_bind_signal_shape(expr: &Expr) -> bool {
    match expr {
        Expr::Path(path) => path.path.segments.len() == 1,
        Expr::Paren(paren) => expr_is_bind_signal_shape(&paren.expr),
        Expr::Group(group) => expr_is_bind_signal_shape(&group.expr),
        _ => false,
    }
}

pub(crate) fn rsjs_event_attr_name(raw: &str) -> bool {
    raw.starts_with("on_") || raw.starts_with("on-")
}

pub(crate) fn rsjs_bind_attr_name(raw: &str) -> bool {
    raw.starts_with("bind_") || raw.starts_with("bind-")
}

pub(crate) fn expr_is_simple_ssr_value(expr: &Expr) -> bool {
    match expr {
        Expr::Group(group) => expr_is_simple_ssr_value(&group.expr),
        Expr::Paren(paren) => expr_is_simple_ssr_value(&paren.expr),
        Expr::Reference(reference) => expr_is_simple_ssr_value(&reference.expr),
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Field(field) => expr_is_simple_ssr_value(&field.base),
        _ => false,
    }
}

pub(crate) fn expr_path_single_ident(expr: &Expr) -> Option<String> {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    else {
        return None;
    };
    if path.segments.len() != 1 {
        return None;
    }
    path.segments
        .first()
        .map(|segment| segment.ident.to_string())
}

pub(crate) fn call_path_is_raw_browser_global(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path_is_raw_browser_global(&path.path)
}

pub(crate) fn path_is_raw_browser_global(path: &SynPath) -> bool {
    let mut segments = path.segments.iter();
    let Some(first) = segments.next().map(|segment| segment.ident.to_string()) else {
        return false;
    };
    if first == "rsjs" {
        return false;
    }
    if matches!(
        first.as_str(),
        "window"
            | "document"
            | "navigator"
            | "location"
            | "history"
            | "localStorage"
            | "sessionStorage"
    ) {
        return true;
    }

    first == "web_sys"
        && path
            .segments
            .last()
            .is_some_and(|last| matches!(last.ident.to_string().as_str(), "window" | "document"))
}

pub(crate) fn call_path_is_effect_leak(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| method_name_is_effect_leak(&segment.ident.to_string()))
}

pub(crate) fn method_name_is_effect_leak(method: &str) -> bool {
    matches!(
        method,
        "add_event_listener"
            | "add_event_listener_with_callback"
            | "add_event_listener_with_callback_and_bool"
            | "add_event_listener_with_callback_and_add_event_listener_options"
            | "set_interval"
            | "set_interval_with_callback"
            | "set_timeout"
            | "set_timeout_with_callback"
            | "request_animation_frame"
            | "observe"
    )
}

pub(crate) fn call_path_is_nonstatic_selector(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| method_name_is_nonstatic_selector(&segment.ident.to_string()))
}

pub(crate) fn method_name_is_nonstatic_selector(method: &str) -> bool {
    matches!(
        method,
        "query_selector" | "query_selector_all" | "closest" | "matches" | "get_element_by_id"
    )
}

pub(crate) fn call_path_is_json_parse(call: &ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path_ends(&path.path, &["serde_json", "from_str"])
        || path_ends(&path.path, &["JSON", "parse"])
        || path_ends(&path.path, &["Json", "parse"])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RsjsOpaqueBoundaryKind {
    ServerOnly,
    ServerAttr,
    Island,
}

pub(crate) fn rsjs_opaque_boundary_kind(call: &ExprCall) -> Option<RsjsOpaqueBoundaryKind> {
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    match path.path.segments.last()?.ident.to_string().as_str() {
        "server_only" => Some(RsjsOpaqueBoundaryKind::ServerOnly),
        "server_attr" => Some(RsjsOpaqueBoundaryKind::ServerAttr),
        "island" => Some(RsjsOpaqueBoundaryKind::Island),
        _ => None,
    }
}

pub(crate) fn call_path_is_rsjs_opaque_boundary(call: &ExprCall) -> bool {
    rsjs_opaque_boundary_kind(call).is_some()
}

pub(crate) fn call_path_is_rsjs_server_boundary(call: &ExprCall) -> bool {
    matches!(
        rsjs_opaque_boundary_kind(call),
        Some(RsjsOpaqueBoundaryKind::ServerOnly | RsjsOpaqueBoundaryKind::ServerAttr)
    )
}

pub(crate) fn expr_is_static_string(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) => matches!(lit.lit, Lit::Str(_)),
        Expr::Reference(reference) => expr_is_static_string(&reference.expr),
        _ => false,
    }
}

pub(crate) fn expr_contains_browser_storage_read(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Expr::Path(path) = node.func.as_ref() {
                if path_contains_storage_segment(&path.path) {
                    self.found = true;
                    return;
                }
            }
            visit::visit_expr_call(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if matches!(
                node.method.to_string().as_str(),
                "get_item" | "get_item_or" | "local_storage" | "session_storage"
            ) {
                self.found = true;
                return;
            }
            visit::visit_expr_method_call(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

pub(crate) fn path_contains_storage_segment(path: &SynPath) -> bool {
    path.segments.iter().any(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "local_storage" | "session_storage" | "localStorage" | "sessionStorage"
        )
    })
}

/// Does a macro token stream (write!/format! arguments) contain a string
/// literal that looks like HTML markup?
pub(crate) fn tokens_contain_html_literal(tokens: &proc_macro2::TokenStream) -> bool {
    fn scan(tokens: proc_macro2::TokenStream) -> bool {
        for tree in tokens {
            match tree {
                proc_macro2::TokenTree::Literal(lit) => {
                    let repr = lit.to_string();
                    let bytes = repr.as_bytes();
                    for window in bytes.windows(2) {
                        if window[0] == b'<'
                            && (window[1].is_ascii_alphabetic() || window[1] == b'/')
                        {
                            return true;
                        }
                    }
                }
                proc_macro2::TokenTree::Group(group) if scan(group.stream()) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    scan(tokens.clone())
}

pub(crate) fn expr_is_html_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) => lit_is_html(&lit.lit),
        Expr::Reference(reference) => expr_is_html_literal(&reference.expr),
        Expr::MethodCall(method) if matches!(method.method.to_string().as_str(), "as_ref") => {
            expr_is_html_literal(&method.receiver)
        }
        _ => false,
    }
}

pub(crate) fn lit_is_html(lit: &Lit) -> bool {
    match lit {
        Lit::Str(lit) => string_looks_like_html(&lit.value()),
        Lit::ByteStr(lit) => std::str::from_utf8(&lit.value()).is_ok_and(string_looks_like_html),
        _ => false,
    }
}

pub(crate) fn string_looks_like_html(value: &str) -> bool {
    value.contains('<')
        || value.contains("</")
        || value.contains("class=")
        || value.contains("style=")
        || value.contains("data-")
        || value.contains("aria-")
}

pub(crate) fn expr_contains_format_macro(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
            if node
                .mac
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "format")
            {
                self.found = true;
                return;
            }
            visit::visit_expr_macro(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

pub(crate) fn expr_contains_case_conversion(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if matches!(
                node.method.to_string().as_str(),
                "to_ascii_lowercase" | "to_lowercase"
            ) {
                self.found = true;
                return;
            }
            visit::visit_expr_method_call(self, node);
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

pub(crate) fn binary_mentions_string_literal(node: &ExprBinary) -> bool {
    expr_mentions_string_literal(&node.left) || expr_mentions_string_literal(&node.right)
}

pub(crate) fn expr_mentions_string_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) => matches!(lit.lit, syn::Lit::Str(_)),
        Expr::Binary(binary) => binary_mentions_string_literal(binary),
        _ => false,
    }
}
