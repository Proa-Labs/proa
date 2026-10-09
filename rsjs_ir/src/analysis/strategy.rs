// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Render-strategy classification for a completed island IR.
//!
//! The strategy boundary is deliberately usage-driven. Client-capable
//! values and declared client-module imports only permit values to
//! cross into browser state; they do not force hydration by
//! themselves. Hydration is required when the lowered IR contains a
//! reachable browser root such as a binding, event handler binding,
//! query/mutation observation or action, interval, keyed list, or reusable
//! component-fragment mount.

use crate::abi::HelperCallEdgeId;
use crate::analysis::dep_collector::{collect_client_block_reactive_deps, collect_reactive_deps};
use crate::bindings::EventName;
use crate::client::{
    BrowserStorageArea, ClientBlock, ClientBrowserEffect, ClientBrowserRead, ClientExpr,
    ClientFunctionRef, ClientStmt, LocationField, StorageValueType, WindowField,
};
use crate::island::{ComponentFragmentMount, Island, PayloadAttr, SourceAnchor, ViewNode};
use crate::lists::ListSource;
use crate::reactive::{EventHandler, MutationDecl, QueryDecl, SignalDecl};
use crate::types::{EventHandlerId, ExprId, MutationId, NodeId, NodeRefId, QueryId, SignalId};

/// A parent-island reactive root contributed by a component fragment mount.
///
/// Mounted child fragments keep parent state alive in two ways that live
/// on the mount instead of in `island.exprs` / `island.handlers`, so any
/// consumer that only walks those two side-tables misses them.
pub(crate) enum MountReactiveRoot<'a> {
    /// A parent-side callback adapter body (`mount.callbacks[].body_ir()`).
    Block(&'a ClientBlock),
    /// A parent signal cell handed to the child fragment
    /// (`mount.signal_aliases[].parent`).
    ParentSignal(SignalId),
}

/// Mounts whose emitted JS lives in the parent island module.
///
/// Shared by reachability and by `optimize::dead_signal_elimination`
/// so the two cannot disagree about which mounts contribute roots.
pub(crate) fn parent_runtime_mounts(
    island: &Island,
) -> impl Iterator<Item = &ComponentFragmentMount> {
    island
        .component_fragment_mounts
        .iter()
        .filter(|mount| mount.requires_parent_runtime())
}

/// Enumerate the parent-island reactive roots a single mount keeps live.
///
/// Reachability (`collect_client_reachability_roots`) and
/// `optimize::dead_signal_elimination` must agree on this set by
/// construction. When they drifted apart, the pass deleted signals
/// reachability still reported live, and `emit_component_fragment_mounts`
/// emitted `__sig_<id>` references with no matching declaration: the mount
/// call threw `ReferenceError` at hydration and the child never mounted.
pub(crate) fn mount_reactive_roots(
    mount: &ComponentFragmentMount,
) -> impl Iterator<Item = MountReactiveRoot<'_>> {
    mount
        .callbacks
        .iter()
        .map(|callback| MountReactiveRoot::Block(callback.body_ir()))
        .chain(
            mount
                .signal_aliases
                .iter()
                .map(|alias| MountReactiveRoot::ParentSignal(alias.parent)),
        )
}

/// Strategy the compiler should use for an analyzed
/// `#[rsjs(component...)]` render-trait impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderStrategy {
    /// The impl has no client-reachable state/effects. Emit normal
    /// server-rendered HTML with no island wrapper, seed blob,
    /// per-island JS asset, manifest entry, or runtime tag.
    ServerOnly,
    /// Emit server HTML plus the minimal client asset needed to
    /// hydrate reachable signals, handlers, bindings, queries,
    /// mutations, refs, lists, and fragment mounts.
    SsrHydrate,
    /// Future explicit opt-in only. This classifier never returns it
    /// until the IR grows an author-controlled client-only policy.
    ClientOnlyExplicit,
}

/// One IR fact that makes browser JavaScript reachable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientReachabilityRoot {
    /// A text node reads a Client IR expression.
    TextBinding { node: NodeId },
    /// A dynamic attribute reads a Client IR expression.
    AttrBinding { node: NodeId, name: String },
    /// A DOM event on a node invokes a client handler.
    EventBinding { node: NodeId, event: EventName },
    /// A conditional section toggles server-rendered branches.
    ConditionalSection { parent: NodeId },
    /// A form control is two-way bound to a signal.
    InputBinding { node: NodeId },
    /// A template node is bound to a node-ref handle.
    NodeRefBinding { node: NodeId, node_ref: NodeRefId },
    /// A node-ref handle is read or controlled by reachable browser work.
    NodeRef { id: NodeRefId },
    /// A reactive signal is read or written by reachable browser work.
    Signal { id: SignalId },
    /// A reactive query is observed or controlled by reachable browser work.
    Query { id: QueryId },
    /// A reactive mutation is observed or run by reachable browser work.
    Mutation { id: MutationId },
    /// A browser-global read is evaluated by reachable browser work.
    BrowserRead { kind: String },
    /// A browser-global effect is executed by reachable browser work.
    BrowserEffect { kind: String },
    /// A client-module helper is called by reachable browser work.
    ClientModuleCall { module: String, name: String },
    /// A `#[rsjs::client]` helper call site is reachable from browser work.
    ///
    /// The authored path is diagnostic material only. The descriptor
    /// relocation for `edge` is the authoritative helper identity.
    ClientHelperCall {
        edge: HelperCallEdgeId,
        authored_path: String,
    },
    /// An island-lifetime interval executes browser-side work.
    Interval { index: usize },
    /// A keyed list reconciles DOM children in the browser.
    KeyedList { container: NodeId },
    /// A reusable component fragment is mounted from a client asset.
    ComponentFragmentMount { root: NodeId, asset_id: String },
}

/// One value serialized into browser-visible state because reachable
/// Client IR needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClientCrossing {
    /// Stable crossing kind.
    pub kind: ClientCrossingKind,
    /// Stable emitted browser surface.
    pub surface: ClientCrossingSurface,
    /// Type/codec requirement enforced at the crossing point.
    pub requirement: ClientCrossingRequirement,
    /// Redacted source label, such as a signal local name.
    pub label: Option<String>,
    /// Optional source location of the crossing expression.
    pub source_anchor: Option<SourceAnchor>,
}

/// One declared client-capable primitive that was not reachable from any
/// browser root.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeadClientPrimitive {
    /// Stable dead declaration category.
    pub kind: DeadClientPrimitiveKind,
    /// Redacted source label, such as a prelude local name.
    pub label: Option<String>,
    /// Optional source location of the declaration.
    pub source_anchor: Option<SourceAnchor>,
}

/// Stable dead declaration category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeadClientPrimitiveKind {
    Signal { signal: SignalId },
    Memo { signal: SignalId },
    Handler { handler: EventHandlerId },
    NodeRef { node_ref: NodeRefId },
    Query { query: QueryId },
    Mutation { mutation: MutationId },
}

/// Stable crossing category.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientCrossingKind {
    /// A reachable leaf signal's initial value is serialized into the
    /// island seed blob.
    SignalInitialValue { signal: SignalId },
    /// A DOM event handler reads a server-rendered payload attribute.
    HandlerPayload { node: NodeId, attr: String },
    /// A reachable node-local expression reads a server-rendered payload
    /// attribute from the bound DOM node.
    NodePayload { node: NodeId, attr: String },
    /// A reachable query's cache key is serialized into browser-visible
    /// query metadata.
    QueryKey { query: QueryId },
    /// A reachable query's client fetcher config is serialized into
    /// browser-visible query metadata.
    QueryFetcher { query: QueryId },
    /// A reachable query's server initial value is serialized into
    /// the dehydrated query seed blob.
    QueryInitialValue { query: QueryId },
    /// A reachable mutation's key is serialized into browser-visible
    /// mutation metadata.
    MutationKey { mutation: MutationId },
    /// A reachable mutation's client runner config is serialized into
    /// browser-visible mutation metadata.
    MutationRunner { mutation: MutationId },
    /// A reachable mutation's invalidation key is serialized into
    /// browser-visible mutation metadata.
    MutationInvalidationKey { mutation: MutationId, index: usize },
    /// A parent value is passed into a descriptor-backed child reactive
    /// prop and aliased into the child's browser fragment.
    DescriptorReactiveProp {
        root: NodeId,
        asset_id: String,
        prop: String,
        child_signal: SignalId,
        parent_signal: SignalId,
    },
}

/// Stable browser-visible target of a crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientCrossingSurface {
    SignalSeed,
    HandlerPayloadAttr,
    NodePayloadAttr,
    QueryMetadataBlob,
    QuerySeedBlob,
    MutationMetadataBlob,
    DescriptorReactiveProp,
}

/// Validation required at a crossing point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientCrossingRequirement {
    ClientEncode,
    QueryCodec,
    MutationCodec,
    RequestConfig,
}

/// Result of strategy classification, including the facts that forced
/// client JavaScript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAnalysis {
    /// Selected render strategy.
    pub strategy: RenderStrategy,
    /// Deterministic list of client reachability roots.
    pub roots: Vec<ClientReachabilityRoot>,
    /// Deterministic redacted ledger of public data crossings.
    pub crossings: Vec<ClientCrossing>,
    /// Deterministic redacted list of client-capable declarations that
    /// did not become browser-reachable.
    pub dead_primitives: Vec<DeadClientPrimitive>,
}

impl StrategyAnalysis {
    /// Whether this island can be emitted as server-only output.
    pub fn is_server_only(&self) -> bool {
        self.strategy == RenderStrategy::ServerOnly
    }

    /// Whether this island needs browser JavaScript.
    pub fn needs_client_js(&self) -> bool {
        self.strategy == RenderStrategy::SsrHydrate
    }

    /// Query declarations reachable from browser roots, in deterministic id
    /// order.
    pub fn reachable_query_ids(&self) -> Vec<QueryId> {
        reachable_query_ids_from_roots(&self.roots)
    }

    /// Mutation declarations reachable from browser roots, in deterministic id
    /// order.
    pub fn reachable_mutation_ids(&self) -> Vec<MutationId> {
        reachable_mutation_ids_from_roots(&self.roots)
    }

    /// Signal declarations reachable from browser roots, in deterministic id
    /// order.
    pub fn reachable_signal_ids(&self) -> Vec<SignalId> {
        reachable_signal_ids_from_roots(&self.roots)
    }

    /// Node-ref declarations reachable from browser roots, in deterministic
    /// id order.
    pub fn reachable_node_ref_ids(&self) -> Vec<NodeRefId> {
        reachable_node_ref_ids_from_roots(&self.roots)
    }

    /// Client-module imports reachable from browser roots, in deterministic
    /// module-name order.
    pub fn reachable_client_module_imports(&self) -> Vec<String> {
        let mut modules = Vec::new();
        for root in &self.roots {
            if let ClientReachabilityRoot::ClientModuleCall { module, .. } = root {
                modules.push(module.clone());
            }
        }
        modules.sort();
        modules.dedup();
        modules
    }

    /// Direct client-helper call sites reachable from browser roots, in
    /// deterministic component-local edge order.
    pub fn reachable_helper_call_edge_ids(&self) -> Vec<HelperCallEdgeId> {
        let mut edges = self
            .roots
            .iter()
            .filter_map(|root| match root {
                ClientReachabilityRoot::ClientHelperCall { edge, .. } => Some(*edge),
                _ => None,
            })
            .collect::<Vec<_>>();
        edges.sort_by_key(|edge| edge.index());
        edges.dedup();
        edges
    }

    /// Component fragment assets reachable from browser roots, in
    /// deterministic asset-id order.
    pub fn reachable_component_fragment_imports(&self) -> Vec<String> {
        let mut imports = Vec::new();
        for root in &self.roots {
            if let ClientReachabilityRoot::ComponentFragmentMount { asset_id, .. } = root {
                imports.push(asset_id.clone());
            }
        }
        imports.sort();
        imports.dedup();
        imports
    }

    /// Component fragment mounts reachable from browser roots, in
    /// deterministic `(root, asset_id)` order.
    pub fn reachable_component_fragment_mounts(&self) -> Vec<(NodeId, String)> {
        let mut mounts = Vec::new();
        for root in &self.roots {
            if let ClientReachabilityRoot::ComponentFragmentMount { root, asset_id } = root {
                mounts.push((*root, asset_id.clone()));
            }
        }
        mounts.sort_by(|(left_root, left_asset), (right_root, right_asset)| {
            left_root
                .0
                .cmp(&right_root.0)
                .then(left_asset.cmp(right_asset))
        });
        mounts.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        mounts
    }
}

/// Classify a completed island by its reachable client roots.
///
/// This pass intentionally ignores bare signal/query/mutation declarations,
/// unused handler declarations, and declared-but-unused client-module imports.
/// Those facts are capabilities or dead declarations; they become browser work
/// only when a reachable side-table entry points at them.
pub fn classify_render_strategy(island: &Island) -> StrategyAnalysis {
    let scope = ReachabilityScope::for_island(island);
    let roots = client_reachability_roots_with_scope(island, &scope);
    let strategy = if roots.is_empty() {
        RenderStrategy::ServerOnly
    } else {
        RenderStrategy::SsrHydrate
    };
    let crossings = client_crossings(island, &roots);
    let dead_primitives = dead_client_primitives(island, &roots, &scope);

    StrategyAnalysis {
        strategy,
        roots,
        crossings,
        dead_primitives,
    }
}

/// Return declarations that stayed outside the reachable client graph.
fn dead_client_primitives(
    island: &Island,
    roots: &[ClientReachabilityRoot],
    scope: &ReachabilityScope,
) -> Vec<DeadClientPrimitive> {
    let reachable_signals = reachable_signal_ids_from_roots(roots);
    let reachable_handlers = reachable_handler_ids(island, scope);
    let reachable_node_refs = reachable_node_ref_ids_from_roots(roots);
    let reachable_queries = reachable_query_ids_from_roots(roots);
    let reachable_mutations = reachable_mutation_ids_from_roots(roots);

    let mut dead = Vec::new();
    let mut signals: Vec<_> = island.signals.iter().collect();
    signals.sort_by_key(|signal| signal.id.0);
    for signal in signals {
        if !scope.includes_signal(signal.id) || reachable_signals.contains(&signal.id) {
            continue;
        }
        dead.push(DeadClientPrimitive {
            kind: if signal.derived.is_some() {
                DeadClientPrimitiveKind::Memo { signal: signal.id }
            } else {
                DeadClientPrimitiveKind::Signal { signal: signal.id }
            },
            label: signal.source_name.clone(),
            source_anchor: signal.source_anchor,
        });
    }

    let mut handlers: Vec<_> = island.handlers.iter().collect();
    handlers.sort_by_key(|handler| handler.id.0);
    for handler in handlers {
        if !scope.includes_handler(handler.id) || reachable_handlers.contains(&handler.id) {
            continue;
        }
        dead.push(DeadClientPrimitive {
            kind: DeadClientPrimitiveKind::Handler {
                handler: handler.id,
            },
            label: handler.source_name.clone(),
            source_anchor: handler.source_anchor,
        });
    }

    let mut node_refs: Vec<_> = island.node_refs.iter().collect();
    node_refs.sort_by_key(|node_ref| node_ref.id.0);
    for node_ref in node_refs {
        if !scope.includes_node_ref(node_ref.id) || reachable_node_refs.contains(&node_ref.id) {
            continue;
        }
        dead.push(DeadClientPrimitive {
            kind: DeadClientPrimitiveKind::NodeRef {
                node_ref: node_ref.id,
            },
            label: node_ref.source_name.clone(),
            source_anchor: node_ref.source_anchor,
        });
    }

    let mut queries: Vec<_> = island.queries.iter().collect();
    queries.sort_by_key(|query| query.id.0);
    for query in queries {
        if reachable_queries.contains(&query.id) {
            continue;
        }
        dead.push(DeadClientPrimitive {
            kind: DeadClientPrimitiveKind::Query { query: query.id },
            label: query.source_name.clone(),
            source_anchor: query.source_anchor,
        });
    }

    let mut mutations: Vec<_> = island.mutations.iter().collect();
    mutations.sort_by_key(|mutation| mutation.id.0);
    for mutation in mutations {
        if reachable_mutations.contains(&mutation.id) {
            continue;
        }
        dead.push(DeadClientPrimitive {
            kind: DeadClientPrimitiveKind::Mutation {
                mutation: mutation.id,
            },
            label: mutation.source_name.clone(),
            source_anchor: mutation.source_anchor,
        });
    }

    dead
}

fn reachable_handler_ids(island: &Island, scope: &ReachabilityScope) -> Vec<EventHandlerId> {
    let mut ids: Vec<_> = island
        .bindings
        .event_bindings
        .iter()
        .filter_map(|((node, _), handler)| {
            (scope.includes_node(*node) && scope.includes_handler(*handler)).then_some(*handler)
        })
        .collect();
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

fn reachable_node_ref_ids_from_roots(roots: &[ClientReachabilityRoot]) -> Vec<NodeRefId> {
    let mut ids = Vec::new();
    for root in roots {
        match root {
            ClientReachabilityRoot::NodeRefBinding { node_ref, .. } => ids.push(*node_ref),
            ClientReachabilityRoot::NodeRef { id } => ids.push(*id),
            _ => {}
        }
    }
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

fn reachable_signal_ids_from_roots(roots: &[ClientReachabilityRoot]) -> Vec<SignalId> {
    let mut ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Signal { id } = root {
            ids.push(*id);
        }
    }
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

fn reachable_query_ids_from_roots(roots: &[ClientReachabilityRoot]) -> Vec<QueryId> {
    let mut ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Query { id } = root {
            ids.push(*id);
        }
    }
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

fn reachable_mutation_ids_from_roots(roots: &[ClientReachabilityRoot]) -> Vec<MutationId> {
    let mut ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Mutation { id } = root {
            ids.push(*id);
        }
    }
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

/// Return the deterministic public-data crossings implied by reachable roots.
pub fn client_crossings(island: &Island, roots: &[ClientReachabilityRoot]) -> Vec<ClientCrossing> {
    let mut signal_ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Signal { id } = root {
            signal_ids.push(*id);
        }
    }
    signal_ids.sort_by_key(|id| id.0);
    signal_ids.dedup();

    let mut crossings = Vec::new();
    for id in signal_ids {
        let Some(signal) = signal_decl(island, id) else {
            continue;
        };
        if signal.derived.is_some() || signal.shared.is_some() {
            continue;
        }
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::SignalInitialValue { signal: id },
            surface: ClientCrossingSurface::SignalSeed,
            requirement: ClientCrossingRequirement::ClientEncode,
            label: signal.source_name.clone(),
            source_anchor: signal.source_anchor,
        });
    }

    let mut query_ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Query { id } = root {
            query_ids.push(*id);
        }
    }
    query_ids.sort_by_key(|id| id.0);
    query_ids.dedup();
    for id in query_ids {
        let Some(query) = query_decl(island, id) else {
            continue;
        };
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::QueryKey { query: id },
            surface: ClientCrossingSurface::QueryMetadataBlob,
            requirement: ClientCrossingRequirement::QueryCodec,
            label: query.source_name.clone(),
            source_anchor: query.source_anchor,
        });
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::QueryFetcher { query: id },
            surface: ClientCrossingSurface::QueryMetadataBlob,
            requirement: ClientCrossingRequirement::RequestConfig,
            label: query
                .source_name
                .as_ref()
                .map(|name| format!("{name}.fetcher")),
            source_anchor: query.source_anchor,
        });
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::QueryInitialValue { query: id },
            surface: ClientCrossingSurface::QuerySeedBlob,
            requirement: ClientCrossingRequirement::QueryCodec,
            label: query.source_name.clone(),
            source_anchor: query.source_anchor,
        });
    }

    let mut mutation_ids = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::Mutation { id } = root {
            mutation_ids.push(*id);
        }
    }
    mutation_ids.sort_by_key(|id| id.0);
    mutation_ids.dedup();
    for id in mutation_ids {
        let Some(mutation) = mutation_decl(island, id) else {
            continue;
        };
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::MutationKey { mutation: id },
            surface: ClientCrossingSurface::MutationMetadataBlob,
            requirement: ClientCrossingRequirement::MutationCodec,
            label: mutation.source_name.clone(),
            source_anchor: mutation.source_anchor,
        });
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::MutationRunner { mutation: id },
            surface: ClientCrossingSurface::MutationMetadataBlob,
            requirement: ClientCrossingRequirement::RequestConfig,
            label: mutation
                .source_name
                .as_ref()
                .map(|name| format!("{name}.runner")),
            source_anchor: mutation.source_anchor,
        });
        for (index, _) in mutation.invalidates.iter().enumerate() {
            crossings.push(ClientCrossing {
                kind: ClientCrossingKind::MutationInvalidationKey {
                    mutation: id,
                    index,
                },
                surface: ClientCrossingSurface::MutationMetadataBlob,
                requirement: ClientCrossingRequirement::MutationCodec,
                label: mutation
                    .source_name
                    .as_ref()
                    .map(|name| format!("{name}.invalidates[{index}]")),
                source_anchor: mutation.source_anchor,
            });
        }
    }

    crossings.extend(node_payload_crossings(island, roots));
    crossings.extend(descriptor_reactive_prop_crossings(island, roots));
    crossings.extend(handler_payload_crossings(island, roots));
    crossings
}

fn node_payload_crossings(
    island: &Island,
    roots: &[ClientReachabilityRoot],
) -> Vec<ClientCrossing> {
    let mut crossings = Vec::new();
    for root in roots {
        match root {
            ClientReachabilityRoot::TextBinding { node } => {
                if let Some(expr) = island.bindings.text_bindings.get(node) {
                    collect_node_payload_crossings_from_expr(island, *node, *expr, &mut crossings);
                }
            }
            ClientReachabilityRoot::AttrBinding { node, name } => {
                if let Some(binding) = island.bindings.attr_bindings.get(&(*node, name.clone())) {
                    collect_node_payload_crossings_from_expr(
                        island,
                        *node,
                        binding.expr,
                        &mut crossings,
                    );
                }
            }
            ClientReachabilityRoot::ConditionalSection { parent } => {
                if let Some(sections) = island.bindings.conditional_sections.get(parent) {
                    for section in sections {
                        collect_node_payload_crossings_from_expr(
                            island,
                            *parent,
                            section.condition,
                            &mut crossings,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    crossings
}

fn collect_node_payload_crossings_from_expr(
    island: &Island,
    node: NodeId,
    expr: crate::types::ExprId,
    crossings: &mut Vec<ClientCrossing>,
) {
    let mut attrs = Vec::new();
    collect_node_payload_attrs_from_expr(island.exprs.client_expr(expr), &mut attrs);
    attrs.sort();
    attrs.dedup();
    for attr_name in attrs {
        let Some(attr) = payload_attr(island, node, &attr_name) else {
            continue;
        };
        if crossings.iter().any(|crossing| match &crossing.kind {
            ClientCrossingKind::NodePayload {
                node: existing_node,
                attr: existing_attr,
            } => *existing_node == node && existing_attr == &attr.name,
            _ => false,
        }) {
            continue;
        }
        crossings.push(ClientCrossing {
            kind: ClientCrossingKind::NodePayload {
                node,
                attr: attr.name.clone(),
            },
            surface: ClientCrossingSurface::NodePayloadAttr,
            requirement: ClientCrossingRequirement::ClientEncode,
            label: attr.label.clone(),
            source_anchor: attr.source_anchor,
        });
    }
}

fn descriptor_reactive_prop_crossings(
    island: &Island,
    roots: &[ClientReachabilityRoot],
) -> Vec<ClientCrossing> {
    let mut reachable_mounts = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::ComponentFragmentMount { root, asset_id } = root {
            reachable_mounts.push((*root, asset_id.as_str()));
        }
    }
    if reachable_mounts.is_empty() {
        return Vec::new();
    }

    let mut crossings = Vec::new();
    let mut mounts: Vec<_> = island.component_fragment_mounts.iter().collect();
    mounts.sort_by(|a, b| a.root.0.cmp(&b.root.0).then(a.asset_id.cmp(&b.asset_id)));
    for mount in mounts {
        if !reachable_mounts
            .iter()
            .any(|(root, asset_id)| *root == mount.root && *asset_id == mount.asset_id.as_str())
        {
            continue;
        }
        let mut aliases = mount.signal_aliases.clone();
        aliases.sort_by(|left, right| {
            left.child.0.cmp(&right.child.0).then_with(|| {
                left.prop_name
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.prop_name.as_deref().unwrap_or(""))
            })
        });
        for alias in aliases {
            let prop = alias
                .prop_name
                .clone()
                .unwrap_or_else(|| format!("signal_{}", alias.child.0));
            crossings.push(ClientCrossing {
                kind: ClientCrossingKind::DescriptorReactiveProp {
                    root: mount.root,
                    asset_id: mount.asset_id.clone(),
                    prop: prop.clone(),
                    child_signal: alias.child,
                    parent_signal: alias.parent,
                },
                surface: ClientCrossingSurface::DescriptorReactiveProp,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some(format!("{}::{prop}", mount.asset_id)),
                source_anchor: alias.source_anchor,
            });
        }
    }
    crossings
}

fn handler_payload_crossings(
    island: &Island,
    roots: &[ClientReachabilityRoot],
) -> Vec<ClientCrossing> {
    let mut crossings: Vec<ClientCrossing> = Vec::new();
    let mut event_bindings = Vec::new();
    for root in roots {
        if let ClientReachabilityRoot::EventBinding { node, event } = root {
            let Some(handler) = island.bindings.event_bindings.get(&(*node, *event)) else {
                continue;
            };
            event_bindings.push((*node, *event, *handler));
        }
    }
    event_bindings.sort_by_key(|(node, event, _)| (node.0, event_order(*event)));
    event_bindings.dedup_by_key(|(node, event, handler)| (node.0, event_order(*event), handler.0));

    for (node, _event, handler) in event_bindings {
        let Some(handler) = handler_by_id(island, handler) else {
            continue;
        };
        let mut attrs = Vec::new();
        collect_event_payload_attrs_from_block(handler.body_ir(), &mut attrs);
        attrs.sort();
        attrs.dedup();
        for attr_name in attrs {
            let Some(attr) = payload_attr(island, node, &attr_name) else {
                continue;
            };
            if crossings.iter().any(|crossing| match &crossing.kind {
                ClientCrossingKind::HandlerPayload {
                    node: existing_node,
                    attr: existing_attr,
                } => *existing_node == node && existing_attr == &attr.name,
                _ => false,
            }) {
                continue;
            }
            crossings.push(ClientCrossing {
                kind: ClientCrossingKind::HandlerPayload {
                    node,
                    attr: attr.name.clone(),
                },
                surface: ClientCrossingSurface::HandlerPayloadAttr,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: attr.label.clone(),
                source_anchor: attr.source_anchor,
            });
        }
    }

    crossings
}

fn payload_attr<'a>(island: &'a Island, node: NodeId, name: &str) -> Option<&'a PayloadAttr> {
    match island.view_tree.get(node) {
        ViewNode::Element { payload_attrs, .. } => {
            payload_attrs.iter().find(|attr| attr.name == name)
        }
        _ => None,
    }
}

fn collect_event_payload_attrs_from_block(block: &ClientBlock, attrs: &mut Vec<String>) {
    for stmt in &block.stmts {
        collect_event_payload_attrs_from_stmt(stmt, attrs);
    }
}

fn collect_event_payload_attrs_from_stmt(stmt: &ClientStmt, attrs: &mut Vec<String>) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            collect_event_payload_attrs_from_expr(expr, attrs);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_event_payload_attrs_from_expr(value, attrs);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_event_payload_attrs_from_expr(&closure.body, attrs);
        }
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                collect_event_payload_attrs_from_expr(arg, attrs);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_event_payload_attrs_from_expr(payload, attrs);
        }
        ClientStmt::BrowserEffect(effect) => {
            collect_event_payload_attrs_from_browser_effect(effect, attrs)
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            collect_event_payload_attrs_from_node_ref_effect(effect, attrs);
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_event_payload_attrs_from_expr(condition, attrs);
            collect_event_payload_attrs_from_block(then_block, attrs);
            if let Some(else_block) = else_block {
                collect_event_payload_attrs_from_block(else_block, attrs);
            }
        }
        ClientStmt::Block(block) => collect_event_payload_attrs_from_block(block, attrs),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_event_payload_attrs_from_expr(start, attrs);
            collect_event_payload_attrs_from_expr(end, attrs);
            collect_event_payload_attrs_from_block(body, attrs);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
    }
}

fn collect_event_payload_attrs_from_expr(expr: &ClientExpr, attrs: &mut Vec<String>) {
    match expr {
        ClientExpr::EventPayload { attr } => attrs.push(attr.clone()),
        ClientExpr::Call { args, .. } => {
            for arg in args {
                collect_event_payload_attrs_from_expr(arg, attrs);
            }
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
            collect_event_payload_attrs_from_expr(value, attrs);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_event_payload_attrs_from_expr(condition, attrs);
            collect_event_payload_attrs_from_expr(then_expr, attrs);
            collect_event_payload_attrs_from_expr(else_expr, attrs);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        }
        | ClientExpr::Index {
            base: haystack,
            index: needle,
        } => {
            collect_event_payload_attrs_from_expr(haystack, attrs);
            collect_event_payload_attrs_from_expr(needle, attrs);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_event_payload_attrs_from_expr(part, attrs);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_event_payload_attrs_from_expr(&field.value, attrs);
            }
        }
        ClientExpr::BrowserRead(read) => collect_event_payload_attrs_from_browser_read(read, attrs),
        ClientExpr::DomRead { target, .. } => {
            collect_event_payload_attrs_from_expr(target, attrs);
        }
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_event_payload_attrs_from_expr(index, attrs);
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
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
        | ClientExpr::CallbackPayload
        | ClientExpr::ArrayLit(_)
        | ClientExpr::NodePayload { .. } => {}
    }
}

fn collect_node_payload_attrs_from_expr(expr: &ClientExpr, attrs: &mut Vec<String>) {
    match expr {
        ClientExpr::NodePayload { attr, .. } => attrs.push(attr.clone()),
        ClientExpr::Call { args, .. } => {
            for arg in args {
                collect_node_payload_attrs_from_expr(arg, attrs);
            }
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
            collect_node_payload_attrs_from_expr(value, attrs);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_node_payload_attrs_from_expr(condition, attrs);
            collect_node_payload_attrs_from_expr(then_expr, attrs);
            collect_node_payload_attrs_from_expr(else_expr, attrs);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        }
        | ClientExpr::Index {
            base: haystack,
            index: needle,
        } => {
            collect_node_payload_attrs_from_expr(haystack, attrs);
            collect_node_payload_attrs_from_expr(needle, attrs);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_node_payload_attrs_from_expr(part, attrs);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_node_payload_attrs_from_expr(&field.value, attrs);
            }
        }
        ClientExpr::BrowserRead(read) => collect_node_payload_attrs_from_browser_read(read, attrs),
        ClientExpr::DomRead { target, .. } => {
            collect_node_payload_attrs_from_expr(target, attrs);
        }
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_node_payload_attrs_from_expr(index, attrs);
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
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
        | ClientExpr::ArrayLit(_) => {}
    }
}

fn collect_node_payload_attrs_from_browser_read(read: &ClientBrowserRead, attrs: &mut Vec<String>) {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => {}
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_node_payload_attrs_from_expr(key, attrs);
            if let Some(fallback) = fallback {
                collect_node_payload_attrs_from_expr(fallback, attrs);
            }
        }
        ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_node_payload_attrs_from_expr(arg, attrs);
            }
        }
    }
}

fn collect_event_payload_attrs_from_browser_read(
    read: &ClientBrowserRead,
    attrs: &mut Vec<String>,
) {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => {}
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_event_payload_attrs_from_expr(key, attrs);
            if let Some(fallback) = fallback {
                collect_event_payload_attrs_from_expr(fallback, attrs);
            }
        }
        ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_event_payload_attrs_from_expr(arg, attrs);
            }
        }
    }
}

fn collect_event_payload_attrs_from_browser_effect(
    effect: &ClientBrowserEffect,
    attrs: &mut Vec<String>,
) {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            collect_event_payload_attrs_from_expr(x, attrs);
            collect_event_payload_attrs_from_expr(y, attrs);
        }
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => {}
        ClientBrowserEffect::LocationSetHash { value }
        | ClientBrowserEffect::LocationAssign { url: value }
        | ClientBrowserEffect::LocationReplace { url: value }
        | ClientBrowserEffect::HistoryPushState { url: value }
        | ClientBrowserEffect::HistoryReplaceState { url: value }
        | ClientBrowserEffect::HistoryGo { delta: value }
        | ClientBrowserEffect::DocumentSetTitle { title: value }
        | ClientBrowserEffect::ClipboardWriteText { text: value }
        | ClientBrowserEffect::StorageRemoveItem { key: value, .. }
        | ClientBrowserEffect::UrlDeleteSearchParam { key: value, .. } => {
            collect_event_payload_attrs_from_expr(value, attrs);
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            collect_event_payload_attrs_from_expr(name, attrs);
            if let Some(detail) = detail {
                collect_event_payload_attrs_from_expr(detail, attrs);
            }
        }
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                collect_event_payload_attrs_from_expr(fallback, attrs);
            }
        }
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
            collect_event_payload_attrs_from_expr(options_json, attrs);
            if let Some(fallback) = fallback {
                collect_event_payload_attrs_from_expr(fallback, attrs);
            }
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. }
        | ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            collect_event_payload_attrs_from_expr(key, attrs);
            collect_event_payload_attrs_from_expr(value, attrs);
        }
        ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                collect_event_payload_attrs_from_expr(arg, attrs);
            }
        }
    }
}

fn collect_event_payload_attrs_from_node_ref_effect(
    effect: &crate::client::ClientNodeRefEffect,
    attrs: &mut Vec<String>,
) {
    match effect {
        crate::client::ClientNodeRefEffect::Focus
        | crate::client::ClientNodeRefEffect::Blur
        | crate::client::ClientNodeRefEffect::ScrollIntoView
        | crate::client::ClientNodeRefEffect::Click
        | crate::client::ClientNodeRefEffect::RequestSubmit
        | crate::client::ClientNodeRefEffect::Reset
        | crate::client::ClientNodeRefEffect::ReportValidity
        | crate::client::ClientNodeRefEffect::ShowModal
        | crate::client::ClientNodeRefEffect::Close
        | crate::client::ClientNodeRefEffect::ShowPopover
        | crate::client::ClientNodeRefEffect::HidePopover
        | crate::client::ClientNodeRefEffect::TogglePopover
        | crate::client::ClientNodeRefEffect::Select
        | crate::client::ClientNodeRefEffect::RemoveAttribute { .. } => {}
        crate::client::ClientNodeRefEffect::ScrollTo { x, y }
        | crate::client::ClientNodeRefEffect::ScrollBy { x, y } => {
            collect_event_payload_attrs_from_expr(x, attrs);
            collect_event_payload_attrs_from_expr(y, attrs);
        }
        crate::client::ClientNodeRefEffect::SetPointerCapture(expr)
        | crate::client::ClientNodeRefEffect::ReleasePointerCapture(expr)
        | crate::client::ClientNodeRefEffect::SetValue(expr)
        | crate::client::ClientNodeRefEffect::SetChecked(expr)
        | crate::client::ClientNodeRefEffect::SetCustomValidity(expr)
        | crate::client::ClientNodeRefEffect::SetTextContent(expr)
        | crate::client::ClientNodeRefEffect::AddClass(expr)
        | crate::client::ClientNodeRefEffect::RemoveClass(expr)
        | crate::client::ClientNodeRefEffect::ToggleClass(expr) => {
            collect_event_payload_attrs_from_expr(expr, attrs);
        }
        crate::client::ClientNodeRefEffect::SetAttribute { value, .. }
        | crate::client::ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            collect_event_payload_attrs_from_expr(value, attrs);
        }
        crate::client::ClientNodeRefEffect::SetSelectionRange(start, end) => {
            collect_event_payload_attrs_from_expr(start, attrs);
            collect_event_payload_attrs_from_expr(end, attrs);
        }
    }
}

/// Return the deterministic set of IR roots that require browser JS.
pub fn client_reachability_roots(island: &Island) -> Vec<ClientReachabilityRoot> {
    let scope = ReachabilityScope::for_island(island);
    client_reachability_roots_with_scope(island, &scope)
}

/// Return browser reachability after adding compiler-authenticated fragment
/// roots to the island's ordinary local roots.
///
/// Linked component composition creates browser work that is intentionally
/// absent from the producer's local template: exported signals, lifecycle
/// predicates, and payload adapters are consumed by a separate owner module.
/// Every compiler phase that reasons about that work must use this recursive
/// closure rather than manually unioning direct dependencies. In particular,
/// the closure follows derived signals and query/mutation metadata, so helper
/// calls and imports reached only through those declarations cannot be lost.
///
/// `forced_blocks` remain producer-local Client IR. They are borrowed instead
/// of copied so descriptor retention and final emission walk the exact same
/// bodies.
pub fn client_reachability_roots_with_linked_roots<'a>(
    island: &Island,
    forced_signal_ids: &[SignalId],
    forced_node_ref_ids: &[NodeRefId],
    forced_expression_ids: &[ExprId],
    forced_blocks: impl IntoIterator<Item = &'a ClientBlock>,
) -> Vec<ClientReachabilityRoot> {
    let scope = ReachabilityScope::for_island(island);
    let mut roots = Vec::new();
    let mut reactive = ReactiveReachability::default();

    collect_client_reachability_roots(island, &scope, &mut roots, &mut reactive);

    let mut forced_signal_ids = forced_signal_ids.to_vec();
    forced_signal_ids.sort_by_key(|id| id.0);
    forced_signal_ids.dedup();
    for signal in forced_signal_ids {
        collect_reachable_signal_deps(island, signal, &scope, &mut reactive);
    }

    let mut forced_node_ref_ids = forced_node_ref_ids.to_vec();
    forced_node_ref_ids.sort_by_key(|id| id.0);
    forced_node_ref_ids.dedup();
    for node_ref in forced_node_ref_ids {
        if scope.includes_node_ref(node_ref) {
            reactive.record_node_ref(node_ref);
        }
    }

    let mut forced_expression_ids = forced_expression_ids.to_vec();
    forced_expression_ids.sort_by_key(|id| id.0);
    forced_expression_ids.dedup();
    for expression in forced_expression_ids {
        collect_reachable_expr_deps(island, expression, &scope, &mut reactive);
    }

    for block in forced_blocks {
        collect_reachable_block_deps(island, block, &scope, &mut reactive);
    }

    append_reactive_roots(&mut roots, &reactive);
    roots
}

fn client_reachability_roots_with_scope(
    island: &Island,
    scope: &ReachabilityScope,
) -> Vec<ClientReachabilityRoot> {
    let mut roots = Vec::new();
    let mut reactive = ReactiveReachability::default();

    collect_client_reachability_roots(island, scope, &mut roots, &mut reactive);
    roots
}

fn collect_client_reachability_roots(
    island: &Island,
    scope: &ReachabilityScope,
    roots: &mut Vec<ClientReachabilityRoot>,
    reactive: &mut ReactiveReachability,
) {
    let mut text_nodes: Vec<_> = island
        .bindings
        .text_bindings
        .keys()
        .copied()
        .filter(|node| scope.includes_node(*node))
        .collect();
    text_nodes.sort_by_key(|node| node.0);
    for node in text_nodes {
        roots.push(ClientReachabilityRoot::TextBinding { node });
        if let Some(expr) = island.bindings.text_bindings.get(&node) {
            collect_reachable_expr_deps(island, *expr, scope, reactive);
        }
    }

    let mut attr_bindings: Vec<_> = island
        .bindings
        .attr_bindings
        .iter()
        .map(|((node, name), binding)| (*node, name.clone(), binding.expr))
        .filter(|(node, _, _)| scope.includes_node(*node))
        .collect();
    attr_bindings.sort_by(|(left_node, left_name, _), (right_node, right_name, _)| {
        left_node
            .0
            .cmp(&right_node.0)
            .then_with(|| left_name.cmp(right_name))
    });
    for (node, name, expr) in attr_bindings {
        roots.push(ClientReachabilityRoot::AttrBinding { node, name });
        collect_reachable_expr_deps(island, expr, scope, reactive);
    }

    let mut event_bindings: Vec<_> = island
        .bindings
        .event_bindings
        .iter()
        .map(|((node, event), handler)| (*node, *event, *handler))
        .filter(|(node, _, handler)| scope.includes_node(*node) && scope.includes_handler(*handler))
        .collect();
    event_bindings.sort_by_key(|(node, event, _)| (node.0, event_order(*event)));
    for (node, event, handler) in event_bindings {
        roots.push(ClientReachabilityRoot::EventBinding { node, event });
        if let Some(handler) = handler_by_id(island, handler) {
            collect_reachable_block_deps(island, handler.body_ir(), scope, reactive);
        }
    }

    let mut conditionals: Vec<_> = island
        .bindings
        .conditional_sections
        .iter()
        .map(|(parent, sections)| (*parent, sections))
        .filter(|(parent, _)| scope.includes_node(*parent))
        .collect();
    conditionals.sort_by_key(|(parent, _)| parent.0);
    for (parent, sections) in conditionals {
        roots.push(ClientReachabilityRoot::ConditionalSection { parent });
        for section in sections {
            collect_reachable_expr_deps(island, section.condition, scope, reactive);
        }
    }

    let mut input_bindings: Vec<_> = island
        .bindings
        .input_bindings
        .values()
        .map(|binding| (binding.node, binding.signal))
        .filter(|(node, signal)| scope.includes_node(*node) && scope.includes_signal(*signal))
        .collect();
    input_bindings.sort_by_key(|(node, signal)| (node.0, signal.0));
    for (node, signal) in input_bindings {
        roots.push(ClientReachabilityRoot::InputBinding { node });
        collect_reachable_signal_deps(island, signal, scope, reactive);
    }

    let mut node_ref_bindings: Vec<_> = island
        .bindings
        .node_ref_bindings
        .values()
        .map(|binding| (binding.node, binding.node_ref))
        .filter(|(node, node_ref)| scope.includes_node(*node) && scope.includes_node_ref(*node_ref))
        .collect();
    node_ref_bindings.sort_by_key(|(node, node_ref)| (node.0, node_ref.0));
    roots.extend(
        node_ref_bindings
            .into_iter()
            .map(|(node, node_ref)| ClientReachabilityRoot::NodeRefBinding { node, node_ref }),
    );

    let mut interval_roots = Vec::new();
    for (index, interval) in island.intervals.iter().enumerate() {
        interval_roots.push(ClientReachabilityRoot::Interval { index });
        collect_reachable_block_deps(island, interval.body_ir(), scope, reactive);
    }

    let mut list_roots = Vec::new();
    for list in &island.lists {
        if !scope.includes_node(list.container) {
            continue;
        }
        list_roots.push(ClientReachabilityRoot::KeyedList {
            container: list.container,
        });
        match list.source {
            ListSource::Signal(signal) => {
                collect_reachable_signal_deps(island, signal, scope, reactive);
            }
            ListSource::Expr(expr) => {
                collect_reachable_expr_deps(island, expr, scope, reactive);
            }
        }
    }

    let mut fragment_roots = Vec::new();
    for mount in parent_runtime_mounts(island) {
        fragment_roots.push(ClientReachabilityRoot::ComponentFragmentMount {
            root: mount.root,
            asset_id: mount.asset_id.clone(),
        });
        // Shared with `optimize::dead_signal_elimination` so the pass
        // cannot delete a signal this analysis still reports live.
        for root in mount_reactive_roots(mount) {
            match root {
                MountReactiveRoot::Block(block) => {
                    collect_reachable_block_deps(island, block, scope, reactive);
                }
                MountReactiveRoot::ParentSignal(signal) => {
                    collect_reachable_signal_deps(island, signal, scope, reactive);
                }
            }
        }
    }

    append_reactive_roots(roots, reactive);
    roots.extend(interval_roots);
    roots.extend(list_roots);
    roots.extend(fragment_roots);
}

fn append_reactive_roots(roots: &mut Vec<ClientReachabilityRoot>, reactive: &ReactiveReachability) {
    let additions = reactive
        .node_ref_roots()
        .into_iter()
        .map(|id| ClientReachabilityRoot::NodeRef { id })
        .chain(
            reactive
                .signal_roots()
                .into_iter()
                .map(|id| ClientReachabilityRoot::Signal { id }),
        )
        .chain(
            reactive
                .query_roots()
                .into_iter()
                .map(|id| ClientReachabilityRoot::Query { id }),
        )
        .chain(
            reactive
                .mutation_roots()
                .into_iter()
                .map(|id| ClientReachabilityRoot::Mutation { id }),
        )
        .chain(
            reactive
                .browser_read_roots()
                .into_iter()
                .map(|kind| ClientReachabilityRoot::BrowserRead { kind }),
        )
        .chain(
            reactive
                .browser_effect_roots()
                .into_iter()
                .map(|kind| ClientReachabilityRoot::BrowserEffect { kind }),
        )
        .chain(
            reactive
                .client_helper_roots()
                .into_iter()
                .map(
                    |(edge, authored_path)| ClientReachabilityRoot::ClientHelperCall {
                        edge,
                        authored_path,
                    },
                ),
        )
        .chain(
            reactive
                .client_module_roots()
                .into_iter()
                .map(|(module, name)| ClientReachabilityRoot::ClientModuleCall { module, name }),
        );

    for addition in additions {
        if !roots.contains(&addition) {
            roots.push(addition);
        }
    }
}

fn event_order(event: EventName) -> usize {
    event as usize
}

#[derive(Default)]
struct ReachabilityScope {
    excluded_nodes: Vec<NodeId>,
    excluded_signals: Vec<SignalId>,
    excluded_handlers: Vec<EventHandlerId>,
    excluded_node_refs: Vec<NodeRefId>,
}

impl ReachabilityScope {
    fn for_island(island: &Island) -> Self {
        let mut scope = Self::default();
        for mount in &island.component_fragment_mounts {
            scope
                .excluded_nodes
                .extend(mount.owned_nodes.iter().copied());
            scope
                .excluded_signals
                .extend(mount.owned_signals.iter().copied());
            scope
                .excluded_handlers
                .extend(mount.owned_handlers.iter().copied());
            scope
                .excluded_node_refs
                .extend(mount.owned_node_refs.iter().copied());
        }
        scope.excluded_nodes.sort_by_key(|id| id.0);
        scope.excluded_nodes.dedup();
        scope.excluded_signals.sort_by_key(|id| id.0);
        scope.excluded_signals.dedup();
        scope.excluded_handlers.sort_by_key(|id| id.0);
        scope.excluded_handlers.dedup();
        scope.excluded_node_refs.sort_by_key(|id| id.0);
        scope.excluded_node_refs.dedup();
        scope
    }

    fn includes_node(&self, id: NodeId) -> bool {
        !self.excluded_nodes.contains(&id)
    }

    fn includes_signal(&self, id: SignalId) -> bool {
        !self.excluded_signals.contains(&id)
    }

    fn includes_handler(&self, id: EventHandlerId) -> bool {
        !self.excluded_handlers.contains(&id)
    }

    fn includes_node_ref(&self, id: NodeRefId) -> bool {
        !self.excluded_node_refs.contains(&id)
    }
}

#[derive(Default)]
struct ReactiveReachability {
    queries: Vec<QueryId>,
    mutations: Vec<MutationId>,
    client_helper_calls: Vec<(HelperCallEdgeId, String)>,
    client_module_calls: Vec<(String, String)>,
    node_refs: Vec<NodeRefId>,
    browser_reads: Vec<String>,
    browser_effects: Vec<String>,
    visited_signals: Vec<SignalId>,
}

impl ReactiveReachability {
    fn record_client_helper_call(&mut self, edge: HelperCallEdgeId, authored_path: &str) {
        if !self
            .client_helper_calls
            .iter()
            .any(|(existing_edge, _)| *existing_edge == edge)
        {
            self.client_helper_calls
                .push((edge, authored_path.to_string()));
        }
    }

    fn record_query(&mut self, id: QueryId) -> bool {
        if !self.queries.contains(&id) {
            self.queries.push(id);
            true
        } else {
            false
        }
    }

    fn record_mutation(&mut self, id: MutationId) -> bool {
        if !self.mutations.contains(&id) {
            self.mutations.push(id);
            true
        } else {
            false
        }
    }

    fn record_client_module_call(&mut self, module: &str, name: &str) {
        if !self
            .client_module_calls
            .iter()
            .any(|(existing_module, existing_name)| {
                existing_module == module && existing_name == name
            })
        {
            self.client_module_calls
                .push((module.to_string(), name.to_string()));
        }
    }

    fn record_node_ref(&mut self, id: NodeRefId) {
        if !self.node_refs.contains(&id) {
            self.node_refs.push(id);
        }
    }

    fn record_browser_read(&mut self, kind: String) {
        if !self.browser_reads.contains(&kind) {
            self.browser_reads.push(kind);
        }
    }

    fn record_browser_effect(&mut self, kind: String) {
        if !self.browser_effects.contains(&kind) {
            self.browser_effects.push(kind);
        }
    }

    fn record_signal(&mut self, id: SignalId) -> bool {
        if self.visited_signals.contains(&id) {
            false
        } else {
            self.visited_signals.push(id);
            true
        }
    }

    fn signal_roots(&self) -> Vec<SignalId> {
        let mut signals = self.visited_signals.clone();
        signals.sort_by_key(|id| id.0);
        signals
    }

    fn node_ref_roots(&self) -> Vec<NodeRefId> {
        let mut refs = self.node_refs.clone();
        refs.sort_by_key(|id| id.0);
        refs
    }

    fn query_roots(&self) -> Vec<QueryId> {
        let mut queries = self.queries.clone();
        queries.sort_by_key(|id| id.0);
        queries
    }

    fn mutation_roots(&self) -> Vec<MutationId> {
        let mut mutations = self.mutations.clone();
        mutations.sort_by_key(|id| id.0);
        mutations
    }

    fn browser_read_roots(&self) -> Vec<String> {
        let mut reads = self.browser_reads.clone();
        reads.sort();
        reads
    }

    fn browser_effect_roots(&self) -> Vec<String> {
        let mut effects = self.browser_effects.clone();
        effects.sort();
        effects
    }

    fn client_module_roots(&self) -> Vec<(String, String)> {
        let mut calls = self.client_module_calls.clone();
        calls.sort();
        calls
    }

    fn client_helper_roots(&self) -> Vec<(HelperCallEdgeId, String)> {
        let mut calls = self.client_helper_calls.clone();
        calls.sort_by_key(|(edge, _)| edge.index());
        calls
    }
}

fn collect_reachable_expr_deps(
    island: &Island,
    expr: crate::types::ExprId,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    let deps = collect_reactive_deps(&island.exprs, expr);
    for query in deps.queries {
        collect_reachable_query_deps(island, query, scope, reactive);
    }
    for mutation in deps.mutations {
        collect_reachable_mutation_deps(island, mutation, scope, reactive);
    }
    for signal in deps.signals {
        collect_reachable_signal_deps(island, signal, scope, reactive);
    }
    collect_client_module_calls_from_expr(island.exprs.client_expr(expr), reactive);
    collect_node_refs_from_expr(island.exprs.client_expr(expr), scope, reactive);
    collect_browser_roots_from_expr(island.exprs.client_expr(expr), reactive);
}

fn collect_reachable_block_deps(
    island: &Island,
    block: &ClientBlock,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    let deps = collect_client_block_reactive_deps(block);
    for query in deps.queries {
        collect_reachable_query_deps(island, query, scope, reactive);
    }
    for mutation in deps.mutations {
        collect_reachable_mutation_deps(island, mutation, scope, reactive);
    }
    for signal in deps.signals {
        collect_reachable_signal_deps(island, signal, scope, reactive);
    }
    collect_client_module_calls_from_block(block, reactive);
    collect_node_refs_from_block(block, scope, reactive);
    collect_browser_roots_from_block(block, reactive);
    collect_query_mutation_actions(island, block, scope, reactive);
    collect_signal_write_targets(island, block, scope, reactive);
}

fn collect_reachable_signal_deps(
    island: &Island,
    signal: SignalId,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    if !scope.includes_signal(signal) {
        return;
    }
    if !reactive.record_signal(signal) {
        return;
    }

    if let Some(signal) = signal_decl(island, signal) {
        if let Some(expr) = signal.derived {
            collect_reachable_expr_deps(island, expr, scope, reactive);
        }
    }
}

fn signal_decl(island: &Island, id: SignalId) -> Option<&SignalDecl> {
    island.signals.iter().find(|signal| signal.id == id)
}

/// Look an event handler up by its stored [`EventHandlerId`].
///
/// `island.handlers` is NOT dense: `optimize::handler_dedup` removes
/// merged-away duplicates while preserving the surviving handlers' ids,
/// so after a non-last duplicate is merged the vector holds
/// `[id 0, id 2]` and position 2 is out of bounds. Indexing by `id.0`
/// silently skipped the surviving high-id handler, so reachability never
/// walked its body while the emitter still emitted it: the handler then
/// referenced signal declarations reachability had filtered away, and
/// invoking it threw `ReferenceError` in the browser. Always resolve by
/// stored id, exactly like `signal_decl` / `query_decl` / `mutation_decl`.
fn handler_by_id(island: &Island, id: EventHandlerId) -> Option<&EventHandler> {
    island.handlers.iter().find(|handler| handler.id == id)
}

fn query_decl(island: &Island, id: QueryId) -> Option<&QueryDecl> {
    island.queries.iter().find(|query| query.id == id)
}

fn mutation_decl(island: &Island, id: MutationId) -> Option<&MutationDecl> {
    island.mutations.iter().find(|mutation| mutation.id == id)
}

fn collect_reachable_query_deps(
    island: &Island,
    query: QueryId,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    if !reactive.record_query(query) {
        return;
    }

    if let Some(query) = island.queries.iter().find(|decl| decl.id == query) {
        collect_reachable_expr_deps(island, query.key_expr, scope, reactive);
        collect_reachable_expr_deps(island, query.client_fetcher, scope, reactive);
    }
}

fn collect_reachable_mutation_deps(
    island: &Island,
    mutation: MutationId,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    if !reactive.record_mutation(mutation) {
        return;
    }

    if let Some(mutation) = island.mutations.iter().find(|decl| decl.id == mutation) {
        collect_reachable_expr_deps(island, mutation.key_expr, scope, reactive);
        collect_reachable_expr_deps(island, mutation.client_runner, scope, reactive);
        for invalidation in &mutation.invalidates {
            collect_reachable_expr_deps(island, *invalidation, scope, reactive);
        }
    }
}

fn collect_query_mutation_actions(
    island: &Island,
    block: &ClientBlock,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    for stmt in &block.stmts {
        collect_query_mutation_actions_from_stmt(island, stmt, scope, reactive);
    }
}

fn collect_signal_write_targets(
    island: &Island,
    block: &ClientBlock,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    for stmt in &block.stmts {
        collect_signal_write_targets_from_stmt(island, stmt, scope, reactive);
    }
}

fn collect_client_module_calls_from_block(
    block: &ClientBlock,
    reactive: &mut ReactiveReachability,
) {
    for stmt in &block.stmts {
        collect_client_module_calls_from_stmt(stmt, reactive);
    }
}

fn collect_client_module_calls_from_stmt(stmt: &ClientStmt, reactive: &mut ReactiveReachability) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            collect_client_module_calls_from_expr(expr, reactive);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_client_module_calls_from_expr(value, reactive);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_client_module_calls_from_expr(&closure.body, reactive);
        }
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                collect_client_module_calls_from_expr(arg, reactive);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_client_module_calls_from_expr(payload, reactive);
        }
        ClientStmt::BrowserEffect(effect) => {
            collect_client_module_calls_from_browser_effect(effect, reactive);
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            collect_client_module_calls_from_node_ref_effect(effect, reactive);
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_client_module_calls_from_expr(condition, reactive);
            collect_client_module_calls_from_block(then_block, reactive);
            if let Some(else_block) = else_block {
                collect_client_module_calls_from_block(else_block, reactive);
            }
        }
        ClientStmt::Block(block) => collect_client_module_calls_from_block(block, reactive),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_client_module_calls_from_expr(start, reactive);
            collect_client_module_calls_from_expr(end, reactive);
            collect_client_module_calls_from_block(body, reactive);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
    }
}

fn collect_client_module_calls_from_expr(expr: &ClientExpr, reactive: &mut ReactiveReachability) {
    match expr {
        ClientExpr::Call { function, args } => {
            match function {
                ClientFunctionRef::Import { module, name } => {
                    reactive.record_client_module_call(module, name);
                }
                ClientFunctionRef::Helper {
                    edge,
                    authored_path,
                    ..
                } => {
                    reactive.record_client_helper_call(*edge, authored_path);
                }
                _ => {}
            }
            for arg in args {
                collect_client_module_calls_from_expr(arg, reactive);
            }
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
            collect_client_module_calls_from_expr(value, reactive);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_client_module_calls_from_expr(condition, reactive);
            collect_client_module_calls_from_expr(then_expr, reactive);
            collect_client_module_calls_from_expr(else_expr, reactive);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        }
        | ClientExpr::Index {
            base: haystack,
            index: needle,
        } => {
            collect_client_module_calls_from_expr(haystack, reactive);
            collect_client_module_calls_from_expr(needle, reactive);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_client_module_calls_from_expr(part, reactive);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_client_module_calls_from_expr(&field.value, reactive);
            }
        }
        ClientExpr::BrowserRead(read) => {
            collect_client_module_calls_from_browser_read(read, reactive);
        }
        ClientExpr::DomRead { target, .. } => {
            collect_client_module_calls_from_expr(target, reactive);
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
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_client_module_calls_from_expr(index, reactive);
        }
    }
}

fn collect_client_module_calls_from_browser_read(
    read: &crate::client::ClientBrowserRead,
    reactive: &mut ReactiveReachability,
) {
    match read {
        crate::client::ClientBrowserRead::Window(_)
        | crate::client::ClientBrowserRead::Location(_) => {}
        crate::client::ClientBrowserRead::Storage { key, fallback, .. }
        | crate::client::ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_client_module_calls_from_expr(key, reactive);
            if let Some(fallback) = fallback {
                collect_client_module_calls_from_expr(fallback, reactive);
            }
        }
        crate::client::ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_client_module_calls_from_expr(arg, reactive);
            }
        }
    }
}

fn collect_client_module_calls_from_browser_effect(
    effect: &crate::client::ClientBrowserEffect,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        crate::client::ClientBrowserEffect::WindowScrollTo { x, y }
        | crate::client::ClientBrowserEffect::WindowScrollBy { x, y } => {
            collect_client_module_calls_from_expr(x, reactive);
            collect_client_module_calls_from_expr(y, reactive);
        }
        crate::client::ClientBrowserEffect::WindowPrint
        | crate::client::ClientBrowserEffect::LocationReload
        | crate::client::ClientBrowserEffect::HistoryBack
        | crate::client::ClientBrowserEffect::HistoryForward => {}
        crate::client::ClientBrowserEffect::LocationSetHash { value }
        | crate::client::ClientBrowserEffect::LocationAssign { url: value }
        | crate::client::ClientBrowserEffect::LocationReplace { url: value }
        | crate::client::ClientBrowserEffect::HistoryPushState { url: value }
        | crate::client::ClientBrowserEffect::HistoryReplaceState { url: value }
        | crate::client::ClientBrowserEffect::HistoryGo { delta: value }
        | crate::client::ClientBrowserEffect::DocumentSetTitle { title: value }
        | crate::client::ClientBrowserEffect::ClipboardWriteText { text: value }
        | crate::client::ClientBrowserEffect::StorageRemoveItem { key: value, .. }
        | crate::client::ClientBrowserEffect::UrlDeleteSearchParam { key: value, .. } => {
            collect_client_module_calls_from_expr(value, reactive);
        }
        crate::client::ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            collect_client_module_calls_from_expr(name, reactive);
            if let Some(detail) = detail {
                collect_client_module_calls_from_expr(detail, reactive);
            }
        }
        crate::client::ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | crate::client::ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                collect_client_module_calls_from_expr(fallback, reactive);
            }
        }
        crate::client::ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            fallback,
            ..
        }
        | crate::client::ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            fallback,
            ..
        } => {
            collect_client_module_calls_from_expr(options_json, reactive);
            if let Some(fallback) = fallback {
                collect_client_module_calls_from_expr(fallback, reactive);
            }
        }
        crate::client::ClientBrowserEffect::StorageSetItem { key, value, .. }
        | crate::client::ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            collect_client_module_calls_from_expr(key, reactive);
            collect_client_module_calls_from_expr(value, reactive);
        }
        crate::client::ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                collect_client_module_calls_from_expr(arg, reactive);
            }
        }
    }
}

fn collect_client_module_calls_from_node_ref_effect(
    effect: &crate::client::ClientNodeRefEffect,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        crate::client::ClientNodeRefEffect::Focus
        | crate::client::ClientNodeRefEffect::Blur
        | crate::client::ClientNodeRefEffect::ScrollIntoView
        | crate::client::ClientNodeRefEffect::Click
        | crate::client::ClientNodeRefEffect::RequestSubmit
        | crate::client::ClientNodeRefEffect::Reset
        | crate::client::ClientNodeRefEffect::ReportValidity
        | crate::client::ClientNodeRefEffect::ShowModal
        | crate::client::ClientNodeRefEffect::Close
        | crate::client::ClientNodeRefEffect::ShowPopover
        | crate::client::ClientNodeRefEffect::HidePopover
        | crate::client::ClientNodeRefEffect::TogglePopover
        | crate::client::ClientNodeRefEffect::Select
        | crate::client::ClientNodeRefEffect::RemoveAttribute { .. } => {}
        crate::client::ClientNodeRefEffect::ScrollTo { x, y }
        | crate::client::ClientNodeRefEffect::ScrollBy { x, y } => {
            collect_client_module_calls_from_expr(x, reactive);
            collect_client_module_calls_from_expr(y, reactive);
        }
        crate::client::ClientNodeRefEffect::SetPointerCapture(expr)
        | crate::client::ClientNodeRefEffect::ReleasePointerCapture(expr)
        | crate::client::ClientNodeRefEffect::SetValue(expr)
        | crate::client::ClientNodeRefEffect::SetChecked(expr)
        | crate::client::ClientNodeRefEffect::SetCustomValidity(expr)
        | crate::client::ClientNodeRefEffect::SetTextContent(expr)
        | crate::client::ClientNodeRefEffect::AddClass(expr)
        | crate::client::ClientNodeRefEffect::RemoveClass(expr)
        | crate::client::ClientNodeRefEffect::ToggleClass(expr) => {
            collect_client_module_calls_from_expr(expr, reactive);
        }
        crate::client::ClientNodeRefEffect::SetAttribute { value, .. }
        | crate::client::ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            collect_client_module_calls_from_expr(value, reactive);
        }
        crate::client::ClientNodeRefEffect::SetSelectionRange(start, end) => {
            collect_client_module_calls_from_expr(start, reactive);
            collect_client_module_calls_from_expr(end, reactive);
        }
    }
}

fn collect_node_refs_from_block(
    block: &ClientBlock,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    for stmt in &block.stmts {
        collect_node_refs_from_stmt(stmt, scope, reactive);
    }
}

fn collect_node_refs_from_stmt(
    stmt: &ClientStmt,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            collect_node_refs_from_expr(expr, scope, reactive);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_node_refs_from_expr(value, scope, reactive);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_node_refs_from_expr(&closure.body, scope, reactive);
        }
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                collect_node_refs_from_expr(arg, scope, reactive);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_node_refs_from_expr(payload, scope, reactive);
        }
        ClientStmt::BrowserEffect(effect) => {
            collect_node_refs_from_browser_effect(effect, scope, reactive);
        }
        ClientStmt::NodeRefEffect { id, effect } => {
            if scope.includes_node_ref(*id) {
                reactive.record_node_ref(*id);
            }
            collect_node_refs_from_node_ref_effect(effect, scope, reactive);
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_node_refs_from_expr(condition, scope, reactive);
            collect_node_refs_from_block(then_block, scope, reactive);
            if let Some(else_block) = else_block {
                collect_node_refs_from_block(else_block, scope, reactive);
            }
        }
        ClientStmt::Block(block) => collect_node_refs_from_block(block, scope, reactive),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_node_refs_from_expr(start, scope, reactive);
            collect_node_refs_from_expr(end, scope, reactive);
            collect_node_refs_from_block(body, scope, reactive);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
    }
}

fn collect_node_refs_from_expr(
    expr: &ClientExpr,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match expr {
        ClientExpr::NodeRefRead { id, .. } | ClientExpr::EventTargetWithinNodeRef { id, .. } => {
            if scope.includes_node_ref(*id) {
                reactive.record_node_ref(*id);
            }
        }
        ClientExpr::Call { args, .. } => {
            for arg in args {
                collect_node_refs_from_expr(arg, scope, reactive);
            }
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
            collect_node_refs_from_expr(value, scope, reactive);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_node_refs_from_expr(condition, scope, reactive);
            collect_node_refs_from_expr(then_expr, scope, reactive);
            collect_node_refs_from_expr(else_expr, scope, reactive);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        }
        | ClientExpr::Index {
            base: haystack,
            index: needle,
        } => {
            collect_node_refs_from_expr(haystack, scope, reactive);
            collect_node_refs_from_expr(needle, scope, reactive);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_node_refs_from_expr(part, scope, reactive);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_node_refs_from_expr(&field.value, scope, reactive);
            }
        }
        ClientExpr::BrowserRead(read) => {
            collect_node_refs_from_browser_read(read, scope, reactive);
        }
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_node_refs_from_expr(index, scope, reactive);
        }
        ClientExpr::DomRead { target, .. } => {
            collect_node_refs_from_expr(target, scope, reactive);
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
        | ClientExpr::EventAttribute { .. }
        | ClientExpr::EventPayload { .. }
        | ClientExpr::CallbackPayload
        | ClientExpr::NodePayload { .. } => {}
    }
}

fn collect_node_refs_from_browser_read(
    read: &crate::client::ClientBrowserRead,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match read {
        crate::client::ClientBrowserRead::Window(_)
        | crate::client::ClientBrowserRead::Location(_) => {}
        crate::client::ClientBrowserRead::Storage { key, fallback, .. }
        | crate::client::ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_node_refs_from_expr(key, scope, reactive);
            if let Some(fallback) = fallback {
                collect_node_refs_from_expr(fallback, scope, reactive);
            }
        }
        crate::client::ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_node_refs_from_expr(arg, scope, reactive);
            }
        }
    }
}

fn collect_node_refs_from_browser_effect(
    effect: &crate::client::ClientBrowserEffect,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        crate::client::ClientBrowserEffect::WindowScrollTo { x, y }
        | crate::client::ClientBrowserEffect::WindowScrollBy { x, y } => {
            collect_node_refs_from_expr(x, scope, reactive);
            collect_node_refs_from_expr(y, scope, reactive);
        }
        crate::client::ClientBrowserEffect::WindowPrint
        | crate::client::ClientBrowserEffect::LocationReload
        | crate::client::ClientBrowserEffect::HistoryBack
        | crate::client::ClientBrowserEffect::HistoryForward => {}
        crate::client::ClientBrowserEffect::LocationSetHash { value }
        | crate::client::ClientBrowserEffect::LocationAssign { url: value }
        | crate::client::ClientBrowserEffect::LocationReplace { url: value }
        | crate::client::ClientBrowserEffect::HistoryPushState { url: value }
        | crate::client::ClientBrowserEffect::HistoryReplaceState { url: value }
        | crate::client::ClientBrowserEffect::HistoryGo { delta: value }
        | crate::client::ClientBrowserEffect::DocumentSetTitle { title: value }
        | crate::client::ClientBrowserEffect::ClipboardWriteText { text: value }
        | crate::client::ClientBrowserEffect::StorageRemoveItem { key: value, .. }
        | crate::client::ClientBrowserEffect::UrlDeleteSearchParam { key: value, .. } => {
            collect_node_refs_from_expr(value, scope, reactive);
        }
        crate::client::ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            collect_node_refs_from_expr(name, scope, reactive);
            if let Some(detail) = detail {
                collect_node_refs_from_expr(detail, scope, reactive);
            }
        }
        crate::client::ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | crate::client::ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                collect_node_refs_from_expr(fallback, scope, reactive);
            }
        }
        crate::client::ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            fallback,
            ..
        }
        | crate::client::ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            fallback,
            ..
        } => {
            collect_node_refs_from_expr(options_json, scope, reactive);
            if let Some(fallback) = fallback {
                collect_node_refs_from_expr(fallback, scope, reactive);
            }
        }
        crate::client::ClientBrowserEffect::StorageSetItem { key, value, .. }
        | crate::client::ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            collect_node_refs_from_expr(key, scope, reactive);
            collect_node_refs_from_expr(value, scope, reactive);
        }
        crate::client::ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                collect_node_refs_from_expr(arg, scope, reactive);
            }
        }
    }
}

fn collect_node_refs_from_node_ref_effect(
    effect: &crate::client::ClientNodeRefEffect,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        crate::client::ClientNodeRefEffect::Focus
        | crate::client::ClientNodeRefEffect::Blur
        | crate::client::ClientNodeRefEffect::ScrollIntoView
        | crate::client::ClientNodeRefEffect::Click
        | crate::client::ClientNodeRefEffect::RequestSubmit
        | crate::client::ClientNodeRefEffect::Reset
        | crate::client::ClientNodeRefEffect::ReportValidity
        | crate::client::ClientNodeRefEffect::ShowModal
        | crate::client::ClientNodeRefEffect::Close
        | crate::client::ClientNodeRefEffect::ShowPopover
        | crate::client::ClientNodeRefEffect::HidePopover
        | crate::client::ClientNodeRefEffect::TogglePopover
        | crate::client::ClientNodeRefEffect::Select
        | crate::client::ClientNodeRefEffect::RemoveAttribute { .. } => {}
        crate::client::ClientNodeRefEffect::ScrollTo { x, y }
        | crate::client::ClientNodeRefEffect::ScrollBy { x, y } => {
            collect_node_refs_from_expr(x, scope, reactive);
            collect_node_refs_from_expr(y, scope, reactive);
        }
        crate::client::ClientNodeRefEffect::SetPointerCapture(expr)
        | crate::client::ClientNodeRefEffect::ReleasePointerCapture(expr)
        | crate::client::ClientNodeRefEffect::SetValue(expr)
        | crate::client::ClientNodeRefEffect::SetChecked(expr)
        | crate::client::ClientNodeRefEffect::SetCustomValidity(expr)
        | crate::client::ClientNodeRefEffect::SetTextContent(expr)
        | crate::client::ClientNodeRefEffect::AddClass(expr)
        | crate::client::ClientNodeRefEffect::RemoveClass(expr)
        | crate::client::ClientNodeRefEffect::ToggleClass(expr) => {
            collect_node_refs_from_expr(expr, scope, reactive);
        }
        crate::client::ClientNodeRefEffect::SetAttribute { value, .. }
        | crate::client::ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            collect_node_refs_from_expr(value, scope, reactive);
        }
        crate::client::ClientNodeRefEffect::SetSelectionRange(start, end) => {
            collect_node_refs_from_expr(start, scope, reactive);
            collect_node_refs_from_expr(end, scope, reactive);
        }
    }
}

fn collect_browser_roots_from_block(block: &ClientBlock, reactive: &mut ReactiveReachability) {
    for stmt in &block.stmts {
        collect_browser_roots_from_stmt(stmt, reactive);
    }
}

fn collect_browser_roots_from_stmt(stmt: &ClientStmt, reactive: &mut ReactiveReachability) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            collect_browser_roots_from_expr(expr, reactive);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_browser_roots_from_expr(value, reactive);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_browser_roots_from_expr(&closure.body, reactive);
        }
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                collect_browser_roots_from_expr(arg, reactive);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_browser_roots_from_expr(payload, reactive);
        }
        ClientStmt::BrowserEffect(effect) => {
            reactive.record_browser_effect(browser_effect_kind(effect));
            collect_browser_roots_from_browser_effect(effect, reactive);
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            collect_browser_roots_from_node_ref_effect(effect, reactive);
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_browser_roots_from_expr(condition, reactive);
            collect_browser_roots_from_block(then_block, reactive);
            if let Some(else_block) = else_block {
                collect_browser_roots_from_block(else_block, reactive);
            }
        }
        ClientStmt::Block(block) => collect_browser_roots_from_block(block, reactive),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_browser_roots_from_expr(start, reactive);
            collect_browser_roots_from_expr(end, reactive);
            collect_browser_roots_from_block(body, reactive);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
    }
}

fn collect_browser_roots_from_expr(expr: &ClientExpr, reactive: &mut ReactiveReachability) {
    match expr {
        ClientExpr::BrowserRead(read) => {
            reactive.record_browser_read(browser_read_kind(read));
            collect_browser_roots_from_browser_read(read, reactive);
        }
        ClientExpr::Call { args, .. } => {
            for arg in args {
                collect_browser_roots_from_expr(arg, reactive);
            }
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
            collect_browser_roots_from_expr(value, reactive);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_browser_roots_from_expr(condition, reactive);
            collect_browser_roots_from_expr(then_expr, reactive);
            collect_browser_roots_from_expr(else_expr, reactive);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        }
        | ClientExpr::Index {
            base: haystack,
            index: needle,
        } => {
            collect_browser_roots_from_expr(haystack, reactive);
            collect_browser_roots_from_expr(needle, reactive);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_browser_roots_from_expr(part, reactive);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_browser_roots_from_expr(&field.value, reactive);
            }
        }
        ClientExpr::EventCollectionRead { index, .. } => {
            collect_browser_roots_from_expr(index, reactive);
        }
        ClientExpr::DomRead { target, .. } => {
            collect_browser_roots_from_expr(target, reactive);
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
    }
}

fn collect_browser_roots_from_browser_read(
    read: &ClientBrowserRead,
    reactive: &mut ReactiveReachability,
) {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => {}
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_browser_roots_from_expr(key, reactive);
            if let Some(fallback) = fallback {
                collect_browser_roots_from_expr(fallback, reactive);
            }
        }
        ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_browser_roots_from_expr(arg, reactive);
            }
        }
    }
}

fn collect_browser_roots_from_browser_effect(
    effect: &ClientBrowserEffect,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            collect_browser_roots_from_expr(x, reactive);
            collect_browser_roots_from_expr(y, reactive);
        }
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => {}
        ClientBrowserEffect::LocationSetHash { value }
        | ClientBrowserEffect::LocationAssign { url: value }
        | ClientBrowserEffect::LocationReplace { url: value }
        | ClientBrowserEffect::HistoryPushState { url: value }
        | ClientBrowserEffect::HistoryReplaceState { url: value }
        | ClientBrowserEffect::HistoryGo { delta: value }
        | ClientBrowserEffect::DocumentSetTitle { title: value }
        | ClientBrowserEffect::ClipboardWriteText { text: value }
        | ClientBrowserEffect::StorageRemoveItem { key: value, .. }
        | ClientBrowserEffect::UrlDeleteSearchParam { key: value, .. } => {
            collect_browser_roots_from_expr(value, reactive);
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            collect_browser_roots_from_expr(name, reactive);
            if let Some(detail) = detail {
                collect_browser_roots_from_expr(detail, reactive);
            }
        }
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                collect_browser_roots_from_expr(fallback, reactive);
            }
        }
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
            collect_browser_roots_from_expr(options_json, reactive);
            if let Some(fallback) = fallback {
                collect_browser_roots_from_expr(fallback, reactive);
            }
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. }
        | ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            collect_browser_roots_from_expr(key, reactive);
            collect_browser_roots_from_expr(value, reactive);
        }
        ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                collect_browser_roots_from_expr(arg, reactive);
            }
        }
    }
}

fn collect_browser_roots_from_node_ref_effect(
    effect: &crate::client::ClientNodeRefEffect,
    reactive: &mut ReactiveReachability,
) {
    match effect {
        crate::client::ClientNodeRefEffect::Focus
        | crate::client::ClientNodeRefEffect::Blur
        | crate::client::ClientNodeRefEffect::ScrollIntoView
        | crate::client::ClientNodeRefEffect::Click
        | crate::client::ClientNodeRefEffect::RequestSubmit
        | crate::client::ClientNodeRefEffect::Reset
        | crate::client::ClientNodeRefEffect::ReportValidity
        | crate::client::ClientNodeRefEffect::ShowModal
        | crate::client::ClientNodeRefEffect::Close
        | crate::client::ClientNodeRefEffect::ShowPopover
        | crate::client::ClientNodeRefEffect::HidePopover
        | crate::client::ClientNodeRefEffect::TogglePopover
        | crate::client::ClientNodeRefEffect::Select
        | crate::client::ClientNodeRefEffect::RemoveAttribute { .. } => {}
        crate::client::ClientNodeRefEffect::ScrollTo { x, y }
        | crate::client::ClientNodeRefEffect::ScrollBy { x, y } => {
            collect_browser_roots_from_expr(x, reactive);
            collect_browser_roots_from_expr(y, reactive);
        }
        crate::client::ClientNodeRefEffect::SetPointerCapture(expr)
        | crate::client::ClientNodeRefEffect::ReleasePointerCapture(expr)
        | crate::client::ClientNodeRefEffect::SetValue(expr)
        | crate::client::ClientNodeRefEffect::SetChecked(expr)
        | crate::client::ClientNodeRefEffect::SetCustomValidity(expr)
        | crate::client::ClientNodeRefEffect::SetTextContent(expr)
        | crate::client::ClientNodeRefEffect::AddClass(expr)
        | crate::client::ClientNodeRefEffect::RemoveClass(expr)
        | crate::client::ClientNodeRefEffect::ToggleClass(expr) => {
            collect_browser_roots_from_expr(expr, reactive);
        }
        crate::client::ClientNodeRefEffect::SetAttribute { value, .. }
        | crate::client::ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            collect_browser_roots_from_expr(value, reactive);
        }
        crate::client::ClientNodeRefEffect::SetSelectionRange(start, end) => {
            collect_browser_roots_from_expr(start, reactive);
            collect_browser_roots_from_expr(end, reactive);
        }
    }
}

fn browser_read_kind(read: &ClientBrowserRead) -> String {
    match read {
        ClientBrowserRead::Window(field) => format!("window.{}", window_field_label(*field)),
        ClientBrowserRead::Location(field) => {
            format!("location.{}", location_field_label(*field))
        }
        ClientBrowserRead::Storage {
            area, value_type, ..
        } => {
            format!(
                "storage.{}.{}",
                browser_storage_area_label(*area),
                storage_value_type_label(*value_type)
            )
        }
        ClientBrowserRead::UrlSearchParam { .. } => "url_search_param".to_string(),
        ClientBrowserRead::Declared { helper, .. } => format!("declared.{helper}"),
    }
}

fn browser_effect_kind(effect: &ClientBrowserEffect) -> String {
    match effect {
        ClientBrowserEffect::WindowScrollTo { .. } => "window.scroll_to".to_string(),
        ClientBrowserEffect::WindowScrollBy { .. } => "window.scroll_by".to_string(),
        ClientBrowserEffect::WindowPrint => "window.print".to_string(),
        ClientBrowserEffect::LocationSetHash { .. } => "location.set_hash".to_string(),
        ClientBrowserEffect::LocationAssign { .. } => "location.assign".to_string(),
        ClientBrowserEffect::LocationReplace { .. } => "location.replace".to_string(),
        ClientBrowserEffect::LocationReload => "location.reload".to_string(),
        ClientBrowserEffect::HistoryPushState { .. } => "history.push_state".to_string(),
        ClientBrowserEffect::HistoryReplaceState { .. } => "history.replace_state".to_string(),
        ClientBrowserEffect::HistoryBack => "history.back".to_string(),
        ClientBrowserEffect::HistoryForward => "history.forward".to_string(),
        ClientBrowserEffect::HistoryGo { .. } => "history.go".to_string(),
        ClientBrowserEffect::DocumentDispatchEvent { .. } => "document.dispatch_event".to_string(),
        ClientBrowserEffect::DocumentSetTitle { .. } => "document.set_title".to_string(),
        ClientBrowserEffect::ClipboardWriteText { .. } => "clipboard.write_text".to_string(),
        ClientBrowserEffect::ClipboardReadTextInto { .. } => "clipboard.read_text".to_string(),
        ClientBrowserEffect::WebAuthnCreateJsonInto { .. } => "webauthn.create".to_string(),
        ClientBrowserEffect::WebAuthnGetJsonInto { .. } => "webauthn.get".to_string(),
        ClientBrowserEffect::EventFileTextInto { .. } => "event.file_text".to_string(),
        ClientBrowserEffect::StorageSetItem { area, .. } => {
            format!("storage.{}.set_item", storage_area_label(*area))
        }
        ClientBrowserEffect::StorageRemoveItem { area, .. } => {
            format!("storage.{}.remove_item", storage_area_label(*area))
        }
        ClientBrowserEffect::UrlSetSearchParam { mode, .. } => {
            format!("url_search_param.set.{}", url_history_mode_label(*mode))
        }
        ClientBrowserEffect::UrlDeleteSearchParam { mode, .. } => {
            format!("url_search_param.delete.{}", url_history_mode_label(*mode))
        }
        ClientBrowserEffect::Declared { helper, .. } => format!("declared.{helper}"),
    }
}

fn window_field_label(field: WindowField) -> &'static str {
    match field {
        WindowField::InnerWidth => "inner_width",
        WindowField::InnerHeight => "inner_height",
        WindowField::ScrollX => "scroll_x",
        WindowField::ScrollY => "scroll_y",
    }
}

fn location_field_label(field: LocationField) -> &'static str {
    match field {
        LocationField::Href => "href",
        LocationField::Origin => "origin",
        LocationField::Protocol => "protocol",
        LocationField::Host => "host",
        LocationField::Hostname => "hostname",
        LocationField::Port => "port",
        LocationField::Pathname => "pathname",
        LocationField::Search => "search",
        LocationField::Hash => "hash",
    }
}

fn browser_storage_area_label(area: BrowserStorageArea) -> &'static str {
    match area {
        BrowserStorageArea::Local => "local",
        BrowserStorageArea::Session => "session",
    }
}

fn storage_value_type_label(value_type: StorageValueType) -> &'static str {
    match value_type {
        StorageValueType::String => "string",
        StorageValueType::Bool => "bool",
        StorageValueType::I64 => "i64",
        StorageValueType::F64 => "f64",
    }
}

fn storage_area_label(area: crate::reactive::StorageArea) -> &'static str {
    match area {
        crate::reactive::StorageArea::Local => "local",
        crate::reactive::StorageArea::Session => "session",
    }
}

fn url_history_mode_label(mode: crate::reactive::UrlHistoryMode) -> &'static str {
    match mode {
        crate::reactive::UrlHistoryMode::Replace => "replace",
        crate::reactive::UrlHistoryMode::Push => "push",
    }
}

fn collect_signal_write_targets_from_stmt(
    island: &Island,
    stmt: &ClientStmt,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match stmt {
        ClientStmt::SignalSet { id, .. } | ClientStmt::SignalUpdate { id, .. } => {
            collect_reachable_signal_deps(island, *id, scope, reactive);
        }
        ClientStmt::BrowserEffect(
            ClientBrowserEffect::ClipboardReadTextInto { target, .. }
            | ClientBrowserEffect::WebAuthnCreateJsonInto { target, .. }
            | ClientBrowserEffect::WebAuthnGetJsonInto { target, .. }
            | ClientBrowserEffect::EventFileTextInto { target, .. },
        ) => {
            collect_reachable_signal_deps(island, *target, scope, reactive);
        }
        ClientStmt::If {
            then_block,
            else_block,
            ..
        } => {
            collect_signal_write_targets(island, then_block, scope, reactive);
            if let Some(else_block) = else_block {
                collect_signal_write_targets(island, else_block, scope, reactive);
            }
        }
        ClientStmt::Block(block) => collect_signal_write_targets(island, block, scope, reactive),
        ClientStmt::ForRange { body, .. } => {
            collect_signal_write_targets(island, body, scope, reactive)
        }
        ClientStmt::Expr(_)
        | ClientStmt::Let { .. }
        | ClientStmt::Assign { .. }
        | ClientStmt::AssignPlace { .. }
        | ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::Console { .. }
        | ClientStmt::CallbackInvoke { .. }
        | ClientStmt::RunMutation { .. }
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. }
        | ClientStmt::BrowserEffect(_)
        | ClientStmt::NodeRefEffect { .. }
        | ClientStmt::Return(_) => {}
    }
}

fn collect_query_mutation_actions_from_stmt(
    island: &Island,
    stmt: &ClientStmt,
    scope: &ReachabilityScope,
    reactive: &mut ReactiveReachability,
) {
    match stmt {
        ClientStmt::RunMutation { mutation, .. } => {
            collect_reachable_mutation_deps(island, *mutation, scope, reactive);
        }
        ClientStmt::QueryRefetch { query } | ClientStmt::QueryInvalidate { query } => {
            collect_reachable_query_deps(island, *query, scope, reactive);
        }
        ClientStmt::If {
            then_block,
            else_block,
            ..
        } => {
            collect_query_mutation_actions(island, then_block, scope, reactive);
            if let Some(else_block) = else_block {
                collect_query_mutation_actions(island, else_block, scope, reactive);
            }
        }
        ClientStmt::Block(block) => collect_query_mutation_actions(island, block, scope, reactive),
        ClientStmt::ForRange { body, .. } => {
            collect_query_mutation_actions(island, body, scope, reactive);
        }
        ClientStmt::Expr(_)
        | ClientStmt::Let { .. }
        | ClientStmt::Assign { .. }
        | ClientStmt::AssignPlace { .. }
        | ClientStmt::SignalSet { .. }
        | ClientStmt::SignalUpdate { .. }
        | ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::Console { .. }
        | ClientStmt::CallbackInvoke { .. }
        | ClientStmt::BrowserEffect(_)
        | ClientStmt::NodeRefEffect { .. }
        | ClientStmt::Return(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bindings::{AttrBindingKind, EventName, InputKind};
    use crate::client::{
        ClientBlock, ClientBrowserEffect, ClientBrowserRead, ClientExpr, ClientFunctionRef,
        ClientNodeRefEffect, ClientSimpleClosure, ClientStmt, LocationField, MutationAccessor,
        QueryAccessor,
    };
    use crate::island::{
        ComponentFragmentCallback, ComponentFragmentNodeAlias, ComponentFragmentSignalAlias,
    };
    use crate::lists::{ListDecl, ListItemExpr, ListSource, ListTemplateNode};
    use crate::reactive::{MutationOptions, QueryOptions};
    use crate::{IslandBuilder, Value};

    #[test]
    fn server_only_tree_has_no_client_roots() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        b.static_attr(root, "class", "panel");
        b.server_attr(root, "aria-label", "rsjs::server_attr(self.label)");
        b.text_node_static(root, "Ready");
        b.server_only_node(root, "Icon { name: self.icon }");
        b.add_client_module_import("crate::money".to_string());
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::ServerOnly);
        assert!(analysis.roots.is_empty());
        assert!(analysis.crossings.is_empty());
        assert!(analysis.is_server_only());
    }

    #[test]
    fn unused_client_capabilities_do_not_force_hydration() {
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let node_ref = b.node_ref();
        b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::signal_update(count, ClientSimpleClosure::new("value", ClientExpr::int(1))),
            ClientStmt::NodeRefEffect {
                id: node_ref,
                effect: ClientNodeRefEffect::Focus,
            },
        ]));
        b.add_client_module_import("crate::formatters".to_string());
        let root = b.element("div", None);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::ServerOnly);
        assert!(analysis.roots.is_empty());
        assert!(analysis.crossings.is_empty());
        assert!(analysis.reachable_node_ref_ids().is_empty());
        assert!(analysis.reachable_client_module_imports().is_empty());
    }

    #[test]
    fn unused_query_and_mutation_declarations_do_not_force_hydration() {
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
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::ServerOnly);
        assert!(analysis.roots.is_empty());
        assert!(analysis.crossings.is_empty());
    }

    #[test]
    fn unused_declarations_are_reported_as_dead_primitives() {
        let mut b = IslandBuilder::new();
        let dead_signal = b.signal(Value::Int(0));
        b.set_signal_source(
            dead_signal,
            Some("draft_count".to_string()),
            Some(crate::SourceAnchor {
                start_line: 5,
                start_column: 13,
            }),
        );
        let memo_body = b.expr(ClientExpr::signal_read(dead_signal));
        let dead_memo = b.signal(Value::Null);
        b.set_signal_source(
            dead_memo,
            Some("total".to_string()),
            Some(crate::SourceAnchor {
                start_line: 6,
                start_column: 21,
            }),
        );
        b.set_signal_derived(dead_memo, memo_body);
        let dead_handler = b.handler_from_ir(ClientBlock::from_stmts(Vec::new()));
        b.set_handler_source(
            dead_handler,
            Some("on_save".to_string()),
            Some(crate::SourceAnchor {
                start_line: 7,
                start_column: 21,
            }),
        );
        let dead_node_ref = b.node_ref();
        b.set_node_ref_source(
            dead_node_ref,
            Some("input_ref".to_string()),
            Some(crate::SourceAnchor {
                start_line: 8,
                start_column: 21,
            }),
        );
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        let dead_query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        b.set_query_source(
            dead_query,
            Some("items".to_string()),
            Some(crate::SourceAnchor {
                start_line: 9,
                start_column: 21,
            }),
        );
        let dead_mutation = b.mutation(
            key_expr,
            url_expr,
            url_expr,
            vec![key_expr],
            MutationOptions::default(),
        );
        b.set_mutation_source(
            dead_mutation,
            Some("save_items".to_string()),
            Some(crate::SourceAnchor {
                start_line: 10,
                start_column: 21,
            }),
        );
        let root = b.element("div", None);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::ServerOnly);
        assert_eq!(
            analysis.dead_primitives,
            vec![
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::Signal {
                        signal: dead_signal
                    },
                    label: Some("draft_count".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 5,
                        start_column: 13,
                    }),
                },
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::Memo { signal: dead_memo },
                    label: Some("total".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 6,
                        start_column: 21,
                    }),
                },
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::Handler {
                        handler: dead_handler
                    },
                    label: Some("on_save".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 7,
                        start_column: 21,
                    }),
                },
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::NodeRef {
                        node_ref: dead_node_ref
                    },
                    label: Some("input_ref".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 8,
                        start_column: 21,
                    }),
                },
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::Query { query: dead_query },
                    label: Some("items".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 9,
                        start_column: 21,
                    }),
                },
                DeadClientPrimitive {
                    kind: DeadClientPrimitiveKind::Mutation {
                        mutation: dead_mutation
                    },
                    label: Some("save_items".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 10,
                        start_column: 21,
                    }),
                },
            ]
        );
    }

    #[test]
    fn bindings_force_ssr_hydrate() {
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        b.set_signal_source(
            count,
            Some("count".to_string()),
            Some(crate::SourceAnchor {
                start_line: 4,
                start_column: 21,
            }),
        );
        let count_read = b.expr(ClientExpr::signal_read(count));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        let input = b.element("input", Some(root));
        let button = b.element("button", Some(root));
        b.bind_text(span, count_read);
        b.bind_attr(button, "aria-pressed", AttrBindingKind::Value, count_read);
        b.bind_input(input, InputKind::Value, count);
        let handler = b.handler_from_ir(ClientBlock::from_stmts(Vec::new()));
        b.bind_event(button, EventName::Click, handler);
        b.conditional_section(root, count_read, vec![span], Vec::new());
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert!(analysis.needs_client_js());
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::AttrBinding {
                    node: button,
                    name: "aria-pressed".to_string()
                },
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click
                },
                ClientReachabilityRoot::ConditionalSection { parent: root },
                ClientReachabilityRoot::InputBinding { node: input },
                ClientReachabilityRoot::Signal { id: count },
            ]
        );
        assert_eq!(analysis.reachable_signal_ids(), vec![count]);
        assert_eq!(
            analysis.crossings,
            vec![ClientCrossing {
                kind: ClientCrossingKind::SignalInitialValue { signal: count },
                surface: ClientCrossingSurface::SignalSeed,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some("count".to_string()),
                source_anchor: Some(crate::SourceAnchor {
                    start_line: 4,
                    start_column: 21,
                }),
            }]
        );
    }

    #[test]
    fn event_handler_payload_attrs_are_client_crossings() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        b.payload_attr_with_source(
            button,
            "data-rsjs-payload-p0",
            "day",
            Some("day".to_string()),
            Some(crate::SourceAnchor {
                start_line: 8,
                start_column: 38,
            }),
        );
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            },
        )]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.crossings,
            vec![ClientCrossing {
                kind: ClientCrossingKind::HandlerPayload {
                    node: button,
                    attr: "data-rsjs-payload-p0".to_string(),
                },
                surface: ClientCrossingSurface::HandlerPayloadAttr,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some("day".to_string()),
                source_anchor: Some(crate::SourceAnchor {
                    start_line: 8,
                    start_column: 38,
                }),
            }]
        );
    }

    #[test]
    fn node_payload_attrs_are_client_crossings_from_text_bindings() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        b.payload_attr_with_source(
            root,
            "data-rsjs-payload-p0",
            "item.label",
            Some("item.label".to_string()),
            Some(crate::SourceAnchor {
                start_line: 12,
                start_column: 31,
            }),
        );
        b.payload_attr(root, "data-rsjs-payload-p1", "item.secret");
        let value = b.expr(ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p0".to_string(),
            tokens: "item.label".to_string(),
        });
        b.bind_text(root, value);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.crossings,
            vec![ClientCrossing {
                kind: ClientCrossingKind::NodePayload {
                    node: root,
                    attr: "data-rsjs-payload-p0".to_string(),
                },
                surface: ClientCrossingSurface::NodePayloadAttr,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some("item.label".to_string()),
                source_anchor: Some(crate::SourceAnchor {
                    start_line: 12,
                    start_column: 31,
                }),
            }]
        );
    }

    #[test]
    fn reachable_query_and_mutation_reads_are_reported_from_binding_roots() {
        let mut b = IslandBuilder::new();
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        let query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        let mutation = b.mutation(
            key_expr,
            url_expr,
            url_expr,
            vec![key_expr],
            MutationOptions::default(),
        );
        b.set_mutation_source(
            mutation,
            Some("save_items".to_string()),
            Some(crate::SourceAnchor {
                start_line: 10,
                start_column: 21,
            }),
        );
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let mutation_read = b.expr(ClientExpr::MutationRead {
            id: mutation,
            accessor: MutationAccessor::IsPending,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        let button = b.element("button", Some(root));
        b.bind_text(span, query_read);
        b.bind_attr(
            button,
            "data-pending",
            AttrBindingKind::Value,
            mutation_read,
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::AttrBinding {
                    node: button,
                    name: "data-pending".to_string()
                },
                ClientReachabilityRoot::Query { id: query },
                ClientReachabilityRoot::Mutation { id: mutation },
            ]
        );
    }

    #[test]
    fn reachable_derived_signal_deps_are_reported_from_binding_roots() {
        let mut b = IslandBuilder::new();
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        let query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let memo = b.signal(Value::Null);
        b.set_signal_derived(memo, query_read);
        let memo_read = b.expr(ClientExpr::signal_read(memo));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, memo_read);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::Signal { id: memo },
                ClientReachabilityRoot::Query { id: query },
            ]
        );
        assert_eq!(analysis.reachable_signal_ids(), vec![memo]);
    }

    #[test]
    fn signal_write_targets_are_reported_from_bound_handlers() {
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            count,
            ClientExpr::int(1),
        )]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click
                },
                ClientReachabilityRoot::Signal { id: count },
            ]
        );
        assert_eq!(analysis.reachable_signal_ids(), vec![count]);
    }

    #[test]
    fn reachable_query_metadata_reports_signal_deps() {
        let mut b = IslandBuilder::new();
        let symbol = b.signal(Value::String("AAPL".to_string()));
        let key_expr = b.expr(ClientExpr::QueryKey(vec![
            ClientExpr::string("stock"),
            ClientExpr::signal_read(symbol),
        ]));
        let url_expr = b.expr(ClientExpr::string("/api/stocks"));
        let query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, query_read);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::Signal { id: symbol },
                ClientReachabilityRoot::Query { id: query },
            ]
        );
        assert_eq!(analysis.reachable_signal_ids(), vec![symbol]);
        assert_eq!(analysis.reachable_query_ids(), vec![query]);
    }

    #[test]
    fn reachable_client_module_calls_are_reported_from_binding_roots() {
        let mut b = IslandBuilder::new();
        b.add_client_module_import("crate::money".to_string());
        b.add_client_module_import("crate::dates".to_string());
        let call = b.expr(ClientExpr::call(
            ClientFunctionRef::Import {
                module: "crate::money".to_string(),
                name: "format_usd".to_string(),
            },
            vec![ClientExpr::int(42)],
        ));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, call);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::ClientModuleCall {
                    module: "crate::money".to_string(),
                    name: "format_usd".to_string()
                },
            ]
        );
        assert_eq!(
            analysis.reachable_client_module_imports(),
            vec!["crate::money".to_string()]
        );
        assert!(
            analysis.crossings.is_empty(),
            "helper calls are client roots, but literal/client-side args do not create public-data crossings"
        );
    }

    #[test]
    fn reachable_standalone_helper_calls_keep_exact_source_edges() {
        let mut b = IslandBuilder::new();
        let edge = HelperCallEdgeId::new(7);
        let call = b.expr(ClientExpr::call(
            ClientFunctionRef::Helper {
                edge,
                authored_path: "ImportedAlias".to_string(),
                name: "ImportedAlias".to_string(),
            },
            vec![ClientExpr::int(42)],
        ));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, call);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::ClientHelperCall {
                    edge,
                    authored_path: "ImportedAlias".to_string(),
                },
            ]
        );
        assert_eq!(analysis.reachable_helper_call_edge_ids(), vec![edge]);
        assert!(
            analysis.reachable_client_module_imports().is_empty(),
            "standalone helpers do not require clients(...) bundle eligibility"
        );
    }

    #[test]
    fn client_module_signal_args_use_signal_seed_crossing() {
        let mut b = IslandBuilder::new();
        b.add_client_module_import("crate::money".to_string());
        let cents = b.signal(Value::Int(42));
        b.set_signal_source(
            cents,
            Some("cents".to_string()),
            Some(SourceAnchor {
                start_line: 5,
                start_column: 21,
            }),
        );
        let call = b.expr(ClientExpr::call(
            ClientFunctionRef::Import {
                module: "crate::money".to_string(),
                name: "format_usd".to_string(),
            },
            vec![ClientExpr::signal_read(cents)],
        ));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, call);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::Signal { id: cents },
                ClientReachabilityRoot::ClientModuleCall {
                    module: "crate::money".to_string(),
                    name: "format_usd".to_string(),
                },
            ]
        );
        assert_eq!(
            analysis.crossings,
            vec![ClientCrossing {
                kind: ClientCrossingKind::SignalInitialValue { signal: cents },
                surface: ClientCrossingSurface::SignalSeed,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some("cents".to_string()),
                source_anchor: Some(SourceAnchor {
                    start_line: 5,
                    start_column: 21,
                }),
            }]
        );
    }

    #[test]
    fn client_module_calls_in_handlers_are_reachability_roots() {
        let mut b = IslandBuilder::new();
        b.add_client_module_import("crate::money".to_string());
        let cents = b.signal(Value::Int(42));
        b.set_signal_source(
            cents,
            Some("cents".to_string()),
            Some(SourceAnchor {
                start_line: 5,
                start_column: 21,
            }),
        );
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::call(
                ClientFunctionRef::Import {
                    module: "crate::money".to_string(),
                    name: "format_usd".to_string(),
                },
                vec![ClientExpr::signal_read(cents)],
            ),
        )]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click,
                },
                ClientReachabilityRoot::Signal { id: cents },
                ClientReachabilityRoot::ClientModuleCall {
                    module: "crate::money".to_string(),
                    name: "format_usd".to_string(),
                },
            ]
        );
        assert_eq!(
            analysis.crossings,
            vec![ClientCrossing {
                kind: ClientCrossingKind::SignalInitialValue { signal: cents },
                surface: ClientCrossingSurface::SignalSeed,
                requirement: ClientCrossingRequirement::ClientEncode,
                label: Some("cents".to_string()),
                source_anchor: Some(SourceAnchor {
                    start_line: 5,
                    start_column: 21,
                }),
            }]
        );
    }

    #[test]
    fn reachable_browser_reads_are_reported_from_bindings() {
        let mut b = IslandBuilder::new();
        let pathname = b.expr(ClientExpr::BrowserRead(ClientBrowserRead::Location(
            LocationField::Pathname,
        )));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, pathname);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::TextBinding { node: span },
                ClientReachabilityRoot::BrowserRead {
                    kind: "location.pathname".to_string()
                },
            ]
        );
    }

    #[test]
    fn reachable_browser_effects_are_reported_from_bound_handlers() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
            ClientBrowserEffect::ClipboardWriteText {
                text: ClientExpr::string("copied"),
            },
        )]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click
                },
                ClientReachabilityRoot::BrowserEffect {
                    kind: "clipboard.write_text".to_string()
                },
            ]
        );
    }

    #[test]
    fn query_and_mutation_actions_are_reported_from_bound_handlers() {
        let mut b = IslandBuilder::new();
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        let query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        b.set_query_source(
            query,
            Some("items".to_string()),
            Some(crate::SourceAnchor {
                start_line: 9,
                start_column: 21,
            }),
        );
        let mutation = b.mutation(
            key_expr,
            url_expr,
            url_expr,
            vec![key_expr],
            MutationOptions::default(),
        );
        b.set_mutation_source(
            mutation,
            Some("save_items".to_string()),
            Some(crate::SourceAnchor {
                start_line: 10,
                start_column: 21,
            }),
        );
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::QueryRefetch { query },
            ClientStmt::QueryInvalidate { query },
            ClientStmt::RunMutation {
                mutation,
                args: Vec::new(),
            },
        ]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click
                },
                ClientReachabilityRoot::Query { id: query },
                ClientReachabilityRoot::Mutation { id: mutation },
            ]
        );
        assert_eq!(
            analysis.crossings,
            vec![
                ClientCrossing {
                    kind: ClientCrossingKind::QueryKey { query },
                    surface: ClientCrossingSurface::QueryMetadataBlob,
                    requirement: ClientCrossingRequirement::QueryCodec,
                    label: Some("items".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 9,
                        start_column: 21,
                    }),
                },
                ClientCrossing {
                    kind: ClientCrossingKind::QueryFetcher { query },
                    surface: ClientCrossingSurface::QueryMetadataBlob,
                    requirement: ClientCrossingRequirement::RequestConfig,
                    label: Some("items.fetcher".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 9,
                        start_column: 21,
                    }),
                },
                ClientCrossing {
                    kind: ClientCrossingKind::QueryInitialValue { query },
                    surface: ClientCrossingSurface::QuerySeedBlob,
                    requirement: ClientCrossingRequirement::QueryCodec,
                    label: Some("items".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 9,
                        start_column: 21,
                    }),
                },
                ClientCrossing {
                    kind: ClientCrossingKind::MutationKey { mutation },
                    surface: ClientCrossingSurface::MutationMetadataBlob,
                    requirement: ClientCrossingRequirement::MutationCodec,
                    label: Some("save_items".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 10,
                        start_column: 21,
                    }),
                },
                ClientCrossing {
                    kind: ClientCrossingKind::MutationRunner { mutation },
                    surface: ClientCrossingSurface::MutationMetadataBlob,
                    requirement: ClientCrossingRequirement::RequestConfig,
                    label: Some("save_items.runner".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 10,
                        start_column: 21,
                    }),
                },
                ClientCrossing {
                    kind: ClientCrossingKind::MutationInvalidationKey { mutation, index: 0 },
                    surface: ClientCrossingSurface::MutationMetadataBlob,
                    requirement: ClientCrossingRequirement::MutationCodec,
                    label: Some("save_items.invalidates[0]".to_string()),
                    source_anchor: Some(crate::SourceAnchor {
                        start_line: 10,
                        start_column: 21,
                    }),
                },
            ]
        );
    }

    #[test]
    fn dead_query_and_mutation_declarations_do_not_create_crossings() {
        let mut b = IslandBuilder::new();
        let key_expr = b.expr(ClientExpr::string("items"));
        let url_expr = b.expr(ClientExpr::string("/api/items"));
        let query = b.query(key_expr, key_expr, url_expr, QueryOptions::default());
        b.set_query_source(query, Some("items".to_string()), None);
        let mutation = b.mutation(
            key_expr,
            url_expr,
            url_expr,
            vec![key_expr],
            MutationOptions::default(),
        );
        b.set_mutation_source(mutation, Some("save_items".to_string()), None);
        let root = b.element("div", None);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::ServerOnly);
        assert!(analysis.roots.is_empty());
        assert!(analysis.crossings.is_empty());
    }

    #[test]
    fn node_ref_effects_are_reported_from_bound_handlers() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let node_ref = b.node_ref();
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
            id: node_ref,
            effect: ClientNodeRefEffect::Focus,
        }]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::EventBinding {
                    node: button,
                    event: EventName::Click
                },
                ClientReachabilityRoot::NodeRef { id: node_ref },
            ]
        );
        assert_eq!(analysis.reachable_node_ref_ids(), vec![node_ref]);
    }

    #[test]
    fn node_ref_binding_forces_ssr_hydrate() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let node_ref = b.node_ref();
        b.bind_node_ref(root, node_ref);
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![ClientReachabilityRoot::NodeRefBinding {
                node: root,
                node_ref
            }]
        );
        assert_eq!(analysis.reachable_node_ref_ids(), vec![node_ref]);
    }

    #[test]
    fn interval_list_and_fragment_mount_force_ssr_hydrate() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let loop_node = b.for_loop(root, "", "", "item", "items.get()", "");
        let item_root = b.element("li", Some(loop_node));
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
        b.interval(1000, ClientBlock::from_stmts(Vec::new()));
        b.keyed_list(ListDecl {
            container: root,
            loop_node,
            item_root,
            source: ListSource::expr(key_expr),
            key: ListItemExpr::Item,
            key_tokens: "item".to_string(),
            item: ListTemplateNode::Element {
                tag: "li".to_string(),
                static_attrs: Vec::new(),
                dynamic_attrs: Vec::new(),
                children: vec![ListTemplateNode::Slot(ListItemExpr::Item)],
            },
        });
        b.component_fragment_mount(
            item_root,
            "child_fragment".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::Interval { index: 0 },
                ClientReachabilityRoot::KeyedList { container: root },
                ClientReachabilityRoot::ComponentFragmentMount {
                    root: item_root,
                    asset_id: "child_fragment".to_string()
                },
            ]
        );
        assert_eq!(
            analysis.reachable_component_fragment_imports(),
            vec!["child_fragment".to_string()]
        );
    }

    #[test]
    fn reachable_component_fragment_imports_are_sorted_and_deduped() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let first = b.element("div", Some(root));
        let second = b.element("div", Some(root));
        let third = b.element("div", Some(root));
        b.component_fragment_mount(
            first,
            "child_z".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.component_fragment_mount(
            second,
            "child_a".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.component_fragment_mount(
            third,
            "child_z".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());

        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.reachable_component_fragment_imports(),
            vec!["child_a".to_string(), "child_z".to_string()]
        );
        assert_eq!(
            analysis.reachable_component_fragment_mounts(),
            vec![
                (first, "child_z".to_string()),
                (second, "child_a".to_string()),
                (third, "child_z".to_string()),
            ]
        );
    }

    #[test]
    fn fragment_owned_bindings_are_not_parent_roots_or_crossings() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let child_root = b.element("div", Some(root));
        let child_span = b.element("span", Some(child_root));
        let child_button = b.element("button", Some(child_root));
        let child_input = b.element("input", Some(child_root));
        let child_signal = b.signal(Value::String("Alice".to_string()));
        b.set_signal_source(child_signal, Some("child_name".to_string()), None);
        let child_read = b.expr(ClientExpr::signal_read(child_signal));
        b.bind_text(child_span, child_read);
        b.bind_attr(
            child_span,
            "data-child-name",
            AttrBindingKind::Value,
            child_read,
        );
        b.payload_attr(child_button, "data-rsjs-payload-p0", "child_payload");
        let child_handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            },
        )]));
        b.bind_event(child_button, EventName::Click, child_handler);
        let child_ref = b.node_ref();
        b.bind_node_ref(child_input, child_ref);
        b.component_fragment_mount(
            child_root,
            "child_fragment".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            vec![child_root, child_span, child_button, child_input],
            vec![child_signal],
            vec![child_handler],
            vec![child_ref],
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![ClientReachabilityRoot::ComponentFragmentMount {
                root: child_root,
                asset_id: "child_fragment".to_string(),
            }]
        );
        assert!(analysis.reachable_signal_ids().is_empty());
        assert!(analysis.reachable_node_ref_ids().is_empty());
        assert!(analysis.crossings.is_empty());
        assert!(analysis.dead_primitives.is_empty());
    }

    #[test]
    fn schema_v4_component_fragment_mount_requires_parent_runtime_without_host_coupling() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let child_root = b.element("div", Some(root));
        b.component_fragment_mount(
            child_root,
            "self_contained_child".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);
        let island = b.build();

        assert!(island.component_fragment_mounts[0].requires_parent_runtime());
        let analysis = classify_render_strategy(&island);
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![ClientReachabilityRoot::ComponentFragmentMount {
                root: child_root,
                asset_id: "self_contained_child".to_string(),
            }]
        );
        assert!(analysis.crossings.is_empty());
    }

    #[test]
    fn component_fragment_node_aliases_do_not_create_client_crossings() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let child_root = b.element("div", Some(root));
        let child_span = b.element("span", Some(child_root));
        b.component_fragment_mount(
            child_root,
            "child_fragment".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            Vec::new(),
            vec![ComponentFragmentNodeAlias {
                child: crate::NodeId(41),
                parent: child_span,
            }],
            vec![child_root, child_span],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![ClientReachabilityRoot::ComponentFragmentMount {
                root: child_root,
                asset_id: "child_fragment".to_string(),
            }]
        );
        assert!(
            analysis.crossings.is_empty(),
            "node aliases are marker bookkeeping for fragment mounts, not public client data"
        );
    }

    #[test]
    fn component_fragment_signal_aliases_are_descriptor_reactive_prop_crossings() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let child_root = b.element("div", Some(root));
        let parent_signal = b.signal(Value::String("Alice".to_string()));
        b.component_fragment_mount(
            child_root,
            "child_fragment".to_string(),
            Vec::<ComponentFragmentCallback>::new(),
            vec![ComponentFragmentSignalAlias {
                child: crate::SignalId(7),
                parent: parent_signal,
                prop_name: Some("name".to_string()),
                source_anchor: Some(SourceAnchor {
                    start_line: 13,
                    start_column: 17,
                }),
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);

        let analysis = classify_render_strategy(&b.build());
        assert_eq!(analysis.strategy, RenderStrategy::SsrHydrate);
        assert_eq!(
            analysis.roots,
            vec![
                ClientReachabilityRoot::Signal { id: parent_signal },
                ClientReachabilityRoot::ComponentFragmentMount {
                    root: child_root,
                    asset_id: "child_fragment".to_string(),
                },
            ]
        );
        assert_eq!(
            analysis.crossings,
            vec![
                ClientCrossing {
                    kind: ClientCrossingKind::SignalInitialValue {
                        signal: parent_signal
                    },
                    surface: ClientCrossingSurface::SignalSeed,
                    requirement: ClientCrossingRequirement::ClientEncode,
                    label: None,
                    source_anchor: None,
                },
                ClientCrossing {
                    kind: ClientCrossingKind::DescriptorReactiveProp {
                        root: child_root,
                        asset_id: "child_fragment".to_string(),
                        prop: "name".to_string(),
                        child_signal: crate::SignalId(7),
                        parent_signal,
                    },
                    surface: ClientCrossingSurface::DescriptorReactiveProp,
                    requirement: ClientCrossingRequirement::ClientEncode,
                    label: Some("child_fragment::name".to_string()),
                    source_anchor: Some(SourceAnchor {
                        start_line: 13,
                        start_column: 17,
                    }),
                },
            ]
        );
    }
}
