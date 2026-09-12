// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Derive support for typed page-scoped shared state.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use sha2::{Digest, Sha256};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{Data, DeriveInput, Field, Fields, LitStr};

use crate::client_props_derive::{classify_field_type, FieldKind};

pub(crate) fn derive_shared_state(input: TokenStream) -> TokenStream {
    let input = match syn::parse2::<DeriveInput>(input) {
        Ok(input) => input,
        Err(error) => return error.to_compile_error(),
    };
    match expand(&input) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "#[derive(rsjs::SharedState)] requires an owned, non-generic struct; \
             page-scoped state must be `'static` so provider scopes can cross independent \
             island hydration lifetimes",
        ));
    }

    // Field-level `#[rsjs(...)]` attributes are inert helper attributes to
    // rustc, so the derive must reject them itself. Check before the
    // struct-level `type_key` lookup so a `type_key` written on a field gets
    // the placement diagnostic instead of "missing protocol identity".
    if let Data::Struct(data) = &input.data {
        validate_field_attributes(&data.fields)?;
        validate_reserved_field_names(&data.fields)?;
    }

    let type_key = parse_type_key(input)?;
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) if !fields.named.is_empty() => &fields.named,
            Fields::Named(_) => {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "#[derive(rsjs::SharedState)] requires at least one named field",
                ));
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new(
                    fields.span(),
                    "#[derive(rsjs::SharedState)] requires named fields; tuple structs do not \
                     provide stable field-cell names",
                ));
            }
            Fields::Unit => {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "#[derive(rsjs::SharedState)] requires a struct with named fields",
                ));
            }
        },
        Data::Enum(data) => {
            return Err(syn::Error::new(
                data.enum_token.span,
                "#[derive(rsjs::SharedState)] supports state structs only because top-level \
                 named fields define the independently reactive cells. Put a fieldless \
                 `#[derive(rsjs::ClientProps)]` enum in a named state field instead of deriving \
                 SharedState on the enum itself",
            ));
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span,
                "#[derive(rsjs::SharedState)] does not support unions",
            ));
        }
    };

    // Shared fields are writable in the browser, while the object codec
    // deliberately accepts borrowed encode-only fields for one-way props.
    // Reject those here, with the field's own span,
    // before the codec expansion silently skips the `ClientDecode` impl and
    // leaves the generated `impl SharedState` to fail its supertrait bound
    // with an unguided E0277 at the struct ident.
    ensure_fields_round_trip(fields)?;

    // Reuse the exact rsjs object codec used by ClientProps. Translate its
    // diagnostic prefix so SharedState authors are never told to add a
    // different derive to fix their schema. The translated wording is pinned
    // by unit tests below, which fail if the ClientProps message text drifts
    // out from under these replacements.
    let codec = crate::client_props_derive::expand_client_props(input).map_err(|error| {
        syn::Error::new(
            error.span(),
            error
                .to_string()
                .replace(
                    "#[derive(rsjs::ClientProps)]",
                    "#[derive(rsjs::SharedState)]",
                )
                .replace("ClientProps-derived", "SharedState-derived"),
        )
    })?;

    let struct_ident = &input.ident;
    let handle_ident = format_ident!("{}Handle", struct_ident);
    let meta_ident = format_ident!("__rsjs_shared_state_meta_{}", struct_ident);
    let codec_meta_ident = format_ident!("__rsjs_codec_meta_{}", struct_ident);
    let witness_ident = format_ident!("__RSJS_SHARED_STATE_ABI_WITNESS_{}", struct_ident);
    let definition_ident = format_ident!("__RSJS_SHARED_STATE_DEFINITION_{}", struct_ident);
    let payload_ident = format_ident!("__RSJS_SHARED_STATE_PAYLOAD_{}", struct_ident);
    let visibility = &input.vis;
    let struct_name = struct_ident.to_string();

    let mut field_entries = Vec::with_capacity(fields.len());
    let mut accessors = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let ident = field.ident.as_ref().expect("named field");
        let name = ident.to_string();
        let ty = &field.ty;
        let field_visibility = &field.vis;
        let rust_type = ty
            .to_token_stream()
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        let field_id = index as u32;
        let field_kind = classify_field_type(field)?;
        field_entries.push(quote! {
            ::rsjs::SharedStateFieldMeta {
                id: #field_id,
                name: #name,
                rust_type: #rust_type,
                equality: ::rsjs::SharedStateEquality::ObjectIs,
            }
        });
        // The accessor is on the per-request SSR hot path, so it must not
        // copy the field value: it hands out a handle borrowing the field
        // inside the provider's `Rc` snapshot. Preserve the source field's
        // visibility so deriving a store never widens its Rust API.
        accessors.push(quote! {
            #field_visibility fn #ident(&self) -> ::rsjs::SharedField<'_, #ty> {
                ::rsjs::SharedField::__from_server(
                    &self.__rsjs_value.#ident,
                    #field_id,
                    #name,
                )
            }
        });

        // SSR replay accessor. `emit_rust` has to declare one Rust local per
        // reachable shared field so the server render can evaluate the same
        // expressions the client will, and that local must borrow rather than
        // clone for the string-like and collection-like shapes (the F25
        // no-temporary-String property). Deciding borrow-versus-value needs the
        // field's real Rust type, which the island macro cannot see. It is
        // decided here instead, where `classify_field_type` already has it, and
        // rustc type-checks the result at the call site.
        let replay_ident = format_ident!("__rsjs_replay_{}", ident);
        accessors.push(match &field_kind {
            // String-like: hand back `&str`. The render local then borrows the
            // provider's snapshot with no temporary `String`, and `&str` both
            // renders directly in a text slot and formats in an attribute.
            FieldKind::String { .. } => quote! {
                #[doc(hidden)]
                #field_visibility fn #replay_ident(&self) -> &str {
                    ::core::convert::AsRef::<str>::as_ref(self.#ident().__rsjs_get_ref())
                }
            },
            // Collections and nested props: borrow. These never reach a text
            // slot as a whole value, and cloning one per request would be a
            // per-render allocation on the SSR hot path.
            FieldKind::VecOf(_) | FieldKind::SliceOf(_) | FieldKind::NestedPath { .. } => quote! {
                #[doc(hidden)]
                #field_visibility fn #replay_ident(&self) -> &#ty {
                    self.#ident().__rsjs_get_ref()
                }
            },
            // Scalars and `Option`s: by value, matching `.get()`.
            _ => quote! {
                #[doc(hidden)]
                #field_visibility fn #replay_ident(&self) -> #ty {
                    self.#ident().get()
                }
            },
        });
    }

    let type_key = type_key.value();
    let cargo_unit_id = crate::descriptor_cargo_unit_id()
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?;
    let schema_abi = canonical_shared_state_schema_abi(fields)?;
    let schema_status = if schema_abi.resolved {
        quote!(::rsjs::__private::SharedStateAbiSchemaStatus::Resolved)
    } else {
        quote!(::rsjs::__private::SharedStateAbiSchemaStatus::UnresolvedNestedClientProps)
    };
    let schema_abi = schema_abi.bytes;
    let shared_state_record_id = shared_state_record_id(input, &type_key, &schema_abi);
    let payload = rsjs_transport::encode_shared_state_definition_payload_record(
        cargo_unit_id,
        shared_state_record_id,
        &type_key,
        &schema_abi,
    );
    let schema_abi_hash = rsjs_transport::shared_state_schema_abi_hash(&schema_abi);
    let cargo_unit_id = byte_array_tokens(cargo_unit_id);
    let shared_state_record_id = byte_array_tokens(shared_state_record_id);
    let schema_abi_hash = byte_array_tokens(schema_abi_hash);
    let payload_len = syn::LitInt::new(&payload.len().to_string(), proc_macro2::Span::call_site());
    let payload = syn::LitByteStr::new(&payload, proc_macro2::Span::call_site());

    Ok(quote! {
        #codec

        #[derive(Clone)]
        #visibility struct #handle_ident {
            __rsjs_value: ::std::rc::Rc<#struct_ident>,
        }

        impl #handle_ident {
            #[doc(hidden)]
            pub fn __rsjs_from_server(value: ::std::rc::Rc<#struct_ident>) -> Self {
                Self { __rsjs_value: value }
            }

            #(#accessors)*
        }

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        pub static #witness_ident: ::rsjs::__private::SharedStateAbiWitness =
            ::rsjs::__private::SharedStateAbiWitness {
                format_magic: ::rsjs::__private::SHARED_STATE_ABI_WITNESS_MAGIC,
                witness_version: ::rsjs::__private::SHARED_STATE_ABI_WITNESS_VERSION,
                shared_state_abi_version: ::rsjs::SHARED_STATE_ABI_VERSION,
                schema_status: #schema_status,
                type_key: #type_key,
                schema_abi_hash: [#(#schema_abi_hash),*],
                defining_symbol: concat!(
                    module_path!(),
                    "::",
                    stringify!(#witness_ident),
                ),
            };

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_state")
        )]
        #[cfg_attr(
            any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox"
            ),
            unsafe(link_section = ".rsjs.shared_state")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$J"))]
        static #definition_ident: ::rsjs::__private::SharedStateDefinitionRecord<
            ::rsjs::__private::SharedStateAbiWitness,
        > = ::rsjs::__private::SharedStateDefinitionRecord::new(
            [#(#cargo_unit_id),*],
            [#(#shared_state_record_id),*],
            &#witness_ident,
        );

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_spay")
        )]
        #[cfg_attr(
            any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox"
            ),
            unsafe(link_section = ".rsjs.shared_state_payload")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$R"))]
        static #payload_ident: ::rsjs::__private::PayloadRecord<#payload_len> =
            ::rsjs::__private::PayloadRecord::new(*#payload);

        #[automatically_derived]
        impl ::rsjs::__private::SharedStateAbi for #struct_ident {
            const WITNESS: &'static ::rsjs::__private::SharedStateAbiWitness =
                &#witness_ident;
        }

        #[automatically_derived]
        impl ::rsjs::SharedState for #struct_ident {
            type Handle = #handle_ident;

            const TYPE_KEY: &'static str = #type_key;
            const META: &'static ::rsjs::SharedStateMeta = #meta_ident;

            fn schema_hash() -> &'static str {
                static HASH: ::std::sync::OnceLock<::std::string::String> =
                    ::std::sync::OnceLock::new();
                HASH.get_or_init(|| {
                    ::rsjs::__shared_state_schema_hash(Self::TYPE_KEY, #codec_meta_ident)
                })
                .as_str()
            }

            fn __rsjs_wide_paths_json() -> &'static str {
                static PATHS: ::std::sync::OnceLock<::std::string::String> =
                    ::std::sync::OnceLock::new();
                PATHS.get_or_init(|| {
                    ::rsjs::__shared_state_wide_paths_json(#codec_meta_ident)
                })
                .as_str()
            }

            #[doc(hidden)]
            fn __rsjs_handle_from_server(value: ::std::rc::Rc<Self>) -> Self::Handle {
                #handle_ident::__rsjs_from_server(value)
            }
        }

        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        #[doc(hidden)]
        #visibility const #meta_ident: &'static ::rsjs::SharedStateMeta =
            &::rsjs::SharedStateMeta {
                abi_version: ::rsjs::SHARED_STATE_ABI_VERSION,
                type_key: #type_key,
                schema_hash_fn: <#struct_ident as ::rsjs::SharedState>::schema_hash,
                rust_type: #struct_name,
                fields: &[#(#field_entries),*],
                wide_paths_json_fn:
                    <#struct_ident as ::rsjs::SharedState>::__rsjs_wide_paths_json,
            };

        #[cfg(not(any(
            target_vendor = "apple",
            target_os = "linux",
            target_os = "android",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku",
            target_os = "redox",
            target_os = "windows"
        )))]
        compile_error!(
            "RSJS SharedState relocation transport does not support this target object format"
        );
    })
}

struct CanonicalSharedStateSchema {
    bytes: Vec<u8>,
    resolved: bool,
}

fn canonical_shared_state_schema_abi(
    fields: &Punctuated<Field, Comma>,
) -> syn::Result<CanonicalSharedStateSchema> {
    let mut schema_fields = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let kind = classify_field_type(field)?;
        let Some(codec) = canonical_schema_codec(&kind) else {
            return Ok(CanonicalSharedStateSchema {
                bytes: rsjs_transport::encode_unresolved_shared_state_schema_abi(
                    rsjs_transport::SHARED_STATE_RUNTIME_ABI_VERSION,
                    rsjs_transport::SharedStateSchemaUnresolvedReason::NestedClientProps,
                ),
                resolved: false,
            });
        };
        let id = u32::try_from(index).map_err(|_| {
            syn::Error::new(
                field.span(),
                "SharedState field count exceeds the canonical schema ABI",
            )
        })?;
        schema_fields.push(rsjs_transport::SharedStateSchemaField {
            id,
            name: field.ident.as_ref().expect("named field").to_string(),
            equality: rsjs_transport::SharedStateSchemaEquality::ObjectIs,
            codec,
        });
    }
    let bytes = rsjs_transport::encode_resolved_shared_state_schema_abi(
        rsjs_transport::SHARED_STATE_RUNTIME_ABI_VERSION,
        &schema_fields,
    )
    .map_err(|error| {
        syn::Error::new(
            fields.span(),
            format!("failed to encode canonical SharedState schema ABI: {error}"),
        )
    })?;
    Ok(CanonicalSharedStateSchema {
        bytes,
        resolved: true,
    })
}

fn canonical_schema_codec(kind: &FieldKind) -> Option<rsjs_transport::SharedStateSchemaCodec> {
    use rsjs_transport::SharedStateSchemaCodec as Codec;
    match kind {
        FieldKind::Bool => Some(Codec::Bool),
        FieldKind::NarrowSigned => Some(Codec::I32),
        FieldKind::NarrowUnsigned => Some(Codec::U32),
        FieldKind::WideSigned => Some(Codec::I64),
        FieldKind::WideUnsigned => Some(Codec::U64),
        FieldKind::F32 => Some(Codec::F32),
        FieldKind::F64 => Some(Codec::F64),
        FieldKind::String { .. } => Some(Codec::String),
        FieldKind::OptionOf(inner) => {
            canonical_schema_codec(inner).map(|codec| Codec::Option(Box::new(codec)))
        }
        FieldKind::VecOf(inner) | FieldKind::SliceOf(inner) => {
            canonical_schema_codec(inner).map(|codec| Codec::Vec(Box::new(codec)))
        }
        FieldKind::NestedPath { .. } => None,
    }
}

/// Build-local join identifier for the fixed definition and inline payload.
///
/// The canonical semantic identity remains `type_key` plus `schema_abi`.
/// Authored tokens only disambiguate object records inside one Cargo unit and
/// are never emitted as schema bytes or used to decide compatibility.
fn shared_state_record_id(input: &DeriveInput, type_key: &str, schema_abi: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.shared-state-record.v1\0");
    hasher.update((type_key.len() as u64).to_le_bytes());
    hasher.update(type_key.as_bytes());
    hasher.update((schema_abi.len() as u64).to_le_bytes());
    hasher.update(schema_abi);
    let authored = input.to_token_stream().to_string();
    hasher.update((authored.len() as u64).to_le_bytes());
    hasher.update(authored.as_bytes());
    let span = input.ident.span();
    let source_file = stable_source_file(&span);
    let start = span.start();
    let end = span.end();
    hasher.update((source_file.len() as u64).to_le_bytes());
    hasher.update(source_file.as_bytes());
    hasher.update((start.line as u64).to_le_bytes());
    hasher.update((start.column as u64).to_le_bytes());
    hasher.update((end.line as u64).to_le_bytes());
    hasher.update((end.column as u64).to_le_bytes());
    hasher.finalize().into()
}

/// Reproducible source-file component of a build-local transport join ID.
///
/// `Span::local_file()` may be an absolute checkout path. Prefer its path
/// relative to `CARGO_MANIFEST_DIR`; fall back to rustc's display path, which
/// honors path remapping. An absolute fallback keeps only its final three
/// components so moving the checkout does not perturb plan inputs.
pub(crate) fn stable_source_file(span: &proc_macro2::Span) -> String {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from);
    let local_file = span.local_file();
    stable_source_file_from_paths(local_file.as_deref(), &span.file(), manifest_dir.as_deref())
}

fn stable_source_file_from_paths(
    local_file: Option<&std::path::Path>,
    display: &str,
    manifest_dir: Option<&std::path::Path>,
) -> String {
    if let (Some(local_file), Some(manifest_dir)) = (local_file, manifest_dir) {
        if let Ok(relative) = local_file.strip_prefix(manifest_dir) {
            return normalized_path(relative);
        }
    }

    let display_path = std::path::Path::new(display);
    if let Some(manifest_dir) = manifest_dir {
        if let Ok(relative) = display_path.strip_prefix(manifest_dir) {
            return normalized_path(relative);
        }
    }
    if display_path.is_absolute() {
        let components = display_path.components().rev().take(3).collect::<Vec<_>>();
        return components
            .into_iter()
            .rev()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
    }
    normalized_path(display_path)
}

fn normalized_path(path: &std::path::Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn byte_array_tokens(bytes: [u8; 32]) -> Vec<proc_macro2::Literal> {
    bytes
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect()
}

fn parse_type_key(input: &DeriveInput) -> syn::Result<LitStr> {
    let mut type_key = None;
    for attribute in &input.attrs {
        if !attribute.path().is_ident("rsjs") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("type_key") {
                if type_key.is_some() {
                    return Err(meta.error(
                        "duplicate `type_key`; a SharedState schema has exactly one protocol identity",
                    ));
                }
                type_key = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else {
                Err(meta.error(
                    "unknown SharedState option; expected `#[rsjs(type_key = \"...\")]`",
                ))
            }
        })?;
    }

    let type_key = type_key.ok_or_else(|| {
        syn::Error::new(
            input.ident.span(),
            "missing shared-state protocol identity; add `#[rsjs(type_key = \"your.namespace.state\")]`",
        )
    })?;
    let value = type_key.value();
    if value.trim().is_empty() {
        return Err(syn::Error::new(
            type_key.span(),
            "shared-state `type_key` cannot be empty",
        ));
    }
    if value != value.trim() || value.chars().any(char::is_whitespace) {
        return Err(syn::Error::new(
            type_key.span(),
            "shared-state `type_key` must not contain whitespace",
        ));
    }
    if !value.contains('.') {
        return Err(syn::Error::new(
            type_key.span(),
            "shared-state `type_key` must be namespaced, for example `acme.workbench`",
        ));
    }
    Ok(type_key)
}

/// Reject `#[rsjs(...)]` on fields. The derive registers `rsjs` as a helper
/// attribute, so rustc accepts it anywhere on the item; without this check a
/// field-level option would be silently ignored, and a field-level `type_key`
/// would surface as the misleading "missing protocol identity" error.
fn validate_field_attributes(fields: &Fields) -> syn::Result<()> {
    for field in fields.iter() {
        let field_name = field
            .ident
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "_".to_string());
        for attribute in &field.attrs {
            if !attribute.path().is_ident("rsjs") {
                continue;
            }
            match &attribute.meta {
                syn::Meta::List(list) if !list.tokens.is_empty() => {
                    attribute.parse_nested_meta(|meta| {
                        if meta.path.is_ident("type_key") {
                            // Consume the value so the diagnostic is about
                            // placement, not a trailing-token parse artifact.
                            if let Ok(value) = meta.value() {
                                let _ = value.parse::<LitStr>();
                            }
                            Err(meta.error(format!(
                                "`type_key` belongs on the struct, not on field \
                                 `{field_name}`: the protocol identity names the whole \
                                 shared-state schema. Move it above the struct as \
                                 `#[rsjs(type_key = \"...\")]`",
                            )))
                        } else {
                            Err(meta.error(format!(
                                "#[rsjs(...)] options are not supported on shared-state \
                                 fields; remove this attribute from field `{field_name}`. \
                                 Every field uses `Object.is` equality and has no per-field \
                                 configuration in v1",
                            )))
                        }
                    })?;
                }
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        format!(
                            "#[rsjs] does nothing on shared-state field `{field_name}`; \
                             remove it. Struct-level options such as \
                             `#[rsjs(type_key = \"...\")]` go above the struct",
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_reserved_field_names(fields: &Fields) -> syn::Result<()> {
    for field in fields.iter() {
        let Some(ident) = &field.ident else {
            continue;
        };
        if ident.to_string().starts_with("__rsjs_") {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "shared-state field `{ident}` uses the reserved `__rsjs_` prefix; rename it so the derive can generate its typed handle methods without a collision"
                ),
            ));
        }
    }
    Ok(())
}

/// Page-scoped shared fields are writable client cells, so their Rust schema
/// must describe owned values rather than one-way borrowed props. Requiring
/// the owned `ClientDecode` side of the codec is how v1 enforces that shape;
/// no browser mutation is sent back to the server implicitly.
fn ensure_fields_round_trip(fields: &Punctuated<Field, Comma>) -> syn::Result<()> {
    let mut combined: Option<syn::Error> = None;
    for field in fields {
        // Types the codec cannot classify at all are reported by the codec
        // expansion itself (with the SharedState wording applied by
        // `expand`), so only decode support is checked here.
        let Ok(kind) = classify_field_type(field) else {
            continue;
        };
        if kind.supports_decode() {
            continue;
        }
        let ident = field.ident.as_ref().expect("named field");
        let error = syn::Error::new(
            field.ty.span(),
            format!(
                "shared-state field `{ident}` is encode-only: shared fields are writable \
                 browser cells, so every #[derive(rsjs::SharedState)] field must use an \
                 owned client value rather than a one-way borrowed prop. Browser writes \
                 remain page-local and are not sent back to Rust automatically. {}",
                owned_alternative(&kind)
            ),
        );
        match combined.as_mut() {
            Some(existing) => existing.combine(error),
            None => combined = Some(error),
        }
    }
    match combined {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Concrete owned replacement suggestion for an encode-only field kind.
fn owned_alternative(kind: &FieldKind) -> &'static str {
    match kind {
        FieldKind::String { .. } => "Replace the borrowed string with an owned `String`",
        FieldKind::SliceOf(_) => "Replace the borrowed slice with an owned `Vec<T>`",
        FieldKind::NestedPath { .. } => {
            "Store the value by ownership instead of behind a reference"
        }
        FieldKind::OptionOf(inner) | FieldKind::VecOf(inner) => owned_alternative(inner),
        _ => "Use an owned type that implements `rsjs::ClientDecode`",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(source: &str) -> String {
        let input = syn::parse_str::<DeriveInput>(source).unwrap();
        expand(&input).unwrap_err().to_string()
    }

    /// Successful expansion as a whitespace-free string, so assertions about
    /// generated metadata do not depend on token-stream spacing.
    fn expansion(source: &str) -> String {
        let input = syn::parse_str::<DeriveInput>(source).unwrap();
        expand(&input)
            .unwrap()
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    fn canonical_schema(source: &str) -> rsjs_transport::SharedStateSchemaAbi {
        let input = syn::parse_str::<DeriveInput>(source).unwrap();
        let fields = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(fields) => &fields.named,
                _ => panic!("fixture must use named fields"),
            },
            _ => panic!("fixture must be a struct"),
        };
        let schema = canonical_shared_state_schema_abi(fields).unwrap();
        rsjs_transport::decode_shared_state_schema_abi(&schema.bytes).unwrap()
    }

    fn canonical_schema_bytes(source: &str) -> Vec<u8> {
        let input = syn::parse_str::<DeriveInput>(source).unwrap();
        let fields = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(fields) => &fields.named,
                _ => panic!("fixture must use named fields"),
            },
            _ => panic!("fixture must be a struct"),
        };
        canonical_shared_state_schema_abi(fields).unwrap().bytes
    }

    #[test]
    fn missing_key_names_the_fix() {
        let message = error("struct WorkbenchState { mode: bool }");
        assert!(message.contains("missing shared-state protocol identity"));
        assert!(message.contains("#[rsjs(type_key = \"your.namespace.state\")]"));
    }

    #[test]
    fn protocol_key_must_be_namespaced() {
        let message =
            error(r#"#[rsjs(type_key = "workbench")] struct WorkbenchState { mode: bool }"#);
        assert!(message.contains("must be namespaced"));
        assert!(message.contains("acme.workbench"));
    }

    #[test]
    fn tuple_struct_explains_stable_field_requirement() {
        let message = error(r#"#[rsjs(type_key = "acme.workbench")] struct WorkbenchState(bool);"#);
        assert!(message.contains("requires named fields"));
        assert!(message.contains("stable field-cell names"));
    }

    #[test]
    fn generic_state_explains_page_lifetime() {
        let message =
            error(r#"#[rsjs(type_key = "acme.workbench")] struct WorkbenchState<T> { value: T }"#);
        assert!(message.contains("owned, non-generic struct"));
        assert!(message.contains("page-scoped state must be `'static`"));
    }

    #[test]
    fn enum_is_rejected_with_field_codec_guidance() {
        let message = error(r#"#[rsjs(type_key = "acme.workbench")] enum Mode { On, Off }"#);
        assert!(message.contains("supports state structs only"));
        assert!(message.contains("top-level named fields"));
        assert!(message.contains("#[derive(rsjs::ClientProps)]"));
    }

    #[test]
    fn union_is_rejected() {
        let message =
            error(r#"#[rsjs(type_key = "acme.workbench")] union Raw { bits: u32, flag: u32 }"#);
        assert!(message.contains("does not support unions"));
    }

    #[test]
    fn unit_struct_is_rejected() {
        let message = error(r#"#[rsjs(type_key = "acme.workbench")] struct Empty;"#);
        assert!(message.contains("requires a struct with named fields"));
    }

    #[test]
    fn empty_named_struct_is_rejected() {
        let message = error(r#"#[rsjs(type_key = "acme.workbench")] struct Empty {}"#);
        assert!(message.contains("requires at least one named field"));
    }

    #[test]
    fn empty_type_key_is_rejected() {
        let message = error(r#"#[rsjs(type_key = "")] struct WorkbenchState { mode: bool }"#);
        assert!(message.contains("cannot be empty"));
    }

    #[test]
    fn whitespace_type_key_is_rejected() {
        let message =
            error(r#"#[rsjs(type_key = "acme. workbench")] struct WorkbenchState { mode: bool }"#);
        assert!(message.contains("must not contain whitespace"));
    }

    #[test]
    fn duplicate_type_key_is_rejected_across_attributes() {
        let message = error(
            r#"#[rsjs(type_key = "acme.a")] #[rsjs(type_key = "acme.b")]
               struct WorkbenchState { mode: bool }"#,
        );
        assert!(message.contains("duplicate `type_key`"));
        assert!(message.contains("exactly one protocol identity"));
    }

    #[test]
    fn unknown_struct_option_is_rejected() {
        let message = error(r#"#[rsjs(equality = "deep")] struct WorkbenchState { mode: bool }"#);
        assert!(message.contains("unknown SharedState option"));
        assert!(message.contains("type_key"));
    }

    // F24: field-level #[rsjs(...)] attributes must be validated, not
    // silently ignored as inert helper attributes.

    #[test]
    fn field_level_type_key_names_the_struct_placement() {
        let message =
            error(r#"struct WorkbenchState { #[rsjs(type_key = "acme.workbench")] mode: bool }"#);
        assert!(message.contains("`type_key` belongs on the struct"));
        assert!(message.contains("field `mode`"));
        assert!(message.contains("Move it above the struct"));
    }

    #[test]
    fn field_level_option_is_rejected() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { #[rsjs(equality = "deep")] mode: bool }"#,
        );
        assert!(message.contains("not supported on shared-state fields"));
        assert!(message.contains("field `mode`"));
    }

    #[test]
    fn bare_field_level_attribute_is_rejected() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { #[rsjs] mode: bool }"#,
        );
        assert!(message.contains("does nothing on shared-state field `mode`"));
    }

    #[test]
    fn reserved_generated_field_prefix_is_rejected() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { __rsjs_from_server: bool }"#,
        );
        assert!(message.contains("reserved `__rsjs_` prefix"));
        assert!(message.contains("rename it"));
    }

    #[test]
    fn generated_accessors_preserve_field_visibility() {
        let generated = expansion(
            r#"#[rsjs(type_key = "acme.workbench")]
               pub struct WorkbenchState {
                   pub mode: bool,
                   pub(crate) radius: u32,
                   label: String,
               }"#,
        );
        assert!(generated.contains("pubfnmode("), "{generated}");
        assert!(generated.contains("pub(crate)fnradius("), "{generated}");
        assert!(generated.contains("fnlabel("), "{generated}");
        assert!(!generated.contains("pubfnlabel("), "{generated}");
    }

    #[test]
    fn expansion_emits_one_rust_witness_definition_record_and_inline_payload() {
        let generated = expansion(
            r#"#[rsjs(type_key = "acme.workbench")]
               pub struct WorkbenchState {
                   pub mode: bool,
                   pub selected: Option<String>,
               }"#,
        );
        assert!(
            generated.contains("__RSJS_SHARED_STATE_ABI_WITNESS_WorkbenchState"),
            "{generated}"
        );
        assert!(
            generated.contains("impl::rsjs::__private::SharedStateAbiforWorkbenchState"),
            "{generated}"
        );
        assert!(
            generated.contains("SharedStateDefinitionRecord"),
            "{generated}"
        );
        assert!(generated.contains("PayloadRecord"), "{generated}");
        assert!(
            generated.contains("unsafe(link_section=\".rsjs.shared_state\")"),
            "{generated}"
        );
        assert!(
            generated.contains("unsafe(link_section=\".rsjs.shared_state_payload\")"),
            "{generated}"
        );
        assert!(
            generated.contains("type_key:\"acme.workbench\""),
            "{generated}"
        );
        assert!(
            generated.contains("SharedStateAbiSchemaStatus::Resolved"),
            "{generated}"
        );
    }

    #[test]
    fn canonical_schema_uses_closed_normalized_recursive_codec_vocabulary() {
        let schema = canonical_schema(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState {
                   enabled: bool,
                   count: i16,
                   total: usize,
                   ratio: f64,
                   labels: Option<Vec<String>>,
               }"#,
        );
        assert_eq!(
            schema.resolution,
            rsjs_transport::SharedStateSchemaAbiResolution::Resolved
        );
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|field| (&*field.name, &field.codec))
                .collect::<Vec<_>>(),
            vec![
                ("enabled", &rsjs_transport::SharedStateSchemaCodec::Bool),
                ("count", &rsjs_transport::SharedStateSchemaCodec::I32),
                ("total", &rsjs_transport::SharedStateSchemaCodec::U64),
                ("ratio", &rsjs_transport::SharedStateSchemaCodec::F64),
                (
                    "labels",
                    &rsjs_transport::SharedStateSchemaCodec::Option(Box::new(
                        rsjs_transport::SharedStateSchemaCodec::Vec(Box::new(
                            rsjs_transport::SharedStateSchemaCodec::String,
                        )),
                    )),
                ),
            ]
        );
    }

    #[test]
    fn canonical_schema_bytes_change_with_name_order_or_codec() {
        let baseline = canonical_schema_bytes(
            r#"struct State {
                first: bool,
                second: u32,
            }"#,
        );
        assert_eq!(
            baseline,
            canonical_schema_bytes(
                r#"struct State {
                    first: bool,
                    second: u32,
                }"#,
            ),
            "identical field contracts must produce byte-identical ABI"
        );
        assert_ne!(
            baseline,
            canonical_schema_bytes(
                r#"struct State {
                    renamed: bool,
                    second: u32,
                }"#,
            )
        );
        assert_ne!(
            baseline,
            canonical_schema_bytes(
                r#"struct State {
                    second: u32,
                    first: bool,
                }"#,
            )
        );
        assert_ne!(
            baseline,
            canonical_schema_bytes(
                r#"struct State {
                    first: bool,
                    second: u64,
                }"#,
            )
        );
    }

    #[test]
    fn nested_client_props_schema_is_explicitly_unresolved_without_path_identity() {
        let input = syn::parse_str::<DeriveInput>(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState {
                   nested: crate::models::NestedProps,
               }"#,
        )
        .unwrap();
        let fields = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(fields) => &fields.named,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let schema = canonical_shared_state_schema_abi(fields).unwrap();
        assert!(!schema.resolved);
        assert!(!schema
            .bytes
            .windows(b"crate::models::NestedProps".len())
            .any(|window| window == b"crate::models::NestedProps"));
        assert_eq!(
            rsjs_transport::decode_shared_state_schema_abi(&schema.bytes)
                .unwrap()
                .resolution,
            rsjs_transport::SharedStateSchemaAbiResolution::Unresolved(
                rsjs_transport::SharedStateSchemaUnresolvedReason::NestedClientProps,
            )
        );

        let generated = expand(&input)
            .unwrap()
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            generated.contains("SharedStateAbiSchemaStatus::UnresolvedNestedClientProps"),
            "{generated}"
        );
    }

    #[test]
    fn identical_same_named_definitions_in_two_modules_get_distinct_join_ids() {
        let file = syn::parse_str::<syn::File>(
            r#"
                mod first {
                    #[rsjs(type_key = "acme.shared")]
                    struct State { value: u32 }
                }

                mod second {
                    #[rsjs(type_key = "acme.shared")]
                    struct State { value: u32 }
                }
            "#,
        )
        .unwrap();
        let definitions = file
            .items
            .iter()
            .map(|item| {
                let syn::Item::Mod(module) = item else {
                    panic!("fixture item must be a module");
                };
                let (_, items) = module.content.as_ref().expect("inline module");
                let syn::Item::Struct(item) = &items[0] else {
                    panic!("fixture module must contain a struct");
                };
                DeriveInput {
                    attrs: item.attrs.clone(),
                    vis: item.vis.clone(),
                    ident: item.ident.clone(),
                    generics: item.generics.clone(),
                    data: Data::Struct(syn::DataStruct {
                        struct_token: item.struct_token,
                        fields: item.fields.clone(),
                        semi_token: item.semi_token,
                    }),
                }
            })
            .collect::<Vec<_>>();
        let ids = definitions
            .iter()
            .map(|definition| {
                let fields = match &definition.data {
                    Data::Struct(data) => match &data.fields {
                        Fields::Named(fields) => &fields.named,
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                };
                let schema = canonical_shared_state_schema_abi(fields).unwrap();
                shared_state_record_id(definition, "acme.shared", &schema.bytes)
            })
            .collect::<Vec<_>>();
        assert_ne!(
            ids[0], ids[1],
            "source location must disambiguate build-local record joins; schema identity remains byte-identical"
        );
    }

    #[test]
    fn build_local_source_identity_is_checkout_relative() {
        let first_manifest = std::path::Path::new("/checkout-one/proa");
        let second_manifest = std::path::Path::new("/different/root/proa");
        let first_file = first_manifest.join("src/state.rs");
        let second_file = second_manifest.join("src/state.rs");
        assert_eq!(
            stable_source_file_from_paths(
                Some(&first_file),
                "/checkout-one/proa/src/state.rs",
                Some(first_manifest),
            ),
            "src/state.rs"
        );
        assert_eq!(
            stable_source_file_from_paths(
                Some(&first_file),
                "/checkout-one/proa/src/state.rs",
                Some(first_manifest),
            ),
            stable_source_file_from_paths(
                Some(&second_file),
                "/different/root/proa/src/state.rs",
                Some(second_manifest),
            ),
            "moving an identical checkout must not perturb a record id that enters PlanId"
        );
    }

    // F8: encode-only (borrowed) fields must fail in the derive with a
    // field-pointed round-trip explanation, not a deferred E0277 on the
    // generated `impl SharedState`.

    #[test]
    fn borrowed_str_field_explains_round_trip_and_names_field() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { label: &'static str }"#,
        );
        assert!(message.contains("field `label` is encode-only"));
        assert!(message.contains("shared fields are writable browser cells"));
        assert!(message.contains("not sent back to Rust automatically"));
        assert!(message.contains("owned `String`"));
    }

    #[test]
    fn borrowed_slice_field_suggests_vec() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { sizes: &'static [u32] }"#,
        );
        assert!(message.contains("field `sizes` is encode-only"));
        assert!(message.contains("owned `Vec<T>`"));
    }

    #[test]
    fn vec_of_borrowed_strings_suggests_owned_string() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { labels: Vec<&'static str> }"#,
        );
        assert!(message.contains("field `labels` is encode-only"));
        assert!(message.contains("owned `String`"));
    }

    #[test]
    fn every_encode_only_field_is_reported() {
        let input = syn::parse_str::<DeriveInput>(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { first: &'static str, second: &'static [u32] }"#,
        )
        .unwrap();
        let errors: Vec<String> = expand(&input)
            .unwrap_err()
            .into_iter()
            .map(|error| error.to_string())
            .collect();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("field `first`"));
        assert!(errors[1].contains("field `second`"));
    }

    // Wide paths are derived recursively from the same codec metadata used
    // for schema hashing, avoiding a second top-level-only type classifier.

    #[test]
    fn wide_paths_are_generated_from_codec_metadata() {
        let generated = expansion(
            r#"#[rsjs(type_key = "acme.cart")]
               struct CartState { count: usize, total: i64, label: String }"#,
        );
        assert!(
            generated.contains("__shared_state_wide_paths_json(__rsjs_codec_meta_CartState)"),
            "wide-path codec hook missing from generated metadata: {generated}"
        );
    }

    #[test]
    fn wide_path_function_is_wired_into_meta() {
        let generated = expansion(
            r#"#[rsjs(type_key = "acme.cart")]
               struct CartState { discount: Option<u64> }"#,
        );
        assert!(
            generated.contains(
                "wide_paths_json_fn:<CartStateas::rsjs::SharedState>::__rsjs_wide_paths_json"
            ),
            "wide-path function pointer missing from generated metadata: {generated}"
        );
    }

    #[test]
    fn every_schema_uses_the_runtime_codec_walker() {
        let generated = expansion(
            r#"#[rsjs(type_key = "acme.cart")]
               struct CartState { count: u32, label: String, ratio: f64 }"#,
        );
        assert!(
            generated.contains("__shared_state_wide_paths_json(__rsjs_codec_meta_CartState)"),
            "codec walker must decide whether the final path list is empty: {generated}"
        );
    }

    // F22: the ClientProps-to-SharedState diagnostic translation is a string
    // replacement against text authored in client_props_derive.rs. These
    // tests pin the translated wording: if the ClientProps message drifts so
    // the replacement no longer matches, the "ClientProps" leftovers below
    // fail the assertions.

    #[test]
    fn codec_error_translation_rewrites_derive_name() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { value: Option<Option<u32>> }"#,
        );
        assert!(message.contains("#[derive(rsjs::SharedState)]"));
        assert!(
            !message.contains("ClientProps"),
            "translated codec diagnostic still mentions ClientProps: {message}"
        );
    }

    #[test]
    fn codec_error_translation_rewrites_derived_struct_wording() {
        let message = error(
            r#"#[rsjs(type_key = "acme.workbench")]
               struct WorkbenchState { pair: (u32, u32) }"#,
        );
        assert!(message.contains("SharedState-derived"));
        assert!(
            !message.contains("ClientProps"),
            "translated codec diagnostic still mentions ClientProps: {message}"
        );
    }
}
