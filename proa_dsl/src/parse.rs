// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Parsing for the `html!` and `xml!` macro syntax
use std::fmt;

use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream, Result};
use syn::spanned::Spanned;
use syn::{braced, Expr, Ident, LitInt, LitStr, Token};

/// Root of the HTML tree
pub struct HtmlRoot {
    pub nodes: Vec<Node>,
}

/// HTML root plus the native start span of each top-level node.
///
/// This is an internal compiler seam for incremental source partitioning. The
/// ordinary [`HtmlRoot`] remains the canonical syntax owner; callers use these
/// spans only to recover exact, independently parseable source slices.
#[doc(hidden)]
pub struct HtmlRootWithTopLevelSpans {
    pub root: HtmlRoot,
    pub starts: Vec<proc_macro2::Span>,
}

/// Root of the XML tree.
///
/// XML uses the same node representation as `HtmlRoot`, but parses with XML
/// name rules: `prefix:local` qualified names are accepted and HTML void
/// element inference is disabled.
pub struct XmlRoot {
    pub nodes: Vec<Node>,
}

impl XmlRoot {
    pub fn into_inner(self) -> HtmlRoot {
        HtmlRoot { nodes: self.nodes }
    }
}

pub(crate) struct AttributeRoot {
    pub attr: Attribute,
}

/// A node in the HTML tree
pub enum Node {
    /// An HTML element: `<div>...</div>`
    Element(Element),
    /// A fragment: `<>...</>`
    Fragment(Vec<Node>),
    /// A text node: `"Hello world"`
    Text(LitStr),
    /// A Rust expression: `{expr}`
    Block(Expr),
    /// A DOCTYPE declaration: `<!DOCTYPE html>`
    Doctype(String),
}

/// An HTML element
pub struct Element {
    /// Name of the tag.
    ///
    /// `MarkupName` preserves the source-level markup spelling for emitters
    /// (`rsjs-island`, `svg:path`, `xlink:href`) while retaining a legacy
    /// identifier spelling for existing consumers that call `to_string()`.
    pub name: MarkupName,
    pub attrs: Vec<Attribute>,
    pub children: Vec<Node>,
    pub self_closing: bool,
    pub opening_end: proc_macro2::Span,
}

impl Element {
    /// Canonical tag name as it appeared in the source.
    ///
    /// HTML parsing normalizes legacy underscore storage back to hyphenated
    /// markup spelling. XML parsing preserves source-level spelling, including
    /// namespace separators and underscores.
    pub fn tag_str(&self) -> String {
        self.name.as_markup_str().to_owned()
    }

    pub fn opening_end_line_column(&self) -> proc_macro2::LineColumn {
        self.opening_end.start()
    }
}

/// A tag or attribute name as written in markup.
///
/// `legacy_ident` keeps old `name.to_string()` behavior alive for HTML-oriented
/// consumers: dashes and namespace separators are flattened to underscores.
/// XML emitters should use [`MarkupName::as_markup_str`] so `:` and `_` remain
/// meaningful markup characters.
pub struct MarkupName {
    raw: String,
    legacy_ident: Ident,
}

impl MarkupName {
    pub fn as_markup_str(&self) -> &str {
        &self.raw
    }

    pub fn legacy_ident(&self) -> &Ident {
        &self.legacy_ident
    }

    pub fn span(&self) -> proc_macro2::Span {
        self.legacy_ident.span()
    }

    pub fn start_line_column(&self) -> proc_macro2::LineColumn {
        self.span().start()
    }
}

impl Clone for MarkupName {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            legacy_ident: self.legacy_ident.clone(),
        }
    }
}

impl fmt::Display for MarkupName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.legacy_ident.fmt(f)
    }
}

impl PartialEq for MarkupName {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for MarkupName {}

impl PartialEq<str> for MarkupName {
    fn eq(&self, other: &str) -> bool {
        self.legacy_ident == other
    }
}

impl PartialEq<&str> for MarkupName {
    fn eq(&self, other: &&str) -> bool {
        self.legacy_ident == *other
    }
}

/// An attribute on an element
pub enum Attribute {
    /// Static attribute: `class="container"`
    Static { name: MarkupName, value: LitStr },
    /// Dynamic attribute: `class={expr}`
    Dynamic { name: MarkupName, value: Box<Expr> },
    /// Boolean attribute: `disabled`
    Bool { name: MarkupName },
}

impl Parse for HtmlRoot {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_root(input, ParseMode::Html).map(|nodes| HtmlRoot { nodes })
    }
}

impl Parse for HtmlRootWithTopLevelSpans {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut nodes = Vec::new();
        let mut starts = Vec::new();

        while !input.is_empty() {
            starts.push(input.span());
            nodes.push(parse_node(input, ParseMode::Html)?);
        }

        Ok(Self {
            root: HtmlRoot { nodes },
            starts,
        })
    }
}

impl Parse for XmlRoot {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_root(input, ParseMode::Xml).map(|nodes| XmlRoot { nodes })
    }
}

impl Parse for AttributeRoot {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_attribute(input, ParseMode::Html).map(|attr| AttributeRoot { attr })
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum ParseMode {
    Html,
    Xml,
}

fn parse_root(input: ParseStream, mode: ParseMode) -> Result<Vec<Node>> {
    let mut nodes = Vec::new();

    while !input.is_empty() {
        nodes.push(parse_node(input, mode)?);
    }

    Ok(nodes)
}

fn parse_node(input: ParseStream, mode: ParseMode) -> Result<Node> {
    let lookahead = input.lookahead1();

    if lookahead.peek(Token![<]) {
        // Check if this is a DOCTYPE, fragment, or regular element
        let fork = input.fork();
        fork.parse::<Token![<]>()?;

        // Check for `<!` (DOCTYPE)
        if fork.peek(Token![!]) {
            return parse_doctype(input);
        }

        // Check for `<>` (fragment)
        if fork.peek(Token![>]) {
            return parse_fragment(input, mode);
        }

        // Regular element
        parse_element(input, mode).map(Node::Element)
    } else if lookahead.peek(LitStr) {
        // Text literal
        input.parse::<LitStr>().map(Node::Text)
    } else if lookahead.peek(syn::token::Brace) {
        // Rust expression block
        let content;
        braced!(content in input);
        let expr = content.parse::<Expr>()?;
        Ok(Node::Block(expr))
    } else {
        Err(lookahead.error())
    }
}

fn parse_doctype(input: ParseStream) -> Result<Node> {
    // Parse `<!DOCTYPE html>`
    input.parse::<Token![<]>()?;
    input.parse::<Token![!]>()?;

    // Parse the DOCTYPE keyword
    let doctype_ident = input.parse::<Ident>()?;
    if doctype_ident != "DOCTYPE" {
        return Err(syn::Error::new(
            doctype_ident.span(),
            format!("expected `DOCTYPE`, found `{}`", doctype_ident),
        ));
    }

    // Parse the rest until `>`
    let mut doctype_content = String::from("<!DOCTYPE");

    // Collect tokens until we hit `>`
    while !input.peek(Token![>]) {
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "unclosed DOCTYPE declaration",
            ));
        }

        // Try to parse as identifier or literal
        if let Ok(ident) = input.parse::<Ident>() {
            doctype_content.push(' ');
            doctype_content.push_str(&ident.to_string());
        } else {
            // Skip other tokens (whitespace is already handled)
            break;
        }
    }

    input.parse::<Token![>]>()?;
    doctype_content.push('>');

    Ok(Node::Doctype(doctype_content))
}

fn parse_fragment(input: ParseStream, mode: ParseMode) -> Result<Node> {
    // Parse `<>...</>`
    let span = input.span();
    input.parse::<Token![<]>()?;
    input.parse::<Token![>]>()?;

    // Parse children until we hit `</>`
    let mut children = Vec::new();
    while !peek_closing_fragment(input) {
        if input.is_empty() {
            return Err(syn::Error::new(span, "unclosed fragment `<>`"));
        }
        children.push(parse_node(input, mode)?);
    }

    // Parse closing fragment: `</>`
    input.parse::<Token![<]>()?;
    input.parse::<Token![/]>()?;
    input.parse::<Token![>]>()?;

    Ok(Node::Fragment(children))
}

fn peek_closing_fragment(input: ParseStream) -> bool {
    // Look ahead to see if we have `</>`
    let fork = input.fork();
    if fork.parse::<Token![<]>().is_err() {
        return false;
    }
    if fork.parse::<Token![/]>().is_err() {
        return false;
    }
    fork.peek(Token![>])
}

/// HTML and SVG void elements that never have children or closing tags.
fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        // HTML void elements
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input"
        | "link" | "meta" | "source" | "track" | "wbr"
        // SVG void elements
        | "path" | "circle" | "ellipse" | "line" | "polygon" | "polyline"
        | "rect" | "stop" | "use" | "image"
        | "animate" | "animateMotion" | "animateTransform" | "set"
    )
}

/// Parse a (possibly hyphenated, and in XML mode possibly namespaced) tag name.
///
/// HTML5 custom elements use kebab-case names like `rsjs-island` or
/// `my-deeply-nested-component`. `syn` only hands out one `Ident` at a time,
/// so we collect the leading ident plus any `-Ident` continuations and keep
/// both the source-level markup name and the flattened legacy identifier.
///
/// Backwards compatibility: a plain `div` parses as exactly one segment,
/// producing `Ident("div")` with the original span. Behaviour for
/// non-hyphenated tags is byte-identical to the previous parser.
fn parse_tag_name(input: ParseStream, mode: ParseMode) -> Result<MarkupName> {
    parse_markup_name(input, mode, false)
}

// proa-perf-ok(scope): parser diagnostics quote tag syntax; no HTML is rendered
fn parse_element(input: ParseStream, mode: ParseMode) -> Result<Element> {
    let span = input.span();

    // Parse opening tag: `<div`, `<rsjs-island`, `<my-component`, ...
    input.parse::<Token![<]>()?;
    let name = parse_tag_name(input, mode)?;

    // Parse attributes
    let mut attrs = Vec::new();
    while !input.peek(Token![>]) && !input.peek(Token![/]) {
        attrs.push(parse_attribute(input, mode)?);
    }

    // Check for self-closing: `/>` vs `>`
    // Also treat known void elements (HTML + SVG) as self-closing
    let (self_closing, opening_end) = if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        let gt = input.parse::<Token![>]>()?;
        (true, gt.span())
    } else {
        let gt = input.parse::<Token![>]>()?;
        let opening_end = gt.span();
        if mode == ParseMode::Html && is_void_element(&tag_display(&name)) {
            // Void element: optionally consume an explicit closing tag like `</path>`
            if peek_closing_tag(input, &name, mode) {
                input.parse::<Token![<]>()?;
                input.parse::<Token![/]>()?;
                let closing_name = parse_tag_name(input, mode)?;
                if closing_name != name {
                    return Err(syn::Error::new(
                        closing_name.span(),
                        format!(
                            "mismatched closing tag: expected `</{}>`, found `</{}>`",
                            tag_display(&name),
                            tag_display(&closing_name),
                        ),
                    ));
                }
                input.parse::<Token![>]>()?;
            }
            (true, opening_end)
        } else {
            (false, opening_end)
        }
    };

    // Parse children (if not self-closing)
    let mut children = Vec::new();
    if !self_closing {
        while !input.peek(Token![<]) || !peek_closing_tag(input, &name, mode) {
            if input.is_empty() {
                return Err(syn::Error::new(
                    span,
                    format!("unclosed element `<{}>`", tag_display(&name)),
                ));
            }
            children.push(parse_node(input, mode)?);
        }

        // Parse closing tag: `</div>` or `</rsjs-island>`.
        input.parse::<Token![<]>()?;
        input.parse::<Token![/]>()?;
        let closing_name = parse_tag_name(input, mode)?;
        if closing_name != name {
            return Err(syn::Error::new(
                closing_name.span(),
                format!(
                    "mismatched closing tag: expected `</{}>`, found `</{}>`",
                    tag_display(&name),
                    tag_display(&closing_name),
                ),
            ));
        }
        input.parse::<Token![>]>()?;
    }

    Ok(Element {
        name,
        attrs,
        children,
        self_closing,
        opening_end,
    })
}

fn peek_closing_tag(input: ParseStream, expected_name: &MarkupName, mode: ParseMode) -> bool {
    // Look ahead to see if we have `</name>` or `</hyphen-ated-name>`.
    let fork = input.fork();
    if fork.parse::<Token![<]>().is_err() {
        return false;
    }
    if fork.parse::<Token![/]>().is_err() {
        return false;
    }
    if let Ok(name) = parse_tag_name(&fork, mode) {
        return name == *expected_name;
    }
    false
}

/// Render a tag-name `Ident` for inclusion in a diagnostic message.
///
/// Render a tag name for inclusion in a diagnostic message.
fn tag_display(name: &MarkupName) -> String {
    name.as_markup_str().to_owned()
}

fn tag_part_to_storage(ident: &Ident) -> String {
    ident
        .to_string()
        .strip_prefix("r#")
        .map(str::to_owned)
        .unwrap_or_else(|| ident.to_string())
}

fn tag_storage_to_html(stored: &str) -> String {
    stored
        .split('_')
        .map(|part| part.strip_prefix("r#").unwrap_or(part))
        .collect::<Vec<_>>()
        .join("-")
}

fn parse_attribute(input: ParseStream, mode: ParseMode) -> Result<Attribute> {
    let name = parse_markup_name(input, mode, true)?;

    // Check for boolean attribute (no value)
    if !input.peek(Token![=]) {
        return Ok(Attribute::Bool { name });
    }

    input.parse::<Token![=]>()?;

    let lookahead = input.lookahead1();
    if lookahead.peek(LitStr) {
        // Static attribute: name="value"
        let value = input.parse::<LitStr>()?;
        Ok(Attribute::Static { name, value })
    } else if lookahead.peek(syn::token::Brace) {
        // Dynamic attribute: name={expr}
        let content;
        braced!(content in input);
        let value = content.parse::<Expr>()?;
        Ok(Attribute::Dynamic {
            name,
            value: Box::new(value),
        })
    } else {
        Err(lookahead.error())
    }
}

fn parse_markup_name(
    input: ParseStream,
    mode: ParseMode,
    allow_numeric_dash_parts: bool,
) -> Result<MarkupName> {
    let first = input.call(Ident::parse_any)?;
    let span = first.span();
    let mut raw_parts = vec![tag_part_to_storage(&first)];
    let mut storage_parts = raw_parts.clone();

    parse_dash_name_tail(
        input,
        mode,
        allow_numeric_dash_parts,
        &mut raw_parts,
        &mut storage_parts,
    )?;

    let mut raw = raw_parts.join("-");
    if mode == ParseMode::Xml && input.peek(Token![:]) {
        input.parse::<Token![:]>()?;
        let local_first = input.call(Ident::parse_any)?;
        let mut local_raw_parts = vec![tag_part_to_storage(&local_first)];
        let mut local_storage_parts = local_raw_parts.clone();
        parse_dash_name_tail(
            input,
            mode,
            allow_numeric_dash_parts,
            &mut local_raw_parts,
            &mut local_storage_parts,
        )?;

        if input.peek(Token![:]) {
            return Err(syn::Error::new(
                input.span(),
                "XML qualified names may contain at most one `:` namespace separator",
            ));
        }

        raw.push(':');
        raw.push_str(&local_raw_parts.join("-"));
        storage_parts.extend(local_storage_parts);
    }

    let legacy_ident = if storage_parts.len() == 1 {
        first
    } else {
        Ident::new(&storage_parts.join("_"), span)
    };

    let raw = if mode == ParseMode::Html {
        tag_storage_to_html(&legacy_ident.to_string())
    } else {
        raw
    };

    Ok(MarkupName { raw, legacy_ident })
}

fn parse_dash_name_tail(
    input: ParseStream,
    mode: ParseMode,
    allow_numeric_dash_parts: bool,
    raw_parts: &mut Vec<String>,
    storage_parts: &mut Vec<String>,
) -> Result<()> {
    while input.peek(Token![-]) && !input.peek2(Token![>]) {
        let fork = input.fork();
        fork.parse::<Token![-]>()?;
        let has_part = if fork.call(Ident::parse_any).is_ok() {
            true
        } else if allow_numeric_dash_parts && fork.peek(LitInt) {
            fork.parse::<LitInt>()?;
            true
        } else {
            false
        };
        if !has_part {
            break;
        }

        input.parse::<Token![-]>()?;
        if allow_numeric_dash_parts && input.peek(LitInt) {
            let lit = input.parse::<LitInt>()?;
            let part = lit.to_string();
            raw_parts.push(part.clone());
            storage_parts.push(part);
        } else {
            let ident = input.call(Ident::parse_any)?;
            let part = tag_part_to_storage(&ident);
            raw_parts.push(part.clone());
            storage_parts.push(part);
        }
    }

    if mode == ParseMode::Html && input.peek(Token![:]) {
        return Err(syn::Error::new(
            input.span(),
            "`:` namespace separators are only supported by xml! and xml_sync!",
        ));
    }

    Ok(())
}
