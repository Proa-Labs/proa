// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compile-time Tailwind class merging helpers.
//!
//! This crate exposes the [`tw_merge!`] macro which accepts one or more
//! string literals and expands to the merged class list. Because it emits a
//! string literal, the result can be used in `const` contexts.
//!
//! ```rust
//! use tailwind_merge_macro::tw_merge;
//!
//! const BUTTON: &str = tw_merge!("px-2", "px-4", "text-sm", "text-lg");
//! assert_eq!(BUTTON, "px-4 text-lg");
//! ```

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::vec::Vec;
use syn::{parse::Parse, parse_macro_input, LitStr, Result as ParseResult, Token};

struct MergeInput {
    values: Vec<LitStr>,
}

impl Parse for MergeInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> ParseResult<Self> {
        let mut values = Vec::new();
        while !input.is_empty() {
            let literal: LitStr = input.parse()?;
            values.push(literal);
            if input.is_empty() {
                break;
            }
            let _comma: Token![,] = input.parse()?;
        }

        if values.is_empty() {
            return Err(input.error("expected at least one string literal"));
        }

        Ok(Self { values })
    }
}

/// Merge Tailwind class literals at compile time.
#[proc_macro]
pub fn tw_merge(input: TokenStream) -> TokenStream {
    let MergeInput { values } = parse_macro_input!(input as MergeInput);

    let parts: Vec<String> = values.iter().map(|literal| literal.value()).collect();

    let mut merger = tailwind_merge::TailwindMerge::new(tailwind_merge::Config::clone_default());
    let merged = merger.merge_iter(parts.iter().map(String::as_str));

    let literal = LitStr::new(&merged, Span::call_site());
    quote!(#literal).into()
}
