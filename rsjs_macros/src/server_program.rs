// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Site-local lowering for an analyzed component's authored server program.
//!
//! The component attribute owns browser IR and hydration markup; it does not
//! own the surrounding Rust method. This module keeps the authored
//! `WebRenderSync` method (or the authored pending `WebRender` future), replaces
//! compiler-owned declarations where they originally appeared, and swaps only
//! the final `html_sync!` / `html!` invocation. Ordinary statements, `?`,
//! returns, drop scopes, the author's render call, and async poll boundaries
//! therefore remain ordinary Rust.

use std::collections::{BTreeSet, HashMap, HashSet};

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use rsjs_ir::{Island, MutationId, NodeId, NodeRefId, QueryId, SharedStoreId, SignalId};
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{Block, Expr, ExprAsync, ImplItem, ItemImpl, Macro, Stmt};

use crate::emit_rust::{
    self, MutationInitializer, QueryInitializer, SharedStateInitializer, SignalInitializer,
};
use crate::final_plan::{
    FinalCallbackShape, FinalComponentSelection, FinalLiveCallback, FinalLivePropUse,
    FinalOwnerActivationStrategy, FinalOwnerRole, FinalTemporalPropMode,
};
use crate::parse::{ParsedIsland, RenderKind};

/// Validate the site-local rewrite before emission.
///
/// Opaque server children deliberately have `0..N` element cardinality. They
/// are safe in hydrated owners because `walk_path` verifies its fast index
/// result and falls back to an owner-scoped marker lookup on mismatch.
pub(crate) fn validate_lossless_rewrite(
    _island: &Island,
    _needs_client_js: bool,
) -> syn::Result<()> {
    Ok(())
}

/// Rewrite one explicit render-trait impl without reconstructing its method.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_lossless_impl(
    authored: &ItemImpl,
    parsed: &ParsedIsland,
    island: &Island,
    asset_id: &str,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    query_initializers: &[QueryInitializer],
    mutation_initializers: &[MutationInitializer],
    prelude_signal_ids: &HashMap<usize, SignalId>,
    prelude_shared_store_ids: &HashMap<usize, SharedStoreId>,
    prelude_query_ids: &HashMap<usize, QueryId>,
    prelude_mutation_ids: &HashMap<usize, MutationId>,
    prelude_node_ref_ids: &HashMap<usize, NodeRefId>,
    client_prelude_local_indices: &HashSet<usize>,
    query_source_ide_hints: &HashMap<QueryId, TokenStream>,
    ide_helper_hints: &TokenStream,
    ide_source_hints: &TokenStream,
    server_expressions: &HashMap<NodeId, Expr>,
    direct_component_site_nodes: &HashMap<NodeId, u32>,
    shared_state_provider_sites: &[crate::analyze::DirectSharedStateProviderSite],
    direct_component_server_signal_ids: &HashSet<SignalId>,
    direct_component_server_node_ref_ids: &HashSet<NodeRefId>,
    final_component_selection: Option<&FinalComponentSelection>,
) -> syn::Result<TokenStream> {
    let mut item = authored.clone();
    let item_span = item.span();
    let linked_site_ident =
        final_component_selection.map(|_| format_ident!("__rsjs_linked_render_site"));
    if let Some(selection) = final_component_selection {
        validate_linked_shared_state_provider_sites(selection, shared_state_provider_sites)?;
    }
    let render = item
        .items
        .iter_mut()
        .find_map(|item| match item {
            ImplItem::Fn(method) if method.sig.ident == "render" => Some(method),
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new(
                item_span,
                "rsjs internal error: the validated render method disappeared \
                 while preserving the server program",
            )
        })?;

    let assertion = render_trait_identity_assertion(parsed, final_component_selection)?;

    let server_block = match parsed.render_kind {
        RenderKind::Sync => &mut render.block,
        RenderKind::Async => &mut locate_pending_async_body_mut(&mut render.block)?.block,
    };
    rewrite_server_block(
        server_block,
        parsed,
        island,
        asset_id,
        signal_initializers,
        shared_state_initializers,
        query_initializers,
        mutation_initializers,
        prelude_signal_ids,
        prelude_shared_store_ids,
        prelude_query_ids,
        prelude_mutation_ids,
        prelude_node_ref_ids,
        client_prelude_local_indices,
        query_source_ide_hints,
        ide_helper_hints,
        ide_source_hints,
        server_expressions,
        direct_component_site_nodes,
        shared_state_provider_sites,
        direct_component_server_signal_ids,
        direct_component_server_node_ref_ids,
        linked_site_ident.as_ref(),
    )?;
    render.block.stmts.insert(0, assertion);

    let Some(selection) = final_component_selection else {
        return Ok(item.into_token_stream());
    };
    emit_linked_component_impls(authored, item, parsed, selection)
}

fn validate_linked_shared_state_provider_sites(
    selection: &FinalComponentSelection,
    provider_sites: &[crate::analyze::DirectSharedStateProviderSite],
) -> syn::Result<()> {
    let expected = provider_sites
        .iter()
        .map(|provider| provider.id.index())
        .collect::<BTreeSet<_>>();
    for site in &selection.sites {
        let actual = selection
            .shared_state_providers_by_host
            .get(&site.candidate_id)
            .into_iter()
            .flatten()
            .map(|provider| provider.provider_site_id)
            .collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "RSJS candidate {} has SharedState provider sites {actual:?}, but its authenticated server program declares {expected:?}",
                    site.candidate_id,
                ),
            ));
        }
    }
    Ok(())
}

fn render_trait_identity_assertion(
    parsed: &ParsedIsland,
    final_component_selection: Option<&FinalComponentSelection>,
) -> syn::Result<Stmt> {
    let trait_path = &parsed.trait_path;
    // Trust chain:
    // 1. descriptor compilation emits and type-checks the authored impl, so
    //    `Self` was already proven to implement the authored render trait;
    // 2. final linking may intentionally withhold that public impl when a
    //    required immutable slot exists only at an exact incoming site;
    // 3. in that hidden-only final form, framework-owned `()` proves that the
    //    unchanged authored path (including aliases and generic arguments)
    //    still resolves to Proa's canonical trait, while the exact-site trait
    //    is the sole implementation carrying the authored body.
    //
    // `()` implements both canonical render traits for every buffer/loader.
    let render_trait_witness_receiver = match final_component_selection {
        Some(selection) if selection.standalone_candidate_id.is_none() => quote!(()),
        _ => quote!(Self),
    };
    match parsed.render_kind {
        RenderKind::Sync => syn::parse2(quote! {
            let _: &::proa_core::render::web_render::__private::SyncRenderTraitWitness =
                &<#render_trait_witness_receiver as #trait_path>::__PROA_RENDER_TRAIT_WITNESS;
        }),
        RenderKind::Async => syn::parse2(quote! {
            let _: &::proa_core::render::web_render::__private::AsyncRenderTraitWitness =
                &<#render_trait_witness_receiver as #trait_path>::__PROA_RENDER_TRAIT_WITNESS;
        }),
    }
}

/// Factor a final-plan component into a compiler-only entry that receives an
/// exact occurrence plan and, when allowed by the descriptor, an ordinary
/// standalone render entry.
///
/// The authored body lives only in the hidden impl. For a root-eligible
/// component, the ordinary Proa render trait delegates directly to its
/// standalone occurrence. A component with a required immutable static slot
/// deliberately has no ordinary render impl in the final program: it can only
/// be reached through a linker-validated direct child expression wrapped by
/// `LinkedComponent<T>`. No render mode is stored in `WebContext`, and the
/// asynchronous form returns the hidden RPITIT directly.
fn emit_linked_component_impls(
    authored: &ItemImpl,
    mut hidden: ItemImpl,
    parsed: &ParsedIsland,
    selection: &FinalComponentSelection,
) -> syn::Result<TokenStream> {
    let hidden_trait_path = linked_render_trait_path(parsed)?;
    let Some((_, hidden_path, _)) = hidden.trait_.as_mut() else {
        return Err(syn::Error::new(
            hidden.span(),
            "rsjs internal error: a linked component lost its render trait",
        ));
    };
    *hidden_path = hidden_trait_path.clone();

    let hidden_render = find_render_method_mut(&mut hidden)?;
    hidden_render.sig.ident = format_ident!("render_at");
    hidden_render.sig.inputs.push(syn::parse_quote! {
        __rsjs_linked_render_site:
            &'static ::rsjs::__private::LinkedRenderSitePlan
    });
    hidden
        .items
        .push(ImplItem::Fn(linked_site_lookup_method(selection)?));

    let Some(standalone_candidate_id) = selection.standalone_candidate_id else {
        return Ok(quote! {
            #hidden
        });
    };

    let mut delegate = authored.clone();
    let delegate_render = find_render_method_mut(&mut delegate)?;
    let identity = linked_runtime_identity(selection, standalone_candidate_id)?;
    let standalone_candidate_id = identity.candidate_id;
    let scope_id = byte_array_tokens(identity.scope_id);
    let context_ident = &parsed.context_ident;
    delegate_render.block = syn::parse2(quote! {{
        let __rsjs_linked_render_site =
            <Self as #hidden_trait_path>::linked_site(
                #standalone_candidate_id,
                #scope_id,
            );
        <Self as #hidden_trait_path>::render_at(
            self,
            #context_ident,
            __rsjs_linked_render_site,
        )
    }})?;

    Ok(quote! {
        #delegate
        #hidden
    })
}

fn find_render_method_mut(item: &mut ItemImpl) -> syn::Result<&mut syn::ImplItemFn> {
    let item_span = item.span();
    item.items
        .iter_mut()
        .find_map(|item| match item {
            ImplItem::Fn(method)
                if method.sig.ident == "render" || method.sig.ident == "render_at" =>
            {
                Some(method)
            }
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new(
                item_span,
                "rsjs internal error: a linked component lost its render method",
            )
        })
}

fn linked_render_trait_path(parsed: &ParsedIsland) -> syn::Result<syn::Path> {
    let arguments = parsed
        .trait_path
        .segments
        .last()
        .ok_or_else(|| {
            syn::Error::new(
                parsed.trait_path.span(),
                "rsjs internal error: the authored render trait path is empty",
            )
        })?
        .arguments
        .clone();
    let mut path: syn::Path = match parsed.render_kind {
        RenderKind::Sync => syn::parse_quote!(::rsjs::__private::ComponentSiteRenderSync),
        RenderKind::Async => syn::parse_quote!(::rsjs::__private::ComponentSiteRender),
    };
    path.segments
        .last_mut()
        .expect("compiler-authored linked render trait path has one segment")
        .arguments = arguments;
    Ok(path)
}

fn linked_site_lookup_method(selection: &FinalComponentSelection) -> syn::Result<syn::ImplItemFn> {
    let mut sites = selection.sites.iter().collect::<Vec<_>>();
    sites.sort_by_key(|site| site.candidate_id);
    let site_candidates = sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    for candidate_id in selection
        .live_props_by_child
        .keys()
        .chain(selection.live_callbacks_by_child.keys())
    {
        if !site_candidates.contains(candidate_id) {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "rsjs internal error: final live-input plan names candidate {candidate_id}, \
                     but that occurrence does not belong to this component definition"
                ),
            ));
        }
    }

    let mut declarations = Vec::with_capacity(sites.len() * 4);
    let mut match_arms = Vec::with_capacity(sites.len());
    for site in sites {
        let candidate_id = site.candidate_id;
        let identity = linked_runtime_identity(selection, candidate_id)?;
        let runtime_candidate_id = identity.candidate_id;
        let scope_id = byte_array_tokens(identity.scope_id);
        let children_ident = format_ident!("__RSJS_LINKED_CHILDREN_{runtime_candidate_id}");
        let live_props_ident = format_ident!("__RSJS_LINKED_LIVE_PROPS_{runtime_candidate_id}");
        let live_callbacks_ident =
            format_ident!("__RSJS_LINKED_LIVE_CALLBACKS_{runtime_candidate_id}");
        let shared_state_ident =
            format_ident!("__RSJS_LINKED_SHARED_STATE_REQUIREMENTS_{runtime_candidate_id}");
        let site_ident = format_ident!("__RSJS_LINKED_SITE_{runtime_candidate_id}");
        let children = selection
            .child_sites
            .iter()
            .filter_map(|(&(parent_candidate_id, edge_id), &child_candidate_id)| {
                (parent_candidate_id == candidate_id).then_some((edge_id, child_candidate_id))
            })
            .map(|(edge_id, child_candidate_id)| {
                let child_identity = linked_runtime_identity(selection, child_candidate_id)?;
                let child_candidate_id = child_identity.candidate_id;
                let child_scope_id = byte_array_tokens(child_identity.scope_id);
                Ok(quote! {
                    ::rsjs::__private::LinkedChildSite::new(
                        #edge_id,
                        #child_scope_id,
                        #child_candidate_id,
                    )
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let child_count = children.len();
        let mut live_props = selection
            .live_props_by_child
            .get(&candidate_id)
            .cloned()
            .unwrap_or_default();
        live_props.sort_unstable();
        for record in &live_props {
            if record.child_candidate_id != candidate_id {
                return Err(linked_input_candidate_error(
                    "live prop",
                    candidate_id,
                    record.child_candidate_id,
                ));
            }
        }
        let live_props = live_props
            .iter()
            .map(|record| linked_live_prop_tokens(selection, record))
            .collect::<syn::Result<Vec<_>>>()?;
        let live_prop_count = live_props.len();

        let mut live_callbacks = selection
            .live_callbacks_by_child
            .get(&candidate_id)
            .cloned()
            .unwrap_or_default();
        live_callbacks.sort_unstable();
        for record in &live_callbacks {
            if record.child_candidate_id != candidate_id {
                return Err(linked_input_candidate_error(
                    "live callback",
                    candidate_id,
                    record.child_candidate_id,
                ));
            }
        }
        let live_callbacks = live_callbacks
            .iter()
            .map(|record| linked_live_callback_tokens(selection, record))
            .collect::<syn::Result<Vec<_>>>()?;
        let live_callback_count = live_callbacks.len();
        let mut shared_state_requirements = selection
            .shared_state_requirements_by_consumer
            .get(&candidate_id)
            .cloned()
            .unwrap_or_default();
        shared_state_requirements.sort_by_key(|requirement| requirement.reference_id);
        for requirement in &shared_state_requirements {
            if requirement.consumer_candidate_id != candidate_id {
                return Err(linked_input_candidate_error(
                    "SharedState requirement",
                    candidate_id,
                    requirement.consumer_candidate_id,
                ));
            }
        }
        let shared_state_requirements = shared_state_requirements
            .iter()
            .map(|requirement| {
                let reference_id = requirement.reference_id;
                let provider_identity =
                    linked_runtime_identity(selection, requirement.provider_host_candidate_id)?;
                let provider_host_candidate_id = provider_identity.candidate_id;
                let provider_scope_id = byte_array_tokens(provider_identity.scope_id);
                let provider_site_id = requirement.provider_site_id;
                Ok(quote! {
                    ::rsjs::__private::LinkedSharedStateRequirement::new(
                        #reference_id,
                        #provider_scope_id,
                        #provider_host_candidate_id,
                        #provider_site_id,
                    )
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let shared_state_requirement_count = shared_state_requirements.len();

        let (owner_id, owner_root_candidate_id) = match site.owner_role {
            FinalOwnerRole::ServerOnly => (
                quote!(::core::option::Option::None),
                quote!(::core::option::Option::None),
            ),
            FinalOwnerRole::Root { .. } => (
                quote!(::core::option::Option::Some(#runtime_candidate_id)),
                quote!(::core::option::Option::Some(#runtime_candidate_id)),
            ),
            FinalOwnerRole::Member {
                root_candidate_id, ..
            } => {
                let root_id = linked_runtime_identity(selection, root_candidate_id)?.candidate_id;
                (
                    quote!(::core::option::Option::Some(#root_id)),
                    quote!(::core::option::Option::Some(#root_id)),
                )
            }
        };
        let owner_strategy = match (&site.owner_role, &site.owner_strategy) {
            (FinalOwnerRole::ServerOnly, None) => quote!(::core::option::Option::None),
            (FinalOwnerRole::Root { .. } | FinalOwnerRole::Member { .. }, Some(strategy)) => {
                let strategy = linked_owner_strategy_tokens(strategy);
                quote!(::core::option::Option::Some(#strategy))
            }
            (FinalOwnerRole::ServerOnly, Some(_)) => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "server-only RSJS candidate {candidate_id} unexpectedly carries an owner activation strategy"
                    ),
                ));
            }
            (FinalOwnerRole::Root { .. } | FinalOwnerRole::Member { .. }, None) => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "owned RSJS candidate {candidate_id} is missing its linked activation strategy"
                    ),
                ));
            }
        };
        let (owner_asset_id, seed_key) =
            match (&site.owner_role, &site.owner_asset_id, &site.seed_key) {
                (FinalOwnerRole::ServerOnly, None, None) => (
                    quote!(::core::option::Option::None),
                    quote!(::core::option::Option::None),
                ),
                (
                    FinalOwnerRole::Root { .. } | FinalOwnerRole::Member { .. },
                    Some(owner_asset_id),
                    Some(seed_key),
                ) => {
                    let owner_asset_id =
                        syn::LitStr::new(owner_asset_id, proc_macro2::Span::call_site());
                    let seed_key = syn::LitStr::new(seed_key, proc_macro2::Span::call_site());
                    (
                        quote!(::core::option::Option::Some(#owner_asset_id)),
                        quote!(::core::option::Option::Some(#seed_key)),
                    )
                }
                _ => {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                        "RSJS candidate {candidate_id} has inconsistent owner asset/seed metadata"
                    ),
                    ));
                }
            };
        if site.fragment_asset_id.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("RSJS candidate {candidate_id} has an empty fragment asset ID"),
            ));
        }
        let fragment_asset_id =
            syn::LitStr::new(&site.fragment_asset_id, proc_macro2::Span::call_site());
        declarations.push(quote! {
            static #children_ident:
                [::rsjs::__private::LinkedChildSite; #child_count] =
                [#(#children),*];
            static #live_props_ident:
                [::rsjs::__private::LinkedLiveProp; #live_prop_count] =
                [#(#live_props),*];
            static #live_callbacks_ident:
                [::rsjs::__private::LinkedLiveCallback; #live_callback_count] =
                [#(#live_callbacks),*];
            static #shared_state_ident:
                [::rsjs::__private::LinkedSharedStateRequirement; #shared_state_requirement_count] =
                [#(#shared_state_requirements),*];
            static #site_ident: ::rsjs::__private::LinkedRenderSitePlan =
                ::rsjs::__private::LinkedRenderSitePlan::new_with_shared_state(
                    #scope_id,
                    #runtime_candidate_id,
                    #owner_id,
                    #owner_root_candidate_id,
                    #owner_strategy,
                    #owner_asset_id,
                    #seed_key,
                    #fragment_asset_id,
                    &#children_ident,
                    &#live_props_ident,
                    &#live_callbacks_ident,
                    &#shared_state_ident,
                );
        });
        match_arms.push(quote!(#runtime_candidate_id => &#site_ident));
    }

    syn::parse2(quote! {
        fn linked_site(
            candidate_id: u64,
            expected_scope_id: [u8; 32],
        ) -> &'static ::rsjs::__private::LinkedRenderSitePlan {
            #(#declarations)*
            let site = match candidate_id {
                #(#match_arms,)*
                _ => panic!(
                    "RSJS linked render plan has no occurrence {candidate_id} for this component",
                ),
            };
            assert!(
                expected_scope_id == site.scope_id,
                "RSJS linked render-site lookup used a foreign render-root scope",
            );
            site
        }
    })
}

fn linked_runtime_identity(
    selection: &FinalComponentSelection,
    candidate: u32,
) -> syn::Result<crate::final_plan::FinalRuntimeSiteIdentity> {
    selection
        .runtime_site(candidate)
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))
}

fn linked_owner_strategy_tokens(strategy: &FinalOwnerActivationStrategy) -> TokenStream {
    match strategy {
        FinalOwnerActivationStrategy::Eager => {
            quote!(::rsjs::__private::LinkedOwnerActivationStrategy::Eager)
        }
        FinalOwnerActivationStrategy::Visible => {
            quote!(::rsjs::__private::LinkedOwnerActivationStrategy::Visible)
        }
        FinalOwnerActivationStrategy::Idle => {
            quote!(::rsjs::__private::LinkedOwnerActivationStrategy::Idle)
        }
        FinalOwnerActivationStrategy::Interaction { event } => match event {
            Some(event) => quote!(
                ::rsjs::__private::LinkedOwnerActivationStrategy::Interaction {
                    event: ::core::option::Option::Some(#event),
                }
            ),
            None => quote!(
                ::rsjs::__private::LinkedOwnerActivationStrategy::Interaction {
                    event: ::core::option::Option::None,
                }
            ),
        },
        FinalOwnerActivationStrategy::Media { query } => quote!(
            ::rsjs::__private::LinkedOwnerActivationStrategy::Media {
                query: #query,
            }
        ),
    }
}

fn linked_input_candidate_error(
    kind: &str,
    map_candidate_id: u32,
    record_candidate_id: u32,
) -> syn::Error {
    syn::Error::new(
        proc_macro2::Span::call_site(),
        format!(
            "rsjs internal error: final {kind} record for candidate {map_candidate_id} \
             names child candidate {record_candidate_id}"
        ),
    )
}

fn linked_live_prop_tokens(
    selection: &FinalComponentSelection,
    record: &FinalLivePropUse,
) -> syn::Result<TokenStream> {
    let parent_candidate_id =
        linked_runtime_identity(selection, record.parent_candidate_id)?.candidate_id;
    let edge_id = record.edge_id;
    let child_candidate_id =
        linked_runtime_identity(selection, record.child_candidate_id)?.candidate_id;
    let field_id = record.field_id;
    let crossing_hook_id = record.crossing_hook_id;
    let parent_value_id = record.parent_value_id;
    let child_value_id = record.child_value_id;
    let prop_site_id = record.prop_site_id;
    let temporal = match record.temporal {
        FinalTemporalPropMode::LiveRead => {
            quote!(::rsjs::__private::LinkedTemporalPropMode::LiveRead)
        }
        FinalTemporalPropMode::SeedOnce { seed_site_id } => quote! {
            ::rsjs::__private::LinkedTemporalPropMode::SeedOnce {
                seed_site_id: #seed_site_id,
            }
        },
        FinalTemporalPropMode::EffectDependency => {
            quote!(::rsjs::__private::LinkedTemporalPropMode::EffectDependency)
        }
        FinalTemporalPropMode::QueryKeyDependency => {
            quote!(::rsjs::__private::LinkedTemporalPropMode::QueryKeyDependency)
        }
    };
    Ok(quote! {
        ::rsjs::__private::LinkedLiveProp::new(
            #parent_candidate_id,
            #edge_id,
            #child_candidate_id,
            #field_id,
            #crossing_hook_id,
            #parent_value_id,
            #child_value_id,
            #prop_site_id,
            #temporal,
        )
    })
}

fn linked_live_callback_tokens(
    selection: &FinalComponentSelection,
    record: &FinalLiveCallback,
) -> syn::Result<TokenStream> {
    let parent_candidate_id =
        linked_runtime_identity(selection, record.parent_candidate_id)?.candidate_id;
    let edge_id = record.edge_id;
    let child_candidate_id =
        linked_runtime_identity(selection, record.child_candidate_id)?.candidate_id;
    let field_id = record.field_id;
    let crossing_hook_id = record.crossing_hook_id;
    let parent_value_id = record.parent_value_id;
    let child_value_id = record.child_value_id;
    // Required and optional callback containers share the same invocation-time
    // runtime dispatch. The exact shape remains part of the linked plan and is
    // validated before this codegen boundary.
    match record.shape {
        FinalCallbackShape::Required | FinalCallbackShape::Optional => {}
    }
    Ok(quote! {
        ::rsjs::__private::LinkedLiveCallback::new(
            #parent_candidate_id,
            #edge_id,
            #child_candidate_id,
            #field_id,
            #crossing_hook_id,
            #parent_value_id,
            #child_value_id,
            ::rsjs::__private::LinkedCallbackDispatch::ResolveCurrent,
        )
    })
}

fn byte_array_tokens(bytes: [u8; 32]) -> TokenStream {
    let bytes = bytes
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    quote!([#(#bytes),*])
}

#[allow(clippy::too_many_arguments)]
fn rewrite_server_block(
    block: &mut Block,
    parsed: &ParsedIsland,
    island: &Island,
    asset_id: &str,
    signal_initializers: &[SignalInitializer],
    shared_state_initializers: &[SharedStateInitializer],
    query_initializers: &[QueryInitializer],
    mutation_initializers: &[MutationInitializer],
    prelude_signal_ids: &HashMap<usize, SignalId>,
    prelude_shared_store_ids: &HashMap<usize, SharedStoreId>,
    prelude_query_ids: &HashMap<usize, QueryId>,
    prelude_mutation_ids: &HashMap<usize, MutationId>,
    prelude_node_ref_ids: &HashMap<usize, NodeRefId>,
    client_prelude_local_indices: &HashSet<usize>,
    query_source_ide_hints: &HashMap<QueryId, TokenStream>,
    ide_helper_hints: &TokenStream,
    ide_source_hints: &TokenStream,
    server_expressions: &HashMap<NodeId, Expr>,
    direct_component_site_nodes: &HashMap<NodeId, u32>,
    shared_state_provider_sites: &[crate::analyze::DirectSharedStateProviderSite],
    direct_component_server_signal_ids: &HashSet<SignalId>,
    direct_component_server_node_ref_ids: &HashSet<NodeRefId>,
    linked_site_ident: Option<&syn::Ident>,
) -> syn::Result<()> {
    let mut authored = std::mem::take(&mut block.stmts);
    let mut tail = authored.pop().ok_or_else(|| {
        syn::Error::new(
            block.span(),
            "rsjs internal error: the validated render site disappeared during lossless rewriting",
        )
    })?;

    let mut required_signal_ids = emit_rust::lossless_rust_signal_ids(island);
    for signal in direct_component_server_signal_ids {
        if !required_signal_ids.contains(signal) {
            required_signal_ids.push(*signal);
        }
    }
    required_signal_ids.sort_by_key(|signal| signal.0);
    let strategy = rsjs_ir::classify_render_strategy(island);
    let reachable_query_ids = strategy.reachable_query_ids();
    let reachable_mutation_ids = strategy.reachable_mutation_ids();
    let mut prepared_signal_ids = HashSet::new();
    let mut prepared_shared_store_ids = HashSet::new();
    let mut prepared_query_ids = HashSet::new();
    let mut prepared_mutation_ids = HashSet::new();
    let mut rewritten = Vec::with_capacity(authored.len() + 16);
    let mut local_index = 0_usize;
    let authored_signal_ids = prelude_signal_ids.values().copied().collect::<HashSet<_>>();

    for statement in authored {
        match statement {
            Stmt::Local(local) => {
                let index = local_index;
                local_index += 1;

                if let Some(id) = prelude_shared_store_ids.get(&index).copied() {
                    let initializer = shared_state_initializers
                        .iter()
                        .find(|initializer| initializer.id == id)
                        .ok_or_else(|| {
                            syn::Error::new(
                                local.span(),
                                "rsjs internal error: a shared-state source declaration lost its initializer",
                            )
                        })?;
                    let tokens = emit_rust::emit_lossless_shared_state_declaration(
                        island,
                        initializer,
                        id,
                        asset_id,
                        &parsed.context_ident,
                        linked_site_ident,
                    );
                    append_tokens_as_statements(&mut rewritten, tokens)?;
                    prepared_shared_store_ids.insert(id);
                    continue;
                }

                if let Some(id) = prelude_signal_ids.get(&index).copied() {
                    // A memo declaration can read a synthetic inbound or
                    // SharedState field that has no authored `let` of its own.
                    // Materialize those source values immediately before the
                    // memo value, preserving Rust's lexical order while
                    // ensuring the derived initializer never references a
                    // tail-emitted local that is not in scope yet.
                    if let Some(derived) = island
                        .signals
                        .iter()
                        .find(|signal| signal.id == id)
                        .and_then(|signal| signal.derived)
                    {
                        for dependency in rsjs_ir::analysis::collect_deps(&island.exprs, derived) {
                            if dependency != id
                                && required_signal_ids.contains(&dependency)
                                && prepared_signal_ids.insert(dependency)
                            {
                                let tokens = emit_rust::emit_lossless_signal_declaration(
                                    island,
                                    signal_initializers,
                                    dependency,
                                    &required_signal_ids,
                                );
                                append_tokens_as_statements(&mut rewritten, tokens)?;
                            }
                        }
                    }
                    let tokens = emit_rust::emit_lossless_signal_declaration(
                        island,
                        signal_initializers,
                        id,
                        &required_signal_ids,
                    );
                    append_tokens_as_statements(&mut rewritten, tokens)?;
                    prepared_signal_ids.insert(id);
                    continue;
                }

                if let Some(id) = prelude_query_ids.get(&index).copied() {
                    // A query key may read `self.<field>` directly. That read
                    // owns a synthetic inbound signal but has no authored
                    // `let signal(...)` statement at which the lossless
                    // rewriter could otherwise prepare its server value.
                    // Materialize every still-unprepared key dependency at
                    // the query declaration before serializing the key.
                    if let Some(query) = island.queries.iter().find(|query| query.id == id) {
                        for signal in rsjs_ir::analysis::collect_deps(&island.exprs, query.key_expr)
                        {
                            if required_signal_ids.contains(&signal)
                                && prepared_signal_ids.insert(signal)
                            {
                                let tokens = emit_rust::emit_lossless_signal_declaration(
                                    island,
                                    signal_initializers,
                                    signal,
                                    &required_signal_ids,
                                );
                                append_tokens_as_statements(&mut rewritten, tokens)?;
                            }
                        }
                    }
                    if let Some(initializer) = query_initializers
                        .iter()
                        .find(|initializer| initializer.id == id)
                    {
                        let tokens = emit_rust::emit_lossless_query_declaration(
                            island,
                            initializer,
                            id,
                            &reachable_query_ids,
                            emit_rust::query_initial_requires_typed_value(island, id),
                            query_source_ide_hints.get(&id),
                        );
                        append_tokens_as_statements(&mut rewritten, tokens)?;
                        prepared_query_ids.insert(id);
                    }
                    continue;
                }

                if let Some(id) = prelude_mutation_ids.get(&index).copied() {
                    if let Some(initializer) = mutation_initializers
                        .iter()
                        .find(|initializer| initializer.id == id)
                    {
                        let tokens = emit_rust::emit_lossless_mutation_declaration(
                            initializer,
                            id,
                            &reachable_mutation_ids,
                        );
                        append_tokens_as_statements(&mut rewritten, tokens)?;
                        prepared_mutation_ids.insert(id);
                    }
                    continue;
                }

                // An exact `let value = self.field;` is dual-use. The client
                // analyzer retains it as an inbound live-value alias, while
                // ordinary server Rust later in the authored program may
                // still name `value` (for example as an SSR `for` iterable).
                // Replaying only the synthetic inbound signal and deleting
                // the authored local leaves that server Rust with an
                // unresolved identifier.
                //
                // Keep the authored binding at its exact lexical position.
                // When this field also owns a reachable synthetic inbound
                // signal, prepare that signal from a borrow of the alias here
                // instead of evaluating `self.field` again at the tail. The
                // borrow avoids a second move for non-Copy fields, preserves
                // the author's evaluation/drop order, and remains accepted by
                // ClientEncode's reference implementation.
                if client_prelude_local_indices.contains(&index) {
                    if let Some((alias, field_name)) = dual_use_inbound_alias(&local) {
                        rewritten.push(Stmt::Local(local));
                        for initializer in signal_initializers {
                            if authored_signal_ids.contains(&initializer.id)
                                || !required_signal_ids.contains(&initializer.id)
                                || prepared_signal_ids.contains(&initializer.id)
                                || crate::analyze::inbound_signal_self_field(&initializer.expr)
                                    .as_ref()
                                    .map(|(name, _)| name.as_str())
                                    != Some(field_name.as_str())
                            {
                                continue;
                            }
                            let mut borrowed = initializer.clone();
                            borrowed.expr = syn::parse_quote!(&#alias);
                            let tokens = emit_rust::emit_lossless_signal_declaration(
                                island,
                                std::slice::from_ref(&borrowed),
                                borrowed.id,
                                &required_signal_ids,
                            );
                            append_tokens_as_statements(&mut rewritten, tokens)?;
                            prepared_signal_ids.insert(borrowed.id);
                        }
                        continue;
                    }
                }

                // Most node-ref declarations are browser-only and disappear
                // after template lowering. A handle passed through a direct
                // component literal is also a real zero-sized Rust field
                // value during SSR, so retain that exact lexical declaration.
                if crate::analyze::is_node_ref_local(&local) {
                    if prelude_node_ref_ids.get(&index).is_some_and(|node_ref| {
                        direct_component_server_node_ref_ids.contains(node_ref)
                    }) {
                        rewritten.push(Stmt::Local(local));
                    }
                    continue;
                }

                // Handler and client-derived alias declarations have no
                // server binding after the analyzed template is lowered.
                if crate::analyze::is_handler_local(&local)
                    || client_prelude_local_indices.contains(&index)
                {
                    continue;
                }

                rewritten.push(Stmt::Local(local));
            }
            Stmt::Expr(Expr::Call(call), _) if crate::parse::is_interval_call_expr(&call) => {
                // Timers are compiler-owned browser declarations and never
                // tick during SSR.
            }
            other => rewritten.push(other),
        }
    }

    let linked_server_expressions = rewrite_linked_shared_state_provider_children(
        server_expressions,
        shared_state_provider_sites,
        linked_site_ident,
    )?;
    let site = emit_rust::emit_lossless_render_site(
        island,
        asset_id,
        signal_initializers,
        shared_state_initializers,
        query_initializers,
        mutation_initializers,
        &prepared_signal_ids,
        direct_component_server_signal_ids,
        &prepared_shared_store_ids,
        &prepared_query_ids,
        &prepared_mutation_ids,
        ide_helper_hints,
        ide_source_hints,
        &parsed.context_ident,
        &linked_server_expressions,
        direct_component_site_nodes,
        linked_site_ident,
    );
    append_tokens_as_statements(&mut rewritten, site.setup)?;
    replace_render_macro(&mut tail, site.template)?;
    rewritten.push(tail);
    block.stmts = rewritten;
    Ok(())
}

fn rewrite_linked_shared_state_provider_children(
    server_expressions: &HashMap<NodeId, Expr>,
    provider_sites: &[crate::analyze::DirectSharedStateProviderSite],
    linked_site_ident: Option<&syn::Ident>,
) -> syn::Result<HashMap<NodeId, Expr>> {
    let mut rewritten = server_expressions.clone();
    let Some(linked_site_ident) = linked_site_ident else {
        return Ok(rewritten);
    };
    for provider in provider_sites {
        let expression = rewritten.get_mut(&provider.site_node).ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "rsjs internal error: a SharedStateProvider topology site at {}:{} lost its preserved server expression",
                    provider.source_anchor.start_line, provider.source_anchor.start_column,
                ),
            )
        })?;
        let Expr::Struct(provider_expression) = expression else {
            return Err(syn::Error::new(
                expression.span(),
                "rsjs internal error: a SharedStateProvider topology site no longer contains a struct literal",
            ));
        };
        let provider_span = provider_expression.span();
        let mut children = provider_expression.fields.iter_mut().filter(
            |field| matches!(&field.member, syn::Member::Named(name) if name == "children"),
        );
        let child_field = children.next().ok_or_else(|| {
            syn::Error::new(
                provider_span,
                "rsjs internal error: a SharedStateProvider topology site lost its `children` field",
            )
        })?;
        if children.next().is_some() {
            return Err(syn::Error::new(
                provider_span,
                "rsjs internal error: a SharedStateProvider topology site contains duplicate `children` fields",
            ));
        }
        let child = child_field.expr.clone();
        let child_edge = provider.child_edge;
        let provider_site_id = provider.id.index();
        child_field.colon_token = Some(Default::default());
        child_field.expr = syn::parse_quote! {
            ::rsjs::__private::LinkedSharedStateProviderChildren::new(
                ::rsjs::__private::LinkedComponent::new(
                    #child,
                    #linked_site_ident.child(#child_edge),
                ),
                #linked_site_ident.scope_id,
                #linked_site_ident.candidate_id,
                #provider_site_id,
            )
        };
    }
    Ok(rewritten)
}

/// Exact inbound-field aliases participate in client dataflow but remain real
/// authored server locals.
///
/// Keep this deliberately narrower than
/// [`crate::analyze::inbound_signal_self_field`]. Calls such as
/// `self.field.clone()` and `self.field.unwrap_or(...)` are computations with
/// their own move/clone semantics; only a plain field move has an authored
/// local whose identity can safely seed the same inbound signal.
pub(crate) fn is_dual_use_inbound_alias(local: &syn::Local) -> bool {
    dual_use_inbound_alias(local).is_some()
}

fn dual_use_inbound_alias(local: &syn::Local) -> Option<(syn::Ident, String)> {
    let syn::Pat::Ident(pattern) = &local.pat else {
        return None;
    };
    if pattern.by_ref.is_some() || pattern.subpat.is_some() {
        return None;
    }
    let initializer = local.init.as_ref()?.expr.as_ref();
    let Expr::Field(field) = initializer else {
        return None;
    };
    let Expr::Path(base) = field.base.as_ref() else {
        return None;
    };
    if !base.path.is_ident("self") {
        return None;
    }
    let syn::Member::Named(member) = &field.member else {
        return None;
    };
    Some((pattern.ident.clone(), member.to_string()))
}

fn append_tokens_as_statements(output: &mut Vec<Stmt>, tokens: TokenStream) -> syn::Result<()> {
    if tokens.is_empty() {
        return Ok(());
    }
    let block: Block = syn::parse2(quote!({ #tokens }))?;
    output.extend(block.stmts);
    Ok(())
}

fn replace_render_macro(statement: &mut Stmt, replacement: TokenStream) -> syn::Result<()> {
    let replacement_expr: Expr = syn::parse2(replacement)?;
    let Expr::Macro(replacement) = replacement_expr else {
        return Err(syn::Error::new(
            replacement_expr.span(),
            "rsjs internal error: the lossless render-site emitter did not produce one HTML macro",
        ));
    };
    let mut rewriter = RenderMacroRewriter {
        replacement: replacement.mac,
        replaced: 0,
    };
    rewriter.visit_stmt_mut(statement);
    match rewriter.replaced {
        1 => Ok(()),
        0 => Err(syn::Error::new(
            statement.span(),
            "rsjs internal error: the validated HTML render site disappeared during rewriting",
        )),
        _ => Err(syn::Error::new(
            statement.span(),
            "rsjs/multiple-render-sites: one analyzed render tail resolved to more than one HTML macro; split the branches into separate components",
        )),
    }
}

struct RenderMacroRewriter {
    replacement: Macro,
    replaced: usize,
}

impl VisitMut for RenderMacroRewriter {
    fn visit_macro_mut(&mut self, mac: &mut Macro) {
        let is_html = mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "html" || segment.ident == "html_sync");
        if is_html {
            *mac = self.replacement.clone();
            self.replaced += 1;
            return;
        }
        visit_mut::visit_macro_mut(self, mac);
    }
}

fn locate_pending_async_body_mut(block: &mut Block) -> syn::Result<&mut ExprAsync> {
    let block_span = block.span();
    let [Stmt::Expr(Expr::Call(call), None)] = block.stmts.as_mut_slice() else {
        return Err(syn::Error::new(
            block_span,
            "an analyzed `WebRender` method must return exactly \
             `RenderOutcome::pending(async move { ... })`",
        ));
    };
    let call_span = call.span();
    let Some(Expr::Async(async_body)) = call.args.first_mut() else {
        return Err(syn::Error::new(
            call_span,
            "expected `RenderOutcome::pending(async move { ... })`",
        ));
    };
    Ok(async_body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::final_plan::{
        FinalComponentSite, FinalOwnerRole, FinalSharedStateProviderRequirement,
    };
    use rsjs_ir::abi::ProviderSiteId;
    use rsjs_ir::SourceAnchor;
    use std::collections::BTreeMap;

    #[test]
    fn replaces_only_the_html_macro_inside_the_authored_tail() {
        let mut tail: Stmt = syn::parse_quote! {
            return html_sync! { <main>"old"</main> }.render(cx);
        };
        replace_render_macro(
            &mut tail,
            quote! { ::proa_macros::html_sync! { <rsjs-island>"new"</rsjs-island> } },
        )
        .unwrap();
        let emitted = tail.to_token_stream().to_string();

        assert!(emitted.starts_with("return"));
        assert!(emitted.contains("proa_macros :: html_sync !"));
        assert!(emitted.contains("rsjs - island"));
        assert!(emitted.ends_with(". render (cx) ;"));
        assert!(!emitted.contains("\"old\""));
    }

    #[test]
    fn linked_sync_impl_factors_the_authored_body_and_emits_exact_child_sites() {
        let authored: ItemImpl = syn::parse_quote! {
            impl<L: proa_core::DataLoader, B: proa_core::WriteBuf>
                proa_core::WebRenderSync<L, B> for Page
            {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<L, B>,
                ) -> Result<(), proa_core::WriteError> {
                    let label = load_label()?;
                    return proa_macros::html_sync! {
                        <main>{label}</main>
                    }
                    .render(cx);
                }
            }
        };
        let parsed = crate::parse::parse_island_item(
            authored.clone(),
            crate::parse::parse_attr_args(quote!(component)).unwrap(),
        )
        .unwrap();
        let selection = linked_test_selection();

        let emitted = emit_linked_component_impls(&authored, authored.clone(), &parsed, &selection)
            .unwrap()
            .to_string();

        assert!(emitted.contains("WebRenderSync < L , B > for Page"));
        assert!(emitted.contains("ComponentSiteRenderSync < L , B > for Page"));
        assert!(emitted.contains("fn render_at"));
        assert!(emitted.contains("fn linked_site"));
        assert!(emitted.contains("LinkedChildSite :: new (5"));
        assert!(emitted.contains("render_at (self , cx"));
        assert_eq!(
            emitted.matches("load_label").count(),
            1,
            "the authored body must exist only in the hidden render_at impl"
        );
        assert_eq!(emitted.matches('?').count(), 1);
    }

    #[test]
    fn linked_async_delegate_returns_the_hidden_rpitit_without_a_wrapper_future() {
        let authored: ItemImpl = syn::parse_quote! {
            impl<B: proa_core::WriteBuf> proa_core::WebRender<::proa_core::NoLoader, B> for AsyncPage {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<::proa_core::NoLoader, B>,
                ) -> proa_core::RenderOutcome<
                    impl core::future::Future<
                        Output = Result<(), proa_core::WriteError>,
                    >,
                >
                {
                    proa_core::RenderOutcome::pending(async move {
                        proa_macros::html! { <main>"async"</main> }
                            .render(cx)
                            .resolve()
                            .await
                    })
                }
            }
        };
        let parsed = crate::parse::parse_island_item(
            authored.clone(),
            crate::parse::parse_attr_args(quote!(component)).unwrap(),
        )
        .unwrap();
        let selection = linked_test_selection();

        let emitted = emit_linked_component_impls(&authored, authored.clone(), &parsed, &selection)
            .unwrap()
            .to_string();

        assert!(
            emitted.contains("ComponentSiteRender < :: proa_core :: NoLoader , B > for AsyncPage")
        );
        assert_eq!(
            emitted.matches("RenderOutcome :: pending").count(),
            1,
            "only the factored authored body may create the pending future"
        );
        assert_eq!(emitted.matches("async move").count(), 1);
        assert!(!emitted.contains("Box"));
        assert!(emitted.contains("render_at (self , cx"));
    }

    #[test]
    fn root_ineligible_component_emits_only_the_exact_site_render_impl() {
        let authored: ItemImpl = syn::parse_quote! {
            impl<L: proa_core::DataLoader, B: proa_core::WriteBuf>
                proa_core::WebRenderSync<L, B> for Disclosure
            {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<L, B>,
                ) -> Result<(), proa_core::WriteError> {
                    load_slot()?;
                    return proa_macros::html_sync! {
                        <section>"disclosure"</section>
                    }
                    .render(cx);
                }
            }
        };
        let parsed = crate::parse::parse_island_item(
            authored.clone(),
            crate::parse::parse_attr_args(quote!(component)).unwrap(),
        )
        .unwrap();
        let mut selection = linked_test_selection();
        selection.standalone_candidate_id = None;
        selection
            .sites
            .retain(|site| site.parent_candidate_id.is_some());

        let emitted = emit_linked_component_impls(&authored, authored.clone(), &parsed, &selection)
            .unwrap()
            .to_string();

        assert!(!emitted.contains("WebRenderSync < L , B > for Disclosure"));
        assert!(emitted.contains("ComponentSiteRenderSync < L , B > for Disclosure"));
        assert!(emitted.contains("fn render_at"));
        assert!(emitted.contains("fn linked_site"));
        assert_eq!(
            emitted.matches("load_slot").count(),
            1,
            "the authored body must exist only in the exact-site implementation"
        );
    }

    #[test]
    fn root_ineligible_trait_identity_probe_preserves_the_authored_generic_alias() {
        let authored: ItemImpl = syn::parse_quote! {
            impl<L: proa_core::DataLoader, B: proa_core::WriteBuf>
                RenderSync<L, B> for Disclosure
            {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<L, B>,
                ) -> Result<(), proa_core::WriteError> {
                    proa_macros::html_sync! { <section>"disclosure"</section> }
                        .render(cx)
                }
            }
        };
        let parsed = crate::parse::parse_island_item(
            authored,
            crate::parse::parse_attr_args(quote!(component)).unwrap(),
        )
        .unwrap();
        let mut selection = linked_test_selection();
        selection.standalone_candidate_id = None;

        let assertion = render_trait_identity_assertion(&parsed, Some(&selection))
            .unwrap()
            .to_token_stream()
            .to_string();

        assert!(
            assertion.contains("< () as RenderSync < L , B > >"),
            "the final identity probe must retain rustc-resolved alias and generic arguments: \
             {assertion}"
        );
        assert!(!assertion.contains("< Self as RenderSync"));
        assert!(assertion.contains("__PROA_RENDER_TRAIT_WITNESS"));
    }

    #[test]
    fn root_eligible_trait_identity_and_standalone_delegate_remain_on_self() {
        let authored: ItemImpl = syn::parse_quote! {
            impl<L: proa_core::DataLoader, B: proa_core::WriteBuf>
                RenderSync<L, B> for Page
            {
                fn render(
                    self,
                    cx: &mut proa_core::WebContext<L, B>,
                ) -> Result<(), proa_core::WriteError> {
                    proa_macros::html_sync! { <main>"page"</main> }.render(cx)
                }
            }
        };
        let parsed = crate::parse::parse_island_item(
            authored.clone(),
            crate::parse::parse_attr_args(quote!(component)).unwrap(),
        )
        .unwrap();
        let selection = linked_test_selection();

        let assertion = render_trait_identity_assertion(&parsed, Some(&selection))
            .unwrap()
            .to_token_stream()
            .to_string();
        let emitted = emit_linked_component_impls(&authored, authored.clone(), &parsed, &selection)
            .unwrap()
            .to_string();

        assert!(assertion.contains("< Self as RenderSync < L , B > >"));
        assert!(!assertion.contains("< () as RenderSync"));
        assert!(emitted.contains("RenderSync < L , B > for Page"));
        assert!(emitted.contains("ComponentSiteRenderSync < L , B > for Page"));
        assert!(emitted.contains("linked_site (103u64"));
        assert!(emitted.contains("render_at (self , cx"));
    }

    #[test]
    fn linked_site_tokens_preserve_every_live_input_temporal_mode_and_current_callback() {
        let emitted = linked_site_lookup_method(&linked_test_selection())
            .unwrap()
            .to_token_stream()
            .to_string();

        assert_eq!(emitted.matches("LinkedLiveProp :: new").count(), 4);
        assert!(emitted.contains("LinkedTemporalPropMode :: LiveRead"));
        assert!(emitted.contains("LinkedTemporalPropMode :: SeedOnce { seed_site_id : 51u32"));
        assert!(emitted.contains("LinkedTemporalPropMode :: EffectDependency"));
        assert!(emitted.contains("LinkedTemporalPropMode :: QueryKeyDependency"));
        assert_eq!(emitted.matches("LinkedLiveCallback :: new").count(), 1);
        assert!(emitted.contains("LinkedCallbackDispatch :: ResolveCurrent"));
        assert!(emitted.contains("LinkedOwnerActivationStrategy :: Visible"));
        assert!(emitted.contains("LinkedOwnerActivationStrategy :: Interaction"));
        assert!(emitted.contains("Option :: Some (\"click\")"));
        assert!(emitted.contains("LinkedLiveProp :: new (104u64 , 2u32 , 108u64"));
        assert!(emitted.contains("LinkedLiveCallback :: new (104u64 , 2u32 , 108u64"));
        assert!(emitted.contains("61u32 , 62u32 , 63u32"));
        assert!(emitted.contains("& __RSJS_LINKED_LIVE_PROPS_108"));
        assert!(emitted.contains("& __RSJS_LINKED_LIVE_CALLBACKS_108"));
    }

    #[test]
    fn linked_site_tokens_bind_shared_state_to_the_exact_provider_occurrence() {
        let mut selection = linked_test_selection();
        selection.shared_state_requirements_by_consumer.insert(
            8,
            vec![FinalSharedStateProviderRequirement {
                consumer_candidate_id: 8,
                reference_id: 4,
                provider_host_candidate_id: 3,
                provider_site_id: 2,
                type_key: "acme.workbench".to_owned(),
                schema_abi: [7; 32],
            }],
        );

        let emitted = linked_site_lookup_method(&selection)
            .unwrap()
            .to_token_stream()
            .to_string();

        assert_eq!(
            emitted
                .matches("LinkedSharedStateRequirement :: new")
                .count(),
            1
        );
        assert!(
            emitted.contains("LinkedSharedStateRequirement :: new (4u32"),
            "{emitted}"
        );
        assert!(emitted.contains("103u64 , 2u32"), "{emitted}");
        assert!(
            emitted.contains("& __RSJS_LINKED_SHARED_STATE_REQUIREMENTS_108"),
            "{emitted}"
        );
    }

    #[test]
    fn linked_provider_rewrite_preserves_the_provider_and_replaces_only_children() {
        let node = NodeId(4);
        let expressions = HashMap::from([(
            node,
            syn::parse_quote! {
                rsjs::SharedStateProvider {
                    state: seed_once(),
                    children: Child { label: "hello" },
                }
            },
        )]);
        let providers = [crate::analyze::DirectSharedStateProviderSite {
            id: ProviderSiteId::new(2),
            state_ty: syn::parse_quote!(WorkbenchState),
            child_edge: 0,
            site_node: node,
            source_anchor: SourceAnchor {
                start_line: 10,
                start_column: 5,
            },
        }];
        let linked_site = format_ident!("__rsjs_linked_render_site");

        let rewritten = rewrite_linked_shared_state_provider_children(
            &expressions,
            &providers,
            Some(&linked_site),
        )
        .unwrap();
        let emitted = rewritten[&node].to_token_stream().to_string();

        assert!(
            emitted.starts_with("rsjs :: SharedStateProvider"),
            "{emitted}"
        );
        assert_eq!(emitted.matches("seed_once").count(), 1, "{emitted}");
        assert_eq!(
            emitted
                .matches("LinkedSharedStateProviderChildren :: new")
                .count(),
            1,
            "{emitted}"
        );
        assert_eq!(
            emitted.matches("LinkedComponent :: new").count(),
            1,
            "{emitted}"
        );
        assert!(emitted.contains("Child { label : \"hello\" }"), "{emitted}");
        assert!(
            emitted.contains("__rsjs_linked_render_site . scope_id"),
            "{emitted}"
        );
        assert!(
            emitted.contains("__rsjs_linked_render_site . candidate_id"),
            "{emitted}"
        );
        assert!(
            emitted.contains("__rsjs_linked_render_site . child (0u32)"),
            "{emitted}"
        );
        assert!(emitted.contains(", 2u32"), "{emitted}");
    }

    #[test]
    fn linked_site_codegen_rejects_a_live_input_under_the_wrong_child_candidate() {
        let mut selection = linked_test_selection();
        selection.live_props_by_child.get_mut(&8).unwrap()[0].child_candidate_id = 99;

        let error = linked_site_lookup_method(&selection).unwrap_err();
        assert!(error
            .to_string()
            .contains("record for candidate 8 names child candidate 99"));
    }

    fn linked_test_selection() -> FinalComponentSelection {
        FinalComponentSelection {
            plan_id: [9; 32],
            dependency_id: [9; 32],
            fragment_asset_id: "test_fragment_generation".to_owned(),
            runtime_sites: [3, 4, 8, 13, 21]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        crate::final_plan::FinalRuntimeSiteIdentity {
                            scope_id: [42; 32],
                            site_id: [id as u8; 32],
                            candidate_id: 100 + u64::from(id),
                        },
                    )
                })
                .collect(),
            standalone_candidate_id: Some(3),
            sites: vec![
                FinalComponentSite {
                    candidate_id: 3,
                    parent_candidate_id: None,
                    incoming_edge_id: None,
                    structural_path: Vec::new(),
                    incoming_lifecycle: None,
                    reactive_lifecycle_root_candidate_id: None,
                    owner_role: FinalOwnerRole::Root { owner_id: 7 },
                    owner_strategy: Some(FinalOwnerActivationStrategy::Visible),
                    owner_asset_id: Some("rsjs_owner_test_7".to_owned()),
                    seed_key: Some("rsjs_owner_test_7".to_owned()),
                    fragment_asset_id: "page_fragment".to_owned(),
                },
                FinalComponentSite {
                    candidate_id: 8,
                    parent_candidate_id: Some(4),
                    incoming_edge_id: Some(2),
                    structural_path: vec![2],
                    incoming_lifecycle: None,
                    reactive_lifecycle_root_candidate_id: None,
                    owner_role: FinalOwnerRole::Member {
                        owner_id: 11,
                        root_candidate_id: 4,
                    },
                    owner_strategy: Some(FinalOwnerActivationStrategy::Interaction {
                        event: Some("click".to_owned()),
                    }),
                    owner_asset_id: Some("rsjs_owner_test_11".to_owned()),
                    seed_key: Some("rsjs_member_test_8".to_owned()),
                    fragment_asset_id: "child_fragment".to_owned(),
                },
            ],
            child_sites: BTreeMap::from([((3, 5), 13), ((8, 6), 21)]),
            live_props_by_child: BTreeMap::from([(
                8,
                vec![
                    FinalLivePropUse {
                        parent_candidate_id: 4,
                        edge_id: 2,
                        child_candidate_id: 8,
                        field_id: 10,
                        crossing_hook_id: 20,
                        parent_value_id: 30,
                        child_value_id: 40,
                        prop_site_id: 50,
                        temporal: FinalTemporalPropMode::LiveRead,
                    },
                    FinalLivePropUse {
                        parent_candidate_id: 4,
                        edge_id: 2,
                        child_candidate_id: 8,
                        field_id: 11,
                        crossing_hook_id: 21,
                        parent_value_id: 31,
                        child_value_id: 41,
                        prop_site_id: 51,
                        temporal: FinalTemporalPropMode::SeedOnce { seed_site_id: 51 },
                    },
                    FinalLivePropUse {
                        parent_candidate_id: 4,
                        edge_id: 2,
                        child_candidate_id: 8,
                        field_id: 12,
                        crossing_hook_id: 22,
                        parent_value_id: 32,
                        child_value_id: 42,
                        prop_site_id: 52,
                        temporal: FinalTemporalPropMode::EffectDependency,
                    },
                    FinalLivePropUse {
                        parent_candidate_id: 4,
                        edge_id: 2,
                        child_candidate_id: 8,
                        field_id: 13,
                        crossing_hook_id: 23,
                        parent_value_id: 33,
                        child_value_id: 43,
                        prop_site_id: 53,
                        temporal: FinalTemporalPropMode::QueryKeyDependency,
                    },
                ],
            )]),
            live_callbacks_by_child: BTreeMap::from([(
                8,
                vec![FinalLiveCallback {
                    parent_candidate_id: 4,
                    edge_id: 2,
                    child_candidate_id: 8,
                    field_id: 60,
                    crossing_hook_id: 61,
                    parent_value_id: 62,
                    child_value_id: 63,
                    shape: FinalCallbackShape::Required,
                }],
            )]),
            shared_state_providers_by_host: BTreeMap::new(),
            shared_state_requirements_by_consumer: BTreeMap::new(),
            node_ref_aliases_by_parent: BTreeMap::new(),
            node_ref_aliases_by_child: BTreeMap::new(),
            codegen_by_candidate: BTreeMap::new(),
            all_sites: Vec::new(),
            all_child_sites: BTreeMap::new(),
            all_live_props_by_child: BTreeMap::new(),
            all_live_callbacks_by_child: BTreeMap::new(),
            all_node_ref_aliases: Vec::new(),
            helper_linkage: crate::final_plan::FinalHelperLinkage::default(),
        }
    }
}
