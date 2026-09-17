// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Derive macro for Proa render components.
//!
//! Automatically generates `render_opening`, `render`, and `render_closing`
//! implementations for UI components based on field-level annotations.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::visit::{self, Visit};
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Expr, Fields, GenericArgument, GenericParam,
    Ident, LitStr, PathArguments, Type,
};

/// How a field value should be accessed when generating code.
#[derive(Clone)]
enum Accessor {
    /// Use the field directly (`self.field`).
    Field,
    /// Call a method on the field (`self.field.method()`).
    FieldMethod(Ident),
    /// Call a method on the struct (`self.method()`).
    SelfMethod(Ident),
}

impl Accessor {
    fn from_lit_str(value: &LitStr, is_self: bool) -> Self {
        let ident = Ident::new(&value.value(), Span::call_site());
        if is_self {
            Accessor::SelfMethod(ident)
        } else {
            Accessor::FieldMethod(ident)
        }
    }
}

#[derive(Clone, Copy)]
enum AccessTarget {
    Attr,
    BoolAttr,
    Class,
    Action,
    Children,
}

impl Accessor {
    fn to_expr(&self, field: &Ident, target: AccessTarget) -> proc_macro2::TokenStream {
        match (target, self.clone()) {
            (AccessTarget::Attr, Accessor::Field) => quote! { self.#field },
            (AccessTarget::Attr, Accessor::FieldMethod(method)) => {
                quote! { Some(self.#field.#method()) }
            }
            (AccessTarget::Attr, Accessor::SelfMethod(method)) => quote! { self.#method() },

            (AccessTarget::BoolAttr, Accessor::Field) => quote! { self.#field },
            (AccessTarget::BoolAttr, Accessor::FieldMethod(method)) => {
                quote! { self.#field.#method() }
            }
            (AccessTarget::BoolAttr, Accessor::SelfMethod(method)) => quote! { self.#method() },

            (AccessTarget::Class, Accessor::Field) => quote! { self.#field },
            (AccessTarget::Class, Accessor::FieldMethod(method)) => {
                quote! { self.#field.#method() }
            }
            (AccessTarget::Class, Accessor::SelfMethod(method)) => quote! { self.#method() },

            (AccessTarget::Action, Accessor::Field) => quote! { self.#field.as_ref() },
            (AccessTarget::Action, Accessor::FieldMethod(method)) => {
                quote! { self.#field.#method() }
            }
            (AccessTarget::Action, Accessor::SelfMethod(method)) => quote! { self.#method() },

            (AccessTarget::Children, Accessor::Field) => quote! { self.#field },
            (AccessTarget::Children, Accessor::FieldMethod(method)) => {
                quote! { self.#field.#method() }
            }
            (AccessTarget::Children, Accessor::SelfMethod(method)) => quote! { self.#method() },
        }
    }
}

struct AttrField {
    field: Ident,
    name: LitStr,
    accessor: Accessor,
}

struct BoolAttrField {
    field: Ident,
    name: LitStr,
    accessor: Accessor,
}

struct ActionField {
    field: Ident,
    attr_name: LitStr,
    accessor: Accessor,
}

#[derive(Clone)]
enum ConstructorDefault {
    DefaultTrait,
    Expression(Expr),
}

struct OptionalBuilder {
    method: Ident,
    source: LitStr,
}

struct AuthoringField {
    field: Ident,
    ty: Type,
    default: Option<ConstructorDefault>,
    optional_builder: Option<OptionalBuilder>,
}

struct AuthoringMethods {
    inherent: proc_macro2::TokenStream,
    extra_impls: proc_macro2::TokenStream,
}

/// Derive `ProaComponent` - generates rendering helpers.
// proa-perf-ok(scope): proc-macro host code folds compile-time-known tag and
// attribute fragments; it never assembles response HTML at render time
pub fn derive_proa_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;

    let config = match get_component_config(&input) {
        Ok(config) => config,
        Err(e) => return TokenStream::from(e.to_compile_error()),
    };
    let tag = &config.tag;
    let opening_tag = LitStr::new(&format!("<{tag}"), Span::call_site());
    let closing_tag = LitStr::new(&format!("</{tag}>"), Span::call_site());

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        name,
                        "ProaComponent can only be derived for structs with named fields",
                    )
                    .to_compile_error(),
                );
            }
        },
        _ => {
            return TokenStream::from(
                syn::Error::new_spanned(name, "ProaComponent can only be derived for structs")
                    .to_compile_error(),
            );
        }
    };

    let mut attr_fields: Vec<AttrField> = Vec::new();
    let mut bool_attr_fields: Vec<BoolAttrField> = Vec::new();
    let mut action_fields: Vec<ActionField> = Vec::new(); // Support multiple action fields!
    let mut class_field: Option<(Ident, Accessor)> = None;
    let mut children_field: Option<(Ident, Accessor, Type)> = None;
    let mut authoring_fields: Vec<AuthoringField> = Vec::new();

    for field in fields {
        let field_ident = match field.ident.clone() {
            Some(ident) => ident,
            None => {
                return TokenStream::from(
                    syn::Error::new_spanned(field, "ProaComponent requires named fields")
                        .to_compile_error(),
                );
            }
        };

        let mut attr_config: Option<(LitStr, Accessor)> = None;
        let mut bool_attr_config: Option<(LitStr, Accessor)> = None;
        let mut class_config: Option<Accessor> = None;
        let mut action_config: Option<(LitStr, Accessor)> = None; // (attr_name, accessor)
        let mut children_config: Option<Accessor> = None;
        let mut constructor_default: Option<ConstructorDefault> = None;
        let mut optional_builder: Option<OptionalBuilder> = None;

        for attr in &field.attrs {
            if !attr.path().is_ident("component") {
                continue;
            }

            let mut last_target: Option<AccessTarget> = None;

            let parse_result = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("attr") {
                    // attr must be followed by attr_name
                    attr_config = Some((LitStr::new("", Span::call_site()), Accessor::Field));
                    last_target = Some(AccessTarget::Attr);
                } else if meta.path.is_ident("bool_attr") {
                    // bool_attr must be followed by attr_name
                    bool_attr_config = Some((LitStr::new("", Span::call_site()), Accessor::Field));
                    last_target = Some(AccessTarget::BoolAttr);
                } else if meta.path.is_ident("class") {
                    class_config = Some(Accessor::Field);
                    last_target = Some(AccessTarget::Class);
                } else if meta.path.is_ident("action") {
                    // action must be followed by attr_name
                    action_config = Some((LitStr::new("", Span::call_site()), Accessor::Field));
                    last_target = Some(AccessTarget::Action);
                } else if meta.path.is_ident("children") {
                    children_config = Some(Accessor::Field);
                    last_target = Some(AccessTarget::Children);
                } else if meta.path.is_ident("attr_name") {
                    let lit: LitStr = meta.value()?.parse()?;
                    match last_target {
                        Some(AccessTarget::Attr) => {
                            if let Some((attr_name, _accessor)) = attr_config.as_mut() {
                                *attr_name = lit;
                            }
                        }
                        Some(AccessTarget::BoolAttr) => {
                            if let Some((attr_name, _accessor)) = bool_attr_config.as_mut() {
                                *attr_name = lit;
                            }
                        }
                        Some(AccessTarget::Action) => {
                            if let Some((attr_name, _accessor)) = action_config.as_mut() {
                                *attr_name = lit;
                            }
                        }
                        _ => {
                            return Err(meta.error("attr_name must follow attr, bool_attr, or action"));
                        }
                    }
                } else if meta.path.is_ident("map") {
                    let lit: LitStr = meta.value()?.parse()?;
                    let accessor = Accessor::from_lit_str(&lit, false);
                    match last_target {
                        Some(AccessTarget::Attr) => {
                            if let Some((_name, acc)) = attr_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::BoolAttr) => {
                            if let Some((_name, acc)) = bool_attr_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::Class) => {
                            class_config = Some(accessor);
                        }
                        Some(AccessTarget::Action) => {
                            if let Some((_attr_name, acc)) = action_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::Children) => {
                            children_config = Some(accessor);
                        }
                        None => {
                            return Err(meta.error("map must follow a component target (attr/bool_attr/class/action/children)"));
                        }
                    }
                } else if meta.path.is_ident("via") {
                    let lit: LitStr = meta.value()?.parse()?;
                    let accessor = Accessor::from_lit_str(&lit, true);
                    match last_target {
                        Some(AccessTarget::Attr) => {
                            if let Some((_name, acc)) = attr_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::BoolAttr) => {
                            if let Some((_name, acc)) = bool_attr_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::Class) => {
                            class_config = Some(accessor);
                        }
                        Some(AccessTarget::Action) => {
                            if let Some((_attr_name, acc)) = action_config.as_mut() {
                                *acc = accessor;
                            }
                        }
                        Some(AccessTarget::Children) => {
                            children_config = Some(accessor);
                        }
                        None => {
                            return Err(meta.error("via must follow a component target (attr/bool_attr/class/action/children)"));
                        }
                    }
                } else if meta.path.is_ident("default") {
                    if constructor_default.is_some() {
                        return Err(meta.error(
                            "duplicate `default`; configure a field's constructor default once",
                        ));
                    }
                    constructor_default = if meta.input.peek(syn::Token![=]) {
                        Some(ConstructorDefault::Expression(meta.value()?.parse()?))
                    } else {
                        Some(ConstructorDefault::DefaultTrait)
                    };
                } else if meta.path.is_ident("optional_builder") {
                    if optional_builder.is_some() {
                        return Err(meta.error(
                            "duplicate `optional_builder`; configure one builder per field",
                        ));
                    }
                    let source: LitStr = meta.value()?.parse()?;
                    let method = syn::parse_str::<Ident>(&source.value()).map_err(|_| {
                        syn::Error::new(
                            source.span(),
                            "`optional_builder` must name a valid Rust identifier, for example `optional_builder = \"with_id\"`",
                        )
                    })?;
                    optional_builder = Some(OptionalBuilder { method, source });
                }
                Ok(())
            });
            if let Err(e) = parse_result {
                return TokenStream::from(e.to_compile_error());
            }
        }

        if let Some((name, accessor)) = attr_config {
            if name.value().is_empty() {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        &field_ident,
                        "#[component(attr)] requires attr_name parameter: #[component(attr, attr_name = \"...\")]",
                    )
                    .to_compile_error(),
                );
            }
            attr_fields.push(AttrField {
                field: field_ident.clone(),
                name,
                accessor,
            });
        }

        if let Some((name, accessor)) = bool_attr_config {
            if name.value().is_empty() {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        &field_ident,
                        "#[component(bool_attr)] requires attr_name parameter: #[component(bool_attr, attr_name = \"...\")]",
                    )
                    .to_compile_error(),
                );
            }
            bool_attr_fields.push(BoolAttrField {
                field: field_ident.clone(),
                name,
                accessor,
            });
        }

        if let Some(accessor) = class_config {
            class_field = Some((field_ident.clone(), accessor));
        }

        if let Some((attr_name, accessor)) = action_config {
            if attr_name.value().is_empty() {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        &field_ident,
                        "#[component(action)] requires attr_name parameter: #[component(action, attr_name = \"...\")]",
                    )
                    .to_compile_error(),
                );
            }
            action_fields.push(ActionField {
                field: field_ident.clone(),
                attr_name,
                accessor,
            });
        }

        if let Some(accessor) = children_config {
            if config.children_builder && children_field.is_some() {
                return TokenStream::from(
                    syn::Error::new_spanned(
                        &field_ident,
                        "`children_builder` requires exactly one #[component(children)] field",
                    )
                    .to_compile_error(),
                );
            }
            children_field = Some((field_ident.clone(), accessor, field.ty.clone()));
        }

        authoring_fields.push(AuthoringField {
            field: field_ident,
            ty: field.ty.clone(),
            default: constructor_default,
            optional_builder,
        });
    }

    let children_builder_method = if config.children_builder {
        match generate_children_builder(&input, fields, children_field.as_ref()) {
            Ok(method) => method,
            Err(error) => return TokenStream::from(error.to_compile_error()),
        }
    } else {
        quote! {}
    };

    let authoring_methods = match generate_authoring_methods(
        &input,
        &config,
        &authoring_fields,
        children_field.as_ref(),
    ) {
        Ok(methods) => methods,
        Err(error) => return TokenStream::from(error.to_compile_error()),
    };
    let inherent_authoring_methods = authoring_methods.inherent;
    let extra_authoring_impls = authoring_methods.extra_impls;

    let attr_render_block = attr_fields.iter().map(|attr_field| {
        let AttrField {
            field,
            name,
            accessor,
        } = attr_field;
        let expr = accessor.to_expr(field, AccessTarget::Attr);
        let prefix = LitStr::new(&format!(" {}=\"", name.value()), name.span());
        quote! {
            if let Some(value) = { #expr } {
                proa_core::WebRenderSync::render(UnsafeRaw(#prefix), cx)?;
                proa_core::shared::escape::escape_attr_value(&mut cx.out, value)?;
                proa_core::WebRenderSync::render(UnsafeRaw("\""), cx)?;
            }
        }
    });

    let bool_attr_render_block = bool_attr_fields.iter().map(|bool_attr_field| {
        let BoolAttrField {
            field,
            name,
            accessor,
        } = bool_attr_field;
        let expr = accessor.to_expr(field, AccessTarget::BoolAttr);
        let attribute = LitStr::new(&format!(" {}", name.value()), name.span());
        quote! {
            if { #expr } {
                proa_core::WebRenderSync::render(UnsafeRaw(#attribute), cx)?;
            }
        }
    });

    let class_render_block = if let Some((field, accessor)) = &class_field {
        let expr = accessor.to_expr(field, AccessTarget::Class);
        quote! {
            let class_slice = &classes[..class_count];
            let custom_class = { #expr };
            if !class_slice.is_empty() || custom_class.is_some() {
                proa_core::WebRenderSync::render(UnsafeRaw(" class=\""), cx)?;
                for (idx, class) in class_slice.iter().enumerate() {
                    if idx > 0 {
                        proa_core::WebRenderSync::render(UnsafeRaw(" "), cx)?;
                    }
                    proa_core::WebRenderSync::render(UnsafeRaw(class), cx)?;
                }
                if let Some(custom) = custom_class {
                    if !class_slice.is_empty() {
                        proa_core::WebRenderSync::render(UnsafeRaw(" "), cx)?;
                    }
                    proa_core::WebRenderSync::render(UnsafeRaw(custom), cx)?;
                }
                proa_core::WebRenderSync::render(UnsafeRaw("\""), cx)?;
            }
        }
    } else {
        quote! {
            let class_slice = &classes[..class_count];
            if !class_slice.is_empty() {
                proa_core::WebRenderSync::render(UnsafeRaw(" class=\""), cx)?;
                for (idx, class) in class_slice.iter().enumerate() {
                    if idx > 0 {
                        proa_core::WebRenderSync::render(UnsafeRaw(" "), cx)?;
                    }
                    proa_core::WebRenderSync::render(UnsafeRaw(class), cx)?;
                }
                proa_core::WebRenderSync::render(UnsafeRaw("\""), cx)?;
            }
        }
    };

    let action_render_block = action_fields.iter().map(|action_field| {
        let ActionField {
            field,
            attr_name,
            accessor,
        } = action_field;
        let expr = accessor.to_expr(field, AccessTarget::Action);
        let prefix = LitStr::new(&format!(" {}=\"", attr_name.value()), attr_name.span());
        quote! {
            if let Some(action_data) = { #expr } {
                proa_core::WebRenderSync::render(UnsafeRaw(#prefix), cx)?;
                proa_core::shared::escape::escape_attr_value(&mut cx.out, action_data.action)?;
                proa_core::WebRenderSync::render(UnsafeRaw("\""), cx)?;

                // Parse params JSON string and render data-param-* attributes
                let params_value = action_data.params_value();
                if let Some(params_obj) = params_value.as_object() {
                    for (key, value) in params_obj {
                        // Validate key as a safe HTML attribute name segment.
                        // Only allow ASCII alphanumeric, underscore, and hyphen.
                        // HTML-escaping the key is wrong because entities are
                        // invalid inside attribute names. Skip unsafe keys.
                        if key.is_empty()
                            || !key.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
                            })
                        {
                            continue;
                        }
                        proa_core::WebRenderSync::render(UnsafeRaw(" data-param-"), cx)?;
                        proa_core::WebRenderSync::render(UnsafeRaw(key.as_str()), cx)?;
                        proa_core::WebRenderSync::render(UnsafeRaw("=\""), cx)?;
                        match value {
                            ::serde_json::Value::String(s) => {
                                proa_core::shared::escape::escape_attr_value(&mut cx.out, s.as_str())?;
                            }
                            ::serde_json::Value::Number(n) => {
                                let s = n.to_string();
                                proa_core::shared::escape::escape_attr_value(&mut cx.out, &s)?;
                            }
                            ::serde_json::Value::Bool(b) => {
                                proa_core::WebRenderSync::render(
                                    UnsafeRaw(if *b { "true" } else { "false" }),
                                    cx,
                                )?;
                            }
                            v => {
                                let s = v.to_string();
                                proa_core::shared::escape::escape_attr_value(&mut cx.out, &s)?;
                            }
                        }
                        proa_core::WebRenderSync::render(UnsafeRaw("\""), cx)?;
                    }
                }
            }
        }
    });

    let children_render_block = children_field
        .as_ref()
        .map(|(field, accessor, _)| {
            let expr = accessor.to_expr(field, AccessTarget::Children);
            quote! {
                proa_core::WebRenderSync::render(#expr, cx)?;
            }
        })
        .unwrap_or_else(|| quote! {});

    let generics = input.generics.clone();
    let (inherent_impl_generics, ty_generics, inherent_where_clause) = generics.split_for_impl();

    // The component's own generic parameters remain visible, while the
    // loader and buffer parameters are generated here rather than leaking
    // into routine component authoring.
    let mut render_generics = input.generics.clone();
    render_generics
        .params
        .push(parse_quote!(__ProaLoader: proa_core::DataLoader));
    render_generics
        .params
        .push(parse_quote!(__ProaBuffer: proa_core::WriteBuf));

    if let Some((_, Accessor::Field, child_ty)) = &children_field {
        render_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #child_ty: proa_core::WebRenderSync<__ProaLoader, __ProaBuffer>
            ));
    }

    let (render_impl_generics, _, render_where_clause) = render_generics.split_for_impl();

    let async_render_impl = config.async_render.then(|| {
        quote! {
            impl #render_impl_generics proa_core::WebRender<__ProaLoader, __ProaBuffer>
                for #name #ty_generics
                #render_where_clause
            {
                fn render(
                    self,
                    cx: &mut proa_core::web::WebContext<__ProaLoader, __ProaBuffer>,
                ) -> proa_core::RenderOutcome<
                    impl ::core::future::Future<
                        Output = Result<(), proa_core::shared::write_buf::WriteError>,
                    >,
                >
                {
                    proa_core::RenderOutcome::<
                        ::core::future::Ready<
                            Result<(), proa_core::shared::write_buf::WriteError>,
                        >,
                    >::Done(proa_core::WebRenderSync::render(self, cx))
                }
            }
        }
    });

    let expanded = quote! {
        impl #inherent_impl_generics #name #ty_generics #inherent_where_clause {
            #inherent_authoring_methods
            #children_builder_method

            #[inline(always)]
            pub fn render_opening<
                __ProaBuffer: proa_core::WriteBuf,
                __ProaLoader: proa_core::DataLoader,
            >(
                &self,
                cx: &mut proa_core::web::WebContext<__ProaLoader, __ProaBuffer>,
            ) -> Result<(), proa_core::shared::write_buf::WriteError> {
                #[allow(unused_imports)]
                use proa_core::text::UnsafeRaw;
                #[allow(unused_imports)]
                use proa_core::WebRenderSync;

                proa_core::WebRenderSync::render(UnsafeRaw(#opening_tag), cx)?;

                let (classes, class_count) = self.classes();
                #class_render_block

                #(#attr_render_block)*
                #(#bool_attr_render_block)*
                #(#action_render_block)*

                proa_core::WebRenderSync::render(UnsafeRaw(">"), cx)?;
                Ok(())
            }

            #[inline(always)]
            pub fn render_closing<
                __ProaBuffer: proa_core::WriteBuf,
                __ProaLoader: proa_core::DataLoader,
            >(
                &self,
                cx: &mut proa_core::web::WebContext<__ProaLoader, __ProaBuffer>,
            ) -> Result<(), proa_core::shared::write_buf::WriteError> {
                Self::__proa_render_closing(cx)
            }

            #[doc(hidden)]
            #[inline(always)]
            fn __proa_render_closing<
                __ProaBuffer: proa_core::WriteBuf,
                __ProaLoader: proa_core::DataLoader,
            >(
                cx: &mut proa_core::web::WebContext<__ProaLoader, __ProaBuffer>,
            ) -> Result<(), proa_core::shared::write_buf::WriteError> {
                use proa_core::text::UnsafeRaw;
                use proa_core::WebRenderSync;

                proa_core::WebRenderSync::render(UnsafeRaw(#closing_tag), cx)?;
                Ok(())
            }
        }

        impl #render_impl_generics proa_core::WebRenderSync<__ProaLoader, __ProaBuffer>
            for #name #ty_generics
            #render_where_clause
        {
            #[inline(always)]
            fn render(
                self,
                cx: &mut proa_core::web::WebContext<__ProaLoader, __ProaBuffer>,
            ) -> Result<(), proa_core::shared::write_buf::WriteError> {
                self.render_opening(cx)?;
                #children_render_block
                Self::__proa_render_closing(cx)?;
                Ok(())
            }
        }

        #extra_authoring_impls
        #async_render_impl
    };

    TokenStream::from(expanded)
}

struct ComponentConfig {
    tag: String,
    async_render: bool,
    children_builder: bool,
    constructor: bool,
}

fn get_component_config(input: &DeriveInput) -> Result<ComponentConfig, syn::Error> {
    let mut tag = None;
    let mut async_render = false;
    let mut children_builder = false;
    let mut constructor = false;

    for attr in &input.attrs {
        if !attr.path().is_ident("component") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                tag = Some(lit.value());
            } else if meta.path.is_ident("async_render") {
                async_render = true;
            } else if meta.path.is_ident("children_builder") {
                children_builder = true;
            } else if meta.path.is_ident("constructor") {
                constructor = true;
            }
            Ok(())
        })?;
    }

    if let Some(tag) = tag {
        return Ok(ComponentConfig {
            tag,
            async_render,
            children_builder,
            constructor,
        });
    }

    Err(syn::Error::new_spanned(
        &input.ident,
        "ProaComponent requires #[component(tag = \"element\")] attribute",
    ))
}

// proa-perf-ok(scope): constructor and builder syntax is assembled by the
// proc-macro host; generated methods only move fields and construct `Self`
fn generate_authoring_methods(
    input: &DeriveInput,
    config: &ComponentConfig,
    fields: &[AuthoringField],
    children_field: Option<&(Ident, Accessor, Type)>,
) -> Result<AuthoringMethods, syn::Error> {
    if !config.constructor {
        if let Some(field) = fields.iter().find(|field| field.default.is_some()) {
            return Err(syn::Error::new_spanned(
                &field.field,
                "`default` configures the generated constructor; add `constructor` to the struct-level #[component(...)] attribute or remove this field default",
            ));
        }
    }

    if config.children_builder {
        if let Some((child, _, _)) = children_field {
            if let Some(field) = fields.iter().find(|field| field.field == *child) {
                if field.default.is_some() {
                    return Err(syn::Error::new_spanned(
                        &field.field,
                        "`children_builder` already initializes its direct child to None in the generated constructor; remove the child field's `default`",
                    ));
                }
                if field.optional_builder.is_some() {
                    return Err(syn::Error::new_spanned(
                        &field.field,
                        "`optional_builder` cannot also target the direct `children_builder` field; use the generated type-replacing `with_children` method",
                    ));
                }
            }
        }
    }

    let mut generated_builder_names: Vec<(&Ident, &Ident)> = Vec::new();
    let mut optional_builder_methods = Vec::new();

    for field in fields {
        let Some(builder) = &field.optional_builder else {
            continue;
        };

        let inner_ty = option_inner_type(&field.ty).ok_or_else(|| {
            syn::Error::new_spanned(
                &field.ty,
                "`optional_builder` requires a field with the exact shape Option<T>, std::option::Option<T>, or core::option::Option<T>",
            )
        })?;

        let method_name = builder.method.to_string();
        let reserved_reason = match method_name.as_str() {
            "classes" => Some("the component's required `classes` method"),
            "render" | "render_opening" | "render_closing" | "__proa_render_closing" => {
                Some("the generated component render API")
            }
            "new" if config.constructor => Some("the generated constructor"),
            "with_children" if config.children_builder => {
                Some("the generated `children_builder` method")
            }
            _ => None,
        };
        if let Some(reason) = reserved_reason {
            return Err(syn::Error::new_spanned(
                &builder.source,
                format!(
                    "optional builder method `{}` conflicts with {reason}; choose a different explicit method name",
                    builder.method
                ),
            ));
        }

        if let Some((previous_field, _)) = generated_builder_names
            .iter()
            .find(|(_, method)| **method == builder.method)
        {
            return Err(syn::Error::new_spanned(
                &builder.source,
                format!(
                    "optional builder method `{}` is already generated for field `{previous_field}`; every builder method name must be unique",
                    builder.method
                ),
            ));
        }
        generated_builder_names.push((&field.field, &builder.method));

        let field_name = &field.field;
        let method = &builder.method;
        optional_builder_methods.push(quote! {
            /// Sets this optional prop to `Some(value)`.
            #[must_use]
            pub fn #method(mut self, value: #inner_ty) -> Self {
                self.#field_name = ::core::option::Option::Some(value);
                self
            }
        });
    }

    let specialized_child = if config.constructor && config.children_builder {
        children_field
            .and_then(|(_, _, ty)| option_type_parameter(ty))
            .cloned()
    } else {
        None
    };

    let constructor_method = if config.constructor {
        let child_field = children_field.map(|(field, _, _)| field);
        let mut parameters = Vec::new();
        let mut initializers = Vec::new();
        let mut default_bounds = Vec::new();

        for field in fields {
            let field_name = &field.field;
            let field_ty = &field.ty;
            let initializer = match &field.default {
                Some(ConstructorDefault::DefaultTrait) => {
                    if type_mentions_any_non_lifetime_parameter(field_ty, &input.generics) {
                        default_bounds.push(field_ty);
                    }
                    quote! {
                        #field_name: ::core::default::Default::default()
                    }
                }
                Some(ConstructorDefault::Expression(expression)) => {
                    quote! { #field_name: #expression }
                }
                None if field.optional_builder.is_some()
                    || (config.children_builder
                        && child_field.is_some_and(|child| child == field_name)) =>
                {
                    quote! { #field_name: ::core::option::Option::None }
                }
                None => {
                    parameters.push(quote! { #field_name: #field_ty });
                    quote! { #field_name }
                }
            };
            initializers.push(initializer);
        }

        let default_where_clause = if default_bounds.is_empty() {
            quote! {}
        } else {
            quote! {
                where
                    #(#default_bounds: ::core::default::Default,)*
            }
        };

        quote! {
            /// Constructs this component from its required props.
            ///
            /// Fields with `default`, `default = <expr>`, an
            /// `optional_builder`, or the direct `children_builder` child are
            /// initialized without constructor arguments.
            #[must_use]
            pub fn new(#(#parameters),*) -> Self
            #default_where_clause
            {
                Self {
                    #(#initializers,)*
                }
            }
        }
    } else {
        quote! {}
    };

    let (inherent_constructor, extra_impls) = if let Some(child_generic) = specialized_child {
        let mut specialized_generics = input.generics.clone();
        specialized_generics.params = input
            .generics
            .params
            .iter()
            .filter(|param| {
                !matches!(
                    param,
                    GenericParam::Type(type_param) if type_param.ident == child_generic
                )
            })
            .cloned()
            .collect();
        let (impl_generics, _, where_clause) = specialized_generics.split_for_impl();
        let component_name = &input.ident;
        let specialized_type = component_type_replacing_parameter(
            component_name,
            &input.generics,
            &child_generic,
            quote! { () },
        );

        (
            quote! {},
            quote! {
                impl #impl_generics #specialized_type #where_clause {
                    #constructor_method
                }
            },
        )
    } else {
        (constructor_method, quote! {})
    };

    Ok(AuthoringMethods {
        inherent: quote! {
            #inherent_constructor
            #(#optional_builder_methods)*
        },
        extra_impls,
    })
}

fn component_type_replacing_parameter(
    name: &Ident,
    generics: &syn::Generics,
    target: &Ident,
    replacement: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let replacement = &replacement;
    let output_arguments = generics.params.iter().map(|param| match param {
        GenericParam::Lifetime(param) => {
            let lifetime = &param.lifetime;
            quote! { #lifetime }
        }
        GenericParam::Type(param) if param.ident == *target => {
            quote! { #replacement }
        }
        GenericParam::Type(param) => {
            let ident = &param.ident;
            quote! { #ident }
        }
        GenericParam::Const(param) => {
            let ident = &param.ident;
            quote! { #ident }
        }
    });
    quote! { #name<#(#output_arguments),*> }
}

// proa-perf-ok(scope): derive-time diagnostics quote component syntax; this
// proc-macro host code never assembles or renders response HTML
fn generate_children_builder(
    input: &DeriveInput,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    children_field: Option<&(Ident, Accessor, Type)>,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = &input.ident;
    let (child_field, accessor, child_field_ty) = children_field.ok_or_else(|| {
        syn::Error::new_spanned(
            name,
            "`children_builder` requires a direct #[component(children)] field of type Option<C>",
        )
    })?;

    if !matches!(accessor, Accessor::Field) {
        return Err(syn::Error::new_spanned(
            child_field,
            "`children_builder` only supports a direct #[component(children)] field; `map` and `via` accessors require a handwritten builder",
        ));
    }

    let child_generic = option_type_parameter(child_field_ty).ok_or_else(|| {
        syn::Error::new_spanned(
            child_field_ty,
            "`children_builder` requires the #[component(children)] field to have the shape Option<C>, where C is a type parameter",
        )
    })?;

    let child_param = input
        .generics
        .type_params()
        .find(|param| param.ident == *child_generic)
        .ok_or_else(|| {
            syn::Error::new_spanned(
                child_field_ty,
                format!(
                    "`children_builder` child type `{child_generic}` must be a type parameter declared by this component"
                ),
            )
        })?;

    if !child_param.bounds.is_empty() {
        return Err(syn::Error::new_spanned(
            &child_param.bounds,
            format!(
                "`children_builder` requires child type parameter `{child_generic}` to be unconstrained; put render bounds on generated implementations or write `with_children` manually"
            ),
        ));
    }

    for field in fields {
        if field.ident.as_ref() == Some(child_field) {
            continue;
        }
        if type_mentions_parameter(&field.ty, child_generic) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!(
                    "`children_builder` cannot replace `{child_generic}` because field `{}` also uses it; the child type parameter must be used only by the #[component(children)] field",
                    field
                        .ident
                        .as_ref()
                        .map_or_else(|| "<unnamed>".to_owned(), ToString::to_string)
                ),
            ));
        }
    }

    if generics_mention_parameter(&input.generics, child_generic) {
        return Err(syn::Error::new_spanned(
            &input.generics,
            format!(
                "`children_builder` cannot replace `{child_generic}` because a generic bound, default, or where-clause also uses it; keep the child parameter unconstrained or write `with_children` manually"
            ),
        ));
    }

    let new_child = unique_new_child_ident(&input.generics);
    let output_type = component_type_replacing_parameter(
        name,
        &input.generics,
        child_generic,
        quote! { #new_child },
    );

    let field_initializers = fields.iter().map(|field| {
        let ident = field
            .ident
            .as_ref()
            .expect("named fields were validated before builder generation");
        if ident == child_field {
            quote! { #ident: ::core::option::Option::Some(child) }
        } else {
            quote! { #ident: self.#ident }
        }
    });

    Ok(quote! {
        /// Replaces this component's optional child while preserving every
        /// other field and generic argument.
        #[must_use]
        pub fn with_children<#new_child>(self, child: #new_child) -> #output_type {
            #name {
                #(#field_initializers,)*
            }
        }
    })
}

fn option_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }

    let segments: Vec<_> = type_path.path.segments.iter().collect();
    let is_option = match segments.as_slice() {
        [option] => option.ident == "Option",
        [root, module, option] => {
            (root.ident == "std" || root.ident == "core")
                && module.ident == "option"
                && option.ident == "Option"
        }
        _ => false,
    };
    if !is_option {
        return None;
    }

    let option = segments.last()?;
    let PathArguments::AngleBracketed(arguments) = &option.arguments else {
        return None;
    };
    let [GenericArgument::Type(inner)] = arguments.args.iter().collect::<Vec<_>>().as_slice()
    else {
        return None;
    };
    Some(inner)
}

fn option_type_parameter(ty: &Type) -> Option<&Ident> {
    let Type::Path(child_path) = option_inner_type(ty)? else {
        return None;
    };
    if child_path.qself.is_some() || child_path.path.segments.len() != 1 {
        return None;
    }
    let child = child_path.path.segments.first()?;
    if !matches!(child.arguments, PathArguments::None) {
        return None;
    }
    Some(&child.ident)
}

struct ParameterUseVisitor<'a> {
    target: &'a Ident,
    found: bool,
}

impl<'ast> Visit<'ast> for ParameterUseVisitor<'_> {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if path
            .path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == *self.target)
        {
            self.found = true;
            return;
        }
        visit::visit_type_path(self, path);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path
            .path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == *self.target)
        {
            self.found = true;
            return;
        }
        visit::visit_expr_path(self, path);
    }
}

fn type_mentions_parameter(ty: &Type, target: &Ident) -> bool {
    let mut visitor = ParameterUseVisitor {
        target,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

fn type_mentions_any_non_lifetime_parameter(ty: &Type, generics: &syn::Generics) -> bool {
    generics.params.iter().any(|param| match param {
        GenericParam::Type(param) => type_mentions_parameter(ty, &param.ident),
        GenericParam::Const(param) => type_mentions_parameter(ty, &param.ident),
        GenericParam::Lifetime(_) => false,
    })
}

fn generics_mention_parameter(generics: &syn::Generics, target: &Ident) -> bool {
    let mut visitor = ParameterUseVisitor {
        target,
        found: false,
    };
    visitor.visit_generics(generics);
    visitor.found
}

fn unique_new_child_ident(generics: &syn::Generics) -> Ident {
    let mut candidate = "__ProaNewChild".to_owned();
    while generics.params.iter().any(|param| match param {
        GenericParam::Lifetime(param) => param.lifetime.ident == candidate,
        GenericParam::Type(param) => param.ident == candidate,
        GenericParam::Const(param) => param.ident == candidate,
    }) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::mixed_site())
}
