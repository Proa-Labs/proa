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

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{braced, Expr, LitStr};

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
                cx.write_prepared_static(BYTES)?;
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
                |cx: &mut ::proa_core::md::MdContext<'_, _>| {
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
            |cx: &mut ::proa_core::md::MdContext<'_, _>| {
                static BYTES: &[u8] = #bytes;
                cx.write_prepared_static(BYTES)?;
                Ok(())
            }
        )
    }
}

// ═══════════════════════════════════════════════════════════════
//  Raw Markdown template mode: md_sync! { r###"# {title}\n\nBody"### }
// ═══════════════════════════════════════════════════════════════

enum TplNode {
    Text(String),
    Expr(Expr),
    For {
        pat: Box<syn::Pat>,
        iter: Box<Expr>,
        body: Vec<TplNode>,
    },
    If {
        branches: Vec<IfBranch>,
    },
}

struct IfBranch {
    cond: Option<Expr>, // None = else
    body: Vec<TplNode>,
}

// -- public entry --

/// One `name = expr` argument following the template literal, mirroring
/// `format!`'s named arguments.
pub struct MdTemplateArg {
    pub name: syn::Ident,
    pub value: Expr,
}

/// Template-mode macro input: the Markdown literal plus optional named
/// arguments.
///
/// Named arguments exist for IDE ergonomics: an expression written as macro
/// tokens (`min = stats.min`) keeps its real source spans, so
/// go-to-definition, rename, and type diagnostics work on it — none of which
/// is possible for an expression parsed back out of the string's content.
pub struct MdTemplateInput {
    pub lit: LitStr,
    pub args: Vec<MdTemplateArg>,
}

impl Parse for MdTemplateInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        let mut args = Vec::new();
        while !input.is_empty() {
            input.parse::<syn::Token![,]>()?;
            if input.is_empty() {
                break; // trailing comma
            }
            let name: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            let value: Expr = input.parse()?;
            args.push(MdTemplateArg { name, value });
        }
        Ok(MdTemplateInput { lit, args })
    }
}

pub fn generate_template(tpl: &MdTemplateInput, mode: EmitMode) -> Result<TokenStream, syn::Error> {
    let lit = &tpl.lit;
    let value = lit.value();
    let input = if proa_dsl::md_layout::is_multiline_template(&value) {
        // Keep the shared formatter/compiler layout rule for templates whose
        // opening quote is followed by a newline, whether cooked or raw.
        proa_dsl::md_layout::dedent_template(&value)
    } else if is_multiline_raw_literal(lit) {
        // Raw templates may also begin with content on their first line. In
        // that form, preserve the first line and remove source indentation
        // from each following line.
        dedent_multiline_literal(value)
    } else {
        value
    };
    let mut nodes = parse_tpl(&input).map_err(|msg| syn::Error::new(lit.span(), msg))?;
    // Computed before substitution: the shadow describes the placeholders as
    // written in the string, with named arguments passed through alongside.
    let shadow = format_args_shadow(lit, &nodes, &tpl.args);
    substitute_named_args(&mut nodes, &tpl.args)?;
    Ok(codegen_tpl(&nodes, shadow, mode))
}

/// Splice each `name = expr` argument into the placeholders that reference it.
///
/// A named argument takes precedence over a like-named local, matching
/// `format!`. The expression is spliced at every placeholder that uses it, so
/// a name referenced twice renders its expression twice.
fn substitute_named_args(nodes: &mut [TplNode], args: &[MdTemplateArg]) -> Result<(), syn::Error> {
    if args.is_empty() {
        return Ok(());
    }
    for (index, arg) in args.iter().enumerate() {
        if args[..index].iter().any(|prior| prior.name == arg.name) {
            return Err(syn::Error::new(
                arg.name.span(),
                format!("duplicate named argument `{}`", arg.name),
            ));
        }
    }
    let mut used = vec![false; args.len()];
    substitute_nodes(nodes, args, &mut used);
    for (arg, used) in args.iter().zip(&used) {
        if !used {
            return Err(syn::Error::new(
                arg.name.span(),
                format!(
                    "named argument `{}` is never used by the template",
                    arg.name
                ),
            ));
        }
    }
    Ok(())
}

fn substitute_nodes(nodes: &mut [TplNode], args: &[MdTemplateArg], used: &mut [bool]) {
    for node in nodes {
        match node {
            TplNode::Expr(expr) => {
                let index =
                    bare_ident(expr).and_then(|name| args.iter().position(|arg| arg.name == *name));
                if let Some(index) = index {
                    used[index] = true;
                    *expr = args[index].value.clone();
                }
            }
            TplNode::For { body, .. } => substitute_nodes(body, args, used),
            TplNode::If { branches } => {
                for branch in branches {
                    substitute_nodes(&mut branch.body, args, used);
                }
            }
            TplNode::Text(_) => {}
        }
    }
}

fn bare_ident(expr: &Expr) -> Option<&syn::Ident> {
    let Expr::Path(path) = expr else { return None };
    if path.attrs.is_empty() && path.qself.is_none() {
        path.path.get_ident()
    } else {
        None
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

/// Return the end of a fenced Markdown code block that starts at `line_start`.
///
/// CommonMark permits up to three leading spaces, then at least three backticks
/// or tildes. An unclosed fence consumes the rest of the document. Treating the
/// complete block as static Markdown prevents Rust examples such as
/// `Thing { field }` from being mistaken for template interpolation.
fn fenced_code_end(input: &str, line_start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut marker_start = line_start;
    while marker_start < bytes.len() && marker_start - line_start < 3 && bytes[marker_start] == b' '
    {
        marker_start += 1;
    }

    let marker = *bytes.get(marker_start)?;
    if marker != b'`' && marker != b'~' {
        return None;
    }

    let mut marker_end = marker_start;
    while bytes.get(marker_end) == Some(&marker) {
        marker_end += 1;
    }
    let marker_len = marker_end - marker_start;
    if marker_len < 3 {
        return None;
    }

    let opening_line_end = input[marker_end..]
        .find('\n')
        .map_or(bytes.len(), |offset| marker_end + offset);
    if marker == b'`' && bytes[marker_end..opening_line_end].contains(&b'`') {
        return None;
    }
    if opening_line_end == bytes.len() {
        return Some(bytes.len());
    }

    let mut candidate_start = opening_line_end + 1;
    while candidate_start < bytes.len() {
        let candidate_line_end = input[candidate_start..]
            .find('\n')
            .map_or(bytes.len(), |offset| candidate_start + offset);
        let mut candidate_marker_start = candidate_start;
        while candidate_marker_start < candidate_line_end
            && candidate_marker_start - candidate_start < 3
            && bytes[candidate_marker_start] == b' '
        {
            candidate_marker_start += 1;
        }

        let mut candidate_marker_end = candidate_marker_start;
        while candidate_marker_end < candidate_line_end && bytes[candidate_marker_end] == marker {
            candidate_marker_end += 1;
        }
        let trailing = &input[candidate_marker_end..candidate_line_end];
        if candidate_marker_end - candidate_marker_start >= marker_len
            && trailing
                .chars()
                .all(|character| matches!(character, ' ' | '\t' | '\r'))
        {
            return Some(if candidate_line_end < bytes.len() {
                candidate_line_end + 1
            } else {
                candidate_line_end
            });
        }

        if candidate_line_end == bytes.len() {
            break;
        }
        candidate_start = candidate_line_end + 1;
    }

    Some(bytes.len())
}

/// Return the end of an inline Markdown code span beginning at `start`.
/// Delimiter runs must be the same length, matching CommonMark's code-span
/// rules. The returned range includes both delimiter runs.
fn inline_code_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut opener_end = start;
    while bytes.get(opener_end) == Some(&b'`') {
        opener_end += 1;
    }
    let delimiter_len = opener_end - start;
    let mut candidate = opener_end;

    while candidate < bytes.len() {
        if bytes[candidate] != b'`' {
            candidate += input[candidate..].chars().next()?.len_utf8();
            continue;
        }

        let mut candidate_end = candidate;
        while bytes.get(candidate_end) == Some(&b'`') {
            candidate_end += 1;
        }
        if candidate_end - candidate == delimiter_len {
            return Some(candidate_end);
        }
        candidate = candidate_end;
    }

    None
}

fn parse_tpl(input: &str) -> Result<Vec<TplNode>, String> {
    let mut nodes = Vec::new();
    let mut pos = 0;
    let bytes = input.as_bytes();
    let mut text = String::new();
    let mut at_line_start = true;

    while pos < bytes.len() {
        if at_line_start {
            if let Some(end) = fenced_code_end(input, pos) {
                text.push_str(&input[pos..end]);
                pos = end;
                at_line_start = true;
                continue;
            }
        }

        if bytes[pos] == b'`' {
            if let Some(end) = inline_code_end(input, pos) {
                text.push_str(&input[pos..end]);
                at_line_start = input[..end].ends_with('\n');
                pos = end;
                continue;
            }
        }

        if bytes[pos] == b'{' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
                text.push('{');
                pos += 2;
                at_line_start = false;
                continue;
            }
            if !text.is_empty() {
                nodes.push(TplNode::Text(std::mem::take(&mut text)));
            }
            pos += 1;
            let (content, end) = extract_braces(input, pos)?;
            pos = end + 1;
            at_line_start = false;
            let trimmed = content.trim();
            if trimmed.starts_with("for ") {
                nodes.push(parse_for(trimmed)?);
            } else if trimmed.starts_with("if ") || trimmed.starts_with("if let ") {
                nodes.push(parse_if(trimmed)?);
            } else {
                let expr = syn::parse_str::<Expr>(trimmed)
                    .map_err(|e| format!("bad expression `{trimmed}`: {e}"))?;
                nodes.push(TplNode::Expr(expr));
            }
        } else if bytes[pos] == b'}' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'}' {
                text.push('}');
                pos += 2;
                at_line_start = false;
                continue;
            }
            return Err(format!("unexpected `}}` at byte {pos}"));
        } else {
            let ch = input[pos..].chars().next().unwrap();
            text.push(ch);
            pos += ch.len_utf8();
            at_line_start = ch == '\n';
        }
    }
    if !text.is_empty() {
        nodes.push(TplNode::Text(text));
    }
    Ok(nodes)
}

fn extract_braces(src: &str, start: usize) -> Result<(String, usize), String> {
    let mut depth: i32 = 1;
    let mut in_str = false;
    let mut esc = false;
    let mut pos = start;
    while pos < src.len() {
        let ch = src[pos..].chars().next().unwrap();
        let clen = ch.len_utf8();
        if esc {
            esc = false;
            pos += clen;
            continue;
        }
        if in_str {
            match ch {
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            pos += clen;
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                pos += clen;
            }
            '{' => {
                depth += 1;
                pos += clen;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((src[start..pos].to_string(), pos));
                }
                pos += clen;
            }
            _ => {
                pos += clen;
            }
        }
    }
    Err("unclosed `{`".into())
}

fn find_body_brace(text: &str) -> Option<usize> {
    let mut pdepth: i32 = 0;
    let mut bdepth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    for (i, ch) in text.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            match ch {
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '(' => pdepth += 1,
            ')' => pdepth -= 1,
            '[' => bdepth += 1,
            ']' => bdepth -= 1,
            '{' if pdepth == 0 && bdepth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn split_body(text: &str) -> Result<(String, &str), String> {
    if !text.starts_with('{') {
        return Err("expected `{`".into());
    }
    let mut depth: i32 = 1;
    let mut in_str = false;
    let mut esc = false;
    let inner_start = 1;
    let mut pos = inner_start;
    while pos < text.len() {
        let ch = text[pos..].chars().next().unwrap();
        let clen = ch.len_utf8();
        if esc {
            esc = false;
            pos += clen;
            continue;
        }
        if in_str {
            match ch {
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            pos += clen;
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                pos += clen;
            }
            '{' => {
                depth += 1;
                pos += clen;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let mut body = &text[inner_start..pos];
                    if body.starts_with('\n') {
                        body = &body[1..];
                    }
                    return Ok((body.to_string(), &text[pos + clen..]));
                }
                pos += clen;
            }
            _ => {
                pos += clen;
            }
        }
    }
    Err("unclosed `{` in body".into())
}

fn parse_for(content: &str) -> Result<TplNode, String> {
    let rest = content.strip_prefix("for ").unwrap();
    let brace = find_body_brace(rest).ok_or("expected `{` in for body")?;
    let before = &rest[..brace];
    let in_pos = before.find(" in ").ok_or("expected `in` in for loop")?;
    let pat_s = before[..in_pos].trim();
    let iter_s = before[in_pos + 4..].trim();
    let pat: syn::Pat = {
        let ts: proc_macro2::TokenStream = pat_s
            .parse()
            .map_err(|e| format!("bad for pattern `{pat_s}`: {e}"))?;
        let parser = |input: syn::parse::ParseStream| syn::Pat::parse_single(input);
        syn::parse::Parser::parse2(parser, ts)
            .map_err(|e| format!("bad for pattern `{pat_s}`: {e}"))?
    };
    let iter_expr =
        syn::parse_str::<Expr>(iter_s).map_err(|e| format!("bad for iterator `{iter_s}`: {e}"))?;
    let (body_str, _) = split_body(&rest[brace..])?;
    let body = parse_tpl(&body_str)?;
    Ok(TplNode::For {
        pat: Box::new(pat),
        iter: Box::new(iter_expr),
        body,
    })
}

fn parse_if(content: &str) -> Result<TplNode, String> {
    let mut branches = Vec::new();
    let mut rem = content;
    rem = rem.strip_prefix("if ").unwrap();
    let (cond, body, rest) = parse_if_branch(rem)?;
    branches.push(IfBranch {
        cond: Some(cond),
        body,
    });
    rem = rest;
    loop {
        let t = rem.trim_start();
        if t.starts_with("else if ") {
            rem = t.strip_prefix("else if ").unwrap();
            let (cond, body, rest) = parse_if_branch(rem)?;
            branches.push(IfBranch {
                cond: Some(cond),
                body,
            });
            rem = rest;
        } else if t.starts_with("else") {
            let after = t.strip_prefix("else").unwrap().trim_start();
            let (body_str, _) = split_body(after)?;
            let body = parse_tpl(&body_str)?;
            branches.push(IfBranch { cond: None, body });
            break;
        } else {
            break;
        }
    }
    Ok(TplNode::If { branches })
}

fn parse_if_branch(text: &str) -> Result<(Expr, Vec<TplNode>, &str), String> {
    let brace = find_body_brace(text).ok_or("expected `{` in if body")?;
    let cond_s = text[..brace].trim();
    let cond =
        syn::parse_str::<Expr>(cond_s).map_err(|e| format!("bad if condition `{cond_s}`: {e}"))?;
    let (body_str, rest) = split_body(&text[brace..])?;
    let body = parse_tpl(&body_str)?;
    Ok((cond, body, rest))
}

// ═══════════════════════════════════════════════════════════════
//  Token composition mode: source lines are implicit Markdown block boundaries
// ═══════════════════════════════════════════════════════════════

pub struct MdRoot {
    nodes: Vec<MdNode>,
}

struct MdNode {
    kind: MdNodeKind,
    starts_new_line: bool,
}

enum MdNodeKind {
    Text { value: LitStr, dedent: bool },
    Block(Box<Expr>),
}

#[derive(Clone, Copy)]
struct SourceLines {
    start: usize,
    end: usize,
}

fn source_lines(start: Span, end: Span) -> Option<SourceLines> {
    let start = start.start().line;
    let end = end.end().line;
    // Real invocation spans carry stable line locations on the workspace MSRV.
    // Synthetic spans report zero; keep their historical token-only behavior
    // rather than inventing boundaries that the caller did not author.
    if start == 0 || end == 0 {
        None
    } else {
        Some(SourceLines { start, end })
    }
}

fn dedent_multiline_literal(value: String) -> String {
    let common_indent = value
        .split('\n')
        .skip(1)
        .filter(|line| {
            !line
                .trim_matches(|character| matches!(character, ' ' | '\t' | '\r'))
                .is_empty()
        })
        .map(|line| {
            line.as_bytes()
                .iter()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count()
        })
        .min()
        .unwrap_or(0);

    if common_indent == 0 {
        return value;
    }

    let mut output = String::with_capacity(value.len());
    let mut lines = value.split('\n');
    output.push_str(lines.next().unwrap_or_default());
    for line in lines {
        output.push('\n');
        if line
            .trim_matches(|character| matches!(character, ' ' | '\t' | '\r'))
            .is_empty()
        {
            if line.ends_with('\r') {
                output.push('\r');
            }
        } else {
            output.push_str(&line[common_indent..]);
        }
    }
    output
}

fn text_value(value: &LitStr, dedent: bool) -> String {
    let value = value.value();
    if dedent {
        dedent_multiline_literal(value)
    } else {
        value
    }
}

fn is_multiline_raw_literal(value: &LitStr) -> bool {
    let is_raw = value
        .span()
        .source_text()
        .is_some_and(|source| source.trim_start().starts_with('r'));
    is_raw && value.value().contains(['\n', '\r'])
}

impl Parse for MdRoot {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut nodes = Vec::new();
        let mut previous_end_line = None;
        while !input.is_empty() {
            let (kind, lines) = if input.peek(LitStr) {
                let value: LitStr = input.parse()?;
                let lines = source_lines(value.span(), value.span());
                let dedent = is_multiline_raw_literal(&value);
                (MdNodeKind::Text { value, dedent }, lines)
            } else if input.peek(syn::token::Brace) {
                let content;
                let braces = braced!(content in input);
                let lines = source_lines(braces.span.open(), braces.span.close());
                (MdNodeKind::Block(Box::new(content.parse()?)), lines)
            } else {
                return Err(input.error("expected string literal or {expression}"));
            };

            let starts_new_line = match (previous_end_line, lines) {
                (Some(previous_end), Some(current)) => current.start > previous_end,
                _ => false,
            };
            previous_end_line = lines.map(|lines| lines.end);
            nodes.push(MdNode {
                kind,
                starts_new_line,
            });
        }
        Ok(MdRoot { nodes })
    }
}

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
            stmts.push(quote! { cx.block_boundary()?; });
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
