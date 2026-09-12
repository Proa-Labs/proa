// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Closed Rust value-object modules for `#[rsjs::client]`.
//!
//! Authored structs and impls remain as rustc witnesses. Each constructor,
//! receiver method, and generated field projection becomes a private linked
//! helper definition, so method bodies are emitted once rather than copied
//! into every public root. The core JavaScript representation remains a plain
//! object plus explicit functions; a public `class` is a later ABI wrapper.

use std::collections::BTreeMap;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use sha2::{Digest, Sha256};
use syn::fold::{self, Fold};
use syn::spanned::Spanned;

#[derive(Clone)]
struct ScalarType {
    rust_name: String,
    rust_ty: syn::Type,
}

#[derive(Clone)]
struct ObjectField {
    ident: Ident,
    scalar: ScalarType,
    getter: Ident,
    setter: Ident,
}

#[derive(Clone)]
struct ObjectSpec {
    ident: Ident,
    type_key: String,
    fields: Vec<ObjectField>,
    literal_constructor: Ident,
    clone_helper: Ident,
    exported: bool,
}

impl ObjectSpec {
    fn internal_spec(&self) -> crate::client_attr::InternalValueObjectSpec {
        crate::client_attr::InternalValueObjectSpec {
            type_key: self.type_key.clone(),
            fields: self
                .fields
                .iter()
                .map(|field| (field.ident.to_string(), field.scalar.rust_name.clone()))
                .collect(),
            rust_type_names: vec![self.ident.to_string()],
        }
    }

    fn field(&self, name: &Ident) -> Option<&ObjectField> {
        self.fields.iter().find(|field| field.ident == *name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MethodKind {
    Constructor,
    Receiver { mutable: bool },
}

#[derive(Clone)]
struct ObjectMethod {
    owner: String,
    method: syn::ImplItemFn,
    hidden: Ident,
    kind: MethodKind,
    params: Vec<(Ident, ScalarType)>,
    return_type: MethodReturn,
}

#[derive(Clone)]
enum MethodReturn {
    Unit,
    Scalar(ScalarType),
    Object,
}

#[derive(Default)]
struct ObjectCatalog {
    objects: BTreeMap<String, ObjectSpec>,
    methods: BTreeMap<(String, String), ObjectMethod>,
}

pub(crate) fn expand(module: syn::ItemMod, export_classes: bool) -> TokenStream {
    match try_expand(module, export_classes) {
        Ok(module) => module.into_token_stream(),
        Err(error) => error.to_tokens(),
    }
}

fn try_expand(
    mut module: syn::ItemMod,
    export_classes: bool,
) -> Result<syn::ItemMod, ExpansionError> {
    let (brace, items) = module.content.take().ok_or_else(|| {
        Diagnostic::new(
            module.ident.span(),
            "`#[rsjs::client]` value-object modules must be inline",
            "write `pub mod name { ... }` with the structs, impls, and helper roots in the body",
            "the proc macro needs the method bodies and helper call sites in one syntax tree",
        )
    })?;
    let catalog = ObjectCatalog::collect(&module, &items)?;
    if catalog.objects.is_empty() {
        return Err(Diagnostic::new(
            module.ident.span(),
            "a `#[rsjs::client]` module must declare at least one named-field struct",
            "add the client value-object struct and its inherent impl to this inline module",
            "free-function-only groups should continue to use `#[rsjs::client_module]`",
        )
        .into());
    }

    let class_module = export_classes
        .then(|| generate_class_module(&catalog))
        .transpose()?;
    let mut output = Vec::new();
    let mut root_count = 0usize;
    for item in items {
        let syn::Item::Fn(function) = item else {
            output.push(item);
            continue;
        };
        if !matches!(function.vis, syn::Visibility::Public(_)) {
            output.push(syn::Item::Fn(function));
            continue;
        }

        root_count += 1;
        let original = function.clone();
        let mut rewritten = function;
        rewritten
            .attrs
            .retain(|attribute| !is_client_attribute(attribute));
        rewrite_public_root(&mut rewritten, &catalog)?;
        ensure_unit_tail(&mut rewritten);

        let expanded = crate::client_attr::expand_rewritten_object_root(&rewritten)
            .map_err(ExpansionError::Expanded)?;
        output.extend(parse_expanded_items(expanded, rewritten.sig.ident.span())?);
        output.push(syn::Item::Fn(source_check(original)));
    }
    if root_count == 0 && !export_classes {
        return Err(Diagnostic::new(
            module.ident.span(),
            "a `#[rsjs::client]` value-object module needs at least one public helper root",
            "add a `pub fn` with a scalar boundary that constructs and uses the object",
            "objects remain helper-internal; the public function is the reachable transport-safe entry point",
        )
        .into());
    }

    if root_count > 0 {
        for object in catalog.objects.values() {
            for helper in generated_field_helpers(object) {
                output.extend(expand_internal_helper(helper, object)?);
            }
        }
        for method in catalog.methods.values() {
            let object = catalog
                .objects
                .get(&method.owner)
                .expect("catalog methods always have a catalog object owner");
            output.extend(expand_internal_helper(
                generated_method_helper(method, &catalog)?,
                object,
            )?);
        }
    }
    if let Some(class_module) = class_module {
        output.extend(class_manifest_items(&class_module, module.ident.span())?);
    }

    module.content = Some((brace, output));
    Ok(module)
}

fn expand_internal_helper(
    helper: syn::ItemFn,
    object: &ObjectSpec,
) -> Result<Vec<syn::Item>, ExpansionError> {
    let expanded =
        crate::client_attr::expand_internal_value_object_helper(&helper, &object.internal_spec())
            .map_err(ExpansionError::Expanded)?;
    parse_expanded_items(expanded, helper.sig.ident.span())
}

fn parse_expanded_items(
    expanded: TokenStream,
    span: Span,
) -> Result<Vec<syn::Item>, ExpansionError> {
    syn::parse2::<syn::File>(expanded)
        .map(|file| file.items)
        .map_err(|error| {
            Diagnostic::new(
                span,
                "failed to parse generated linked value-object helper items",
                "report this as an rsjs compiler bug",
                &error.to_string(),
            )
            .into()
        })
}

fn source_check(mut function: syn::ItemFn) -> syn::ItemFn {
    function.vis = syn::Visibility::Inherited;
    function.sig.ident = format_ident!(
        "__rsjs_client_object_source_check_{}",
        function.sig.ident,
        span = function.sig.ident.span()
    );
    function.attrs = vec![syn::parse_quote!(#[allow(dead_code)])];
    function
}

fn is_client_attribute(attribute: &syn::Attribute) -> bool {
    attribute
        .path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "client")
}

impl ObjectCatalog {
    fn collect(module: &syn::ItemMod, items: &[syn::Item]) -> Result<Self, ExpansionError> {
        let mut catalog = Self::default();
        for (ordinal, item) in items.iter().enumerate() {
            let syn::Item::Struct(item_struct) = item else {
                continue;
            };
            validate_plain_ident(&item_struct.ident, "client value-object struct")?;
            if !item_struct.generics.params.is_empty() {
                return Err(Diagnostic::new(
                    item_struct.generics.span(),
                    "client value-object structs cannot be generic yet",
                    "use one concrete named-field struct",
                    "the nominal helper-internal identity does not carry monomorphization arguments yet",
                )
                .into());
            }
            let syn::Fields::Named(named) = &item_struct.fields else {
                return Err(Diagnostic::new(
                    item_struct.fields.span(),
                    "client value objects must use named fields",
                    "replace tuple/unit fields with named scalar fields",
                    "Client IR object places address stable property names",
                )
                .into());
            };
            let fields = named
                .named
                .iter()
                .map(|field| {
                    let ident = field.ident.clone().expect("named field identifier");
                    let scalar = scalar_type(&field.ty).ok_or_else(|| {
                        Diagnostic::new(
                            field.ty.span(),
                            "unsupported client value-object field type",
                            "use bool, i32/u32/i64/u64, f64, String, or &str fields",
                            "the first nominal object schema carries scalar own properties only",
                        )
                    })?;
                    Ok(ObjectField {
                        getter: format_ident!(
                            "__rsjs_object_{}_get_{}",
                            snake_case_ident(&item_struct.ident),
                            ident,
                            span = ident.span()
                        ),
                        setter: format_ident!(
                            "__rsjs_object_{}_set_{}",
                            snake_case_ident(&item_struct.ident),
                            ident,
                            span = ident.span()
                        ),
                        ident,
                        scalar,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            let owner = item_struct.ident.to_string();
            let object = ObjectSpec {
                ident: item_struct.ident.clone(),
                type_key: value_object_type_key(module, item_struct, ordinal),
                fields,
                literal_constructor: format_ident!(
                    "__rsjs_object_{}_from_fields",
                    snake_case_ident(&item_struct.ident),
                    span = item_struct.ident.span()
                ),
                clone_helper: format_ident!(
                    "__rsjs_object_{}_clone_value",
                    snake_case_ident(&item_struct.ident),
                    span = item_struct.ident.span()
                ),
                exported: matches!(item_struct.vis, syn::Visibility::Public(_)),
            };
            if catalog.objects.insert(owner, object).is_some() {
                return Err(Diagnostic::new(
                    item_struct.ident.span(),
                    "duplicate client value-object struct name",
                    "give each object type a unique name inside the module",
                    "linked helper names derive from the closed object name",
                )
                .into());
            }
        }

        for item in items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            let Some(owner) = simple_type_name(&item_impl.self_ty) else {
                continue;
            };
            if !catalog.objects.contains_key(&owner) {
                continue;
            }
            if item_impl.trait_.is_some() {
                return Err(Diagnostic::new(
                    item_impl.impl_token.span(),
                    "client value objects cannot have trait impls",
                    "move trait behavior outside the client object module and expose explicit inherent methods",
                    "JavaScript class emission cannot preserve implicit trait dispatch or `Drop` cleanup",
                )
                .into());
            }
            if !item_impl.generics.params.is_empty() || item_impl.generics.where_clause.is_some() {
                return Err(Diagnostic::new(
                    item_impl.generics.span(),
                    "client value-object impls cannot be generic yet",
                    "use a concrete inherent impl",
                    "linked receiver definitions need one closed nominal object identity",
                )
                .into());
            }
            for impl_item in &item_impl.items {
                let syn::ImplItem::Fn(method) = impl_item else {
                    continue;
                };
                validate_plain_ident(&method.sig.ident, "client value-object method")?;
                let method = catalog_method(owner.clone(), method.clone())?;
                let key = (owner.clone(), method.method.sig.ident.to_string());
                if catalog.methods.insert(key, method).is_some() {
                    return Err(Diagnostic::new(
                        impl_item.span(),
                        "duplicate client value-object method name",
                        "give each inherent method a unique name",
                        "the closed catalog does not perform Rust overload resolution",
                    )
                    .into());
                }
            }
        }
        Ok(catalog)
    }

    fn object(&self, owner: &str) -> Option<&ObjectSpec> {
        self.objects.get(owner)
    }

    fn method(&self, owner: &str, method: &Ident) -> Option<&ObjectMethod> {
        self.methods.get(&(owner.to_owned(), method.to_string()))
    }
}

fn catalog_method(owner: String, method: syn::ImplItemFn) -> Result<ObjectMethod, ExpansionError> {
    if method.sig.constness.is_some()
        || method.sig.asyncness.is_some()
        || method.sig.unsafety.is_some()
        || method.sig.abi.is_some()
        || method.sig.variadic.is_some()
        || !method.sig.generics.params.is_empty()
        || method.sig.generics.where_clause.is_some()
    {
        return Err(Diagnostic::new(
            method.sig.span(),
            "client value-object methods must be ordinary synchronous non-generic Rust functions",
            "remove const, async, unsafe, extern, variadic, and generic qualifiers",
            "each method becomes one deterministic linked Client IR definition",
        )
        .into());
    }
    let receiver = method.sig.inputs.first().and_then(|input| match input {
        syn::FnArg::Receiver(receiver) => Some(receiver),
        syn::FnArg::Typed(_) => None,
    });
    let kind = match receiver {
        Some(receiver) => {
            if receiver.reference.is_none() || receiver.colon_token.is_some() {
                return Err(Diagnostic::new(
                    receiver.span(),
                    "client value-object methods must borrow their receiver",
                    "use `&self` or `&mut self`",
                    "the linked method receives a shared helper-internal object carrier",
                )
                .into());
            }
            MethodKind::Receiver {
                mutable: receiver.mutability.is_some(),
            }
        }
        None => MethodKind::Constructor,
    };
    let return_type = method_return(&method.sig.output, &owner)?;
    match (kind, &return_type) {
        (MethodKind::Constructor, MethodReturn::Object) => {}
        (MethodKind::Constructor, _) => {
            return Err(Diagnostic::new(
                method.sig.output.span(),
                "associated client value-object functions must construct and return `Self`",
                "return `Self`, or make it a borrowed receiver method",
                "receiver-less functions are constructors in the closed object surface",
            )
            .into());
        }
        (MethodKind::Receiver { .. }, MethodReturn::Object) => {
            return Err(Diagnostic::new(
                method.sig.output.span(),
                "client value-object receiver methods cannot return the object yet",
                "return a supported scalar or `()`",
                "object identity remains internal to the linked helper graph",
            )
            .into());
        }
        (MethodKind::Receiver { .. }, _) => {}
    }
    let skip = usize::from(matches!(kind, MethodKind::Receiver { .. }));
    let params = method
        .sig
        .inputs
        .iter()
        .skip(skip)
        .map(method_parameter)
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let hidden = format_ident!(
        "__rsjs_object_{}_method_{}",
        snake_case(&owner),
        method.sig.ident,
        span = method.sig.ident.span()
    );
    Ok(ObjectMethod {
        owner,
        method,
        hidden,
        kind,
        params,
        return_type,
    })
}

fn method_parameter(input: &syn::FnArg) -> Result<(Ident, ScalarType), Diagnostic> {
    let syn::FnArg::Typed(typed) = input else {
        return Err(Diagnostic::new(
            input.span(),
            "only the first client value-object method argument may be a receiver",
            "put `&self` or `&mut self` first",
            "linked receiver helpers have one explicit internal object parameter",
        ));
    };
    let syn::Pat::Ident(pattern) = typed.pat.as_ref() else {
        return Err(Diagnostic::new(
            typed.pat.span(),
            "client value-object method parameters must be plain identifiers",
            "replace destructuring with one named scalar parameter",
            "method arguments map one-to-one to linked helper parameters",
        ));
    };
    if !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
    {
        return Err(Diagnostic::new(
            pattern.span(),
            "client value-object method parameters must be plain value identifiers",
            "remove `ref`, `mut`, attributes, and subpatterns",
            "the generated helper preserves one stable parameter identity",
        ));
    }
    let scalar = scalar_type(&typed.ty).ok_or_else(|| {
        Diagnostic::new(
            typed.ty.span(),
            "unsupported client value-object method parameter type",
            "use bool, i32/u32/i64/u64, f64, String, or &str",
            "the first linked receiver ABI accepts scalar value arguments",
        )
    })?;
    Ok((pattern.ident.clone(), scalar))
}

fn method_return(output: &syn::ReturnType, owner: &str) -> Result<MethodReturn, ExpansionError> {
    match output {
        syn::ReturnType::Default => Ok(MethodReturn::Unit),
        syn::ReturnType::Type(_, ty) if is_unit_type(ty) => Ok(MethodReturn::Unit),
        syn::ReturnType::Type(_, ty) if is_owner_type(ty, owner) => Ok(MethodReturn::Object),
        syn::ReturnType::Type(_, ty) => {
            scalar_type(ty).map(MethodReturn::Scalar).ok_or_else(|| {
                Diagnostic::new(
                    ty.span(),
                    "unsupported client value-object method return type",
                    "return `()`, a supported scalar, or `Self` from a constructor",
                    "linked receiver values must remain inside the closed object graph",
                )
                .into()
            })
        }
    }
}

fn generated_field_helpers(object: &ObjectSpec) -> Vec<syn::ItemFn> {
    let owner = &object.ident;
    let constructor = &object.literal_constructor;
    let field_names = object.fields.iter().map(|field| &field.ident);
    let field_params = object.fields.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.scalar.rust_ty;
        quote!(#ident: #ty)
    });
    let mut helpers = vec![
        syn::parse_quote_spanned!(owner.span()=>
            fn #constructor(#(#field_params),*) -> ::rsjs::__runtime::ClientObjectValue {
                #owner { #(#field_names),* }
            }
        ),
        generated_clone_helper(object),
    ];
    for field in &object.fields {
        let field_name = &field.ident;
        let getter = &field.getter;
        let setter = &field.setter;
        let ty = &field.scalar.rust_ty;
        helpers.push(syn::parse_quote_spanned!(field_name.span()=>
            fn #getter(
                __rsjs_receiver: ::rsjs::__runtime::ClientObjectValue,
            ) -> #ty {
                let __rsjs_self = __rsjs_receiver;
                __rsjs_self.#field_name
            }
        ));
        helpers.push(syn::parse_quote_spanned!(field_name.span()=>
            fn #setter(
                __rsjs_receiver: ::rsjs::__runtime::ClientObjectValue,
                __rsjs_value: #ty,
            ) -> () {
                let mut __rsjs_self = __rsjs_receiver;
                __rsjs_self.#field_name = __rsjs_value;
                return;
            }
        ));
    }
    helpers
}

fn generated_clone_helper(object: &ObjectSpec) -> syn::ItemFn {
    let owner = &object.ident;
    let clone_helper = &object.clone_helper;
    let clone_fields = object.fields.iter().map(|field| {
        let ident = &field.ident;
        quote!(#ident: __rsjs_self.#ident)
    });
    syn::parse_quote_spanned!(owner.span()=>
        fn #clone_helper(
            __rsjs_receiver: ::rsjs::__runtime::ClientObjectValue,
        ) -> ::rsjs::__runtime::ClientObjectValue {
            let __rsjs_self = __rsjs_receiver;
            #owner { #(#clone_fields),* }
        }
    )
}

fn generated_method_helper(
    method: &ObjectMethod,
    catalog: &ObjectCatalog,
) -> Result<syn::ItemFn, ExpansionError> {
    let hidden = &method.hidden;
    let params = method.params.iter().map(|(ident, scalar)| {
        let ty = &scalar.rust_ty;
        quote!(#ident: #ty)
    });
    let mut body = method.method.block.clone();
    match method.kind {
        MethodKind::Constructor => {
            let mut rewriter = ObjectRewriter::internal(catalog, Some(method.owner.clone()));
            body = rewriter.fold_block(body);
            rewriter.finish()?;
            Ok(syn::parse_quote_spanned!(hidden.span()=>
                fn #hidden(#(#params),*) -> ::rsjs::__runtime::ClientObjectValue #body
            ))
        }
        MethodKind::Receiver { mutable } => {
            body = SelfToLocal.fold_block(body);
            let mut rewriter = ObjectRewriter::internal(catalog, Some(method.owner.clone()));
            rewriter
                .locals
                .insert("__rsjs_self".to_owned(), method.owner.clone());
            body = rewriter.fold_block(body);
            rewriter.finish()?;
            if matches!(method.return_type, MethodReturn::Unit) {
                normalize_unit_body(&mut body);
            }
            let binding: syn::Stmt = if mutable {
                syn::parse_quote!(let mut __rsjs_self = __rsjs_receiver;)
            } else {
                syn::parse_quote!(let __rsjs_self = __rsjs_receiver;)
            };
            body.stmts.insert(0, binding);
            let return_ty = match &method.return_type {
                MethodReturn::Unit => syn::parse_quote!(()),
                MethodReturn::Scalar(scalar) => scalar.rust_ty.clone(),
                MethodReturn::Object => unreachable!("receiver object returns are rejected"),
            };
            Ok(syn::parse_quote_spanned!(hidden.span()=>
                fn #hidden(
                    __rsjs_receiver: ::rsjs::__runtime::ClientObjectValue,
                    #(#params),*
                ) -> #return_ty #body
            ))
        }
    }
}

fn normalize_unit_body(body: &mut syn::Block) {
    if let Some(syn::Stmt::Expr(expression, None)) = body.stmts.last().cloned() {
        let statement: syn::Stmt = syn::parse_quote_spanned!(expression.span()=> #expression;);
        *body
            .stmts
            .last_mut()
            .expect("the cloned final expression proves a final statement") = statement;
    }
    body.stmts.push(syn::parse_quote!(return;));
}

fn rewrite_public_root(
    function: &mut syn::ItemFn,
    catalog: &ObjectCatalog,
) -> Result<(), ExpansionError> {
    let mut rewriter = ObjectRewriter::public(catalog);
    *function.block = rewriter.fold_block((*function.block).clone());
    rewriter.finish()
}

fn ensure_unit_tail(function: &mut syn::ItemFn) {
    let returns_unit = match &function.sig.output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => is_unit_type(ty),
    };
    if returns_unit && !matches!(function.block.stmts.last(), Some(syn::Stmt::Expr(_, None))) {
        function.block.stmts.push(syn::parse_quote!(return;));
    }
}

struct GeneratedClassModule {
    javascript: String,
    export_names: Vec<String>,
}

struct LoweredClassFunction {
    hidden_name: String,
    span: Span,
    ir: crate::client_attr::InternalValueObjectClientIr,
}

fn generate_class_module(catalog: &ObjectCatalog) -> Result<GeneratedClassModule, ExpansionError> {
    let exported = catalog
        .objects
        .values()
        .filter(|object| object.exported)
        .collect::<Vec<_>>();
    if exported.is_empty() {
        return Err(Diagnostic::new(
            Span::call_site(),
            "`#[rsjs::client(class)]` needs at least one public value-object struct",
            "make each Rust struct that should become an ESM class `pub`",
            "non-public structs remain implementation details and are not exported to JavaScript",
        )
        .into());
    }

    for object in &exported {
        require_javascript_identifier(&object.ident, "exported client class")?;
        let owner = object.ident.to_string();
        let constructor = catalog
            .methods
            .get(&(owner.clone(), "new".to_owned()))
            .ok_or_else(|| {
                Diagnostic::new(
                    object.ident.span(),
                    "an exported client class needs a `pub fn new(...) -> Self` constructor",
                    "add a public associated `new` function to the inherent impl",
                    "the JavaScript `new Class(...)` call delegates to that checked Rust constructor",
                )
            })?;
        if constructor.kind != MethodKind::Constructor
            || !matches!(constructor.method.vis, syn::Visibility::Public(_))
        {
            return Err(Diagnostic::new(
                constructor.method.sig.ident.span(),
                "an exported client class constructor must be `pub fn new(...) -> Self`",
                "make `new` public and keep it receiver-free",
                "the generated ESM class exposes it as JavaScript's constructor",
            )
            .into());
        }
        for method in catalog
            .methods
            .values()
            .filter(|method| method.owner == owner)
        {
            if matches!(method.method.vis, syn::Visibility::Public(_)) {
                if matches!(method.kind, MethodKind::Receiver { .. })
                    && method.method.sig.ident == "constructor"
                {
                    return Err(Diagnostic::new(
                        method.method.sig.ident.span(),
                        "an exported client class method cannot be named `constructor`",
                        "rename the Rust receiver method",
                        "JavaScript class syntax reserves `constructor` for the one generated wrapper constructor",
                    )
                    .into());
                }
                require_javascript_identifier(&method.method.sig.ident, "exported class method")?;
                if method.kind == MethodKind::Constructor && method.method.sig.ident != "new" {
                    return Err(Diagnostic::new(
                        method.method.sig.ident.span(),
                        "exported client classes support one public associated constructor",
                        "make additional constructors private, or express them as borrowed receiver methods",
                        "JavaScript has one `constructor`; silently omitting a public Rust factory would create an incomplete interop API",
                    )
                    .into());
                }
            }
        }
    }

    let mut lowered = Vec::with_capacity(catalog.methods.len() + catalog.objects.len());
    for object in catalog.objects.values() {
        let helper = generated_clone_helper(object);
        let ir = crate::client_attr::lower_internal_value_object_client_ir(
            &helper,
            &object.internal_spec(),
        )
        .map_err(ExpansionError::Expanded)?;
        lowered.push(LoweredClassFunction {
            hidden_name: object.clone_helper.to_string(),
            span: object.ident.span(),
            ir,
        });
    }
    for method in catalog.methods.values() {
        let object = catalog
            .objects
            .get(&method.owner)
            .expect("catalog methods always have a catalog object owner");
        let helper = generated_method_helper(method, catalog)?;
        let ir = crate::client_attr::lower_internal_value_object_client_ir(
            &helper,
            &object.internal_spec(),
        )
        .map_err(ExpansionError::Expanded)?;
        lowered.push(LoweredClassFunction {
            hidden_name: method.hidden.to_string(),
            span: method.method.sig.ident.span(),
            ir,
        });
    }

    let mut function_ids = BTreeMap::new();
    for (index, function) in lowered.iter().enumerate() {
        if function_ids
            .insert(
                function.hidden_name.clone(),
                rsjs_ir::ClientFunctionId(index as u32),
            )
            .is_some()
        {
            return Err(Diagnostic::new(
                function.span,
                "generated client class symbols collide",
                "rename one of the class structs or methods so their snake-case generated names are distinct",
                "class modules require a one-to-one mapping from Rust methods to private JavaScript functions",
            )
            .into());
        }
    }
    let mut signatures = BTreeMap::new();
    for function in &lowered {
        let params = function
            .ir
            .params
            .iter()
            .map(|(_, rust_type)| {
                rsjs_ir::parse_client_helper_type(
                    rust_type,
                    rsjs_ir::ClientHelperTypePosition::Parameter,
                )
                .map_err(|error| {
                    Diagnostic::new(
                        function.span,
                        "invalid exported class method parameter type",
                        "use the documented scalar/object receiver subset",
                        &error.to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let return_ty = rsjs_ir::parse_client_helper_type(
            &function.ir.return_type,
            rsjs_ir::ClientHelperTypePosition::Return,
        )
        .map_err(|error| {
            Diagnostic::new(
                function.span,
                "invalid exported class method return type",
                "use the documented scalar/object constructor subset",
                &error.to_string(),
            )
        })?;
        signatures.insert(
            function.hidden_name.clone(),
            rsjs_ir::ClientHelperTargetSignature::new(params, return_ty),
        );
    }
    let target_signatures = function_ids
        .iter()
        .map(|(name, id)| {
            (
                rsjs_ir::abi::HelperCallEdgeId::new(id.0),
                signatures
                    .get(name)
                    .expect("every class function id has a parsed signature")
                    .clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut module =
        rsjs_ir::ClientModule::new("rsjs_client_class", rsjs_ir::ClientModuleKind::ClientModule);
    for (index, function) in lowered.iter().enumerate() {
        for call in &function.ir.direct_calls {
            let target = call.rsplit("::").next().unwrap_or(call);
            if !function_ids.contains_key(target) {
                return Err(Diagnostic::new(
                    function.span,
                    "exported client class methods must form a closed call graph",
                    "move the called logic into an inherent method in this class module",
                    &format!(
                        "the standalone class asset cannot relocation-link external helper `{call}`"
                    ),
                )
                .into());
            }
        }
        let resolve = |path: &str| {
            let target = path.rsplit("::").next().unwrap_or(path);
            function_ids
                .get(target)
                .copied()
                .map(rsjs_ir::ClientFunctionRef::Local)
        };
        let resolve_for_validation = |path: &str| {
            let target = path.rsplit("::").next().unwrap_or(path);
            function_ids
                .get(target)
                .copied()
                .map(|id| rsjs_ir::ClientFunctionRef::Helper {
                    edge: rsjs_ir::abi::HelperCallEdgeId::new(id.0),
                    authored_path: path.to_owned(),
                    name: target.to_owned(),
                })
        };
        let validation_options = rsjs_ir::ClientJsonParseOptions::new()
            .with_unresolved_function_ref(&resolve_for_validation);
        let validation_body = rsjs_ir::parse_client_body_json_str_with_options(
            &function.ir.body_ir_json,
            &validation_options,
        )
        .map_err(|error| {
            Diagnostic::new(
                function.span,
                "failed to parse an exported class method's Client IR for type checking",
                "report this as an rsjs compiler bug",
                &error.to_string(),
            )
        })?;
        let signature = signatures
            .get(&function.hidden_name)
            .expect("every lowered class function has a parsed signature");
        rsjs_ir::validate_client_helper_block_types(
            &validation_body,
            &signature.params,
            &signature.return_ty,
            &target_signatures,
        )
        .map_err(|error| {
            Diagnostic::new(
                function.span,
                "exported client class method has inconsistent Client IR types",
                "give literals and mutable locals the same explicit supported Rust types as the class fields and method signature",
                &error.to_string(),
            )
        })?;
        let options = rsjs_ir::ClientJsonParseOptions::new().with_unresolved_function_ref(&resolve);
        let body =
            rsjs_ir::parse_client_body_json_str_with_options(&function.ir.body_ir_json, &options)
                .map_err(|error| {
                Diagnostic::new(
                    function.span,
                    "failed to parse an exported class method's Client IR",
                    "report this as an rsjs compiler bug",
                    &error.to_string(),
                )
            })?;
        let return_ty = signature.return_ty.clone();
        let mut client_function = rsjs_ir::ClientFunction::new(
            rsjs_ir::ClientFunctionId(index as u32),
            &function.hidden_name,
        )
        .with_return_ty(return_ty)
        .with_body(body);
        for (param_index, ty) in signature.params.iter().cloned().enumerate() {
            client_function
                .params
                .push(rsjs_ir::ClientParam::new(format!("__p{param_index}"), ty));
        }
        module.add_function(client_function);
    }

    let mut javascript = String::from(
        "// Auto-generated by rsjs from #[rsjs::client(class)].\n\
         // Rust records and linked functions remain the semantic core.\n",
    );
    javascript.push_str(
        &rsjs_ir::client::emit_client_module_js(&module).map_err(|error| {
            Diagnostic::new(
                Span::call_site(),
                "failed to emit an exported client class module",
                "report this as an rsjs compiler bug",
                &error.to_string(),
            )
        })?,
    );
    for object in &exported {
        append_class_wrapper(&mut javascript, object, catalog, &function_ids);
    }
    Ok(GeneratedClassModule {
        javascript,
        export_names: exported
            .iter()
            .map(|object| object.ident.to_string())
            .collect(),
    })
}

fn append_class_wrapper(
    javascript: &mut String,
    object: &ObjectSpec,
    catalog: &ObjectCatalog,
    function_ids: &BTreeMap<String, rsjs_ir::ClientFunctionId>,
) {
    use std::fmt::Write as _;

    let owner = object.ident.to_string();
    let constructor = catalog
        .methods
        .get(&(owner.clone(), "new".to_owned()))
        .expect("generate_class_module validates an exported new constructor");
    let constructor_id = function_ids
        .get(&constructor.hidden.to_string())
        .expect("every catalog method has a class-module function id");
    let _ = writeln!(javascript, "export class {} {{", object.ident);
    javascript.push_str("  #__rsjs_state;\n");
    javascript.push_str("  constructor(");
    append_js_args(javascript, constructor.params.len());
    javascript.push_str(") {\n    this.#__rsjs_state = __fn_");
    let _ = write!(javascript, "{}(", constructor_id.0);
    append_js_args(javascript, constructor.params.len());
    javascript.push_str(");\n  }\n");
    for method in catalog.methods.values().filter(|method| {
        method.owner == owner
            && matches!(method.kind, MethodKind::Receiver { .. })
            && matches!(method.method.vis, syn::Visibility::Public(_))
    }) {
        let id = function_ids
            .get(&method.hidden.to_string())
            .expect("every catalog method has a class-module function id");
        let _ = write!(javascript, "  {}(", method.method.sig.ident);
        append_js_args(javascript, method.params.len());
        javascript.push_str(") {\n    return __fn_");
        let _ = write!(javascript, "{}(this.#__rsjs_state", id.0);
        if !method.params.is_empty() {
            javascript.push_str(", ");
            append_js_args(javascript, method.params.len());
        }
        javascript.push_str(");\n  }\n");
    }
    javascript.push_str("}\n");
}

fn append_js_args(javascript: &mut String, count: usize) {
    use std::fmt::Write as _;

    for index in 0..count {
        if index > 0 {
            javascript.push_str(", ");
        }
        let _ = write!(javascript, "__a{index}");
    }
}

fn require_javascript_identifier(ident: &Ident, label: &str) -> Result<(), ExpansionError> {
    let name = ident.to_string();
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    if !valid {
        return Err(Diagnostic::new(
            ident.span(),
            &format!("{label} names must be portable JavaScript identifiers"),
            "use ASCII letters, digits, and underscores, starting with a letter or underscore",
            "the spelling becomes a public ESM class or method name",
        )
        .into());
    }
    Ok(())
}

fn class_manifest_items(
    class_module: &GeneratedClassModule,
    span: Span,
) -> Result<Vec<syn::Item>, ExpansionError> {
    let javascript = syn::LitStr::new(&class_module.javascript, span);
    let export_names = class_module
        .export_names
        .iter()
        .map(|name| syn::LitStr::new(name, span))
        .collect::<Vec<_>>();
    let tokens = quote_spanned!(span=>
        #[doc(hidden)]
        #[allow(non_snake_case, non_upper_case_globals)]
        const _: () = {
            static __RSJS_CLIENT_CLASS_ENTRY: ::rsjs::ClientClassManifestEntry =
                ::rsjs::ClientClassManifestEntry {
                    module_path: module_path!(),
                    export_names: &[#(#export_names),*],
                    js_source: #javascript,
                };

            #[::rsjs::linkme::distributed_slice(::rsjs::CLIENT_CLASS_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static __RSJS_CLIENT_CLASS_SUBMIT: &'static ::rsjs::ClientClassManifestEntry =
                &__RSJS_CLIENT_CLASS_ENTRY;
        };
    );
    parse_expanded_items(tokens, span)
}

struct ObjectRewriter<'a> {
    catalog: &'a ObjectCatalog,
    locals: BTreeMap<String, String>,
    self_owner: Option<String>,
    rewrite_fields: bool,
    rewrite_structs: bool,
    error: Option<Diagnostic>,
}

impl<'a> ObjectRewriter<'a> {
    fn public(catalog: &'a ObjectCatalog) -> Self {
        Self {
            catalog,
            locals: BTreeMap::new(),
            self_owner: None,
            rewrite_fields: true,
            rewrite_structs: true,
            error: None,
        }
    }

    fn internal(catalog: &'a ObjectCatalog, self_owner: Option<String>) -> Self {
        Self {
            catalog,
            locals: BTreeMap::new(),
            self_owner,
            rewrite_fields: false,
            rewrite_structs: false,
            error: None,
        }
    }

    fn finish(self) -> Result<(), ExpansionError> {
        self.error.map_or(Ok(()), |error| Err(error.into()))
    }

    fn fail(&mut self, diagnostic: Diagnostic) {
        if self.error.is_none() {
            self.error = Some(diagnostic);
        }
    }

    fn owner_for_path(&self, path: &syn::Path) -> Option<String> {
        if path.segments.len() != 1 {
            return None;
        }
        let name = path.segments.first()?.ident.to_string();
        if name == "Self" {
            return self.self_owner.clone();
        }
        self.catalog.objects.contains_key(&name).then_some(name)
    }

    fn owner_for_receiver(&self, expression: &syn::Expr) -> Option<String> {
        simple_expr_ident(expression).and_then(|ident| self.locals.get(&ident.to_string()).cloned())
    }

    fn binding_owner(&self, statement: &syn::Stmt) -> Option<(String, String)> {
        let syn::Stmt::Local(local) = statement else {
            return None;
        };
        let name = local_pattern_ident(&local.pat)?.to_string();
        if let syn::Pat::Type(typed) = &local.pat {
            if let Some(owner) = simple_type_name(&typed.ty) {
                if self.catalog.objects.contains_key(&owner) {
                    return Some((name, owner));
                }
            }
        }
        self.object_expression_owner(&local.init.as_ref()?.expr)
            .map(|owner| (name, owner))
    }

    fn object_expression_owner(&self, expression: &syn::Expr) -> Option<String> {
        match expression {
            syn::Expr::Paren(paren) => self.object_expression_owner(&paren.expr),
            syn::Expr::Group(group) => self.object_expression_owner(&group.expr),
            syn::Expr::Path(path) if path.qself.is_none() => self
                .locals
                .get(&path.path.get_ident()?.to_string())
                .cloned(),
            syn::Expr::Struct(item) if item.qself.is_none() => self.owner_for_path(&item.path),
            syn::Expr::Call(call) => {
                let syn::Expr::Path(path) = call.func.as_ref() else {
                    return None;
                };
                let (owner, method) =
                    associated_call_parts(&path.path, self.self_owner.as_deref())?;
                self.catalog
                    .method(&owner, method)
                    .filter(|method| method.kind == MethodKind::Constructor)
                    .map(|_| owner)
            }
            _ => None,
        }
    }

    fn object_alias_owner(&self, expression: &syn::Expr) -> Option<String> {
        match expression {
            syn::Expr::Paren(paren) => self.object_alias_owner(&paren.expr),
            syn::Expr::Group(group) => self.object_alias_owner(&group.expr),
            syn::Expr::Path(path) if path.qself.is_none() => {
                let name = path.path.get_ident()?.to_string();
                // An authored `let alias = self;` aliases a borrowed receiver;
                // it is not a Rust value copy. The generated receiver binding
                // is inserted after this rewrite and is likewise intentionally
                // shared across linked method calls.
                (name != "__rsjs_self")
                    .then(|| self.locals.get(&name).cloned())
                    .flatten()
            }
            _ => None,
        }
    }

    fn clone_object_alias(&self, statement: &mut syn::Stmt, owner: &str) {
        let syn::Stmt::Local(local) = statement else {
            return;
        };
        let Some(init) = &mut local.init else {
            return;
        };
        let clone_helper = &self
            .catalog
            .object(owner)
            .expect("object alias owners come from the catalog")
            .clone_helper;
        let expression = (*init.expr).clone();
        *init.expr = syn::parse_quote_spanned!(expression.span()=> #clone_helper(#expression));
    }

    fn expression_is_object(&self, expression: &syn::Expr) -> bool {
        self.object_expression_owner(expression).is_some()
            || self.owner_for_receiver(expression).is_some()
    }

    fn reject_object_call_arguments(
        &mut self,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    ) -> bool {
        let Some(argument) = args
            .iter()
            .find(|argument| self.expression_is_object(argument))
        else {
            return false;
        };
        self.fail(Diagnostic::new(
            argument.span(),
            "a client value object cannot escape to an ordinary function call",
            "call a constructor or borrowed method declared in this closed object module",
            "the nominal object carrier is legal only on compiler-generated transitive helper edges",
        ));
        true
    }

    fn rewrite_method_call(&mut self, call: syn::ExprMethodCall) -> syn::Expr {
        let Some(owner) = self.owner_for_receiver(&call.receiver) else {
            self.reject_object_call_arguments(&call.args);
            return syn::Expr::MethodCall(fold::fold_expr_method_call(self, call));
        };
        let Some(method) = self.catalog.method(&owner, &call.method) else {
            self.fail(Diagnostic::new(
                call.method.span(),
                "unknown method on a client value object",
                "define the method in the object's inherent impl",
                "browser-reachable receiver calls resolve in the closed module catalog",
            ));
            return syn::Expr::MethodCall(call);
        };
        if !matches!(method.kind, MethodKind::Receiver { .. }) {
            self.fail(Diagnostic::new(
                call.method.span(),
                "client value-object constructors must use associated-call syntax",
                "call `Type::constructor(...)`",
                "receiver syntax maps only to borrowed linked methods",
            ));
            return syn::Expr::MethodCall(call);
        }
        if call.turbofish.is_some() {
            self.fail(Diagnostic::new(
                call.method.span(),
                "client value-object method calls cannot use generic arguments",
                "remove the turbofish",
                "the closed catalog contains one concrete method definition",
            ));
            return syn::Expr::MethodCall(call);
        }
        let hidden = &method.hidden;
        let receiver = self.fold_expr(*call.receiver);
        let args = call
            .args
            .into_iter()
            .map(|argument| self.fold_expr(argument))
            .collect::<Vec<_>>();
        syn::parse_quote_spanned!(call.method.span()=> #hidden(#receiver, #(#args),*))
    }

    fn rewrite_associated_call(&mut self, call: syn::ExprCall) -> syn::Expr {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            self.reject_object_call_arguments(&call.args);
            return syn::Expr::Call(fold::fold_expr_call(self, call));
        };
        let Some((owner, method_name)) =
            associated_call_parts(&path.path, self.self_owner.as_deref())
        else {
            self.reject_object_call_arguments(&call.args);
            return syn::Expr::Call(fold::fold_expr_call(self, call));
        };
        let Some(method) = self.catalog.method(&owner, method_name) else {
            self.reject_object_call_arguments(&call.args);
            return syn::Expr::Call(fold::fold_expr_call(self, call));
        };
        if method.kind != MethodKind::Constructor {
            self.fail(Diagnostic::new(
                path.span(),
                "borrowed client value-object methods must use receiver syntax",
                "call `value.method(...)`",
                "the receiver is an explicit first linked-helper argument",
            ));
            return syn::Expr::Call(call);
        }
        let hidden = &method.hidden;
        let args = call
            .args
            .into_iter()
            .map(|argument| self.fold_expr(argument))
            .collect::<Vec<_>>();
        syn::parse_quote_spanned!(path.span()=> #hidden(#(#args),*))
    }

    fn rewrite_struct(&mut self, item: syn::ExprStruct) -> syn::Expr {
        if !self.rewrite_structs {
            return syn::Expr::Struct(fold::fold_expr_struct(self, item));
        }
        let Some(owner) = self.owner_for_path(&item.path) else {
            return syn::Expr::Struct(fold::fold_expr_struct(self, item));
        };
        let object = self
            .catalog
            .object(&owner)
            .expect("owner_for_path checks the object catalog");
        if item.rest.is_some() || item.fields.len() != object.fields.len() {
            self.fail(Diagnostic::new(
                item.span(),
                "client value-object literals must initialize every field explicitly",
                "spell out the complete named-field literal without `..rest`",
                "the generated constructor has one argument per closed schema field",
            ));
            return syn::Expr::Struct(item);
        }
        for (authored, expected) in item.fields.iter().zip(&object.fields) {
            if authored.member != syn::Member::Named(expected.ident.clone()) {
                self.fail(Diagnostic::new(
                    authored.member.span(),
                    "client value-object literal fields must appear in declaration order",
                    "reorder the fields to match the struct declaration",
                    "helper arguments preserve Rust field evaluation order",
                ));
                return syn::Expr::Struct(item);
            }
        }
        let constructor = &object.literal_constructor;
        let span = item.span();
        let args = item
            .fields
            .into_iter()
            .map(|field| self.fold_expr(field.expr))
            .collect::<Vec<_>>();
        syn::parse_quote_spanned!(span=> #constructor(#(#args),*))
    }

    fn rewrite_field(&mut self, field: syn::ExprField) -> syn::Expr {
        if !self.rewrite_fields {
            return syn::Expr::Field(fold::fold_expr_field(self, field));
        }
        let Some(owner) = self.owner_for_receiver(&field.base) else {
            return syn::Expr::Field(fold::fold_expr_field(self, field));
        };
        let syn::Member::Named(name) = &field.member else {
            self.fail(Diagnostic::new(
                field.member.span(),
                "tuple fields are not supported on client value objects",
                "use named fields",
                "Client IR projections address own-property names",
            ));
            return syn::Expr::Field(field);
        };
        let Some(object_field) = self
            .catalog
            .object(&owner)
            .and_then(|object| object.field(name))
        else {
            self.fail(Diagnostic::new(
                name.span(),
                "unknown field on a client value object",
                "read a field declared by the object struct",
                "field projections resolve through the closed schema",
            ));
            return syn::Expr::Field(field);
        };
        let getter = &object_field.getter;
        let base = self.fold_expr(*field.base);
        syn::parse_quote_spanned!(name.span()=> #getter(#base))
    }

    fn rewrite_assignment(&mut self, assign: syn::ExprAssign) -> syn::Expr {
        if !self.rewrite_fields {
            return syn::Expr::Assign(fold::fold_expr_assign(self, assign));
        }
        let syn::Expr::Field(field) = assign.left.as_ref() else {
            return syn::Expr::Assign(fold::fold_expr_assign(self, assign));
        };
        let Some(owner) = self.owner_for_receiver(&field.base) else {
            return syn::Expr::Assign(fold::fold_expr_assign(self, assign));
        };
        let syn::Member::Named(name) = &field.member else {
            return syn::Expr::Assign(fold::fold_expr_assign(self, assign));
        };
        let Some(object_field) = self
            .catalog
            .object(&owner)
            .and_then(|object| object.field(name))
        else {
            self.fail(Diagnostic::new(
                name.span(),
                "unknown field on a client value object",
                "assign a field declared by the object struct",
                "field places resolve through the closed schema",
            ));
            return syn::Expr::Assign(assign);
        };
        let setter = &object_field.setter;
        let span = assign.eq_token.span();
        let base = self.fold_expr((*field.base).clone());
        let value = self.fold_expr(*assign.right);
        syn::parse_quote_spanned!(span=> #setter(#base, #value))
    }

    fn rewrite_binary(&mut self, binary: syn::ExprBinary) -> syn::Expr {
        if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_))
            && (self.expression_is_object(&binary.left) || self.expression_is_object(&binary.right))
        {
            self.fail(Diagnostic::new(
                binary.op.span(),
                "direct client value-object equality is not supported",
                "compare the scalar fields that define the value's meaning",
                "JavaScript object equality is identity-based and would not preserve Rust structural equality",
            ));
            return syn::Expr::Binary(binary);
        }
        syn::Expr::Binary(fold::fold_expr_binary(self, binary))
    }
}

impl Fold for ObjectRewriter<'_> {
    fn fold_block(&mut self, mut block: syn::Block) -> syn::Block {
        let outer = self.locals.clone();
        let mut statements = Vec::with_capacity(block.stmts.len());
        for statement in block.stmts {
            let binding = self.binding_owner(&statement);
            let alias_owner = match &statement {
                syn::Stmt::Local(local) => local
                    .init
                    .as_ref()
                    .and_then(|init| self.object_alias_owner(&init.expr)),
                _ => None,
            };
            let mut statement = self.fold_stmt(statement);
            if let Some(owner) = alias_owner {
                self.clone_object_alias(&mut statement, &owner);
            }
            if let Some((name, owner)) = binding {
                self.locals.insert(name, owner);
            }
            statements.push(statement);
        }
        self.locals = outer;
        block.stmts = statements;
        block
    }

    fn fold_expr(&mut self, expression: syn::Expr) -> syn::Expr {
        match expression {
            syn::Expr::MethodCall(call) => self.rewrite_method_call(call),
            syn::Expr::Call(call) => self.rewrite_associated_call(call),
            syn::Expr::Struct(item) => self.rewrite_struct(item),
            syn::Expr::Field(field) => self.rewrite_field(field),
            syn::Expr::Assign(assign) => self.rewrite_assignment(assign),
            syn::Expr::Binary(binary) => self.rewrite_binary(binary),
            other => fold::fold_expr(self, other),
        }
    }
}

struct SelfToLocal;

impl Fold for SelfToLocal {
    fn fold_expr_path(&mut self, path: syn::ExprPath) -> syn::ExprPath {
        if path.qself.is_none() && path.path.is_ident("self") {
            syn::parse_quote_spanned!(path.span()=> __rsjs_self)
        } else {
            fold::fold_expr_path(self, path)
        }
    }
}

fn associated_call_parts<'a>(
    path: &'a syn::Path,
    self_owner: Option<&str>,
) -> Option<(String, &'a Ident)> {
    if path.leading_colon.is_some() || path.segments.len() != 2 {
        return None;
    }
    let owner = path.segments.first()?.ident.to_string();
    let owner = if owner == "Self" {
        self_owner?.to_owned()
    } else {
        owner
    };
    Some((owner, &path.segments.last()?.ident))
}

fn local_pattern_ident(pattern: &syn::Pat) -> Option<&Ident> {
    match pattern {
        syn::Pat::Ident(pattern) if pattern.by_ref.is_none() && pattern.subpat.is_none() => {
            Some(&pattern.ident)
        }
        syn::Pat::Type(typed) => local_pattern_ident(&typed.pat),
        _ => None,
    }
}

fn simple_expr_ident(expression: &syn::Expr) -> Option<&Ident> {
    match expression {
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path.path.leading_colon.is_none()
                && path.path.segments.len() == 1 =>
        {
            Some(&path.path.segments.first()?.ident)
        }
        syn::Expr::Paren(paren) => simple_expr_ident(&paren.expr),
        syn::Expr::Group(group) => simple_expr_ident(&group.expr),
        _ => None,
    }
}

fn simple_type_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments.first()?.ident.to_string())
}

fn is_owner_type(ty: &syn::Type, owner: &str) -> bool {
    simple_type_name(ty).is_some_and(|name| name == "Self" || name == owner)
}

fn is_unit_type(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn scalar_type(ty: &syn::Type) -> Option<ScalarType> {
    match ty {
        syn::Type::Path(path)
            if path.qself.is_none()
                && crate::client_attr::is_std_rooted_path(&path.path)
                && path.path.segments.last().is_some_and(|segment| {
                    matches!(segment.arguments, syn::PathArguments::None)
                }) =>
        {
            let name = path.path.segments.last()?.ident.to_string();
            let canonical = match name.as_str() {
                "bool" | "i32" | "u32" | "i64" | "u64" | "f64" | "String" => name,
                _ => return None,
            };
            Some(ScalarType {
                rust_ty: syn::parse_str(&canonical).ok()?,
                rust_name: canonical,
            })
        }
        syn::Type::Reference(reference) if reference.mutability.is_none() => {
            let syn::Type::Path(path) = reference.elem.as_ref() else {
                return None;
            };
            if path.qself.is_none()
                && crate::client_attr::is_std_rooted_path(&path.path)
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "str")
            {
                Some(ScalarType {
                    rust_name: "String".to_owned(),
                    rust_ty: syn::parse_quote!(String),
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn value_object_type_key(
    module: &syn::ItemMod,
    item_struct: &syn::ItemStruct,
    ordinal: usize,
) -> String {
    let start = module.ident.span().start();
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.client-value-object.type.v1\0");
    hasher.update(std::env::var("CARGO_PKG_NAME").unwrap_or_default());
    hasher.update(b"\0");
    hasher.update(std::env::var("CARGO_CRATE_NAME").unwrap_or_default());
    hasher.update(b"\0");
    // Nominal identities enter linked Client IR and generated diagnostics, so
    // moving an otherwise identical checkout must not perturb them. Reuse the
    // checkout-relative source identity already used by shared-state records.
    hasher.update(crate::shared_state_derive::stable_source_file(
        &module.ident.span(),
    ));
    hasher.update(b"\0");
    hasher.update(start.line.to_le_bytes());
    hasher.update(start.column.to_le_bytes());
    hasher.update(ordinal.to_le_bytes());
    hasher.update(module.ident.to_string());
    hasher.update(b"\0");
    hasher.update(item_struct.to_token_stream().to_string());
    let digest: [u8; 32] = hasher.finalize().into();
    let mut result = String::with_capacity(64);
    use std::fmt::Write as _;
    for byte in digest {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn snake_case_ident(ident: &Ident) -> String {
    snake_case(&ident.to_string())
}

fn snake_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    let mut previous_was_lower_or_digit = false;
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            if previous_was_lower_or_digit {
                result.push('_');
            }
            result.push(character.to_ascii_lowercase());
            previous_was_lower_or_digit = false;
        } else {
            previous_was_lower_or_digit =
                character.is_ascii_lowercase() || character.is_ascii_digit();
            result.push(character);
        }
    }
    result
}

fn validate_plain_ident(ident: &Ident, label: &str) -> Result<(), ExpansionError> {
    if ident.to_string().starts_with("r#") {
        return Err(Diagnostic::new(
            ident.span(),
            &format!("{label} names cannot be raw identifiers"),
            "rename it to a non-keyword identifier",
            "linked helper symbols derive from the Rust identifier",
        )
        .into());
    }
    Ok(())
}

#[derive(Debug)]
struct Diagnostic {
    span: Span,
    what: String,
    fix: String,
    why: String,
}

impl Diagnostic {
    fn new(span: Span, what: &str, fix: &str, why: &str) -> Self {
        Self {
            span,
            what: what.to_owned(),
            fix: fix.to_owned(),
            why: why.to_owned(),
        }
    }

    fn to_tokens(&self) -> TokenStream {
        let message = format!(
            "rsjs: {}\n  help: {}\n  note: {}",
            self.what, self.fix, self.why
        );
        let literal = syn::LitStr::new(&message, self.span);
        quote_spanned!(self.span => ::core::compile_error!(#literal);)
    }
}

#[derive(Debug)]
enum ExpansionError {
    Diagnostic(Diagnostic),
    Expanded(TokenStream),
}

impl ExpansionError {
    fn to_tokens(&self) -> TokenStream {
        match self {
            Self::Diagnostic(diagnostic) => diagnostic.to_tokens(),
            Self::Expanded(tokens) => tokens.clone(),
        }
    }
}

impl From<Diagnostic> for ExpansionError {
    fn from(value: Diagnostic) -> Self {
        Self::Diagnostic(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn linked_module() -> syn::ItemMod {
        syn::parse2(quote! {
            pub mod menu {
                pub struct MenuController { open: bool }
                pub struct CounterController { count: i32 }

                impl MenuController {
                    pub fn new(open: bool) -> Self { Self { open } }
                    pub fn toggle(&mut self) { self.open = !self.open; }
                    pub fn is_open(&self) -> bool { self.open }
                }

                impl CounterController {
                    pub fn new() -> Self { Self { count: 0_i32 } }
                    pub fn adjust(&mut self, enabled: bool, amount: i32) {
                        let mut step: i32 = amount;
                        step += 1_i32;
                        if enabled { self.count += step; } else { self.count -= step; }
                    }
                    pub fn projected(&self, offset: i32, double: bool) -> i32 {
                        let mut value: i32 = self.count + offset;
                        if double { value *= 2_i32; }
                        value
                    }
                }

                pub fn toggled() -> bool {
                    let mut menu = MenuController::new(false);
                    menu.toggle();
                    menu.is_open()
                }

                pub fn projected(double: bool) -> i32 {
                    let counter = CounterController { count: 3_i32 };
                    counter.projected(1_i32, double) + 1_i32
                }
            }
        })
        .unwrap()
    }

    #[test]
    fn emits_linked_constructor_and_receiver_definitions() {
        let expanded = try_expand(linked_module(), false)
            .unwrap()
            .into_token_stream()
            .to_string();
        assert!(
            expanded.contains("__rsjs_object_menu_controller_method_new"),
            "{expanded}"
        );
        assert!(
            expanded.contains("__rsjs_object_menu_controller_method_toggle"),
            "{expanded}"
        );
        assert!(
            expanded.contains("__rsjs_object_menu_controller_method_is_open"),
            "{expanded}"
        );
        assert!(expanded.contains("@rsjs_object("), "{expanded}");
        assert!(
            expanded.contains("__rsjs_client_object_source_check_toggled"),
            "{expanded}"
        );
        assert!(
            !expanded.contains("__rsjs_method_menu_toggle"),
            "{expanded}"
        );
    }

    #[test]
    fn nested_argument_bearing_reads_become_linked_calls() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                pub struct Counter { count: i32 }
                impl Counter {
                    pub fn offset(&self, amount: i32) -> i32 { self.count + amount }
                }
                pub fn value() -> i32 {
                    let counter = Counter { count: 1_i32 };
                    counter.offset(2_i32) + counter.offset(3_i32)
                }
            }
        })
        .unwrap();
        let expanded = try_expand(module, false)
            .unwrap()
            .into_token_stream()
            .to_string();
        assert!(
            expanded
                .matches("__rsjs_object_counter_method_offset")
                .count()
                >= 3,
            "{expanded}"
        );
    }

    #[test]
    fn rejects_object_escape_to_an_ordinary_function() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                pub struct Counter { count: i32 }
                fn consume(_counter: Counter) -> i32 { 0_i32 }
                pub fn value() -> i32 {
                    let counter = Counter { count: 1_i32 };
                    consume(counter)
                }
            }
        })
        .unwrap();
        let error = try_expand(module, false).unwrap_err();
        assert!(
            matches!(
                error,
                ExpansionError::Diagnostic(Diagnostic { ref what, .. })
                    if what.contains("cannot escape")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_object_identity_equality_before_javascript_emission() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                #[derive(PartialEq)]
                pub struct Counter { count: i32 }
                pub fn equal() -> bool {
                    let left = Counter { count: 1_i32 };
                    let right = Counter { count: 1_i32 };
                    left == right
                }
            }
        })
        .unwrap();
        let error = try_expand(module, false).unwrap_err();
        assert!(
            matches!(
                error,
                ExpansionError::Diagnostic(Diagnostic { ref what, .. })
                    if what.contains("equality")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn exported_class_wraps_the_same_lowered_constructor_and_methods() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                pub struct Counter { count: i32 }
                impl Counter {
                    pub fn new(seed: i32) -> Self { Self { count: seed } }
                    pub fn increment(&mut self, amount: i32) {
                        let next: i32 = self.count + amount;
                        self.count = next;
                    }
                    pub fn value(&self) -> i32 { self.count }
                }
            }
        })
        .unwrap();
        let expanded = try_expand(module, true)
            .unwrap()
            .into_token_stream()
            .to_string();
        assert!(expanded.contains("ClientClassManifestEntry"), "{expanded}");
        assert!(expanded.contains("export class Counter"), "{expanded}");
        assert!(expanded.contains("#__rsjs_state"), "{expanded}");
        assert!(expanded.contains("checked_int_add"), "{expanded}");
        assert!(expanded.contains("increment(__a0)"), "{expanded}");
        assert!(expanded.contains("value()"), "{expanded}");
        assert!(
            !expanded.contains("CLIENT_HELPER_DEFINITION"),
            "a class-only module should not submit unused linked-helper records: {expanded}"
        );
    }

    #[test]
    fn exported_class_requires_a_public_new_constructor() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                pub struct Counter { count: i32 }
                impl Counter {
                    fn new(seed: i32) -> Self { Self { count: seed } }
                    pub fn value(&self) -> i32 { self.count }
                }
            }
        })
        .unwrap();
        let error = try_expand(module, true).unwrap_err();
        assert!(
            matches!(
                error,
                ExpansionError::Diagnostic(Diagnostic { ref what, .. })
                    if what.contains("constructor must be `pub fn new")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn exported_class_rejects_drop_instead_of_losing_cleanup() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod counter {
                pub struct Counter { count: i32 }
                impl Counter {
                    pub fn new(seed: i32) -> Self { Self { count: seed } }
                }
                impl Drop for Counter {
                    fn drop(&mut self) { self.count = 0_i32; }
                }
            }
        })
        .unwrap();
        let error = try_expand(module, true).unwrap_err();
        assert!(
            matches!(
                error,
                ExpansionError::Diagnostic(Diagnostic { ref what, .. })
                    if what.contains("cannot have trait impls")
            ),
            "{error:?}"
        );
    }
}
