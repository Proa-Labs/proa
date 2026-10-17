// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa DSL: shared parser and AST for proa's `html!` / `html_sync!` template
//! macros.
//!
//! This crate exists because `proa_macros` is a `proc-macro = true` crate,
//! and Cargo forbids regular library crates from importing types out of a
//! proc-macro crate (regardless of visibility). Splitting the parser into a
//! plain library crate lets multiple consumers (proa's own macros, rsjs's
//! macros, future codegen tooling) share a single parser definition without
//! vendoring duplicate copies.
//!
//! ## Round-trip support
//!
//! In addition to [`syn::parse::Parse`] impls, the AST types in [`parse`]
//! implement [`quote::ToTokens`] so they can be re-emitted back into a
//! `html_sync!`-compatible token stream. This is the primitive rsjs uses to
//! parse, rewrite, and re-emit templates without re-implementing the parser.
//!
//! ## Known asymmetry: `<!DOCTYPE>` round-trip is lossy
//!
//! The parser currently stores doctype declarations as a [`String`] with no
//! retained reference to the original tokens or spans. The [`quote::ToTokens`]
//! impl emits a canonical `<!DOCTYPE html>` regardless of the stored content.
//! In practice proa only supports the HTML5 `<!DOCTYPE html>` form, and
//! islands (the primary rsjs round-trip target) never contain doctypes, so
//! this is acceptable for current uses. If precise doctype round-trip is ever
//! needed, the parser would need to retain the original token stream.

pub mod consolidate;
pub mod content;
pub mod emit;
pub mod md;
pub mod md_layout;
pub mod parse;
pub mod text;

pub use parse::{Attribute, Element, HtmlRoot, Node};

/// A structured template parse error, mapped from `syn::Error` so callers
/// never see syn types: message text plus the error's position in the
/// parsed source.
///
/// Positions come from proc-macro2's `span-locations` feature (enabled by
/// this crate): `line` is 1-based, `column` is 0-based, both relative to
/// the start of the string passed to [`parse_str`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

impl ParseError {
    /// Convert one `syn::Error` (which may be a combination of several)
    /// into structured errors, one per underlying diagnostic.
    pub fn from_syn(error: syn::Error) -> Vec<ParseError> {
        error
            .into_iter()
            .map(|err| {
                let start = err.span().start();
                ParseError {
                    message: err.to_string(),
                    line: start.line,
                    column: start.column,
                }
            })
            .collect()
    }
}

/// Parse `html!`-dialect template source at runtime.
///
/// This is the server-side entry point for runtime-template tooling: the same
/// parser the `html!` / `html_sync!` macros use, runnable without a
/// proc-macro context because the whole crate is syn-based. Errors are
/// returned as structured [`ParseError`]s (never raw `syn::Error`),
/// positioned relative to `source`.
///
/// Parsing performs no validation beyond syntax; run
/// [`consolidate::validate`] on the result to apply the security, a11y,
/// and content-model passes the macros run.
pub fn parse_str(source: &str) -> Result<HtmlRoot, Vec<ParseError>> {
    syn::parse_str::<HtmlRoot>(source).map_err(ParseError::from_syn)
}

/// Parse one HTML attribute at runtime.
///
/// This is intended for tools that need to build or rewrite template AST
/// attributes without wrapping the attribute in a synthetic element string.
pub fn parse_attribute_str(source: &str) -> Result<Attribute, Vec<ParseError>> {
    syn::parse_str::<parse::AttributeRoot>(source)
        .map(|root| root.attr)
        .map_err(ParseError::from_syn)
}

#[cfg(test)]
mod parse_str_tests {
    use super::*;

    #[test]
    fn parses_a_template() {
        let root = parse_str(r#"<div class="x"><span>{value}</span></div>"#).unwrap();
        assert_eq!(root.nodes.len(), 1);
    }

    #[test]
    fn maps_errors_to_line_and_column() {
        let errors = match parse_str("<div>\n  <span></div>\n</div>") {
            Ok(_) => panic!("mismatched close tag should fail to parse"),
            Err(errors) => errors,
        };
        assert!(!errors.is_empty());
        let first = &errors[0];
        assert!(first.line >= 2, "error should point at line 2+: {first}");
        assert!(!first.message.is_empty());
    }

    #[test]
    fn parses_static_attribute() {
        let attr = parse_attribute_str(r#"data-title="Hello""#).unwrap();
        match attr {
            Attribute::Static { name, value } => {
                assert_eq!(name.as_markup_str(), "data-title");
                assert_eq!(value.value(), "Hello");
            }
            _ => panic!("expected static attribute"),
        }
    }

    #[test]
    fn parses_dynamic_attribute() {
        let attr = parse_attribute_str("aria-label={label}").unwrap();
        match attr {
            Attribute::Dynamic { name, value } => {
                assert_eq!(name.as_markup_str(), "aria-label");
                assert_eq!(quote::quote!(#value).to_string(), "label");
            }
            _ => panic!("expected dynamic attribute"),
        }
    }
}

#[doc(hidden)]
pub mod dev_hot;
