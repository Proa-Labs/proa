// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Derive macro for ProaVariant
//!
//! Automatically implements VariantSpec trait for variant enums

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Ident, LitStr, Path};

struct VariantConfig {
    name: String,
    path: Path,
}

pub fn derive_proa_variant(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match generate_variant_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_variant_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let enum_name = &input.ident;

    // Parse attributes
    let config = parse_variant_config(&input.attrs)?;
    let variant_name = config.name;
    let variant_path = config.path;
    let ts_type = enum_name.to_string();

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return Err(Error::new_spanned(
                enum_name,
                "ProaVariant can only be derived for enums",
            ))
        }
    };

    // Find default variant
    let _default_variant = variants
        .iter()
        .find(|v| v.attrs.iter().any(|attr| attr.path().is_ident("default")))
        .map(|v| &v.ident)
        .ok_or_else(|| {
            Error::new_spanned(enum_name, "ProaVariant requires a #[default] variant")
        })?;

    // Generate VariantOption entries and variant data
    let mut variant_data = Vec::new();
    for variant in variants {
        let variant_ident = &variant.ident;

        // Check for unit variants only
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                "ProaVariant only supports unit variants",
            ));
        }

        // Parse attributes for label and value overrides
        let label = extract_attr_value(&variant.attrs, "label")
            .unwrap_or_else(|| variant_ident.to_string());
        let value = extract_attr_value(&variant.attrs, "value")
            .unwrap_or_else(|| to_kebab_case(&variant_ident.to_string()));

        variant_data.push((variant_ident, label, value));
    }

    let options_init = variant_data.iter().map(|(variant_ident, label, value)| {
        quote! {
            #variant_path::VariantOption {
                variant: #enum_name::#variant_ident,
                label: #label,
                value: #value,
                classes: #enum_name::#variant_ident.classes(),
            }
        }
    });

    let expanded = quote! {
        impl #variant_path::VariantSpec for #enum_name {
            const NAME: &'static str = #variant_name;
            const TS_TYPE: &'static str = #ts_type;
            const OPTIONS: &'static [#variant_path::VariantOption<Self>] = &[
                #(#options_init),*
            ];

            fn classes(self) -> &'static str {
                // Use UFCS to call the inherent impl method, not this trait method
                #enum_name::classes(self)
            }
        }
    };

    Ok(expanded)
}

/// Parse `#[variant(name = "...", path = "...")]` on an enum.
fn parse_variant_config(attrs: &[syn::Attribute]) -> Result<VariantConfig, Error> {
    for attr in attrs {
        if attr.path().is_ident("variant") {
            let mut name = None;
            let mut path = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value: LitStr = meta.value()?.parse()?;
                    name = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("path") {
                    let value: LitStr = meta.value()?.parse()?;
                    path = Some(value.parse::<Path>()?);
                    return Ok(());
                }
                Err(meta.error("expected `name` or `path`"))
            })?;
            let name = name
                .ok_or_else(|| Error::new_spanned(attr, "ProaVariant requires `name = \"...\"`"))?;
            return Ok(VariantConfig {
                name,
                path: path.unwrap_or_else(|| syn::parse_quote!(crate::variant_spec)),
            });
        }
    }
    Err(Error::new(
        proc_macro2::Span::call_site(),
        "ProaVariant requires #[variant(name = \"...\")] attribute",
    ))
}

/// Extract a named attribute value like #[variant(label = "...")]
fn extract_attr_value(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("variant") {
            if let Ok(Some(value)) = attr.parse_args_with(|input: syn::parse::ParseStream| {
                let ident: Ident = input.parse()?;
                if ident == key {
                    input.parse::<syn::Token![=]>()?;
                    let value: syn::LitStr = input.parse()?;
                    Ok(Some(value.value()))
                } else {
                    Ok(None)
                }
            }) {
                return Some(value);
            }
        }
    }
    None
}

/// Convert PascalCase or SCREAMING_SNAKE_CASE to kebab-case
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_was_lowercase = false;

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 && prev_was_lowercase {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
            prev_was_lowercase = false;
        } else if ch == '_' {
            result.push('-');
            prev_was_lowercase = false;
        } else {
            result.push(ch);
            prev_was_lowercase = true;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("Sm"), "sm");
        assert_eq!(to_kebab_case("Default"), "default");
        assert_eq!(to_kebab_case("PrimaryColor"), "primary-color");
        assert_eq!(to_kebab_case("LARGE_SIZE"), "large-size");
    }
}
