// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `#[derive(rsjs::ClientProps)]` — generate a `ClientEncode` impl,
//! a `ClientDecode` impl when the fields are owned/decodeable, plus a sibling
//! `__rsjs_codec_meta_<TypeIdent>` const for a user struct.
//!
//! See the module-level docs in `rsjs/src/client_codec.rs` for the
//! wire format. This file is the proc-macro frontend: it parses the
//! `DeriveInput`, classifies each field's type into the closed
//! [`FieldKind`] vocabulary, then expands to:
//!
//! 1. `impl rsjs::ClientEncode for <T>` — hand-written JSON object
//!    output, fields in declaration order, using the per-primitive
//!    `ClientEncode` impls in `rsjs::client_codec`.
//! 2. `impl rsjs::ClientDecode for <T>` when every field can be
//!    decoded into an owned Rust value — a small per-struct JSON
//!    parser that scans `{`, then for each field expects
//!    `"<name>":<value>` (unknown keys error), then `}`.
//! 3. `pub const __rsjs_codec_meta_<T>: &'static rsjs::ClientPropsMeta`
//!    carrying the top-level wire shape, field names, and closed-vocabulary
//!    field types so
//!    downstream proc macros (M7e/M7g/M7h) can introspect the codec
//!    by Rust name resolution.
//!
//! Named-field structs and fieldless enums are supported. Tuple structs, unit
//! structs, unions, and data-carrying enums are rejected with span-pointed
//! errors.
//! Borrowed fields such as `&'static str` automatically emit
//! `ClientEncode` and metadata but skip `ClientDecode`, because
//! browser JSON cannot soundly recreate Rust borrows.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Field, Fields, GenericArgument, GenericParam,
    PathArguments, Type,
};

/// Closed-vocabulary classification of a field's Rust type.
///
/// Mirrors `rsjs::ClientPropsFieldType` exactly. We carry the `syn`
/// types we need to refer to from the generated code so the emitted
/// tokens use the user's spelling (preserving spans for "the type
/// you wrote here doesn't have a `ClientEncode` impl" errors when
/// the inner type is an unknown path).
pub(crate) enum FieldKind {
    Bool,
    /// Narrow signed: `i8`/`i16`/`i32`. Encoded as JSON number.
    NarrowSigned,
    /// Narrow unsigned: `u8`/`u16`/`u32`. Encoded as JSON number.
    NarrowUnsigned,
    /// Wide signed: `i64`/`i128`/`isize`. Encoded as JSON string.
    WideSigned,
    /// Wide unsigned: `u64`/`u128`/`usize`. Encoded as JSON string.
    WideUnsigned,
    F32,
    F64,
    String {
        borrowed: bool,
    },
    /// `Option<inner>`. v0.1 rejects nested options.
    OptionOf(Box<FieldKind>),
    /// `Vec<inner>`.
    VecOf(Box<FieldKind>),
    /// Borrowed slice `&[inner]`. Encodes like a JSON array but cannot
    /// participate in Rust-side `ClientDecode`.
    SliceOf(Box<FieldKind>),
    /// Any other user type path. Assumed to be another
    /// `ClientProps`-derived type; the generated code references
    /// the type's sibling metadata const by Rust name resolution.
    /// Carries the original `syn::Path` so we can name it back in
    /// the metadata literal.
    NestedPath {
        path: syn::Path,
        borrowed: bool,
    },
}

impl FieldKind {
    pub(crate) fn supports_decode(&self) -> bool {
        match self {
            Self::String { borrowed } | Self::NestedPath { borrowed, .. } => !borrowed,
            Self::SliceOf(_) => false,
            Self::OptionOf(inner) | Self::VecOf(inner) => inner.supports_decode(),
            Self::Bool
            | Self::NarrowSigned
            | Self::NarrowUnsigned
            | Self::WideSigned
            | Self::WideUnsigned
            | Self::F32
            | Self::F64 => true,
        }
    }
}

/// Variant tag used inside `quote!` to construct the runtime
/// `ClientPropsFieldType` literal. Returns a `TokenStream` so we can
/// pass through nested types recursively.
fn field_kind_to_meta_tokens(kind: &FieldKind) -> TokenStream {
    match kind {
        FieldKind::Bool => quote! { ::rsjs::ClientPropsFieldType::Bool },
        FieldKind::NarrowSigned => quote! { ::rsjs::ClientPropsFieldType::I32 },
        FieldKind::NarrowUnsigned => quote! { ::rsjs::ClientPropsFieldType::U32 },
        FieldKind::WideSigned => quote! { ::rsjs::ClientPropsFieldType::I64 },
        FieldKind::WideUnsigned => quote! { ::rsjs::ClientPropsFieldType::U64 },
        FieldKind::F32 => quote! { ::rsjs::ClientPropsFieldType::F32 },
        FieldKind::F64 => quote! { ::rsjs::ClientPropsFieldType::F64 },
        FieldKind::String { .. } => quote! { ::rsjs::ClientPropsFieldType::String },
        FieldKind::OptionOf(inner) => {
            let inner_tokens = field_kind_to_meta_tokens(inner);
            quote! {
                ::rsjs::ClientPropsFieldType::OptionOf(&#inner_tokens)
            }
        }
        FieldKind::VecOf(inner) => {
            let inner_tokens = field_kind_to_meta_tokens(inner);
            quote! {
                ::rsjs::ClientPropsFieldType::VecOf(&#inner_tokens)
            }
        }
        FieldKind::SliceOf(inner) => {
            let inner_tokens = field_kind_to_meta_tokens(inner);
            quote! {
                ::rsjs::ClientPropsFieldType::VecOf(&#inner_tokens)
            }
        }
        FieldKind::NestedPath { path, .. } => {
            // Reference the nested type's sibling metadata const by
            // Rust name resolution: the const lives in the same
            // module as the type, named __rsjs_codec_meta_<Ident>.
            // We reconstruct the path by replacing the last segment
            // with the metadata identifier.
            let nested_path = nested_meta_path(path);
            quote! {
                ::rsjs::ClientPropsFieldType::NestedClientProps(#nested_path)
            }
        }
    }
}

/// Build the path `<module-prefix-of-path>::__rsjs_codec_meta_<Ident>`
/// from a user-supplied type path. For `crate::a::Foo`, this yields
/// `crate::a::__rsjs_codec_meta_Foo`. For a bare `Foo`, this yields
/// `__rsjs_codec_meta_Foo` (which Rust's name resolution will look
/// up in the current module).
fn nested_meta_path(path: &syn::Path) -> syn::Path {
    let mut new_path = path.clone();
    if let Some(last) = new_path.segments.last_mut() {
        let ident = format_ident!("__rsjs_codec_meta_{}", last.ident);
        last.ident = ident;
        last.arguments = PathArguments::None;
    }
    new_path
}

fn validate_generics(input: &DeriveInput) -> Result<(), syn::Error> {
    for param in &input.generics.params {
        if !matches!(param, GenericParam::Lifetime(_)) {
            return Err(syn::Error::new(
                param.span(),
                "#[derive(rsjs::ClientProps)] supports lifetime parameters for borrowed \
                 Rust-to-JS props, but type and const generics are not supported yet; \
                 generic shapes cannot be represented in ClientProps metadata yet",
            ));
        }
    }
    Ok(())
}

/// Top-level entry point. Returns a `TokenStream` that's either the
/// expanded impl or a `compile_error!` invocation.
pub fn derive_client_props(input: TokenStream) -> TokenStream {
    let input = match syn::parse2::<DeriveInput>(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    match expand_client_props(&input) {
        Ok(t) => t,
        Err(e) => e.to_compile_error(),
    }
}

pub(crate) fn expand_client_props(input: &DeriveInput) -> Result<TokenStream, syn::Error> {
    match &input.data {
        Data::Struct(data) => expand_struct(input, data),
        Data::Enum(data) => expand_enum(input, data),
        Data::Union(u) => Err(syn::Error::new(
            u.union_token.span,
            "#[derive(rsjs::ClientProps)] supports named-field structs and fieldless enums; \
                 unions are not supported",
        )),
    }
}

fn expand_struct(input: &DeriveInput, data: &DataStruct) -> Result<TokenStream, syn::Error> {
    validate_generics(input)?;

    let named = match &data.fields {
        Fields::Named(named) => named,
        Fields::Unnamed(unnamed) => {
            // Point at the first unnamed field if present, else the
            // parens themselves.
            let span = unnamed
                .unnamed
                .first()
                .map(|f| f.span())
                .unwrap_or_else(|| unnamed.paren_token.span.span());
            return Err(syn::Error::new(
                span,
                "#[derive(rsjs::ClientProps)] requires named fields; \
                 tuple structs are not supported in v0.1",
            ));
        }
        Fields::Unit => {
            return Err(syn::Error::new(
                input.ident.span(),
                "#[derive(rsjs::ClientProps)] requires a struct with named fields; \
                 unit structs are not supported",
            ));
        }
    };

    let struct_ident = &input.ident;
    let struct_name_lit = struct_ident.to_string();
    let meta_ident = format_ident!("__rsjs_codec_meta_{}", struct_ident);
    // Round-3 review #6: the metadata const is always emitted as
    // `pub`, regardless of the user's struct visibility. Nested
    // `ClientProps` resolution looks the const up across modules
    // (`<other_module>::__rsjs_codec_meta_<Inner>`); when the user's
    // struct is private (or `pub(crate)`), mirroring its visibility
    // would make the const unreachable from the consuming island's
    // module. The const is `#[doc(hidden)]` to keep it out of public
    // rustdoc, and the leading `__rsjs_codec_meta_` namespace makes
    // accidental external references unlikely.
    let _ = &input.vis;

    // Per-field state used by the encode body, the decode body, and
    // the metadata literal.
    struct FieldInfo<'a> {
        ident: &'a syn::Ident,
        name_str: String,
        kind: FieldKind,
        ty: &'a Type,
    }

    let mut fields: Vec<FieldInfo<'_>> = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let kind = classify_field_type(field)?;
        let ident = field.ident.as_ref().expect("named fields have idents");
        let name_str = ident.to_string();
        fields.push(FieldInfo {
            ident,
            name_str,
            kind,
            ty: &field.ty,
        });
    }

    // ----- ClientEncode body --------------------------------------
    // We hand-emit `{"f1":<v1>,"f2":<v2>,...}` directly into the
    // output buffer, calling each field's `ClientEncode::encode_into`
    // at the right slot. `write_json_string` handles JSON-escaping
    // the field name.
    let encode_field_writes = fields.iter().enumerate().map(|f| {
        let (idx, info) = f;
        let ident = info.ident;
        let name_lit = &info.name_str;
        let ty = info.ty;
        let leading_comma = if idx == 0 {
            quote! {}
        } else {
            quote! { __out.push(','); }
        };
        // Use a fully-qualified UFCS call so the bound is checked at
        // the field's span. If the user's field type doesn't impl
        // ClientEncode, the error points at the field's `ty`.
        quote_spanned! { ty.span() =>
            #leading_comma
            ::rsjs::__codec_internals::write_json_string(__out, #name_lit);
            __out.push(':');
            <#ty as ::rsjs::ClientEncode>::encode_into(&self.#ident, __out);
        }
    });

    // ----- ClientDecode body --------------------------------------
    // Strategy (v0.1): expect `{`, then loop reading
    // `"key":value(,)?` entries until `}`. Track each declared
    // field's seen-state in a small set of `Option<T>` slots; reject
    // unknown keys; reject duplicates; require all fields present at
    // the end.
    let field_slot_decls = fields.iter().map(|info| {
        let slot_ident = format_ident!("__slot_{}", info.ident);
        let ty = info.ty;
        quote! {
            let mut #slot_ident: ::core::option::Option<#ty> = ::core::option::Option::None;
        }
    });

    let key_match_arms = fields.iter().map(|info| {
        let slot_ident = format_ident!("__slot_{}", info.ident);
        let name_lit = &info.name_str;
        let ty = info.ty;
        quote_spanned! { info.ty.span() =>
            #name_lit => {
                if #slot_ident.is_some() {
                    return ::core::result::Result::Err(::rsjs::ClientDecodeError {
                        msg: concat!("duplicate field '", #name_lit, "' in JSON object"),
                    });
                }
                let (__v, __used) =
                    <#ty as ::rsjs::ClientDecode>::decode_partial(&__s[__i..])?;
                #slot_ident = ::core::option::Option::Some(__v);
                __i += __used;
            }
        }
    });

    let field_takes = fields.iter().map(|info| {
        let slot_ident = format_ident!("__slot_{}", info.ident);
        let ident = info.ident;
        let name_lit = &info.name_str;
        quote! {
            #ident: match #slot_ident {
                ::core::option::Option::Some(v) => v,
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(::rsjs::ClientDecodeError {
                        msg: concat!("missing field '", #name_lit, "' in JSON object"),
                    });
                }
            }
        }
    });

    // ----- Metadata sibling const --------------------------------
    let meta_field_entries = fields.iter().map(|info| {
        let name_lit = &info.name_str;
        let kind_tokens = field_kind_to_meta_tokens(&info.kind);
        quote! {
            ::rsjs::ClientPropsField {
                name: #name_lit,
                ty: &#kind_tokens,
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let should_emit_decode =
        input.generics.params.is_empty() && fields.iter().all(|field| field.kind.supports_decode());
    let decode_impl = if should_emit_decode {
        quote! {
            #[automatically_derived]
            #[allow(unused_mut, unused_variables, unreachable_code)]
            impl #impl_generics ::rsjs::ClientDecode for #struct_ident #ty_generics #where_clause {
                fn decode_partial(
                    __s: &str,
                ) -> ::core::result::Result<(Self, usize), ::rsjs::ClientDecodeError> {
                    let mut __i = ::rsjs::__codec_internals::skip_ws(__s, 0);
                    __i = ::rsjs::__codec_internals::expect_byte(__s, __i, b'{')?;
                    __i = ::rsjs::__codec_internals::skip_ws(__s, __i);

                    #(#field_slot_decls)*

                    if __s.as_bytes().get(__i) == ::core::option::Option::Some(&b'}') {
                        __i += 1;
                    } else {
                        loop {
                            let (__key, __after_key) =
                                ::rsjs::__codec_internals::parse_json_string(__s, __i)?;
                            __i = ::rsjs::__codec_internals::skip_ws(__s, __after_key);
                            __i = ::rsjs::__codec_internals::expect_byte(__s, __i, b':')?;
                            __i = ::rsjs::__codec_internals::skip_ws(__s, __i);

                            match __key.as_str() {
                                #(#key_match_arms)*
                                _ => {
                                    return ::core::result::Result::Err(::rsjs::ClientDecodeError {
                                        msg: "unknown field in JSON object",
                                    });
                                }
                            }

                            __i = ::rsjs::__codec_internals::skip_ws(__s, __i);
                            match __s.as_bytes().get(__i) {
                                ::core::option::Option::Some(&b',') => {
                                    __i = ::rsjs::__codec_internals::skip_ws(__s, __i + 1);
                                }
                                ::core::option::Option::Some(&b'}') => {
                                    __i += 1;
                                    break;
                                }
                                _ => {
                                    return ::core::result::Result::Err(::rsjs::ClientDecodeError {
                                        msg: "expected ',' or '}' in JSON object",
                                    });
                                }
                            }
                        }
                    }

                    let __value = #struct_ident {
                        #(#field_takes),*
                    };
                    ::core::result::Result::Ok((__value, __i))
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[automatically_derived]
        #[allow(unused_mut, unused_variables, unreachable_code)]
        impl #impl_generics ::rsjs::ClientEncode for #struct_ident #ty_generics #where_clause {
            fn encode_into(&self, __out: &mut ::std::string::String) {
                __out.push('{');
                #(#encode_field_writes)*
                __out.push('}');
            }
        }

        #decode_impl

        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        #[doc(hidden)]
        pub const #meta_ident: &'static ::rsjs::ClientPropsMeta = &::rsjs::ClientPropsMeta {
            name: #struct_name_lit,
            shape: ::rsjs::ClientPropsShape::Object,
            fields: &[
                #(#meta_field_entries),*
            ],
        };
    };

    Ok(expanded)
}

fn expand_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream, syn::Error> {
    validate_generics(input)?;

    let mut field_error: Option<syn::Error> = None;
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            let error = syn::Error::new(
                variant.fields.span(),
                format!(
                    "#[derive(rsjs::ClientProps)] supports fieldless enums only; \
                     variant `{}` carries data",
                    variant.ident
                ),
            );
            if let Some(errors) = &mut field_error {
                errors.combine(error);
            } else {
                field_error = Some(error);
            }
        }
    }
    if let Some(error) = field_error {
        return Err(error);
    }

    let enum_ident = &input.ident;
    let enum_name_lit = enum_ident.to_string();
    let meta_ident = format_ident!("__rsjs_codec_meta_{}", enum_ident);
    let variant_idents = data
        .variants
        .iter()
        .map(|variant| &variant.ident)
        .collect::<Vec<_>>();
    let variant_tags = variant_idents
        .iter()
        .map(|variant| variant.unraw().to_string())
        .collect::<Vec<_>>();
    let encode_arms = variant_idents
        .iter()
        .zip(&variant_tags)
        .map(|(variant, tag)| {
            quote! {
                Self::#variant => {
                    ::rsjs::__codec_internals::write_json_string(__out, #tag);
                }
            }
        });
    let decode_arms = variant_idents
        .iter()
        .zip(&variant_tags)
        .map(|(variant, tag)| {
            quote! {
                #tag => ::core::result::Result::Ok((Self::#variant, __end)),
            }
        });
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::rsjs::ClientEncode for #enum_ident #ty_generics #where_clause {
            fn encode_into(&self, __out: &mut ::std::string::String) {
                match self {
                    #(#encode_arms)*
                }
            }
        }

        #[automatically_derived]
        impl #impl_generics ::rsjs::ClientDecode for #enum_ident #ty_generics #where_clause {
            fn decode_partial(
                __s: &str,
            ) -> ::core::result::Result<(Self, usize), ::rsjs::ClientDecodeError> {
                let __start = ::rsjs::__codec_internals::skip_ws(__s, 0);
                let (__tag, __end) =
                    ::rsjs::__codec_internals::parse_json_string(__s, __start)?;
                match __tag.as_str() {
                    #(#decode_arms)*
                    _ => ::core::result::Result::Err(::rsjs::ClientDecodeError {
                        msg: concat!("unknown fieldless enum tag for `", #enum_name_lit, "`"),
                    }),
                }
            }
        }

        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        #[doc(hidden)]
        pub const #meta_ident: &'static ::rsjs::ClientPropsMeta = &::rsjs::ClientPropsMeta {
            name: #enum_name_lit,
            shape: ::rsjs::ClientPropsShape::TaggedString {
                variants: &[#(#variant_tags),*],
            },
            fields: &[],
        };
    })
}

/// Classify `field.ty` into the closed [`FieldKind`] vocabulary.
///
/// Crate-visible so `#[derive(rsjs::SharedState)]` can pre-check that every
/// field round-trips (decode support) with a field-pointed diagnostic before
/// reusing this module's codec expansion.
pub(crate) fn classify_field_type(field: &Field) -> Result<FieldKind, syn::Error> {
    classify_type(&field.ty)
}

fn classify_type(ty: &Type) -> Result<FieldKind, syn::Error> {
    let path = match ty {
        Type::Path(tp) if tp.qself.is_none() => &tp.path,
        Type::Reference(reference) => return classify_reference_type(reference),
        _ => {
            return Err(syn::Error::new(
                ty.span(),
                "#[derive(rsjs::ClientProps)] cannot encode this field type; \
                 expected a primitive (`bool`, integer, float, `String`), \
                 `Option<T>`, `Vec<T>`, or another ClientProps-derived type",
            ));
        }
    };

    // Single-segment idents: primitives, `String`, or generic
    // `Option<T>` / `Vec<T>`.
    if path.segments.len() == 1 {
        let seg = &path.segments[0];
        let name = seg.ident.to_string();

        // No-args primitives.
        if matches!(seg.arguments, PathArguments::None) {
            return match name.as_str() {
                "bool" => Ok(FieldKind::Bool),
                "i8" | "i16" | "i32" => Ok(FieldKind::NarrowSigned),
                "u8" | "u16" | "u32" => Ok(FieldKind::NarrowUnsigned),
                "i64" | "i128" | "isize" => Ok(FieldKind::WideSigned),
                "u64" | "u128" | "usize" => Ok(FieldKind::WideUnsigned),
                "f32" => Ok(FieldKind::F32),
                "f64" => Ok(FieldKind::F64),
                "String" => Ok(FieldKind::String { borrowed: false }),
                _ => Ok(FieldKind::NestedPath {
                    path: path.clone(),
                    borrowed: false,
                }),
            };
        }

        // Generics: `Option<T>` and `Vec<T>` (one nesting only).
        if name == "Option" {
            let inner = single_generic_arg(seg, "Option")?;
            let inner_kind = classify_type(inner)?;
            if matches!(inner_kind, FieldKind::OptionOf(_)) {
                return Err(syn::Error::new(
                    inner.span(),
                    "#[derive(rsjs::ClientProps)] does not support nested `Option<Option<T>>` in v0.1",
                ));
            }
            return Ok(FieldKind::OptionOf(Box::new(inner_kind)));
        }
        if name == "Vec" {
            let inner = single_generic_arg(seg, "Vec")?;
            let inner_kind = classify_type(inner)?;
            return Ok(FieldKind::VecOf(Box::new(inner_kind)));
        }

        // Any other path with generic arguments is unsupported in
        // v0.1 (we only know how to lower `Option<T>` and `Vec<T>`,
        // and arbitrary user types that are themselves
        // `ClientProps`-derived must be non-generic).
        if !matches!(seg.arguments, PathArguments::None) {
            return Err(syn::Error::new(
                ty.span(),
                "#[derive(rsjs::ClientProps)] cannot encode this field type; \
                 generic types other than `Option<T>` and `Vec<T>` are not supported in v0.1",
            ));
        }

        // Otherwise treat as a nested ClientProps-derived path.
        return Ok(FieldKind::NestedPath {
            path: path.clone(),
            borrowed: false,
        });
    }

    // Multi-segment paths: assumed to be a ClientProps-derived type
    // in another module. We can't classify generics here, so reject
    // any path with non-empty arguments on its last segment (apart
    // from the special-cased Option/Vec above which were
    // single-segment).
    let last = path.segments.last().expect("non-empty path");
    if !matches!(last.arguments, PathArguments::None) {
        return Err(syn::Error::new(
            ty.span(),
            "#[derive(rsjs::ClientProps)] cannot encode this field type; \
             generic paths other than `Option<T>` and `Vec<T>` are not supported",
        ));
    }

    // If the last segment names a v0.1 primitive (`String`, `bool`,
    // `i32`, ...), require the path to be rooted at `core` / `std` /
    // `alloc`. Otherwise `my_crate::String` would pass through here
    // as a `NestedPath` and the JSON wire format would mis-encode it
    // (the user-defined type would be looked up via its custom
    // ClientPropsCodec impl, but if no such impl exists the failure
    // mode is a confusing trait-bound error rather than a clear
    // "you cannot use a non-std primitive name here" message).
    let last_name = last.ident.to_string();
    if matches!(
        last_name.as_str(),
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "String"
    ) {
        let first = &path
            .segments
            .first()
            .expect("multi-segment path has a first segment")
            .ident;
        if first != "core" && first != "std" && first != "alloc" {
            let path_str = quote::quote! { #path }.to_string().replace(' ', "");
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "v0.1 of #[derive(rsjs::ClientProps)] only accepts primitive \
                     types from `core`, `std`, or `alloc` (and the \
                     `std::collections::Vec` / `std::option::Option` containers); \
                     got `{path_str}`. Rename the field to use the std primitive directly \
                     or add a custom ClientPropsCodec impl for the user type.",
                ),
            ));
        }
    }

    Ok(FieldKind::NestedPath {
        path: path.clone(),
        borrowed: false,
    })
}

fn classify_reference_type(reference: &syn::TypeReference) -> Result<FieldKind, syn::Error> {
    if reference.mutability.is_some() {
        return Err(syn::Error::new(
            reference.span(),
            "#[derive(rsjs::ClientProps)] cannot encode mutable references; \
             use an immutable borrowed field or an owned value",
        ));
    }

    match reference.elem.as_ref() {
        Type::Path(tp) if tp.qself.is_none() => classify_borrowed_path(&tp.path),
        Type::Slice(slice) => {
            let inner = classify_type(&slice.elem)?;
            Ok(FieldKind::SliceOf(Box::new(inner)))
        }
        _ => Err(syn::Error::new(
            reference.elem.span(),
            "#[derive(rsjs::ClientProps)] cannot encode this borrowed field type; \
             supported borrowed fields are `&str`, `&String`, `&[T]`, or `&ClientPropsType`",
        )),
    }
}

fn classify_borrowed_path(path: &syn::Path) -> Result<FieldKind, syn::Error> {
    if path.segments.len() == 1 {
        let seg = &path.segments[0];
        let name = seg.ident.to_string();
        if matches!(seg.arguments, PathArguments::None) {
            return match name.as_str() {
                "str" | "String" => Ok(FieldKind::String { borrowed: true }),
                _ => Ok(FieldKind::NestedPath {
                    path: path.clone(),
                    borrowed: true,
                }),
            };
        }
    }

    let last = path.segments.last().expect("non-empty path");
    if !matches!(last.arguments, PathArguments::None) {
        return Err(syn::Error::new(
            path.span(),
            "#[derive(rsjs::ClientProps)] cannot encode this borrowed generic path; \
             use `&[T]` for borrowed collections or an owned `Vec<T>`",
        ));
    }

    let last_name = last.ident.to_string();
    if matches!(last_name.as_str(), "str" | "String") {
        let first = &path
            .segments
            .first()
            .expect("multi-segment path has a first segment")
            .ident;
        if first == "std" || first == "alloc" || first == "core" {
            return Ok(FieldKind::String { borrowed: true });
        }
    }

    Ok(FieldKind::NestedPath {
        path: path.clone(),
        borrowed: true,
    })
}

/// Extract the single type argument of a generic segment such as
/// `Option<T>` or `Vec<T>`. Errors with a span at the segment if the
/// shape doesn't match.
fn single_generic_arg<'a>(seg: &'a syn::PathSegment, name: &str) -> Result<&'a Type, syn::Error> {
    let args = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a,
        _ => {
            return Err(syn::Error::new(
                seg.ident.span(),
                format!("`{}` requires a single type argument", name),
            ));
        }
    };
    let mut type_args = args.args.iter().filter_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    });
    let first = type_args.next().ok_or_else(|| {
        syn::Error::new(
            seg.ident.span(),
            format!("`{}` requires a type argument", name),
        )
    })?;
    if type_args.next().is_some() {
        return Err(syn::Error::new(
            seg.ident.span(),
            format!("`{}` takes exactly one type argument", name),
        ));
    }
    Ok(first)
}
