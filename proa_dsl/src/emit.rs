// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `ToTokens` impls for the parser AST.
//!
//! These let consumers (rsjs in particular) parse a `html_sync!` body, rewrite
//! the tree, and re-emit a token stream that proa's own `html_sync!` macro can
//! re-parse. The emitted form is structurally equivalent to the input even
//! though it is not byte-identical (whitespace, span fidelity, and original
//! token-level formatting are not preserved).
//!
//! ## Round-trip discipline
//!
//! - Names preserve their markup spelling (`data-foo`, `svg:path`) separately
//!   from the parser's legacy identifier spelling, so the emitter writes the
//!   markup spelling back as token sequences the parser accepts.
//! - Keyword name parts (`type`, `for`, `loop`, `async`, `in`) are emitted as
//!   keyword tokens; the parser accepts them through `Ident::parse_any`.
//! - Numeric parts (`aria-level-2`) are emitted as `LitInt` segments.
//! - Doctypes are emitted as the canonical `<!DOCTYPE html>`. The parser's
//!   stored doctype string is intentionally discarded; see the crate-level
//!   docs for the rationale.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use syn::{Ident, LitInt, Token};

use crate::parse::{Attribute, Element, HtmlRoot, MarkupName, Node, XmlDeclaration, XmlRoot};

impl ToTokens for HtmlRoot {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for node in &self.nodes {
            node.to_tokens(tokens);
        }
    }
}

impl ToTokens for XmlRoot {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for node in &self.nodes {
            node.to_tokens(tokens);
        }
    }
}

impl ToTokens for Node {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Node::Element(elem) => elem.to_tokens(tokens),
            Node::Fragment(children) => {
                let mut inner = TokenStream::new();
                for c in children {
                    c.to_tokens(&mut inner);
                }
                tokens.append_all(quote::quote! { <> #inner </> });
            }
            Node::Text(lit) => lit.to_tokens(tokens),
            Node::Block(expr) => {
                tokens.append_all(quote::quote! { { #expr } });
            }
            Node::Doctype(_) => {
                // The parser collapses doctype payloads into a String with no
                // path back to the original tokens. proa only supports the
                // HTML5 form, so emit the canonical declaration.
                tokens.append_all(quote::quote! { <!DOCTYPE html> });
            }
            Node::XmlDeclaration(declaration) => declaration.to_tokens(tokens),
        }
    }
}

impl ToTokens for XmlDeclaration {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let version = &self.version;
        let encoding = self
            .encoding
            .as_ref()
            .map(|value| quote::quote! { encoding = #value });
        let standalone = self
            .standalone
            .as_ref()
            .map(|value| quote::quote! { standalone = #value });
        tokens.append_all(quote::quote! { <?xml version = #version #encoding #standalone ?> });
    }
}

impl ToTokens for Element {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // Emit the tag name as a sequence of `Ident -` `Ident` pairs so
        // hyphenated custom-element tags (`rsjs-island`,
        // `my-deeply-nested-component`) survive the round-trip. For plain
        // tags this produces a single `Ident` token, byte-identical to the
        // pre-hyphen behaviour.
        let mut name_tokens = TokenStream::new();
        emit_markup_name(&self.name, &mut name_tokens, false);

        let mut attr_tokens = TokenStream::new();
        for attr in &self.attrs {
            attr.to_tokens(&mut attr_tokens);
        }

        if self.self_closing {
            tokens.append_all(quote::quote! { < #name_tokens #attr_tokens /> });
            return;
        }

        let mut child_tokens = TokenStream::new();
        for child in &self.children {
            child.to_tokens(&mut child_tokens);
        }
        let mut closing_name_tokens = TokenStream::new();
        emit_markup_name(&self.name, &mut closing_name_tokens, false);
        tokens.append_all(
            quote::quote! { < #name_tokens #attr_tokens > #child_tokens </ #closing_name_tokens > },
        );
    }
}

impl ToTokens for Attribute {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Attribute::Static { name, value } => {
                emit_markup_name(name, tokens, true);
                tokens.append_all(quote::quote! { = #value });
            }
            Attribute::Dynamic { name, value } => {
                emit_markup_name(name, tokens, true);
                tokens.append_all(quote::quote! { = { #value } });
            }
            Attribute::Bool { name } => {
                emit_markup_name(name, tokens, true);
            }
        }
    }
}

/// Emit a markup name token sequence that the parser will accept.
fn emit_markup_name(name: &MarkupName, tokens: &mut TokenStream, allow_numeric_parts: bool) {
    let raw = name.as_markup_str();
    let span = name.span();
    let mut current = String::new();

    for ch in raw.chars() {
        match ch {
            '-' | ':' => {
                emit_markup_name_part(&current, span, tokens, allow_numeric_parts);
                current.clear();
                if ch == '-' {
                    tokens.append_all(quote::quote! { - });
                } else {
                    tokens.append_all(quote::quote! { : });
                }
            }
            _ => current.push(ch),
        }
    }

    emit_markup_name_part(&current, span, tokens, allow_numeric_parts);
}

fn emit_markup_name_part(
    part: &str,
    span: Span,
    tokens: &mut TokenStream,
    allow_numeric_parts: bool,
) {
    let part = part.strip_prefix("r#").unwrap_or(part);

    // Keyword parts: must round-trip as keyword tokens since the parser
    // dispatches on `Token![type]`, `Token![for]`, etc.
    match part {
        "type" => {
            let t: Token![type] = syn::token::Type(span);
            t.to_tokens(tokens);
            return;
        }
        "for" => {
            let t: Token![for] = syn::token::For(span);
            t.to_tokens(tokens);
            return;
        }
        "loop" => {
            let t: Token![loop] = syn::token::Loop(span);
            t.to_tokens(tokens);
            return;
        }
        "async" => {
            let t: Token![async] = syn::token::Async(span);
            t.to_tokens(tokens);
            return;
        }
        "in" => {
            let t: Token![in] = syn::token::In(span);
            t.to_tokens(tokens);
            return;
        }
        _ => {}
    }

    // Numeric-only parts (e.g. `2` in `aria-level-2`): emit as integer literal
    // so the parser's `LitInt` branch picks them up.
    if allow_numeric_parts && !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
        let lit = LitInt::new(part, span);
        lit.to_tokens(tokens);
        return;
    }

    // Plain identifier part.
    let ident = Ident::new(part, span);
    ident.to_tokens(tokens);
}
