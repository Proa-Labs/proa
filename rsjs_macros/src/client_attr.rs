// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stage M7d: the `#[rsjs::client]` attribute macro for free-function
//! client helpers.
//!
//! This module is the implementation behind
//! [`crate::client`]. It is intentionally independent of
//! [`crate::parse`] / [`crate::analyze`] (which target
//! `#[rsjs(component...)]` render-trait impls): client helpers are free
//! functions and
//! their constraint surface is the JS-lowerable `Expr` subset, not a
//! lifted `html!` parse tree.
//!
//! # Surface
//!
//! ```ignore
//! #[rsjs::client]
//! pub fn subtotal(price: i64, qty: i64) -> i64 {
//!     price * qty
//! }
//! ```
//!
//! Constraints (rejection categories — each pointed at the offending
//! span):
//!
//! 1. The function must be an ordinary synchronous Rust-ABI function
//!    (`const`, `async`, `unsafe`, `extern`, and variadic functions are
//!    rejected rather than silently changing their contract).
//! 2. The function must have no type or const generic parameters.
//! 3. Each parameter type must be a v0.1 JSON-shape primitive
//!    (`bool`, `i32`/`i64`/`u32`/`u64`, `f64`, `String`,
//!    `&str`) plus `Option<T>`, `Vec<T>`, and `&[T]` around helper-
//!    supported values. M7f's `#[derive(rsjs::ClientProps)]` extends
//!    this set; until then the attribute rejects unknown types here.
//! 4. The return type uses the owned subset of the same set: borrowed
//!    strings and slices are parameter-only.
//! 5. The body may be a single tail expression, or a statement block
//!    containing `let` bindings, assignment to mutable scalar locals,
//!    helper-local named struct value objects, field assignment on mutable
//!    object locals, `if`-guarded early `return` statements, and a final tail
//!    expression. Arbitrary statements are still outside the helper surface.
//! 6. The body cannot reference `self`, `cx`, the `event`
//!    argument, or any identifier other than the function's own
//!    parameters, immutable helper locals, and primitive literals.
//! 7. Closures, `.await`, and `async` blocks are rejected.
//! 8. Method calls are restricted to the v0.1 helper-extension
//!    surface: `.is_non_empty()` lowers to
//!    [`rsjs_ir::ClientExpr::IsNonEmpty`], `.is_empty()` lowers to
//!    `!IsNonEmpty`, `.len()` lowers to a Client IR runtime call,
//!    `.to_lowercase()` / `.to_uppercase()` / `.trim()` /
//!    `.contains(...)` lower to shared Client IR string nodes,
//!    `.to_string()` is an identity on an already-proven string
//!    result, and `.is_some()` / `.is_none()` lower to null
//!    comparisons for `Option<T>`.
//!    Every other method call raises a span-pointed error
//!    pointing at the method name.
//! 9. Unary `!` and unary `-` lower to Client IR v3 unary operators.
//! 10. Value-level `if` / `else` expressions are accepted when both
//!     branches are single-expression blocks.
//! 11. Tail `match` expressions over literals and bounded ranges lower
//!     to Client IR `let` + nested `if` / `return` blocks when they have
//!     a final `_` fallback.
//! 12. Free function calls inside the body record the callee path
//!     and arg expressions as a `ClientCall` node in the helper's
//!     body JSON. The bundling layer resolves same-bundle helper
//!     calls into exported JS function calls, and the generated
//!     server evaluator calls the corresponding Rust shim for SSR
//!     initial values.
//!
//! # Output shape
//!
//! For an input function `fn name(p1: T1, ...) -> R { body }` the
//! macro replaces the original item with:
//!
//! ```ignore
//! #[doc(hidden)]
//! #[allow(non_camel_case_types)]
//! pub struct __rsjs_client_<name>_meta;
//!
//! #[allow(non_upper_case_globals)]
//! impl __rsjs_client_<name>_meta {
//!     pub const NAME: &'static str = "<name>";
//!     pub const PARAMS: &'static [(&'static str, &'static str)] =
//!         &[("p1", "i64"), ...];
//!     pub const RETURN_TY: &'static str = "i64";
//!     pub const BODY_HIR_VERSION: u32 = 1;
//!     pub const BODY_HIR_JSON: &'static str = "<serialised helper HIR body>";
//!     pub const BODY_IR_VERSION: u32 = 3;
//!     pub const BODY_IR_JSON: &'static str = "<serialised Client IR v3 body>";
//!     pub fn eval_server(args: &[(&'static str, ::rsjs::__runtime::ClientArg)])
//!         -> ::rsjs::__runtime::ClientValue { ... }
//! }
//!
//! pub fn name(p1: T1, ...) -> R {
//!     ::rsjs::__runtime::run_client_helper::<__rsjs_client_<name>_meta, R>(
//!         "name",
//!         &[
//!             ("p1", ::rsjs::__runtime::ClientArg::from_value(&p1)),
//!             ...
//!         ],
//!     )
//! }
//! ```
//!
//! The generated function evaluates the helper body on the server
//! through the metadata evaluator. Client-module metadata writes bundle
//! JS from `BODY_IR_JSON`; `BODY_HIR_JSON` remains a direct const for
//! frontend snapshot coverage.
//!
//! # `BODY_HIR_JSON` schema
//!
//! `BODY_HIR_JSON` is a hand-emitted JSON document (no `serde`; the proc-
//! macro crate cannot pull in a heavy serialisation dep). Schema (v1):
//!
//! ```json
//! { "span": <SourceSpan>, "kind": "Lit",       "value": <Value> }
//! { "span": <SourceSpan>, "kind": "Param",     "index": <usize>, "name": "<ident>" }
//! { "span": <SourceSpan>, "kind": "Local",     "local": <u32>, "name": "<ident>" }
//! { "span": <SourceSpan>, "kind": "StructInit", "type_name": "<path>",
//!   "fields": [{ "name": "<field>", "value": <body> }, ...] }
//! { "span": <SourceSpan>, "kind": "Field", "base": <body>, "name": "<field>" }
//! { "span": <SourceSpan>, "kind": "BinaryOp",  "op": "Add"|"Sub"|"Mul"|"Div"|"Rem"|"Eq"|"Neq"
//!                              |"Lt"|"Gt"|"Le"|"Ge"|"And"|"Or",
//!   "lhs": <body>, "rhs": <body> }
//! { "span": <SourceSpan>, "kind": "IsNonEmpty","target": <body> }
//! { "span": <SourceSpan>, "kind": "StringTransform","op": "to_uppercase"|"trim", "target": <body> }
//! { "span": <SourceSpan>, "kind": "StringPredicate","op": "starts_with"|"ends_with",
//!   "haystack": <body>, "needle": <body> }
//! { "span": <SourceSpan>, "kind": "ClientCall","callee": "<path>", "args": [<body>, ...] }
//! { "kind": "Block",     "statements": [<stmt>, ...] }
//! { "span": <SourceSpan>, "kind": "Let",
//!   "local": <u32>, "name": "<ident>", "mutable": true?, "value": <body> }
//! { "span": <SourceSpan>, "kind": "AssignLocal",
//!   "local": <u32>, "value": <body> }
//! { "span": <SourceSpan>, "kind": "AssignField",
//!   "local": <u32>, "field": "<field>", "value": <body> }
//! { "span": <SourceSpan>, "kind": "Return", "value": <body>|null }
//! { "span": <SourceSpan>, "kind": "If",
//!   "condition": <body>, "then": <block>, "else": <block>|null }
//! ```
//!
//! `<SourceSpan>` stores concrete `start`/`end` line and column pairs from
//! the Rust source for helper HIR diagnostics.
//!
//! `<Value>` follows the same shape as
//! [`rsjs_macros::emit_rust::push_value_as_json`]:
//!
//! ```json
//! { "v": "Bool",   "data": true|false }
//! { "v": "Int",    "data": <Number-backed integer as JSON number> }
//! { "v": "BigInt", "data": "<canonical decimal integer>" }
//! { "v": "Float",  "data": <f64 as JSON number> }
//! { "v": "String", "data": "..." }
//! { "v": "Null" }
//! ```
//!
//! `BODY_HIR_JSON` is the frontend helper HIR snapshot shape. Bundle
//! emission and runtime metadata use `BODY_IR_JSON`, a syn-free Client
//! IR v3-compatible shape that uses lower-case node names (`literal`,
//! `local`, `binary`, `block`, `let`, `if`, `return`, `is_non_empty`,
//! `call`) and keeps unresolved helper calls explicit until module graph
//! lowering resolves them.

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION;
#[cfg(test)]
use rsjs_ir::ClientEmitContext;
use rsjs_ir::{
    client_block_json, client_expr_json, CheckedIntegerOp, ClientBinaryOp, ClientBlock, ClientExpr,
    ClientFunctionRef, ClientLocalId, ClientPlace, ClientStmt, ClientStructFieldValue, ClientType,
    ClientUnaryOp, IntegerDomain, StringPredicateOp, StringTransformOp, Value,
};
use sha2::{Digest, Sha256};
use syn::spanned::Spanned;

/// Entry point invoked from the `#[proc_macro_attribute]` shim.
///
/// `attr` is empty for ordinary helpers/value-object modules and `class` for
/// the explicit exported ESM class wrapper.
pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let export_class = if attr.is_empty() {
        false
    } else {
        let Ok(mode) = syn::parse2::<Ident>(attr.clone()) else {
            return error_at(
                attr.span(),
                "unsupported `#[rsjs::client(...)]` mode",
                "use `#[rsjs::client]` for a helper or closed object module, or `#[rsjs::client(class)]` for an exported JavaScript class module",
                "class export is the only argument-bearing client-helper mode",
            );
        };
        if mode != "class" {
            return error_at(
                mode.span(),
                "unsupported `#[rsjs::client(...)]` mode",
                "use `#[rsjs::client]` or `#[rsjs::client(class)]`",
                "class export is the only argument-bearing client-helper mode",
            );
        }
        true
    };

    let func: syn::ItemFn = match syn::parse2(item.clone()) {
        Ok(f) if !export_class => f,
        Ok(f) => {
            return error_at(
                f.sig.ident.span(),
                "`#[rsjs::client(class)]` must annotate an inline module",
                "place the named-field struct, inherent impl, and optional scalar helper roots inside `pub mod name { ... }`",
                "one class module can export one or more public Rust structs as JavaScript classes",
            );
        }
        Err(function_error) => match syn::parse2::<syn::ItemMod>(item) {
            Ok(module) => {
                return crate::client_object_module_attr::expand(module, export_class);
            }
            Err(_) => return function_error.to_compile_error(),
        },
    };

    match try_expand(&func) {
        Ok(ts) => ts,
        Err(diag) => diag.to_tokens(),
    }
}

fn try_expand(func: &syn::ItemFn) -> Result<TokenStream, Diagnostic> {
    try_expand_with_options(func, HelperExpansionOptions::default())
}

/// Closed value-object schema used only by the inline-module frontend.
///
/// The public `#[rsjs::client] fn` surface never receives this option, so the
/// reserved carrier cannot be authored as a standalone helper boundary.
#[derive(Clone, Debug)]
pub(crate) struct InternalValueObjectSpec {
    pub(crate) type_key: String,
    pub(crate) fields: Vec<(String, String)>,
    pub(crate) rust_type_names: Vec<String>,
}

#[derive(Clone, Copy)]
struct HelperExpansionOptions<'a> {
    internal_object: Option<&'a InternalValueObjectSpec>,
    emit_source_check: bool,
}

impl HelperExpansionOptions<'_> {
    fn ordinary() -> Self {
        Self {
            internal_object: None,
            emit_source_check: true,
        }
    }
}

impl Default for HelperExpansionOptions<'_> {
    fn default() -> Self {
        Self::ordinary()
    }
}

/// Expand a rewritten public value-object root. Its original Rust body is
/// emitted separately by the module frontend as the rustc source witness, so
/// this lowered body intentionally skips the ordinary nested source checker.
pub(crate) fn expand_rewritten_object_root(func: &syn::ItemFn) -> Result<TokenStream, TokenStream> {
    try_expand_with_options(
        func,
        HelperExpansionOptions {
            internal_object: None,
            emit_source_check: false,
        },
    )
    .map_err(|diagnostic| diagnostic.to_tokens())
}

/// Expand one private constructor/receiver helper for the closed object
/// module. Object parameters and returns use an opaque Rust carrier while the
/// descriptor signature records the nominal schema.
pub(crate) fn expand_internal_value_object_helper(
    func: &syn::ItemFn,
    object: &InternalValueObjectSpec,
) -> Result<TokenStream, TokenStream> {
    try_expand_with_options(
        func,
        HelperExpansionOptions {
            internal_object: Some(object),
            emit_source_check: false,
        },
    )
    .map_err(|diagnostic| diagnostic.to_tokens())
}

/// Syn-free body material used to build an exported JavaScript class module
/// from the exact same internal object helper lowering as linked islands.
pub(crate) struct InternalValueObjectClientIr {
    pub(crate) params: Vec<(String, String)>,
    pub(crate) return_type: String,
    pub(crate) body_ir_json: String,
    pub(crate) direct_calls: Vec<String>,
}

pub(crate) fn lower_internal_value_object_client_ir(
    func: &syn::ItemFn,
    object: &InternalValueObjectSpec,
) -> Result<InternalValueObjectClientIr, TokenStream> {
    let lowered = lower_helper_with_options(func, Some(object))
        .map_err(|diagnostic| diagnostic.to_tokens())?;
    Ok(InternalValueObjectClientIr {
        params: lowered
            .params
            .iter()
            .map(|param| (param.name.to_string(), param.ty_name.clone()))
            .collect(),
        return_type: lowered.ret_ty_name.clone(),
        body_ir_json: lowered.body.to_ir_json(),
        direct_calls: lowered
            .body
            .direct_client_calls()
            .into_iter()
            .map(|call| call.authored_path)
            .collect(),
    })
}

fn try_expand_with_options(
    func: &syn::ItemFn,
    options: HelperExpansionOptions<'_>,
) -> Result<TokenStream, Diagnostic> {
    let sig = &func.sig;
    let lowered = match options.internal_object {
        Some(object) => lower_helper_with_options(func, Some(object))?,
        None => lower_helper(func)?,
    };
    let cargo_unit_id = crate::descriptor_cargo_unit_id().map_err(|message| {
        Diagnostic::new(
            Span::call_site(),
            &message,
            "invoke descriptor compilation through `cargo proa`; ordinary Cargo checking does not require this environment",
            "descriptor records must carry the exact Cargo-unit identity assigned by the orchestrator",
        )
    })?;

    // ---------------- emit ----------------
    let fn_name = sig.ident.clone();
    let fn_name_str = fn_name.to_string();
    // Round-3 review #7: `format_ident!` panics if the interpolated
    // ident's `Display` form contains characters invalid in an ident
    // (e.g. a raw `r#fn` -> `__rsjs_client_r#fn_meta`). Reject raw
    // idents up front with a clean diagnostic instead.
    if fn_name_str.starts_with("r#") {
        return Err(Diagnostic::new(
            fn_name.span(),
            "`#[rsjs::client]` helper names cannot be raw identifiers",
            "rename the function to a non-keyword identifier (e.g. `to_fn` instead of `r#fn`)",
            "the macro derives `__rsjs_client_<name>_meta` from the helper's name; raw \
             identifiers cannot be used as part of a generated ident",
        ));
    }
    let vis = &func.vis;
    let meta_ident = format_ident!("__rsjs_client_{}_meta", fn_name);
    let definition_ident = format_ident!("__RSJS_CLIENT_HELPER_DEFINITION_{}", fn_name);
    let identity_text_ident = format_ident!("__RSJS_CLIENT_HELPER_IDENTITY_TEXT_{}", fn_name);
    let identity_ident = format_ident!("__RSJS_CLIENT_HELPER_IDENTITY_{}", fn_name);
    let source_check = if options.emit_source_check {
        let source_check_ident = format_ident!("__rsjs_client_{}_source_typecheck", fn_name);
        let mut source_check_sig = sig.clone();
        source_check_sig.ident = source_check_ident;
        let source_check_body = &func.block;
        quote! {
            #[allow(dead_code)]
            #source_check_sig #source_check_body
        }
    } else {
        TokenStream::new()
    };

    let params = &lowered.params;
    let param_idents: Vec<_> = params.iter().map(|p| p.name.clone()).collect();
    let param_tys: Vec<_> = params.iter().map(|p| p.ty_tokens.clone()).collect();
    let param_names: Vec<&str> = params.iter().map(|p| p.name_str()).collect();
    let param_ty_names: Vec<&str> = params.iter().map(|p| p.ty_name.as_str()).collect();

    let body_hir_json = lowered.body.to_json();
    let body_ir_json = lowered.body.to_ir_json();
    let body_hir_json_lit = syn::LitStr::new(&body_hir_json, Span::call_site());
    let body_ir_json_lit = syn::LitStr::new(&body_ir_json, Span::call_site());
    let body_ir_version = proc_macro2::Literal::u32_unsuffixed(CLIENT_IR_SCHEMA_VERSION);
    let body_server_eval = lowered.body.to_server_eval_tokens();
    let fn_name_lit = syn::LitStr::new(&fn_name_str, Span::call_site());
    let ret_ty_lit = syn::LitStr::new(&lowered.ret_ty_name, Span::call_site());
    let ret_ty_tokens = &lowered.ret_ty_tokens;
    let helper_record_id = helper_record_id(func);
    let final_plan_selection = crate::final_plan::validate_current_definition(
        crate::final_plan::DefinitionKind::Helper,
        cargo_unit_id,
        helper_record_id,
    )
    .map_err(|message| {
        Diagnostic::new(
            Span::call_site(),
            &message,
            "run the build through `cargo proa build` and regenerate the exact linked plan",
            "final helper expansion is selected only by its descriptor Cargo-unit and relocation-backed definition record",
        )
    })?;
    let final_plan_witness = crate::final_plan::witness_tokens(
        &format!("helper_{}", fn_name_str),
        final_plan_selection.as_ref(),
    );
    let helper_record_id_tokens = helper_record_id
        .iter()
        .map(|byte| proc_macro2::Literal::u8_unsuffixed(*byte))
        .collect::<Vec<_>>();
    let cargo_unit_id_tokens = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let direct_calls = lowered.body.direct_client_calls();
    let helper_payload_json = standalone_helper_payload_json(
        func,
        &lowered,
        &body_hir_json,
        &body_ir_json,
        helper_record_id,
        &direct_calls,
    )?;
    let helper_payload_tokens = emit_helper_payload_record(
        &fn_name,
        cargo_unit_id,
        helper_record_id,
        helper_payload_json.as_bytes(),
    );
    let helper_edge_tokens =
        emit_helper_direct_call_edges(&fn_name, cargo_unit_id, helper_record_id, &direct_calls);

    let attrs = &func.attrs;

    let expanded = quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #vis struct #meta_ident;

        #[doc(hidden)]
        #[allow(non_upper_case_globals, dead_code)]
        impl #meta_ident {
            pub const NAME: &'static str = #fn_name_lit;
            pub const PARAMS: &'static [(&'static str, &'static str)] =
                &[ #( (#param_names, #param_ty_names) ),* ];
            pub const RETURN_TY: &'static str = #ret_ty_lit;
            pub const BODY_HIR_VERSION: u32 = 1;
            pub const BODY_HIR_JSON: &'static str = #body_hir_json_lit;
            pub const BODY_IR_VERSION: u32 = #body_ir_version;
            pub const BODY_IR_JSON: &'static str = #body_ir_json_lit;

            pub fn eval_server(
                __rsjs_args: &[(&'static str, ::rsjs::__runtime::ClientArg)],
            ) -> ::rsjs::__runtime::ClientValue {
                #body_server_eval
            }

            /// `&'static ClientHelperMeta` view of this helper's
            /// per-fn consts. M7e's `#[rsjs::client_module]` attribute
            /// collects these references into a bundle so the bundle
            /// metadata is a single `&[&ClientHelperMeta]` slice with
            /// no redeclaration of the underlying data.
            pub const META: &'static ::rsjs::ClientHelperMeta = &::rsjs::ClientHelperMeta {
                name: Self::NAME,
                params: Self::PARAMS,
                return_ty: Self::RETURN_TY,
                body_ir_version: Self::BODY_IR_VERSION,
                body_ir_json: Self::BODY_IR_JSON,
                server_eval: Self::eval_server,
            };
        }

        impl ::rsjs::__runtime::ClientHelperRuntimeMeta for #meta_ident {
            const META: &'static ::rsjs::ClientHelperMeta = #meta_ident::META;
        }

        #( #attrs )*
        #vis fn #fn_name( #( #param_idents : #param_tys ),* ) -> #ret_ty_tokens {
            #source_check

            ::rsjs::__runtime::run_client_helper::<
                #meta_ident,
                #ret_ty_tokens,
            >(
                #fn_name_lit,
                &[ #( ( #param_names, ::rsjs::__runtime::ClientArg::from_value(&#param_idents) ) ),* ],
            )
        }

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_help")
        )]
        #[cfg_attr(
            any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox"
            ),
            unsafe(link_section = ".rsjs.helper")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$H"))]
        static #definition_ident: ::rsjs::__private::HelperDefinitionRecord =
            ::rsjs::__private::HelperDefinitionRecord::new(
                [#(#cargo_unit_id_tokens),*],
                [#(#helper_record_id_tokens),*],
                #fn_name as *const (),
            );

        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const #identity_text_ident: &str = concat!(
            env!("CARGO_PKG_NAME"),
            "\0",
            env!("CARGO_CRATE_NAME"),
            "\0",
            module_path!(),
            "\0",
            stringify!(#fn_name),
        );

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_ident")
        )]
        #[cfg_attr(
            any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox"
            ),
            unsafe(link_section = ".rsjs.identity")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$S"))]
        static #identity_ident: ::rsjs::__private::SemanticIdentityRecord<
            { #identity_text_ident.len() },
        > = ::rsjs::__private::SemanticIdentityRecord::new(
            ::rsjs::__private::SemanticIdentityKind::Helper,
            [#(#cargo_unit_id_tokens),*],
            [#(#helper_record_id_tokens),*],
            ::rsjs::__private::encode_semantic_identity::<
                { #identity_text_ident.len() },
            >(#identity_text_ident),
        );

        #helper_payload_tokens
        #helper_edge_tokens
        #final_plan_witness

        #[cfg(not(any(
            target_vendor = "apple",
            target_os = "linux",
            target_os = "android",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku",
            target_os = "redox",
            target_os = "windows"
        )))]
        compile_error!(
            "RSJS descriptor relocation transport does not support this target object format"
        );
    };

    Ok(expanded)
}

fn helper_record_id(func: &syn::ItemFn) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.client-helper-record.v1\0");
    hasher.update(func.to_token_stream().to_string().as_bytes());
    hasher.finalize().into()
}

fn helper_direct_call_id(helper_record_id: [u8; 32], ordinal: usize) -> [u8; 32] {
    let ordinal = u32::try_from(ordinal)
        .expect("a standalone helper cannot contain more than u32::MAX direct calls");
    rsjs_transport::helper_direct_call_source_edge_id(helper_record_id, ordinal)
}

fn standalone_helper_payload_json(
    func: &syn::ItemFn,
    lowered: &LoweredHelper,
    body_hir_json: &str,
    body_ir_json: &str,
    helper_record_id: [u8; 32],
    direct_calls: &[DirectClientCall],
) -> Result<String, Diagnostic> {
    let parse_embedded_json = |label: &str, text: &str| {
        serde_json::from_str::<serde_json::Value>(text).map_err(|error| {
            Diagnostic::new(
                func.sig.ident.span(),
                &format!("failed to construct standalone helper descriptor: {label} is invalid"),
                "report this as an rsjs compiler bug",
                &error.to_string(),
            )
        })
    };
    let body_hir = parse_embedded_json("helper HIR", body_hir_json)?;
    let body_ir = parse_embedded_json("Client IR", body_ir_json)?;
    let params = lowered
        .params
        .iter()
        .map(|param| {
            serde_json::json!({
                "name": param.name_str(),
                "rust_type": param.ty_name,
            })
        })
        .collect::<Vec<_>>();
    let calls = direct_calls
        .iter()
        .enumerate()
        .map(|(ordinal, callee)| {
            serde_json::json!({
                "authored_path": callee.authored_path,
                "ordinal": ordinal,
                "source_edge_id": hex_id(&helper_direct_call_id(helper_record_id, ordinal)),
            })
        })
        .collect::<Vec<_>>();
    let defining_package =
        std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "<unknown-package>".to_string());
    let defining_crate =
        std::env::var("CARGO_CRATE_NAME").unwrap_or_else(|_| defining_package.replace('-', "_"));

    // The module portion of the semantic key is deliberately resolved later
    // from the defining Rust relocation. A proc macro cannot soundly recover
    // rustc's module identity from source path tokens.
    let mut payload = serde_json::json!({
        "body_hir": body_hir,
        "body_hir_version": 1,
        "body_ir": body_ir,
        "body_ir_version": CLIENT_IR_SCHEMA_VERSION,
        "codec_abi_version": 1,
        "defining_record_id": hex_id(&helper_record_id),
        "direct_calls": calls,
        "effect_class": "origin_free",
        "format": "rsjs.standalone_helper_descriptor",
        "provisional_identity": {
            "crate": defining_crate,
            "item": func.sig.ident.to_string(),
            "key_schema_version": 1,
            "module_identity": "rustc_defining_relocation",
            "package": defining_package,
        },
        "schema_version": 1,
        "signature": {
            "params": params,
            "return_type": lowered.ret_ty_name,
        },
    });
    let semantic_bytes = rsjs_transport::canonical_json_bytes(&payload).map_err(|error| {
        Diagnostic::new(
            func.sig.ident.span(),
            "failed to serialize standalone helper descriptor",
            "report this as an rsjs compiler bug",
            &error.to_string(),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.standalone-helper.semantic.v1\0");
    hasher.update(&semantic_bytes);
    let semantic_hash: [u8; 32] = hasher.finalize().into();
    payload
        .as_object_mut()
        .expect("helper descriptor is an object")
        .insert(
            "semantic_hash".to_string(),
            serde_json::Value::String(format!("sha256:{}", hex_id(&semantic_hash))),
        );
    let payload_bytes = rsjs_transport::canonical_json_bytes(&payload).map_err(|error| {
        Diagnostic::new(
            func.sig.ident.span(),
            "failed to serialize standalone helper descriptor",
            "report this as an rsjs compiler bug",
            &error.to_string(),
        )
    })?;
    Ok(String::from_utf8(payload_bytes)
        .expect("canonical JSON serialization always produces valid UTF-8"))
}

fn emit_helper_payload_record(
    fn_name: &Ident,
    cargo_unit_id: [u8; 32],
    helper_record_id: [u8; 32],
    payload_json: &[u8],
) -> TokenStream {
    let encoded = rsjs_transport::encode_payload_record(
        rsjs_transport::PayloadKind::StandaloneHelperDescriptor,
        cargo_unit_id,
        helper_record_id,
        payload_json,
    );
    let payload_len = syn::LitInt::new(&encoded.len().to_string(), Span::call_site());
    let payload = syn::LitByteStr::new(&encoded, Span::call_site());
    let payload_ident = format_ident!("__RSJS_CLIENT_HELPER_PAYLOAD_{}", fn_name);

    quote! {
        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_hpay")
        )]
        #[cfg_attr(
            any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox"
            ),
            unsafe(link_section = ".rsjs.helper_payload")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$Q"))]
        static #payload_ident: ::rsjs::__private::PayloadRecord<#payload_len> =
            ::rsjs::__private::PayloadRecord::new(*#payload);
    }
}

fn emit_helper_direct_call_edges(
    fn_name: &Ident,
    cargo_unit_id: [u8; 32],
    helper_record_id: [u8; 32],
    direct_calls: &[DirectClientCall],
) -> TokenStream {
    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let mut records = Vec::with_capacity(direct_calls.len());
    for (ordinal, callee) in direct_calls.iter().enumerate() {
        // This must be the original parsed path, not a string round-trip.
        // The relocation is rustc's authority for helper identity, so losing
        // an absolute-path marker or identifier hygiene here could resolve a
        // different item than the source expression that the hidden checker
        // typechecked.
        let callee_path = &callee.path;
        let record_ident = format_ident!("__RSJS_CLIENT_HELPER_EDGE_{}_{}", fn_name, ordinal);
        let source_edge_id = helper_direct_call_id(helper_record_id, ordinal)
            .into_iter()
            .map(proc_macro2::Literal::u8_unsuffixed)
            .collect::<Vec<_>>();
        records.push(quote! {
            ::rsjs::__private::__rsjs_emit_client_helper_edge!(
                #record_ident,
                [#(#cargo_unit_id),*],
                [#(#source_edge_id),*],
                #callee_path
            );
        });
    }
    quote! { #(#records)* }
}

fn hex_id(id: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(64);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// Type recognition
// ---------------------------------------------------------------------------

/// Recognise a v0.1 primitive type. Returns the canonical name we
/// emit into `PARAMS` and `RETURN_TY`, or `None` if the type is not
/// in the v0.1 set. (M7f extends this surface for ClientProps-derived
/// structs; that resolution is deferred to the bundling pass.)
///
/// Path validation: a primitive must be either a single ident
/// (`String`, `bool`) or rooted at `core` / `std` / `alloc` / `::`.
/// Anything else (e.g. `my_crate::String`) returns `None` so the
/// downstream JSON wire format does not silently misencode a
/// shadowed-name custom type.
fn recognise_primitive_ty(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) if p.qself.is_none() => {
            if !is_std_rooted_path(&p.path) {
                return None;
            }
            let ident = p.path.segments.last()?.ident.to_string();
            match ident.as_str() {
                "bool" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32"
                | "u64" | "u128" | "usize" | "f64" | "String" => Some(ident),
                "Option" => recognise_option_ty(&p.path),
                "Vec" => recognise_vec_ty(&p.path),
                _ => None,
            }
        }
        syn::Type::Reference(r) if r.mutability.is_none() => {
            if let syn::Type::Path(p) = &*r.elem {
                if p.qself.is_none()
                    && is_std_rooted_path(&p.path)
                    && p.path
                        .segments
                        .last()
                        .map(|s| s.ident == "str")
                        .unwrap_or(false)
                {
                    return Some("&str".to_string());
                }
            }
            if let syn::Type::Slice(slice) = &*r.elem {
                let inner = recognise_primitive_ty(&slice.elem)?;
                return Some(format!("&[{inner}]"));
            }
            None
        }
        syn::Type::Tuple(t) if t.elems.is_empty() => Some("()".to_string()),
        _ => None,
    }
}

fn is_unsupported_checked_integer_name(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "u8" | "u16" | "i128" | "u128" | "isize" | "usize"
    )
}

/// Find an unsupported fixed-width integer anywhere inside a helper-visible
/// type. Container wrappers do not erase Rust's element semantics: accepting
/// `Option<u8>`, `Vec<u16>`, or `&[i128]` would let an unsupported domain cross
/// into client-executed code even though the scalar form is rejected.
fn unsupported_helper_integer_leaf(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() => {
            let segment = path.path.segments.last()?;
            let name = segment.ident.to_string();
            if is_unsupported_checked_integer_name(&name) {
                return Some(name);
            }
            let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return None;
            };
            arguments.args.iter().find_map(|argument| match argument {
                syn::GenericArgument::Type(inner) => unsupported_helper_integer_leaf(inner),
                _ => None,
            })
        }
        syn::Type::Reference(reference) => unsupported_helper_integer_leaf(&reference.elem),
        syn::Type::Slice(slice) => unsupported_helper_integer_leaf(&slice.elem),
        syn::Type::Array(array) => unsupported_helper_integer_leaf(&array.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().find_map(unsupported_helper_integer_leaf),
        syn::Type::Paren(paren) => unsupported_helper_integer_leaf(&paren.elem),
        syn::Type::Group(group) => unsupported_helper_integer_leaf(&group.elem),
        _ => None,
    }
}

/// Whether an `f32` leaf appears anywhere inside a helper-visible type.
///
/// JavaScript Number and the current Client IR preserve f64 arithmetic, not
/// Rust's required per-operation f32 rounding. A wrapper must not make that
/// semantic distinction disappear.
fn helper_type_contains_f32(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() => {
            let Some(segment) = path.path.segments.last() else {
                return false;
            };
            if segment.ident == "f32" {
                return true;
            }
            let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return false;
            };
            arguments.args.iter().any(|argument| {
                matches!(argument, syn::GenericArgument::Type(inner) if helper_type_contains_f32(inner))
            })
        }
        syn::Type::Reference(reference) => helper_type_contains_f32(&reference.elem),
        syn::Type::Slice(slice) => helper_type_contains_f32(&slice.elem),
        syn::Type::Array(array) => helper_type_contains_f32(&array.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(helper_type_contains_f32),
        syn::Type::Paren(paren) => helper_type_contains_f32(&paren.elem),
        syn::Type::Group(group) => helper_type_contains_f32(&group.elem),
        _ => false,
    }
}

/// Whether a borrowed `str` leaf appears anywhere inside a return type.
fn helper_type_contains_borrowed_str(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(reference) => {
            let is_str = matches!(
                &*reference.elem,
                syn::Type::Path(path)
                    if path.qself.is_none()
                        && is_std_rooted_path(&path.path)
                        && path.path.segments.last().is_some_and(|segment| segment.ident == "str")
            );
            is_str || helper_type_contains_borrowed_str(&reference.elem)
        }
        syn::Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .and_then(|segment| match &segment.arguments {
                syn::PathArguments::AngleBracketed(arguments) => Some(&arguments.args),
                _ => None,
            })
            .is_some_and(|arguments| {
                arguments.iter().any(|argument| {
                    matches!(argument, syn::GenericArgument::Type(inner) if helper_type_contains_borrowed_str(inner))
                })
            }),
        syn::Type::Slice(slice) => helper_type_contains_borrowed_str(&slice.elem),
        syn::Type::Array(array) => helper_type_contains_borrowed_str(&array.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(helper_type_contains_borrowed_str),
        syn::Type::Paren(paren) => helper_type_contains_borrowed_str(&paren.elem),
        syn::Type::Group(group) => helper_type_contains_borrowed_str(&group.elem),
        _ => false,
    }
}

fn unsupported_helper_integer_domain(span: Span, name: &str) -> Diagnostic {
    Diagnostic::new(
        span,
        &format!(
            "rsjs/integer-domain-unsupported: `{name}` is not supported in \
             client-executed `#[rsjs::client]` value types or checked arithmetic"
        ),
        "use `i32`, `u32`, `i64`, or `u64`, or keep this operation in server Rust",
        "the helper compiler must carry the exact Rust overflow domain into JavaScript; \
         putting an unsupported integer inside Option, Vec, a slice, or another wrapper \
         does not erase its overflow and representation semantics",
    )
}

fn unsupported_helper_f32_domain(span: Span) -> Diagnostic {
    Diagnostic::new(
        span,
        "rsjs/float-domain-unsupported: `f32` is not supported in client-executed `#[rsjs::client]` signatures",
        "use `f64`, or keep f32-sensitive arithmetic in server Rust",
        "the current helper IR and JavaScript Number operations carry f64 semantics; Rust requires f32 rounding after each operation (which diverges at boundaries such as 2^24), so accepting f32 would silently miscompile the helper",
    )
}

fn unsupported_helper_borrowed_return(span: Span) -> Diagnostic {
    Diagnostic::new(
        span,
        "rsjs/borrowed-return-unsupported: standalone client helpers cannot return `&str`",
        "return an owned `String` instead; `&str` remains supported for parameters",
        "the helper server evaluator owns its decoded string value and Client IR carries no Rust borrow provenance; fabricating a returned reference would require leaking that allocation",
    )
}

fn helper_integer_domain_mismatch(
    span: Span,
    actual: IntegerDomain,
    expected: IntegerDomain,
) -> Diagnostic {
    Diagnostic::new(
        span,
        &format!(
            "rsjs/integer-domain-mismatch: helper expression mixes `{}` and `{}`",
            actual.as_str(),
            expected.as_str()
        ),
        "use the same supported integer type on both sides",
        "the helper macro replaces the original body, so it validates Rust's exact integer \
         typing before emitting checked JavaScript",
    )
}

fn recognise_option_ty(path: &syn::Path) -> Option<String> {
    let option = path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &option.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    let inner = recognise_primitive_ty(inner)?;
    if inner.starts_with("Option<") {
        return None;
    }
    Some(format!("Option<{inner}>"))
}

fn recognise_vec_ty(path: &syn::Path) -> Option<String> {
    let vec = path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &vec.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    let inner = recognise_primitive_ty(inner)?;
    Some(format!("Vec<{inner}>"))
}

fn helper_ty_tokens(ty_name: &str, original: &syn::Type) -> TokenStream {
    helper_canonical_ty_tokens(ty_name).unwrap_or_else(|| original.to_token_stream())
}

fn helper_canonical_ty_tokens(ty_name: &str) -> Option<TokenStream> {
    if let Some(inner) = ty_name
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let inner = helper_canonical_ty_tokens(inner)?;
        return Some(quote!(::core::option::Option<#inner>));
    }
    if let Some(inner) = ty_name
        .strip_prefix("Vec<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let inner = helper_canonical_ty_tokens(inner)?;
        return Some(quote!(::std::vec::Vec<#inner>));
    }
    if let Some(inner) = ty_name.strip_prefix("&[").and_then(|s| s.strip_suffix(']')) {
        let inner = helper_canonical_ty_tokens(inner)?;
        return Some(quote!(&[#inner]));
    }
    Some(match ty_name {
        "bool" => quote!(bool),
        "i8" => quote!(i8),
        "i16" => quote!(i16),
        "i32" => quote!(i32),
        "i64" => quote!(i64),
        "i128" => quote!(i128),
        "isize" => quote!(isize),
        "u8" => quote!(u8),
        "u16" => quote!(u16),
        "u32" => quote!(u32),
        "u64" => quote!(u64),
        "u128" => quote!(u128),
        "usize" => quote!(usize),
        "f64" => quote!(f64),
        "String" => quote!(::std::string::String),
        "&str" => quote!(&str),
        "()" => quote!(()),
        _ => return None,
    })
}

/// Whether `path` is acceptable as a primitive-type spelling: either
/// a bare single-segment ident or a path whose leading segment is
/// `core`, `std`, or `alloc` (or starts with `::` so the leading
/// segment is one of those crates by absolute path).
///
/// The check rejects shadowed-primitive paths like `my_crate::String`,
/// which the v0.1 wire format would otherwise mis-encode. The leading-
/// `::` form is preserved; `syn::Path::leading_colon` flags it
/// without touching the segment list.
pub(crate) fn is_std_rooted_path(path: &syn::Path) -> bool {
    if path.segments.len() == 1 {
        return true;
    }
    let first = match path.segments.first() {
        Some(s) => &s.ident,
        None => return false,
    };
    first == "core" || first == "std" || first == "alloc"
}

// ---------------------------------------------------------------------------
// Body lowering
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ParamInfo {
    name: syn::Ident,
    ty_name: String,
    ty_tokens: TokenStream,
    index: usize,
    integer_domain: Option<IntegerDomain>,
    object_shape: Option<ObjectShape>,
}

impl ParamInfo {
    fn name_str(&self) -> &'static str {
        // Leak the name into a 'static string; this happens at most
        // once per parameter at compile time and the proc-macro
        // process is short-lived.
        Box::leak(self.name.to_string().into_boxed_str())
    }
}

struct BodyLowerer<'a> {
    params: &'a [ParamInfo],
    locals: Vec<LocalInfo>,
    generated_local_counter: u32,
    return_integer_domain: Option<IntegerDomain>,
    internal_object: Option<&'a InternalValueObjectSpec>,
}

struct LocalInfo {
    name: syn::Ident,
    id: u32,
    ty: HelperValueType,
    constant: Option<LiteralValue>,
    mutable: bool,
    object_shape: Option<ObjectShape>,
    visible: bool,
}

#[derive(Debug, Clone)]
struct ObjectShape {
    fields: Vec<(String, HelperValueType)>,
}

struct LoweredHelper {
    params: Vec<ParamInfo>,
    ret_ty_name: String,
    ret_ty_tokens: TokenStream,
    body: HelperBody,
}

#[derive(Debug)]
enum HelperBody {
    Expr(BodyNode),
    Block(BodyBlock),
}

#[derive(Debug)]
struct BodyNode {
    span: Span,
    kind: BodyNodeKind,
}

#[derive(Debug)]
enum BodyNodeKind {
    Literal(LiteralValue),
    Param {
        index: usize,
        name: String,
    },
    Local {
        id: u32,
        name: String,
    },
    StructInit {
        type_name: String,
        fields: Vec<(String, BodyNode)>,
    },
    Field {
        base: Box<BodyNode>,
        name: String,
    },
    Binary {
        op: &'static str,
        lhs: Box<BodyNode>,
        rhs: Box<BodyNode>,
    },
    Unary {
        op: &'static str,
        expr: Box<BodyNode>,
    },
    Conditional {
        condition: Box<BodyNode>,
        then_expr: Box<BodyNode>,
        else_expr: Box<BodyNode>,
    },
    IsNonEmpty(Box<BodyNode>),
    Len(Box<BodyNode>),
    StringToLowercase(Box<BodyNode>),
    StringContains {
        haystack: Box<BodyNode>,
        needle: Box<BodyNode>,
    },
    StringTransform {
        op: StringTransformOp,
        target: Box<BodyNode>,
    },
    StringPredicate {
        op: StringPredicateOp,
        haystack: Box<BodyNode>,
        needle: Box<BodyNode>,
    },
    /// A free-function call we couldn't resolve at this milestone.
    /// M7e cross-references the manifest to confirm the callee is a
    /// peer `#[rsjs::client]` helper.
    ClientCall {
        callee: DirectClientCall,
        args: Vec<BodyNode>,
    },
}

/// One authored helper-to-helper call path.
///
/// `path` is the compiler-bearing representation: it retains the leading
/// `::`, spans, and macro hygiene that rustc must resolve. Explicit path
/// arguments are rejected before this representation is constructed.
/// `authored_path` is deliberately diagnostic/transport text only. Keeping
/// both makes it impossible for display formatting to become name-resolution
/// input later in the pipeline.
#[derive(Clone, Debug)]
struct DirectClientCall {
    path: syn::Path,
    authored_path: String,
}

impl BodyNode {
    fn new(span: Span, kind: BodyNodeKind) -> Self {
        Self { span, kind }
    }

    fn literal(span: Span, value: LiteralValue) -> Self {
        Self::new(span, BodyNodeKind::Literal(value))
    }

    fn param(span: Span, index: usize, name: String) -> Self {
        Self::new(span, BodyNodeKind::Param { index, name })
    }

    fn local(span: Span, id: u32, name: String) -> Self {
        Self::new(span, BodyNodeKind::Local { id, name })
    }

    fn binary(span: Span, op: &'static str, lhs: BodyNode, rhs: BodyNode) -> Self {
        Self::new(
            span,
            BodyNodeKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        )
    }

    fn unary(span: Span, op: &'static str, expr: BodyNode) -> Self {
        Self::new(
            span,
            BodyNodeKind::Unary {
                op,
                expr: Box::new(expr),
            },
        )
    }

    fn conditional(
        span: Span,
        condition: BodyNode,
        then_expr: BodyNode,
        else_expr: BodyNode,
    ) -> Self {
        Self::new(
            span,
            BodyNodeKind::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            },
        )
    }

    fn is_non_empty(span: Span, target: BodyNode) -> Self {
        Self::new(span, BodyNodeKind::IsNonEmpty(Box::new(target)))
    }

    fn len(span: Span, target: BodyNode) -> Self {
        Self::new(span, BodyNodeKind::Len(Box::new(target)))
    }

    fn string_to_lowercase(span: Span, target: BodyNode) -> Self {
        Self::new(span, BodyNodeKind::StringToLowercase(Box::new(target)))
    }

    fn string_transform(span: Span, op: StringTransformOp, target: BodyNode) -> Self {
        Self::new(
            span,
            BodyNodeKind::StringTransform {
                op,
                target: Box::new(target),
            },
        )
    }

    fn string_predicate(
        span: Span,
        op: StringPredicateOp,
        haystack: BodyNode,
        needle: BodyNode,
    ) -> Self {
        Self::new(
            span,
            BodyNodeKind::StringPredicate {
                op,
                haystack: Box::new(haystack),
                needle: Box::new(needle),
            },
        )
    }

    fn string_contains(span: Span, haystack: BodyNode, needle: BodyNode) -> Self {
        Self::new(
            span,
            BodyNodeKind::StringContains {
                haystack: Box::new(haystack),
                needle: Box::new(needle),
            },
        )
    }

    fn client_call(span: Span, callee: DirectClientCall, args: Vec<BodyNode>) -> Self {
        Self::new(span, BodyNodeKind::ClientCall { callee, args })
    }

    fn struct_init(span: Span, type_name: String, fields: Vec<(String, BodyNode)>) -> Self {
        Self::new(span, BodyNodeKind::StructInit { type_name, fields })
    }

    fn field(span: Span, base: BodyNode, name: String) -> Self {
        Self::new(
            span,
            BodyNodeKind::Field {
                base: Box::new(base),
                name,
            },
        )
    }

    fn collect_direct_client_calls(&self, calls: &mut Vec<DirectClientCall>) {
        match &self.kind {
            BodyNodeKind::Literal(_) | BodyNodeKind::Param { .. } | BodyNodeKind::Local { .. } => {}
            BodyNodeKind::StructInit { fields, .. } => {
                for (_, value) in fields {
                    value.collect_direct_client_calls(calls);
                }
            }
            BodyNodeKind::Field { base, .. } => base.collect_direct_client_calls(calls),
            BodyNodeKind::Binary { lhs, rhs, .. } => {
                lhs.collect_direct_client_calls(calls);
                rhs.collect_direct_client_calls(calls);
            }
            BodyNodeKind::Unary { expr, .. }
            | BodyNodeKind::IsNonEmpty(expr)
            | BodyNodeKind::Len(expr)
            | BodyNodeKind::StringToLowercase(expr)
            | BodyNodeKind::StringTransform { target: expr, .. } => {
                expr.collect_direct_client_calls(calls);
            }
            BodyNodeKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                condition.collect_direct_client_calls(calls);
                then_expr.collect_direct_client_calls(calls);
                else_expr.collect_direct_client_calls(calls);
            }
            BodyNodeKind::StringContains { haystack, needle }
            | BodyNodeKind::StringPredicate {
                haystack, needle, ..
            } => {
                haystack.collect_direct_client_calls(calls);
                needle.collect_direct_client_calls(calls);
            }
            BodyNodeKind::ClientCall { callee, args } => {
                calls.push(callee.clone());
                for arg in args {
                    arg.collect_direct_client_calls(calls);
                }
            }
        }
    }
}

#[derive(Debug)]
struct BodyBlock {
    stmts: Vec<BodyStmt>,
}

#[derive(Debug)]
enum BodyStmt {
    Let {
        span: Span,
        local: u32,
        name: String,
        mutable: bool,
        value: BodyNode,
    },
    AssignLocal {
        span: Span,
        local: u32,
        value: BodyNode,
    },
    AssignField {
        span: Span,
        local: u32,
        field: String,
        value: BodyNode,
    },
    Expr {
        span: Span,
        value: BodyNode,
    },
    Return {
        span: Span,
        value: Option<BodyNode>,
    },
    If {
        span: Span,
        condition: BodyNode,
        then_block: BodyBlock,
        else_block: Option<BodyBlock>,
    },
}

#[derive(Debug, Clone)]
enum LiteralValue {
    Null,
    Bool(bool),
    Integer {
        value: i128,
        /// `None` means an unsuffixed Rust literal whose domain may be
        /// inherited from a surrounding typed expression.
        domain: Option<IntegerDomain>,
    },
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperValueType {
    Bool,
    Integer(IntegerDomain),
    IntegerLiteral(Option<IntegerDomain>),
    IntegerUnproven,
    Float,
    String,
    Array,
    Option,
    Object,
    Unit,
    Unknown,
}

impl HelperValueType {
    fn from_type_name(name: &str) -> Self {
        if name == "bool" {
            Self::Bool
        } else if let Some(domain) = IntegerDomain::from_name(name) {
            Self::Integer(domain)
        } else if matches!(
            name,
            "i8" | "i16" | "u8" | "u16" | "i128" | "u128" | "isize" | "usize"
        ) {
            Self::IntegerUnproven
        } else if matches!(name, "f32" | "f64") {
            Self::Float
        } else if matches!(name, "String" | "&str") {
            Self::String
        } else if name.starts_with("Vec<") || name.starts_with("&[") {
            Self::Array
        } else if name.starts_with("Option<") {
            Self::Option
        } else if name == "()" {
            Self::Unit
        } else {
            Self::Unknown
        }
    }

    fn normalized_storage_type(self) -> Self {
        match self {
            Self::IntegerLiteral(Some(domain)) => Self::Integer(domain),
            Self::IntegerLiteral(None) => Self::IntegerUnproven,
            other => other,
        }
    }
}

fn is_internal_object_carrier(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() {
        return false;
    }
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    segments == ["rsjs", "__runtime", "ClientObjectValue"] || segments == ["ClientObjectValue"]
}

fn internal_object_type_name(object: &InternalValueObjectSpec) -> Result<String, Diagnostic> {
    if object.type_key.len() != 64 || !object.type_key.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Diagnostic::new(
            Span::call_site(),
            "generated client value-object identity is malformed",
            "report this as an rsjs compiler bug",
            "helper-internal nominal identities are exactly 64 hexadecimal characters",
        ));
    }
    let mut result = format!("@rsjs_object({}", object.type_key.to_ascii_lowercase());
    let mut seen = std::collections::BTreeSet::new();
    for (name, rust_type) in &object.fields {
        if !is_internal_object_field_name(name) || !seen.insert(name) {
            return Err(Diagnostic::new(
                Span::call_site(),
                "generated client value-object field schema is malformed",
                "use unique Rust identifier field names",
                "the reserved helper signature encodes one closed own-property schema",
            ));
        }
        let canonical = internal_object_scalar_type(rust_type).ok_or_else(|| {
            Diagnostic::new(
                Span::call_site(),
                "unsupported client value-object field type",
                "use bool, i32/u32/i64/u64, f64, String, or &str fields",
                "helper-internal object schemas currently carry scalar leaves only",
            )
        })?;
        result.push(';');
        result.push_str(name);
        result.push('=');
        result.push_str(canonical);
    }
    result.push(')');
    Ok(result)
}

fn internal_object_shape(object: &InternalValueObjectSpec) -> Result<ObjectShape, Diagnostic> {
    object
        .fields
        .iter()
        .map(|(name, rust_type)| {
            let canonical = internal_object_scalar_type(rust_type).ok_or_else(|| {
                Diagnostic::new(
                    Span::call_site(),
                    "unsupported client value-object field type",
                    "use bool, i32/u32/i64/u64, f64, String, or &str fields",
                    "helper-internal object schemas currently carry scalar leaves only",
                )
            })?;
            Ok((
                name.clone(),
                HelperValueType::from_type_name(canonical).normalized_storage_type(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|fields| ObjectShape { fields })
}

fn internal_object_scalar_type(rust_type: &str) -> Option<&'static str> {
    match rust_type {
        "bool" => Some("bool"),
        "i32" => Some("i32"),
        "u32" => Some("u32"),
        "i64" => Some("i64"),
        "u64" => Some("u64"),
        "f64" => Some("f64"),
        "String" | "&str" => Some("String"),
        _ => None,
    }
}

fn is_internal_object_field_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn helper_assignment_types_compatible(expected: HelperValueType, actual: HelperValueType) -> bool {
    expected == actual
        || matches!(actual, HelperValueType::Unknown)
        || matches!(
            (expected, actual),
            (
                HelperValueType::Integer(domain),
                HelperValueType::IntegerLiteral(Some(literal))
            ) if domain == literal
        )
}

fn is_helper_compound_assignment(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
    )
}

fn lower_helper(func: &syn::ItemFn) -> Result<LoweredHelper, Diagnostic> {
    lower_helper_with_options(func, None)
}

fn lower_helper_with_options(
    func: &syn::ItemFn,
    internal_object: Option<&InternalValueObjectSpec>,
) -> Result<LoweredHelper, Diagnostic> {
    let sig = &func.sig;

    if let Some(constness) = &sig.constness {
        return Err(Diagnostic::new(
            constness.span(),
            "`#[rsjs::client]` helpers cannot be `const`",
            "remove the `const` qualifier",
            "the generated server dispatcher performs runtime decoding and cannot preserve const-evaluation semantics; silently removing constness would change the public Rust API",
        ));
    }
    if let Some(asyncness) = sig.asyncness {
        return Err(Diagnostic::new(
            asyncness.span(),
            "`#[rsjs::client]` helpers cannot be `async`",
            "remove the `async` keyword",
            "client helpers lower to a synchronous JS expression; \
             use `#[rsjs(...)]` on a `WebRender` impl if you need to await server data",
        ));
    }
    if let Some(variadic) = &sig.variadic {
        return Err(Diagnostic::new(
            variadic.span(),
            "`#[rsjs::client]` helpers cannot be variadic",
            "replace the variadic arguments with one explicit helper-supported collection parameter",
            "standalone helper signatures have a fixed, hashed parameter list; dropping a C variadic tail would change both the Rust ABI and the browser call contract",
        ));
    }
    if let Some(unsafety) = &sig.unsafety {
        return Err(Diagnostic::new(
            unsafety.span(),
            "`#[rsjs::client]` helpers cannot be `unsafe`",
            "move the unsafe operation behind a safe, server-only abstraction",
            "JavaScript has no Rust unsafe-call boundary, and regenerating this item as a safe function would erase the caller-visible safety contract",
        ));
    }
    if let Some(abi) = &sig.abi {
        return Err(Diagnostic::new(
            abi.span(),
            "`#[rsjs::client]` helpers cannot declare an `extern` ABI",
            "use an ordinary Rust-ABI free function",
            "the generated dispatcher and browser function do not preserve a foreign calling convention; silently rewriting the ABI would be unsound",
        ));
    }
    // Round-3 review #3: lifetime-only generics (e.g.
    // `fn helper<'a>(s: &'a str)`) do not affect the JS counterpart;
    // they only annotate borrow lifetimes on the Rust side. Reject
    // type and const generic parameters but allow lifetime parameters
    // through (we normalise `&'a str` to `&str` when stamping out the
    // regenerated signature below).
    let non_lifetime_generic = sig
        .generics
        .params
        .iter()
        .find(|p| !matches!(p, syn::GenericParam::Lifetime(_)));
    if let Some(bad) = non_lifetime_generic {
        return Err(Diagnostic::new(
            bad.span(),
            "`#[rsjs::client]` helpers cannot be generic in v0.1",
            "remove the type/const generic parameter or define a separate concrete helper",
            "the JS counterpart is a single specialised function; type / const generics \
             would require monomorphisation at the macro layer, which is out of scope for \
             v0.1. (Lifetime-only generics are accepted.)",
        ));
    }
    if let Some(where_clause) = &sig.generics.where_clause {
        return Err(Diagnostic::new(
            where_clause.span(),
            "`#[rsjs::client]` helpers cannot carry a `where` clause in v0.1",
            "remove the `where` clause",
            "the JS counterpart is unconstrained; trait bounds have no client-side \
             meaning. Concrete primitive types (i64, String, bool, ...) are allowed.",
        ));
    }

    // Parameters: each must be a typed receiver-less argument with a recognised type.
    let mut params: Vec<ParamInfo> = Vec::with_capacity(sig.inputs.len());
    for (idx, input) in sig.inputs.iter().enumerate() {
        let typed = match input {
            syn::FnArg::Typed(t) => t,
            syn::FnArg::Receiver(r) => {
                return Err(Diagnostic::new(
                    r.span(),
                    "`#[rsjs::client]` helpers must be free functions, not methods",
                    "remove the `self` receiver and convert the method into a free function",
                    "client helpers are bundled into a JS module; they have no enclosing struct",
                ));
            }
        };
        let name = match &*typed.pat {
            syn::Pat::Ident(p)
                if p.attrs.is_empty()
                    && p.by_ref.is_none()
                    && p.mutability.is_none()
                    && p.subpat.is_none() =>
            {
                p.ident.clone()
            }
            other => {
                return Err(Diagnostic::new(
                    other.span(),
                    "`#[rsjs::client]` parameter patterns must be plain identifiers",
                    "rename the parameter to a single identifier (e.g. `price: i64`)",
                    "destructuring patterns have no JS counterpart in v0.1",
                ));
            }
        };
        if let Some(object) = internal_object.filter(|_| is_internal_object_carrier(&typed.ty)) {
            params.push(ParamInfo {
                name,
                ty_name: internal_object_type_name(object)?,
                ty_tokens: quote!(::rsjs::__runtime::ClientObjectValue),
                index: idx,
                integer_domain: None,
                object_shape: Some(internal_object_shape(object)?),
            });
            continue;
        }
        if let Some(name) = unsupported_helper_integer_leaf(&typed.ty) {
            return Err(unsupported_helper_integer_domain(typed.ty.span(), &name));
        }
        if helper_type_contains_f32(&typed.ty) {
            return Err(unsupported_helper_f32_domain(typed.ty.span()));
        }
        let ty_name = recognise_primitive_ty(&typed.ty).ok_or_else(|| {
            Diagnostic::new(
                typed.ty.span(),
                "unsupported parameter type for `#[rsjs::client]` (v0.1 accepts \
                 JSON primitives plus Option<T>, Vec<T>, and &[T] around helper-supported \
                 values: bool, i32/i64/u32/u64, f64, String, &str)",
                "use a v0.1 primitive, or wait for `#[derive(rsjs::ClientProps)]` (M7f) for structs",
                "the macro lowers each parameter into a JSON-shaped argument crossing \
                 the server/client boundary; richer types arrive with the ClientProps derive",
            )
        })?;
        // Round-3 review #3: normalise `&'a str` to `&str` when
        // re-emitting the regenerated signature. The user's lifetime
        // would otherwise need to be declared on the regenerated
        // function header, but we deliberately strip lifetime-only
        // generics from the regen impl. `&str` carries the lifetime
        // implicitly via elision in fn signatures.
        let ty_tokens = helper_ty_tokens(&ty_name, &typed.ty);
        let integer_domain = IntegerDomain::from_name(&ty_name);
        params.push(ParamInfo {
            name,
            ty_name,
            ty_tokens,
            index: idx,
            integer_domain,
            object_shape: None,
        });
    }

    // Return type
    let (ret_ty_tokens, ret_ty_name) = match &sig.output {
        syn::ReturnType::Default => (quote!(()), String::from("()")),
        syn::ReturnType::Type(_, ty)
            if internal_object.is_some() && is_internal_object_carrier(ty) =>
        {
            let object = internal_object.expect("guarded by is_some");
            (
                quote!(::rsjs::__runtime::ClientObjectValue),
                internal_object_type_name(object)?,
            )
        }
        syn::ReturnType::Type(_, ty) => {
            if let Some(name) = unsupported_helper_integer_leaf(ty) {
                return Err(unsupported_helper_integer_domain(ty.span(), &name));
            }
            if helper_type_contains_f32(ty) {
                return Err(unsupported_helper_f32_domain(ty.span()));
            }
            if helper_type_contains_borrowed_str(ty) {
                return Err(unsupported_helper_borrowed_return(ty.span()));
            }
            let name = recognise_primitive_ty(ty).ok_or_else(|| {
                Diagnostic::new(
                    ty.span(),
                    "unsupported return type for `#[rsjs::client]`",
                    "return one of the owned v0.1 values: bool, i32/i64/u32/u64, f64, String, Option<T>, or Vec<T>",
                    "richer return types lower through the same JSON bridge as parameters; \
                     v0.1 ships only the primitive/Option/Vec set",
                )
            })?;
            if name.contains("&[") {
                return Err(Diagnostic::new(
                    ty.span(),
                    "slice return types are not supported in `#[rsjs::client]` helpers",
                    "return `Vec<T>` instead of `&[T]`",
                    "helper return values cross the server/client boundary as owned JSON-shaped \
                     values; slices are accepted for parameters only because they can be encoded \
                     from borrowed Rust inputs",
                ));
            }
            (ty.to_token_stream(), name)
        }
    };

    // Body: keep the expression-only fast path stable, but allow the
    // statement-level shape needed for local bindings and early-return helpers:
    //
    //     let y = x + 1;
    //     if cond { return x; }
    //     tail
    //
    // The multi-statement path still rejects arbitrary expression
    // statements so the helper surface remains explicit.
    if func.block.stmts.is_empty() {
        return Err(Diagnostic::new(
            func.block.span(),
            "`#[rsjs::client]` helpers must have a body",
            "supply a tail expression, or early `return` statements followed by a tail expression",
            "client helpers lower to a deterministic Client IR v3 function body",
        ));
    }

    let mut lowerer = BodyLowerer {
        params: &params,
        locals: Vec::new(),
        generated_local_counter: 0,
        return_integer_domain: helper_return_integer_domain(&ret_ty_name).map(|(domain, _)| domain),
        internal_object,
    };
    let mut body = if func.block.stmts.len() == 1 {
        match &func.block.stmts[0] {
            syn::Stmt::Expr(syn::Expr::Match(match_expr), None) => {
                HelperBody::Block(lowerer.lower_tail_match(match_expr)?)
            }
            syn::Stmt::Expr(expr, None) => HelperBody::Expr(lowerer.lower(expr)?),
            syn::Stmt::Expr(_, Some(semi)) => {
                return Err(Diagnostic::new(
                    semi.span(),
                    "`#[rsjs::client]` helpers must end with an expression, not a statement",
                    "remove the trailing `;` so the value is the function's tail",
                    "a trailing `;` makes the body evaluate to `()`, which has no client-side mapping",
                ));
            }
            other => {
                return Err(Diagnostic::new(
                    other.span(),
                    "`#[rsjs::client]` helpers must contain a tail expression or early returns",
                    "use a single tail expression, or `let` / `if cond { return value; }` before the tail",
                    "arbitrary statements are not part of the client helper statement subset yet",
                ));
            }
        }
    } else {
        HelperBody::Block(lowerer.lower_statement_block(&func.block)?)
    };
    if let Some((domain, nullable)) = helper_return_integer_domain(&ret_ty_name) {
        lowerer.contextualize_helper_returns(&mut body, domain, nullable)?;
    }
    Ok(LoweredHelper {
        params,
        ret_ty_name,
        ret_ty_tokens,
        body,
    })
}

impl<'a> BodyLowerer<'a> {
    fn lower_statement_block(&mut self, block: &syn::Block) -> Result<BodyBlock, Diagnostic> {
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for (idx, stmt) in block.stmts.iter().enumerate() {
            let is_last = idx + 1 == block.stmts.len();
            if is_last {
                if let syn::Stmt::Expr(syn::Expr::Match(match_expr), None) = stmt {
                    stmts.extend(self.lower_tail_match(match_expr)?.stmts);
                    continue;
                }
            }
            stmts.push(self.lower_statement(stmt, is_last)?);
        }
        Ok(BodyBlock { stmts })
    }

    fn lower_branch_block(&mut self, block: &syn::Block) -> Result<BodyBlock, Diagnostic> {
        if block.stmts.is_empty() {
            return Err(Diagnostic::new(
                block.span(),
                "`#[rsjs::client]` helper branches cannot be empty",
                "return a value from the branch, or move the branch out of the client helper",
                "Client IR v3 needs an explicit statement for every lowered branch",
            ));
        }
        let scope_start = self.locals.len();
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in &block.stmts {
            stmts.push(self.lower_branch_statement(stmt)?);
        }
        for local in &mut self.locals[scope_start..] {
            local.visible = false;
        }
        Ok(BodyBlock { stmts })
    }

    fn lower_branch_statement(&mut self, stmt: &syn::Stmt) -> Result<BodyStmt, Diagnostic> {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Return(ret), _) => {
                let value = ret.expr.as_deref().map(|expr| self.lower(expr)).transpose()?;
                Ok(BodyStmt::Return {
                    span: ret.return_token.span,
                    value,
                })
            }
            syn::Stmt::Expr(syn::Expr::If(if_expr), _) => self.lower_if_statement(if_expr),
            syn::Stmt::Expr(syn::Expr::Assign(assign), Some(_)) => self.lower_assignment(assign),
            syn::Stmt::Expr(syn::Expr::Binary(binary), Some(_))
                if is_helper_compound_assignment(&binary.op) =>
            {
                self.lower_compound_assignment(binary)
            }
            syn::Stmt::Expr(expr, Some(_)) => Ok(BodyStmt::Expr {
                span: expr.span(),
                value: self.lower(expr)?,
            }),
            syn::Stmt::Expr(expr, _) => Err(Diagnostic::new(
                expr.span(),
                "`#[rsjs::client]` early-return branches must use explicit `return`",
                "write `return value;` inside the branch, or move the branch logic into a value-level `if` / `else` expression",
                "a statement-level `if` without an explicit return should fall through in Rust; \
                 lowering a branch tail expression as a JS return would change that behavior",
            )),
            syn::Stmt::Local(local) => self.lower_local_statement(local, false),
            other => Err(Diagnostic::new(
                other.span(),
                "this statement is not allowed in a `#[rsjs::client]` helper branch",
                "use explicit `return` statements inside early-return branches",
                "the helper branch subset is intentionally smaller than general Rust",
            )),
        }
    }

    fn lower_statement(&mut self, stmt: &syn::Stmt, is_last: bool) -> Result<BodyStmt, Diagnostic> {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Return(ret), _) => {
                let value = ret.expr.as_deref().map(|expr| self.lower(expr)).transpose()?;
                Ok(BodyStmt::Return {
                    span: ret.return_token.span,
                    value,
                })
            }
            syn::Stmt::Expr(syn::Expr::If(if_expr), _) if !is_last => {
                self.lower_if_statement(if_expr)
            }
            syn::Stmt::Expr(syn::Expr::Assign(assign), Some(_)) if !is_last => {
                self.lower_assignment(assign)
            }
            syn::Stmt::Expr(syn::Expr::Binary(binary), Some(_))
                if !is_last && is_helper_compound_assignment(&binary.op) =>
            {
                self.lower_compound_assignment(binary)
            }
            syn::Stmt::Expr(expr, Some(_)) if !is_last => Ok(BodyStmt::Expr {
                span: expr.span(),
                value: self.lower(expr)?,
            }),
            syn::Stmt::Expr(syn::Expr::If(if_expr), _) => Err(Diagnostic::new(
                if_expr.if_token.span(),
                "`#[rsjs::client]` statement-level `if` needs a following tail expression",
                "write `if cond { return value; }` followed by the helper's final tail value",
                "the macro does not yet prove that all paths inside a final statement-level \
                 `if` return",
            )),
            syn::Stmt::Expr(expr, None) if is_last => Ok(BodyStmt::Return {
                span: expr.span(),
                value: Some(self.lower(expr)?),
            }),
            syn::Stmt::Expr(_, Some(semi)) if is_last => Err(Diagnostic::new(
                semi.span(),
                "`#[rsjs::client]` helper blocks must end with a value",
                "remove the trailing `;` from the final expression",
                "the final expression becomes the helper's JavaScript `return` value",
            )),
            syn::Stmt::Expr(expr, _) => Err(Diagnostic::new(
                expr.span(),
                "this statement is not allowed in a `#[rsjs::client]` helper block",
                "use `if cond { return value; }` for early returns, then finish with a tail expression",
                "Client IR v3 helper statements currently cover only early returns and \
                 statement-level `if`",
            )),
            syn::Stmt::Local(local) => self.lower_local_statement(local, is_last),
            other => Err(Diagnostic::new(
                other.span(),
                "this statement is not allowed in a `#[rsjs::client]` helper block",
                "use early `return` statements plus a final tail expression",
                "the helper statement subset is intentionally smaller than general Rust",
            )),
        }
    }

    fn lower_local_statement(
        &mut self,
        local: &syn::Local,
        is_last: bool,
    ) -> Result<BodyStmt, Diagnostic> {
        if is_last {
            return Err(Diagnostic::new(
                local.span(),
                "`#[rsjs::client]` helper blocks must end with a value",
                "add a tail expression after the `let` binding",
                "a local declaration does not produce the helper's return value",
            ));
        }
        let (pat, explicit_ty) = match &local.pat {
            syn::Pat::Ident(pat) => (pat, None),
            syn::Pat::Type(typed) => {
                let syn::Pat::Ident(pat) = typed.pat.as_ref() else {
                    return Err(Diagnostic::new(
                        typed.pat.span(),
                        "`#[rsjs::client]` typed `let` bindings must use a plain identifier",
                        "write `let name: i64 = value;`",
                        "destructuring locals need typed Client IR pattern support",
                    ));
                };
                if let Some(name) = unsupported_helper_integer_leaf(&typed.ty) {
                    return Err(unsupported_helper_integer_domain(typed.ty.span(), &name));
                }
                if helper_type_contains_f32(&typed.ty) {
                    return Err(unsupported_helper_f32_domain(typed.ty.span()));
                }
                let ty_name = recognise_primitive_ty(&typed.ty).ok_or_else(|| {
                    Diagnostic::new(
                        typed.ty.span(),
                        "unsupported typed local in `#[rsjs::client]`",
                        "use a helper-supported primitive local type",
                        "typed helper locals must have the same deterministic client representation as parameters",
                    )
                })?;
                (pat, Some(HelperValueType::from_type_name(&ty_name)))
            }
            other => {
                return Err(Diagnostic::new(
                    other.span(),
                    "`#[rsjs::client]` `let` bindings must use a plain identifier",
                    "write `let name = value;`",
                    "destructuring locals need typed Client IR pattern support",
                ));
            }
        };
        let is_mutable = pat.mutability.is_some();
        if pat.by_ref.is_some() || pat.subpat.is_some() {
            return Err(Diagnostic::new(
                pat.span(),
                "`#[rsjs::client]` `let` bindings cannot use ref bindings or subpatterns",
                "write `let name = value;`",
                "the helper local surface is intentionally limited to direct value locals",
            ));
        }
        let name = pat.ident.clone();
        if self.params.iter().any(|param| param.name == name)
            || self
                .locals
                .iter()
                .any(|local| local.visible && local.name == name)
        {
            return Err(Diagnostic::new(
                name.span(),
                "`#[rsjs::client]` local shadowing is not supported yet",
                "use a unique local name",
                "stable helper metadata maps each local to a single Client IR local id",
            ));
        }
        let Some(init) = &local.init else {
            return Err(Diagnostic::new(
                local.span(),
                "`#[rsjs::client]` `let` bindings need an initializer",
                "write `let name = value;`",
                "uninitialized locals have no JavaScript expression counterpart",
            ));
        };
        if init.diverge.is_some() {
            return Err(Diagnostic::new(
                local.span(),
                "`#[rsjs::client]` `let ... else` bindings are not supported",
                "rewrite the branch as a value-level `if` / `else` or an early return",
                "`let ... else` needs pattern and control-flow lowering beyond this helper subset",
            ));
        }
        let mut value = self.lower(&init.expr)?;
        if let Some(HelperValueType::Integer(domain)) = explicit_ty {
            self.contextualize_integer_node(&mut value, domain, false)?;
        }
        let local_ty = match explicit_ty {
            Some(ty) => ty,
            None => match self.value_type(&value) {
                // Rust defaults an otherwise unconstrained integer local to
                // i32. Store the resolved domain on the local so a later
                // assignment or compound assignment sees one stable type.
                HelperValueType::IntegerLiteral(None) => {
                    self.contextualize_integer_node(&mut value, IntegerDomain::I32, false)?;
                    HelperValueType::Integer(IntegerDomain::I32)
                }
                ty => ty.normalized_storage_type(),
            },
        };
        let object_shape = self.object_shape_for_node(&value);
        let constant = if explicit_ty.is_none() && !is_mutable {
            match &value.kind {
                BodyNodeKind::Literal(value) => Some(value.clone()),
                _ => None,
            }
        } else {
            None
        };
        let local_id = self.alloc_local(name.clone(), local_ty, constant);
        let local_info = self
            .locals
            .last_mut()
            .expect("alloc_local always appends one local");
        debug_assert_eq!(local_info.id, local_id);
        local_info.mutable = is_mutable;
        local_info.object_shape = object_shape;
        Ok(BodyStmt::Let {
            span: local.span(),
            local: local_id,
            name: name.to_string(),
            mutable: is_mutable,
            value,
        })
    }

    fn lower_assignment(&self, assign: &syn::ExprAssign) -> Result<BodyStmt, Diagnostic> {
        match assign.left.as_ref() {
            syn::Expr::Path(path) => {
                self.lower_local_assignment(path, &assign.right, assign.span())
            }
            syn::Expr::Field(field) => {
                self.lower_field_assignment(field, &assign.right, assign.span())
            }
            _ => Err(Diagnostic::new(
                assign.left.span(),
                "`#[rsjs::client]` assignment requires a mutable local or named struct field",
                "assign to `value` or to a field such as `controller.open`",
                "index and destructuring places are not part of the client helper subset yet",
            )),
        }
    }

    fn lower_local_assignment(
        &self,
        path: &syn::ExprPath,
        right: &syn::Expr,
        span: Span,
    ) -> Result<BodyStmt, Diagnostic> {
        if path.qself.is_some()
            || path.path.leading_colon.is_some()
            || path.path.segments.len() != 1
        {
            return Err(Diagnostic::new(
                path.span(),
                "`#[rsjs::client]` local assignment needs one local identifier",
                "assign directly to a mutable helper local",
                "paths and associated items are not mutable Client IR locals",
            ));
        }
        let ident = &path.path.segments[0].ident;
        let Some(local) = self
            .locals
            .iter()
            .find(|local| local.visible && local.name == *ident)
        else {
            return Err(Diagnostic::new(
                ident.span(),
                "`#[rsjs::client]` assignment target is not a helper local",
                "declare it with `let mut` before assigning",
                "helper parameters and outer captures are immutable at this boundary",
            ));
        };
        if !local.mutable {
            return Err(Diagnostic::new(
                ident.span(),
                "cannot assign to an immutable `#[rsjs::client]` local",
                "declare the local with `let mut`",
                "the Client IR and generated server evaluator preserve Rust local mutability",
            ));
        }
        if local.ty == HelperValueType::Object {
            return Err(Diagnostic::new(
                ident.span(),
                "whole-object reassignment is not supported in `#[rsjs::client]` yet",
                "mutate named fields on the existing object local",
                "the frontend keeps one closed field shape for each helper-local object",
            ));
        }
        let mut value = self.lower(right)?;
        if let HelperValueType::Integer(domain) = local.ty {
            self.contextualize_integer_node(&mut value, domain, false)?;
        }
        let actual_ty = self.value_type(&value);
        if !helper_assignment_types_compatible(local.ty, actual_ty) {
            return Err(Diagnostic::new(
                right.span(),
                "assigned value has a different client-side local type",
                "assign a value with the same type as the mutable local's initializer",
                "Client IR locals retain one stable browser representation",
            ));
        }
        Ok(BodyStmt::AssignLocal {
            span,
            local: local.id,
            value,
        })
    }

    fn lower_field_assignment(
        &self,
        field: &syn::ExprField,
        right: &syn::Expr,
        span: Span,
    ) -> Result<BodyStmt, Diagnostic> {
        let syn::Member::Named(field_ident) = &field.member else {
            return Err(Diagnostic::new(
                field.member.span(),
                "tuple-field assignment is not supported in `#[rsjs::client]`",
                "use a named-field struct for client value objects",
                "Client IR object places are named own properties",
            ));
        };
        let syn::Expr::Path(base_path) = field.base.as_ref() else {
            return Err(Diagnostic::new(
                field.base.span(),
                "nested field assignment is not supported in `#[rsjs::client]` yet",
                "assign directly to a mutable local, for example `controller.open = value`",
                "the underlying Client IR supports nested places, but the Rust frontend does not yet carry nested object shapes",
            ));
        };
        if base_path.qself.is_some()
            || base_path.path.leading_colon.is_some()
            || base_path.path.segments.len() != 1
        {
            return Err(Diagnostic::new(
                base_path.span(),
                "`#[rsjs::client]` field assignment must start from a local identifier",
                "assign directly to a mutable helper-local struct",
                "paths and associated items are not mutable Client IR places",
            ));
        }
        let base_ident = &base_path.path.segments[0].ident;
        let Some(local) = self
            .locals
            .iter()
            .find(|local| local.visible && local.name == *base_ident)
        else {
            return Err(Diagnostic::new(
                base_ident.span(),
                "`#[rsjs::client]` field assignment target is not a helper local",
                "initialize a mutable struct local before assigning its field",
                "only helper-local value objects have an assignable Client IR place",
            ));
        };
        if !local.mutable {
            return Err(Diagnostic::new(
                base_ident.span(),
                "cannot assign through an immutable `#[rsjs::client]` local",
                "declare the value object with `let mut`",
                "the generated server evaluator preserves Rust local mutability",
            ));
        }
        let field_name = field_ident.to_string();
        let expected_ty = local
            .object_shape
            .as_ref()
            .and_then(|shape| {
                shape
                    .fields
                    .iter()
                    .find_map(|(name, ty)| (name == &field_name).then_some(*ty))
            })
            .ok_or_else(|| {
                Diagnostic::new(
                    field_ident.span(),
                    "unknown field on this `#[rsjs::client]` value object",
                    "assign one of the fields present in the struct initializer",
                    "the client helper frontend tracks the initializer's closed own-property shape",
                )
            })?;
        let mut value = self.lower(right)?;
        if let HelperValueType::Integer(domain) = expected_ty {
            self.contextualize_integer_node(&mut value, domain, false)?;
        }
        let actual_ty = self.value_type(&value);
        if !helper_assignment_types_compatible(expected_ty, actual_ty) {
            return Err(Diagnostic::new(
                right.span(),
                "assigned value has a different client-side field type",
                "assign a value with the same scalar shape as the initialized field",
                "client value-object fields retain one stable Client IR type",
            ));
        }
        Ok(BodyStmt::AssignField {
            span,
            local: local.id,
            field: field_name,
            value,
        })
    }

    fn lower_compound_assignment(&self, binary: &syn::ExprBinary) -> Result<BodyStmt, Diagnostic> {
        let left = &binary.left;
        let right = &binary.right;
        let expanded: syn::ExprAssign = match &binary.op {
            syn::BinOp::AddAssign(_) => {
                syn::parse_quote_spanned!(binary.span()=> #left = #left + #right)
            }
            syn::BinOp::SubAssign(_) => {
                syn::parse_quote_spanned!(binary.span()=> #left = #left - #right)
            }
            syn::BinOp::MulAssign(_) => {
                syn::parse_quote_spanned!(binary.span()=> #left = #left * #right)
            }
            syn::BinOp::DivAssign(_) => {
                syn::parse_quote_spanned!(binary.span()=> #left = #left / #right)
            }
            syn::BinOp::RemAssign(_) => {
                syn::parse_quote_spanned!(binary.span()=> #left = #left % #right)
            }
            _ => {
                return Err(Diagnostic::new(
                    binary.op.span(),
                    "this compound assignment is not supported for client value-object fields",
                    "use one of `+=`, `-=`, `*=`, `/=`, or `%=`",
                    "the supported forms lower through the checked Client IR arithmetic operators",
                ));
            }
        };
        self.lower_assignment(&expanded)
    }

    fn object_shape_for_node(&self, node: &BodyNode) -> Option<ObjectShape> {
        match &node.kind {
            BodyNodeKind::StructInit { fields, .. } => Some(ObjectShape {
                fields: fields
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            self.value_type(value).normalized_storage_type(),
                        )
                    })
                    .collect(),
            }),
            BodyNodeKind::Local { id, .. } => self
                .locals
                .iter()
                .find(|local| local.id == *id)
                .and_then(|local| local.object_shape.clone()),
            BodyNodeKind::Param { index, .. } => self
                .params
                .get(*index)
                .and_then(|param| param.object_shape.clone()),
            _ => None,
        }
    }

    fn alloc_local(
        &mut self,
        name: syn::Ident,
        ty: HelperValueType,
        constant: Option<LiteralValue>,
    ) -> u32 {
        let local_id = (self.params.len() + self.locals.len()) as u32;
        self.locals.push(LocalInfo {
            name,
            id: local_id,
            ty,
            constant,
            mutable: false,
            object_shape: None,
            visible: true,
        });
        local_id
    }

    fn alloc_generated_local(
        &mut self,
        prefix: &str,
        span: Span,
        ty: HelperValueType,
    ) -> (u32, String) {
        let idx = self.generated_local_counter;
        self.generated_local_counter += 1;
        let name = format!("__rsjs_{prefix}_{idx}");
        let id = self.alloc_local(Ident::new(&name, span), ty, None);
        (id, name)
    }

    fn lower_if_statement(&mut self, if_expr: &syn::ExprIf) -> Result<BodyStmt, Diagnostic> {
        let condition = self.lower(&if_expr.cond)?;
        self.require_bool(&condition, if_expr.cond.span())?;
        let then_block = self.lower_branch_block(&if_expr.then_branch)?;
        let else_block = match &if_expr.else_branch {
            Some((_, else_expr)) => Some(match &**else_expr {
                syn::Expr::If(nested) => BodyBlock {
                    stmts: vec![self.lower_if_statement(nested)?],
                },
                syn::Expr::Block(block) => self.lower_branch_block(&block.block)?,
                other => BodyBlock {
                    stmts: vec![BodyStmt::Return {
                        span: other.span(),
                        value: Some(self.lower(other)?),
                    }],
                },
            }),
            None => None,
        };
        Ok(BodyStmt::If {
            span: if_expr.if_token.span(),
            condition,
            then_block,
            else_block,
        })
    }

    fn lower(&self, expr: &syn::Expr) -> Result<BodyNode, Diagnostic> {
        match expr {
            syn::Expr::Lit(lit) => self.lower_lit(&lit.lit),
            syn::Expr::Path(p) => self.lower_path(p),
            syn::Expr::Binary(b) => {
                let op = match b.op {
                    syn::BinOp::Add(_) => "Add",
                    syn::BinOp::Sub(_) => "Sub",
                    syn::BinOp::Mul(_) => "Mul",
                    syn::BinOp::Div(_) => "Div",
                    syn::BinOp::Rem(_) => "Rem",
                    syn::BinOp::Eq(_) => "Eq",
                    syn::BinOp::Ne(_) => "Neq",
                    syn::BinOp::Lt(_) => "Lt",
                    syn::BinOp::Gt(_) => "Gt",
                    syn::BinOp::Le(_) => "Le",
                    syn::BinOp::Ge(_) => "Ge",
                    syn::BinOp::And(_) => "And",
                    syn::BinOp::Or(_) => "Or",
                    other => {
                        return Err(Diagnostic::new(
                            other.span(),
                            "this binary operator is not allowed in `#[rsjs::client]` bodies",
                            "use one of the v0.1 operators (+ - * / % == != < > <= >= && ||)",
                            "the v0.1 IR's `BinOp` covers arithmetic, comparison, and short-circuit \
                             logic; bit-twiddling and assignment operators arrive in later milestones",
                        ));
                    }
                };
                let mut lhs = self.lower(&b.left)?;
                let mut rhs = self.lower(&b.right)?;
                if matches!(op, "And" | "Or") {
                    self.require_bool(&lhs, b.left.span())?;
                    self.require_bool(&rhs, b.right.span())?;
                }
                let op = self.resolve_numeric_binary_op(
                    op,
                    &mut lhs,
                    &mut rhs,
                    b.span(),
                )?;
                Ok(BodyNode::binary(b.span(), op, lhs, rhs))
            }
            syn::Expr::Paren(p) => self.lower(&p.expr),
            syn::Expr::Group(g) => self.lower(&g.expr),
            syn::Expr::Unary(u) => self.lower_unary(u),
            syn::Expr::MethodCall(m) => self.lower_method_call(m),
            syn::Expr::Call(c) => self.lower_call(c),
            syn::Expr::If(i) => self.lower_if(i),
            syn::Expr::Match(m) => Err(Diagnostic::new(
                m.match_token.span,
                "`match` is supported in `#[rsjs::client]` helpers only as a tail value for now",
                "make the `match` the helper's final expression, or move this branch into another `#[rsjs::client]` helper",
                "tail matches lower to Client IR `let` + nested `if`/`return` blocks. Nested match expressions need expression-level match IR first",
            )),
            syn::Expr::Closure(c) => Err(Diagnostic::new(
                c.span(),
                "closures are not allowed in `#[rsjs::client]` bodies",
                "extract the closure body into another `#[rsjs::client]` helper, or use \
                 the helper's tail expression directly",
                "client helpers lower to a single straight-line JS expression; closures \
                 imply a JS function literal, which is the handler-mode surface (M7bc)",
            )),
            syn::Expr::Async(a) => Err(Diagnostic::new(
                a.async_token.span(),
                "`async` blocks are not allowed in `#[rsjs::client]` bodies",
                "remove the `async` block",
                "client helpers cannot await server work; the body must be synchronous so \
                 it can run during SSR and on the client",
            )),
            syn::Expr::Await(a) => Err(Diagnostic::new(
                a.await_token.span(),
                "`.await` is not allowed in `#[rsjs::client]` bodies",
                "remove the `.await`",
                "client helpers must lower to synchronous JS; awaiting is the async island \
                 surface (use `#[rsjs(component, client)]` on a canonical `WebRender` impl)",
            )),
            syn::Expr::Block(b) => self.lower_block_tail(&b.block),
            syn::Expr::Struct(s) => self.lower_struct(s),
            syn::Expr::Field(f) if is_self_or_cx_root(&f.base) => Err(Diagnostic::new(
                f.span(),
                "`self` / `cx` fields are not accessible from `#[rsjs::client]` helpers",
                "pass the value in as a function parameter instead",
                "client helpers run independently of any enclosing impl or render context",
            )),
            syn::Expr::Field(f) => self.lower_field(f),
            other => Err(self.unsupported(other)),
        }
    }

    fn lower_struct(&self, expr: &syn::ExprStruct) -> Result<BodyNode, Diagnostic> {
        if expr.qself.is_some()
            || expr
                .path
                .segments
                .iter()
                .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
        {
            return Err(Diagnostic::new(
                expr.path.span(),
                "client value-object constructors must use a concrete struct path",
                "construct a non-generic named struct directly",
                "generic and qualified struct identities need rustc monomorphization evidence that helper metadata does not carry yet",
            ));
        }
        if let Some(rest) = &expr.rest {
            return Err(Diagnostic::new(
                rest.span(),
                "struct update syntax is not supported in `#[rsjs::client]`",
                "spell out every client value-object field",
                "plain JavaScript object construction needs a closed, explicit own-property shape",
            ));
        }
        let mut fields = Vec::with_capacity(expr.fields.len());
        for field in &expr.fields {
            let syn::Member::Named(name) = &field.member else {
                return Err(Diagnostic::new(
                    field.member.span(),
                    "tuple structs are not supported as client value objects",
                    "use a struct with named fields",
                    "Client IR object fields are named own properties",
                ));
            };
            let field_name = name.to_string();
            if fields
                .iter()
                .any(|(existing, _): &(String, BodyNode)| existing == &field_name)
            {
                return Err(Diagnostic::new(
                    name.span(),
                    "duplicate field in client value-object construction",
                    "initialize each named field once",
                    "the emitted JavaScript object has one own property per Rust field",
                ));
            }
            fields.push((field_name, self.lower(&field.expr)?));
        }
        let authored_type = canonical_helper_path_spelling(&expr.path);
        let type_name = self
            .internal_object
            .filter(|object| {
                authored_type == "Self"
                    || object.rust_type_names.iter().any(|name| {
                        authored_type == *name || authored_type.ends_with(&format!("::{name}"))
                    })
            })
            .map_or(authored_type, |object| object.type_key.to_ascii_lowercase());
        Ok(BodyNode::struct_init(expr.span(), type_name, fields))
    }

    fn lower_field(&self, expr: &syn::ExprField) -> Result<BodyNode, Diagnostic> {
        let syn::Member::Named(name) = &expr.member else {
            return Err(Diagnostic::new(
                expr.member.span(),
                "tuple fields are not supported in `#[rsjs::client]`",
                "use a client value object with named fields",
                "Client IR field reads address named own properties",
            ));
        };
        let base = self.lower(&expr.base)?;
        let field_name = name.to_string();
        let Some(shape) = self.object_shape_for_node(&base) else {
            return Err(Diagnostic::new(
                expr.base.span(),
                "field reads require a helper-local client value object",
                "read a field from a named struct literal or a local initialized from one",
                "arbitrary Rust field projection has no JavaScript representation in this helper slice",
            ));
        };
        if !shape.fields.iter().any(|(field, _)| field == &field_name) {
            return Err(Diagnostic::new(
                name.span(),
                "unknown field on this `#[rsjs::client]` value object",
                "read one of the fields present in the struct initializer",
                "the client helper frontend tracks a closed own-property shape",
            ));
        }
        Ok(BodyNode::field(expr.span(), base, field_name))
    }

    fn resolve_numeric_binary_op(
        &self,
        op: &'static str,
        lhs: &mut BodyNode,
        rhs: &mut BodyNode,
        span: Span,
    ) -> Result<&'static str, Diagnostic> {
        let arithmetic = matches!(op, "Add" | "Sub" | "Mul" | "Div" | "Rem");
        let comparison = matches!(op, "Eq" | "Neq" | "Lt" | "Gt" | "Le" | "Ge");
        if !arithmetic && !comparison {
            return Ok(op);
        }
        let lhs_ty = self.value_type(lhs);
        let rhs_ty = self.value_type(rhs);
        if matches!(lhs_ty, HelperValueType::Object) || matches!(rhs_ty, HelperValueType::Object) {
            return Err(Diagnostic::new(
                span,
                "client value objects cannot be used directly with binary operators",
                "compare or combine named scalar fields instead",
                "JavaScript object equality is identity-based and would not preserve Rust value semantics",
            ));
        }
        if (matches!(lhs_ty, HelperValueType::IntegerUnproven)
            && matches!(&lhs.kind, BodyNodeKind::Field { .. }))
            || (matches!(rhs_ty, HelperValueType::IntegerUnproven)
                && matches!(&rhs.kind, BodyNodeKind::Field { .. }))
        {
            return Err(Diagnostic::new(
                span,
                "rsjs/integer-domain-unproven: numeric client value-object field has no explicit Rust width",
                "initialize the field from an i32, u32, i64, or u64 value (for literals, add a suffix such as `0_i32`)",
                "the proc macro cannot infer a surrounding struct declaration's field type, so it refuses to guess Number versus BigInt or overflow semantics",
            ));
        }
        let resolve_domain = || match (lhs_ty, rhs_ty) {
            (HelperValueType::Integer(lhs), HelperValueType::Integer(rhs)) if lhs == rhs => {
                Ok(Some(lhs))
            }
            (HelperValueType::Integer(lhs), HelperValueType::Integer(rhs)) => {
                Err(helper_integer_domain_mismatch(span, lhs, rhs))
            }
            (HelperValueType::Integer(domain), HelperValueType::IntegerLiteral(Some(literal)))
            | (HelperValueType::IntegerLiteral(Some(literal)), HelperValueType::Integer(domain))
                if domain != literal =>
            {
                Err(helper_integer_domain_mismatch(span, domain, literal))
            }
            (HelperValueType::Integer(domain), HelperValueType::IntegerLiteral(None))
            | (HelperValueType::IntegerLiteral(None), HelperValueType::Integer(domain)) => {
                Ok(Some(domain))
            }
            (HelperValueType::Integer(domain), HelperValueType::IntegerLiteral(Some(literal)))
            | (HelperValueType::IntegerLiteral(Some(literal)), HelperValueType::Integer(domain)) => {
                debug_assert_eq!(domain, literal);
                Ok(Some(domain))
            }
            (
                HelperValueType::IntegerLiteral(Some(lhs)),
                HelperValueType::IntegerLiteral(Some(rhs)),
            ) if lhs != rhs => Err(helper_integer_domain_mismatch(span, lhs, rhs)),
            (
                HelperValueType::IntegerLiteral(Some(domain)),
                HelperValueType::IntegerLiteral(None),
            )
            | (
                HelperValueType::IntegerLiteral(None),
                HelperValueType::IntegerLiteral(Some(domain)),
            ) => Ok(Some(domain)),
            (
                HelperValueType::IntegerLiteral(Some(lhs)),
                HelperValueType::IntegerLiteral(Some(rhs)),
            ) => {
                debug_assert_eq!(lhs, rhs);
                Ok(Some(lhs))
            }
            (HelperValueType::IntegerLiteral(None), HelperValueType::IntegerLiteral(None)) => {
                Ok(Some(IntegerDomain::I32))
            }
            (HelperValueType::Unknown, HelperValueType::Integer(domain))
                if matches!(&lhs.kind, BodyNodeKind::ClientCall { .. }) =>
            {
                Ok(Some(domain))
            }
            (HelperValueType::Integer(domain), HelperValueType::Unknown)
                if matches!(&rhs.kind, BodyNodeKind::ClientCall { .. }) =>
            {
                Ok(Some(domain))
            }
            (HelperValueType::Unknown, HelperValueType::IntegerLiteral(Some(domain)))
                if matches!(&lhs.kind, BodyNodeKind::ClientCall { .. }) =>
            {
                Ok(Some(domain))
            }
            (HelperValueType::IntegerLiteral(Some(domain)), HelperValueType::Unknown)
                if matches!(&rhs.kind, BodyNodeKind::ClientCall { .. }) =>
            {
                Ok(Some(domain))
            }
            (HelperValueType::Unknown, HelperValueType::IntegerLiteral(None))
                if matches!(&lhs.kind, BodyNodeKind::ClientCall { .. })
                    && self.return_integer_domain.is_some() =>
            {
                Ok(self.return_integer_domain)
            }
            (HelperValueType::IntegerLiteral(None), HelperValueType::Unknown)
                if matches!(&rhs.kind, BodyNodeKind::ClientCall { .. })
                    && self.return_integer_domain.is_some() =>
            {
                Ok(self.return_integer_domain)
            }
            (HelperValueType::Unknown, HelperValueType::Unknown)
                if matches!(&lhs.kind, BodyNodeKind::ClientCall { .. })
                    && matches!(&rhs.kind, BodyNodeKind::ClientCall { .. })
                    && self.return_integer_domain.is_some() =>
            {
                Ok(self.return_integer_domain)
            }
            (
                HelperValueType::IntegerUnproven | HelperValueType::Unknown,
                HelperValueType::Integer(_) | HelperValueType::IntegerLiteral(_),
            )
            | (
                HelperValueType::Integer(_) | HelperValueType::IntegerLiteral(_),
                HelperValueType::IntegerUnproven | HelperValueType::Unknown,
            )
            | (HelperValueType::IntegerUnproven, HelperValueType::IntegerUnproven) => {
                if arithmetic {
                    Err(Diagnostic::new(
                        span,
                        "rsjs/integer-domain-unproven: helper arithmetic has no proven fixed-width Rust domain",
                        "use i32, u32, i64, or u64 parameters/typed locals",
                        "native JavaScript arithmetic would lose Rust overflow, division, or remainder semantics",
                    ))
                } else {
                    // `usize`-producing `.len()` values may be displayed and
                    // compared, but do not enter arithmetic until a pointer
                    // width domain is supported.
                    Ok(None)
                }
            }
            _ => Ok(None),
        };

        let domain = resolve_domain()?;
        if arithmetic {
            if let Some(domain) = domain {
                coerce_helper_integer_literals(lhs, domain)?;
                coerce_helper_integer_literals(rhs, domain)?;
                return Ok(checked_helper_binary_op(op, domain));
            }
            if matches!(
                lhs_ty,
                HelperValueType::Unknown | HelperValueType::IntegerUnproven
            ) || matches!(
                rhs_ty,
                HelperValueType::Unknown | HelperValueType::IntegerUnproven
            ) {
                return Err(Diagnostic::new(
                    span,
                    "rsjs/integer-domain-unproven: helper arithmetic has an unknown operand type",
                    "add a supported concrete type or move the operation to server Rust",
                    "the helper compiler refuses to emit native JavaScript arithmetic without a Rust domain proof",
                ));
            }
        } else if let Some(domain) = domain {
            // Equality must not compare `5n === 5`; contextual Rust literals
            // inherit the proven operand domain for all comparisons too.
            coerce_helper_integer_literals(lhs, domain)?;
            coerce_helper_integer_literals(rhs, domain)?;
        }
        Ok(op)
    }

    fn contextualize_helper_returns(
        &self,
        body: &mut HelperBody,
        domain: IntegerDomain,
        nullable: bool,
    ) -> Result<(), Diagnostic> {
        match body {
            HelperBody::Expr(expr) => self.contextualize_integer_node(expr, domain, nullable),
            HelperBody::Block(block) => self.contextualize_return_block(block, domain, nullable),
        }
    }

    fn contextualize_return_block(
        &self,
        block: &mut BodyBlock,
        domain: IntegerDomain,
        nullable: bool,
    ) -> Result<(), Diagnostic> {
        for stmt in &mut block.stmts {
            match stmt {
                BodyStmt::Return {
                    value: Some(value), ..
                } => self.contextualize_integer_node(value, domain, nullable)?,
                BodyStmt::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    self.contextualize_return_block(then_block, domain, nullable)?;
                    if let Some(else_block) = else_block {
                        self.contextualize_return_block(else_block, domain, nullable)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn contextualize_integer_node(
        &self,
        node: &mut BodyNode,
        domain: IntegerDomain,
        nullable: bool,
    ) -> Result<(), Diagnostic> {
        if nullable && matches!(&node.kind, BodyNodeKind::Literal(LiteralValue::Null)) {
            return Ok(());
        }
        if !self.node_has_integer_anchor(node) {
            if self.node_is_contextual_integer(node) {
                rewrite_literal_only_checked_domain(node, domain);
                return coerce_helper_integer_literals(node, domain);
            }
            if matches!(&node.kind, BodyNodeKind::ClientCall { .. }) {
                // The generated dead source checker preserves rustc's exact
                // callee return type validation when metadata is unavailable.
                return Ok(());
            }
            return Err(Diagnostic::new(
                node.span,
                &format!(
                    "rsjs/integer-domain-mismatch: helper return/local expects `{}` but this expression is not a compatible integer",
                    domain.as_str()
                ),
                "return or assign a value with the declared integer type",
                "the helper macro validates the source type before replacing the Rust body",
            ));
        }
        self.validate_expected_integer_node(node, domain, nullable)
    }

    fn node_has_integer_anchor(&self, node: &BodyNode) -> bool {
        match &node.kind {
            BodyNodeKind::Param { index, .. } => self
                .params
                .get(*index)
                .and_then(|param| param.integer_domain)
                .is_some(),
            BodyNodeKind::Local { id, .. } => self
                .locals
                .iter()
                .find(|local| local.id == *id)
                .is_some_and(|local| matches!(local.ty, HelperValueType::Integer(_))),
            BodyNodeKind::Binary { lhs, rhs, .. } => {
                self.node_has_integer_anchor(lhs) || self.node_has_integer_anchor(rhs)
            }
            BodyNodeKind::Unary { expr, .. }
            | BodyNodeKind::Conditional {
                then_expr: expr,
                else_expr: _,
                ..
            } => {
                let first = self.node_has_integer_anchor(expr);
                if let BodyNodeKind::Conditional { else_expr, .. } = &node.kind {
                    first || self.node_has_integer_anchor(else_expr)
                } else {
                    first
                }
            }
            BodyNodeKind::Literal(LiteralValue::Integer {
                domain: Some(_), ..
            }) => true,
            BodyNodeKind::Literal(_) => false,
            BodyNodeKind::StructInit { .. } => false,
            BodyNodeKind::Field { .. } => matches!(
                self.value_type(node),
                HelperValueType::Integer(_) | HelperValueType::IntegerLiteral(Some(_))
            ),
            BodyNodeKind::ClientCall { .. }
            | BodyNodeKind::Len(_)
            | BodyNodeKind::IsNonEmpty(_)
            | BodyNodeKind::StringToLowercase(_)
            | BodyNodeKind::StringContains { .. }
            | BodyNodeKind::StringTransform { .. }
            | BodyNodeKind::StringPredicate { .. } => true,
        }
    }

    fn node_is_contextual_integer(&self, node: &BodyNode) -> bool {
        match &node.kind {
            BodyNodeKind::Literal(LiteralValue::Integer { domain: None, .. }) => true,
            BodyNodeKind::Binary { op, lhs, rhs } => {
                parse_checked_helper_binary_op(op).is_some()
                    && self.node_is_contextual_integer(lhs)
                    && self.node_is_contextual_integer(rhs)
            }
            BodyNodeKind::Unary { op, expr } => {
                parse_checked_helper_unary_op(op).is_some() && self.node_is_contextual_integer(expr)
            }
            BodyNodeKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.node_is_contextual_integer(then_expr)
                    && self.node_is_contextual_integer(else_expr)
            }
            _ => false,
        }
    }

    fn validate_expected_integer_node(
        &self,
        node: &mut BodyNode,
        expected: IntegerDomain,
        nullable: bool,
    ) -> Result<(), Diagnostic> {
        match &mut node.kind {
            BodyNodeKind::Literal(LiteralValue::Null) if nullable => Ok(()),
            BodyNodeKind::Literal(LiteralValue::Integer { domain, .. }) => {
                if let Some(actual) = *domain {
                    if actual != expected {
                        return Err(helper_integer_domain_mismatch(node.span, actual, expected));
                    }
                }
                coerce_helper_integer_literals(node, expected)
            }
            BodyNodeKind::Param { index, .. } => {
                let actual = self
                    .params
                    .get(*index)
                    .and_then(|param| param.integer_domain);
                self.validate_known_expected_domain(node.span, actual, expected)
            }
            BodyNodeKind::Local { id, .. } => {
                let actual =
                    self.locals.iter().find(|local| local.id == *id).and_then(
                        |local| match local.ty {
                            HelperValueType::Integer(domain) => Some(domain),
                            HelperValueType::IntegerLiteral(domain) => domain,
                            _ => None,
                        },
                    );
                self.validate_known_expected_domain(node.span, actual, expected)
            }
            BodyNodeKind::Field { .. } => {
                let actual = match self.value_type(node) {
                    HelperValueType::Integer(domain) => Some(domain),
                    HelperValueType::IntegerLiteral(domain) => domain,
                    _ => None,
                };
                self.validate_known_expected_domain(node.span, actual, expected)
            }
            BodyNodeKind::Binary { op, lhs, rhs } => {
                if let Some((_, actual)) = parse_checked_helper_binary_op(op) {
                    if actual != expected {
                        return Err(helper_integer_domain_mismatch(node.span, actual, expected));
                    }
                    self.validate_expected_integer_node(lhs, expected, false)?;
                    self.validate_expected_integer_node(rhs, expected, false)
                } else {
                    Err(self.non_integer_expected_error(node.span, expected))
                }
            }
            BodyNodeKind::Unary { op, expr } => {
                if let Some(actual) = parse_checked_helper_unary_op(op) {
                    if actual != expected {
                        return Err(helper_integer_domain_mismatch(node.span, actual, expected));
                    }
                    self.validate_expected_integer_node(expr, expected, false)
                } else {
                    Err(self.non_integer_expected_error(node.span, expected))
                }
            }
            BodyNodeKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.contextualize_integer_node(then_expr, expected, nullable)?;
                self.contextualize_integer_node(else_expr, expected, nullable)
            }
            BodyNodeKind::ClientCall { .. } => Ok(()),
            _ => Err(self.non_integer_expected_error(node.span, expected)),
        }
    }

    fn validate_known_expected_domain(
        &self,
        span: Span,
        actual: Option<IntegerDomain>,
        expected: IntegerDomain,
    ) -> Result<(), Diagnostic> {
        match actual {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(helper_integer_domain_mismatch(span, actual, expected)),
            None => Err(self.non_integer_expected_error(span, expected)),
        }
    }

    fn non_integer_expected_error(&self, span: Span, expected: IntegerDomain) -> Diagnostic {
        Diagnostic::new(
            span,
            &format!(
                "rsjs/integer-domain-mismatch: expected `{}` but found a non-integer or unproven value",
                expected.as_str()
            ),
            "use a matching i32, u32, i64, or u64 expression",
            "checked client lowering cannot substitute a runtime conversion for Rust source typing",
        )
    }

    fn lower_tail_match(&mut self, match_expr: &syn::ExprMatch) -> Result<BodyBlock, Diagnostic> {
        if match_expr.arms.is_empty() {
            return Err(self.unsupported_match(match_expr.match_token.span));
        }

        let match_value = self.lower(&match_expr.expr)?;
        let match_ty = self.value_type(&match_value);
        let (match_local, match_name) =
            self.alloc_generated_local("match", match_expr.match_token.span, match_ty);
        let mut stmts = vec![BodyStmt::Let {
            span: match_expr.match_token.span,
            local: match_local,
            name: match_name.clone(),
            mutable: false,
            value: match_value,
        }];

        let mut conditional_arms = Vec::new();
        let mut fallback: Option<BodyBlock> = None;
        for (idx, arm) in match_expr.arms.iter().enumerate() {
            if matches!(arm.pat, syn::Pat::Wild(_)) {
                if let Some((if_token, _)) = &arm.guard {
                    return Err(Diagnostic::new(
                        if_token.span,
                        "`#[rsjs::client]` tail `match` fallback `_` must be unguarded",
                        "move the guard onto an earlier literal/range arm, or make `_` the unconditional final fallback",
                        "generated JavaScript needs an explicit final unmatched-case branch",
                    ));
                }
                if idx + 1 != match_expr.arms.len() {
                    return Err(Diagnostic::new(
                        arm.pat.span(),
                        "`#[rsjs::client]` tail `match` fallback `_` must be the final arm",
                        "move the `_` arm to the end of the match",
                        "arms after an unconditional fallback would be unreachable in the generated Client IR",
                    ));
                }
                fallback = Some(BodyBlock {
                    stmts: vec![BodyStmt::Return {
                        span: arm.body.span(),
                        value: Some(self.lower_match_arm_value(&arm.body)?),
                    }],
                });
                continue;
            }

            let mut condition =
                self.lower_match_pattern_condition(match_local, &match_name, &arm.pat)?;
            if let Some((_, guard_expr)) = &arm.guard {
                let guard = self.lower(guard_expr)?;
                self.require_bool(&guard, guard_expr.span())?;
                condition = BodyNode::binary(arm.span(), "And", condition, guard);
            }
            let body = BodyBlock {
                stmts: vec![BodyStmt::Return {
                    span: arm.body.span(),
                    value: Some(self.lower_match_arm_value(&arm.body)?),
                }],
            };
            conditional_arms.push((condition, body));
        }

        let Some(mut else_block) = fallback else {
            return Err(Diagnostic::new(
                match_expr.match_token.span,
                "`#[rsjs::client]` tail `match` needs a final `_` fallback arm",
                "add `_ => value` as the final arm",
                "the generated JavaScript must have an explicit value for unmatched cases",
            ));
        };

        for (condition, then_block) in conditional_arms.into_iter().rev() {
            else_block = BodyBlock {
                stmts: vec![BodyStmt::If {
                    span: match_expr.match_token.span,
                    condition,
                    then_block,
                    else_block: Some(else_block),
                }],
            };
        }
        stmts.extend(else_block.stmts);
        Ok(BodyBlock { stmts })
    }

    fn lower_match_arm_value(&self, expr: &syn::Expr) -> Result<BodyNode, Diagnostic> {
        match expr {
            syn::Expr::Block(block) => self.lower_block_tail(&block.block),
            other => self.lower(other),
        }
    }

    fn lower_match_pattern_condition(
        &self,
        local: u32,
        local_name: &str,
        pat: &syn::Pat,
    ) -> Result<BodyNode, Diagnostic> {
        match pat {
            syn::Pat::Paren(paren) => {
                self.lower_match_pattern_condition(local, local_name, &paren.pat)
            }
            syn::Pat::Type(typed) => {
                self.lower_match_pattern_condition(local, local_name, &typed.pat)
            }
            syn::Pat::Range(range) => self.lower_match_range_condition(local, local_name, range),
            _ => {
                let mut lhs = BodyNode::local(pat.span(), local, local_name.to_string());
                let mut rhs = self.lower_match_literal_pattern(pat)?;
                let op = self.resolve_numeric_binary_op("Eq", &mut lhs, &mut rhs, pat.span())?;
                Ok(BodyNode::binary(pat.span(), op, lhs, rhs))
            }
        }
    }

    fn lower_match_range_condition(
        &self,
        local: u32,
        local_name: &str,
        range: &syn::PatRange,
    ) -> Result<BodyNode, Diagnostic> {
        let Some(start) = &range.start else {
            return Err(self.unsupported_match(range.span()));
        };
        let Some(end) = &range.end else {
            return Err(self.unsupported_match(range.span()));
        };
        let mut lower_lhs = BodyNode::local(range.span(), local, local_name.to_string());
        let mut lower_rhs = self.lower_match_range_bound(start)?;
        let lower_op =
            self.resolve_numeric_binary_op("Ge", &mut lower_lhs, &mut lower_rhs, range.span())?;
        let lower = BodyNode::binary(range.span(), lower_op, lower_lhs, lower_rhs);
        let mut upper_lhs = BodyNode::local(range.span(), local, local_name.to_string());
        let mut upper_rhs = self.lower_match_range_bound(end)?;
        let upper_name = if matches!(range.limits, syn::RangeLimits::Closed(_)) {
            "Le"
        } else {
            "Lt"
        };
        let upper_op = self.resolve_numeric_binary_op(
            upper_name,
            &mut upper_lhs,
            &mut upper_rhs,
            range.span(),
        )?;
        let upper = BodyNode::binary(range.span(), upper_op, upper_lhs, upper_rhs);
        Ok(BodyNode::binary(range.span(), "And", lower, upper))
    }

    fn lower_match_range_bound(&self, expr: &syn::Expr) -> Result<BodyNode, Diagnostic> {
        match expr {
            syn::Expr::Lit(lit) => self.lower_lit(&lit.lit),
            syn::Expr::Paren(paren) => self.lower_match_range_bound(&paren.expr),
            syn::Expr::Group(group) => self.lower_match_range_bound(&group.expr),
            other => Err(self.unsupported_match(other.span())),
        }
    }

    fn lower_match_literal_pattern(&self, pat: &syn::Pat) -> Result<BodyNode, Diagnostic> {
        match pat {
            syn::Pat::Lit(lit) => self.lower_lit(&lit.lit),
            syn::Pat::Paren(paren) => self.lower_match_literal_pattern(&paren.pat),
            syn::Pat::Type(typed) => self.lower_match_literal_pattern(&typed.pat),
            syn::Pat::Ident(ident)
                if ident.by_ref.is_none()
                    && ident.mutability.is_none()
                    && ident.subpat.is_none()
                    && matches!(ident.ident.to_string().as_str(), "true" | "false") =>
            {
                Ok(BodyNode::literal(
                    ident.span(),
                    LiteralValue::Bool(ident.ident == "true"),
                ))
            }
            other => Err(self.unsupported_match(other.span())),
        }
    }

    fn lower_unary(&self, unary: &syn::ExprUnary) -> Result<BodyNode, Diagnostic> {
        let op = match unary.op {
            syn::UnOp::Not(_) => "Not",
            syn::UnOp::Neg(_) => {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(literal),
                    ..
                }) = unary.expr.as_ref()
                {
                    let domain = if literal.suffix().is_empty() {
                        None
                    } else {
                        Some(IntegerDomain::from_name(literal.suffix()).ok_or_else(|| {
                            unsupported_helper_integer_domain(literal.span(), literal.suffix())
                        })?)
                    };
                    let magnitude = literal.base10_parse::<i128>().map_err(|err| {
                        Diagnostic::new(
                            literal.span(),
                            "integer literal too large for `#[rsjs::client]`",
                            "use a literal inside i32/u32/i64/u64",
                            &err.to_string(),
                        )
                    })?;
                    if domain.is_some_and(|domain| !domain.is_signed()) {
                        return Err(Diagnostic::new(
                            unary.span(),
                            "rsjs/integer-negation-unsigned: unary `-` is not defined for an unsigned helper integer",
                            "use a signed i32/i64 value or remove unary negation",
                            "Rust rejects unary negation for u32/u64 and the client compiler preserves that domain rule",
                        ));
                    }
                    let value = helper_integer_literal_value(-magnitude, domain, unary.span())?;
                    return Ok(BodyNode::literal(unary.span(), value));
                }
                "Neg"
            }
            other => {
                return Err(Diagnostic::new(
                    other.span(),
                    "this unary operator is not allowed in `#[rsjs::client]` bodies",
                    "use unary `!`, unary `-`, or rewrite the expression",
                    "the helper expression subset supports only operators with direct JS \
                     equivalents in Client IR v3",
                ));
            }
        };
        let expr = self.lower(&unary.expr)?;
        if op == "Not" {
            self.require_bool(&expr, unary.expr.span())?;
            return Ok(BodyNode::unary(unary.span(), op, expr));
        }
        let ty = self.value_type(&expr);
        match ty {
            HelperValueType::Float => Ok(BodyNode::unary(unary.span(), "Neg", expr)),
            HelperValueType::Integer(domain) if domain.is_signed() => Ok(BodyNode::unary(
                unary.span(),
                checked_helper_unary_op(domain),
                expr,
            )),
            HelperValueType::Integer(domain) => Err(Diagnostic::new(
                unary.span(),
                &format!(
                    "rsjs/integer-negation-unsigned: unary `-` is not defined for `{}`",
                    domain.as_str()
                ),
                "use a signed i32/i64 operand",
                "checked client lowering preserves Rust's unsigned negation rejection",
            )),
            HelperValueType::IntegerLiteral(Some(domain)) if !domain.is_signed() => {
                Err(Diagnostic::new(
                    unary.span(),
                    &format!(
                        "rsjs/integer-negation-unsigned: unary `-` is not defined for `{}`",
                        domain.as_str()
                    ),
                    "use a signed i32/i64 operand",
                    "checked client lowering preserves Rust's unsigned negation rejection",
                ))
            }
            HelperValueType::IntegerLiteral(Some(domain)) => Ok(BodyNode::unary(
                unary.span(),
                checked_helper_unary_op(domain),
                expr,
            )),
            HelperValueType::IntegerLiteral(None) => {
                let mut expr = expr;
                coerce_helper_integer_literals(&mut expr, IntegerDomain::I32)?;
                Ok(BodyNode::unary(
                    unary.span(),
                    checked_helper_unary_op(IntegerDomain::I32),
                    expr,
                ))
            }
            HelperValueType::IntegerUnproven | HelperValueType::Unknown => Err(Diagnostic::new(
                unary.span(),
                "rsjs/integer-domain-unproven: helper negation has no proven signed integer domain",
                "use an i32 or i64 parameter/typed local",
                "native JavaScript negation cannot reproduce fixed-width MIN overflow",
            )),
            _ => Ok(BodyNode::unary(unary.span(), "Neg", expr)),
        }
    }

    fn lower_if(&self, if_expr: &syn::ExprIf) -> Result<BodyNode, Diagnostic> {
        let condition = self.lower(&if_expr.cond)?;
        self.require_bool(&condition, if_expr.cond.span())?;
        let then_expr = self.lower_block_tail(&if_expr.then_branch)?;
        let Some((_, else_expr)) = &if_expr.else_branch else {
            return Err(Diagnostic::new(
                if_expr.if_token.span(),
                "`#[rsjs::client]` value-level `if` expressions must have an `else` branch",
                "add an `else` branch that returns the same primitive shape",
                "client helpers lower to expressions; without `else`, Rust's value is `()` \
                 and there is no JS expression counterpart",
            ));
        };
        let else_expr = match &**else_expr {
            syn::Expr::If(nested) => self.lower_if(nested)?,
            syn::Expr::Block(block) => self.lower_block_tail(&block.block)?,
            other => self.lower(other)?,
        };
        Ok(BodyNode::conditional(
            if_expr.span(),
            condition,
            then_expr,
            else_expr,
        ))
    }

    fn value_type(&self, node: &BodyNode) -> HelperValueType {
        match &node.kind {
            BodyNodeKind::Literal(value) => match value {
                LiteralValue::Null => HelperValueType::Option,
                LiteralValue::Bool(_) => HelperValueType::Bool,
                LiteralValue::Integer { domain, .. } => HelperValueType::IntegerLiteral(*domain),
                LiteralValue::Float(_) => HelperValueType::Float,
                LiteralValue::String(_) => HelperValueType::String,
            },
            BodyNodeKind::Param { index, .. } => self
                .params
                .get(*index)
                .map(|param| {
                    if param.object_shape.is_some() {
                        HelperValueType::Object
                    } else {
                        param
                            .integer_domain
                            .map(HelperValueType::Integer)
                            .unwrap_or_else(|| HelperValueType::from_type_name(&param.ty_name))
                    }
                })
                .unwrap_or(HelperValueType::Unknown),
            BodyNodeKind::Local { id, .. } => self
                .locals
                .iter()
                .find(|local| local.id == *id)
                .map(|local| local.ty)
                .unwrap_or(HelperValueType::Unknown),
            BodyNodeKind::StructInit { .. } => HelperValueType::Object,
            BodyNodeKind::Field { base, name } => self
                .object_shape_for_node(base)
                .and_then(|shape| {
                    shape
                        .fields
                        .into_iter()
                        .find_map(|(field, ty)| (field == *name).then_some(ty))
                })
                .unwrap_or(HelperValueType::Unknown),
            BodyNodeKind::Binary { op, lhs, rhs } => match *op {
                "Eq" | "Neq" | "Lt" | "Gt" | "Le" | "Ge" | "And" | "Or" => HelperValueType::Bool,
                "Add" | "Sub" | "Mul" | "Div" | "Rem" => {
                    let lhs = self.value_type(lhs);
                    let rhs = self.value_type(rhs);
                    if lhs == rhs {
                        lhs
                    } else {
                        HelperValueType::Unknown
                    }
                }
                _ => parse_checked_helper_binary_op(op)
                    .map(|(_, domain)| HelperValueType::Integer(domain))
                    .unwrap_or(HelperValueType::Unknown),
            },
            BodyNodeKind::Unary { op: "Not", .. } => HelperValueType::Bool,
            BodyNodeKind::Unary { op, expr } => parse_checked_helper_unary_op(op)
                .map(HelperValueType::Integer)
                .unwrap_or_else(|| self.value_type(expr)),
            BodyNodeKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let then_ty = self.value_type(then_expr);
                let else_ty = self.value_type(else_expr);
                if then_ty == else_ty {
                    then_ty
                } else {
                    HelperValueType::Unknown
                }
            }
            BodyNodeKind::IsNonEmpty(_)
            | BodyNodeKind::StringContains { .. }
            | BodyNodeKind::StringPredicate { .. } => HelperValueType::Bool,
            BodyNodeKind::Len(_) => HelperValueType::IntegerUnproven,
            BodyNodeKind::StringToLowercase(_) | BodyNodeKind::StringTransform { .. } => {
                HelperValueType::String
            }
            BodyNodeKind::ClientCall { .. } => HelperValueType::Unknown,
        }
    }

    fn require_bool(&self, node: &BodyNode, span: Span) -> Result<(), Diagnostic> {
        let ty = self.value_type(node);
        if matches!(ty, HelperValueType::Bool | HelperValueType::Unknown) {
            return Ok(());
        }
        Err(Diagnostic::new(
            span,
            "`#[rsjs::client]` conditions must have Rust `bool` semantics",
            "compare the value explicitly, for example `count != 0` or `text != \"\"`",
            "JavaScript truthiness is not Rust semantics; this condition's concrete helper type is not `bool`",
        ))
    }

    fn lower_block_tail(&self, block: &syn::Block) -> Result<BodyNode, Diagnostic> {
        if block.stmts.len() != 1 {
            return Err(Diagnostic::new(
                block.span(),
                "`#[rsjs::client]` blocks must contain exactly one expression",
                "rewrite the block as a single expression",
                "the helper body lowers to one client expression; statements and local \
                 declarations are not part of the helper expression subset yet",
            ));
        }
        match &block.stmts[0] {
            syn::Stmt::Expr(expr, None) => self.lower(expr),
            syn::Stmt::Expr(_, Some(semi)) => Err(Diagnostic::new(
                semi.span(),
                "`#[rsjs::client]` blocks must end with an expression, not a statement",
                "remove the trailing `;`",
                "a trailing semicolon turns the branch into `()`, which does not preserve \
                 the helper's primitive return value",
            )),
            other => Err(Diagnostic::new(
                other.span(),
                "`#[rsjs::client]` blocks cannot contain statements yet",
                "rewrite the branch as a single expression",
                "local declarations in helper bodies are reserved for the statement-level \
                 Client IR lowering milestone",
            )),
        }
    }

    fn lower_lit(&self, lit: &syn::Lit) -> Result<BodyNode, Diagnostic> {
        match lit {
            syn::Lit::Bool(b) => Ok(BodyNode::literal(b.span(), LiteralValue::Bool(b.value))),
            syn::Lit::Int(i) => {
                let suffix = i.suffix();
                if !suffix.is_empty() && IntegerDomain::from_name(suffix).is_none() {
                    return Err(unsupported_helper_integer_domain(i.span(), suffix));
                }
                let v = i.base10_parse::<i128>().map_err(|err| {
                    Diagnostic::new(
                        i.span(),
                        "integer literal too large for `#[rsjs::client]`",
                        "use a literal that fits in the supported i32/u32/i64/u64 domains",
                        &format!("{}", err),
                    )
                })?;
                let value =
                    helper_integer_literal_value(v, IntegerDomain::from_name(suffix), i.span())?;
                Ok(BodyNode::literal(i.span(), value))
            }
            syn::Lit::Float(f) => {
                if f.suffix() == "f32" {
                    return Err(unsupported_helper_f32_domain(f.span()));
                }
                let v = f.base10_parse::<f64>().map_err(|err| {
                    Diagnostic::new(
                        f.span(),
                        "float literal not parseable as f64",
                        "use a literal that fits in f64",
                        &format!("{}", err),
                    )
                })?;
                Ok(BodyNode::literal(f.span(), LiteralValue::Float(v)))
            }
            syn::Lit::Str(s) => Ok(BodyNode::literal(s.span(), LiteralValue::String(s.value()))),
            other => Err(Diagnostic::new(
                other.span(),
                "this literal kind is not supported in `#[rsjs::client]` bodies",
                "use a bool, integer, float, or string literal",
                "v0.1 lowers only the JSON-primitive literals; chars, byte strings, \
                 and raw byte literals arrive later if a use case demands them",
            )),
        }
    }

    fn lower_path(&self, p: &syn::ExprPath) -> Result<BodyNode, Diagnostic> {
        if p.qself.is_none() && p.path.segments.len() >= 2 {
            let root = p
                .path
                .segments
                .first()
                .map(|segment| segment.ident.to_string());
            if let Some(root) = root.filter(|root| is_unsupported_checked_integer_name(root)) {
                return Err(unsupported_helper_integer_domain(p.span(), &root));
            }
        }
        if p.qself.is_some() || p.path.leading_colon.is_some() || p.path.segments.len() != 1 {
            return Err(Diagnostic::new(
                p.span(),
                "`#[rsjs::client]` bodies cannot reference paths from outside the helper",
                "use a function parameter, a literal, or another `#[rsjs::client]` helper call",
                "outer-scope captures are forbidden so the JS counterpart can be a pure \
                 function of its arguments",
            ));
        }
        let ident = &p.path.segments[0].ident;
        let name = ident.to_string();
        if name == "None" {
            return Ok(BodyNode::literal(p.span(), LiteralValue::Null));
        }
        if name == "self" || name == "Self" {
            return Err(Diagnostic::new(
                ident.span(),
                "`self` is not in scope inside a `#[rsjs::client]` helper",
                "remove the `self` reference; client helpers are free functions",
                "client helpers have no enclosing receiver",
            ));
        }
        if name == "cx" {
            return Err(Diagnostic::new(
                ident.span(),
                "`cx` is not accessible from a `#[rsjs::client]` helper",
                "pass the value you need from `cx` in as a function parameter",
                "the render context lives in the island's render method, not in client helpers",
            ));
        }
        if let Some(p) = self.params.iter().find(|p| p.name == *ident) {
            return Ok(BodyNode::param(ident.span(), p.index, name.clone()));
        }
        if let Some(local) = self
            .locals
            .iter()
            .find(|local| local.visible && local.name == *ident)
        {
            if let Some(constant) = &local.constant {
                return Ok(BodyNode::literal(ident.span(), constant.clone()));
            }
            return Ok(BodyNode::local(ident.span(), local.id, name.clone()));
        }
        Err(Diagnostic::new(
            ident.span(),
            &format!("unknown identifier `{name}` in `#[rsjs::client]` body"),
            "reference one of the helper's parameters, a literal, or call another \
             `#[rsjs::client]` helper",
            "client helpers cannot capture from outer scope; everything they need must \
             flow through their argument list",
        ))
    }

    fn lower_method_call(&self, m: &syn::ExprMethodCall) -> Result<BodyNode, Diagnostic> {
        // v0.1 method allowlist: `is_non_empty()` for parity with the
        // M5 form fixture, Rust's more idiomatic `is_empty()` as the
        // inverse of the same IR node, `len()` as a runtime-call
        // expression that can become first-class Client IR later, and
        // the string helpers already present in shared Client IR.
        // `to_string()` is accepted only on a proven string value and
        // lowers as an identity; it lets ordinary Rust own a borrowed
        // transform result such as `value.trim().to_string()`.
        let method = m.method.to_string();
        let is_allowed = matches!(
            method.as_str(),
            "is_non_empty"
                | "is_empty"
                | "len"
                | "to_lowercase"
                | "to_uppercase"
                | "trim"
                | "to_string"
                | "contains"
                | "starts_with"
                | "ends_with"
                | "is_some"
                | "is_none"
        );
        if !is_allowed {
            return Err(Diagnostic::new(
                m.method.span(),
                &format!("method `.{method}(...)` is not supported in `#[rsjs::client]` bodies"),
                "use `.is_non_empty()`, `.is_empty()`, `.len()`, `.to_lowercase()`, `.to_uppercase()`, `.trim()`, `.to_string()`, `.contains(...)`, `.starts_with(...)`, `.ends_with(...)`, `.is_some()`, `.is_none()`, or a binary operator; \
                 wrap heavier logic in another `#[rsjs::client]` helper",
                "the v0.1 IR's call surface is exactly the `Expr` enum's variants. \
                 Method-resolution at the macro layer would require type information \
                 we don't have; the allowlist keeps the contract honest.",
            ));
        }
        let expected_args = if matches!(method.as_str(), "contains" | "starts_with" | "ends_with") {
            1
        } else {
            0
        };
        if m.args.len() != expected_args {
            return Err(Diagnostic::new(
                m.args.span(),
                &format!("`.{method}()` expects {expected_args} argument(s)"),
                "adjust the method arguments",
                "the helper method allowlist maps each method to a fixed Client IR expression shape",
            ));
        }
        if let Some(turbofish) = &m.turbofish {
            return Err(Diagnostic::new(
                turbofish.span(),
                &format!("`.{method}()` is not generic"),
                "remove the turbofish",
                "the JS counterpart performs a runtime non-emptiness check; there is no \
                 type to specialise",
            ));
        }
        let target = self.lower(&m.receiver)?;
        let target_ty = self.value_type(&target);
        if target_ty == HelperValueType::Object {
            return Err(Diagnostic::new(
                m.receiver.span(),
                "client value objects do not expose collection or string methods",
                "call the method on a named scalar field instead",
                "the object itself is an ephemeral record; inherent-method lowering is a separate compiler step",
            ));
        }
        match method.as_str() {
            "len" if !matches!(target_ty, HelperValueType::String | HelperValueType::Array) => {
                return Err(Diagnostic::new(
                    m.receiver.span(),
                    "`#[rsjs::client]` `.len()` needs a Rust String, Vec, or slice receiver",
                    "use a concretely typed String, Vec<T>, or &[T] value",
                    "the macro rejects unknown receiver types instead of guessing JavaScript object or UTF-16 length semantics",
                ));
            }
            "to_lowercase" | "to_uppercase" | "trim" | "to_string" | "contains" | "starts_with"
            | "ends_with"
                if target_ty != HelperValueType::String =>
            {
                return Err(Diagnostic::new(
                    m.receiver.span(),
                    "`#[rsjs::client]` string methods need a concrete Rust String or &str receiver",
                    "use a String or &str parameter/local, or make the conversion explicit before the helper boundary",
                    "the macro rejects unknown receiver types instead of guessing JavaScript coercion semantics",
                ));
            }
            _ => {}
        }
        match method.as_str() {
            "is_non_empty" => Ok(BodyNode::is_non_empty(m.span(), target)),
            "is_empty" => Ok(BodyNode::unary(
                m.span(),
                "Not",
                BodyNode::is_non_empty(m.span(), target),
            )),
            "len" => Ok(BodyNode::len(m.span(), target)),
            "to_lowercase" => Ok(BodyNode::string_to_lowercase(m.span(), target)),
            "to_uppercase" => Ok(BodyNode::string_transform(
                m.span(),
                StringTransformOp::ToUppercase,
                target,
            )),
            "trim" => Ok(BodyNode::string_transform(
                m.span(),
                StringTransformOp::Trim,
                target,
            )),
            "to_string" => Ok(target),
            "contains" => Ok(BodyNode::string_contains(
                m.span(),
                target,
                self.lower(&m.args[0])?,
            )),
            "starts_with" => Ok(BodyNode::string_predicate(
                m.span(),
                StringPredicateOp::StartsWith,
                target,
                self.lower(&m.args[0])?,
            )),
            "ends_with" => Ok(BodyNode::string_predicate(
                m.span(),
                StringPredicateOp::EndsWith,
                target,
                self.lower(&m.args[0])?,
            )),
            "is_some" => Ok(BodyNode::binary(
                m.span(),
                "Neq",
                target,
                BodyNode::literal(m.method.span(), LiteralValue::Null),
            )),
            "is_none" => Ok(BodyNode::binary(
                m.span(),
                "Eq",
                target,
                BodyNode::literal(m.method.span(), LiteralValue::Null),
            )),
            _ => unreachable!("method allowlist checked above"),
        }
    }

    fn lower_call(&self, c: &syn::ExprCall) -> Result<BodyNode, Diagnostic> {
        let path = match &*c.func {
            syn::Expr::Path(p) if p.qself.is_none() => &p.path,
            other => {
                return Err(Diagnostic::new(
                    other.span(),
                    "`#[rsjs::client]` bodies can only call paths (no dynamic call expressions)",
                    "replace the call with a direct path call to another `#[rsjs::client]` helper",
                    "dynamic dispatch has no client-side counterpart in v0.1",
                ));
            }
        };
        if let Some(arguments) = path.segments.iter().find_map(|segment| {
            (!matches!(segment.arguments, syn::PathArguments::None)).then_some(&segment.arguments)
        }) {
            return Err(Diagnostic::new(
                arguments.span(),
                "`#[rsjs::client]` helper calls cannot use explicit path arguments",
                "remove the turbofish/path arguments and call a concrete standalone helper",
                "standalone helper identity names one concrete hashed signature; preserving generic path arguments would require rustc monomorphization evidence that the helper transport does not carry",
            ));
        }
        if path.leading_colon.is_none() && path.segments.len() == 1 && path.is_ident("Some") {
            if c.args.len() != 1 {
                return Err(Diagnostic::new(
                    c.args.span(),
                    "`Some(...)` in a `#[rsjs::client]` helper expects exactly one argument",
                    "wrap one primitive expression in `Some(...)`, or return `None`",
                    "`Option<T>` helpers lower `Some(value)` as the contained JSON-shaped value \
                     and `None` as a null literal",
                ));
            }
            return self.lower(&c.args[0]);
        }
        let callee = DirectClientCall {
            path: path.clone(),
            authored_path: canonical_helper_path_spelling(path),
        };
        let mut args = Vec::with_capacity(c.args.len());
        for a in &c.args {
            args.push(self.lower(a)?);
        }
        Ok(BodyNode::client_call(c.span(), callee, args))
    }

    fn unsupported(&self, expr: &syn::Expr) -> Diagnostic {
        Diagnostic::new(
            expr.span(),
            "this expression is not allowed in a `#[rsjs::client]` body",
            "rewrite using literals, parameter/local references, binary operators, \
             `.is_non_empty()`, `.is_some()`, or calls to other `#[rsjs::client]` helpers",
            "the v0.1 surface is the `Expr` IR plus parameter/local reads. Anything \
             outside that set has no JS counterpart at this milestone.",
        )
    }

    fn unsupported_match(&self, span: Span) -> Diagnostic {
        Diagnostic::new(
            span,
            "this `match` pattern is not supported in a `#[rsjs::client]` helper yet",
            "use literal arms, bounded range arms, optional guards, and a final `_` fallback",
            "client helper tail matches currently lower to Client IR `let` + nested `if` / \
             `return` blocks. Path patterns, bindings, and nested destructuring need richer \
             typed pattern lowering",
        )
    }
}

/// Produce stable descriptor/diagnostic text for an authored helper path.
///
/// This spelling is never reparsed for name resolution. The original
/// [`syn::Path`] remains the only compiler-bearing representation. Removing
/// token-printer whitespace keeps the legacy diagnostic convention
/// (`crate::module::helper`) while retaining an authored leading `::`.
fn canonical_helper_path_spelling(path: &syn::Path) -> String {
    path.to_token_stream()
        .to_string()
        .split_whitespace()
        .collect()
}

fn is_self_or_cx_root(expr: &syn::Expr) -> bool {
    if let syn::Expr::Path(p) = expr {
        if let Some(seg) = p.path.segments.first() {
            let s = seg.ident.to_string();
            return s == "self" || s == "Self" || s == "cx";
        }
    }
    false
}

// ---------------------------------------------------------------------------
// JSON serialisation
// ---------------------------------------------------------------------------

impl HelperBody {
    fn to_json(&self) -> String {
        let mut out = String::new();
        match self {
            HelperBody::Expr(expr) => expr.write_json(&mut out),
            HelperBody::Block(block) => block.write_json(&mut out),
        }
        out
    }

    fn to_ir_json(&self) -> String {
        match self {
            HelperBody::Expr(expr) => client_expr_json(&expr.to_unresolved_client_expr()),
            HelperBody::Block(block) => client_block_json(&block.to_client_block()),
        }
    }

    fn to_server_eval_tokens(&self) -> TokenStream {
        let args = format_ident!("__rsjs_args");
        match self {
            HelperBody::Expr(expr) => expr.write_server_eval_tokens(&args),
            HelperBody::Block(block) => block.write_server_eval_tokens(&args),
        }
    }

    fn direct_client_calls(&self) -> Vec<DirectClientCall> {
        let mut calls = Vec::new();
        match self {
            HelperBody::Expr(expr) => expr.collect_direct_client_calls(&mut calls),
            HelperBody::Block(block) => block.collect_direct_client_calls(&mut calls),
        }
        calls
    }

    #[cfg(test)]
    fn to_js_expr(&self, params: &[ParamInfo], bundle_safe_name: Option<&str>) -> String {
        match self {
            HelperBody::Expr(expr) => expr.to_js_expr(params, bundle_safe_name),
            HelperBody::Block(block) => block.to_iife_js_expr(params, bundle_safe_name),
        }
    }
}

#[cfg(test)]
fn helper_local_js_name(id: u32) -> String {
    format!("__local_{id}")
}

fn helper_local_rust_ident(id: u32) -> Ident {
    format_ident!("__rsjs_client_local_{id}")
}

impl BodyBlock {
    fn collect_direct_client_calls(&self, calls: &mut Vec<DirectClientCall>) {
        for stmt in &self.stmts {
            stmt.collect_direct_client_calls(calls);
        }
    }

    fn to_client_block(&self) -> ClientBlock {
        ClientBlock::from_stmts(
            self.stmts
                .iter()
                .map(BodyStmt::to_client_stmt)
                .collect::<Vec<_>>(),
        )
    }

    fn write_json(&self, out: &mut String) {
        out.push_str(r#"{"kind":"Block","statements":["#);
        for (idx, stmt) in self.stmts.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            stmt.write_json(out);
        }
        out.push_str("]}");
    }

    fn write_server_statement_tokens(&self, args: &Ident) -> TokenStream {
        let stmts: Vec<TokenStream> = self
            .stmts
            .iter()
            .map(|stmt| stmt.write_server_statement_tokens(args))
            .collect();
        quote! {
            #( #stmts )*
        }
    }

    fn write_server_eval_tokens(&self, args: &Ident) -> TokenStream {
        let Some((last, prefix)) = self.stmts.split_last() else {
            return quote! { ::rsjs::__runtime::ClientValue::Unit };
        };
        let prefix_tokens: Vec<TokenStream> = prefix
            .iter()
            .map(|stmt| stmt.write_server_statement_tokens(args))
            .collect();
        match last {
            BodyStmt::Return {
                value: Some(value), ..
            } => {
                let value = value.write_server_eval_tokens(args);
                quote! {
                    #( #prefix_tokens )*
                    #value
                }
            }
            BodyStmt::Return { value: None, .. } => quote! {
                #( #prefix_tokens )*
                ::rsjs::__runtime::ClientValue::Unit
            },
            BodyStmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition = condition.write_server_eval_tokens(args);
                let then_block = then_block.write_server_eval_tokens(args);
                let else_block = else_block.as_ref().map_or_else(
                    || quote! { ::rsjs::__runtime::ClientValue::Unit },
                    |block| block.write_server_eval_tokens(args),
                );
                quote! {
                    #( #prefix_tokens )*
                    if ::rsjs::__runtime::client_bool(#condition) {
                        #then_block
                    } else {
                        #else_block
                    }
                }
            }
            other => {
                let last = other.write_server_statement_tokens(args);
                quote! {
                    #( #prefix_tokens )*
                    #last
                    ::rsjs::__runtime::ClientValue::Unit
                }
            }
        }
    }

    #[cfg(test)]
    fn to_iife_js_expr(&self, params: &[ParamInfo], bundle_safe_name: Option<&str>) -> String {
        let mut out = String::from("(() => {");
        self.write_js_code(params, bundle_safe_name, &mut out);
        out.push_str("})()");
        out
    }

    #[cfg(test)]
    fn write_js_code(
        &self,
        params: &[ParamInfo],
        bundle_safe_name: Option<&str>,
        out: &mut String,
    ) {
        for stmt in &self.stmts {
            stmt.write_js_code(params, bundle_safe_name, out);
        }
    }
}

impl BodyStmt {
    fn collect_direct_client_calls(&self, calls: &mut Vec<DirectClientCall>) {
        match self {
            BodyStmt::Let { value, .. }
            | BodyStmt::AssignLocal { value, .. }
            | BodyStmt::AssignField { value, .. }
            | BodyStmt::Expr { value, .. } => value.collect_direct_client_calls(calls),
            BodyStmt::Return { value, .. } => {
                if let Some(value) = value {
                    value.collect_direct_client_calls(calls);
                }
            }
            BodyStmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                condition.collect_direct_client_calls(calls);
                then_block.collect_direct_client_calls(calls);
                if let Some(else_block) = else_block {
                    else_block.collect_direct_client_calls(calls);
                }
            }
        }
    }

    fn to_client_stmt(&self) -> ClientStmt {
        match self {
            BodyStmt::Let {
                local, name, value, ..
            } => ClientStmt::Let {
                local: ClientLocalId(*local),
                name: name.clone(),
                // Standalone helper locals are inferred by the closed-world
                // verifier from their initializer. Persisting an initializer
                // hint here bypasses that protocol and is rejected by the
                // semantic linker.
                ty: ClientType::Unknown,
                value: Some(value.to_unresolved_client_expr()),
            },
            BodyStmt::AssignLocal { local, value, .. } => ClientStmt::Assign {
                local: ClientLocalId(*local),
                value: value.to_unresolved_client_expr(),
            },
            BodyStmt::AssignField {
                local,
                field,
                value,
                ..
            } => ClientStmt::AssignPlace {
                place: ClientPlace::field(ClientPlace::local(ClientLocalId(*local)), field),
                value: value.to_unresolved_client_expr(),
            },
            BodyStmt::Expr { value, .. } => ClientStmt::Expr(value.to_unresolved_client_expr()),
            BodyStmt::Return { value, .. } => {
                ClientStmt::Return(value.as_ref().map(BodyNode::to_unresolved_client_expr))
            }
            BodyStmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => ClientStmt::If {
                condition: condition.to_unresolved_client_expr(),
                then_block: then_block.to_client_block(),
                else_block: else_block.as_ref().map(BodyBlock::to_client_block),
            },
        }
    }

    fn write_json(&self, out: &mut String) {
        use std::fmt::Write as _;

        match self {
            BodyStmt::Let {
                span,
                local,
                name,
                mutable,
                value,
            } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                let _ = write!(out, r#","kind":"Let","local":{local},"name":""#);
                push_string_escaped(out, name);
                if *mutable {
                    out.push_str(r#"","mutable":true,"value":"#);
                } else {
                    out.push_str(r#"","value":"#);
                }
                value.write_json(out);
                out.push('}');
            }
            BodyStmt::AssignLocal { span, local, value } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                let _ = write!(out, r#","kind":"AssignLocal","local":{local},"value":"#);
                value.write_json(out);
                out.push('}');
            }
            BodyStmt::AssignField {
                span,
                local,
                field,
                value,
            } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                let _ = write!(out, r#","kind":"AssignField","local":{local},"field":""#);
                push_string_escaped(out, field);
                out.push_str(r#"","value":"#);
                value.write_json(out);
                out.push('}');
            }
            BodyStmt::Expr { span, value } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                out.push_str(r#","kind":"Expr","value":"#);
                value.write_json(out);
                out.push('}');
            }
            BodyStmt::Return { span, value } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                out.push_str(r#","kind":"Return","value":"#);
                match value {
                    Some(value) => value.write_json(out),
                    None => out.push_str("null"),
                }
                out.push('}');
            }
            BodyStmt::If {
                span,
                condition,
                then_block,
                else_block,
            } => {
                out.push_str(r#"{"span":"#);
                push_span_json(out, *span);
                out.push_str(r#","kind":"If","condition":"#);
                condition.write_json(out);
                out.push_str(r#","then":"#);
                then_block.write_json(out);
                out.push_str(r#","else":"#);
                match else_block {
                    Some(block) => block.write_json(out),
                    None => out.push_str("null"),
                }
                out.push('}');
            }
        }
    }

    fn write_server_statement_tokens(&self, args: &Ident) -> TokenStream {
        match self {
            BodyStmt::Let {
                local,
                mutable,
                value,
                ..
            } => {
                let local = helper_local_rust_ident(*local);
                let value = value.write_server_eval_tokens(args);
                if *mutable {
                    quote! {
                        let mut #local = #value;
                    }
                } else {
                    quote! {
                        let #local = #value;
                    }
                }
            }
            BodyStmt::AssignLocal { local, value, .. } => {
                let local = helper_local_rust_ident(*local);
                let value = value.write_server_eval_tokens(args);
                quote! {
                    {
                        let __rsjs_client_next_local_value = #value;
                        #local = __rsjs_client_next_local_value;
                    }
                }
            }
            BodyStmt::AssignField {
                local,
                field,
                value,
                ..
            } => {
                let local = helper_local_rust_ident(*local);
                let field = syn::LitStr::new(field, Span::call_site());
                let value = value.write_server_eval_tokens(args);
                quote! {
                    {
                        let __rsjs_client_next_field_value = #value;
                        ::rsjs::__runtime::client_set_field(
                            &mut #local,
                            #field,
                            __rsjs_client_next_field_value,
                        );
                    }
                }
            }
            BodyStmt::Expr { value, .. } => {
                let value = value.write_server_eval_tokens(args);
                quote! {
                    let _ = #value;
                }
            }
            BodyStmt::Return { value, .. } => match value {
                Some(value) => {
                    let value = value.write_server_eval_tokens(args);
                    quote! {
                        return #value;
                    }
                }
                None => quote! {
                    return ::rsjs::__runtime::ClientValue::Unit;
                },
            },
            BodyStmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition = condition.write_server_eval_tokens(args);
                let then_block = then_block.write_server_statement_tokens(args);
                let else_block = else_block.as_ref().map(|block| {
                    let block = block.write_server_statement_tokens(args);
                    quote! {
                        else {
                            #block
                        }
                    }
                });
                quote! {
                    if ::rsjs::__runtime::client_bool(#condition) {
                        #then_block
                    } #else_block
                }
            }
        }
    }

    #[cfg(test)]
    fn write_js_code(
        &self,
        params: &[ParamInfo],
        bundle_safe_name: Option<&str>,
        out: &mut String,
    ) {
        use std::fmt::Write as _;

        match self {
            BodyStmt::Let {
                local,
                mutable,
                value,
                ..
            } => {
                let _ = write!(
                    out,
                    "{binding} {name} = {value}",
                    binding = if *mutable { "let" } else { "const" },
                    name = helper_local_js_name(*local),
                    value = value.to_js_expr(params, bundle_safe_name)
                );
                out.push(';');
            }
            BodyStmt::AssignLocal { local, value, .. } => {
                let _ = write!(
                    out,
                    "{name} = {value};",
                    name = helper_local_js_name(*local),
                    value = value.to_js_expr(params, bundle_safe_name)
                );
            }
            BodyStmt::AssignField {
                local,
                field,
                value,
                ..
            } => {
                let _ = write!(out, "{}[\"", helper_local_js_name(*local));
                push_string_escaped(out, field);
                let _ = write!(out, "\"] = {};", value.to_js_expr(params, bundle_safe_name));
            }
            BodyStmt::Expr { value, .. } => {
                out.push_str(&value.to_js_expr(params, bundle_safe_name));
                out.push(';');
            }
            BodyStmt::Return { value, .. } => {
                out.push_str("return");
                if let Some(value) = value {
                    out.push(' ');
                    out.push_str(&value.to_js_expr(params, bundle_safe_name));
                }
                out.push(';');
            }
            BodyStmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let _ = write!(
                    out,
                    "if ({condition}) {{",
                    condition = condition.to_js_expr(params, bundle_safe_name)
                );
                then_block.write_js_code(params, bundle_safe_name, out);
                out.push('}');
                if let Some(block) = else_block {
                    out.push_str("else {");
                    block.write_js_code(params, bundle_safe_name, out);
                    out.push('}');
                }
            }
        }
    }
}

#[derive(Clone)]
enum HelperCallLowering {
    Unresolved,
    #[cfg(test)]
    Runtime {
        bundle_safe_name: Option<String>,
    },
}

impl BodyNode {
    fn to_unresolved_client_expr(&self) -> ClientExpr {
        self.to_client_expr_with_calls(&HelperCallLowering::Unresolved)
    }

    fn to_client_expr_with_calls(&self, helper_calls: &HelperCallLowering) -> ClientExpr {
        match &self.kind {
            BodyNodeKind::Literal(v) => ClientExpr::Literal(v.to_value()),
            BodyNodeKind::Param { index, .. } => ClientExpr::local(ClientLocalId(*index as u32)),
            BodyNodeKind::Local { id, .. } => ClientExpr::local(ClientLocalId(*id)),
            BodyNodeKind::StructInit { type_name, fields } => ClientExpr::StructInit {
                type_name: type_name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| ClientStructFieldValue {
                        name: name.clone(),
                        value: value.to_client_expr_with_calls(helper_calls),
                    })
                    .collect(),
            },
            BodyNodeKind::Field { base, name } => ClientExpr::Field {
                base: Box::new(base.to_client_expr_with_calls(helper_calls)),
                name: name.clone(),
            },
            BodyNodeKind::Binary { op, lhs, rhs } => ClientExpr::binary(
                client_binary_op(op),
                lhs.to_client_expr_with_calls(helper_calls),
                rhs.to_client_expr_with_calls(helper_calls),
            ),
            BodyNodeKind::Unary { op, expr } => ClientExpr::unary(
                client_unary_op(op),
                expr.to_client_expr_with_calls(helper_calls),
            ),
            BodyNodeKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => ClientExpr::Conditional {
                condition: Box::new(condition.to_client_expr_with_calls(helper_calls)),
                then_expr: Box::new(then_expr.to_client_expr_with_calls(helper_calls)),
                else_expr: Box::new(else_expr.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::IsNonEmpty(target) => ClientExpr::IsNonEmpty {
                target: Box::new(target.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::Len(target) => ClientExpr::Call {
                function: ClientFunctionRef::Runtime(
                    rsjs_ir::CLIENT_HELPER_LEN_INTRINSIC_JS.to_owned(),
                ),
                args: vec![target.to_client_expr_with_calls(helper_calls)],
            },
            BodyNodeKind::StringToLowercase(target) => ClientExpr::StringToLowercase {
                target: Box::new(target.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::StringContains { haystack, needle } => ClientExpr::StringContains {
                haystack: Box::new(haystack.to_client_expr_with_calls(helper_calls)),
                needle: Box::new(needle.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::StringTransform { op, target } => ClientExpr::StringTransform {
                op: *op,
                target: Box::new(target.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::StringPredicate {
                op,
                haystack,
                needle,
            } => ClientExpr::StringPredicate {
                op: *op,
                haystack: Box::new(haystack.to_client_expr_with_calls(helper_calls)),
                needle: Box::new(needle.to_client_expr_with_calls(helper_calls)),
            },
            BodyNodeKind::ClientCall { callee, args } => {
                let function = match helper_calls {
                    HelperCallLowering::Unresolved => {
                        ClientFunctionRef::Unresolved(callee.authored_path.clone())
                    }
                    #[cfg(test)]
                    HelperCallLowering::Runtime { bundle_safe_name } => ClientFunctionRef::Runtime(
                        helper_call_export_name(&callee.authored_path, bundle_safe_name.as_deref()),
                    ),
                };
                ClientExpr::call(
                    function,
                    args.iter()
                        .map(|arg| arg.to_client_expr_with_calls(helper_calls))
                        .collect(),
                )
            }
        }
    }

    /// Lower this body node to a JavaScript expression string through
    /// Client IR v3. Kept in tests as an equivalence check for the
    /// metadata-time writer emitted by `#[rsjs::client]`.
    #[cfg(test)]
    pub(crate) fn to_js_expr(
        &self,
        params: &[ParamInfo],
        bundle_safe_name: Option<&str>,
    ) -> String {
        let expr = self.to_client_expr_with_calls(&HelperCallLowering::Runtime {
            bundle_safe_name: bundle_safe_name.map(str::to_string),
        });
        let ctx = params.iter().fold(ClientEmitContext::default(), |ctx, p| {
            ctx.with_local_name(ClientLocalId(p.index as u32), p.name.to_string())
        });
        let mut out = String::new();
        rsjs_ir::emit_client_expr_js_with_context(&mut out, &expr, &ctx)
            .expect("client helper BodyNode subset must be supported by Client IR v3 JS emitter");
        out
    }

    fn write_server_eval_tokens(&self, args: &Ident) -> TokenStream {
        match &self.kind {
            BodyNodeKind::Literal(value) => value.to_server_value_tokens(),
            BodyNodeKind::Param { index, name } => {
                let index_lit = syn::LitInt::new(&index.to_string(), Span::call_site());
                let name_lit = syn::LitStr::new(name, Span::call_site());
                quote! {
                    ::rsjs::__runtime::client_arg_value(#args, #index_lit, #name_lit)
                }
            }
            BodyNodeKind::Local { id, .. } => {
                let local = helper_local_rust_ident(*id);
                quote! { #local.clone() }
            }
            BodyNodeKind::StructInit { fields, .. } => {
                let fields = fields.iter().map(|(name, value)| {
                    let name = syn::LitStr::new(name, Span::call_site());
                    let value = value.write_server_eval_tokens(args);
                    quote! { (#name, #value) }
                });
                quote! {
                    ::rsjs::__runtime::client_object(::std::vec![ #( #fields ),* ])
                }
            }
            BodyNodeKind::Field { base, name } => {
                let base = base.write_server_eval_tokens(args);
                let name = syn::LitStr::new(name, Span::call_site());
                quote! {
                    ::rsjs::__runtime::client_field(#base, #name)
                }
            }
            BodyNodeKind::Binary { op, lhs, rhs } => {
                let op_lit = syn::LitStr::new(client_binary_op_json(op), Span::call_site());
                let lhs = lhs.write_server_eval_tokens(args);
                let rhs = rhs.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_binary_op(#op_lit, #lhs, #rhs)
                }
            }
            BodyNodeKind::Unary { op, expr } => {
                let op_lit = syn::LitStr::new(client_unary_op_json(op), Span::call_site());
                let expr = expr.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_unary_op(#op_lit, #expr)
                }
            }
            BodyNodeKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = condition.write_server_eval_tokens(args);
                let then_expr = then_expr.write_server_eval_tokens(args);
                let else_expr = else_expr.write_server_eval_tokens(args);
                quote! {
                    if ::rsjs::__runtime::client_bool(#condition) {
                        #then_expr
                    } else {
                        #else_expr
                    }
                }
            }
            BodyNodeKind::IsNonEmpty(target) => {
                let target = target.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_is_non_empty(#target)
                }
            }
            BodyNodeKind::Len(target) => {
                let target = target.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_len(#target)
                }
            }
            BodyNodeKind::StringToLowercase(target) => {
                let target = target.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_string_to_lowercase(#target)
                }
            }
            BodyNodeKind::StringContains { haystack, needle } => {
                let haystack = haystack.write_server_eval_tokens(args);
                let needle = needle.write_server_eval_tokens(args);
                quote! {
                    ::rsjs::__runtime::client_string_contains(#haystack, #needle)
                }
            }
            BodyNodeKind::StringTransform { op, target } => {
                let target = target.write_server_eval_tokens(args);
                let op = string_transform_op_str(*op);
                quote! {
                    ::rsjs::__runtime::client_string_transform(#op, #target)
                }
            }
            BodyNodeKind::StringPredicate {
                op,
                haystack,
                needle,
            } => {
                let haystack = haystack.write_server_eval_tokens(args);
                let needle = needle.write_server_eval_tokens(args);
                let op = string_predicate_op_str(*op);
                quote! {
                    ::rsjs::__runtime::client_string_predicate(#op, #haystack, #needle)
                }
            }
            BodyNodeKind::ClientCall {
                callee,
                args: call_args,
            } => {
                // Use the exact parsed source path for the server-side call as
                // well as for the relocation record. Reconstructing this path
                // from descriptor text would make the server and browser
                // identities disagree under absolute paths or macro hygiene.
                let callee_path = &callee.path;
                let arg_tokens: Vec<TokenStream> = call_args
                    .iter()
                    .map(|call_arg| {
                        let value = call_arg.write_server_eval_tokens(args);
                        quote! {
                            ::rsjs::__runtime::ClientReturnValue::from_client_value(#value)
                        }
                    })
                    .collect();
                quote! {
                    ::rsjs::__runtime::client_value_from_return(#callee_path( #( #arg_tokens ),* ))
                }
            }
        }
    }

    fn write_json(&self, out: &mut String) {
        use std::fmt::Write as _;

        out.push_str(r#"{"span":"#);
        push_span_json(out, self.span);
        out.push_str(r#","kind":""#);
        match &self.kind {
            BodyNodeKind::Literal(v) => {
                out.push_str(r#"Lit","value":"#);
                v.write_json(out);
                out.push('}');
            }
            BodyNodeKind::Param { index, name } => {
                let _ = write!(out, r#"Param","index":{index},"name":""#);
                push_string_escaped(out, name);
                out.push_str(r#""}"#);
            }
            BodyNodeKind::Local { id, name } => {
                let _ = write!(out, r#"Local","local":{id},"name":""#);
                push_string_escaped(out, name);
                out.push_str(r#""}"#);
            }
            BodyNodeKind::StructInit { type_name, fields } => {
                out.push_str(r#"StructInit","type_name":""#);
                push_string_escaped(out, type_name);
                out.push_str(r#"","fields":["#);
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(r#"{"name":""#);
                    push_string_escaped(out, name);
                    out.push_str(r#"","value":"#);
                    value.write_json(out);
                    out.push('}');
                }
                out.push_str("]}");
            }
            BodyNodeKind::Field { base, name } => {
                out.push_str(r#"Field","base":"#);
                base.write_json(out);
                out.push_str(r#","name":""#);
                push_string_escaped(out, name);
                out.push_str(r#""}"#);
            }
            BodyNodeKind::Binary { op, lhs, rhs } => {
                let _ = write!(out, r#"BinaryOp","op":"{op}","lhs":"#);
                lhs.write_json(out);
                out.push_str(r#","rhs":"#);
                rhs.write_json(out);
                out.push('}');
            }
            BodyNodeKind::Unary { op, expr } => {
                let _ = write!(out, r#"UnaryOp","op":"{op}","expr":"#);
                expr.write_json(out);
                out.push('}');
            }
            BodyNodeKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                out.push_str(r#"Conditional","condition":"#);
                condition.write_json(out);
                out.push_str(r#","then":"#);
                then_expr.write_json(out);
                out.push_str(r#","else":"#);
                else_expr.write_json(out);
                out.push('}');
            }
            BodyNodeKind::IsNonEmpty(target) => {
                out.push_str(r#"IsNonEmpty","target":"#);
                target.write_json(out);
                out.push('}');
            }
            BodyNodeKind::Len(target) => {
                out.push_str(r#"Len","target":"#);
                target.write_json(out);
                out.push('}');
            }
            BodyNodeKind::StringToLowercase(target) => {
                out.push_str(r#"StringToLowercase","target":"#);
                target.write_json(out);
                out.push('}');
            }
            BodyNodeKind::StringContains { haystack, needle } => {
                out.push_str(r#"StringContains","haystack":"#);
                haystack.write_json(out);
                out.push_str(r#","needle":"#);
                needle.write_json(out);
                out.push('}');
            }
            BodyNodeKind::StringTransform { op, target } => {
                let _ = write!(
                    out,
                    r#"StringTransform","op":"{op}","target":"#,
                    op = string_transform_op_str(*op)
                );
                target.write_json(out);
                out.push('}');
            }
            BodyNodeKind::StringPredicate {
                op,
                haystack,
                needle,
            } => {
                let _ = write!(
                    out,
                    r#"StringPredicate","op":"{op}","haystack":"#,
                    op = string_predicate_op_str(*op)
                );
                haystack.write_json(out);
                out.push_str(r#","needle":"#);
                needle.write_json(out);
                out.push('}');
            }
            BodyNodeKind::ClientCall { callee, args } => {
                out.push_str(r#"ClientCall","callee":""#);
                push_string_escaped(out, &callee.authored_path);
                out.push_str(r#"","args":["#);
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    a.write_json(out);
                }
                out.push_str("]}");
            }
        }
    }
}

impl LiteralValue {
    fn to_value(&self) -> Value {
        match self {
            LiteralValue::Null => Value::Null,
            LiteralValue::Bool(b) => Value::Bool(*b),
            LiteralValue::Integer { value, domain }
                if domain.is_some_and(IntegerDomain::is_wide) || i64::try_from(*value).is_err() =>
            {
                Value::BigInt(*value)
            }
            LiteralValue::Integer { value, .. } => {
                Value::Int(i64::try_from(*value).expect("integer representation checked above"))
            }
            LiteralValue::Float(f) => Value::Float(*f),
            LiteralValue::String(s) => Value::String(s.clone()),
        }
    }

    fn to_server_value_tokens(&self) -> TokenStream {
        match self {
            LiteralValue::Null => quote! {
                ::rsjs::__runtime::ClientValue::Null
            },
            LiteralValue::Bool(value) => quote! {
                ::rsjs::__runtime::ClientValue::Bool(#value)
            },
            LiteralValue::Integer {
                value,
                domain: Some(IntegerDomain::U64),
            } => {
                let value = u64::try_from(*value).expect("u64 helper literals are range validated");
                let lit = syn::LitInt::new(&format!("{value}u64"), Span::call_site());
                quote! { ::rsjs::__runtime::ClientValue::U64(#lit) }
            }
            LiteralValue::Integer { value, .. } if i64::try_from(*value).is_ok() => {
                let value = i64::try_from(*value).expect("guarded above");
                let lit = syn::LitInt::new(&format!("{value}i64"), Span::call_site());
                quote! { ::rsjs::__runtime::ClientValue::I64(#lit) }
            }
            LiteralValue::Integer { value, .. } => {
                let value = u64::try_from(*value)
                    .expect("positive helper integer literals are bounded by u64");
                let lit = syn::LitInt::new(&format!("{value}u64"), Span::call_site());
                quote! { ::rsjs::__runtime::ClientValue::U64(#lit) }
            }
            LiteralValue::Float(value) => {
                let lit = syn::LitFloat::new(&format!("{value}f64"), Span::call_site());
                quote! { ::rsjs::__runtime::ClientValue::F64(#lit) }
            }
            LiteralValue::String(value) => {
                let lit = syn::LitStr::new(value, Span::call_site());
                quote! {
                    ::rsjs::__runtime::ClientValue::String(::std::string::String::from(#lit))
                }
            }
        }
    }

    fn write_json(&self, out: &mut String) {
        use std::fmt::Write as _;

        match self {
            LiteralValue::Null => {
                out.push_str(r#"{"v":"Null"}"#);
            }
            LiteralValue::Bool(b) => {
                let _ = write!(
                    out,
                    r#"{{"v":"Bool","data":{data}"#,
                    data = if *b { "true" } else { "false" }
                );
                out.push('}');
            }
            LiteralValue::Integer { .. } => match self.to_value() {
                Value::Int(i) => {
                    let _ = write!(out, r#"{{"v":"Int","data":{i}"#);
                    out.push('}');
                }
                Value::BigInt(i) => {
                    let _ = write!(out, r#"{{"v":"BigInt","data":"{i}"}}"#);
                }
                _ => unreachable!("integer literal converts to an integer IR value"),
            },
            LiteralValue::Float(f) => {
                let _ = write!(out, r#"{{"v":"Float","data":{f}"#);
                out.push('}');
            }
            LiteralValue::String(s) => {
                out.push_str(r#"{"v":"String","data":""#);
                push_string_escaped(out, s);
                out.push_str(r#""}"#);
            }
        }
    }
}

fn helper_integer_literal_value(
    value: i128,
    domain: Option<IntegerDomain>,
    span: Span,
) -> Result<LiteralValue, Diagnostic> {
    if let Some(domain) = domain {
        let (min, max) = helper_integer_bounds(domain);
        if !(min..=max).contains(&value) {
            return Err(Diagnostic::new(
                span,
                &format!(
                    "rsjs/integer-literal-out-of-range: `{value}` is outside the `{}` domain",
                    domain.as_str()
                ),
                "use a literal inside the declared fixed-width range",
                "checked helper arithmetic cannot begin with an out-of-domain operand",
            ));
        }
        return Ok(LiteralValue::Integer {
            value,
            domain: Some(domain),
        });
    }
    Ok(LiteralValue::Integer {
        value,
        domain: None,
    })
}

fn helper_integer_bounds(domain: IntegerDomain) -> (i128, i128) {
    match domain {
        IntegerDomain::I32 => (i32::MIN as i128, i32::MAX as i128),
        IntegerDomain::U32 => (0, u32::MAX as i128),
        IntegerDomain::I64 => (i64::MIN as i128, i64::MAX as i128),
        IntegerDomain::U64 => (0, u64::MAX as i128),
    }
}

fn helper_return_integer_domain(name: &str) -> Option<(IntegerDomain, bool)> {
    if let Some(domain) = IntegerDomain::from_name(name) {
        return Some((domain, false));
    }
    let inner = name
        .strip_prefix("Option<")
        .and_then(|name| name.strip_suffix('>'))?;
    IntegerDomain::from_name(inner).map(|domain| (domain, true))
}

fn coerce_helper_integer_literals(
    node: &mut BodyNode,
    domain: IntegerDomain,
) -> Result<(), Diagnostic> {
    match &mut node.kind {
        BodyNodeKind::Literal(LiteralValue::Integer {
            value,
            domain: literal_domain,
        }) => {
            if let Some(literal_domain) = *literal_domain {
                if literal_domain != domain {
                    return Err(helper_integer_domain_mismatch(
                        node.span,
                        literal_domain,
                        domain,
                    ));
                }
            }
            let (min, max) = helper_integer_bounds(domain);
            if !(min..=max).contains(value) {
                return Err(Diagnostic::new(
                    node.span,
                    &format!(
                        "rsjs/integer-literal-out-of-range: `{value}` is outside the `{}` domain",
                        domain.as_str()
                    ),
                    "use a literal inside the declared fixed-width range",
                    "checked helper arithmetic cannot begin with an out-of-domain operand",
                ));
            }
            *literal_domain = Some(domain);
        }
        BodyNodeKind::Binary { lhs, rhs, .. } => {
            coerce_helper_integer_literals(lhs, domain)?;
            coerce_helper_integer_literals(rhs, domain)?;
        }
        BodyNodeKind::Unary { expr, .. } => coerce_helper_integer_literals(expr, domain)?,
        BodyNodeKind::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            coerce_helper_integer_literals(then_expr, domain)?;
            coerce_helper_integer_literals(else_expr, domain)?;
        }
        _ => {}
    }
    Ok(())
}

fn rewrite_literal_only_checked_domain(node: &mut BodyNode, domain: IntegerDomain) {
    match &mut node.kind {
        BodyNodeKind::Binary { op, lhs, rhs } => {
            if let Some((operation, _)) = parse_checked_helper_binary_op(op) {
                let source = match operation {
                    CheckedIntegerOp::Add => "Add",
                    CheckedIntegerOp::Sub => "Sub",
                    CheckedIntegerOp::Mul => "Mul",
                    CheckedIntegerOp::Div => "Div",
                    CheckedIntegerOp::Rem => "Rem",
                };
                *op = checked_helper_binary_op(source, domain);
            }
            rewrite_literal_only_checked_domain(lhs, domain);
            rewrite_literal_only_checked_domain(rhs, domain);
        }
        BodyNodeKind::Unary { op, expr } => {
            if parse_checked_helper_unary_op(op).is_some() && domain.is_signed() {
                *op = checked_helper_unary_op(domain);
            }
            rewrite_literal_only_checked_domain(expr, domain);
        }
        BodyNodeKind::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            rewrite_literal_only_checked_domain(then_expr, domain);
            rewrite_literal_only_checked_domain(else_expr, domain);
        }
        _ => {}
    }
}

fn push_string_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

fn push_span_json(out: &mut String, span: Span) {
    use std::fmt::Write as _;

    let start = span.start();
    let end = span.end();
    let _ = write!(
        out,
        r#"{{"start":{{"line":{start_line},"column":{start_column}}},"end":{{"line":{end_line},"column":{end_column}}}}}"#,
        start_line = start.line,
        start_column = start.column,
        end_line = end.line,
        end_column = end.column
    );
}

// ---------------------------------------------------------------------------
// Diagnostic helper
// ---------------------------------------------------------------------------

/// A span-pointed compile error in the rustc shape: `what`, `why`,
/// `how to fix`. Renders as a single `compile_error!(...)` invocation
/// with the message formatted as multi-line so each section reads
/// distinctly in the user's terminal.
struct Diagnostic {
    span: Span,
    what: String,
    fix: String,
    why: String,
}

impl Diagnostic {
    fn new(span: Span, what: &str, fix: &str, why: &str) -> Self {
        Self {
            span,
            what: what.to_string(),
            fix: fix.to_string(),
            why: why.to_string(),
        }
    }

    fn to_tokens(&self) -> TokenStream {
        let msg = format!(
            "rsjs: {}\n  help: {}\n  note: {}",
            self.what, self.fix, self.why
        );
        let lit = syn::LitStr::new(&msg, self.span);
        quote_spanned!(self.span => ::core::compile_error!(#lit);)
    }
}

fn error_at(span: Span, what: &str, fix: &str, why: &str) -> TokenStream {
    Diagnostic::new(span, what, fix, why).to_tokens()
}

/// Client IR v3 operator for a `BodyNode::Binary` operator string.
fn checked_helper_binary_op(op: &str, domain: IntegerDomain) -> &'static str {
    match (domain, op) {
        (IntegerDomain::I32, "Add") => "CheckedI32Add",
        (IntegerDomain::I32, "Sub") => "CheckedI32Sub",
        (IntegerDomain::I32, "Mul") => "CheckedI32Mul",
        (IntegerDomain::I32, "Div") => "CheckedI32Div",
        (IntegerDomain::I32, "Rem") => "CheckedI32Rem",
        (IntegerDomain::U32, "Add") => "CheckedU32Add",
        (IntegerDomain::U32, "Sub") => "CheckedU32Sub",
        (IntegerDomain::U32, "Mul") => "CheckedU32Mul",
        (IntegerDomain::U32, "Div") => "CheckedU32Div",
        (IntegerDomain::U32, "Rem") => "CheckedU32Rem",
        (IntegerDomain::I64, "Add") => "CheckedI64Add",
        (IntegerDomain::I64, "Sub") => "CheckedI64Sub",
        (IntegerDomain::I64, "Mul") => "CheckedI64Mul",
        (IntegerDomain::I64, "Div") => "CheckedI64Div",
        (IntegerDomain::I64, "Rem") => "CheckedI64Rem",
        (IntegerDomain::U64, "Add") => "CheckedU64Add",
        (IntegerDomain::U64, "Sub") => "CheckedU64Sub",
        (IntegerDomain::U64, "Mul") => "CheckedU64Mul",
        (IntegerDomain::U64, "Div") => "CheckedU64Div",
        (IntegerDomain::U64, "Rem") => "CheckedU64Rem",
        _ => unreachable!("checked helper operator is validated before lowering"),
    }
}

fn parse_checked_helper_binary_op(op: &str) -> Option<(CheckedIntegerOp, IntegerDomain)> {
    let (domain, suffix) = if let Some(suffix) = op.strip_prefix("CheckedI32") {
        (IntegerDomain::I32, suffix)
    } else if let Some(suffix) = op.strip_prefix("CheckedU32") {
        (IntegerDomain::U32, suffix)
    } else if let Some(suffix) = op.strip_prefix("CheckedI64") {
        (IntegerDomain::I64, suffix)
    } else if let Some(suffix) = op.strip_prefix("CheckedU64") {
        (IntegerDomain::U64, suffix)
    } else {
        return None;
    };
    let operation = match suffix {
        "Add" => CheckedIntegerOp::Add,
        "Sub" => CheckedIntegerOp::Sub,
        "Mul" => CheckedIntegerOp::Mul,
        "Div" => CheckedIntegerOp::Div,
        "Rem" => CheckedIntegerOp::Rem,
        _ => return None,
    };
    Some((operation, domain))
}

fn checked_helper_unary_op(domain: IntegerDomain) -> &'static str {
    match domain {
        IntegerDomain::I32 => "CheckedI32Neg",
        IntegerDomain::I64 => "CheckedI64Neg",
        IntegerDomain::U32 | IntegerDomain::U64 => {
            unreachable!("unsigned helper negation is rejected before lowering")
        }
    }
}

fn parse_checked_helper_unary_op(op: &str) -> Option<IntegerDomain> {
    match op {
        "CheckedI32Neg" => Some(IntegerDomain::I32),
        "CheckedI64Neg" => Some(IntegerDomain::I64),
        _ => None,
    }
}

fn client_binary_op(op: &str) -> ClientBinaryOp {
    if let Some((op, domain)) = parse_checked_helper_binary_op(op) {
        return ClientBinaryOp::Checked { op, domain };
    }
    match op {
        "Add" => ClientBinaryOp::Add,
        "Sub" => ClientBinaryOp::Sub,
        "Mul" => ClientBinaryOp::Mul,
        "Div" => ClientBinaryOp::Div,
        "Rem" => ClientBinaryOp::Rem,
        "Eq" => ClientBinaryOp::Eq,
        "Neq" => ClientBinaryOp::Ne,
        "Lt" => ClientBinaryOp::Lt,
        "Gt" => ClientBinaryOp::Gt,
        "Le" => ClientBinaryOp::Le,
        "Ge" => ClientBinaryOp::Ge,
        "And" => ClientBinaryOp::And,
        "Or" => ClientBinaryOp::Or,
        _ => unreachable!("BodyLowerer validates binary operators before JS emission"),
    }
}

fn client_unary_op(op: &str) -> ClientUnaryOp {
    if let Some(domain) = parse_checked_helper_unary_op(op) {
        return ClientUnaryOp::CheckedNeg(domain);
    }
    match op {
        "Not" => ClientUnaryOp::Not,
        "Neg" => ClientUnaryOp::Neg,
        _ => unreachable!("BodyLowerer validates unary operators before JS emission"),
    }
}

fn client_unary_op_json(op: &str) -> &'static str {
    if let Some(domain) = parse_checked_helper_unary_op(op) {
        return match domain {
            IntegerDomain::I32 => "checked_i32_neg",
            IntegerDomain::I64 => "checked_i64_neg",
            IntegerDomain::U32 | IntegerDomain::U64 => unreachable!(),
        };
    }
    match op {
        "Not" => "not",
        "Neg" => "neg",
        _ => unreachable!("BodyLowerer validates unary operators before IR serialization"),
    }
}

fn client_binary_op_json(op: &str) -> &'static str {
    if let Some((operation, domain)) = parse_checked_helper_binary_op(op) {
        return match (domain, operation) {
            (IntegerDomain::I32, CheckedIntegerOp::Add) => "checked_i32_add",
            (IntegerDomain::I32, CheckedIntegerOp::Sub) => "checked_i32_sub",
            (IntegerDomain::I32, CheckedIntegerOp::Mul) => "checked_i32_mul",
            (IntegerDomain::I32, CheckedIntegerOp::Div) => "checked_i32_div",
            (IntegerDomain::I32, CheckedIntegerOp::Rem) => "checked_i32_rem",
            (IntegerDomain::U32, CheckedIntegerOp::Add) => "checked_u32_add",
            (IntegerDomain::U32, CheckedIntegerOp::Sub) => "checked_u32_sub",
            (IntegerDomain::U32, CheckedIntegerOp::Mul) => "checked_u32_mul",
            (IntegerDomain::U32, CheckedIntegerOp::Div) => "checked_u32_div",
            (IntegerDomain::U32, CheckedIntegerOp::Rem) => "checked_u32_rem",
            (IntegerDomain::I64, CheckedIntegerOp::Add) => "checked_i64_add",
            (IntegerDomain::I64, CheckedIntegerOp::Sub) => "checked_i64_sub",
            (IntegerDomain::I64, CheckedIntegerOp::Mul) => "checked_i64_mul",
            (IntegerDomain::I64, CheckedIntegerOp::Div) => "checked_i64_div",
            (IntegerDomain::I64, CheckedIntegerOp::Rem) => "checked_i64_rem",
            (IntegerDomain::U64, CheckedIntegerOp::Add) => "checked_u64_add",
            (IntegerDomain::U64, CheckedIntegerOp::Sub) => "checked_u64_sub",
            (IntegerDomain::U64, CheckedIntegerOp::Mul) => "checked_u64_mul",
            (IntegerDomain::U64, CheckedIntegerOp::Div) => "checked_u64_div",
            (IntegerDomain::U64, CheckedIntegerOp::Rem) => "checked_u64_rem",
        };
    }
    match op {
        "Add" => "add",
        "Sub" => "sub",
        "Mul" => "mul",
        "Div" => "div",
        "Rem" => "rem",
        "Eq" => "eq",
        "Neq" => "neq",
        "Lt" => "lt",
        "Gt" => "gt",
        "Le" => "le",
        "Ge" => "ge",
        "And" => "and",
        "Or" => "or",
        _ => unreachable!("BodyLowerer validates binary operators before IR serialization"),
    }
}

fn string_transform_op_str(op: StringTransformOp) -> &'static str {
    match op {
        StringTransformOp::ToUppercase => "to_uppercase",
        StringTransformOp::Trim => "trim",
        _ => unreachable!("BodyLowerer only constructs known string transform ops"),
    }
}

fn string_predicate_op_str(op: StringPredicateOp) -> &'static str {
    match op {
        StringPredicateOp::StartsWith => "starts_with",
        StringPredicateOp::EndsWith => "ends_with",
        _ => unreachable!("BodyLowerer only constructs known string predicate ops"),
    }
}

#[cfg(test)]
fn helper_call_export_name(callee: &str, bundle_safe_name: Option<&str>) -> String {
    let mut segments = callee.rsplit("::");
    let helper = segments.next().unwrap_or(callee);
    match segments.next() {
        Some(module) => format!("{module}__{helper}"),
        None => bundle_safe_name
            .map(|module| format!("{module}__{helper}"))
            .unwrap_or_else(|| helper.to_string()),
    }
}

/// Lower a `pub fn` (with `#[rsjs::client]`-shape constraints) to a
/// JavaScript function declaration: `export function <export_name>(<p>...) {
/// return <expr>; }`.
///
/// `bundle_export_name` is `<safe_module>__<helper>` (matching emit_js's
/// `bundle_export_name` convention); the function declaration uses that
/// name as its ident so consuming island JS can
/// `import { <safe_module>__<helper> } from "/_rsjs/client_modules/<safe_module>.js"`.
///
/// Returns `None` if the function does not pass the v0.1
/// `#[rsjs::client]` shape constraints (e.g. async, type/const
/// generic, multi-statement body). The caller (`client_module_attr`)
/// treats `None` as "skip silently"; the per-fn `#[rsjs::client]`
/// macro will surface the real diagnostic at the helper's site.
#[cfg(test)]
pub(crate) fn lower_helper_fn_to_js(
    func: &syn::ItemFn,
    bundle_export_name: &str,
    bundle_safe_name: Option<&str>,
) -> Option<String> {
    use std::fmt::Write as _;

    let lowered = lower_helper(func).ok()?;
    let body_js = lowered.body.to_js_expr(&lowered.params, bundle_safe_name);

    let mut out = String::new();
    let _ = write!(out, "export function {bundle_export_name}");
    out.push('(');
    for (i, p) in lowered.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name.to_string());
    }
    let _ = writeln!(out, ") {{ return {body_js}; }}");
    Some(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn expand_ok(input: TokenStream) -> String {
        let func: syn::ItemFn = syn::parse2(input).expect("input parses as fn");
        let ts = try_expand(&func).unwrap_or_else(|d| panic!("expected ok: {}", d.what));
        ts.to_string()
    }

    fn expand_err(input: TokenStream) -> String {
        let func: syn::ItemFn = syn::parse2(input).expect("input parses as fn");
        match try_expand(&func) {
            Ok(_) => panic!("expected an error, got success"),
            Err(d) => d.what.clone(),
        }
    }

    fn assert_json_snapshot(actual: &str, relative_path: &str) {
        let actual: serde_json::Value =
            serde_json::from_str(actual).expect("actual snapshot JSON should parse");
        let expected: serde_json::Value = serde_json::from_str(&read_snapshot(relative_path))
            .expect("expected snapshot JSON should parse");
        assert_eq!(actual, expected);
    }

    fn assert_json_shape_snapshot(actual: &str, relative_path: &str) {
        let mut actual: serde_json::Value =
            serde_json::from_str(actual).expect("actual snapshot JSON should parse");
        let mut expected: serde_json::Value = serde_json::from_str(&read_snapshot(relative_path))
            .expect("expected snapshot JSON should parse");
        remove_span_fields(&mut actual);
        remove_span_fields(&mut expected);
        assert_eq!(actual, expected);
    }

    fn remove_span_fields(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    remove_span_fields(item);
                }
            }
            serde_json::Value::Object(fields) => {
                fields.remove("span");
                for field in fields.values_mut() {
                    remove_span_fields(field);
                }
            }
            _ => {}
        }
    }

    fn assert_text_snapshot(actual: &str, relative_path: &str) {
        assert_eq!(actual, read_snapshot(relative_path));
    }

    fn read_snapshot(relative_path: &str) -> String {
        let path = workspace_root().join(relative_path);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read snapshot {}: {err}", path.display()))
    }

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("rsjs_macros lives directly under the workspace root")
            .to_path_buf()
    }

    #[test]
    fn client_helper_emits_one_direct_function_relocation_definition() {
        let out = expand_ok(quote! {
            pub fn subtotal(price: i64, quantity: i64) -> i64 {
                price * quantity
            }
        });
        let compact = out.split_whitespace().collect::<String>();

        assert_eq!(
            compact
                .matches("static__RSJS_CLIENT_HELPER_DEFINITION_subtotal")
                .count(),
            1,
            "one helper definition record must be emitted:\n{out}"
        );
        assert!(
            compact.contains("HelperDefinitionRecord")
                && compact.contains("subtotalas*const()")
                && compact.contains("__DATA,__rsjs_help")
                && compact.contains(".rsjs.helper")
                && compact.contains(".rsjs$H"),
            "the definition must relocate directly to the rewritten Rust helper:\n{out}"
        );
        assert_eq!(
            compact
                .matches("static__RSJS_CLIENT_HELPER_IDENTITY_subtotal")
                .count(),
            1,
            "one normalized helper identity must accompany the exact definition:\n{out}"
        );
        assert!(
            compact.contains("SemanticIdentityRecord")
                && compact.contains("env!(\"CARGO_PKG_NAME\")")
                && compact.contains("env!(\"CARGO_CRATE_NAME\")")
                && compact.contains("module_path!()")
                && compact.contains("stringify!(subtotal)")
                && compact.contains("__DATA,__rsjs_ident")
                && compact.contains(".rsjs.identity")
                && compact.contains(".rsjs$S"),
            "helper identity must come from its defining expansion:\n{out}"
        );
        assert!(
            compact.contains("static__RSJS_CLIENT_HELPER_PAYLOAD_subtotal")
                && compact.contains("PayloadRecord")
                && compact.contains("__DATA,__rsjs_hpay")
                && compact.contains(".rsjs.helper_payload")
                && compact.contains(".rsjs$Q"),
            "the complete helper descriptor must be embedded inline:\n{out}"
        );
    }

    #[test]
    fn standalone_helper_payload_carries_signature_ir_identity_and_direct_edges() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn label(count: u32) -> String {
                pluralize(count)
            }
        })
        .unwrap();
        let lowered = lower_helper(&func)
            .unwrap_or_else(|diagnostic| panic!("helper lowering failed: {}", diagnostic.what));
        let helper_record_id = helper_record_id(&func);
        let calls = lowered.body.direct_client_calls();
        let json = standalone_helper_payload_json(
            &func,
            &lowered,
            &lowered.body.to_json(),
            &lowered.body.to_ir_json(),
            helper_record_id,
            &calls,
        )
        .unwrap_or_else(|diagnostic| {
            panic!(
                "standalone helper payload construction failed: {}",
                diagnostic.what
            )
        });
        let payload: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(payload["format"], "rsjs.standalone_helper_descriptor");
        assert_eq!(payload["schema_version"], 1);
        assert_eq!(payload["effect_class"], "origin_free");
        assert_eq!(payload["signature"]["params"][0]["name"], "count");
        assert_eq!(payload["signature"]["params"][0]["rust_type"], "u32");
        assert_eq!(payload["signature"]["return_type"], "String");
        assert_eq!(payload["body_hir_version"], 1);
        assert_eq!(payload["body_ir_version"], CLIENT_IR_SCHEMA_VERSION);
        assert!(payload["body_hir"].is_object());
        assert!(payload["body_ir"].is_object());
        assert_eq!(payload["direct_calls"][0]["ordinal"], 0);
        assert_eq!(payload["direct_calls"][0]["authored_path"], "pluralize");
        assert_eq!(
            payload["direct_calls"][0]["source_edge_id"],
            hex_id(&helper_direct_call_id(helper_record_id, 0))
        );
        assert_eq!(
            payload["provisional_identity"]["module_identity"],
            "rustc_defining_relocation"
        );
        assert!(payload["semantic_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:") && hash.len() == 71));

        let out = expand_ok(quote! {
            pub fn label(count: u32) -> String {
                pluralize(count)
            }
        });
        let compact = out.split_whitespace().collect::<String>();
        assert_eq!(
            compact.matches("__rsjs_emit_client_helper_edge!").count(),
            1,
            "one direct helper call must emit one Rust-resolved edge:\n{out}"
        );
    }

    fn client_module_snapshot_js(
        module_path: &str,
        helper_name: &str,
        lowered: &LoweredHelper,
    ) -> String {
        let param_pairs = lowered
            .params
            .iter()
            .map(|param| {
                (
                    leak_str(param.name.to_string()),
                    leak_str(param.ty_name.clone()),
                )
            })
            .collect::<Vec<_>>();
        let params: &'static [(&'static str, &'static str)] =
            Box::leak(param_pairs.into_boxed_slice());
        let helper: &'static rsjs::ClientHelperMeta = Box::leak(Box::new(rsjs::ClientHelperMeta {
            name: leak_str(helper_name.to_string()),
            params,
            return_ty: leak_str(lowered.ret_ty_name.clone()),
            body_ir_version: CLIENT_IR_SCHEMA_VERSION,
            body_ir_json: leak_str(lowered.body.to_ir_json()),
            server_eval: noop_server_eval,
        }));
        let helpers: &'static [&'static rsjs::ClientHelperMeta] =
            Box::leak(vec![helper].into_boxed_slice());
        let bundle = rsjs::ClientModuleManifestEntry {
            module_path: leak_str(module_path.to_string()),
            helpers,
        };
        rsjs::client_module_js_source(&bundle)
    }

    fn leak_str(value: String) -> &'static str {
        Box::leak(value.into_boxed_str())
    }

    fn noop_server_eval(
        _args: &[(&'static str, rsjs::__runtime::ClientArg)],
    ) -> rsjs::__runtime::ClientValue {
        rsjs::__runtime::ClientValue::Unit
    }

    #[test]
    fn lowers_simple_arithmetic() {
        let out = expand_ok(quote! {
            pub fn subtotal(price: i64, qty: i64) -> i64 {
                price * qty
            }
        });
        // The body JSON should contain a checked i64 multiplication and
        // two Param children indexed 0 and 1.
        assert!(
            out.contains("\\\"kind\\\":\\\"BinaryOp\\\""),
            "missing BinaryOp: {out}"
        );
        assert!(
            out.contains("\\\"op\\\":\\\"CheckedI64Mul\\\""),
            "missing checked i64 Mul op: {out}"
        );
        assert!(out.contains("\\\"name\\\":\\\"price\\\""));
        assert!(out.contains("\\\"name\\\":\\\"qty\\\""));
        assert!(out.contains("__rsjs_client_subtotal_meta"));
    }

    #[test]
    fn rejects_mixed_explicit_integer_domains_in_arithmetic() {
        let msg = expand_err(quote! {
            pub fn mixed(value: i32) -> i32 {
                value + 1_u32
            }
        });
        assert!(msg.contains("integer-domain-mismatch"), "got: {msg}");
        assert!(msg.contains("i32") && msg.contains("u32"), "got: {msg}");
    }

    #[test]
    fn rejects_typed_local_integer_domain_mismatch() {
        let msg = expand_err(quote! {
            pub fn mixed(value: i32) -> i32 {
                let widened: u32 = value;
                widened
            }
        });
        assert!(msg.contains("integer-domain-mismatch"), "got: {msg}");
    }

    #[test]
    fn rejects_return_integer_domain_mismatch() {
        let msg = expand_err(quote! {
            pub fn mixed(value: i32) -> u32 {
                value
            }
        });
        assert!(msg.contains("integer-domain-mismatch"), "got: {msg}");
    }

    #[test]
    fn infers_direct_peer_helper_call_from_declared_wide_return() {
        let out = expand_ok(quote! {
            pub fn outer(value: i64) -> i64 {
                inner(value) + 1
            }
        });
        assert!(
            out.contains("\\\"kind\\\":\\\"ClientCall\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"op\\\":\\\"CheckedI64Add\\\""),
            "out: {out}"
        );
    }

    #[test]
    fn lowers_is_non_empty() {
        let out = expand_ok(quote! {
            pub fn is_filled(s: String) -> bool {
                s.is_non_empty()
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"IsNonEmpty\\\""));
    }

    #[test]
    fn lowers_is_empty_as_not_is_non_empty() {
        let out = expand_ok(quote! {
            pub fn is_blank(s: String) -> bool {
                s.is_empty()
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"UnaryOp\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"Not\\\""), "out: {out}");
        assert!(
            out.contains("\\\"kind\\\":\\\"IsNonEmpty\\\""),
            "out: {out}"
        );
        assert!(out.contains("\\\"kind\\\":\\\"unary\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"not\\\""), "out: {out}");
        assert!(
            out.contains("\\\"kind\\\":\\\"is_non_empty\\\""),
            "out: {out}"
        );
    }

    #[test]
    fn lowers_len_as_runtime_call() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn has_label(s: String) -> bool {
                s.len() > 0
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "labels__has_label", Some("labels")).unwrap();
        assert!(
            js.contains("new TextEncoder().encode(__v).length"),
            "js: {js}"
        );

        let out = expand_ok(quote! {
            pub fn has_label(s: String) -> bool {
                s.len() > 0
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"Len\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"call\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"runtime\\\""), "out: {out}");
        assert!(out.contains("client_len"), "out: {out}");
    }

    #[test]
    fn rejects_provably_non_bool_helper_conditions() {
        let err = expand_err(quote! {
            pub fn numeric_truthiness(count: i64) -> i64 {
                if count { 1 } else { 0 }
            }
        });
        assert!(err.contains("conditions must have Rust `bool` semantics"));

        let err = expand_err(quote! {
            pub fn local_truthiness(count: i64) -> i64 {
                let next = count + 1;
                if next {
                    return 1;
                }
                0
            }
        });
        assert!(err.contains("conditions must have Rust `bool` semantics"));

        let err = expand_err(quote! {
            pub fn logical_truthiness(count: i64) -> i64 {
                if count && true { 1 } else { 0 }
            }
        });
        assert!(err.contains("conditions must have Rust `bool` semantics"));

        let err = expand_err(quote! {
            pub fn unary_truthiness(count: i64) -> i64 {
                if !count { 1 } else { 0 }
            }
        });
        assert!(err.contains("conditions must have Rust `bool` semantics"));

        let err = expand_err(quote! {
            pub fn guarded_truthiness(value: i64, count: i64) -> i64 {
                match value {
                    1 if count => 1,
                    _ => 0,
                }
            }
        });
        assert!(err.contains("conditions must have Rust `bool` semantics"));
    }

    #[test]
    fn lowers_string_helpers_to_shared_client_ir() {
        let lower_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn lower_label(s: String) -> String {
                    s.to_lowercase()
                }
            })
            .unwrap(),
            "labels__lower_label",
            Some("labels"),
        )
        .unwrap();
        assert!(lower_js.contains(".toLowerCase()"), "js: {lower_js}");

        let contains_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn label_contains(s: String, needle: &'static str) -> bool {
                    s.contains(needle)
                }
            })
            .unwrap(),
            "labels__label_contains",
            Some("labels"),
        )
        .unwrap();
        assert!(contains_js.contains(".includes("), "js: {contains_js}");

        let out = expand_ok(quote! {
            pub fn label_contains(s: String, needle: &'static str) -> bool {
                s.to_lowercase().contains(needle)
            }
        });
        assert!(
            out.contains("\\\"kind\\\":\\\"StringToLowercase\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"StringContains\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"string_to_lowercase\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"string_contains\\\""),
            "out: {out}"
        );
        assert!(out.contains("client_string_contains"), "out: {out}");
    }

    #[test]
    fn lowers_string_transforms_and_predicates_to_shared_client_ir() {
        let upper_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn upper_label(s: String) -> String {
                    s.to_uppercase()
                }
            })
            .unwrap(),
            "labels__upper_label",
            Some("labels"),
        )
        .unwrap();
        assert!(upper_js.contains(".toUpperCase()"), "js: {upper_js}");

        let trim_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn trim_label(s: String) -> String {
                    s.trim().to_string()
                }
            })
            .unwrap(),
            "labels__trim_label",
            Some("labels"),
        )
        .unwrap();
        assert!(trim_js.contains("\\u0085"), "js: {trim_js}");
        assert!(!trim_js.contains(".trim()"), "js: {trim_js}");

        let starts_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn label_starts(s: String, prefix: &'static str) -> bool {
                    s.starts_with(prefix)
                }
            })
            .unwrap(),
            "labels__label_starts",
            Some("labels"),
        )
        .unwrap();
        assert!(starts_js.contains(".startsWith("), "js: {starts_js}");

        let ends_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn label_ends(s: String, suffix: &'static str) -> bool {
                    s.ends_with(suffix)
                }
            })
            .unwrap(),
            "labels__label_ends",
            Some("labels"),
        )
        .unwrap();
        assert!(ends_js.contains(".endsWith("), "js: {ends_js}");

        let out = expand_ok(quote! {
            pub fn label_matches(s: String, prefix: &'static str, suffix: &'static str) -> bool {
                s.trim().to_uppercase().starts_with(prefix) && s.ends_with(suffix)
            }
        });
        assert!(
            out.contains("\\\"kind\\\":\\\"StringTransform\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"StringPredicate\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"op\\\":\\\"to_uppercase\\\""),
            "out: {out}"
        );
        assert!(out.contains("\\\"op\\\":\\\"trim\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"starts_with\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"ends_with\\\""), "out: {out}");
        assert!(
            out.contains("\\\"kind\\\":\\\"string_transform\\\""),
            "out: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"string_predicate\\\""),
            "out: {out}"
        );
        assert!(out.contains("client_string_transform"), "out: {out}");
        assert!(out.contains("client_string_predicate"), "out: {out}");
    }

    #[test]
    fn lowers_option_presence_methods_to_null_checks() {
        let some_js = lower_helper_fn_to_js(
            &syn::parse2(quote! {
                pub fn optional_label(value: Option<&'static str>) -> bool {
                    value.is_some()
                }
            })
            .unwrap(),
            "labels__optional_label",
            Some("labels"),
        )
        .unwrap();
        assert!(
            some_js.contains("(value !== null)"),
            "is_some should lower to strict null inequality, got:\n{some_js}"
        );

        let out = expand_ok(quote! {
            pub fn optional_missing(value: Option<&'static str>) -> bool {
                value.is_none()
            }
        });
        assert!(out.contains("Option < & str >"), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"Eq\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"eq\\\""), "out: {out}");
        assert!(out.contains("\\\"v\\\":\\\"Null\\\""), "out: {out}");
        assert!(out.contains("ClientValue :: Null"), "out: {out}");
    }

    #[test]
    fn lowers_some_and_none_for_option_returns() {
        let out = expand_ok(quote! {
            pub fn maybe_discount(price: i64, enabled: bool) -> Option<i64> {
                if enabled {
                    Some(price - 10)
                } else {
                    None
                }
            }
        });
        assert!(out.contains("\\\"v\\\":\\\"Null\\\""), "out: {out}");
        assert!(out.contains("ClientValue :: Null"), "out: {out}");
        assert!(
            out.contains("\\\"op\\\":\\\"CheckedI64Sub\\\"")
                && out.contains("\\\"op\\\":\\\"checked_i64_sub\\\""),
            "out: {out}"
        );
    }

    #[test]
    fn accepts_vec_and_slice_helper_types() {
        let out = expand_ok(quote! {
            pub fn has_more_labels(labels: Vec<&'static str>, scores: &[i64]) -> bool {
                labels.len() > scores.len()
            }
        });
        assert!(out.contains("Vec<&str>"), "out: {out}");
        assert!(out.contains("&[i64]"), "out: {out}");
        assert!(out.contains(":: std :: vec :: Vec < & str >"), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"Len\\\""), "out: {out}");
        assert!(out.contains("client_len"), "out: {out}");
    }

    #[test]
    fn accepts_nested_vec_option_shapes() {
        let out = expand_ok(quote! {
            pub fn no_scores(scores: Vec<Option<i64>>) -> bool {
                scores.is_empty()
            }
        });
        assert!(out.contains("Vec<Option<i64>>"), "out: {out}");
        assert!(
            out.contains("\\\"kind\\\":\\\"IsNonEmpty\\\""),
            "out: {out}"
        );
    }

    #[test]
    fn rejects_slice_return_type() {
        let msg = expand_err(quote! {
            pub fn scores(values: &[i64]) -> &[i64] {
                values
            }
        });
        assert!(msg.contains("slice return types"), "got: {msg}");
    }

    #[test]
    fn lowers_helper_call() {
        let out = expand_ok(quote! {
            pub fn double(x: i64) -> i64 {
                subtotal(x, 2)
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"ClientCall\\\""));
        assert!(out.contains("\\\"callee\\\":\\\"subtotal\\\""));
        assert!(out.contains("client_value_from_return"));
    }

    #[test]
    fn lowers_value_level_if_else() {
        let out = expand_ok(quote! {
            pub fn discounted(price: i64) -> i64 {
                if price > 100 {
                    price - 10
                } else {
                    price
                }
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"Conditional\\\""));
        assert!(out.contains("\\\"kind\\\":\\\"conditional\\\""));
        assert!(out.contains("client_bool"));
    }

    #[test]
    fn lowers_unary_ops() {
        let out = expand_ok(quote! {
            pub fn invert(flag: bool) -> bool {
                !flag
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"UnaryOp\\\""));
        assert!(out.contains("\\\"op\\\":\\\"Not\\\""));
        assert!(out.contains("client_unary_op"));

        let out = expand_ok(quote! {
            pub fn negative(value: i64) -> i64 {
                -value
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"unary\\\""));
        assert!(out.contains("\\\"op\\\":\\\"CheckedI64Neg\\\""));
        assert!(out.contains("\\\"op\\\":\\\"checked_i64_neg\\\""));
    }

    #[test]
    fn lowers_early_return_block() {
        let out = expand_ok(quote! {
            pub fn label(price: i64) -> String {
                if price > 100 {
                    return "premium".to_string();
                }
                "standard".to_string()
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"Block\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"block\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"if\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"return\\\""), "out: {out}");
    }

    #[test]
    fn lowers_tail_literal_match() {
        let out = expand_ok(quote! {
            pub fn status_label(status: i64) -> String {
                match status {
                    0 => "new".to_string(),
                    1 => "ready".to_string(),
                    _ => "unknown".to_string(),
                }
            }
        });
        assert!(out.contains("\\\"kind\\\":\\\"Block\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"If\\\""), "out: {out}");
        assert!(out.contains("\\\"kind\\\":\\\"if\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"Eq\\\""), "out: {out}");
        assert!(out.contains("\\\"op\\\":\\\"eq\\\""), "out: {out}");
        assert!(out.contains("__rsjs_match_0"), "out: {out}");
    }

    #[test]
    fn lowers_tail_range_match() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn bucket(score: i64) -> String {
                match score {
                    0..=59 => "low".to_string(),
                    60..100 => "high".to_string(),
                    _ => "unknown".to_string(),
                }
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "scores__bucket", Some("scores")).unwrap();
        assert!(
            js.contains("(__local_1 >= 0n)") && js.contains("(__local_1 <= 59n)"),
            "inclusive range should lower to >= and <=, got:\n{js}"
        );
        assert!(
            js.contains("(__local_1 >= 60n)") && js.contains("(__local_1 < 100n)"),
            "half-open range should lower to >= and <, got:\n{js}"
        );
    }

    #[test]
    fn lowers_tail_guarded_match_arm() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn bucket(score: i64) -> String {
                match score {
                    0 if score > 10 => "impossible".to_string(),
                    _ => "unknown".to_string(),
                }
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "scores__bucket", Some("scores")).unwrap();
        assert!(
            js.contains("(__local_1 === 0n)") && js.contains("&&") && js.contains("(score > 10n)"),
            "guarded arm should lower to pattern && guard, got:\n{js}"
        );
    }

    #[test]
    fn rejects_tail_match_without_fallback() {
        let msg = expand_err(quote! {
            pub fn status_label(status: i64) -> String {
                match status {
                    0 => "new",
                    1 => "ready",
                }
            }
        });
        assert!(msg.contains("needs a final `_` fallback"), "msg: {msg}");
    }

    #[test]
    fn rejects_non_tail_match_with_guidance() {
        let msg = expand_err(quote! {
            pub fn status_label(status: i64) -> String {
                let label = match status {
                    0 => "new",
                    _ => "unknown",
                };
                label
            }
        });
        assert!(msg.contains("only as a tail value"), "msg: {msg}");
    }

    #[test]
    fn rejects_value_level_if_without_else() {
        let msg = expand_err(quote! {
            pub fn discounted(price: i64) -> i64 {
                if price > 100 {
                    price - 10
                }
            }
        });
        assert!(msg.contains("must have an `else` branch"), "msg: {msg}");
    }

    #[test]
    fn rejects_statement_if_branch_without_explicit_return() {
        let msg = expand_err(quote! {
            pub fn label(price: i64) -> String {
                if price > 100 {
                    "premium"
                }
                "standard"
            }
        });
        assert!(msg.contains("must use explicit `return`"), "msg: {msg}");
    }

    #[test]
    fn bundle_js_emission_uses_v3_expr_emitter_with_param_names() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn subtotal(price: i64, qty: i64) -> i64 {
                price * qty
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "pricing__subtotal", Some("pricing")).unwrap();
        assert_eq!(
            js,
            "export function pricing__subtotal(price, qty) { return checked_int_mul(price, qty, \"i64\"); }\n"
        );
    }

    #[test]
    fn bundle_js_lowers_mutable_rust_struct_local_to_plain_object_fields() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn toggle_menu_value_object() -> bool {
                let mut menu = MenuController {
                    open: false,
                    label: "Menu",
                };
                menu.open = !menu.open;
                menu.open && menu.label == "Menu"
            }
        })
        .unwrap();
        let js =
            lower_helper_fn_to_js(&func, "menu__toggle_menu_value_object", Some("menu")).unwrap();
        assert!(
            js.contains(r#"let __local_0 = ({"open": false, "label": "Menu"});"#),
            "struct literal should become a plain object, got:\n{js}"
        );
        assert!(
            js.contains(r#"__local_0["open"] = "#)
                && js.contains(r#"(__local_0)["open"]"#)
                && js.contains(r#"(__local_0)["label"]"#),
            "field reads and assignment should use own-property access, got:\n{js}"
        );
    }

    #[test]
    fn bundle_js_lowers_mutable_scalar_locals_to_checked_assignments() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn adjust(value: i32, increase: bool) -> i32 {
                let mut result: i32 = value;
                if increase {
                    result += 2_i32;
                } else {
                    result -= 2_i32;
                }
                result
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "math__adjust", Some("math")).unwrap();
        assert!(js.contains("let __local_2 = value;"), "got:\n{js}");
        assert!(
            js.contains("__local_2 = checked_int_add(__local_2, 2, \"i32\");")
                && js.contains("__local_2 = checked_int_sub(__local_2, 2, \"i32\");"),
            "got:\n{js}"
        );
    }

    #[test]
    fn bundle_js_lowers_lexically_scoped_branch_locals() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn adjust(value: i32, increase: bool) -> i32 {
                let mut result: i32 = value;
                if increase {
                    let next: i32 = result + 2_i32;
                    result = next;
                } else {
                    let next: i32 = result - 2_i32;
                    result = next;
                }
                result
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "math__adjust", Some("math")).unwrap();
        assert!(
            js.contains("const __local_3 = checked_int_add"),
            "got:\n{js}"
        );
        assert!(
            js.contains("const __local_4 = checked_int_sub"),
            "got:\n{js}"
        );
        assert!(
            js.contains("__local_2 = __local_3;") && js.contains("__local_2 = __local_4;"),
            "got:\n{js}"
        );
    }

    #[test]
    fn branch_locals_do_not_escape_their_rust_scope() {
        let message = expand_err(quote! {
            pub fn adjust(value: i32, increase: bool) -> i32 {
                let mut result: i32 = value;
                if increase {
                    let next: i32 = result + 2_i32;
                    result = next;
                }
                result = next;
                result
            }
        });
        assert!(
            message.contains("unknown identifier `next`"),
            "got: {message}"
        );
    }

    #[test]
    fn value_objects_reject_identity_semantics_and_invalid_mutation() {
        let object_equality = expand_err(quote! {
            pub fn same_menu() -> bool {
                let left = MenuController { open: false };
                let right = MenuController { open: false };
                left == right
            }
        });
        assert!(
            object_equality.contains("cannot be used directly with binary operators"),
            "got: {object_equality}"
        );

        let immutable_scalar = expand_err(quote! {
            pub fn flip() -> bool {
                let flag = false;
                flag = true;
                flag
            }
        });
        assert!(
            immutable_scalar.contains("cannot assign to an immutable"),
            "got: {immutable_scalar}"
        );

        let immutable_field = expand_err(quote! {
            pub fn open() -> bool {
                let menu = MenuController { open: false };
                menu.open = true;
                menu.open
            }
        });
        assert!(
            immutable_field.contains("cannot assign through an immutable"),
            "got: {immutable_field}"
        );

        let unproven_integer_field = expand_err(quote! {
            pub fn is_zero() -> bool {
                let counter = CounterValue { count: 0 };
                counter.count == 0
            }
        });
        assert!(
            unproven_integer_field.contains("integer-domain-unproven"),
            "got: {unproven_integer_field}"
        );
    }

    #[test]
    fn bundle_js_rewrites_peer_helper_calls_to_export_names() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn double(price: i64, qty: i64) -> i64 {
                let subtotal_value: i64 = subtotal(price, qty);
                subtotal_value * 2
            }
        })
        .unwrap();
        let js = lower_helper_fn_to_js(&func, "pricing__double", Some("pricing")).unwrap();
        assert!(
            js.contains("pricing__subtotal(price, qty)")
                && js.contains("checked_int_mul(__local_2, 2n, \"i64\")"),
            "peer helper call should use the exported JS function name, got:\n{js}"
        );
    }

    #[test]
    fn bundle_js_emits_early_return_as_iife_fallback() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn label(price: i64) -> String {
                if price > 100 {
                    return "premium".to_string();
                }
                "standard".to_string()
            }
        })
        .unwrap();
        let js =
            lower_helper_fn_to_js(&func, "pricing_labels__label", Some("pricing_labels")).unwrap();
        assert!(
            js.contains(
                "return (() => {if ((price > 100n)) {return \"premium\";}return \"standard\";})();"
            ),
            "expected IIFE fallback for statement-level helper, got:\n{js}"
        );
    }

    #[test]
    fn client_helper_early_return_conformance_snapshots_match_lowering() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn label(price: i64) -> String {
                if price > 100 {
                    return "premium".to_string();
                }
                "standard".to_string()
            }
        })
        .unwrap();
        let lowered = lower_helper(&func)
            .unwrap_or_else(|err| panic!("fixture helper should lower: {}", err.what));

        assert_json_shape_snapshot(
            &lowered.body.to_json(),
            "rsjs_conformance/snapshots/client_helper/control_flow_early_return.body-hir.json",
        );
        assert_json_snapshot(
            &lowered.body.to_ir_json(),
            "rsjs_conformance/snapshots/client_helper/control_flow_early_return.body-ir.json",
        );
        assert_text_snapshot(
            &client_module_snapshot_js("pricing_labels", "label", &lowered),
            "rsjs_conformance/snapshots/client_helper/control_flow_early_return.bundle.js",
        );
    }

    #[test]
    fn client_helper_rust_string_semantics_conformance_snapshots_match_lowering() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn has_bytes(label: String) -> bool {
                let length = label.len();
                length > 0
            }
        })
        .unwrap();
        let lowered = lower_helper(&func)
            .unwrap_or_else(|err| panic!("fixture helper should lower: {}", err.what));

        assert_json_shape_snapshot(
            &lowered.body.to_json(),
            "rsjs_conformance/snapshots/client_helper/rust_string_semantics.body-hir.json",
        );
        assert_json_snapshot(
            &lowered.body.to_ir_json(),
            "rsjs_conformance/snapshots/client_helper/rust_string_semantics.body-ir.json",
        );
    }

    #[test]
    fn rejects_async() {
        let msg = expand_err(quote! {
            pub async fn boom(x: i64) -> i64 { x }
        });
        assert!(msg.contains("cannot be `async`"), "got: {msg}");
    }

    #[test]
    fn rejects_signature_qualifiers_instead_of_erasing_the_rust_contract() {
        let cases = [
            (
                quote! { pub const fn boom(x: i64) -> i64 { x } },
                "cannot be `const`",
            ),
            (
                quote! { pub unsafe fn boom(x: i64) -> i64 { x } },
                "cannot be `unsafe`",
            ),
            (
                quote! { pub extern "C" fn boom(x: i64) -> i64 { x } },
                "cannot declare an `extern` ABI",
            ),
            (
                quote! { pub unsafe extern "C" fn boom(x: i64, ...) -> i64 { x } },
                "cannot be variadic",
            ),
        ];
        for (input, expected) in cases {
            let message = expand_err(input);
            assert!(message.contains(expected), "got: {message}");
        }
    }

    #[test]
    fn rejects_non_plain_parameter_bindings() {
        for input in [
            quote! { pub fn boom(mut x: i64) -> i64 { x } },
            quote! { pub fn boom(ref x: i64) -> i64 { *x } },
            quote! { pub fn boom(x @ _: i64) -> i64 { x } },
        ] {
            let message = expand_err(input);
            assert!(
                message.contains("parameter patterns must be plain identifiers"),
                "got: {message}"
            );
        }
    }

    #[test]
    fn rejects_f32_signatures_locals_and_literals_until_rounding_is_explicit() {
        for input in [
            quote! { pub fn boom(value: f32) -> bool { value > 0.0 } },
            quote! { pub fn boom(value: bool) -> Option<Vec<f32>> { None } },
            quote! {
                pub fn boom() -> bool {
                    let boundary: f32 = 16_777_216.0_f32;
                    boundary + 1.0_f32 == boundary
                }
            },
            quote! { pub fn boom() -> bool { 16_777_216.0_f32 + 1.0_f32 == 16_777_216.0_f32 } },
        ] {
            let message = expand_err(input);
            assert!(
                message.contains("float-domain-unsupported"),
                "got: {message}"
            );
            assert!(message.contains("f32"), "got: {message}");
        }
    }

    #[test]
    fn borrowed_strings_remain_parameters_but_are_rejected_in_returns() {
        expand_ok(quote! {
            pub fn present(value: &'static str) -> bool { value.is_non_empty() }
        });

        for input in [
            quote! { pub fn boom(value: &'static str) -> &'static str { value } },
            quote! {
                pub fn boom(value: &'static str) -> Option<Vec<&'static str>> {
                    Some(vec![value])
                }
            },
        ] {
            let message = expand_err(input);
            assert!(
                message.contains("borrowed-return-unsupported"),
                "got: {message}"
            );
        }
    }

    #[test]
    fn helper_calls_keep_the_exact_compiler_path_separate_from_descriptor_text() {
        let func: syn::ItemFn = syn::parse2(quote! {
            pub fn local(value: i32) -> i32 {
                ::external_helper_dep::public::reexported_serial(value)
            }
        })
        .unwrap();
        let lowered = lower_helper(&func)
            .unwrap_or_else(|error| panic!("absolute helper path should lower: {}", error.what));
        let calls = lowered.body.direct_client_calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].path.leading_colon.is_some());
        assert_eq!(
            calls[0].authored_path,
            "::external_helper_dep::public::reexported_serial"
        );

        let server = lowered.body.to_server_eval_tokens().to_string();
        assert!(
            server.contains(":: external_helper_dep :: public :: reexported_serial"),
            "server evaluator must use the original absolute path: {server}"
        );
        let name = format_ident!("local");
        let edges = emit_helper_direct_call_edges(&name, [7; 32], [9; 32], &calls).to_string();
        assert!(
            edges.contains(":: external_helper_dep :: public :: reexported_serial"),
            "relocation must use the original absolute path: {edges}"
        );
    }

    #[test]
    fn rejects_helper_call_path_arguments_at_the_authored_boundary() {
        for input in [
            quote! { pub fn local(value: i32) -> i32 { helper::<i32>(value) } },
            quote! { pub fn local(value: i32) -> i32 { module::helper::<'static>(value) } },
        ] {
            let message = expand_err(input);
            assert!(
                message.contains("cannot use explicit path arguments"),
                "got: {message}"
            );
        }
    }

    #[test]
    fn rejects_closure() {
        // A closure-typed body would not type-check as `i64`, but the
        // attribute runs before type-checking so we reject the closure
        // tail expression on its own merits with a span-pointed error.
        let msg = expand_err(quote! {
            pub fn boom(x: i64) -> i64 {
                |y: i64| y + x
            }
        });
        assert!(msg.contains("closures are not allowed"), "got: {msg}");
    }

    #[test]
    fn rejects_cx_reference() {
        let msg = expand_err(quote! {
            pub fn boom(x: i64) -> i64 { cx }
        });
        assert!(msg.contains("`cx` is not accessible"), "got: {msg}");
    }

    #[test]
    fn rejects_unknown_method() {
        let msg = expand_err(quote! {
            pub fn boom(x: i64) -> i64 { x.frobnicate() }
        });
        assert!(msg.contains("frobnicate"), "got: {msg}");
        assert!(msg.contains("not supported"), "got: {msg}");
    }

    #[test]
    fn rejects_outer_scope_capture() {
        let msg = expand_err(quote! {
            pub fn boom(x: i64) -> i64 { OUTSIDE + x }
        });
        assert!(msg.contains("unknown identifier"), "got: {msg}");
    }

    #[test]
    fn rejects_self_param() {
        let msg = expand_err(quote! {
            pub fn boom(self) -> i64 { 0 }
        });
        assert!(msg.contains("free functions"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_param_type() {
        let msg = expand_err(quote! {
            pub fn boom(c: std::collections::HashMap<i64, i64>) -> i64 { 0 }
        });
        assert!(msg.contains("unsupported parameter type"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_integer_inside_option_parameter() {
        let msg = expand_err(quote! {
            pub fn boom(value: Option<u8>) -> bool { value.is_some() }
        });
        assert!(msg.contains("integer-domain-unsupported"), "got: {msg}");
        assert!(msg.contains("`u8`"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_integer_inside_vec_return() {
        let msg = expand_err(quote! {
            pub fn boom(values: Vec<i32>) -> Vec<u16> { values }
        });
        assert!(msg.contains("integer-domain-unsupported"), "got: {msg}");
        assert!(msg.contains("`u16`"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_integer_inside_borrowed_slice_local() {
        let msg = expand_err(quote! {
            pub fn boom(values: &[i32]) -> bool {
                let unsupported: &[i128] = values;
                unsupported.is_empty()
            }
        });
        assert!(msg.contains("integer-domain-unsupported"), "got: {msg}");
        assert!(msg.contains("`i128`"), "got: {msg}");
    }

    #[test]
    fn lowers_let_binding() {
        let out = expand_ok(quote! {
            pub fn boom(x: i64) -> i64 {
                let y = x + 1;
                y
            }
        });
        assert!(
            out.contains("\\\"kind\\\":\\\"Let\\\""),
            "missing legacy let node: {out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"let\\\""),
            "missing Client IR let node: {out}"
        );
        assert!(
            out.contains("__rsjs_client_local_1"),
            "missing server-eval local: {out}"
        );
    }

    #[test]
    fn rejects_await() {
        let msg = expand_err(quote! {
            pub fn boom(x: i64) -> i64 { x.await }
        });
        assert!(msg.contains("`.await` is not allowed"), "got: {msg}");
    }

    /// Round-3 review #3: a lifetime-parameter on a `&str` argument
    /// must not be confused with a generic-type parameter rejection.
    /// `fn helper<'a>(s: &'a str)` should expand cleanly because the
    /// lifetime carries no JS-side meaning.
    #[test]
    fn accepts_lifetime_only_generics_on_str_ref_param() {
        let out = expand_ok(quote! {
            pub fn echo<'a>(s: &'a str) -> bool {
                s.is_non_empty()
            }
        });
        // The regenerated fn must not carry the user's `'a` (we
        // strip it from the regen impl) and the param type must
        // normalise to `&str`.
        assert!(
            out.contains("__rsjs_client_echo_meta"),
            "expected echo meta to be emitted: {out}"
        );
        // The public regenerated function uses elided `&str`; the nested
        // dead source checker deliberately retains `'a` so rustc validates
        // the original signature and body.
        assert!(
            out.contains("pub fn echo (s : & str) -> bool")
                && out.contains("__rsjs_client_echo_source_typecheck < 'a >"),
            "regen signature should normalize the public lifetime while preserving source checking: {out}"
        );
    }

    /// Type generics still produce the rejection error, but the new
    /// wording calls out the lifetime carve-out.
    #[test]
    fn rejects_type_generics_with_clear_message() {
        let msg = expand_err(quote! {
            pub fn boom<T>(x: T) -> i64 { 0 }
        });
        assert!(msg.contains("cannot be generic"), "got: {msg}");
    }
}
