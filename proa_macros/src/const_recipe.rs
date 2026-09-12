// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ConstRecipe derive macro
//!
//! Generates const fn methods with match-based lookups for zero-cost recipe implementations.
//!
//! ## Example
//!
//! Each enum should implement its own `const fn classes(self) -> &str` method.
//! The macro generates the recipe's `CLASS_CAPACITY` and `classes()` method.
//!
//! ```rust,ignore
//! use proa_macros::ConstRecipe;
//!
//! impl ButtonSize {
//!     pub const fn classes(self) -> &str {
//!         match self {
//!             ButtonSize::Xs => BUTTON_SIZE_XS,
//!             ButtonSize::Sm => BUTTON_SIZE_SM,
//!             // ...
//!         }
//!     }
//! }
//!
//! // Similar implementations for ButtonColor, ButtonVariant, ButtonRadius
//!
//! #[derive(ConstRecipe)]
//! pub struct ButtonRecipe {
//!     base: &str,
//!     size_default: ButtonSize,
//!     color_default: ButtonColor,
//!     variant_default: ButtonVariant,
//!     radius_default: ButtonRadius,
//! }
//! ```
//!
//! This generates:
//! - `pub const CLASS_CAPACITY: usize` - Maximum number of class strings (base + # of variants)
//! - `pub const fn classes(...) -> ([&str; CLASS_CAPACITY], usize)` - Const class generation
//!
//! The generated `classes()` method calls each enum's `.classes()` method!

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, Ident, Type};

pub fn derive_const_recipe(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match generate_const_recipe_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct VariantField {
    field_name: Ident,
    inner_type: Type, // The actual enum type (unwrapped from Option if needed)
}

fn generate_const_recipe_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let struct_name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(Error::new_spanned(
                    struct_name,
                    "ConstRecipe can only be derived for structs with named fields",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                struct_name,
                "ConstRecipe can only be derived for structs",
            ))
        }
    };

    // Find base field and variant fields
    let mut has_base = false;
    let mut variant_fields = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();

        if field_name_str == "base" {
            if !is_static_str_type(&field.ty) {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`base` field must be of type `&str`",
                ));
            }
            has_base = true;
        } else if field_name_str.ends_with("_default") {
            // This is a variant dimension - extract the base name
            let variant_name = field_name_str.trim_end_matches("_default");

            // Extract inner type from Option<T>
            let inner_type = extract_option_inner_type(&field.ty).ok_or_else(|| {
                Error::new_spanned(
                    &field.ty,
                    "variant default fields must be of type Option<T>",
                )
            })?;

            variant_fields.push(VariantField {
                field_name: format_ident!("{}", variant_name),
                inner_type: inner_type.clone(),
            });
        }
    }

    if !has_base {
        return Err(Error::new_spanned(
            struct_name,
            "ConstRecipe requires a `base: &str` field",
        ));
    }

    if variant_fields.is_empty() {
        return Err(Error::new_spanned(
            struct_name,
            "ConstRecipe requires at least one variant dimension (e.g., `size_default: ButtonSize`)",
        ));
    }

    // Calculate CLASS_CAPACITY: base + number of variant dimensions
    let class_capacity = 1 + variant_fields.len();

    // Generate classes() method signature
    let classes_params = variant_fields.iter().map(|vf| {
        let variant_name = &vf.field_name;
        let inner_type = &vf.inner_type;
        quote! { #variant_name: Option<#inner_type> }
    });

    // Generate classes() method body
    let classes_body_parts = variant_fields.iter().map(|vf| {
        let variant_name = &vf.field_name;
        let default_field = format_ident!("{}_default", variant_name);

        quote! {
            // Add #variant_name classes
            // Default fields are Option<T>, use match for const compatibility
            let #variant_name = match #variant_name {
                Some(v) => Some(v),
                None => self.#default_field,
            };

            if let Some(v) = #variant_name {
                let classes = v.classes();
                if !classes.is_empty() {
                    result[idx] = classes;
                    idx += 1;
                }
            }
        }
    });

    // Extract the component name (e.g., "ButtonRecipe" -> "Button")
    let struct_name_str = struct_name.to_string();
    let component_name = struct_name_str.trim_end_matches("Recipe");

    // Generate VariantSpec::variant_metadata() calls
    // Check if the default is None to set the optional flag
    let variant_metadata_calls = variant_fields.iter().map(|vf| {
        let inner_type = &vf.inner_type;
        let variant_name = &vf.field_name;
        let default_field = format_ident!("{}_default", variant_name);

        quote! {
            {
                let mut metadata = crate::variant_spec::variant_metadata::<#inner_type>();
                // If the recipe's default is None, mark this variant as optional
                metadata.optional = self.#default_field.is_none();
                metadata
            }
        }
    });

    let expanded = quote! {
        impl #struct_name {
            /// Maximum number of class strings that can be generated
            pub const CLASS_CAPACITY: usize = #class_capacity;

            /// Generate Tailwind classes for the given variant selections
            ///
            /// This is a const fn, meaning it can be evaluated at compile time!
            /// The compiler can pre-compute class arrays for known variant combinations.
            pub const fn classes(
                &self,
                #(#classes_params),*
            ) -> ([&str; Self::CLASS_CAPACITY], usize) {
                let mut result = [""; Self::CLASS_CAPACITY];
                let mut idx = 0;

                // Add base classes
                if !self.base.is_empty() {
                    result[idx] = self.base;
                    idx += 1;
                }

                #(#classes_body_parts)*

                (result, idx)
            }
        }

        impl crate::variant_spec::ComponentSpec for #struct_name {
            const NAME: &'static str = #component_name;

            fn base_classes(&self) -> &'static str {
                self.base
            }

            fn variant_metadata(&self) -> Vec<crate::variant_spec::VariantMetadata> {
                vec![
                    #(#variant_metadata_calls),*
                ]
            }
        }
    };

    Ok(expanded)
}

fn is_static_str_type(ty: &Type) -> bool {
    if let Type::Reference(r) = ty {
        // Check if it's &str or &'static str
        matches!(&*r.elem, Type::Path(p) if p.path.is_ident("str"))
    } else {
        false
    }
}

fn extract_option_inner_type(ty: &Type) -> Option<&Type> {
    // Check if the type is Option<T> and extract T
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}
