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
//! # {data.title}
//!
//! {for highlight in &data.highlights {
//! - **{highlight.title}** — {highlight.description}
//! }}
//! "###
//! }
//! .render_md(cx);
//! ```
//!
//! The raw string is the Markdown document, so source newlines are output
//! newlines and no escaped `"\n"` separators are needed. `{expr}` composes a
//! typed render child and `{for ... { ... }}` / `{if ... { ... }}` provide
//! control flow. Braces inside fenced code blocks and inline code spans remain
//! literal Markdown; outside code, `{{` and `}}` render literal braces.
//!
//! Three hashes are the default delimiter so ordinary quotes and Rust raw
//! strings with fewer hashes can appear in the Markdown. Increase the delimiter
//! only when the document itself contains the exact closing sequence `"###`.
//!
//! Physically multiline raw templates remove their common source indentation,
//! preserving relative indentation without leaking the surrounding Rust block's
//! alignment into the rendered Markdown:
//!
//! ```rust,ignore
//! md_sync! { r###"
//! ---
//! title: Example
//! description: Source indentation is not rendered.
//! ---
//! "###
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

use proa_dsl::md::{
    bare_ident, substitute_named_args, text_value, IfBranch, MdNodeKind, MdTemplateArg, TplNode,
};
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

/// Flush accumulated static text into one state-aware prepared-static write.
fn flush(stmts: &mut Vec<TokenStream>, run: &mut String) {
    if !run.is_empty() {
        let bytes = proc_macro2::Literal::byte_string(run.as_bytes());
        stmts.push(quote! {
            {
                static BYTES: &[u8] = #bytes;
                cx.write_md_prepared_static(BYTES)?;
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
    match mode {
        EmitMode::Sync => quote! {
            (#expr).__proa_render_md_sync(cx)?;
        },
        EmitMode::Async => quote! {
            (#expr).__proa_render_md_async(cx).resolve().await?;
        },
    }
}

/// Wrap generated body statements in the appropriate runtime fragment.
fn wrap_render(stmts: &[TokenStream], mode: EmitMode) -> TokenStream {
    match mode {
        EmitMode::Sync => quote! {
            ::proa_core::OptimizedMd::new(
                |cx: &mut ::proa_core::WebContext<_, _>| {
                    use ::proa_core::MdRenderSyncExt as _;
                    #(#stmts)*
                    Ok(())
                }
            )
        },
        EmitMode::Async => quote! {
            ::proa_core::md::async_md_from_fn(::std::boxed::Box::new(|cx| {
                ::std::boxed::Box::pin(async move {
                    use ::proa_core::MdRenderAsyncExt as _;
                    #(#stmts)*
                    Ok(())
                })
            }))
        },
    }
}

fn static_fragment(bytes: proc_macro2::Literal) -> TokenStream {
    quote! {
        ::proa_core::OptimizedMd::new(
            |cx: &mut ::proa_core::WebContext<_, _>| {
                static BYTES: &[u8] = #bytes;
                cx.write_md_prepared_static(BYTES)?;
                Ok(())
            }
        )
    }
}

// ═══════════════════════════════════════════════════════════════
//  Raw Markdown template mode: md_sync! { r###"# {title}\n\nBody"### }
// ═══════════════════════════════════════════════════════════════

pub fn generate_template(tpl: &MdTemplateInput, mode: EmitMode) -> Result<TokenStream, syn::Error> {
    let lit = &tpl.lit;
    let mut nodes = tpl.parse_nodes()?;
    // Computed before substitution: the shadow describes the placeholders as
    // written in the string, with named arguments passed through alongside.
    let shadow = format_args_shadow(lit, &nodes, &tpl.args);
    substitute_named_args(&mut nodes, &tpl.args)?;
    Ok(codegen_tpl(&nodes, shadow, mode))
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
            TplNode::Expr(expr) => placeholders.push(bare_ident(expr)?.to_string()),
            // Control-flow templates are not format strings.
            TplNode::For { .. } | TplNode::If { .. } => return None,
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
        #[cfg(rust_analyzer)]
        let _ = ::core::format_args!(#lit #(, #names = #values)*);
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

fn codegen_tpl(nodes: &[TplNode], shadow: Option<TokenStream>, mode: EmitMode) -> TokenStream {
    // all-static fast path
    if nodes.iter().all(|n| matches!(n, TplNode::Text(_))) {
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
    let mut run = String::new();
    emit_tpl_nodes(nodes, &mut stmts, &mut run, mode);
    flush(&mut stmts, &mut run);
    wrap_render(&stmts, mode)
}

fn emit_tpl_nodes(
    nodes: &[TplNode],
    stmts: &mut Vec<TokenStream>,
    run: &mut String,
    mode: EmitMode,
) {
    for node in nodes {
        match node {
            TplNode::Text(t) => run.push_str(t),
            TplNode::Expr(expr) => {
                flush(stmts, run);
                stmts.push(emit_md_render(expr, mode));
            }
            TplNode::For { pat, iter, body } => {
                flush(stmts, run);
                let mut inner = Vec::new();
                let mut inner_run = String::new();
                emit_tpl_nodes(body, &mut inner, &mut inner_run, mode);
                flush(&mut inner, &mut inner_run);
                stmts.push(quote! {
                    for #pat in #iter { #(#inner)* }
                });
            }
            TplNode::If { branches } => {
                flush(stmts, run);
                stmts.push(codegen_if(branches, mode));
            }
        }
    }
}

fn codegen_if(branches: &[IfBranch], mode: EmitMode) -> TokenStream {
    if branches.is_empty() {
        return quote! {};
    }
    let first = &branches[0];
    let cond = first.cond.as_ref().unwrap();
    let then = codegen_tpl_body(&first.body, mode);
    let mut elses = TokenStream::new();
    for branch in &branches[1..] {
        let body = codegen_tpl_body(&branch.body, mode);
        if let Some(c) = &branch.cond {
            elses.extend(quote! { else if #c { #body } });
        } else {
            elses.extend(quote! { else { #body } });
        }
    }
    quote! { if #cond { #then } #elses }
}

fn codegen_tpl_body(nodes: &[TplNode], mode: EmitMode) -> TokenStream {
    let mut stmts = Vec::new();
    let mut run = String::new();
    emit_tpl_nodes(nodes, &mut stmts, &mut run, mode);
    flush(&mut stmts, &mut run);
    quote! { #(#stmts)* }
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
            flush(&mut stmts, &mut run);
            stmts.push(quote! { cx.md_block_boundary()?; });
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

// -- control-flow transforms for token mode --

fn gen_block_expr(expr: &Expr, stmts: &mut Vec<TokenStream>, mode: EmitMode) {
    match expr {
        Expr::If(e) => {
            let cond = &e.cond;
            let then = xform_block(&e.then_branch, mode);
            let el = e.else_branch.as_ref().map(|(_, ex)| gen_else(ex, mode));
            stmts.push(quote! { if #cond { #then } #el });
        }
        Expr::Match(e) => {
            let scrut = &e.expr;
            let arms: Vec<_> = e
                .arms
                .iter()
                .map(|a| {
                    let pat = &a.pat;
                    let guard = a.guard.as_ref().map(|(t, g)| quote! { #t #g });
                    let body = gen_expr_write(&a.body, mode);
                    quote! { #pat #guard => { #body } }
                })
                .collect();
            stmts.push(quote! { match #scrut { #(#arms),* } });
        }
        Expr::ForLoop(e) => {
            let pat = &e.pat;
            let it = &e.expr;
            let body = xform_block(&e.body, mode);
            stmts.push(quote! { for #pat in #it { #body } });
        }
        Expr::While(e) => {
            let cond = &e.cond;
            let body = xform_block(&e.body, mode);
            stmts.push(quote! { while #cond { #body } });
        }
        Expr::Loop(e) => {
            let body = xform_block(&e.body, mode);
            stmts.push(quote! { loop { #body } });
        }
        _ => {
            stmts.push(emit_md_render(expr, mode));
        }
    }
}

fn gen_else(expr: &Expr, mode: EmitMode) -> TokenStream {
    match expr {
        Expr::If(e) => {
            let cond = &e.cond;
            let then = xform_block(&e.then_branch, mode);
            let el = e.else_branch.as_ref().map(|(_, ex)| gen_else(ex, mode));
            quote! { else if #cond { #then } #el }
        }
        Expr::Block(e) => {
            let body = xform_block(&e.block, mode);
            quote! { else { #body } }
        }
        _ => {
            let w = gen_expr_write(expr, mode);
            quote! { else { #w } }
        }
    }
}

fn xform_block(block: &syn::Block, mode: EmitMode) -> TokenStream {
    let s = &block.stmts;
    if s.is_empty() {
        return quote! {};
    }
    let (init, last) = s.split_at(s.len() - 1);
    match &last[0] {
        syn::Stmt::Expr(expr, None) => {
            let w = gen_expr_write(expr, mode);
            quote! { #(#init)* #w }
        }
        syn::Stmt::Macro(m) if m.semi_token.is_none() => {
            let mac = &m.mac;
            match mode {
                EmitMode::Sync => {
                    quote! {
                        #(#init)*
                        {
                            use ::proa_core::MdRenderSyncExt as _;
                            (#mac).__proa_render_md_sync(cx)?;
                        }
                    }
                }
                EmitMode::Async => {
                    quote! {
                        #(#init)*
                        {
                            use ::proa_core::MdRenderAsyncExt as _;
                            (#mac).__proa_render_md_async(cx).resolve().await?;
                        }
                    }
                }
            }
        }
        _ => quote! { #(#s)* },
    }
}

fn gen_expr_write(expr: &Expr, mode: EmitMode) -> TokenStream {
    match expr {
        Expr::If(_) | Expr::Match(_) | Expr::ForLoop(_) | Expr::While(_) | Expr::Loop(_) => {
            let mut v = Vec::new();
            gen_block_expr(expr, &mut v, mode);
            quote! { #(#v)* }
        }
        _ => emit_md_render(expr, mode),
    }
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
            {for item in &items {
            - {item}
            }}
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
    fn named_args_substitute_and_keep_the_shadow() {
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
            "render body must splice the named expression: {out}"
        );
    }

    #[test]
    fn unused_named_arg_is_an_error() {
        let err = expand_err(quote! { r###"
            # {title}
            "###, extra = 1 });
        assert!(err.contains("never used"), "wrong error: {err}");
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
