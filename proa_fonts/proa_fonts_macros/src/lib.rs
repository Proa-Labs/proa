// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Declare self-hosted fonts where you use them.
//!
//! ```ignore
//! use proa_fonts::{Font, google_font};
//!
//! static SANS: Font = google_font!("Geist", subsets = [latin], weights = [400, 700], variable = "--font-sans");
//! ```
//!
//! A call expands to a reference to a `const` that the build-time collector
//! generates into `OUT_DIR` (see `proa_fonts_build`). This macro and the
//! collector parse the call through the same crate (`proa_fonts_parse`) and
//! derive the same id, so the reference and the generated definition always
//! line up. The macro performs **no** I/O: it only names the const the build
//! step must produce.
//!
//! The stock collector entry (`collect::run`) resolves both sources: local
//! files are read from the crate, and Google sources are fetched when the
//! default-on `fetch` feature of `proa_fonts_build` is enabled. Vendoring is
//! gated by `proa-fonts.lock`, so unchanged declarations regenerate offline
//! from the committed vendored files.
//!
//! Your crate needs one line of boilerplate wiring the generated file in
//! (add it once near your crate root):
//!
//! ```ignore
//! mod __proa_fonts_generated {
//!     include!(concat!(env!("OUT_DIR"), "/proa_fonts.rs"));
//! }
//! ```

use proa_fonts_parse::FontRequest;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// The module a `google_font!` / `local_font!` expansion references. A consumer
/// crate wires the build-generated file in at this path.
const GENERATED_MODULE: &str = "__proa_fonts_generated";

/// Declare a Google font to self-host. See the crate docs for the grammar.
#[proc_macro]
pub fn google_font(input: TokenStream) -> TokenStream {
    expand(proa_fonts_parse::parse_google_font(input.into())).into()
}

/// Declare a font vendored from local files.
///
/// ```ignore
/// static BRAND: Font = local_font!("Brand", files = ["fonts/Brand.woff2"], variable = "--font-brand");
/// ```
#[proc_macro]
pub fn local_font(input: TokenStream) -> TokenStream {
    expand(proa_fonts_parse::parse_local_font(input.into())).into()
}

fn expand(parsed: syn::Result<FontRequest>) -> TokenStream2 {
    match parsed {
        Ok(request) => {
            let ident = syn::Ident::new(&request.const_ident(), proc_macro2::Span::call_site());
            let module = syn::Ident::new(GENERATED_MODULE, proc_macro2::Span::call_site());
            quote! { crate::#module::#ident }
        }
        Err(err) => err.to_compile_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote as q;

    #[test]
    fn expansion_references_the_generated_const() {
        let request = proa_fonts_parse::parse_google_font(q!("Geist", weights = [400])).unwrap();
        let expanded = expand(Ok(request.clone())).to_string();
        assert!(expanded.contains("__proa_fonts_generated"));
        assert!(expanded.contains(&request.const_ident()));
    }

    #[test]
    fn parse_error_becomes_a_compile_error_token_stream() {
        let expanded =
            expand(proa_fonts_parse::parse_google_font(q!("Geist", bogus = 1))).to_string();
        assert!(expanded.contains("compile_error"));
    }
}
