// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The experimental hot-static macro's shared validation and consolidation.

use crate::{
    consolidate::{
        sink::{ConsolidationSink, DynamicAttr, RawTextKind},
        MarkupDialect,
    },
    HtmlRoot,
};
pub use proa_core::dev_hot::{StaticManifest, StaticTemplate};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::{
    parse::{Parse, ParseStream},
    Expr, LitStr, Token,
};

/// Explicit identity followed by a normal sync HTML template.
pub struct HotInput {
    /// Project-unique static template ID.
    pub id: LitStr,
    /// Ordinary checked HTML tree.
    pub root: HtmlRoot,
    tokens: TokenStream,
}

impl Parse for HotInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let id: LitStr = input.parse()?;
        input.parse::<Token![;]>()?;
        let tokens: TokenStream = input.parse()?;
        let root = syn::parse2(tokens.clone())?;
        Ok(Self { id, root, tokens })
    }
}

/// Compile only the supported static-update surface. Dynamic expressions and
/// element/attribute names participate verbatim in the compatibility shape.
pub fn compile(input: &HotInput) -> Result<StaticTemplate, String> {
    if input.id.value().is_empty() {
        return Err("hot template ID must not be empty".into());
    }
    fn has_raw_text(nodes: &[crate::Node]) -> bool {
        nodes.iter().any(|node| match node {
            crate::Node::Element(element) => {
                matches!(element.name.as_markup_str(), "script" | "style")
                    || has_raw_text(&element.children)
            }
            crate::Node::Fragment(nodes) => has_raw_text(nodes),
            _ => false,
        })
    }
    if has_raw_text(&input.root.nodes) {
        return Err("script/style edits require a normal template".into());
    }
    let validation = crate::consolidate::validate(&input.root);
    if !validation.errors.is_empty() {
        return Err(format!("{:?}", validation.errors));
    }
    let mut sink = HotSink::default();
    crate::consolidate::walk::consolidate_root(&input.root, MarkupDialect::Html, &mut sink);
    if let Some(error) = sink.error {
        return Err(error);
    }
    let tokens = canonical_tokens(mask_static_literals(input.tokens.clone()));
    Ok(StaticTemplate {
        // Emptying a literal can remove a static run, even while the masked
        // token structure (or total slot count) stays unchanged. Track the
        // ordered walk events so data cannot move across compiled Rust slots.
        shape: format!(
            "T{}:{tokens}R{}:{}",
            tokens.len(),
            sink.layout.len(),
            sink.layout
        ),
        slots: sink.slots,
    })
}

// Compiler-native TokenStream Display preserves source spacing/newlines,
// while the CLI lexer prints canonical spaces. Compare token structure rather
// than either renderer's text formatting.
fn canonical_tokens(tokens: TokenStream) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for token in tokens {
        match token {
            TokenTree::Ident(ident) => {
                let value = ident.to_string();
                write!(result, "I{}:{value};", value.len()).unwrap();
            }
            TokenTree::Literal(literal) => {
                let value = literal.to_string();
                write!(result, "L{}:{value};", value.len()).unwrap();
            }
            TokenTree::Punct(punct) => {
                write!(result, "P{}{:?};", punct.as_char(), punct.spacing()).unwrap();
            }
            TokenTree::Group(group) => {
                let body = canonical_tokens(group.stream());
                write!(result, "G{:?}{}:{body};", group.delimiter(), body.len()).unwrap();
            }
        }
    }
    result
}

fn mask_static_literals(tokens: TokenStream) -> TokenStream {
    tokens
        .into_iter()
        .map(|token| match token {
            TokenTree::Literal(ref literal)
                if syn::parse_str::<LitStr>(&literal.to_string()).is_ok() =>
            {
                TokenTree::Literal(proc_macro2::Literal::string("__static_string__"))
            }
            TokenTree::Group(group) if group.delimiter() != Delimiter::Brace => TokenTree::Group(
                proc_macro2::Group::new(group.delimiter(), mask_static_literals(group.stream())),
            ),
            // Braces delimit dynamic Rust slots; string literals inside them
            // must remain part of the compiled program's exact shape.
            token => token,
        })
        .collect()
}

#[derive(Default)]
struct HotSink {
    slots: Vec<Vec<u8>>,
    layout: String,
    error: Option<String>,
}
impl ConsolidationSink for HotSink {
    fn static_run(&mut self, bytes: &str) {
        self.layout.push('S');
        self.slots.push(bytes.as_bytes().to_vec());
    }
    fn dynamic_node(&mut self, expr: &Expr) {
        self.layout.push('N');
        if !matches!(
            expr,
            Expr::Path(_)
                | Expr::Field(_)
                | Expr::Lit(_)
                | Expr::Binary(_)
                | Expr::Struct(_)
                | Expr::Call(_)
                | Expr::MethodCall(_)
        ) {
            self.error = Some("hot templates currently support straight-line typed child expressions; control flow and text! require a normal template".into());
        }
    }
    fn dynamic_attr(&mut self, _attr: DynamicAttr<'_>) {
        self.layout.push('A');
    }
    fn begin_raw_text(&mut self, _kind: RawTextKind) {
        self.error = Some("raw-text regions require a normal template".into());
    }
    fn end_raw_text(&mut self, _kind: RawTextKind) {}
}
