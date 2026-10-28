// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, spanned::Spanned, Data, DeriveInput, Expr, Fields, Type};

mod literal;

/// Compile JSON literals to a validated, compact `&'static str`.
#[proc_macro]
pub fn const_json(input: TokenStream) -> TokenStream {
    let literal = parse_macro_input!(input as literal::JsonLiteral);
    let output = syn::LitStr::new(&literal.0.to_string(), Span::call_site());
    quote!(#output).into()
}

#[proc_macro_derive(ConstJson, attributes(const_json))]
pub fn derive_const_json(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_const_json(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_const_json(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = input.generics.clone();
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    data.struct_token.span,
                    "ConstJson supports structs with named fields only",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new(
                input.span(),
                "ConstJson can only be derived for structs",
            ))
        }
    };

    let capacity_expr = parse_capacity(&input.attrs)?;

    let mut field_tokens = Vec::new();
    let mut counter: usize = 0;
    for (idx, field) in fields.iter().enumerate() {
        let ident = field.ident.as_ref().ok_or_else(|| {
            syn::Error::new(
                field.span(),
                "Unnamed fields are not supported by ConstJson",
            )
        })?;
        let key = ident.to_string();
        let field_expr = quote! { self.#ident };
        let writer = build_value_writer(&field_expr, &field.ty, &mut counter)?;

        let separator = if idx > 0 {
            quote! { const_json::ser::field_separator(&mut buffer); }
        } else {
            quote! {}
        };

        field_tokens.push(quote! {
            #separator
            const_json::ser::write_key(&mut buffer, #key);
            #writer
        });
    }

    let expanded = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const fn to_json(&self) -> const_json::JsonBuffer<{ #capacity_expr }> {
                let mut buffer = const_json::JsonBuffer::<{ #capacity_expr }>::new();
                const_json::ser::begin_object(&mut buffer);
                #(#field_tokens)*
                const_json::ser::end_object(&mut buffer);
                buffer
            }
        }
    };

    Ok(expanded)
}

fn parse_capacity(attrs: &[syn::Attribute]) -> syn::Result<Expr> {
    let mut capacity: Option<Expr> = None;
    for attr in attrs {
        if !attr.path().is_ident("const_json") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("capacity") {
                let expr: Expr = meta.value()?.parse()?;
                if capacity.is_some() {
                    return Err(meta.error("duplicate capacity attribute"));
                }
                capacity = Some(expr);
                Ok(())
            } else {
                Err(meta.error("unsupported const_json attribute"))
            }
        })?;
    }

    capacity
        .ok_or_else(|| syn::Error::new(Span::call_site(), "missing #[const_json(capacity = ...)]"))
}

fn build_value_writer(
    expr: &TokenStream2,
    ty: &Type,
    counter: &mut usize,
) -> syn::Result<TokenStream2> {
    let expr_wrapped = quote! { (#expr) };
    match ty {
        Type::Path(path) => {
            if path.qself.is_some() {
                return Err(syn::Error::new(
                    ty.span(),
                    "ConstJson does not support qualified paths",
                ));
            }

            let segment = path
                .path
                .segments
                .last()
                .ok_or_else(|| syn::Error::new(ty.span(), "Unsupported path"))?;

            if !matches!(segment.arguments, syn::PathArguments::None) {
                return Err(syn::Error::new(
                    segment.arguments.span(),
                    "ConstJson does not support generic field types",
                ));
            }

            match segment.ident.to_string().as_str() {
                "bool" => Ok(quote! { const_json::ser::write_bool(&mut buffer, #expr_wrapped); }),
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Ok(
                    quote! { const_json::ser::write_unsigned(&mut buffer, #expr_wrapped as u128); },
                ),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => Ok(
                    quote! { const_json::ser::write_signed(&mut buffer, #expr_wrapped as i128); },
                ),
                "str" => Ok(quote! { const_json::ser::write_string(&mut buffer, #expr_wrapped); }),
                _ => {
                    let buf_ident = fresh_ident(counter, "nested_buf");
                    let bytes_ident = fresh_ident(counter, "nested_bytes");
                    let idx_ident = fresh_ident(counter, "nested_idx");
                    Ok(quote! {
                        {
                            let #buf_ident = #expr_wrapped.to_json();
                            let #bytes_ident = #buf_ident.as_bytes();
                            let mut #idx_ident: usize = 0;
                            while #idx_ident < #bytes_ident.len() {
                                buffer.push_byte(#bytes_ident[#idx_ident]);
                                #idx_ident += 1;
                            }
                        }
                    })
                }
            }
        }
        Type::Reference(reference) => match &*reference.elem {
            Type::Path(path) if path.path.is_ident("str") => {
                Ok(quote! { const_json::ser::write_string(&mut buffer, #expr_wrapped); })
            }
            _ => Err(syn::Error::new(
                ty.span(),
                "ConstJson string fields must be &str",
            )),
        },
        Type::Array(array) => {
            let arr_ident = fresh_ident(counter, "arr");
            let len_ident = fresh_ident(counter, "len");
            let idx_ident = fresh_ident(counter, "idx");
            let element_expr = quote! { #arr_ident[#idx_ident] };
            let element_writer = build_value_writer(&element_expr, &array.elem, counter)?;
            Ok(quote! {
                {
                    let #arr_ident = #expr_wrapped;
                    let #len_ident = #arr_ident.len();
                    const_json::ser::begin_array(&mut buffer);
                    let mut #idx_ident: usize = 0;
                    while #idx_ident < #len_ident {
                        if #idx_ident > 0 {
                            const_json::ser::field_separator(&mut buffer);
                        }
                        #element_writer
                        #idx_ident += 1;
                    }
                    const_json::ser::end_array(&mut buffer);
                }
            })
        }
        _ => Err(syn::Error::new(
            ty.span(),
            "ConstJson does not support this field type",
        )),
    }
}

fn fresh_ident(counter: &mut usize, prefix: &str) -> syn::Ident {
    *counter += 1;
    syn::Ident::new(
        &format!("__const_json_{}{}", prefix, counter),
        Span::call_site(),
    )
}
