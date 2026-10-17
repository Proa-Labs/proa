// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Procedural macros for rsjs.
//!
//! This crate is the proc-macro frontend. Its component entry point is the
//! `#[rsjs(component...)]` attribute on an authored Proa render-trait impl;
//! helper attributes and derives are re-exported through the `rsjs` crate.
//!
//! There is no separate body macro. The authored `WebRenderSync::render` or
//! `WebRender::render` method ends in a normal `html_sync!` / `html!`
//! invocation from `proa_macros`; the component attribute lifts that
//! invocation's parse tree and runs additional analysis on top of it.
//!
//! # Pipeline (across crates)
//!
//! ```text
//!  #[rsjs(component...)] authored render-trait impl
//!         │
//!         │   rsjs_macros::parse        — validate the real render signature,
//!         │                                lift the tail html macro
//!         ▼
//!  lifted html! parse tree
//!         │
//!         │   rsjs_macros::analyze      — walk the tree, identify signals,
//!         │                                event handlers, conditional
//!         │                                sections, input bindings;
//!         │                                validate work against the authored
//!         │                                sync/async render surface
//!         ▼
//!  rsjs_ir::Island                      — side-tables keyed to NodeIds
//!         │                                in the lifted parse tree
//!         │   rsjs_ir::optimize         — passes (static consolidation,
//!         │                                dead signal elimination, ...)
//!         ▼
//!  rsjs_ir::Island   (optimised)
//!         │
//!         ├── rsjs_ir::analysis         — classify render strategy
//!         │                                (`ServerOnly`, `SsrHydrate`, ...)
//!         ├── rsjs_ir::emit_js          — for hydrated islands, per-island
//!         │                                JS source baked into the crate
//!         │                                and registered on the manifest
//!         └── rsjs_macros::emit_rust    — preserve the authored impl header
//!                                          and render signature; rewrite only
//!                                          the analyzed render body
//! ```
//!
//! # Why split across two crates
//!
//! `rsjs_ir` owns the side-table types, optimisation passes, and the
//! JS emitter. It has no `syn` or `proc_macro` dependency, which
//! means the IR is unit-testable in isolation: write a test that
//! constructs an `Island` by hand, run the passes, assert the JS
//! emitter output. No macro-expansion machinery needed.
//!
//! `rsjs_macros` owns the proc-macro entry point, the integration
//! with `proa_macros` (lifting the `html!` parse tree), the rsjs
//! analyser, and `emit_rust` (which rewrites the authored
//! `WebRenderSync<L>` / `WebRender<L>` impl token stream — kept here because
//! it depends on `quote!`, spans, and macro hygiene).
//! This crate is the only place that depends on `syn` and
//! `proa_macros`.

extern crate proc_macro;

mod js_sentinel;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION;
use sha2::{Digest, Sha256};
use syn::spanned::Spanned;

const DESCRIPTOR_PHASE_ENV: &str = "RSJS_DESCRIPTOR_PHASE";
const FINAL_PHASE_ENV: &str = "RSJS_FINAL_PHASE";
const CARGO_UNIT_ID_ENV: &str = "RSJS_CARGO_UNIT_ID";
const FINAL_CARGO_UNIT_STATUS_ENV: &str = "RSJS_FINAL_CARGO_UNIT_STATUS";
const FINAL_CARGO_UNIT_STATUS_MAPPED: &str = "mapped-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompilationPhase {
    Plain,
    Descriptor,
    Final,
}

impl CompilationPhase {
    const fn allows_legacy_filesystem_artifacts(self) -> bool {
        matches!(self, Self::Plain)
    }
}

fn compilation_phase() -> Result<CompilationPhase, String> {
    let descriptor = phase_enabled(DESCRIPTOR_PHASE_ENV)?;
    let final_phase = phase_enabled(FINAL_PHASE_ENV)?;
    match (descriptor, final_phase) {
        (false, false) => Ok(CompilationPhase::Plain),
        (true, false) => Ok(CompilationPhase::Descriptor),
        (false, true) => Ok(CompilationPhase::Final),
        (true, true) => Err(format!(
            "`{DESCRIPTOR_PHASE_ENV}` and `{FINAL_PHASE_ENV}` cannot both be enabled"
        )),
    }
}

fn mirror_filesystem_artifacts(phase: CompilationPhase, mirror: impl FnOnce()) {
    if phase.allows_legacy_filesystem_artifacts() {
        mirror();
    }
}

fn reject_unlinked_helper_sentinel(kind: &str, javascript: &str) -> syn::Result<()> {
    match js_sentinel::contains_unlinked_helper_identifier(javascript) {
        Ok(false) => Ok(()),
        Ok(true) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs/final-helper-link: deployable {kind} still contains reserved unresolved-helper identifier `{}`",
                js_sentinel::UNLINKED_HELPER_IDENTIFIER_PREFIX
            ),
        )),
        Err(error) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs/final-helper-link: could not validate deployable {kind} for unresolved-helper identifiers: {error}"
            ),
        )),
    }
}

/// Resolve the descriptor-pass Cargo-unit identity supplied by the build
/// orchestrator.
///
/// Ordinary Cargo and rust-analyzer expansion deliberately retain the
/// all-zero placeholder so local checking does not require the two-compilation
/// driver. Descriptor and final compilation are fail-closed: once either phase
/// marker is present, its wrapper must supply exactly 32 bytes of lowercase
/// hex. Final compilation receives the *original descriptor unit* identity,
/// not a plan-specific artifact hash.
fn descriptor_cargo_unit_id() -> Result<[u8; 32], String> {
    descriptor_cargo_unit_id_for_phase(compilation_phase()?)
}

fn descriptor_cargo_unit_id_for_phase(phase: CompilationPhase) -> Result<[u8; 32], String> {
    if phase == CompilationPhase::Plain {
        return Ok([0; 32]);
    }
    if phase == CompilationPhase::Final {
        let status = std::env::var(FINAL_CARGO_UNIT_STATUS_ENV).map_err(|error| match error {
            std::env::VarError::NotPresent => format!(
                "final RSJS compilation is missing `{FINAL_CARGO_UNIT_STATUS_ENV}`; \
                     invoke it through `cargo proa build`"
            ),
            std::env::VarError::NotUnicode(_) => format!(
                "`{FINAL_CARGO_UNIT_STATUS_ENV}` must be valid Unicode and equal to \
                     `{FINAL_CARGO_UNIT_STATUS_MAPPED}` for a descriptor-bearing Cargo unit"
            ),
        })?;
        if status != FINAL_CARGO_UNIT_STATUS_MAPPED {
            return Err(format!(
                "final RSJS compilation could not map this Cargo unit to its descriptor pass \
                 (`{FINAL_CARGO_UNIT_STATUS_ENV}={status}`); refusing to guess a component or \
                 helper identity—run the exact target through `cargo proa build`"
            ));
        }
    }

    let encoded = std::env::var(CARGO_UNIT_ID_ENV).map_err(|error| match error {
        std::env::VarError::NotPresent => format!(
            "RSJS staged compilation is missing `{CARGO_UNIT_ID_ENV}`; \
             invoke it through the `cargo proa` rustc wrapper"
        ),
        std::env::VarError::NotUnicode(_) => {
            format!("`{CARGO_UNIT_ID_ENV}` must be 64 lowercase hexadecimal characters")
        }
    })?;
    parse_descriptor_cargo_unit_id(&encoded)
}

fn phase_enabled(name: &'static str) -> Result<bool, String> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("`{name}` must be valid Unicode and equal to `1`"))
        }
        Ok(value) if value == "1" => Ok(true),
        Ok(value) => Err(format!(
            "`{name}` must equal `1` when enabled; received `{value}`"
        )),
    }
}

fn parse_descriptor_cargo_unit_id(encoded: &str) -> Result<[u8; 32], String> {
    if encoded.len() != 64 {
        return Err(format!(
            "`{CARGO_UNIT_ID_ENV}` must contain exactly 64 lowercase hexadecimal characters; \
             received {}",
            encoded.len()
        ));
    }

    let mut decoded = [0_u8; 32];
    let bytes = encoded.as_bytes();
    let mut index = 0;
    while index < decoded.len() {
        let high_index = index * 2;
        let low_index = high_index + 1;
        let high = decode_lower_hex(bytes[high_index]).ok_or_else(|| {
            format!(
                "`{CARGO_UNIT_ID_ENV}` contains a non-lowercase-hex byte at offset {high_index}"
            )
        })?;
        let low = decode_lower_hex(bytes[low_index]).ok_or_else(|| {
            format!("`{CARGO_UNIT_ID_ENV}` contains a non-lowercase-hex byte at offset {low_index}")
        })?;
        decoded[index] = (high << 4) | low;
        index += 1;
    }
    Ok(decoded)
}

fn decode_lower_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Match the same direct `text!` paths that the Proa HTML macros treat as an
/// intrinsic. Namespaced macros from other crates remain opaque to rsjs.
fn is_text_intrinsic_path(path: &syn::Path) -> bool {
    let mut segments = path.segments.iter();
    match (segments.next(), segments.next(), segments.next()) {
        (Some(text), None, None) => {
            path.leading_colon.is_none()
                && text.ident == "text"
                && matches!(text.arguments, syn::PathArguments::None)
        }
        (Some(crate_name), Some(text), None) => {
            crate_name.ident == "proa_macros"
                && matches!(crate_name.arguments, syn::PathArguments::None)
                && text.ident == "text"
                && matches!(text.arguments, syn::PathArguments::None)
        }
        _ => false,
    }
}

mod analyze;
mod asset;
mod client_attr;
mod client_module_attr;
mod client_object_module_attr;
mod client_props_derive;
mod component_descriptor;
mod final_plan;
mod parse;
mod server_program;
mod shared_state_derive;

/// Rust emission for analyzed component render implementations.
///
/// The production path preserves the user-authored impl header, trait path,
/// generics, render signature, and context parameter. It replaces only the
/// render body with the SSR-only or hydration-marked Proa template derived
/// from [`rsjs_ir::Island`]. Reachable signals, query/mutation metadata, and
/// shared-state bindings are threaded through that body.
///
/// Historical standalone emitters remain available to this crate's focused
/// IR/golden tests; they are not an authoring surface and do not make the
/// component attribute synthesize a trait implementation.
pub(crate) mod emit_rust;

/// Mark a component render block as a reactive client-side island.
///
/// Components author a real Proa render-trait impl. The attribute opts that
/// impl into analysis and descriptor generation:
///
/// ```ignore
/// #[rsjs(component)]
/// impl<L: DataLoader> WebRenderSync<L> for Badge {
///     fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
///         WebRenderSync::render(
///             proa_macros::html_sync! { <span>{self.label}</span> },
///             cx,
///         )
///     }
/// }
/// ```
///
/// Add `client` when the component originates browser behavior:
/// `#[rsjs(component, client)]`. Async components author the real
/// `WebRender::render` signature and return
/// `RenderOutcome::pending(async move { ... })`; `async fn render` sugar is not
/// accepted.
///
/// The macro lifts the `html_sync!` / `html!` parse tree,
/// builds an `rsjs_ir::Island` IR, runs the default optimisation passes, and
/// rewrites the authored impl so its body threads hydration markers through
/// Proa's normal byte-emission path. The impl header and render signature are
/// preserved.
///
/// Ordinary, unstaged analysis builds also mirror per-island JS and compiler
/// diagnostics to a discoverable location:
///
/// - Build-script context (`OUT_DIR` set): `<OUT_DIR>/rsjs/<asset_id>.js`.
/// - Otherwise: `<CARGO_TARGET_DIR>/rsjs/<asset_id>.js`, falling back
///   to `target/rsjs/<asset_id>.js`.
///
/// `<asset_id>` is the lowercased final segment of the impl's self type path
/// (`Counter` -> `counter`), matching the convention `emit_rust` uses for
/// hydration markers. Orchestrated descriptor and final compilation do not
/// write these non-authoritative mirrors: descriptor data travels in validated
/// object records, and final JavaScript is embedded in manifest registrations.
#[proc_macro_attribute]
pub fn rsjs(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2: TokenStream2 = attr.into();
    let item2: TokenStream2 = item.into();
    match expand_rsjs(attr2, item2) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_rsjs(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let compilation_phase = compilation_phase()
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?;
    // Ordinary `cargo check` and rust-analyzer need the provisional expansion,
    // but an ordinary codegen/link must never become a deployable server
    // artifact. The generated link guard is inert for metadata-only checking
    // and absent from both orchestrated phases.
    let emit_plain_build_guard = compilation_phase == CompilationPhase::Plain;
    let cargo_unit_id = descriptor_cargo_unit_id_for_phase(compilation_phase)
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?;
    let attr_tokens_for_fingerprint = attr.clone();
    let attr_args = parse::parse_attr_args(attr)?;
    // Round-3 review #8: keep a copy of the impl tokens for asset-id
    // disambiguation. Two `impl ... for Counter` blocks in different
    // modules both write the same self-type (`Counter`) and would
    // hash identically against the path alone; folding the full
    // impl-block token stream into the hash separates them in the
    // common case where their bodies / attributes / surrounding
    // tokens differ.
    let impl_tokens_for_hash = item.clone();
    let parsed_item = syn::parse2::<syn::ItemImpl>(item)?;
    let authored_item = parsed_item.clone();
    let impl_start = parsed_item.impl_token.span.start();
    let parsed = parse::parse_island_item(parsed_item, attr_args)?;
    let input_fingerprint =
        strategy_input_fingerprint(&attr_tokens_for_fingerprint, &impl_tokens_for_hash);
    let final_component_selection = final_plan::validate_current_component(
        cargo_unit_id,
        component_record_id(&input_fingerprint),
    )
    .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?;
    let query_initializers: Vec<emit_rust::QueryInitializer> =
        analyze::query_initializers(&parsed.prelude_lets)?
            .into_iter()
            .map(|(id, expr, key_expr)| emit_rust::QueryInitializer { id, expr, key_expr })
            .collect();
    let analysis = analyze::analyze(&parsed)?;
    let analyze::AnalysisOutput {
        mut island,
        hir_sidecar,
        signal_initializers,
        query_initializers: child_query_initializers,
        mutation_initializers,
        shared_state_initializers,
        prelude_signal_ids,
        prelude_shared_store_ids,
        prelude_query_ids,
        prelude_mutation_ids,
        prelude_node_ref_ids,
        shared_prelude_local_indices,
        client_prelude_local_indices,
        direct_component_edges,
        shared_state_provider_sites,
        direct_helper_edges,
        server_expressions,
        direct_component_site_nodes,
        mut inbound_inputs,
        generic_static_slot,
        direct_payload_handler_values,
    } = analysis;
    let signal_initializers: Vec<emit_rust::SignalInitializer> = signal_initializers
        .into_iter()
        .map(|initializer| emit_rust::SignalInitializer {
            id: initializer.id,
            expr: initializer.expr,
            explicit_ty: initializer.explicit_ty,
        })
        .collect();
    let mut query_initializers = query_initializers;
    query_initializers.extend(child_query_initializers.into_iter().map(|init| {
        emit_rust::QueryInitializer {
            id: init.id,
            expr: init.expr,
            key_expr: init.key_expr,
        }
    }));
    let mutation_initializers: Vec<emit_rust::MutationInitializer> = mutation_initializers
        .into_iter()
        .map(|init| emit_rust::MutationInitializer {
            id: init.id,
            key_expr: init.key_expr,
            invalidates_expr: init.invalidates_expr,
        })
        .collect();
    let shared_state_initializers: Vec<emit_rust::SharedStateInitializer> =
        shared_state_initializers
            .into_iter()
            .map(|init| emit_rust::SharedStateInitializer {
                id: init.id,
                source_name: init.source_name,
                ty: init.ty,
                expected_type_key: init.expected_type_key,
            })
            .collect();
    let asset_id = asset::derive_asset_id_with_context(&parsed.struct_path, &impl_tokens_for_hash);
    let fragment_asset_id = final_component_selection
        .as_ref()
        .map(|selection| selection.fragment_asset_id.clone())
        .unwrap_or_else(|| asset_id.clone());
    let struct_name = derive_struct_name(&parsed.struct_path);
    let direct_component_server_signal_ids =
        direct_component_server_signal_ids(&direct_component_edges, &server_expressions);
    let direct_component_server_node_ref_ids =
        direct_component_server_node_ref_ids(&direct_component_edges);
    let strategy_analysis = rsjs_ir::classify_render_strategy(&island);
    let needs_client_js = strategy_analysis.needs_client_js();
    let linked_fragment_active = final_component_selection.as_ref().is_some_and(|selection| {
        selection
            .sites
            .iter()
            .any(|site| !matches!(site.owner_role, final_plan::FinalOwnerRole::ServerOnly))
    });
    let linked_fragment_signal_exports = final_component_selection
        .as_ref()
        .map(final_plan::linked_fragment_signal_exports)
        .transpose()
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?
        .unwrap_or_default();
    let linked_fragment_lifecycle_exports = final_component_selection
        .as_ref()
        .map(final_plan::linked_fragment_lifecycle_exports)
        .transpose()
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?
        .unwrap_or_default();
    let linked_fragment_node_ref_exports = final_component_selection
        .as_ref()
        .map(final_plan::linked_fragment_node_ref_exports)
        .unwrap_or_default();
    let linked_fragment_node_ref_imports = final_component_selection
        .as_ref()
        .map(final_plan::linked_fragment_node_ref_imports)
        .unwrap_or_default();
    let linked_payload_handler_exports = if linked_fragment_active {
        linked_payload_handler_exports(&island, &direct_payload_handler_values)?
    } else {
        Vec::new()
    };
    let needs_component_fragment_js = if final_component_selection.is_some() {
        // Final owner partitioning is authoritative. Latent participant
        // bindings (for example a direct live prop read) may make the local
        // descriptor look browser-capable without activating any concrete
        // occurrence. Emit a fragment only when at least one final site
        // actually belongs to an owner.
        linked_fragment_active
    } else {
        needs_client_js
    };
    let linked_owner_plans = final_component_selection
        .as_ref()
        .map(final_plan::linked_owner_module_plans)
        .transpose()
        .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?
        .unwrap_or_default();
    let linked_owner_assets = linked_owner_plans
        .iter()
        .map(|plan| {
            let node_ref_aliases = final_component_selection
                .as_ref()
                .map(|selection| final_plan::linked_owner_node_ref_aliases(selection, plan))
                .transpose()
                .map_err(|message| syn::Error::new(proc_macro2::Span::call_site(), message))?
                .unwrap_or_default();
            let js = rsjs_ir::emit_linked_owner_module_with_node_refs(plan, &node_ref_aliases)
                .map_err(|error| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!("rsjs/linked-owner-emission: {error}"),
                    )
                })?;
            reject_unlinked_helper_sentinel("linked owner module", &js)?;
            Ok((plan, js))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    server_program::validate_lossless_rewrite(&island, needs_client_js)?;
    // Descriptor relocation records must cover every helper that a later
    // final owner can make live. Compute that conservative pre-pass closure
    // with the same recursive analysis used by final fragment emission. This
    // also supplies the complete signal-retention set: retaining only an
    // exported derived signal would let DSE delete one of its dependencies
    // before the exact post-pass closure is constructed.
    let mut potential_forced_signal_ids = direct_component_server_signal_ids
        .iter()
        .copied()
        .collect::<Vec<_>>();
    potential_forced_signal_ids.extend_from_slice(&linked_fragment_signal_exports);
    potential_forced_signal_ids.sort_by_key(|signal| signal.0);
    potential_forced_signal_ids.dedup();
    let mut potential_forced_node_ref_ids = direct_component_server_node_ref_ids
        .iter()
        .copied()
        .collect::<Vec<_>>();
    potential_forced_node_ref_ids.extend_from_slice(&linked_fragment_node_ref_exports);
    potential_forced_node_ref_ids.extend_from_slice(&linked_fragment_node_ref_imports);
    potential_forced_node_ref_ids.sort_by_key(|node_ref| node_ref.0);
    potential_forced_node_ref_ids.dedup();
    let potential_lifecycle_expression_ids = linked_fragment_lifecycle_exports
        .iter()
        .map(|control| control.producer_expression_id)
        .collect::<Vec<_>>();
    let potential_reachability = rsjs_ir::client_reachability_roots_with_linked_roots(
        &island,
        &potential_forced_signal_ids,
        &potential_forced_node_ref_ids,
        &potential_lifecycle_expression_ids,
        direct_payload_handler_values
            .iter()
            .map(|value| &value.body_ir),
    );
    let mut reachable_signal_ids = potential_reachability
        .iter()
        .filter_map(|root| match root {
            rsjs_ir::ClientReachabilityRoot::Signal { id } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    reachable_signal_ids.sort_by_key(|signal| signal.0);
    reachable_signal_ids.dedup();
    analyze::prune_dead_signal_seed_inputs(&mut inbound_inputs, &reachable_signal_ids);
    let mut reachable_query_ids = potential_reachability
        .iter()
        .filter_map(|root| match root {
            rsjs_ir::ClientReachabilityRoot::Query { id } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut reachable_mutation_ids = potential_reachability
        .iter()
        .filter_map(|root| match root {
            rsjs_ir::ClientReachabilityRoot::Mutation { id } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut reachable_client_module_imports = potential_reachability
        .iter()
        .filter_map(|root| match root {
            rsjs_ir::ClientReachabilityRoot::ClientModuleCall { module, .. } => {
                Some(module.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut reachable_helper_call_edge_ids = potential_reachability
        .iter()
        .filter_map(|root| match root {
            rsjs_ir::ClientReachabilityRoot::ClientHelperCall { edge, .. } => Some(*edge),
            _ => None,
        })
        .collect::<Vec<_>>();
    reachable_query_ids.sort_by_key(|query| query.0);
    reachable_query_ids.dedup();
    reachable_mutation_ids.sort_by_key(|mutation| mutation.0);
    reachable_mutation_ids.dedup();
    reachable_client_module_imports.sort();
    reachable_client_module_imports.dedup();
    reachable_helper_call_edge_ids.sort_by_key(|edge| edge.index());
    reachable_helper_call_edge_ids.dedup();
    let reachable_component_fragment_imports =
        strategy_analysis.reachable_component_fragment_imports();
    let retained_linked_signals = island
        .signals
        .iter()
        .filter(|signal| reachable_signal_ids.contains(&signal.id))
        .cloned()
        .collect::<Vec<_>>();
    let retained_direct_node_refs = island
        .node_refs
        .iter()
        .filter(|node_ref| direct_component_server_node_ref_ids.contains(&node_ref.id))
        .cloned()
        .collect::<Vec<_>>();
    rsjs_ir::default_passes().run_all(&mut island);
    for signal in retained_linked_signals {
        if !island
            .signals
            .iter()
            .any(|existing| existing.id == signal.id)
        {
            island.signals.push(signal);
        }
    }
    island.signals.sort_by_key(|signal| signal.id.0);
    for node_ref in retained_direct_node_refs {
        if !island
            .node_refs
            .iter()
            .any(|existing| existing.id == node_ref.id)
        {
            island.node_refs.push(node_ref);
        }
    }
    island.node_refs.sort_by_key(|node_ref| node_ref.id.0);
    let render_start = parsed.render_sig.ident.span().start();
    let strategy_source = StrategySourceInfo {
        impl_start_line: impl_start.line,
        impl_start_column: impl_start.column + 1,
        render_start_line: render_start.line,
        render_start_column: render_start.column + 1,
    };

    // Hydrated islands bake JS into the manifest registration below. Plain
    // analysis builds may also mirror it for external tooling; orchestrated
    // phases remain object/registration-only. Server-only islands keep their
    // embedded descriptor but intentionally have no runtime JS registration.
    let js = if needs_client_js {
        rsjs_ir::emit_js::emit(&island, &asset_id)
    } else {
        String::new()
    };
    let fragment_js = if needs_component_fragment_js {
        if let Some(selection) = &final_component_selection {
            rsjs_ir::emit_js::try_emit_fragment_resolving_helpers_and_node_refs(
                &island,
                &fragment_asset_id,
                &linked_fragment_signal_exports,
                &linked_payload_handler_exports,
                &linked_fragment_lifecycle_exports,
                &linked_fragment_node_ref_exports,
                &linked_fragment_node_ref_imports,
                |exact_helper_edges| {
                    final_plan::linked_helper_program(
                        selection,
                        exact_helper_edges.iter().copied().map(|edge| {
                            (
                                edge,
                                helper_source_edge_id(&input_fingerprint, edge.index()),
                            )
                        }),
                    )
                    .map_err(|message| format!("rsjs/final-helper-link: {message}"))
                },
            )
            .map_err(|error| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("rsjs/final-helper-emission: {error}"),
                )
            })?
        } else {
            rsjs_ir::emit_js::emit_fragment(&island, &asset_id)
        }
    } else {
        String::new()
    };
    if final_component_selection.is_some() && !fragment_js.is_empty() {
        reject_unlinked_helper_sentinel("final component fragment", &fragment_js)?;
    }
    let fragment_export_name = if needs_component_fragment_js {
        rsjs_ir::emit_js::component_fragment_export_name(&fragment_asset_id)
    } else {
        String::new()
    };
    // Plain analysis builds retain the legacy filesystem mirrors used by
    // `cargo proa rsjs explain` and low-level compiler tooling. Descriptor and
    // final compilation already carry their authoritative data in object
    // records and manifest registrations. Writing mirrors into Cargo's
    // `OUT_DIR` in those phases would make a later unit identity depend on
    // stale output from an earlier expansion of that same unit.
    mirror_filesystem_artifacts(compilation_phase, || {
        if needs_client_js && final_component_selection.is_none() {
            write_js_asset(&asset_id, &js);
        }
        if needs_component_fragment_js {
            write_component_fragment_asset(&fragment_asset_id, &fragment_js);
        }
        for (plan, js) in &linked_owner_assets {
            write_js_asset(&plan.owner_asset_id, js);
        }
        write_client_ir_asset(&asset_id, &island);
        write_hir_asset(&asset_id, &hir_sidecar);
        write_strategy_asset(
            &asset_id,
            &struct_name,
            &parsed.self_ty.to_token_stream().to_string(),
            &input_fingerprint,
            strategy_source,
            parsed.client_origin_allowed,
            &island,
            &strategy_analysis,
            &island.hydration_strategy,
            &island.client_module_imports,
            &reachable_client_module_imports,
            needs_client_js,
            !reachable_query_ids.is_empty(),
            !reachable_mutation_ids.is_empty(),
        );
    });
    let mut descriptor_island = island.clone();
    analyze::expand_server_prelude_locals_for_descriptor(
        &mut descriptor_island,
        &parsed.prelude_lets,
    )?;
    let descriptor_query_initializers: Vec<_> = query_initializers
        .iter()
        .map(|init| emit_rust::QueryInitializer {
            id: init.id,
            expr: analyze::expand_server_prelude_expr_for_descriptor(
                &init.expr,
                &parsed.prelude_lets,
            ),
            key_expr: init.key_expr.as_ref().map(|key_expr| {
                analyze::expand_server_prelude_expr_for_descriptor(key_expr, &parsed.prelude_lets)
            }),
        })
        .collect();
    let descriptor_mutation_initializers: Vec<_> = mutation_initializers
        .iter()
        .map(|init| emit_rust::MutationInitializer {
            id: init.id,
            key_expr: analyze::expand_server_prelude_expr_for_descriptor(
                &init.key_expr,
                &parsed.prelude_lets,
            ),
            invalidates_expr: init.invalidates_expr.as_ref().map(|invalidates_expr| {
                analyze::expand_server_prelude_expr_for_descriptor(
                    invalidates_expr,
                    &parsed.prelude_lets,
                )
            }),
        })
        .collect();

    let component_descriptor_json = build_component_descriptor(
        &asset_id,
        &struct_name,
        &parsed,
        &descriptor_island,
        &js,
        &signal_initializers,
        &inbound_inputs,
        &shared_state_provider_sites,
        &descriptor_query_initializers,
        &descriptor_mutation_initializers,
    );
    mirror_filesystem_artifacts(compilation_phase, || {
        write_component_descriptor_asset(&asset_id, &component_descriptor_json);
    });
    let component_field_witnesses =
        component_field_witness_claims(&input_fingerprint, &inbound_inputs)?;

    let ide_helper_hints = collect_rsjs_ide_helper_hints(&impl_tokens_for_hash);
    let mut ide_source_local_indices = client_prelude_local_indices.clone();
    ide_source_local_indices.extend(shared_prelude_local_indices.iter().copied());
    for (index, local) in parsed.prelude_lets.iter().enumerate() {
        if server_program::is_dual_use_inbound_alias(local) {
            // The real server body retains this authored binding. Replaying
            // it in the dead IDE-only source closure as well can move a
            // non-Copy `self.field` twice, so it is not a removed-source hint.
            ide_source_local_indices.remove(&index);
        }
    }
    let query_source_ide_hints = collect_lossless_query_source_ide_hints(
        &parsed.prelude_lets,
        &prelude_query_ids,
        &reachable_query_ids,
    );
    let inbound_live_field_ide_signals =
        inbound_live_field_ide_signal_substitutions(&inbound_inputs)?;
    let ide_source_hints = collect_rsjs_source_ide_hints(
        &parsed.prelude_statements,
        &parsed.html_tree,
        &ide_source_local_indices,
        &prelude_signal_ids,
        &emit_rust::lossless_rust_signal_ids(&island),
        &prelude_query_ids,
        &prelude_mutation_ids,
        &reachable_mutation_ids,
        &inbound_live_field_ide_signals,
    );
    let impl_tokens = server_program::emit_lossless_impl(
        &authored_item,
        &parsed,
        &island,
        &fragment_asset_id,
        &signal_initializers,
        &shared_state_initializers,
        &query_initializers,
        &mutation_initializers,
        &prelude_signal_ids,
        &prelude_shared_store_ids,
        &prelude_query_ids,
        &prelude_mutation_ids,
        &prelude_node_ref_ids,
        &client_prelude_local_indices,
        &query_source_ide_hints,
        &ide_helper_hints,
        &ide_source_hints,
        &server_expressions,
        &direct_component_site_nodes,
        &shared_state_provider_sites,
        &direct_component_server_signal_ids,
        &direct_component_server_node_ref_ids,
        final_component_selection.as_ref(),
    )?;
    let component_abi_tokens =
        emit_component_abi_definition(&parsed, &asset_id, &input_fingerprint, cargo_unit_id);
    let component_payload_tokens = emit_component_payload_record(
        &asset_id,
        &input_fingerprint,
        cargo_unit_id,
        &component_descriptor_json,
        &direct_component_edges,
        &direct_helper_edges,
        &reachable_helper_call_edge_ids,
        &component_field_witnesses,
        &shared_state_initializers,
        &shared_state_provider_sites,
        &direct_payload_handler_values,
        &parsed.hydration_strategy,
        parsed.hydration_strategy_explicit,
    )?;
    let component_edge_tokens = emit_component_edge_records(
        &asset_id,
        &input_fingerprint,
        &component_descriptor_json,
        &direct_component_edges,
        cargo_unit_id,
    )?;
    let helper_edge_tokens = emit_helper_edge_records(
        &asset_id,
        &input_fingerprint,
        &direct_helper_edges,
        &reachable_helper_call_edge_ids,
        cargo_unit_id,
    );
    let component_field_tokens = emit_component_field_witnesses(
        &parsed,
        &asset_id,
        cargo_unit_id,
        &component_field_witnesses,
    )?;
    let generic_static_slot_tokens =
        emit_generic_static_slot_validation(&parsed, &asset_id, generic_static_slot.as_ref())?;
    let shared_state_acquisition_tokens = emit_shared_state_acquisition_records(
        &asset_id,
        &input_fingerprint,
        cargo_unit_id,
        &shared_state_initializers,
    );
    let shared_state_provider_tokens = emit_shared_state_provider_records(
        &asset_id,
        &input_fingerprint,
        cargo_unit_id,
        &component_descriptor_json,
        &direct_component_edges,
        &shared_state_provider_sites,
    )?;
    let registration_tokens = if final_component_selection.is_some() {
        let mut tokens = TokenStream2::new();
        if needs_component_fragment_js {
            tokens.extend(emit_component_fragment_registration(
                &fragment_asset_id,
                &struct_name,
                &fragment_js,
                &fragment_export_name,
            ));
        }
        for (plan, js) in &linked_owner_assets {
            tokens.extend(emit_linked_owner_manifest_registration(plan, js));
        }
        tokens
    } else if needs_client_js {
        let uses_queries = !reachable_query_ids.is_empty();
        let uses_mutations = !reachable_mutation_ids.is_empty();
        emit_manifest_registration(
            &asset_id,
            &struct_name,
            &js,
            island.is_async,
            &island.hydration_strategy,
            uses_queries,
            uses_mutations,
            &parsed.struct_path,
            &reachable_client_module_imports,
            &reachable_component_fragment_imports,
            &fragment_js,
            &fragment_export_name,
        )
    } else {
        TokenStream2::new()
    };

    // Keep the rewritten authored impl as the compiled output. For editor
    // completions, workspace rust-analyzer config ignores the `rsjs` attribute
    // macro so the analyzer can use its tolerant parser on the original body.
    let mut out = impl_tokens;
    out.extend(component_abi_tokens);
    out.extend(component_payload_tokens);
    out.extend(component_edge_tokens);
    out.extend(helper_edge_tokens);
    out.extend(component_field_tokens);
    out.extend(generic_static_slot_tokens);
    out.extend(shared_state_acquisition_tokens);
    out.extend(shared_state_provider_tokens);
    out.extend(registration_tokens);
    out.extend(emit_island_shared_stores_impl(
        &parsed.self_ty,
        &shared_state_initializers,
    ));
    out.extend(final_plan::witness_tokens(
        &asset_id,
        final_component_selection.as_ref(),
    ));
    if emit_plain_build_guard {
        out.extend(emit_plain_cargo_build_guard(&asset_id));
    }
    Ok(out)
}

/// Make an unstaged codegen/link fail with an actionable symbol while keeping
/// ordinary `cargo check` and rust-analyzer metadata builds usable.
///
/// A proc macro cannot observe rustc's `--emit=metadata` versus link choice.
/// A retained constructor relocation is therefore the narrow phase boundary:
/// metadata compilation type-checks it, while Mach-O, ELF, and COFF linkers
/// must resolve the deliberately missing symbol. `cargo proa` omits this guard
/// in descriptor and final compilation. Test and rustdoc targets are excluded
/// so normal Rust test/doc tooling can continue to exercise provisional code;
/// deployable binaries still require the linked pipeline.
fn emit_plain_cargo_build_guard(asset_id: &str) -> TokenStream2 {
    let guard_ident = quote::format_ident!("__RSJS_PLAIN_CARGO_BUILD_GUARD_{asset_id}");

    quote! {
        #[cfg(all(not(test), not(doc)))]
        const _: () = {
            unsafe extern "C" {
                #[link_name = "__RSJS_ERROR_plain_cargo_build_is_not_deployable_use_cargo_proa_build"]
                fn __rsjs_plain_cargo_build_requires_linked_pipeline();
            }

            #[doc(hidden)]
            #[used]
            #[allow(non_upper_case_globals)]
            #[cfg_attr(
                target_vendor = "apple",
                unsafe(link_section = "__DATA,__mod_init_func")
            )]
            #[cfg_attr(
                all(unix, not(target_vendor = "apple")),
                unsafe(link_section = ".init_array")
            )]
            #[cfg_attr(target_os = "windows", unsafe(link_section = ".CRT$XCU"))]
            static #guard_ident: unsafe extern "C" fn() =
                __rsjs_plain_cargo_build_requires_linked_pipeline;
        };
    }
}

fn linked_payload_handler_exports(
    island: &rsjs_ir::Island,
    values: &[analyze::DirectPayloadHandlerValue],
) -> syn::Result<Vec<rsjs_ir::emit_js::FragmentPayloadHandlerExport>> {
    let declared_signals = island
        .signals
        .iter()
        .map(|signal| signal.id)
        .collect::<std::collections::HashSet<_>>();
    values
        .iter()
        .map(|value| {
            let dependencies = rsjs_ir::collect_client_block_external_dependencies(&value.body_ir)
                .map_err(|error| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "rsjs/linked-payload-handler-open-graph: payload handler prelude binding {} is not a closed client graph: {error:?}",
                            value.prelude_local_index
                        ),
                    )
                })?;
            let missing_signal = dependencies
                .signal_reads
                .iter()
                .chain(&dependencies.signal_writes)
                .find(|signal| !declared_signals.contains(signal));
            if let Some(signal) = missing_signal {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "rsjs/linked-payload-handler-missing-signal: payload handler prelude binding {} references producer signal {}, but that signal is absent from the component fragment",
                        value.prelude_local_index, signal.0
                    ),
                ));
            }
            let unsupported = !dependencies.queries.is_empty()
                || !dependencies.mutations.is_empty()
                || !dependencies.node_refs.is_empty()
                || !dependencies.callback_slots.is_empty()
                || !dependencies.helper_edges.is_empty()
                || !dependencies.client_module_calls.is_empty()
                || dependencies.uses_event_payload
                || dependencies.uses_node_payload;
            if unsupported {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "rsjs/linked-payload-handler-unsupported-dependency: payload handler prelude binding {} reaches query, mutation, DOM/node, helper, module, or event payload state that the linked-owner fragment ABI does not transport yet; keep those dependencies outside this callback until the corresponding typed transport witness is available",
                        value.prelude_local_index
                    ),
                ));
            }
            if dependencies.uses_callback_payload && value.payload_arity != 1 {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "rsjs/linked-payload-handler-arity: payload handler prelude binding {} has arity {}, but its Client IR payload use disagrees",
                        value.prelude_local_index, value.payload_arity
                    ),
                ));
            }
            Ok(rsjs_ir::emit_js::FragmentPayloadHandlerExport {
                producer_id: value.prelude_local_index,
                payload_arity: value.payload_arity,
                body: value.body_ir.clone(),
            })
        })
        .collect()
}

/// Emit one fixed relocation record for each direct struct-literal child.
///
/// The source edge ID is deterministic over the containing component and the
/// complete behavior-bearing occurrence tuple; it is intentionally not a
/// child identity. Rustc resolves the authored path in the generated
/// `__rsjs_emit_component_edge!` invocation, including ordinary imports,
/// renamed imports, and re-exports; the record relocation is the authoritative
/// relation to the child's defining witness.
fn emit_component_edge_records(
    asset_id: &str,
    input_fingerprint: &str,
    descriptor_json: &str,
    edges: &[analyze::DirectComponentSourceEdge],
    cargo_unit_id: [u8; 32],
) -> syn::Result<TokenStream2> {
    use quote::{format_ident, quote};

    let descriptor =
        serde_json::from_str::<serde_json::Value>(descriptor_json).map_err(|error| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("failed to authenticate direct component edge topology: {error}"),
            )
        })?;
    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let records = edges
        .iter()
        .map(|edge| {
            let record_ident =
                format_ident!("__RSJS_COMPONENT_EDGE_{}_{}", asset_id, edge.local_id);
            let source_edge_id = component_source_edge_id(input_fingerprint, edge, &descriptor)?
                .into_iter()
                .map(proc_macro2::Literal::u8_unsuffixed)
                .collect::<Vec<_>>();
            let component_ty = &edge.component_ty;

            Ok(quote! {
                ::rsjs::__private::__rsjs_emit_component_edge!(
                    #record_ident,
                    [#(#cargo_unit_id),*],
                    [#(#source_edge_id),*],
                    #component_ty
                );
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        #(#records)*
    })
}

fn component_source_edge_id(
    input_fingerprint: &str,
    edge: &analyze::DirectComponentSourceEdge,
    descriptor: &serde_json::Value,
) -> syn::Result<[u8; 32]> {
    let site_node = edge.site_node.ok_or_else(|| {
        syn::Error::new(
            edge.component_ty.span(),
            format!(
                "RSJS internal direct component edge {} has no authenticated view site",
                edge.local_id
            ),
        )
    })?;
    let lifecycle = transport_direct_component_lifecycle(edge.lifecycle, descriptor)?;
    let fields = edge
        .fields
        .iter()
        .map(|field| rsjs_transport::TransportDirectComponentField {
            name: &field.name,
            binding: transport_direct_component_binding(field.binding),
        })
        .collect::<Vec<_>>();
    Ok(rsjs_transport::component_source_edge_id(
        input_fingerprint,
        edge.local_id,
        &rsjs_transport::TransportDirectComponentEdge {
            boundary: match edge.boundary {
                analyze::DirectComponentBoundary::Composed => {
                    rsjs_transport::TransportDirectComponentBoundary::Composed
                }
                analyze::DirectComponentBoundary::ExplicitIsland => {
                    rsjs_transport::TransportDirectComponentBoundary::ExplicitIsland
                }
            },
            site_node: site_node.0,
            lifecycle,
            has_rest: edge.has_rest,
            fields: &fields,
        },
    ))
}

/// Emit one fixed relocation record for every SharedState store present in
/// this component descriptor.
///
/// The payload claims ownership by `(store_id, source_edge_id)`. Rustc resolves
/// the authored state type—including imports, aliases, and re-exports—through
/// `SharedStateAbi::WITNESS`; source path text is never an identity authority.
/// Records are emitted for dead acquisitions as well as reachable ones because
/// transport completeness and runtime activation are separate decisions.
fn emit_shared_state_acquisition_records(
    asset_id: &str,
    input_fingerprint: &str,
    cargo_unit_id: [u8; 32],
    initializers: &[emit_rust::SharedStateInitializer],
) -> TokenStream2 {
    use quote::{format_ident, quote};

    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let records = initializers.iter().map(|initializer| {
        let store_id = initializer.id.0;
        let record_ident =
            format_ident!("__RSJS_SHARED_STATE_ACQUISITION_{}_{}", asset_id, store_id);
        let source_edge_id = shared_state_source_edge_id(input_fingerprint, initializer.id)
            .into_iter()
            .map(proc_macro2::Literal::u8_unsuffixed)
            .collect::<Vec<_>>();
        let ty = &initializer.ty;

        quote! {
            #[doc(hidden)]
            #[used]
            #[allow(non_upper_case_globals)]
            #[cfg_attr(
                target_vendor = "apple",
                unsafe(link_section = "__DATA,__rsjs_sedge")
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
                unsafe(link_section = ".rsjs.shared_acquisition")
            )]
            #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$K"))]
            static #record_ident: ::rsjs::__private::SharedStateAcquisitionRecord<
                ::rsjs::__private::SharedStateAbiWitness,
            > = ::rsjs::__private::SharedStateAcquisitionRecord::new(
                [#(#cargo_unit_id),*],
                [#(#source_edge_id),*],
                <#ty as ::rsjs::__private::SharedStateAbi>::WITNESS,
            );
        }
    });

    quote! {
        #(#records)*
    }
}

fn shared_state_source_edge_id(
    input_fingerprint: &str,
    store_id: rsjs_ir::SharedStoreId,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.shared-state-acquisition.v1\0");
    hasher.update(input_fingerprint.as_bytes());
    hasher.update(store_id.0.to_le_bytes());
    hasher.finalize().into()
}

fn shared_state_provider_source_edge_id(
    input_fingerprint: &str,
    site: &analyze::DirectSharedStateProviderSite,
    child_source_edge_id: [u8; 32],
) -> [u8; 32] {
    rsjs_transport::shared_state_provider_source_edge_id(
        input_fingerprint,
        site.id.index(),
        &rsjs_transport::TransportSharedStateProviderSite {
            site_node: site.site_node.0,
            child_edge_ordinal: site.child_edge,
            child_source_edge_id,
        },
    )
}

/// Emit one provider-specific SharedState witness relocation for every static
/// compiler-recognized provider site.
///
/// The fixed object layout intentionally reuses
/// `SharedStateAcquisitionRecord`: both relations resolve one nominal
/// SharedState type through rustc. Their source-edge domains and component
/// payload claims remain distinct, so an acquisition relocation cannot be
/// substituted for provider topology.
fn emit_shared_state_provider_records(
    asset_id: &str,
    input_fingerprint: &str,
    cargo_unit_id: [u8; 32],
    descriptor_json: &str,
    component_edges: &[analyze::DirectComponentSourceEdge],
    sites: &[analyze::DirectSharedStateProviderSite],
) -> syn::Result<TokenStream2> {
    use quote::{format_ident, quote};

    let descriptor =
        serde_json::from_str::<serde_json::Value>(descriptor_json).map_err(|error| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("failed to authenticate generated SharedState provider topology: {error}"),
            )
        })?;
    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let records = sites
        .iter()
        .map(|site| {
            let child_edge = component_edges
                .iter()
                .find(|edge| edge.local_id == site.child_edge)
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "RSJS SharedState provider site {} refers to missing child edge {}",
                            site.id.index(),
                            site.child_edge
                        ),
                    )
                })?;
            let child_source_edge_id =
                component_source_edge_id(input_fingerprint, child_edge, &descriptor)?;
            let source_edge_id =
                shared_state_provider_source_edge_id(input_fingerprint, site, child_source_edge_id)
                    .into_iter()
                    .map(proc_macro2::Literal::u8_unsuffixed)
                    .collect::<Vec<_>>();
            let record_ident = format_ident!(
                "__RSJS_SHARED_STATE_PROVIDER_{}_{}",
                asset_id,
                site.id.index()
            );
            let state_ty = &site.state_ty;

            Ok(quote! {
                #[doc(hidden)]
                #[used]
                #[allow(non_upper_case_globals)]
                #[cfg_attr(
                    target_vendor = "apple",
                    unsafe(link_section = "__DATA,__rsjs_sedge")
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
                    unsafe(link_section = ".rsjs.shared_acquisition")
                )]
                #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$K"))]
                static #record_ident: ::rsjs::__private::SharedStateAcquisitionRecord<
                    ::rsjs::__private::SharedStateAbiWitness,
                > = ::rsjs::__private::SharedStateAcquisitionRecord::new(
                    [#(#cargo_unit_id),*],
                    [#(#source_edge_id),*],
                    <#state_ty as ::rsjs::__private::SharedStateAbi>::WITNESS,
                );
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        #(#records)*
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ComponentFieldWitnessClaim {
    field_id: u32,
    field_name: String,
    crossing_hook_id: u32,
    role: rsjs_transport::TransportComponentFieldRole,
    callback_shape: rsjs_transport::TransportCallbackShape,
    node_ref_exact_target: Option<rsjs_ir::dom_registry::DomElementKind>,
    field_record_id: [u8; 32],
}

fn component_field_witness_claims(
    input_fingerprint: &str,
    inbound_inputs: &analyze::InboundInputMetadata,
) -> syn::Result<Vec<ComponentFieldWitnessClaim>> {
    use rsjs_transport::{TransportCallbackShape, TransportComponentFieldRole};

    inbound_inputs
        .fields
        .iter()
        .enumerate()
        .map(|(ordinal, field)| {
            if field.id as usize != ordinal {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal component-field metadata is not dense: ordinal {ordinal}, field id {}",
                        field.id
                    ),
                ));
            }
            let crossing = inbound_inputs
                .crossings
                .get(field.crossing as usize)
                .filter(|crossing| crossing.id == field.crossing && crossing.field == field.id)
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "RSJS internal component-field metadata for `{}` references invalid crossing {}",
                            field.name, field.crossing
                        ),
                    )
                })?;
            if field.value.is_none() {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal client-visible field `{}` has no symbolic value id",
                        field.name
                    ),
                ));
            }

            let (
                role,
                callback_shape,
                expected_crossing,
                node_ref_exact_target,
            ) = match field.role {
                analyze::InboundFieldRole::Value => {
                    if field.callback_shape.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!(
                                "RSJS internal value field `{}` unexpectedly carries a callback shape",
                                field.name
                            ),
                        ));
                    }
                    (
                        TransportComponentFieldRole::Value,
                        TransportCallbackShape::NotCallback,
                        analyze::InboundCrossingKind::Value,
                        None,
                    )
                }
                analyze::InboundFieldRole::Callback => {
                    let shape = match field.callback_shape {
                        Some(analyze::InboundCallbackShape::Required) => {
                            TransportCallbackShape::Required
                        }
                        Some(analyze::InboundCallbackShape::Optional) => {
                            TransportCallbackShape::Optional
                        }
                        None => {
                            return Err(syn::Error::new(
                                proc_macro2::Span::call_site(),
                                format!(
                                    "RSJS internal callback field `{}` has no required/optional shape",
                                    field.name
                                ),
                            ));
                        }
                    };
                    (
                        TransportComponentFieldRole::Callback,
                        shape,
                        analyze::InboundCrossingKind::CallbackResolveCurrent,
                        None,
                    )
                }
                analyze::InboundFieldRole::NodeRef => {
                    if field.callback_shape.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!(
                                "RSJS internal node-ref field `{}` unexpectedly carries a callback shape",
                                field.name
                            ),
                        ));
                    }
                    let matching_handles = inbound_inputs
                        .node_ref_fields
                        .iter()
                        .filter(|handle| handle.field == field.id)
                        .collect::<Vec<_>>();
                    if matching_handles.len() != 1 {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!(
                                "RSJS internal node-ref field `{}` must have exactly one handle record",
                                field.name
                            ),
                        ));
                    }
                    (
                        TransportComponentFieldRole::NodeRef,
                        TransportCallbackShape::NotCallback,
                        analyze::InboundCrossingKind::NodeRefAlias,
                        matching_handles[0].exact_bind_target,
                    )
                }
                analyze::InboundFieldRole::DeferredProjection => {
                    if field.callback_shape.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!(
                                "RSJS internal deferred projection field `{}` unexpectedly carries a callback shape",
                                field.name
                            ),
                        ));
                    }
                    let matching_projections = inbound_inputs
                        .projection_fields
                        .iter()
                        .filter(|projection| projection.field == field.id)
                        .count();
                    if matching_projections != 1 {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!(
                                "RSJS internal deferred projection field `{}` must have exactly one latent projection record",
                                field.name
                            ),
                        ));
                    }
                    (
                        TransportComponentFieldRole::DeferredProjection,
                        TransportCallbackShape::NotCallback,
                        analyze::InboundCrossingKind::DeferredProjection,
                        None,
                    )
                }
            };
            if crossing.kind != expected_crossing {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal component-field crossing kind disagrees with `{}`",
                        field.name
                    ),
                ));
            }
            let field_record_id = rsjs_transport::component_field_record_id(
                input_fingerprint,
                field.id,
                field.crossing,
                role,
                callback_shape,
            );
            Ok(ComponentFieldWitnessClaim {
                field_id: field.id,
                field_name: field.name.clone(),
                crossing_hook_id: field.crossing,
                role,
                callback_shape,
                node_ref_exact_target,
                field_record_id,
            })
        })
        .collect()
}

fn emit_component_field_witnesses(
    parsed: &parse::ParsedIsland,
    asset_id: &str,
    cargo_unit_id: [u8; 32],
    claims: &[ComponentFieldWitnessClaim],
) -> syn::Result<TokenStream2> {
    use quote::{format_ident, quote};

    if claims.is_empty() {
        return Ok(TokenStream2::new());
    }

    let witness_ident = format_ident!("__RSJS_COMPONENT_ABI_WITNESS_{}", asset_id);
    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let records = claims.iter().map(|claim| {
        let record_ident = format_ident!("__RSJS_COMPONENT_FIELD_{}_{}", asset_id, claim.field_id);
        let field_record_id = claim
            .field_record_id
            .iter()
            .map(|byte| proc_macro2::Literal::u8_unsuffixed(*byte))
            .collect::<Vec<_>>();
        quote! {
            ::rsjs::__private::__rsjs_emit_component_field!(
                #record_ident,
                [#(#cargo_unit_id),*],
                [#(#field_record_id),*],
                &#witness_ident
            );
        }
    });

    let checks = claims
        .iter()
        .map(|claim| {
            let field_ident = syn::parse_str::<syn::Ident>(&claim.field_name).map_err(|error| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS could not project analyzed component field `{}`: {error}",
                        claim.field_name
                    ),
                )
            })?;
            let assertion = match (claim.role, claim.callback_shape) {
                (
                    rsjs_transport::TransportComponentFieldRole::Value,
                    rsjs_transport::TransportCallbackShape::NotCallback,
                ) => quote!(::rsjs::__private::assert_component_value_field),
                (
                    rsjs_transport::TransportComponentFieldRole::Callback,
                    rsjs_transport::TransportCallbackShape::Required,
                ) => quote!(::rsjs::__private::assert_required_component_callback),
                (
                    rsjs_transport::TransportComponentFieldRole::Callback,
                    rsjs_transport::TransportCallbackShape::Optional,
                ) => quote!(::rsjs::__private::assert_optional_component_callback),
                (
                    rsjs_transport::TransportComponentFieldRole::NodeRef,
                    rsjs_transport::TransportCallbackShape::NotCallback,
                ) => quote!(::rsjs::__private::assert_component_node_ref_field),
                (
                    rsjs_transport::TransportComponentFieldRole::DeferredProjection,
                    rsjs_transport::TransportCallbackShape::NotCallback,
                ) => quote!(::rsjs::__private::assert_component_value_field),
                _ => {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "RSJS internal component-field role/shape mismatch for `{}`",
                            claim.field_name
                        ),
                    ));
                }
            };
            let node_ref_target_assertion =
                claim
                    .node_ref_exact_target
                    .map_or_else(TokenStream2::new, |target| {
                        let target = dom_element_kind_source_ide_type(target);
                        quote! {
                            ::rsjs::__private::assert_component_node_ref_target::<_, #target>(
                                &self.#field_ident
                            );
                        }
                    });
            Ok(quote! {
                #assertion(&self.#field_ident);
                #node_ref_target_assertion
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let self_ty = &parsed.self_ty;
    let validation_generics = component_abi_impl_generics(&parsed.impl_generics, &parsed.self_ty);
    let (impl_generics, _, where_clause) = validation_generics.split_for_impl();
    let validation_method_ident = format_ident!("__rsjs_validate_component_fields_{}", asset_id);

    Ok(quote! {
        #(#records)*

        #[doc(hidden)]
        #[automatically_derived]
        impl #impl_generics #self_ty #where_clause {
            #[doc(hidden)]
            #[allow(dead_code)]
            fn #validation_method_ident(&self) {
                #(#checks)*
            }
        }
    })
}

/// Prove that the analyzer's one open-generic immutable slot is the component
/// field whose Rust type it claims to be.
///
/// This emits no component-field relocation and no client codec obligation.
/// The method is never called, but rustc still type-checks its body. That keeps
/// the proc macro from guessing that an arbitrary `self.field` has the open
/// generic type selected from `Self`.
fn emit_generic_static_slot_validation(
    parsed: &parse::ParsedIsland,
    asset_id: &str,
    slot: Option<&analyze::GenericStaticSlotField>,
) -> syn::Result<TokenStream2> {
    use quote::{format_ident, quote};

    let Some(slot) = slot else {
        return Ok(TokenStream2::new());
    };
    let field_ident = syn::parse_str::<syn::Ident>(&slot.name).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "RSJS could not project inferred static-slot field `{}`: {error}",
                slot.name
            ),
        )
    })?;
    let field_ty = &slot.ty;
    let self_ty = &parsed.self_ty;
    let validation_generics = component_abi_impl_generics(&parsed.impl_generics, self_ty);
    let (impl_generics, _, where_clause) = validation_generics.split_for_impl();
    let validation_method_ident = format_ident!("__rsjs_validate_generic_static_slot_{}", asset_id);

    Ok(quote! {
        #[doc(hidden)]
        #[automatically_derived]
        impl #impl_generics #self_ty #where_clause {
            #[doc(hidden)]
            #[allow(dead_code)]
            fn #validation_method_ident(&self) {
                let _: *const #field_ty = ::core::ptr::addr_of!(self.#field_ident);
            }
        }
    })
}

fn direct_component_server_signal_ids(
    edges: &[analyze::DirectComponentSourceEdge],
    server_expressions: &std::collections::HashMap<rsjs_ir::NodeId, syn::Expr>,
) -> std::collections::HashSet<rsjs_ir::SignalId> {
    use syn::visit::Visit;

    struct SnapshotFinder {
        signals: std::collections::HashSet<rsjs_ir::SignalId>,
    }

    impl<'ast> Visit<'ast> for SnapshotFinder {
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            const PREFIX: &str = "__rsjs_initial_signal_";
            if let Some(ident) = path.path.get_ident() {
                if let Some(index) = ident.to_string().strip_prefix(PREFIX) {
                    if let Ok(index) = index.parse::<u32>() {
                        self.signals.insert(rsjs_ir::SignalId(index));
                    }
                }
            }
            syn::visit::visit_expr_path(self, path);
        }
    }

    let mut finder = SnapshotFinder {
        signals: edges
            .iter()
            .flat_map(|edge| edge.fields.iter())
            .filter_map(|field| match field.binding {
                analyze::DirectComponentBindingClassification::LiveValue { signal, .. } => {
                    Some(signal)
                }
                analyze::DirectComponentBindingClassification::DeferredProjection {
                    signal,
                    ..
                } => Some(signal),
                _ => None,
            })
            .collect(),
    };
    for expression in server_expressions.values() {
        finder.visit_expr(expression);
    }
    finder.signals
}

fn direct_component_server_node_ref_ids(
    edges: &[analyze::DirectComponentSourceEdge],
) -> std::collections::HashSet<rsjs_ir::NodeRefId> {
    edges
        .iter()
        .flat_map(|edge| edge.fields.iter())
        .filter_map(|field| match field.binding {
            analyze::DirectComponentBindingClassification::NodeRefHandle { node_ref } => {
                Some(node_ref)
            }
            analyze::DirectComponentBindingClassification::DeferredProjection {
                node_ref, ..
            } => Some(node_ref),
            _ => None,
        })
        .collect()
}

/// Emit a fixed relocation record for every browser-reachable helper call.
///
/// The authored path is passed directly to the hidden macro so rustc—not the
/// proc macro—resolves same-module names, imports, aliases, and re-exports to
/// the helper's defining function symbol.
fn emit_helper_edge_records(
    asset_id: &str,
    input_fingerprint: &str,
    edges: &[analyze::DirectHelperSourceEdge],
    reachable: &[rsjs_ir::abi::HelperCallEdgeId],
    cargo_unit_id: [u8; 32],
) -> TokenStream2 {
    use quote::{format_ident, quote};

    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let records = edges
        .iter()
        .filter(|edge| {
            reachable
                .iter()
                .any(|reachable_edge| reachable_edge.index() == edge.local_id)
        })
        .map(|edge| {
            let record_ident =
                format_ident!("__RSJS_CLIENT_HELPER_EDGE_{}_{}", asset_id, edge.local_id);
            let source_edge_id = helper_source_edge_id(input_fingerprint, edge.local_id)
                .into_iter()
                .map(proc_macro2::Literal::u8_unsuffixed)
                .collect::<Vec<_>>();
            let helper_path = &edge.helper_path;
            let return_witness = edge.expected_return_domain.map(|domain| {
                let witness_ident = format_ident!(
                    "__RSJS_CLIENT_HELPER_RETURN_WITNESS_{}_{}",
                    asset_id,
                    edge.local_id
                );
                let return_ty = match domain {
                    rsjs_ir::IntegerDomain::I32 => quote! { ::core::primitive::i32 },
                    rsjs_ir::IntegerDomain::U32 => quote! { ::core::primitive::u32 },
                    rsjs_ir::IntegerDomain::I64 => quote! { ::core::primitive::i64 },
                    rsjs_ir::IntegerDomain::U64 => quote! { ::core::primitive::u64 },
                };
                let inferred_args = (0..edge.argument_count)
                    .map(|_| quote! { _ })
                    .collect::<Vec<_>>();
                quote! {
                    // Contextual fixed-width typing is recorded in Client IR,
                    // but rustc remains the authority on the imported helper's
                    // actual signature. A function-pointer coercion validates
                    // aliases and re-exports without executing any code.
                    #[doc(hidden)]
                    #[allow(non_upper_case_globals)]
                    const #witness_ident: () = {
                        let _: fn(#(#inferred_args),*) -> #return_ty = #helper_path;
                    };
                }
            });

            quote! {
                ::rsjs::__private::__rsjs_emit_client_helper_edge!(
                    #record_ident,
                    [#(#cargo_unit_id),*],
                    [#(#source_edge_id),*],
                    #helper_path
                );
                #return_witness
            }
        });

    quote! {
        #(#records)*
    }
}

fn helper_source_edge_id(input_fingerprint: &str, local_id: u32) -> [u8; 32] {
    rsjs_transport::component_helper_source_edge_id(input_fingerprint, local_id)
}

fn component_record_id(input_fingerprint: &str) -> [u8; 32] {
    rsjs_transport::component_record_id(input_fingerprint)
}

/// Embed the complete deterministic component descriptor beside its defining
/// relocation record.
///
/// The linker joins the two records by `(CargoUnitId, component_record_id)`.
/// The JSON sidecar remains a human/tooling diagnostic only; it is never the
/// authoritative input once this payload is present.
fn emit_component_payload_record(
    asset_id: &str,
    input_fingerprint: &str,
    cargo_unit_id: [u8; 32],
    descriptor_json: &str,
    component_edges: &[analyze::DirectComponentSourceEdge],
    helper_edges: &[analyze::DirectHelperSourceEdge],
    reachable_helper_edges: &[rsjs_ir::abi::HelperCallEdgeId],
    component_field_witnesses: &[ComponentFieldWitnessClaim],
    shared_state_initializers: &[emit_rust::SharedStateInitializer],
    shared_state_provider_sites: &[analyze::DirectSharedStateProviderSite],
    direct_payload_handler_values: &[analyze::DirectPayloadHandlerValue],
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    hydration_strategy_explicit: bool,
) -> syn::Result<TokenStream2> {
    use proc_macro2::Span;
    use quote::{format_ident, quote};

    let component_record_id = component_record_id(input_fingerprint);
    let payload_json = component_transport_payload_json(
        descriptor_json,
        input_fingerprint,
        component_record_id,
        component_edges,
        helper_edges,
        reachable_helper_edges,
        component_field_witnesses,
        shared_state_initializers,
        shared_state_provider_sites,
        direct_payload_handler_values,
        hydration_strategy,
        hydration_strategy_explicit,
    )?;
    let encoded = rsjs_transport::encode_payload_record(
        rsjs_transport::PayloadKind::ComponentDescriptor,
        cargo_unit_id,
        component_record_id,
        payload_json.as_bytes(),
    );
    let payload_len = syn::LitInt::new(&encoded.len().to_string(), Span::call_site());
    let payload = syn::LitByteStr::new(&encoded, Span::call_site());
    let payload_ident = format_ident!("__RSJS_COMPONENT_PAYLOAD_{}", asset_id);

    Ok(quote! {
        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_cpay")
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
            unsafe(link_section = ".rsjs.component_payload")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$P"))]
        static #payload_ident: ::rsjs::__private::PayloadRecord<#payload_len> =
            ::rsjs::__private::PayloadRecord::new(*#payload);
    })
}

fn component_transport_payload_json(
    descriptor_json: &str,
    input_fingerprint: &str,
    definition_record_id: [u8; 32],
    component_edges: &[analyze::DirectComponentSourceEdge],
    helper_edges: &[analyze::DirectHelperSourceEdge],
    reachable_helper_edges: &[rsjs_ir::abi::HelperCallEdgeId],
    component_field_witnesses: &[ComponentFieldWitnessClaim],
    shared_state_initializers: &[emit_rust::SharedStateInitializer],
    shared_state_provider_sites: &[analyze::DirectSharedStateProviderSite],
    direct_payload_handler_values: &[analyze::DirectPayloadHandlerValue],
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    hydration_strategy_explicit: bool,
) -> syn::Result<String> {
    let descriptor =
        serde_json::from_str::<serde_json::Value>(descriptor_json).map_err(|error| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("failed to embed the generated RSJS component descriptor: {error}"),
            )
        })?;
    let component_edge_claims = component_edges
        .iter()
        .map(|edge| {
            let site_node = edge.site_node.ok_or_else(|| {
                syn::Error::new(
                    edge.component_ty.span(),
                    format!(
                        "RSJS internal direct component edge {} has no preserved schema-v9 view site",
                        edge.local_id
                    ),
                )
            })?;
            let lifecycle = direct_component_lifecycle_json(edge.lifecycle, &descriptor)?;
            let fields = edge
                .fields
                .iter()
                .enumerate()
                .map(|(field_ordinal, field)| {
                    serde_json::json!({
                        "binding": direct_component_binding_json(field.binding),
                        "field_ordinal": field_ordinal,
                        "name": field.name,
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({
                "authored_path": edge.component_ty.to_token_stream().to_string(),
                "boundary": edge.boundary.as_str(),
                "fields": fields,
                "has_rest": edge.has_rest,
                "identity_authority": "rustc_component_witness_relocation",
                "lifecycle": lifecycle,
                "local_ordinal": edge.local_id,
                "site_node": site_node.0,
                "source_anchor": {
                    "column": edge.source_anchor.start_column,
                    "line": edge.source_anchor.start_line,
                },
                "source_edge_id": transport_id_hex(&component_source_edge_id(
                    input_fingerprint,
                    edge,
                    &descriptor,
                )?),
            }))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let helper_edges = helper_edges
        .iter()
        .filter(|edge| {
            reachable_helper_edges
                .iter()
                .any(|reachable| reachable.index() == edge.local_id)
        })
        .map(|edge| {
            serde_json::json!({
                "authored_path": edge.helper_path.to_token_stream().to_string(),
                "identity_authority": "rustc_function_relocation",
                "local_ordinal": edge.local_id,
                "source_edge_id": transport_id_hex(&helper_source_edge_id(
                    input_fingerprint,
                    edge.local_id,
                )),
            })
        })
        .collect::<Vec<_>>();
    let component_field_witnesses = component_field_witnesses
        .iter()
        .map(|claim| {
            let role = match claim.role {
                rsjs_transport::TransportComponentFieldRole::Value => "value",
                rsjs_transport::TransportComponentFieldRole::Callback => "callback",
                rsjs_transport::TransportComponentFieldRole::NodeRef => "node_ref",
                rsjs_transport::TransportComponentFieldRole::DeferredProjection => {
                    "deferred_projection"
                }
            };
            let callback_shape = match claim.callback_shape {
                rsjs_transport::TransportCallbackShape::NotCallback => "not_callback",
                rsjs_transport::TransportCallbackShape::Required => "required",
                rsjs_transport::TransportCallbackShape::Optional => "optional",
            };
            serde_json::json!({
                "callback_shape": callback_shape,
                "crossing_hook_id": claim.crossing_hook_id,
                "field_id": claim.field_id,
                "field_name": claim.field_name,
                "field_record_id": transport_id_hex(&claim.field_record_id),
                "role": role,
            })
        })
        .collect::<Vec<_>>();
    let shared_state_acquisitions = shared_state_initializers
        .iter()
        .enumerate()
        .map(|(ordinal, initializer)| {
            let expected = u32::try_from(ordinal).map_err(|_| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "RSJS SharedState acquisition count exceeds the canonical u32 ID space",
                )
            })?;
            if initializer.id.0 != expected {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal SharedState acquisition metadata is not dense: ordinal \
                         {ordinal}, store id {}",
                        initializer.id.0
                    ),
                ));
            }
            Ok(serde_json::json!({
                "source_edge_id": transport_id_hex(&shared_state_source_edge_id(
                    input_fingerprint,
                    initializer.id,
                )),
                "store_id": initializer.id.0,
            }))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let mut claimed_provider_children = std::collections::BTreeSet::new();
    let shared_state_provider_sites = shared_state_provider_sites
        .iter()
        .enumerate()
        .map(|(ordinal, site)| {
            let expected = u32::try_from(ordinal).map_err(|_| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "RSJS SharedState provider-site count exceeds the canonical u32 ID space",
                )
            })?;
            if site.id.index() != expected {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal SharedState provider-site metadata is not dense: ordinal \
                         {ordinal}, site id {}",
                        site.id.index()
                    ),
                ));
            }
            let child_edge = component_edges
                .iter()
                .find(|edge| edge.local_id == site.child_edge)
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "RSJS SharedState provider site {} refers to missing child edge {}",
                            site.id.index(),
                            site.child_edge
                        ),
                    )
                })?;
            if !claimed_provider_children.insert(site.child_edge) {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS SharedState provider child edge {} is claimed by more than one \
                         provider site",
                        site.child_edge
                    ),
                ));
            }
            if !matches!(
                child_edge.boundary,
                analyze::DirectComponentBoundary::Composed
            ) || !matches!(
                child_edge.lifecycle,
                analyze::DirectComponentLifecycle::StaticSingle
            ) || child_edge.has_rest
                || child_edge.site_node != Some(site.site_node)
                || child_edge.fields.iter().any(|field| {
                    !matches!(
                        field.binding,
                        analyze::DirectComponentBindingClassification::ServerOnly
                            | analyze::DirectComponentBindingClassification::StaticCallback { .. }
                    )
                })
            {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS SharedState provider site {} must own one unique static composed \
                         child edge at server node {} with only server/static-callback fields",
                        site.id.index(),
                        site.site_node.0
                    ),
                ));
            }
            let child_source_edge_id =
                component_source_edge_id(input_fingerprint, child_edge, &descriptor)?;
            let source_edge_id =
                shared_state_provider_source_edge_id(input_fingerprint, site, child_source_edge_id);
            Ok(serde_json::json!({
                "child_edge_local_ordinal": site.child_edge,
                "child_source_edge_id": transport_id_hex(&child_source_edge_id),
                "identity_authority": "rustc_shared_state_witness_relocation",
                "provider_site_id": site.id.index(),
                "site_node": site.site_node.0,
                "source_anchor": {
                    "column": site.source_anchor.start_column,
                    "line": site.source_anchor.start_line,
                },
                "source_edge_id": transport_id_hex(&source_edge_id),
                "state_rust_type": site.state_ty.to_token_stream().to_string(),
                "topology": "static_single_wrapperless",
            }))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let direct_payload_handler_values = direct_payload_handler_values
        .iter()
        .map(direct_payload_handler_value_json)
        .collect::<syn::Result<Vec<_>>>()?;

    let mut payload = serde_json::json!({
        "component_descriptor": descriptor,
        "format": "rsjs.component_transport_payload",
        "schema_version": 9,
        "transport": {
            "client_value_sources": {
                "payload_handlers": direct_payload_handler_values,
            },
            "component_field_witnesses": component_field_witnesses,
            "component_input_fingerprint": input_fingerprint,
            "definition_record_id": transport_id_hex(&definition_record_id),
            "direct_component_edges": component_edge_claims,
            "hydration_constraint": hydration_constraint_json(
                hydration_strategy,
                hydration_strategy_explicit,
            ),
            "reachable_helper_edges": helper_edges,
            "shared_state_acquisitions": shared_state_acquisitions,
            "shared_state_provider_sites": shared_state_provider_sites,
        },
    });
    let semantic_bytes = rsjs_transport::canonical_json_bytes(&payload).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to serialize the RSJS component payload: {error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.component-transport-payload.semantic.v8\0");
    hasher.update(&semantic_bytes);
    let semantic_hash: [u8; 32] = hasher.finalize().into();
    payload
        .as_object_mut()
        .expect("component transport payload is an object")
        .insert(
            "semantic_hash".to_string(),
            serde_json::Value::String(format!("sha256:{}", transport_id_hex(&semantic_hash))),
        );
    let payload_bytes = rsjs_transport::canonical_json_bytes(&payload).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to serialize the RSJS component payload: {error}"),
        )
    })?;
    Ok(String::from_utf8(payload_bytes)
        .expect("canonical JSON serialization always produces valid UTF-8"))
}

fn direct_component_lifecycle_json(
    lifecycle: analyze::DirectComponentLifecycle,
    descriptor: &serde_json::Value,
) -> syn::Result<serde_json::Value> {
    Ok(
        match transport_direct_component_lifecycle(lifecycle, descriptor)? {
            rsjs_transport::TransportDirectComponentLifecycle::StaticSingle => {
                serde_json::json!({ "kind": "static_single" })
            }
            rsjs_transport::TransportDirectComponentLifecycle::Repeated => {
                serde_json::json!({ "kind": "repeated" })
            }
            rsjs_transport::TransportDirectComponentLifecycle::UnknownNestedConditionals => {
                serde_json::json!({
                    "kind": "unknown",
                    "reason": "nested_conditionals",
                })
            }
            rsjs_transport::TransportDirectComponentLifecycle::ConditionalSingle {
                control,
                predicate_expression,
                predicate_graph: _,
                branch,
            } => serde_json::json!({
                "branch": match branch {
                    rsjs_transport::TransportDirectComponentBranch::Then => "then",
                    rsjs_transport::TransportDirectComponentBranch::Else => "else",
                },
                "control": control,
                "kind": "conditional_single",
                "predicate_expression": predicate_expression,
            }),
        },
    )
}

fn transport_direct_component_lifecycle(
    lifecycle: analyze::DirectComponentLifecycle,
    descriptor: &serde_json::Value,
) -> syn::Result<rsjs_transport::TransportDirectComponentLifecycle> {
    match lifecycle {
        analyze::DirectComponentLifecycle::StaticSingle => {
            Ok(rsjs_transport::TransportDirectComponentLifecycle::StaticSingle)
        }
        analyze::DirectComponentLifecycle::Repeated => {
            Ok(rsjs_transport::TransportDirectComponentLifecycle::Repeated)
        }
        analyze::DirectComponentLifecycle::UnknownNestedConditionals => {
            Ok(rsjs_transport::TransportDirectComponentLifecycle::UnknownNestedConditionals)
        }
        analyze::DirectComponentLifecycle::ConditionalSingle(scope) => {
            let conditionals = descriptor
                .get("bindings")
                .and_then(|bindings| bindings.get("conditionals"))
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "RSJS internal component descriptor has no canonical conditional table",
                    )
                })?;
            let mut parent_ordinal = 0_u32;
            let mut canonical_control = None;
            for (control, conditional) in conditionals.iter().enumerate() {
                let parent = conditional
                    .get("parent")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "RSJS internal component descriptor contains an invalid conditional parent",
                        )
                    })?;
                if parent != u64::from(scope.parent.0) {
                    continue;
                }
                if parent_ordinal == scope.section_ordinal {
                    canonical_control = Some(u32::try_from(control).map_err(|_| {
                        syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "RSJS conditional control count exceeds the canonical u32 ID space",
                        )
                    })?);
                    break;
                }
                parent_ordinal = parent_ordinal.checked_add(1).ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "RSJS conditional section ordinal exceeds the canonical u32 ID space",
                    )
                })?;
            }
            let control = canonical_control.ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal component lifecycle refers to missing conditional parent {} section {}",
                        scope.parent.0, scope.section_ordinal
                    ),
                )
            })?;
            let predicate_expression = conditionals
                .get(control as usize)
                .and_then(|conditional| conditional.get("condition"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|predicate| u32::try_from(predicate).ok())
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "RSJS conditional control {control} has no canonical u32 predicate expression"
                        ),
                    )
                })?;
            // `descriptor` excludes transport claims and the outer semantic
            // hash, so this commitment is non-cyclic. Serialization and
            // hashing both live in rsjs_transport, ensuring macro and adapter
            // consume byte-identical canonical JSON.
            let descriptor = descriptor.as_object().ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "RSJS internal component descriptor must be a JSON object",
                )
            })?;
            let predicate_graph = rsjs_transport::component_lifecycle_predicate_graph_commitment(
                descriptor,
                control,
                predicate_expression,
            );
            Ok(
                rsjs_transport::TransportDirectComponentLifecycle::ConditionalSingle {
                    control,
                    predicate_expression,
                    predicate_graph,
                    branch: match scope.branch {
                        analyze::DirectConditionalBranch::Then => {
                            rsjs_transport::TransportDirectComponentBranch::Then
                        }
                        analyze::DirectConditionalBranch::Else => {
                            rsjs_transport::TransportDirectComponentBranch::Else
                        }
                    },
                },
            )
        }
    }
}

fn transport_direct_component_binding(
    binding: analyze::DirectComponentBindingClassification,
) -> rsjs_transport::TransportDirectComponentBinding {
    match binding {
        analyze::DirectComponentBindingClassification::ServerOnly => {
            rsjs_transport::TransportDirectComponentBinding::ServerOnly
        }
        analyze::DirectComponentBindingClassification::LiveValue { signal, source } => {
            rsjs_transport::TransportDirectComponentBinding::LiveValue {
                signal: signal.0,
                source: match source {
                    analyze::DirectLiveValueSource::SignalRead => {
                        rsjs_transport::TransportDirectLiveValueSource::DirectSignalRead
                    }
                    analyze::DirectLiveValueSource::ClientAlias => {
                        rsjs_transport::TransportDirectLiveValueSource::ClientAlias
                    }
                    analyze::DirectLiveValueSource::InboundField => {
                        rsjs_transport::TransportDirectLiveValueSource::InboundField
                    }
                },
            }
        }
        analyze::DirectComponentBindingClassification::StaticCallback { form } => {
            rsjs_transport::TransportDirectComponentBinding::StaticCallback {
                form: match form {
                    analyze::DirectStaticCallbackForm::None => {
                        rsjs_transport::TransportDirectStaticCallbackForm::None
                    }
                    analyze::DirectStaticCallbackForm::NoopHandler => {
                        rsjs_transport::TransportDirectStaticCallbackForm::NoopHandler
                    }
                    analyze::DirectStaticCallbackForm::SomeNoopHandler => {
                        rsjs_transport::TransportDirectStaticCallbackForm::SomeNoopHandler
                    }
                },
            }
        }
        analyze::DirectComponentBindingClassification::LiveCallback {
            handler_prelude_local,
            form,
        } => rsjs_transport::TransportDirectComponentBinding::LiveCallback {
            handler_prelude_local,
            form: match form {
                analyze::DirectLiveCallbackForm::Handler => {
                    rsjs_transport::TransportDirectLiveCallbackForm::Handler
                }
                analyze::DirectLiveCallbackForm::SomeHandler => {
                    rsjs_transport::TransportDirectLiveCallbackForm::SomeHandler
                }
            },
        },
        analyze::DirectComponentBindingClassification::NodeRefHandle { node_ref } => {
            rsjs_transport::TransportDirectComponentBinding::NodeRefHandle {
                node_ref: node_ref.0,
            }
        }
        analyze::DirectComponentBindingClassification::DeferredProjection {
            field,
            signal,
            node_ref,
        } => rsjs_transport::TransportDirectComponentBinding::DeferredProjection {
            field,
            signal: signal.0,
            node_ref: node_ref.0,
        },
        analyze::DirectComponentBindingClassification::StaticSlotExactOne => {
            rsjs_transport::TransportDirectComponentBinding::StaticSlotExactOne
        }
        analyze::DirectComponentBindingClassification::Unsupported => {
            rsjs_transport::TransportDirectComponentBinding::Unsupported
        }
    }
}

fn direct_payload_handler_value_json(
    value: &analyze::DirectPayloadHandlerValue,
) -> syn::Result<serde_json::Value> {
    let body_ir =
        serde_json::from_str::<serde_json::Value>(&rsjs_ir::client_block_json(&value.body_ir))
            .map_err(|error| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("failed to serialize a direct payload-handler Client IR body: {error}"),
                )
            })?;
    let client_module_calls = value
        .dependencies
        .client_module_calls
        .iter()
        .map(|call| {
            serde_json::json!({
                "module": call.module,
                "name": call.name,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "body_ir": body_ir,
        "body_ir_version": CLIENT_IR_SCHEMA_VERSION,
        "dependencies": {
            "callback_slots": value
                .dependencies
                .callback_slots
                .iter()
                .map(|slot| slot.0)
                .collect::<Vec<_>>(),
            "client_module_calls": client_module_calls,
            "helper_edges": value
                .dependencies
                .helper_edges
                .iter()
                .map(|edge| edge.index())
                .collect::<Vec<_>>(),
            "mutations": value
                .dependencies
                .mutations
                .iter()
                .map(|mutation| mutation.0)
                .collect::<Vec<_>>(),
            "node_refs": value
                .dependencies
                .node_refs
                .iter()
                .map(|node_ref| node_ref.0)
                .collect::<Vec<_>>(),
            "queries": value
                .dependencies
                .queries
                .iter()
                .map(|query| query.0)
                .collect::<Vec<_>>(),
            "signal_reads": value
                .dependencies
                .signal_reads
                .iter()
                .map(|signal| signal.0)
                .collect::<Vec<_>>(),
            "signal_writes": value
                .dependencies
                .signal_writes
                .iter()
                .map(|signal| signal.0)
                .collect::<Vec<_>>(),
        },
        "kind": "payload_handler",
        "payload_arity": value.payload_arity,
        "prelude_local_index": value.prelude_local_index,
        "source_anchor": {
            "column": value.source_anchor.start_column,
            "line": value.source_anchor.start_line,
        },
    }))
}

fn hydration_constraint_json(
    strategy: &rsjs_ir::HydrationStrategy,
    explicit: bool,
) -> serde_json::Value {
    if !explicit {
        return serde_json::json!({ "kind": "unconstrained" });
    }
    match strategy {
        rsjs_ir::HydrationStrategy::Immediate => serde_json::json!({
            "kind": "eager",
        }),
        rsjs_ir::HydrationStrategy::Visible => serde_json::json!({
            "kind": "visible",
        }),
        rsjs_ir::HydrationStrategy::Idle => serde_json::json!({
            "kind": "idle",
        }),
        rsjs_ir::HydrationStrategy::Interaction { event } => serde_json::json!({
            "event": event,
            "kind": "interaction",
        }),
        rsjs_ir::HydrationStrategy::Media { query } => serde_json::json!({
            "kind": "media",
            "query": query,
        }),
    }
}

fn direct_component_binding_json(
    binding: analyze::DirectComponentBindingClassification,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "kind": binding.as_str(),
        "proof": binding.proof(),
    });
    let object = value
        .as_object_mut()
        .expect("direct component binding metadata is an object");
    match binding {
        analyze::DirectComponentBindingClassification::LiveValue { signal, source } => {
            object.insert(
                "parent_source".to_owned(),
                serde_json::json!({
                    // This is the stable legacy analyzer namespace, not a
                    // canonical `LocalValueId`. The semantic adapter maps it
                    // only after importing the parent client graph.
                    "id": signal.0,
                    "namespace": "legacy_signal",
                }),
            );
            object.insert(
                "source_form".to_owned(),
                serde_json::Value::String(source.as_str().to_owned()),
            );
        }
        analyze::DirectComponentBindingClassification::LiveCallback {
            handler_prelude_local,
            form,
        } => {
            object.insert(
                "callback_shape".to_owned(),
                serde_json::Value::String(form.callback_shape().to_owned()),
            );
            object.insert(
                "parent_source".to_owned(),
                serde_json::json!({
                    // Payload handlers do not have a canonical graph-value ID
                    // at macro expansion time. This exact lexical binding is
                    // the deterministic input from which the semantic adapter
                    // must form and validate that value.
                    "id": handler_prelude_local,
                    "namespace": "payload_handler_prelude_binding",
                }),
            );
        }
        analyze::DirectComponentBindingClassification::StaticCallback { form } => {
            object.insert(
                "callback_shape".to_owned(),
                serde_json::Value::String(form.callback_shape().to_owned()),
            );
            object.insert(
                "source_form".to_owned(),
                serde_json::Value::String(form.as_str().to_owned()),
            );
        }
        analyze::DirectComponentBindingClassification::NodeRefHandle { node_ref } => {
            object.insert(
                "handle_kind".to_owned(),
                serde_json::Value::String("node_ref".to_owned()),
            );
            object.insert(
                "parent_source".to_owned(),
                serde_json::json!({
                    "id": node_ref.0,
                    "namespace": "legacy_node_ref",
                }),
            );
        }
        analyze::DirectComponentBindingClassification::DeferredProjection {
            field,
            signal,
            node_ref,
        } => {
            object.insert("parent_field_id".to_owned(), serde_json::json!(field));
            object.insert("latent_signal_id".to_owned(), serde_json::json!(signal.0));
            object.insert(
                "latent_node_ref_id".to_owned(),
                serde_json::json!(node_ref.0),
            );
        }
        analyze::DirectComponentBindingClassification::ServerOnly
        | analyze::DirectComponentBindingClassification::StaticSlotExactOne
        | analyze::DirectComponentBindingClassification::Unsupported => {}
    }
    value
}

fn transport_id_hex(id: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(64);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Emit the defining side of the hidden Rust-resolved component ABI.
///
/// Every accepted `#[rsjs(component...)]` impl gets exactly one retained
/// witness static and one `ComponentAbi` impl. The associated reference is the
/// linker's build-local relocation/address identity. The companion inline
/// identity record supplies the normalized package/crate/module/item namespace;
/// `provisional_source_key` remains diagnostic-only token material.
fn emit_component_abi_definition(
    parsed: &parse::ParsedIsland,
    asset_id: &str,
    input_fingerprint: &str,
    cargo_unit_id: [u8; 32],
) -> TokenStream2 {
    use proc_macro2::Span;
    use quote::{format_ident, quote};

    let witness_ident = format_ident!("__RSJS_COMPONENT_ABI_WITNESS_{}", asset_id);
    let definition_ident = format_ident!("__RSJS_COMPONENT_DEFINITION_{}", asset_id);
    let identity_text_ident = format_ident!("__RSJS_COMPONENT_IDENTITY_TEXT_{}", asset_id);
    let identity_ident = format_ident!("__RSJS_COMPONENT_IDENTITY_{}", asset_id);
    let component_record_id = component_record_id(input_fingerprint)
        .iter()
        .map(|byte| proc_macro2::Literal::u8_unsuffixed(*byte))
        .collect::<Vec<_>>();
    let cargo_unit_id = cargo_unit_id
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    let asset_id = syn::LitStr::new(asset_id, Span::call_site());
    let descriptor_schema_version = component_descriptor::DESCRIPTOR_SCHEMA_VERSION;
    let self_ty = &parsed.self_ty;
    let item_ident = &parsed
        .struct_path
        .segments
        .last()
        .expect("component paths always contain an item")
        .ident;
    let abi_generics = component_abi_impl_generics(&parsed.impl_generics, self_ty);
    let (impl_generics, _, where_clause) = abi_generics.split_for_impl();
    let render_kind = match parsed.render_kind {
        parse::RenderKind::Sync => {
            quote!(::rsjs::__private::ComponentRenderKind::Sync)
        }
        parse::RenderKind::Async => {
            quote!(::rsjs::__private::ComponentRenderKind::Async)
        }
    };
    let origin_permission = if parsed.client_origin_allowed {
        quote!(::rsjs::__private::ComponentOriginPermission::OriginCapable)
    } else {
        quote!(::rsjs::__private::ComponentOriginPermission::ParticipantOnly)
    };

    quote! {
        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        pub static #witness_ident: ::rsjs::__private::ComponentAbiWitness =
            ::rsjs::__private::ComponentAbiWitness {
                format_magic: ::rsjs::__private::COMPONENT_ABI_FORMAT_MAGIC,
                abi_version: ::rsjs::__private::COMPONENT_ABI_VERSION,
                descriptor_schema_version: #descriptor_schema_version,
                source_key_version: ::rsjs::__private::COMPONENT_SOURCE_KEY_VERSION,
                render_kind: #render_kind,
                origin_permission: #origin_permission,
                asset_id: #asset_id,
                provisional_source_key: concat!(
                    env!("CARGO_PKG_NAME"),
                    "::",
                    module_path!(),
                    "::",
                    stringify!(#self_ty),
                ),
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
            unsafe(link_section = "__DATA,__rsjs_comp")
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
            unsafe(link_section = ".rsjs.component")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$C"))]
        static #definition_ident: ::rsjs::__private::ComponentDefinitionRecord<
            ::rsjs::__private::ComponentAbiWitness,
        > = ::rsjs::__private::ComponentDefinitionRecord::new(
            [#(#cargo_unit_id),*],
            [#(#component_record_id),*],
            #witness_ident.transport_definition_abi(),
            &#witness_ident,
        );

        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const #identity_text_ident: &str = concat!(
            env!("CARGO_PKG_NAME"),
            "\0",
            env!("CARGO_CRATE_NAME"),
            "\0",
            module_path!(),
            "\0",
            stringify!(#item_ident),
        );

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__rsjs_ident")
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
            unsafe(link_section = ".rsjs.identity")
        )]
        #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$S"))]
        static #identity_ident: ::rsjs::__private::SemanticIdentityRecord<
            { #identity_text_ident.len() },
        > = ::rsjs::__private::SemanticIdentityRecord::new(
            ::rsjs::__private::SemanticIdentityKind::Component,
            [#(#cargo_unit_id),*],
            [#(#component_record_id),*],
            ::rsjs::__private::encode_semantic_identity::<
                { #identity_text_ident.len() },
            >(#identity_text_ident),
        );

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
            "RSJS descriptor relocation transport does not support this target object format"
        );

        #[automatically_derived]
        impl #impl_generics ::rsjs::__private::ComponentAbi for #self_ty #where_clause {
            const WITNESS: &'static ::rsjs::__private::ComponentAbiWitness =
                &#witness_ident;
        }
    }
}

#[derive(Default)]
struct ComponentAbiGenericUses {
    type_or_const: std::collections::HashSet<String>,
    lifetimes: std::collections::HashSet<String>,
}

impl<'ast> syn::visit::Visit<'ast> for ComponentAbiGenericUses {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path
                .path
                .segments
                .first()
                .is_some_and(|segment| matches!(segment.arguments, syn::PathArguments::None))
        {
            self.type_or_const.insert(
                path.path
                    .segments
                    .first()
                    .expect("single path segment checked")
                    .ident
                    .to_string(),
            );
        }
        syn::visit::visit_type_path(self, path);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path
                .path
                .segments
                .first()
                .is_some_and(|segment| matches!(segment.arguments, syn::PathArguments::None))
        {
            self.type_or_const.insert(
                path.path
                    .segments
                    .first()
                    .expect("single path segment checked")
                    .ident
                    .to_string(),
            );
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        self.lifetimes.insert(lifetime.ident.to_string());
    }
}

fn component_abi_impl_generics(
    impl_generics: &syn::Generics,
    self_ty: &syn::Type,
) -> syn::Generics {
    use syn::visit::Visit;

    let mut self_uses = ComponentAbiGenericUses::default();
    self_uses.visit_type(self_ty);

    let retained = |param: &syn::GenericParam| match param {
        syn::GenericParam::Type(param) => {
            self_uses.type_or_const.contains(&param.ident.to_string())
        }
        syn::GenericParam::Const(param) => {
            self_uses.type_or_const.contains(&param.ident.to_string())
        }
        syn::GenericParam::Lifetime(param) => self_uses
            .lifetimes
            .contains(&param.lifetime.ident.to_string()),
    };

    let excluded_type_or_const: std::collections::HashSet<String> = impl_generics
        .params
        .iter()
        .filter(|param| !retained(param))
        .filter_map(|param| match param {
            syn::GenericParam::Type(param) => Some(param.ident.to_string()),
            syn::GenericParam::Const(param) => Some(param.ident.to_string()),
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    let excluded_lifetimes: std::collections::HashSet<String> = impl_generics
        .params
        .iter()
        .filter(|param| !retained(param))
        .filter_map(|param| match param {
            syn::GenericParam::Lifetime(param) => Some(param.lifetime.ident.to_string()),
            syn::GenericParam::Type(_) | syn::GenericParam::Const(_) => None,
        })
        .collect();

    let references_excluded = |uses: &ComponentAbiGenericUses| {
        uses.type_or_const
            .iter()
            .any(|name| excluded_type_or_const.contains(name))
            || uses
                .lifetimes
                .iter()
                .any(|name| excluded_lifetimes.contains(name))
    };

    let mut abi_generics = impl_generics.clone();
    abi_generics.params = impl_generics
        .params
        .iter()
        .filter(|param| retained(param))
        .cloned()
        .map(|mut param| {
            match &mut param {
                syn::GenericParam::Type(param) => {
                    param.eq_token = None;
                    param.default = None;
                    param.bounds = param
                        .bounds
                        .iter()
                        .filter(|bound| {
                            let mut uses = ComponentAbiGenericUses::default();
                            uses.visit_type_param_bound(bound);
                            !references_excluded(&uses)
                        })
                        .cloned()
                        .collect();
                }
                syn::GenericParam::Lifetime(param) => {
                    param.bounds = param
                        .bounds
                        .iter()
                        .filter(|bound| !excluded_lifetimes.contains(&bound.ident.to_string()))
                        .cloned()
                        .collect();
                }
                syn::GenericParam::Const(param) => {
                    param.eq_token = None;
                    param.default = None;
                }
            }
            param
        })
        .collect();

    if abi_generics.params.is_empty() {
        abi_generics.lt_token = None;
        abi_generics.gt_token = None;
    }

    if let Some(where_clause) = &impl_generics.where_clause {
        let mut filtered = where_clause.clone();
        filtered.predicates = where_clause
            .predicates
            .iter()
            .filter(|predicate| {
                let mut uses = ComponentAbiGenericUses::default();
                uses.visit_where_predicate(predicate);
                !references_excluded(&uses)
            })
            .cloned()
            .collect();
        abi_generics.where_clause = (!filtered.predicates.is_empty()).then_some(filtered);
    }

    abi_generics
}

/// Publish this island's shared-state protocol identities, resolved by rustc in
/// the crate that wrote the acquisitions.
///
/// A parent that fully inlines this component compares each entry against the
/// type key the same Rust name resolves to in *its* module, which is the only
/// way to catch a name that silently rebinds to a different provider scope.
/// See [`rsjs::IslandSharedStores`].
///
/// Emitted only for a plain, non-generic `Self` type: an additional trait impl
/// on anything else would need generic parameters this function cannot recover,
/// and a component with a generic `Self` is not inlinable by name anyway.
fn emit_island_shared_stores_impl(
    self_ty: &syn::Type,
    initializers: &[emit_rust::SharedStateInitializer],
) -> TokenStream2 {
    if initializers.is_empty() {
        return TokenStream2::new();
    }
    let syn::Type::Path(path) = self_ty else {
        return TokenStream2::new();
    };
    if path.qself.is_some()
        || !path
            .path
            .segments
            .iter()
            .all(|segment| matches!(segment.arguments, syn::PathArguments::None))
    {
        return TokenStream2::new();
    }
    // Store ids are dense and allocated in acquisition order, so a slot per id
    // keeps the array indexable by `store.id` from the inlining parent.
    let mut by_id: Vec<Option<&syn::Type>> = Vec::new();
    for init in initializers {
        let index = init.id.0 as usize;
        if by_id.len() <= index {
            by_id.resize(index + 1, None);
        }
        by_id[index] = Some(&init.ty);
    }
    let entries: Vec<TokenStream2> = by_id
        .iter()
        .map(|slot| match slot {
            Some(ty) => quote! { <#ty as ::rsjs::SharedState>::TYPE_KEY },
            None => quote! { "" },
        })
        .collect();
    quote! {
        #[automatically_derived]
        impl ::rsjs::IslandSharedStores for #self_ty {
            const SHARED_TYPE_KEYS: &'static [&'static str] = &[#(#entries),*];
        }
    }
}

fn collect_rsjs_ide_helper_hints(item: &TokenStream2) -> TokenStream2 {
    let mut hints = Vec::new();
    collect_rsjs_ide_helper_hints_from_stream(item.clone(), &mut hints);

    if hints.is_empty() {
        TokenStream2::new()
    } else {
        quote! {
            #(#hints)*
        }
    }
}

// `syn::Visit` does not descend into `html_sync! { ... }` macro bodies, which
// is exactly where helpers like `rsjs::server_attr(...)` are stripped during
// lowering. Scan the original token tree instead so the generated no-op
// references keep the helper path's original span for IDE go-to-def.
fn collect_rsjs_ide_helper_hints_from_stream(stream: TokenStream2, hints: &mut Vec<TokenStream2>) {
    let tokens: Vec<proc_macro2::TokenTree> = stream.into_iter().collect();

    for (i, token) in tokens.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            collect_rsjs_ide_helper_hints_from_stream(group.stream(), hints);
        }

        let proc_macro2::TokenTree::Ident(ident) = token else {
            continue;
        };
        if is_html_macro_name(&ident.to_string()) && macro_ident_is_invoked(&tokens, i) {
            if let Some(path) = path_ending_at_ident(&tokens, i) {
                hints.push(html_macro_ide_hint_for_path(&path));
            }
            continue;
        }
        if !is_rsjs_helper_name(&ident.to_string()) {
            continue;
        }
        if i > 0 && is_punct(&tokens[i - 1], '.') {
            continue;
        }
        if !helper_ident_is_called(&tokens, i) {
            continue;
        }
        let Some(path) = path_ending_at_ident(&tokens, i) else {
            continue;
        };
        if let Some(hint) = rsjs_ide_hint_for_path(&path) {
            hints.push(hint);
        }
    }
}

fn is_html_macro_name(name: &str) -> bool {
    name == "html" || name == "html_sync"
}

fn macro_ident_is_invoked(tokens: &[proc_macro2::TokenTree], ident_index: usize) -> bool {
    is_punct_at(tokens, ident_index + 1, '!')
        && matches!(
            tokens.get(ident_index + 2),
            Some(proc_macro2::TokenTree::Group(group))
                if group.delimiter() == proc_macro2::Delimiter::Brace
        )
}

fn html_macro_ide_hint_for_path(path: &syn::Path) -> TokenStream2 {
    if path.segments.len() == 1 {
        let ident = &path.segments[0].ident;
        quote! { let _ = ::proa_macros::#ident! { "" }; }
    } else {
        quote! { let _ = #path! { "" }; }
    }
}

fn is_rsjs_helper_name(name: &str) -> bool {
    matches!(
        name,
        "server_attr"
            | "server_only"
            | "island"
            | "signal"
            | "handler"
            | "event_handler"
            | "node_ref"
            | "noop_handler"
    )
}

fn helper_ident_is_called(tokens: &[proc_macro2::TokenTree], helper_index: usize) -> bool {
    let mut next = helper_index + 1;
    if matches!(
        tokens.get(next),
        Some(proc_macro2::TokenTree::Group(group))
            if group.delimiter() == proc_macro2::Delimiter::Parenthesis
    ) {
        return true;
    }

    if !(is_punct_at(tokens, next, ':')
        && is_punct_at(tokens, next + 1, ':')
        && is_punct_at(tokens, next + 2, '<'))
    {
        return false;
    }

    next += 2;
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(next) {
        if is_punct(token, '<') {
            depth += 1;
        } else if is_punct(token, '>') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return matches!(
                    tokens.get(next + 1),
                    Some(proc_macro2::TokenTree::Group(group))
                        if group.delimiter() == proc_macro2::Delimiter::Parenthesis
                );
            }
        }
        next += 1;
    }
    false
}

fn path_ending_at_ident(
    tokens: &[proc_macro2::TokenTree],
    ident_index: usize,
) -> Option<syn::Path> {
    let mut start = ident_index;
    while start >= 3
        && is_ident(&tokens[start - 3])
        && is_punct(&tokens[start - 2], ':')
        && is_punct(&tokens[start - 1], ':')
    {
        start -= 3;
    }
    if start >= 2 && is_punct(&tokens[start - 2], ':') && is_punct(&tokens[start - 1], ':') {
        start -= 2;
    }

    let path_tokens: TokenStream2 = tokens[start..=ident_index].iter().cloned().collect();
    syn::parse2::<syn::Path>(path_tokens).ok()
}

fn is_ident(token: &proc_macro2::TokenTree) -> bool {
    matches!(token, proc_macro2::TokenTree::Ident(_))
}

fn is_punct_at(tokens: &[proc_macro2::TokenTree], index: usize, ch: char) -> bool {
    tokens.get(index).is_some_and(|token| is_punct(token, ch))
}

fn is_punct(token: &proc_macro2::TokenTree, ch: char) -> bool {
    matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ch)
}

fn rsjs_ide_hint_for_path(path: &syn::Path) -> Option<TokenStream2> {
    let helper = path.segments.last()?.ident.to_string();
    let claimed_rsjs_path = match path.segments.len() {
        1 => true,
        2 => path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "rsjs"),
        _ => false,
    };
    if !claimed_rsjs_path {
        return None;
    }
    let path = path_without_last_segment_args(path);

    match helper.as_str() {
        "server_attr" | "server_only" | "island" | "signal" | "node_ref" | "noop_handler" => {
            Some(quote! { let _ = #path::<()>; })
        }
        "handler" => Some(quote! { let _ = #path::<fn(())>; }),
        "event_handler" => Some(quote! {
            let _ = #path::<fn(::rsjs::Event), (::rsjs::Event,)>;
        }),
        _ => None,
    }
}

fn path_without_last_segment_args(path: &syn::Path) -> syn::Path {
    let mut path = path.clone();
    if let Some(last) = path.segments.last_mut() {
        last.arguments = syn::PathArguments::None;
    }
    path
}

fn collect_rsjs_source_ide_hints(
    prelude_statements: &[syn::Stmt],
    root: &proa_dsl::parse::HtmlRoot,
    compiler_prelude_local_indices: &std::collections::HashSet<usize>,
    prelude_signal_ids: &std::collections::HashMap<usize, rsjs_ir::SignalId>,
    required_signal_ids: &[rsjs_ir::SignalId],
    prelude_query_ids: &std::collections::HashMap<usize, rsjs_ir::QueryId>,
    prelude_mutation_ids: &std::collections::HashMap<usize, rsjs_ir::MutationId>,
    reachable_mutation_ids: &[rsjs_ir::MutationId],
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
) -> TokenStream2 {
    let mut prelude_hints = Vec::new();
    let mut event_handler_names = std::collections::BTreeSet::new();
    let mut local_index = 0_usize;
    for statement in prelude_statements {
        match statement {
            syn::Stmt::Local(local) => {
                if let Some(name) = event_handler_local_name(local) {
                    event_handler_names.insert(name);
                }
                let index = local_index;
                local_index += 1;
                if let Some(id) = prelude_signal_ids.get(&index).copied() {
                    if required_signal_ids.contains(&id) {
                        prelude_hints.push(lossless_signal_local_source_ide_hint(local, id));
                    }
                    continue;
                }
                if prelude_query_ids.contains_key(&index) {
                    // The type-sensitive witness is emitted at the declaration
                    // site while the typed initial value is alive. Keep a
                    // non-evaluating handle binding in the tail IDE closure so
                    // source expressions such as `query.data()` still resolve.
                    if let Some(hint) = lossless_query_tail_source_ide_hint(local) {
                        prelude_hints.push(hint);
                    }
                    continue;
                }
                if let Some(id) = prelude_mutation_ids.get(&index).copied() {
                    if reachable_mutation_ids.contains(&id) {
                        if let Some(hint) = lossless_client_declaration_source_ide_hint(local) {
                            prelude_hints.push(hint);
                        }
                    }
                    continue;
                }
                if let Some(hint) = prelude_local_source_ide_hint(local) {
                    prelude_hints.push(hint);
                } else if compiler_prelude_local_indices.contains(&index) {
                    prelude_hints.push(quote! { #local });
                }
            }
            syn::Stmt::Expr(syn::Expr::Call(call), _)
                if crate::parse::is_interval_call_expr(call) =>
            {
                // Keep the timer between the exact bindings visible at its
                // declaration site. Appending all intervals after all lets
                // made IDE checking bind a timer closure to a later shadow.
                prelude_hints.push(quote! { #call; });
            }
            _ => {}
        }
    }
    let mut hints = Vec::new();
    collect_rsjs_source_ide_hints_from_nodes(
        &root.nodes,
        &mut hints,
        inbound_live_field_signals,
        &event_handler_names,
    );
    if prelude_hints.is_empty() && hints.is_empty() {
        TokenStream2::new()
    } else {
        quote! {
            #[allow(unreachable_code, unused_assignments, unused_mut, unused_variables)]
            let _ = || {
                fn __rsjs_ide_infer<T>(_: &T) -> T {
                    loop {}
                }
                fn __rsjs_ide_payload<T>() -> T {
                    loop {}
                }
                #(#prelude_hints)*
                #(#hints)*
            };
        }
    }
}

/// Resolve every compiler-owned inbound live field to the exact signal local
/// that replaced it in the analyzed server template.
///
/// Signal-seed sites are deliberately excluded: `signal(self.field)` is an
/// authored move, so later authored uses must retain normal Rust move
/// checking. `LiveSignal`, by contrast, is the compiler's representation of a
/// live prop read and every such read must reuse one signal.
fn inbound_live_field_ide_signal_substitutions(
    inputs: &analyze::InboundInputMetadata,
) -> syn::Result<std::collections::BTreeMap<String, rsjs_ir::SignalId>> {
    let mut substitutions = std::collections::BTreeMap::new();
    for site in &inputs.prop_sites {
        let analyze::InboundPropTarget::LiveSignal(signal) = site.target else {
            continue;
        };
        let field = inputs
            .fields
            .get(site.field as usize)
            .filter(|field| field.id == site.field)
            .ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal inbound prop site {} references invalid field {}",
                        site.id, site.field
                    ),
                )
            })?;
        match substitutions.entry(field.name.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(signal);
            }
            std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == signal => {}
            std::collections::btree_map::Entry::Occupied(entry) => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "RSJS internal inbound field `{}` maps to conflicting live signals {} and {}",
                        field.name,
                        entry.get().0,
                        signal.0
                    ),
                ));
            }
        }
    }
    Ok(substitutions)
}

struct InboundLiveFieldIdeSubstitution<'a> {
    signals: &'a std::collections::BTreeMap<String, rsjs_ir::SignalId>,
}

impl syn::fold::Fold for InboundLiveFieldIdeSubstitution<'_> {
    fn fold_expr(&mut self, expr: syn::Expr) -> syn::Expr {
        if let syn::Expr::Field(field) = &expr {
            if expr_is_self(field.base.as_ref()) {
                if let syn::Member::Named(member) = &field.member {
                    if let Some(signal) = self.signals.get(&member.to_string()) {
                        let local = emit_rust::signal_local_ident(*signal);
                        let span = syn::spanned::Spanned::span(field);
                        return syn::parse_quote_spanned!(span=> #local);
                    }
                }
            }
        }
        syn::fold::fold_expr(self, expr)
    }
}

fn substitute_inbound_live_fields_for_ide(
    expr: &syn::Expr,
    signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
) -> syn::Expr {
    syn::fold::Fold::fold_expr(
        &mut InboundLiveFieldIdeSubstitution { signals },
        expr.clone(),
    )
}

fn collect_lossless_query_source_ide_hints(
    prelude_lets: &[syn::Local],
    prelude_query_ids: &std::collections::HashMap<usize, rsjs_ir::QueryId>,
    reachable_query_ids: &[rsjs_ir::QueryId],
) -> std::collections::HashMap<rsjs_ir::QueryId, TokenStream2> {
    prelude_lets
        .iter()
        .enumerate()
        .filter_map(|(index, local)| {
            let id = prelude_query_ids.get(&index).copied()?;
            reachable_query_ids
                .contains(&id)
                .then(|| lossless_query_declaration_source_ide_hint(local, id))
                .flatten()
                .map(|hint| (id, hint))
        })
        .collect()
}

/// Rebind a compiler-owned signal name for IDE/source checking without
/// replaying its authored initializer.
///
/// The real initializer has already run at the declaration's exact lexical
/// position and may have moved a field out of `self`. Referring to that source
/// expression again in the late IDE-only closure would make otherwise valid
/// Rust fail borrow checking. The generated initial local carries rustc's
/// resolved value type, so borrowing it preserves inference without a second
/// read, move, allocation, or side effect.
fn lossless_signal_local_source_ide_hint(
    local: &syn::Local,
    id: rsjs_ir::SignalId,
) -> TokenStream2 {
    let pat = &local.pat;
    let initial = quote::format_ident!("__rsjs_initial_signal_{}", id.0);
    let signal = local
        .init
        .as_ref()
        .and_then(|init| match init.expr.as_ref() {
            syn::Expr::Call(call) => analyze::signal_call_explicit_type(call),
            _ => None,
        })
        .map(|ty| quote! { ::rsjs::signal::<#ty> })
        .unwrap_or_else(|| quote! { ::rsjs::signal });
    quote! {
        let #pat = #signal(__rsjs_ide_infer(&#initial));
    }
}

/// Keep query name/type inference active without evaluating any authored
/// metadata expression twice. The real typed initial local is available from
/// the site-local server declaration; every browser-only argument is replaced
/// by the diverging IDE payload helper.
fn lossless_query_declaration_source_ide_hint(
    local: &syn::Local,
    id: rsjs_ir::QueryId,
) -> Option<TokenStream2> {
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    if call.args.len() < 2 {
        return None;
    }
    let pat = &local.pat;
    let func = &call.func;
    let typed_initial = emit_rust::query_initial_typed_value_local_ident(id);
    let args = call.args.iter().enumerate().map(|(index, _)| match index {
        1 => quote! { __rsjs_ide_infer(&#typed_initial) },
        3 => quote! { ::rsjs::QueryOptions::default() },
        _ => quote! { () },
    });
    Some(quote! {
        #[allow(unreachable_code, unused_assignments, unused_mut, unused_variables)]
        let _ = || {
            fn __rsjs_ide_infer<T>(_: &T) -> T {
                loop {}
            }
            let #pat = #func(#(#args),*);
        };
    })
}

fn lossless_query_tail_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    let pat = &local.pat;
    let func = &call.func;
    let with_options = analyze::path_ends_with(func, "query_with_initial")
        || analyze::path_ends_with(func, "query_with_options");
    let args = call.args.iter().enumerate().map(|(index, _)| {
        if with_options && index == 3 {
            quote! { ::rsjs::QueryOptions::default() }
        } else if index == 1 {
            quote! { __rsjs_ide_payload() }
        } else {
            quote! { () }
        }
    });
    Some(quote! {
        let #pat = #func(#(#args),*);
    })
}

/// Keep the declaration's return type and helper path visible to rustc/IDE
/// while replacing browser-only arguments with non-evaluating placeholders.
fn lossless_client_declaration_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    let pat = &local.pat;
    let func = &call.func;
    let with_options = analyze::path_ends_with(func, "mutation_with_options");
    let args = call.args.iter().enumerate().map(|(index, _)| {
        if with_options && index == 3 {
            quote! { ::rsjs::MutationOptions::default() }
        } else {
            quote! { () }
        }
    });
    Some(quote! {
        let #pat = #func(#(#args),*);
    })
}

fn prelude_local_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    if analyze::is_signal_local(local) {
        return signal_local_source_ide_hint(local);
    }
    if analyze::is_memo_local(local) {
        // Replay verbatim: the signal hints above rebind the source
        // names as real `::rsjs::Signal` values, so the original
        // `memo(|| ...)` closure type-checks against the rsjs stub
        // and IDE go-to-def / inference keep working.
        return Some(quote! { #local });
    }
    if analyze::is_handler_local(local) {
        return handler_local_source_ide_hint(local);
    }
    if analyze::is_node_ref_local(local) {
        return Some(quote! { #local });
    }
    if analyze::is_query_runtime_initial_local(local) {
        return query_with_initial_source_ide_hint(local);
    }
    if analyze::is_query_local(local) || analyze::is_mutation_local(local) {
        return Some(quote! { #local });
    }
    None
}

fn query_with_initial_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    local_pat_ident(&local.pat)?;
    let pat = &local.pat;
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    if call.args.len() < 2 {
        return Some(quote! { #local });
    }

    let func = &call.func;
    let mut args: Vec<syn::Expr> = call.args.iter().cloned().collect();
    let _ = args.get(1)?;
    args[1] = syn::parse_quote! { __rsjs_ide_payload() };
    Some(quote! {
        let #pat = #func(#(#args),*);
    })
}

fn handler_local_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    let ident = local_pat_ident(&local.pat)?;
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return Some(quote! { let #ident = (); });
    };
    let closure = call.args.first().and_then(|arg| match arg {
        syn::Expr::Closure(closure) => Some(closure),
        _ => None,
    });
    let Some(closure) = closure else {
        return Some(quote! { let #ident = (); });
    };

    if !analyze::path_ends_with(&call.func, "event_handler") {
        return Some(quote! { let #ident = (); });
    }

    let event_ty = closure
        .inputs
        .iter()
        .next()
        .and_then(handler_pat_type)
        .map(|ty| quote! { #ty })
        .unwrap_or_else(|| quote! { ::rsjs::Event });

    let event_ident = closure.inputs.first().and_then(handler_event_pat_ident);
    let handler_bindings = event_handler_param_bindings(closure);
    let mut body_hints = Vec::new();
    collect_event_handler_ide_hints(closure.body.as_ref(), event_ident.as_ref(), &mut body_hints);

    Some(quote! {
        let #ident = ::rsjs::__private::event_handler_type::<#event_ty>();
        {
            #handler_bindings
            #(#body_hints)*
        }
    })
}

fn event_handler_local_name(local: &syn::Local) -> Option<String> {
    let ident = local_pat_ident(&local.pat)?;
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    if !analyze::path_ends_with(&call.func, "event_handler") {
        return None;
    }
    Some(ident.to_string())
}

fn handler_pat_type(pat: &syn::Pat) -> Option<&syn::Type> {
    match pat {
        syn::Pat::Type(pat_ty) => Some(pat_ty.ty.as_ref()),
        _ => None,
    }
}

fn local_pat_ident(pat: &syn::Pat) -> Option<syn::Ident> {
    match pat {
        syn::Pat::Ident(pi) => Some(pi.ident.clone()),
        syn::Pat::Type(pt) => local_pat_ident(pt.pat.as_ref()),
        _ => None,
    }
}

fn signal_local_source_ide_hint(local: &syn::Local) -> Option<TokenStream2> {
    let init = local.init.as_ref()?;
    let syn::Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    let pat = &local.pat;
    let arg = call.args.first()?;
    if expr_contains_try_operator(arg) {
        return None;
    }
    let signal = if let Some(ty) = analyze::signal_call_explicit_type(call) {
        quote! { ::rsjs::signal::<#ty> }
    } else {
        quote! { ::rsjs::signal }
    };
    Some(quote! {
        let #pat = #signal(__rsjs_ide_infer(&(#arg)));
    })
}

fn expr_contains_try_operator(expr: &syn::Expr) -> bool {
    struct TryVisitor {
        found: bool,
    }

    impl<'ast> syn::visit::Visit<'ast> for TryVisitor {
        fn visit_expr_try(&mut self, _node: &'ast syn::ExprTry) {
            self.found = true;
        }
    }

    let mut visitor = TryVisitor { found: false };
    syn::visit::Visit::visit_expr(&mut visitor, expr);
    visitor.found
}

fn collect_rsjs_source_ide_hints_from_nodes(
    nodes: &[proa_dsl::parse::Node],
    hints: &mut Vec<TokenStream2>,
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
    event_handler_names: &std::collections::BTreeSet<String>,
) {
    for node in nodes {
        match node {
            proa_dsl::parse::Node::Element(el) => {
                collect_rsjs_source_ide_hints_from_attrs(
                    el.tag_str().as_str(),
                    &el.attrs,
                    hints,
                    inbound_live_field_signals,
                    event_handler_names,
                );
                collect_rsjs_source_ide_hints_from_nodes(
                    &el.children,
                    hints,
                    inbound_live_field_signals,
                    event_handler_names,
                );
            }
            proa_dsl::parse::Node::Fragment(children) => {
                collect_rsjs_source_ide_hints_from_nodes(
                    children,
                    hints,
                    inbound_live_field_signals,
                    event_handler_names,
                );
            }
            proa_dsl::parse::Node::Block(expr) => {
                collect_rsjs_source_ide_hint_from_block_expr(
                    expr,
                    hints,
                    inbound_live_field_signals,
                    event_handler_names,
                );
            }
            proa_dsl::parse::Node::Text(_) | proa_dsl::parse::Node::Doctype(_) => {}
        }
    }
}

fn collect_rsjs_source_ide_hint_from_block_expr(
    expr: &syn::Expr,
    hints: &mut Vec<TokenStream2>,
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
    event_handler_names: &std::collections::BTreeSet<String>,
) {
    match expr {
        syn::Expr::ForLoop(for_loop) => {
            if let Some(hint) = rsjs_for_loop_source_ide_hint(
                for_loop,
                inbound_live_field_signals,
                event_handler_names,
            ) {
                hints.push(hint);
            }
        }
        expr => {
            collect_nested_html_macro_ide_hints(
                expr,
                hints,
                inbound_live_field_signals,
                event_handler_names,
            );
            let expr = substitute_inbound_live_fields_for_ide(expr, inbound_live_field_signals);
            collect_source_expr_ide_hints(&expr, hints);
        }
    }
}

fn collect_nested_html_macro_ide_hints(
    expr: &syn::Expr,
    hints: &mut Vec<TokenStream2>,
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
    event_handler_names: &std::collections::BTreeSet<String>,
) {
    struct Collector<'a> {
        hints: &'a mut Vec<TokenStream2>,
        inbound_live_field_signals: &'a std::collections::BTreeMap<String, rsjs_ir::SignalId>,
        event_handler_names: &'a std::collections::BTreeSet<String>,
    }

    impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
        fn visit_macro(&mut self, item: &'ast syn::Macro) {
            let is_html = item
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "html" || segment.ident == "html_sync");
            if !is_html {
                return;
            }
            let Ok(root) = syn::parse2::<proa_dsl::parse::HtmlRoot>(item.tokens.clone()) else {
                return;
            };
            collect_rsjs_source_ide_hints_from_nodes(
                &root.nodes,
                self.hints,
                self.inbound_live_field_signals,
                self.event_handler_names,
            );
        }
    }

    let mut collector = Collector {
        hints,
        inbound_live_field_signals,
        event_handler_names,
    };
    syn::visit::Visit::visit_expr(&mut collector, expr);
}

fn collect_rsjs_source_ide_hints_from_attrs(
    tag: &str,
    attrs: &[proa_dsl::parse::Attribute],
    hints: &mut Vec<TokenStream2>,
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
    event_handler_names: &std::collections::BTreeSet<String>,
) {
    for attr in attrs {
        let proa_dsl::parse::Attribute::Dynamic { name, value } = attr else {
            continue;
        };
        let value = substitute_inbound_live_fields_for_ide(value, inbound_live_field_signals);
        if name.as_markup_str() == "ref" {
            let target = node_ref_target_source_ide_type(tag);
            let span = value.span();
            hints.push(quote::quote_spanned! {span=>
                ::rsjs::__private::assert_component_node_ref_target::<_, #target>(&(#value));
            });
            continue;
        }
        if name.as_markup_str().starts_with("on_") || name.as_markup_str().starts_with("on-") {
            // HTML source names are normalized to markup spelling (`on-input`)
            // while the RSJS event vocabulary deliberately uses Rust-friendly
            // identifiers (`on_input`). Match the same legacy spelling used by
            // the analyzer, but keep markup spelling for ordinary attributes.
            let event_name = name.legacy_ident().to_string();
            let Some(event) = rsjs_ir::EventName::from_attr_name(&event_name) else {
                collect_source_expr_ide_hints(&value, hints);
                continue;
            };
            // `on_*={rsjs::debounce(|event| ..., delay_ms)}`: hint the wrapped
            // closure like a plain handler closure. Referencing the bare
            // generic `debounce` fn item would fail `F` inference.
            let value = debounce_wrapped_handler_expr(&value).unwrap_or(&value);
            if let Some(hint) = event_handler_source_ide_hint(value, event, tag) {
                hints.push(hint);
            } else {
                collect_named_event_handler_call_ide_hints(
                    value,
                    event,
                    tag,
                    event_handler_names,
                    hints,
                );
            }
            continue;
        }
        if let Some(inner) = helper_inner_expr(&value, "server_attr") {
            hints.push(quote! { let _ = &(#inner); });
        } else {
            collect_source_expr_ide_hints(&value, hints);
        }
    }
}

/// Match `rsjs::debounce(<closure>, delay_ms)` (bare or `rsjs::`-qualified)
/// and return the wrapped closure expression.
fn debounce_wrapped_handler_expr(expr: &syn::Expr) -> Option<&syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !analyze::path_ends_with(&call.func, "debounce") {
        return None;
    }
    call.args.first()
}

fn collect_named_event_handler_call_ide_hints(
    expr: &syn::Expr,
    event: rsjs_ir::EventName,
    tag: &str,
    event_handler_names: &std::collections::BTreeSet<String>,
    hints: &mut Vec<TokenStream2>,
) {
    let syn::Expr::Call(call) = expr else {
        collect_source_expr_ide_hints(expr, hints);
        return;
    };

    match call.func.as_ref() {
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path
                    .path
                    .get_ident()
                    .is_some_and(|ident| event_handler_names.contains(&ident.to_string())) =>
        {
            hints.push(event_binding_source_ide_assertion(
                event,
                tag,
                quote! { #path },
                call.span(),
            ));
            for arg in &call.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        syn::Expr::Path(path) if path.qself.is_none() => {
            hints.push(quote! { let _ = &(#path); });
            for arg in &call.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        other => {
            collect_self_field_receiver_ide_hint(other, hints);
            for arg in &call.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
    }
}

fn event_handler_source_ide_hint(
    expr: &syn::Expr,
    event: rsjs_ir::EventName,
    tag: &str,
) -> Option<TokenStream2> {
    let syn::Expr::Closure(closure) = expr else {
        return None;
    };

    let event_ident = closure.inputs.first().and_then(handler_event_pat_ident);
    let handler_bindings = event_handler_param_bindings(closure);
    let mut body_hints = Vec::new();
    collect_event_handler_ide_hints(closure.body.as_ref(), event_ident.as_ref(), &mut body_hints);

    let event_ty = closure
        .inputs
        .first()
        .and_then(handler_pat_type)
        .map(|ty| quote! { #ty })
        .unwrap_or_else(|| quote! { ::rsjs::Event });
    let event_assertion = event_type_source_ide_assertion(event, tag, event_ty, closure.span());

    Some(quote! {
        {
            #event_assertion
            #handler_bindings
            #(#body_hints)*
        }
    })
}

fn event_interface_source_ide_type(event: rsjs_ir::EventName) -> TokenStream2 {
    let name = event.interface().as_rust_marker_name();
    let ident = quote::format_ident!("{name}");
    quote! { ::rsjs::__private::event_interface::#ident }
}

fn node_ref_target_source_ide_type(tag: &str) -> TokenStream2 {
    element_target_source_ide_type(tag)
}

fn element_target_source_ide_type(tag: &str) -> TokenStream2 {
    let element = rsjs_ir::dom_registry::find_dom_tag(tag)
        .map(|spec| spec.element)
        .unwrap_or(rsjs_ir::dom_registry::DomElementKind::HtmlElement);
    dom_element_kind_source_ide_type(element)
}

fn event_target_source_ide_type(event: rsjs_ir::EventName, tag: &str) -> TokenStream2 {
    if event.is_document_event() {
        quote! { ::rsjs::web::dom::Document }
    } else if event.is_window_event() {
        quote! { ::rsjs::web::dom::Window }
    } else {
        element_target_source_ide_type(tag)
    }
}

fn dom_element_kind_source_ide_type(
    element: rsjs_ir::dom_registry::DomElementKind,
) -> TokenStream2 {
    let name = element.as_rust_marker_name();
    let ident = quote::format_ident!("{name}");
    quote! { ::rsjs::web::dom::#ident }
}

fn event_type_source_ide_assertion(
    event: rsjs_ir::EventName,
    tag: &str,
    event_ty: TokenStream2,
    span: proc_macro2::Span,
) -> TokenStream2 {
    let interface = event_interface_source_ide_type(event);
    let target = event_target_source_ide_type(event, tag);
    quote::quote_spanned! {span=>
        ::rsjs::__private::assert_event_type::<#target, #interface, #event_ty>();
    }
}

fn event_binding_source_ide_assertion(
    event: rsjs_ir::EventName,
    tag: &str,
    binding: TokenStream2,
    span: proc_macro2::Span,
) -> TokenStream2 {
    let interface = event_interface_source_ide_type(event);
    let target = event_target_source_ide_type(event, tag);
    quote::quote_spanned! {span=>
        ::rsjs::__private::assert_event_binding::<#target, #interface, _>(&(#binding));
    }
}

fn event_handler_param_binding(pat: &syn::Pat) -> TokenStream2 {
    match pat {
        syn::Pat::Type(_) => quote! { let #pat = __rsjs_ide_payload(); },
        _ => quote! { let #pat: ::rsjs::Event = __rsjs_ide_payload(); },
    }
}

fn event_handler_param_bindings(closure: &syn::ExprClosure) -> TokenStream2 {
    let bindings = closure
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, pat)| {
            if index == 0 {
                return Some(event_handler_param_binding(pat));
            }
            (!matches!(pat, syn::Pat::Wild(_))).then(|| quote! { let #pat = __rsjs_ide_payload(); })
        });
    quote! { #(#bindings)* }
}

fn handler_event_pat_ident(pat: &syn::Pat) -> Option<syn::Ident> {
    match pat {
        syn::Pat::Ident(ident) => Some(ident.ident.clone()),
        syn::Pat::Type(pat_ty) => handler_event_pat_ident(pat_ty.pat.as_ref()),
        _ => None,
    }
}

fn collect_event_handler_ide_hints(
    expr: &syn::Expr,
    event_ident: Option<&syn::Ident>,
    hints: &mut Vec<TokenStream2>,
) {
    let dom_handles = event_current_target_local_names(expr, event_ident);
    collect_event_handler_ide_hints_inner(expr, event_ident, &dom_handles, hints);
}

fn event_current_target_local_names(
    expr: &syn::Expr,
    event_ident: Option<&syn::Ident>,
) -> std::collections::HashSet<String> {
    struct Collector<'a> {
        event_ident: Option<&'a syn::Ident>,
        names: std::collections::HashSet<String>,
    }

    impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let Some(init) = &local.init {
                if receiver_is_event_current_target_expr(init.expr.as_ref(), self.event_ident) {
                    if let Some(ident) = local_pat_ident(&local.pat) {
                        self.names.insert(ident.to_string());
                    }
                }
            }
            syn::visit::visit_local(self, local);
        }
    }

    let mut collector = Collector {
        event_ident,
        names: std::collections::HashSet::new(),
    };
    syn::visit::Visit::visit_expr(&mut collector, expr);
    collector.names
}

fn receiver_is_named_dom_handle(
    expr: &syn::Expr,
    names: &std::collections::HashSet<String>,
) -> bool {
    match expr {
        syn::Expr::Path(path) => {
            path.qself.is_none()
                && path.path.segments.len() == 1
                && names.contains(path.path.segments[0].ident.to_string().as_str())
        }
        syn::Expr::Paren(paren) => receiver_is_named_dom_handle(paren.expr.as_ref(), names),
        syn::Expr::Group(group) => receiver_is_named_dom_handle(group.expr.as_ref(), names),
        _ => false,
    }
}

fn collect_event_handler_ide_hints_inner(
    expr: &syn::Expr,
    event_ident: Option<&syn::Ident>,
    dom_handles: &std::collections::HashSet<String>,
    hints: &mut Vec<TokenStream2>,
) {
    match expr {
        syn::Expr::MethodCall(method) if method_call_is_self_callback(method) => {
            let callback = &method.method;
            hints.push(quote! { let _ = &(self.#callback); });
            for arg in &method.args {
                collect_event_handler_ide_hints_inner(arg, event_ident, dom_handles, hints);
            }
        }
        syn::Expr::MethodCall(method)
            if method.method == "call" || method.method == "call_if_some" =>
        {
            hints.push(callback_method_call_ide_hint(method));
            for arg in &method.args {
                collect_event_handler_ide_hints_inner(arg, event_ident, dom_handles, hints);
            }
        }
        syn::Expr::MethodCall(method)
            if receiver_is_event_current_target_expr(method.receiver.as_ref(), event_ident) =>
        {
            hints.push(quote! { let _ = &(#expr); });
        }
        syn::Expr::MethodCall(method)
            if receiver_is_event_param(method.receiver.as_ref(), event_ident) =>
        {
            hints.push(quote! { let _ = &(#expr); });
        }
        syn::Expr::MethodCall(method)
            if receiver_is_named_dom_handle(method.receiver.as_ref(), dom_handles) =>
        {
            hints.push(quote! { let _ = &(#expr); });
        }
        syn::Expr::MethodCall(method) => {
            collect_event_handler_ide_hints_inner(
                method.receiver.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
            for arg in &method.args {
                collect_event_handler_ide_hints_inner(arg, event_ident, dom_handles, hints);
            }
        }
        syn::Expr::Call(call) => {
            let qualified_source_call = match call.func.as_ref() {
                syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() >= 2 => {
                    let root = path
                        .path
                        .segments
                        .first()
                        .expect("len checked")
                        .ident
                        .to_string();
                    !matches!(root.as_str(), "rsjs" | "std" | "core" | "alloc" | "console")
                }
                _ => false,
            };
            if qualified_source_call {
                // Imported `#[rsjs::client_module]` calls in handlers are
                // stripped from generated server execution. Keep an uncalled
                // closure so rustc still validates helper arity and argument
                // types without performing the call.
                hints.push(quote! {
                    let _ = || {
                        let _ = #call;
                    };
                });
            } else {
                collect_self_field_receiver_ide_hint(call.func.as_ref(), hints);
            }
            for arg in &call.args {
                collect_event_handler_ide_hints_inner(arg, event_ident, dom_handles, hints);
            }
        }
        syn::Expr::Binary(binary) => {
            collect_event_handler_ide_hints_inner(
                binary.left.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
            collect_event_handler_ide_hints_inner(
                binary.right.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
        }
        syn::Expr::Unary(unary) => {
            collect_event_handler_ide_hints_inner(
                unary.expr.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
        }
        syn::Expr::Paren(paren) => {
            collect_event_handler_ide_hints_inner(
                paren.expr.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
        }
        syn::Expr::Reference(reference) => {
            collect_event_handler_ide_hints_inner(
                reference.expr.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
        }
        syn::Expr::Block(block) => {
            collect_event_handler_block_ide_hints(&block.block, event_ident, dom_handles, hints);
        }
        syn::Expr::If(if_expr) => {
            collect_event_handler_ide_hints_inner(
                if_expr.cond.as_ref(),
                event_ident,
                dom_handles,
                hints,
            );
            collect_event_handler_block_ide_hints(
                &if_expr.then_branch,
                event_ident,
                dom_handles,
                hints,
            );
            if let Some((_, else_expr)) = &if_expr.else_branch {
                collect_event_handler_ide_hints_inner(
                    else_expr.as_ref(),
                    event_ident,
                    dom_handles,
                    hints,
                );
            }
        }
        _ => {}
    }
}

fn collect_event_handler_block_ide_hints(
    block: &syn::Block,
    event_ident: Option<&syn::Ident>,
    dom_handles: &std::collections::HashSet<String>,
    hints: &mut Vec<TokenStream2>,
) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    if init.diverge.is_none() {
                        let pat = &local.pat;
                        let expr = init.expr.as_ref();
                        if handler_local_is_owned_client_string(expr)
                            && matches!(pat, syn::Pat::Ident(_))
                        {
                            // Client string transforms, including `trim`,
                            // produce owned JavaScript strings. Replaying
                            // Rust's borrowed `str::trim` result would borrow
                            // from the temporary String returned by an event
                            // read and introduce an artificial E0716 in this
                            // never-called source witness.
                            hints.push(quote! {
                                let #pat: ::std::string::String = __rsjs_ide_payload();
                            });
                            collect_event_handler_ide_hints_inner(
                                expr,
                                event_ident,
                                dom_handles,
                                hints,
                            );
                            continue;
                        }
                        // Preserve handler-local names and infer their real
                        // Rust types without replaying the authored value.
                        // Later source hints may refer to these locals (for
                        // example, a measured width passed to `f64::from`).
                        // Borrowing the initializer and returning a diverging
                        // value of the same type also avoids an extra move or
                        // browser-side effect in this never-called IDE
                        // witness.
                        hints.push(quote! {
                            let #pat = __rsjs_ide_infer(&(#expr));
                        });
                    } else {
                        collect_event_handler_ide_hints_inner(
                            init.expr.as_ref(),
                            event_ident,
                            dom_handles,
                            hints,
                        );
                    }
                }
            }
            syn::Stmt::Expr(expr, _) => {
                collect_event_handler_ide_hints_inner(expr, event_ident, dom_handles, hints);
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
    }
}

fn handler_local_is_owned_client_string(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(method) => method.method == "trim",
        syn::Expr::Paren(paren) => handler_local_is_owned_client_string(&paren.expr),
        syn::Expr::Group(group) => handler_local_is_owned_client_string(&group.expr),
        _ => false,
    }
}

fn receiver_is_event_param(expr: &syn::Expr, event_ident: Option<&syn::Ident>) -> bool {
    let Some(event_ident) = event_ident else {
        return false;
    };
    matches!(
        expr,
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == *event_ident
    )
}

fn receiver_is_event_current_target_expr(
    expr: &syn::Expr,
    event_ident: Option<&syn::Ident>,
) -> bool {
    match expr {
        syn::Expr::MethodCall(call) => {
            call.method == "current_target"
                && call.args.is_empty()
                && receiver_is_event_param(call.receiver.as_ref(), event_ident)
        }
        syn::Expr::Paren(paren) => {
            receiver_is_event_current_target_expr(paren.expr.as_ref(), event_ident)
        }
        _ => false,
    }
}

fn collect_self_field_receiver_ide_hint(expr: &syn::Expr, hints: &mut Vec<TokenStream2>) {
    match expr {
        syn::Expr::Field(field) if expr_is_self(field.base.as_ref()) => {
            hints.push(quote! { let _ = &(#field); });
        }
        syn::Expr::Paren(paren) => collect_self_field_receiver_ide_hint(paren.expr.as_ref(), hints),
        _ => {}
    }
}

fn expr_is_self(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == "self"
    )
}

fn rsjs_for_loop_source_ide_hint(
    for_loop: &syn::ExprForLoop,
    inbound_live_field_signals: &std::collections::BTreeMap<String, rsjs_ir::SignalId>,
    event_handler_names: &std::collections::BTreeSet<String>,
) -> Option<TokenStream2> {
    let for_loop = match substitute_inbound_live_fields_for_ide(
        &syn::Expr::ForLoop(for_loop.clone()),
        inbound_live_field_signals,
    ) {
        syn::Expr::ForLoop(for_loop) => for_loop,
        _ => unreachable!("folding a for loop must preserve its expression kind"),
    };
    let (prefix, tail_tokens) = split_loop_prefix_and_tail_html_tokens(&for_loop.body)?;
    if prefix
        .iter()
        .any(|stmt| !matches!(stmt, syn::Stmt::Local(_)))
    {
        return None;
    }

    let prefix_tokens: TokenStream2 = prefix.iter().map(|stmt| quote! { #stmt }).collect();
    if prefix_tokens.to_string().contains('?') {
        return None;
    }

    let inner = syn::parse2::<proa_dsl::parse::HtmlRoot>(tail_tokens).ok()?;
    let mut inner_hints = Vec::new();
    collect_rsjs_source_ide_hints_from_nodes(
        &inner.nodes,
        &mut inner_hints,
        inbound_live_field_signals,
        event_handler_names,
    );
    let inner_hints = quote! { #(#inner_hints)* };
    let pat = &for_loop.pat;
    let source_expr = &for_loop.expr;
    let iter_expr = loop_source_ide_iter_expr(source_expr.as_ref())?;

    Some(quote! {
        {
            let _ = &(#source_expr);
            for #pat in #iter_expr {
            #prefix_tokens
            #inner_hints
            }
        }
    })
}

fn collect_source_expr_ide_hints(expr: &syn::Expr, hints: &mut Vec<TokenStream2>) {
    if let Some(inner) = helper_inner_expr(expr, "server_only") {
        hints.push(quote! { let _ = &(#inner); });
        return;
    }

    match expr {
        syn::Expr::Path(path) if path.qself.is_none() => hints.push(quote! { let _ = &(#path); }),
        syn::Expr::Field(_) | syn::Expr::Index(_) => hints.push(quote! { let _ = &(#expr); }),
        syn::Expr::MethodCall(method) if method.method == "contains" => {
            collect_source_expr_ide_hints(method.receiver.as_ref(), hints);
            for arg in &method.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        syn::Expr::MethodCall(method)
            if method.method == "call" || method.method == "call_if_some" =>
        {
            hints.push(callback_method_call_ide_hint(method));
            for arg in &method.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        syn::Expr::MethodCall(method) if method_call_is_self_callback(method) => {
            let callback = &method.method;
            hints.push(quote! { let _ = &(self.#callback); });
            for arg in &method.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        syn::Expr::MethodCall(_) => hints.push(quote! { let _ = &(#expr); }),
        syn::Expr::Call(call) => {
            collect_self_field_receiver_ide_hint(call.func.as_ref(), hints);
            for arg in &call.args {
                collect_source_expr_ide_hints(arg, hints);
            }
        }
        syn::Expr::Binary(binary) => {
            collect_source_expr_ide_hints(binary.left.as_ref(), hints);
            collect_source_expr_ide_hints(binary.right.as_ref(), hints);
        }
        syn::Expr::Unary(unary) => {
            collect_source_expr_ide_hints(unary.expr.as_ref(), hints);
        }
        syn::Expr::Paren(paren) => {
            collect_source_expr_ide_hints(paren.expr.as_ref(), hints);
        }
        syn::Expr::Reference(reference) => {
            collect_source_expr_ide_hints(reference.expr.as_ref(), hints);
        }
        syn::Expr::Block(block) => {
            collect_source_block_ide_hints(&block.block, hints);
        }
        syn::Expr::If(if_expr) => {
            collect_source_expr_ide_hints(if_expr.cond.as_ref(), hints);
            collect_source_block_ide_hints(&if_expr.then_branch, hints);
            if let Some((_, else_expr)) = &if_expr.else_branch {
                collect_source_expr_ide_hints(else_expr.as_ref(), hints);
            }
        }
        _ => {}
    }
}

fn method_call_is_self_callback(method: &syn::ExprMethodCall) -> bool {
    if !method.method.to_string().starts_with("on_") {
        return false;
    }
    matches!(
        method.receiver.as_ref(),
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == "self"
    )
}

fn callback_method_call_ide_hint(method: &syn::ExprMethodCall) -> TokenStream2 {
    let receiver = method.receiver.as_ref();
    let method_name = &method.method;
    if method.method == "call_if_some" {
        quote! {
            {
                use ::rsjs::OptionalHandlerExt as _;
                let _ = &(#receiver.#method_name(__rsjs_ide_payload()));
            }
        }
    } else {
        quote! { let _ = &(#receiver.#method_name(__rsjs_ide_payload())); }
    }
}

fn collect_source_block_ide_hints(block: &syn::Block, hints: &mut Vec<TokenStream2>) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    collect_source_expr_ide_hints(init.expr.as_ref(), hints);
                }
            }
            syn::Stmt::Item(_) => {}
            syn::Stmt::Expr(expr, _) => {
                collect_source_expr_ide_hints(expr, hints);
            }
            syn::Stmt::Macro(_) => {}
        }
    }
}

fn loop_source_ide_iter_expr(expr: &syn::Expr) -> Option<TokenStream2> {
    match expr {
        syn::Expr::Range(_) => Some(quote! { #expr }),
        syn::Expr::MethodCall(_) if method_chain_has_iter(expr) => Some(quote! { #expr }),
        syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_) => {
            Some(quote! { (#expr).iter() })
        }
        syn::Expr::Reference(reference) => {
            let inner = reference.expr.as_ref();
            Some(quote! { (#inner).iter() })
        }
        syn::Expr::Paren(paren) => loop_source_ide_iter_expr(paren.expr.as_ref()),
        _ => None,
    }
}

fn method_chain_has_iter(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(method) => {
            let name = method.method.to_string();
            name == "iter" || name == "iter_mut" || method_chain_has_iter(method.receiver.as_ref())
        }
        syn::Expr::Paren(paren) => method_chain_has_iter(paren.expr.as_ref()),
        syn::Expr::Reference(reference) => method_chain_has_iter(reference.expr.as_ref()),
        _ => false,
    }
}

fn helper_inner_expr<'a>(expr: &'a syn::Expr, helper: &str) -> Option<&'a syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    if path.path.segments.last()?.ident != helper || call.args.len() != 1 {
        return None;
    }
    call.args.first()
}

fn split_loop_prefix_and_tail_html_tokens(
    block: &syn::Block,
) -> Option<(&[syn::Stmt], TokenStream2)> {
    let (last, prefix) = block.stmts.split_last()?;
    match last {
        syn::Stmt::Expr(expr, _) => {
            let syn::Expr::Macro(mac) = expr else {
                return None;
            };
            if is_html_macro_path(&mac.mac.path) {
                Some((prefix, mac.mac.tokens.clone()))
            } else {
                None
            }
        }
        syn::Stmt::Macro(mac) if is_html_macro_path(&mac.mac.path) => {
            Some((prefix, mac.mac.tokens.clone()))
        }
        _ => None,
    }
}

fn is_html_macro_path(path: &syn::Path) -> bool {
    path.segments
        .last()
        .map(|segment| {
            let ident = segment.ident.to_string();
            ident == "html_sync" || ident == "html"
        })
        .unwrap_or(false)
}

fn emit_manifest_hydration_strategy(strategy: &rsjs_ir::HydrationStrategy) -> TokenStream2 {
    use proc_macro2::Span;
    use quote::quote;

    match strategy {
        rsjs_ir::HydrationStrategy::Immediate => quote!(::rsjs::IslandHydrationStrategy::Immediate),
        rsjs_ir::HydrationStrategy::Visible => quote!(::rsjs::IslandHydrationStrategy::Visible),
        rsjs_ir::HydrationStrategy::Idle => quote!(::rsjs::IslandHydrationStrategy::Idle),
        rsjs_ir::HydrationStrategy::Interaction { event } => {
            if let Some(event) = event {
                let event = syn::LitStr::new(event, Span::call_site());
                quote!(::rsjs::IslandHydrationStrategy::Interaction {
                    event: Some(#event),
                })
            } else {
                quote!(::rsjs::IslandHydrationStrategy::Interaction { event: None })
            }
        }
        rsjs_ir::HydrationStrategy::Media { query } => {
            let query = syn::LitStr::new(query, Span::call_site());
            quote!(::rsjs::IslandHydrationStrategy::Media { query: #query })
        }
    }
}

fn emit_component_fragment_registration(
    asset_id: &str,
    struct_name: &str,
    fragment_js: &str,
    fragment_export_name: &str,
) -> TokenStream2 {
    use proc_macro2::Span;
    use quote::{format_ident, quote};

    let asset_id_lit = syn::LitStr::new(asset_id, Span::call_site());
    let struct_name_lit = syn::LitStr::new(struct_name, Span::call_site());
    let fragment_js_lit = syn::LitStr::new(fragment_js, Span::call_site());
    let fragment_export_lit = syn::LitStr::new(fragment_export_name, Span::call_site());
    let entry_ident = format_ident!("__RSJS_COMPONENT_FRAGMENT_ENTRY_{}", asset_id);
    let submit_ident = format_ident!("__RSJS_COMPONENT_FRAGMENT_SUBMIT_{}", asset_id);

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, non_upper_case_globals)]
        const _: () = {
            static #entry_ident: ::rsjs::ComponentFragmentManifestEntry =
                ::rsjs::ComponentFragmentManifestEntry {
                    asset_id: #asset_id_lit,
                    struct_name: #struct_name_lit,
                    export_name: #fragment_export_lit,
                    js_source: #fragment_js_lit,
                };

            #[::rsjs::linkme::distributed_slice(::rsjs::COMPONENT_FRAGMENT_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static #submit_ident: &'static ::rsjs::ComponentFragmentManifestEntry =
                &#entry_ident;
        };
    }
}

fn emit_linked_owner_manifest_registration(
    plan: &rsjs_ir::LinkedOwnerModulePlan,
    js: &str,
) -> TokenStream2 {
    use proc_macro2::Span;
    use quote::{format_ident, quote};

    let asset_id_lit = syn::LitStr::new(&plan.owner_asset_id, Span::call_site());
    let struct_name = format!("LinkedOwner{}", plan.root_candidate_id);
    let struct_name_lit = syn::LitStr::new(&struct_name, Span::call_site());
    let js_lit = syn::LitStr::new(js, Span::call_site());
    let hydration_strategy = match &plan.strategy {
        rsjs_ir::link::OwnerActivationStrategy::Eager => {
            quote!(::rsjs::IslandHydrationStrategy::Immediate)
        }
        rsjs_ir::link::OwnerActivationStrategy::Visible => {
            quote!(::rsjs::IslandHydrationStrategy::Visible)
        }
        rsjs_ir::link::OwnerActivationStrategy::Idle => {
            quote!(::rsjs::IslandHydrationStrategy::Idle)
        }
        rsjs_ir::link::OwnerActivationStrategy::Interaction { event } => {
            let event = match event {
                Some(event) => {
                    let event = syn::LitStr::new(event, Span::call_site());
                    quote!(::core::option::Option::Some(#event))
                }
                None => quote!(::core::option::Option::None),
            };
            quote!(::rsjs::IslandHydrationStrategy::Interaction {
                event: #event,
            })
        }
        rsjs_ir::link::OwnerActivationStrategy::Media { query } => {
            let query = syn::LitStr::new(query, Span::call_site());
            quote!(::rsjs::IslandHydrationStrategy::Media { query: #query })
        }
        _ => unreachable!("the linked owner emitter accepted an unknown activation strategy"),
    };
    let fragment_imports = plan
        .mounts
        .iter()
        .map(|mount| mount.fragment_asset_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|asset| syn::LitStr::new(asset, Span::call_site()))
        .collect::<Vec<_>>();
    let uses_queries = plan.uses_queries;
    let uses_mutations = plan.uses_mutations;
    let entry_ident = format_ident!("__RSJS_LINKED_OWNER_ENTRY_{}", plan.owner_asset_id);
    let submit_ident = format_ident!("__RSJS_LINKED_OWNER_SUBMIT_{}", plan.owner_asset_id);

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, non_upper_case_globals)]
        const _: () = {
            static #entry_ident: ::rsjs::IslandManifestEntry =
                ::rsjs::IslandManifestEntry {
                    asset_id: #asset_id_lit,
                    struct_name: #struct_name_lit,
                    js_source: #js_lit,
                    is_async: false,
                    hydration_strategy: #hydration_strategy,
                    client_module_imports: &[],
                    component_fragment_imports: &[#(#fragment_imports),*],
                    uses_queries: #uses_queries,
                    uses_mutations: #uses_mutations,
                };

            #[::rsjs::linkme::distributed_slice(::rsjs::ISLAND_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static #submit_ident: &'static ::rsjs::IslandManifestEntry = &#entry_ident;
        };
    }
}

/// Emit the M7g manifest-registration block for an island.
///
/// Each hydrated `#[rsjs(component...)]` impl block expands to an extra
/// `#[doc(hidden)] const _: () = { ... };` scope that:
///
/// 1. Stores a `static __RSJS_ISLAND_ENTRY` of type
///    `::rsjs::IslandManifestEntry` carrying the island's identity
///    (`asset_id`, `struct_name`, `is_async`) and JS source as a
///    `&'static str` literal baked into `.rodata`.
/// 2. Submits a `&'static IslandManifestEntry` reference into
///    `::rsjs::ISLAND_REGISTRY` via `linkme::distributed_slice`.
///
/// The registration uses `crate = ::rsjs::linkme` so user crates do
/// not need a direct `linkme` dependency.
///
/// `client_module_imports` and `component_fragment_imports` are the
/// reachability-filtered asset edges from the strategy pass. Unused legacy
/// bundle paths and server-only descriptor children
/// are intentionally absent. The IR keeps canonical string forms such as
/// `"crate::helpers::pricing"` and fragment asset ids; this function bakes
/// each entry as a `&'static str` literal into the manifest entry so framework
/// adapters and the optional collector can resolve bundle deps at link time.
#[allow(clippy::too_many_arguments)]
fn emit_manifest_registration(
    asset_id: &str,
    struct_name: &str,
    js: &str,
    is_async: bool,
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    uses_queries: bool,
    uses_mutations: bool,
    struct_path: &syn::Path,
    client_module_imports: &[String],
    component_fragment_imports: &[String],
    fragment_js: &str,
    fragment_export_name: &str,
) -> TokenStream2 {
    use proc_macro2::Span;
    use quote::{format_ident, quote};

    let asset_id_lit = syn::LitStr::new(asset_id, Span::call_site());
    let struct_name_lit = syn::LitStr::new(struct_name, Span::call_site());
    let js_lit = syn::LitStr::new(js, Span::call_site());
    let fragment_js_lit = syn::LitStr::new(fragment_js, Span::call_site());
    let fragment_export_lit = syn::LitStr::new(fragment_export_name, Span::call_site());
    let hydration_strategy_tokens = emit_manifest_hydration_strategy(hydration_strategy);

    // Use the lowercased asset id to disambiguate the static names
    // when multiple islands compile in the same module. Linkme
    // requires every distributed-slice submission to have a unique
    // ident at the source-text level.
    let entry_ident = format_ident!("__RSJS_ISLAND_ENTRY_{}", asset_id);
    let fragment_entry_ident = format_ident!("__RSJS_COMPONENT_FRAGMENT_ENTRY_{}", asset_id);
    let submit_ident = format_ident!("__RSJS_ISLAND_SUBMIT_{}", asset_id);
    let fragment_submit_ident = format_ident!("__RSJS_COMPONENT_FRAGMENT_SUBMIT_{}", asset_id);

    // Suppress an "unused" warning if the user references the path
    // only through trait dispatch.
    let _ = struct_path;

    let import_lits: Vec<syn::LitStr> = client_module_imports
        .iter()
        .map(|p| syn::LitStr::new(p, Span::call_site()))
        .collect();
    let fragment_import_lits: Vec<syn::LitStr> = component_fragment_imports
        .iter()
        .map(|p| syn::LitStr::new(p, Span::call_site()))
        .collect();

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, non_upper_case_globals)]
        const _: () = {
            static #entry_ident: ::rsjs::IslandManifestEntry = ::rsjs::IslandManifestEntry {
                asset_id: #asset_id_lit,
                struct_name: #struct_name_lit,
                js_source: #js_lit,
                is_async: #is_async,
                hydration_strategy: #hydration_strategy_tokens,
                client_module_imports: &[ #( #import_lits ),* ],
                component_fragment_imports: &[ #( #fragment_import_lits ),* ],
                uses_queries: #uses_queries,
                uses_mutations: #uses_mutations,
            };

            static #fragment_entry_ident: ::rsjs::ComponentFragmentManifestEntry = ::rsjs::ComponentFragmentManifestEntry {
                asset_id: #asset_id_lit,
                struct_name: #struct_name_lit,
                export_name: #fragment_export_lit,
                js_source: #fragment_js_lit,
            };

            // The `crate = ::rsjs::linkme` attribute redirects linkme's
            // internal type lookups through rsjs's re-export, so user
            // crates do not need to depend on `linkme` directly.
            #[::rsjs::linkme::distributed_slice(::rsjs::ISLAND_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static #submit_ident: &'static ::rsjs::IslandManifestEntry = &#entry_ident;

            #[::rsjs::linkme::distributed_slice(::rsjs::COMPONENT_FRAGMENT_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static #fragment_submit_ident: &'static ::rsjs::ComponentFragmentManifestEntry = &#fragment_entry_ident;
        };
    }
}

/// Recover the unqualified user struct name (e.g. `Counter`) from
/// the analyzed component impl block's self-type path.
fn derive_struct_name(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_else(|| "Island".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn ordinary_compilation_mirrors_tooling_artifacts() {
        let writes = std::cell::Cell::new(0_u8);
        mirror_filesystem_artifacts(CompilationPhase::Plain, || writes.set(writes.get() + 1));
        assert_eq!(writes.get(), 1);
        assert!(CompilationPhase::Plain.allows_legacy_filesystem_artifacts());
    }

    #[test]
    fn orchestrated_compilation_never_mirrors_tooling_artifacts() {
        for phase in [CompilationPhase::Descriptor, CompilationPhase::Final] {
            let writes = std::cell::Cell::new(0_u8);
            mirror_filesystem_artifacts(phase, || writes.set(writes.get() + 1));
            assert_eq!(writes.get(), 0, "{phase:?} unexpectedly invoked a mirror");
            assert!(!phase.allows_legacy_filesystem_artifacts());
        }
    }

    #[test]
    fn descriptor_cargo_unit_id_decodes_exact_lowercase_hex() {
        let encoded = "000102030405060708090a0b0c0d0e0f\
                       101112131415161718191a1b1c1d1e1f";
        let decoded =
            parse_descriptor_cargo_unit_id(encoded).expect("valid identity should decode");
        assert_eq!(decoded, core::array::from_fn(|index| index as u8));
    }

    #[test]
    fn descriptor_cargo_unit_id_rejects_wrong_width_and_uppercase() {
        let wrong_width =
            parse_descriptor_cargo_unit_id("00").expect_err("short identity must fail");
        assert!(wrong_width.contains("exactly 64"));

        let uppercase =
            parse_descriptor_cargo_unit_id(&"A0".repeat(32)).expect_err("uppercase must fail");
        assert!(uppercase.contains("non-lowercase-hex byte at offset 0"));
    }

    #[test]
    fn text_intrinsic_path_recognition_is_narrow() {
        for accepted in ["text", "proa_macros::text", "::proa_macros::text"] {
            let path: syn::Path = syn::parse_str(accepted).expect("accepted path should parse");
            assert!(
                is_text_intrinsic_path(&path),
                "expected {accepted} to match"
            );
        }

        for rejected in [
            "::text",
            "foreign::text",
            "crate::text",
            "foreign::proa_macros::text",
            "proa_macros::other",
        ] {
            let path: syn::Path = syn::parse_str(rejected).expect("rejected path should parse");
            assert!(
                !is_text_intrinsic_path(&path),
                "expected {rejected} not to match"
            );
        }
    }

    #[test]
    fn rsjs_expansion_does_not_emit_tool_specific_cfg_branches() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let count = rsjs::signal(0_i64);
                        html_sync! {
                            <span>{count.get()}</span>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("simple rsjs impl should expand")
        .to_string();

        assert!(
            !out.contains("rust_analyzer"),
            "rsjs expansion should not emit an inactive rust_analyzer branch:\n{out}"
        );
        assert!(
            !out.contains("not (rust_analyzer)") && !out.contains("cfg (rust_analyzer)"),
            "rsjs expansion should not duplicate the impl behind tool-specific cfgs:\n{out}"
        );
        assert!(
            out.contains("ISLAND_REGISTRY") && out.contains("COMPONENT_FRAGMENT_REGISTRY"),
            "hydrated rsjs output should still register runtime island assets:\n{out}"
        );
    }

    #[test]
    fn component_preserves_authored_render_impl_and_context_name() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LaunchCounter {
                    fn render(
                        self,
                        output: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        let count = rsjs::signal::<i32>(self.initial);
                        proa_macros::html_sync! {
                            <button type="button" on_click={|_| count.update(|value| value + 1)}>
                                {count.get()}
                            </button>
                        }
                        .render(output)
                    }
                }
            },
        )
        .expect("explicit component trait impl should expand")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            compact.contains("impl<L:DataLoader,B:WriteBuf>WebRenderSync<L,B>forLaunchCounter"),
            "component expansion should preserve the authored trait impl:\n{out}"
        );
        assert!(
            compact.contains("fnrender(self,output:&mutWebContext<L,B>,)")
                && compact.contains("->Result<(),WriteError>")
                && compact.contains("output.rsjs_island_used"),
            "component expansion should preserve and reuse the authored context name:\n{out}"
        );
        assert!(
            !out.contains("Box <") && !out.contains("dyn "),
            "component expansion must not introduce boxed or dynamic rendering:\n{out}"
        );
    }

    #[test]
    fn component_emits_exactly_one_defining_abi_static_and_impl() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AbiCard {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <article>"ABI"</article>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("component ABI definition should expand")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert_eq!(
            compact
                .matches("static__RSJS_COMPONENT_ABI_WITNESS_")
                .count(),
            1,
            "one defining static must be emitted per component:\n{out}"
        );
        assert_eq!(
            compact
                .matches("static__RSJS_COMPONENT_DEFINITION_")
                .count(),
            1,
            "one fixed transport definition record must be emitted per component:\n{out}"
        );
        assert_eq!(
            compact
                .matches("impl::rsjs::__private::ComponentAbiforAbiCard")
                .count(),
            1,
            "one ComponentAbi impl must be emitted per component:\n{out}"
        );
        assert!(
            compact.contains("ComponentRenderKind::Sync")
                && compact.contains("ComponentOriginPermission::ParticipantOnly")
                && compact.contains("descriptor_schema_version:9u64"),
            "defining metadata should retain render, permission, and descriptor distinctions:\n{out}"
        );
        assert!(
            compact.contains("ComponentDefinitionRecord")
                && compact.contains(".transport_definition_abi()")
                && compact.contains("__DATA,__rsjs_comp")
                && compact.contains(".rsjs.component")
                && compact.contains(".rsjs$C"),
            "the fixed cross-format record must project ABI facts from the defining witness:\n{out}"
        );
        assert_eq!(
            compact.matches("static__RSJS_COMPONENT_IDENTITY_").count(),
            1,
            "one normalized semantic identity must accompany the exact definition:\n{out}"
        );
        assert!(
            compact.contains("SemanticIdentityRecord")
                && compact.contains("env!(\"CARGO_PKG_NAME\")")
                && compact.contains("env!(\"CARGO_CRATE_NAME\")")
                && compact.contains("module_path!()")
                && compact.contains("stringify!(AbiCard)")
                && compact.contains("__DATA,__rsjs_ident")
                && compact.contains(".rsjs.identity")
                && compact.contains(".rsjs$S"),
            "component identity must come from the defining expansion, not token-path lookup:\n{out}"
        );
        assert_eq!(
            compact.matches("static__RSJS_COMPONENT_PAYLOAD_").count(),
            1,
            "one complete inline descriptor payload must accompany the definition:\n{out}"
        );
        assert!(
            compact.contains("PayloadRecord")
                && compact.contains("__DATA,__rsjs_cpay")
                && compact.contains(".rsjs.component_payload")
                && compact.contains(".rsjs$P"),
            "the descriptor payload must use dedicated cross-format sections:\n{out}"
        );
    }

    #[test]
    fn one_open_generic_static_slot_has_no_client_field_or_codec_obligation() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L, B, C> WebRenderSync<L, B> for Disclosure<C>
                where
                    B: WriteBuf,
                    L: DataLoader,
                    C: WebRenderSync<L, B>,
                {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        let open = rsjs::signal(false);
                        html_sync! {
                            <section data-open={open.get()}>
                                {self.children}
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("one generic server slot should expand without a client codec")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            compact.contains("let_:*constC=::core::ptr::addr_of!(self.children)")
                && compact.contains("ComponentAbiforDisclosure<C>"),
            "rustc must validate the exact generic field and family ABI:\n{out}"
        );
        assert!(
            !compact.contains("assert_component_value_field(&self.children)")
                && !compact.contains("__RSJS_COMPONENT_FIELD_"),
            "a static-only slot must not emit a client field witness or ClientEncode bound:\n{out}"
        );
        assert!(
            out.contains("\\\"kind\\\":\\\"static_slot\\\"")
                && out.contains("\\\"permits_nested_owner\\\":false"),
            "the descriptor must carry the immutable slot contract:\n{out}"
        );
    }

    #[test]
    fn sole_generic_value_field_remains_a_live_prop_not_a_static_slot() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L, B, T> WebRenderSync<L, B> for GenericValue<T>
                where
                    B: WriteBuf,
                    L: DataLoader,
                    T: WebRenderSync<L, B>,
                {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <span>{self.value}</span>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("a generic scalar participant should retain its live field ABI")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            compact.contains("assert_component_value_field(&self.value)")
                && compact.contains("__RSJS_COMPONENT_FIELD_"),
            "a generic scalar value must remain a rustc-witnessed live prop:\n{out}"
        );
        assert!(
            !compact.contains("__rsjs_validate_generic_static_slot_")
                && !out.contains("\\\"kind\\\":\\\"static_slot\\\""),
            "a sole generic `value` field must not be silently frozen as a static slot:\n{out}"
        );
    }

    fn component_field_input_fixture() -> analyze::InboundInputMetadata {
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 12,
            start_column: 9,
        };
        analyze::InboundInputMetadata {
            fields: vec![
                analyze::InboundField {
                    id: 0,
                    name: "initial".to_string(),
                    role: analyze::InboundFieldRole::Value,
                    value: Some(0),
                    callback_shape: None,
                    crossing: 0,
                    source_anchor: anchor,
                },
                analyze::InboundField {
                    id: 1,
                    name: "on_change".to_string(),
                    role: analyze::InboundFieldRole::Callback,
                    value: Some(1),
                    callback_shape: Some(analyze::InboundCallbackShape::Required),
                    crossing: 1,
                    source_anchor: anchor,
                },
                analyze::InboundField {
                    id: 2,
                    name: "on_optional".to_string(),
                    role: analyze::InboundFieldRole::Callback,
                    value: Some(2),
                    callback_shape: Some(analyze::InboundCallbackShape::Optional),
                    crossing: 2,
                    source_anchor: anchor,
                },
            ],
            values: (0..3)
                .map(|id| analyze::InboundValue {
                    id,
                    field: id,
                    source_anchor: anchor,
                })
                .collect(),
            prop_sites: vec![analyze::InboundPropSite {
                id: 0,
                field: 0,
                value: 0,
                crossing: 0,
                temporal: analyze::InboundTemporalMode::SeedOnce,
                target: analyze::InboundPropTarget::SignalSeed(rsjs_ir::SignalId(0)),
                source_anchor: anchor,
            }],
            crossings: vec![
                analyze::InboundCrossing {
                    id: 0,
                    field: 0,
                    kind: analyze::InboundCrossingKind::Value,
                    source_anchor: anchor,
                },
                analyze::InboundCrossing {
                    id: 1,
                    field: 1,
                    kind: analyze::InboundCrossingKind::CallbackResolveCurrent,
                    source_anchor: anchor,
                },
                analyze::InboundCrossing {
                    id: 2,
                    field: 2,
                    kind: analyze::InboundCrossingKind::CallbackResolveCurrent,
                    source_anchor: anchor,
                },
            ],
            node_ref_fields: vec![],
            projection_fields: vec![],
        }
    }

    #[test]
    fn client_visible_fields_emit_one_rust_checked_record_each() {
        let expand = || {
            expand_rsjs(
                quote! { component, client },
                quote! {
                    impl<L, B, T: Clone> WebRenderSync<L, B> for GenericFieldProbe<T>
                    where
                        B: WriteBuf,
                        L: DataLoader,
                        T: Send,
                    {
                        fn render(
                            self,
                            cx: &mut WebContext<L, B>,
                        ) -> Result<(), WriteError> {
                            let count = rsjs::signal(self.initial);
                            html_sync! {
                                <button on_click={|_| {
                                    self.on_change.call(count.get());
                                    self.on_optional.call_if_some(count.get());
                                }}>
                                    {count.get()}
                                </button>
                            }
                            .render(cx)
                        }
                    }
                },
            )
            .expect("client-visible fields should emit Rust witness records")
            .to_string()
        };

        let first = expand();
        let second = expand();
        let compact = first.split_whitespace().collect::<String>();

        assert_eq!(first, second, "field witness tokens must be deterministic");
        assert_eq!(
            compact.matches("__rsjs_emit_component_field!").count(),
            3,
            "one record must be emitted for each value/required/optional field:\n{first}"
        );
        assert!(
            compact.contains("__RSJS_COMPONENT_FIELD_genericfieldprobe_")
                && compact.contains("_0,")
                && compact.contains("_1,")
                && compact.contains("_2,")
                && compact.contains("&__RSJS_COMPONENT_ABI_WITNESS_genericfieldprobe_"),
            "every field record must relocate to this component's defining witness:\n{first}"
        );
        assert!(
            compact.contains("impl<T:Clone>GenericFieldProbe<T>whereT:Send")
                && compact.contains("assert_component_value_field(&self.initial)")
                && compact.contains("assert_required_component_callback(&self.on_change)")
                && compact.contains("assert_optional_component_callback(&self.on_optional)"),
            "the never-called inherent method must preserve self generics and check real projections:\n{first}"
        );
        assert_eq!(
            compact
                .matches("__rsjs_validate_component_fields_genericfieldprobe_")
                .count(),
            1,
            "the validation method must only be defined, never called from the render path:\n{first}"
        );
    }

    #[test]
    fn inbound_node_ref_witness_checks_required_shape_and_bound_element_family() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L, B> WebRenderSync<L, B> for InputRefParticipant
                where
                    B: WriteBuf,
                    L: DataLoader,
                {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <input ref={self.input_ref} />
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("a required inbound NodeRef<T> is valid participant syntax")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            compact.contains("assert_component_node_ref_field(&self.input_ref)")
                && compact.contains(
                    "assert_component_node_ref_target::<_,::rsjs::web::dom::HtmlInputElement>(&self.input_ref)"
                ),
            "the generated Rust witness must prove both required NodeRef<T> shape and input compatibility:\n{out}"
        );
        assert!(
            out.contains("\\\"role\\\":\\\"node_ref\\\"")
                && out.contains("\\\"bind_target\\\":\\\"input\\\""),
            "descriptor transport must retain the exact handle role and bound element family:\n{out}"
        );
    }

    #[test]
    fn component_payload_claims_deterministic_field_witnesses() {
        let input_fingerprint = "sha256:component-field-witness-fixture";
        let inbound = component_field_input_fixture();
        let first = component_field_witness_claims(input_fingerprint, &inbound)
            .expect("valid inbound fields should produce witness claims");
        let second = component_field_witness_claims(input_fingerprint, &inbound)
            .expect("field witness claims should be repeatable");
        assert_eq!(first, second);
        assert_eq!(
            first[0].field_record_id,
            rsjs_transport::component_field_record_id(
                input_fingerprint,
                0,
                0,
                rsjs_transport::TransportComponentFieldRole::Value,
                rsjs_transport::TransportCallbackShape::NotCallback,
            )
        );
        assert_eq!(
            first[1].field_record_id,
            rsjs_transport::component_field_record_id(
                input_fingerprint,
                1,
                1,
                rsjs_transport::TransportComponentFieldRole::Callback,
                rsjs_transport::TransportCallbackShape::Required,
            )
        );
        assert_eq!(
            first[2].field_record_id,
            rsjs_transport::component_field_record_id(
                input_fingerprint,
                2,
                2,
                rsjs_transport::TransportComponentFieldRole::Callback,
                rsjs_transport::TransportCallbackShape::Optional,
            )
        );

        let payload = component_transport_payload_json(
            r#"{"format":"rsjs.component_descriptor","schema_version":9}"#,
            input_fingerprint,
            component_record_id(input_fingerprint),
            &[],
            &[],
            &[],
            &first,
            &[],
            &[],
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect("component field witness payload should serialize");
        let value: serde_json::Value =
            serde_json::from_str(&payload).expect("component payload should be valid JSON");
        let transport = &value["transport"];
        let claims = transport["component_field_witnesses"]
            .as_array()
            .expect("field witness claims should be an array");

        assert_eq!(value["schema_version"], 9);
        assert_eq!(
            transport["hydration_constraint"],
            serde_json::json!({ "kind": "unconstrained" })
        );
        assert_eq!(
            transport["client_value_sources"]["payload_handlers"],
            serde_json::json!([])
        );
        assert_eq!(transport["component_input_fingerprint"], input_fingerprint);
        assert_eq!(claims.len(), 3);
        assert_eq!(claims[0]["field_id"], 0);
        assert_eq!(claims[0]["field_name"], "initial");
        assert_eq!(claims[0]["crossing_hook_id"], 0);
        assert_eq!(claims[0]["role"], "value");
        assert_eq!(claims[0]["callback_shape"], "not_callback");
        assert_eq!(claims[1]["role"], "callback");
        assert_eq!(claims[1]["callback_shape"], "required");
        assert_eq!(claims[2]["callback_shape"], "optional");
        for (claim, expected) in claims.iter().zip(&first) {
            assert_eq!(
                claim["field_record_id"],
                transport_id_hex(&expected.field_record_id)
            );
        }

        let repeated = component_transport_payload_json(
            r#"{"format":"rsjs.component_descriptor","schema_version":9}"#,
            input_fingerprint,
            component_record_id(input_fingerprint),
            &[],
            &[],
            &[],
            &second,
            &[],
            &[],
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect("repeated component field payload should serialize");
        assert_eq!(payload, repeated, "payload bytes must be deterministic");
    }

    #[test]
    fn direct_struct_literals_emit_distinct_deterministic_component_edges() {
        let expand = || {
            expand_rsjs(
                quote! { component },
                quote! {
                    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EdgeParentProbe {
                        fn render(
                            self,
                            cx: &mut WebContext<L, B>,
                        ) -> Result<(), WriteError> {
                            html_sync! {
                                <section>
                                    {SameModuleEdgeProbe {}}
                                    {imported::OrdinaryEdgeProbe {}}
                                    {RenamedEdgeProbe {}}
                                    {RenamedEdgeProbe {}}
                                    {rsjs::island(reexports::PublicEdgeProbe {})}
                                </section>
                            }
                            .render(cx)
                        }
                    }
                },
            )
            .expect("direct component edges should expand")
            .to_string()
        };

        let first = expand();
        let second = expand();
        let compact = first.split_whitespace().collect::<String>();

        assert_eq!(
            first, second,
            "the same component source must emit byte-identical edge records"
        );
        assert_eq!(
            compact.matches("__rsjs_emit_component_edge!").count(),
            5,
            "every direct struct-literal site, including repeated and explicit-island sites, \
             must emit one edge:\n{first}"
        );
        for local_id in 0..5 {
            assert!(
                compact.contains("__RSJS_COMPONENT_EDGE_edgeparentprobe_")
                    && compact.contains(&format!("_{local_id},")),
                "component-local edge {local_id} should have a distinct generated record:\n{first}"
            );
        }
    }

    #[test]
    fn direct_live_fields_use_server_initials_and_noop_callbacks_without_legacy_inlining() {
        let output = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EdgeParentProbe {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        let value = rsjs::signal(String::new());
                        let on_change =
                            rsjs::handler(|next: String| value.set(next));
                        html_sync! {
                            <section>
                                {ImportedChild {
                                    value: value.get(),
                                    optional: Some(value.get()),
                                    on_change,
                                }}
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("direct live fields should lower through the canonical edge")
        .to_string();
        let compact = output.split_whitespace().collect::<String>();

        assert!(
            compact.contains("let__rsjs_initial_signal_0=String::new();"),
            "the live prop's SSR snapshot must evaluate the authored signal seed exactly once:\n{output}"
        );
        assert!(
            compact.contains("value:::core::clone::Clone::clone(&__rsjs_initial_signal_0)")
                && compact.contains(
                    "optional:Some(::core::clone::Clone::clone(&__rsjs_initial_signal_0))"
                )
                && compact.contains("on_change:::rsjs::noop_handler()"),
            "the opaque child value must receive server-safe placeholders while its live edge \
             remains in transport:\n{output}"
        );
        assert!(
            !compact.contains("value:value.get()") && !compact.contains("on_change,"),
            "compiler-only locals must not leak into the preserved server expression:\n{output}"
        );
    }

    #[test]
    fn direct_inbound_pass_through_preserves_move_semantics_without_clone() {
        let output = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B>
                    for PresentationalParent
                {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <section>
                                {ImportedChild {
                                    value: self.value,
                                }}
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("direct inbound forwarding should lower as participant-only composition")
        .to_string();
        let compact = output.split_whitespace().collect::<String>();

        assert!(
            compact.contains("let__rsjs_initial_signal_0=self.value;")
                && compact.contains("value:__rsjs_initial_signal_0"),
            "the authored inbound value must be evaluated once and moved into the child:\n{output}"
        );
        assert!(
            !compact.contains("value:::core::clone::Clone::clone(&__rsjs_initial_signal_0)"),
            "pass-through must not add a hidden `Clone` bound:\n{output}"
        );
        assert!(
            compact.contains("rustc_linked_child_field_resolves_exact_projection_role_v1")
                && compact.contains("deferred_projection"),
            "transport must retain the rustc-linked deferred projection proof:\n{output}"
        );
    }

    #[test]
    fn explicit_island_inbound_scalar_stays_server_bound_with_a_live_caller_guard() {
        let output = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B>
                    for IndependentBoundaryParent
                {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <section>
                                {rsjs::island(ImportedChild {
                                    value: self.value,
                                    on_change: rsjs::noop_handler(),
                                })}
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("a direct server scalar may seed an independent child")
        .to_string();
        let compact = output.split_whitespace().collect::<String>();

        assert!(
            compact.contains("value:self.value"),
            "the static Rust value must remain the independent child's ordinary SSR input:\n{output}"
        );
        assert!(
            !compact.contains("__rsjs_initial_signal_"),
            "an explicit owner cut must not allocate a parent live alias or implicit snapshot:\n{output}"
        );
        assert!(
            compact.contains("visible_syntax_has_no_client_dependency_v1")
                && compact.contains("forbidden_boundary_crossing"),
            "transport must carry both the static edge binding and the latent rejection for a \
             live outer caller:\n{output}"
        );
    }

    #[test]
    fn helper_edge_emits_contextual_return_type_witness() {
        let edges = [analyze::DirectHelperSourceEdge {
            local_id: 3,
            helper_path: syn::parse_quote!(ImportedAlias),
            argument_count: 2,
            expected_return_domain: Some(rsjs_ir::IntegerDomain::U32),
        }];
        let tokens = emit_helper_edge_records(
            "typed_helper",
            "sha256:typed-helper",
            &edges,
            &[rsjs_ir::abi::HelperCallEdgeId::new(3)],
            [0x31; 32],
        );
        let compact = tokens
            .to_string()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();

        assert!(compact.contains(
            "const__RSJS_CLIENT_HELPER_RETURN_WITNESS_typed_helper_3:()={let_:fn(_,_)->::core::primitive::u32=ImportedAlias;}"
        ), "expected rustc return-type witness in: {compact}");
        assert!(compact.contains("__rsjs_emit_client_helper_edge!"));
    }

    #[test]
    fn component_payload_claims_exact_ordered_transport_edges() {
        let input_fingerprint = "component-payload-edge-claims";
        let definition_record_id = component_record_id(input_fingerprint);
        let component_edges = vec![
            analyze::DirectComponentSourceEdge {
                local_id: 0,
                component_ty: syn::parse_quote!(ImportedAlias),
                fields: vec![
                    analyze::DirectComponentSourceField {
                        name: "label".to_string(),
                        binding: analyze::DirectComponentBindingClassification::ServerOnly,
                    },
                    analyze::DirectComponentSourceField {
                        name: "value".to_string(),
                        binding: analyze::DirectComponentBindingClassification::LiveValue {
                            signal: rsjs_ir::SignalId(4),
                            source: analyze::DirectLiveValueSource::ClientAlias,
                        },
                    },
                    analyze::DirectComponentSourceField {
                        name: "on_change".to_string(),
                        binding: analyze::DirectComponentBindingClassification::LiveCallback {
                            handler_prelude_local: 7,
                            form: analyze::DirectLiveCallbackForm::SomeHandler,
                        },
                    },
                    analyze::DirectComponentSourceField {
                        name: "on_close".to_string(),
                        binding: analyze::DirectComponentBindingClassification::StaticCallback {
                            form: analyze::DirectStaticCallbackForm::None,
                        },
                    },
                    analyze::DirectComponentSourceField {
                        name: "forwarded".to_string(),
                        binding: analyze::DirectComponentBindingClassification::LiveValue {
                            signal: rsjs_ir::SignalId(9),
                            source: analyze::DirectLiveValueSource::InboundField,
                        },
                    },
                ],
                has_rest: false,
                boundary: analyze::DirectComponentBoundary::Composed,
                lifecycle: analyze::DirectComponentLifecycle::StaticSingle,
                site_node: Some(rsjs_ir::NodeId(0)),
                source_anchor: rsjs_ir::SourceAnchor {
                    start_line: 12,
                    start_column: 9,
                },
            },
            analyze::DirectComponentSourceEdge {
                local_id: 1,
                component_ty: syn::parse_quote!(crate::widgets::Disclosure),
                fields: vec![
                    analyze::DirectComponentSourceField {
                        name: "children".to_string(),
                        binding: analyze::DirectComponentBindingClassification::ServerOnly,
                    },
                    analyze::DirectComponentSourceField {
                        name: "open".to_string(),
                        binding: analyze::DirectComponentBindingClassification::Unsupported,
                    },
                ],
                has_rest: true,
                boundary: analyze::DirectComponentBoundary::ExplicitIsland,
                lifecycle: analyze::DirectComponentLifecycle::Repeated,
                site_node: Some(rsjs_ir::NodeId(1)),
                source_anchor: rsjs_ir::SourceAnchor {
                    start_line: 19,
                    start_column: 13,
                },
            },
            analyze::DirectComponentSourceEdge {
                local_id: 2,
                component_ty: syn::parse_quote!(ConditionalBadge),
                fields: Vec::new(),
                has_rest: false,
                boundary: analyze::DirectComponentBoundary::Composed,
                lifecycle: analyze::DirectComponentLifecycle::ConditionalSingle(
                    analyze::DirectConditionalLifecycleScope {
                        parent: rsjs_ir::NodeId(5),
                        section_ordinal: 0,
                        branch: analyze::DirectConditionalBranch::Then,
                    },
                ),
                site_node: Some(rsjs_ir::NodeId(2)),
                source_anchor: rsjs_ir::SourceAnchor {
                    start_line: 25,
                    start_column: 17,
                },
            },
        ];
        let helper_edges = vec![
            analyze::DirectHelperSourceEdge {
                local_id: 0,
                helper_path: syn::parse_quote!(dead_server_helper),
                argument_count: 0,
                expected_return_domain: None,
            },
            analyze::DirectHelperSourceEdge {
                local_id: 1,
                helper_path: syn::parse_quote!(labels::pluralize),
                argument_count: 1,
                expected_return_domain: None,
            },
        ];
        let payload = component_transport_payload_json(
            r#"{"bindings":{"conditionals":[{"condition":0,"else":[],"parent":5,"then":[2]}]},"format":"rsjs.component_descriptor","schema_version":9}"#,
            input_fingerprint,
            definition_record_id,
            &component_edges,
            &helper_edges,
            &[rsjs_ir::abi::HelperCallEdgeId::new(1)],
            &[],
            &[],
            &[],
            &[],
            &rsjs_ir::HydrationStrategy::Visible,
            true,
        )
        .expect("component transport payload should serialize");
        let repeated_payload = component_transport_payload_json(
            r#"{"bindings":{"conditionals":[{"condition":0,"else":[],"parent":5,"then":[2]}]},"format":"rsjs.component_descriptor","schema_version":9}"#,
            input_fingerprint,
            definition_record_id,
            &component_edges,
            &helper_edges,
            &[rsjs_ir::abi::HelperCallEdgeId::new(1)],
            &[],
            &[],
            &[],
            &[],
            &rsjs_ir::HydrationStrategy::Visible,
            true,
        )
        .expect("component transport payload should serialize deterministically");
        assert_eq!(
            payload, repeated_payload,
            "live binding provenance and its semantic hash must be deterministic"
        );
        let payload: serde_json::Value =
            serde_json::from_str(&payload).expect("component payload should be valid JSON");
        let transport = &payload["transport"];
        assert_eq!(
            transport["hydration_constraint"],
            serde_json::json!({ "kind": "visible" })
        );

        assert_eq!(
            transport["definition_record_id"],
            transport_id_hex(&definition_record_id)
        );
        assert_eq!(
            transport["direct_component_edges"][0]["source_edge_id"],
            transport_id_hex(
                &component_source_edge_id(
                    input_fingerprint,
                    &component_edges[0],
                    &payload["component_descriptor"],
                )
                .unwrap()
            )
        );
        assert_eq!(
            transport["direct_component_edges"][1]["source_edge_id"],
            transport_id_hex(
                &component_source_edge_id(
                    input_fingerprint,
                    &component_edges[1],
                    &payload["component_descriptor"],
                )
                .unwrap()
            )
        );
        assert_eq!(
            transport["direct_component_edges"][2]["source_edge_id"],
            transport_id_hex(
                &component_source_edge_id(
                    input_fingerprint,
                    &component_edges[2],
                    &payload["component_descriptor"],
                )
                .unwrap()
            )
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][0]["name"],
            "label"
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][0]["binding"]["kind"],
            "server_only"
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][1]["binding"],
            serde_json::json!({
                "kind": "live_value",
                "parent_source": {
                    "id": 4,
                    "namespace": "legacy_signal",
                },
                "proof": "client_alias_resolves_to_exact_signal_read_v1",
                "source_form": "client_alias",
            })
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][2]["binding"],
            serde_json::json!({
                "callback_shape": "optional",
                "kind": "live_callback",
                "parent_source": {
                    "id": 7,
                    "namespace": "payload_handler_prelude_binding",
                },
                "proof": "some_payload_handler_prelude_binding_v1",
            })
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][3]["binding"],
            serde_json::json!({
                "callback_shape": "optional",
                "kind": "static_callback",
                "proof": "literal_none_callback_candidate_v1",
                "source_form": "none",
            })
        );
        assert_eq!(
            transport["direct_component_edges"][0]["fields"][4]["binding"],
            serde_json::json!({
                "kind": "live_value",
                "parent_source": {
                    "id": 9,
                    "namespace": "legacy_signal",
                },
                "proof": "inbound_live_field_reuses_exact_live_signal_v1",
                "source_form": "inbound_field",
            }),
            "the transport must preserve inbound pass-through provenance while reusing the \
             canonical legacy signal identity"
        );
        assert_eq!(
            transport["direct_component_edges"][0]["lifecycle"],
            serde_json::json!({ "kind": "static_single" })
        );
        assert_eq!(transport["direct_component_edges"][0]["site_node"], 0);
        assert_eq!(
            transport["direct_component_edges"][1]["boundary"],
            "explicit_island"
        );
        assert_eq!(
            transport["direct_component_edges"][1]["fields"][1]["binding"]["kind"],
            "unsupported"
        );
        assert_eq!(
            transport["direct_component_edges"][1]["lifecycle"],
            serde_json::json!({ "kind": "repeated" })
        );
        assert_eq!(transport["direct_component_edges"][1]["site_node"], 1);
        assert_eq!(transport["direct_component_edges"][1]["has_rest"], true);
        assert_eq!(
            transport["direct_component_edges"][2]["lifecycle"],
            serde_json::json!({
                "kind": "conditional_single",
                "control": 0,
                "predicate_expression": 0,
                "branch": "then",
            })
        );
        assert_eq!(transport["direct_component_edges"][2]["site_node"], 2);

        let helper_claims = transport["reachable_helper_edges"]
            .as_array()
            .expect("helper claims should be an array");
        assert_eq!(
            helper_claims.len(),
            1,
            "unreachable helper edges must not be claimed by the component payload"
        );
        assert_eq!(helper_claims[0]["local_ordinal"], 1);
        assert_eq!(
            helper_claims[0]["source_edge_id"],
            transport_id_hex(&helper_source_edge_id(input_fingerprint, 1))
        );
    }

    #[test]
    fn component_payload_materializes_only_referenced_payload_handler_values() {
        let input_fingerprint = "direct-payload-handler-value";
        let helper_edges = [analyze::DirectHelperSourceEdge {
            local_id: 2,
            helper_path: syn::parse_quote!(labels::normalize),
            argument_count: 1,
            expected_return_domain: None,
        }];
        let value = analyze::DirectPayloadHandlerValue {
            prelude_local_index: 7,
            payload_arity: 1,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 14,
                start_column: 9,
            },
            dependencies: analyze::DirectPayloadHandlerDependencies {
                signal_reads: vec![rsjs_ir::SignalId(1)],
                signal_writes: vec![rsjs_ir::SignalId(0)],
                queries: vec![rsjs_ir::QueryId(3)],
                mutations: vec![rsjs_ir::MutationId(4)],
                node_refs: vec![rsjs_ir::NodeRefId(5)],
                callback_slots: vec![rsjs_ir::CallbackSlotId(6)],
                helper_edges: vec![rsjs_ir::abi::HelperCallEdgeId::new(2)],
                client_module_calls: vec![analyze::DirectClientModuleCall {
                    module: "legacy::labels".to_string(),
                    name: "format".to_string(),
                }],
            },
            body_ir: rsjs_ir::ClientBlock::from_stmts(vec![rsjs_ir::ClientStmt::SignalSet {
                id: rsjs_ir::SignalId(0),
                value: rsjs_ir::ClientExpr::CallbackPayload,
            }]),
        };
        let serialize = || {
            component_transport_payload_json(
                r#"{"format":"rsjs.component_descriptor","schema_version":9}"#,
                input_fingerprint,
                component_record_id(input_fingerprint),
                &[],
                &helper_edges,
                &[rsjs_ir::abi::HelperCallEdgeId::new(2)],
                &[],
                &[],
                &[],
                std::slice::from_ref(&value),
                &rsjs_ir::HydrationStrategy::Interaction {
                    event: Some("pointerdown".to_string()),
                },
                true,
            )
            .expect("payload handler value should serialize")
        };
        let first = serialize();
        assert_eq!(first, serialize(), "transport bytes must be deterministic");
        let payload: serde_json::Value = serde_json::from_str(&first).unwrap();
        let handler = &payload["transport"]["client_value_sources"]["payload_handlers"][0];
        assert_eq!(payload["schema_version"], 9);
        assert_eq!(handler["kind"], "payload_handler");
        assert_eq!(handler["prelude_local_index"], 7);
        assert_eq!(handler["payload_arity"], 1);
        assert_eq!(
            handler["source_anchor"],
            serde_json::json!({ "column": 9, "line": 14 })
        );
        assert_eq!(
            handler["dependencies"]["signal_reads"],
            serde_json::json!([1])
        );
        assert_eq!(
            handler["dependencies"]["signal_writes"],
            serde_json::json!([0])
        );
        assert_eq!(
            handler["dependencies"]["helper_edges"],
            serde_json::json!([2])
        );
        assert_eq!(
            handler["body_ir"],
            serde_json::json!({
                "kind": "block",
                "stmts": [{
                    "kind": "signal_set",
                    "signal": 0,
                    "value": { "kind": "callback_payload" },
                }],
            })
        );
        assert_eq!(
            payload["transport"]["hydration_constraint"],
            serde_json::json!({
                "event": "pointerdown",
                "kind": "interaction",
            })
        );
    }

    #[test]
    fn linked_payload_handler_exports_browser_capabilities_without_extra_transport() {
        use rsjs_ir::{
            ClientBlock, ClientBrowserEffect, ClientBrowserRead, ClientExpr, ClientStmt,
            LocationField,
        };

        let mut builder = rsjs_ir::builder::IslandBuilder::new();
        let root = builder.element("section", None);
        builder.set_root(root);
        let island = builder.build();
        let payload_handler = analyze::DirectPayloadHandlerValue {
            prelude_local_index: 7,
            payload_arity: 0,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 14,
                start_column: 9,
            },
            dependencies: analyze::DirectPayloadHandlerDependencies::default(),
            body_ir: ClientBlock::from_stmts(vec![
                ClientStmt::Expr(ClientExpr::BrowserRead(ClientBrowserRead::Location(
                    LocationField::Pathname,
                ))),
                ClientStmt::BrowserEffect(ClientBrowserEffect::WindowPrint),
            ]),
        };

        let exports =
            linked_payload_handler_exports(&island, std::slice::from_ref(&payload_handler))
                .expect("browser-only payload callbacks need no additional owner transport");
        assert_eq!(exports.len(), 1);
        let fragment = rsjs_ir::emit_js::emit_fragment_with_linked_exports(
            &island,
            "browser_callback",
            &[],
            &exports,
            &[],
        );
        assert!(fragment.contains("options.expose.payloadHandlers[7] = (__payload) => {"));
        assert!(fragment.contains("window.location.pathname"));
        assert!(fragment.contains("window.print?.();"));
    }

    #[test]
    fn hydration_constraint_transport_distinguishes_absent_and_explicit_strategies() {
        assert_eq!(
            hydration_constraint_json(&rsjs_ir::HydrationStrategy::Immediate, false),
            serde_json::json!({ "kind": "unconstrained" })
        );
        assert_eq!(
            hydration_constraint_json(&rsjs_ir::HydrationStrategy::Immediate, true),
            serde_json::json!({ "kind": "eager" })
        );
        assert_eq!(
            hydration_constraint_json(&rsjs_ir::HydrationStrategy::Visible, true),
            serde_json::json!({ "kind": "visible" })
        );
        assert_eq!(
            hydration_constraint_json(&rsjs_ir::HydrationStrategy::Idle, true),
            serde_json::json!({ "kind": "idle" })
        );
        assert_eq!(
            hydration_constraint_json(
                &rsjs_ir::HydrationStrategy::Interaction { event: None },
                true,
            ),
            serde_json::json!({ "event": null, "kind": "interaction" })
        );
        assert_eq!(
            hydration_constraint_json(
                &rsjs_ir::HydrationStrategy::Media {
                    query: "(min-width: 60rem)".to_string(),
                },
                true,
            ),
            serde_json::json!({
                "kind": "media",
                "query": "(min-width: 60rem)",
            })
        );
    }

    #[test]
    fn reachable_helpers_emit_rust_resolved_deterministic_edges_without_clients_allowlist() {
        let expand = || {
            expand_rsjs(
                quote! { component, client },
                quote! {
                    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HelperEdgeProbe {
                        fn render(
                            self,
                            cx: &mut WebContext<L, B>,
                        ) -> Result<(), WriteError> {
                            let dead_server_value = same_module(1_i64);
                            let count = rsjs::signal(0_i64);
                            html_sync! {
                                <section
                                    data-dead={dead_server_value}
                                    data-static-call={server_attr_value()}
                                >
                                    <span>{same_module(count.get())}</span>
                                    <span>{ImportedAlias(count.get())}</span>
                                    <span>{renamed::HelperAlias(count.get())}</span>
                                    <span>{reexports::PublicHelper(count.get())}</span>
                                    <span>{same_module(count.get())}</span>
                                    <aside>{server_child()}</aside>
                                </section>
                            }
                            .render(cx)
                        }
                    }
                },
            )
            .expect("standalone helper edges should expand without clients(...)")
            .to_string()
        };

        let first = expand();
        let second = expand();
        let compact = first.split_whitespace().collect::<String>();

        assert_eq!(
            first, second,
            "the same helper calls must emit byte-identical source edges"
        );
        assert_eq!(
            compact.matches("__rsjs_emit_client_helper_edge!").count(),
            5,
            "only the five browser-reachable calls should emit records; the static prelude \
             and template helper calls must remain ordinary server Rust:\n{first}"
        );
        assert!(
            compact.contains("server_attr_value()") && compact.contains("server_child()"),
            "dependency-free server calls should survive inside a hydrated component:\n{first}"
        );
        for authored_path in [
            "same_module",
            "ImportedAlias",
            "renamed::HelperAlias",
            "reexports::PublicHelper",
        ] {
            assert!(
                compact.contains(authored_path),
                "the generated relocation should preserve authored path `{authored_path}`:\n{first}"
            );
        }
        assert!(
            compact.matches("same_module").count() >= 3,
            "the ordinary server call plus both live repeated calls should survive:\n{first}"
        );

        let fingerprint = strategy_input_fingerprint(
            &quote! { component, client },
            &quote! { impl WebRenderSync for HelperEdgeProbe {} },
        );
        assert_eq!(
            helper_source_edge_id(&fingerprint, 4),
            helper_source_edge_id(&fingerprint, 4),
            "one helper call site must hash deterministically"
        );
        assert_ne!(
            helper_source_edge_id(&fingerprint, 4),
            helper_source_edge_id(&fingerprint, 5),
            "repeated calls must retain distinct source edge ids"
        );
    }

    #[test]
    fn dependency_free_free_calls_remain_opaque_server_rendering() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticCallProbe {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <section data-label={server_attr_value()}>
                                {server_child()}
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("ordinary dependency-free server calls should remain valid")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            !compact.contains("__rsjs_emit_client_helper_edge!")
                && !compact.contains("ISLAND_REGISTRY")
                && !compact.contains("rsjs_island_used"),
            "dependency-free free calls must not create browser roots or helper edges:\n{out}"
        );
        assert!(
            compact.contains("server_attr_value()") && compact.contains("server_child()"),
            "the authored server calls must survive losslessly:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_server_only_skips_runtime_registration() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticPanel {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        html_sync! {
                            <section class="panel">
                                <h2>"Overview"</h2>
                                <p>"Fully server rendered"</p>
                            </section>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("server-only rsjs impl should expand")
        .to_string();

        assert!(
            !out.contains("ISLAND_REGISTRY")
                && !out.contains("COMPONENT_FRAGMENT_REGISTRY")
                && !out.contains("rsjs_island_used")
                && !out.contains("data - rsjs - island"),
            "server-only rsjs output should not register runtime assets or emit island scaffolding:\n{out}"
        );
    }

    #[test]
    fn shared_state_acquisition_transport_is_relocation_backed_and_deadness_neutral() {
        let attr = quote! { component, client };
        let item = quote! {
            impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticSharedProbe {
                fn render(
                    self,
                    cx: &mut WebContext<L, B>,
                ) -> Result<(), WriteError> {
                    let _workbench = rsjs::shared::<StateAlias>();
                    html_sync! {
                        <section data-static-shared-probe>"Server only"</section>
                    }
                    .render(cx)
                }
            }
        };
        let input_fingerprint = strategy_input_fingerprint(&attr, &item);
        let out = expand_rsjs(attr, item)
            .expect("an unused aliased SharedState acquisition should still emit transport")
            .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert_eq!(
            compact
                .matches("static__RSJS_SHARED_STATE_ACQUISITION_")
                .count(),
            1,
            "every descriptor SharedStoreId must emit one fixed acquisition record:\n{out}"
        );
        assert!(
            compact.contains("<StateAliasas::rsjs::__private::SharedStateAbi>::WITNESS")
                && compact.contains("__DATA,__rsjs_sedge")
                && compact.contains(".rsjs.shared_acquisition")
                && compact.contains(".rsjs$K"),
            "the record must relocate through rustc's resolved state alias in every supported \
             object format:\n{out}"
        );
        assert!(
            !compact.contains("ISLAND_REGISTRY")
                && !compact.contains("COMPONENT_FRAGMENT_REGISTRY")
                && !compact.contains("rsjs_island_used")
                && !compact.contains("data-rsjs-island"),
            "transport completeness must not make an unused acquisition activate an owner:\n{out}"
        );

        let initializers = [emit_rust::SharedStateInitializer {
            id: rsjs_ir::SharedStoreId(0),
            source_name: syn::parse_quote!(_workbench),
            ty: syn::parse_quote!(StateAlias),
            expected_type_key: None,
        }];
        let payload = component_transport_payload_json(
            r#"{"format":"rsjs.component_descriptor","schema_version":9}"#,
            &input_fingerprint,
            component_record_id(&input_fingerprint),
            &[],
            &[],
            &[],
            &[],
            &initializers,
            &[],
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect("SharedState ownership claims should serialize");
        let payload: serde_json::Value =
            serde_json::from_str(&payload).expect("component payload should be valid JSON");
        assert_eq!(payload["schema_version"], 9);
        assert_eq!(
            payload["transport"]["shared_state_acquisitions"],
            serde_json::json!([{
                "source_edge_id": transport_id_hex(&shared_state_source_edge_id(
                    &input_fingerprint,
                    rsjs_ir::SharedStoreId(0),
                )),
                "store_id": 0,
            }])
        );
    }

    #[test]
    fn shared_state_provider_transport_authenticates_schema_and_child_topology() {
        let input_fingerprint = "shared-state-provider-topology";
        let child_edge = analyze::DirectComponentSourceEdge {
            local_id: 0,
            component_ty: syn::parse_quote!(ImportedWorkbenchChild),
            fields: vec![analyze::DirectComponentSourceField {
                name: "label".to_string(),
                binding: analyze::DirectComponentBindingClassification::ServerOnly,
            }],
            has_rest: false,
            boundary: analyze::DirectComponentBoundary::Composed,
            lifecycle: analyze::DirectComponentLifecycle::StaticSingle,
            site_node: Some(rsjs_ir::NodeId(4)),
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 12,
                start_column: 9,
            },
        };
        let provider_site = analyze::DirectSharedStateProviderSite {
            id: rsjs_ir::abi::ProviderSiteId::new(0),
            state_ty: syn::parse_quote!(StateAlias),
            child_edge: 0,
            site_node: rsjs_ir::NodeId(4),
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 10,
                start_column: 5,
            },
        };
        let descriptor = r#"{
            "format":"rsjs.component_descriptor",
            "schema_version":9,
            "shared_state_provider_sites":[{
                "id":0,
                "child_edge":0,
                "site_node":4,
                "state_rust_type":"StateAlias",
                "topology":"static_single_wrapperless",
                "source_anchor":{"start_line":10,"start_column":5}
            }]
        }"#;
        let payload = component_transport_payload_json(
            descriptor,
            input_fingerprint,
            component_record_id(input_fingerprint),
            std::slice::from_ref(&child_edge),
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&provider_site),
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect("static provider topology should serialize");
        let repeated = component_transport_payload_json(
            descriptor,
            input_fingerprint,
            component_record_id(input_fingerprint),
            std::slice::from_ref(&child_edge),
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&provider_site),
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect("provider topology should serialize deterministically");
        assert_eq!(payload, repeated);

        let payload: serde_json::Value =
            serde_json::from_str(&payload).expect("component payload should be valid JSON");
        let claim = &payload["transport"]["shared_state_provider_sites"][0];
        let child_source_edge_id = component_source_edge_id(
            input_fingerprint,
            &child_edge,
            &payload["component_descriptor"],
        )
        .unwrap();
        assert_eq!(payload["schema_version"], 9);
        assert_eq!(claim["provider_site_id"], 0);
        assert_eq!(claim["child_edge_local_ordinal"], 0);
        assert_eq!(
            claim["child_source_edge_id"],
            transport_id_hex(&child_source_edge_id)
        );
        assert_eq!(claim["site_node"], 4);
        assert_eq!(claim["state_rust_type"], "StateAlias");
        assert_eq!(
            claim["source_edge_id"],
            transport_id_hex(&shared_state_provider_source_edge_id(
                input_fingerprint,
                &provider_site,
                child_source_edge_id,
            ))
        );

        let records = emit_shared_state_provider_records(
            "provider_fixture",
            input_fingerprint,
            [0x44; 32],
            descriptor,
            std::slice::from_ref(&child_edge),
            std::slice::from_ref(&provider_site),
        )
        .expect("the provider schema relocation should emit")
        .to_string();
        let compact = records.split_whitespace().collect::<String>();
        assert!(
            compact.contains("__RSJS_SHARED_STATE_PROVIDER_provider_fixture_0")
                && compact.contains("<StateAliasas::rsjs::__private::SharedStateAbi>::WITNESS")
                && compact.contains("SharedStateAcquisitionRecord")
                && compact.contains("__DATA,__rsjs_sedge")
                && compact.contains(".rsjs.shared_acquisition")
                && compact.contains(".rsjs$K"),
            "provider schema identity must come from a rustc relocation in every object format:\n{records}"
        );

        let mut mismatched_site = provider_site.clone();
        mismatched_site.site_node = rsjs_ir::NodeId(5);
        let error = component_transport_payload_json(
            descriptor,
            input_fingerprint,
            component_record_id(input_fingerprint),
            std::slice::from_ref(&child_edge),
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&mismatched_site),
            &[],
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
        )
        .expect_err("a provider claim cannot move away from its child server node");
        assert!(
            error
                .to_string()
                .contains("must own one unique static composed child edge"),
            "unexpected provider topology diagnostic: {error}"
        );
    }

    #[test]
    fn shared_state_provider_expansion_keeps_wrapperless_ssr_and_emits_both_edges() {
        let output = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for WorkbenchPage {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> Result<(), WriteError> {
                        html_sync! {
                            <main>
                                {rsjs::SharedStateProvider {
                                    state: StateAlias {
                                        mode: "select".to_owned(),
                                    },
                                    children: ImportedWorkbenchChild {
                                        label: "static",
                                    },
                                }}
                            </main>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("a static provider with one direct child should expand")
        .to_string();
        let compact = output.split_whitespace().collect::<String>();

        assert_eq!(
            compact.matches("__rsjs_emit_component_edge!").count(),
            1,
            "the provider child must remain one normal rustc-resolved component edge:\n{output}"
        );
        assert_eq!(
            compact
                .matches("static__RSJS_SHARED_STATE_PROVIDER_")
                .count(),
            1,
            "the provider must emit one distinct schema witness relocation:\n{output}"
        );
        assert!(
            compact.contains("rsjs::SharedStateProvider")
                && compact.contains("children:ImportedWorkbenchChild")
                && compact.contains("<StateAliasas::rsjs::__private::SharedStateAbi>::WITNESS"),
            "the authored provider expression and Rust-resolved state type must survive:\n{output}"
        );
        assert!(
            !compact.contains("ISLAND_REGISTRY")
                && !compact.contains("COMPONENT_FRAGMENT_REGISTRY")
                && !compact.contains("rsjs_island_used"),
            "provider topology alone must not create an owner or browser origin:\n{output}"
        );
    }

    #[test]
    fn participant_component_rejects_a_local_client_origin() {
        let error = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let count = rsjs::signal(0_i64);
                        html_sync! {
                            <output>{count.get()}</output>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect_err("a participant-only component must reject local writable state");

        let message = error.to_string();
        assert!(
            message.contains("writable local signal")
                && message.contains("#[rsjs(component, client)]"),
            "origin diagnostic should name the capability and the fix: {message}"
        );
    }

    #[test]
    fn participant_component_rejects_a_browser_capability_read() {
        let error = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LocaleLabel {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        html_sync! {
                            <output>{rsjs::navigator::language()}</output>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect_err("a participant-only component must reject browser reads");

        assert!(
            error.to_string().contains("browser capability read"),
            "browser-origin diagnostic should identify the read: {error}"
        );
    }

    #[test]
    fn latent_inbound_seed_alone_does_not_create_a_browser_owner() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SeedOnlyCard {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let local = rsjs::signal(self.initial);
                        html_sync! {
                            <article>"Static server output"</article>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("a latent inbound seed with no reachable client use should expand")
        .to_string();
        let compact = out.split_whitespace().collect::<String>();

        assert!(
            compact.contains("ComponentOriginPermission::OriginCapable"),
            "the authoring permission should remain recorded:\n{out}"
        );
        assert!(
            !compact.contains("rsjs_island_used")
                && !compact.contains("register_island")
                && !compact.contains("register_component_fragment"),
            "latent inbound metadata must not become a browser-obligation root or emit JS registration:\n{out}"
        );
        assert!(
            !compact.contains("__RSJS_COMPONENT_FIELD_"),
            "a dead seed-only input must downgrade to an ordinary server prop instead of emitting a client field witness:\n{out}"
        );
    }

    #[test]
    fn manifest_registration_uses_filtered_imports_and_runtime_features() {
        let struct_path: syn::Path = syn::parse_quote!(MoneyLabel);
        let imports = vec!["crate::money".to_string()];
        let fragment_imports = vec!["child_fragment".to_string()];
        let tokens = emit_manifest_registration(
            "moneylabel_1234",
            "MoneyLabel",
            "export function hydrate() {}",
            false,
            &rsjs_ir::HydrationStrategy::Immediate,
            false,
            false,
            &struct_path,
            &imports,
            &fragment_imports,
            "export function mount() {}",
            "__rsjs_mount_moneylabel_1234",
        )
        .to_string();
        let compact = tokens.split_whitespace().collect::<String>();

        assert!(
            compact.contains("client_module_imports:&[\"crate::money\"]")
                && !compact.contains("crate::dates"),
            "manifest should contain only reachable client-module imports:\n{tokens}"
        );
        assert!(
            compact.contains("component_fragment_imports:&[\"child_fragment\"]")
                && !compact.contains("server_only_child"),
            "manifest should contain only reachable component-fragment imports:\n{tokens}"
        );
        assert!(
            compact.contains("uses_queries:false") && compact.contains("uses_mutations:false"),
            "manifest should use reachable query/mutation flags:\n{tokens}"
        );
    }

    #[test]
    fn final_component_fragment_registration_does_not_create_a_legacy_island() {
        let tokens = emit_component_fragment_registration(
            "statusbadge_1234",
            "StatusBadge",
            "export function mountStatusBadge() {}",
            "mountStatusBadge",
        )
        .to_string();
        let compact = tokens.split_whitespace().collect::<String>();

        assert!(
            compact.contains("::rsjs::ComponentFragmentManifestEntry")
                && compact.contains("::rsjs::COMPONENT_FRAGMENT_REGISTRY"),
            "a final-phase component definition should register its reusable fragment:\n{tokens}"
        );
        assert!(
            compact.contains("asset_id:\"statusbadge_1234\"")
                && compact.contains("struct_name:\"StatusBadge\"")
                && compact.contains("export_name:\"mountStatusBadge\"")
                && compact.contains("js_source:\"exportfunctionmountStatusBadge(){}\""),
            "the fragment registration should retain the exact fragment identity and source:\n{tokens}"
        );
        assert!(
            !compact.contains("::rsjs::IslandManifestEntry")
                && !compact.contains("::rsjs::ISLAND_REGISTRY")
                && !compact.contains("__RSJS_ISLAND_SUBMIT"),
            "a fragment-only final-phase registration must not submit the legacy standalone island:\n{tokens}"
        );
    }

    #[test]
    fn linked_owner_registration_uses_owner_identity_fragments_and_strategy() {
        let plan = rsjs_ir::LinkedOwnerModulePlan {
            owner_asset_id: "linked_owner_12_9000".to_owned(),
            root_candidate_id: "12".to_owned(),
            strategy: rsjs_ir::link::OwnerActivationStrategy::Interaction {
                event: Some("pointerdown".to_owned()),
            },
            uses_queries: true,
            uses_mutations: true,
            mounts: vec![
                rsjs_ir::LinkedOwnerMount {
                    candidate_id: "12".to_owned(),
                    parent_candidate_id: None,
                    fragment_asset_id: "zeta_fragment".to_owned(),
                    fragment_export: "mount_zeta".to_owned(),
                    seed_key: "linked_owner_12_9000".to_owned(),
                    lifecycle_root_candidate_id: None,
                },
                rsjs_ir::LinkedOwnerMount {
                    candidate_id: "13".to_owned(),
                    parent_candidate_id: Some("12".to_owned()),
                    fragment_asset_id: "alpha_fragment".to_owned(),
                    fragment_export: "mount_alpha".to_owned(),
                    seed_key: "member_seed_13".to_owned(),
                    lifecycle_root_candidate_id: None,
                },
                rsjs_ir::LinkedOwnerMount {
                    candidate_id: "14".to_owned(),
                    parent_candidate_id: Some("13".to_owned()),
                    fragment_asset_id: "zeta_fragment".to_owned(),
                    fragment_export: "mount_zeta".to_owned(),
                    seed_key: "member_seed_14".to_owned(),
                    lifecycle_root_candidate_id: None,
                },
            ],
            signal_aliases: Vec::new(),
            signal_seeds: Vec::new(),
            callback_edges: Vec::new(),
            lifecycles: Vec::new(),
        };
        let tokens =
            emit_linked_owner_manifest_registration(&plan, "export function activateOwner() {}")
                .to_string();
        let compact = tokens.split_whitespace().collect::<String>();

        assert!(
            compact.contains("::rsjs::IslandManifestEntry")
                && compact.contains("::rsjs::ISLAND_REGISTRY")
                && !compact.contains("::rsjs::COMPONENT_FRAGMENT_REGISTRY"),
            "a linked owner should submit exactly one browser owner entry:\n{tokens}"
        );
        assert!(
            compact.contains("asset_id:\"linked_owner_12_9000\"")
                && compact.contains("struct_name:\"LinkedOwner12\"")
                && compact.contains("js_source:\"exportfunctionactivateOwner(){}\"")
                && compact.contains("is_async:false"),
            "the owner manifest should use the owner asset rather than a member fragment identity:\n{tokens}"
        );
        assert!(
            compact.contains(
                "hydration_strategy:::rsjs::IslandHydrationStrategy::Interaction{event:::core::option::Option::Some(\"pointerdown\"),}"
            ),
            "the linked activation strategy and interaction event should survive registration:\n{tokens}"
        );
        assert!(
            compact.contains("client_module_imports:&[]")
                && compact.contains(
                    "component_fragment_imports:&[\"alpha_fragment\",\"zeta_fragment\"]"
                )
                && compact.contains("uses_queries:true")
                && compact.contains("uses_mutations:true"),
            "the manifest should carry sorted, deduplicated fragment imports and explicit runtime feature fields:\n{tokens}"
        );
        assert!(
            !compact.contains("member_seed_13") && !compact.contains("member_seed_14"),
            "member seed keys belong to SSR wrappers, not the owner manifest; the root owner asset is the manifest identity:\n{tokens}"
        );
    }

    #[test]
    fn strategy_sidecar_json_records_runtime_absence_for_server_only() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::ServerOnly,
            roots: Vec::new(),
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let json = strategy_asset_json_with_origin_permission(
            "staticpanel_1234",
            "StaticPanel",
            "StaticPanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            false,
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            false,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["format"], "rsjs.strategy");
        assert_eq!(value["strategy_version"], 1);
        assert_eq!(value["render_strategy"], "server_only");
        assert_eq!(
            value["client_origin_permission"], "participant_only",
            "origin permission must survive into linker/tooling sidecars"
        );
        assert_eq!(value["runtime_asset_id"], serde_json::Value::Null);
        assert_eq!(value["input_fingerprint"], "sha256:test");
        assert_eq!(value["source"]["impl_start_line"], 10);
        assert_eq!(value["source"]["render_start_column"], 5);
        assert_eq!(value["register_island"], false);
        assert_eq!(value["hydration_strategy"]["kind"], "immediate");
        assert_eq!(value["hydration_strategy"]["label"], "immediate");
        assert_eq!(value["client_modules"].as_array().unwrap().len(), 0);
        assert_eq!(value["runtime_features"].as_array().unwrap().len(), 0);
        assert_eq!(value["emitted"]["js_asset"], false);
        assert_eq!(value["roots"].as_array().unwrap().len(), 0);
        assert_eq!(value["crossings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_server_surfaces() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let root = builder.element("div", None);
        builder.server_attr(root, "aria-label", "rsjs::server_attr(label)");
        builder.server_only_node(root, "label");
        builder.server_attr_with_source_anchor(
            root,
            "data-label",
            "rsjs::server_attr(label)",
            rsjs_ir::ServerSurfaceSource::ExplicitSimpleSsrBoundary,
            Some(rsjs_ir::ServerSurfaceAnchor {
                start_line: 12,
                start_column: 21,
            }),
        );
        builder.server_attr_with_source(
            root,
            "data-id",
            "rsjs::server_attr(self.id)",
            rsjs_ir::ServerSurfaceSource::InferredSimpleSsr,
        );
        builder.server_only_node_with_source_anchor(
            root,
            "self.title",
            rsjs_ir::ServerSurfaceSource::InferredSimpleSsr,
            Some(rsjs_ir::ServerSurfaceAnchor {
                start_line: 14,
                start_column: 17,
            }),
        );
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::ServerOnly,
            roots: Vec::new(),
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };

        let json = strategy_asset_json(
            "staticpanel_1234",
            "StaticPanel",
            "StaticPanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            false,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let surfaces = value["server_surfaces"]
            .as_array()
            .expect("server_surfaces should be an array");

        assert_eq!(surfaces.len(), 5);
        assert_eq!(surfaces[0]["kind"], "server_attr");
        assert_eq!(surfaces[0]["attr"], "aria-label");
        assert_eq!(surfaces[0]["source"], "explicit_boundary");
        assert_eq!(surfaces[0]["reason"], "explicit SSR boundary");
        assert!(surfaces[0].get("tokens").is_none());
        assert_eq!(surfaces[1]["kind"], "server_attr");
        assert_eq!(surfaces[1]["attr"], "data-label");
        assert_eq!(surfaces[1]["source"], "explicit_simple_ssr_boundary");
        assert_eq!(
            surfaces[1]["reason"],
            "explicit wrapper around simple SSR projection"
        );
        assert_eq!(surfaces[1]["source_anchor"]["start_line"], 12);
        assert_eq!(surfaces[1]["source_anchor"]["start_column"], 21);
        assert!(surfaces[1].get("tokens").is_none());
        assert_eq!(surfaces[2]["kind"], "server_attr");
        assert_eq!(surfaces[2]["attr"], "data-id");
        assert_eq!(surfaces[2]["source"], "inferred_simple_ssr");
        assert_eq!(surfaces[2]["reason"], "inferred simple SSR projection");
        assert!(surfaces[2].get("tokens").is_none());
        assert_eq!(surfaces[3]["kind"], "server_slot");
        assert_eq!(surfaces[3]["source"], "explicit_boundary");
        assert_eq!(surfaces[3]["reason"], "explicit SSR boundary");
        assert!(surfaces[3].get("tokens").is_none());
        assert_eq!(surfaces[4]["kind"], "server_slot");
        assert_eq!(surfaces[4]["source"], "inferred_simple_ssr");
        assert_eq!(surfaces[4]["source_anchor"]["start_line"], 14);
        assert_eq!(surfaces[4]["source_anchor"]["start_column"], 17);
        assert_eq!(surfaces[4]["reason"], "inferred simple SSR projection");
        assert!(surfaces[4].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_reachable_runtime_features() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::SsrHydrate,
            roots: vec![
                rsjs_ir::ClientReachabilityRoot::Query {
                    id: rsjs_ir::QueryId(0),
                },
                rsjs_ir::ClientReachabilityRoot::Mutation {
                    id: rsjs_ir::MutationId(1),
                },
            ],
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let json = strategy_asset_json(
            "loader_1234",
            "LoaderPanel",
            "LoaderPanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            true,
            true,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["runtime_features"][0], "query");
        assert_eq!(value["runtime_features"][1], "mutation");
    }

    #[test]
    fn strategy_sidecar_json_records_shared_state_dependencies() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let store = builder.shared_store(
            "workbench".to_string(),
            "crate::StrategyAuditProbeState".to_string(),
            Some(rsjs_ir::SourceAnchor {
                start_line: 12,
                start_column: 17,
            }),
        );
        let button_bg =
            builder.shared_field(store, "button_bg".to_string(), rsjs_ir::ClientType::String);
        let island = builder.build();
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::SsrHydrate,
            roots: vec![rsjs_ir::ClientReachabilityRoot::Signal { id: button_bg }],
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };

        let json = strategy_asset_json(
            "theme_1234",
            "ThemePanel",
            "ThemePanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["runtime_features"][0], "shared_state");
        assert_eq!(value["emitted"]["uses_shared_state"], true);
        assert_eq!(
            value["shared_state_dependencies"][0]["rust_type"],
            "crate::StrategyAuditProbeState"
        );
        assert_eq!(
            value["shared_state_dependencies"][0]["fields"][0],
            "button_bg"
        );
        // The macro cannot resolve a Rust type, so the sidecar reports the
        // store's Rust type as written and the fields this island reads, and
        // leaves the provider-side identity to the runtime. See
        // `Analyzer::shared_field_client_type`.
        assert!(value["shared_state_dependencies"][0]["type_key"].is_null());
        assert_eq!(
            value["shared_state_dependencies"][0]["serialized_fields"],
            serde_json::json!([])
        );
        assert_eq!(
            value["shared_state_dependencies"][0]["provider_serializes_full_snapshot"],
            true
        );
        assert_eq!(
            value["shared_state_dependencies"][0]["source"]["start_line"],
            12
        );
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_dead_primitives() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let signal = builder.signal(rsjs_ir::Value::Int(0));
        builder.set_signal_source(
            signal,
            Some("draft_count".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 12,
                start_column: 21,
            }),
        );
        let memo_body = builder.expr(rsjs_ir::ClientExpr::signal_read(signal));
        let memo = builder.signal(rsjs_ir::Value::Null);
        builder.set_signal_source(
            memo,
            Some("total".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 13,
                start_column: 21,
            }),
        );
        builder.set_signal_derived(memo, memo_body);
        let handler = builder.handler_from_ir(rsjs_ir::ClientBlock::from_stmts(Vec::new()));
        builder.set_handler_source(
            handler,
            Some("on_save".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 14,
                start_column: 21,
            }),
        );
        let node_ref = builder.node_ref();
        builder.set_node_ref_source(
            node_ref,
            Some("input_ref".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 15,
                start_column: 21,
            }),
        );
        let key = builder.expr(rsjs_ir::ClientExpr::string("items"));
        let fetcher = builder.expr(rsjs_ir::ClientExpr::string("/api/items"));
        let query = builder.query(key, key, fetcher, rsjs_ir::QueryOptions::default());
        builder.set_query_source(
            query,
            Some("items".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 16,
                start_column: 25,
            }),
        );
        let root = builder.element("section", None);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);
        let json = strategy_asset_json(
            "dead_1234",
            "DeadPanel",
            "DeadPanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            false,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let dead = value["dead_primitives"]
            .as_array()
            .expect("dead_primitives should be an array");
        assert_eq!(dead.len(), 5);
        assert_eq!(dead[0]["kind"], "signal");
        assert_eq!(dead[0]["signal"], signal.0);
        assert_eq!(dead[0]["label"], "draft_count");
        assert_eq!(dead[0]["source_anchor"]["start_line"], 12);
        assert_eq!(dead[0]["source_anchor"]["start_column"], 21);
        assert_eq!(dead[1]["kind"], "memo");
        assert_eq!(dead[1]["signal"], memo.0);
        assert_eq!(dead[1]["label"], "total");
        assert_eq!(dead[1]["source_anchor"]["start_line"], 13);
        assert_eq!(dead[2]["kind"], "handler");
        assert_eq!(dead[2]["handler"], handler.0);
        assert_eq!(dead[2]["label"], "on_save");
        assert_eq!(dead[3]["kind"], "node_ref");
        assert_eq!(dead[3]["node_ref"], node_ref.0);
        assert_eq!(dead[3]["label"], "input_ref");
        assert_eq!(dead[4]["kind"], "query");
        assert_eq!(dead[4]["query"], query.0);
        assert_eq!(dead[4]["label"], "items");
        assert!(dead[0].get("value").is_none());
        assert!(dead[0].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_signal_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let count = builder.signal(rsjs_ir::Value::Int(0));
        builder.set_signal_source(
            count,
            Some("count".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 12,
                start_column: 21,
            }),
        );
        let count_read = builder.expr(rsjs_ir::ClientExpr::signal_read(count));
        let root = builder.element("div", None);
        let span = builder.element("span", Some(root));
        builder.bind_text(span, count_read);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "counter_1234",
            "Counter",
            "Counter",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0]["kind"], "signal_initial_value");
        assert_eq!(crossings[0]["surface"], "signal_seed");
        assert_eq!(crossings[0]["requirement"], "client_encode");
        assert_eq!(crossings[0]["signal"], 0);
        assert_eq!(crossings[0]["label"], "count");
        assert_eq!(crossings[0]["source_anchor"]["start_line"], 12);
        assert_eq!(crossings[0]["source_anchor"]["start_column"], 21);
        assert!(crossings[0].get("value").is_none());
        assert!(crossings[0].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_handler_payload_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let root = builder.element("div", None);
        let button = builder.element("button", Some(root));
        builder.payload_attr_with_source(
            button,
            "data-rsjs-payload-p0",
            "day",
            Some("day".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 15,
                start_column: 42,
            }),
        );
        let handler = builder.handler_from_ir(rsjs_ir::ClientBlock::from_stmts(vec![
            rsjs_ir::ClientStmt::Expr(rsjs_ir::ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            }),
        ]));
        builder.bind_event(button, rsjs_ir::EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "calendar_1234",
            "Calendar",
            "Calendar",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0]["kind"], "handler_payload");
        assert_eq!(crossings[0]["surface"], "handler_payload_attr");
        assert_eq!(crossings[0]["requirement"], "client_encode");
        assert_eq!(crossings[0]["node"], button.0);
        assert_eq!(crossings[0]["attr"], "data-rsjs-payload-p0");
        assert_eq!(crossings[0]["label"], "day");
        assert_eq!(crossings[0]["source_anchor"]["start_line"], 15);
        assert_eq!(crossings[0]["source_anchor"]["start_column"], 42);
        assert!(crossings[0].get("value").is_none());
        assert!(crossings[0].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_node_payload_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let root = builder.element("div", None);
        builder.payload_attr_with_source(
            root,
            "data-rsjs-payload-p0",
            "item.label",
            Some("item.label".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 16,
                start_column: 31,
            }),
        );
        let value = builder.expr(rsjs_ir::ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p0".to_string(),
            tokens: "item.label".to_string(),
        });
        builder.bind_text(root, value);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "list_1234",
            "List",
            "List",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0]["kind"], "node_payload");
        assert_eq!(crossings[0]["surface"], "node_payload_attr");
        assert_eq!(crossings[0]["requirement"], "client_encode");
        assert_eq!(crossings[0]["node"], root.0);
        assert_eq!(crossings[0]["attr"], "data-rsjs-payload-p0");
        assert_eq!(crossings[0]["label"], "item.label");
        assert_eq!(crossings[0]["source_anchor"]["start_line"], 16);
        assert_eq!(crossings[0]["source_anchor"]["start_column"], 31);
        assert!(crossings[0].get("value").is_none());
        assert!(crossings[0].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_query_initial_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let key = builder.expr(rsjs_ir::ClientExpr::string("stock:AAPL"));
        let fetcher = builder.expr(rsjs_ir::ClientExpr::string("/api/stocks/AAPL"));
        let query = builder.query(key, key, fetcher, rsjs_ir::QueryOptions::default());
        builder.set_query_source(
            query,
            Some("price".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 18,
                start_column: 25,
            }),
        );
        let root = builder.element("div", None);
        let button = builder.element("button", Some(root));
        let handler = builder.handler_from_ir(rsjs_ir::ClientBlock::from_stmts(vec![
            rsjs_ir::ClientStmt::QueryRefetch { query },
        ]));
        builder.bind_event(button, rsjs_ir::EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "stock_1234",
            "Stock",
            "Stock",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            true,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 3);
        assert_eq!(crossings[0]["kind"], "query_key");
        assert_eq!(crossings[0]["surface"], "query_metadata_blob");
        assert_eq!(crossings[0]["requirement"], "query_codec");
        assert_eq!(crossings[0]["query"], query.0);
        assert_eq!(crossings[0]["label"], "price");
        assert_eq!(crossings[0]["source_anchor"]["start_line"], 18);
        assert_eq!(crossings[0]["source_anchor"]["start_column"], 25);
        assert!(crossings[0].get("value").is_none());
        assert!(crossings[0].get("tokens").is_none());
        assert_eq!(crossings[1]["kind"], "query_fetcher");
        assert_eq!(crossings[1]["surface"], "query_metadata_blob");
        assert_eq!(crossings[1]["requirement"], "request_config");
        assert_eq!(crossings[1]["query"], query.0);
        assert_eq!(crossings[1]["label"], "price.fetcher");
        assert_eq!(crossings[1]["source_anchor"]["start_line"], 18);
        assert_eq!(crossings[1]["source_anchor"]["start_column"], 25);
        assert!(crossings[1].get("value").is_none());
        assert!(crossings[1].get("tokens").is_none());
        assert_eq!(crossings[2]["kind"], "query_initial_value");
        assert_eq!(crossings[2]["surface"], "query_seed_blob");
        assert_eq!(crossings[2]["requirement"], "query_codec");
        assert_eq!(crossings[2]["query"], query.0);
        assert_eq!(crossings[2]["label"], "price");
        assert_eq!(crossings[2]["source_anchor"]["start_line"], 18);
        assert_eq!(crossings[2]["source_anchor"]["start_column"], 25);
        assert!(crossings[2].get("value").is_none());
        assert!(crossings[2].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_mutation_key_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let key = builder.expr(rsjs_ir::ClientExpr::string("save"));
        let runner = builder.expr(rsjs_ir::ClientExpr::string("/api/save"));
        let mutation = builder.mutation(
            key,
            runner,
            runner,
            vec![key],
            rsjs_ir::MutationOptions::default(),
        );
        builder.set_mutation_source(
            mutation,
            Some("save".to_string()),
            Some(rsjs_ir::SourceAnchor {
                start_line: 21,
                start_column: 25,
            }),
        );
        let root = builder.element("div", None);
        let button = builder.element("button", Some(root));
        let handler = builder.handler_from_ir(rsjs_ir::ClientBlock::from_stmts(vec![
            rsjs_ir::ClientStmt::RunMutation {
                mutation,
                args: Vec::new(),
            },
        ]));
        builder.bind_event(button, rsjs_ir::EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "save_1234",
            "Save",
            "Save",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            true,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 3);
        assert_eq!(crossings[0]["kind"], "mutation_key");
        assert_eq!(crossings[0]["surface"], "mutation_metadata_blob");
        assert_eq!(crossings[0]["requirement"], "mutation_codec");
        assert_eq!(crossings[0]["mutation"], mutation.0);
        assert_eq!(crossings[0]["label"], "save");
        assert_eq!(crossings[0]["source_anchor"]["start_line"], 21);
        assert_eq!(crossings[0]["source_anchor"]["start_column"], 25);
        assert!(crossings[0].get("value").is_none());
        assert!(crossings[0].get("tokens").is_none());
        assert_eq!(crossings[1]["kind"], "mutation_runner");
        assert_eq!(crossings[1]["surface"], "mutation_metadata_blob");
        assert_eq!(crossings[1]["requirement"], "request_config");
        assert_eq!(crossings[1]["mutation"], mutation.0);
        assert_eq!(crossings[1]["label"], "save.runner");
        assert_eq!(crossings[1]["source_anchor"]["start_line"], 21);
        assert_eq!(crossings[1]["source_anchor"]["start_column"], 25);
        assert!(crossings[1].get("value").is_none());
        assert!(crossings[1].get("tokens").is_none());
        assert_eq!(crossings[2]["kind"], "mutation_invalidation_key");
        assert_eq!(crossings[2]["surface"], "mutation_metadata_blob");
        assert_eq!(crossings[2]["requirement"], "mutation_codec");
        assert_eq!(crossings[2]["mutation"], mutation.0);
        assert_eq!(crossings[2]["invalidation_index"], 0);
        assert_eq!(crossings[2]["label"], "save.invalidates[0]");
        assert_eq!(crossings[2]["source_anchor"]["start_line"], 21);
        assert_eq!(crossings[2]["source_anchor"]["start_column"], 25);
        assert!(crossings[2].get("value").is_none());
        assert!(crossings[2].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_redacted_descriptor_reactive_prop_crossings() {
        let mut builder = rsjs_ir::IslandBuilder::new();
        let root = builder.element("section", None);
        let child_root = builder.element("div", Some(root));
        let parent_signal = builder.signal(rsjs_ir::Value::String("Alice".to_string()));
        builder.component_fragment_mount(
            child_root,
            "child_fragment".to_string(),
            Vec::new(),
            vec![rsjs_ir::ComponentFragmentSignalAlias {
                child: rsjs_ir::SignalId(7),
                parent: parent_signal,
                prop_name: Some("name".to_string()),
                source_anchor: Some(rsjs_ir::SourceAnchor {
                    start_line: 12,
                    start_column: 33,
                }),
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        builder.set_root(root);
        let island = builder.build();
        let analysis = rsjs_ir::classify_render_strategy(&island);

        let json = strategy_asset_json(
            "parent_1234",
            "Parent",
            "Parent",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &island,
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");
        let crossings = value["crossings"]
            .as_array()
            .expect("crossings should be an array");

        assert_eq!(crossings.len(), 2);
        assert_eq!(crossings[0]["kind"], "signal_initial_value");
        assert_eq!(crossings[1]["kind"], "descriptor_reactive_prop");
        assert_eq!(crossings[1]["surface"], "descriptor_reactive_prop");
        assert_eq!(crossings[1]["requirement"], "client_encode");
        assert_eq!(crossings[1]["root"], child_root.0);
        assert_eq!(crossings[1]["asset_id"], "child_fragment");
        assert_eq!(crossings[1]["prop"], "name");
        assert_eq!(crossings[1]["child_signal"], 7);
        assert_eq!(crossings[1]["parent_signal"], parent_signal.0);
        assert_eq!(crossings[1]["label"], "child_fragment::name");
        assert_eq!(crossings[1]["source_anchor"]["start_line"], 12);
        assert_eq!(crossings[1]["source_anchor"]["start_column"], 33);
        assert!(crossings[1].get("value").is_none());
        assert!(crossings[1].get("tokens").is_none());
    }

    #[test]
    fn strategy_sidecar_json_records_requested_hydration_strategy() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::ServerOnly,
            roots: Vec::new(),
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let json = strategy_asset_json(
            "staticpanel_1234",
            "StaticPanel",
            "StaticPanel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Media {
                query: "(min-width: 768px)".to_string(),
            },
            &[],
            &[],
            false,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["hydration_strategy"]["kind"], "media");
        assert_eq!(
            value["hydration_strategy"]["label"],
            "media:(min-width: 768px)"
        );
        assert_eq!(value["hydration_strategy"]["query"], "(min-width: 768px)");
        assert_eq!(value["emitted"]["js_asset"], false);
    }

    #[test]
    fn strategy_sidecar_json_records_browser_roots() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::SsrHydrate,
            roots: vec![
                rsjs_ir::ClientReachabilityRoot::BrowserRead {
                    kind: "location.pathname".to_string(),
                },
                rsjs_ir::ClientReachabilityRoot::BrowserEffect {
                    kind: "clipboard.write_text".to_string(),
                },
            ],
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let json = strategy_asset_json(
            "browser_1234",
            "BrowserProbe",
            "BrowserProbe",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["roots"][0]["kind"], "browser_read");
        assert_eq!(value["roots"][0]["read"], "location.pathname");
        assert_eq!(value["roots"][1]["kind"], "browser_effect");
        assert_eq!(value["roots"][1]["effect"], "clipboard.write_text");
    }

    #[test]
    fn strategy_sidecar_json_records_reachable_client_modules() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::SsrHydrate,
            roots: vec![rsjs_ir::ClientReachabilityRoot::ClientModuleCall {
                module: "crate::money".to_string(),
                name: "format_usd".to_string(),
            }],
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let declared_modules = vec!["crate::money".to_string(), "crate::dates".to_string()];
        let modules = vec!["crate::money".to_string()];
        let json = strategy_asset_json(
            "money_1234",
            "MoneyLabel",
            "MoneyLabel",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &declared_modules,
            &modules,
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["declared_client_modules"][0], "crate::money");
        assert_eq!(value["declared_client_modules"][1], "crate::dates");
        assert_eq!(value["client_modules"][0], "crate::money");
        assert_eq!(value["client_modules"].as_array().unwrap().len(), 1);
        assert_eq!(value["roots"][0]["kind"], "client_module_call");
    }

    #[test]
    fn strategy_sidecar_json_records_reachable_component_fragment_imports() {
        let analysis = rsjs_ir::StrategyAnalysis {
            strategy: rsjs_ir::RenderStrategy::SsrHydrate,
            roots: vec![
                rsjs_ir::ClientReachabilityRoot::ComponentFragmentMount {
                    root: rsjs_ir::NodeId(6),
                    asset_id: "child_z".to_string(),
                },
                rsjs_ir::ClientReachabilityRoot::ComponentFragmentMount {
                    root: rsjs_ir::NodeId(7),
                    asset_id: "child_a".to_string(),
                },
                rsjs_ir::ClientReachabilityRoot::ComponentFragmentMount {
                    root: rsjs_ir::NodeId(8),
                    asset_id: "child_z".to_string(),
                },
            ],
            crossings: Vec::new(),
            dead_primitives: Vec::new(),
        };
        let json = strategy_asset_json(
            "parent_1234",
            "Parent",
            "Parent",
            "sha256:test",
            StrategySourceInfo {
                impl_start_line: 10,
                impl_start_column: 1,
                render_start_line: 11,
                render_start_column: 5,
            },
            &rsjs_ir::Island::default(),
            &analysis,
            &rsjs_ir::HydrationStrategy::Immediate,
            &[],
            &[],
            true,
            false,
            false,
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("strategy sidecar JSON should parse");

        assert_eq!(value["component_fragment_imports"][0], "child_a");
        assert_eq!(value["component_fragment_imports"][1], "child_z");
        assert_eq!(
            value["component_fragment_imports"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(value["roots"][0]["kind"], "component_fragment_mount");
    }

    #[test]
    fn rsjs_expansion_keeps_active_ide_hints_for_stripped_helpers() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let count = rsjs::signal(0_i64);
                        let class_name = "rounded-md";
                        let label = Label;
                        html_sync! {
                            <div class={rsjs::server_attr(class_name)}>
                                <button on_click={|_| count.set(count.get() + 1)}>
                                    {rsjs::server_only(label)}
                                </button>
                            </div>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("simple rsjs impl should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact.contains("let_=rsjs::signal::<()>;"),
            "rsjs expansion should keep an active signal helper hint:\n{out}"
        );
        assert!(
            compact.contains("let_=rsjs::server_attr::<()>;"),
            "rsjs expansion should keep an active server_attr helper hint:\n{out}"
        );
        assert!(
            compact.contains("let_=rsjs::server_only::<()>;"),
            "rsjs expansion should keep an active server_only helper hint:\n{out}"
        );
        assert!(
            compact.contains("let_=::proa_macros::html_sync!{\"\"};"),
            "rsjs expansion should keep an active html_sync macro-path hint:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_keeps_loop_prefix_ide_hints_for_range_loops() {
        let out = expand_rsjs(
            quote! { component },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeLoopProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let dim = 3_u8;
                        let month = 5_u8;
                        let year = 2026_u16;
                        html_sync! {
                            <div>
                                {for d in 1u8..=dim {
                                    let date_buf = format_date_buf(year, month, d);
                                    let date_str = core::str::from_utf8(&date_buf).unwrap();
                                    html_sync! {
                                        <button data_date={rsjs::server_attr(date_str)}>
                                            {d}
                                        </button>
                                    }
                                }}
                            </div>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("range-loop rsjs impl should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact.contains(
                "let_=&(1u8..=dim);fordin1u8..=dim{letdate_buf=format_date_buf(year,month,d);"
            ),
            "rsjs expansion should keep active loop-prefix source hints:\n{out}"
        );
        assert!(
            compact.contains("let_=&(date_str);") && compact.contains("let_=&(d);"),
            "rsjs expansion should keep active loop-local attr/text hints:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_keeps_non_range_loop_and_signal_ide_hints() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeSelectProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let value = rsjs::signal(self.value);
                        html_sync! {
                            <select
                                value={value.get()}
                                on_change={|event| self.on_change.call(event.value())}
                            >
                                {for option in self.options {
                                    html_sync! {
                                        <option
                                            value={option.value}
                                            disabled={option.disabled}
                                            selected={value.get() == option.value}
                                        >
                                            {option.label}
                                        </option>
                                    }
                                }}
                            </select>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("non-range-loop rsjs impl should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact
                .contains("letvalue=::rsjs::signal(__rsjs_ide_infer(&__rsjs_initial_signal_0));"),
            "rsjs expansion should keep a typed IDE-only signal local without \
             replaying a potentially moving initializer:\n{out}"
        );
        assert!(
            compact.contains("letevent:::rsjs::Event=__rsjs_ide_payload();")
                && compact.contains("let_=&(self.on_change.call(__rsjs_ide_payload()));")
                && compact.contains("let_=&(event.value());"),
            "rsjs expansion should keep active event handler source hints:\n{out}"
        );
        assert!(
            compact.contains("let_=&(self.options);foroptionin(self.options).iter(){"),
            "rsjs expansion should keep an active non-range loop iterable hint:\n{out}"
        );
        assert!(
            compact.contains("let_=&(value.get());")
                && compact.contains("let_=&(option.value);")
                && compact.contains("let_=&(option.disabled);")
                && compact.contains("let_=&(option.label);"),
            "rsjs expansion should keep active attr/text hints inside non-range loops:\n{out}"
        );
    }

    #[test]
    fn signal_ide_hint_preserves_constructor_turbofish() {
        let statement: syn::Stmt =
            syn::parse_quote!(let maybe = rsjs::signal::<Option<i64>>(None););
        let syn::Stmt::Local(local) = statement else {
            panic!("fixture should parse as a local statement");
        };
        let out = signal_local_source_ide_hint(&local)
            .expect("signal local should produce an IDE hint")
            .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact.contains("letmaybe=::rsjs::signal::<Option<i64>>(__rsjs_ide_infer(&(None)));"),
            "signal IDE hint must preserve its explicit value type:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_keeps_named_handler_call_ide_hints_without_calling_it() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeCalendarProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let selected = rsjs::signal::<u32>(self.selected.unwrap_or(0_u32));
                        let dim = 31_u32;
                        let on_day_click = rsjs::event_handler(|event: rsjs::Event, day: u32| {
                            let selected_day = day + 1_u32;
                            self.on_select.call_if_some(selected_day);
                            self.on_select_date.call_if_some(event.closest_attr("data-date"));
                        });

                        html_sync! {
                            <div>
                                {for d in 1u32..=dim {
                                    let is_disabled = false;
                                    html_sync! {
                                        <button
                                            data_calendar_day={d}
                                            disabled={rsjs::server_attr(is_disabled)}
                                            data_selected={(selected.get() == (d as u32))}
                                            on_click={on_day_click(d)}
                                        >
                                            {d}
                                        </button>
                                    }
                                }}
                            </div>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("calendar-like rsjs impl should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact.contains(
                "leton_day_click=::rsjs::__private::event_handler_type::<rsjs::Event>();"
            ),
            "rsjs expansion should retain the named handler's event type:\n{out}"
        );
        assert!(
            compact.contains("letevent:rsjs::Event=__rsjs_ide_payload();")
                && compact.contains("letday:u32=__rsjs_ide_payload();")
                && compact.contains("letselected_day=__rsjs_ide_infer(&(day+1_u32));"),
            "rsjs expansion should keep event, payload, and handler-local names in scope:\n{out}"
        );
        assert!(
            compact.contains(
                "assert_event_binding::<::rsjs::web::dom::HtmlButtonElement,::rsjs::__private::event_interface::MouseEvent,_>(&(on_day_click));"
            ),
            "rsjs expansion should check a named handler against the template event and tag:\n{out}"
        );
        assert!(
            !compact.contains("let_=&(on_day_click(d));"),
            "rsjs expansion should not type-check the DSL-only named handler call as Rust:\n{out}"
        );
        assert!(
            compact.contains("let_=&(is_disabled);")
                && compact.contains("let_=&(selected.get());")
                && compact.contains("let_=&(d);"),
            "rsjs expansion should keep active loop-local attr/text hints around named handlers:\n{out}"
        );
        assert!(
            compact.contains("let_=rsjs::event_handler::<fn(::rsjs::Event),(::rsjs::Event,)>;"),
            "rsjs expansion should keep an active event_handler helper hint:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_keeps_interval_ide_witness_at_its_lexical_position() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for IdeIntervalShadowProbe {
                    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                        let tick = rsjs::signal(0_i32);
                        rsjs::interval(1000, || tick.update(|value| value + 1_i32));
                        let tick = rsjs::signal(0_i64);

                        html_sync! {
                            <output>{tick.get()}</output>
                        }
                        .render(cx)
                    }
                }
            },
        )
        .expect("same-named signals around an interval should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        let lexical_witness = concat!(
            "lettick=::rsjs::signal(__rsjs_ide_infer(&__rsjs_initial_signal_0));",
            "rsjs::interval(1000,||tick.update(|value|value+1_i32));",
            "lettick=::rsjs::signal(__rsjs_ide_infer(&__rsjs_initial_signal_1));"
        );

        assert!(
            compact.contains(lexical_witness),
            "the IDE-only program must keep the timer between the two exact signal bindings:\n{out}"
        );
    }

    #[test]
    fn rsjs_expansion_preserves_typed_query_ide_hint_without_moving_initial() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for IdeQueryProbe {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> RenderOutcome<
                        impl core::future::Future<
                            Output = Result<(), WriteError>,
                        >,
                    >
                    {
                        RenderOutcome::pending(async move {
                            let symbol = String::from("AAPL");
                            let initial_price: StockPrice = cx
                                .load_json(LoadKey::named("stock", symbol.as_str()))
                                .await?;
                            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                                QueryKey::new(("stock", symbol.as_str())),
                                initial_price,
                                rsjs::FetchRequest {
                                    url: "/api/stocks/AAPL",
                                    response: Some(rsjs::ResponseKind::Json),
                                    ..Default::default()
                                },
                                rsjs::QueryOptions::default(),
                            );
                            html! {
                                <div>
                                    <span>{price.data().value}</span>
                                    <button on_click={|_| price.refetch()}>"Refresh"</button>
                                </div>
                            }
                            .render(cx)
                            .resolve()
                            .await
                        })
                    }
                }
            },
        )
        .expect("typed query rsjs impl should expand")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        assert!(
            compact.contains("letprice:rsjs::Query<StockPrice>=rsjs::query_with_initial("),
            "rsjs expansion should preserve the source query type annotation in its IDE hint:\n{out}"
        );
        assert!(
            compact.contains("__rsjs_ide_payload()"),
            "rsjs expansion should avoid moving the server initial value into the IDE hint closure:\n{out}"
        );
        assert!(
            !compact.contains("letprice:rsjs::Query<StockPrice>=rsjs::query_with_initial(QueryKey::new((\"stock\",symbol.as_str())),initial_price,"),
            "rsjs expansion should not move the server initial value while preserving the typed query hint:\n{out}"
        );
    }

    #[test]
    fn inbound_query_key_and_render_hint_reuse_the_compiler_owned_signal() {
        let out = expand_rsjs(
            quote! { component, client },
            quote! {
                impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for InboundQueryKeyProbe {
                    fn render(
                        self,
                        cx: &mut WebContext<L, B>,
                    ) -> RenderOutcome<
                        impl core::future::Future<
                            Output = Result<(), WriteError>,
                        >,
                    >
                    {
                        RenderOutcome::pending(async move {
                            let resource: rsjs::Query<String> = rsjs::query_with_initial(
                                rsjs::QueryKey::new(("resource", self.resource)),
                                String::new(),
                                "/api/resource",
                                rsjs::QueryOptions::default(),
                            );
                            html! {
                                <section>
                                    <span>{self.resource}</span>
                                    <output>{resource.data()}</output>
                                </section>
                            }
                            .render(cx)
                            .resolve()
                            .await
                        })
                    }
                }
            },
        )
        .expect("an owned inbound query-key field should remain renderable")
        .to_string();
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();

        let initial = compact
            .find("let__rsjs_initial_signal_0=self.resource;")
            .expect("the inbound field should be evaluated exactly once");
        let render_hint = compact
            .find("let_=&(__rsjs_initial_signal_0);")
            .expect("the IDE witness should type-check the exact compiler-owned value");
        assert!(
            initial < render_hint,
            "the generated signal local must be in scope before its IDE witness:\n{out}"
        );
        let server_impl_end = compact
            .find("#[doc(hidden)]#[used]")
            .expect("component ABI records should follow the authored render impl");
        let server_impl = &compact[..server_impl_end];
        assert_eq!(
            server_impl.matches("self.resource").count(),
            1,
            "the generated render impl must not replay the moved field in an IDE-only closure:\n{out}"
        );
        assert!(
            !compact.contains("let_=&(self.resource);"),
            "the direct-render IDE hint must not borrow the field after the query key moved it:\n{out}"
        );
    }
}

/// Mirror per-island JS for ordinary, unstaged analysis tooling.
///
/// Resolution order:
/// 1. `OUT_DIR` (set when the user's crate has a build script): write
///    to `<OUT_DIR>/rsjs/<asset_id>.js`. This is the canonical home
///    when the user wires up a build script that consumes the
///    manifest in M7g.
/// 2. `CARGO_TARGET_DIR` (set in some workspace configurations):
///    write to `<CARGO_TARGET_DIR>/rsjs/<asset_id>.js`.
/// 3. Fallback: `target/rsjs/<asset_id>.js` relative to the current
///    working directory at compile time. This is the path
///    `cargo build` exposes when no build script is in play.
///
/// All errors are swallowed because these files are diagnostic mirrors, not
/// linker or runtime inputs. Staged compilation never calls this writer.
fn write_js_asset(asset_id: &str, js: &str) {
    let dir = rsjs_output_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.js", asset_id));
    let _ = std::fs::write(path, js);
}

fn write_component_fragment_asset(asset_id: &str, js: &str) {
    let dir = rsjs_output_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("component_{asset_id}.js"));
    let _ = std::fs::write(path, js);
}

fn write_client_ir_asset(asset_id: &str, island: &rsjs_ir::Island) {
    let dir = rsjs_output_dir().join("client_ir");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{asset_id}.json"));
    let _ = std::fs::write(path, client_ir_asset_json(asset_id, island));
}

fn write_hir_asset(asset_id: &str, sidecar: &analyze::HirSidecar) {
    let dir = rsjs_output_dir().join("hir");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{asset_id}.json"));
    let _ = std::fs::write(path, analyze::hir_sidecar_json(asset_id, sidecar));
}

#[derive(Debug, Clone, Copy)]
struct StrategySourceInfo {
    impl_start_line: usize,
    impl_start_column: usize,
    render_start_line: usize,
    render_start_column: usize,
}

fn strategy_input_fingerprint(attr_tokens: &TokenStream2, impl_tokens: &TokenStream2) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rsjs.strategy.input.v1");
    hasher.update(b"\0attr\0");
    hasher.update(normalized_token_string(attr_tokens).as_bytes());
    hasher.update(b"\0impl\0");
    hasher.update(normalized_token_string(impl_tokens).as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn normalized_token_string(tokens: &TokenStream2) -> String {
    tokens
        .to_string()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn write_strategy_asset(
    analysis_id: &str,
    struct_name: &str,
    struct_path: &str,
    input_fingerprint: &str,
    source: StrategySourceInfo,
    client_origin_allowed: bool,
    island: &rsjs_ir::Island,
    analysis: &rsjs_ir::StrategyAnalysis,
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    declared_client_modules: &[String],
    client_modules: &[String],
    needs_client_js: bool,
    has_queries: bool,
    has_mutations: bool,
) {
    let dir = rsjs_output_dir().join("strategy");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{analysis_id}.json"));
    let _ = std::fs::write(
        path,
        strategy_asset_json_with_origin_permission(
            analysis_id,
            struct_name,
            struct_path,
            input_fingerprint,
            source,
            client_origin_allowed,
            island,
            analysis,
            hydration_strategy,
            declared_client_modules,
            client_modules,
            needs_client_js,
            has_queries,
            has_mutations,
        ),
    );
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn strategy_asset_json(
    analysis_id: &str,
    struct_name: &str,
    struct_path: &str,
    input_fingerprint: &str,
    source: StrategySourceInfo,
    island: &rsjs_ir::Island,
    analysis: &rsjs_ir::StrategyAnalysis,
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    declared_client_modules: &[String],
    client_modules: &[String],
    needs_client_js: bool,
    has_queries: bool,
    has_mutations: bool,
) -> String {
    strategy_asset_json_with_origin_permission(
        analysis_id,
        struct_name,
        struct_path,
        input_fingerprint,
        source,
        true,
        island,
        analysis,
        hydration_strategy,
        declared_client_modules,
        client_modules,
        needs_client_js,
        has_queries,
        has_mutations,
    )
}

#[allow(clippy::too_many_arguments)]
fn strategy_asset_json_with_origin_permission(
    analysis_id: &str,
    struct_name: &str,
    struct_path: &str,
    input_fingerprint: &str,
    source: StrategySourceInfo,
    client_origin_allowed: bool,
    island: &rsjs_ir::Island,
    analysis: &rsjs_ir::StrategyAnalysis,
    hydration_strategy: &rsjs_ir::HydrationStrategy,
    declared_client_modules: &[String],
    client_modules: &[String],
    needs_client_js: bool,
    has_queries: bool,
    has_mutations: bool,
) -> String {
    let runtime_asset_id = needs_client_js.then_some(analysis_id);
    let render_strategy = render_strategy_json_name(analysis.strategy);
    let roots: Vec<_> = analysis.roots.iter().map(strategy_root_json).collect();
    let component_fragment_imports = analysis.reachable_component_fragment_imports();
    let crossings: Vec<_> = analysis
        .crossings
        .iter()
        .map(strategy_crossing_json)
        .collect();
    let dead_primitives: Vec<_> = analysis
        .dead_primitives
        .iter()
        .map(strategy_dead_primitive_json)
        .collect();
    let hydration_strategy = hydration_strategy_json(hydration_strategy);
    let has_shared_state = analysis.reachable_signal_ids().iter().any(|id| {
        island
            .signals
            .iter()
            .find(|signal| signal.id == *id)
            .is_some_and(|signal| signal.shared.is_some())
    });
    let runtime_features = runtime_features_json(
        needs_client_js,
        has_shared_state,
        has_queries,
        has_mutations,
    );
    let shared_state_dependencies: Vec<_> = island
        .shared_stores
        .iter()
        .filter_map(|store| {
            let fields: Vec<_> = island
                .signals
                .iter()
                .filter(|signal| analysis.reachable_signal_ids().contains(&signal.id))
                .filter_map(|signal| signal.shared.as_ref())
                .filter(|shared| shared.store == store.id)
                .map(|shared| shared.field.clone())
                .collect();
            // `type_key` and the provider's full serialized field list are
            // properties of the Rust type, which a proc macro cannot resolve.
            // The sidecar reports what the island itself knows: the store's
            // Rust type as written and the fields this island reads. See
            // `Analyzer::shared_field_client_type` for why nothing here asks
            // an out-of-band registry for the rest.
            (!fields.is_empty()).then(|| {
                serde_json::json!({
                    "store": store.id.0,
                    "rust_type": store.rust_type,
                    "type_key": serde_json::Value::Null,
                    "fields": fields,
                    "serialized_fields": Vec::<String>::new(),
                    "provider_serializes_full_snapshot": true,
                    "source": store.source_anchor.map(|anchor| serde_json::json!({
                        "start_line": anchor.start_line,
                        "start_column": anchor.start_column,
                    })),
                })
            })
        })
        .collect();
    let server_surfaces = strategy_server_surfaces(island);
    serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": analysis_id,
        "runtime_asset_id": runtime_asset_id,
        "struct_name": struct_name,
        "struct_path": struct_path,
        "client_origin_permission": if client_origin_allowed {
            "origin_capable"
        } else {
            "participant_only"
        },
        "render_strategy": render_strategy,
        "render_mode": render_strategy,
        "register_island": needs_client_js,
        "register_component_fragment": needs_client_js,
        "hydration_strategy": hydration_strategy,
        "declared_client_modules": declared_client_modules,
        "client_modules": client_modules,
        "component_fragment_imports": component_fragment_imports,
        "runtime_features": runtime_features,
        "shared_state_dependencies": shared_state_dependencies,
        "server_surfaces": server_surfaces,
        "crossings": crossings,
        "dead_primitives": dead_primitives,
        "input_fingerprint": input_fingerprint,
        "source": {
            "file": serde_json::Value::Null,
            "impl_start_line": source.impl_start_line,
            "impl_start_column": source.impl_start_column,
            "render_start_line": source.render_start_line,
            "render_start_column": source.render_start_column,
        },
        "roots": roots,
        "emitted": {
            "runtime_asset_id": runtime_asset_id,
            "wrapper": needs_client_js,
            "seed_script": needs_client_js,
            "manifest_registration": needs_client_js,
            "js_asset": needs_client_js,
            "component_fragment_asset": needs_client_js,
            "uses_queries": needs_client_js && has_queries,
            "uses_mutations": needs_client_js && has_mutations,
            "uses_shared_state": needs_client_js && has_shared_state,
        }
    })
    .to_string()
}

fn strategy_dead_primitive_json(dead: &rsjs_ir::DeadClientPrimitive) -> serde_json::Value {
    let mut value = serde_json::json!({
        "kind": dead_primitive_kind_json(dead.kind),
        "label": dead.label,
    });
    match dead.kind {
        rsjs_ir::DeadClientPrimitiveKind::Signal { signal } => {
            value["signal"] = serde_json::json!(signal.0);
        }
        rsjs_ir::DeadClientPrimitiveKind::Memo { signal } => {
            value["signal"] = serde_json::json!(signal.0);
        }
        rsjs_ir::DeadClientPrimitiveKind::Handler { handler } => {
            value["handler"] = serde_json::json!(handler.0);
        }
        rsjs_ir::DeadClientPrimitiveKind::NodeRef { node_ref } => {
            value["node_ref"] = serde_json::json!(node_ref.0);
        }
        rsjs_ir::DeadClientPrimitiveKind::Query { query } => {
            value["query"] = serde_json::json!(query.0);
        }
        rsjs_ir::DeadClientPrimitiveKind::Mutation { mutation } => {
            value["mutation"] = serde_json::json!(mutation.0);
        }
        _ => {}
    }
    add_source_anchor(&mut value, dead.source_anchor);
    value
}

fn dead_primitive_kind_json(kind: rsjs_ir::DeadClientPrimitiveKind) -> &'static str {
    match kind {
        rsjs_ir::DeadClientPrimitiveKind::Signal { .. } => "signal",
        rsjs_ir::DeadClientPrimitiveKind::Memo { .. } => "memo",
        rsjs_ir::DeadClientPrimitiveKind::Handler { .. } => "handler",
        rsjs_ir::DeadClientPrimitiveKind::NodeRef { .. } => "node_ref",
        rsjs_ir::DeadClientPrimitiveKind::Query { .. } => "query",
        rsjs_ir::DeadClientPrimitiveKind::Mutation { .. } => "mutation",
        _ => "unknown",
    }
}

fn strategy_crossing_json(crossing: &rsjs_ir::ClientCrossing) -> serde_json::Value {
    let mut value = serde_json::json!({
        "kind": crossing_kind_json(&crossing.kind),
        "surface": crossing_surface_json(crossing.surface),
        "requirement": crossing_requirement_json(crossing.requirement),
        "label": crossing.label,
    });
    match crossing.kind {
        rsjs_ir::ClientCrossingKind::SignalInitialValue { signal } => {
            value["signal"] = serde_json::json!(signal.0);
        }
        rsjs_ir::ClientCrossingKind::HandlerPayload { node, ref attr } => {
            value["node"] = serde_json::json!(node.0);
            value["attr"] = serde_json::json!(attr);
        }
        rsjs_ir::ClientCrossingKind::NodePayload { node, ref attr } => {
            value["node"] = serde_json::json!(node.0);
            value["attr"] = serde_json::json!(attr);
        }
        rsjs_ir::ClientCrossingKind::QueryKey { query }
        | rsjs_ir::ClientCrossingKind::QueryFetcher { query }
        | rsjs_ir::ClientCrossingKind::QueryInitialValue { query } => {
            value["query"] = serde_json::json!(query.0);
        }
        rsjs_ir::ClientCrossingKind::MutationKey { mutation }
        | rsjs_ir::ClientCrossingKind::MutationRunner { mutation } => {
            value["mutation"] = serde_json::json!(mutation.0);
        }
        rsjs_ir::ClientCrossingKind::MutationInvalidationKey { mutation, index } => {
            value["mutation"] = serde_json::json!(mutation.0);
            value["invalidation_index"] = serde_json::json!(index);
        }
        rsjs_ir::ClientCrossingKind::DescriptorReactiveProp {
            root,
            ref asset_id,
            ref prop,
            child_signal,
            parent_signal,
        } => {
            value["root"] = serde_json::json!(root.0);
            value["asset_id"] = serde_json::json!(asset_id);
            value["prop"] = serde_json::json!(prop);
            value["child_signal"] = serde_json::json!(child_signal.0);
            value["parent_signal"] = serde_json::json!(parent_signal.0);
        }
        _ => {}
    }
    add_source_anchor(&mut value, crossing.source_anchor);
    value
}

fn crossing_kind_json(kind: &rsjs_ir::ClientCrossingKind) -> &'static str {
    match kind {
        rsjs_ir::ClientCrossingKind::SignalInitialValue { .. } => "signal_initial_value",
        rsjs_ir::ClientCrossingKind::HandlerPayload { .. } => "handler_payload",
        rsjs_ir::ClientCrossingKind::NodePayload { .. } => "node_payload",
        rsjs_ir::ClientCrossingKind::QueryKey { .. } => "query_key",
        rsjs_ir::ClientCrossingKind::QueryFetcher { .. } => "query_fetcher",
        rsjs_ir::ClientCrossingKind::QueryInitialValue { .. } => "query_initial_value",
        rsjs_ir::ClientCrossingKind::MutationKey { .. } => "mutation_key",
        rsjs_ir::ClientCrossingKind::MutationRunner { .. } => "mutation_runner",
        rsjs_ir::ClientCrossingKind::MutationInvalidationKey { .. } => "mutation_invalidation_key",
        rsjs_ir::ClientCrossingKind::DescriptorReactiveProp { .. } => "descriptor_reactive_prop",
        _ => "unknown",
    }
}

fn crossing_surface_json(surface: rsjs_ir::ClientCrossingSurface) -> &'static str {
    match surface {
        rsjs_ir::ClientCrossingSurface::SignalSeed => "signal_seed",
        rsjs_ir::ClientCrossingSurface::HandlerPayloadAttr => "handler_payload_attr",
        rsjs_ir::ClientCrossingSurface::NodePayloadAttr => "node_payload_attr",
        rsjs_ir::ClientCrossingSurface::QueryMetadataBlob => "query_metadata_blob",
        rsjs_ir::ClientCrossingSurface::QuerySeedBlob => "query_seed_blob",
        rsjs_ir::ClientCrossingSurface::MutationMetadataBlob => "mutation_metadata_blob",
        rsjs_ir::ClientCrossingSurface::DescriptorReactiveProp => "descriptor_reactive_prop",
        _ => "unknown",
    }
}

fn crossing_requirement_json(requirement: rsjs_ir::ClientCrossingRequirement) -> &'static str {
    match requirement {
        rsjs_ir::ClientCrossingRequirement::ClientEncode => "client_encode",
        rsjs_ir::ClientCrossingRequirement::QueryCodec => "query_codec",
        rsjs_ir::ClientCrossingRequirement::MutationCodec => "mutation_codec",
        rsjs_ir::ClientCrossingRequirement::RequestConfig => "request_config",
        _ => "unknown",
    }
}

fn runtime_features_json(
    needs_client_js: bool,
    has_shared_state: bool,
    has_queries: bool,
    has_mutations: bool,
) -> Vec<&'static str> {
    if !needs_client_js {
        return Vec::new();
    }

    let mut features = Vec::new();
    if has_shared_state {
        features.push("shared_state");
    }
    if has_queries {
        features.push("query");
    }
    if has_mutations {
        features.push("mutation");
    }
    features
}

fn strategy_server_surfaces(island: &rsjs_ir::Island) -> Vec<serde_json::Value> {
    let mut surfaces = Vec::new();
    if let Some(root) = island.view_tree.root() {
        collect_strategy_server_surfaces(root, island, &mut surfaces);
    }
    surfaces
}

fn collect_strategy_server_surfaces(
    node: rsjs_ir::NodeId,
    island: &rsjs_ir::Island,
    surfaces: &mut Vec<serde_json::Value>,
) {
    match island.view_tree.get(node) {
        rsjs_ir::ViewNode::Element {
            children,
            server_attrs,
            ..
        } => {
            for attr in server_attrs {
                let mut surface = serde_json::json!({
                    "kind": "server_attr",
                    "node": node.0,
                    "attr": attr.name,
                    "source": attr.source.as_str(),
                    "reason": attr.source.reason(),
                });
                add_server_surface_anchor(&mut surface, attr.source_anchor);
                surfaces.push(surface);
            }
            for child in children {
                collect_strategy_server_surfaces(*child, island, surfaces);
            }
        }
        rsjs_ir::ViewNode::ServerOnly(server) => {
            let mut surface = serde_json::json!({
                "kind": "server_slot",
                "node": node.0,
                "source": server.source.as_str(),
                "reason": server.source.reason(),
            });
            add_server_surface_anchor(&mut surface, server.source_anchor);
            surfaces.push(surface);
        }
        rsjs_ir::ViewNode::ForLoop { children, .. } => {
            for child in children {
                collect_strategy_server_surfaces(*child, island, surfaces);
            }
        }
        _ => {}
    }
}

fn add_server_surface_anchor(
    surface: &mut serde_json::Value,
    anchor: Option<rsjs_ir::ServerSurfaceAnchor>,
) {
    add_source_anchor(surface, anchor);
}

fn add_source_anchor(surface: &mut serde_json::Value, anchor: Option<rsjs_ir::SourceAnchor>) {
    let Some(anchor) = anchor else {
        return;
    };
    surface["source_anchor"] = serde_json::json!({
        "start_line": anchor.start_line,
        "start_column": anchor.start_column,
    });
}

fn hydration_strategy_json(strategy: &rsjs_ir::HydrationStrategy) -> serde_json::Value {
    match strategy {
        rsjs_ir::HydrationStrategy::Immediate => serde_json::json!({
            "kind": "immediate",
            "label": "immediate",
        }),
        rsjs_ir::HydrationStrategy::Visible => serde_json::json!({
            "kind": "visible",
            "label": "visible",
        }),
        rsjs_ir::HydrationStrategy::Idle => serde_json::json!({
            "kind": "idle",
            "label": "idle",
        }),
        rsjs_ir::HydrationStrategy::Interaction { event } => serde_json::json!({
            "kind": "interaction",
            "label": match event {
                Some(event) => format!("interaction:{event}"),
                None => "interaction".to_string(),
            },
            "event": event,
        }),
        rsjs_ir::HydrationStrategy::Media { query } => serde_json::json!({
            "kind": "media",
            "label": format!("media:{query}"),
            "query": query,
        }),
    }
}

fn render_strategy_json_name(strategy: rsjs_ir::RenderStrategy) -> &'static str {
    match strategy {
        rsjs_ir::RenderStrategy::ServerOnly => "server_only",
        rsjs_ir::RenderStrategy::SsrHydrate => "ssr_hydrate",
        rsjs_ir::RenderStrategy::ClientOnlyExplicit => "client_only_explicit",
        _ => "unknown",
    }
}

fn strategy_root_json(root: &rsjs_ir::ClientReachabilityRoot) -> serde_json::Value {
    match root {
        rsjs_ir::ClientReachabilityRoot::TextBinding { node } => {
            serde_json::json!({ "kind": "text_binding", "node": node.0 })
        }
        rsjs_ir::ClientReachabilityRoot::AttrBinding { node, name } => {
            serde_json::json!({ "kind": "attr_binding", "node": node.0, "name": name })
        }
        rsjs_ir::ClientReachabilityRoot::EventBinding { node, event } => serde_json::json!({
            "kind": "event_binding",
            "node": node.0,
            "event": event.as_attr_name(),
            "dom_event": event.as_dom_name(),
        }),
        rsjs_ir::ClientReachabilityRoot::ConditionalSection { parent } => {
            serde_json::json!({ "kind": "conditional_section", "parent": parent.0 })
        }
        rsjs_ir::ClientReachabilityRoot::InputBinding { node } => {
            serde_json::json!({ "kind": "input_binding", "node": node.0 })
        }
        rsjs_ir::ClientReachabilityRoot::NodeRefBinding { node, node_ref } => serde_json::json!({
            "kind": "node_ref_binding",
            "node": node.0,
            "node_ref": node_ref.0,
        }),
        rsjs_ir::ClientReachabilityRoot::NodeRef { id } => {
            serde_json::json!({ "kind": "node_ref", "id": id.0 })
        }
        rsjs_ir::ClientReachabilityRoot::Signal { id } => {
            serde_json::json!({ "kind": "signal", "id": id.0 })
        }
        rsjs_ir::ClientReachabilityRoot::Query { id } => {
            serde_json::json!({ "kind": "query", "id": id.0 })
        }
        rsjs_ir::ClientReachabilityRoot::Mutation { id } => {
            serde_json::json!({ "kind": "mutation", "id": id.0 })
        }
        rsjs_ir::ClientReachabilityRoot::BrowserRead { kind } => {
            serde_json::json!({ "kind": "browser_read", "read": kind })
        }
        rsjs_ir::ClientReachabilityRoot::BrowserEffect { kind } => {
            serde_json::json!({ "kind": "browser_effect", "effect": kind })
        }
        rsjs_ir::ClientReachabilityRoot::ClientModuleCall { module, name } => {
            serde_json::json!({ "kind": "client_module_call", "module": module, "name": name })
        }
        rsjs_ir::ClientReachabilityRoot::ClientHelperCall {
            edge,
            authored_path,
        } => serde_json::json!({
            "kind": "client_helper_call",
            "edge": edge.index(),
            "authored_path": authored_path,
        }),
        rsjs_ir::ClientReachabilityRoot::Interval { index } => {
            serde_json::json!({ "kind": "interval", "index": index })
        }
        rsjs_ir::ClientReachabilityRoot::KeyedList { container } => {
            serde_json::json!({ "kind": "keyed_list", "container": container.0 })
        }
        rsjs_ir::ClientReachabilityRoot::ComponentFragmentMount { root, asset_id } => {
            serde_json::json!({
                "kind": "component_fragment_mount",
                "root": root.0,
                "asset_id": asset_id,
            })
        }
        _ => serde_json::json!({ "kind": "unknown" }),
    }
}

fn client_ir_asset_json(asset_id: &str, island: &rsjs_ir::Island) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"format\":\"rsjs.client_ir\",\"client_ir_version\":{CLIENT_IR_SCHEMA_VERSION},\"asset_id\":"
    );
    push_json_string(&mut out, asset_id);
    out.push_str(",\"handlers\":[");
    for (idx, handler) in island.handlers.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"id\":{id},\"body_ir\":", id = handler.id.0);
        rsjs_ir::write_client_block_json(&mut out, handler.body_ir());
        out.push('}');
    }
    out.push_str("],\"intervals\":[");
    for (idx, interval) in island.intervals.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"ms\":{ms},\"body_ir\":", ms = interval.ms);
        rsjs_ir::write_client_block_json(&mut out, interval.body_ir());
        out.push('}');
    }
    out.push_str("],\"component_fragment_mounts\":[");
    for (idx, mount) in island.component_fragment_mounts.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"root\":{root},\"asset_id\":", root = mount.root.0);
        push_json_string(&mut out, &mount.asset_id);
        out.push_str(",\"callbacks\":[");
        for (callback_idx, callback) in mount.callbacks.iter().enumerate() {
            if callback_idx > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"slot\":{slot},\"body_ir\":",
                slot = callback.slot.0
            );
            rsjs_ir::write_client_block_json(&mut out, callback.body_ir());
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn rsjs_output_dir() -> std::path::PathBuf {
    if let Ok(out) = std::env::var("OUT_DIR") {
        std::path::PathBuf::from(out).join("rsjs")
    } else if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        std::path::PathBuf::from(target).join("rsjs")
    } else {
        std::path::PathBuf::from("target").join("rsjs")
    }
}

static COMPONENT_DESCRIPTOR_WRITE_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn write_component_descriptor_atomically(path: &std::path::Path, json: &str) {
    let Some(dir) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    // Descriptor readers run in other rustc processes during the same Cargo
    // build. Writing the final path directly lets a reader observe the file
    // after truncation but before the JSON bytes arrive. Write a sibling that
    // descriptor discovery ignores, then publish it with an atomic rename.
    let write_id = COMPONENT_DESCRIPTOR_WRITE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("component.json");
    let temporary = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        write_id
    ));
    if std::fs::write(&temporary, json).is_err() {
        return;
    }

    if std::fs::rename(&temporary, path).is_ok() {
        return;
    }

    // Windows does not replace an existing destination with rename. Asset ids
    // include a content hash, so concurrent writers normally produced the
    // same descriptor; retain the already-published complete file in that
    // case. The removal branch only recovers stale partial files written by
    // older compiler versions.
    if std::fs::read(path).is_ok_and(|existing| existing == json.as_bytes()) {
        let _ = std::fs::remove_file(&temporary);
        return;
    }
    let _ = std::fs::remove_file(path);
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_component_descriptor(
    asset_id: &str,
    struct_name: &str,
    parsed: &parse::ParsedIsland,
    island: &rsjs_ir::Island,
    js: &str,
    signal_initializers: &[emit_rust::SignalInitializer],
    inbound_inputs: &analyze::InboundInputMetadata,
    shared_state_provider_sites: &[analyze::DirectSharedStateProviderSite],
    query_initializers: &[emit_rust::QueryInitializer],
    mutation_initializers: &[emit_rust::MutationInitializer],
) -> String {
    component_descriptor::to_json_with_initializers_and_provider_sites(
        asset_id,
        struct_name,
        &parsed.self_ty.to_token_stream().to_string(),
        island,
        js,
        parsed.client_origin_allowed,
        signal_initializers,
        inbound_inputs,
        shared_state_provider_sites,
        query_initializers,
        mutation_initializers,
    )
}

fn write_component_descriptor_asset(asset_id: &str, json: &str) {
    let dir = rsjs_output_dir().join("components");
    let path = dir.join(format!("{}.json", asset_id));
    // This sidecar is a non-authoritative debugging aid. The exact descriptor
    // object/rlib carries the same JSON in a validated inline payload record.
    write_component_descriptor_atomically(&path, json);
}

/// `#[derive(rsjs::ClientProps)]` — generate matched
/// `ClientEncode` + `ClientDecode` impls and a sibling
/// `__rsjs_codec_meta_<TypeIdent>` const for a user struct.
///
/// See `rsjs::client_codec` for the wire format and
/// `rsjs_macros::client_props_derive` for the implementation.
#[proc_macro_derive(ClientProps)]
pub fn derive_client_props(input: TokenStream) -> TokenStream {
    client_props_derive::derive_client_props(input.into()).into()
}

/// `#[derive(rsjs::SharedState)]`: generate the rsjs client codec, stable
/// protocol metadata, and the typed handle used by `rsjs::shared::<T>()`.
#[proc_macro_derive(SharedState, attributes(rsjs))]
pub fn derive_shared_state(input: TokenStream) -> TokenStream {
    shared_state_derive::derive_shared_state(input.into()).into()
}

/// `#[rsjs::client]` — mark a free function as a reusable client-side
/// helper, or define a closed client value-object module.
///
/// The annotated function's body must conform to the v0.1 constrained
/// `Expr` IR (literals, parameter reads, binary operators,
/// `.is_non_empty()`, calls to other `#[rsjs::client]` helpers). The
/// macro replaces the original function with:
///
/// 1. A `#[doc(hidden)]` metadata struct
///    `__rsjs_client_<name>_meta` carrying the helper's `NAME`,
///    `PARAMS`, `RETURN_TY`, frontend `BODY_HIR_JSON`, and Client IR v3
///    `BODY_IR_JSON` blobs describing the lowered body. The bundling
///    pass and `ClientHelperMeta` view use the v3 body shape; the
///    frontend HIR blob remains a direct generated const for snapshot
///    coverage.
/// 2. A re-emitted Rust function with the same signature whose body
///    delegates to the private rsjs runtime helper
///    `::rsjs::__runtime::run_client_helper`, which evaluates the
///    generated metadata server evaluator for the supported primitive
///    helper subset.
///
/// See `docs/agent/rsjs-islands.md` for supported helper authoring and the
/// macro crate's internal client-lowering module for schema and rejection
/// rules.
///
/// On an inline visible module, the same attribute provides the initial
/// value-object authoring slice: named-field structs, direct `new(...) -> Self`
/// constructors, borrowed read/mutation methods with typed value arguments,
/// scalar `let` / `let mut` locals, local/field mutation, and assignment-oriented
/// `if` / `else` branches. Public free functions in the module are the
/// transport-safe helper roots. Method arguments are evaluated once before
/// method-body statements. Each constructor and receiver method becomes one
/// private relocation-linked helper definition, so nested argument-bearing
/// reads work and reachable method bodies are emitted once.
///
/// `#[rsjs::client(class)]` applies the same lowering to an explicit ESM
/// interoperability boundary. Every public struct needs a public
/// `new(...) -> Self`; it becomes an exported JavaScript class whose private
/// state wraps the lowered nominal record. Public borrowed receiver methods
/// become class methods, so JavaScript gets `new`, `instanceof`, persistent
/// per-instance mutation, and ordinary method syntax without making
/// prototype/`this` semantics part of Client IR.
#[proc_macro_attribute]
pub fn client(attr: TokenStream, item: TokenStream) -> TokenStream {
    client_attr::expand(attr.into(), item.into()).into()
}

/// `#[rsjs::client_module]` — group `#[rsjs::client]` helpers into a
/// single client-loadable JS bundle.
///
/// Applied to `pub mod <name> { ... }`. The macro collects every
/// `pub fn` inside the module (each must be `#[rsjs::client]`-marked,
/// or rustc surfaces a clean name-resolution error pointing at the
/// missing `__rsjs_client_<name>_meta::META` const) and emits a
/// sibling `__rsjs_client_module_<name>_meta:
/// &'static rsjs::ClientModuleMeta` for optional bundle metadata.
///
/// Consuming components call helpers through ordinary Rust paths. Reachable
/// helper edges are resolved through rustc; no component allowlist is needed.
///
/// The runtime manifest entry points at that helper metadata; the
/// framework adapter / collector builds the actual bundle JS from
/// post-expansion metadata instead of this outer macro re-lowering
/// helper bodies from raw syntax.
///
/// See `rsjs_macros::client_module_attr` for the full expansion rules and
/// `docs/agent/rsjs-islands.md` for the public helper visibility contract.
#[proc_macro_attribute]
pub fn client_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    client_module_attr::expand(attr.into(), item.into()).into()
}
