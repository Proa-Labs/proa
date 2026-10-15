// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Strict schema-v9 reader for the legacy RSJS client graph.
//!
//! This module isolates legacy validation from canonical ABI lowering so the
//! adapter never has to infer semantics from partially checked JSON.

use std::collections::{BTreeMap, BTreeSet};

use rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION;
use rsjs_ir::{
    client_block_json, client_expr_json, collect_client_block_external_dependencies,
    collect_client_expr_external_dependencies, parse_client_block_json, parse_client_expr_json,
    parse_client_type_json, walk_client_block, walk_client_expr, write_client_type_json,
    ClientBlock, ClientBlockExternalDependencies, ClientBrowserEffect, ClientBrowserRead,
    ClientExpr, ClientNodeRefEffect, ClientStmt, ClientType, MutationAccessor, QueryAccessor,
    SourceAnchor,
};
use serde_json::{Map, Value};

const DESCRIPTOR_FORMAT: &str = "rsjs.component_descriptor";
const DESCRIPTOR_SCHEMA: u64 = 9;
const CLIENT_IR_SCHEMA: u64 = CLIENT_IR_SCHEMA_VERSION as u64;

const UNSUPPORTED_FEATURE_ORDER: &[&str] = &[
    "queries",
    "mutations",
    "intervals",
    "shared_backed_signals",
    "client_imports",
    "unsupported_shapes",
];

/// The validated legacy graph subset that can be lowered without consulting
/// source-token identity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyClientGraph {
    /// SharedState acquisitions in strictly increasing legacy-ID order.
    ///
    /// Schema v9 carries only source metadata and Rust type tokens. The
    /// canonical type key/schema ABI is intentionally left for a later
    /// relocation-backed adapter slice.
    pub(crate) shared_stores: Vec<LegacySharedStore>,
    /// Surviving signals in strictly increasing legacy-ID order.
    pub(crate) signals: Vec<LegacySignal>,
    /// Arena expressions in strictly increasing legacy-ID order.
    pub(crate) expressions: Vec<LegacyExpression>,
    /// DOM/event handlers in strictly increasing legacy-ID order.
    pub(crate) handlers: Vec<LegacyBoundHandler>,
    /// Component callback slots in strictly increasing legacy-ID order.
    pub(crate) callback_slots: Vec<LegacyCallbackSlot>,
    /// Query declarations in strictly increasing legacy-ID order.
    pub(crate) queries: Vec<LegacyQuery>,
    /// Exact query IDs reached from rendered observations or bound handler
    /// refetch/invalidation work.
    ///
    /// A declaration absent from this set remains dead metadata and must not
    /// create a canonical query origin or owner.
    pub(crate) reachable_queries: Vec<u32>,
    /// Mutation declarations in strictly increasing legacy-ID order.
    pub(crate) mutations: Vec<LegacyMutation>,
    /// Exact mutation IDs reached from rendered state observations or bound
    /// handler `run(...)` calls.
    ///
    /// A declaration absent from this set remains dead metadata and must not
    /// create a canonical mutation origin or load the mutation runtime.
    pub(crate) reachable_mutations: Vec<u32>,
    /// Expressions in the dependency closure rooted by rendered bindings,
    /// bound handlers, and reachable query declarations.
    ///
    /// Query operations are canonicalized only when their enclosing
    /// expression is in this set.
    pub(crate) reachable_expressions: Vec<u32>,
    /// Node-reference declarations in strictly increasing legacy-ID order.
    pub(crate) node_refs: Vec<LegacyNodeRef>,
    /// Exact declaration IDs reached from rendered bindings and live client
    /// code. A declaration absent from this set remains a dead prelude local
    /// and must not create a canonical browser origin.
    pub(crate) reachable_node_refs: Vec<u32>,
    /// DOM bindings whose targets and selected graph values were validated.
    pub(crate) bindings: LegacyDomBindings,
    /// Keyed collection declarations in dense producer order.
    pub(crate) keyed_collections: Vec<LegacyKeyedCollection>,
    /// Dense schema-v9 view-node count used by the validated DOM bindings.
    pub(crate) node_count: u32,
    /// Valid schema-v9 behavior outside the initial lossless lowering subset.
    ///
    /// Values use [`UNSUPPORTED_FEATURE_ORDER`] rather than discovery order.
    pub(crate) unsupported_features: Vec<String>,
}

/// One compiler-declared keyed collection and its reactive source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyKeyedCollection {
    pub(crate) id: u32,
    pub(crate) container: u32,
    pub(crate) loop_node: u32,
    pub(crate) item_root: u32,
    pub(crate) source: LegacyKeyedCollectionSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyKeyedCollectionSource {
    Signal(u32),
    Expression(u32),
}

/// One surviving legacy signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacySignal {
    pub(crate) id: u32,
    pub(crate) derived_expression: Option<u32>,
    /// Compatibility projection retained for the current adapter.
    pub(crate) shared: bool,
    /// Exact schema-v9 SharedState field identity, when present.
    pub(crate) shared_field: Option<LegacySharedFieldRef>,
}

/// One schema-v9 component-local node-reference declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyNodeRef {
    pub(crate) id: u32,
    pub(crate) source_name: Option<String>,
    pub(crate) source_anchor: Option<SourceAnchor>,
}

/// One schema-v9 `rsjs::shared::<T>()` acquisition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacySharedStore {
    pub(crate) id: u32,
    pub(crate) source_name: String,
    pub(crate) rust_type: String,
    pub(crate) source_anchor: Option<SourceAnchor>,
}

/// Exact SharedState field cell backing one legacy signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacySharedFieldRef {
    pub(crate) store: u32,
    pub(crate) field: String,
}

/// Fully normalized schema-v9 query declaration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyQuery {
    pub(crate) id: u32,
    pub(crate) source_name: Option<String>,
    pub(crate) source_anchor: Option<SourceAnchor>,
    pub(crate) key_expression: u32,
    pub(crate) server_initial_expression: u32,
    pub(crate) client_fetcher_expression: u32,
    pub(crate) data_type: ClientType,
    pub(crate) options: LegacyQueryOptions,
    /// Rust-side initializer source joined by query ID.
    pub(crate) initializer: Option<LegacyQueryInitializer>,
}

/// Query options preserved without merging observer-local and entry-wide
/// semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LegacyQueryOptions {
    pub(crate) interval_ms: Option<u32>,
    pub(crate) refetch_on_focus: bool,
    pub(crate) download_progress: bool,
    pub(crate) stream_text: bool,
    pub(crate) retry_count: u32,
    pub(crate) stale_time_ms: u32,
}

/// Fully normalized schema-v9 mutation declaration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyMutation {
    pub(crate) id: u32,
    pub(crate) source_name: Option<String>,
    pub(crate) source_anchor: Option<SourceAnchor>,
    pub(crate) key_expression: u32,
    pub(crate) server_runner_expression: u32,
    pub(crate) client_runner_expression: u32,
    pub(crate) invalidates: Vec<u32>,
    pub(crate) options: LegacyMutationOptions,
}

/// Mutation policy preserved exactly from the producer descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LegacyMutationOptions {
    pub(crate) retry_count: u32,
    pub(crate) on_success_invalidate_strategy: LegacyInvalidationStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyInvalidationStrategy {
    Refetch,
    Reset,
}

/// Raw Rust initializer syntax retained for server-codegen compatibility.
///
/// These strings are not canonical query codec/spec authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyQueryInitializer {
    pub(crate) expression: String,
    pub(crate) key_expression: Option<String>,
}

/// One canonical Client IR v3 expression plus dependencies recomputed from it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyExpression {
    pub(crate) id: u32,
    pub(crate) expression: ClientExpr,
    pub(crate) dependencies: ClientBlockExternalDependencies,
    /// Browser capability operations in stable Client IR preorder.
    pub(crate) browser_operations: Vec<LegacyBrowserOperation>,
    /// All browser/query/SharedState operations in stable structural preorder.
    pub(crate) client_operations: Vec<LegacyClientOperation>,
}

/// One canonical Client IR v3 event-handler body plus recomputed dependencies.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyBoundHandler {
    pub(crate) id: u32,
    pub(crate) debounce_ms: Option<u64>,
    pub(crate) body: ClientBlock,
    pub(crate) dependencies: ClientBlockExternalDependencies,
    /// Browser capability operations in stable Client IR preorder.
    pub(crate) browser_operations: Vec<LegacyBrowserOperation>,
    /// All browser/query/SharedState operations in stable structural preorder.
    pub(crate) client_operations: Vec<LegacyClientOperation>,
}

/// One browser capability operation retained from schema-v9 Client IR.
///
/// Operation IDs are dense within their enclosing expression or handler and
/// follow structural preorder. `consumer` is therefore either the enclosing
/// value/effect or an earlier enclosing capability operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyBrowserOperation {
    pub(crate) kind: LegacyBrowserOperationKind,
    pub(crate) consumer: LegacyBrowserConsumer,
    /// Exact transitive external inputs to this operation's argument
    /// expressions, recomputed from canonical Client IR.
    pub(crate) dependencies: ClientBlockExternalDependencies,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyBrowserOperationKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyBrowserConsumer {
    /// The enclosing expression value or bound event-handler effect.
    Enclosing,
    /// Another capability operation in the same preorder sequence.
    Operation(u32),
}

/// One structurally observed browser/query/SharedState operation.
///
/// The unified preorder makes cross-kind nesting explicit. The existing
/// `browser_operations` projection remains available until the canonical
/// adapter migrates to this richer record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LegacyClientOperation {
    pub(crate) kind: LegacyClientOperationKind,
    pub(crate) consumer: LegacyClientOperationConsumer,
    /// Exact external inputs to this operation's arguments.
    pub(crate) dependencies: ClientBlockExternalDependencies,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyClientOperationKind {
    BrowserRead,
    BrowserWrite,
    NodeRefRead {
        node_ref: u32,
    },
    NodeRefWrite {
        node_ref: u32,
    },
    QueryRead {
        query: u32,
        accessor: QueryAccessor,
    },
    QueryRefetch {
        query: u32,
    },
    QueryInvalidate {
        query: u32,
    },
    MutationRead {
        mutation: u32,
        accessor: MutationAccessor,
    },
    MutationRun {
        mutation: u32,
    },
    SharedRead {
        signal: u32,
        field: LegacySharedFieldRef,
    },
    SharedSet {
        signal: u32,
        field: LegacySharedFieldRef,
    },
    SharedUpdate {
        signal: u32,
        field: LegacySharedFieldRef,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyClientOperationConsumer {
    /// The enclosing expression value or event-handler effect.
    Enclosing,
    /// Another client operation in the same unified preorder sequence.
    Operation(u32),
}

/// One named callback slot that a handler body may invoke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyCallbackSlot {
    pub(crate) id: u32,
    pub(crate) name: String,
}

/// Validated DOM bindings supported by the first lowering subset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LegacyDomBindings {
    pub(crate) text: Vec<LegacyTextBinding>,
    pub(crate) attributes: Vec<LegacyAttributeBinding>,
    pub(crate) inputs: Vec<LegacyInputBinding>,
    pub(crate) events: Vec<LegacyEventBinding>,
    pub(crate) conditionals: Vec<LegacyConditionalBinding>,
    pub(crate) node_refs: Vec<LegacyNodeRefBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyTextBinding {
    pub(crate) node: u32,
    pub(crate) expression: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyAttributeBinding {
    pub(crate) node: u32,
    pub(crate) name: String,
    pub(crate) kind: LegacyAttributeBindingKind,
    pub(crate) expression: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyAttributeBindingKind {
    Value,
    Boolean,
    Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyInputBinding {
    pub(crate) node: u32,
    pub(crate) kind: LegacyInputBindingKind,
    pub(crate) signal: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyInputBindingKind {
    Value,
    Checked,
    Selected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyEventBinding {
    pub(crate) node: u32,
    pub(crate) event: String,
    pub(crate) handler: u32,
}

/// One source-ordered structural control retained from schema-v9 bindings.
///
/// `control` is the dense ordinal in the canonical, parent-ordered
/// `bindings.conditionals` array. It is producer-local identity, not a DOM
/// node ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyConditionalBinding {
    pub(crate) control: u32,
    pub(crate) parent: u32,
    pub(crate) condition: u32,
    pub(crate) then_children: Vec<u32>,
    pub(crate) else_children: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyNodeRefBinding {
    pub(crate) node: u32,
    pub(crate) node_ref: u32,
}

/// Parse and close a schema-v9 client graph.
///
/// `reachable_helper_ordinals` is relocation-derived producer-local helper
/// edge authority. A helper call embedded in Client IR is accepted only when
/// its ordinal is present in this set.
#[cfg(test)]
pub(crate) fn parse_legacy_client_graph(
    descriptor: &Map<String, Value>,
    reachable_helper_ordinals: &BTreeSet<u32>,
) -> Result<LegacyClientGraph, String> {
    parse_legacy_client_graph_with_inbound_signals(
        descriptor,
        reachable_helper_ordinals,
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
}

/// Parse a schema-v9 graph while accepting unresolved client types for
/// authenticated inbound signal targets.
///
/// A component macro cannot name the Rust type of `self.field` while it emits
/// its descriptor, so a synthetic `LiveRead`, `QueryKeyDependency`, or
/// `SeedOnce` signal may carry `ClientType::Unknown`. The transport field
/// relocation plus generated Rust crossing/codec checks authenticate that
/// value path; type-sensitive client operations remain responsible for
/// requiring a concrete Client IR type. Unknown types anywhere else stay
/// fail-closed.
pub(crate) fn parse_legacy_client_graph_with_inbound_signals(
    descriptor: &Map<String, Value>,
    reachable_helper_ordinals: &BTreeSet<u32>,
    authenticated_inbound_signals: &BTreeSet<u32>,
    extra_signal_roots: &BTreeSet<u32>,
) -> Result<LegacyClientGraph, String> {
    validate_descriptor_shell(descriptor)?;

    let mut unsupported = UnsupportedFeatures::default();
    let shared_stores = parse_shared_stores(descriptor)?;
    let shared_store_ids = shared_stores.iter().map(|store| store.id).collect();
    let signals = parse_signals(
        descriptor,
        &shared_store_ids,
        authenticated_inbound_signals,
        &mut unsupported,
    )?;
    let shared_fields = signals
        .iter()
        .filter_map(|signal| signal.shared_field.clone().map(|field| (signal.id, field)))
        .collect::<BTreeMap<_, _>>();
    let callback_slots = parse_callback_slots(descriptor)?;
    let node_refs = parse_node_refs(descriptor)?;
    let node_ref_ids = node_refs.iter().map(|node_ref| node_ref.id).collect();

    let expressions = parse_expressions(descriptor, &shared_fields)?;
    let handlers = parse_handlers(descriptor, &shared_fields, &mut unsupported)?;
    if expressions
        .iter()
        .flat_map(|expression| &expression.client_operations)
        .chain(
            handlers
                .iter()
                .flat_map(|handler| &handler.client_operations),
        )
        .any(|operation| {
            matches!(
                &operation.kind,
                LegacyClientOperationKind::NodeRefRead { .. }
                    | LegacyClientOperationKind::NodeRefWrite { .. }
            ) && !matches!(operation.consumer, LegacyClientOperationConsumer::Enclosing)
        })
    {
        // Cross-kind operation nesting needs one unified canonical operation
        // preorder. Keep it closed until the adapter can preserve the exact
        // consumer instead of attaching the ref to the enclosing expression.
        unsupported.insert("unsupported_shapes");
    }
    let mut queries = parse_queries(descriptor, &mut unsupported)?;
    join_query_initializers(descriptor, &mut queries)?;
    let mutations = parse_mutations(descriptor, &mut unsupported)?;
    let view = parse_view_tree(descriptor, &mut unsupported)?;

    let signal_ids = signals.iter().map(|signal| signal.id).collect();
    let expression_ids = expressions.iter().map(|expression| expression.id).collect();
    let handler_ids = handlers.iter().map(|handler| handler.id).collect();
    let callback_slot_ids = callback_slots.iter().map(|slot| slot.id).collect();
    let query_ids = queries.iter().map(|query| query.id).collect();
    let mutation_ids = mutations.iter().map(|mutation| mutation.id).collect();

    let declarations = DeclaredIds {
        signals: &signal_ids,
        expressions: &expression_ids,
        handlers: &handler_ids,
        callback_slots: &callback_slot_ids,
        queries: &query_ids,
        mutations: &mutation_ids,
        node_refs: &node_ref_ids,
        helpers: reachable_helper_ordinals,
    };
    let keyed_collections = parse_keyed_collections(descriptor, &view, &declarations)?;

    for signal in &signals {
        if let Some(expression) = signal.derived_expression {
            require_declared(
                declarations.expressions,
                expression,
                &format!("signal {} derived expression", signal.id),
            )?;
        }
    }

    for expression in &expressions {
        validate_external_dependencies(
            &format!("expression {}", expression.id),
            &expression.dependencies,
            DependencyContext::Expression,
            &declarations,
            &mut unsupported,
        )?;
    }
    for handler in &handlers {
        validate_external_dependencies(
            &format!("handler {}", handler.id),
            &handler.dependencies,
            DependencyContext::EventHandler,
            &declarations,
            &mut unsupported,
        )?;
    }

    validate_query_refs(&queries, &declarations)?;
    validate_mutation_refs(&mutations, &declarations)?;
    validate_mutation_initializers(descriptor, &declarations)?;
    validate_intervals(descriptor, &declarations, &mut unsupported)?;
    let bindings = parse_bindings(descriptor, &view, &declarations, &mut unsupported)?;
    for signal in extra_signal_roots {
        require_declared(
            declarations.signals,
            *signal,
            "extra browser-visible signal root",
        )?;
    }
    let resource_reachability = collect_resource_reachability(
        &queries,
        &mutations,
        &signals,
        &expressions,
        &handlers,
        &bindings,
        &keyed_collections,
        extra_signal_roots,
    );
    let reachable_node_refs = collect_reachable_node_refs(
        &signals,
        &expressions,
        &handlers,
        &bindings,
        extra_signal_roots,
        &resource_reachability.expressions,
    );
    validate_payload_crossings(
        &view,
        &signals,
        &expressions,
        &handlers,
        &queries,
        &mutations,
        &bindings,
        &keyed_collections,
        &resource_reachability,
        &mut unsupported,
    )?;
    let bound_handlers = bindings
        .events
        .iter()
        .map(|binding| binding.handler)
        .collect::<BTreeSet<_>>();
    if expressions
        .iter()
        .filter(|expression| resource_reachability.expressions.contains(&expression.id))
        .flat_map(|expression| &expression.client_operations)
        .chain(
            handlers
                .iter()
                .filter(|handler| bound_handlers.contains(&handler.id))
                .flat_map(|handler| &handler.client_operations),
        )
        .any(|operation| {
            matches!(
                operation.kind,
                LegacyClientOperationKind::QueryRead { .. }
                    | LegacyClientOperationKind::QueryRefetch { .. }
                    | LegacyClientOperationKind::QueryInvalidate { .. }
                    | LegacyClientOperationKind::MutationRead { .. }
                    | LegacyClientOperationKind::MutationRun { .. }
            ) && !matches!(operation.consumer, LegacyClientOperationConsumer::Enclosing)
        })
    {
        // Canonical query/mutation lowering currently preserves direct reads
        // and handler operations. Cross-kind nesting needs a single operation
        // preorder shared with browser, node-ref, and SharedState effects.
        unsupported.insert("unsupported_shapes");
    }

    Ok(LegacyClientGraph {
        shared_stores,
        signals,
        expressions,
        handlers,
        callback_slots,
        queries,
        reachable_queries: resource_reachability.queries.into_iter().collect(),
        mutations,
        reachable_mutations: resource_reachability.mutations.into_iter().collect(),
        reachable_expressions: resource_reachability.expressions.into_iter().collect(),
        node_refs,
        reachable_node_refs,
        bindings,
        keyed_collections,
        node_count: u32::try_from(view.kinds.len())
            .map_err(|_| "view_tree.nodes exceeds u32::MAX".to_string())?,
        unsupported_features: unsupported.ordered(),
    })
}

fn validate_descriptor_shell(descriptor: &Map<String, Value>) -> Result<(), String> {
    require_known_fields(
        "component descriptor v9",
        descriptor,
        &[
            "asset_id",
            "bindings",
            "callback_slots",
            "client_ir_version",
            "client_origin_permission",
            "counts",
            "descriptor_hash",
            "exprs",
            "fields",
            "format",
            "handlers",
            "inbound_inputs",
            "intervals",
            "is_async",
            "js_hash",
            "keyed_collections",
            "mutation_initializers",
            "mutations",
            "node_refs",
            "queries",
            "query_initializers",
            "schema_version",
            "self_ty",
            "shared_stores",
            "shared_state_provider_sites",
            "signals",
            "struct_name",
            "view_tree",
        ],
    )?;
    require_exact_string(descriptor, "format", DESCRIPTOR_FORMAT)?;
    require_exact_u64(descriptor, "schema_version", DESCRIPTOR_SCHEMA)?;
    require_exact_u64(descriptor, "client_ir_version", CLIENT_IR_SCHEMA)?;
    for field in [
        "asset_id",
        "descriptor_hash",
        "js_hash",
        "self_ty",
        "struct_name",
    ] {
        required_string(descriptor, field)?;
    }
    match required_string(descriptor, "client_origin_permission")? {
        "participant_only" | "origin_capable" => {}
        other => {
            return Err(format!(
                "client_origin_permission has unsupported value `{other}`"
            ));
        }
    }
    required_bool(descriptor, "is_async")?;

    for field in [
        "callback_slots",
        "exprs",
        "fields",
        "handlers",
        "mutations",
        "node_refs",
        "queries",
        "shared_stores",
        "signals",
    ] {
        required_array(descriptor, field)?;
    }
    optional_array(descriptor, "shared_state_provider_sites")?;
    for field in [
        "intervals",
        "keyed_collections",
        "mutation_initializers",
        "query_initializers",
    ] {
        optional_array(descriptor, field)?;
    }
    required_object(descriptor, "bindings")?;
    required_object(descriptor, "view_tree")?;

    let inbound = required_object(descriptor, "inbound_inputs")?;
    require_known_fields(
        "component inbound_inputs",
        inbound,
        &[
            "crossings",
            "node_ref_fields",
            "projection_fields",
            "prop_sites",
            "values",
        ],
    )?;
    for field in [
        "crossings",
        "node_ref_fields",
        "projection_fields",
        "prop_sites",
        "values",
    ] {
        required_array(inbound, field)?;
    }

    validate_counts(descriptor)
}

fn validate_counts(descriptor: &Map<String, Value>) -> Result<(), String> {
    let counts = required_object(descriptor, "counts")?;
    require_known_fields(
        "component descriptor counts",
        counts,
        &[
            "callback_slots",
            "crossings",
            "exprs",
            "fields",
            "handlers",
            "keyed_collections",
            "node_ref_fields",
            "node_refs",
            "nodes",
            "projection_fields",
            "prop_sites",
            "shared_state_provider_sites",
            "signals",
            "values",
        ],
    )?;
    for (count, array) in [
        ("callback_slots", "callback_slots"),
        ("exprs", "exprs"),
        ("fields", "fields"),
        ("handlers", "handlers"),
        ("node_refs", "node_refs"),
        ("signals", "signals"),
    ] {
        validate_count(counts, count, required_array(descriptor, array)?.len())?;
    }
    if let Some(providers) = optional_array(descriptor, "shared_state_provider_sites")? {
        validate_count(counts, "shared_state_provider_sites", providers.len())?;
    } else if counts.contains_key("shared_state_provider_sites") {
        validate_count(counts, "shared_state_provider_sites", 0)?;
    }
    if let Some(collections) = optional_array(descriptor, "keyed_collections")? {
        validate_count(counts, "keyed_collections", collections.len())?;
    } else if counts.contains_key("keyed_collections") {
        validate_count(counts, "keyed_collections", 0)?;
    }
    let inbound = required_object(descriptor, "inbound_inputs")?;
    for (count, array) in [
        ("crossings", "crossings"),
        ("node_ref_fields", "node_ref_fields"),
        ("projection_fields", "projection_fields"),
        ("prop_sites", "prop_sites"),
        ("values", "values"),
    ] {
        validate_count(counts, count, required_array(inbound, array)?.len())?;
    }
    let view = required_object(descriptor, "view_tree")?;
    validate_count(counts, "nodes", required_array(view, "nodes")?.len())
}

fn validate_count(counts: &Map<String, Value>, field: &str, actual: usize) -> Result<(), String> {
    let expected = required_u64(counts, field)?;
    if expected == actual as u64 {
        Ok(())
    } else {
        Err(format!(
            "counts.{field} is {expected}, but the corresponding array has {actual} entries"
        ))
    }
}

fn parse_shared_stores(descriptor: &Map<String, Value>) -> Result<Vec<LegacySharedStore>, String> {
    let mut stores = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(descriptor, "shared_stores")?
        .iter()
        .enumerate()
    {
        let store = object_value(value, &format!("shared_stores[{index}]"))?;
        require_known_fields(
            "shared store",
            store,
            &["id", "rust_type", "source_anchor", "source_name"],
        )?;
        let id = required_u32(store, "id")?;
        require_increasing_id(&mut previous, id, "shared_stores")?;
        stores.push(LegacySharedStore {
            id,
            source_name: required_string(store, "source_name")?.to_owned(),
            rust_type: required_string(store, "rust_type")?.to_owned(),
            source_anchor: parse_optional_source_anchor(store, "source_anchor")?,
        });
    }
    Ok(stores)
}

fn parse_signals(
    descriptor: &Map<String, Value>,
    shared_store_ids: &BTreeSet<u32>,
    authenticated_inbound_signals: &BTreeSet<u32>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacySignal>, String> {
    let mut signals = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(descriptor, "signals")?.iter().enumerate() {
        let signal = object_value(value, &format!("signals[{index}]"))?;
        require_known_fields(
            "signal",
            signal,
            &[
                "derived",
                "id",
                "init",
                "integer_domain",
                "shared",
                "source_anchor",
                "source_name",
                "ty",
            ],
        )?;
        let id = required_u32(signal, "id")?;
        require_increasing_id(&mut previous, id, "signals")?;
        if signal.contains_key("source_name") {
            required_string(signal, "source_name")?;
        }
        validate_optional_source_anchor(signal, "source_anchor")?;
        validate_signal_initializer(
            signal
                .get("init")
                .ok_or_else(|| format!("signal {id} is missing `init`"))?,
            unsupported,
        )?;
        let _ = parse_canonical_client_type(
            signal
                .get("ty")
                .ok_or_else(|| format!("signal {id} is missing `ty`"))?,
            &format!("signal {id} type"),
            unsupported,
            authenticated_inbound_signals.contains(&id),
        )?;
        if let Some(domain) = signal.get("integer_domain") {
            match domain.as_str() {
                Some("i32" | "u32" | "i64" | "u64") => {}
                Some(other) => {
                    return Err(format!(
                        "signal {id} has unsupported integer_domain `{other}`"
                    ));
                }
                None => return Err(format!("signal {id} integer_domain must be a string")),
            }
        }
        let derived_expression = optional_u32(signal, "derived")?;
        let shared_field = match signal.get("shared") {
            None => None,
            Some(value) => {
                let shared = object_value(value, &format!("signal {id} shared field"))?;
                require_known_fields("signal shared field", shared, &["field", "store"])?;
                let store = required_u32(shared, "store")?;
                require_declared(
                    shared_store_ids,
                    store,
                    &format!("signal {id} shared store"),
                )?;
                let field = required_string(shared, "field")?.to_owned();
                unsupported.insert("shared_backed_signals");
                Some(LegacySharedFieldRef { store, field })
            }
        };
        signals.push(LegacySignal {
            id,
            derived_expression,
            shared: shared_field.is_some(),
            shared_field,
        });
    }
    Ok(signals)
}

fn validate_signal_initializer(
    value: &Value,
    unsupported: &mut UnsupportedFeatures,
) -> Result<(), String> {
    let init = object_value(value, "signal initializer")?;
    require_known_fields("signal initializer", init, &["kind", "value"])?;
    let kind = required_string(init, "kind")?;
    let value = init
        .get("value")
        .ok_or_else(|| "signal initializer is missing `value`".to_string())?;
    match kind {
        "bool" if value.is_boolean() => Ok(()),
        "int" if value.as_i64().is_some() => Ok(()),
        "bigint" => value
            .as_str()
            .ok_or_else(|| "bigint signal initializer must be a string".to_string())?
            .parse::<i128>()
            .map(|_| ())
            .map_err(|error| format!("invalid bigint signal initializer: {error}")),
        "float" if value.as_f64().is_some() => Ok(()),
        "float" => value
            .as_str()
            .ok_or_else(|| "float signal initializer must be a number or string".to_string())?
            .parse::<f64>()
            .map(|_| ())
            .map_err(|error| format!("invalid float signal initializer: {error}")),
        "string" if value.is_string() => Ok(()),
        "null" if value.is_null() => Ok(()),
        "unknown" if value.is_null() => {
            unsupported.insert("unsupported_shapes");
            Ok(())
        }
        "bool" | "int" | "string" | "null" | "unknown" => Err(format!(
            "signal initializer kind `{kind}` has an invalid `value`"
        )),
        other => Err(format!("unsupported signal initializer kind `{other}`")),
    }
}

fn parse_canonical_client_type(
    value: &Value,
    label: &str,
    unsupported: &mut UnsupportedFeatures,
    allow_unknown: bool,
) -> Result<ClientType, String> {
    let client_type = parse_client_type_json(value).map_err(|error| format!("{label}: {error}"))?;
    let mut canonical = String::new();
    write_client_type_json(&mut canonical, &client_type);
    let canonical = serde_json::from_str::<Value>(&canonical)
        .map_err(|error| format!("{label}: canonical JSON was invalid: {error}"))?;
    if canonical != *value {
        return Err(format!("{label} is not canonical Client IR v3 JSON"));
    }
    if !allow_unknown && value.get("kind").and_then(Value::as_str) == Some("unknown") {
        unsupported.insert("unsupported_shapes");
    }
    Ok(client_type)
}

fn parse_callback_slots(
    descriptor: &Map<String, Value>,
) -> Result<Vec<LegacyCallbackSlot>, String> {
    let mut slots = Vec::new();
    let mut previous = None;
    let mut names = BTreeSet::new();
    for (index, value) in required_array(descriptor, "callback_slots")?
        .iter()
        .enumerate()
    {
        let slot = object_value(value, &format!("callback_slots[{index}]"))?;
        require_known_fields("callback slot", slot, &["id", "name"])?;
        let id = required_u32(slot, "id")?;
        require_increasing_id(&mut previous, id, "callback_slots")?;
        let name = required_string(slot, "name")?.to_owned();
        if !names.insert(name.clone()) {
            return Err(format!(
                "callback slot name `{name}` is declared more than once"
            ));
        }
        slots.push(LegacyCallbackSlot { id, name });
    }
    Ok(slots)
}

fn parse_node_refs(descriptor: &Map<String, Value>) -> Result<Vec<LegacyNodeRef>, String> {
    let mut node_refs = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(descriptor, "node_refs")?.iter().enumerate() {
        let node_ref = object_value(value, &format!("node_refs[{index}]"))?;
        require_known_fields(
            "node ref",
            node_ref,
            &["id", "source_anchor", "source_name"],
        )?;
        let id = required_u32(node_ref, "id")?;
        require_increasing_id(&mut previous, id, "node_refs")?;
        let source_name = node_ref
            .contains_key("source_name")
            .then(|| required_string(node_ref, "source_name").map(str::to_owned))
            .transpose()?;
        let source_anchor = parse_optional_source_anchor(node_ref, "source_anchor")?;
        node_refs.push(LegacyNodeRef {
            id,
            source_name,
            source_anchor,
        });
    }
    Ok(node_refs)
}

fn parse_expressions(
    descriptor: &Map<String, Value>,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
) -> Result<Vec<LegacyExpression>, String> {
    let mut expressions = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(descriptor, "exprs")?.iter().enumerate() {
        let record = object_value(value, &format!("exprs[{index}]"))?;
        require_known_fields("expression record", record, &["expr", "id"])?;
        let id = required_u32(record, "id")?;
        require_increasing_id(&mut previous, id, "exprs")?;
        let expression = parse_canonical_expression(
            record
                .get("expr")
                .ok_or_else(|| format!("expression {id} is missing `expr`"))?,
            &format!("expression {id}"),
        )?;
        let dependencies = collect_client_expr_external_dependencies(&expression)
            .map_err(|error| format!("expression {id} is not dependency-closed: {error:?}"))?;
        let browser_operations = collect_expression_browser_operations(&expression)
            .map_err(|error| format!("expression {id} browser operations: {error}"))?;
        let client_operations = collect_expression_client_operations(&expression, shared_fields)
            .map_err(|error| format!("expression {id} client operations: {error}"))?;
        expressions.push(LegacyExpression {
            id,
            expression,
            dependencies,
            browser_operations,
            client_operations,
        });
    }
    Ok(expressions)
}

fn parse_handlers(
    descriptor: &Map<String, Value>,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyBoundHandler>, String> {
    let mut handlers = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(descriptor, "handlers")?.iter().enumerate() {
        let record = object_value(value, &format!("handlers[{index}]"))?;
        require_known_fields(
            "handler",
            record,
            &[
                "body_ir",
                "body_ir_version",
                "debounce_ms",
                "id",
                "source_anchor",
                "source_name",
            ],
        )?;
        let id = required_u32(record, "id")?;
        require_increasing_id(&mut previous, id, "handlers")?;
        require_exact_u64(record, "body_ir_version", CLIENT_IR_SCHEMA)?;
        if record.contains_key("source_name") {
            required_string(record, "source_name")?;
        }
        validate_optional_source_anchor(record, "source_anchor")?;
        let debounce_ms = optional_u64(record, "debounce_ms")?;
        if debounce_ms.is_some() {
            unsupported.insert("unsupported_shapes");
        }
        let body = parse_canonical_block(
            record
                .get("body_ir")
                .ok_or_else(|| format!("handler {id} is missing `body_ir`"))?,
            &format!("handler {id}"),
        )?;
        let dependencies = collect_client_block_external_dependencies(&body)
            .map_err(|error| format!("handler {id} is not dependency-closed: {error:?}"))?;
        let browser_operations = collect_handler_browser_operations(&body)
            .map_err(|error| format!("handler {id} browser operations: {error}"))?;
        let client_operations = collect_handler_client_operations(&body, shared_fields)
            .map_err(|error| format!("handler {id} client operations: {error}"))?;
        handlers.push(LegacyBoundHandler {
            id,
            debounce_ms,
            body,
            dependencies,
            browser_operations,
            client_operations,
        });
    }
    Ok(handlers)
}

fn parse_queries(
    descriptor: &Map<String, Value>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyQuery>, String> {
    let values = required_array(descriptor, "queries")?;
    let mut queries = Vec::new();
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let query = object_value(value, &format!("queries[{index}]"))?;
        require_known_fields(
            "query",
            query,
            &[
                "client_fetcher",
                "data_ty",
                "id",
                "key_expr",
                "options",
                "server_initial",
                "source_anchor",
                "source_name",
            ],
        )?;
        let id = required_u32(query, "id")?;
        require_increasing_id(&mut previous, id, "queries")?;
        let source_name = query
            .contains_key("source_name")
            .then(|| required_string(query, "source_name").map(str::to_owned))
            .transpose()?;
        let source_anchor = parse_optional_source_anchor(query, "source_anchor")?;
        let data_type = parse_canonical_client_type(
            query
                .get("data_ty")
                .ok_or_else(|| format!("query {id} is missing `data_ty`"))?,
            &format!("query {id} data_ty"),
            unsupported,
            false,
        )?;
        let options = parse_query_options(required_object(query, "options")?, id)?;
        queries.push(LegacyQuery {
            id,
            source_name,
            source_anchor,
            key_expression: required_u32(query, "key_expr")?,
            server_initial_expression: required_u32(query, "server_initial")?,
            client_fetcher_expression: required_u32(query, "client_fetcher")?,
            data_type,
            options,
            initializer: None,
        });
    }
    Ok(queries)
}

fn parse_query_options(
    options: &Map<String, Value>,
    id: u32,
) -> Result<LegacyQueryOptions, String> {
    require_known_fields(
        "query options",
        options,
        &[
            "download_progress",
            "interval_ms",
            "refetch_on_focus",
            "retry_count",
            "stale_time_ms",
            "stream_text",
        ],
    )?;
    let interval_ms = match options
        .get("interval_ms")
        .ok_or_else(|| format!("query {id} options are missing `interval_ms`"))?
    {
        Value::Null => None,
        value => Some(as_u32(value, &format!("query {id} interval_ms"))?),
    };
    Ok(LegacyQueryOptions {
        interval_ms,
        refetch_on_focus: required_bool(options, "refetch_on_focus")?,
        download_progress: required_bool(options, "download_progress")?,
        stream_text: required_bool(options, "stream_text")?,
        retry_count: required_u32(options, "retry_count")?,
        stale_time_ms: required_u32(options, "stale_time_ms")?,
    })
}

fn parse_mutations(
    descriptor: &Map<String, Value>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyMutation>, String> {
    let values = required_array(descriptor, "mutations")?;
    let mut mutations = Vec::new();
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let mutation = object_value(value, &format!("mutations[{index}]"))?;
        require_known_fields(
            "mutation",
            mutation,
            &[
                "client_runner",
                "id",
                "invalidates",
                "key_expr",
                "options",
                "server_runner",
                "source_anchor",
                "source_name",
            ],
        )?;
        let id = required_u32(mutation, "id")?;
        require_increasing_id(&mut previous, id, "mutations")?;
        let source_name = mutation
            .contains_key("source_name")
            .then(|| required_string(mutation, "source_name").map(str::to_owned))
            .transpose()?;
        let source_anchor = parse_optional_source_anchor(mutation, "source_anchor")?;
        let options =
            validate_mutation_options(required_object(mutation, "options")?, id, unsupported)?;
        let invalidates = required_array(mutation, "invalidates")?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                as_u32(
                    value,
                    &format!("mutation {id} invalidation expression at index {index}"),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for expression in &invalidates {
            if !seen.insert(*expression) {
                return Err(format!(
                    "mutation {id} repeats invalidation expression {expression}"
                ));
            }
        }
        mutations.push(LegacyMutation {
            id,
            source_name,
            source_anchor,
            key_expression: required_u32(mutation, "key_expr")?,
            server_runner_expression: required_u32(mutation, "server_runner")?,
            client_runner_expression: required_u32(mutation, "client_runner")?,
            invalidates,
            options,
        });
    }
    Ok(mutations)
}

fn validate_mutation_options(
    options: &Map<String, Value>,
    id: u32,
    unsupported: &mut UnsupportedFeatures,
) -> Result<LegacyMutationOptions, String> {
    require_known_fields(
        "mutation options",
        options,
        &["on_success_invalidate_strategy", "retry_count"],
    )?;
    let retry_count = required_u32(options, "retry_count")?;
    let on_success_invalidate_strategy =
        match required_string(options, "on_success_invalidate_strategy")? {
            "refetch" => LegacyInvalidationStrategy::Refetch,
            "reset" => LegacyInvalidationStrategy::Reset,
            "unknown" => {
                unsupported.insert("unsupported_shapes");
                // Preserve validation progress while the enclosing unsupported
                // report keeps this descriptor out of canonical linking.
                LegacyInvalidationStrategy::Refetch
            }
            other => {
                return Err(format!(
                    "mutation {id} has unsupported invalidation strategy `{other}`"
                ));
            }
        };
    Ok(LegacyMutationOptions {
        retry_count,
        on_success_invalidate_strategy,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewNodeKind {
    Element,
    TextBindingSlot,
    StaticText,
    ServerOnly,
    ForLoop,
    Unknown,
}

#[derive(Clone, Debug)]
struct ValidatedView {
    kinds: Vec<ViewNodeKind>,
    children: Vec<Vec<u32>>,
    payload_attrs: Vec<BTreeMap<String, String>>,
    inside_for_loop: Vec<bool>,
}

fn parse_view_tree(
    descriptor: &Map<String, Value>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<ValidatedView, String> {
    let tree = required_object(descriptor, "view_tree")?;
    require_known_fields("view_tree", tree, &["nodes", "root"])?;
    let nodes = required_array(tree, "nodes")?;
    u32::try_from(nodes.len()).map_err(|_| "view_tree.nodes exceeds u32::MAX".to_string())?;
    let mut kinds = Vec::with_capacity(nodes.len());
    let mut child_lists = Vec::with_capacity(nodes.len());
    let mut payload_attrs = Vec::with_capacity(nodes.len());

    for (expected, value) in nodes.iter().enumerate() {
        let record = object_value(value, &format!("view_tree.nodes[{expected}]"))?;
        require_known_fields("view node record", record, &["id", "node"])?;
        let id = required_u32(record, "id")?;
        if id != expected as u32 {
            return Err(format!(
                "view_tree node IDs must be dense; expected {expected}, found {id}"
            ));
        }
        let node = required_object(record, "node")?;
        let (kind, children, node_payload_attrs) = parse_view_node(node, id, unsupported)?;
        kinds.push(kind);
        child_lists.push(children);
        payload_attrs.push(node_payload_attrs);
    }

    for (node, children) in child_lists.iter().enumerate() {
        for child in children {
            validate_node_bound(*child, kinds.len(), &format!("view node {node} child"))?;
        }
    }

    let root = match tree
        .get("root")
        .ok_or_else(|| "view_tree is missing `root`".to_string())?
    {
        Value::Null if kinds.is_empty() => None,
        Value::Null => return Err("view_tree.root is null but nodes are present".to_string()),
        value => {
            let root = as_u32(value, "view_tree.root")?;
            validate_node_bound(root, kinds.len(), "view_tree.root")?;
            Some(root)
        }
    };
    validate_view_topology(&child_lists, root)?;

    let mut inside_for_loop = vec![false; kinds.len()];
    if let Some(root) = root {
        let mut stack = vec![(root, false)];
        while let Some((node, inherited_loop)) = stack.pop() {
            let node_index = node as usize;
            let in_loop = inherited_loop || kinds[node_index] == ViewNodeKind::ForLoop;
            inside_for_loop[node_index] = in_loop;
            stack.extend(
                child_lists[node_index]
                    .iter()
                    .copied()
                    .map(|child| (child, in_loop)),
            );
        }
    }

    Ok(ValidatedView {
        kinds,
        children: child_lists,
        payload_attrs,
        inside_for_loop,
    })
}

fn parse_keyed_collections(
    descriptor: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
) -> Result<Vec<LegacyKeyedCollection>, String> {
    let records = optional_array(descriptor, "keyed_collections")?.unwrap_or_default();
    let tree = required_object(descriptor, "view_tree")?;
    let nodes = required_array(tree, "nodes")?;
    let mut marker_nodes = BTreeSet::new();
    for (node_id, record) in nodes.iter().enumerate() {
        let Some(node) = record.get("node").and_then(Value::as_object) else {
            continue;
        };
        let Some(attrs) = node.get("static_attrs").and_then(Value::as_array) else {
            continue;
        };
        for attr in attrs {
            let Some(attr) = attr.as_object() else {
                continue;
            };
            if attr.get("name").and_then(Value::as_str) != Some("data-rsjs-list") {
                continue;
            }
            let expected = node_id as u32;
            let actual = attr
                .get("value")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("view node {node_id} has a non-string data-rsjs-list marker")
                })?
                .parse::<u32>()
                .map_err(|_| format!("view node {node_id} has an invalid data-rsjs-list marker"))?;
            if actual != expected {
                return Err(format!(
                    "view node {node_id} has data-rsjs-list=\"{actual}\"; expected \"{expected}\""
                ));
            }
            if !marker_nodes.insert(expected) {
                return Err(format!(
                    "view node {node_id} declares data-rsjs-list more than once"
                ));
            }
        }
    }

    let mut collections = Vec::with_capacity(records.len());
    let mut declared_markers = BTreeSet::new();
    let mut claimed_loop_nodes = BTreeSet::new();
    let mut claimed_item_roots = BTreeSet::new();
    for (index, value) in records.iter().enumerate() {
        let record = object_value(value, &format!("keyed_collections[{index}]"))?;
        require_known_fields(
            "keyed collection",
            record,
            &[
                "container",
                "id",
                "item",
                "item_root",
                "key",
                "key_tokens",
                "loop_node",
                "source",
            ],
        )?;
        let id = required_u32(record, "id")?;
        if id != index as u32 {
            return Err(format!(
                "keyed collection IDs must be dense; expected {index}, found {id}"
            ));
        }
        let container = required_u32(record, "container")?;
        let loop_node = required_u32(record, "loop_node")?;
        let item_root = required_u32(record, "item_root")?;
        validate_node_bound(container, view.kinds.len(), "keyed collection container")?;
        validate_node_bound(loop_node, view.kinds.len(), "keyed collection loop node")?;
        validate_node_bound(item_root, view.kinds.len(), "keyed collection item root")?;
        if view.kinds[container as usize] != ViewNodeKind::Element
            || view.kinds[loop_node as usize] != ViewNodeKind::ForLoop
            || view.kinds[item_root as usize] != ViewNodeKind::Element
        {
            return Err(format!(
                "keyed collection {id} must reference an element container, for-loop node, and element item root"
            ));
        }
        if view.inside_for_loop[container as usize] {
            return Err(format!(
                "keyed collection {id} container node {container} is nested inside another for-loop"
            ));
        }
        if view.children[container as usize].as_slice() != [loop_node]
            || view.children[loop_node as usize].as_slice() != [item_root]
        {
            return Err(format!(
                "keyed collection {id} must be the container's only child and render exactly one item root"
            ));
        }
        if !marker_nodes.contains(&container) || !declared_markers.insert(container) {
            return Err(format!(
                "keyed collection {id} has a missing or duplicate data-rsjs-list container marker"
            ));
        }
        if !claimed_loop_nodes.insert(loop_node) || !claimed_item_roots.insert(item_root) {
            return Err(format!(
                "keyed collection {id} reuses another collection's loop or item-root node"
            ));
        }

        let source = required_object(record, "source")?;
        require_known_fields("keyed collection source", source, &["id", "kind"])?;
        let source_id = required_u32(source, "id")?;
        let source = match required_string(source, "kind")? {
            "signal" => {
                require_declared(declarations.signals, source_id, "keyed collection source")?;
                LegacyKeyedCollectionSource::Signal(source_id)
            }
            "expression" => {
                require_declared(
                    declarations.expressions,
                    source_id,
                    "keyed collection source",
                )?;
                LegacyKeyedCollectionSource::Expression(source_id)
            }
            other => {
                return Err(format!(
                    "keyed collection {id} has unsupported source kind `{other}`"
                ));
            }
        };
        validate_list_item_expr(
            record
                .get("key")
                .ok_or_else(|| "keyed collection is missing `key`".to_owned())?,
            "keyed collection key",
        )?;
        required_string(record, "key_tokens")?;
        validate_list_template_node(
            record
                .get("item")
                .ok_or_else(|| "keyed collection is missing `item`".to_owned())?,
        )?;
        collections.push(LegacyKeyedCollection {
            id,
            container,
            loop_node,
            item_root,
            source,
        });
    }
    if marker_nodes != declared_markers {
        return Err(format!(
            "view tree declares {} keyed-list marker(s), but keyed_collections describes {}",
            marker_nodes.len(),
            declared_markers.len()
        ));
    }
    Ok(collections)
}

fn validate_list_item_expr(value: &Value, label: &str) -> Result<(), String> {
    let expression = object_value(value, label)?;
    match required_string(expression, "kind")? {
        "item" => require_known_fields(label, expression, &["kind"]),
        "field" => {
            require_known_fields(label, expression, &["kind", "name"])?;
            if required_string(expression, "name")?.is_empty() {
                return Err(format!("{label} field name cannot be empty"));
            }
            Ok(())
        }
        other => Err(format!("{label} has unsupported kind `{other}`")),
    }
}

fn validate_list_template_node(value: &Value) -> Result<(), String> {
    let node = object_value(value, "keyed collection item node")?;
    match required_string(node, "kind")? {
        "element" => {
            require_known_fields(
                "keyed collection item element",
                node,
                &["children", "dynamic_attrs", "kind", "static_attrs", "tag"],
            )?;
            if required_string(node, "tag")?.is_empty() {
                return Err("keyed collection item tag cannot be empty".to_owned());
            }
            for attr in required_array(node, "static_attrs")? {
                let attr = object_value(attr, "keyed collection static attribute")?;
                require_known_fields(
                    "keyed collection static attribute",
                    attr,
                    &["name", "value"],
                )?;
                required_string(attr, "name")?;
                required_string(attr, "value")?;
            }
            for attr in required_array(node, "dynamic_attrs")? {
                let attr = object_value(attr, "keyed collection dynamic attribute")?;
                require_known_fields(
                    "keyed collection dynamic attribute",
                    attr,
                    &["name", "value"],
                )?;
                required_string(attr, "name")?;
                validate_list_item_expr(
                    attr.get("value").ok_or_else(|| {
                        "keyed collection dynamic attribute is missing `value`".to_owned()
                    })?,
                    "keyed collection dynamic attribute value",
                )?;
            }
            for child in required_array(node, "children")? {
                validate_list_template_node(child)?;
            }
            Ok(())
        }
        "text" => {
            require_known_fields("keyed collection text node", node, &["kind", "text"])?;
            required_string(node, "text")?;
            Ok(())
        }
        "slot" => {
            require_known_fields("keyed collection slot node", node, &["kind", "value"])?;
            validate_list_item_expr(
                node.get("value")
                    .ok_or_else(|| "keyed collection slot is missing `value`".to_owned())?,
                "keyed collection slot value",
            )
        }
        other => Err(format!(
            "keyed collection item node has unsupported kind `{other}`"
        )),
    }
}

fn validate_view_topology(child_lists: &[Vec<u32>], root: Option<u32>) -> Result<(), String> {
    if child_lists.is_empty() {
        debug_assert!(root.is_none());
        return Ok(());
    }
    let root = root.expect("non-empty view tree has a validated root");
    let root_index = root as usize;
    let mut parents = vec![None; child_lists.len()];

    for (parent, children) in child_lists.iter().enumerate() {
        let mut siblings = BTreeSet::new();
        for &child in children {
            let child_index = child as usize;
            if child_index == parent {
                return Err(format!("view node {parent} has a self-cycle"));
            }
            if !siblings.insert(child) {
                return Err(format!(
                    "view node {parent} references child {child} more than once"
                ));
            }
            if let Some(previous_parent) = parents[child_index] {
                return Err(format!(
                    "view node {child} has multiple parents {previous_parent} and {parent}"
                ));
            }
            parents[child_index] = Some(parent);
        }
    }

    if let Some(parent) = parents[root_index] {
        return Err(format!(
            "view_tree.root {root} is also a child of view node {parent}"
        ));
    }

    // Validate the entire declared graph, not only the selected root. A
    // disconnected cycle must be diagnosed as a cycle rather than hidden
    // behind the later reachability failure.
    let mut states = vec![0_u8; child_lists.len()];
    for start in 0..child_lists.len() {
        if states[start] != 0 {
            continue;
        }
        states[start] = 1;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next_child)) = stack.last_mut() {
            if *next_child == child_lists[*node].len() {
                states[*node] = 2;
                stack.pop();
                continue;
            }
            let child = child_lists[*node][*next_child] as usize;
            *next_child += 1;
            match states[child] {
                0 => {
                    states[child] = 1;
                    stack.push((child, 0));
                }
                1 => {
                    return Err(format!(
                        "view_tree contains a cycle through nodes {node} and {child}"
                    ));
                }
                2 => {}
                _ => unreachable!("view topology state is internal"),
            }
        }
    }

    let mut reachable = vec![false; child_lists.len()];
    reachable[root_index] = true;
    let mut stack = vec![root_index];
    while let Some(node) = stack.pop() {
        for &child in &child_lists[node] {
            let child = child as usize;
            if !reachable[child] {
                reachable[child] = true;
                stack.push(child);
            }
        }
    }
    if let Some(unreachable) = reachable.iter().position(|reachable| !reachable) {
        return Err(format!(
            "view node {unreachable} is unreachable from view_tree.root {root}"
        ));
    }

    Ok(())
}

fn parse_view_node(
    node: &Map<String, Value>,
    id: u32,
    unsupported: &mut UnsupportedFeatures,
) -> Result<(ViewNodeKind, Vec<u32>, BTreeMap<String, String>), String> {
    match required_string(node, "kind")? {
        "element" => {
            require_known_fields(
                "element view node",
                node,
                &[
                    "children",
                    "kind",
                    "payload_attrs",
                    "server_attrs",
                    "static_attrs",
                    "tag",
                ],
            )?;
            required_string(node, "tag")?;
            let children = parse_node_ids(required_array(node, "children")?, "element children")?;
            let static_attrs = validate_static_attrs(required_array(node, "static_attrs")?)?;
            let server_attrs = validate_server_attrs(required_array(node, "server_attrs")?)?;
            let payload_attrs = validate_payload_attrs(
                required_array(node, "payload_attrs")?,
                &static_attrs,
                &server_attrs,
                id,
            )?;
            Ok((ViewNodeKind::Element, children, payload_attrs))
        }
        "text_binding_slot" => {
            require_known_fields("text binding view node", node, &["kind"])?;
            Ok((ViewNodeKind::TextBindingSlot, Vec::new(), BTreeMap::new()))
        }
        "static_text" => {
            require_known_fields("static text view node", node, &["kind", "text"])?;
            required_string(node, "text")?;
            Ok((ViewNodeKind::StaticText, Vec::new(), BTreeMap::new()))
        }
        "server_only" => {
            require_known_fields("server-only view node", node, &["kind", "source", "tokens"])?;
            required_string(node, "source")?;
            required_string(node, "tokens")?;
            Ok((ViewNodeKind::ServerOnly, Vec::new(), BTreeMap::new()))
        }
        "for_loop" => {
            require_known_fields(
                "for-loop view node",
                node,
                &[
                    "attrs",
                    "body_prefix",
                    "children",
                    "expr",
                    "kind",
                    "label",
                    "pat",
                ],
            )?;
            for field in ["attrs", "body_prefix", "expr", "label", "pat"] {
                required_string(node, field)?;
            }
            Ok((
                ViewNodeKind::ForLoop,
                parse_node_ids(
                    required_array(node, "children")?,
                    &format!("for-loop node {id} children"),
                )?,
                BTreeMap::new(),
            ))
        }
        "unknown" => {
            require_known_fields("unknown view node", node, &["kind"])?;
            unsupported.insert("unsupported_shapes");
            Ok((ViewNodeKind::Unknown, Vec::new(), BTreeMap::new()))
        }
        other => Err(format!("view node {id} has unsupported kind `{other}`")),
    }
}

fn validate_static_attrs(values: &[Value]) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let attr = object_value(value, &format!("static_attrs[{index}]"))?;
        require_known_fields("static attribute", attr, &["name", "value"])?;
        names.insert(required_string(attr, "name")?.to_owned());
        required_string(attr, "value")?;
    }
    Ok(names)
}

fn validate_server_attrs(values: &[Value]) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let attr = object_value(value, &format!("server_attrs[{index}]"))?;
        require_known_fields("server attribute", attr, &["name", "source", "tokens"])?;
        names.insert(required_string(attr, "name")?.to_owned());
        required_string(attr, "source")?;
        required_string(attr, "tokens")?;
    }
    Ok(names)
}

fn validate_payload_attrs(
    values: &[Value],
    static_attrs: &BTreeSet<String>,
    server_attrs: &BTreeSet<String>,
    node: u32,
) -> Result<BTreeMap<String, String>, String> {
    let mut payload_attrs = BTreeMap::new();
    for (index, value) in values.iter().enumerate() {
        let attr = object_value(value, &format!("payload_attrs[{index}]"))?;
        require_known_fields(
            "payload attribute",
            attr,
            &["label", "name", "source_anchor", "tokens"],
        )?;
        let name = required_string(attr, "name")?;
        if !is_canonical_payload_attr_name(name) {
            return Err(format!(
                "view node {node} payload attribute `{name}` is not a canonical compiler-owned payload name"
            ));
        }
        if static_attrs.contains(name) || server_attrs.contains(name) {
            return Err(format!(
                "view node {node} payload attribute `{name}` collides with another attribute declaration"
            ));
        }
        let tokens = required_string(attr, "tokens")?;
        if tokens.is_empty() {
            return Err(format!(
                "view node {node} payload attribute `{name}` has empty source tokens"
            ));
        }
        if payload_attrs
            .insert(name.to_owned(), tokens.to_owned())
            .is_some()
        {
            return Err(format!(
                "view node {node} declares payload attribute `{name}` more than once"
            ));
        }
        if attr.contains_key("label") {
            required_string(attr, "label")?;
        }
        validate_optional_source_anchor(attr, "source_anchor")?;
    }
    Ok(payload_attrs)
}

fn is_canonical_payload_attr_name(name: &str) -> bool {
    let Some(index) = name.strip_prefix("data-rsjs-payload-p") else {
        return false;
    };
    !index.is_empty()
        && index
            .parse::<u32>()
            .is_ok_and(|parsed| parsed.to_string() == index)
}

fn parse_bindings(
    descriptor: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<LegacyDomBindings, String> {
    let bindings = required_object(descriptor, "bindings")?;
    require_known_fields(
        "bindings",
        bindings,
        &["attrs", "conditionals", "events", "inputs", "refs", "text"],
    )?;
    let text = parse_text_bindings(bindings, view, declarations)?;
    let attributes = parse_attribute_bindings(bindings, view, declarations, unsupported)?;
    let inputs = parse_input_bindings(bindings, view, declarations, unsupported)?;
    let events = parse_event_bindings(bindings, view, declarations, unsupported)?;
    let conditionals = parse_conditionals(bindings, view, declarations)?;
    let node_refs = parse_ref_bindings(bindings, view, declarations)?;
    Ok(LegacyDomBindings {
        text,
        attributes,
        inputs,
        events,
        conditionals,
        node_refs,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_payload_crossings(
    view: &ValidatedView,
    signals: &[LegacySignal],
    expressions: &[LegacyExpression],
    handlers: &[LegacyBoundHandler],
    queries: &[LegacyQuery],
    mutations: &[LegacyMutation],
    bindings: &LegacyDomBindings,
    keyed_collections: &[LegacyKeyedCollection],
    reachability: &ResourceReachability,
    unsupported: &mut UnsupportedFeatures,
) -> Result<(), String> {
    for binding in &bindings.attributes {
        if view.payload_attrs[binding.node as usize].contains_key(&binding.name) {
            return Err(format!(
                "attribute binding `{}` on node {} collides with a compiler-owned payload attribute",
                binding.name, binding.node
            ));
        }
    }

    let mut direct_expression_nodes = BTreeMap::<u32, BTreeSet<u32>>::new();
    for binding in &bindings.text {
        direct_expression_nodes
            .entry(binding.expression)
            .or_default()
            .insert(binding.node);
    }
    for binding in &bindings.attributes {
        direct_expression_nodes
            .entry(binding.expression)
            .or_default()
            .insert(binding.node);
    }
    for binding in &bindings.conditionals {
        direct_expression_nodes
            .entry(binding.condition)
            .or_default()
            .insert(binding.parent);
    }

    let mut indirect_expression_consumers = BTreeSet::new();
    indirect_expression_consumers.extend(signals.iter().filter_map(|signal| {
        reachability
            .signals
            .contains(&signal.id)
            .then_some(signal.derived_expression)
            .flatten()
    }));
    for query in queries
        .iter()
        .filter(|query| reachability.queries.contains(&query.id))
    {
        indirect_expression_consumers.extend([
            query.key_expression,
            query.server_initial_expression,
            query.client_fetcher_expression,
        ]);
    }
    for mutation in mutations
        .iter()
        .filter(|mutation| reachability.mutations.contains(&mutation.id))
    {
        indirect_expression_consumers.extend([
            mutation.key_expression,
            mutation.server_runner_expression,
            mutation.client_runner_expression,
        ]);
        indirect_expression_consumers.extend(mutation.invalidates.iter().copied());
    }
    indirect_expression_consumers.extend(keyed_collections.iter().filter_map(|collection| {
        match collection.source {
            LegacyKeyedCollectionSource::Expression(expression) => Some(expression),
            LegacyKeyedCollectionSource::Signal(_) => None,
        }
    }));
    let mut keyed_collection_nodes = BTreeSet::new();
    for collection in keyed_collections {
        let mut stack = vec![collection.loop_node];
        while let Some(node) = stack.pop() {
            if keyed_collection_nodes.insert(node) {
                stack.extend(view.children[node as usize].iter().copied());
            }
        }
    }

    for expression in expressions.iter().filter(|expression| {
        expression.dependencies.uses_node_payload
            && reachability.expressions.contains(&expression.id)
    }) {
        let mut reads = BTreeSet::new();
        walk_client_expr(&expression.expression, &mut |nested| {
            if let ClientExpr::NodePayload { attr, tokens } = nested {
                reads.insert((attr.clone(), tokens.clone()));
            }
        });
        if reads.is_empty() {
            return Err(format!(
                "expression {} reports a node-payload dependency without a node-payload read",
                expression.id
            ));
        }

        let targets = direct_expression_nodes.get(&expression.id);
        if targets.is_none() || indirect_expression_consumers.contains(&expression.id) {
            // A node payload is contextual: evaluating it as a signal seed,
            // resource input, keyed source, or other node-less value would
            // lower to `null`. Keep valid-but-unrepresentable legacy shapes
            // closed without rejecting unrelated latent declarations.
            unsupported.insert("unsupported_shapes");
        }
        let Some(targets) = targets else {
            continue;
        };

        for &node in targets {
            require_node_kind(view, node, ViewNodeKind::Element, "node-payload expression")?;
            if !view.inside_for_loop[node as usize] {
                return Err(format!(
                    "node-payload expression {} is bound to node {node} outside a repeated for-loop",
                    expression.id
                ));
            }
            if keyed_collection_nodes.contains(&node) {
                return Err(format!(
                    "node-payload expression {} is bound inside keyed collection node {node}, which has no stable node-payload context",
                    expression.id
                ));
            }
            let declarations = &view.payload_attrs[node as usize];
            for (attr, tokens) in &reads {
                let declared_tokens = declarations.get(attr).ok_or_else(|| {
                    format!(
                        "node-payload expression {} reads `{attr}` on node {node}, but that node does not declare it",
                        expression.id
                    )
                })?;
                if declared_tokens != tokens {
                    return Err(format!(
                        "node-payload expression {} tokens for `{attr}` do not match node {node}'s payload declaration",
                        expression.id
                    ));
                }
            }
        }
    }

    let mut event_nodes_by_handler = BTreeMap::<u32, BTreeSet<u32>>::new();
    for binding in &bindings.events {
        event_nodes_by_handler
            .entry(binding.handler)
            .or_default()
            .insert(binding.node);
    }
    for handler in handlers {
        let mut attrs = BTreeSet::new();
        walk_client_block(&handler.body, &mut |nested| {
            if let ClientExpr::EventPayload { attr } = nested {
                attrs.insert(attr.clone());
            }
        });
        if attrs.is_empty() {
            // The dependency summary deliberately groups ordinary EventRead,
            // EventCollection, EventAttribute, and compiler payload reads.
            // Only the last form needs a node-local payload declaration.
            continue;
        }
        let Some(nodes) = event_nodes_by_handler.get(&handler.id) else {
            // Unbound handlers are latent descriptor metadata.
            continue;
        };
        for &node in nodes {
            let declarations = &view.payload_attrs[node as usize];
            for attr in &attrs {
                if !declarations.contains_key(attr) {
                    return Err(format!(
                        "handler {} reads event payload `{attr}` on node {node}, but that node does not declare it",
                        handler.id
                    ));
                }
            }
        }
    }

    Ok(())
}

fn parse_text_bindings(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
) -> Result<Vec<LegacyTextBinding>, String> {
    let mut out = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(bindings, "text")?.iter().enumerate() {
        let binding = object_value(value, &format!("bindings.text[{index}]"))?;
        require_known_fields("text binding", binding, &["expr", "node"])?;
        let node = required_u32(binding, "node")?;
        require_increasing_id(&mut previous, node, "bindings.text nodes")?;
        require_node_kind_one_of(
            view,
            node,
            &[ViewNodeKind::Element, ViewNodeKind::TextBindingSlot],
            "text binding",
        )?;
        let expression = required_u32(binding, "expr")?;
        require_declared(
            declarations.expressions,
            expression,
            &format!("text binding on node {node} expression"),
        )?;
        out.push(LegacyTextBinding { node, expression });
    }
    Ok(out)
}

fn parse_attribute_bindings(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyAttributeBinding>, String> {
    let mut out = Vec::new();
    let mut previous: Option<(u32, String)> = None;
    for (index, value) in required_array(bindings, "attrs")?.iter().enumerate() {
        let binding = object_value(value, &format!("bindings.attrs[{index}]"))?;
        require_known_fields(
            "attribute binding",
            binding,
            &["expr", "kind", "name", "node"],
        )?;
        let node = required_u32(binding, "node")?;
        require_node_kind(view, node, ViewNodeKind::Element, "attribute binding")?;
        let name = required_string(binding, "name")?.to_owned();
        let key = (node, name.clone());
        require_increasing_key(&mut previous, &key, "bindings.attrs")?;
        let kind = match required_string(binding, "kind")? {
            "value" => LegacyAttributeBindingKind::Value,
            "boolean" => LegacyAttributeBindingKind::Boolean,
            "url" => LegacyAttributeBindingKind::Url,
            "unknown" => {
                unsupported.insert("unsupported_shapes");
                LegacyAttributeBindingKind::Value
            }
            other => return Err(format!("unsupported attribute binding kind `{other}`")),
        };
        let expression = required_u32(binding, "expr")?;
        require_declared(
            declarations.expressions,
            expression,
            &format!("attribute binding `{name}` on node {node} expression"),
        )?;
        out.push(LegacyAttributeBinding {
            node,
            name,
            kind,
            expression,
        });
    }
    Ok(out)
}

fn parse_input_bindings(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyInputBinding>, String> {
    let mut out = Vec::new();
    let mut previous = None;
    for (index, value) in required_array(bindings, "inputs")?.iter().enumerate() {
        let binding = object_value(value, &format!("bindings.inputs[{index}]"))?;
        require_known_fields("input binding", binding, &["kind", "node", "signal"])?;
        let node = required_u32(binding, "node")?;
        require_increasing_id(&mut previous, node, "bindings.inputs nodes")?;
        require_node_kind(view, node, ViewNodeKind::Element, "input binding")?;
        let kind = match required_string(binding, "kind")? {
            "value" => LegacyInputBindingKind::Value,
            "checked" => LegacyInputBindingKind::Checked,
            "selected" => LegacyInputBindingKind::Selected,
            "unknown" => {
                unsupported.insert("unsupported_shapes");
                LegacyInputBindingKind::Value
            }
            other => return Err(format!("unsupported input binding kind `{other}`")),
        };
        let signal = required_u32(binding, "signal")?;
        require_declared(
            declarations.signals,
            signal,
            &format!("input binding on node {node} signal"),
        )?;
        out.push(LegacyInputBinding { node, kind, signal });
    }
    Ok(out)
}

fn parse_event_bindings(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<Vec<LegacyEventBinding>, String> {
    let mut out = Vec::new();
    let mut previous: Option<(u32, String)> = None;
    for (index, value) in required_array(bindings, "events")?.iter().enumerate() {
        let binding = object_value(value, &format!("bindings.events[{index}]"))?;
        require_known_fields("event binding", binding, &["event", "handler", "node"])?;
        let node = required_u32(binding, "node")?;
        require_node_kind(view, node, ViewNodeKind::Element, "event binding")?;
        let event = required_string(binding, "event")?.to_owned();
        if event == "unknown" {
            unsupported.insert("unsupported_shapes");
        } else if !is_known_event(&event) {
            return Err(format!("unsupported event binding name `{event}`"));
        }
        let key = (node, event.clone());
        require_increasing_key(&mut previous, &key, "bindings.events")?;
        let handler = required_u32(binding, "handler")?;
        require_declared(
            declarations.handlers,
            handler,
            &format!("event binding `{event}` on node {node} handler"),
        )?;
        out.push(LegacyEventBinding {
            node,
            event,
            handler,
        });
    }
    Ok(out)
}

fn parse_conditionals(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
) -> Result<Vec<LegacyConditionalBinding>, String> {
    let values = required_array(bindings, "conditionals")?;
    let mut out = Vec::with_capacity(values.len());
    let mut previous_parent = None;
    let mut controlled_children = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let section = object_value(value, &format!("bindings.conditionals[{index}]"))?;
        require_known_fields(
            "conditional binding",
            section,
            &["condition", "else", "parent", "then"],
        )?;
        let parent = required_u32(section, "parent")?;
        if previous_parent.is_some_and(|previous| parent < previous) {
            return Err(format!(
                "bindings.conditionals must be ordered by parent; previous {previous_parent:?}, got {parent}"
            ));
        }
        previous_parent = Some(parent);
        require_node_kind(view, parent, ViewNodeKind::Element, "conditional parent")?;
        let condition = required_u32(section, "condition")?;
        require_declared(
            declarations.expressions,
            condition,
            &format!("conditional on parent {parent} expression"),
        )?;
        let then_children = parse_node_ids(
            required_array(section, "then")?,
            &format!("conditional parent {parent} then branch"),
        )?;
        let else_children = parse_node_ids(
            required_array(section, "else")?,
            &format!("conditional parent {parent} else branch"),
        )?;
        if then_children.is_empty() && else_children.is_empty() {
            return Err(format!(
                "conditional parent {parent} must control at least one branch element"
            ));
        }
        let mut section_children = BTreeSet::new();
        for (branch, children) in [
            ("then", then_children.as_slice()),
            ("else", else_children.as_slice()),
        ] {
            for &child in children {
                validate_node_bound(
                    child,
                    view.kinds.len(),
                    &format!("conditional parent {parent} {branch} child"),
                )?;
                require_node_kind(
                    view,
                    child,
                    ViewNodeKind::Element,
                    &format!("conditional parent {parent} {branch} child"),
                )?;
                if !view.children[parent as usize].contains(&child) {
                    return Err(format!(
                        "conditional parent {parent} {branch} child {child} is not a direct child of its parent"
                    ));
                }
                if !section_children.insert(child) {
                    return Err(format!(
                        "conditional parent {parent} repeats child {child} across its branches"
                    ));
                }
                if !controlled_children.insert((parent, child)) {
                    return Err(format!(
                        "conditional parent {parent} child {child} is controlled by more than one section"
                    ));
                }
            }
        }
        let control = u32::try_from(index)
            .map_err(|_| "bindings.conditionals exceeds u32::MAX".to_string())?;
        out.push(LegacyConditionalBinding {
            control,
            parent,
            condition,
            then_children,
            else_children,
        });
    }
    Ok(out)
}

fn parse_ref_bindings(
    bindings: &Map<String, Value>,
    view: &ValidatedView,
    declarations: &DeclaredIds<'_>,
) -> Result<Vec<LegacyNodeRefBinding>, String> {
    let values = required_array(bindings, "refs")?;
    let mut out = Vec::with_capacity(values.len());
    let mut previous = None;
    let mut bound_nodes = BTreeMap::new();
    for (index, value) in values.iter().enumerate() {
        let binding = object_value(value, &format!("bindings.refs[{index}]"))?;
        require_known_fields("node-ref binding", binding, &["node", "ref"])?;
        let node = required_u32(binding, "node")?;
        require_increasing_id(&mut previous, node, "bindings.refs nodes")?;
        require_node_kind(view, node, ViewNodeKind::Element, "node-ref binding")?;
        let node_ref = required_u32(binding, "ref")?;
        require_declared(
            declarations.node_refs,
            node_ref,
            &format!("node-ref binding on node {node}"),
        )?;
        if let Some(previous_node) = bound_nodes.insert(node_ref, node) {
            return Err(format!(
                "node ref {node_ref} is bound to multiple nodes {previous_node} and {node}"
            ));
        }
        out.push(LegacyNodeRefBinding { node, node_ref });
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum NodeRefReachabilityWork {
    Signal(u32),
    Expression(u32),
    Handler(u32),
}

/// Close component-local node-reference reachability from browser-visible
/// roots only.
///
/// Schema v9 retains declarations after source-level dead-code elimination,
/// so declaration presence is not a client origin. Rendered `ref={...}`
/// bindings are roots, as are node-ref dependencies reached through a live
/// DOM expression, input cell, bound event handler, authenticated
/// cross-component signal export, or reachable resource registration
/// expression. Signal-to-derived-expression edges are followed transitively;
/// unbound handlers and otherwise latent expressions never enter the worklist.
fn collect_reachable_node_refs(
    signals: &[LegacySignal],
    expressions: &[LegacyExpression],
    handlers: &[LegacyBoundHandler],
    bindings: &LegacyDomBindings,
    extra_signal_roots: &BTreeSet<u32>,
    extra_expression_roots: &BTreeSet<u32>,
) -> Vec<u32> {
    let signals_by_id = signals
        .iter()
        .map(|signal| (signal.id, signal))
        .collect::<BTreeMap<_, _>>();
    let expressions_by_id = expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let handlers_by_id = handlers
        .iter()
        .map(|handler| (handler.id, handler))
        .collect::<BTreeMap<_, _>>();

    let mut reachable = bindings
        .node_refs
        .iter()
        .map(|binding| binding.node_ref)
        .collect::<BTreeSet<_>>();
    let mut work = Vec::new();
    work.extend(
        extra_signal_roots
            .iter()
            .copied()
            .map(NodeRefReachabilityWork::Signal),
    );
    work.extend(
        extra_expression_roots
            .iter()
            .copied()
            .map(NodeRefReachabilityWork::Expression),
    );
    work.extend(
        bindings
            .text
            .iter()
            .map(|binding| NodeRefReachabilityWork::Expression(binding.expression)),
    );
    work.extend(
        bindings
            .attributes
            .iter()
            .map(|binding| NodeRefReachabilityWork::Expression(binding.expression)),
    );
    work.extend(
        bindings
            .inputs
            .iter()
            .map(|binding| NodeRefReachabilityWork::Signal(binding.signal)),
    );
    work.extend(
        bindings
            .events
            .iter()
            .map(|binding| NodeRefReachabilityWork::Handler(binding.handler)),
    );
    work.extend(
        bindings
            .conditionals
            .iter()
            .map(|binding| NodeRefReachabilityWork::Expression(binding.condition)),
    );

    let mut visited_signals = BTreeSet::new();
    let mut visited_expressions = BTreeSet::new();
    let mut visited_handlers = BTreeSet::new();
    while let Some(next) = work.pop() {
        match next {
            NodeRefReachabilityWork::Signal(id) => {
                if !visited_signals.insert(id) {
                    continue;
                }
                if let Some(expression) = signals_by_id
                    .get(&id)
                    .and_then(|signal| signal.derived_expression)
                {
                    work.push(NodeRefReachabilityWork::Expression(expression));
                }
            }
            NodeRefReachabilityWork::Expression(id) => {
                if !visited_expressions.insert(id) {
                    continue;
                }
                if let Some(expression) = expressions_by_id.get(&id) {
                    enqueue_node_ref_dependencies(
                        &expression.dependencies,
                        &mut reachable,
                        &mut work,
                    );
                }
            }
            NodeRefReachabilityWork::Handler(id) => {
                if !visited_handlers.insert(id) {
                    continue;
                }
                if let Some(handler) = handlers_by_id.get(&id) {
                    enqueue_node_ref_dependencies(&handler.dependencies, &mut reachable, &mut work);
                }
            }
        }
    }

    reachable.into_iter().collect()
}

fn enqueue_node_ref_dependencies(
    dependencies: &ClientBlockExternalDependencies,
    reachable: &mut BTreeSet<u32>,
    work: &mut Vec<NodeRefReachabilityWork>,
) {
    reachable.extend(dependencies.node_refs.iter().map(|node_ref| node_ref.0));
    work.extend(
        dependencies
            .signal_reads
            .iter()
            .chain(&dependencies.signal_writes)
            .map(|signal| NodeRefReachabilityWork::Signal(signal.0)),
    );
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ResourceReachability {
    signals: BTreeSet<u32>,
    queries: BTreeSet<u32>,
    mutations: BTreeSet<u32>,
    expressions: BTreeSet<u32>,
}

#[derive(Clone, Copy)]
enum ResourceReachabilityWork {
    Signal(u32),
    Expression(u32),
    Handler(u32),
    Query(u32),
    Mutation(u32),
}

/// Close query/mutation reachability from actual browser obligations.
///
/// Rendered expression bindings, bound event handlers, and authenticated
/// cross-component signal exports are roots. A reached query or mutation pulls
/// in the exact expressions required to register that resource. Those
/// expressions may in turn depend on signals or another resource. Merely
/// declaring one or mentioning it from an unbound handler does not make it
/// live.
fn collect_resource_reachability(
    queries: &[LegacyQuery],
    mutations: &[LegacyMutation],
    signals: &[LegacySignal],
    expressions: &[LegacyExpression],
    handlers: &[LegacyBoundHandler],
    bindings: &LegacyDomBindings,
    keyed_collections: &[LegacyKeyedCollection],
    extra_signal_roots: &BTreeSet<u32>,
) -> ResourceReachability {
    let queries_by_id = queries
        .iter()
        .map(|query| (query.id, query))
        .collect::<BTreeMap<_, _>>();
    let signals_by_id = signals
        .iter()
        .map(|signal| (signal.id, signal))
        .collect::<BTreeMap<_, _>>();
    let mutations_by_id = mutations
        .iter()
        .map(|mutation| (mutation.id, mutation))
        .collect::<BTreeMap<_, _>>();
    let expressions_by_id = expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let handlers_by_id = handlers
        .iter()
        .map(|handler| (handler.id, handler))
        .collect::<BTreeMap<_, _>>();

    let mut work = Vec::new();
    work.extend(
        extra_signal_roots
            .iter()
            .copied()
            .map(ResourceReachabilityWork::Signal),
    );
    work.extend(
        bindings
            .text
            .iter()
            .map(|binding| ResourceReachabilityWork::Expression(binding.expression)),
    );
    work.extend(
        bindings
            .attributes
            .iter()
            .map(|binding| ResourceReachabilityWork::Expression(binding.expression)),
    );
    work.extend(
        bindings
            .inputs
            .iter()
            .map(|binding| ResourceReachabilityWork::Signal(binding.signal)),
    );
    work.extend(
        bindings
            .events
            .iter()
            .map(|binding| ResourceReachabilityWork::Handler(binding.handler)),
    );
    work.extend(
        bindings
            .conditionals
            .iter()
            .map(|binding| ResourceReachabilityWork::Expression(binding.condition)),
    );
    work.extend(
        keyed_collections
            .iter()
            .map(|collection| match collection.source {
                LegacyKeyedCollectionSource::Signal(signal) => {
                    ResourceReachabilityWork::Signal(signal)
                }
                LegacyKeyedCollectionSource::Expression(expression) => {
                    ResourceReachabilityWork::Expression(expression)
                }
            }),
    );

    let mut result = ResourceReachability::default();
    let mut visited_handlers = BTreeSet::new();
    while let Some(next) = work.pop() {
        match next {
            ResourceReachabilityWork::Signal(id) => {
                if !result.signals.insert(id) {
                    continue;
                }
                if let Some(expression) = signals_by_id
                    .get(&id)
                    .and_then(|signal| signal.derived_expression)
                {
                    work.push(ResourceReachabilityWork::Expression(expression));
                }
            }
            ResourceReachabilityWork::Expression(id) => {
                if !result.expressions.insert(id) {
                    continue;
                }
                if let Some(expression) = expressions_by_id.get(&id) {
                    enqueue_resource_dependencies(&expression.dependencies, &mut work);
                }
            }
            ResourceReachabilityWork::Handler(id) => {
                if !visited_handlers.insert(id) {
                    continue;
                }
                if let Some(handler) = handlers_by_id.get(&id) {
                    enqueue_resource_dependencies(&handler.dependencies, &mut work);
                }
            }
            ResourceReachabilityWork::Query(id) => {
                if !result.queries.insert(id) {
                    continue;
                }
                if let Some(query) = queries_by_id.get(&id) {
                    work.extend(
                        [
                            query.key_expression,
                            query.server_initial_expression,
                            query.client_fetcher_expression,
                        ]
                        .into_iter()
                        .map(ResourceReachabilityWork::Expression),
                    );
                }
            }
            ResourceReachabilityWork::Mutation(id) => {
                if !result.mutations.insert(id) {
                    continue;
                }
                if let Some(mutation) = mutations_by_id.get(&id) {
                    work.extend(
                        [
                            mutation.key_expression,
                            mutation.server_runner_expression,
                            mutation.client_runner_expression,
                        ]
                        .into_iter()
                        .chain(mutation.invalidates.iter().copied())
                        .map(ResourceReachabilityWork::Expression),
                    );
                }
            }
        }
    }
    result
}

fn enqueue_resource_dependencies(
    dependencies: &ClientBlockExternalDependencies,
    work: &mut Vec<ResourceReachabilityWork>,
) {
    work.extend(
        dependencies
            .signal_reads
            .iter()
            .chain(&dependencies.signal_writes)
            .map(|signal| ResourceReachabilityWork::Signal(signal.0)),
    );
    work.extend(
        dependencies
            .queries
            .iter()
            .map(|query| ResourceReachabilityWork::Query(query.0)),
    );
    work.extend(
        dependencies
            .mutations
            .iter()
            .map(|mutation| ResourceReachabilityWork::Mutation(mutation.0)),
    );
}

fn validate_query_refs(
    queries: &[LegacyQuery],
    declarations: &DeclaredIds<'_>,
) -> Result<(), String> {
    for query in queries {
        for (name, expression) in [
            ("key_expr", query.key_expression),
            ("server_initial", query.server_initial_expression),
            ("client_fetcher", query.client_fetcher_expression),
        ] {
            require_declared(
                declarations.expressions,
                expression,
                &format!("query {} {name}", query.id),
            )?;
        }
    }
    Ok(())
}

fn validate_mutation_refs(
    mutations: &[LegacyMutation],
    declarations: &DeclaredIds<'_>,
) -> Result<(), String> {
    for mutation in mutations {
        for (name, expression) in [
            ("key_expr", mutation.key_expression),
            ("server_runner", mutation.server_runner_expression),
            ("client_runner", mutation.client_runner_expression),
        ] {
            require_declared(
                declarations.expressions,
                expression,
                &format!("mutation {} {name}", mutation.id),
            )?;
        }
        for expression in &mutation.invalidates {
            require_declared(
                declarations.expressions,
                *expression,
                &format!("mutation {} invalidation key", mutation.id),
            )?;
        }
    }
    Ok(())
}

fn join_query_initializers(
    descriptor: &Map<String, Value>,
    queries: &mut [LegacyQuery],
) -> Result<(), String> {
    let Some(values) = optional_array(descriptor, "query_initializers")? else {
        return Ok(());
    };
    let query_indices = queries
        .iter()
        .enumerate()
        .map(|(index, query)| (query.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let initializer = object_value(value, &format!("query_initializers[{index}]"))?;
        require_known_fields(
            "query initializer",
            initializer,
            &["expr", "id", "key_expr"],
        )?;
        let id = required_u32(initializer, "id")?;
        require_increasing_id(&mut previous, id, "query_initializers")?;
        let Some(&query_index) = query_indices.get(&id) else {
            return Err(format!("query initializer refers to undeclared ID {id}"));
        };
        let key_expression = match initializer
            .get("key_expr")
            .ok_or_else(|| "missing `key_expr`".to_string())?
        {
            Value::Null => None,
            Value::String(value) => Some(value.clone()),
            _ => return Err("`key_expr` must be a string or null".to_string()),
        };
        let joined = LegacyQueryInitializer {
            expression: required_string(initializer, "expr")?.to_owned(),
            key_expression,
        };
        if queries[query_index].initializer.replace(joined).is_some() {
            return Err(format!(
                "query initializer for query {id} is declared more than once"
            ));
        }
    }
    Ok(())
}

fn validate_mutation_initializers(
    descriptor: &Map<String, Value>,
    declarations: &DeclaredIds<'_>,
) -> Result<(), String> {
    if let Some(values) = optional_array(descriptor, "mutation_initializers")? {
        let mut previous = None;
        for (index, value) in values.iter().enumerate() {
            let initializer = object_value(value, &format!("mutation_initializers[{index}]"))?;
            require_known_fields(
                "mutation initializer",
                initializer,
                &["id", "invalidates_expr", "key_expr"],
            )?;
            let id = required_u32(initializer, "id")?;
            require_increasing_id(&mut previous, id, "mutation_initializers")?;
            require_declared(declarations.mutations, id, "mutation initializer")?;
            required_string(initializer, "key_expr")?;
            validate_nullable_string(initializer, "invalidates_expr")?;
        }
    }
    Ok(())
}

fn validate_nullable_string(object: &Map<String, Value>, field: &str) -> Result<(), String> {
    match object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
    {
        Value::Null | Value::String(_) => Ok(()),
        _ => Err(format!("`{field}` must be a string or null")),
    }
}

fn validate_intervals(
    descriptor: &Map<String, Value>,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<(), String> {
    let Some(values) = optional_array(descriptor, "intervals")? else {
        return Ok(());
    };
    if !values.is_empty() {
        unsupported.insert("intervals");
    }
    for (index, value) in values.iter().enumerate() {
        let interval = object_value(value, &format!("intervals[{index}]"))?;
        require_known_fields("interval", interval, &["body_ir", "body_ir_version", "ms"])?;
        required_u64(interval, "ms")?;
        require_exact_u64(interval, "body_ir_version", CLIENT_IR_SCHEMA)?;
        let body = parse_canonical_block(
            interval
                .get("body_ir")
                .ok_or_else(|| format!("interval {index} is missing `body_ir`"))?,
            &format!("interval {index}"),
        )?;
        let dependencies = collect_client_block_external_dependencies(&body)
            .map_err(|error| format!("interval {index} is not dependency-closed: {error:?}"))?;
        validate_external_dependencies(
            &format!("interval {index}"),
            &dependencies,
            DependencyContext::Interval,
            declarations,
            unsupported,
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum DependencyContext {
    Expression,
    EventHandler,
    Interval,
}

struct DeclaredIds<'a> {
    signals: &'a BTreeSet<u32>,
    expressions: &'a BTreeSet<u32>,
    handlers: &'a BTreeSet<u32>,
    callback_slots: &'a BTreeSet<u32>,
    queries: &'a BTreeSet<u32>,
    mutations: &'a BTreeSet<u32>,
    node_refs: &'a BTreeSet<u32>,
    helpers: &'a BTreeSet<u32>,
}

fn validate_external_dependencies(
    label: &str,
    dependencies: &ClientBlockExternalDependencies,
    context: DependencyContext,
    declarations: &DeclaredIds<'_>,
    unsupported: &mut UnsupportedFeatures,
) -> Result<(), String> {
    for signal in dependencies
        .signal_reads
        .iter()
        .chain(&dependencies.signal_writes)
    {
        require_declared(declarations.signals, signal.0, &format!("{label} signal"))?;
    }
    for callback in &dependencies.callback_slots {
        require_declared(
            declarations.callback_slots,
            callback.0,
            &format!("{label} callback slot"),
        )?;
    }
    for query in &dependencies.queries {
        require_declared(declarations.queries, query.0, &format!("{label} query"))?;
    }
    for mutation in &dependencies.mutations {
        require_declared(
            declarations.mutations,
            mutation.0,
            &format!("{label} mutation"),
        )?;
    }
    for node_ref in &dependencies.node_refs {
        require_declared(
            declarations.node_refs,
            node_ref.0,
            &format!("{label} node ref"),
        )?;
    }
    for helper in &dependencies.helper_edges {
        require_declared(
            declarations.helpers,
            helper.index(),
            &format!("{label} helper ordinal"),
        )?;
    }
    if !dependencies.client_module_calls.is_empty() {
        unsupported.insert("client_imports");
    }
    if matches!(context, DependencyContext::Interval)
        && (dependencies.uses_browser_read || dependencies.uses_browser_write)
    {
        unsupported.insert("unsupported_shapes");
    }
    if dependencies.uses_callback_payload
        || (dependencies.uses_node_payload && !matches!(context, DependencyContext::Expression))
        || (dependencies.uses_event_payload && !matches!(context, DependencyContext::EventHandler))
    {
        unsupported.insert("unsupported_shapes");
    }
    Ok(())
}

fn collect_expression_client_operations(
    expression: &ClientExpr,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
) -> Result<Vec<LegacyClientOperation>, String> {
    let mut operations = Vec::new();
    collect_client_expression_operations(
        expression,
        LegacyClientOperationConsumer::Enclosing,
        shared_fields,
        &mut operations,
    )?;
    Ok(operations)
}

fn collect_handler_client_operations(
    block: &ClientBlock,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
) -> Result<Vec<LegacyClientOperation>, String> {
    let mut operations = Vec::new();
    collect_client_block_operations(block, shared_fields, &mut operations)?;
    Ok(operations)
}

fn collect_client_block_operations(
    block: &ClientBlock,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
    operations: &mut Vec<LegacyClientOperation>,
) -> Result<(), String> {
    for statement in &block.stmts {
        collect_client_statement_operations(statement, shared_fields, operations)?;
    }
    Ok(())
}

fn collect_client_statement_operations(
    statement: &ClientStmt,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
    operations: &mut Vec<LegacyClientOperation>,
) -> Result<(), String> {
    let enclosing = LegacyClientOperationConsumer::Enclosing;
    match statement {
        ClientStmt::Expr(expression)
        | ClientStmt::Assign {
            value: expression, ..
        } => {
            collect_client_expression_operations(expression, enclosing, shared_fields, operations)?;
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(expression) = value {
                collect_client_expression_operations(
                    expression,
                    enclosing,
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::SignalSet {
            id,
            value: expression,
        } => {
            if let Some(field) = shared_fields.get(&id.0) {
                let mut dependencies = collect_client_expr_external_dependencies(expression)
                    .map_err(|error| {
                        format!("shared set argument is not dependency-closed: {error:?}")
                    })?;
                push_unique(&mut dependencies.signal_writes, *id);
                let operation = push_client_operation(
                    operations,
                    LegacyClientOperation {
                        kind: LegacyClientOperationKind::SharedSet {
                            signal: id.0,
                            field: field.clone(),
                        },
                        consumer: enclosing,
                        dependencies,
                    },
                )?;
                collect_client_expression_operations(
                    expression,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            } else {
                collect_client_expression_operations(
                    expression,
                    enclosing,
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::SignalUpdate { id, closure } => {
            if let Some(field) = shared_fields.get(&id.0) {
                let mut dependencies = collect_client_expr_external_dependencies(&closure.body)
                    .map_err(|error| {
                        format!("shared update argument is not dependency-closed: {error:?}")
                    })?;
                push_unique(&mut dependencies.signal_reads, *id);
                push_unique(&mut dependencies.signal_writes, *id);
                let operation = push_client_operation(
                    operations,
                    LegacyClientOperation {
                        kind: LegacyClientOperationKind::SharedUpdate {
                            signal: id.0,
                            field: field.clone(),
                        },
                        consumer: enclosing,
                        dependencies,
                    },
                )?;
                collect_client_expression_operations(
                    &closure.body,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            } else {
                collect_client_expression_operations(
                    &closure.body,
                    enclosing,
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::QueryRefetch { query } => {
            let mut dependencies = ClientBlockExternalDependencies::default();
            dependencies.queries.push(*query);
            push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::QueryRefetch { query: query.0 },
                    consumer: enclosing,
                    dependencies,
                },
            )?;
        }
        ClientStmt::QueryInvalidate { query } => {
            let mut dependencies = ClientBlockExternalDependencies::default();
            dependencies.queries.push(*query);
            push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::QueryInvalidate { query: query.0 },
                    consumer: enclosing,
                    dependencies,
                },
            )?;
        }
        ClientStmt::RunMutation { mutation, args } => {
            let mut dependencies =
                collect_expression_list_dependencies(&args.iter().collect::<Vec<_>>())?;
            push_unique(&mut dependencies.mutations, *mutation);
            let operation = push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::MutationRun {
                        mutation: mutation.0,
                    },
                    consumer: enclosing,
                    dependencies,
                },
            )?;
            for argument in args {
                collect_client_expression_operations(
                    argument,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::Console { args, .. } => {
            for argument in args {
                collect_client_expression_operations(
                    argument,
                    enclosing,
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_client_expression_operations(payload, enclosing, shared_fields, operations)?;
        }
        ClientStmt::BrowserEffect(effect) => {
            let arguments = browser_effect_arguments(effect);
            let mut dependencies = collect_expression_list_dependencies(&arguments)?;
            if let Some(target) = browser_effect_signal_target(effect) {
                push_unique(&mut dependencies.signal_writes, target);
            }
            let operation = push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::BrowserWrite,
                    consumer: enclosing,
                    dependencies,
                },
            )?;
            for argument in arguments {
                collect_client_expression_operations(
                    argument,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::NodeRefEffect { id, effect } => {
            let arguments = node_ref_effect_arguments(effect);
            let mut dependencies = collect_expression_list_dependencies(&arguments)?;
            push_unique(&mut dependencies.node_refs, *id);
            let operation = push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::NodeRefWrite { node_ref: id.0 },
                    consumer: enclosing,
                    dependencies,
                },
            )?;
            for argument in arguments {
                collect_client_expression_operations(
                    argument,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientStmt::Block(block) => {
            collect_client_block_operations(block, shared_fields, operations)?;
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_client_expression_operations(condition, enclosing, shared_fields, operations)?;
            collect_client_block_operations(then_block, shared_fields, operations)?;
            if let Some(else_block) = else_block {
                collect_client_block_operations(else_block, shared_fields, operations)?;
            }
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_client_expression_operations(start, enclosing, shared_fields, operations)?;
            collect_client_expression_operations(end, enclosing, shared_fields, operations)?;
            collect_client_block_operations(body, shared_fields, operations)?;
        }
        ClientStmt::EventPreventDefault | ClientStmt::EventStopPropagation => {}
        _ => {
            return Err(
                "unsupported future Client IR statement in client-operation traversal".to_owned(),
            );
        }
    }
    Ok(())
}

fn collect_client_expression_operations(
    expression: &ClientExpr,
    consumer: LegacyClientOperationConsumer,
    shared_fields: &BTreeMap<u32, LegacySharedFieldRef>,
    operations: &mut Vec<LegacyClientOperation>,
) -> Result<(), String> {
    match expression {
        ClientExpr::SignalRead(signal) => {
            if let Some(field) = shared_fields.get(&signal.0) {
                let mut dependencies = ClientBlockExternalDependencies::default();
                dependencies.signal_reads.push(*signal);
                push_client_operation(
                    operations,
                    LegacyClientOperation {
                        kind: LegacyClientOperationKind::SharedRead {
                            signal: signal.0,
                            field: field.clone(),
                        },
                        consumer,
                        dependencies,
                    },
                )?;
            }
        }
        ClientExpr::QueryRead { id, accessor } => {
            let mut dependencies = ClientBlockExternalDependencies::default();
            dependencies.queries.push(*id);
            push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::QueryRead {
                        query: id.0,
                        accessor: *accessor,
                    },
                    consumer,
                    dependencies,
                },
            )?;
        }
        ClientExpr::MutationRead { id, accessor } => {
            let mut dependencies = ClientBlockExternalDependencies::default();
            dependencies.mutations.push(*id);
            push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::MutationRead {
                        mutation: id.0,
                        accessor: *accessor,
                    },
                    consumer,
                    dependencies,
                },
            )?;
        }
        ClientExpr::BrowserRead(read) => {
            let arguments = browser_read_arguments(read)?;
            let dependencies = collect_expression_list_dependencies(&arguments)?;
            let operation = push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::BrowserRead,
                    consumer,
                    dependencies,
                },
            )?;
            for argument in arguments {
                collect_client_expression_operations(
                    argument,
                    LegacyClientOperationConsumer::Operation(operation),
                    shared_fields,
                    operations,
                )?;
            }
        }
        ClientExpr::NodeRefRead { id, .. } | ClientExpr::EventTargetWithinNodeRef { id, .. } => {
            let mut dependencies = ClientBlockExternalDependencies::default();
            dependencies.node_refs.push(*id);
            push_client_operation(
                operations,
                LegacyClientOperation {
                    kind: LegacyClientOperationKind::NodeRefRead { node_ref: id.0 },
                    consumer,
                    dependencies,
                },
            )?;
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::ArrayLit(_)
        | ClientExpr::Local(_)
        | ClientExpr::EventRead(_)
        | ClientExpr::EventCurrentTarget
        | ClientExpr::EventCollectionLen(_)
        | ClientExpr::EventAttribute { .. }
        | ClientExpr::EventPayload { .. }
        | ClientExpr::CallbackPayload
        | ClientExpr::NodePayload { .. } => {}
        ClientExpr::EventCollectionRead { index, .. }
        | ClientExpr::Encoding { value: index, .. }
        | ClientExpr::IsNonEmpty { target: index }
        | ClientExpr::StringToLowercase { target: index }
        | ClientExpr::StringTransform { target: index, .. }
        | ClientExpr::Project { target: index, .. }
        | ClientExpr::AsType { target: index, .. }
        | ClientExpr::IsPresent { target: index }
        | ClientExpr::Eq { target: index, .. }
        | ClientExpr::Neq { target: index, .. }
        | ClientExpr::Coalesce { target: index, .. }
        | ClientExpr::Unary { expr: index, .. }
        | ClientExpr::Field { base: index, .. }
        | ClientExpr::DomRead { target: index, .. } => {
            collect_client_expression_operations(index, consumer, shared_fields, operations)?;
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_client_expression_operations(condition, consumer, shared_fields, operations)?;
            collect_client_expression_operations(then_expr, consumer, shared_fields, operations)?;
            collect_client_expression_operations(else_expr, consumer, shared_fields, operations)?;
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
            collect_client_expression_operations(haystack, consumer, shared_fields, operations)?;
            collect_client_expression_operations(needle, consumer, shared_fields, operations)?;
        }
        ClientExpr::Index { base, index } => {
            collect_client_expression_operations(base, consumer, shared_fields, operations)?;
            collect_client_expression_operations(index, consumer, shared_fields, operations)?;
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_client_expression_operations(part, consumer, shared_fields, operations)?;
            }
        }
        ClientExpr::Call { args, .. } => {
            for argument in args {
                collect_client_expression_operations(
                    argument,
                    consumer,
                    shared_fields,
                    operations,
                )?;
            }
        }
        _ => {
            return Err(
                "unsupported future Client IR expression in client-operation traversal".to_owned(),
            );
        }
    }
    Ok(())
}

fn push_client_operation(
    operations: &mut Vec<LegacyClientOperation>,
    operation: LegacyClientOperation,
) -> Result<u32, String> {
    let id = u32::try_from(operations.len())
        .map_err(|_| "client operation count exceeds u32::MAX".to_owned())?;
    operations.push(operation);
    Ok(id)
}

fn collect_expression_browser_operations(
    expression: &ClientExpr,
) -> Result<Vec<LegacyBrowserOperation>, String> {
    let mut operations = Vec::new();
    collect_browser_reads(
        expression,
        LegacyBrowserConsumer::Enclosing,
        &mut operations,
    )?;
    Ok(operations)
}

pub(crate) fn collect_handler_browser_operations(
    block: &ClientBlock,
) -> Result<Vec<LegacyBrowserOperation>, String> {
    let mut operations = Vec::new();
    collect_block_browser_operations(block, &mut operations)?;
    Ok(operations)
}

fn collect_block_browser_operations(
    block: &ClientBlock,
    operations: &mut Vec<LegacyBrowserOperation>,
) -> Result<(), String> {
    for statement in &block.stmts {
        collect_statement_browser_operations(statement, operations)?;
    }
    Ok(())
}

fn collect_statement_browser_operations(
    statement: &ClientStmt,
    operations: &mut Vec<LegacyBrowserOperation>,
) -> Result<(), String> {
    match statement {
        ClientStmt::Expr(expression)
        | ClientStmt::Assign {
            value: expression, ..
        }
        | ClientStmt::SignalSet {
            value: expression, ..
        } => collect_browser_reads(expression, LegacyBrowserConsumer::Enclosing, operations)?,
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(expression) = value {
                collect_browser_reads(expression, LegacyBrowserConsumer::Enclosing, operations)?;
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_browser_reads(&closure.body, LegacyBrowserConsumer::Enclosing, operations)?
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for argument in args {
                collect_browser_reads(argument, LegacyBrowserConsumer::Enclosing, operations)?;
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_browser_reads(payload, LegacyBrowserConsumer::Enclosing, operations)?
        }
        ClientStmt::BrowserEffect(effect) => {
            let arguments = browser_effect_arguments(effect);
            let mut dependencies = collect_expression_list_dependencies(&arguments)?;
            if let Some(target) = browser_effect_signal_target(effect) {
                push_unique(&mut dependencies.signal_writes, target);
            }
            let operation = push_browser_operation(
                operations,
                LegacyBrowserOperation {
                    kind: LegacyBrowserOperationKind::Write,
                    consumer: LegacyBrowserConsumer::Enclosing,
                    dependencies,
                },
            )?;
            for argument in arguments {
                collect_browser_reads(
                    argument,
                    LegacyBrowserConsumer::Operation(operation),
                    operations,
                )?;
            }
        }
        ClientStmt::NodeRefEffect { .. } => {
            // Node-reference operations remain outside this lowering slice.
            // Their declared NodeRef dependency makes the whole graph
            // unsupported before canonical population.
        }
        ClientStmt::Block(block) => collect_block_browser_operations(block, operations)?,
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_browser_reads(condition, LegacyBrowserConsumer::Enclosing, operations)?;
            collect_block_browser_operations(then_block, operations)?;
            if let Some(else_block) = else_block {
                collect_block_browser_operations(else_block, operations)?;
            }
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_browser_reads(start, LegacyBrowserConsumer::Enclosing, operations)?;
            collect_browser_reads(end, LegacyBrowserConsumer::Enclosing, operations)?;
            collect_block_browser_operations(body, operations)?;
        }
        _ => {
            return Err(
                "unsupported future Client IR statement in browser-operation traversal".to_owned(),
            );
        }
    }
    Ok(())
}

fn collect_browser_reads(
    expression: &ClientExpr,
    consumer: LegacyBrowserConsumer,
    operations: &mut Vec<LegacyBrowserOperation>,
) -> Result<(), String> {
    match expression {
        ClientExpr::BrowserRead(read) => {
            let arguments = browser_read_arguments(read)?;
            let dependencies = collect_expression_list_dependencies(&arguments)?;
            let operation = push_browser_operation(
                operations,
                LegacyBrowserOperation {
                    kind: LegacyBrowserOperationKind::Read,
                    consumer,
                    dependencies,
                },
            )?;
            for argument in arguments {
                collect_browser_reads(
                    argument,
                    LegacyBrowserConsumer::Operation(operation),
                    operations,
                )?;
            }
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::ArrayLit(_)
        | ClientExpr::Local(_)
        | ClientExpr::SignalRead(_)
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
        ClientExpr::EventCollectionRead { index, .. }
        | ClientExpr::Encoding { value: index, .. }
        | ClientExpr::IsNonEmpty { target: index }
        | ClientExpr::StringToLowercase { target: index }
        | ClientExpr::StringTransform { target: index, .. }
        | ClientExpr::Project { target: index, .. }
        | ClientExpr::AsType { target: index, .. }
        | ClientExpr::IsPresent { target: index }
        | ClientExpr::Eq { target: index, .. }
        | ClientExpr::Neq { target: index, .. }
        | ClientExpr::Coalesce { target: index, .. }
        | ClientExpr::Unary { expr: index, .. }
        | ClientExpr::Field { base: index, .. }
        | ClientExpr::DomRead { target: index, .. } => {
            collect_browser_reads(index, consumer, operations)?;
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_browser_reads(condition, consumer, operations)?;
            collect_browser_reads(then_expr, consumer, operations)?;
            collect_browser_reads(else_expr, consumer, operations)?;
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
            collect_browser_reads(haystack, consumer, operations)?;
            collect_browser_reads(needle, consumer, operations)?;
        }
        ClientExpr::Index { base, index } => {
            collect_browser_reads(base, consumer, operations)?;
            collect_browser_reads(index, consumer, operations)?;
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_browser_reads(part, consumer, operations)?;
            }
        }
        ClientExpr::Call { args, .. } => {
            for argument in args {
                collect_browser_reads(argument, consumer, operations)?;
            }
        }
        _ => {
            return Err(
                "unsupported future Client IR expression in browser-operation traversal".to_owned(),
            );
        }
    }
    Ok(())
}

fn push_browser_operation(
    operations: &mut Vec<LegacyBrowserOperation>,
    operation: LegacyBrowserOperation,
) -> Result<u32, String> {
    let id = u32::try_from(operations.len())
        .map_err(|_| "browser operation count exceeds u32::MAX".to_owned())?;
    operations.push(operation);
    Ok(id)
}

fn collect_expression_list_dependencies(
    expressions: &[&ClientExpr],
) -> Result<ClientBlockExternalDependencies, String> {
    let mut dependencies = ClientBlockExternalDependencies::default();
    for expression in expressions {
        let next = collect_client_expr_external_dependencies(expression)
            .map_err(|error| format!("operation argument is not dependency-closed: {error:?}"))?;
        merge_external_dependencies(&mut dependencies, next);
    }
    Ok(dependencies)
}

fn merge_external_dependencies(
    target: &mut ClientBlockExternalDependencies,
    source: ClientBlockExternalDependencies,
) {
    for value in source.signal_reads {
        push_unique(&mut target.signal_reads, value);
    }
    for value in source.signal_writes {
        push_unique(&mut target.signal_writes, value);
    }
    for value in source.queries {
        push_unique(&mut target.queries, value);
    }
    for value in source.mutations {
        push_unique(&mut target.mutations, value);
    }
    for value in source.node_refs {
        push_unique(&mut target.node_refs, value);
    }
    for value in source.callback_slots {
        push_unique(&mut target.callback_slots, value);
    }
    for value in source.helper_edges {
        push_unique(&mut target.helper_edges, value);
    }
    for value in source.client_module_calls {
        push_unique(&mut target.client_module_calls, value);
    }
    target.uses_callback_payload |= source.uses_callback_payload;
    target.uses_browser_read |= source.uses_browser_read;
    target.uses_browser_write |= source.uses_browser_write;
    target.uses_event_payload |= source.uses_event_payload;
    target.uses_node_payload |= source.uses_node_payload;
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn browser_read_arguments(read: &ClientBrowserRead) -> Result<Vec<&ClientExpr>, String> {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => Ok(Vec::new()),
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            let mut arguments = vec![key.as_ref()];
            if let Some(fallback) = fallback {
                arguments.push(fallback.as_ref());
            }
            Ok(arguments)
        }
        ClientBrowserRead::Declared { args, .. } => Ok(args.iter().collect()),
        _ => Err("unsupported future Client IR browser read".to_owned()),
    }
}

fn browser_effect_arguments(effect: &ClientBrowserEffect) -> Vec<&ClientExpr> {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => vec![x, y],
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => Vec::new(),
        ClientBrowserEffect::LocationSetHash { value } => vec![value],
        ClientBrowserEffect::LocationAssign { url }
        | ClientBrowserEffect::LocationReplace { url }
        | ClientBrowserEffect::HistoryPushState { url }
        | ClientBrowserEffect::HistoryReplaceState { url } => vec![url],
        ClientBrowserEffect::HistoryGo { delta } => vec![delta],
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            let mut arguments = vec![name];
            if let Some(detail) = detail {
                arguments.push(detail);
            }
            arguments
        }
        ClientBrowserEffect::DocumentSetTitle { title } => vec![title],
        ClientBrowserEffect::ClipboardWriteText { text } => vec![text],
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => fallback.iter().collect(),
        ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            fallback,
            ..
        }
        | ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            fallback,
            ..
        } => {
            let mut arguments = vec![options_json];
            if let Some(fallback) = fallback {
                arguments.push(fallback);
            }
            arguments
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. }
        | ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => vec![key, value],
        ClientBrowserEffect::StorageRemoveItem { key, .. }
        | ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => vec![key],
        ClientBrowserEffect::Declared { args, .. } => args.iter().collect(),
    }
}

fn node_ref_effect_arguments(effect: &ClientNodeRefEffect) -> Vec<&ClientExpr> {
    match effect {
        ClientNodeRefEffect::Focus
        | ClientNodeRefEffect::Blur
        | ClientNodeRefEffect::ScrollIntoView
        | ClientNodeRefEffect::Click
        | ClientNodeRefEffect::RequestSubmit
        | ClientNodeRefEffect::Reset
        | ClientNodeRefEffect::ReportValidity
        | ClientNodeRefEffect::ShowModal
        | ClientNodeRefEffect::Close
        | ClientNodeRefEffect::ShowPopover
        | ClientNodeRefEffect::HidePopover
        | ClientNodeRefEffect::TogglePopover
        | ClientNodeRefEffect::Select
        | ClientNodeRefEffect::RemoveAttribute { .. } => Vec::new(),
        ClientNodeRefEffect::ScrollTo { x, y } | ClientNodeRefEffect::ScrollBy { x, y } => {
            vec![x, y]
        }
        ClientNodeRefEffect::SetPointerCapture(value)
        | ClientNodeRefEffect::ReleasePointerCapture(value)
        | ClientNodeRefEffect::SetValue(value)
        | ClientNodeRefEffect::SetChecked(value)
        | ClientNodeRefEffect::SetCustomValidity(value)
        | ClientNodeRefEffect::SetTextContent(value)
        | ClientNodeRefEffect::AddClass(value)
        | ClientNodeRefEffect::RemoveClass(value)
        | ClientNodeRefEffect::ToggleClass(value)
        | ClientNodeRefEffect::SetAttribute { value, .. }
        | ClientNodeRefEffect::SetStyleProperty { value, .. } => vec![value],
        ClientNodeRefEffect::SetSelectionRange(start, end) => vec![start, end],
    }
}

fn browser_effect_signal_target(effect: &ClientBrowserEffect) -> Option<rsjs_ir::SignalId> {
    match effect {
        ClientBrowserEffect::ClipboardReadTextInto { target, .. }
        | ClientBrowserEffect::WebAuthnCreateJsonInto { target, .. }
        | ClientBrowserEffect::WebAuthnGetJsonInto { target, .. }
        | ClientBrowserEffect::EventFileTextInto { target, .. } => Some(*target),
        _ => None,
    }
}

fn parse_canonical_expression(value: &Value, label: &str) -> Result<ClientExpr, String> {
    let expression = parse_client_expr_json(value).map_err(|error| format!("{label}: {error}"))?;
    let canonical = serde_json::from_str::<Value>(&client_expr_json(&expression))
        .map_err(|error| format!("{label}: canonical expression JSON was invalid: {error}"))?;
    if canonical != *value {
        return Err(format!("{label} is not canonical Client IR v3 JSON"));
    }
    Ok(expression)
}

fn parse_canonical_block(value: &Value, label: &str) -> Result<ClientBlock, String> {
    let block = parse_client_block_json(value).map_err(|error| format!("{label}: {error}"))?;
    let canonical = serde_json::from_str::<Value>(&client_block_json(&block))
        .map_err(|error| format!("{label}: canonical block JSON was invalid: {error}"))?;
    if canonical != *value {
        return Err(format!("{label} is not canonical Client IR v3 JSON"));
    }
    Ok(block)
}

#[derive(Default)]
struct UnsupportedFeatures {
    values: BTreeSet<&'static str>,
}

impl UnsupportedFeatures {
    fn insert(&mut self, feature: &'static str) {
        debug_assert!(UNSUPPORTED_FEATURE_ORDER.contains(&feature));
        self.values.insert(feature);
    }

    fn ordered(self) -> Vec<String> {
        UNSUPPORTED_FEATURE_ORDER
            .iter()
            .filter(|feature| self.values.contains(**feature))
            .map(|feature| (*feature).to_owned())
            .collect()
    }
}

fn require_node_kind(
    view: &ValidatedView,
    node: u32,
    expected: ViewNodeKind,
    label: &str,
) -> Result<(), String> {
    validate_node_bound(node, view.kinds.len(), label)?;
    let actual = view.kinds[node as usize];
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{label} targets node {node} with kind {actual:?}, expected {expected:?}"
        ))
    }
}

fn require_node_kind_one_of(
    view: &ValidatedView,
    node: u32,
    expected: &[ViewNodeKind],
    label: &str,
) -> Result<(), String> {
    validate_node_bound(node, view.kinds.len(), label)?;
    let actual = view.kinds[node as usize];
    if expected.contains(&actual) {
        Ok(())
    } else {
        Err(format!(
            "{label} targets node {node} with kind {actual:?}, expected one of {expected:?}"
        ))
    }
}

fn validate_node_bound(node: u32, count: usize, label: &str) -> Result<(), String> {
    if (node as usize) < count {
        Ok(())
    } else {
        Err(format!(
            "{label} refers to node {node}, but view_tree has {count} nodes"
        ))
    }
}

fn parse_node_ids(values: &[Value], label: &str) -> Result<Vec<u32>, String> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| as_u32(value, &format!("{label}[{index}]")))
        .collect()
}

fn require_declared(declarations: &BTreeSet<u32>, id: u32, label: &str) -> Result<(), String> {
    if declarations.contains(&id) {
        Ok(())
    } else {
        Err(format!("{label} refers to undeclared ID {id}"))
    }
}

fn require_increasing_id(previous: &mut Option<u32>, id: u32, label: &str) -> Result<(), String> {
    if previous.is_some_and(|previous| id <= previous) {
        return Err(format!(
            "{label} must use strictly increasing unique IDs; previous {previous:?}, got {id}"
        ));
    }
    *previous = Some(id);
    Ok(())
}

fn require_increasing_key(
    previous: &mut Option<(u32, String)>,
    key: &(u32, String),
    label: &str,
) -> Result<(), String> {
    if previous.as_ref().is_some_and(|previous| key <= previous) {
        return Err(format!(
            "{label} must use strictly increasing unique (node, name) keys; previous {previous:?}, got {key:?}"
        ));
    }
    *previous = Some(key.clone());
    Ok(())
}

fn is_known_event(event: &str) -> bool {
    matches!(
        event,
        "click"
            | "input"
            | "change"
            | "paste"
            | "focus"
            | "blur"
            | "keydown"
            | "keyup"
            | "mousedown"
            | "mouseup"
            | "mousemove"
            | "mouseover"
            | "mouseout"
            | "scroll"
            | "wheel"
            | "beforeinput"
            | "compositionstart"
            | "compositionupdate"
            | "compositionend"
            | "dragstart"
            | "dragenter"
            | "dragover"
            | "dragleave"
            | "drop"
            | "dragend"
            | "toggle"
            | "beforetoggle"
            | "animationstart"
            | "animationiteration"
            | "animationend"
            | "animationcancel"
            | "transitionrun"
            | "transitionstart"
            | "transitionend"
            | "transitioncancel"
            | "pointerdown"
            | "pointerup"
            | "pointermove"
            | "pointerover"
            | "pointerout"
            | "pointerenter"
            | "pointerleave"
            | "gotpointercapture"
            | "lostpointercapture"
            | "touchstart"
            | "touchmove"
            | "touchend"
            | "touchcancel"
            | "document_click"
            | "document_pointerdown"
            | "document_keydown"
            | "window_popstate"
            | "window_hashchange"
            | "window_storage"
            | "window_resize"
            | "window_scroll"
            | "submit"
    )
}

fn validate_optional_source_anchor(object: &Map<String, Value>, field: &str) -> Result<(), String> {
    parse_optional_source_anchor(object, field).map(|_| ())
}

fn parse_optional_source_anchor(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<SourceAnchor>, String> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let anchor = object_value(value, field)?;
    require_known_fields(field, anchor, &["start_column", "start_line"])?;
    Ok(Some(SourceAnchor {
        start_line: usize::try_from(required_u32(anchor, "start_line")?)
            .map_err(|_| "`start_line` exceeds usize::MAX".to_string())?,
        start_column: usize::try_from(required_u32(anchor, "start_column")?)
            .map_err(|_| "`start_column` exceeds usize::MAX".to_string())?,
    }))
}

fn require_known_fields(
    label: &str,
    object: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    let mut unknown = object
        .keys()
        .filter(|field| !allowed.contains(&field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{label} contains unknown field(s): {}",
            unknown.join(", ")
        ))
    }
}

fn object_value<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
        .as_object()
        .ok_or_else(|| format!("`{field}` must be an object"))
}

fn required_array<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a [Value], String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("`{field}` must be an array"))
}

fn optional_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a [Value]>, String> {
    object
        .get(field)
        .map(|value| {
            value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| format!("`{field}` must be an array"))
        })
        .transpose()
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
        .as_str()
        .ok_or_else(|| format!("`{field}` must be a string"))
}

fn require_exact_string(
    object: &Map<String, Value>,
    field: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = required_string(object, field)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!("`{field}` must be `{expected}`, found `{actual}`"))
    }
}

fn required_bool(object: &Map<String, Value>, field: &str) -> Result<bool, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
        .as_bool()
        .ok_or_else(|| format!("`{field}` must be a boolean"))
}

fn required_u64(object: &Map<String, Value>, field: &str) -> Result<u64, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?
        .as_u64()
        .ok_or_else(|| format!("`{field}` must be an unsigned integer"))
}

fn require_exact_u64(
    object: &Map<String, Value>,
    field: &str,
    expected: u64,
) -> Result<(), String> {
    let actual = required_u64(object, field)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!("`{field}` must be {expected}, found {actual}"))
    }
}

fn required_u32(object: &Map<String, Value>, field: &str) -> Result<u32, String> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("missing `{field}`"))?;
    as_u32(value, field)
}

fn optional_u32(object: &Map<String, Value>, field: &str) -> Result<Option<u32>, String> {
    object
        .get(field)
        .map(|value| as_u32(value, field))
        .transpose()
}

fn optional_u64(object: &Map<String, Value>, field: &str) -> Result<Option<u64>, String> {
    object
        .get(field)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| format!("`{field}` must be an unsigned integer"))
        })
        .transpose()
}

fn as_u32(value: &Value, label: &str) -> Result<u32, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("{label} must be an unsigned integer"))?;
    u32::try_from(value).map_err(|_| format!("{label} exceeds u32::MAX"))
}

#[cfg(test)]
mod tests;
