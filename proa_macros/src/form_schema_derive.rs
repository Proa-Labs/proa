// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Legacy derive macro for the deprecated `proa_ir::FormSchema` trait.
//!
//! It remains available for source compatibility. New applications should use
//! native forms and validate input in their handler or service layer; schema
//! integrations can be built downstream without being a Proa dependency.
//!
//! Generates type-safe form field access like React Hook Form's register() function.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

pub fn derive_form_schema(input: DeriveInput) -> TokenStream {
    let name = &input.ident;

    // Parse struct fields
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        name,
                        "FormSchema can only be derived for structs with named fields",
                    )
                    .to_compile_error(),
                );
            }
        },
        _ => {
            return TokenStream::from(
                syn::Error::new_spanned(name, "FormSchema can only be derived for structs")
                    .to_compile_error(),
            );
        }
    };

    // Parse the #[form(id = "...")] attribute
    let form_id = get_form_id_from_attrs(&input.attrs).unwrap_or_else(|| {
        // Default to struct name in snake_case
        to_snake_case(&name.to_string())
    });

    // Collect field info
    let mut field_infos: Vec<FieldInfo> = Vec::new();

    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();

        // Parse field attributes like #[validate(required, email)]
        let rules = parse_validate_attrs(&field.attrs);

        field_infos.push(FieldInfo {
            name: field_name.clone(),
            name_str: field_name_str,
            rules,
        });
    }

    let field_name_strs: Vec<&String> = field_infos.iter().map(|f| &f.name_str).collect();

    // Generate validation rules for each field
    let field_rules_match_arms: Vec<_> = field_infos
        .iter()
        .map(|f| {
            let name_str = &f.name_str;
            let rules = &f.rules;
            quote! {
                #name_str => vec![#(#rules),*]
            }
        })
        .collect();

    // Generate the FieldAccessor struct - a simple struct with &'static str fields
    let accessor_name = Ident::new(&format!("{}Fields", name), Span::call_site());

    // Generate field accessors that return the field name
    let field_accessor_fields: Vec<_> = field_infos
        .iter()
        .map(|f| {
            let field_name = &f.name;
            quote! {
                pub #field_name: &'static str
            }
        })
        .collect();

    let field_accessor_init: Vec<_> = field_infos
        .iter()
        .map(|f| {
            let field_name = &f.name;
            let field_name_str = &f.name_str;
            quote! {
                #field_name: #field_name_str
            }
        })
        .collect();

    // Generate the form handle wrapper name
    let handle_name = Ident::new(&format!("{}Form", name), Span::call_site());

    // Generate specific field registration methods
    let field_register_methods: Vec<_> = field_infos
        .iter()
        .map(|f| {
            let method_name = Ident::new(&format!("register_{}", f.name), f.name.span());
            let field_name_str = &f.name_str;
            quote! {
                /// Register the field and get its props
                pub fn #method_name(&self) -> proa_ir::FieldRegistration {
                    let rules = <#name as proa_ir::FormSchema>::field_rules(#field_name_str);
                    proa_ir::FieldRegistration::new(#field_name_str).with_rules(rules)
                }
            }
        })
        .collect();

    // Generate specific field state methods
    let field_state_methods: Vec<_> = field_infos
        .iter()
        .map(|f| {
            let method_name = Ident::new(&format!("field_{}", f.name), f.name.span());
            let field_name_str = &f.name_str;
            quote! {
                /// Get state for the field
                pub fn #method_name(&self) -> proa_ir::FieldState {
                    proa_ir::FieldState::new(#field_name_str)
                }
            }
        })
        .collect();

    let expanded = quote! {
        /// Field accessor struct for type-safe field access
        #[allow(non_snake_case)]
        pub struct #accessor_name {
            #(#field_accessor_fields,)*
        }

        impl #accessor_name {
            /// Constant instance with field names
            pub const FIELDS: Self = Self {
                #(#field_accessor_init,)*
            };
        }

        /// Typed form handle wrapper with field-specific methods
        pub struct #handle_name {
            inner: proa_ir::FormHandle<#name>,
        }

        impl #handle_name {
            /// Create a new form handle
            pub fn new() -> Self {
                Self {
                    inner: proa_ir::FormHandle::new(#form_id),
                }
            }

            /// Set the validation mode
            pub fn mode(mut self, mode: proa_ir::ValidationMode) -> Self {
                self.inner = self.inner.mode(mode);
                self
            }

            /// Get form-level state (is_valid, is_submitting, etc.)
            pub fn form_state(&self) -> proa_ir::FormState {
                self.inner.form_state()
            }

            /// Get attributes to spread on the form element
            pub fn attrs(&self) -> proa_ir::FormAttrs {
                self.inner.attrs()
            }

            /// Get the form ID
            pub fn id(&self) -> &'static str {
                self.inner.id
            }

            #(#field_register_methods)*
            #(#field_state_methods)*
        }

        impl Default for #handle_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl proa_ir::FormFields for #name {
            fn field_name<F, R>(_f: F) -> &'static str
            where
                F: FnOnce(&Self) -> R,
            {
                // The closure-based field lookup is not supported by the derive
                // macro — use the generated `register_<fieldname>` methods on
                // FormHandle instead. This returns an empty string as an inert
                // fallback rather than panicking at runtime.
                ""
            }
        }

        impl proa_ir::FormSchema for #name {
            fn use_form() -> proa_ir::FormHandle<Self> {
                proa_ir::FormHandle::new(#form_id)
            }

            fn field_rules(name: &'static str) -> Vec<proa_ir::ValidationRule> {
                match name {
                    #(#field_rules_match_arms,)*
                    _ => vec![],
                }
            }
        }

        // Provide a way to get field names directly
        impl #name {
            /// Get the field accessor for type-safe field access
            pub const fn fields() -> #accessor_name {
                #accessor_name::FIELDS
            }

            /// List of all field names
            pub const FIELD_NAMES: &'static [&'static str] = &[#(#field_name_strs),*];

            /// Create a typed form handle
            pub fn form() -> #handle_name {
                #handle_name::new()
            }
        }
    };

    TokenStream::from(expanded)
}

struct FieldInfo {
    name: Ident,
    name_str: String,
    rules: Vec<proc_macro2::TokenStream>,
}

fn get_form_id_from_attrs(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("form") {
            continue;
        }

        let mut id = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                id = Some(lit.value());
            }
            Ok(())
        })
        .ok();

        if id.is_some() {
            return id;
        }
    }
    None
}

fn parse_validate_attrs(attrs: &[syn::Attribute]) -> Vec<proc_macro2::TokenStream> {
    let mut rules = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }

        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("required") {
                let message = parse_optional_message(&meta)
                    .unwrap_or_else(|| "This field is required".into());
                rules.push(quote! {
                    proa_ir::ValidationRule::required(#message)
                });
            } else if meta.path.is_ident("email") {
                let message = parse_optional_message(&meta)
                    .unwrap_or_else(|| "Please enter a valid email".into());
                rules.push(quote! {
                    proa_ir::ValidationRule::email(#message)
                });
            } else if meta.path.is_ident("min_length") {
                if let Some((min, message)) = parse_length_rule(&meta) {
                    rules.push(quote! {
                        proa_ir::ValidationRule::min_length(#min, #message)
                    });
                }
            } else if meta.path.is_ident("max_length") {
                if let Some((max, message)) = parse_length_rule(&meta) {
                    rules.push(quote! {
                        proa_ir::ValidationRule::max_length(#max, #message)
                    });
                }
            } else if meta.path.is_ident("pattern") {
                if let Some((pattern, message)) = parse_pattern_rule(&meta) {
                    rules.push(quote! {
                        proa_ir::ValidationRule::pattern(#pattern, #message)
                    });
                }
            }
            Ok(())
        });
    }

    rules
}

fn parse_optional_message(meta: &syn::meta::ParseNestedMeta) -> Option<String> {
    // Try to parse = "message"
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Token![=] = meta.input.parse().ok()?;
        let lit: syn::LitStr = meta.input.parse().ok()?;
        return Some(lit.value());
    }
    None
}

fn parse_length_rule(meta: &syn::meta::ParseNestedMeta) -> Option<(usize, String)> {
    // Parse (len) or (len, "message")
    use syn::parse::Parser;

    if !meta.input.peek(syn::token::Paren) {
        return None;
    }

    let group: proc_macro2::Group = meta.input.parse().ok()?;
    let tokens = group.stream();

    let parser = |input: syn::parse::ParseStream| -> syn::Result<(usize, String)> {
        let len: syn::LitInt = input.parse()?;
        let len_val = len.base10_parse()?;

        let msg = if input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            lit.value()
        } else {
            "Length requirement not met".to_string()
        };

        Ok((len_val, msg))
    };

    parser.parse2(tokens).ok()
}

fn parse_pattern_rule(meta: &syn::meta::ParseNestedMeta) -> Option<(String, String)> {
    // Parse ("pattern") or ("pattern", "message")
    use syn::parse::Parser;

    if !meta.input.peek(syn::token::Paren) {
        return None;
    }

    let group: proc_macro2::Group = meta.input.parse().ok()?;
    let tokens = group.stream();

    let parser = |input: syn::parse::ParseStream| -> syn::Result<(String, String)> {
        let pattern: syn::LitStr = input.parse()?;

        let msg = if input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            lit.value()
        } else {
            "Invalid format".to_string()
        };

        Ok((pattern.value(), msg))
    };

    parser.parse2(tokens).ok()
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}
