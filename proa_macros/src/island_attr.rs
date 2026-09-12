// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `#[island]` attribute macro implementation.
//!
//! Parses `#[island] pub struct Counter;`, reads the corresponding TSX file,
//! extracts the props interface, and generates a typed Rust struct + `IslandDef` static.
//!
//! We use `pub struct Counter;` (a unit struct) as the input syntax because attribute
//! macros receive tokens after rustc's parser validates the item form. A bare
//! `pub static Counter;` without a type annotation is rejected before the macro runs.
//! The macro completely replaces the unit struct with the generated props struct + static.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, Visibility};

use crate::tsconfig;
use crate::tsx_parser::{self, RustType};
use crate::tsx_resolve;

/// Parsed attribute arguments for `#[island(...)]`.
#[derive(Default)]
struct IslandArgs {
    /// Explicit path to the TSX file.
    path: Option<String>,
    /// Module specifier (resolved via tsconfig aliases).
    module: Option<String>,
    /// Export name (if different from the static name).
    export: Option<String>,
    /// Default hydration strategy.
    strategy: Option<String>,
}

/// Parse attribute args like `#[island(module = "...", export = "...")]`.
fn parse_args(args: TokenStream) -> syn::Result<IslandArgs> {
    if args.is_empty() {
        return Ok(IslandArgs::default());
    }

    let mut result = IslandArgs::default();

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("path") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            result.path = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("module") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            result.module = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("export") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            result.export = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("strategy") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            result.strategy = Some(lit.value());
            Ok(())
        } else {
            Err(meta.error("unknown island attribute"))
        }
    });

    syn::parse::Parser::parse2(parser, args)?;
    Ok(result)
}

/// Main entry point for the `#[island]` attribute macro.
///
/// Input must be a unit struct: `pub struct Counter;`
/// The macro replaces it with a props struct + `IslandDef` static.
pub fn island_attr(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let args = parse_args(args)?;

    // Parse as a DeriveInput — the input must be a unit struct like `pub struct Counter;`
    let item: syn::DeriveInput = syn::parse2(input)?;

    // Validate it's a unit struct
    match &item.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Unit,
            ..
        }) => {}
        _ => {
            return Err(syn::Error::new_spanned(
                &item,
                "#[island] must be applied to a unit struct, e.g. `pub struct Counter;`",
            ));
        }
    }

    let vis = &item.vis;
    let static_name = &item.ident;
    let static_name_string = static_name.to_string();
    let component_name = args.export.as_deref().unwrap_or(&static_name_string);
    let component_name_owned = component_name.to_string();
    let props_struct_name = Ident::new(&format!("{}Props", static_name), static_name.span());

    // Resolve the TSX file
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let manifest_path = std::path::Path::new(&manifest_dir);

    let config = tsconfig::load_resolve_config(manifest_path).map_err(|e| {
        syn::Error::new(
            static_name.span(),
            format!("Failed to load tsconfig: {}", e),
        )
    })?;

    let tsx_path = tsx_resolve::resolve_tsx_path(
        &component_name_owned,
        args.path.as_deref(),
        args.module.as_deref(),
        &config,
        manifest_path,
    )
    .map_err(|e| syn::Error::new(static_name.span(), e))?;

    let tsx_content = std::fs::read_to_string(&tsx_path).map_err(|e| {
        syn::Error::new(
            static_name.span(),
            format!("Failed to read {:?}: {}", tsx_path, e),
        )
    })?;

    // Extract props
    let props = tsx_parser::extract_props(&tsx_content, &component_name_owned)
        .map_err(|e| syn::Error::new(static_name.span(), format!("TSX parse error: {}", e)))?;

    // Generate the props struct
    let (struct_def, _has_children) = if let Some(props) = props {
        generate_props_struct(vis, &props_struct_name, &props)?
    } else {
        // No props interface found — generate empty struct
        let def = quote! {
            #[derive(serde::Serialize)]
            #vis struct #props_struct_name;
        };
        (def, false)
    };

    // Generate the static
    let name_lit = syn::LitStr::new(&static_name.to_string(), Span::call_site());

    let static_def = quote! {
        #[allow(non_upper_case_globals)]
        #vis static #static_name: proa_react::IslandDef<#props_struct_name> =
            proa_react::IslandDef::new(#name_lit);
    };

    // Emit include_bytes! so Cargo watches the TSX file for changes
    let tsx_path_str = tsx_path.to_string_lossy().to_string();
    let tsx_path_lit = syn::LitStr::new(&tsx_path_str, Span::call_site());
    let file_watch = quote! {
        const _: &[u8] = include_bytes!(#tsx_path_lit);
    };

    let expanded = quote! {
        #struct_def
        #static_def
        #file_watch
    };

    Ok(expanded)
}

/// Generate a `#[derive(Serialize)] pub struct FooProps { ... }`.
fn generate_props_struct(
    vis: &Visibility,
    struct_name: &Ident,
    props: &tsx_parser::PropsInterface,
) -> syn::Result<(TokenStream, bool)> {
    let mut field_defs = Vec::new();
    let mut has_children = false;

    for field in &props.fields {
        let field_name = Ident::new(&field.rust_name, Span::call_site());
        let field_type = field.rust_type.to_tokens();

        let is_children = matches!(field.rust_type, RustType::Children)
            || matches!(&field.rust_type, RustType::Option(inner) if matches!(inner.as_ref(), RustType::Children));

        let mut attrs = Vec::new();

        // Add #[serde(rename = "camelCase")] if rust name differs from ts name
        if field.rust_name != field.ts_name {
            let ts_name = &field.ts_name;
            attrs.push(quote! { #[serde(rename = #ts_name)] });
        }

        // Children fields are #[serde(skip)]
        if is_children {
            has_children = true;
            attrs.push(quote! { #[serde(skip)] });
        }

        // Optional fields skip None values
        if field.optional && !is_children {
            attrs.push(quote! { #[serde(skip_serializing_if = "Option::is_none")] });
        }

        field_defs.push(quote! {
            #(#attrs)*
            pub #field_name: #field_type
        });
    }

    let def = quote! {
        #[derive(serde::Serialize)]
        #vis struct #struct_name {
            #(#field_defs,)*
        }
    };

    Ok((def, has_children))
}
