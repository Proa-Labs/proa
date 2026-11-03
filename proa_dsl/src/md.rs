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
use syn::{braced, Expr, LitStr};

mod code;
mod source;
mod syntax;

/// Location of an expression in authored source, before template dedenting.
#[derive(Clone, Debug)]
pub enum ExprLocation {
    /// Byte range in an unencoded template passed directly to `parse_tpl`.
    Decoded(Range<usize>),
    /// Byte range in the complete Rust literal token, including its delimiters.
    Literal { span: Span, range: Range<usize> },
    /// Ordinary Rust tokens, including named-argument values.
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
        self.locate_token_range(span, 0..span.byte_range().len())
    }

    /// Map bytes within a token parsed from this slot (for example, a nested
    /// template literal). Compose this with enclosing slots to reach the file.
    pub fn locate_token_range(&self, span: Span, range: Range<usize>) -> Option<ExprLocation> {
        let source = self.source.as_ref()?;
        source.whole.join(span)?;
        let relative = span.byte_range();
        if range.start > range.end || range.end > relative.len() || relative.end > source.len {
            return None;
        }
        let decoded = source.origin + relative.start + range.start
            ..source.origin + relative.start + range.end;
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

/// Identify slots whose surrounding Markdown requires inline content.
/// Rust slots are opaque single values when resolving links, headings, inline
/// formatting (including GFM strikethrough), and table cells; their Rust layout
/// is not Markdown layout.
/// The returned flags follow expression order, without changing authored bytes.
pub fn inline_slot_contexts(nodes: &[TplNode]) -> Vec<bool> {
    use pulldown_cmark::{Event, Options, Parser, Tag};

    let mut source = String::new();
    let mut slots = Vec::new();
    for node in nodes {
        match node {
            TplNode::Text(text) => source.push_str(text),
            TplNode::Expr(_) => {
                slots.push(source.len());
                source.push('x');
            }
        }
    }
    let mut inline = vec![false; slots.len()];
    if slots.is_empty() {
        return inline;
    }
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
        if matches!(
            event,
            Event::Start(
                Tag::Link { .. }
                    | Tag::Image { .. }
                    | Tag::Heading { .. }
                    | Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::TableCell
            )
        ) {
            for (slot, restricted) in slots.iter().zip(&mut inline) {
                *restricted |= range.contains(slot);
            }
        }
    }
    inline
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
    /// Named arguments are ordinary bindings surrounding these nodes; they
    /// must not be substituted into expressions, which would capture locals.
    pub fn parse_nodes(&self) -> syn::Result<Vec<TplNode>> {
        for (index, arg) in self.args.iter().enumerate() {
            if self.args[..index]
                .iter()
                .any(|prior| prior.name == arg.name)
            {
                return Err(syn::Error::new(
                    arg.name.span(),
                    format!("duplicate named argument `{}`", arg.name),
                ));
            }
        }
        let source = source::MappedLiteral::new(&self.lit);
        let mut nodes =
            parse_tpl(&source.text).map_err(|message| syn::Error::new(self.lit.span(), message))?;
        source.map_nodes(&mut nodes, self.lit.span());
        Ok(nodes)
    }

    /// Bind argument values once, in source order, before rendering the nodes.
    /// A tuple evaluates every initializer in the caller's scope before any
    /// argument name is bound. Rust then handles shadowing, closures, nested
    /// macros, and ownership without a second implementation of its scopes.
    pub fn argument_binding(&self) -> Option<syn::Local> {
        if self.args.is_empty() {
            return None;
        }
        let names = self.args.iter().map(|arg| &arg.name);
        let values = self.args.iter().map(|arg| &arg.value);
        let syn::Stmt::Local(binding) = syn::parse_quote! (
            #[deny(unused_variables)]
            let (#(#names,)*) = (#(#values,)*);
        ) else {
            unreachable!()
        };
        Some(binding)
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

pub fn bare_ident(expr: &Expr) -> Option<&syn::Ident> {
    let Expr::Path(path) = expr else { return None };
    if path.attrs.is_empty() && path.qself.is_none() {
        path.path.get_ident()
    } else {
        None
    }
}

const CONTROL_HINT: &str = r#"

Text inside `{ ... }` is Rust, including the bodies of `if`, `for`, and `match`. A branch renders its final expression, so write Markdown there as a nested template: `{if ready { md_sync! { "**In stock**" } } else { "Backordered" }}`. Plain Markdown is never parsed inside a slot."#;

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
/// literal braces, and Markdown code blocks or inline code spans are copied
/// verbatim. A slot runs to the brace that balances its opener under Rust
/// lexical rules, so nested blocks, strings, and nested template literals
/// never need escaping.
pub fn parse_tpl(input: &str) -> Result<Vec<TplNode>, String> {
    let bytes = input.as_bytes();
    let mut nodes = Vec::new();
    let mut text = String::new();
    let mut pos = 0;
    let mut code = code::CodeContexts::new(input);

    while pos < bytes.len() {
        if let Some(end) = code.end_at(pos) {
            text.push_str(&input[pos..end]);
            pos = end;
            continue;
        }

        match bytes[pos] {
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
                code.exclude_slot(pos..end + 1);
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

    #[test]
    fn strikethrough_slots_require_inline_children_but_escaped_tildes_do_not() {
        let nodes = parse_tpl("{outside} ~~{inside}~~").unwrap();
        assert_eq!(inline_slot_contexts(&nodes), [false, true]);
        let nodes = parse_tpl(r"\~\~{child}\~\~").unwrap();
        assert_eq!(inline_slot_contexts(&nodes), [false]);
    }

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
        let nodes = kinds(
            r#"{if show {
    md_sync! { "- " {label} "\n" }
}}
"#,
        );
        assert_eq!(nodes.len(), 2, "{nodes:?}");
        assert!(nodes[0].starts_with("expr(if show {"), "{nodes:?}");
        assert_eq!(nodes[1], "text\"\\n\"");
        let nested = parse_tpl(
            r##"{for x in xs { md_sync! { r#"
    - {x} }} not a closer
"# } }}"##,
        )
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
    fn unmatched_and_escaped_backticks_do_not_hide_slots() {
        for source in [
            "a `` {value} ` end",
            "a ``` {value} `` end",
            r"Escaped \`{value}`",
            r"Escaped \\\`{value}`",
        ] {
            let nodes = parse_tpl(source).unwrap();
            assert_eq!(
                nodes
                    .iter()
                    .filter(|node| matches!(node, TplNode::Expr(_)))
                    .count(),
                1,
                "{source}"
            );
        }
        for source in [
            "a `` {literal} `` end",
            r"Even \\`{literal}`",
            r"Escaped \``{literal}`",
            "a `` {literal} ` still code `` end",
        ] {
            assert!(
                parse_tpl(source)
                    .unwrap()
                    .iter()
                    .all(|node| matches!(node, TplNode::Text(_))),
                "{source}"
            );
        }
    }

    #[test]
    fn sub_expressions_locate_inside_their_slot() {
        let nodes =
            parse_tpl(r#"ab {if self.stock > 0 { md_sync! { "x" } } else { other }} c"#).unwrap();
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
            &r#"ab {if self.stock > 0 { md_sync! { "x" } } else { other }} c"#[range],
            "self.stock > 0"
        );
        let foreign: Expr = syn::parse_str("elsewhere").unwrap();
        assert!(slot.locate(foreign.span()).is_none());
    }

    #[test]
    fn named_arguments_bind_outside_control_expressions() {
        let template: MdTemplateInput = syn::parse_str(
            r####"r###"{if show { md_sync! { "x" } }}{for item in items { item }}"###, show = flag, items = &values"####,
        )
        .unwrap();
        let nodes = template.parse_nodes().unwrap();
        let rendered: Vec<String> = nodes
            .iter()
            .map(|node| match node {
                TplNode::Expr(expr) => expr.to_token_stream().to_string(),
                TplNode::Text(text) => text.clone(),
            })
            .collect();
        let binding = template.argument_binding().unwrap();
        assert_eq!(
            binding.to_token_stream().to_string(),
            "# [deny (unused_variables)] let (show , items ,) = (flag , & values ,) ;"
        );
        assert!(rendered[0].starts_with("if show {"), "{rendered:?}");
        assert!(
            rendered[1].starts_with("for item in items {"),
            "{rendered:?}"
        );
    }
}
