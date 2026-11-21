// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Resolve generated runtime paths in consumers of individual crates or Proa's facade.

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;

/// Prefer a consumer's direct dependency, including a renamed dependency.
/// Otherwise use the matching facade re-export. The fallback keeps standalone
/// source analysis and generated-token tests independent of Cargo manifests.
pub fn runtime(package: &str) -> TokenStream {
    if let Ok(found) = crate_name(package) {
        return found_path(found, package);
    }
    if let Ok(found) = crate_name("proa") {
        let facade = found_path(found, "proa");
        return match package {
            "proa_core" | "proa_macros" => facade,
            "rsjs" => quote!(#facade::rsjs),
            "proa_ir" => quote!(#facade::ir),
            "proa_actions" => quote!(#facade::actions),
            "proa_react" => quote!(#facade::react),
            "serde" => quote!(#facade::__private::serde),
            "serde_json" => quote!(#facade::__private::serde_json),
            _ => direct_path(package),
        };
    }
    direct_path(package)
}

fn found_path(found: FoundCrate, package: &str) -> TokenStream {
    match found {
        // Use the library's extern name so integration tests and doctests in
        // that package resolve its library rather than their own crate root.
        FoundCrate::Itself => direct_path(&package.replace('-', "_")),
        FoundCrate::Name(name) => direct_path(&name),
    }
}

fn direct_path(name: &str) -> TokenStream {
    let name = Ident::new(name, Span::call_site());
    quote!(::#name)
}

/// Recognize intrinsic macros through either public package or a Cargo rename.
pub fn is_macro_namespace(ident: &Ident) -> bool {
    if ident == "proa" || ident == "proa_macros" {
        return true;
    }
    ["proa", "proa_macros"].iter().any(|package| {
        matches!(crate_name(package), Ok(FoundCrate::Name(name)) if ident == name.as_str())
    })
}
