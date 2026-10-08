// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stage 1: parse a `#[rsjs(component...)]` render-trait impl and lift its
//! `html!` body.
//!
//! This module owns the syn-based parsing of an island's trait impl
//! block and the location of the `html!` / `html_sync!` invocation
//! inside its render method body. Output is a [`ParsedIsland`] that
//! the analyser walks to build the rsjs IR side-tables.
//!
//! Validation enforced here (syntactic only; proc macros cannot
//! type-resolve, so anything beyond syntax is left to Rust's normal
//! type-checker):
//!
//! - The annotated item must be a render-trait `impl` block using the explicit
//!   `#[rsjs(component)]` or `#[rsjs(component, client)]` component marker.
//!   Rustc validates the exact trait identity through a generated associated
//!   witness; the parser selects sync versus async from the canonical authored
//!   return type.
//! - The impl contains exactly one method named `render`.
//! - The render method body contains one `html!` / `html_sync!` invocation as
//!   its tail expression (modulo a `.render(cx)` chain for sync, or
//!   `.render(cx).resolve().await` for async). The parser retains arbitrary
//!   preceding server statements instead of rejecting them before client
//!   reachability is known. Site-local emission preserves them for both
//!   server-only and hydrated results, while rejecting a statement only when
//!   it visibly reads compiler-owned client state or browser behavior.
//!
//! Async components author the real `WebRender::render` signature and return
//! `RenderOutcome::pending(async move { ... })`; `async fn render` sugar is
//! intentionally rejected.

use proa_dsl::parse::HtmlRoot;
use proc_macro2::TokenStream;
use quote::ToTokens;
use rsjs_ir::HydrationStrategy;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Block, Expr, ExprAsync, FnArg, Generics, ImplItem, ImplItemFn, ItemImpl, Local, Macro, Pat,
    Path, ReturnType, Signature, Stmt, Type,
};

/// Canonical render surface selected from the authored method return type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderKind {
    Sync,
    Async,
}

impl RenderKind {
    pub(crate) const fn is_async(self) -> bool {
        matches!(self, Self::Async)
    }
}

/// Result of parsing a `#[rsjs(component...)]` input.
pub(crate) struct ParsedIsland {
    /// Path of the user's struct (the impl's self type).
    pub struct_path: Path,
    /// The full impl generics from the user-authored `impl` block.
    pub impl_generics: Generics,
    /// The trait path from the user-authored impl header.
    pub trait_path: Path,
    /// The full self type from the user-authored impl header.
    pub self_ty: Type,
    /// The user-authored render method signature.
    pub render_sig: Signature,
    /// Canonical render surface selected from the authored return type.
    pub render_kind: RenderKind,
    /// The user-authored binding for the render context argument.
    pub context_ident: syn::Ident,
    /// Whether this component carries explicit permission to originate browser
    /// behavior. Phase 1 records the permission for the origin-analysis pass.
    #[allow(dead_code)]
    pub client_origin_allowed: bool,
    /// The `let` bindings that appear before the tail `html!` /
    /// `html_sync!` invocation. The analyser scans these for signal
    /// declarations (`let foo = signal(...);`).
    pub prelude_lets: Vec<Local>,
    /// Every statement before the analyzed render site, in authored order.
    ///
    /// The analyzer uses this only for lexical validation of opaque ordinary
    /// statements. Keeping the interleaving with `let` declarations prevents
    /// a later reactive shadow from changing the meaning of an earlier server
    /// statement (and vice versa).
    pub prelude_statements: Vec<Stmt>,
    /// The lifted `html!` / `html_sync!` parse tree.
    pub html_tree: HtmlRoot,
    /// Client-side hydration trigger declared by the `strategy = "..."`
    /// argument on the component attribute.
    pub hydration_strategy: HydrationStrategy,
    /// Whether the hydration strategy was authored explicitly.
    ///
    /// `HydrationStrategy::Immediate` is also the local renderer's default,
    /// but the semantic component ABI distinguishes an absent constraint from
    /// an explicit `strategy = "eager"` / `"immediate"` requirement.
    pub hydration_strategy_explicit: bool,
}

/// Parsed `#[rsjs(...)]` attribute arguments.
#[derive(Debug)]
pub(crate) struct ParsedAttrArgs {
    pub hydration_strategy: HydrationStrategy,
    pub hydration_strategy_explicit: bool,
    /// Opt the authored render-trait impl into component analysis.
    pub component: bool,
    /// Permit this component to originate browser behavior.
    pub client_origin_allowed: bool,
}

/// Parse the `#[rsjs(...)]` attribute argument list.
///
/// v0.1 grammar:
///
/// ```text
/// rsjs_attr_args := (strategy_arg | component_arg | client_arg)*
/// strategy_arg   := "strategy" "=" string_lit
/// component_arg  := "component"
/// client_arg     := "client"
/// ```
///
/// Empty argument lists default to immediate hydration. The removed
/// component-level `clients(...)` allowlist gets a focused migration
/// diagnostic: helper eligibility now comes exclusively from the helper
/// definition's `#[rsjs::client]` annotation, and rustc resolves normal call
/// paths through the relocation-backed linker.
pub(crate) fn parse_attr_args(attr: TokenStream) -> syn::Result<ParsedAttrArgs> {
    if attr.is_empty() {
        return Ok(ParsedAttrArgs {
            hydration_strategy: HydrationStrategy::Immediate,
            hydration_strategy_explicit: false,
            component: false,
            client_origin_allowed: false,
        });
    }
    let parsed: AttrArgs = syn::parse2(attr)?;
    Ok(ParsedAttrArgs {
        hydration_strategy: parsed.hydration_strategy,
        hydration_strategy_explicit: parsed.hydration_strategy_explicit,
        component: parsed.component,
        client_origin_allowed: parsed.client_origin_allowed,
    })
}

/// Render a `syn::Path` to the canonical whitespace-stripped spelling retained
/// on rustc-resolved component/helper source edges. A leading `::` is omitted
/// because the semantic identity comes from the resolved relocation rather
/// than from these source tokens.
pub(crate) fn path_to_canonical_string(path: &Path) -> String {
    let raw: String = path
        .to_token_stream()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    raw.strip_prefix("::").map(str::to_owned).unwrap_or(raw)
}

/// Internal parse-tree for the `#[rsjs(...)]` argument list.
struct AttrArgs {
    hydration_strategy: HydrationStrategy,
    hydration_strategy_explicit: bool,
    component: bool,
    client_origin_allowed: bool,
}

impl syn::parse::Parse for AttrArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut hydration_strategy = None;
        let mut component = false;
        let mut client_keyword = None;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            if key == "clients" {
                return Err(syn::Error::new(
                    key.span(),
                    "`clients(...)` was removed from `#[rsjs(...)]`; annotate each portable \
                     helper definition once with `#[rsjs::client]` and call it through an \
                     ordinary Rust path. Reachable helper calls are linked automatically; \
                     `#[rsjs::client_module]` remains optional grouping.",
                ));
            } else if key == "strategy" {
                if hydration_strategy.is_some() {
                    return Err(syn::Error::new(
                        key.span(),
                        "`strategy = ...` may only be specified once in `#[rsjs(...)]`",
                    ));
                }
                let _: syn::Token![=] = input.parse()?;
                let value: syn::LitStr = input.parse()?;
                hydration_strategy = Some(parse_hydration_strategy(&value)?);
            } else if key == "component" {
                if component {
                    return Err(syn::Error::new(
                        key.span(),
                        "`#[rsjs(component)]` may only be specified once",
                    ));
                }
                component = true;
            } else if key == "client" {
                if client_keyword.is_some() {
                    return Err(syn::Error::new(
                        key.span(),
                        "`#[rsjs(..., client)]` may only be specified once",
                    ));
                }
                client_keyword = Some(key);
            } else if key == "render" {
                let _: syn::Token![=] = input.parse()?;
                let value: syn::LitStr = input.parse()?;
                return Err(unsupported_render_mode(&key, &value));
            } else if key == "client_only" {
                return Err(unsupported_client_only_alias(&key));
            } else if key == "fallback" {
                return Err(unsupported_client_only_fallback(&key));
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!(
                        "`#[rsjs(...)]` does not recognise `{key}`; \
                         expected `component`, `client`, or `strategy = \"...\"`",
                    ),
                ));
            }
            // Allow a trailing comma between top-level args.
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        if let Some(client_keyword) = &client_keyword {
            if !component {
                return Err(syn::Error::new(
                    client_keyword.span(),
                    "`client` is only valid with `component`; write \
                     `#[rsjs(component, client)]` on a real `WebRenderSync` / \
                     `WebRender` trait impl",
                ));
            }
        }
        Ok(AttrArgs {
            hydration_strategy_explicit: hydration_strategy.is_some(),
            hydration_strategy: hydration_strategy.unwrap_or_default(),
            component,
            client_origin_allowed: client_keyword.is_some(),
        })
    }
}

fn unsupported_render_mode(key: &syn::Ident, value: &syn::LitStr) -> syn::Error {
    let mode = value.value();
    let trimmed = mode.trim();
    if trimmed == "client" {
        return syn::Error::new(
            key.span(),
            "`#[rsjs(render = \"client\")]` is reserved but not supported yet; \
             client-only rendering changes SSR, no-JS behaviour, accessibility, and SEO, \
             so it will require an explicit fallback policy before rsjs accepts it",
        );
    }

    syn::Error::new(
        key.span(),
        format!(
            "`#[rsjs(render = {:?})]` is not supported; rsjs currently infers \
             `ServerOnly` or `SSR + Hydrate` from reachable client roots, and \
             `strategy = \"...\"` only controls when hydration runs",
            mode
        ),
    )
}

fn unsupported_client_only_alias(key: &syn::Ident) -> syn::Error {
    syn::Error::new(
        key.span(),
        "`#[rsjs(client_only)]` is reserved but not supported yet; use an SSR-capable \
         island today, because future client-only rendering must make its fallback/no-JS \
         policy explicit",
    )
}

fn unsupported_client_only_fallback(key: &syn::Ident) -> syn::Error {
    syn::Error::new(
        key.span(),
        "`#[rsjs(fallback = ...)]` is reserved for future client-only rendering; regular \
         rsjs islands must produce SSR from their render body",
    )
}

fn parse_hydration_strategy(value: &syn::LitStr) -> syn::Result<HydrationStrategy> {
    let raw = value.value();
    let trimmed = raw.trim();
    match trimmed {
        "eager" | "immediate" => Ok(HydrationStrategy::Immediate),
        "visible" => Ok(HydrationStrategy::Visible),
        "idle" => Ok(HydrationStrategy::Idle),
        "interaction" => Ok(HydrationStrategy::Interaction { event: None }),
        _ if trimmed.starts_with("interaction:") => {
            let event = trimmed
                .strip_prefix("interaction:")
                .expect("prefix checked")
                .trim();
            if event.is_empty() {
                return Err(invalid_hydration_strategy(
                    value,
                    "interaction strategies must name an event after `interaction:`",
                ));
            }
            Ok(HydrationStrategy::Interaction {
                event: Some(event.to_string()),
            })
        }
        _ if trimmed.starts_with("media:") => {
            let query = trimmed
                .strip_prefix("media:")
                .expect("prefix checked")
                .trim();
            if query.is_empty() {
                return Err(invalid_hydration_strategy(
                    value,
                    "media strategies must include a media query after `media:`",
                ));
            }
            Ok(HydrationStrategy::Media {
                query: query.to_string(),
            })
        }
        _ => Err(invalid_hydration_strategy(
            value,
            "expected `eager`, `immediate`, `visible`, `idle`, `interaction`, \
             `interaction:<event>`, or `media:<query>`",
        )),
    }
}

fn invalid_hydration_strategy(value: &syn::LitStr, reason: &str) -> syn::Error {
    syn::Error::new(
        value.span(),
        format!(
            "unsupported RSJS hydration strategy {:?}: {reason}",
            value.value()
        ),
    )
}

pub(crate) fn parse_island_item(
    item: ItemImpl,
    attr_args: ParsedAttrArgs,
) -> syn::Result<ParsedIsland> {
    if !attr_args.component {
        return Err(syn::Error::new(
            item.span(),
            "bare `#[rsjs]` render-trait analysis has been removed; use \
             `#[rsjs(component)]` for a participant component or \
             `#[rsjs(component, client)]` when this impl originates browser behavior",
        ));
    }
    validate_reserved_identifiers(&item)?;

    // 1. The item must be a trait impl. Exact trait identity is checked by the
    //    generated associated witness, which lets ordinary imports, aliases,
    //    and re-exports resolve through rustc instead of token-name matching.
    let trait_ref = match &item.trait_ {
        Some((bang, path, _)) => {
            if let Some(b) = bang {
                return Err(syn::Error::new(
                    b.span(),
                    "`#[rsjs]` does not support negative trait impls (`impl !Trait for ...`)",
                ));
            }
            path
        }
        None => {
            return Err(inherent_component_migration_error(&item));
        }
    };

    // 2. Self type must be a path (a struct).
    let struct_path = match item.self_ty.as_ref() {
        syn::Type::Path(tp) if tp.qself.is_none() => tp.path.clone(),
        other => {
            return Err(syn::Error::new(
                other.span(),
                "`#[rsjs]` expects the impl's self type to be a struct path \
                 (e.g. `for Counter` or `for crate::ui::Counter`); \
                 references, tuples, and qualified-self types are not supported in v0.1",
            ));
        }
    };

    // 3. There must be exactly one method named `render`.
    let render_fn = find_render_method(&item)?;
    reject_extra_impl_items(&item)?;

    // 4. The authored return type, not an await scan or trait-path spelling,
    //    selects the descriptor render kind.
    let (render_kind, context_ident) = validate_render_signature(&render_fn.sig)?;

    // 5. Locate the analyzed future body for async `WebRender`, then locate the
    //    tail template in the same constrained prelude shape used by sync.
    let analyzed_block = match render_kind {
        RenderKind::Sync => {
            if block_contains_await(&render_fn.block) {
                return Err(syn::Error::new(
                    render_fn.block.span(),
                    "a `WebRenderSync` component cannot await server work; implement the \
                     real `WebRender` signature and return \
                     `RenderOutcome::pending(async move { ... })`",
                ));
            }
            &render_fn.block
        }
        RenderKind::Async => &locate_pending_async_body(&render_fn.block)?.block,
    };
    let (prelude_lets, _prelude_intervals, _opaque_prelude_statements, tail_expr) =
        split_body(analyzed_block)?;
    let prelude_statements = analyzed_block.stmts[..analyzed_block.stmts.len() - 1].to_vec();
    let macro_call = locate_html_macro(tail_expr)?;
    validate_html_macro_for_render_kind(macro_call, render_kind)?;
    let html_tree: HtmlRoot = syn::parse2(macro_call.tokens.clone()).map_err(|e| {
        syn::Error::new(
            macro_call.span(),
            format!("`#[rsjs]` failed to parse the `html_sync!` / `html!` body as HTML: {e}"),
        )
    })?;

    Ok(ParsedIsland {
        struct_path,
        impl_generics: item.generics.clone(),
        trait_path: trait_ref.clone(),
        self_ty: (*item.self_ty).clone(),
        render_sig: render_fn.sig.clone(),
        render_kind,
        context_ident,
        client_origin_allowed: attr_args.client_origin_allowed,
        prelude_lets,
        prelude_statements,
        html_tree,
        hydration_strategy: attr_args.hydration_strategy,
        hydration_strategy_explicit: attr_args.hydration_strategy_explicit,
    })
}

fn validate_reserved_identifiers(item: &ItemImpl) -> syn::Result<()> {
    struct ReservedIdent {
        error: Option<syn::Error>,
    }

    impl<'ast> Visit<'ast> for ReservedIdent {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            if self.error.is_none() && ident.to_string().starts_with("__rsjs_") {
                self.error = Some(syn::Error::new(
                    ident.span(),
                    "rsjs/reserved-identifier-prefix: identifiers beginning with `__rsjs_` \
                     are reserved for hygienic compiler state\nhelp: rename this identifier",
                ));
                return;
            }
            visit::visit_ident(self, ident);
        }
    }

    let mut visitor = ReservedIdent { error: None };
    visitor.visit_item_impl(item);
    if let Some(error) = visitor.error {
        return Err(error);
    }
    Ok(())
}

fn inherent_component_migration_error(item: &ItemImpl) -> syn::Error {
    let self_ty = item.self_ty.to_token_stream().to_string();
    syn::Error::new(
        item.span(),
        format!(
            "`#[rsjs(component)]` now requires a real Proa render-trait impl; \
             inherent `fn render(self)` shorthand has been removed\n\
             help: write:\n\
             impl<L: ::proa_core::DataLoader> \
             ::proa_core::WebRenderSync<L> for {self_ty} {{\n    \
                 fn render(self, cx: &mut ::proa_core::WebContext<L>) \
                 -> Result<(), ::proa_core::WriteError> {{\n        \
                     ::proa_core::WebRenderSync::render(\n            \
                         ::proa_macros::html_sync! {{ /* ... */ }},\n            \
                         cx,\n        \
                     )\n    \
                 }}\n\
             }}"
        ),
    )
}

fn reject_extra_impl_items(item: &ItemImpl) -> syn::Result<()> {
    if let Some(extra) = item.items.iter().find(
        |impl_item| !matches!(impl_item, ImplItem::Fn(method) if method.sig.ident == "render"),
    ) {
        return Err(syn::Error::new(
            extra.span(),
            "`#[rsjs(component...)]` currently analyzes an impl containing only \
             the render method; move associated constants, types, and helper \
             methods to a separate impl block",
        ));
    }
    Ok(())
}

fn validate_render_signature(sig: &Signature) -> syn::Result<(RenderKind, syn::Ident)> {
    if sig.asyncness.is_some() {
        return Err(syn::Error::new(
            sig.asyncness.span(),
            "`async fn render` sugar has been removed; implement the real \
             `WebRender::render` signature returning \
             `RenderOutcome<impl Future<Output = Result<(), WriteError>>>` \
             and return `RenderOutcome::pending(async move { ... })`",
        ));
    }
    if sig.inputs.len() != 2 {
        return Err(syn::Error::new(
            sig.inputs.span(),
            "an analyzed render method must take exactly `self` and one named \
             `WebContext` argument",
        ));
    }
    let context_arg = sig
        .inputs
        .iter()
        .nth(1)
        .expect("input length checked above");
    let FnArg::Typed(context_arg) = context_arg else {
        return Err(syn::Error::new(
            context_arg.span(),
            "the second render argument must be a named `WebContext` binding",
        ));
    };
    let Pat::Ident(context_pat) = context_arg.pat.as_ref() else {
        return Err(syn::Error::new(
            context_arg.pat.span(),
            "the render context argument must use a plain identifier, for example \
             `cx: &mut WebContext<L>`",
        ));
    };
    let render_kind = classify_render_return_type(&sig.output)?;
    Ok((render_kind, context_pat.ident.clone()))
}

fn classify_render_return_type(output: &ReturnType) -> syn::Result<RenderKind> {
    let ReturnType::Type(_, ty) = output else {
        return Err(canonical_render_return_error(output.span()));
    };
    if is_result_unit_write_error(ty) {
        return Ok(RenderKind::Sync);
    }
    if is_render_outcome_future(ty) {
        return Ok(RenderKind::Async);
    }
    Err(canonical_render_return_error(output.span()))
}

fn canonical_render_return_error(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "analyzed components must use one canonical render return type: \
         `Result<(), WriteError>` for `WebRenderSync`, or \
         `RenderOutcome<impl Future<Output = Result<(), WriteError>>>` \
         for `WebRender`; return-type aliases and concrete future refinements \
         are not supported in Phase 1",
    )
}

fn is_result_unit_write_error(ty: &Type) -> bool {
    let Some(args) = type_path_arguments(ty, "Result") else {
        return false;
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let Some(ok_ty) = types.next() else {
        return false;
    };
    let Some(err_ty) = types.next() else {
        return false;
    };
    types.next().is_none()
        && args.args.len() == 2
        && matches!(ok_ty, Type::Tuple(tuple) if tuple.elems.is_empty())
        && type_path_ends_with(err_ty, "WriteError")
}

fn is_render_outcome_future(ty: &Type) -> bool {
    let Some(args) = type_path_arguments(ty, "RenderOutcome") else {
        return false;
    };
    if args.args.len() != 1 {
        return false;
    }
    let Some(syn::GenericArgument::Type(Type::ImplTrait(impl_trait))) = args.args.first() else {
        return false;
    };

    let mut saw_future = false;
    for bound in &impl_trait.bounds {
        match bound {
            syn::TypeParamBound::Trait(trait_bound) if !saw_future => {
                let Some(future_args) = trait_path_arguments(&trait_bound.path, "Future") else {
                    return false;
                };
                if future_args.args.len() != 1 {
                    return false;
                }
                let Some(syn::GenericArgument::AssocType(output)) = future_args.args.first() else {
                    return false;
                };
                if output.ident != "Output" || !is_result_unit_write_error(&output.ty) {
                    return false;
                }
                saw_future = true;
            }
            _ => return false,
        }
    }
    saw_future
}

fn type_path_arguments<'a>(
    ty: &'a Type,
    final_ident: &str,
) -> Option<&'a syn::AngleBracketedGenericArguments> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    trait_path_arguments(&type_path.path, final_ident)
}

fn trait_path_arguments<'a>(
    path: &'a Path,
    final_ident: &str,
) -> Option<&'a syn::AngleBracketedGenericArguments> {
    let segment = path.segments.last()?;
    if segment.ident != final_ident {
        return None;
    }
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => Some(args),
        _ => None,
    }
}

fn type_path_ends_with(ty: &Type, final_ident: &str) -> bool {
    matches!(
        ty,
        Type::Path(type_path)
            if type_path.qself.is_none()
                && type_path.path.segments.last().is_some_and(|segment| {
                    segment.ident == final_ident
                        && matches!(segment.arguments, syn::PathArguments::None)
                })
    )
}

/// Find the unique `render` method on the impl block. Errors if zero
/// or more than one is present.
fn find_render_method(item: &ItemImpl) -> syn::Result<&ImplItemFn> {
    let mut found: Option<&ImplItemFn> = None;
    for ii in &item.items {
        if let ImplItem::Fn(f) = ii {
            if f.sig.ident == "render" {
                if found.is_some() {
                    return Err(syn::Error::new(
                        f.sig.ident.span(),
                        "`#[rsjs]` impl block has more than one `render` method; \
                         exactly one is required",
                    ));
                }
                found = Some(f);
            }
        }
    }
    found.ok_or_else(|| {
        syn::Error::new(
            item.span(),
            "`#[rsjs]` impl block must define exactly one method named `render` \
             (the `WebRenderSync::render` / `WebRender::render` method)",
        )
    })
}

fn locate_pending_async_body(block: &Block) -> syn::Result<&ExprAsync> {
    let [Stmt::Expr(Expr::Call(call), None)] = block.stmts.as_slice() else {
        return Err(syn::Error::new(
            block.span(),
            "an analyzed `WebRender` method must return exactly \
             `RenderOutcome::pending(async move { ... })`; put the server \
             prelude and `html!` render inside that async block",
        ));
    };
    let Expr::Path(function) = call.func.as_ref() else {
        return Err(syn::Error::new(
            call.func.span(),
            "expected `RenderOutcome::pending(async move { ... })`",
        ));
    };
    let mut segments = function.path.segments.iter().rev();
    let is_pending_constructor = segments
        .next()
        .is_some_and(|segment| segment.ident == "pending")
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "RenderOutcome");
    if !is_pending_constructor || call.args.len() != 1 {
        return Err(syn::Error::new(
            call.span(),
            "expected the canonical `RenderOutcome::pending(async move { ... })` \
             return expression",
        ));
    }
    let Some(Expr::Async(async_body)) = call.args.first() else {
        return Err(syn::Error::new(
            call.args.span(),
            "`RenderOutcome::pending` must receive one `async move { ... }` block",
        ));
    };
    if async_body.capture.is_none() {
        return Err(syn::Error::new(
            async_body.async_token.span(),
            "the pending render future must be `async move { ... }` so it owns \
             the component and its server prelude",
        ));
    }
    Ok(async_body)
}

/// Split a block into (`let` bindings before the tail,
/// `rsjs::interval(...)` prelude statements, opaque server statements, tail
/// expression). The tail must be the last statement.
///
/// Opaque server statements are retained rather than reconstructed. The
/// site-local emitter keeps them in the authored block for server-only and
/// hydrated components alike; analysis separately rejects visible client-state
/// reads that cannot execute as ordinary server Rust.
fn split_body(block: &Block) -> syn::Result<(Vec<Local>, Vec<syn::ExprCall>, Vec<Stmt>, &Expr)> {
    let stmts = &block.stmts;
    let (last, prelude) = match stmts.split_last() {
        Some(p) => p,
        None => {
            return Err(syn::Error::new(
                block.span(),
                "`#[rsjs]` expects a non-empty render body ending in an \
                 `html_sync!` / `html!` invocation",
            ));
        }
    };

    let tail_expr = match last {
        Stmt::Expr(expr, _) => expr,
        other => {
            return Err(syn::Error::new(
                other.span(),
                "`#[rsjs]` expects the render body's tail to be an \
                 `html_sync!` / `html!` invocation chained through `.render(cx)`; \
                 the last statement must be an expression",
            ));
        }
    };

    let (lets, intervals, opaque) = parse_prelude_statements(prelude);
    Ok((lets, intervals, opaque, tail_expr))
}

fn parse_prelude_statements(prelude: &[Stmt]) -> (Vec<Local>, Vec<syn::ExprCall>, Vec<Stmt>) {
    let mut lets: Vec<Local> = Vec::new();
    let mut intervals: Vec<syn::ExprCall> = Vec::new();
    let mut opaque: Vec<Stmt> = Vec::new();
    for stmt in prelude {
        match stmt {
            Stmt::Local(l) => lets.push(l.clone()),
            // `rsjs::interval(ms, || ...);` is the one sanctioned
            // non-`let` prelude statement: it declares a client-only
            // timer, returns nothing, and is never replayed during
            // SSR.
            Stmt::Expr(Expr::Call(call), _) if is_interval_call_expr(call) => {
                intervals.push(call.clone());
            }
            // Ordinary Rust remains part of the server program. Do not inspect
            // or reject it before client reachability is known.
            other => opaque.push(other.clone()),
        }
    }
    (lets, intervals, opaque)
}

/// Returns `true` if `call` is an `interval(...)` /
/// `rsjs::interval(...)` invocation (bare or `rsjs::`-qualified,
/// matching the recognition rule used for `signal(...)` and
/// `memo(...)`).
pub(crate) fn is_interval_call_expr(call: &syn::ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    let Some(last) = path.path.segments.last() else {
        return false;
    };
    last.ident == "interval"
        && match path.path.segments.len() {
            1 => true,
            2 => path
                .path
                .segments
                .first()
                .is_some_and(|segment| segment.ident == "rsjs"),
            _ => false,
        }
}

/// Walk a tail expression to the `html!` / `html_sync!` macro call.
///
/// Accepted shapes:
/// - `html_sync! { ... }.render(cx)` (sync)
/// - `html! { ... }.render(cx).resolve().await` (async)
///
/// Returns a reference to the inner [`Macro`] node so the caller can
/// re-parse its body.
fn locate_html_macro(expr: &Expr) -> syn::Result<&Macro> {
    // Drill in: `.await` -> `.resolve()` -> `.render(cx)` -> macro.
    let mut cur = expr;
    loop {
        match cur {
            Expr::Await(a) => cur = &a.base,
            Expr::MethodCall(mc) => cur = &mc.receiver,
            Expr::Return(ret) => {
                cur = ret.expr.as_deref().ok_or_else(|| {
                    syn::Error::new(
                        ret.return_token.span(),
                        "`return` at the analyzed render site must return the \
                         `html_sync!` / `html!` render expression",
                    )
                })?;
            }
            Expr::Paren(paren) => cur = &paren.expr,
            Expr::Group(group) => cur = &group.expr,
            Expr::Macro(m) => {
                let path = &m.mac.path;
                let last = path.segments.last().map(|s| s.ident.to_string());
                match last.as_deref() {
                    Some("html_sync") | Some("html") => return Ok(&m.mac),
                    _ => {
                        return Err(syn::Error::new(
                            path.span(),
                            "`#[rsjs]` expects the render body's tail to invoke \
                             `html_sync!` (sync) or `html!` (async); \
                             other macros are not supported as the tail expression",
                        ));
                    }
                }
            }
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "`#[rsjs]` expects the render body's tail to be \
                     `html_sync!{...}.render(cx)` or \
                     `html!{...}.render(cx).resolve().await`; \
                     the tail walker hit an unsupported expression shape",
                ));
            }
        }
    }
}

fn validate_html_macro_for_render_kind(
    macro_call: &Macro,
    render_kind: RenderKind,
) -> syn::Result<()> {
    let macro_name = macro_call
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string());
    let expected = match render_kind {
        RenderKind::Sync => "html_sync",
        RenderKind::Async => "html",
    };
    if macro_name.as_deref() == Some(expected) {
        return Ok(());
    }
    let expected_invocation = match render_kind {
        RenderKind::Sync => "`html_sync! { ... }`",
        RenderKind::Async => "`html! { ... }`",
    };
    Err(syn::Error::new(
        macro_call.path.span(),
        format!(
            "this render signature requires {expected_invocation}; sync components \
             use `WebRenderSync` + `html_sync!`, while pending `WebRender` futures \
             use `WebRender` + `html!`"
        ),
    ))
}

/// Returns true if any `.await` appears anywhere inside the block,
/// excluding awaits buried inside nested `async { ... }` blocks or
/// closures.
///
/// An `async {}` block constructs a future; awaits inside it run when
/// the future is polled, not when the surrounding `fn` body runs, so
/// they should not flip the surrounding island into the async path.
/// Same goes for closures: a closure body that awaits has to be an
/// `async` closure, which is itself an `ExprAsync` from the outer
/// block's POV.
fn block_contains_await(block: &Block) -> bool {
    struct AwaitFinder {
        found: bool,
    }
    impl<'ast> Visit<'ast> for AwaitFinder {
        fn visit_expr_await(&mut self, _: &'ast syn::ExprAwait) {
            self.found = true;
        }
        fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {
            // Do not recurse: awaits inside an async block belong to
            // the future the block constructs, not to the surrounding
            // function.
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {
            // Do not recurse: a closure body's awaits run when the
            // closure is called, not where the closure expression
            // appears. (An async closure is itself an `ExprAsync` so
            // it is already excluded by `visit_expr_async`.)
        }
    }
    let mut f = AwaitFinder { found: false };
    visit::visit_block(&mut f, block);
    f.found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_block(src: &str) -> Block {
        syn::parse_str(src).expect("parse_block: invalid block syntax")
    }

    #[test]
    fn await_inside_nested_async_block_does_not_flip_outer_to_async() {
        // The user's prelude constructs a future without awaiting it;
        // the outer fn is sync. Pre-fix this returned `true`.
        let block =
            parse_block("{ let f = async { other().await }; html_sync!{<div></div>}.render(cx) }");
        assert!(!block_contains_await(&block));
    }

    #[test]
    fn await_inside_closure_body_does_not_flip_outer_to_async() {
        // A closure that awaits internally is itself only constructed
        // here; the outer fn does not await.
        let block = parse_block(
            "{ let f = || async { other().await }; html_sync!{<div></div>}.render(cx) }",
        );
        assert!(!block_contains_await(&block));
    }

    #[test]
    fn legitimate_top_level_await_is_detected() {
        let block =
            parse_block("{ let v = real().await; html!{<div></div>}.render(cx).resolve().await }");
        assert!(block_contains_await(&block));
    }

    #[test]
    fn awaited_value_in_let_initializer_is_detected() {
        let block = parse_block("{ let v = real().await; html_sync!{<div></div>}.render(cx) }");
        assert!(block_contains_await(&block));
    }

    /// Source-edge spellings are informational rather than identity
    /// authority, so leading absolute-path punctuation is normalized away.
    #[test]
    fn canonical_string_strips_leading_double_colon() {
        let p_abs: Path = syn::parse_str("::other_crate::foo").unwrap();
        let p_rel: Path = syn::parse_str("other_crate::foo").unwrap();
        assert_eq!(
            path_to_canonical_string(&p_abs),
            path_to_canonical_string(&p_rel)
        );
    }

    #[test]
    fn attr_args_reject_removed_clients_allowlist_with_migration() {
        let attr: proc_macro2::TokenStream = "component, client, clients(other_crate::foo)"
            .parse()
            .unwrap();
        let err = parse_attr_args(attr).expect_err("removed clients syntax must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("`clients(...)` was removed")
                && msg.contains("#[rsjs::client]")
                && msg.contains("linked automatically")
                && msg.contains("#[rsjs::client_module]"),
            "got: {msg}"
        );
    }

    #[test]
    fn attr_args_parse_strategy() {
        let attr: proc_macro2::TokenStream = r#"strategy = "visible""#.parse().unwrap();
        let parsed = parse_attr_args(attr).expect("attr args should parse");
        assert_eq!(parsed.hydration_strategy, HydrationStrategy::Visible);
        assert!(parsed.hydration_strategy_explicit);
    }

    #[test]
    fn attr_args_distinguish_default_from_explicit_eager_strategy() {
        let default =
            parse_attr_args(proc_macro2::TokenStream::new()).expect("empty attr args should parse");
        assert_eq!(default.hydration_strategy, HydrationStrategy::Immediate);
        assert!(!default.hydration_strategy_explicit);

        for spelling in ["eager", "immediate"] {
            let attr: proc_macro2::TokenStream =
                format!(r#"strategy = "{spelling}""#).parse().unwrap();
            let parsed = parse_attr_args(attr).expect("explicit eager strategy should parse");
            assert_eq!(parsed.hydration_strategy, HydrationStrategy::Immediate);
            assert!(parsed.hydration_strategy_explicit);
        }
    }

    #[test]
    fn attr_args_parse_component_with_client_and_strategy() {
        let attr: proc_macro2::TokenStream =
            r#"component, client, strategy = "visible""#.parse().unwrap();
        let parsed = parse_attr_args(attr).expect("component attr args should parse");
        assert!(parsed.component);
        assert!(parsed.client_origin_allowed);
        assert_eq!(parsed.hydration_strategy, HydrationStrategy::Visible);
    }

    #[test]
    fn attr_args_reject_duplicate_component_flag() {
        let attr: proc_macro2::TokenStream = "component, component".parse().unwrap();
        let err = parse_attr_args(attr).expect_err("duplicate component flag must be rejected");
        assert!(
            err.to_string().contains("may only be specified once"),
            "got: {err}"
        );
    }

    #[test]
    fn attr_args_reject_client_without_component() {
        let attr: proc_macro2::TokenStream = "client".parse().unwrap();
        let err = parse_attr_args(attr).expect_err("client must require component");
        assert!(
            err.to_string().contains("only valid with `component`"),
            "got: {err}"
        );
    }

    #[test]
    fn canonical_return_types_select_render_kind() {
        let sync: Signature = syn::parse_quote! {
            fn render(
                self,
                output: &mut WebContext<L, B>,
            ) -> Result<(), WriteError>
        };
        let (kind, context) = validate_render_signature(&sync).unwrap();
        assert_eq!(kind, RenderKind::Sync);
        assert_eq!(context, "output");

        let asynchronous: Signature = syn::parse_quote! {
            fn render(
                self,
                ctx: &mut WebContext<L, B>,
            ) -> RenderOutcome<
                impl core::future::Future<Output = Result<(), WriteError>>
            >
            where
        };
        let (kind, context) = validate_render_signature(&asynchronous).unwrap();
        assert_eq!(kind, RenderKind::Async);
        assert_eq!(context, "ctx");
    }

    #[test]
    fn raw_web_render_extracts_pending_async_move_body() {
        let block = parse_block(
            "{
                RenderOutcome::pending(async move {
                    let value = load().await?;
                    html! { <span>{value}</span> }.render(ctx).resolve().await
                })
            }",
        );
        let async_body = locate_pending_async_body(&block).unwrap();
        let (prelude, intervals, opaque, tail) = split_body(&async_body.block).unwrap();
        assert_eq!(prelude.len(), 1);
        assert!(intervals.is_empty());
        assert!(opaque.is_empty());
        let html = locate_html_macro(tail).unwrap();
        validate_html_macro_for_render_kind(html, RenderKind::Async).unwrap();
    }

    #[test]
    fn ordinary_server_statements_are_retained_for_reachability_validation() {
        let block = parse_block(
            "{
                audit_start();
                let label = load_label()?;
                if label.is_empty() { return fallback(); }
                html_sync! { <span>{label}</span> }.render(cx)
            }",
        );
        let (prelude, intervals, opaque, tail) = split_body(&block).unwrap();
        assert_eq!(prelude.len(), 1);
        assert!(intervals.is_empty());
        assert_eq!(opaque.len(), 2);
        assert!(locate_html_macro(tail).is_ok());
    }

    #[test]
    fn explicit_return_can_wrap_the_tail_render_site() {
        let block = parse_block("{ return html_sync! { <span>\"ok\"</span> }.render(cx); }");
        let (_, _, opaque, tail) = split_body(&block).unwrap();
        assert!(opaque.is_empty());
        let html = locate_html_macro(tail).unwrap();
        validate_html_macro_for_render_kind(html, RenderKind::Sync).unwrap();
    }

    #[test]
    fn async_fn_render_has_guided_rejection() {
        let signature: Signature = syn::parse_quote! {
            async fn render(
                self,
                cx: &mut WebContext<L, B>,
            ) -> Result<(), WriteError>
        };
        let error = validate_render_signature(&signature).unwrap_err();
        assert!(error
            .to_string()
            .contains("async fn render` sugar has been removed"));
    }

    #[test]
    fn attr_args_parse_interaction_event_strategy() {
        let attr: proc_macro2::TokenStream = r#"strategy = "interaction:click""#.parse().unwrap();
        let parsed = parse_attr_args(attr).expect("attr args should parse");
        assert_eq!(
            parsed.hydration_strategy,
            HydrationStrategy::Interaction {
                event: Some("click".to_string())
            }
        );
    }

    #[test]
    fn attr_args_reject_empty_media_strategy() {
        let attr: proc_macro2::TokenStream = r#"strategy = "media:""#.parse().unwrap();
        let err = parse_attr_args(attr).expect_err("empty media strategy must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("media strategies"), "got: {msg}");
    }

    #[test]
    fn attr_args_reject_client_render_mode_with_policy_message() {
        let attr: proc_macro2::TokenStream = r#"render = "client""#.parse().unwrap();
        let err = parse_attr_args(attr).expect_err("client-only render must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("reserved but not supported yet"), "got: {msg}");
        assert!(msg.contains("fallback policy"), "got: {msg}");
    }

    #[test]
    fn attr_args_reject_client_only_alias() {
        let attr: proc_macro2::TokenStream = "client_only".parse().unwrap();
        let err = parse_attr_args(attr).expect_err("client_only alias must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("reserved but not supported yet"), "got: {msg}");
        assert!(msg.contains("fallback/no-JS"), "got: {msg}");
    }

    #[test]
    fn attr_args_reject_future_fallback_policy_without_client_only() {
        let attr: proc_macro2::TokenStream = r#"fallback = "empty""#.parse().unwrap();
        let err = parse_attr_args(attr).expect_err("fallback attr must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("reserved for future client-only"),
            "got: {msg}"
        );
    }
}
