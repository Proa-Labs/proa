// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared semantic parser for raw and token-composed Markdown templates.
//!
//! Compiler code generation and source tooling consume these same nodes.
//! Raw expressions retain their authored literal ranges; token expressions
//! retain their ordinary Rust spans.

use proc_macro2::Span;
use quote::{ToTokens, TokenStreamExt};
use std::ops::{Deref, Range};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{braced, Expr, LitStr};

mod control;
mod source;
mod syntax;

pub use control::format_control_flow;

/// Location of an expression in authored source, before template dedenting.
#[derive(Clone, Debug)]
pub enum ExprLocation {
    /// Byte range in an unencoded template passed directly to `parse_tpl`.
    Decoded(Range<usize>),
    /// Byte range in the complete Rust literal token, including its delimiters.
    Literal { span: Span, range: Range<usize> },
    /// Ordinary Rust tokens, including substituted named arguments.
    Rust(Span),
}

/// A Rust expression together with its location in the authored template.
#[derive(Clone, Debug)]
pub struct TemplateExpr {
    pub expr: Expr,
    pub location: ExprLocation,
}

impl Deref for TemplateExpr {
    type Target = Expr;
    fn deref(&self) -> &Expr {
        &self.expr
    }
}

impl ToTokens for TemplateExpr {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        tokens.append_all(self.expr.to_token_stream());
    }
}

#[derive(Clone, Debug)]
pub enum TplNode {
    Text(String),
    Expr(TemplateExpr),
    For {
        pat: Box<syn::Pat>,
        iter: Box<TemplateExpr>,
        body: Vec<TplNode>,
    },
    If {
        branches: Vec<IfBranch>,
    },
}

#[derive(Clone, Debug)]
pub struct IfBranch {
    pub cond: Option<TemplateExpr>, // None = else
    pub body: Vec<TplNode>,
}

// -- public entry --

/// One `name = expr` argument following the template literal, mirroring
/// `format!`'s named arguments.
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub struct MdTemplateInput {
    pub lit: LitStr,
    pub args: Vec<MdTemplateArg>,
}

impl MdTemplateInput {
    /// Parse with the same layout and grammar used by the Markdown macros.
    /// Named arguments are deliberately substituted separately, so codegen
    /// can preserve its IDE shadow before replacing placeholders.
    pub fn parse_nodes(&self) -> syn::Result<Vec<TplNode>> {
        let source = source::MappedLiteral::new(&self.lit);
        let mut nodes =
            parse_tpl(&source.text).map_err(|message| syn::Error::new(self.lit.span(), message))?;
        source.map_nodes(&mut nodes, self.lit.span());
        Ok(nodes)
    }

    /// Parse and substitute named arguments for semantic consumers.
    pub fn semantic_nodes(&self) -> syn::Result<Vec<TplNode>> {
        let mut nodes = self.parse_nodes()?;
        substitute_named_args(&mut nodes, &self.args)?;
        Ok(nodes)
    }
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

pub fn substitute_named_args(
    nodes: &mut [TplNode],
    args: &[MdTemplateArg],
) -> Result<(), syn::Error> {
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
            TplNode::Expr(expr) => substitute_expr(expr, args, used),
            TplNode::For { iter, body, .. } => {
                substitute_expr(iter, args, used);
                substitute_nodes(body, args, used);
            }
            TplNode::If { branches } => {
                for branch in branches {
                    if let Some(condition) = &mut branch.cond {
                        substitute_expr(condition, args, used);
                    }
                    substitute_nodes(&mut branch.body, args, used);
                }
            }
            TplNode::Text(_) => {}
        }
    }
}

fn substitute_expr(expr: &mut TemplateExpr, args: &[MdTemplateArg], used: &mut [bool]) {
    if let Some(index) =
        bare_ident(expr).and_then(|name| args.iter().position(|arg| arg.name == *name))
    {
        used[index] = true;
        expr.expr = args[index].value.clone();
        expr.location = ExprLocation::Rust(args[index].value.span());
    }
}

pub fn bare_ident(expr: &Expr) -> Option<&syn::Ident> {
    let Expr::Path(path) = expr else { return None };
    if path.attrs.is_empty() && path.qself.is_none() {
        path.path.get_ident()
    } else {
        None
    }
}

/// Return the end of a fenced Markdown code block that starts at `line_start`.
///
/// CommonMark permits up to three leading spaces, then at least three backticks
/// or tildes. An unclosed fence consumes the rest of the document. Treating the
/// complete block as static Markdown prevents Rust examples such as
/// `Thing { field }` from being mistaken for template interpolation.
fn fenced_code_end(input: &str, line_start: usize, layout: &str) -> Option<usize> {
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
        let content_start = candidate_start + layout_prefix_len(layout, &input[candidate_start..]);
        let mut candidate_marker_start = content_start;
        while candidate_marker_start < candidate_line_end
            && candidate_marker_start - content_start < 3
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

pub fn parse_tpl(input: &str) -> Result<Vec<TplNode>, String> {
    control::parse(input)
}

/// Number of source-layout bytes shared with a line (also safe for blank lines).
fn layout_prefix_len(layout: &str, line: &str) -> usize {
    layout
        .char_indices()
        .zip(line.chars())
        .take_while(|((_, a), b)| a == b)
        .map(|((index, ch), _)| index + ch.len_utf8())
        .last()
        .unwrap_or(0)
}

fn extract_braces(src: &str, start: usize) -> Result<(&str, usize), String> {
    let end = syntax::rust_brace_end(src, start)?;
    Ok((&src[start..end], end))
}

fn subslice_offset(parent: &str, child: &str) -> usize {
    child.as_ptr() as usize - parent.as_ptr() as usize
}

fn split_body(text: &str, closers: usize) -> Result<(&str, &str, bool), String> {
    if !text.starts_with('{') {
        return Err("expected `{`".into());
    }
    let end = syntax::markdown_body_end(text, 1, closers)?;
    let mut body = &text[1..end];
    let opening = body.trim_start_matches([' ', '\t', '\r']);
    let multiline = opening.starts_with('\n');
    if multiline {
        body = &opening[1..];
    }
    // A standalone closing delimiter contributes no trailing layout spaces.
    if let Some(last_newline) = body.rfind('\n') {
        if body[last_newline + 1..].trim().is_empty() {
            body = &body[..last_newline + 1];
        }
    }
    Ok((body, &text[end + 1..], multiline))
}

#[derive(Clone, Debug)]
pub struct MdRoot {
    pub nodes: Vec<MdNode>,
}

#[derive(Clone, Debug)]
pub struct MdNode {
    pub kind: MdNodeKind,
    pub starts_new_line: bool,
}

#[derive(Clone, Debug)]
pub enum MdNodeKind {
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

pub fn dedent_multiline_literal(value: String) -> String {
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

pub fn text_value(value: &LitStr, dedent: bool) -> String {
    let value = value.value();
    if dedent {
        dedent_multiline_literal(value)
    } else {
        value
    }
}

pub fn is_multiline_raw_literal(value: &LitStr) -> bool {
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
