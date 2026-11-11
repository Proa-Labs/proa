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
//! blocks cannot render inside inline scopes.
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
//! # Composition without authored Markdown
//!
//! The macro body is always one string template. To render values one after
//! another without writing Markdown around them, compose them in Rust:
//!
//! ```rust,ignore
//! (overview, impact, activity).render_md(cx)
//! ```
//!
//! Token composition such as `md_sync! { "# " {title} }` is rejected with a
//! pointer to the string form. The parser for it stays because the editor
//! projection presents templates to rust-analyzer in that shape; see
//! [`expand`].

use proa_dsl::md::{bare_ident, text_value, MdNodeKind, MdTemplateArg, TplNode};
pub use proa_dsl::md::{MdRoot, MdTemplateInput};
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Expr, LitStr};

#[derive(Clone, Copy)]
pub enum EmitMode {
    Sync,
    Async,
}

impl EmitMode {
    /// The macro an author wrote, for diagnostics.
    pub fn macro_name(self) -> &'static str {
        match self {
            EmitMode::Sync => "md_sync",
            EmitMode::Async => "md",
        }
    }
}

/// Expand a macro body.
///
/// The body is one string template; [`proa_dsl::md::template_shape_error`]
/// explains anything else. Token composition such as `{a} "x" {b}` is still
/// parsed, because the editor projection presents a template to rust-analyzer
/// in exactly that shape: the Markdown is blanked and every `{slot}` keeps its
/// authored position. Under `cfg(rust_analyzer)` that shape expands so the IDE
/// keeps slot diagnostics and the template's result type; a real build sees
/// the rejection as a `compile_error!` at the offending token instead.
pub fn expand(input: TokenStream, mode: EmitMode) -> syn::Result<TokenStream> {
    let rejection = match syn::parse2::<MdTemplateInput>(input.clone()) {
        Ok(template) => return generate_template(&template, mode),
        Err(parse_error) => {
            proa_dsl::md::template_shape_error(mode.macro_name(), &input).unwrap_or(parse_error)
        }
    };
    let Ok(root) = syn::parse2::<MdRoot>(input) else {
        return Err(rejection);
    };
    let message = rejection.to_string();
    let error = quote_spanned! {rejection.span()=> ::core::compile_error!(#message); };
    let expansion = generate_tokens(&root, mode);
    // Crates outside this workspace do not register `cfg(rust_analyzer)` with
    // check-cfg, so the gate carries its own allow, as the IDE shadow does.
    Ok(quote! {{
        #[allow(unexpected_cfgs)]
        {
            #[cfg(not(rust_analyzer))]
            #error
        }
        #expansion
    }})
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
        let text = proc_macro2::Literal::string(run);
        stmts.push(quote! {
            {
                static TEXT: &str = #text;
                #cx.write_md_prepared_static_str(TEXT)?;
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
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let cx = render_context();
    match mode {
        EmitMode::Sync => quote! {
            #proa_core::OptimizedMd::new(
                |#cx: &mut #proa_core::WebContext<_, _>| {
                    use #proa_core::MdRenderSyncExt as _;
                    #(#stmts)*
                    ::core::result::Result::Ok(())
                }
            )
        },
        EmitMode::Async => quote! {
            #proa_core::md::async_md_from_closure(async move |#cx| {
                use #proa_core::MdRenderAsyncExt as _;
                #(#stmts)*
                ::core::result::Result::Ok(())
            })
        },
    }
}

fn static_fragment(text: proc_macro2::Literal) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let cx = render_context();
    quote! {
        #proa_core::OptimizedMd::new(
            |#cx: &mut #proa_core::WebContext<_, _>| {
                static TEXT: &str = #text;
                #cx.write_md_prepared_static_str(TEXT)?;
                ::core::result::Result::Ok(())
            }
        )
    }
}

// ═══════════════════════════════════════════════════════════════
//  Raw Markdown template mode: md_sync! { r###"# {title}\n\nBody"### }
// ═══════════════════════════════════════════════════════════════

pub fn generate_template(tpl: &MdTemplateInput, mode: EmitMode) -> Result<TokenStream, syn::Error> {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let lit = &tpl.lit;
    let nodes = tpl.parse_nodes()?;
    let block = proa_dsl::md::is_multiline_raw_literal(lit);
    // An inline template is itself one inline scope, so every slot in it is
    // already restricted: a block child anywhere is rejected, and a per-slot
    // scope could only save and restore state that cannot change. Only block
    // templates classify their slots.
    let inline_slots = if block {
        proa_dsl::md::inline_slot_contexts(&nodes)
    } else {
        let slot_count = nodes
            .iter()
            .filter(|node| matches!(node, TplNode::Expr(_)))
            .count();
        vec![false; slot_count]
    };
    let mut slots = inline_slots.iter().copied();
    for node in &nodes {
        if let TplNode::Expr(expr) = node {
            let restricted = slots.next().expect("one context per expression");
            if !block || restricted {
                reject_direct_block_child(expr)?;
            }
        }
    }
    let shadow = format_args_shadow(lit, &nodes, &tpl.args);
    let body = codegen_tpl(
        &nodes,
        &inline_slots,
        shadow,
        tpl.argument_binding(),
        mode,
        block,
    );
    let template_mode = if block {
        quote! { #proa_core::md::MD_TEMPLATE_BLOCK }
    } else {
        quote! { #proa_core::md::MD_TEMPLATE_INLINE }
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
            // Match the macro under its own name: the usual
            // `use proa_macros::md_sync;`, qualified paths, and re-exports.
            // A renamed import cannot be recognized and keeps the runtime
            // check, as do children behind helpers. A user macro sharing the
            // name is claimed only when its input is a single multiline raw
            // literal, and that false positive is a compile error at the
            // nested call, not a miscompile.
            if child
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
    block: bool,
) -> TokenStream {
    // all-static fast path
    if binding.is_none() && nodes.iter().all(|n| matches!(n, TplNode::Text(_))) {
        let mut s = String::new();
        for n in nodes {
            if let TplNode::Text(t) = n {
                s.push_str(t);
            }
        }
        return static_fragment(proc_macro2::Literal::string(&s));
    }

    let mut stmts = Vec::new();
    // First so its borrows begin and end before any rendering statement.
    stmts.extend(shadow);
    stmts.extend(binding.map(|binding| quote! { #binding }));
    let mut run = String::new();
    emit_tpl_nodes(nodes, inline_slots, &mut stmts, &mut run, mode, block);
    flush(&mut stmts, &mut run);
    wrap_render(&stmts, mode)
}

fn emit_tpl_nodes(
    nodes: &[TplNode],
    inline_slots: &[bool],
    stmts: &mut Vec<TokenStream>,
    run: &mut String,
    mode: EmitMode,
    block: bool,
) {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let mut slots = inline_slots.iter().copied();
    // In a block template, the line breaks that follow a block-position slot
    // are layout rather than prose: a block child has already completed a
    // blank line through its own epilogue, and a row template already ends
    // in its line break. Emit them as boundaries, which write the authored
    // break after inline text and nothing after a child that ended a line.
    // Any further authored blank lines stay literal.
    let mut after_block_slot = false;
    for node in nodes {
        match node {
            TplNode::Text(t) => {
                let mut text = t.as_str();
                if after_block_slot {
                    let breaks = text.bytes().take_while(|byte| *byte == b'\n').count();
                    if breaks > 0 {
                        let cx = render_context();
                        flush(stmts, run);
                        if breaks == 1 {
                            stmts.push(quote! { #cx.md_line_boundary()?; });
                        } else {
                            stmts.push(quote! { #cx.md_block_boundary()?; });
                        }
                        text = &text[breaks.min(2)..];
                    }
                }
                run.push_str(text);
                after_block_slot = false;
            }
            // Control flow and plain values share the token-mode lowering, so
            // both template forms render a branch's final expression the same
            // way.
            TplNode::Expr(expr) => {
                flush(stmts, run);
                let restricted = slots.next().expect("one context per expression");
                after_block_slot = block && !restricted;
                if restricted {
                    let cx = render_context();
                    let scope =
                        syn::Ident::new("__proa_md_inline_scope", proc_macro2::Span::mixed_site());
                    let mut child = Vec::new();
                    gen_block_expr(&expr.expr, &mut child, mode);
                    stmts.push(quote! {
                        {
                            let mut #scope = #proa_core::md::__MdTemplateScope::<_, _, { #proa_core::md::MD_TEMPLATE_INLINE }>::enter(#cx)?;
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
//  Token composition: the editor's view of a template, expanded only for
//  rust-analyzer (see `expand`). Source lines are implicit block boundaries.
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
        return static_fragment(proc_macro2::Literal::string(&text));
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

// Templates share the same Rust control-flow lowering as HTML.
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

    fn expand_body(input: proc_macro2::TokenStream, mode: EmitMode) -> Result<String, String> {
        super::expand(input, mode)
            .map(|tokens| tokens.to_string())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn non_template_bodies_fail_the_build_but_still_expand_for_rust_analyzer() {
        let out = expand_body(quote::quote! { {overview}{impact} }, EmitMode::Sync).unwrap();
        assert!(out.contains("compile_error !"), "{out}");
        assert!(out.contains("cfg (not (rust_analyzer))"), "{out}");
        assert!(out.contains("`md_sync!` takes a string template"), "{out}");
        assert!(out.contains("use a tuple"), "{out}");
        assert!(out.contains("OptimizedMd"), "{out}");

        let out = expand_body(quote::quote! { "**" {child} "**" }, EmitMode::Sync).unwrap();
        assert!(
            out.contains("unexpected tokens after the template string"),
            "{out}"
        );

        let out = expand_body(quote::quote! {}, EmitMode::Sync).unwrap();
        assert!(out.contains("to render nothing"), "{out}");

        let out = expand_body(quote::quote! { {section} }, EmitMode::Async).unwrap();
        assert!(out.contains("`md!` takes a string template"), "{out}");
        assert!(out.contains("render each value in turn"), "{out}");

        // Bodies that are not even token composition surface the rejection
        // directly, and a template with a malformed named argument keeps the
        // ordinary parse error.
        let err = expand_body(quote::quote! { foo bar }, EmitMode::Sync).unwrap_err();
        assert!(err.contains("takes a string template"), "{err}");
        let err = expand_body(quote::quote! { "a", b }, EmitMode::Sync).unwrap_err();
        assert!(err.contains("expected `=`"), "{err}");

        // String templates never carry the rejection.
        let out = expand_body(quote::quote! { "**{child}**" }, EmitMode::Sync).unwrap();
        assert!(!out.contains("compile_error"), "{out}");
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
            {for item in &items { md_sync! { "- {item}\n" } }}
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
    fn nested_block_literals_are_rejected_however_the_macro_is_named() {
        for path in [
            "md_sync",
            "md",
            "proa_macros::md_sync",
            "::proa_macros::md",
            "crate::prelude::md_sync",
        ] {
            let value = format!("**{{{path}! {{ r#\"\n## Heading\n\"# }}}}**");
            let lit = proc_macro2::Literal::string(&value);
            let err = expand_err(quote! { #lit });
            assert!(err.contains("inside an inline scope"), "{path}: {err}");
        }
        // Nested inline literals and block literals in block slots stay legal.
        expand(quote! { r#"**{md_sync! { "inline" }}**"# });
        expand(quote! { r###"
            {md_sync! { r#"
                ## Heading
                "# }}
            "### });
    }

    #[test]
    fn inline_templates_do_not_open_per_slot_scopes() {
        let out = expand(quote! { "**{value}** [{label}](/target)" });
        assert!(!out.contains("__MdTemplateScope"), "redundant scope: {out}");
        let out = expand(quote! { r###"
            **{value}**
            "### });
        assert!(out.contains("__MdTemplateScope"), "missing scope: {out}");
    }

    #[test]
    fn all_static_template_emits_no_shadow() {
        let out = expand(quote! { r###"
            # Static heading
            "### });
        assert!(!out.contains("format_args"), "unexpected shadow: {out}");
    }
}
