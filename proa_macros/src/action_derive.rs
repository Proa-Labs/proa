// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn derive_action(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return TokenStream::from(
                syn::Error::new_spanned(&input.ident, "Action can only be derived for enums")
                    .to_compile_error(),
            );
        }
    };

    // Parse the #[action(scope = "...")] attribute
    let scope = match get_scope_from_attrs(&input) {
        Ok(s) => s,
        Err(e) => return TokenStream::from(e.to_compile_error()),
    };

    let mut action_names = Vec::new();
    let mut action_constants = Vec::new();
    let mut action_params = Vec::new();
    let mut to_data_match_arms = Vec::new();
    let mut from_data_match_arms = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;
        let action_string = format!("{}.{}.{}", scope, name, variant_name);

        // Convert variant name to SCREAMING_SNAKE_CASE for constant
        let const_name_str = variant_name
            .to_string()
            .chars()
            .enumerate()
            .flat_map(|(i, c)| {
                if c.is_uppercase() && i > 0 {
                    vec!['_', c]
                } else {
                    vec![c]
                }
            })
            .collect::<String>()
            .to_uppercase();
        let const_name = syn::Ident::new(&const_name_str, variant_name.span());

        action_names.push(variant_name.clone());
        action_constants.push(quote! {
            pub const #const_name: &'static str = #action_string;
        });

        match &variant.fields {
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
                let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();

                let params_struct_name =
                    syn::Ident::new(&format!("{}Params", variant_name), variant_name.span());

                action_params.push(quote! {
                    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                    pub struct #params_struct_name {
                        #(pub #field_names: Option<#field_types>),*
                    }
                });

                to_data_match_arms.push(quote! {
                    #name::#variant_name { #(#field_names),* } => {
                        proa_actions::ActionData {
                            action: #action_string.to_string(),
                            params: serde_json::to_value(
                                #params_struct_name {
                                    #(#field_names: Some(#field_names.clone())),*
                                }
                            ).expect("Failed to serialize action params — check that all field types implement Serialize"),
                        }
                    }
                });

                from_data_match_arms.push(quote! {
                    #action_string => {
                        let params: #params_struct_name = serde_json::from_value(data.params)?;
                        Ok(#name::#variant_name {
                            #(#field_names: params.#field_names.ok_or_else(|| {
                                serde_json::Error::custom(format!("Missing field: {}", stringify!(#field_names)))
                            })?),*
                        })
                    }
                });
            }
            Fields::Unit => {
                to_data_match_arms.push(quote! {
                    #name::#variant_name => {
                        proa_actions::ActionData {
                            action: #action_string.to_string(),
                            params: serde_json::json!({}),
                        }
                    }
                });

                from_data_match_arms.push(quote! {
                    #action_string => Ok(#name::#variant_name),
                });
            }
            Fields::Unnamed(_) => {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        &variant.ident,
                        "Action variants with unnamed fields are not supported. Use named fields instead.",
                    )
                    .to_compile_error(),
                );
            }
        }
    }

    let expanded = quote! {
        impl #name {
            #(#action_constants)*
        }

        #(#action_params)*

        impl proa_actions::ActionHandler for #name {
            fn to_action_data(&self) -> proa_actions::ActionData {
                match self {
                    #(#to_data_match_arms)*
                }
            }

            fn from_action_data(data: proa_actions::ActionData) -> Result<Self, serde_json::Error>
            where
                Self: Sized
            {
                use serde::de::Error as _;

                match data.action.as_str() {
                    #(#from_data_match_arms)*
                    _ => Err(serde_json::Error::custom(format!("Unknown action: {}", data.action))),
                }
            }
        }
    };

    TokenStream::from(expanded)
}

fn get_scope_from_attrs(input: &DeriveInput) -> Result<String, syn::Error> {
    for attr in &input.attrs {
        if !attr.path().is_ident("action") {
            continue;
        }

        let mut scope = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("scope") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                scope = Some(lit.value());
            }
            Ok(())
        })
        .ok();

        if let Some(scope) = scope {
            if scope.is_empty() {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "Action scope cannot be empty. Use #[action(scope = \"your_scope_name\")]",
                ));
            }
            // Validate scope format: only alphanumeric, underscore, hyphen
            if !scope
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    format!(
                        "Action scope must contain only alphanumeric characters, underscores, or hyphens. Got: \"{}\"",
                        scope
                    ),
                ));
            }
            return Ok(scope);
        }
    }

    Err(syn::Error::new_spanned(
        &input.ident,
        "Action requires #[action(scope = \"...\")] attribute to prevent naming collisions.\n\
         Example: #[derive(Action)] #[action(scope = \"home_page\")] enum MyAction { ... }\n\
         The scope should uniquely identify this action enum across your entire application.",
    ))
}
