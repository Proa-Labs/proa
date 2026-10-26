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

mod source;

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
            TplNode::Expr(expr) => {
                let index =
                    bare_ident(expr).and_then(|name| args.iter().position(|arg| arg.name == *name));
                if let Some(index) = index {
                    used[index] = true;
                    expr.expr = args[index].value.clone();
                    expr.location = ExprLocation::Rust(args[index].value.span());
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

pub fn parse_tpl(input: &str) -> Result<Vec<TplNode>, String> {
    parse_tpl_at(input, 0)
}

fn parse_tpl_at(input: &str, base: usize) -> Result<Vec<TplNode>, String> {
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
            let content_start = pos;
            let (content, end) = extract_braces(input, pos)?;
            pos = end + 1;
            at_line_start = false;
            let trimmed = content.trim();
            let origin = base + content_start + subslice_offset(&content, trimmed);
            if trimmed.starts_with("for ") {
                nodes.push(parse_for(trimmed, origin)?);
            } else if trimmed.starts_with("if ") || trimmed.starts_with("if let ") {
                nodes.push(parse_if(trimmed, origin)?);
            } else {
                let expr = syn::parse_str::<Expr>(trimmed)
                    .map_err(|e| format!("bad expression `{trimmed}`: {e}"))?;
                nodes.push(TplNode::Expr(TemplateExpr {
                    expr,
                    location: ExprLocation::Decoded(origin..origin + trimmed.len()),
                }));
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

fn subslice_offset(parent: &str, child: &str) -> usize {
    child.as_ptr() as usize - parent.as_ptr() as usize
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

fn split_body(text: &str) -> Result<(&str, &str), String> {
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
                    return Ok((body, &text[pos + clen..]));
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

fn parse_for(content: &str, base: usize) -> Result<TplNode, String> {
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
    let body = parse_tpl_at(body_str, base + subslice_offset(content, body_str))?;
    let iter_start = base + subslice_offset(content, iter_s);
    Ok(TplNode::For {
        pat: Box::new(pat),
        iter: Box::new(TemplateExpr {
            expr: iter_expr,
            location: ExprLocation::Decoded(iter_start..iter_start + iter_s.len()),
        }),
        body,
    })
}

fn parse_if(content: &str, base: usize) -> Result<TplNode, String> {
    let mut branches = Vec::new();
    let mut rem = content;
    rem = rem.strip_prefix("if ").unwrap();
    let (cond, body, rest) = parse_if_branch(rem, base + subslice_offset(content, rem))?;
    branches.push(IfBranch {
        cond: Some(cond),
        body,
    });
    rem = rest;
    loop {
        let t = rem.trim_start();
        if t.starts_with("else if ") {
            rem = t.strip_prefix("else if ").unwrap();
            let (cond, body, rest) = parse_if_branch(rem, base + subslice_offset(content, rem))?;
            branches.push(IfBranch {
                cond: Some(cond),
                body,
            });
            rem = rest;
        } else if t.starts_with("else") {
            let after = t.strip_prefix("else").unwrap().trim_start();
            let (body_str, _) = split_body(after)?;
            let body = parse_tpl_at(body_str, base + subslice_offset(content, body_str))?;
            branches.push(IfBranch { cond: None, body });
            break;
        } else {
            break;
        }
    }
    Ok(TplNode::If { branches })
}

fn parse_if_branch(text: &str, base: usize) -> Result<(TemplateExpr, Vec<TplNode>, &str), String> {
    let brace = find_body_brace(text).ok_or("expected `{` in if body")?;
    let cond_s = text[..brace].trim();
    let cond =
        syn::parse_str::<Expr>(cond_s).map_err(|e| format!("bad if condition `{cond_s}`: {e}"))?;
    let (body_str, rest) = split_body(&text[brace..])?;
    let body = parse_tpl_at(body_str, base + subslice_offset(text, body_str))?;
    let start = base + subslice_offset(text, cond_s);
    Ok((
        TemplateExpr {
            expr: cond,
            location: ExprLocation::Decoded(start..start + cond_s.len()),
        },
        body,
        rest,
    ))
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
