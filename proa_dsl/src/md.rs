// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared semantic parser for raw and token-composed Markdown templates.
//!
//! The raw-template grammar has one lexical rule, identical at every depth:
//! text outside `{ ... }` is Markdown, and text inside is a Rust expression
//! parsed by `syn`. Control flow is therefore ordinary Rust (`if`, `for`,
//! `match`, ...) whose branches render their final expression, and Markdown
//! re-enters a branch only through a nested `md_sync!` / `md!` call. No word
//! is ever reserved in Markdown text, and a line means the same thing whether
//! it sits at the top level or inside a branch.
//!
//! Compiler code generation and source tooling consume these same nodes.
//! Raw expressions retain their authored literal ranges; token expressions
//! retain their ordinary Rust spans.

use proc_macro2::Span;
use quote::{ToTokens, TokenStreamExt};
use std::ops::{Deref, Range};
use std::sync::Arc;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{braced, Expr, LitStr};

mod source;
mod syntax;

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
    source: Option<SlotSource>,
}

/// Where a slot's parsed text sits, so sub-expression spans can be mapped back
/// to the authored literal by tooling.
#[derive(Clone, Debug)]
struct SlotSource {
    /// Span of the whole parsed slot; sub-spans from the same parse join with it.
    whole: Span,
    /// Decoded template offset of the parsed text's first byte.
    origin: usize,
    len: usize,
    literal: Option<Arc<source::LiteralMap>>,
}

impl TemplateExpr {
    /// An expression written as ordinary Rust tokens, such as a named argument
    /// or a custom grammar's output. Its span is already a source location.
    pub fn rust(expr: Expr, span: Span) -> Self {
        Self {
            expr,
            location: ExprLocation::Rust(span),
            source: None,
        }
    }

    /// Locate a sub-expression of this slot in the authored template.
    ///
    /// `span` must belong to the same parse as `self.expr`: a span of one of its
    /// sub-expressions, or of tokens inside a nested macro call within it. This
    /// is how tooling points a diagnostic at an `if` condition or a `for`
    /// iterator rather than at the whole slot. Returns `None` when the span is
    /// not part of this slot, or when spans carry no location at all (inside a
    /// running proc macro).
    pub fn locate(&self, span: Span) -> Option<ExprLocation> {
        let source = self.source.as_ref()?;
        source.whole.join(span)?;
        let relative = span.byte_range();
        if relative.start > relative.end || relative.end > source.len {
            return None;
        }
        let decoded = source.origin + relative.start..source.origin + relative.end;
        Some(match &source.literal {
            Some(literal) => literal.locate(decoded),
            None => ExprLocation::Decoded(decoded),
        })
    }

    fn attach(&mut self, literal: &Arc<source::LiteralMap>) {
        if let Some(source) = &mut self.source {
            source.literal = Some(Arc::clone(literal));
        }
    }
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

/// One node of a raw Markdown template.
///
/// Control flow is not a separate node kind: an `{if ...}` or `{for ...}` slot
/// is an [`Expr::If`] / [`Expr::ForLoop`] like any other slot expression, and
/// code generation lowers it exactly as the token-composition form does.
#[derive(Clone, Debug)]
pub enum TplNode {
    Text(String),
    Expr(TemplateExpr),
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

/// Replace bare-identifier placeholders with their named-argument values.
///
/// A placeholder is matched anywhere in a slot expression, including the
/// condition of an `{if ...}` or the iterator of a `{for ...}`, but never
/// inside the token stream of a nested macro call. Named arguments are an IDE
/// convenience for simple placeholders; a loop pattern that shadows an
/// argument name is not tracked.
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
    for node in nodes {
        if let TplNode::Expr(expr) = node {
            substitute_expr(expr, args, &mut used);
        }
    }
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

fn substitute_expr(expr: &mut TemplateExpr, args: &[MdTemplateArg], used: &mut [bool]) {
    if let Some(index) =
        bare_ident(expr).and_then(|name| args.iter().position(|arg| arg.name == *name))
    {
        // A whole-slot placeholder takes the argument's real span so IDE
        // features land on the macro tokens the author wrote.
        used[index] = true;
        expr.expr = args[index].value.clone();
        expr.location = ExprLocation::Rust(args[index].value.span());
        expr.source = None;
        return;
    }
    Substitute { args, used }.visit_expr_mut(&mut expr.expr);
}

struct Substitute<'a> {
    args: &'a [MdTemplateArg],
    used: &'a mut [bool],
}

impl VisitMut for Substitute<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        if let Some(index) =
            bare_ident(expr).and_then(|name| self.args.iter().position(|arg| arg.name == *name))
        {
            self.used[index] = true;
            *expr = self.args[index].value.clone();
            return;
        }
        syn::visit_mut::visit_expr_mut(self, expr);
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

const CONTROL_HINT: &str =
    "\n\nText inside `{ ... }` is Rust, including the bodies of `if`, `for`, and `match`. \
A branch renders its final expression, so write Markdown there as a nested template: \
`{if ready { md_sync! { \"**In stock**\" } } else { \"Backordered\" }}`. \
Plain Markdown is never parsed inside a slot.";

/// Parse a slot's Rust source, pointing authors who wrote Markdown inside a
/// control body at the nested-template form.
fn parse_slot(source: &str) -> Result<Expr, String> {
    syn::parse_str::<Expr>(source).map_err(|error| {
        let mut message = format!("bad expression `{source}`: {error}");
        let keyword = source.split(|c: char| !c.is_ascii_alphabetic()).next();
        if matches!(
            keyword,
            Some("if" | "for" | "match" | "while" | "loop" | "else")
        ) {
            message.push_str(CONTROL_HINT);
        }
        message
    })
}

/// Split a decoded template into Markdown text and Rust slot expressions.
///
/// Only four things are special in text: `{` opens a slot, `{{` and `}}` are
/// literal braces, and fenced code blocks or inline code spans are copied
/// verbatim. A slot runs to the brace that balances its opener under Rust
/// lexical rules, so nested blocks, strings, and nested template literals
/// never need escaping.
pub fn parse_tpl(input: &str) -> Result<Vec<TplNode>, String> {
    let bytes = input.as_bytes();
    let mut nodes = Vec::new();
    let mut text = String::new();
    let mut pos = 0;
    let mut at_line_start = true;

    while pos < bytes.len() {
        if at_line_start {
            if let Some(end) = fenced_code_end(input, pos) {
                text.push_str(&input[pos..end]);
                pos = end;
                continue;
            }
            at_line_start = false;
        }

        match bytes[pos] {
            b'`' => {
                if let Some(end) = inline_code_end(input, pos) {
                    text.push_str(&input[pos..end]);
                    at_line_start = input[..end].ends_with('\n');
                    pos = end;
                    continue;
                }
                text.push('`');
                pos += 1;
            }
            b'{' => {
                if bytes.get(pos + 1) == Some(&b'{') {
                    text.push('{');
                    pos += 2;
                    continue;
                }
                if !text.is_empty() {
                    nodes.push(TplNode::Text(std::mem::take(&mut text)));
                }
                let content_start = pos + 1;
                let end = syntax::rust_brace_end(input, content_start)?;
                let content = &input[content_start..end];
                let trimmed = content.trim();
                let origin =
                    content_start + (trimmed.as_ptr() as usize - content.as_ptr() as usize);
                let expr = parse_slot(trimmed)?;
                let whole = expr.span();
                nodes.push(TplNode::Expr(TemplateExpr {
                    expr,
                    location: ExprLocation::Decoded(origin..origin + trimmed.len()),
                    source: Some(SlotSource {
                        whole,
                        origin,
                        len: trimmed.len(),
                        literal: None,
                    }),
                }));
                pos = end + 1;
            }
            b'}' => {
                if bytes.get(pos + 1) == Some(&b'}') {
                    text.push('}');
                    pos += 2;
                    continue;
                }
                return Err(format!(
                    "unexpected `}}` at byte {pos}; write `}}}}` for a literal closing brace"
                ));
            }
            _ => {
                let ch = input[pos..].chars().next().unwrap();
                text.push(ch);
                pos += ch.len_utf8();
                at_line_start = ch == '\n';
            }
        }
    }
    if !text.is_empty() {
        nodes.push(TplNode::Text(text));
    }
    Ok(nodes)
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

/// Whether a literal was written as a multiline raw string.
///
/// The literal token keeps its authored spelling even when it reaches a macro
/// through another expansion (a nested `md_sync!` inside a raw template), so
/// the prefix is read from the token rather than from `source_text`.
pub fn is_multiline_raw_literal(value: &LitStr) -> bool {
    value.token().to_string().starts_with('r') && value.value().contains(['\n', '\r'])
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<String> {
        parse_tpl(input)
            .unwrap()
            .into_iter()
            .map(|node| match node {
                TplNode::Text(text) => format!("text{text:?}"),
                TplNode::Expr(expr) => format!("expr({})", expr.to_token_stream()),
            })
            .collect()
    }

    #[test]
    fn prose_is_never_control_flow_at_any_depth() {
        assert_eq!(
            kinds("if enabled {label}\n"),
            ["text\"if enabled \"", "expr(label)", "text\"\\n\""]
        );
        assert_eq!(
            kinds("for guests in rooms {count}\n"),
            ["text\"for guests in rooms \"", "expr(count)", "text\"\\n\""]
        );
    }

    #[test]
    fn control_flow_is_one_rust_slot() {
        let nodes = kinds("{if show {\n    md_sync! { \"- \" {label} \"\\n\" }\n}}\n");
        assert_eq!(nodes.len(), 2, "{nodes:?}");
        assert!(nodes[0].starts_with("expr(if show {"), "{nodes:?}");
        assert_eq!(nodes[1], "text\"\\n\"");
        let nested =
            parse_tpl("{for x in xs { md_sync! { r#\"\n    - {x} }} not a closer\n\"# } }}")
                .unwrap();
        assert!(matches!(&nested[0], TplNode::Expr(expr) if matches!(expr.expr, Expr::ForLoop(_))));
    }

    #[test]
    fn markdown_in_a_control_body_is_rejected_with_the_nested_template_hint() {
        let error = parse_tpl("{if ready { Available } else { **Sold out** }}").unwrap_err();
        assert!(error.starts_with("bad expression `if ready"), "{error}");
        assert!(error.contains("nested template"), "{error}");
        let plain = parse_tpl("{self.title ++}").unwrap_err();
        assert!(!plain.contains("nested template"), "{plain}");
    }

    #[test]
    fn escapes_and_code_stay_literal() {
        assert_eq!(kinds("Use {{expr}} here"), ["text\"Use {expr} here\""]);
        assert_eq!(
            kinds("`{literal}` and {x}"),
            ["text\"`{literal}` and \"", "expr(x)"]
        );
        assert_eq!(
            kinds("```rust\nThing { field }\n```\n{x}"),
            ["text\"```rust\\nThing { field }\\n```\\n\"", "expr(x)"]
        );
        assert!(parse_tpl("lonely }").unwrap_err().contains("`}}`"));
        assert!(parse_tpl("{open").unwrap_err().contains("`{{`"));
    }

    #[test]
    fn sub_expressions_locate_inside_their_slot() {
        let nodes =
            parse_tpl("ab {if self.stock > 0 { md_sync! { \"x\" } } else { other }} c").unwrap();
        let TplNode::Expr(slot) = &nodes[1] else {
            panic!("{nodes:?}");
        };
        let Expr::If(expr_if) = &slot.expr else {
            panic!("{slot:?}");
        };
        let Some(ExprLocation::Decoded(range)) = slot.locate(expr_if.cond.span()) else {
            panic!("condition not located");
        };
        assert_eq!(
            &"ab {if self.stock > 0 { md_sync! { \"x\" } } else { other }} c"[range],
            "self.stock > 0"
        );
        let foreign: Expr = syn::parse_str("elsewhere").unwrap();
        assert!(slot.locate(foreign.span()).is_none());
    }

    #[test]
    fn named_arguments_substitute_inside_control_expressions() {
        let template: MdTemplateInput = syn::parse_str(
            "r###\"{if show { md_sync! { \"x\" } }}{for item in items { item }}\"###, show = flag, items = &values",
        )
        .unwrap();
        let nodes = template.semantic_nodes().unwrap();
        let rendered: Vec<String> = nodes
            .iter()
            .map(|node| match node {
                TplNode::Expr(expr) => expr.to_token_stream().to_string(),
                TplNode::Text(text) => text.clone(),
            })
            .collect();
        assert!(rendered[0].starts_with("if flag {"), "{rendered:?}");
        assert!(
            rendered[1].starts_with("for item in & values {"),
            "{rendered:?}"
        );
    }
}
