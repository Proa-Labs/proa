// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `md_sync!` / `md!` macros — write Markdown naturally with `{expr}`
//! interpolation.
//!
//! `md_sync!` returns an `OptimizedMd` for `MdRenderSync` composition. `md!`
//! returns an async-capable Markdown fragment that drives interpolated
//! `MdRender` children.
//!
//! The split mirrors `html_sync!` / `html!`: synchronous children compose
//! directly in `md_sync!`, while `md!` awaits async-capable children.
//!
//! # Raw Markdown template mode (preferred)
//!
//! ```rust,ignore
//! md_sync! { r###"
//!     # {data.title}
//!
//!     {for highlight in &data.highlights {
//!         md_sync! { "- **{highlight.title}** — {highlight.description}\n" }
//!     }}
//!     "###
//! }
//! .render_md(cx);
//! ```
//!
//! The raw string is the Markdown document, so source newlines are output
//! newlines and no escaped `"\n"` separators are needed. The grammar has one
//! rule at every depth: text outside `{ ... }` is Markdown, text inside is a
//! Rust expression. `{expr}` composes a typed render child; `{if ...}`,
//! `{for ...}`, and `{match ...}` are ordinary Rust control flow whose
//! bodies render only their final expression, just like HTML slots. A
//! branch re-enters Markdown through a nested `md_sync!` / `md!` call (a
//! string tail renders as escaped text). Plain Markdown is never parsed inside
//! a slot, and no word is reserved in Markdown text. Braces inside fenced code
//! blocks and inline code spans remain literal Markdown; outside code, `{{`
//! and `}}` render literal braces.
//!
//! Keep short fragments on one line, such as `md_sync! { "- {item}\n" }`.
//! A single string is still a template; retain its newline to separate rows.
//! Reserve raw literals for substantial multiline fragments or useful escaping.
//! Three hashes are the default delimiter for documents; nested raw templates
//! use fewer hashes so their closing delimiter cannot close the parent.
//! Increase the delimiter when the document contains the exact closing
//! sequence `"###`.
//!
//! Physically multiline raw literals select block mode and complete paragraph
//! boundaries before their body and after successful rendering. Ordinary strings
//! and single-line raw literals select inline mode without adding boundaries.
//! Scopes restore the enclosing mode on errors, unwinding, or async cancellation;
//! blocks cannot render inside inline scopes. Token composition inherits its mode.
//!
//! Physically multiline raw templates remove their common source indentation,
//! preserving relative indentation without leaking the surrounding Rust block's
//! alignment into the rendered Markdown:
//!
//! ```rust,ignore
//! md_sync! { r###"
//!     ---
//!     title: Example
//!     description: Source indentation is not rendered.
//!     ---
//!     "###
//! }
//! ```
//!
//! # Token composition mode
//!
//! ```rust,ignore
//! md_sync! { "# " { data.title } }.render_md(cx);
//! ```
//!
//! This expression-visible form remains available for generated templates and
//! tooling that needs Rust tokens outside a string literal.
//!
//! Raw templates keep Markdown inline with the Rust component. The token
//! composition form above is also supported for expression-visible tooling.

use proa_dsl::md::{bare_ident, text_value, MdNodeKind, MdTemplateArg, TplNode};
pub use proa_dsl::md::{MdRoot, MdTemplateInput};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, LitStr};

#[derive(Clone, Copy)]
pub enum EmitMode {
    Sync,
    Async,
}

// ═══════════════════════════════════════════════════════════════
//  Shared codegen helpers
// ═══════════════════════════════════════════════════════════════

fn render_context() -> syn::Ident {
    syn::Ident::new("__proa_md_cx", proc_macro2::Span::mixed_site())
}

/// Flush accumulated static text into one state-aware prepared-static write.
fn flush(stmts: &mut Vec<TokenStream>, run: &mut String) {
    if !run.is_empty() {
        let cx = render_context();
        let bytes = proc_macro2::Literal::byte_string(run.as_bytes());
        stmts.push(quote! {
            {
                static BYTES: &[u8] = #bytes;
                #cx.write_md_prepared_static(BYTES)?;
            }
        });
        run.clear();
    }
}

/// Emit a Markdown render call on the interpolated expression.
///
/// Method syntax preserves Rust's auto-deref behavior (for example,
/// `&&str` → `&str`) on both the sync and async paths.
fn emit_md_render(expr: &Expr, mode: EmitMode) -> TokenStream {
    let cx = render_context();
    match mode {
        EmitMode::Sync => quote! {
            (#expr).__proa_render_md_sync(#cx)?;
        },
        EmitMode::Async => quote! {
            (#expr).__proa_render_md_async(#cx).resolve().await?;
        },
    }
}

/// Wrap generated body statements in the appropriate runtime fragment.
fn wrap_render(stmts: &[TokenStream], mode: EmitMode) -> TokenStream {
    let cx = render_context();
    match mode {
        EmitMode::Sync => quote! {
            ::proa_core::OptimizedMd::new(
                |#cx: &mut ::proa_core::WebContext<_, _>| {
                    use ::proa_core::MdRenderSyncExt as _;
                    #(#stmts)*
                    ::core::result::Result::Ok(())
                }
            )
        },
        EmitMode::Async => quote! {
            ::proa_core::md::async_md_from_closure(async move |#cx| {
                use ::proa_core::MdRenderAsyncExt as _;
                #(#stmts)*
                ::core::result::Result::Ok(())
            })
        },
    }
}

fn static_fragment(bytes: proc_macro2::Literal) -> TokenStream {
    let cx = render_context();
    quote! {
        ::proa_core::OptimizedMd::new(
            |#cx: &mut ::proa_core::WebContext<_, _>| {
                static BYTES: &[u8] = #bytes;
                #cx.write_md_prepared_static(BYTES)?;
                ::core::result::Result::Ok(())
            }
        )
    }
}

// ═══════════════════════════════════════════════════════════════
//  Raw Markdown template mode: md_sync! { r###"# {title}\n\nBody"### }
// ═══════════════════════════════════════════════════════════════

pub fn generate_template(tpl: &MdTemplateInput, mode: EmitMode) -> Result<TokenStream, syn::Error> {
    let lit = &tpl.lit;
    let nodes = tpl.parse_nodes()?;
    let block = proa_dsl::md::is_multiline_raw_literal(lit);
    let inline_slots = proa_dsl::md::inline_slot_contexts(&nodes);
    let mut slots = inline_slots.iter();
    for node in &nodes {
        if let TplNode::Expr(expr) = node {
            if !block || *slots.next().expect("one context per expression") {
                reject_direct_block_child(expr)?;
            }
        }
    }
    let shadow = format_args_shadow(lit, &nodes, &tpl.args);
    let body = codegen_tpl(&nodes, &inline_slots, shadow, tpl.argument_binding(), mode);
    let template_mode = if block {
        quote! { ::proa_core::md::MD_TEMPLATE_BLOCK }
    } else {
        quote! { ::proa_core::md::MD_TEMPLATE_INLINE }
    };
    Ok(quote! {
        (#body).__proa_template_mode::<{ #template_mode }>()
    })
}

/// Directly rendered block macros have a known mode. Opaque/typed children
/// retain the runtime check, including when they are reached through helpers.
fn reject_direct_block_child(expr: &Expr) -> syn::Result<()> {
    let error = std::cell::RefCell::new(None);
    crate::render_control::lower(expr, &|child| {
        if let Expr::Macro(child) = child {
            // Unqualified names may be shadowed by user macros. Only claim
            // the known Proa path; other children retain the runtime check.
            if child.mac.path.segments.len() == 2
                && child.mac.path.segments[0].ident == "proa_macros"
                && child
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "md_sync" || segment.ident == "md")
            {
                if let Ok(template) = syn::parse2::<MdTemplateInput>(child.mac.tokens.clone()) {
                    if proa_dsl::md::is_multiline_raw_literal(&template.lit) {
                        *error.borrow_mut() = Some(syn::Error::new_spanned(
                            child,
                            "cannot render a block Markdown template inside an inline scope; use an ordinary-string inline template",
                        ));
                    }
                }
            }
        }
        TokenStream::new()
    });
    match error.into_inner() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Emit an IDE-only `format_args!` shadow of the template.
///
/// rust-analyzer resolves `{ident}` placeholders inside `format_args!` string
/// literals — go-to-definition, rename, completions — but only for the real
/// compiler builtin. Passing the original literal token (span intact) through
/// a dead `format_args!` statement borrows that support for templates whose
/// placeholders are exactly what `format_args!` would parse.
///
/// The shadow is emitted only under `cfg(rust_analyzer)`, which rust-analyzer
/// sets and rustc never does, so real builds compile nothing extra and the
/// `Display` bound `format_args!` puts on interpolated values applies only
/// inside the IDE. Templates interpolating non-`Display` values (nested
/// fragments, tuples) opt out by writing the placeholder as `{ ident }` with
/// spaces — Markdown rendering trims them; `format_args!` compatibility does
/// not.
fn format_args_shadow(
    lit: &LitStr,
    nodes: &[TplNode],
    args: &[MdTemplateArg],
) -> Option<TokenStream> {
    // The scan below runs on the literal's decoded value while format_args!
    // reads its source text, so the two must agree byte-for-byte where braces
    // are concerned. Raw literals always do; cooked literals only when they
    // contain no escapes (`\u{7b}` would desynchronize them).
    let token = lit.token().to_string();
    if !token.starts_with('r') && token.contains('\\') {
        return None;
    }
    let mut placeholders = Vec::new();
    for node in nodes {
        match node {
            TplNode::Text(_) => {}
            // A control-flow slot is not a bare identifier, so a template
            // containing one is not a format string.
            TplNode::Expr(expr) => placeholders.push(bare_ident(expr)?.to_string()),
        }
    }
    if placeholders.is_empty() {
        return None;
    }
    // The Markdown parser leaves braces inside fenced code and inline code
    // spans literal, and trims `{ spaced }` placeholder contents; format_args!
    // does neither. Re-scan the raw value and require exact agreement so the
    // shadow can never diverge from template semantics.
    if scan_bare_brace_regions(&lit.value())? != placeholders {
        return None;
    }
    let names = args.iter().map(|arg| &arg.name);
    let values = args.iter().map(|arg| &arg.value);
    Some(quote! {
        #[allow(unexpected_cfgs)]
        {
            #[cfg(rust_analyzer)]
            let _ = ::core::format_args!(#lit #(, #names = #values)*);
        }
    })
}

/// Scan `{...}` regions the way `format_args!` tokenizes them: `{{` and `}}`
/// are escapes, everything else must open a placeholder. Returns the region
/// contents in order when every region is exactly a bare identifier; `None`
/// otherwise.
fn scan_bare_brace_regions(value: &str) -> Option<Vec<String>> {
    let bytes = value.as_bytes();
    let mut regions = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'{' if bytes.get(pos + 1) == Some(&b'{') => pos += 2,
            b'}' if bytes.get(pos + 1) == Some(&b'}') => pos += 2,
            b'{' => {
                let end = value[pos + 1..].find('}')? + pos + 1;
                let inner = &value[pos + 1..end];
                if !is_bare_ident(inner) {
                    return None;
                }
                regions.push(inner.to_string());
                pos = end + 1;
            }
            b'}' => return None,
            _ => pos += 1,
        }
    }
    Some(regions)
}

fn is_bare_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

// -- template codegen --

fn codegen_tpl(
    nodes: &[TplNode],
    inline_slots: &[bool],
    shadow: Option<TokenStream>,
    binding: Option<syn::Local>,
    mode: EmitMode,
) -> TokenStream {
    // all-static fast path
    if binding.is_none() && nodes.iter().all(|n| matches!(n, TplNode::Text(_))) {
        let mut s = String::new();
        for n in nodes {
            if let TplNode::Text(t) = n {
                s.push_str(t);
            }
        }
        let bytes = proc_macro2::Literal::byte_string(s.as_bytes());
        return static_fragment(bytes);
    }

    let mut stmts = Vec::new();
    // First so its borrows begin and end before any rendering statement.
    stmts.extend(shadow);
    stmts.extend(binding.map(|binding| quote! { #binding }));
    let mut run = String::new();
    emit_tpl_nodes(nodes, inline_slots, &mut stmts, &mut run, mode);
    flush(&mut stmts, &mut run);
    wrap_render(&stmts, mode)
}

fn emit_tpl_nodes(
    nodes: &[TplNode],
    inline_slots: &[bool],
    stmts: &mut Vec<TokenStream>,
    run: &mut String,
    mode: EmitMode,
) {
    let mut slots = inline_slots.iter().copied();
    for node in nodes {
        match node {
            TplNode::Text(t) => run.push_str(t),
            // Control flow and plain values share the token-mode lowering, so
            // both template forms render a branch's final expression the same
            // way.
            TplNode::Expr(expr) => {
                flush(stmts, run);
                if slots.next().expect("one context per expression") {
                    let cx = render_context();
                    let scope =
                        syn::Ident::new("__proa_md_inline_scope", proc_macro2::Span::mixed_site());
                    let mut child = Vec::new();
                    gen_block_expr(&expr.expr, &mut child, mode);
                    stmts.push(quote! {
                        {
                            let mut #scope = ::proa_core::md::__MdTemplateScope::<_, _, { ::proa_core::md::MD_TEMPLATE_INLINE }>::enter(#cx)?;
                            let #cx = #scope.context();
                            #(#child)*
                        }
                    });
                } else {
                    gen_block_expr(&expr.expr, stmts, mode);
                }
            }
        }
    }
}

// -- template parser --

// ═══════════════════════════════════════════════════════════════
//  Token composition mode: source lines are implicit Markdown block boundaries
// ═══════════════════════════════════════════════════════════════

pub fn generate_tokens(root: &MdRoot, mode: EmitMode) -> TokenStream {
    let all_static = root
        .nodes
        .iter()
        .all(|node| matches!(&node.kind, MdNodeKind::Text { .. }) && !node.starts_new_line);
    if all_static {
        let mut text = String::new();
        for node in &root.nodes {
            if let MdNodeKind::Text { value, dedent } = &node.kind {
                text.push_str(&text_value(value, *dedent));
            }
        }
        let bytes = proc_macro2::Literal::byte_string(text.as_bytes());
        return static_fragment(bytes);
    }

    let mut stmts = Vec::new();
    let mut run = String::new();
    for node in &root.nodes {
        if node.starts_new_line {
            let cx = render_context();
            flush(&mut stmts, &mut run);
            stmts.push(quote! { #cx.md_block_boundary()?; });
        }
        match &node.kind {
            MdNodeKind::Text { value, dedent } => run.push_str(&text_value(value, *dedent)),
            MdNodeKind::Block(expr) => {
                flush(&mut stmts, &mut run);
                gen_block_expr(expr, &mut stmts, mode);
            }
        }
    }
    flush(&mut stmts, &mut run);
    wrap_render(&stmts, mode)
}

// Raw and token templates share the same Rust control-flow lowering as HTML.
fn gen_block_expr(expr: &Expr, stmts: &mut Vec<TokenStream>, mode: EmitMode) {
    stmts.push(crate::render_control::lower(expr, &|expr| {
        emit_md_render(expr, mode)
    }));
}

#[cfg(test)]
mod template_tests {
    use super::*;

    fn expand(input: proc_macro2::TokenStream) -> String {
        let tpl: MdTemplateInput = syn::parse2(input).expect("template input parses");
        generate_template(&tpl, EmitMode::Sync)
            .expect("template generates")
            .to_string()
    }

    fn expand_err(input: proc_macro2::TokenStream) -> String {
        let tpl: MdTemplateInput = syn::parse2(input).expect("template input parses");
        generate_template(&tpl, EmitMode::Sync)
            .expect_err("template must fail")
            .to_string()
    }

    #[test]
    fn literal_modes_are_independent_of_every_supported_raw_hash_count() {
        for count in 0..=255 {
            let hashes = "#".repeat(count);
            for (body, expected) in [
                ("\n## Heading\n", "MD_TEMPLATE_BLOCK"),
                ("inline", "MD_TEMPLATE_INLINE"),
            ] {
                let source = format!("r{hashes}\"{body}\"{hashes}");
                let template: MdTemplateInput = syn::parse_str(&source).unwrap();
                for mode in [EmitMode::Sync, EmitMode::Async] {
                    let output = generate_template(&template, mode).unwrap().to_string();
                    assert!(output.contains(expected), "{count} hashes: {output}");
                }
            }
        }
        for source in ["\"one\\ntwo\"", "\"one\ntwo\""] {
            let cooked: MdTemplateInput = syn::parse_str(source).unwrap();
            assert!(generate_template(&cooked, EmitMode::Sync)
                .unwrap()
                .to_string()
                .contains("MD_TEMPLATE_INLINE"));
        }
    }

    #[test]
    fn shadow_emitted_for_bare_ident_placeholders() {
        let out = expand(quote! { r###"
            # {title}

            {body}
            "### });
        assert!(out.contains("format_args"), "missing shadow: {out}");
        assert!(
            out.contains("rust_analyzer"),
            "shadow must be IDE-only: {out}"
        );
    }

    #[test]
    fn shadow_skipped_for_field_access_placeholders() {
        let out = expand(quote! { r###"
            | {count} | {stats.min} |
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }

    #[test]
    fn shadow_skipped_for_control_flow() {
        let out = expand(quote! { r###"
            {for item in &items { md_sync! { "- " {item} "\n" } }}
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }

    #[test]
    fn spaced_placeholder_opts_out_of_the_shadow() {
        let out = expand(quote! { r###"
            # {title}

            { body }
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }

    #[test]
    fn code_fence_braces_disqualify_the_shadow() {
        let out = expand(quote! { r###"
            # {title}

            ```rust
            fn demo() { body }
            ```
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }

    #[test]
    fn named_args_bind_once_and_keep_the_shadow() {
        let out = expand(quote! { r###"
            | {count} | {min} |
            "###, min = stats.min });
        assert!(out.contains("format_args"), "missing shadow: {out}");
        assert!(
            out.contains("min = stats . min") || out.contains("min = stats.min"),
            "shadow must carry the named argument: {out}"
        );
        assert!(
            out.contains("stats . min") || out.contains("stats.min"),
            "render body must bind the named expression: {out}"
        );
    }

    #[test]
    fn named_args_delegate_unused_checks_to_rust() {
        let out = expand(quote! { r###"
            # {title}
            "###, extra = 1 });
        assert!(
            out.contains("deny (unused_variables)"),
            "missing lint: {out}"
        );
    }

    #[test]
    fn duplicate_named_arg_is_an_error() {
        let err = expand_err(quote! { r###"
            {value} {value}
            "###, value = 1, value = 2 });
        assert!(err.contains("duplicate"), "wrong error: {err}");
    }

    #[test]
    fn all_static_template_emits_no_shadow() {
        let out = expand(quote! { r###"
            # Static heading
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }
}
