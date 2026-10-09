// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Module-level docs intentionally live on `mod emit_rust` in
// `lib.rs`. This file is also `include!`d by the build script of the
// `rsjs_demo_m3` smoke-test crate, and `include!` does not accept
// inner doc comments (`//!`). Keeping the docs at the module
// declaration site keeps a single source of truth without breaking
// the include.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use rsjs_ir::markers::{MARKER_COMPONENT, MARKER_COMPONENT_MOUNT};
use rsjs_ir::{
    AsType, AttrBindingKind, ClientBinaryOp, ClientBrowserRead, ClientExpr, ClientFunctionRef,
    ClientUnaryOp, EncodingOp, EventName, ExprId, InputKind, Island, ListSource, MutationAccessor,
    MutationId, NodeId, QueryAccessor, QueryId, SharedStoreId, SignalId, StringPredicateOp,
    StringTransformOp, Value, ViewNode, ISLAND_MARKER_ATTR, ISLAND_ROOT_ATTR, ISLAND_WRAPPER_TAG,
    MARKER_ATTR, MARKER_BIND, MARKER_HANDLER, MARKER_INPUT_CHECKED, MARKER_INPUT_SELECTED,
    MARKER_INPUT_VALUE, MARKER_LIST, MARKER_LIST_KEY, MARKER_REF,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct SignalInitializer {
    pub id: SignalId,
    pub expr: syn::Expr,
    pub explicit_ty: Option<syn::Type>,
}

#[derive(Clone)]
pub(crate) struct SharedStateInitializer {
    pub id: SharedStoreId,
    pub source_name: syn::Ident,
    pub ty: syn::Type,
    /// A const expression evaluating to the `SharedState::TYPE_KEY` this
    /// acquisition was originally compiled against, when the acquisition was
    /// replayed from an inlined component. `None` for an acquisition written
    /// here, where the type is by definition the one the author named.
    ///
    /// See [`emit_shared_type_key_assertion`].
    pub expected_type_key: Option<TokenStream>,
}

#[derive(Clone)]
pub(crate) struct QueryInitializer {
    pub id: QueryId,
    pub expr: syn::Expr,
    pub key_expr: Option<syn::Expr>,
}

#[derive(Clone)]
pub(crate) struct MutationInitializer {
    pub id: MutationId,
    pub key_expr: syn::Expr,
    pub invalidates_expr: Option<syn::Expr>,
}

enum ServerExpr {
    Rust(TokenStream),
    Json(TokenStream),
}

/// Emit a `WebRenderSync<L, B>` impl for the given island and struct.
///
/// `island` is the IR (M1 hand-built fixture, M7+ parser-driven).
/// `struct_path` is the path of the user's struct, e.g. `Counter`
/// or `crate::components::Counter`. The emitted impl uses this path
/// verbatim as the `Self` type.
///
/// The asset id used in the hydration markers is derived from the
/// last segment of `struct_path`, lowercased and snake_cased on the
/// fly: a struct named `Counter` becomes the asset id `"counter"`.
/// This matches the convention M7 will use for manifest registration
/// (the asset id is a stable, predictable function of the struct
/// name; uniqueness is the manifest's responsibility).
///
/// # Example
///
/// ```ignore
/// let island = build_counter();           // M1 hand-built fixture
/// let struct_path = syn::parse_quote!(Counter);
/// let tokens = rsjs_macros::emit_rust(&island, &struct_path);
/// // tokens is now a TokenStream containing the impl block.
/// ```
#[cfg(test)]
pub fn emit_rust(island: &Island, struct_path: &syn::Path) -> TokenStream {
    let asset_id = crate::asset::derive_asset_id(struct_path);
    emit_rust_with_prelude_and_asset_id(island, struct_path, &asset_id, &[])
}

/// Variant of [`emit_rust`] that replays additional `let` bindings
/// verbatim into the generated render body, before the rsjs-managed
/// signal locals.
///
/// Used by the proc-macro path so that non-`signal(...)` prelude
/// bindings (e.g. `let user = cx.load::<User>(id).await?;`) are not
/// silently dropped: they are emitted exactly as the user wrote them
/// so the analyser-recognised `signal(...)` initializers AND the
/// async data-loading prelude both run during render.
///
/// `extra_prelude_lets` is rendered verbatim with `quote!`; callers
/// are responsible for filtering out any locals that the analyser
/// already lowered into the IR.
#[cfg(test)]
pub fn emit_rust_with_prelude(
    island: &Island,
    struct_path: &syn::Path,
    extra_prelude_lets: &[syn::Local],
) -> TokenStream {
    // Backwards-compatible entry point: derives the asset id from the
    // struct path alone. Prefer `emit_rust_with_prelude_and_asset_id`
    // when the caller has already computed the with-context asset id
    // (the one registered into `ISLAND_REGISTRY`); the two must match
    // or the per-island JS URL the page emits will not resolve.
    let asset_id = crate::asset::derive_asset_id(struct_path);
    emit_rust_with_prelude_and_asset_id(island, struct_path, &asset_id, extra_prelude_lets)
}

/// Same as [`emit_rust_with_prelude`] but accepts a caller-computed
/// `asset_id`. The macro pipeline's main entry (`expand_rsjs`) computes
/// the asset id once with [`crate::asset::derive_asset_id_with_context`]
/// (path + impl-token hash, for cross-module disambiguation) and threads
/// it through here so the rendered HTML's
/// `<rsjs-island data-rsjs-island-root="...">`
/// matches the registry entry exactly.
#[cfg(test)]
pub fn emit_rust_with_prelude_and_asset_id(
    island: &Island,
    struct_path: &syn::Path,
    asset_id: &str,
    extra_prelude_lets: &[syn::Local],
) -> TokenStream {
    let strategy_analysis = rsjs_ir::classify_render_strategy(island);
    let strategy = strategy_analysis.strategy;
    let reachable_signal_ids = strategy_analysis.reachable_signal_ids();
    let reachable_query_ids = strategy_analysis.reachable_query_ids();
    let reachable_mutation_ids = strategy_analysis.reachable_mutation_ids();
    let reachable_payload_attrs = emitted_payload_attrs(island, &strategy_analysis);
    let asset_id_lit = asset_id;
    // `struct_path` is intentionally unused here — the asset id is now
    // derived in the caller. Keep the parameter so the signature stays
    // close to `emit_rust_with_prelude`'s; future call sites that need
    // the path again won't have to change call signatures.
    let _ = struct_path;

    // Order the binding tables deterministically so the indices we
    // emit into the hydration markers (`data-rsjs-bind="N"`,
    // `data-rsjs-handler="N"`) are stable across compilations and
    // line up with the side-tables.
    let text_binding_order: Vec<NodeId> = sorted_text_bindings(island);
    let event_binding_order: Vec<(NodeId, EventName)> = sorted_event_bindings(island);
    let signal_initializers: &[SignalInitializer] = &[];

    // Round-3 review #1: replay non-`signal(...)` prelude `let`
    // bindings verbatim into the generated render body so async data
    // loads and other user-authored locals actually run. The analyser
    // skips them silently (it only lowers `signal(...)`) so without
    // this they would be dropped.
    let prelude_lets_tokens: TokenStream = extra_prelude_lets
        .iter()
        .map(|local| quote! { #local })
        .collect();
    let rust_signal_ids = rust_signal_initializer_ids(island, &reachable_signal_ids);
    let signal_lets = emit_signal_initializers(island, signal_initializers, &rust_signal_ids);
    // Shared stores are recovered from the IR rather than handed in: this entry
    // point has no analyzer output to thread through. Without them a shared
    // island would reference an undeclared handle local and the seed writer
    // would have no Rust type to read `TYPE_KEY` from.
    let shared_state_initializers = match shared_state_initializers_from_island(island) {
        Ok(initializers) => initializers,
        Err(error) => return error,
    };
    if island.is_async
        && strategy != rsjs_ir::RenderStrategy::ServerOnly
        && !shared_state_initializers.is_empty()
    {
        // The hydrated async shape declares its locals outside the returned
        // future, where the `?` a provider lookup needs has no `Result` to
        // return into. Say so instead of emitting code that fails to compile
        // for a reason the author cannot act on.
        return compile_error_tokens(
            "rsjs/shared-state-async-standalone-emit: this emitter cannot build an async island that acquires shared state, because the provider lookup has to run where the generated async render body cannot propagate its error. Render the shared state from a sync `WebRenderSync` island (it may still contain async children), or author a canonical `WebRender` impl with `#[rsjs(component, client)]`, which emits the async form with the lookup inside the future",
        );
    }
    let shared_state_lets = emit_shared_state_initializers(
        island,
        &shared_state_initializers,
        &reachable_signal_ids,
        asset_id,
        &format_ident!("cx"),
        None,
    );
    let init_state_json = emit_init_state_json(
        island,
        signal_initializers,
        &shared_state_initializers,
        &reachable_signal_ids,
    );
    let init_state_json_let = init_state_json.let_tokens;
    let init_state_json_raw = init_state_json.raw_expr;
    let query_initializers: &[QueryInitializer] = &[];
    let mutation_initializers: &[MutationInitializer] = &[];
    let init_query_state_json_let = emit_init_query_state_json(
        island,
        query_initializers,
        &reachable_query_ids,
        &format_ident!("cx"),
    );
    let init_mutations_json_let =
        emit_init_mutations_json(island, mutation_initializers, &reachable_mutation_ids);
    let query_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-queries={::proa_core::perf_ok(#asset_id_lit)}
    };
    let query_blob = if reachable_query_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #query_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_query_state_json.as_ref())}
            </script>
        }
    };
    let mutations_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-mutations={::proa_core::perf_ok(#asset_id_lit)}
    };
    let mutations_blob = if reachable_mutation_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #mutations_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_mutations_json.as_ref())}
            </script>
        }
    };

    // Pre-compute the marker attribute-name + wrapper-tag token sequences
    // from the shared `rsjs_ir` constants so the `quote!` body below does
    // not hard-code `data-rsjs-island` or `rsjs-island` as inline tokens.
    let island_attr_name = attr_name_tokens(ISLAND_MARKER_ATTR);
    let island_root_attr_name = attr_name_tokens(ISLAND_ROOT_ATTR);
    let wrapper_open = tag_name_tokens(ISLAND_WRAPPER_TAG);
    let wrapper_close = tag_name_tokens(ISLAND_WRAPPER_TAG);

    // Pre-compute, per branch element, the marker attribute and the
    // initial-render style attribute (`""` or `"display:none"`) by
    // const-folding each conditional section's predicate against the
    // signals' literal initial values. M4 only needs to handle the
    // predicate shape `SignalRead op Literal` (with optional Literal
    // op SignalRead), which is what the ConditionalCounter fixture
    // produces. Anything we can't fold defaults to "show", letting JS
    // hydrate to the truthful state on first dep change.
    let fragment_marker_aliases = component_fragment_marker_aliases(island);
    let cond_branches = compute_conditional_branch_metadata(
        island,
        signal_initializers,
        query_initializers,
        &fragment_marker_aliases,
    );

    let view_tokens = match island.view_tree.root() {
        Some(root) => emit_view_node(
            root,
            island,
            &text_binding_order,
            &event_binding_order,
            &cond_branches,
            &fragment_marker_aliases,
            &reachable_payload_attrs,
            signal_initializers,
            query_initializers,
            None,
            None,
            None,
        ),
        // No root: emit nothing structural. We still emit the
        // dehydrated state blob below for forward compatibility.
        None => quote! {},
    };

    if strategy == rsjs_ir::RenderStrategy::ServerOnly {
        if island.is_async {
            return quote! {
                #[automatically_derived]
                impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf>
                    ::proa_core::WebRender<L, B> for #struct_path
                {
                    fn render(
                        self,
                        cx: &mut proa_core::WebContext<L, B>,
                    ) -> ::proa_core::RenderOutcome<
                        impl ::core::future::Future<
                            Output = ::core::result::Result<(), ::proa_core::WriteError>,
                        >,
                    >
                    {
                        ::proa_core::RenderOutcome::pending(async move {
                            use ::proa_core::UnsafeRaw;

                            #shared_state_lets
                            #prelude_lets_tokens
                            #signal_lets

                            ::proa_core::WebRender::render(
                                ::proa_macros::html! {
                                    #view_tokens
                                },
                                cx,
                            )
                            .resolve()
                            .await
                        })
                    }
                }
            };
        }

        return quote! {
            #[automatically_derived]
            impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf>
                ::proa_core::WebRenderSync<L, B> for #struct_path
            {
                fn render(
                    self,
                    cx: &mut ::proa_core::WebContext<L, B>,
                ) -> ::core::result::Result<(), ::proa_core::WriteError> {
                    use ::proa_core::UnsafeRaw;

                    #shared_state_lets
                    #prelude_lets_tokens
                    #signal_lets

                    ::proa_core::WebRenderSync::render(
                        ::proa_macros::html_sync! {
                            #view_tokens
                        },
                        cx,
                    )
                }
            }
        };
    }

    if island.is_async {
        // Async path: emit `WebRender<L, B>` using `proa_macros::html!`
        // (the async-capable macro) and end the body with
        // `.render(cx).resolve().await`. The fixture for this branch
        // is M5's AvailabilityProbe; M7's parser will set
        // `Island::is_async` automatically when the lifted body
        // contains an `.await`.
        quote! {
            #[automatically_derived]
            impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf>
                ::proa_core::WebRender<L, B> for #struct_path
            {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<L, B>,
                ) -> ::proa_core::RenderOutcome<
                    impl ::core::future::Future<
                        Output = ::core::result::Result<(), ::proa_core::WriteError>,
                    >,
                >
                {
                    use ::proa_core::UnsafeRaw;

                    // Route asset graph (rsjs/OPTIMIZATIONS.md § 1):
                    // record this island's asset_id on the WebContext.
                    cx.rsjs_island_used(#asset_id_lit);
                    let __rsjs_csp_nonce = cx.csp_nonce_cloned();

                    #shared_state_lets
                    #prelude_lets_tokens
                    #signal_lets
                    #init_state_json_let
                    #init_query_state_json_let
                    #init_mutations_json_let

                    ::proa_core::RenderOutcome::pending(async move {
                        ::proa_core::WebRender::render(
                            ::proa_macros::html! {
                                < #wrapper_open #island_root_attr_name=#asset_id_lit >
                                    #view_tokens
                                </ #wrapper_close >
                                <script type="application/json" #island_attr_name=#asset_id_lit nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                                    {UnsafeRaw(#init_state_json_raw)}
                                </script>
                                #query_blob
                                #mutations_blob
                            },
                            cx,
                        )
                        .resolve()
                        .await
                    })
                }
            }
        }
    } else {
        quote! {
            #[automatically_derived]
            impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf>
                ::proa_core::WebRenderSync<L, B> for #struct_path
            {
                fn render(
                    self,
                    cx: &mut ::proa_core::WebContext<L, B>,
                ) -> ::core::result::Result<(), ::proa_core::WriteError> {
                    // Bring `UnsafeRaw` into scope by its bare name so
                    // the OWASP-XSS-in-script-or-style lint in
                    // `proa_macros::html_sync!` recognises the raw-content
                    // wrapper (the lint matches on the textual form
                    // `UnsafeRaw(...)`).
                    use ::proa_core::UnsafeRaw;

                    // Route asset graph (rsjs/OPTIMIZATIONS.md § 1):
                    // record this island's asset_id on the WebContext.
                    cx.rsjs_island_used(#asset_id_lit);
                    let __rsjs_csp_nonce = cx.csp_nonce_cloned();

                    #shared_state_lets
                    #prelude_lets_tokens
                    #signal_lets
                    #init_state_json_let
                    #init_query_state_json_let
                    #init_mutations_json_let

                    ::proa_core::WebRenderSync::render(
                        ::proa_macros::html_sync! {
                            < #wrapper_open #island_root_attr_name=#asset_id_lit >
                                #view_tokens
                            </ #wrapper_close >
                            <script type="application/json" #island_attr_name=#asset_id_lit nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                                {UnsafeRaw(#init_state_json_raw)}
                            </script>
                            #query_blob
                        },
                        cx,
                    )
                }
            }
        }
    }
}

/// Macro-path emitter that preserves the user's impl header.
///
/// The legacy emitter above still rebuilds the canonical
/// `impl<L, B> ... for Struct` shape because the old hand-built
/// fixtures and golden tests depend on that output. The proc macro
/// path uses this entry point so generic impl parameters, const
/// parameters, lifetime parameters, trait arguments, and where
/// clauses pass through unchanged.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn emit_rust_preserving_impl_with_prelude_and_asset_id(
    island: &Island,
    impl_generics: &syn::Generics,
    trait_path: &syn::Path,
    self_ty: &syn::Type,
    render_sig: &syn::Signature,
    context_ident: &syn::Ident,
    asset_id: &str,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    query_initializers: &[QueryInitializer],
    mutation_initializers: &[MutationInitializer],
    extra_prelude_lets: &[syn::Local],
    ide_helper_hints: &TokenStream,
    ide_source_hints: &TokenStream,
) -> TokenStream {
    let (impl_params, _, where_clause) = impl_generics.split_for_impl();
    let render_body = emit_render_body(
        island,
        asset_id,
        signal_initializers,
        shared_state_initializers,
        query_initializers,
        mutation_initializers,
        extra_prelude_lets,
        ide_helper_hints,
        ide_source_hints,
        trait_path,
        context_ident,
    );
    let render_method = quote! { #render_sig #render_body };

    quote! {
        #[automatically_derived]
        impl #impl_params #trait_path for #self_ty #where_clause {
            #render_method
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn emit_render_body(
    island: &Island,
    asset_id: &str,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    query_initializers: &[QueryInitializer],
    mutation_initializers: &[MutationInitializer],
    extra_prelude_lets: &[syn::Local],
    ide_helper_hints: &TokenStream,
    ide_source_hints: &TokenStream,
    trait_path: &syn::Path,
    context_ident: &syn::Ident,
) -> TokenStream {
    let strategy_analysis = rsjs_ir::classify_render_strategy(island);
    let strategy = strategy_analysis.strategy;
    let reachable_signal_ids = strategy_analysis.reachable_signal_ids();
    let reachable_query_ids = strategy_analysis.reachable_query_ids();
    let reachable_mutation_ids = strategy_analysis.reachable_mutation_ids();
    let reachable_payload_attrs = emitted_payload_attrs(island, &strategy_analysis);
    let asset_id_lit = asset_id;
    let trait_witness = if island.is_async {
        quote! {
            let _: &::proa_core::render::web_render::__private::AsyncRenderTraitWitness =
                &<Self as #trait_path>::__PROA_RENDER_TRAIT_WITNESS;
        }
    } else {
        quote! {
            let _: &::proa_core::render::web_render::__private::SyncRenderTraitWitness =
                &<Self as #trait_path>::__PROA_RENDER_TRAIT_WITNESS;
        }
    };

    let text_binding_order: Vec<NodeId> = sorted_text_bindings(island);
    let event_binding_order: Vec<(NodeId, EventName)> = sorted_event_bindings(island);

    let prelude_lets_tokens: TokenStream = extra_prelude_lets
        .iter()
        .map(|local| quote! { #local })
        .collect();
    let rust_signal_ids = rust_signal_initializer_ids(island, &reachable_signal_ids);
    let shared_state_lets = emit_shared_state_initializers(
        island,
        shared_state_initializers,
        &reachable_signal_ids,
        asset_id,
        context_ident,
        None,
    );
    let signal_lets = emit_signal_initializers(island, signal_initializers, &rust_signal_ids);
    let query_value_lets =
        emit_query_initial_value_locals(island, query_initializers, &reachable_query_ids);
    let init_state_json = emit_init_state_json(
        island,
        signal_initializers,
        shared_state_initializers,
        &reachable_signal_ids,
    );
    let init_state_json_let = init_state_json.let_tokens;
    let init_state_json_raw = init_state_json.raw_expr;
    let init_query_state_json_let = emit_init_query_state_json(
        island,
        query_initializers,
        &reachable_query_ids,
        context_ident,
    );
    let init_mutations_json_let =
        emit_init_mutations_json(island, mutation_initializers, &reachable_mutation_ids);
    let query_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-queries={::proa_core::perf_ok(#asset_id_lit)}
    };
    let query_blob = if reachable_query_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #query_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_query_state_json.as_ref())}
            </script>
        }
    };
    let mutations_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-mutations={::proa_core::perf_ok(#asset_id_lit)}
    };
    let mutations_blob = if reachable_mutation_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #mutations_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_mutations_json.as_ref())}
            </script>
        }
    };

    let island_attr_name = attr_name_tokens(ISLAND_MARKER_ATTR);
    let island_root_attr_name = attr_name_tokens(ISLAND_ROOT_ATTR);
    let wrapper_open = tag_name_tokens(ISLAND_WRAPPER_TAG);
    let wrapper_close = tag_name_tokens(ISLAND_WRAPPER_TAG);
    let fragment_marker_aliases = component_fragment_marker_aliases(island);
    let cond_branches = compute_conditional_branch_metadata(
        island,
        signal_initializers,
        query_initializers,
        &fragment_marker_aliases,
    );

    let view_tokens = match island.view_tree.root() {
        Some(root) => emit_view_node(
            root,
            island,
            &text_binding_order,
            &event_binding_order,
            &cond_branches,
            &fragment_marker_aliases,
            &reachable_payload_attrs,
            signal_initializers,
            query_initializers,
            None,
            None,
            None,
        ),
        None => quote! {},
    };

    if strategy == rsjs_ir::RenderStrategy::ServerOnly {
        if island.is_async {
            return quote! {
                {
                        #trait_witness
                        ::proa_core::RenderOutcome::pending(async move {
                            use ::proa_core::UnsafeRaw;

                            #ide_helper_hints
                            // Shared-state handles are declared before the
                            // replayed prelude: the author wrote
                            // `let s = rsjs::shared::<T>();` above the ordinary
                            // server `let`s, and those `let`s may read `s`.
                            #shared_state_lets
                            #prelude_lets_tokens
                            #ide_source_hints
                            #signal_lets

                            ::proa_core::WebRender::render(
                                ::proa_macros::html! {
                                #view_tokens
                            },
                            #context_ident,
                        )
                        .resolve()
                        .await
                    })
                }
            };
        }

        return quote! {
            {
                    #trait_witness
                    use ::proa_core::UnsafeRaw;

                    #ide_helper_hints
                    // See the async branch above: shared-state handles bind
                    // before the replayed prelude reads them.
                    #shared_state_lets
                    #prelude_lets_tokens
                    #ide_source_hints
                    #signal_lets

                    ::proa_core::WebRenderSync::render(
                        ::proa_macros::html_sync! {
                        #view_tokens
                    },
                    #context_ident,
                )
            }
        };
    }

    if island.is_async {
        quote! {
            {
                #trait_witness
                // Route asset graph (rsjs/OPTIMIZATIONS.md § 1):
                // record this island's asset_id on the WebContext so
                // the framework adapter can emit script tags only for
                // the islands actually rendered. Side-effect only;
                // duplicates dedupe in the underlying HashSet.
                #context_ident.rsjs_island_used(#asset_id_lit);
                let __rsjs_csp_nonce = #context_ident.csp_nonce_cloned();
                ::proa_core::RenderOutcome::pending(async move {
                    use ::proa_core::UnsafeRaw;

                    #ide_helper_hints
                    // See `emit_render_body`'s server-only branch: shared-state
                    // handles bind before the replayed prelude reads them.
                    #shared_state_lets
                    #prelude_lets_tokens
                    #ide_source_hints
                    #signal_lets
                    #query_value_lets
                    #init_state_json_let
                    #init_query_state_json_let
                    #init_mutations_json_let

                    ::proa_core::WebRender::render(
                        ::proa_macros::html! {
                            < #wrapper_open #island_root_attr_name=#asset_id_lit >
                                #view_tokens
                            </ #wrapper_close >
                            <script type="application/json" #island_attr_name=#asset_id_lit nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                                {UnsafeRaw(#init_state_json_raw)}
                            </script>
                            #query_blob
                            #mutations_blob
                        },
                        #context_ident,
                    )
                    .resolve()
                    .await
                })
            }
        }
    } else {
        quote! {
            {
                #trait_witness
                // Bring `UnsafeRaw` into scope by its bare name so
                // the OWASP-XSS-in-script-or-style lint in
                // `proa_macros::html_sync!` recognises the raw-content
                // wrapper (the lint matches on the textual form
                // `UnsafeRaw(...)`).
                use ::proa_core::UnsafeRaw;

                // Route asset graph (rsjs/OPTIMIZATIONS.md § 1):
                // record this island's asset_id on the WebContext so
                // the framework adapter can emit script tags only for
                // the islands actually rendered. Recorded BEFORE the
                // body writes so streaming flushes see the asset id
                // in the set as soon as the chunk lands.
                #context_ident.rsjs_island_used(#asset_id_lit);
                let __rsjs_csp_nonce = #context_ident.csp_nonce_cloned();

                #ide_helper_hints
                // See `emit_render_body`'s server-only branch: shared-state
                // handles bind before the replayed prelude reads them.
                #shared_state_lets
                #prelude_lets_tokens
                #ide_source_hints
                #signal_lets
                #query_value_lets
                #init_state_json_let
                #init_query_state_json_let
                #init_mutations_json_let

                ::proa_core::WebRenderSync::render(
                    ::proa_macros::html_sync! {
                        < #wrapper_open #island_root_attr_name=#asset_id_lit >
                            #view_tokens
                        </ #wrapper_close >
                        <script type="application/json" #island_attr_name=#asset_id_lit nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                            {UnsafeRaw(#init_state_json_raw)}
                        </script>
                        #query_blob
                        #mutations_blob
                    },
                    #context_ident,
                )
            }
        }
    }
}

/// The two pieces needed by the lossless authored-program rewriter.
///
/// `setup` contains only work that belongs immediately before the analyzed
/// render site (artifact publication, hydration seed construction, and
/// compiler-only IDE witnesses). `template` is one `html_sync!` / `html!`
/// invocation. The caller keeps the authored `.render(cx)` chain around that
/// invocation, so method dispatch and async polling remain ordinary Rust.
pub(crate) struct LosslessRenderSite {
    pub(crate) setup: TokenStream,
    pub(crate) template: TokenStream,
}

/// Emit the final analyzed template without reconstructing the surrounding
/// render method.
///
/// Compiler-owned declarations which have an authored lexical site are
/// emitted by `server_program` at that site. The `prepared_*` sets identify
/// those declarations. Anything introduced by descriptor composition has no
/// authored parent-method statement, so it is emitted here immediately before
/// the render site.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_lossless_render_site(
    island: &Island,
    asset_id: &str,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    query_initializers: &[QueryInitializer],
    mutation_initializers: &[MutationInitializer],
    prepared_signal_ids: &HashSet<SignalId>,
    required_server_signal_ids: &HashSet<SignalId>,
    prepared_shared_store_ids: &HashSet<SharedStoreId>,
    prepared_query_ids: &HashSet<QueryId>,
    prepared_mutation_ids: &HashSet<MutationId>,
    ide_helper_hints: &TokenStream,
    ide_source_hints: &TokenStream,
    context_ident: &syn::Ident,
    server_expressions: &HashMap<NodeId, syn::Expr>,
    direct_component_site_nodes: &HashMap<NodeId, u32>,
    linked_site_ident: Option<&syn::Ident>,
) -> LosslessRenderSite {
    let strategy_analysis = rsjs_ir::classify_render_strategy(island);
    let strategy = strategy_analysis.strategy;
    let reachable_signal_ids = strategy_analysis.reachable_signal_ids();
    let reachable_query_ids = strategy_analysis.reachable_query_ids();
    let reachable_mutation_ids = strategy_analysis.reachable_mutation_ids();
    let reachable_payload_attrs = emitted_payload_attrs(island, &strategy_analysis);
    let asset_id_lit = asset_id;

    let text_binding_order: Vec<NodeId> = sorted_text_bindings(island);
    let event_binding_order: Vec<(NodeId, EventName)> = sorted_event_bindings(island);
    let mut rust_signal_ids = rust_signal_initializer_ids(island, &reachable_signal_ids);
    for signal in required_server_signal_ids {
        if !rust_signal_ids.contains(signal) {
            rust_signal_ids.push(*signal);
        }
    }
    rust_signal_ids.sort_by_key(|signal| signal.0);
    let remaining_signal_ids: Vec<_> = rust_signal_ids
        .iter()
        .copied()
        .filter(|id| !prepared_signal_ids.contains(id))
        .collect();
    let remaining_shared_state_initializers: Vec<_> = shared_state_initializers
        .iter()
        .filter(|init| !prepared_shared_store_ids.contains(&init.id))
        .cloned()
        .collect();
    let remaining_query_initializers: Vec<_> = query_initializers
        .iter()
        .filter(|init| !prepared_query_ids.contains(&init.id))
        .cloned()
        .collect();

    let shared_state_lets = emit_shared_state_initializers(
        island,
        &remaining_shared_state_initializers,
        &reachable_signal_ids,
        asset_id,
        context_ident,
        linked_site_ident,
    );
    let signal_lets = emit_signal_initializers(island, signal_initializers, &remaining_signal_ids);
    let query_value_lets = emit_query_initial_value_locals(
        island,
        &remaining_query_initializers,
        &reachable_query_ids,
    );
    let init_state_json = emit_init_state_json(
        island,
        signal_initializers,
        shared_state_initializers,
        &reachable_signal_ids,
    );
    let init_state_json_let = init_state_json.let_tokens;
    let init_state_json_raw = init_state_json.raw_expr;
    let init_query_state_json_let = emit_lossless_init_query_state_json(
        island,
        query_initializers,
        &reachable_query_ids,
        prepared_query_ids,
        context_ident,
    );
    let init_mutations_json_let = emit_lossless_init_mutations_json(
        island,
        mutation_initializers,
        &reachable_mutation_ids,
        prepared_mutation_ids,
    );

    let query_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-queries={::proa_core::perf_ok(#asset_id_lit)}
    };
    let query_blob = if reachable_query_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #query_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_query_state_json.as_ref())}
            </script>
        }
    };
    let mutations_blob_attrs = quote! {
        type="application/json"
        data-rsjs-island-mutations={::proa_core::perf_ok(#asset_id_lit)}
    };
    let mutations_blob = if reachable_mutation_ids.is_empty() {
        quote! {}
    } else {
        quote! {
            <script #mutations_blob_attrs nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
                {UnsafeRaw(__rsjs_init_mutations_json.as_ref())}
            </script>
        }
    };

    let fragment_marker_aliases = component_fragment_marker_aliases(island);
    let cond_branches = compute_conditional_branch_metadata(
        island,
        signal_initializers,
        query_initializers,
        &fragment_marker_aliases,
    );
    let view_tokens = match island.view_tree.root() {
        Some(root) => emit_view_node(
            root,
            island,
            &text_binding_order,
            &event_binding_order,
            &cond_branches,
            &fragment_marker_aliases,
            &reachable_payload_attrs,
            signal_initializers,
            query_initializers,
            Some(server_expressions),
            Some(direct_component_site_nodes),
            linked_site_ident,
        ),
        None => quote! {},
    };

    if let Some(linked_site_ident) = linked_site_ident {
        let linked_init_state_json_let = quote! {
            let __rsjs_linked_init_state_json =
                ::rsjs::__private::linked_owner_value(
                    #linked_site_ident,
                    || {
                        #init_state_json_let
                        ::core::result::Result::<_, ::proa_core::WriteError>::Ok(
                            __rsjs_init_state_json
                        )
                    },
                )?;
        };
        let (linked_init_query_state_json_let, query_state) = if reachable_query_ids.is_empty() {
            (quote! {}, quote! { ::core::option::Option::None })
        } else {
            (
                quote! {
                    let __rsjs_linked_init_query_state_json =
                        ::rsjs::__private::linked_owner_value(
                            #linked_site_ident,
                            || {
                                #init_query_state_json_let
                                ::core::result::Result::<_, ::proa_core::WriteError>::Ok(
                                    __rsjs_init_query_state_json
                                )
                            },
                        )?;
                },
                quote! {
                    __rsjs_linked_init_query_state_json.as_deref()
                },
            )
        };
        let (linked_init_mutations_json_let, mutation_state) = if reachable_mutation_ids.is_empty()
        {
            (quote! {}, quote! { ::core::option::Option::None })
        } else {
            (
                quote! {
                    let __rsjs_linked_init_mutations_json =
                        ::rsjs::__private::linked_owner_value(
                            #linked_site_ident,
                            || {
                                #init_mutations_json_let
                                ::core::result::Result::<_, ::proa_core::WriteError>::Ok(
                                    __rsjs_init_mutations_json
                                )
                            },
                        )?;
                },
                quote! {
                    __rsjs_linked_init_mutations_json.as_deref()
                },
            )
        };
        let signal_state = quote! {
            __rsjs_linked_init_state_json.as_deref()
        };
        let boundary = if island.is_async {
            quote! {
                ::rsjs::__private::LinkedRenderBoundary::new(
                    ::proa_macros::html! { #view_tokens },
                    #linked_site_ident,
                    #signal_state,
                    #query_state,
                    #mutation_state,
                )
            }
        } else {
            quote! {
                ::rsjs::__private::LinkedRenderBoundary::new(
                    ::proa_macros::html_sync! { #view_tokens },
                    #linked_site_ident,
                    #signal_state,
                    #query_state,
                    #mutation_state,
                )
            }
        };
        let template = if island.is_async {
            quote! { ::proa_macros::html! { { #boundary } } }
        } else {
            quote! { ::proa_macros::html_sync! { { #boundary } } }
        };
        return LosslessRenderSite {
            setup: quote! {
                use ::proa_core::UnsafeRaw;
                #ide_helper_hints
                #shared_state_lets
                #signal_lets
                #ide_source_hints
                #query_value_lets
                #linked_init_state_json_let
                #linked_init_query_state_json_let
                #linked_init_mutations_json_let
            },
            template,
        };
    }

    if strategy == rsjs_ir::RenderStrategy::ServerOnly {
        let template = if island.is_async {
            quote! { ::proa_macros::html! { #view_tokens } }
        } else {
            quote! { ::proa_macros::html_sync! { #view_tokens } }
        };
        return LosslessRenderSite {
            setup: quote! {
                use ::proa_core::UnsafeRaw;
                #ide_helper_hints
                #shared_state_lets
                #signal_lets
                #ide_source_hints
                #query_value_lets
            },
            template,
        };
    }

    let island_attr_name = attr_name_tokens(ISLAND_MARKER_ATTR);
    let island_root_attr_name = attr_name_tokens(ISLAND_ROOT_ATTR);
    let wrapper_open = tag_name_tokens(ISLAND_WRAPPER_TAG);
    let wrapper_close = tag_name_tokens(ISLAND_WRAPPER_TAG);
    let template_body = quote! {
        < #wrapper_open #island_root_attr_name=#asset_id_lit >
            #view_tokens
        </ #wrapper_close >
        <script type="application/json" #island_attr_name=#asset_id_lit nonce={::proa_core::csp_nonce_attr(__rsjs_csp_nonce.as_ref())}>
            {UnsafeRaw(#init_state_json_raw)}
        </script>
        #query_blob
        #mutations_blob
    };
    let template = if island.is_async {
        quote! { ::proa_macros::html! { #template_body } }
    } else {
        quote! { ::proa_macros::html_sync! { #template_body } }
    };

    LosslessRenderSite {
        setup: quote! {
            use ::proa_core::UnsafeRaw;
            #context_ident.rsjs_island_used(#asset_id_lit);
            let __rsjs_csp_nonce = #context_ident.csp_nonce_cloned();
            #ide_helper_hints
            #shared_state_lets
            #signal_lets
            #ide_source_hints
            #query_value_lets
            #init_state_json_let
            #init_query_state_json_let
            #init_mutations_json_let
        },
        template,
    }
}

/// Signal ids whose initial server values are required by the emitted view or
/// hydration seeds.
pub(crate) fn lossless_rust_signal_ids(island: &Island) -> Vec<SignalId> {
    let analysis = rsjs_ir::classify_render_strategy(island);
    rust_signal_initializer_ids(island, &analysis.reachable_signal_ids())
}

/// Emit one compiler-owned signal/memo/shared-field server binding at the
/// exact lexical position of its authored declaration.
pub(crate) fn emit_lossless_signal_declaration(
    island: &Island,
    signal_initializers: &[SignalInitializer],
    id: SignalId,
    required_ids: &[SignalId],
) -> TokenStream {
    if !required_ids.contains(&id) {
        return TokenStream::new();
    }
    emit_signal_initializers(island, signal_initializers, &[id])
}

/// Emit one SharedState acquisition at its authored lexical position.
pub(crate) fn emit_lossless_shared_state_declaration(
    island: &Island,
    shared_state_initializer: &SharedStateInitializer,
    id: SharedStoreId,
    asset_id: &str,
    context_ident: &syn::Ident,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    debug_assert_eq!(shared_state_initializer.id, id);
    let analysis = rsjs_ir::classify_render_strategy(island);
    emit_shared_state_initializers(
        island,
        std::slice::from_ref(shared_state_initializer),
        &analysis.reachable_signal_ids(),
        asset_id,
        context_ident,
        linked_site_ident,
    )
}

/// Emit one query's server initial value at its authored lexical position.
pub(crate) fn emit_lossless_query_declaration(
    island: &Island,
    query_initializer: &QueryInitializer,
    id: QueryId,
    reachable_query_ids: &[QueryId],
    retain_typed_initial: bool,
    source_ide_hint: Option<&TokenStream>,
) -> TokenStream {
    if !reachable_query_ids.contains(&id) {
        return TokenStream::new();
    }
    let key = query_initializer.key_expr.as_ref().map(|key_expr| {
        let key_expr =
            live_query_key_server_expr(island, id).unwrap_or_else(|| quote! { #key_expr });
        let key_json = query_key_json_local_ident(id);
        quote! {
            let #key_json =
                ::rsjs::__private::to_script_json(&(#key_expr))
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?;
        }
    });
    let typed_local = query_initial_typed_value_local_ident(id);
    let local = query_initial_value_local_ident(id);
    let expr = &query_initializer.expr;
    let initial = if retain_typed_initial {
        quote! {
            let #typed_local = #expr;
            #source_ide_hint
            let #local: ::rsjs::__private::serde_json::Value =
                ::rsjs::__private::serde_json::to_value(&#typed_local)
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?;
        }
    } else {
        quote! {
            let #local: ::rsjs::__private::serde_json::Value = {
                let #typed_local = #expr;
                #source_ide_hint
                ::rsjs::__private::serde_json::to_value(&#typed_local)
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?
            };
        }
    };
    quote! {
        #key
        #initial
    }
}

/// Rebuild a signal-derived query key from the compiler-owned server signal
/// locals.
///
/// The lossless server-program rewriter removes the authored
/// `let selected = signal(...)` binding. Replaying the authored query key
/// (`selected.get()`) after that removal therefore references a local that no
/// longer exists. Client IR already resolves those reads to stable `SignalId`s;
/// use the matching `__rsjs_initial_signal_<id>` locals for the SSR key and
/// adjacent dehydrated-state identity. Server-only key expressions keep their
/// authored Rust spelling through the `None` fallback above.
fn live_query_key_server_expr(island: &Island, id: QueryId) -> Option<TokenStream> {
    let query = island.queries.iter().find(|query| query.id == id)?;
    if rsjs_ir::analysis::collect_deps(&island.exprs, query.key_expr).is_empty() {
        return None;
    }

    match island.exprs.client_expr(query.key_expr) {
        ClientExpr::QueryKey(parts) => {
            let parts = parts
                .iter()
                .map(live_query_key_part_server_expr)
                .collect::<Option<Vec<_>>>()?;
            Some(quote! { (#(#parts),*,) })
        }
        expr => {
            let expr = live_query_key_part_server_expr(expr)?;
            Some(quote! { (#expr,) })
        }
    }
}

fn live_query_key_part_server_expr(expr: &ClientExpr) -> Option<TokenStream> {
    if let ClientExpr::SignalRead(signal) = expr {
        let local = signal_local_ident(*signal);
        // Query-key serialization only borrows the value. Keeping direct
        // signal parts by reference avoids moving String/Vec/object seeds
        // before the later hydration-state serializer and SSR bindings read
        // the same compiler-owned local.
        return Some(quote! { &#local });
    }
    emit_expr_as_rust(expr, &[])
}

pub(crate) fn query_initial_requires_typed_value(island: &Island, id: QueryId) -> bool {
    let needle = query_initial_typed_value_local_ident(id).to_string();
    (0..island.view_tree.len()).any(|index| {
        matches!(
            island.view_tree.get(NodeId(index as u32)),
            ViewNode::ForLoop { expr, .. } if expr.contains(&needle)
        )
    })
}

/// Evaluate one mutation's server-visible metadata at the declaration's exact
/// lexical position. The later page-blob builder only concatenates these
/// prepared strings, so ordinary statements retain their authored ordering
/// around the compiler-owned declaration.
pub(crate) fn emit_lossless_mutation_declaration(
    mutation_initializer: &MutationInitializer,
    id: MutationId,
    reachable_mutation_ids: &[MutationId],
) -> TokenStream {
    if !reachable_mutation_ids.contains(&id) {
        return TokenStream::new();
    }
    let key_expr = &mutation_initializer.key_expr;
    let key_json = mutation_json_local_ident(id, "key");
    let invalidates = mutation_initializer.invalidates_expr.as_ref().map(|expr| {
        let invalidates_json = mutation_json_local_ident(id, "invalidates");
        quote! {
            let #invalidates_json =
                ::rsjs::__private::to_script_json(&(#expr))
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?;
        }
    });
    quote! {
        let #key_json =
            ::rsjs::__private::to_script_json(&(#key_expr))
                .map_err(|err| {
                    ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                })?;
        #invalidates
    }
}

/// Sort text-binding NodeIds for deterministic index assignment.
fn sorted_text_bindings(island: &Island) -> Vec<NodeId> {
    let mut keys: Vec<NodeId> = island.bindings.text_bindings.keys().copied().collect();
    keys.sort_by_key(|n| n.0);
    keys
}

/// Sort event-binding `(NodeId, EventName)` pairs for deterministic
/// index assignment. Primary key is NodeId, secondary is the source
/// attribute name so regular and document-scoped events that share a DOM
/// name (`click`, `keydown`, etc.) still stay distinct and stable.
fn sorted_event_bindings(island: &Island) -> Vec<(NodeId, EventName)> {
    let mut keys: Vec<(NodeId, EventName)> =
        island.bindings.event_bindings.keys().copied().collect();
    keys.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then(a.1.as_attr_name().cmp(b.1.as_attr_name()))
    });
    keys
}

/// Payload attrs are public data surfaces only when the strategy ledger records
/// a reachable handler/body read for that specific `(node, attr)` pair.
fn reachable_payload_attrs(analysis: &rsjs_ir::StrategyAnalysis) -> Vec<(NodeId, String)> {
    let mut attrs = Vec::new();
    for crossing in &analysis.crossings {
        match &crossing.kind {
            rsjs_ir::ClientCrossingKind::HandlerPayload { node, attr }
            | rsjs_ir::ClientCrossingKind::NodePayload { node, attr } => {
                attrs.push((*node, attr.clone()));
            }
            _ => {}
        }
    }
    attrs.sort_by(|(left_node, left_attr), (right_node, right_attr)| {
        left_node
            .0
            .cmp(&right_node.0)
            .then_with(|| left_attr.cmp(right_attr))
    });
    attrs.dedup();
    attrs
}

fn payload_attr_is_reachable(
    reachable_payload_attrs: &[(NodeId, String)],
    node: NodeId,
    attr: &str,
) -> bool {
    reachable_payload_attrs
        .iter()
        .any(|(reachable_node, reachable_attr)| *reachable_node == node && reachable_attr == attr)
}

fn emitted_payload_attrs(
    island: &Island,
    analysis: &rsjs_ir::StrategyAnalysis,
) -> Vec<(NodeId, String)> {
    let mut attrs = reachable_payload_attrs(analysis);
    attrs.extend(fragment_owned_payload_attrs(island));
    attrs.sort_by(|(left_node, left_attr), (right_node, right_attr)| {
        left_node
            .0
            .cmp(&right_node.0)
            .then_with(|| left_attr.cmp(right_attr))
    });
    attrs.dedup();
    attrs
}

fn fragment_owned_payload_attrs(island: &Island) -> Vec<(NodeId, String)> {
    let mut owned_nodes: HashSet<NodeId> = HashSet::new();
    for mount in &island.component_fragment_mounts {
        owned_nodes.extend(mount.owned_nodes.iter().copied());
    }

    let mut attrs = Vec::new();
    for node in owned_nodes {
        if let ViewNode::Element { payload_attrs, .. } = island.view_tree.get(node) {
            attrs.extend(payload_attrs.iter().map(|attr| (node, attr.name.clone())));
        }
    }
    attrs
}

/// Acquire every page-scoped store that contributes a reachable shared field.
/// The generated Rust names and field methods are intentionally typechecked by
/// rustc, so a state type may live outside the island module without requiring
/// the HIR frontend to resolve its definition.
fn emit_shared_state_initializers(
    island: &Island,
    initializers: &[SharedStateInitializer],
    reachable_signal_ids: &[SignalId],
    asset_id: &str,
    context_ident: &syn::Ident,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    let required: HashSet<SharedStoreId> = island
        .signals
        .iter()
        .filter(|signal| reachable_signal_ids.contains(&signal.id))
        .filter_map(|signal| signal.shared.as_ref().map(|shared| shared.store))
        .collect();
    let mut out = TokenStream::new();
    for init in initializers
        .iter()
        .filter(|init| required.contains(&init.id))
    {
        let ty = &init.ty;
        let source_name = &init.source_name;
        let value_name = format_ident!("__rsjs_shared_value_{}", init.id.0);
        let scope_name = shared_scope_local_ident(init.id);
        out.extend(emit_shared_type_key_assertion(
            ty,
            init.expected_type_key.clone(),
        ));
        let acquire = match linked_site_ident {
            Some(linked_site_ident) => {
                let reference_id = init.id.0;
                quote! {
                    #context_ident.rsjs_shared_state_acquire_linked::<#ty>(
                        #linked_site_ident.shared_state_provider(#reference_id),
                        <#ty as ::rsjs::SharedState>::TYPE_KEY,
                        <#ty as ::rsjs::SharedState>::schema_hash(),
                        <#ty as ::rsjs::SharedState>::META.rust_type,
                        #asset_id,
                    )
                }
            }
            None => quote! {
                #context_ident.rsjs_shared_state_acquire::<#ty>(
                    <#ty as ::rsjs::SharedState>::TYPE_KEY,
                    <#ty as ::rsjs::SharedState>::schema_hash(),
                    <#ty as ::rsjs::SharedState>::META.rust_type,
                    #asset_id,
                )
            },
        };
        out.extend(quote! {
            let (#value_name, #scope_name) = #acquire?;
            let #source_name = <#ty as ::rsjs::SharedState>::__rsjs_handle_from_server(#value_name);
        });
    }
    out
}

/// Emit a compile-time check that the Rust type rustc resolves at this call
/// site really is the shared state the island was lowered against.
///
/// The check exists because a *name* is not an identity. A component that
/// acquires `Cfg` and is then fully inlined into a parent module re-resolves
/// `Cfg` in the parent's scope; if the parent has its own `Cfg`, the inlined
/// markup would silently read and write an unrelated provider scope, with no
/// error at compile time and no exception at run time. Wrong data, quietly.
///
/// `expected` is the `SharedState::TYPE_KEY` the component recorded in its own
/// crate, where the acquisition was written. The assertion compares it against
/// `<#ty as SharedState>::TYPE_KEY` as resolved *here*. That is a trait-item
/// reference over the exact tokens the author wrote, so rustc resolves it: it
/// works through `use` imports, re-exports, aliases, local modules, and across
/// crates, it does not depend on expansion order, and it consults nothing
/// outside this compilation. There is no registry and no lookup, so there is
/// nothing to be stale.
fn emit_shared_type_key_assertion(ty: &syn::Type, expected: Option<TokenStream>) -> TokenStream {
    let Some(expected) = expected else {
        return TokenStream::new();
    };
    let message = syn::LitStr::new(
        "rsjs/shared-state-type-identity: this shared-state acquisition was replayed from an inlined component, and the Rust type name the component wrote resolves to a different `SharedState` in this module. The inlined markup would read and write this module's provider scope instead of the one the component was compiled against. Acquire the state inside the component through a crate-qualified type path, or give the two state structs distinct Rust type names",
        proc_macro2::Span::call_site(),
    );
    quote! {
        const _: () = {
            const fn __rsjs_type_key_matches(actual: &str, expected: &str) -> bool {
                let actual = actual.as_bytes();
                let expected = expected.as_bytes();
                if actual.len() != expected.len() {
                    return false;
                }
                let mut index = 0;
                while index < actual.len() {
                    if actual[index] != expected[index] {
                        return false;
                    }
                    index += 1;
                }
                true
            }
            ::core::assert!(
                __rsjs_type_key_matches(<#ty as ::rsjs::SharedState>::TYPE_KEY, #expected),
                #message
            );
        };
    }
}

/// Rebuild the Rust-side shared-store initializers from an island's IR.
///
/// The component-attribute pipeline threads the analyzer's initializers (which retain the
/// author's `syn::Type`) into [`emit_render_body`]. The standalone
/// [`emit_rust`] entry point has no analyzer output, so it recovers the same
/// information from the IR: `SharedStoreDecl` keeps both the acquisition local
/// name and the Rust type tokens. Returning a `compile_error!` token stream
/// keeps a malformed IR a diagnostic rather than a proc-macro panic.
// proa-perf-ok(scope): compile-time diagnostics; the `::<T>` in the example text is Rust, not HTML
#[cfg(test)]
fn shared_state_initializers_from_island(
    island: &Island,
) -> Result<Vec<SharedStateInitializer>, TokenStream> {
    let mut initializers = Vec::with_capacity(island.shared_stores.len());
    for store in &island.shared_stores {
        let ty = syn::parse_str::<syn::Type>(&store.rust_type).map_err(|error| {
            compile_error_tokens(&format!(
                "rsjs/shared-state-store-type: the shared-state store bound to `{}` records `{}` as its Rust type, which does not parse as a type ({error}). Acquire the store with a plain type path, for example `rsjs::shared::<WorkbenchState>()` or `rsjs::shared::<state_crate::WorkbenchState>()`",
                store.source_name, store.rust_type
            ))
        })?;
        let source_name = syn::parse_str::<syn::Ident>(&store.source_name).map_err(|error| {
            compile_error_tokens(&format!(
                "rsjs/shared-state-store-local: the shared-state store typed `{}` records `{}` as its acquisition local, which is not a Rust identifier ({error}). Bind the acquisition to a plain local, for example `let workbench = rsjs::shared::<WorkbenchState>();`",
                store.rust_type, store.source_name
            ))
        })?;
        initializers.push(SharedStateInitializer {
            id: store.id,
            source_name,
            ty,
            expected_type_key: None,
        });
    }
    Ok(initializers)
}

fn shared_scope_local_ident(id: SharedStoreId) -> syn::Ident {
    format_ident!("__rsjs_shared_scope_{}", id.0)
}

/// Emit `let __rsjs_initial_signal_<N>: <T> = <literal>;` lines for
/// reachable signals, in declaration order. The locals are then
/// referenced from inside the `html_sync!` body by signal index.
fn emit_signal_initializers(
    island: &Island,
    signal_initializers: &[SignalInitializer],
    reachable_signal_ids: &[SignalId],
) -> TokenStream {
    let mut out = TokenStream::new();
    for sig in island
        .signals
        .iter()
        .filter(|sig| reachable_signal_ids.contains(&sig.id))
    {
        let name = signal_local_ident(sig.id);
        if let Some(shared) = &sig.shared {
            let store = island
                .shared_stores
                .iter()
                .find(|store| store.id == shared.store)
                .expect("shared field must reference a declared shared store");
            let source = format_ident!("{}", store.source_name);
            // Borrow-versus-value is decided by `#[derive(SharedState)]`,
            // which has the field's real Rust type, and emitted as a
            // per-field `__rsjs_replay_<field>()` accessor. The island macro
            // cannot see that type (see `Analyzer::shared_field_client_type`),
            // and guessing it from the IR's `ClientType` was exactly the
            // dependency this design removed. rustc type-checks the result.
            let replay = format_ident!("__rsjs_replay_{}", shared.field);
            let integer_domain_assertion = sig.integer_domain.map(|domain| {
                let ty = integer_domain_rust_type_tokens(domain);
                quote! {
                    let _: &#ty = &#name;
                }
            });
            out.extend(quote! {
                let #name = #source.#replay();
                #integer_domain_assertion
            });
            continue;
        }
        if let Some(body) = sig.derived {
            // Memo cell: replay the closure body as ordinary
            // server Rust over the earlier `__rsjs_initial_signal_*`
            // locals (declaration order is id order, and a derived
            // body can only read names declared before it), so SSR
            // shows the derived value evaluated over signal initials.
            let expr = emit_expr_as_rust(island.exprs.client_expr(body), &[]);
            match expr {
                Some(expr) => out.extend(quote! {
                    let #name = #expr;
                }),
                None => {
                    let (ty, lit) = render_value_as_rust(&sig.init);
                    out.extend(quote! {
                        let #name: #ty = #lit;
                    });
                }
            }
            continue;
        }
        if let Some(init) = signal_initializer_for(signal_initializers, sig.id) {
            let expr = &init.expr;
            let explicit_ty = init
                .explicit_ty
                .as_ref()
                .map(|ty| quote! { : #ty })
                .unwrap_or_else(TokenStream::new);
            out.extend(quote! {
                let #name #explicit_ty = #expr;
            });
        } else if let Some(domain) = sig.integer_domain {
            let (ty, lit) = render_integer_value_as_rust(domain, &sig.init)
                .expect("integer-domain signal must have an integer fallback");
            out.extend(quote! {
                let #name: #ty = #lit;
            });
        } else {
            let (ty, lit) = render_value_as_rust(&sig.init);
            out.extend(quote! {
                let #name: #ty = #lit;
            });
        }
    }
    out
}

fn rust_signal_initializer_ids(
    island: &Island,
    reachable_signal_ids: &[SignalId],
) -> Vec<SignalId> {
    let mut ids: HashSet<SignalId> = HashSet::new();
    let mut visiting: HashSet<SignalId> = HashSet::new();

    for id in reachable_signal_ids {
        collect_signal_id_with_deps(island, *id, &mut ids, &mut visiting);
    }

    for expr_id in island.bindings.text_bindings.values() {
        collect_signal_reads_from_expr(
            island.exprs.client_expr(*expr_id),
            island,
            &mut ids,
            &mut visiting,
        );
    }
    for binding in island.bindings.attr_bindings.values() {
        collect_signal_reads_from_expr(
            island.exprs.client_expr(binding.expr),
            island,
            &mut ids,
            &mut visiting,
        );
    }
    for binding in island.bindings.input_bindings.values() {
        collect_signal_id_with_deps(island, binding.signal, &mut ids, &mut visiting);
    }
    for sections in island.bindings.conditional_sections.values() {
        for section in sections {
            collect_signal_reads_from_expr(
                island.exprs.client_expr(section.condition),
                island,
                &mut ids,
                &mut visiting,
            );
        }
    }
    for list in &island.lists {
        match list.source {
            ListSource::Signal(signal) => {
                collect_signal_id_with_deps(island, signal, &mut ids, &mut visiting);
            }
            ListSource::Expr(expr) => {
                collect_signal_reads_from_expr(
                    island.exprs.client_expr(expr),
                    island,
                    &mut ids,
                    &mut visiting,
                );
            }
        }
    }

    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_by_key(|id| id.0);
    ids
}

fn collect_signal_id_with_deps(
    island: &Island,
    id: SignalId,
    ids: &mut HashSet<SignalId>,
    visiting: &mut HashSet<SignalId>,
) {
    ids.insert(id);
    if !visiting.insert(id) {
        return;
    }
    if let Some(signal) = island.signals.iter().find(|signal| signal.id == id) {
        if let Some(body) = signal.derived {
            collect_signal_reads_from_expr(island.exprs.client_expr(body), island, ids, visiting);
        }
    }
    visiting.remove(&id);
}

fn collect_signal_reads_from_expr(
    expr: &ClientExpr,
    island: &Island,
    ids: &mut HashSet<SignalId>,
    visiting: &mut HashSet<SignalId>,
) {
    match expr {
        ClientExpr::SignalRead(id) => collect_signal_id_with_deps(island, *id, ids, visiting),
        ClientExpr::QueryKey(parts) | ClientExpr::Call { args: parts, .. } => {
            for part in parts {
                collect_signal_reads_from_expr(part, island, ids, visiting);
            }
        }
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_signal_reads_from_expr(index, island, ids, visiting);
        }
        ClientExpr::DomRead { target, .. } => {
            collect_signal_reads_from_expr(target, island, ids, visiting);
        }
        ClientExpr::BrowserRead(read) => {
            collect_signal_reads_from_browser_read(read, island, ids, visiting);
        }
        ClientExpr::Encoding { value, .. }
        | ClientExpr::IsNonEmpty { target: value }
        | ClientExpr::StringToLowercase { target: value }
        | ClientExpr::StringTransform { target: value, .. }
        | ClientExpr::Project { target: value, .. }
        | ClientExpr::AsType { target: value, .. }
        | ClientExpr::IsPresent { target: value }
        | ClientExpr::Eq { target: value, .. }
        | ClientExpr::Neq { target: value, .. }
        | ClientExpr::Coalesce { target: value, .. }
        | ClientExpr::Unary { expr: value, .. }
        | ClientExpr::Field { base: value, .. } => {
            collect_signal_reads_from_expr(value, island, ids, visiting);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        } => {
            collect_signal_reads_from_expr(haystack, island, ids, visiting);
            collect_signal_reads_from_expr(needle, island, ids, visiting);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_signal_reads_from_expr(condition, island, ids, visiting);
            collect_signal_reads_from_expr(then_expr, island, ids, visiting);
            collect_signal_reads_from_expr(else_expr, island, ids, visiting);
        }
        ClientExpr::Index { base, index } => {
            collect_signal_reads_from_expr(base, island, ids, visiting);
            collect_signal_reads_from_expr(index, island, ids, visiting);
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::ArrayLit(_)
        | ClientExpr::Local(_)
        | ClientExpr::QueryRead { .. }
        | ClientExpr::MutationRead { .. }
        | ClientExpr::EventRead(_)
        | ClientExpr::EventCurrentTarget
        | ClientExpr::EventCollectionLen(_)
        | ClientExpr::NodeRefRead { .. }
        | ClientExpr::EventTargetWithinNodeRef { .. }
        | ClientExpr::EventAttribute { .. }
        | ClientExpr::EventPayload { .. }
        | ClientExpr::CallbackPayload
        | ClientExpr::NodePayload { .. } => {}
        _ => {}
    }
}

fn collect_signal_reads_from_browser_read(
    read: &rsjs_ir::ClientBrowserRead,
    island: &Island,
    ids: &mut HashSet<SignalId>,
    visiting: &mut HashSet<SignalId>,
) {
    match read {
        rsjs_ir::ClientBrowserRead::Storage { key, fallback, .. }
        | rsjs_ir::ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_signal_reads_from_expr(key, island, ids, visiting);
            if let Some(fallback) = fallback {
                collect_signal_reads_from_expr(fallback, island, ids, visiting);
            }
        }
        rsjs_ir::ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_signal_reads_from_expr(arg, island, ids, visiting);
            }
        }
        rsjs_ir::ClientBrowserRead::Window(_) | rsjs_ir::ClientBrowserRead::Location(_) => {}
        _ => {}
    }
}

struct InitStateJsonTokens {
    let_tokens: TokenStream,
    raw_expr: TokenStream,
}

/// Emit the dehydrated state JSON literal binding.
///
/// Shape: `{"signals":[<slot0>, <slot1>, ...]}` up to the highest
/// reachable leaf signal id. Dead or derived slots in that range are
/// `null` placeholders so the runtime's SignalId-indexed lookup stays
/// aligned without serializing unreachable values. Static literal-only
/// signals are built by hand into a `&'static str`. Runtime signal
/// initializers such as `signal(self.field)` build the same JSON shape
/// into a `String` during render via `rsjs::ClientEncode`.
fn emit_init_state_json(
    island: &Island,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    reachable_signal_ids: &[SignalId],
) -> InitStateJsonTokens {
    let has_reachable_shared = island
        .signals
        .iter()
        .any(|sig| reachable_signal_ids.contains(&sig.id) && sig.shared.is_some());
    if has_reachable_shared
        || island.signals.iter().any(|sig| {
            reachable_signal_ids.contains(&sig.id)
                && sig.derived.is_none()
                && signal_initializer_for(signal_initializers, sig.id).is_some()
        })
    {
        return emit_runtime_init_state_json(
            island,
            signal_initializers,
            shared_state_initializers,
            reachable_signal_ids,
        );
    }

    let mut json = String::from("{\"signals\":[");
    if let Some(max_id) = max_reachable_leaf_signal_id(island, reachable_signal_ids) {
        for id in 0..=max_id {
            if id > 0 {
                json.push(',');
            }
            let signal = island.signals.iter().find(|sig| sig.id == SignalId(id));
            let Some(sig) = signal else {
                json.push_str("null");
                continue;
            };
            if !reachable_signal_ids.contains(&sig.id) || sig.derived.is_some() {
                json.push_str("null");
                continue;
            }
            push_value_as_json(&mut json, &sig.init);
        }
    }
    json.push_str("]}");
    let json_lit = syn::LitStr::new(&json, proc_macro2::Span::call_site());
    InitStateJsonTokens {
        let_tokens: quote! {
            let __rsjs_init_state_json: &'static str = #json_lit;
        },
        raw_expr: quote! { __rsjs_init_state_json },
    }
}

fn max_reachable_leaf_signal_id(island: &Island, reachable_signal_ids: &[SignalId]) -> Option<u32> {
    island
        .signals
        .iter()
        .filter(|sig| reachable_signal_ids.contains(&sig.id) && sig.derived.is_none())
        .map(|sig| sig.id.0)
        .max()
}

fn emit_runtime_init_state_json(
    island: &Island,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    reachable_signal_ids: &[SignalId],
) -> InitStateJsonTokens {
    let mut tokens = quote! {
        let mut __rsjs_init_state_json = ::std::string::String::from("{\"signals\":[");
    };

    if let Some(max_id) = max_reachable_leaf_signal_id(island, reachable_signal_ids) {
        for id in 0..=max_id {
            if id > 0 {
                tokens.extend(quote! {
                    __rsjs_init_state_json.push(',');
                });
            }
            let signal = island.signals.iter().find(|sig| sig.id == SignalId(id));
            let Some(sig) = signal else {
                tokens.extend(quote! {
                    __rsjs_init_state_json.push_str("null");
                });
                continue;
            };
            if !reachable_signal_ids.contains(&sig.id) || sig.derived.is_some() {
                // Derived cells are never seeded: the client derives the
                // value from the seeded source signals. Dead or removed
                // signals keep a null slot only when needed for a higher
                // reachable SignalId.
                tokens.extend(quote! {
                    __rsjs_init_state_json.push_str("null");
                });
                continue;
            }
            if signal_initializer_for(signal_initializers, sig.id).is_some() {
                let local = signal_local_ident(sig.id);
                tokens.extend(quote! {
                    ::rsjs::ClientEncode::encode_into(&#local, &mut __rsjs_init_state_json);
                });
            } else {
                let mut json = String::new();
                push_value_as_json(&mut json, &sig.init);
                let chunk = syn::LitStr::new(&json, proc_macro2::Span::call_site());
                tokens.extend(quote! {
                    __rsjs_init_state_json.push_str(#chunk);
                });
            }
        }
    }

    let required_stores: HashSet<SharedStoreId> = island
        .signals
        .iter()
        .filter(|signal| reachable_signal_ids.contains(&signal.id))
        .filter_map(|signal| signal.shared.as_ref().map(|shared| shared.store))
        .collect();
    if required_stores.is_empty() {
        tokens.extend(quote! {
            __rsjs_init_state_json.push_str("]}");
            ::rsjs::__private::make_script_json_safe(&mut __rsjs_init_state_json);
        });
        return InitStateJsonTokens {
            let_tokens: tokens,
            raw_expr: quote! { __rsjs_init_state_json.as_str() },
        };
    }

    tokens.extend(quote! {
        __rsjs_init_state_json.push_str("],\"shared\":[");
    });

    if let Some(max_store) = required_stores.iter().map(|id| id.0).max() {
        for id in 0..=max_store {
            if id > 0 {
                tokens.extend(quote! { __rsjs_init_state_json.push(','); });
            }
            let store_id = SharedStoreId(id);
            if !required_stores.contains(&store_id) {
                tokens.extend(quote! { __rsjs_init_state_json.push_str("null"); });
                continue;
            }
            let init = shared_state_initializers
                .iter()
                .find(|init| init.id == store_id)
                .expect("reachable shared store must retain its Rust initializer");
            let ty = &init.ty;
            let scope = shared_scope_local_ident(store_id);
            tokens.extend(quote! {
                __rsjs_init_state_json.push_str("{\"scope_id\":");
                // Write the scope id digits straight into the seed buffer.
                // The page seed table serializes scope_id as a bare number,
                // and `u64::to_string()` here would heap-allocate a
                // temporary String per shared store per island render.
                // (`fmt::Write` for `String` never fails, so the result can
                // be ignored.)
                {
                    use ::core::fmt::Write as _;
                    let _ = ::core::write!(__rsjs_init_state_json, "{}", #scope);
                }
                __rsjs_init_state_json.push_str(",\"type_key\":");
                ::rsjs::ClientEncode::encode_into(
                    <#ty as ::rsjs::SharedState>::TYPE_KEY,
                    &mut __rsjs_init_state_json,
                );
                __rsjs_init_state_json.push_str(",\"schema_hash\":");
                ::rsjs::ClientEncode::encode_into(
                    <#ty as ::rsjs::SharedState>::schema_hash(),
                    &mut __rsjs_init_state_json,
                );
                __rsjs_init_state_json.push_str(",\"rust_type\":");
                ::rsjs::ClientEncode::encode_into(
                    <#ty as ::rsjs::SharedState>::META.rust_type,
                    &mut __rsjs_init_state_json,
                );
                __rsjs_init_state_json.push('}');
            });
        }
    }

    tokens.extend(quote! {
        __rsjs_init_state_json.push_str("]}");
        ::rsjs::__private::make_script_json_safe(&mut __rsjs_init_state_json);
    });

    InitStateJsonTokens {
        let_tokens: tokens,
        raw_expr: quote! { __rsjs_init_state_json.as_str() },
    }
}

fn signal_initializer_for(
    signal_initializers: &[SignalInitializer],
    id: SignalId,
) -> Option<&SignalInitializer> {
    signal_initializers.iter().find(|init| init.id == id)
}

fn emit_query_initial_value_locals(
    island: &Island,
    query_initializers: &[QueryInitializer],
    reachable_query_ids: &[QueryId],
) -> TokenStream {
    let mut out = TokenStream::new();
    for init in query_initializers {
        if !reachable_query_ids.contains(&init.id) {
            continue;
        }
        let typed_local = query_initial_typed_value_local_ident(init.id);
        let local = query_initial_value_local_ident(init.id);
        let expr = &init.expr;
        if query_initial_requires_typed_value(island, init.id) {
            out.extend(quote! {
                let #typed_local = #expr;
                let #local: ::rsjs::__private::serde_json::Value =
                    ::rsjs::__private::serde_json::to_value(&#typed_local)
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?;
            });
        } else {
            out.extend(quote! {
                let #local: ::rsjs::__private::serde_json::Value = {
                    let #typed_local = #expr;
                    ::rsjs::__private::serde_json::to_value(&#typed_local)
                        .map_err(|err| {
                            ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                        })?
                };
            });
        }
    }
    out
}

pub(crate) fn query_initial_typed_value_local_ident(id: QueryId) -> proc_macro2::Ident {
    format_ident!("__rsjs_initial_query_typed_{}", id.0)
}

fn query_initial_value_local_ident(id: QueryId) -> proc_macro2::Ident {
    format_ident!("__rsjs_initial_query_{}", id.0)
}

fn query_key_json_local_ident(id: QueryId) -> proc_macro2::Ident {
    format_ident!("__rsjs_query_key_json_{}", id.0)
}

fn mutation_json_local_ident(id: MutationId, field: &str) -> proc_macro2::Ident {
    format_ident!("__rsjs_mutation_{}_json_{}", field, id.0)
}

/// Emit the dehydrated query state JSON binding.
///
/// Shape: `{"<query_id>":{"key":<parts>?,"data":<value>,"data_updated_at":<ms>}, ...}`.
/// Literal-initial queries are still folded at macro-expansion time.
/// `query_with_initial(...)` queries serialize a server-side Rust expression at
/// render time through `rsjs::__private::to_script_json(...)`.
///
/// Anything the literal path does not know how to handle becomes `null`, which
/// the runtime treats as "no dehydrated state, fall through to the loading
/// path."
fn emit_init_query_state_json(
    island: &Island,
    query_initializers: &[QueryInitializer],
    reachable_query_ids: &[QueryId],
    context_ident: &syn::Ident,
) -> TokenStream {
    emit_lossless_init_query_state_json(
        island,
        query_initializers,
        reachable_query_ids,
        &HashSet::new(),
        context_ident,
    )
}

fn emit_lossless_init_query_state_json(
    island: &Island,
    query_initializers: &[QueryInitializer],
    reachable_query_ids: &[QueryId],
    prepared_query_ids: &HashSet<QueryId>,
    context_ident: &syn::Ident,
) -> TokenStream {
    use std::fmt::Write as _;
    if reachable_query_ids.is_empty() {
        return quote! {};
    }
    if !query_initializers.is_empty() {
        return emit_runtime_init_query_state_json(
            island,
            query_initializers,
            reachable_query_ids,
            prepared_query_ids,
            context_ident,
        );
    }
    let mut json = String::from("{");
    for (i, q) in island
        .queries
        .iter()
        .filter(|q| reachable_query_ids.contains(&q.id))
        .enumerate()
    {
        if i > 0 {
            json.push(',');
        }
        json.push('"');
        let _ = write!(json, "{id}\":{{\"data\":", id = q.id.0);
        let data = match island.exprs.client_expr(q.server_initial) {
            // The v0.1 stub: a literal string carrying pre-rendered
            // JSON. Splice it in verbatim (the user is responsible
            // for the string being valid JSON; the runtime falls
            // back to "no dehydrated state" on parse failure).
            // Reject HTML/JS hazards at proc-macro time: the blob
            // ships inside a <script> tag.
            ClientExpr::Literal(Value::String(s)) => {
                if let Some(reason) = scan_verbatim_json_for_hazards(s) {
                    return compile_error_tokens(reason);
                }
                s.clone()
            }
            // Other literal kinds get the standard JSON treatment so
            // a fixture that wants a non-string initial (e.g. a
            // bool query) still seeds correctly.
            ClientExpr::Literal(v) => {
                let mut tmp = String::new();
                push_value_as_json(&mut tmp, v);
                tmp
            }
            // Anything else: defer to the runtime's loading path.
            _ => "null".to_string(),
        };
        let _ = write!(json, "{data},\"data_updated_at\":0}}");
    }
    json.push('}');
    let json_lit = syn::LitStr::new(&json, proc_macro2::Span::call_site());
    quote! {
        let __rsjs_init_query_state_json: &'static str = #json_lit;
    }
}

fn emit_runtime_init_query_state_json(
    island: &Island,
    query_initializers: &[QueryInitializer],
    reachable_query_ids: &[QueryId],
    prepared_query_ids: &HashSet<QueryId>,
    context_ident: &syn::Ident,
) -> TokenStream {
    let mut tokens = quote! {
        let mut __rsjs_init_query_state_json = ::std::string::String::new();
        __rsjs_init_query_state_json.push('{');
    };

    for (i, q) in island
        .queries
        .iter()
        .filter(|q| reachable_query_ids.contains(&q.id))
        .enumerate()
    {
        let suffix = ",\"data_updated_at\":0}";
        if let Some(init) = query_initializer_for(query_initializers, q.id) {
            let entry_prefix = if i > 0 {
                format!(",\"{}\":{{", q.id.0)
            } else {
                format!("\"{}\":{{", q.id.0)
            };
            let local = query_initial_value_local_ident(init.id);
            tokens.extend(quote! {
                __rsjs_init_query_state_json.push_str(#entry_prefix);
            });
            if let Some(key_expr) = &init.key_expr {
                let key_setup = if prepared_query_ids.contains(&q.id) {
                    let key_json = query_key_json_local_ident(q.id);
                    quote! {
                        let __rsjs_query_key_json = #key_json.as_str();
                    }
                } else {
                    quote! {
                        let __rsjs_query_key_json_owned =
                            ::rsjs::__private::to_script_json(&(#key_expr))
                                .map_err(|err| {
                                    ::proa_core::WriteError::from(
                                        ::proa_core::LoadError::from(err),
                                    )
                                })?;
                        let __rsjs_query_key_json =
                            __rsjs_query_key_json_owned.as_str();
                    }
                };
                tokens.extend(quote! {
                    __rsjs_init_query_state_json.push_str("\"key\":");
                    #key_setup
                    __rsjs_init_query_state_json.push_str(__rsjs_query_key_json);
                    __rsjs_init_query_state_json.push('}');

                    let __rsjs_query_data_json =
                        ::rsjs::__private::to_script_json(&#local)
                            .map_err(|err| {
                                ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                            })?;

                    let mut __rsjs_query_seed_state_json =
                        ::std::string::String::from("{\"key\":");
                    __rsjs_query_seed_state_json.push_str(__rsjs_query_key_json);
                    __rsjs_query_seed_state_json.push_str(",\"data\":");
                    __rsjs_query_seed_state_json.push_str(&__rsjs_query_data_json);
                    __rsjs_query_seed_state_json.push_str(#suffix);
                    #context_ident.rsjs_query_seed_json(
                        __rsjs_query_key_json,
                        &__rsjs_query_seed_state_json,
                    );
                });
            } else {
                tokens.extend(quote! {
                    __rsjs_init_query_state_json.push_str("\"data\":");
                    let __rsjs_query_data_json =
                        ::rsjs::__private::to_script_json(&#local)
                            .map_err(|err| {
                                ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                            })?;
                    __rsjs_init_query_state_json.push_str(&__rsjs_query_data_json);
                    __rsjs_init_query_state_json.push_str(#suffix);
                });
            }
            continue;
        }

        let prefix = if i > 0 {
            format!(",\"{}\":{{\"data\":", q.id.0)
        } else {
            format!("\"{}\":{{\"data\":", q.id.0)
        };
        let data = match island.exprs.client_expr(q.server_initial) {
            ClientExpr::Literal(Value::String(s)) => {
                if let Some(reason) = scan_verbatim_json_for_hazards(s) {
                    return compile_error_tokens(reason);
                }
                s.clone()
            }
            ClientExpr::Literal(v) => {
                let mut tmp = String::new();
                push_value_as_json(&mut tmp, v);
                tmp
            }
            _ => "null".to_string(),
        };
        tokens.extend(quote! {
            __rsjs_init_query_state_json.push_str(#prefix);
            __rsjs_init_query_state_json.push_str(#data);
            __rsjs_init_query_state_json.push_str(#suffix);
        });
    }

    tokens.extend(quote! {
        __rsjs_init_query_state_json.push('}');
    });
    tokens
}

fn query_initializer_for(
    query_initializers: &[QueryInitializer],
    id: QueryId,
) -> Option<&QueryInitializer> {
    query_initializers.iter().find(|init| init.id == id)
}

/// Emit the dehydrated mutation registration JSON literal binding.
///
/// Shape: `{"<mutation_id>":{"key"?:<key>,"runner"?:<runner_config>,
/// "invalidates"?:[<key>...],"options":{"retry_count":<n>,
/// "on_success_invalidate_strategy":"refetch"|"reset"}}, ...}`.
///
/// Built by hand (no `serde_json` dep), one entry per
/// [`rsjs_ir::MutationDecl`]. Mutations have no server-side
/// dehydrated *result* state (they always boot in the `idle` state
/// on hydration); what the server emits is the *registration
/// metadata* the client runtime can use to wire each mutation's
/// runner, invalidation list, and policy. Generated island JS still
/// passes fallback values directly to `register_mutation`; the
/// runtime overlays only fields present in this blob. That lets
/// signal-backed mutation keys/invalidations keep using their
/// client-side expressions until broader server-local mutation
/// metadata exists.
///
/// **v0.1 runner stub.** As with M8 queries, the IR's `client_runner`
/// is an [`ExprId`] expected to resolve to a [`ClientExpr::Literal`] of
/// [`Value::String`]. Hand-built fixtures pass either a bare URL
/// ("/api/cart") or a JSON-encoded runner config
/// (`{"url":"/api/cart","method":"POST"}`); the runtime accepts both.
/// Anything else becomes `null` and the runtime treats the mutation
/// as a no-op (safe degradation; the rest of the page still renders).
fn emit_init_mutations_json(
    island: &Island,
    mutation_initializers: &[MutationInitializer],
    reachable_mutation_ids: &[MutationId],
) -> TokenStream {
    emit_lossless_init_mutations_json(
        island,
        mutation_initializers,
        reachable_mutation_ids,
        &HashSet::new(),
    )
}

fn emit_lossless_init_mutations_json(
    island: &Island,
    mutation_initializers: &[MutationInitializer],
    reachable_mutation_ids: &[MutationId],
    prepared_mutation_ids: &HashSet<MutationId>,
) -> TokenStream {
    use std::fmt::Write as _;
    if reachable_mutation_ids.is_empty() {
        return quote! {};
    }
    if !mutation_initializers.is_empty() {
        return emit_runtime_init_mutations_json(
            island,
            mutation_initializers,
            reachable_mutation_ids,
            prepared_mutation_ids,
        );
    }
    let mut json = String::from("{");
    for (i, m) in island
        .mutations
        .iter()
        .filter(|m| reachable_mutation_ids.contains(&m.id))
        .enumerate()
    {
        if i > 0 {
            json.push(',');
        }
        json.push('"');
        let _ = write!(json, "{id}\":{{", id = m.id.0);
        let mut field_count = 0_usize;

        // key: emitted only when every part is static-server-safe.
        // Signal-backed keys are left to the generated island JS.
        match static_query_key_json(island.exprs.client_expr(m.key_expr)) {
            Ok(Some(key_json)) => {
                push_json_object_field(&mut json, &mut field_count, "key", &key_json)
            }
            Ok(None) => {}
            Err(reason) => return compile_error_tokens(reason),
        }

        // runner: the client runner config the runtime parses into
        // method+url+body. JSON-encoded request descriptors are
        // spliced through verbatim; non-static runner expressions
        // fall back to the generated island JS.
        match static_expr_json(island.exprs.client_expr(m.client_runner), true) {
            Ok(Some(runner_json)) => {
                push_json_object_field(&mut json, &mut field_count, "runner", &runner_json);
            }
            Ok(None) => {}
            Err(reason) => return compile_error_tokens(reason),
        }

        // invalidates: array of structural keys, one per ExprId the
        // IR recorded. Omit the field if any target needs client-side
        // signal reads.
        match static_invalidation_list_json(island, &m.invalidates) {
            Ok(Some(invalidates_json)) => {
                push_json_object_field(
                    &mut json,
                    &mut field_count,
                    "invalidates",
                    &invalidates_json,
                );
            }
            Ok(None) => {}
            Err(reason) => return compile_error_tokens(reason),
        }

        // options.
        let options_json = mutation_options_json(&m.options);
        push_json_object_field(&mut json, &mut field_count, "options", &options_json);

        json.push('}');
    }
    json.push('}');
    let json_lit = syn::LitStr::new(&json, proc_macro2::Span::call_site());
    quote! {
        let __rsjs_init_mutations_json: &'static str = #json_lit;
    }
}

fn emit_runtime_init_mutations_json(
    island: &Island,
    mutation_initializers: &[MutationInitializer],
    reachable_mutation_ids: &[MutationId],
    prepared_mutation_ids: &HashSet<MutationId>,
) -> TokenStream {
    let mut tokens = quote! {
        let mut __rsjs_init_mutations_json = ::std::string::String::new();
        __rsjs_init_mutations_json.push('{');
    };

    for (i, m) in island
        .mutations
        .iter()
        .filter(|m| reachable_mutation_ids.contains(&m.id))
        .enumerate()
    {
        let entry_prefix = if i > 0 {
            format!(",\"{}\":{{", m.id.0)
        } else {
            format!("\"{}\":{{", m.id.0)
        };
        tokens.extend(quote! {
            __rsjs_init_mutations_json.push_str(#entry_prefix);
        });

        let mut field_count = 0_usize;
        if let Some(init) = mutation_initializer_for(mutation_initializers, m.id) {
            let prepared = prepared_mutation_ids
                .contains(&m.id)
                .then(|| mutation_json_local_ident(m.id, "key"));
            push_runtime_mutation_json_field(
                &mut tokens,
                &mut field_count,
                "key",
                &init.key_expr,
                prepared.as_ref(),
            );
        } else {
            match static_query_key_json(island.exprs.client_expr(m.key_expr)) {
                Ok(Some(key_json)) => {
                    push_static_mutation_json_field(
                        &mut tokens,
                        &mut field_count,
                        "key",
                        &key_json,
                    );
                }
                Ok(None) => {}
                Err(reason) => return compile_error_tokens(reason),
            }
        }

        match static_expr_json(island.exprs.client_expr(m.client_runner), true) {
            Ok(Some(runner_json)) => {
                push_static_mutation_json_field(
                    &mut tokens,
                    &mut field_count,
                    "runner",
                    &runner_json,
                );
            }
            Ok(None) => {}
            Err(reason) => return compile_error_tokens(reason),
        }

        if let Some(invalidates_expr) = mutation_initializer_for(mutation_initializers, m.id)
            .and_then(|init| init.invalidates_expr.as_ref())
        {
            push_runtime_mutation_json_field(
                &mut tokens,
                &mut field_count,
                "invalidates",
                invalidates_expr,
                prepared_mutation_ids
                    .contains(&m.id)
                    .then(|| mutation_json_local_ident(m.id, "invalidates"))
                    .as_ref(),
            );
        } else {
            match static_invalidation_list_json(island, &m.invalidates) {
                Ok(Some(invalidates_json)) => {
                    push_static_mutation_json_field(
                        &mut tokens,
                        &mut field_count,
                        "invalidates",
                        &invalidates_json,
                    );
                }
                Ok(None) => {}
                Err(reason) => return compile_error_tokens(reason),
            }
        }

        let options_json = mutation_options_json(&m.options);
        push_static_mutation_json_field(&mut tokens, &mut field_count, "options", &options_json);
        tokens.extend(quote! {
            __rsjs_init_mutations_json.push('}');
        });
    }

    tokens.extend(quote! {
        __rsjs_init_mutations_json.push('}');
    });
    tokens
}

fn mutation_initializer_for(
    mutation_initializers: &[MutationInitializer],
    id: MutationId,
) -> Option<&MutationInitializer> {
    mutation_initializers.iter().find(|init| init.id == id)
}

fn push_static_mutation_json_field(
    tokens: &mut TokenStream,
    field_count: &mut usize,
    name: &str,
    value_json: &str,
) {
    let prefix = if *field_count > 0 {
        format!(",\"{name}\":")
    } else {
        format!("\"{name}\":")
    };
    tokens.extend(quote! {
        __rsjs_init_mutations_json.push_str(#prefix);
        __rsjs_init_mutations_json.push_str(#value_json);
    });
    *field_count += 1;
}

fn push_runtime_mutation_json_field(
    tokens: &mut TokenStream,
    field_count: &mut usize,
    name: &str,
    expr: &syn::Expr,
    prepared_json: Option<&syn::Ident>,
) {
    let prefix = if *field_count > 0 {
        format!(",\"{name}\":")
    } else {
        format!("\"{name}\":")
    };
    if let Some(prepared_json) = prepared_json {
        tokens.extend(quote! {
            __rsjs_init_mutations_json.push_str(#prefix);
            __rsjs_init_mutations_json.push_str(&#prepared_json);
        });
    } else {
        let json_local = format_ident!("__rsjs_mutation_{}_json", name);
        tokens.extend(quote! {
            __rsjs_init_mutations_json.push_str(#prefix);
            let #json_local =
                ::rsjs::__private::to_script_json(&(#expr))
                    .map_err(|err| {
                        ::proa_core::WriteError::from(::proa_core::LoadError::from(err))
                    })?;
            __rsjs_init_mutations_json.push_str(&#json_local);
        });
    }
    *field_count += 1;
}

fn push_json_object_field(out: &mut String, field_count: &mut usize, name: &str, value_json: &str) {
    use std::fmt::Write as _;
    if *field_count > 0 {
        out.push(',');
    }
    out.push('"');
    let _ = write!(out, "{name}\":{value_json}");
    *field_count += 1;
}

fn mutation_options_json(options: &rsjs_ir::MutationOptions) -> String {
    use std::fmt::Write as _;
    let mut json = String::from("{\"retry_count\":");
    let _ = write!(
        json,
        "{retry_count},\"on_success_invalidate_strategy\":\"{strategy}\"}}",
        retry_count = options.retry_count,
        strategy = match options.on_success_invalidate_strategy {
            rsjs_ir::InvalidationStrategy::Refetch => "refetch",
            rsjs_ir::InvalidationStrategy::Reset => "reset",
            // `InvalidationStrategy` is `#[non_exhaustive]`; default to
            // the v0.1 `refetch` semantics for any future variant the
            // emitter has not been taught yet.
            _ => "refetch",
        },
    );
    json
}

fn static_invalidation_list_json(
    island: &Island,
    invalidates: &[ExprId],
) -> Result<Option<String>, &'static str> {
    let mut json = String::from("[");
    for (i, inv_id) in invalidates.iter().enumerate() {
        let Some(key_json) = static_query_key_json(island.exprs.client_expr(*inv_id))? else {
            return Ok(None);
        };
        if i > 0 {
            json.push(',');
        }
        json.push_str(&key_json);
    }
    json.push(']');
    Ok(Some(json))
}

fn static_query_key_json(expr: &ClientExpr) -> Result<Option<String>, &'static str> {
    match expr {
        ClientExpr::QueryKey(parts) => {
            let mut json = String::from("[");
            for (i, part) in parts.iter().enumerate() {
                let Some(part_json) = static_expr_json(part, false)? else {
                    return Ok(None);
                };
                if i > 0 {
                    json.push(',');
                }
                json.push_str(&part_json);
            }
            json.push(']');
            Ok(Some(json))
        }
        other => {
            let Some(value_json) = static_expr_json(other, false)? else {
                return Ok(None);
            };
            Ok(Some(format!("[{value_json}]")))
        }
    }
}

fn static_expr_json(
    expr: &ClientExpr,
    allow_verbatim_json_strings: bool,
) -> Result<Option<String>, &'static str> {
    let mut json = String::new();
    if push_static_expr_as_json_value(&mut json, expr, allow_verbatim_json_strings)? {
        Ok(Some(json))
    } else {
        Ok(None)
    }
}

fn push_static_expr_as_json_value(
    out: &mut String,
    expr: &ClientExpr,
    allow_verbatim_json_strings: bool,
) -> Result<bool, &'static str> {
    match expr {
        ClientExpr::Literal(Value::String(s)) if allow_verbatim_json_strings => {
            let trimmed = s.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if let Some(reason) = scan_verbatim_json_for_hazards(s) {
                    return Err(reason);
                }
                out.push_str(s);
            } else {
                push_json_string(out, s);
            }
            Ok(true)
        }
        ClientExpr::Literal(v) => {
            push_value_as_json(out, v);
            Ok(true)
        }
        ClientExpr::QueryKey(parts) => {
            let mut json = String::from("[");
            for (i, part) in parts.iter().enumerate() {
                let Some(part_json) = static_expr_json(part, false)? else {
                    return Ok(false);
                };
                if i > 0 {
                    json.push(',');
                }
                json.push_str(&part_json);
            }
            json.push(']');
            out.push_str(&json);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Append the JSON form of `value` to `out`. Strings are
/// HTML/JS-safely escaped; the encoded blob is shipped inside a
/// `<script type="application/json">` tag, so script-context hazards
/// (`<`, `>`, `&`, U+2028, U+2029) and control characters must escape.
fn push_value_as_json(out: &mut String, value: &Value) {
    match value {
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        // Wide-int seed: JSON string, the ClientEncode wire
        // convention for the i64 family. The runtime's
        // signal_initial_wide decodes it back to BigInt; a bare JSON
        // number would round past 2^53 in JSON.parse.
        Value::BigInt(i) => {
            out.push('"');
            out.push_str(&i.to_string());
            out.push('"');
        }
        Value::Float(f) => out.push_str(&f.to_string()),
        Value::String(s) => push_json_string(out, s),
        Value::Null => out.push_str("null"),
        // `Value` is `#[non_exhaustive]`; future variants serialise to
        // JSON `null` until the emitter learns them.
        _ => out.push_str("null"),
    }
}

/// Append `s` as a JSON-encoded, HTML/JS-context-safe string literal
/// to `out` (including the surrounding quotes).
///
/// Escape table mirrors `rsjs::client_codec::write_json_string`:
///
/// - `"` -> `\"`, `\` -> `\\`
/// - `\n` -> `\n`, `\r` -> `\r`, `\t` -> `\t`, `\b` -> `\b`, `\f` -> `\f`
/// - `<` / `>` / `&` -> `<` / `>` / `&` (script-tag XSS)
/// - U+2028 / U+2029 -> ` ` / ` ` (JS line-terminator hazard)
/// - Other control chars (`< 0x20`) -> `\u00XX`
///
/// Output is valid JSON and, separately, safe to embed verbatim in a
/// `<script type="application/json">` block.
fn push_json_string(out: &mut String, s: &str) {
    use std::fmt::Write as _;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Build a `compile_error!(<msg>)` token stream so the proc-macro
/// front-end surfaces a JSON-hazard rejection as a Rust diagnostic.
fn compile_error_tokens(msg: &str) -> TokenStream {
    let lit = syn::LitStr::new(msg, proc_macro2::Span::call_site());
    quote! { ::core::compile_error!(#lit); }
}

/// Scan a verbatim-spliced JSON literal for HTML/JS hazards. Returns
/// a human-readable reason on rejection, `None` on accept.
///
/// Used by compile-time query and mutation seed emission, whose literal
/// JSON paths splice the user-supplied
/// string verbatim into a `<script type="application/json">` block,
/// so any unescaped `</`, `<!`, U+2028, or U+2029 in the literal would
/// allow tag-break or JS line-terminator hazards.
fn scan_verbatim_json_for_hazards(s: &str) -> Option<&'static str> {
    if s.contains("</") {
        return Some(
            "v0.1 of rsjs rejects verbatim-spliced JSON literals containing `</` \
             because the dehydrated state blob ships inside a <script> tag; \
             encode the value as a regular string literal instead",
        );
    }
    if s.contains("<!") {
        return Some(
            "v0.1 of rsjs rejects verbatim-spliced JSON literals containing `<!` \
             because the dehydrated state blob ships inside a <script> tag; \
             encode the value as a regular string literal instead",
        );
    }
    if s.contains('\u{2028}') || s.contains('\u{2029}') {
        return Some(
            "v0.1 of rsjs rejects verbatim-spliced JSON literals containing the \
             U+2028/U+2029 JS line-terminator characters; encode the value as a \
             regular string literal instead",
        );
    }
    None
}

/// Render a [`Value`] as a `(rust_type, rust_literal)` pair so the
/// emitted local has the right type for `WebRenderSync`'s blanket
/// numeric / string impls.
fn render_value_as_rust(value: &Value) -> (TokenStream, TokenStream) {
    match value {
        Value::Bool(b) => (quote! { bool }, quote! { #b }),
        Value::Int(i) => {
            let lit = proc_macro2::Literal::i64_suffixed(*i);
            (quote! { i64 }, quote! { #lit })
        }
        // The carrier is i128 so it can represent the full u64 domain.
        // Domain-specific signal emission narrows this to the proven source
        // type; this generic literal path keeps the value lossless.
        Value::BigInt(i) => {
            let lit = proc_macro2::Literal::i128_suffixed(*i);
            (quote! { i128 }, quote! { #lit })
        }
        Value::Float(f) => {
            let lit = proc_macro2::Literal::f64_suffixed(*f);
            (quote! { f64 }, quote! { #lit })
        }
        Value::String(s) => {
            let lit = syn::LitStr::new(s, proc_macro2::Span::call_site());
            (quote! { &'static str }, quote! { #lit })
        }
        Value::Null => (
            quote! { ::core::option::Option<()> },
            quote! { ::core::option::Option::<()>::None },
        ),
        // `Value` is `#[non_exhaustive]`; treat any unknown future
        // variant as a typed `None` so the emitted impl still
        // type-checks.
        _ => (
            quote! { ::core::option::Option<()> },
            quote! { ::core::option::Option::<()>::None },
        ),
    }
}

fn render_integer_value_as_rust(
    domain: rsjs_ir::IntegerDomain,
    value: &Value,
) -> Option<(TokenStream, TokenStream)> {
    let integer = match value {
        Value::Int(value) => i128::from(*value),
        Value::BigInt(value) => *value,
        _ => return None,
    };
    Some(match domain {
        rsjs_ir::IntegerDomain::I32 => {
            let value = i32::try_from(integer).ok()?;
            let lit = proc_macro2::Literal::i32_suffixed(value);
            (quote! { i32 }, quote! { #lit })
        }
        rsjs_ir::IntegerDomain::U32 => {
            let value = u32::try_from(integer).ok()?;
            let lit = proc_macro2::Literal::u32_suffixed(value);
            (quote! { u32 }, quote! { #lit })
        }
        rsjs_ir::IntegerDomain::I64 => {
            let value = i64::try_from(integer).ok()?;
            let lit = proc_macro2::Literal::i64_suffixed(value);
            (quote! { i64 }, quote! { #lit })
        }
        rsjs_ir::IntegerDomain::U64 => {
            let value = u64::try_from(integer).ok()?;
            let lit = proc_macro2::Literal::u64_suffixed(value);
            (quote! { u64 }, quote! { #lit })
        }
    })
}

/// Per-node metadata describing membership in a conditional branch.
///
/// Built once at the top of `emit_rust` by walking
/// `island.bindings.conditional_sections`. For each branch element,
/// we record:
///
/// - the marker attribute (`data-rsjs-cond-then="<parent>:<idx>"` or
///   `data-rsjs-cond-else="..."`); and
/// - whether the predicate evaluated truthy on the server given the
///   signals' literal initial values. The branch is initially shown
///   when its side matches the predicate's truth value; otherwise we
///   inject a `style="display:none"` so the SSR HTML matches what
///   the client runtime will produce on first dep evaluation.
///
/// Predicates we cannot const-fold default to "show both branches"
/// (no inline style); JS will hide the wrong one on the first dep
/// firing. M4 only needs to handle the ConditionalCounter shape; M5+
/// can promote `evaluate_const` to a more thorough evaluator.
struct CondBranchInfo {
    marker_name: TokenStream,
    marker_value: syn::LitStr,
    style_attr: Option<TokenStream>,
}

fn component_fragment_marker_aliases(island: &Island) -> HashMap<NodeId, NodeId> {
    let mut aliases = HashMap::new();
    for mount in &island.component_fragment_mounts {
        for alias in &mount.node_aliases {
            aliases.insert(alias.parent, alias.child);
        }
    }
    aliases
}

fn marker_id_for(node: NodeId, fragment_marker_aliases: &HashMap<NodeId, NodeId>) -> NodeId {
    fragment_marker_aliases.get(&node).copied().unwrap_or(node)
}

/// Build the per-node conditional-branch metadata table.
fn compute_conditional_branch_metadata(
    island: &Island,
    signal_initializers: &[SignalInitializer],
    query_initializers: &[QueryInitializer],
    fragment_marker_aliases: &HashMap<NodeId, NodeId>,
) -> HashMap<NodeId, CondBranchInfo> {
    let mut out: HashMap<NodeId, CondBranchInfo> = HashMap::new();
    for (parent, sections) in &island.bindings.conditional_sections {
        let mut then_offset = 0usize;
        let mut else_offset = 0usize;
        for section in sections {
            // Try to const-fold the predicate against signal initial
            // values. None means "we can't decide; default to show."
            let predicate_expr = island.exprs.client_expr(section.condition);
            let predicate_initial = evaluate_const(predicate_expr, island, signal_initializers)
                .and_then(|v| match v {
                    Value::Bool(b) => Some(b),
                    _ => None,
                });
            let predicate_runtime = predicate_initial
                .is_none()
                .then(|| emit_expr_as_bool_rust(predicate_expr, query_initializers))
                .flatten();

            for (idx, child) in section.then_children.iter().enumerate() {
                let marker_parent = marker_id_for(*parent, fragment_marker_aliases);
                let attr_value = format!("{}:{}", marker_parent.0, then_offset + idx);
                let marker_value = syn::LitStr::new(&attr_value, proc_macro2::Span::call_site());
                let marker_name = quote! { data-rsjs-cond-then };
                // `then` is visible when the predicate is true. If we
                // couldn't fold the predicate, default to visible.
                let style_attr =
                    conditional_style_attr(predicate_initial, predicate_runtime.as_ref(), true);
                out.insert(
                    *child,
                    CondBranchInfo {
                        marker_name,
                        marker_value,
                        style_attr,
                    },
                );
            }
            for (idx, child) in section.else_children.iter().enumerate() {
                let marker_parent = marker_id_for(*parent, fragment_marker_aliases);
                let attr_value = format!("{}:{}", marker_parent.0, else_offset + idx);
                let marker_value = syn::LitStr::new(&attr_value, proc_macro2::Span::call_site());
                let marker_name = quote! { data-rsjs-cond-else };
                // `else` is visible when the predicate is false. If we
                // couldn't fold, default to visible (the JS will hide
                // whichever branch is wrong on first run).
                let style_attr =
                    conditional_style_attr(predicate_initial, predicate_runtime.as_ref(), false);
                out.insert(
                    *child,
                    CondBranchInfo {
                        marker_name,
                        marker_value,
                        style_attr,
                    },
                );
            }
            then_offset += section.then_children.len();
            else_offset += section.else_children.len();
        }
    }
    out
}

fn conditional_style_attr(
    predicate_initial: Option<bool>,
    predicate_runtime: Option<&TokenStream>,
    then_branch: bool,
) -> Option<TokenStream> {
    match predicate_initial {
        Some(value) => {
            let visible = if then_branch { value } else { !value };
            if visible {
                None
            } else {
                let lit = syn::LitStr::new("display:none", proc_macro2::Span::call_site());
                Some(quote! { style=#lit })
            }
        }
        None => predicate_runtime.map(|predicate| {
            if then_branch {
                quote! { style={ ::proa_core::style_ok(if #predicate { "" } else { "display:none" }) } }
            } else {
                quote! { style={ ::proa_core::style_ok(if #predicate { "display:none" } else { "" }) } }
            }
        }),
    }
}

/// Const-fold a predicate against signals' literal initial values.
///
/// Handles the v0.1 ClientExpr surface to the extent that the M4
/// ConditionalCounter fixture needs: literals, signal reads (which
/// resolve to the signal's `init` value), and a `BinaryOp` over those.
/// Returns `None` for anything we can't reduce (e.g. a `Call`,
/// a `Binary` over non-numeric/non-boolean operands, an arena cycle).
fn evaluate_const(
    expr: &ClientExpr,
    island: &Island,
    signal_initializers: &[SignalInitializer],
) -> Option<Value> {
    match expr {
        ClientExpr::Literal(v) => Some(v.clone()),
        ClientExpr::SignalRead(id) => {
            if signal_initializer_for(signal_initializers, *id).is_some() {
                None
            } else {
                let decl = island.signals.iter().find(|s| s.id == *id)?;
                match decl.derived {
                    // Memo cell: fold through its body expression.
                    // Cycles cannot occur (a derived body can only read
                    // cells declared before it).
                    Some(body) => {
                        evaluate_const(island.exprs.client_expr(body), island, signal_initializers)
                    }
                    None => Some(decl.init.clone()),
                }
            }
        }
        ClientExpr::EventAttribute { .. }
        | ClientExpr::BrowserRead(_)
        | ClientExpr::EventCurrentTarget
        | ClientExpr::DomRead { .. }
        | ClientExpr::NodeRefRead { .. }
        | ClientExpr::EventTargetWithinNodeRef { .. } => None,
        ClientExpr::Binary { op, left, right } => {
            let lhs = evaluate_const(left, island, signal_initializers)?;
            let rhs = evaluate_const(right, island, signal_initializers)?;
            apply_binop(*op, &lhs, &rhs)
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => match evaluate_const(condition, island, signal_initializers)? {
            Value::Bool(true) => evaluate_const(then_expr, island, signal_initializers),
            Value::Bool(false) => evaluate_const(else_expr, island, signal_initializers),
            _ => None,
        },
        ClientExpr::IsNonEmpty { target } => {
            let target = evaluate_const(target, island, signal_initializers)?;
            Some(Value::Bool(value_is_non_empty(&target)))
        }
        ClientExpr::StringToLowercase { target } => {
            let target = evaluate_const(target, island, signal_initializers)?;
            Some(Value::String(value_to_string(&target).to_lowercase()))
        }
        ClientExpr::StringContains { haystack, needle } => {
            let haystack = evaluate_const(haystack, island, signal_initializers)?;
            let needle = evaluate_const(needle, island, signal_initializers)?;
            Some(Value::Bool(
                value_to_string(&haystack).contains(value_to_string(&needle).as_str()),
            ))
        }
        ClientExpr::StringTransform { op, target } => {
            let target = evaluate_const(target, island, signal_initializers)?;
            let target = value_to_string(&target);
            Some(Value::String(match op {
                StringTransformOp::ToUppercase => target.to_uppercase(),
                StringTransformOp::Trim => target.trim().to_string(),
                _ => return None,
            }))
        }
        ClientExpr::StringPredicate {
            op,
            haystack,
            needle,
        } => {
            let haystack = evaluate_const(haystack, island, signal_initializers)?;
            let needle = evaluate_const(needle, island, signal_initializers)?;
            let haystack = value_to_string(&haystack);
            let needle = value_to_string(&needle);
            Some(Value::Bool(match op {
                StringPredicateOp::StartsWith => haystack.starts_with(needle.as_str()),
                StringPredicateOp::EndsWith => haystack.ends_with(needle.as_str()),
                _ => return None,
            }))
        }
        ClientExpr::Encoding { .. } => None,
        // Calls are opaque to the const folder: M7e calls the user-
        // provided helper at runtime; we don't have its body lowered
        // into the IR side-table at this layer.
        ClientExpr::Call { .. } => None,
        // M8 query-projection variants. The const folder cannot see
        // through a `QueryRead` (the cache value is runtime data).
        // Projection / predicate / coalesce wrappers similarly
        // cannot fold without runtime data; they default to "unknown"
        // and the conditional emitter falls back to "show" for the
        // SSR pass, letting JS reconcile on first dep firing.
        ClientExpr::QueryRead { .. }
        | ClientExpr::Project { .. }
        | ClientExpr::AsType { .. }
        | ClientExpr::IsPresent { .. }
        | ClientExpr::Eq { .. }
        | ClientExpr::Neq { .. }
        | ClientExpr::Coalesce { .. } => None,
        // M9 mutation read: opaque to the const folder, same property
        // as `QueryRead` (the cache value is runtime data; mutations
        // boot in the `idle` state and never have a server-side
        // dehydrated initial value).
        ClientExpr::MutationRead { .. } => None,
        // `ClientExpr` is `#[non_exhaustive]`; future variants are
        // opaque to the v0.1 const folder.
        _ => None,
    }
}

/// Mirror the JS-side non-emptiness check at compile time.
///
/// Strings: non-empty iff `.len() > 0`. Other variants: empty (the
/// JS-side helper returns `false` for booleans/numbers/null too, so
/// this matches). Used for initial-render predicate const-folding so
/// the SSR HTML for a conditional gated on `username.is_non_empty()`
/// matches what the client runtime would produce on first dep firing.
fn value_is_non_empty(v: &Value) -> bool {
    match v {
        Value::String(s) => !s.is_empty(),
        _ => false,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Null => String::new(),
        _ => String::new(),
    }
}

fn emit_expr_as_rust(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> Option<TokenStream> {
    match emit_server_expr(expr, query_initializers)? {
        ServerExpr::Rust(tokens) => Some(tokens),
        ServerExpr::Json(tokens) => Some(quote! { ::rsjs::__private::json_text(&(#tokens)) }),
    }
}

fn emit_expr_as_bool_rust(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> Option<TokenStream> {
    match emit_server_expr(expr, query_initializers)? {
        ServerExpr::Rust(tokens) => Some(tokens),
        ServerExpr::Json(tokens) => Some(quote! { ::rsjs::__private::json_as_bool(&(#tokens)) }),
    }
}

fn emit_server_expr(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> Option<ServerExpr> {
    match expr {
        ClientExpr::Literal(value) => {
            let (_, lit) = render_value_as_rust(value);
            Some(ServerExpr::Rust(quote! { #lit }))
        }
        ClientExpr::SignalRead(sig) => {
            let local = signal_local_ident(*sig);
            Some(ServerExpr::Rust(quote! { #local }))
        }
        ClientExpr::BrowserRead(read) => Some(ServerExpr::Rust(emit_browser_read_as_rust(
            read,
            query_initializers,
        ))),
        ClientExpr::NodePayload { tokens, .. } => {
            let parsed = tokens.parse::<TokenStream>().ok()?;
            Some(ServerExpr::Rust(quote! { #parsed }))
        }
        ClientExpr::Binary { op, left, right } => {
            if let ClientBinaryOp::Checked { op, domain } = op {
                let lhs = emit_checked_integer_operand_as_rust(left, *domain, query_initializers)?;
                let rhs = emit_checked_integer_operand_as_rust(right, *domain, query_initializers)?;
                let method = match op {
                    rsjs_ir::CheckedIntegerOp::Add => format_ident!("checked_add"),
                    rsjs_ir::CheckedIntegerOp::Sub => format_ident!("checked_sub"),
                    rsjs_ir::CheckedIntegerOp::Mul => format_ident!("checked_mul"),
                    rsjs_ir::CheckedIntegerOp::Div => format_ident!("checked_div"),
                    rsjs_ir::CheckedIntegerOp::Rem => format_ident!("checked_rem"),
                };
                let message = syn::LitStr::new(
                    &format!(
                        "rsjs checked {} {} failed",
                        domain.as_str(),
                        match op {
                            rsjs_ir::CheckedIntegerOp::Add => "addition",
                            rsjs_ir::CheckedIntegerOp::Sub => "subtraction",
                            rsjs_ir::CheckedIntegerOp::Mul => "multiplication",
                            rsjs_ir::CheckedIntegerOp::Div => "division",
                            rsjs_ir::CheckedIntegerOp::Rem => "remainder",
                        }
                    ),
                    proc_macro2::Span::call_site(),
                );
                return Some(ServerExpr::Rust(
                    quote! { (#lhs).#method(#rhs).expect(#message) },
                ));
            }
            let bool_context = matches!(op, ClientBinaryOp::And | ClientBinaryOp::Or)
                || (matches!(op, ClientBinaryOp::Eq | ClientBinaryOp::Ne)
                    && (matches!(left.as_ref(), ClientExpr::Literal(Value::Bool(_)))
                        || matches!(right.as_ref(), ClientExpr::Literal(Value::Bool(_)))));
            // A numeric literal meeting a runtime expression must stay
            // unsuffixed. For a shared-state read the local's Rust type is
            // whatever the state struct declared, and for a checked integer
            // expression the operation already pins the exact source domain.
            // A generic `Value::BigInt` literal is otherwise rendered as
            // `i128`, which would make a valid `i64_checked_expr < 23_i64`
            // replay fail to type-check. Let rustc infer the literal's width
            // from its non-literal peer.
            let unsuffixed = op.dyn_helper_js().is_some()
                || matches!(left.as_ref(), ClientExpr::SignalRead(_))
                || matches!(right.as_ref(), ClientExpr::SignalRead(_))
                || !matches!(left.as_ref(), ClientExpr::Literal(_))
                || !matches!(right.as_ref(), ClientExpr::Literal(_));
            let lhs = if bool_context {
                emit_expr_as_bool_rust(left, query_initializers)?
            } else {
                emit_numeric_operand_as_rust(left, query_initializers, unsuffixed)?
            };
            let rhs = if bool_context {
                emit_expr_as_bool_rust(right, query_initializers)?
            } else {
                emit_numeric_operand_as_rust(right, query_initializers, unsuffixed)?
            };
            let op = bin_op_rust(*op);
            Some(ServerExpr::Rust(quote! { (#lhs #op #rhs) }))
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let condition = emit_expr_as_bool_rust(condition, query_initializers)?;
            let then_expr = emit_expr_as_rust(then_expr, query_initializers)?;
            let else_expr = emit_expr_as_rust(else_expr, query_initializers)?;
            Some(ServerExpr::Rust(
                quote! { if (#condition) { #then_expr } else { #else_expr } },
            ))
        }
        ClientExpr::Unary { op, expr } => match op {
            ClientUnaryOp::Not => {
                let expr = emit_expr_as_bool_rust(expr, query_initializers)?;
                Some(ServerExpr::Rust(quote! { !(#expr) }))
            }
            ClientUnaryOp::CheckedNeg(domain) => {
                let expr = emit_checked_integer_operand_as_rust(expr, *domain, query_initializers)?;
                let message = syn::LitStr::new(
                    &format!("rsjs checked {} negation failed", domain.as_str()),
                    proc_macro2::Span::call_site(),
                );
                Some(ServerExpr::Rust(
                    quote! { (#expr).checked_neg().expect(#message) },
                ))
            }
            ClientUnaryOp::Neg => {
                let expr = emit_expr_as_rust(expr, query_initializers)?;
                let op = unary_op_rust(*op);
                Some(ServerExpr::Rust(quote! { (#op #expr) }))
            }
        },
        ClientExpr::IsNonEmpty { target } => {
            let target = emit_expr_as_rust(target, query_initializers)?;
            Some(ServerExpr::Rust(quote! { !(#target).is_empty() }))
        }
        ClientExpr::StringToLowercase { target } => {
            let target = emit_expr_as_rust(target, query_initializers)?;
            Some(ServerExpr::Rust(quote! { (#target).to_lowercase() }))
        }
        ClientExpr::StringContains { haystack, needle } => {
            let haystack = emit_expr_as_rust(haystack, query_initializers)?;
            let needle = emit_expr_as_rust(needle, query_initializers)?;
            Some(ServerExpr::Rust(quote! {{
                let __rsjs_needle = #needle;
                (#haystack).contains(::core::convert::AsRef::<str>::as_ref(&__rsjs_needle))
            }}))
        }
        ClientExpr::StringTransform { op, target } => {
            let target = emit_expr_as_rust(target, query_initializers)?;
            let tokens = match op {
                StringTransformOp::ToUppercase => quote! { (#target).to_uppercase() },
                // Rust `trim` returns a borrowed `&str`; convert to an
                // owned `String` so the transform yields the same owned
                // type as `to_lowercase` / `to_uppercase` regardless of
                // how the surrounding slot consumes it. The allocation is
                // required by that ownership boundary, so mark it as reviewed
                // when the generated expression re-enters `html_sync!`.
                StringTransformOp::Trim => {
                    quote! { ::proa_core::perf_ok((#target).trim().to_string()) }
                }
                _ => return None,
            };
            Some(ServerExpr::Rust(tokens))
        }
        ClientExpr::StringPredicate {
            op,
            haystack,
            needle,
        } => {
            let haystack = emit_expr_as_rust(haystack, query_initializers)?;
            let needle = emit_expr_as_rust(needle, query_initializers)?;
            let method = match op {
                StringPredicateOp::StartsWith => quote! { starts_with },
                StringPredicateOp::EndsWith => quote! { ends_with },
                _ => return None,
            };
            Some(ServerExpr::Rust(quote! {{
                let __rsjs_needle = #needle;
                (#haystack).#method(::core::convert::AsRef::<str>::as_ref(&__rsjs_needle))
            }}))
        }
        ClientExpr::Encoding { op, value } => {
            let value = emit_expr_as_rust(value, query_initializers)?;
            Some(ServerExpr::Rust(emit_encoding_as_rust(*op, value)))
        }
        ClientExpr::Call { .. } => Some(ServerExpr::Rust(emit_client_call_as_rust(
            expr,
            query_initializers,
        ))),
        ClientExpr::QueryRead { id, accessor } => {
            emit_query_read_as_server_expr(*id, *accessor, query_initializers)
        }
        ClientExpr::MutationRead { accessor, .. } => emit_mutation_read_as_server_expr(*accessor),
        ClientExpr::Project { target, path } => {
            let target = emit_expr_as_json_value(target, query_initializers)
                .or_else(|| signal_read_as_json_value(target))?;
            let pointer = syn::LitStr::new(path.as_str(), proc_macro2::Span::call_site());
            Some(ServerExpr::Json(quote! {
                ::rsjs::__private::json_project(&(#target), #pointer)
            }))
        }
        ClientExpr::AsType { target, ty } => {
            if matches!(ty, AsType::F64FromInt) {
                let target = emit_expr_as_rust(target, query_initializers)?;
                return Some(ServerExpr::Rust(quote! {
                    ::core::primitive::f64::from(#target)
                }));
            }
            let target = emit_expr_as_json_value(target, query_initializers)?;
            Some(ServerExpr::Rust(emit_json_as_type(*ty, target)))
        }
        ClientExpr::IsPresent { target } => {
            let target = emit_expr_as_json_value(target, query_initializers)?;
            Some(ServerExpr::Rust(quote! {
                ::rsjs::__private::json_is_present(&(#target))
            }))
        }
        ClientExpr::Eq { target, value } => {
            let target = emit_expr_as_json_value(target, query_initializers)?;
            let value = json_value_tokens(value);
            Some(ServerExpr::Rust(quote! {
                ::rsjs::__private::json_eq_value(&(#target), &(#value))
            }))
        }
        ClientExpr::Neq { target, value } => {
            let target = emit_expr_as_json_value(target, query_initializers)?;
            let value = json_value_tokens(value);
            Some(ServerExpr::Rust(quote! {
                !::rsjs::__private::json_eq_value(&(#target), &(#value))
            }))
        }
        ClientExpr::Coalesce { target, fallback } => {
            if let Some(target) = emit_expr_as_json_value(target, query_initializers) {
                let fallback = json_value_tokens(fallback);
                Some(ServerExpr::Json(quote! {
                    ::rsjs::__private::json_coalesce(&(#target), #fallback)
                }))
            } else {
                let target = emit_expr_as_rust(target, query_initializers)?;
                Some(ServerExpr::Rust(target))
            }
        }
        _ => None,
    }
}

/// Lower one operand of a binary expression for the SSR replay.
///
/// With `unsuffixed`, an integer or float literal is emitted without a type
/// suffix so rustc infers its width from the other operand. That matters most
/// for shared-state reads: the shared local's Rust type is whatever the state
/// struct declared, and the macro cannot see it, so any suffix it picked would
/// be wrong for some state type.
fn emit_numeric_operand_as_rust(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
    unsuffixed: bool,
) -> Option<TokenStream> {
    if unsuffixed {
        if let ClientExpr::Literal(value) = expr {
            match value {
                Value::Int(int) => {
                    let lit = proc_macro2::Literal::i64_unsuffixed(*int);
                    return Some(quote! { #lit });
                }
                Value::BigInt(int) => {
                    let lit = proc_macro2::Literal::i128_unsuffixed(*int);
                    return Some(quote! { #lit });
                }
                Value::Float(float) => {
                    let lit = proc_macro2::Literal::f64_unsuffixed(*float);
                    return Some(quote! { #lit });
                }
                _ => {}
            }
        }
    }
    emit_expr_as_rust(expr, query_initializers)
}

fn emit_checked_integer_operand_as_rust(
    expr: &ClientExpr,
    domain: rsjs_ir::IntegerDomain,
    query_initializers: &[QueryInitializer],
) -> Option<TokenStream> {
    if let ClientExpr::Literal(value) = expr {
        let (_, literal) = render_integer_value_as_rust(domain, value)?;
        return Some(literal);
    }
    let expr = emit_expr_as_rust(expr, query_initializers)?;
    let ty = integer_domain_rust_type_tokens(domain);
    // Checked Client IR is also a source-type assertion. In particular,
    // `rsjs::div_int::<i32>(shared_u64.get(), ...)` must not replay on the
    // server as native u64 merely because the shared field entered Client IR
    // as `Dyn`. `identity::<T>` is zero-cost and makes rustc verify that every
    // nonliteral operand really has the IR's declared fixed-width domain.
    Some(quote! {
        ::core::convert::identity::<#ty>(#expr)
    })
}

fn integer_domain_rust_type_tokens(domain: rsjs_ir::IntegerDomain) -> TokenStream {
    match domain {
        rsjs_ir::IntegerDomain::I32 => quote! { ::core::primitive::i32 },
        rsjs_ir::IntegerDomain::U32 => quote! { ::core::primitive::u32 },
        rsjs_ir::IntegerDomain::I64 => quote! { ::core::primitive::i64 },
        rsjs_ir::IntegerDomain::U64 => quote! { ::core::primitive::u64 },
    }
}

fn emit_expr_as_json_value(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> Option<TokenStream> {
    match expr {
        ClientExpr::Literal(value) => Some(json_value_tokens(value)),
        ClientExpr::QueryRead {
            id,
            accessor: QueryAccessor::Data,
        } => {
            query_initializer_for(query_initializers, *id)?;
            let local = query_initial_value_local_ident(*id);
            Some(quote! { #local.clone() })
        }
        ClientExpr::QueryRead {
            id: _,
            accessor: QueryAccessor::Error,
        } => Some(quote! { ::rsjs::__private::serde_json::Value::Null }),
        ClientExpr::MutationRead {
            accessor: MutationAccessor::Data | MutationAccessor::Error,
            ..
        } => Some(quote! { ::rsjs::__private::serde_json::Value::Null }),
        ClientExpr::Project { target, path } => {
            let target = emit_expr_as_json_value(target, query_initializers)
                .or_else(|| signal_read_as_json_value(target))?;
            let pointer = syn::LitStr::new(path.as_str(), proc_macro2::Span::call_site());
            Some(quote! { ::rsjs::__private::json_project(&(#target), #pointer) })
        }
        ClientExpr::Coalesce { target, fallback } => {
            let target = emit_expr_as_json_value(target, query_initializers)?;
            let fallback = json_value_tokens(fallback);
            Some(quote! { ::rsjs::__private::json_coalesce(&(#target), #fallback) })
        }
        _ => None,
    }
}

/// Server-side JSON view of a signal a projection reads through.
///
/// `shared_field.get().label` and `shared_field.get()[0]` lower to
/// `Project { target: SignalRead(..), .. }`. The client projects the live
/// signal value; the server has the same value in the signal's initial-value
/// local (`__rsjs_initial_signal_<n>`), but as a Rust value rather than as
/// JSON. Re-encoding it through `ClientEncode` (the exact codec the client
/// hydrates from) gives the projection the same document to walk, so SSR and
/// the first client render agree instead of the server emitting an empty node.
///
/// Only projection bases go through here: every other server expression keeps
/// reading the Rust local directly, so this costs one buffer per projected
/// binding and nothing at all for islands without projections.
fn signal_read_as_json_value(expr: &ClientExpr) -> Option<TokenStream> {
    let ClientExpr::SignalRead(signal) = expr else {
        return None;
    };
    let local = signal_local_ident(*signal);
    Some(quote! {{
        let mut __rsjs_projection_json = ::std::string::String::new();
        ::rsjs::ClientEncode::encode_into(&#local, &mut __rsjs_projection_json);
        ::rsjs::__private::serde_json::from_str::<::rsjs::__private::serde_json::Value>(
            &__rsjs_projection_json,
        )
        .unwrap_or(::rsjs::__private::serde_json::Value::Null)
    }})
}

fn emit_mutation_read_as_server_expr(accessor: MutationAccessor) -> Option<ServerExpr> {
    let expr = match accessor {
        MutationAccessor::Status => quote! { ::proa_core::perf_ok("idle") },
        MutationAccessor::IsIdle => quote! { true },
        MutationAccessor::IsPending => quote! { false },
        MutationAccessor::IsSuccess => quote! { false },
        MutationAccessor::IsError => quote! { false },
        MutationAccessor::Data | MutationAccessor::Error => {
            return Some(ServerExpr::Json(quote! {
                ::rsjs::__private::serde_json::Value::Null
            }));
        }
        _ => return None,
    };
    Some(ServerExpr::Rust(expr))
}

fn emit_query_read_as_server_expr(
    id: QueryId,
    accessor: QueryAccessor,
    query_initializers: &[QueryInitializer],
) -> Option<ServerExpr> {
    query_initializer_for(query_initializers, id)?;
    let local = query_initial_value_local_ident(id);
    let expr = match accessor {
        QueryAccessor::Data => {
            return Some(ServerExpr::Json(quote! { #local.clone() }));
        }
        QueryAccessor::Status => quote! { ::proa_core::perf_ok("success") },
        QueryAccessor::IsLoading => quote! { false },
        QueryAccessor::IsSuccess => quote! { true },
        QueryAccessor::IsError => quote! { false },
        QueryAccessor::IsRefetching => quote! { false },
        QueryAccessor::IsStale => quote! { false },
        QueryAccessor::Error => {
            return Some(ServerExpr::Json(quote! {
                ::rsjs::__private::serde_json::Value::Null
            }));
        }
        QueryAccessor::DataUpdatedAt => quote! { 0.0_f64 },
        QueryAccessor::ErrorUpdatedAt => quote! { 0.0_f64 },
        QueryAccessor::FetchCount => quote! { 1_u32 },
        QueryAccessor::DownloadLoaded => quote! { 0.0_f64 },
        QueryAccessor::DownloadTotal => quote! { ::core::option::Option::<f64>::None },
        QueryAccessor::DownloadPercent => quote! { 0.0_f64 },
        _ => return None,
    };
    Some(ServerExpr::Rust(expr))
}

fn emit_json_as_type(ty: AsType, value: TokenStream) -> TokenStream {
    match ty {
        AsType::Str => quote! { ::rsjs::__private::json_as_str(&(#value)) },
        AsType::F64 => quote! { ::rsjs::__private::json_as_f64(&(#value)) },
        AsType::F64FromInt => quote! { ::core::primitive::f64::from(#value) },
        AsType::I64 => quote! { ::rsjs::__private::json_as_i64(&(#value)) },
        AsType::Bool => quote! { ::rsjs::__private::json_as_bool(&(#value)) },
        AsType::Array => quote! { ::rsjs::__private::json_as_array(&(#value)) },
        AsType::Object => quote! { ::rsjs::__private::json_as_object(&(#value)) },
        _ => quote! { ::rsjs::__private::json_text(&(#value)) },
    }
}

fn json_value_tokens(value: &Value) -> TokenStream {
    match value {
        Value::Bool(value) => quote! { ::rsjs::__private::serde_json::Value::Bool(#value) },
        Value::Int(value) => {
            let lit = proc_macro2::Literal::i64_suffixed(*value);
            quote! {
                ::rsjs::__private::serde_json::Value::Number(
                    ::rsjs::__private::serde_json::Number::from(#lit)
                )
            }
        }
        // Wide integers use the public client-codec string shape. A JSON
        // number cannot represent the full u64 domain losslessly.
        Value::BigInt(value) => {
            let value = syn::LitStr::new(&value.to_string(), proc_macro2::Span::call_site());
            quote! {
                ::rsjs::__private::serde_json::Value::String(
                    ::std::string::String::from(#value)
                )
            }
        }
        Value::Float(value) => {
            let lit = proc_macro2::Literal::f64_suffixed(*value);
            quote! {
                ::rsjs::__private::serde_json::Number::from_f64(#lit)
                    .map(::rsjs::__private::serde_json::Value::Number)
                    .unwrap_or(::rsjs::__private::serde_json::Value::Null)
            }
        }
        Value::String(value) => {
            let lit = syn::LitStr::new(value, proc_macro2::Span::call_site());
            quote! {
                ::rsjs::__private::serde_json::Value::String(
                    ::std::string::String::from(#lit)
                )
            }
        }
        Value::Null => quote! { ::rsjs::__private::serde_json::Value::Null },
        _ => quote! { ::rsjs::__private::serde_json::Value::Null },
    }
}

fn emit_encoding_as_rust(op: EncodingOp, value: TokenStream) -> TokenStream {
    let fn_name = match op {
        EncodingOp::Utf8Bytes => format_ident!("utf8_bytes"),
        EncodingOp::Utf8String => format_ident!("utf8_string"),
        EncodingOp::Base64Encode => format_ident!("base64_encode"),
        EncodingOp::Base64Decode => format_ident!("base64_decode"),
        EncodingOp::Base64UrlEncode => format_ident!("base64url_encode"),
        EncodingOp::Base64UrlDecode => format_ident!("base64url_decode"),
        _ => format_ident!("utf8_string"),
    };
    quote! { ::rsjs::encoding::#fn_name(#value) }
}

fn emit_browser_read_as_rust(
    read: &ClientBrowserRead,
    query_initializers: &[QueryInitializer],
) -> TokenStream {
    fn perf_ok(tokens: TokenStream) -> TokenStream {
        quote! { ::proa_core::perf_ok(#tokens) }
    }

    match read {
        ClientBrowserRead::Window(_) => perf_ok(quote! { 0 }),
        ClientBrowserRead::Location(_) => perf_ok(quote! { "" }),
        ClientBrowserRead::Storage {
            fallback: Some(fallback),
            ..
        } => perf_ok(
            emit_expr_as_rust(fallback, query_initializers).unwrap_or_else(|| quote! { "" }),
        ),
        ClientBrowserRead::Storage {
            value_type,
            fallback: None,
            ..
        } => {
            let fallback = match value_type {
                rsjs_ir::StorageValueType::String => quote! { "" },
                rsjs_ir::StorageValueType::Bool => quote! { false },
                rsjs_ir::StorageValueType::I64 => quote! { 0_i64 },
                rsjs_ir::StorageValueType::F64 => quote! { 0.0_f64 },
                _ => quote! { "" },
            };
            perf_ok(fallback)
        }
        ClientBrowserRead::UrlSearchParam {
            fallback: Some(fallback),
            ..
        } => perf_ok(
            emit_expr_as_rust(fallback, query_initializers).unwrap_or_else(|| quote! { "" }),
        ),
        ClientBrowserRead::UrlSearchParam { fallback: None, .. } => perf_ok(quote! { "" }),
        // Registry-declared reads SSR-render the type-appropriate
        // empty value; the JS template's own SSR guard returns the
        // same shape client-side before hydration.
        ClientBrowserRead::Declared { helper, .. } => {
            let fallback = match rsjs_ir::browser_registry::find_by_id(helper)
                .map(|spec| spec.value_type.clone())
            {
                Some(rsjs_ir::client::ClientType::Bool) => quote! { false },
                Some(
                    rsjs_ir::client::ClientType::I32
                    | rsjs_ir::client::ClientType::U32
                    | rsjs_ir::client::ClientType::I64
                    | rsjs_ir::client::ClientType::U64,
                ) => quote! { 0 },
                Some(rsjs_ir::client::ClientType::F32 | rsjs_ir::client::ClientType::F64) => {
                    quote! { 0.0 }
                }
                _ => quote! { "" },
            };
            perf_ok(fallback)
        }
        _ => perf_ok(quote! { "" }),
    }
}

fn bin_op_rust(op: ClientBinaryOp) -> TokenStream {
    match op {
        ClientBinaryOp::Add => quote! { + },
        ClientBinaryOp::Sub => quote! { - },
        ClientBinaryOp::Mul => quote! { * },
        ClientBinaryOp::Div => quote! { / },
        ClientBinaryOp::DivInt => quote! { / },
        ClientBinaryOp::Rem => quote! { % },
        ClientBinaryOp::Eq => quote! { == },
        ClientBinaryOp::Ne => quote! { != },
        ClientBinaryOp::Lt => quote! { < },
        ClientBinaryOp::Gt => quote! { > },
        ClientBinaryOp::Le => quote! { <= },
        ClientBinaryOp::Ge => quote! { >= },
        ClientBinaryOp::And => quote! { && },
        ClientBinaryOp::Or => quote! { || },
        ClientBinaryOp::Checked { .. } => {
            unreachable!("checked integer operations emit checked_* method calls")
        }
        // The `Dyn*` operators exist because the *client* cannot tell BigInt
        // from Number without the page seed. The server has real Rust types, so
        // the SSR replay is the ordinary operator and rustc type-checks it.
        ClientBinaryOp::DynAdd => quote! { + },
        ClientBinaryOp::DynSub => quote! { - },
        ClientBinaryOp::DynMul => quote! { * },
        ClientBinaryOp::DynDiv | ClientBinaryOp::DynDivInt => quote! { / },
        ClientBinaryOp::DynRem => quote! { % },
        ClientBinaryOp::DynEq => quote! { == },
        ClientBinaryOp::DynNe => quote! { != },
    }
}

fn unary_op_rust(op: ClientUnaryOp) -> TokenStream {
    match op {
        ClientUnaryOp::Not => quote! { ! },
        ClientUnaryOp::Neg => quote! { - },
        ClientUnaryOp::CheckedNeg(_) => {
            unreachable!("checked integer negation emits checked_neg")
        }
    }
}

/// Apply a [`ClientBinaryOp`] to two folded [`Value`]s. Returns `None` for
/// type mismatches that v0.1 considers ill-typed at the source
/// language level.
fn apply_binop(op: ClientBinaryOp, lhs: &Value, rhs: &Value) -> Option<Value> {
    use Value::{BigInt, Bool, Float, Int};
    match (op, lhs, rhs) {
        (ClientBinaryOp::Checked { op, domain }, lhs, rhs) => {
            apply_checked_integer_binop(op, domain, lhs, rhs)
        }
        (ClientBinaryOp::Add, Int(a), Int(b)) => Some(Int(a.wrapping_add(*b))),
        (ClientBinaryOp::Sub, Int(a), Int(b)) => Some(Int(a.wrapping_sub(*b))),
        (ClientBinaryOp::Mul, Int(a), Int(b)) => Some(Int(a.wrapping_mul(*b))),
        (ClientBinaryOp::Div, Int(a), Int(b)) if *b != 0 => Some(Int(a / b)),
        (ClientBinaryOp::DivInt, Int(a), Int(b)) if *b != 0 => Some(Int(a / b)),
        (ClientBinaryOp::Rem, Int(a), Int(b)) if *b != 0 => Some(Int(a % b)),
        // Wide ints fold with the same i64 arithmetic as Int; the
        // result stays BigInt so downstream emission keeps exact
        // client semantics. Lowering promotes literals so mixed
        // Int/BigInt arithmetic never reaches the folder.
        (ClientBinaryOp::Add, BigInt(a), BigInt(b)) => Some(BigInt(a.wrapping_add(*b))),
        (ClientBinaryOp::Sub, BigInt(a), BigInt(b)) => Some(BigInt(a.wrapping_sub(*b))),
        (ClientBinaryOp::Mul, BigInt(a), BigInt(b)) => Some(BigInt(a.wrapping_mul(*b))),
        (ClientBinaryOp::Div, BigInt(a), BigInt(b)) if *b != 0 => Some(BigInt(a / b)),
        (ClientBinaryOp::DivInt, BigInt(a), BigInt(b)) if *b != 0 => Some(BigInt(a / b)),
        (ClientBinaryOp::Rem, BigInt(a), BigInt(b)) if *b != 0 => Some(BigInt(a % b)),
        (ClientBinaryOp::Lt, BigInt(a), BigInt(b)) => Some(Bool(a < b)),
        (ClientBinaryOp::Gt, BigInt(a), BigInt(b)) => Some(Bool(a > b)),
        (ClientBinaryOp::Le, BigInt(a), BigInt(b)) => Some(Bool(a <= b)),
        (ClientBinaryOp::Ge, BigInt(a), BigInt(b)) => Some(Bool(a >= b)),
        (ClientBinaryOp::Add, Float(a), Float(b)) => Some(Float(a + b)),
        (ClientBinaryOp::Sub, Float(a), Float(b)) => Some(Float(a - b)),
        (ClientBinaryOp::Mul, Float(a), Float(b)) => Some(Float(a * b)),
        (ClientBinaryOp::Div, Float(a), Float(b)) => Some(Float(a / b)),
        (ClientBinaryOp::Rem, Float(a), Float(b)) => Some(Float(a % b)),
        (ClientBinaryOp::Eq, a, b) => Some(Bool(a == b)),
        (ClientBinaryOp::Ne, a, b) => Some(Bool(a != b)),
        (ClientBinaryOp::Lt, Int(a), Int(b)) => Some(Bool(a < b)),
        (ClientBinaryOp::Gt, Int(a), Int(b)) => Some(Bool(a > b)),
        (ClientBinaryOp::Le, Int(a), Int(b)) => Some(Bool(a <= b)),
        (ClientBinaryOp::Ge, Int(a), Int(b)) => Some(Bool(a >= b)),
        (ClientBinaryOp::Lt, Float(a), Float(b)) => Some(Bool(a < b)),
        (ClientBinaryOp::Gt, Float(a), Float(b)) => Some(Bool(a > b)),
        (ClientBinaryOp::Le, Float(a), Float(b)) => Some(Bool(a <= b)),
        (ClientBinaryOp::Ge, Float(a), Float(b)) => Some(Bool(a >= b)),
        (ClientBinaryOp::And, Bool(a), Bool(b)) => Some(Bool(*a && *b)),
        (ClientBinaryOp::Or, Bool(a), Bool(b)) => Some(Bool(*a || *b)),
        _ => None,
    }
}

fn apply_checked_integer_binop(
    op: rsjs_ir::CheckedIntegerOp,
    domain: rsjs_ir::IntegerDomain,
    lhs: &Value,
    rhs: &Value,
) -> Option<Value> {
    let integer = |value: &Value| match value {
        Value::Int(value) => Some(i128::from(*value)),
        Value::BigInt(value) => Some(*value),
        _ => None,
    };
    let lhs = integer(lhs)?;
    let rhs = integer(rhs)?;
    let (min, max) = match domain {
        rsjs_ir::IntegerDomain::I32 => (i32::MIN as i128, i32::MAX as i128),
        rsjs_ir::IntegerDomain::U32 => (0, u32::MAX as i128),
        rsjs_ir::IntegerDomain::I64 => (i64::MIN as i128, i64::MAX as i128),
        rsjs_ir::IntegerDomain::U64 => (0, u64::MAX as i128),
    };
    if !(min..=max).contains(&lhs) || !(min..=max).contains(&rhs) {
        return None;
    }
    let result = match op {
        rsjs_ir::CheckedIntegerOp::Add => lhs.checked_add(rhs)?,
        rsjs_ir::CheckedIntegerOp::Sub => lhs.checked_sub(rhs)?,
        rsjs_ir::CheckedIntegerOp::Mul => lhs.checked_mul(rhs)?,
        rsjs_ir::CheckedIntegerOp::Div => {
            if rhs == 0 || (lhs == min && rhs == -1 && min < 0) {
                return None;
            }
            lhs / rhs
        }
        rsjs_ir::CheckedIntegerOp::Rem => {
            if rhs == 0 || (lhs == min && rhs == -1 && min < 0) {
                return None;
            }
            lhs % rhs
        }
    };
    if !(min..=max).contains(&result) {
        return None;
    }
    Some(if domain.is_wide() {
        Value::BigInt(result)
    } else {
        Value::Int(i64::try_from(result).ok()?)
    })
}

/// Emit a single view-tree node into the `html_sync!` body.
///
/// The island wrapper (`<rsjs-island data-rsjs-island-root="...">`) is injected by the
/// caller in `emit_rust`, so this function never needs to know which
/// node is the root: every element is rendered the same way.
fn emit_static_marker_attr(
    name: TokenStream,
    value: syn::LitStr,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    match linked_site_ident {
        Some(site) => quote! {
            #name={
                ::rsjs::__private::LinkedMarkerAttr::new(#site, #value)
            }
        },
        None => quote! { #name=#value },
    }
}

fn occurrence_marker_value(
    value: TokenStream,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    match linked_site_ident {
        Some(site) => quote! {
            ::rsjs::__private::LinkedMarkerAttr::new(#site, #value)
        },
        None => value,
    }
}

fn occurrence_marker_value_from_fn(
    value: TokenStream,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    match linked_site_ident {
        Some(site) => quote! {
            ::rsjs::__private::LinkedMarkerAttr::from_fn(#site, || #value)
        },
        None => value,
    }
}

fn is_static_hydration_marker(name: &str) -> bool {
    name == MARKER_COMPONENT || name == MARKER_COMPONENT_MOUNT || name == MARKER_LIST
}

#[allow(clippy::too_many_arguments)]
fn emit_view_node(
    node_id: NodeId,
    island: &Island,
    text_order: &[NodeId],
    event_order: &[(NodeId, EventName)],
    cond_branches: &HashMap<NodeId, CondBranchInfo>,
    fragment_marker_aliases: &HashMap<NodeId, NodeId>,
    reachable_payload_attrs: &[(NodeId, String)],
    signal_initializers: &[SignalInitializer],
    query_initializers: &[QueryInitializer],
    server_expressions: Option<&HashMap<NodeId, syn::Expr>>,
    direct_component_site_nodes: Option<&HashMap<NodeId, u32>>,
    linked_site_ident: Option<&syn::Ident>,
) -> TokenStream {
    match island.view_tree.get(node_id) {
        ViewNode::Element {
            tag,
            children,
            static_attrs,
            server_attrs,
            payload_attrs,
        } => {
            // Hyphenated tag names (e.g. `<my-component>`) are valid
            // HTML custom elements but are not valid Rust identifiers,
            // so `format_ident!("{}", tag)` would panic at proc-macro
            // expansion time. Route through `tag_name_tokens` (the
            // same helper the wrapper element uses) which emits the
            // `Ident - Ident` token sequence that `html_sync!` accepts.
            let tag_open = tag_name_tokens(tag);
            let tag_close = tag_name_tokens(tag);

            // Hydration marker attribute names are sourced from the
            // shared `rsjs_ir` constants so the JS and Rust emitters
            // can never drift on attribute spelling.
            let bind_name = attr_name_tokens(MARKER_BIND);
            let attr_marker_name = attr_name_tokens(MARKER_ATTR);
            let handler_name = attr_name_tokens(MARKER_HANDLER);
            let ref_marker_name = attr_name_tokens(MARKER_REF);
            let marker_node_id = marker_id_for(node_id, fragment_marker_aliases);

            // Hydration marker attributes. The attribute *value* is the
            // NodeId so that `emit_js`'s
            // `root.querySelector('[data-rsjs-bind="<NodeId>"]')` can
            // find the node. `text_order` is consulted only to decide
            // whether to emit at all (it defines the deterministic set
            // of text-bound nodes); the position within it is irrelevant.
            let bind_attr = text_order.contains(&node_id).then(|| {
                let lit = syn::LitStr::new(
                    &marker_node_id.0.to_string(),
                    proc_macro2::Span::call_site(),
                );
                emit_static_marker_attr(bind_name.clone(), lit, linked_site_ident)
            });

            let has_attr_binding = island
                .bindings
                .attr_bindings
                .values()
                .any(|binding| binding.node == node_id);
            let attr_marker = has_attr_binding.then(|| {
                let lit = syn::LitStr::new(
                    &marker_node_id.0.to_string(),
                    proc_macro2::Span::call_site(),
                );
                emit_static_marker_attr(attr_marker_name.clone(), lit, linked_site_ident)
            });
            let attr_initials =
                emit_attr_initial_attrs(node_id, island, signal_initializers, query_initializers);

            let ref_marker = island.bindings.node_ref_bindings.get(&node_id).map(|_| {
                let lit = syn::LitStr::new(
                    &marker_node_id.0.to_string(),
                    proc_macro2::Span::call_site(),
                );
                emit_static_marker_attr(ref_marker_name.clone(), lit, linked_site_ident)
            });

            // Input binding marker + SSR-time initial value rendering.
            // The marker is one of `data-rsjs-input-{value,checked,
            // selected}="<NodeId>"` (sortable across kinds for
            // hydration); the initial-value attribute is `value=` for
            // text/select inputs (always emitted) and `checked` for
            // checkboxes (emitted only when truthy, matching the HTML
            // boolean-attribute convention).
            let (input_marker, input_initial): (Option<TokenStream>, Option<TokenStream>) =
                match island.bindings.input_bindings.get(&node_id) {
                    Some(binding) => {
                        let marker_attr = match binding.kind {
                            InputKind::Value => attr_name_tokens(MARKER_INPUT_VALUE),
                            InputKind::Checked => attr_name_tokens(MARKER_INPUT_CHECKED),
                            InputKind::Selected => attr_name_tokens(MARKER_INPUT_SELECTED),
                            // `InputKind` is `#[non_exhaustive]`;
                            // unknown future variants default to the
                            // text-input marker.
                            _ => attr_name_tokens(MARKER_INPUT_VALUE),
                        };
                        let id_lit = syn::LitStr::new(
                            &marker_node_id.0.to_string(),
                            proc_macro2::Span::call_site(),
                        );
                        let marker =
                            emit_static_marker_attr(marker_attr, id_lit, linked_site_ident);
                        let initial = emit_input_initial_attr(
                            binding.kind,
                            binding.signal,
                            island,
                            signal_initializers,
                        );
                        (Some(marker), initial)
                    }
                    None => (None, None),
                };

            // Event handlers need one stable marker per handler-bearing node.
            // The value is the NodeId so emit_js/runtime can resolve the same
            // element for delegated events, direct submit/document listeners,
            // and multiple event names on one element.
            let handler_attr: Option<TokenStream> =
                event_order.iter().any(|(n, _)| *n == node_id).then(|| {
                    let lit = syn::LitStr::new(
                        &marker_node_id.0.to_string(),
                        proc_macro2::Span::call_site(),
                    );
                    emit_static_marker_attr(handler_name.clone(), lit, linked_site_ident)
                });

            // Conditional branch attributes (M4): if this node belongs
            // to a then/else branch of some conditional section, emit
            // the marker attribute and (when the SSR-time predicate
            // evaluation says this branch is initially hidden) an
            // inline `style="display:none"` so the rendered HTML
            // matches what the client runtime will produce.
            let (cond_marker, cond_style): (Option<TokenStream>, Option<TokenStream>) =
                match cond_branches.get(&node_id) {
                    Some(info) => {
                        let marker = emit_static_marker_attr(
                            info.marker_name.clone(),
                            info.marker_value.clone(),
                            linked_site_ident,
                        );
                        let style = info.style_attr.clone();
                        (Some(marker), style)
                    }
                    None => (None, None),
                };

            // Children: either further view nodes, or — if this
            // element has a text binding wired to it directly (the
            // M1 fixture pattern for the span) — the initial value
            // of the bound signal as a `{ ... }` block.
            let text_binding_inline =
                island
                    .bindings
                    .text_bindings
                    .get(&node_id)
                    .copied()
                    .map(|expr_id| {
                        emit_text_binding_initial_value(
                            expr_id,
                            island,
                            signal_initializers,
                            query_initializers,
                        )
                    });

            let child_tokens: Vec<TokenStream> = children
                .iter()
                .map(|cid| {
                    emit_view_node(
                        *cid,
                        island,
                        text_order,
                        event_order,
                        cond_branches,
                        fragment_marker_aliases,
                        reachable_payload_attrs,
                        signal_initializers,
                        query_initializers,
                        server_expressions,
                        direct_component_site_nodes,
                        linked_site_ident,
                    )
                })
                .collect();

            // Author-written static HTML attributes (e.g.
            // `class="error"`, `aria-label="..."`, `id="foo"`,
            // `type="submit"`). The analyser populates these from
            // any attribute it does not recognise as `on_*` /
            // `bind_*`; emit_rust threads them straight into
            // `html_sync!` so the generated HTML preserves
            // user-authored markup verbatim. Boolean attributes
            // (stored with an empty value) are emitted as bare
            // attribute idents, matching HTML's boolean-attribute
            // shape. The attribute *value* round-trips through
            // `LitStr`, so escaping is handled at the macro layer.
            let static_attr_tokens: Vec<TokenStream> = static_attrs
                .iter()
                .map(|(name, value)| {
                    let attr_name = attr_name_tokens(name);
                    if is_static_hydration_marker(name) {
                        let value = syn::LitStr::new(value, proc_macro2::Span::call_site());
                        return emit_static_marker_attr(attr_name, value, linked_site_ident);
                    }
                    if value.is_empty() {
                        quote! { #attr_name }
                    } else {
                        let lit = syn::LitStr::new(value, proc_macro2::Span::call_site());
                        quote! { #attr_name=#lit }
                    }
                })
                .collect();
            let server_attr_tokens: Vec<TokenStream> = server_attrs
                .iter()
                .map(|attr| {
                    let attr_name = attr_name_tokens(&attr.name);
                    let value = emit_server_attr_value(&attr.name, &attr.tokens);
                    quote! { #attr_name={#value} }
                })
                .collect();
            let payload_attr_tokens: Vec<TokenStream> = payload_attrs
                .iter()
                .filter(|attr| {
                    payload_attr_is_reachable(reachable_payload_attrs, node_id, &attr.name)
                })
                .map(|attr| {
                    let attr_name = attr_name_tokens(&attr.name);
                    let value = emit_payload_attr_value(&attr.tokens);
                    let value = occurrence_marker_value_from_fn(value, linked_site_ident);
                    quote! { #attr_name={#value} }
                })
                .collect();

            // Keyed-list item roots carry `data-rsjs-key=<key value>`
            // so the client's `bind_list` adoption pass can pair each
            // server-rendered child with its initial item. The key
            // expression is the user's `key={...}` tokens (in scope:
            // the SSR `for` loop preserves the source loop pattern),
            // stringified with `Display` to match the JS emitter's
            // `String(<key>)` callback.
            let list_key_attr = island
                .lists
                .iter()
                .find(|list| list.item_root == node_id)
                .map(|list| {
                    let key_expr = parse_tokens_or_compile_error(
                        &list.key_tokens,
                        "keyed-list key expression",
                    );
                    let attr_name = attr_name_tokens(MARKER_LIST_KEY);
                    let value = quote! {
                        ::proa_core::shared::attr_value::display_attr(
                            ::core::format_args!("{}", #key_expr)
                        )
                    };
                    let value = occurrence_marker_value(value, linked_site_ident);
                    quote! {
                        #attr_name={#value}
                    }
                });

            quote! {
                < #tag_open #(#static_attr_tokens)* #(#server_attr_tokens)* #(#payload_attr_tokens)* #list_key_attr #(#attr_initials)* #bind_attr #attr_marker #ref_marker #handler_attr #input_marker #input_initial #cond_marker #cond_style >
                    #text_binding_inline
                    #(#child_tokens)*
                </ #tag_close >
            }
        }
        ViewNode::StaticText(text) => {
            // Author-written literal text under an element (e.g.
            // `"Submit"` inside `<button>Submit</button>`). The
            // string is rendered as a literal child of the
            // surrounding `html_sync!`; the macro's text emitter
            // handles HTML escaping.
            let lit = syn::LitStr::new(text, proc_macro2::Span::call_site());
            quote! { #lit }
        }
        ViewNode::TextBindingSlot => {
            // Standalone text-binding slot: emit the initial value
            // of the bound expression, plus a wrapping marker so the
            // client runtime can find it. M1 fixture doesn't use
            // these (it puts the binding on the <span> element
            // directly), but the codegen handles them so future
            // fixtures can rely on them.
            //
            // Use a `<span>` as the carrier so the marker has a host.
            let expr_id = island
                .bindings
                .text_bindings
                .get(&node_id)
                .copied()
                .expect("text binding slot must have a text binding");
            // Sanity-check that this slot is registered for emission;
            // the attribute *value* is the NodeId so it lines up with
            // `emit_js`'s `querySelector('[data-rsjs-bind="<NodeId>"]')`.
            debug_assert!(
                text_order.contains(&node_id),
                "text binding slot must appear in the text-binding order",
            );
            let marker_node_id = marker_id_for(node_id, fragment_marker_aliases);
            let bind_lit = syn::LitStr::new(
                &marker_node_id.0.to_string(),
                proc_macro2::Span::call_site(),
            );
            let bind_name = attr_name_tokens(MARKER_BIND);
            let initial = emit_text_binding_initial_value(
                expr_id,
                island,
                signal_initializers,
                query_initializers,
            );
            match linked_site_ident {
                Some(site) => quote! {
                    {
                        ::rsjs::__private::LinkedTextBinding::new(
                            #initial,
                            #site,
                            #bind_lit,
                        )
                    }
                },
                None => {
                    let marker = emit_static_marker_attr(bind_name, bind_lit, None);
                    quote! {
                        <span #marker>
                            #initial
                        </span>
                    }
                }
            }
        }
        ViewNode::ForLoop {
            attrs,
            label,
            pat,
            expr,
            body_prefix,
            children,
        } => {
            let attrs = parse_tokens_or_compile_error(attrs, "for-loop attributes");
            let label = parse_tokens_or_compile_error(label, "for-loop label");
            let pat = parse_tokens_or_compile_error(pat, "for-loop pattern");
            let expr = parse_tokens_or_compile_error(expr, "for-loop iterator expression");
            let body_prefix =
                parse_tokens_or_compile_error(body_prefix, "for-loop prefix statements");
            let child_tokens: Vec<TokenStream> = children
                .iter()
                .map(|cid| {
                    emit_view_node(
                        *cid,
                        island,
                        text_order,
                        event_order,
                        cond_branches,
                        fragment_marker_aliases,
                        reachable_payload_attrs,
                        signal_initializers,
                        query_initializers,
                        server_expressions,
                        direct_component_site_nodes,
                        linked_site_ident,
                    )
                })
                .collect();

            quote! {
                {
                    #attrs
                    #label for #pat in #expr {
                        #body_prefix
                        ::proa_macros::html_sync! {
                            #(#child_tokens)*
                        }
                    }
                }
            }
        }
        ViewNode::ServerOnly(server) => {
            if let Some(expression) =
                server_expressions.and_then(|expressions| expressions.get(&node_id))
            {
                if let (Some(edge_id), Some(site_ident)) = (
                    direct_component_site_nodes
                        .and_then(|component_sites| component_sites.get(&node_id)),
                    linked_site_ident,
                ) {
                    return quote! {
                        {
                            ::rsjs::__private::LinkedComponent::new(
                                #expression,
                                #site_ident.child(#edge_id),
                            )
                        }
                    };
                }
                return quote! { { #expression } };
            }
            let expr_tokens = server.tokens.parse::<TokenStream>().unwrap_or_else(|e| {
                let msg = format!(
                    "rsjs internal error: failed to reparse server_only slot `{}`: {e}",
                    server.tokens
                );
                let lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
                quote! { ::core::compile_error!(#lit) }
            });
            quote! { { #expr_tokens } }
        }
        // `ViewNode` is `#[non_exhaustive]`; the v0.1 emitter only
        // knows about `Element` / `Text` / `TextBindingSlot` /
        // `ServerOnly`. Future variants emit nothing structural until
        // the emitter learns them.
        _ => quote! {},
    }
}

fn parse_tokens_or_compile_error(tokens: &str, context: &str) -> TokenStream {
    tokens.parse::<TokenStream>().unwrap_or_else(|err| {
        let msg = format!("rsjs internal error: failed to reparse {context} `{tokens}`: {err}");
        let lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
        quote! { ::core::compile_error!(#lit) }
    })
}

fn emit_server_attr_value(attr_name: &str, tokens: &str) -> TokenStream {
    let token_stream = match tokens.parse::<TokenStream>() {
        Ok(token_stream) => token_stream,
        Err(err) => {
            let msg =
                format!("rsjs internal error: failed to reparse server_attr `{tokens}`: {err}");
            let lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
            return quote! { ::core::compile_error!(#lit) };
        }
    };
    let expr = match syn::parse2::<syn::Expr>(token_stream) {
        Ok(expr) => expr,
        Err(err) => {
            let msg = format!("rsjs internal error: failed to parse server_attr `{tokens}`: {err}");
            let lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
            return quote! { ::core::compile_error!(#lit) };
        }
    };
    let syn::Expr::Call(call) = expr else {
        let lit = syn::LitStr::new(
            "rsjs internal error: server_attr token is not a call",
            proc_macro2::Span::call_site(),
        );
        return quote! { ::core::compile_error!(#lit) };
    };
    if call.args.len() != 1 {
        let lit = syn::LitStr::new(
            "rsjs internal error: server_attr token does not have exactly one argument",
            proc_macro2::Span::call_site(),
        );
        return quote! { ::core::compile_error!(#lit) };
    }
    let inner = call.args.first().expect("len checked above");
    if attr_name == "style" {
        return quote! { #inner };
    }
    quote! { ::proa_core::perf_ok(#inner) }
}

fn emit_payload_attr_value(tokens: &str) -> TokenStream {
    let expr_tokens = match tokens.parse::<TokenStream>() {
        Ok(token_stream) => token_stream,
        Err(err) => {
            let msg =
                format!("rsjs internal error: failed to reparse payload attr `{tokens}`: {err}");
            let lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
            return quote! { ::core::compile_error!(#lit) };
        }
    };
    quote! {{
        let mut __rsjs_payload_json = ::std::string::String::new();
        ::rsjs::ClientEncode::encode_into(&(#expr_tokens), &mut __rsjs_payload_json);
        __rsjs_payload_json
    }}
}

fn emit_attr_initial_attrs(
    node_id: NodeId,
    island: &Island,
    _signal_initializers: &[SignalInitializer],
    query_initializers: &[QueryInitializer],
) -> Vec<TokenStream> {
    let mut bindings: Vec<_> = island
        .bindings
        .attr_bindings
        .values()
        .filter(|binding| binding.node == node_id)
        .collect();
    bindings.sort_by(|a, b| a.name.cmp(&b.name));

    bindings
        .into_iter()
        .filter_map(|binding| {
            let attr_name = attr_name_tokens(&binding.name);
            let expr = island.exprs.client_expr(binding.expr);
            Some(match binding.kind {
                AttrBindingKind::Boolean => {
                    let rust_expr = emit_expr_as_bool_rust(expr, query_initializers)?;
                    quote! { #attr_name={#rust_expr} }
                }
                AttrBindingKind::Value | AttrBindingKind::Url => {
                    let rust_expr = emit_expr_as_rust(expr, query_initializers)?;
                    quote! {
                        #attr_name={
                            ::proa_core::shared::attr_value::display_attr(
                                ::core::format_args!("{}", #rust_expr)
                            )
                        }
                    }
                }
                _ => quote! {},
            })
        })
        .collect()
}

/// Emit the initial-value rendering for a text binding.
///
/// At M3 the only reactive expression shape that can land in a text
/// binding is a [`ClientExpr::SignalRead`] (the M1 fixture's `count.get()`).
/// Other shapes (literals, derived expressions) fall back to a
/// `Default::default()` placeholder so the generated code still
/// compiles; once M4 lands the conditional / derived expression
/// fixture, this can grow.
fn emit_text_binding_initial_value(
    expr_id: ExprId,
    island: &Island,
    signal_initializers: &[SignalInitializer],
    query_initializers: &[QueryInitializer],
) -> TokenStream {
    let expr = island.exprs.client_expr(expr_id);
    match expr {
        ClientExpr::SignalRead(sig) => {
            let local = signal_local_ident(*sig);
            let signal = island.signals.iter().find(|signal| signal.id == *sig);
            let value =
                if signal.is_some_and(|signal| matches!(&signal.ty, rsjs_ir::ClientType::String)) {
                    // String-like signal initializers may be `String`, `&str`, or a
                    // borrowed shared-state `&String`. `AsRef<str>` borrows all
                    // three without moving or cloning the render local.
                    quote! { ::core::convert::AsRef::<str>::as_ref(&#local) }
                } else if signal.is_some_and(|signal| signal.shared.is_some()) {
                    // A shared field's local is whatever `__rsjs_replay_<field>()`
                    // returned: `&str` for the string-like shapes, a value for
                    // scalars. Both render directly, and neither clones. The macro
                    // cannot know which one it is (see
                    // `Analyzer::shared_field_client_type`) and does not need to.
                    quote! { #local }
                } else if signal_initializer_for(signal_initializers, *sig).is_some() {
                    // TODO(rsjs-live-read-borrow): synthetic inbound LiveRead
                    // signals currently share the local-state initializer
                    // path, so a String rendered as text incurs a second SSR
                    // clone/allocation. The linked-site String fixture locks
                    // correctness; a later initializer-provenance change can
                    // distinguish LiveInbound from LocalStateSeed and move or
                    // borrow exactly once without widening this ABI slice.
                    quote! { #local.clone() }
                } else {
                    quote! { #local }
                };
            text_binding_value_tokens(value, false)
        }
        ClientExpr::Literal(value) => {
            let (_, lit) = render_value_as_rust(value);
            text_binding_value_tokens(quote! { #lit }, false)
        }
        other => {
            if let Some(rust_expr) = emit_expr_as_rust(other, query_initializers) {
                return text_binding_value_tokens(rust_expr, island.is_async);
            }
            emit_text_binding_default(other)
        }
    }
}

fn text_binding_value_tokens(value: TokenStream, is_async: bool) -> TokenStream {
    if is_async {
        quote! { { ::proa_core::to_async(#value) } }
    } else {
        quote! { { #value } }
    }
}

fn emit_text_binding_default(expr: &ClientExpr) -> TokenStream {
    // M4+ extends this. For now, fall back to a typed default
    // so the code at least compiles.
    let _ = expr;
    quote! { { ::proa_core::perf_ok("") } }
}

/// Lower a client-helper [`ClientExpr::Call`] to a Rust call expression.
///
/// New helper edges replay the exact authored path so rustc resolves ordinary
/// imports, renamed imports, and re-exports. Legacy client-module imports keep
/// their `<module>::<name>` reconstruction for descriptor compatibility.
fn emit_client_call_as_rust(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> TokenStream {
    let ClientExpr::Call { function, args } = expr else {
        return quote! { Default::default() };
    };
    let callee = match function {
        ClientFunctionRef::Import { module, name } => {
            let module_ts: TokenStream = module.parse().unwrap_or_else(|_| {
                quote! { ::core::compile_error!("invalid client_module path") }
            });
            let name_ident = format_ident!("{}", name);
            quote! { #module_ts::#name_ident }
        }
        ClientFunctionRef::Helper { authored_path, .. } => {
            authored_path.parse().unwrap_or_else(|_| {
                quote! { ::core::compile_error!("invalid authored client-helper path") }
            })
        }
        _ => return quote! { Default::default() },
    };
    let arg_tokens: Vec<TokenStream> = args
        .iter()
        .map(|a| emit_client_call_arg_as_rust(a, query_initializers))
        .collect();
    quote! { #callee( #( #arg_tokens ),* ) }
}

fn emit_client_call_arg_as_rust(
    expr: &ClientExpr,
    query_initializers: &[QueryInitializer],
) -> TokenStream {
    match expr {
        ClientExpr::SignalRead(sig) => {
            let local = signal_local_ident(*sig);
            quote! { #local.clone() }
        }
        ClientExpr::Literal(value) => {
            match value {
                // Direct helper parameters provide the same contextual type
                // boundary as ordinary Rust calls. Keep representation-level
                // Client IR literals unsuffixed here so rustc can recover that
                // type. Explicitly suffixed source literals carry a checked
                // domain anchor from reactive lowering and take the typed
                // branch below instead.
                Value::Int(value) => {
                    let literal = proc_macro2::Literal::i64_unsuffixed(*value);
                    quote! { #literal }
                }
                Value::BigInt(value) => {
                    let literal = proc_macro2::Literal::i128_unsuffixed(*value);
                    quote! { #literal }
                }
                _ => {
                    let (_, literal) = render_value_as_rust(value);
                    quote! { #literal }
                }
            }
        }
        ClientExpr::Call { .. } => emit_client_call_as_rust(expr, query_initializers),
        ClientExpr::Binary {
            op: ClientBinaryOp::Checked { .. },
            ..
        } => emit_expr_as_rust(expr, query_initializers)
            .unwrap_or_else(|| quote! { Default::default() }),
        ClientExpr::QueryRead { .. }
        | ClientExpr::NodePayload { .. }
        | ClientExpr::Project { .. }
        | ClientExpr::AsType { .. }
        | ClientExpr::IsPresent { .. }
        | ClientExpr::Eq { .. }
        | ClientExpr::Neq { .. }
        | ClientExpr::Coalesce { .. } => emit_expr_as_rust(expr, query_initializers)
            .unwrap_or_else(|| quote! { Default::default() }),
        // SignalSet / SignalUpdate / IsNonEmpty / BinaryOp inside a
        // ClientCall arg position is unsupported in v0.1; fall back to
        // a default so the generated Rust still compiles, and the
        // server-side SSR value is whatever the helper does with the
        // default.
        _ => quote! { Default::default() },
    }
}

/// Render the SSR-time initial-value attribute for an input binding.
///
/// - `Value` / `Selected` always emit `value="<initial>"` so the
///   rendered HTML reflects the bound signal's starting value. Empty
///   strings still emit `value=""` for symmetry.
/// - `Checked` emits the bare `checked` boolean attribute when the
///   signal's initial value is `Bool(true)` and emits nothing when
///   it is `Bool(false)`. (HTML boolean attributes carry truthiness
///   by presence/absence, not by `="false"`.)
///
/// Returns `None` when no initial-value attribute is needed (the
/// `Checked + false` case), otherwise returns the attribute tokens
/// the caller can splice into the element's attribute list.
fn emit_input_initial_attr(
    kind: InputKind,
    signal: SignalId,
    island: &Island,
    signal_initializers: &[SignalInitializer],
) -> Option<TokenStream> {
    if signal_initializer_for(signal_initializers, signal).is_some() {
        let local = signal_local_ident(signal);
        return match kind {
            InputKind::Value | InputKind::Selected => Some(quote! { value={#local.clone()} }),
            InputKind::Checked => Some(quote! { checked={#local} }),
            _ => None,
        };
    }

    let decl = island.signals.iter().find(|s| s.id == signal);

    // Shared-state field cells never enter `signal_initializers`: they are
    // acquired from the provider scope rather than from a runtime `signal(...)`
    // expression, and their IR `init` is a `Null` placeholder. The seeded value
    // is available as `__rsjs_initial_signal_<n>` (see
    // `emit_signal_initializers`), so read that local the same way
    // `emit_text_binding_initial_value` does. Without this the server renders
    // `value=""` over a value it just seeded, and a form submitted before
    // hydration posts empty fields.
    if decl.is_some_and(|decl| decl.shared.is_some()) {
        let local = signal_local_ident(signal);
        // The macro cannot know a shared field's Rust type (see
        // `Analyzer::shared_field_client_type`), so it does not branch on one.
        // `InputKind` alone decides the shape, and the value is rendered
        // through `Display`, which accepts every scalar the replay accessor
        // hands back plus the borrowed `&String` form, with no clone and no
        // temporary. A field whose Rust type has no `Display` (a `Vec`, a
        // nested struct) is a rustc error at this call site rather than a
        // silently empty control.
        return match kind {
            InputKind::Value | InputKind::Selected => Some(quote! {
                value={
                    ::proa_core::shared::attr_value::display_attr(
                        ::core::format_args!("{}", #local)
                    )
                }
            }),
            InputKind::Checked => Some(quote! { checked={#local} }),
            _ => None,
        };
    }

    let init = decl.map(|s| s.init.clone()).unwrap_or(Value::Null);
    match kind {
        InputKind::Value | InputKind::Selected => {
            let s = match &init {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Int(i) => i.to_string(),
                Value::Float(f) => f.to_string(),
                Value::Null => String::new(),
                // `Value` is `#[non_exhaustive]`; default to empty
                // for any future variant the emitter has not learned.
                _ => String::new(),
            };
            let lit = syn::LitStr::new(&s, proc_macro2::Span::call_site());
            Some(quote! { value=#lit })
        }
        InputKind::Checked => match init {
            Value::Bool(true) => Some(quote! { checked }),
            _ => None,
        },
        // `InputKind` is `#[non_exhaustive]`; future variants emit
        // no initial-value attribute until the emitter learns them.
        _ => None,
    }
}

/// Local-variable ident for the initial value of signal `id`.
/// `pub(crate)` so the analyser can rewrite a keyed list's SSR loop
/// iterator (`items.get()`) to iterate the rendered signal local.
pub(crate) fn signal_local_ident(id: SignalId) -> proc_macro2::Ident {
    format_ident!("__rsjs_initial_signal_{}", id.0)
}

/// Convert a hyphenated HTML attribute name (e.g. `data-rsjs-bind`) into
/// the `Ident - Ident - Ident` token sequence the `html_sync!` parser
/// accepts. Mirrors the reverse mapping done by `proa_dsl::emit`.
///
/// Used to pipe the marker constants from `rsjs_ir` (`MARKER_BIND`,
/// `MARKER_HANDLER`, `ISLAND_MARKER_ATTR`) through `quote!` without
/// duplicating their string literals at the call sites.
fn attr_name_tokens(name: &str) -> TokenStream {
    let mut tokens = TokenStream::new();
    let span = proc_macro2::Span::call_site();
    for (i, part) in name.split('-').enumerate() {
        if i > 0 {
            quote::TokenStreamExt::append(
                &mut tokens,
                proc_macro2::Punct::new('-', proc_macro2::Spacing::Alone),
            );
        }
        append_html_name_part(&mut tokens, part, span);
    }
    tokens
}

fn append_html_name_part(tokens: &mut TokenStream, part: &str, span: proc_macro2::Span) {
    if is_rust_keyword(part) {
        let ts: TokenStream = part
            .parse()
            .expect("hard-coded Rust keyword must parse as a token");
        quote::TokenStreamExt::append_all(tokens, ts);
        return;
    }
    let ident = proc_macro2::Ident::new(part, span);
    quote::TokenStreamExt::append(tokens, ident);
}

fn is_rust_keyword(part: &str) -> bool {
    matches!(
        part,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

/// Convert a hyphenated HTML element tag name (e.g. `rsjs-island`) into
/// the same `Ident - Ident` token sequence used by `attr_name_tokens`.
/// Tag names and attribute names share the same lexical shape after
/// `proa_dsl`'s tag-name parser was extended to accept hyphens, so the
/// implementation is identical; the alias exists for call-site clarity.
fn tag_name_tokens(name: &str) -> TokenStream {
    attr_name_tokens(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsjs_ir::{
        ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, IslandBuilder,
        Value,
    };

    fn signal_update_block(signal: SignalId, op: ClientBinaryOp, value: ClientExpr) -> ClientBlock {
        ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            signal,
            ClientSimpleClosure::new(
                "c",
                ClientExpr::binary(op, ClientExpr::signal_read(signal), value),
            ),
        )])
    }

    #[test]
    fn lossless_server_expression_side_table_wins_over_serialized_tokens() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        let server_node = builder.server_only_node_with_source(
            root,
            "this is deliberately not valid Rust @",
            rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
        );
        builder.set_root(root);
        let island = builder.build();
        let source_expression: syn::Expr = syn::parse_quote!(render_markdown(self.document));
        let server_expressions = HashMap::from([(server_node, source_expression)]);

        let rendered = emit_view_node(
            root,
            &island,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            Some(&server_expressions),
            None,
            None,
        )
        .to_string();

        assert!(rendered.contains("render_markdown"), "{rendered}");
        assert!(rendered.contains("self . document"), "{rendered}");
        assert!(
            !rendered.contains("compile_error"),
            "lossless local emission must not reparse the IR token string: {rendered}"
        );
        assert!(
            !rendered.contains("deliberately"),
            "serialized diagnostic tokens must not replace the authored syn::Expr: {rendered}"
        );
    }

    #[test]
    fn linked_direct_component_wraps_the_original_expression_with_its_child_site() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        let component_node = builder.server_only_node_with_source(
            root,
            "redacted component tokens",
            rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
        );
        builder.set_root(root);
        let island = builder.build();
        let component: syn::Expr = syn::parse_quote! {
            ImportedBadge {
                label: self.title,
            }
        };
        let server_expressions = HashMap::from([(component_node, component)]);
        let component_sites = HashMap::from([(component_node, 7_u32)]);
        let site_ident = syn::Ident::new("__rsjs_site", proc_macro2::Span::call_site());

        let rendered = emit_view_node(
            root,
            &island,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            Some(&server_expressions),
            Some(&component_sites),
            Some(&site_ident),
        )
        .to_string();

        assert!(rendered.contains("LinkedComponent :: new"), "{rendered}");
        assert!(
            rendered.contains("__rsjs_site . child (7u32)"),
            "{rendered}"
        );
        assert!(rendered.contains("ImportedBadge"), "{rendered}");
        assert!(rendered.contains("self . title"), "{rendered}");
    }

    #[test]
    fn linked_lossless_site_uses_one_runtime_boundary_for_every_owner_role() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        builder.set_root(root);
        let island = builder.build();
        let context = syn::Ident::new("cx", proc_macro2::Span::call_site());
        let site_ident =
            syn::Ident::new("__rsjs_linked_render_site", proc_macro2::Span::call_site());

        let site = emit_lossless_render_site(
            &island,
            "static_component",
            &[],
            &[],
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &TokenStream::new(),
            &TokenStream::new(),
            &context,
            &HashMap::new(),
            &HashMap::new(),
            Some(&site_ident),
        );
        let setup = site.setup.to_string();
        let template = site.template.to_string();

        assert!(setup.contains("__rsjs_linked_init_state_json"), "{setup}");
        assert!(setup.contains("linked_owner_value"), "{setup}");
        assert!(
            setup.contains("Result :: < _ , :: proa_core :: WriteError > :: Ok"),
            "owner-only seed construction must remain type-checked and fallible: {setup}"
        );
        assert!(!setup.contains("rsjs_island_used"), "{setup}");
        assert!(
            template.contains("LinkedRenderBoundary :: new"),
            "{template}"
        );
        assert!(
            template.contains("__rsjs_linked_init_state_json . as_deref"),
            "the boundary must receive no signal seed for an inactive occurrence: {template}"
        );
        assert!(template.contains("__rsjs_linked_render_site"), "{template}");
        assert!(
            template.matches("html_sync").count() >= 2,
            "the stable outer template must contain the authored inner template: {template}"
        );
    }

    #[test]
    fn linked_hydration_markers_are_occurrence_aware() {
        let island = build_counter_island();
        let root = island.view_tree.root().expect("counter root");
        let text_order = sorted_text_bindings(&island);
        let event_order = sorted_event_bindings(&island);
        let site_ident =
            syn::Ident::new("__rsjs_linked_render_site", proc_macro2::Span::call_site());

        let linked = emit_view_node(
            root,
            &island,
            &text_order,
            &event_order,
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            None,
            None,
            Some(&site_ident),
        )
        .to_string();
        assert!(linked.contains("LinkedMarkerAttr :: new"), "{linked}");
        assert!(linked.contains("data - rsjs - bind"), "{linked}");
        assert!(linked.contains("data - rsjs - handler"), "{linked}");
        assert!(
            linked.contains("__rsjs_linked_render_site"),
            "the marker decision must come from the exact occurrence: {linked}"
        );

        let provisional = emit_view_node(
            root,
            &island,
            &text_order,
            &event_order,
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            None,
            None,
            None,
        )
        .to_string();
        assert!(
            !provisional.contains("LinkedMarkerAttr"),
            "descriptor/provisional emission keeps its definition-level markers: {provisional}"
        );
    }

    #[test]
    fn linked_payload_markers_construct_their_json_lazily() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        builder.payload_attr(root, "data-rsjs-payload-p0", "payload_value()");
        builder.set_root(root);
        let island = builder.build();
        let site_ident =
            syn::Ident::new("__rsjs_linked_render_site", proc_macro2::Span::call_site());

        let linked = emit_view_node(
            root,
            &island,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &[(root, "data-rsjs-payload-p0".to_owned())],
            &[],
            &[],
            None,
            None,
            Some(&site_ident),
        )
        .to_string();

        assert!(linked.contains("LinkedMarkerAttr :: from_fn"), "{linked}");
        assert!(linked.contains("payload_value"), "{linked}");
        assert!(
            linked.contains("String :: new"),
            "the payload encoder remains inside the owner-only factory: {linked}"
        );
    }

    #[test]
    fn linked_text_slots_use_an_occurrence_aware_carrier() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("p", None);
        let signal = builder.signal(Value::String("initial".to_owned()));
        let expression = builder.expr(ClientExpr::signal_read(signal));
        let slot = builder.text_node(root, expression);
        builder.set_root(root);
        let island = builder.build();
        let text_order = sorted_text_bindings(&island);
        let site_ident =
            syn::Ident::new("__rsjs_linked_render_site", proc_macro2::Span::call_site());

        let linked = emit_view_node(
            slot,
            &island,
            &text_order,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            None,
            None,
            Some(&site_ident),
        )
        .to_string();
        assert!(linked.contains("LinkedTextBinding :: new"), "{linked}");
        assert!(linked.contains("__rsjs_linked_render_site"), "{linked}");
        assert!(
            !linked.contains("< span"),
            "the linked wrapper chooses whether a carrier exists at render time: {linked}"
        );

        let provisional = emit_view_node(
            slot,
            &island,
            &text_order,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &[],
            &[],
            &[],
            None,
            None,
            None,
        )
        .to_string();
        assert!(provisional.contains("< span"), "{provisional}");
        assert!(provisional.contains("data - rsjs - bind"), "{provisional}");
    }

    #[test]
    fn checked_wide_expression_comparison_replays_literal_in_its_i64_domain() {
        let expr = ClientExpr::binary(
            ClientBinaryOp::Lt,
            ClientExpr::binary(
                ClientBinaryOp::Checked {
                    op: rsjs_ir::CheckedIntegerOp::Add,
                    domain: rsjs_ir::IntegerDomain::I64,
                },
                ClientExpr::Literal(Value::BigInt(1)),
                ClientExpr::Literal(Value::BigInt(0)),
            ),
            ClientExpr::Literal(Value::BigInt(23)),
        );

        let rendered = emit_expr_as_rust(&expr, &[])
            .expect("checked comparison should replay in server Rust")
            .to_string();

        assert!(rendered.contains("checked_add"), "{rendered}");
        assert!(!rendered.contains("23i128"), "{rendered}");
    }

    #[test]
    fn checked_nonliteral_operands_replay_through_the_declared_rust_domain() {
        let expr = ClientExpr::binary(
            ClientBinaryOp::Checked {
                op: rsjs_ir::CheckedIntegerOp::Div,
                domain: rsjs_ir::IntegerDomain::I64,
            },
            ClientExpr::SignalRead(SignalId(0)),
            ClientExpr::SignalRead(SignalId(1)),
        );

        let rendered = emit_expr_as_rust(&expr, &[])
            .expect("checked division should replay in server Rust")
            .to_string();

        assert!(rendered.contains("checked_div"), "{rendered}");
        assert_eq!(
            rendered
                .matches("identity :: < :: core :: primitive :: i64 >")
                .count(),
            2,
            "both nonliteral operands need an i64 source-type witness: {rendered}"
        );
    }

    #[test]
    fn typed_shared_signal_initializer_emits_a_rust_domain_witness() {
        let mut builder = IslandBuilder::new();
        let store = builder.shared_store("state".to_string(), "ActualState".to_string(), None);
        let signal = builder.shared_field(store, "value".to_string(), rsjs_ir::ClientType::Dyn);
        builder.set_signal_integer_domain(signal, Some(rsjs_ir::IntegerDomain::I32));
        builder.set_signal_ty(signal, rsjs_ir::ClientType::I32);
        let island = builder.build();

        let rendered = emit_signal_initializers(&island, &[], &[signal]).to_string();

        assert!(
            rendered.contains("let _ : & :: core :: primitive :: i32 = & __rsjs_initial_signal_0"),
            "handler-only shared integer domains need a generated Rust type witness: {rendered}"
        );
    }

    fn all_signal_ids(island: &Island) -> Vec<SignalId> {
        island.signals.iter().map(|signal| signal.id).collect()
    }

    /// Hand-build the M1 Counter island. Same shape as the M1
    /// fixture in `rsjs_ir/tests/counter.rs`; duplicated here so
    /// emit_rust's tests have no cross-crate dependency surface
    /// beyond the IR itself.
    fn build_counter_island() -> Island {
        let mut b = IslandBuilder::new();

        let count_signal = b.signal(Value::Int(0));
        let count_read_expr = b.expr(ClientExpr::SignalRead(count_signal));

        let decrement_handler = b.handler_from_ir(signal_update_block(
            count_signal,
            ClientBinaryOp::Sub,
            ClientExpr::int(1),
        ));
        let increment_handler = b.handler_from_ir(signal_update_block(
            count_signal,
            ClientBinaryOp::Add,
            ClientExpr::int(1),
        ));

        let root = b.element("div", None);
        let decrement_button = b.element("button", Some(root));
        let span = b.element("span", Some(root));
        let increment_button = b.element("button", Some(root));

        b.bind_text(span, count_read_expr);
        b.bind_event(decrement_button, EventName::Click, decrement_handler);
        b.bind_event(increment_button, EventName::Click, increment_handler);
        b.set_root(root);

        b.build()
    }

    fn build_server_only_panel_island() -> Island {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        b.static_attr(root, "class", "panel");
        b.server_attr(root, "aria-label", "rsjs::server_attr(self.label)");
        b.text_node_static(root, "Ready");
        b.server_only_node(root, "UnsafeRaw(\"<strong>Server only</strong>\")");
        b.set_root(root);
        b.build()
    }

    fn pretty_format(tokens: TokenStream) -> String {
        let file: syn::File = match syn::parse2(tokens.clone()) {
            Ok(f) => f,
            Err(e) => panic!(
                "emit_rust output failed to parse as a syn::File: {e}\n\nraw tokens:\n{tokens}"
            ),
        };
        let mut formatted = String::from(
            "// @generated by the rsjs emit_rust golden harness\n\
             // proa-perf-ok(scope): generated fixture embeds trusted serialized island state\n",
        );
        formatted.push_str(&prettyplease::unparse(&file));
        formatted
    }

    /// Golden test: the emitted impl matches the checked-in
    /// `counter_impl.rs` golden. If you change codegen, regenerate
    /// the golden by setting `RSJS_BLESS_GOLDEN=1` and re-running
    /// the test (the harness writes the new golden to disk).
    #[test]
    fn emit_rust_counter_matches_golden() {
        let island = build_counter_island();
        let path: syn::Path = syn::parse_quote!(Counter);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("golden")
            .join("emit_rust_counter.golden.rs");

        if std::env::var("RSJS_BLESS_GOLDEN").is_ok() {
            std::fs::create_dir_all(golden_path.parent().unwrap())
                .expect("failed to create tests/ dir for golden");
            std::fs::write(&golden_path, &actual).expect("failed to write golden");
            return;
        }

        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
            panic!(
                "golden file missing at {}; rerun with RSJS_BLESS_GOLDEN=1 to generate",
                golden_path.display()
            )
        });

        assert_eq!(
            actual, expected,
            "emit_rust output drifted from golden; rerun with RSJS_BLESS_GOLDEN=1 to update"
        );
    }

    /// Regression: `emit_rust` hardcoded an empty shared-initializer slice, so
    /// any island with a reachable shared store aborted the proc macro with
    /// `panicked at "reachable shared store must retain its Rust initializer"`
    /// instead of emitting (or diagnosing) anything.
    #[test]
    fn emit_rust_emits_shared_state_islands_instead_of_panicking() {
        let mut b = IslandBuilder::new();
        let store = b.shared_store("cart".to_string(), "CartEmitState".to_string(), None);
        let signal = b.shared_field(store, "count".to_string(), rsjs_ir::ClientType::U32);
        let read = b.expr(ClientExpr::SignalRead(signal));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, read);
        b.set_root(root);
        let island = b.build();

        let body = pretty_format(emit_rust(&island, &syn::parse_quote!(CartIsland)));

        assert!(body.contains("rsjs_shared_state_acquire"), "{body}");
        assert!(body.contains("__rsjs_handle_from_server"), "{body}");
        // The handle local the signal initializer reads must be declared.
        assert!(body.contains("let cart ="), "{body}");
    }

    /// A shared-state acquisition replayed from an inlined component carries a
    /// const proof that the Rust type name still denotes the state the
    /// component was compiled against.
    ///
    /// The obligation is a comparison of two trait-resolved associated items,
    /// so it works through `use`, re-exports, aliases, local modules, and
    /// across crates, and it consults nothing outside this compilation. The
    /// name-keyed sidecar registry this replaced could answer `Ambiguous` for
    /// legal code and could go stale against `target/`.
    #[test]
    fn an_inlined_acquisition_asserts_the_recorded_type_key() {
        let mut b = IslandBuilder::new();
        let store = b.shared_store("cfg".to_string(), "Cfg".to_string(), None);
        let signal = b.shared_field(store, "label".to_string(), rsjs_ir::ClientType::Dyn);
        let island = b.build();

        let initializers = [SharedStateInitializer {
            id: store,
            source_name: format_ident!("cfg"),
            ty: syn::parse_quote!(Cfg),
            expected_type_key: Some(quote! {
                <Badge as ::rsjs::IslandSharedStores>::SHARED_TYPE_KEYS[0usize]
            }),
        }];
        let tokens = emit_shared_state_initializers(
            &island,
            &initializers,
            &[signal],
            "asset_id",
            &format_ident!("cx"),
            None,
        )
        .to_string();

        assert!(tokens.contains("TYPE_KEY"), "{tokens}");
        assert!(tokens.contains("IslandSharedStores"), "{tokens}");
        assert!(tokens.contains("SHARED_TYPE_KEYS"), "{tokens}");
        assert!(
            tokens.contains("shared-state-type-identity"),
            "the assertion must carry its rsjs diagnostic code: {tokens}"
        );
    }

    /// An acquisition written here needs no identity proof: the type the author
    /// named is by definition the one they meant.
    #[test]
    fn a_locally_written_acquisition_emits_no_identity_assertion() {
        let mut b = IslandBuilder::new();
        let store = b.shared_store("cfg".to_string(), "Cfg".to_string(), None);
        let signal = b.shared_field(store, "label".to_string(), rsjs_ir::ClientType::Dyn);
        let island = b.build();

        let initializers = [SharedStateInitializer {
            id: store,
            source_name: format_ident!("cfg"),
            ty: syn::parse_quote!(Cfg),
            expected_type_key: None,
        }];
        let tokens = emit_shared_state_initializers(
            &island,
            &initializers,
            &[signal],
            "asset_id",
            &format_ident!("cx"),
            None,
        )
        .to_string();

        assert!(!tokens.contains("IslandSharedStores"), "{tokens}");
    }

    #[test]
    fn a_linked_acquisition_uses_only_its_exact_provider_requirement() {
        let mut b = IslandBuilder::new();
        let store = b.shared_store("cfg".to_string(), "Cfg".to_string(), None);
        let signal = b.shared_field(store, "label".to_string(), rsjs_ir::ClientType::Dyn);
        let island = b.build();

        let initializers = [SharedStateInitializer {
            id: store,
            source_name: format_ident!("cfg"),
            ty: syn::parse_quote!(Cfg),
            expected_type_key: None,
        }];
        let linked_site = format_ident!("__rsjs_linked_render_site");
        let tokens = emit_shared_state_initializers(
            &island,
            &initializers,
            &[signal],
            "asset_id",
            &format_ident!("cx"),
            Some(&linked_site),
        )
        .to_string();

        assert!(
            tokens.contains("rsjs_shared_state_acquire_linked"),
            "{tokens}"
        );
        assert!(
            tokens.contains("__rsjs_linked_render_site . shared_state_provider (0u32)"),
            "{tokens}"
        );
        assert!(
            !tokens.contains("rsjs_shared_state_acquire ::"),
            "linked code must never fall back to nearest-provider lookup: {tokens}"
        );
    }

    /// F25 regression: the shared-store scope id is written straight into
    /// the seed buffer as a bare number. `u64::to_string()` here would
    /// heap-allocate one temporary String per shared store per island
    /// render, in per-request generated code.
    #[test]
    fn shared_scope_seed_writes_integer_without_to_string() {
        let mut b = IslandBuilder::new();
        let store = b.shared_store("workbench".to_string(), "WorkbenchState".to_string(), None);
        let signal = b.shared_field(store, "button_bg".to_string(), rsjs_ir::ClientType::String);
        let island = b.build();

        let initializers = [SharedStateInitializer {
            id: store,
            source_name: format_ident!("workbench"),
            ty: syn::parse_quote!(WorkbenchState),
            expected_type_key: None,
        }];
        let tokens = emit_init_state_json(&island, &[], &initializers, &[signal]);
        let body = tokens.let_tokens.to_string();
        assert!(body.contains("scope_id"), "{body}");
        assert!(
            !body.contains("to_string"),
            "scope id seed must not allocate via to_string: {body}"
        );
        assert!(
            body.contains("write !"),
            "scope id seed must write digits into the existing buffer: {body}"
        );
        assert!(
            body.contains("make_script_json_safe"),
            "runtime ClientEncode output must be sanitized at the script-data boundary: {body}"
        );
    }

    /// Round-3 review #1: when `emit_rust_with_prelude` is given
    /// extra prelude lets, they must appear verbatim in the emitted
    /// impl body.
    #[test]
    fn emit_rust_with_prelude_replays_extra_lets_verbatim() {
        let island = build_counter_island();
        let path: syn::Path = syn::parse_quote!(Counter);
        let extra: syn::Stmt = syn::parse_quote!(let user = some_helper(self.id););
        let local = match extra {
            syn::Stmt::Local(l) => l,
            _ => panic!("expected a Local"),
        };
        let tokens = emit_rust_with_prelude(&island, &path, &[local]);
        let s = tokens.to_string();
        assert!(
            s.contains("let user = some_helper"),
            "expected the prelude `let user = some_helper(...)` to be \
             replayed verbatim in the emitted impl body, got:\n{s}"
        );
    }

    #[test]
    fn emit_rust_server_only_strategy_omits_island_scaffolding() {
        let island = build_server_only_panel_island();
        let path: syn::Path = syn::parse_quote!(ServerPanel);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        assert!(
            actual.contains("html_sync!") && actual.contains("Server only"),
            "server-only output should still render the view tree, got:\n{actual}"
        );
        assert!(
            !actual.contains("rsjs_island_used")
                && !actual.contains("rsjs - island")
                && !actual.contains("__rsjs_init_state_json"),
            "server-only output must not emit wrapper, seed JSON, or asset usage, got:\n{actual}"
        );
    }

    #[test]
    fn emit_rust_emits_only_reachable_payload_attrs() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let span = b.element("span", Some(root));
        b.payload_attr(button, "data-rsjs-payload-p0", "public_payload()");
        b.payload_attr(button, "data-rsjs-payload-p1", "secret_payload()");
        b.payload_attr(span, "data-rsjs-payload-p2", "loop_label()");
        b.payload_attr(span, "data-rsjs-payload-p3", "loop_secret()");
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            },
        )]));
        b.bind_event(button, EventName::Click, handler);
        let text = b.expr(ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p2".to_string(),
            tokens: "loop_label()".to_string(),
        });
        b.bind_text(span, text);
        b.set_root(root);
        let island = b.build();

        let path: syn::Path = syn::parse_quote!(PayloadButton);
        let tokens = emit_rust(&island, &path).to_string();

        assert!(
            tokens.contains("public_payload"),
            "reachable handler payload should be emitted: {tokens}"
        );
        assert!(
            tokens.contains("loop_label"),
            "reachable node payload should be emitted: {tokens}"
        );
        assert!(
            !tokens.contains("secret_payload"),
            "unread payload attrs must not serialize into generated Rust: {tokens}"
        );
        assert!(
            !tokens.contains("loop_secret"),
            "unread node payload attrs must not serialize into generated Rust: {tokens}"
        );
    }

    #[test]
    fn client_helper_ssr_replays_authored_node_payload_argument() {
        let call = ClientExpr::Call {
            function: ClientFunctionRef::Helper {
                edge: rsjs_ir::abi::HelperCallEdgeId::new(0),
                authored_path: "crate::cell_serial".to_string(),
                name: "cell_serial".to_string(),
            },
            args: vec![ClientExpr::NodePayload {
                attr: "data-rsjs-payload-p0".to_string(),
                tokens: "idx".to_string(),
            }],
        };

        let tokens = emit_client_call_as_rust(&call, &[]).to_string();
        assert!(tokens.contains("crate :: cell_serial"), "{tokens}");
        assert!(tokens.contains("idx"), "{tokens}");
        assert!(!tokens.contains("Default :: default"), "{tokens}");
    }

    #[test]
    fn client_helper_ssr_leaves_contextual_integer_literals_unsuffixed() {
        let call = ClientExpr::Call {
            function: ClientFunctionRef::Helper {
                edge: rsjs_ir::abi::HelperCallEdgeId::new(0),
                authored_path: "crate::wide_identity".to_string(),
                name: "wide_identity".to_string(),
            },
            args: vec![
                ClientExpr::Literal(Value::Int(-7)),
                ClientExpr::Literal(Value::BigInt(9_007_199_254_740_993)),
            ],
        };

        let tokens = emit_client_call_as_rust(&call, &[]).to_string();
        assert!(tokens.contains("- 7"), "{tokens}");
        assert!(tokens.contains("9007199254740993"), "{tokens}");
        assert!(!tokens.contains("i64"), "{tokens}");
        assert!(!tokens.contains("i128"), "{tokens}");
    }

    #[test]
    fn signal_initializer_local_uses_signal_id_index() {
        let island = build_counter_island();
        let reachable = all_signal_ids(&island);
        let signal_lets = emit_signal_initializers(&island, &[], &reachable);
        let s = signal_lets.to_string();
        assert!(
            s.contains("__rsjs_initial_signal_0"),
            "expected an initializer for signal 0, got: {s}"
        );
        assert!(s.contains("0i64"), "initial value must be i64-typed: {s}");
    }

    #[test]
    fn init_state_json_lists_signals_in_declaration_order() {
        let island = build_counter_island();
        let reachable = all_signal_ids(&island);
        let blob = emit_init_state_json(&island, &[], &[], &reachable).let_tokens;
        let s = blob.to_string();
        assert!(
            s.contains(r#""{\"signals\":[0]}""#),
            "expected the JSON blob to enumerate signals in declaration order: {s}"
        );
    }

    #[test]
    fn init_state_json_omits_unreachable_signal_values() {
        let mut b = IslandBuilder::new();
        let dead = b.signal(Value::String("secret".to_string()));
        let live = b.signal(Value::Int(7));
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            dead,
            ClientExpr::string("still-dead"),
        )]));
        let live_read = b.expr(ClientExpr::signal_read(live));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, live_read);
        b.set_root(root);
        let island = b.build();
        let reachable = rsjs_ir::classify_render_strategy(&island).reachable_signal_ids();

        let blob = emit_init_state_json(&island, &[], &[], &reachable)
            .let_tokens
            .to_string();

        assert!(
            blob.contains(r#""{\"signals\":[null,7]}""#),
            "dead signal slot should be a null placeholder and live signal should stay aligned: {blob}"
        );
        assert!(
            !blob.contains("secret") && !blob.contains("still-dead"),
            "unreachable signal values must not enter the seed blob: {blob}"
        );
    }

    #[test]
    fn asset_id_is_lowercased_struct_name_with_disambiguating_hash() {
        // The asset id format is `<lowercased_last_segment>_<hash8>`;
        // see `crate::asset` for the rationale.
        let path: syn::Path = syn::parse_quote!(my_app::Counter);
        let id = crate::asset::derive_asset_id(&path);
        assert!(id.starts_with("counter_"), "got: {id}");

        let path: syn::Path = syn::parse_quote!(SignupForm);
        let id = crate::asset::derive_asset_id(&path);
        assert!(id.starts_with("signupform_"), "got: {id}");
    }

    /// Hand-build the M4 ConditionalCounter island (mirrors
    /// `rsjs_ir/tests/conditional.rs`). Adds a `<p>` whose visibility
    /// is gated on `count.get() > 5`; with `count` initialised to 0
    /// the const-folder decides "hide on first render."
    fn build_conditional_counter_island() -> Island {
        let mut b = IslandBuilder::new();

        let count_signal = b.signal(Value::Int(0));
        let count_read_expr = b.expr(ClientExpr::SignalRead(count_signal));

        let decrement_handler = b.handler_from_ir(signal_update_block(
            count_signal,
            ClientBinaryOp::Sub,
            ClientExpr::int(1),
        ));
        let increment_handler = b.handler_from_ir(signal_update_block(
            count_signal,
            ClientBinaryOp::Add,
            ClientExpr::int(1),
        ));

        let root = b.element("div", None);
        let dec_button = b.element("button", Some(root));
        let span = b.element("span", Some(root));
        let inc_button = b.element("button", Some(root));
        let hint = b.element("p", Some(root));

        b.bind_text(span, count_read_expr);
        b.bind_event(dec_button, EventName::Click, decrement_handler);
        b.bind_event(inc_button, EventName::Click, increment_handler);

        let condition_expr = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::Gt,
            left: Box::new(ClientExpr::SignalRead(count_signal)),
            right: Box::new(ClientExpr::Literal(Value::Int(5))),
        });
        b.conditional_section(root, condition_expr, vec![hint], Vec::new());
        b.set_root(root);

        b.build()
    }

    #[test]
    fn emit_rust_conditional_matches_golden() {
        let island = build_conditional_counter_island();
        let path: syn::Path = syn::parse_quote!(ConditionalCounter);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("golden")
            .join("emit_rust_conditional.golden.rs");

        if std::env::var("RSJS_BLESS_GOLDEN").is_ok() {
            std::fs::create_dir_all(golden_path.parent().unwrap())
                .expect("failed to create tests/ dir for golden");
            std::fs::write(&golden_path, &actual).expect("failed to write golden");
            return;
        }

        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
            panic!(
                "golden file missing at {}; rerun with RSJS_BLESS_GOLDEN=1 to generate",
                golden_path.display()
            )
        });

        assert_eq!(
            actual, expected,
            "emit_rust output drifted from golden; rerun with RSJS_BLESS_GOLDEN=1 to update"
        );
    }

    /// Hand-build the M5 UserForm island (mirrors
    /// `rsjs_ir/tests/user_form.rs`). Two `<input>` elements with
    /// two-way `bind_value` bindings, a submit handler that flips a
    /// `submitting` signal, and two error paragraphs gated by per-
    /// field conditional sections that exercise both predicate
    /// shapes M5 commits to (`ClientExpr::IsNonEmpty` and
    /// `BinaryOp { op: Eq, ... }` against `Value::String("")`).
    fn build_user_form_island() -> Island {
        use rsjs_ir::InputKind;

        let mut b = IslandBuilder::new();

        let username_signal = b.signal(Value::String(String::new()));
        let email_signal = b.signal(Value::String(String::new()));
        let submitting_signal = b.signal(Value::Bool(false));

        let username_invalid_expr = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::Eq,
            left: Box::new(ClientExpr::IsNonEmpty {
                target: Box::new(ClientExpr::SignalRead(username_signal)),
            }),
            right: Box::new(ClientExpr::Literal(Value::Bool(false))),
        });
        let email_invalid_expr = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::Eq,
            left: Box::new(ClientExpr::SignalRead(email_signal)),
            right: Box::new(ClientExpr::Literal(Value::String(String::new()))),
        });

        let submit_handler =
            b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                submitting_signal,
                ClientExpr::bool(true),
            )]));

        let form = b.element("form", None);
        let username_input = b.element("input", Some(form));
        let username_error_wrapper = b.element("div", Some(form));
        let username_error = b.element("p", Some(username_error_wrapper));
        let email_input = b.element("input", Some(form));
        let email_error_wrapper = b.element("div", Some(form));
        let email_error = b.element("p", Some(email_error_wrapper));
        let _submit_button = b.element("button", Some(form));

        b.bind_input(username_input, InputKind::Value, username_signal);
        b.bind_input(email_input, InputKind::Value, email_signal);
        b.bind_event(form, EventName::Submit, submit_handler);
        b.conditional_section(
            username_error_wrapper,
            username_invalid_expr,
            vec![username_error],
            Vec::new(),
        );
        b.conditional_section(
            email_error_wrapper,
            email_invalid_expr,
            vec![email_error],
            Vec::new(),
        );
        b.set_root(form);
        b.build()
    }

    #[test]
    fn emit_rust_user_form_matches_golden() {
        let island = build_user_form_island();
        let path: syn::Path = syn::parse_quote!(UserForm);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("golden")
            .join("emit_rust_user_form.golden.rs");

        if std::env::var("RSJS_BLESS_GOLDEN").is_ok() {
            std::fs::create_dir_all(golden_path.parent().unwrap())
                .expect("failed to create golden dir");
            std::fs::write(&golden_path, &actual).expect("failed to write golden");
            return;
        }

        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
            panic!(
                "golden file missing at {}; rerun with RSJS_BLESS_GOLDEN=1 to generate",
                golden_path.display()
            )
        });

        assert_eq!(
            actual, expected,
            "emit_rust output drifted from golden; rerun with RSJS_BLESS_GOLDEN=1 to update"
        );
    }

    /// Hand-build a tiny async island that exercises the `is_async`
    /// emit path without modelling the await itself (M7+ extends the
    /// IR to track awaits inside the render body; M5 only needs the
    /// structural async-vs-sync flag flipping `WebRender` vs
    /// `WebRenderSync`). The user-visible body conceptually awaits a
    /// username-availability lookup, but the IR is just a single
    /// `<div>{username.get()}</div>` plus the `is_async` flag.
    fn build_availability_probe_island() -> Island {
        let mut b = IslandBuilder::new();
        let username = b.signal(Value::String(String::new()));
        let read = b.expr(ClientExpr::SignalRead(username));
        let root = b.element("div", None);
        b.bind_text(root, read);
        b.set_root(root);
        b.set_async(true);
        b.build()
    }

    #[test]
    fn emit_rust_availability_probe_matches_golden() {
        let island = build_availability_probe_island();
        let path: syn::Path = syn::parse_quote!(AvailabilityProbe);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("golden")
            .join("emit_rust_availability_probe.golden.rs");

        if std::env::var("RSJS_BLESS_GOLDEN").is_ok() {
            std::fs::create_dir_all(golden_path.parent().unwrap())
                .expect("failed to create golden dir");
            std::fs::write(&golden_path, &actual).expect("failed to write golden");
            return;
        }

        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
            panic!(
                "golden file missing at {}; rerun with RSJS_BLESS_GOLDEN=1 to generate",
                golden_path.display()
            )
        });

        assert_eq!(
            actual, expected,
            "emit_rust output drifted from golden; rerun with RSJS_BLESS_GOLDEN=1 to update"
        );
    }

    /// Hand-build a tiny M8 StockTicker island. Mirrors
    /// `rsjs_ir/tests/stock_ticker.rs` minus the v0.1 fixture
    /// scaffolding; this test only exercises the dehydrated
    /// `<script>` blob the server emitter renders.
    fn build_stock_ticker_island() -> Island {
        use rsjs_ir::{AsType, JsonPointer, QueryAccessor, QueryOptions};
        let mut b = IslandBuilder::new();
        let _symbol = b.signal(Value::String("AAPL".to_string()));
        let key = b.expr(ClientExpr::Literal(Value::String("stock:AAPL".to_string())));
        let server = b.expr(ClientExpr::Literal(Value::String(
            r#"{"value":172.5}"#.to_string(),
        )));
        let client = b.expr(ClientExpr::Literal(Value::String(
            "/api/stocks/AAPL".to_string(),
        )));
        let q = b.query(
            key,
            server,
            client,
            QueryOptions {
                interval_ms: Some(5_000),
                refetch_on_focus: true,
                download_progress: false,
                stream_text: false,
                retry_count: 3,
                stale_time_ms: 2_000,
            },
        );
        let price_expr = b.expr(ClientExpr::AsType {
            target: Box::new(ClientExpr::Project {
                target: Box::new(ClientExpr::QueryRead {
                    id: q,
                    accessor: QueryAccessor::Data,
                }),
                path: JsonPointer::new("/value"),
            }),
            ty: AsType::F64,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, price_expr);
        b.set_root(root);
        b.set_async(true);
        b.build()
    }

    #[test]
    fn emit_rust_stock_ticker_emits_dehydrated_query_blob() {
        let island = build_stock_ticker_island();
        let path: syn::Path = syn::parse_quote!(StockTicker);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        // The server-rendered HTML must include a sibling
        // <script type="application/json"
        //         data-rsjs-island-queries="stockticker">
        // blob whose body carries the dehydrated state. Note that
        // prettyplease formats the dashed attribute name with
        // surrounding whitespace because each segment is a Rust
        // ident, not a single token.
        let compact = actual.split_whitespace().collect::<String>();
        assert!(
            compact.contains("data-rsjs-island-queries"),
            "expected dehydrated queries marker; got:\n{actual}"
        );
        // The blob should reference the v0.1 stub literal verbatim
        // (the server initial JSON), embedded in
        // the `let __rsjs_init_query_state_json: &'static str =
        // ...` binding the macro emits.
        assert!(
            actual.contains(r#""{\"0\":{\"data\":{\"value\":172.5},\"data_updated_at\":0}}""#,),
            "expected dehydrated state for query 0; got:\n{actual}"
        );
        // The async (WebRender) impl path was selected.
        assert!(
            actual.contains("WebRender<L, B>"),
            "expected async WebRender impl; got:\n{actual}"
        );
    }

    #[test]
    fn emit_rust_query_with_initial_serializes_server_value_at_render_time() {
        use rsjs_ir::QueryOptions;
        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::Literal(Value::String("stock:AAPL".to_string())));
        let server = b.expr(ClientExpr::Literal(Value::Null));
        let client = b.expr(ClientExpr::Literal(Value::String(
            "/api/stocks/AAPL".to_string(),
        )));
        let q = b.query(key, server, client, QueryOptions::default());
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        let data = b.expr(ClientExpr::QueryRead {
            id: q,
            accessor: QueryAccessor::Data,
        });
        b.bind_text(span, data);
        b.set_root(root);
        b.set_async(true);
        let island = b.build();
        let reachable_query_ids = rsjs_ir::classify_render_strategy(&island).reachable_query_ids();
        let initializers = [QueryInitializer {
            id: q,
            expr: syn::parse_quote!(initial_price),
            key_expr: Some(syn::parse_quote!(("stock", symbol.as_str()))),
        }];

        let value_tokens =
            emit_query_initial_value_locals(&island, &initializers, &reachable_query_ids);
        let tokens = emit_init_query_state_json(
            &island,
            &initializers,
            &reachable_query_ids,
            &format_ident!("cx"),
        );
        let actual = pretty_format(
            quote! { fn f() -> Result<(), ::proa_core::WriteError> { #value_tokens #tokens Ok(()) } },
        );

        assert!(
            actual.contains("::rsjs::__private::serde_json::to_value")
                && actual.contains("let __rsjs_initial_query_typed_0 = initial_price")
                && actual.contains("&__rsjs_initial_query_typed_0"),
            "query_with_initial should first capture the typed server value and serialize it as JSON, got:\n{actual}"
        );
        assert!(
            actual.contains("::rsjs::__private::to_script_json")
                && actual.contains("&__rsjs_initial_query_0"),
            "query_with_initial should serialize the captured JSON value into the query blob, got:\n{actual}"
        );
        assert!(
            actual.contains("__rsjs_init_query_state_json.push_str(\"\\\"0\\\":{\")")
                && actual.contains("__rsjs_init_query_state_json.push_str(\"\\\"key\\\":\")")
                && actual.contains("::rsjs::__private::to_script_json")
                && actual.contains("((\"stock\", symbol.as_str()))")
                && actual.contains("cx.rsjs_query_seed_json"),
            "query-state JSON should include a key locator and page-level seed recording, got:\n{actual}"
        );
    }

    #[test]
    fn emit_rust_skips_query_blob_for_non_query_islands() {
        // The Counter fixture has no queries; its emitted HTML
        // must not carry a data-rsjs-island-queries blob (existing
        // goldens depend on this).
        let island = build_counter_island();
        let path: syn::Path = syn::parse_quote!(Counter);
        let tokens = emit_rust(&island, &path);
        let s = pretty_format(tokens);
        assert!(
            !s.contains("data - rsjs - island - queries"),
            "Counter must not emit a queries blob; got:\n{s}"
        );
        assert!(
            !s.contains("__rsjs_init_query_state_json"),
            "Counter must not bind the queries JSON local; got:\n{s}"
        );
    }

    #[test]
    fn emit_rust_skips_query_mutation_blobs_for_unreachable_declarations() {
        use rsjs_ir::{MutationOptions, QueryOptions};
        let mut b = IslandBuilder::new();
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        b.mutation(
            key_expr,
            url_expr,
            url_expr,
            vec![key_expr],
            MutationOptions::default(),
        );
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(Vec::new()));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);
        b.set_async(true);
        let island = b.build();
        let path: syn::Path = syn::parse_quote!(DeadQueryMutation);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);
        let compact = actual.split_whitespace().collect::<String>();

        assert!(
            compact.contains("rsjs-island") && compact.contains("data-rsjs-handler"),
            "event root should still hydrate; got:\n{actual}"
        );
        assert!(
            !compact.contains("data-rsjs-island-queries")
                && !compact.contains("data-rsjs-island-mutations")
                && !actual.contains("__rsjs_init_query_state_json")
                && !actual.contains("__rsjs_init_mutations_json"),
            "unreachable query/mutation declarations must not emit metadata blobs; got:\n{actual}"
        );
    }

    /// Hand-build the M9 UpdateCart island. Mirrors
    /// `rsjs_ir/tests/cart_mutation.rs` minus the v0.1 fixture
    /// scaffolding; this test only exercises the dehydrated
    /// `<script>` blob the server emitter renders.
    fn build_update_cart_island() -> Island {
        use rsjs_ir::{InvalidationStrategy, MutationOptions, QueryOptions};
        let mut b = IslandBuilder::new();
        let cart = b.signal(Value::String("[]".to_string()));
        let qkey = b.expr(ClientExpr::Literal(Value::String("cart".to_string())));
        let qserver = b.expr(ClientExpr::Literal(Value::String(
            r#"{"items":[]}"#.to_string(),
        )));
        let qclient = b.expr(ClientExpr::Literal(Value::String("/api/cart".to_string())));
        let _qid = b.query(qkey, qserver, qclient, QueryOptions::default());

        let mkey = b.expr(ClientExpr::Literal(Value::String(
            "update_cart".to_string(),
        )));
        let mserver = b.expr(ClientExpr::Literal(Value::Null));
        let mclient = b.expr(ClientExpr::Literal(Value::String(
            r#"{"url":"/api/cart","method":"POST"}"#.to_string(),
        )));
        let inv = b.expr(ClientExpr::Literal(Value::String("cart".to_string())));
        let mid = b.mutation(
            mkey,
            mserver,
            mclient,
            vec![inv],
            MutationOptions {
                retry_count: 1,
                on_success_invalidate_strategy: InvalidationStrategy::Refetch,
            },
        );

        let save_handler =
            b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
                mutation: mid,
                args: vec![ClientExpr::signal_read(cart)],
            }]));

        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        b.bind_event(button, EventName::Click, save_handler);
        b.set_root(root);
        b.set_async(true);
        b.build()
    }

    #[test]
    fn emit_rust_update_cart_emits_dehydrated_mutation_blob() {
        let island = build_update_cart_island();
        let path: syn::Path = syn::parse_quote!(UpdateCart);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        // The server-rendered HTML must include a sibling
        // <script type="application/json"
        //         data-rsjs-island-mutations="updatecart">
        // blob whose body carries the registration metadata.
        // prettyplease formats the dashed attribute name with
        // surrounding whitespace because each segment is a Rust
        // ident.
        assert!(
            actual.contains("data - rsjs - island - mutations"),
            "expected dehydrated mutations marker; got:\n{actual}"
        );
        // The blob carries the configured key, runner, invalidates,
        // and options for mutation 0.
        assert!(
            actual.contains(r#"\"0\":{\"key\":[\"update_cart\"]"#),
            "expected mutation 0 key; got:\n{actual}"
        );
        assert!(
            actual.contains(r#"\"runner\":{\"url\":\"/api/cart\",\"method\":\"POST\"}"#),
            "expected runner config to splice through verbatim; got:\n{actual}"
        );
        assert!(
            actual.contains(r#"\"invalidates\":[[\"cart\"]]"#),
            "expected invalidates list to carry [\"cart\"]; got:\n{actual}"
        );
        assert!(
            actual.contains(r#"\"retry_count\":1"#),
            "expected retry_count=1; got:\n{actual}"
        );
        assert!(
            actual.contains(r#"\"on_success_invalidate_strategy\":\"refetch\""#),
            "expected refetch strategy; got:\n{actual}"
        );
    }

    #[test]
    fn emit_rust_mutation_blob_omits_signal_backed_metadata_fields() {
        use rsjs_ir::{InvalidationStrategy, MutationOptions};
        let mut b = IslandBuilder::new();
        let symbol = b.signal(Value::String("AAPL".to_string()));
        let mkey = b.expr(ClientExpr::QueryKey(vec![
            ClientExpr::Literal(Value::String("update_cart".to_string())),
            ClientExpr::SignalRead(symbol),
        ]));
        let mserver = b.expr(ClientExpr::Literal(Value::Null));
        let mclient = b.expr(ClientExpr::Literal(Value::String("/api/cart".to_string())));
        let inv = b.expr(ClientExpr::QueryKey(vec![
            ClientExpr::Literal(Value::String("stock".to_string())),
            ClientExpr::SignalRead(symbol),
        ]));
        let mutation = b.mutation(
            mkey,
            mserver,
            mclient,
            vec![inv],
            MutationOptions {
                retry_count: 0,
                on_success_invalidate_strategy: InvalidationStrategy::Reset,
            },
        );
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
            mutation,
            args: Vec::new(),
        }]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);
        b.set_async(true);
        let island = b.build();
        let path: syn::Path = syn::parse_quote!(DynamicMutation);
        let tokens = emit_rust(&island, &path);
        let actual = pretty_format(tokens);

        assert!(
            actual.contains("__rsjs_init_mutations_json"),
            "expected mutation metadata blob; got:\n{actual}"
        );
        assert!(
            actual.contains(r#"\"0\":{\"runner\":\"/api/cart\",\"options\""#),
            "signal-backed metadata should keep static runner/options; got:\n{actual}"
        );
        assert!(
            !actual.contains(r#"\"key\":"#) && !actual.contains(r#"\"invalidates\":"#),
            "signal-backed key/invalidates must stay in generated JS, got:\n{actual}"
        );
    }

    #[test]
    fn emit_rust_skips_mutation_blob_for_non_mutation_islands() {
        // Counter has no mutations; its emitted HTML must not carry
        // a data-rsjs-island-mutations blob.
        let island = build_counter_island();
        let path: syn::Path = syn::parse_quote!(Counter);
        let tokens = emit_rust(&island, &path);
        let s = pretty_format(tokens);
        assert!(
            !s.contains("data - rsjs - island - mutations"),
            "Counter must not emit a mutations blob; got:\n{s}"
        );
        assert!(
            !s.contains("__rsjs_init_mutations_json"),
            "Counter must not bind the mutations JSON local; got:\n{s}"
        );
    }

    #[test]
    fn evaluate_const_folds_count_initial_zero_against_threshold() {
        let island = build_conditional_counter_island();
        // The conditional section is keyed by the root NodeId; pull
        // its predicate out and fold it.
        let root = island.view_tree.root().expect("root");
        let section = island
            .bindings
            .conditional_sections
            .get(&root)
            .and_then(|sections| sections.first())
            .expect("conditional section");
        let predicate = island.exprs.client_expr(section.condition);
        assert_eq!(
            evaluate_const(predicate, &island, &[]),
            Some(Value::Bool(false)),
            "0 > 5 must fold to false, hiding the then-branch on first SSR"
        );
    }

    /// Hyphenated child tag names (HTML custom elements like
    /// `<my-widget>`) are valid in the IR view tree because the
    /// `proa_dsl` parser was extended to accept them. Prior to this
    /// fix, `emit_view_node` called `format_ident!("{}", tag)` directly
    /// which panics on hyphens at proc-macro expansion time. The
    /// emitter must route hyphenated child tags through
    /// `tag_name_tokens` so the output is the `Ident - Ident` token
    /// sequence `html_sync!` accepts.
    #[test]
    fn emit_rust_handles_hyphenated_child_tag() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let _custom_child = b.element("my-widget", Some(root));
        b.set_root(root);
        let island = b.build();

        let path: syn::Path = syn::parse_quote!(WithCustomChild);
        // Must not panic at expansion time.
        let tokens = emit_rust(&island, &path);
        let rendered = tokens.to_string();
        // The output should reference the hyphenated tag in some form
        // (either as hyphen-separated idents or otherwise textually
        // present). What matters most is that we got here at all
        // without a `format_ident!` panic.
        assert!(
            rendered.contains("my") && rendered.contains("widget"),
            "expected emitted tokens to mention the hyphenated tag parts; got: {rendered}"
        );
        // The token stream must still parse as a valid Rust file so
        // downstream `html_sync!` consumption works.
        let _file: syn::File = syn::parse2(tokens)
            .expect("emit_rust output with hyphenated child tag must parse as syn::File");
    }

    /// Build a tiny island whose sole signal is initialised to a
    /// caller-supplied string. Used by the JSON-escape XSS tests
    /// below; the shape is otherwise irrelevant.
    fn build_island_with_string_signal(initial: &str) -> Island {
        let mut b = IslandBuilder::new();
        let sig = b.signal(Value::String(initial.to_string()));
        let read = b.expr(ClientExpr::SignalRead(sig));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, read);
        b.set_root(root);
        b.build()
    }

    /// XSS regression: a signal initialised to a string containing
    /// `</script>` must not surface as a literal `</script>` in the
    /// emitted dehydrated state blob, or the user payload would
    /// break out of the `<script type="application/json">` tag.
    ///
    /// The check exercises the JSON blob specifically (other parts
    /// of the emitted token stream, e.g. the per-signal initial-
    /// value Rust local, are escaped separately by `html_sync!`'s
    /// standard HTML-text path).
    #[test]
    fn init_state_json_escapes_script_tag_breakout() {
        let payload = "</script><script>alert(1)</script>";
        let island = build_island_with_string_signal(payload);
        let reachable = all_signal_ids(&island);
        let blob = emit_init_state_json(&island, &[], &[], &reachable)
            .let_tokens
            .to_string();

        // The blob is a Rust `let` binding with the JSON value as a
        // string literal: in tokenised form `<` appears as the
        // doubled-backslash escape `\\u003c`.
        assert!(
            !blob.contains("</script>"),
            "user payload `</script>` must be escaped before reaching the dehydrated blob; got:\n{blob}"
        );
        assert!(
            blob.contains("\\u003c"),
            "expected `<` to be encoded as \\u003c in the dehydrated blob; got:\n{blob}"
        );
    }

    /// JSON-validity regression: control characters must be escaped
    /// using JSON's standard backslash sequences (or `\u00XX` for the
    /// rest), not spliced through as raw bytes. The prior table only
    /// handled `"` and `\`, which produced invalid JSON for newlines.
    #[test]
    fn init_state_json_escapes_control_characters() {
        let payload = "hi\nworld\twith\u{0001}control";
        let island = build_island_with_string_signal(payload);
        let reachable = all_signal_ids(&island);
        let blob = emit_init_state_json(&island, &[], &[], &reachable)
            .let_tokens
            .to_string();

        // Each control character should appear as the doubled-
        // backslash form because the JSON literal sits inside a Rust
        // string literal in the emitted token stream.
        assert!(
            blob.contains("\\n"),
            "expected `\\n` JSON escape for newline; got:\n{blob}"
        );
        assert!(
            blob.contains("\\t"),
            "expected `\\t` JSON escape for tab; got:\n{blob}"
        );
        assert!(
            blob.contains("\\u0001"),
            "expected `\\u0001` escape for U+0001; got:\n{blob}"
        );
    }
}
