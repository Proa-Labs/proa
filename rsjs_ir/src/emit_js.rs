// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stage-1 JS emitter for an rsjs island.
//!
//! Lowers an [`Island`] to a per-island JavaScript source string that
//! the runtime in `rsjs/runtime/rsjs-runtime.js` knows how to
//! hydrate. The emitter is intentionally "boring readable JS by
//! default": stable indentation (4 spaces), one declaration per
//! line, semantic identifier names. No minification, no whitespace
//! stripping, no obfuscation. Minification is a separate post-emit
//! responsibility of the framework asset pipeline.
//!
//! Determinism: the bindings tables are [`std::collections::HashMap`]s
//! in v0.1, so the emitter sorts every iteration order before
//! generating output. NodeIds are used as both the sort key and the
//! `data-rsjs-bind` / `data-rsjs-handler` marker attribute value, so
//! the JS is stable across runs and the M3 server emitter has a
//! single, consistent convention to match.
//!
//! Output shape (Counter, abbreviated):
//!
//! ```js
//! import { signal, bind_text, bind_conditional, bind_delegated_events, hydrate } from "/rsjs-runtime.js";
//!
//! hydrate("Counter", (root, resources) => {
//!     const walk_path = (root, path, marker_attr, marker_value) => { ... };
//!     const __sig_0 = signal(0);
//!
//!     function __handler_0(event) {
//!         __sig_0.update((__c0) => (__sig_0.get() - 1));
//!     }
//!
//!     const __node_2 = walk_path(root, [0, 1], "data-rsjs-bind", "2");
//!     resources.push(bind_text(__node_2, () => __sig_0.get(), [__sig_0]));
//!
//!     resources.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
//! });
//! ```
//!
//! Per OPTIMIZATIONS item #2, ordinary event handlers are wired via a
//! single delegated listener on the island root rather than one
//! `addEventListener` per handler. Submit events stay on the per-form
//! `bind_submit(node, handler)` shape because they need
//! `preventDefault` semantics on the form element directly; repeated
//! submit forms resolve every matching handler marker and bind each
//! form instance.
//!
//! The hydration callback receives the rendered island root element
//! (the `<rsjs-island>` wrapper) and resolves each binding's DOM node
//! via a static `.children[i]` index walk that the IR computes at
//! compile time from the view tree. The leading `0` in every path
//! descends from the wrapper into the user-authored island root,
//! which is its first (and only) element child. In dev mode the
//! walker asserts the resolved element carries the expected
//! `data-rsjs-*` marker and falls back to `querySelector` on
//! mismatch; in production it skips the assertion entirely. The
//! marker attributes are still emitted by the Rust SSR side so dev
//! builds have a stable assertion target.

use crate::abi::HelperCallEdgeId;
use crate::analysis::dep_collector::{collect_deps, collect_reactive_deps, ReactiveDeps};
use crate::analysis::typing::TypeEnv;
use crate::analysis::{
    validate_client_root_helper_block_types, validate_client_root_helper_expr_types,
    ClientHelperTypeSite, ClientReachabilityRoot,
};
use crate::bindings::{AttrBinding, ConditionalSection, EventName, InputBinding, InputKind};
use crate::client::{
    emit_client_block_js_with_context_and_indent, emit_client_expr_js_with_context,
    write_client_type_json, ClientBlock, ClientBrowserEffect, ClientBrowserRead, ClientEmitContext,
    ClientEmitError, ClientExpr, ClientNodeRefEffect, ClientStmt, ClientType, ClientUnaryOp,
    LinkedHelperCallTarget, LinkedHelperEmitError, LinkedHelperProgram, QueryAccessor,
};
use crate::island::{HydrationStrategy, Island, ViewNode, ViewTreeRef};
use crate::lists::{ListItemExpr, ListSource, ListTemplateNode};
use crate::markers::{
    MARKER_ATTR, MARKER_BIND, MARKER_COMPONENT_MOUNT, MARKER_HANDLER, MARKER_INPUT_CHECKED,
    MARKER_INPUT_SELECTED, MARKER_INPUT_VALUE, MARKER_LIST, MARKER_REF,
};
use crate::types::{
    CallbackSlotId, EventHandlerId, ExprId, MutationId, NodeId, NodeRefId, QueryId, SignalId,
};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;

/// Indentation unit used by the emitter (4 spaces).
const INDENT: &str = "    ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootMode {
    IslandWrapper,
    FragmentRoot,
}

struct EmitContext {
    root_mode: RootMode,
    excluded_nodes: HashSet<NodeId>,
    excluded_signals: HashSet<SignalId>,
    excluded_handlers: HashSet<EventHandlerId>,
    excluded_node_refs: HashSet<NodeRefId>,
    linked_helper_calls: Option<BTreeMap<HelperCallEdgeId, LinkedHelperCallTarget>>,
}

impl EmitContext {
    fn island(island: &Island) -> Self {
        Self::for_root_mode(island, RootMode::IslandWrapper)
    }

    fn fragment(island: &Island) -> Self {
        Self::for_root_mode(island, RootMode::FragmentRoot)
    }

    fn for_root_mode(island: &Island, root_mode: RootMode) -> Self {
        let mut ctx = Self::empty(root_mode);
        for mount in &island.component_fragment_mounts {
            ctx.excluded_nodes.extend(mount.owned_nodes.iter().copied());
            ctx.excluded_signals
                .extend(mount.owned_signals.iter().copied());
            ctx.excluded_handlers
                .extend(mount.owned_handlers.iter().copied());
            ctx.excluded_node_refs
                .extend(mount.owned_node_refs.iter().copied());
        }
        ctx
    }

    fn empty(root_mode: RootMode) -> Self {
        Self {
            root_mode,
            excluded_nodes: HashSet::new(),
            excluded_signals: HashSet::new(),
            excluded_handlers: HashSet::new(),
            excluded_node_refs: HashSet::new(),
            linked_helper_calls: None,
        }
    }

    fn with_linked_helpers(mut self, program: &LinkedHelperProgram) -> Self {
        self.linked_helper_calls = Some(program.root_call_targets());
        self
    }

    fn client_emit_context(&self) -> ClientEmitContext {
        match &self.linked_helper_calls {
            Some(calls) => ClientEmitContext::default().with_linked_helper_calls(calls.clone()),
            None => ClientEmitContext::default(),
        }
    }

    fn includes_node(&self, node: NodeId) -> bool {
        !self.excluded_nodes.contains(&node)
    }

    fn includes_signal(&self, signal: SignalId) -> bool {
        !self.excluded_signals.contains(&signal)
    }

    fn includes_handler(&self, handler: EventHandlerId) -> bool {
        !self.excluded_handlers.contains(&handler)
    }

    fn includes_node_ref(&self, node_ref: NodeRefId) -> bool {
        !self.excluded_node_refs.contains(&node_ref)
    }
}

/// Whether `node` produces a DOM *element* child slot in its parent's
/// rendered HTML.
///
/// The hydration index-walk uses zero-based `Element.children[i]`
/// indexing, which only enumerates element children (text nodes are
/// not part of `.children`). The mapping mirrors what `emit_rust`
/// actually writes:
///
/// - [`ViewNode::Element`] → an HTML element, counts as 1 slot.
/// - [`ViewNode::TextBindingSlot`] → wrapped in a `<span>` carrier
///   (see `emit_rust.rs`'s `TextBindingSlot` arm), counts as 1 slot.
/// - [`ViewNode::StaticText`] → a literal text node, **not** an
///   element child; does **not** count toward the index.
/// - [`ViewNode::ServerOnly`] → an opaque server-rendered subtree with `0..N`
///   element cardinality. The fast path speculatively advances one slot; the
///   runtime verifies the landed marker and uses an owner-scoped lookup when
///   the actual server output shifts the path.
/// - [`ViewNode::ForLoop`] → a runtime-variable number of child slots.
///   Islands containing loops currently disable static index walking
///   and use marker lookup instead.
fn node_consumes_index_slot(node: &ViewNode) -> bool {
    match node {
        ViewNode::Element { .. } => true,
        ViewNode::TextBindingSlot => true,
        ViewNode::StaticText(_) => false,
        ViewNode::ServerOnly(_) => true,
        ViewNode::ForLoop { .. } => true,
    }
}

/// Compute the index path from the user-authored root element down to
/// `target`, expressed as a sequence of `.children[i]` indices.
///
/// Returns `None` when `target` is not reachable from the view tree's
/// root (a logic bug; the caller falls back to `None` so the dev-mode
/// assertion catches it). The returned path does **not** include the
/// leading `0` for the `<rsjs-island>` wrapper — emitters prepend it.
fn compute_index_path(view_tree: &ViewTreeRef, target: NodeId) -> Option<Vec<u32>> {
    if view_tree_contains_for_loop(view_tree) {
        return None;
    }
    let root = view_tree.root()?;
    let mut path = Vec::new();
    if walk_for_target(view_tree, root, target, &mut path) {
        Some(path)
    } else {
        None
    }
}

/// Recursive DFS that fills `path` with the child-index sequence from
/// `current` to `target`. Returns `true` iff `target` was found in
/// the subtree rooted at `current`.
fn walk_for_target(
    view_tree: &ViewTreeRef,
    current: NodeId,
    target: NodeId,
    path: &mut Vec<u32>,
) -> bool {
    if current == target {
        return true;
    }
    if let Some(children) = view_node_children(view_tree.get(current)) {
        let mut slot: u32 = 0;
        for child in children {
            let child_node = view_tree.get(*child);
            if !node_consumes_index_slot(child_node) {
                continue;
            }
            if *child == target {
                path.push(slot);
                return true;
            }
            // Recurse into the child subtree. The slot index is the
            // child's position in the parent's element-child list, so
            // push it before recursing and pop on miss.
            path.push(slot);
            if walk_for_target(view_tree, *child, target, path) {
                return true;
            }
            path.pop();
            slot += 1;
        }
    }
    false
}

fn view_tree_contains_for_loop(view_tree: &ViewTreeRef) -> bool {
    (0..view_tree.len())
        .any(|idx| matches!(view_tree.get(NodeId(idx as u32)), ViewNode::ForLoop { .. }))
}

fn view_node_children(node: &ViewNode) -> Option<&[NodeId]> {
    match node {
        ViewNode::Element { children, .. } | ViewNode::ForLoop { children, .. } => Some(children),
        _ => None,
    }
}

fn node_is_in_for_loop(view_tree: &ViewTreeRef, target: NodeId) -> bool {
    let Some(root) = view_tree.root() else {
        return false;
    };
    walk_for_loop_membership(view_tree, root, target, false)
}

fn walk_for_loop_membership(
    view_tree: &ViewTreeRef,
    current: NodeId,
    target: NodeId,
    in_loop: bool,
) -> bool {
    let node = view_tree.get(current);
    let now_in_loop = in_loop || matches!(node, ViewNode::ForLoop { .. });
    if current == target {
        return now_in_loop;
    }
    let Some(children) = view_node_children(node) else {
        return false;
    };
    children
        .iter()
        .any(|child| walk_for_loop_membership(view_tree, *child, target, now_in_loop))
}

/// Format an index path as a JS array literal.
///
/// Island modules receive the `<rsjs-island>` wrapper as `root`, so
/// their walks need an initial `0` segment to reach the user-authored
/// root. Component fragments receive the component root element
/// directly, so their walks use the view-tree path as-is.
fn format_index_path(path: &[u32], root_mode: RootMode) -> String {
    let mut out = match root_mode {
        RootMode::IslandWrapper => String::from("[0"),
        RootMode::FragmentRoot => String::from("["),
    };
    for seg in path {
        if out.len() > 1 {
            out.push_str(", ");
        }
        let _ = write!(out, "{seg}");
    }
    out.push(']');
    out
}

/// Lower an [`Island`] to JavaScript.
///
/// `name` is the string passed to the runtime's `hydrate(...)` call;
/// in v0.1 this is the user-visible island name (e.g. `"Counter"`)
/// or, in M3+, the manifest's `asset_id`. Choosing the convention is
/// the caller's job; the emitter just embeds the string verbatim
/// (after JS-escaping).
///
/// If strategy analysis classifies the island as server-only, this
/// returns an empty string. Server-only IR has no hydration root and
/// should not accidentally produce an inert runtime asset when the
/// emitter is called directly.
///
/// This is descriptor/debug emission when the reachable IR contains
/// standalone helper edges. In that case it intentionally writes reserved
/// `__rsjs_unlinked_helper_edge_*` sentinels and the result is not deployable.
/// Final compilation must call [`try_emit_linked`] (or its linked fragment
/// counterpart), which resolves typed edges while re-emitting the IR.
///
/// **The emitter does not run optimisation passes.** It consumes
/// the island IR as-is. Callers that want post-pass JS output should
/// run [`crate::optimize::default_passes`] (or a custom
/// [`crate::optimize::PassRunner`]) on the IR before calling this
/// function. Keeping `emit` pass-free keeps the function pure with
/// respect to its input, lets golden tests exercise the pre- and
/// post-pass IRs independently, and makes it possible to inspect the
/// raw IR's emitted output during debugging.
pub fn emit(island: &Island, name: &str) -> String {
    try_emit_impl(island, name, None)
        .expect("Client IR v3 handler emission should cover production handler surface")
}

/// Lower an island to deployable JavaScript with a relocation-resolved,
/// transitively closed standalone-helper program.
///
/// Unlike [`emit`], this final-link API never permits symbolic helper calls in
/// its output. Caller-local helper bindings must exactly match the Client IR
/// roots that this asset emits. Resolution is structural; it never rewrites
/// descriptor placeholder text.
pub fn try_emit_linked(
    island: &Island,
    name: &str,
    helpers: &LinkedHelperProgram,
) -> Result<String, LinkedHelperEmitError> {
    try_emit_impl(island, name, Some(helpers))
}

fn try_emit_impl(
    island: &Island,
    name: &str,
    linked_helpers: Option<&LinkedHelperProgram>,
) -> Result<String, LinkedHelperEmitError> {
    let strategy_analysis = crate::classify_render_strategy(island);
    let reachable_signal_ids = strategy_analysis.reachable_signal_ids();
    let reachable_node_ref_ids = strategy_analysis.reachable_node_ref_ids();
    let reachable_query_ids = strategy_analysis.reachable_query_ids();
    let reachable_mutation_ids = strategy_analysis.reachable_mutation_ids();
    let reachable_client_module_imports = strategy_analysis.reachable_client_module_imports();
    let reachable_component_fragment_mounts =
        strategy_analysis.reachable_component_fragment_mounts();
    let mut ctx = EmitContext::island(island);
    if let Some(helpers) = linked_helpers {
        ctx = ctx.with_linked_helpers(helpers);
        validate_linked_root_program(
            helpers,
            island,
            &ctx,
            &reachable_signal_ids,
            &reachable_query_ids,
            &reachable_mutation_ids,
            &reachable_component_fragment_mounts,
            &[],
            &[],
        )?;
    }
    if strategy_analysis.is_server_only() {
        return Ok(String::new());
    }
    let has_live_query_keys = island_has_live_query_keys(island, &reachable_query_ids);
    let mut numeric_runtime_helpers = island_numeric_runtime_helpers(
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_query_ids,
        &reachable_mutation_ids,
        &reachable_component_fragment_mounts,
    );
    if let Some(helpers) = linked_helpers {
        numeric_runtime_helpers.extend(linked_helper_numeric_runtime_helpers(helpers));
    }
    let mut out = String::new();
    out.push_str(&runtime_import_line(
        island,
        RuntimeImportMode::Island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        &numeric_runtime_helpers,
    ));
    if !reachable_query_ids.is_empty()
        || !reachable_mutation_ids.is_empty()
        || island_uses_runtime_helpers(
            island,
            &ctx,
            &reachable_signal_ids,
            &reachable_query_ids,
            &reachable_mutation_ids,
            &reachable_component_fragment_mounts,
        )
        || linked_helpers.is_some_and(linked_helper_uses_runtime_helpers)
    {
        out.push_str(&query_runtime_import_line(
            &reachable_query_ids,
            &reachable_mutation_ids,
            has_live_query_keys,
        ));
    }
    // Per-bundle imports for `ClientExpr::Call` sites. Each bundle
    // module the island references (via a reactive expression that
    // calls `<module>::<helper>(...)`) gets one
    // `import { ... } from "/_rsjs/client_modules/<safe_module>.js"`
    // line so the bundle's exports are in scope when the hydration
    // callback runs. The bundle JS itself is written by
    // `rsjs_macros::client_module_attr::write_bundle_js`.
    out.push_str(&client_module_import_lines(
        &reachable_client_module_imports,
    ));
    out.push_str(&component_fragment_import_lines(
        &reachable_component_fragment_mounts,
    ));
    out.push('\n');
    if let Some(helpers) = linked_helpers {
        helpers.emit_private_functions(&mut out)?;
    }
    emit_hydration_open(&mut out, island, name);

    emit_disposer_setup(
        &mut out,
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        DisposerOwner::HydrationRuntime,
    );
    emit_signals(&mut out, island, &ctx, &reachable_signal_ids);
    emit_node_refs(&mut out, island, &ctx, &reachable_node_ref_ids, &[]);
    emit_queries(&mut out, island, name, &reachable_query_ids, &ctx);
    emit_mutations(&mut out, island, name, &reachable_mutation_ids, &ctx);
    emit_handlers_v3(&mut out, island, &ctx)?;
    emit_component_fragment_mounts(&mut out, island, &ctx, &reachable_component_fragment_mounts);
    // Round-3 review #6: every binding that needs a node reference
    // calls `emit_node_lookup`, which emits a `const __node_<id> =
    // root.querySelector(...)` line. A node with multiple bindings
    // (e.g. `<button on_click={...}>{count.get()}</button>` carries
    // both an event AND a text binding on the same node, or a node
    // hosts multiple events) used to redeclare the same `const`,
    // and JS rejects redeclared `const`. Track the set of NodeIds
    // already declared and gate subsequent uses behind it.
    let mut declared_nodes: HashSet<NodeId> = HashSet::new();
    emit_node_ref_bindings(
        &mut out,
        island,
        &mut declared_nodes,
        &ctx,
        &reachable_node_ref_ids,
    );
    emit_text_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_input_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_attr_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_event_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_conditional_sections(&mut out, island, &mut declared_nodes, &ctx);
    emit_lists(&mut out, island, &mut declared_nodes, &ctx);
    emit_intervals(&mut out, island, &ctx)?;
    emit_disposer_return(
        &mut out,
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        false,
    );

    emit_hydration_close(&mut out, &island.hydration_strategy);
    Ok(out)
}

fn emit_hydration_open(out: &mut String, island: &Island, name: &str) {
    if supports_managed_module_import(&island.hydration_strategy) {
        let _ = write!(
            out,
            "hydrate_from_module(import.meta.url, {runtime}",
            runtime = hydration_runtime_import(&island.hydration_strategy)
        );
        out.push(')');
    } else {
        out.push_str(hydration_runtime_import(&island.hydration_strategy));
    }
    let _ = writeln!(
        out,
        "(\"{name}\", (root, __rsjs_disposers = []) => {{",
        name = JsStr(name)
    );
}

fn emit_hydration_close(out: &mut String, strategy: &HydrationStrategy) {
    out.push('}');
    match strategy {
        HydrationStrategy::Immediate | HydrationStrategy::Visible | HydrationStrategy::Idle => {
            out.push_str(");\n");
        }
        HydrationStrategy::Interaction { event: None } => {
            out.push_str(");\n");
        }
        HydrationStrategy::Interaction { event: Some(event) } => {
            let _ = writeln!(out, ", [\"{event}\"]);", event = JsStr(event));
        }
        HydrationStrategy::Media { query } => {
            let _ = writeln!(out, ", \"{query}\");", query = JsStr(query));
        }
    }
}

fn hydration_runtime_import(strategy: &HydrationStrategy) -> &'static str {
    match strategy {
        HydrationStrategy::Immediate => "hydrate",
        HydrationStrategy::Visible => "hydrate_when_visible",
        HydrationStrategy::Idle => "hydrate_when_idle",
        HydrationStrategy::Interaction { .. } => "hydrate_on_interaction",
        HydrationStrategy::Media { .. } => "hydrate_when_media",
    }
}

fn supports_managed_module_import(strategy: &HydrationStrategy) -> bool {
    matches!(
        strategy,
        HydrationStrategy::Visible
            | HydrationStrategy::Idle
            | HydrationStrategy::Interaction { .. }
            | HydrationStrategy::Media { .. }
    )
}

/// Lower an [`Island`] to a reusable component-fragment JavaScript module.
///
/// Fragment modules are not self-hydrating island entries. They export a
/// stable mount function that future parent-island emitters can import and
/// call against already-rendered child DOM. The first implementation slice
/// emits the module and manifest artifact while the macro continues to use the
/// existing inliner for runtime semantics.
pub fn emit_fragment(island: &Island, asset_id: &str) -> String {
    emit_fragment_impl(island, asset_id, &[], &[], &[], &[], &[], false, None)
        .expect("descriptor component-fragment Client IR should emit")
}

/// Emit a deployable component fragment with a closed standalone-helper
/// program and no cross-component exports.
pub fn try_emit_fragment_linked(
    island: &Island,
    asset_id: &str,
    helpers: &LinkedHelperProgram,
) -> Result<String, LinkedHelperEmitError> {
    emit_fragment_impl(
        island,
        asset_id,
        &[],
        &[],
        &[],
        &[],
        &[],
        false,
        Some(helpers),
    )
}

/// One compiler-authored payload handler exported from a component fragment.
///
/// The semantic linker refers to `producer_id`; the body remains producer-local
/// Client IR and therefore names this fragment's signal IDs directly.
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentPayloadHandlerExport {
    pub producer_id: u32,
    pub payload_arity: u8,
    pub body: ClientBlock,
}

/// One compiler-authenticated conditional control exported from a parent
/// component fragment to its linked owner.
///
/// The owner subscribes to this exact producer expression when deciding
/// whether a retained conditional child subtree should have browser resources
/// mounted. It never reconstructs the predicate from source spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentLifecycleControlExport {
    pub control_id: u32,
    pub producer_expression_id: ExprId,
}

/// Failure while the final emitter resolves or lowers a fragment's exact
/// standalone-helper program.
#[derive(Debug)]
pub enum LinkedFragmentEmitError {
    /// The semantic linker could not resolve the emitter-computed root edge
    /// set to a closed helper program.
    HelperResolution(String),
    /// The resolved program disagreed with emitted Client IR or could not be
    /// lowered to JavaScript.
    HelperEmission(LinkedHelperEmitError),
}

impl std::fmt::Display for LinkedFragmentEmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HelperResolution(message) => formatter.write_str(message),
            Self::HelperEmission(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LinkedFragmentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HelperResolution(_) => None,
            Self::HelperEmission(error) => Some(error),
        }
    }
}

impl From<LinkedHelperEmitError> for LinkedFragmentEmitError {
    fn from(error: LinkedHelperEmitError) -> Self {
        Self::HelperEmission(error)
    }
}

/// Lower a reusable fragment with the signal, payload-handler, and lifecycle
/// control exports required by linked owner composition.
///
/// `forced_signal_ids` are additional browser roots established by cross-
/// component edges. They are deliberately explicit: ordinary standalone
/// fragment emission continues to use local reachability only.
pub fn emit_fragment_with_linked_exports(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
) -> String {
    emit_fragment_impl(
        island,
        asset_id,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        &[],
        &[],
        true,
        None,
    )
    .expect("descriptor component-fragment Client IR should emit")
}

/// Lower a reusable fragment with exact cross-component node-ref aliases.
///
/// `exported_node_ref_ids` keep parent acquisitions reachable and publish
/// their one ref object to the linked owner. `imported_node_ref_ids` identify
/// child-local refs that must be supplied through `options.nodeRefs`; unlike a
/// local acquisition, an imported ref never allocates a fallback object.
pub fn emit_fragment_with_linked_node_refs(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
    exported_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
) -> String {
    emit_fragment_impl(
        island,
        asset_id,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
        true,
        None,
    )
    .expect("descriptor component-fragment Client IR should emit")
}

/// Final-link variant of [`emit_fragment_with_linked_node_refs`].
///
/// The helper program is caller-specific: its root edge table must exactly
/// cover helper calls reached by this fragment, including forced payload
/// handlers and lifecycle controls.
#[allow(clippy::too_many_arguments)]
pub fn try_emit_fragment_with_linked_helpers_and_node_refs(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
    exported_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
    helpers: &LinkedHelperProgram,
) -> Result<String, LinkedHelperEmitError> {
    emit_fragment_impl(
        island,
        asset_id,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
        true,
        Some(helpers),
    )
}

/// Final-link a fragment by resolving the exact helper roots computed by the
/// emitter itself.
///
/// The resolver callback is invoked once, after all linked signal, payload,
/// lifecycle, and node-ref roots have been recursively closed. The resulting
/// [`LinkedHelperProgram`] and that same opaque reachability value are then
/// consumed by emission. This makes it impossible for macro-side helper-root
/// selection to drift from the declarations and imports emitted into the
/// fragment.
#[allow(clippy::too_many_arguments)]
pub fn try_emit_fragment_resolving_helpers_and_node_refs(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
    exported_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
    resolve_helpers: impl FnOnce(&[crate::abi::HelperCallEdgeId]) -> Result<LinkedHelperProgram, String>,
) -> Result<String, LinkedFragmentEmitError> {
    let reachability = FragmentReachability::build(
        island,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
    );
    let helpers = resolve_helpers(&reachability.helper_call_edge_ids)
        .map_err(LinkedFragmentEmitError::HelperResolution)?;
    emit_fragment_impl_with_reachability(
        island,
        asset_id,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
        true,
        Some(&helpers),
        reachability,
    )
    .map_err(LinkedFragmentEmitError::HelperEmission)
}

/// Complete browser-resource closure for one component fragment.
///
/// Keep this private and pass it by value into emission: callers may provide
/// authenticated roots, but only the emitter owns the final projection of
/// those roots into declarations, imports, mounts, and helper relocations.
struct FragmentReachability {
    has_browser_roots: bool,
    uses_query_runtime_helpers: bool,
    numeric_runtime_helpers: BTreeSet<&'static str>,
    signal_ids: Vec<SignalId>,
    node_ref_ids: Vec<NodeRefId>,
    query_ids: Vec<QueryId>,
    mutation_ids: Vec<MutationId>,
    client_module_imports: Vec<String>,
    component_fragment_mounts: Vec<(NodeId, String)>,
    helper_call_edge_ids: Vec<crate::abi::HelperCallEdgeId>,
}

impl FragmentReachability {
    fn build(
        island: &Island,
        forced_signal_ids: &[SignalId],
        payload_handlers: &[FragmentPayloadHandlerExport],
        lifecycle_controls: &[FragmentLifecycleControlExport],
        exported_node_ref_ids: &[NodeRefId],
        imported_node_ref_ids: &[NodeRefId],
    ) -> Self {
        let mut forced_node_ref_ids = exported_node_ref_ids.to_vec();
        forced_node_ref_ids.extend_from_slice(imported_node_ref_ids);
        let forced_expression_ids = lifecycle_controls
            .iter()
            .map(|control| control.producer_expression_id)
            .collect::<Vec<_>>();
        let roots = crate::client_reachability_roots_with_linked_roots(
            island,
            forced_signal_ids,
            &forced_node_ref_ids,
            &forced_expression_ids,
            payload_handlers.iter().map(|handler| &handler.body),
        );
        let has_browser_roots = !roots.is_empty();

        let mut signal_ids = Vec::new();
        let mut node_ref_ids = Vec::new();
        let mut query_ids = Vec::new();
        let mut mutation_ids = Vec::new();
        let mut client_module_imports = Vec::new();
        let mut component_fragment_mounts = Vec::new();
        let mut helper_call_edge_ids = Vec::new();
        for root in roots {
            match root {
                ClientReachabilityRoot::Signal { id } => signal_ids.push(id),
                ClientReachabilityRoot::NodeRef { id }
                | ClientReachabilityRoot::NodeRefBinding { node_ref: id, .. } => {
                    node_ref_ids.push(id);
                }
                ClientReachabilityRoot::Query { id } => query_ids.push(id),
                ClientReachabilityRoot::Mutation { id } => mutation_ids.push(id),
                ClientReachabilityRoot::ClientModuleCall { module, .. } => {
                    client_module_imports.push(module);
                }
                ClientReachabilityRoot::ComponentFragmentMount { root, asset_id } => {
                    component_fragment_mounts.push((root, asset_id));
                }
                ClientReachabilityRoot::ClientHelperCall { edge, .. } => {
                    helper_call_edge_ids.push(edge);
                }
                _ => {}
            }
        }

        signal_ids.sort_by_key(|id| id.0);
        signal_ids.dedup();
        node_ref_ids.sort_by_key(|id| id.0);
        node_ref_ids.dedup();
        query_ids.sort_by_key(|id| id.0);
        query_ids.dedup();
        mutation_ids.sort_by_key(|id| id.0);
        mutation_ids.dedup();
        client_module_imports.sort();
        client_module_imports.dedup();
        component_fragment_mounts.sort_by(|(left_root, left_asset), (right_root, right_asset)| {
            left_root
                .0
                .cmp(&right_root.0)
                .then_with(|| left_asset.cmp(right_asset))
        });
        component_fragment_mounts.dedup();
        helper_call_edge_ids.sort_by_key(|edge| edge.index());
        helper_call_edge_ids.dedup();

        let ctx = EmitContext::fragment(island);
        let mut uses_query_runtime_helpers = island_uses_runtime_helpers(
            island,
            &ctx,
            &signal_ids,
            &query_ids,
            &mutation_ids,
            &component_fragment_mounts,
        );
        let mut numeric_runtime_helpers = island_numeric_runtime_helpers(
            island,
            &ctx,
            &signal_ids,
            &query_ids,
            &mutation_ids,
            &component_fragment_mounts,
        );
        for handler in payload_handlers {
            uses_query_runtime_helpers |= client_block_uses_runtime_helpers(&handler.body);
            extend_numeric_runtime_helpers_from_block(&mut numeric_runtime_helpers, &handler.body);
        }
        for control in lifecycle_controls {
            let expression = island.exprs.client_expr(control.producer_expression_id);
            uses_query_runtime_helpers |= client_expr_uses_runtime_helpers(expression);
            extend_numeric_runtime_helpers_from_expr(&mut numeric_runtime_helpers, expression);
        }

        Self {
            has_browser_roots,
            uses_query_runtime_helpers,
            numeric_runtime_helpers,
            signal_ids,
            node_ref_ids,
            query_ids,
            mutation_ids,
            client_module_imports,
            component_fragment_mounts,
            helper_call_edge_ids,
        }
    }
}

fn emit_fragment_impl(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
    exported_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
    force_module: bool,
    linked_helpers: Option<&LinkedHelperProgram>,
) -> Result<String, LinkedHelperEmitError> {
    let reachability = FragmentReachability::build(
        island,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
    );
    emit_fragment_impl_with_reachability(
        island,
        asset_id,
        forced_signal_ids,
        payload_handlers,
        lifecycle_controls,
        exported_node_ref_ids,
        imported_node_ref_ids,
        force_module,
        linked_helpers,
        reachability,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_fragment_impl_with_reachability(
    island: &Island,
    asset_id: &str,
    forced_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
    exported_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
    force_module: bool,
    linked_helpers: Option<&LinkedHelperProgram>,
    reachability: FragmentReachability,
) -> Result<String, LinkedHelperEmitError> {
    let FragmentReachability {
        has_browser_roots,
        uses_query_runtime_helpers,
        mut numeric_runtime_helpers,
        signal_ids: reachable_signal_ids,
        node_ref_ids: reachable_node_ref_ids,
        query_ids: reachable_query_ids,
        mutation_ids: reachable_mutation_ids,
        client_module_imports: reachable_client_module_imports,
        component_fragment_mounts: reachable_component_fragment_mounts,
        helper_call_edge_ids: _,
    } = reachability;
    if !force_module
        && !has_browser_roots
        && forced_signal_ids.is_empty()
        && payload_handlers.is_empty()
        && lifecycle_controls.is_empty()
        && exported_node_ref_ids.is_empty()
        && imported_node_ref_ids.is_empty()
    {
        if let Some(helpers) = linked_helpers {
            helpers.validate_root_calls(&BTreeMap::new())?;
        }
        return Ok(String::new());
    }
    if force_module
        && !has_browser_roots
        && forced_signal_ids.is_empty()
        && payload_handlers.is_empty()
        && lifecycle_controls.is_empty()
        && exported_node_ref_ids.is_empty()
        && imported_node_ref_ids.is_empty()
    {
        if let Some(helpers) = linked_helpers {
            helpers.validate_root_calls(&BTreeMap::new())?;
        }
        return Ok(format!(
            "export function {}(_root, _options = {{}}) {{}}\n",
            component_fragment_export_name(asset_id)
        ));
    }
    let mut ctx = EmitContext::fragment(island);
    if let Some(helpers) = linked_helpers {
        ctx = ctx.with_linked_helpers(helpers);
        validate_linked_root_program(
            helpers,
            island,
            &ctx,
            &reachable_signal_ids,
            &reachable_query_ids,
            &reachable_mutation_ids,
            &reachable_component_fragment_mounts,
            payload_handlers,
            lifecycle_controls,
        )?;
    }
    let has_live_query_keys = island_has_live_query_keys(island, &reachable_query_ids);
    if let Some(helpers) = linked_helpers {
        numeric_runtime_helpers.extend(linked_helper_numeric_runtime_helpers(helpers));
    }
    let mut out = String::new();
    out.push_str(&runtime_import_line(
        island,
        RuntimeImportMode::Fragment,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        &numeric_runtime_helpers,
    ));
    if !reachable_query_ids.is_empty()
        || !reachable_mutation_ids.is_empty()
        || uses_query_runtime_helpers
        || linked_helpers.is_some_and(linked_helper_uses_runtime_helpers)
    {
        out.push_str(&query_runtime_import_line(
            &reachable_query_ids,
            &reachable_mutation_ids,
            has_live_query_keys,
        ));
    }
    out.push_str(&client_module_import_lines(
        &reachable_client_module_imports,
    ));
    out.push_str(&component_fragment_import_lines(
        &reachable_component_fragment_mounts,
    ));
    out.push('\n');
    if let Some(helpers) = linked_helpers {
        helpers.emit_private_functions(&mut out)?;
    }
    let _ = writeln!(
        out,
        "export function {export_name}(root, options = {{}}) {{",
        export_name = component_fragment_export_name(asset_id)
    );

    let has_disposers = island_has_disposers(
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
    );
    emit_disposer_setup(
        &mut out,
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        DisposerOwner::Fragment,
    );
    if has_disposers {
        out.push_str("    try {\n");
    }
    emit_signals(&mut out, island, &ctx, &reachable_signal_ids);
    emit_node_refs(
        &mut out,
        island,
        &ctx,
        &reachable_node_ref_ids,
        imported_node_ref_ids,
    );
    emit_linked_fragment_exports(
        &mut out,
        island,
        &ctx,
        forced_signal_ids,
        payload_handlers,
        exported_node_ref_ids,
    );
    emit_queries(&mut out, island, asset_id, &reachable_query_ids, &ctx);
    emit_mutations(&mut out, island, asset_id, &reachable_mutation_ids, &ctx);
    emit_linked_fragment_lifecycle_exports(&mut out, island, lifecycle_controls, &ctx);
    emit_handlers_v3(&mut out, island, &ctx)?;
    emit_component_fragment_mounts(&mut out, island, &ctx, &reachable_component_fragment_mounts);
    let mut declared_nodes: HashSet<NodeId> = HashSet::new();
    emit_node_ref_bindings(
        &mut out,
        island,
        &mut declared_nodes,
        &ctx,
        &reachable_node_ref_ids,
    );
    emit_text_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_input_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_attr_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_event_bindings(&mut out, island, &mut declared_nodes, &ctx);
    emit_conditional_sections(&mut out, island, &mut declared_nodes, &ctx);
    emit_lists(&mut out, island, &mut declared_nodes, &ctx);
    emit_intervals(&mut out, island, &ctx)?;
    emit_disposer_return(
        &mut out,
        island,
        &ctx,
        &reachable_signal_ids,
        &reachable_node_ref_ids,
        &reachable_query_ids,
        true,
    );
    if has_disposers {
        // A fragment initializer does not pass through run_hydration_init(),
        // so it owns its synchronous rollback boundary. Consume resources
        // registered before the failure (including node-ref clearing) and
        // rethrow so the parent mount can report the scoped error.
        out.push_str("    } catch (__rsjs_init_error) {\n");
        out.push_str("        dispose_all(__rsjs_disposers, \"rsjs fragment init rollback\");\n");
        out.push_str("        throw __rsjs_init_error;\n");
        out.push_str("    }\n");
    }

    out.push_str("}\n");
    Ok(out)
}

fn emit_linked_fragment_exports(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    exported_signal_ids: &[SignalId],
    payload_handlers: &[FragmentPayloadHandlerExport],
    exported_node_ref_ids: &[NodeRefId],
) {
    let exported_signals = island
        .signals
        .iter()
        .filter(|signal| ctx.includes_signal(signal.id) && exported_signal_ids.contains(&signal.id))
        .collect::<Vec<_>>();
    if !exported_signals.is_empty() {
        out.push_str("    if (options.expose?.signals != null) {\n");
        for signal in exported_signals {
            out.push_str("        options.expose.signals[");
            let _ = write!(out, "{}", signal.id.0);
            let _ = writeln!(out, "] = {v};", v = signal_var(signal.id));
        }
        out.push_str("    }\n");
    }

    let mut exported_node_refs = exported_node_ref_ids.to_vec();
    exported_node_refs.sort_by_key(|node_ref| node_ref.0);
    exported_node_refs.dedup();
    if !exported_node_refs.is_empty() {
        out.push_str("    if (options.expose?.nodeRefs != null) {\n");
        for node_ref in exported_node_refs {
            if !ctx.includes_node_ref(node_ref)
                || !island.node_refs.iter().any(|decl| decl.id == node_ref)
            {
                continue;
            }
            out.push_str("        options.expose.nodeRefs[");
            let _ = write!(out, "{}", node_ref.0);
            let _ = writeln!(out, "] = {v};", v = node_ref_var(node_ref));
        }
        out.push_str("    }\n");
    }

    if payload_handlers.is_empty() {
        return;
    }
    let mut payload_handlers = payload_handlers.iter().collect::<Vec<_>>();
    payload_handlers.sort_by_key(|handler| handler.producer_id);
    out.push_str("    if (options.expose?.payloadHandlers != null) {\n");
    for handler in payload_handlers {
        debug_assert!(handler.payload_arity <= 1);
        out.push_str("        options.expose.payloadHandlers[");
        let _ = write!(out, "{}", handler.producer_id);
        out.push_str("] = (__payload) => {\n");
        emit_client_block_js_with_context_and_indent(
            out,
            &handler.body,
            &ctx.client_emit_context(),
            3,
        )
        .expect("linked fragment payload handler Client IR must emit");
        out.push_str("        };\n");
    }
    out.push_str("    }\n");
}

/// Export the exact producer predicates that govern linked conditional child
/// lifetimes.
///
/// This runs after query and mutation registration because the exported
/// dependency arrays may contain those handles. The getter closes over the
/// same producer-local expression used by `bind_conditional`, so the linked
/// owner does not reconstruct predicate semantics from canonical IDs.
fn emit_linked_fragment_lifecycle_exports(
    out: &mut String,
    island: &Island,
    controls: &[FragmentLifecycleControlExport],
    ctx: &EmitContext,
) {
    if controls.is_empty() {
        return;
    }
    let mut controls = controls.to_vec();
    controls.sort_by_key(|control| (control.control_id, control.producer_expression_id.0));
    for pair in controls.windows(2) {
        assert!(
            pair[0].control_id != pair[1].control_id
                || pair[0].producer_expression_id == pair[1].producer_expression_id,
            "one linked fragment control id cannot name multiple predicate expressions"
        );
    }
    controls.dedup();

    out.push_str("    if (options.expose?.lifecycleControls != null) {\n");
    for control in controls {
        let dependencies = collect_reactive_deps(&island.exprs, control.producer_expression_id);
        out.push_str("        options.expose.lifecycleControls[");
        let _ = write!(out, "{}", control.control_id);
        out.push_str("] = {\n            get: () => !!(");
        emit_arena_expr(out, island, control.producer_expression_id, ctx);
        out.push_str("),\n            deps: ");
        emit_dep_array(out, &dependencies.signals);
        out.push_str(",\n            handles: ");
        emit_handle_array(out, &dependencies);
        out.push_str(",\n        };\n");
    }
    out.push_str("    }\n");
}

/// Build the import line for the M8 query / M9 mutation runtime.
/// The runtime ships as a single ES module
/// (`/rsjs-runtime-query.js`) and re-exports the `rsjs` namespace
/// (carrying `__project`, `__as_str`, `__is_present`, ...) plus the
/// `register_query` and `register_mutation` factories used by
/// `emit_queries` and `emit_mutations`. We import only the symbols
/// the island actually needs so the byte budget for query-only or
/// mutation-only islands stays tight.
fn query_runtime_import_line(
    reachable_query_ids: &[QueryId],
    reachable_mutation_ids: &[MutationId],
    has_live_query_keys: bool,
) -> String {
    let mut out = String::from("import { rsjs");
    if !reachable_query_ids.is_empty() {
        out.push_str(", dispose_query, register_query");
    }
    if has_live_query_keys {
        out.push_str(", rebind_query");
    }
    if !reachable_mutation_ids.is_empty() {
        out.push_str(", register_mutation");
    }
    out.push_str(" } from \"/rsjs-runtime-query.js\";\n");
    out
}

/// Build import lines for every reachable linked client-helper bundle.
fn client_module_import_lines(modules: &[String]) -> String {
    let mut out = String::new();
    for module in modules {
        let _ = writeln!(
            out,
            "import * as {ns} from \"./{asset}.js\";",
            ns = client_module_namespace(module),
            asset = JsStr(client_module_asset_name(module))
        );
    }
    out
}

/// Build import lines for reusable component fragment chunks mounted by
/// this island.
fn component_fragment_import_lines(reachable_mounts: &[(NodeId, String)]) -> String {
    if reachable_mounts.is_empty() {
        return String::new();
    }
    let mut by_asset: BTreeMap<&str, String> = BTreeMap::new();
    for (_, asset_id) in reachable_mounts {
        by_asset
            .entry(asset_id.as_str())
            .or_insert_with(|| component_fragment_export_name(asset_id));
    }

    let mut out = String::new();
    for (asset_id, export_name) in by_asset {
        let _ = writeln!(
            out,
            "import {{ {export_name} as {alias} }} from \"./component_{asset}.js\";",
            alias = component_fragment_import_alias(asset_id),
            asset = JsStr(asset_id)
        );
    }
    out
}

/// Walk every [`ClientExpr`] reachable from the island and return
/// whether any of them lower to a `rsjs.__*` runtime helper call.
///
/// The emitter lowers [`ClientExpr::Project`], [`ClientExpr::AsType`],
/// [`ClientExpr::IsPresent`], [`ClientExpr::Eq`], [`ClientExpr::Neq`],
/// and [`ClientExpr::Coalesce`] to `rsjs.__project` / `rsjs.__as_*` /
/// `rsjs.__is_present` / `rsjs.__eq` / `rsjs.__neq` /
/// `rsjs.__coalesce` calls regardless of whether the island has
/// queries or mutations. Without this gate, an island that wires a
/// projection chain through a hand-built signal would emit
/// unimported runtime calls and `ReferenceError` at hydration.
///
/// The walk is reachability-scoped: bindings, reachable signal derived
/// bodies, reachable query/mutation metadata, bound handlers, intervals,
/// lists, and component callback adapter blocks. Dead arena expressions
/// must not pull the query helper runtime into an otherwise smaller asset.
fn handler_is_bound_to_included_node(
    island: &Island,
    ctx: &EmitContext,
    handler_id: EventHandlerId,
) -> bool {
    island
        .bindings
        .event_bindings
        .iter()
        .any(|((node, _), bound)| {
            ctx.includes_node(*node) && ctx.includes_handler(*bound) && *bound == handler_id
        })
}

/// One expression source the emitted island JS actually evaluates.
///
/// Either a node in the island's expression arena or a lowered client block
/// (a handler body, an interval body, a component-callback adapter).
enum ReachableClientRoot<'a> {
    Arena(ExprId),
    Block(&'a ClientBlock),
}

/// Enumerate every [`ReachableClientRoot`] the emitted asset evaluates.
///
/// The walk is reachability-scoped: bindings, reachable signal derived bodies,
/// reachable query/mutation metadata, bound handlers, intervals, lists, and
/// component callback adapter blocks. Dead arena expressions must not pull
/// runtime helpers into an otherwise smaller asset.
///
/// Both import gates ([`island_uses_runtime_helpers`] for the `rsjs.__*`
/// helpers and [`island_numeric_runtime_helpers`] for checked/domain-aware
/// numeric helpers
/// dispatchers) are built on this one function. A helper that is emitted but
/// not imported is a `ReferenceError` at hydration with no compile-time signal,
/// so the two gates must never drift apart by walking different root sets.
fn for_each_reachable_client_root<'a>(
    island: &'a Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_query_ids: &[QueryId],
    reachable_mutation_ids: &[MutationId],
    reachable_component_fragment_mounts: &[(NodeId, String)],
    visit: &mut impl FnMut(ReachableClientRoot<'a>),
) {
    for (node, expr) in &island.bindings.text_bindings {
        if ctx.includes_node(*node) {
            visit(ReachableClientRoot::Arena(*expr));
        }
    }
    for binding in island.bindings.attr_bindings.values() {
        if ctx.includes_node(binding.node) {
            visit(ReachableClientRoot::Arena(binding.expr));
        }
    }
    for (parent, sections) in &island.bindings.conditional_sections {
        if ctx.includes_node(*parent) {
            for section in sections {
                visit(ReachableClientRoot::Arena(section.condition));
            }
        }
    }
    for signal in &island.signals {
        if reachable_signal_ids.contains(&signal.id) && ctx.includes_signal(signal.id) {
            if let Some(expr) = signal.derived {
                visit(ReachableClientRoot::Arena(expr));
            }
        }
    }
    for query in &island.queries {
        if reachable_query_ids.contains(&query.id) {
            visit(ReachableClientRoot::Arena(query.key_expr));
            visit(ReachableClientRoot::Arena(query.client_fetcher));
        }
    }
    for mutation in &island.mutations {
        if reachable_mutation_ids.contains(&mutation.id) {
            visit(ReachableClientRoot::Arena(mutation.key_expr));
            visit(ReachableClientRoot::Arena(mutation.client_runner));
            for expr in &mutation.invalidates {
                visit(ReachableClientRoot::Arena(*expr));
            }
        }
    }
    for list in &island.lists {
        if ctx.includes_node(list.container) {
            if let ListSource::Expr(expr) = list.source {
                visit(ReachableClientRoot::Arena(expr));
            }
        }
    }
    for handler in &island.handlers {
        if handler_is_bound_to_included_node(island, ctx, handler.id) {
            visit(ReachableClientRoot::Block(handler.body_ir()));
        }
    }
    for interval in &island.intervals {
        visit(ReachableClientRoot::Block(interval.body_ir()));
    }
    let reachable_mounts = reachable_component_fragment_mounts
        .iter()
        .map(|(root, asset)| (root.0, asset.as_str()))
        .collect::<HashSet<_>>();
    for mount in &island.component_fragment_mounts {
        if mount.requires_parent_runtime()
            && reachable_mounts.contains(&(mount.root.0, mount.asset_id.as_str()))
        {
            for callback in &mount.callbacks {
                visit(ReachableClientRoot::Block(callback.body_ir()));
            }
        }
    }
}

fn validate_linked_root_program(
    program: &LinkedHelperProgram,
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_query_ids: &[QueryId],
    reachable_mutation_ids: &[MutationId],
    reachable_component_fragment_mounts: &[(NodeId, String)],
    payload_handlers: &[FragmentPayloadHandlerExport],
    lifecycle_controls: &[FragmentLifecycleControlExport],
) -> Result<(), LinkedHelperEmitError> {
    let mut actual = BTreeMap::new();
    let mut conflicting_arity = None;
    let mut collect = |expression: &ClientExpr| {
        if let ClientExpr::Call {
            function: crate::client::ClientFunctionRef::Helper { edge, .. },
            args,
        } = expression
        {
            if let Some(first) = actual.insert(*edge, args.len()) {
                if first != args.len() {
                    conflicting_arity.get_or_insert((*edge, first, args.len()));
                }
            }
        }
    };
    for_each_reachable_client_root(
        island,
        ctx,
        reachable_signal_ids,
        reachable_query_ids,
        reachable_mutation_ids,
        reachable_component_fragment_mounts,
        &mut |root| match root {
            ReachableClientRoot::Arena(expression) => {
                crate::walk_client_expr(island.exprs.client_expr(expression), &mut collect);
            }
            ReachableClientRoot::Block(block) => {
                crate::walk_client_block(block, &mut collect);
            }
        },
    );
    for handler in payload_handlers {
        crate::walk_client_block(&handler.body, &mut collect);
    }
    for control in lifecycle_controls {
        crate::walk_client_expr(
            island.exprs.client_expr(control.producer_expression_id),
            &mut collect,
        );
    }
    if let Some((edge, first, second)) = conflicting_arity {
        return Err(LinkedHelperEmitError::ConflictingRootCallArity {
            edge,
            first,
            second,
        });
    }
    program.validate_root_calls(&actual)?;

    let targets = program.root_target_signatures();
    let mut type_error = None;
    for_each_reachable_client_root(
        island,
        ctx,
        reachable_signal_ids,
        reachable_query_ids,
        reachable_mutation_ids,
        reachable_component_fragment_mounts,
        &mut |root| {
            if type_error.is_some() {
                return;
            }
            let result = match root {
                ReachableClientRoot::Arena(expression) => validate_client_root_helper_expr_types(
                    island.exprs.client_expr(expression),
                    TypeEnv::for_island(island),
                    &targets,
                    None,
                ),
                ReachableClientRoot::Block(block) => validate_client_root_helper_block_types(
                    block,
                    TypeEnv::for_island(island),
                    &targets,
                ),
            };
            if let Err(error) = result {
                type_error = Some(error);
            }
        },
    );
    for handler in payload_handlers {
        if type_error.is_some() {
            break;
        }
        if let Err(error) = validate_client_root_helper_block_types(
            &handler.body,
            TypeEnv::for_island(island),
            &targets,
        ) {
            type_error = Some(error);
        }
    }

    // Arena expressions whose consumer supplies an exact type contract get a
    // second, contextual check. The general pass above checks calls and all
    // typed operators inside each expression; these checks cover the contract
    // outside the expression tree itself.
    if type_error.is_none() {
        'conditionals: for (parent, sections) in &island.bindings.conditional_sections {
            if !ctx.includes_node(*parent) {
                continue;
            }
            for section in sections {
                if let Err(error) = validate_client_root_helper_expr_types(
                    island.exprs.client_expr(section.condition),
                    TypeEnv::for_island(island),
                    &targets,
                    Some((&ClientType::Bool, ClientHelperTypeSite::IfCondition)),
                ) {
                    type_error = Some(error);
                    break 'conditionals;
                }
            }
        }
    }
    if type_error.is_none() {
        for signal in &island.signals {
            if !reachable_signal_ids.contains(&signal.id) || !ctx.includes_signal(signal.id) {
                continue;
            }
            let Some(expression) = signal.derived else {
                continue;
            };
            if signal.ty.is_unknown() {
                continue;
            }
            if let Err(error) = validate_client_root_helper_expr_types(
                island.exprs.client_expr(expression),
                TypeEnv::for_island(island),
                &targets,
                Some((&signal.ty, ClientHelperTypeSite::SignalSet(signal.id))),
            ) {
                type_error = Some(error);
                break;
            }
        }
    }
    if type_error.is_none() {
        for control in lifecycle_controls {
            if let Err(error) = validate_client_root_helper_expr_types(
                island.exprs.client_expr(control.producer_expression_id),
                TypeEnv::for_island(island),
                &targets,
                Some((&ClientType::Bool, ClientHelperTypeSite::IfCondition)),
            ) {
                type_error = Some(error);
                break;
            }
        }
    }

    type_error.map_or(Ok(()), |error| Err(error.into()))
}

fn linked_helper_uses_runtime_helpers(program: &LinkedHelperProgram) -> bool {
    program
        .helper_bodies()
        .any(client_block_uses_runtime_helpers)
}

fn linked_helper_numeric_runtime_helpers(program: &LinkedHelperProgram) -> BTreeSet<&'static str> {
    let mut helpers = BTreeSet::new();
    for block in program.helper_bodies() {
        extend_numeric_runtime_helpers_from_block(&mut helpers, block);
    }
    helpers
}

fn extend_numeric_runtime_helpers_from_block(
    helpers: &mut BTreeSet<&'static str>,
    block: &ClientBlock,
) {
    crate::walk_client_block(block, &mut |expression| {
        record_numeric_runtime_helper(helpers, expression);
    });
}

fn extend_numeric_runtime_helpers_from_expr(
    helpers: &mut BTreeSet<&'static str>,
    expression: &ClientExpr,
) {
    crate::walk_client_expr(expression, &mut |expression| {
        record_numeric_runtime_helper(helpers, expression);
    });
}

fn record_numeric_runtime_helper(helpers: &mut BTreeSet<&'static str>, expression: &ClientExpr) {
    match expression {
        ClientExpr::Binary { op, .. } => {
            if let Some(helper) = op.dyn_helper_js() {
                helpers.insert(helper);
            }
            if let Some(helper) = op.checked_helper_js() {
                helpers.insert(helper);
            }
        }
        ClientExpr::Unary {
            op: ClientUnaryOp::CheckedNeg(_),
            ..
        } => {
            helpers.insert("checked_int_neg");
        }
        _ => {}
    }
}

fn island_uses_runtime_helpers(
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_query_ids: &[QueryId],
    reachable_mutation_ids: &[MutationId],
    reachable_component_fragment_mounts: &[(NodeId, String)],
) -> bool {
    let mut uses = false;
    for_each_reachable_client_root(
        island,
        ctx,
        reachable_signal_ids,
        reachable_query_ids,
        reachable_mutation_ids,
        reachable_component_fragment_mounts,
        &mut |root| {
            if uses {
                return;
            }
            uses = match root {
                ReachableClientRoot::Arena(expr) => arena_expr_uses_runtime_helpers(island, expr),
                ReachableClientRoot::Block(block) => client_block_uses_runtime_helpers(block),
            };
        },
    );
    uses
}

/// Numeric runtime helpers this island's emitted JS calls.
///
/// A shared-state field types as [`rsjs_ir::ClientType::Dyn`]: the compiler
/// cannot prove whether the browser holds it as a BigInt or a Number, because
/// that is decided by the page seed's `wide_paths`. Every arithmetic or
/// equality operator with such an operand therefore lowers to a `Dyn*`
/// [`ClientBinaryOp`], which emits a helper call instead of a bare JS operator.
/// Those helpers have to be imported. Proven fixed-width integer operations
/// also lower to `checked_int_*` helpers; collect both families in one
/// reachability-scoped walk so emission and imports cannot drift.
fn island_numeric_runtime_helpers(
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_query_ids: &[QueryId],
    reachable_mutation_ids: &[MutationId],
    reachable_component_fragment_mounts: &[(NodeId, String)],
) -> BTreeSet<&'static str> {
    let mut helpers = BTreeSet::new();
    for_each_reachable_client_root(
        island,
        ctx,
        reachable_signal_ids,
        reachable_query_ids,
        reachable_mutation_ids,
        reachable_component_fragment_mounts,
        &mut |root| match root {
            ReachableClientRoot::Arena(expr) => {
                extend_numeric_runtime_helpers_from_expr(
                    &mut helpers,
                    island.exprs.client_expr(expr),
                );
            }
            ReachableClientRoot::Block(block) => {
                extend_numeric_runtime_helpers_from_block(&mut helpers, block);
            }
        },
    );
    helpers
}

fn arena_expr_uses_runtime_helpers(island: &Island, expr: ExprId) -> bool {
    client_expr_uses_runtime_helpers(island.exprs.client_expr(expr))
}

fn client_block_uses_runtime_helpers(block: &ClientBlock) -> bool {
    block.stmts.iter().any(client_stmt_uses_runtime_helpers)
}

fn client_node_ref_effect_uses_runtime_helpers(effect: &ClientNodeRefEffect) -> bool {
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
        | ClientNodeRefEffect::Select => false,
        ClientNodeRefEffect::ScrollTo { x, y } | ClientNodeRefEffect::ScrollBy { x, y } => {
            client_expr_uses_runtime_helpers(x) || client_expr_uses_runtime_helpers(y)
        }
        ClientNodeRefEffect::SetPointerCapture(expr)
        | ClientNodeRefEffect::ReleasePointerCapture(expr) => {
            client_expr_uses_runtime_helpers(expr)
        }
        ClientNodeRefEffect::SetValue(expr)
        | ClientNodeRefEffect::SetChecked(expr)
        | ClientNodeRefEffect::SetCustomValidity(expr)
        | ClientNodeRefEffect::SetTextContent(expr)
        | ClientNodeRefEffect::AddClass(expr)
        | ClientNodeRefEffect::RemoveClass(expr)
        | ClientNodeRefEffect::ToggleClass(expr) => client_expr_uses_runtime_helpers(expr),
        ClientNodeRefEffect::SetAttribute { value, .. } => client_expr_uses_runtime_helpers(value),
        ClientNodeRefEffect::RemoveAttribute { .. } => false,
        ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            client_expr_uses_runtime_helpers(value)
        }
        ClientNodeRefEffect::SetSelectionRange(start, end) => {
            client_expr_uses_runtime_helpers(start) || client_expr_uses_runtime_helpers(end)
        }
    }
}

fn client_browser_effect_uses_runtime_helpers(effect: &ClientBrowserEffect) -> bool {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            client_expr_uses_runtime_helpers(x) || client_expr_uses_runtime_helpers(y)
        }
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => false,
        ClientBrowserEffect::LocationSetHash { value } => client_expr_uses_runtime_helpers(value),
        ClientBrowserEffect::LocationAssign { url }
        | ClientBrowserEffect::LocationReplace { url }
        | ClientBrowserEffect::HistoryPushState { url }
        | ClientBrowserEffect::HistoryReplaceState { url } => client_expr_uses_runtime_helpers(url),
        ClientBrowserEffect::HistoryGo { delta } => client_expr_uses_runtime_helpers(delta),
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            client_expr_uses_runtime_helpers(name)
                || detail
                    .as_ref()
                    .is_some_and(client_expr_uses_runtime_helpers)
        }
        ClientBrowserEffect::DocumentSetTitle { title } => client_expr_uses_runtime_helpers(title),
        ClientBrowserEffect::ClipboardWriteText { text } => client_expr_uses_runtime_helpers(text),
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => fallback
            .as_ref()
            .is_some_and(client_expr_uses_runtime_helpers),
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
            client_expr_uses_runtime_helpers(options_json)
                || fallback
                    .as_ref()
                    .is_some_and(client_expr_uses_runtime_helpers)
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. }
        | ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            client_expr_uses_runtime_helpers(key) || client_expr_uses_runtime_helpers(value)
        }
        ClientBrowserEffect::StorageRemoveItem { key, .. }
        | ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => {
            client_expr_uses_runtime_helpers(key)
        }
        ClientBrowserEffect::Declared { args, .. } => {
            args.iter().any(client_expr_uses_runtime_helpers)
        }
    }
}

fn client_stmt_uses_runtime_helpers(stmt: &ClientStmt) -> bool {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => client_expr_uses_runtime_helpers(expr),
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            value.as_ref().is_some_and(client_expr_uses_runtime_helpers)
        }
        ClientStmt::SignalUpdate { closure, .. } => client_expr_uses_runtime_helpers(&closure.body),
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => false,
        ClientStmt::Console { args, .. } => args.iter().any(client_expr_uses_runtime_helpers),
        ClientStmt::CallbackInvoke { payload, .. } => client_expr_uses_runtime_helpers(payload),
        ClientStmt::RunMutation { args, .. } => args.iter().any(client_expr_uses_runtime_helpers),
        ClientStmt::BrowserEffect(effect) => client_browser_effect_uses_runtime_helpers(effect),
        ClientStmt::NodeRefEffect { effect, .. } => {
            client_node_ref_effect_uses_runtime_helpers(effect)
        }
        ClientStmt::Block(block) => client_block_uses_runtime_helpers(block),
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            client_expr_uses_runtime_helpers(condition)
                || client_block_uses_runtime_helpers(then_block)
                || else_block
                    .as_ref()
                    .is_some_and(client_block_uses_runtime_helpers)
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            client_expr_uses_runtime_helpers(start)
                || client_expr_uses_runtime_helpers(end)
                || client_block_uses_runtime_helpers(body)
        }
    }
}

fn client_expr_uses_runtime_helpers(expr: &ClientExpr) -> bool {
    match expr {
        ClientExpr::Project { .. }
        | ClientExpr::AsType { .. }
        | ClientExpr::IsPresent { .. }
        | ClientExpr::Eq { .. }
        | ClientExpr::Neq { .. }
        | ClientExpr::Coalesce { .. } => true,
        ClientExpr::Encoding { value, .. }
        | ClientExpr::DomRead { target: value, .. }
        | ClientExpr::IsNonEmpty { target: value }
        | ClientExpr::StringToLowercase { target: value }
        | ClientExpr::StringTransform { target: value, .. }
        | ClientExpr::Unary { expr: value, .. }
        | ClientExpr::Field { base: value, .. } => client_expr_uses_runtime_helpers(value),
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        } => client_expr_uses_runtime_helpers(haystack) || client_expr_uses_runtime_helpers(needle),
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            client_expr_uses_runtime_helpers(condition)
                || client_expr_uses_runtime_helpers(then_expr)
                || client_expr_uses_runtime_helpers(else_expr)
        }
        ClientExpr::Binary { left, right, .. } => {
            client_expr_uses_runtime_helpers(left) || client_expr_uses_runtime_helpers(right)
        }
        ClientExpr::Index { base, index } => {
            client_expr_uses_runtime_helpers(base) || client_expr_uses_runtime_helpers(index)
        }
        ClientExpr::QueryKey(parts) => parts.iter().any(client_expr_uses_runtime_helpers),
        ClientExpr::StructInit { fields, .. } => fields
            .iter()
            .any(|field| client_expr_uses_runtime_helpers(&field.value)),
        ClientExpr::Call { args, .. } => args.iter().any(client_expr_uses_runtime_helpers),
        ClientExpr::BrowserRead(ClientBrowserRead::Window(_))
        | ClientExpr::BrowserRead(ClientBrowserRead::Location(_)) => false,
        ClientExpr::BrowserRead(ClientBrowserRead::Storage { key, fallback, .. })
        | ClientExpr::BrowserRead(ClientBrowserRead::UrlSearchParam { key, fallback }) => {
            client_expr_uses_runtime_helpers(key)
                || fallback
                    .as_ref()
                    .is_some_and(|fallback| client_expr_uses_runtime_helpers(fallback))
        }
        ClientExpr::BrowserRead(ClientBrowserRead::Declared { args, .. }) => {
            args.iter().any(client_expr_uses_runtime_helpers)
        }
        ClientExpr::EventCollectionRead { index, .. } => client_expr_uses_runtime_helpers(index),
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
        | ClientExpr::NodePayload { .. } => false,
    }
}

/// Build the runtime-import line for this island. The base set is
/// `signal, bind_text, bind_conditional, hydrate` (every island needs
/// them). Signal-state readers, `bind_input`, `bind_submit`, and
/// `bind_delegated_events` are added only when the island shape needs
/// them.
///
/// `bind_event` is no longer imported: per OPTIMIZATIONS item #2,
/// non-submit events route through `bind_delegated_events` instead
/// (one listener per event-name per island root). The runtime still
/// exports `bind_event` for backward compatibility with hand-written
/// callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeImportMode {
    Island,
    Fragment,
}

fn runtime_import_line(
    island: &Island,
    mode: RuntimeImportMode,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_node_ref_ids: &[NodeRefId],
    reachable_query_ids: &[QueryId],
    numeric_runtime_helpers: &BTreeSet<&'static str>,
) -> String {
    let mut imports: Vec<&'static str> = vec!["signal", "bind_text", "bind_conditional"];
    let local_leaf_signals = || {
        island.signals.iter().filter(|sig| {
            sig.derived.is_none()
                && sig.shared.is_none()
                && ctx.includes_signal(sig.id)
                && reachable_signal_ids.contains(&sig.id)
        })
    };
    let uses_typed_seed_decoder =
        || local_leaf_signals().any(|sig| client_type_needs_typed_seed_decode(&sig.ty));
    if local_leaf_signals().next().is_some() {
        imports.push("read_signal_initials");
    }
    if local_leaf_signals().any(|sig| {
        sig.integer_domain.is_none()
            && !matches!(sig.init, Value::BigInt(_))
            && !client_type_needs_typed_seed_decode(&sig.ty)
    }) {
        imports.push("signal_initial");
    }
    if local_leaf_signals()
        .any(|sig| sig.integer_domain.is_none() && matches!(sig.init, Value::BigInt(_)))
    {
        // Wide-int seeds decode their JSON-string wire form to BigInt.
        imports.push("signal_initial_wide");
    }
    if local_leaf_signals().any(|sig| sig.integer_domain.is_some()) {
        imports.push("integer_signal");
        imports.push("signal_initial_integer");
    }
    if uses_typed_seed_decoder() {
        imports.push("signal_initial_typed");
    }
    if island.signals.iter().any(|sig| {
        sig.shared.is_some()
            && ctx.includes_signal(sig.id)
            && reachable_signal_ids.contains(&sig.id)
    }) {
        imports.push("acquire_shared_state");
    }
    if island.signals.iter().any(|sig| {
        sig.derived.is_some()
            && ctx.includes_signal(sig.id)
            && reachable_signal_ids.contains(&sig.id)
    }) || island_has_live_query_keys(island, reachable_query_ids)
    {
        imports.push("memo");
    }
    if !island.intervals.is_empty() {
        imports.push("interval");
    }
    if island
        .lists
        .iter()
        .any(|list| ctx.includes_node(list.container))
    {
        imports.push("bind_list");
    }
    if island.handlers.iter().any(|handler| {
        handler.debounce_ms().is_some()
            && handler_is_bound_to_included_node(island, ctx, handler.id)
    }) {
        imports.push("debounce");
    }
    if island
        .bindings
        .input_bindings
        .keys()
        .any(|node| ctx.includes_node(*node))
    {
        imports.push("bind_input");
    }
    if island
        .bindings
        .attr_bindings
        .values()
        .any(|binding| ctx.includes_node(binding.node))
    {
        imports.push("bind_attr");
    }
    let has_submit = island
        .bindings
        .event_bindings
        .keys()
        .any(|(node, ev)| ctx.includes_node(*node) && matches!(ev, EventName::Submit));
    let has_document = island
        .bindings
        .event_bindings
        .keys()
        .any(|(node, ev)| ctx.includes_node(*node) && ev.is_document_event());
    let has_window = island
        .bindings
        .event_bindings
        .keys()
        .any(|(node, ev)| ctx.includes_node(*node) && ev.is_window_event());
    let has_delegated = island.bindings.event_bindings.keys().any(|(node, ev)| {
        ctx.includes_node(*node)
            && !matches!(ev, EventName::Submit)
            && !ev.is_document_event()
            && !ev.is_window_event()
    });
    if has_submit {
        imports.push("bind_submit");
    }
    if has_document {
        imports.push("bind_document_event");
    }
    if has_window {
        imports.push("bind_window_event");
    }
    if has_delegated {
        imports.push("bind_delegated_events");
    }
    if island_needs_node_walk(island, ctx, reachable_node_ref_ids) {
        imports.push("walk_path");
    }
    if mode == RuntimeImportMode::Island {
        if supports_managed_module_import(&island.hydration_strategy) {
            imports.push("hydrate_from_module");
        }
        imports.push(hydration_runtime_import(&island.hydration_strategy));
    }
    if island_has_disposers(
        island,
        ctx,
        reachable_signal_ids,
        reachable_node_ref_ids,
        reachable_query_ids,
    ) {
        imports.push("dispose_all");
    }
    if island
        .component_fragment_mounts
        .iter()
        .any(|mount| mount.requires_parent_runtime())
    {
        imports.push("report_hydration_error");
    }
    // Representation-dispatch helpers are prepared from the exact same
    // reachability object as emitted code. Fragment callers also include
    // linked payload/lifecycle roots and resolved helper bodies here.
    imports.extend(numeric_runtime_helpers.iter().copied());
    let mut out = String::from("import { ");
    for (i, name) in imports.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(name);
    }
    out.push_str(" } from \"/rsjs-runtime.js\";\n");
    out
}

/// Whether a compound signal seed contains fixed-width integers that require
/// recursive hydration decoding. Top-level integers use the stricter
/// `signal_initial_integer` + `integer_signal` path; this predicate covers
/// integers nested below Option/Vec/object containers.
fn client_type_needs_typed_seed_decode(ty: &ClientType) -> bool {
    fn contains_integer(ty: &ClientType) -> bool {
        match ty {
            ClientType::I32 | ClientType::U32 | ClientType::I64 | ClientType::U64 => true,
            ClientType::Option(inner)
            | ClientType::Vec(inner)
            | ClientType::Signal(inner)
            | ClientType::Callback(inner) => contains_integer(inner),
            ClientType::Object(fields) => fields.iter().any(|field| contains_integer(&field.ty)),
            _ => false,
        }
    }

    matches!(
        ty,
        ClientType::Option(_) | ClientType::Vec(_) | ClientType::Object(_)
    ) && contains_integer(ty)
}

/// Emit `const __sig_<id> = signal(<initial>);` for every leaf signal
/// and `const __sig_<id> = memo(() => <body>, [<deps>]);` for
/// every derived cell.
///
/// Declarations are emitted in id order. Source order guarantees a
/// memo cell's dependencies were declared before it (the body
/// closure can only reference names already in scope), so every
/// `__sig_<n>` a memo body reads exists by the time its line runs.
fn emit_signals(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
) {
    let signals: Vec<_> = island
        .signals
        .iter()
        .filter(|decl| ctx.includes_signal(decl.id) && reachable_signal_ids.contains(&decl.id))
        .collect();
    if signals.is_empty() {
        return;
    }
    if signals
        .iter()
        .any(|decl| decl.derived.is_none() && decl.shared.is_none())
    {
        out.push_str(INDENT);
        if ctx.root_mode == RootMode::FragmentRoot {
            out.push_str(
                "const __rsjs_signal_initials = read_signal_initials(options.seedRoot ?? root);\n",
            );
        } else {
            out.push_str("const __rsjs_signal_initials = read_signal_initials(root);\n");
        }
        if ctx.root_mode == RootMode::FragmentRoot {
            out.push_str(
                "    const __rsjs_signal_overrides = options.signals ?? {};\n    const __rsjs_signal_seeds = options.signalSeeds ?? {};\n",
            );
        }
    }
    let mut acquired_shared_stores = HashSet::new();
    for decl in signals {
        if let Some(shared) = &decl.shared {
            if acquired_shared_stores.insert(shared.store) {
                out.push_str("    const __rsjs_shared_");
                let _ = write!(out, "{}", shared.store.0);
                out.push_str(" = acquire_shared_state(");
                if ctx.root_mode == RootMode::FragmentRoot {
                    out.push_str("options.seedRoot ?? root");
                } else {
                    out.push_str("root");
                }
                out.push_str(", ");
                let _ = write!(out, "{}", shared.store.0);
                out.push_str(");\n");
            }
            let _ = write!(
                out,
                "    const {v} = __rsjs_shared_",
                v = signal_var(decl.id)
            );
            let _ = write!(out, "{}", shared.store.0);
            let _ = writeln!(out, ".field(\"{value}\");", value = JsStr(&shared.field));
            continue;
        }
        if let Some(body) = decl.derived {
            // Derived cell: re-evaluated from its source signals; never
            // seeded from the dehydrated blob and never overridable by
            // a parent fragment mount.
            let _ = write!(out, "    const {v} = memo(() => ", v = signal_var(decl.id));
            emit_arena_expr(out, island, body, ctx);
            out.push_str(", ");
            emit_dep_array(out, &collect_deps(&island.exprs, body));
            out.push_str(");\n");
            let _ = writeln!(
                out,
                "    __rsjs_disposers.push({v}._dispose);",
                v = signal_var(decl.id)
            );
            continue;
        }
        let _ = write!(out, "    const {v}", v = signal_var(decl.id));
        if ctx.root_mode == RootMode::FragmentRoot {
            out.push_str(" = __rsjs_signal_overrides[");
            let _ = write!(out, "{}", decl.id.0);
            out.push_str("] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, ");
            let _ = write!(out, "{}", decl.id.0);
            out.push_str(") ? ");
            emit_linked_signal_seed_initializer(out, decl);
            out.push_str(" : ");
            emit_ssr_signal_initializer(out, decl);
            out.push_str(");\n");
            continue;
        }
        out.push_str(" = ");
        emit_ssr_signal_initializer(out, decl);
        out.push_str(";\n");
    }
}

/// Emit a fresh child-owned signal from a linked `SeedOnce` value.
///
/// This value is already in the browser representation of the rustc-checked
/// parent/child field type, so it must not pass through the JSON seed decoder.
/// Fixed-width integers still use `integer_signal` so subsequent writes retain
/// their proven arithmetic domain.
fn emit_linked_signal_seed_initializer(out: &mut String, decl: &crate::reactive::SignalDecl) {
    if let Some(domain) = decl.integer_domain {
        out.push_str("integer_signal(__rsjs_signal_seeds[");
        let _ = write!(out, "{}", decl.id.0);
        let _ = write!(out, "], \"{domain}\")", domain = domain.as_str());
    } else {
        out.push_str("signal(__rsjs_signal_seeds[");
        let _ = write!(out, "{}", decl.id.0);
        out.push_str("])");
    }
}

/// Emit the ordinary adjacent-SSR-seed initializer for one leaf signal.
fn emit_ssr_signal_initializer(out: &mut String, decl: &crate::reactive::SignalDecl) {
    // Fixed-width integer signals retain their proven Rust domain across every
    // write. Domain-aware seed decoding validates the hydrated value, and
    // `integer_signal` validates/coerces before mutation.
    if let Some(domain) = decl.integer_domain {
        out.push_str("integer_signal(signal_initial_integer(__rsjs_signal_initials, ");
        let _ = write!(out, "{}", decl.id.0);
        out.push_str(", ");
        emit_value(out, &decl.init);
        let _ = write!(
            out,
            ", \"{domain}\"), \"{domain}\")",
            domain = domain.as_str()
        );
        return;
    }
    // Container types retain the exact domains of nested integer values. The
    // JSON codec transports nested i64/u64 leaves as canonical decimal
    // strings, so decode recursively before the value enters the signal.
    if client_type_needs_typed_seed_decode(&decl.ty) {
        out.push_str("signal(signal_initial_typed(__rsjs_signal_initials, ");
        let _ = write!(out, "{}", decl.id.0);
        out.push_str(", ");
        emit_value(out, &decl.init);
        out.push_str(", ");
        write_client_type_json(out, &decl.ty);
        out.push_str("))");
        return;
    }
    // Legacy representation-only BigInt seeds still decode their JSON-string
    // wire form, but carry no fixed-width arithmetic provenance.
    if matches!(decl.init, Value::BigInt(_)) {
        out.push_str("signal(signal_initial_wide(__rsjs_signal_initials, ");
    } else {
        out.push_str("signal(signal_initial(__rsjs_signal_initials, ");
    }
    let _ = write!(out, "{}", decl.id.0);
    out.push_str(", ");
    emit_value(out, &decl.init);
    out.push_str("))");
}

/// Emit per-query registration calls.
///
/// Each query in the island gets one `const __rsjs_query_<id> =
/// register_query(...)` line. The registration call wires:
///
/// 1. The structural cache key (a JS array, structurally compared
///    by the runtime's `QueryClient`).
/// 2. The client fetcher: a JS function the runtime invokes when
///    the refetch policy fires. v0.1 shape is "GET the URL the
///    fetcher expression produced and parse the response as JSON";
///    the runtime owns the actual `fetch` call.
/// 3. The configured options (interval, focus refetch, retry,
///    stale time).
/// 4. The dehydrated initial value: looked up by island name +
///    query id from the dehydrated `<script>` blob the server
///    emits (see `rsjs_macros::emit_rust`). The runtime seeds the
///    cache entry on first read so the island never sees a
///    `Loading` state on hydration.
///
/// Queries are emitted *before* handlers and bindings so that any
/// reactive expression downstream can reference the local handle
/// the registration produces.
fn emit_queries(
    out: &mut String,
    island: &Island,
    asset_id: &str,
    reachable_query_ids: &[QueryId],
    ctx: &EmitContext,
) {
    if reachable_query_ids.is_empty() {
        return;
    }
    for q in island
        .queries
        .iter()
        .filter(|query| reachable_query_ids.contains(&query.id))
    {
        emit_query_registration(out, q, island, asset_id, ctx);
    }
}

fn query_key_signal_deps(island: &Island, query: &crate::reactive::QueryDecl) -> Vec<SignalId> {
    // The current Client IR represents local and derived reactive state as
    // SignalRead nodes, so this completely covers signal-derived query keys.
    //
    // Live props have a normative QueryKeyDependency temporal mode in the
    // component ABI, but do not yet have a distinct expression leaf in the
    // linked Client IR. When that leaf lands, dependency collection must map
    // it to the fused owner's signal/cell here; treating every `Local` as live
    // would incorrectly subscribe static server locals.
    collect_deps(&island.exprs, query.key_expr)
}

fn island_has_live_query_keys(island: &Island, reachable_query_ids: &[QueryId]) -> bool {
    island
        .queries
        .iter()
        .filter(|query| reachable_query_ids.contains(&query.id))
        .any(|query| !query_key_signal_deps(island, query).is_empty())
}

/// Emit one query registration.
///
/// Static keys retain the compact one-shot registration shape. A key that
/// reads one or more signals gets a stable registration factory plus one
/// scheduler-backed watcher. `memo` deduplicates synchronous writes in the
/// runtime scheduler, while [`collect_deps`] deduplicates repeated reads of the
/// same signal before the dependency array is emitted.
fn emit_query_registration(
    out: &mut String,
    q: &crate::reactive::QueryDecl,
    island: &Island,
    asset_id: &str,
    ctx: &EmitContext,
) {
    let deps = query_key_signal_deps(island, q);
    if deps.is_empty() {
        emit_static_query_registration(out, q, island, asset_id, ctx);
    } else {
        emit_live_query_registration(out, q, island, asset_id, &deps, ctx);
    }
}

fn emit_static_query_registration(
    out: &mut String,
    q: &crate::reactive::QueryDecl,
    island: &Island,
    asset_id: &str,
    ctx: &EmitContext,
) {
    let _ = writeln!(
        out,
        "    const {v} = register_query({{",
        v = query_var(q.id)
    );
    emit_query_registration_fields(out, q, island, asset_id, false, ctx);
    out.push_str("    });\n");
    emit_query_disposer(out, q.id);
}

fn emit_live_query_registration(
    out: &mut String,
    q: &crate::reactive::QueryDecl,
    island: &Island,
    asset_id: &str,
    deps: &[SignalId],
    ctx: &EmitContext,
) {
    let _ = writeln!(
        out,
        "    const {v} = () => ({{",
        v = query_registration_var(q.id)
    );
    emit_query_registration_fields(out, q, island, asset_id, true, ctx);
    out.push_str("    });\n");

    let _ = writeln!(
        out,
        "    const {v} = register_query({reg}());",
        v = query_var(q.id),
        reg = query_registration_var(q.id)
    );
    emit_query_disposer(out, q.id);

    let _ = writeln!(
        out,
        "    const {v} = memo(() => {{",
        v = query_key_watcher_var(q.id)
    );
    let _ = writeln!(
        out,
        "        rebind_query({query}, {reg}());",
        query = query_var(q.id),
        reg = query_registration_var(q.id)
    );
    out.push_str("        return null;\n    }, ");
    emit_dep_array(out, deps);
    out.push_str(");\n");
    let _ = writeln!(
        out,
        "    __rsjs_disposers.push({v}._dispose);",
        v = query_key_watcher_var(q.id)
    );
}

fn emit_query_disposer(out: &mut String, query: QueryId) {
    let _ = writeln!(
        out,
        "    __rsjs_disposers.push(() => dispose_query({v}));",
        v = query_var(query)
    );
}

fn emit_query_registration_fields(
    out: &mut String,
    q: &crate::reactive::QueryDecl,
    island: &Island,
    asset_id: &str,
    live_key: bool,
    ctx: &EmitContext,
) {
    // Asset id + query id together identify the dehydrated state
    // entry the runtime seeds the cache from.
    let _ = writeln!(out, "        island: \"{value}\",", value = JsStr(asset_id));

    out.push_str("        query_id: ");
    let _ = write!(out, "{}", q.id.0);
    out.push_str(",\n");

    out.push_str("        ");
    match ctx.root_mode {
        RootMode::IslandWrapper => out.push_str("root,\n"),
        RootMode::FragmentRoot => out.push_str("root: options.seedRoot ?? root,\n"),
    }

    if live_key {
        out.push_str("        live_key: true,\n");
    }

    // Structural cache key. Legacy single-expression keys are
    // wrapped as one-part arrays; QueryKey::new(...) emits the
    // structural array directly.
    out.push_str("        key: ");
    emit_query_key_expr(out, island, q.key_expr, ctx);
    out.push_str(",\n");

    // Client fetcher config: lower the macro-validated literal URL or
    // FetchRequest descriptor. The runtime parses both shapes and owns
    // method/body/header/credentials/timeout/response behavior.
    out.push_str("        fetcher: ");
    if live_key {
        // A live key owns an immutable request snapshot per cache identity.
        // Emitting the descriptor directly inside the registration evaluator
        // prevents an old entry from calling a closure that reads the new
        // signal value after the observer has moved away.
        emit_arena_expr(out, island, q.client_fetcher, ctx);
    } else {
        emit_request_config_expr(out, island, q.client_fetcher, ctx);
    }
    out.push_str(",\n");

    // Options. The runtime defaults match the IR defaults so
    // omitting an interval emits `interval_ms: null`.
    out.push_str("        options: { interval_ms: ");
    match q.options.interval_ms {
        Some(ms) => {
            let _ = write!(out, "{ms}");
        }
        None => out.push_str("null"),
    }
    let _ = write!(
        out,
        ", refetch_on_focus: {focus}, download_progress: {progress}, stream_text: {stream}, retry_count: ",
        focus = q.options.refetch_on_focus,
        progress = q.options.download_progress,
        stream = q.options.stream_text
    );
    let _ = write!(out, "{}", q.options.retry_count);
    out.push_str(", stale_time_ms: ");
    let _ = write!(out, "{}", q.options.stale_time_ms);
    out.push_str(" },\n");
}

/// Emit per-mutation registration calls.
///
/// Each mutation in the island gets one
/// `const __rsjs_mutation_<id> = register_mutation({...})` line.
/// The registration call wires:
///
/// 1. The structural mutation key (a JS array, structurally compared
///    by the runtime's `MUTATIONS` map).
/// 2. The runner config: a JS string or object the runtime parses
///    into method+url+body. v0.1 lowers the runner expression
///    verbatim; the runtime accepts both bare URLs and JSON-encoded
///    config objects.
/// 3. The invalidation list: the structural QueryKeys to invalidate
///    on successful run().
/// 4. The configured options (retry count, on-success invalidate
///    strategy).
///
/// Mutations are emitted *after* queries and *before* handlers so
/// that handler bodies can reference the local handle the
/// registration produces (the `RunMutation` lowering names the
/// handle directly).
fn emit_mutations(
    out: &mut String,
    island: &Island,
    asset_id: &str,
    reachable_mutation_ids: &[MutationId],
    ctx: &EmitContext,
) {
    if reachable_mutation_ids.is_empty() {
        return;
    }
    for m in island
        .mutations
        .iter()
        .filter(|mutation| reachable_mutation_ids.contains(&mutation.id))
    {
        emit_mutation_registration(out, m, island, asset_id, ctx);
    }
}

/// Emit a single mutation registration line.
fn emit_mutation_registration(
    out: &mut String,
    m: &crate::reactive::MutationDecl,
    island: &Island,
    asset_id: &str,
    ctx: &EmitContext,
) {
    let _ = writeln!(
        out,
        "    const {v} = register_mutation({{",
        v = mutation_var(m.id)
    );

    let _ = writeln!(out, "        island: \"{value}\",", value = JsStr(asset_id));

    out.push_str("        mutation_id: ");
    let _ = write!(out, "{}", m.id.0);
    out.push_str(",\n");

    out.push_str("        ");
    match ctx.root_mode {
        RootMode::IslandWrapper => out.push_str("root,\n"),
        RootMode::FragmentRoot => out.push_str("root: options.seedRoot ?? root,\n"),
    }

    // Structural mutation key.
    out.push_str("        key: ");
    emit_query_key_expr(out, island, m.key_expr, ctx);
    out.push_str(",\n");

    // Runner config: lower the macro-validated literal URL or FetchRequest
    // descriptor. The runtime parses both shapes and applies run(args) as
    // the fallback JSON body when the descriptor does not carry a static body.
    out.push_str("        runner: ");
    emit_request_config_expr(out, island, m.client_runner, ctx);
    out.push_str(",\n");

    // Invalidates list. Each entry is a structural QueryKey,
    // emitted as its own JS array.
    out.push_str("        invalidates: [");
    for (i, inv) in m.invalidates.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        emit_query_key_expr(out, island, *inv, ctx);
    }
    out.push_str("],\n");

    // Options.
    out.push_str("        options: { retry_count: ");
    let _ = write!(out, "{}", m.options.retry_count);
    let _ = writeln!(
        out,
        ", on_success_invalidate_strategy: \"{strategy}\" }},",
        strategy = match m.options.on_success_invalidate_strategy {
            crate::reactive::InvalidationStrategy::Refetch => "refetch",
            crate::reactive::InvalidationStrategy::Reset => "reset",
        }
    );

    out.push_str("    });\n");
}

fn emit_handlers_v3(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
) -> Result<(), ClientEmitError> {
    let client_ctx = ctx.client_emit_context();
    for handler in &island.handlers {
        if !handler_is_bound_to_included_node(island, ctx, handler.id) {
            continue;
        }
        if let Some(ms) = handler.debounce_ms() {
            // Debounced handler: the runtime `debounce(fn, delay_ms)` wrapper
            // owns the trailing-edge timer; the delegation tables
            // reference the wrapped function by the usual
            // `__handler_<id>` name, and the wrapper's `_dispose`
            // joins the island disposer list so teardown cancels any
            // pending trailing call.
            out.push('\n');
            let _ = write!(out, "    const {v}", v = handler_var(handler.id));
            out.push_str(" = debounce((event) => {");
            out.push('\n');
            emit_client_block_js_with_context_and_indent(out, handler.body_ir(), &client_ctx, 2)?;
            let _ = writeln!(out, "    }}, {ms});");
            let _ = writeln!(
                out,
                "    __rsjs_disposers.push({v}._dispose);",
                v = handler_var(handler.id)
            );
            continue;
        }
        out.push('\n');
        let _ = writeln!(
            out,
            "    function {v}(event) {{",
            v = handler_var(handler.id)
        );
        emit_client_block_js_with_context_and_indent(out, handler.body_ir(), &client_ctx, 2)?;
        out.push_str("    }\n");
    }
    Ok(())
}

/// Per-element metadata gathered from one keyed-list item template.
///
/// `var` is the JS identifier used for the element inside the
/// generated `create` callback. `path` is the element-index walk from
/// the item root (`children[i]` hops, which skip text nodes) so the
/// `adopt` callback can re-resolve the same element on a
/// server-rendered subtree. `text_expr` is the full `textContent`
/// expression for slot-bearing elements; `static_text` is the literal
/// `textContent` for elements whose children are static text only.
struct ListElementInfo {
    tag: String,
    static_attrs: Vec<(String, String)>,
    dynamic_attrs: Vec<(String, ListItemExpr)>,
    var: String,
    path: Vec<usize>,
    parent_var: Option<String>,
    text_expr: Option<String>,
    static_text: Option<String>,
}

/// Flatten a keyed-list item template into pre-order element records.
fn collect_list_elements(root: &ListTemplateNode, list_index: usize) -> Vec<ListElementInfo> {
    fn walk(
        node: &ListTemplateNode,
        list_index: usize,
        path: Vec<usize>,
        parent_var: Option<String>,
        out: &mut Vec<ListElementInfo>,
    ) {
        let ListTemplateNode::Element {
            tag,
            static_attrs,
            dynamic_attrs,
            children,
        } = node
        else {
            return;
        };
        let var = format!("__li_{}_{}", list_index, out.len());
        let has_slot = children
            .iter()
            .any(|child| matches!(child, ListTemplateNode::Slot(_)));
        let text_level = children
            .iter()
            .all(|child| !matches!(child, ListTemplateNode::Element { .. }));
        let mut info = ListElementInfo {
            tag: tag.clone(),
            static_attrs: static_attrs.clone(),
            dynamic_attrs: dynamic_attrs.clone(),
            var: var.clone(),
            path,
            parent_var,
            text_expr: None,
            static_text: None,
        };
        if text_level {
            if has_slot {
                info.text_expr = Some(list_text_content_expr(children));
            } else if !children.is_empty() {
                let mut text = String::new();
                for child in children {
                    if let ListTemplateNode::Text(t) = child {
                        text.push_str(t);
                    }
                }
                info.static_text = Some(text);
            }
        }
        let base_path = info.path.clone();
        out.push(info);
        if !text_level {
            for (i, child) in children.iter().enumerate() {
                let mut child_path = base_path.clone();
                child_path.push(i);
                walk(child, list_index, child_path, Some(var.clone()), out);
            }
        }
    }

    let mut out = Vec::new();
    walk(root, list_index, Vec::new(), None, &mut out);
    out
}

/// Build the `textContent` expression for a slot-bearing element:
/// static segments as JS string literals, slots as
/// `String(<item expr>)`, joined with `+`.
fn list_text_content_expr(children: &[ListTemplateNode]) -> String {
    let mut out = String::new();
    for child in children {
        if !out.is_empty() {
            out.push_str(" + ");
        }
        match child {
            ListTemplateNode::Text(t) => {
                out.push('"');
                out.push_str(&js_string_inner(t));
                out.push('"');
            }
            ListTemplateNode::Slot(expr) => {
                let _ = write!(out, "String({v}", v = expr.js());
                out.push(')');
            }
            ListTemplateNode::Element { .. } => {}
        }
    }
    out
}

/// Emit the per-item `update` arrow over the slot-bearing elements
/// followed by the shared `{ node, update }` record return.
fn emit_list_update_and_return(
    out: &mut String,
    elements: &[ListElementInfo],
    root_expr: &str,
    apply_initial: bool,
) {
    out.push_str("        const update = (item) => {\n");
    for info in elements {
        for (name, expr) in &info.dynamic_attrs {
            out.push_str("            ");
            let _ = writeln!(
                out,
                "{}.setAttribute(\"{}\", String({}));",
                info.var,
                js_string_inner(name),
                expr.js()
            );
        }
        if let Some(expr) = &info.text_expr {
            out.push_str("            ");
            let _ = writeln!(out, "{}.textContent = {};", info.var, expr);
        }
    }
    out.push_str("        };\n");
    if apply_initial {
        out.push_str("        update(item);\n");
    }
    out.push_str("        ");
    if root_expr == "node" {
        out.push_str("return { node, update };\n");
    } else {
        let _ = writeln!(out, "return {{ node: {root_expr}, update }};");
    }
}

/// Emit one `bind_list(container, getter, {{ key, create, adopt,
/// deps }})` block per keyed list. The `create` callback builds the
/// item subtree imperatively from the analysed template; `adopt`
/// pairs server-rendered children (matched by `data-rsjs-key`) with
/// their initial items and re-resolves the slot-bearing elements via
/// `children[i]` index walks.
fn emit_lists(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    for (i, list) in island.lists.iter().enumerate() {
        if !ctx.includes_node(list.container) {
            continue;
        }
        out.push('\n');
        emit_node_lookup(out, list.container, MARKER_LIST, declared, island, ctx);

        let elements = collect_list_elements(&list.item, i);
        let has_update = elements
            .iter()
            .any(|info| info.text_expr.is_some() || !info.dynamic_attrs.is_empty());
        let create_var = format!("__list_create_{i}");
        let adopt_var = format!("__list_adopt_{i}");

        // create(item): build the item subtree imperatively.
        out.push_str(INDENT);
        let _ = writeln!(out, "const {create_var} = (item) => {{");
        for info in &elements {
            out.push_str("        ");
            let _ = writeln!(
                out,
                "const {} = document.createElement(\"{}\");",
                info.var,
                js_string_inner(&info.tag)
            );
            for (name, value) in &info.static_attrs {
                out.push_str("        ");
                let _ = writeln!(
                    out,
                    "{}.setAttribute(\"{}\", \"{}\");",
                    info.var,
                    js_string_inner(name),
                    js_string_inner(value)
                );
            }
            if let Some(text) = &info.static_text {
                out.push_str("        ");
                let _ = writeln!(
                    out,
                    "{}.textContent = \"{}\";",
                    info.var,
                    js_string_inner(text)
                );
            }
            if let Some(parent) = &info.parent_var {
                out.push_str("        ");
                let _ = writeln!(out, "{}.appendChild({});", parent, info.var);
            }
        }
        let root_var = elements
            .first()
            .map(|info| info.var.clone())
            .unwrap_or_else(|| "null".to_string());
        if has_update {
            emit_list_update_and_return(out, &elements, &root_var, true);
        } else {
            out.push_str("        ");
            let _ = writeln!(out, "return {root_var};");
        }
        out.push_str("    };\n");

        // adopt(node, item): reuse the server-rendered child. Slots
        // are re-resolved through `children[i]` element-index walks;
        // the initial text is already correct from SSR, so `update`
        // only runs on later identity changes.
        out.push_str("    ");
        if has_update {
            let _ = writeln!(out, "const {adopt_var} = (node, item) => {{");
            for info in &elements {
                if info.text_expr.is_none() && info.dynamic_attrs.is_empty() {
                    continue;
                }
                out.push_str("        ");
                let mut resolve = String::from("node");
                for seg in &info.path {
                    let _ = write!(resolve, ".children[{seg}]");
                }
                let _ = writeln!(out, "const {} = {};", info.var, resolve);
            }
            emit_list_update_and_return(out, &elements, "node", false);
            out.push_str("    };\n");
        } else {
            let _ = writeln!(out, "const {adopt_var} = (node) => node;");
        }

        let source_deps = list_source_deps(island, list.source);

        // The bind_list call itself.
        let _ = write!(
            out,
            "    __rsjs_disposers.push(bind_list({v}, () => ",
            v = node_var(list.container)
        );
        emit_list_source_getter(out, island, list.source, ctx);
        out.push_str(", {\n        ");
        let _ = writeln!(out, "key: (item) => String({}),", list.key.js());
        out.push_str("        ");
        let _ = writeln!(out, "create: {create_var},");
        out.push_str("        ");
        let _ = writeln!(out, "adopt: {adopt_var},");
        out.push_str("        deps: ");
        emit_dep_array(out, &source_deps.signals);
        out.push_str(",\n");
        if !source_deps.queries.is_empty() || !source_deps.mutations.is_empty() {
            out.push_str("        extra_handles: ");
            emit_handle_array(out, &source_deps);
            out.push_str(",\n");
        }
        out.push_str("    }));\n");
    }
}

fn list_source_deps(island: &Island, source: ListSource) -> ReactiveDeps {
    match source {
        ListSource::Signal(signal) => ReactiveDeps {
            signals: vec![signal],
            queries: Vec::new(),
            mutations: Vec::new(),
        },
        ListSource::Expr(expr) => collect_reactive_deps(&island.exprs, expr),
    }
}

fn emit_list_source_getter(
    out: &mut String,
    island: &Island,
    source: ListSource,
    ctx: &EmitContext,
) {
    match source {
        ListSource::Signal(signal) => {
            let _ = write!(out, "{v}.get()", v = signal_var(signal));
        }
        ListSource::Expr(expr) => emit_arena_expr(out, island, expr, ctx),
    }
}

/// Emit one `__rsjs_disposers.push(interval(<ms>, () => { ... }));`
/// statement per prelude `rsjs::interval(ms, || ...)` declaration.
///
/// Emitted after every binding so a zero-delay first tick can never
/// observe a partially hydrated island; the runtime `interval()`
/// disposer clears the timer on teardown.
fn emit_intervals(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
) -> Result<(), ClientEmitError> {
    let client_ctx = ctx.client_emit_context();
    for interval in &island.intervals {
        out.push('\n');
        out.push_str(INDENT);
        let _ = write!(
            out,
            "__rsjs_disposers.push(interval({}, () => {{",
            interval.ms
        );
        out.push('\n');
        emit_client_block_js_with_context_and_indent(out, interval.body_ir(), &client_ctx, 2)?;
        out.push_str("    }));\n");
    }
    Ok(())
}

fn emit_node_refs(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    reachable_node_ref_ids: &[NodeRefId],
    imported_node_ref_ids: &[NodeRefId],
) {
    let mut node_refs: Vec<_> = island
        .node_refs
        .iter()
        .filter(|node_ref| {
            ctx.includes_node_ref(node_ref.id) && reachable_node_ref_ids.contains(&node_ref.id)
        })
        .collect();
    node_refs.sort_by_key(|node_ref| node_ref.id.0);

    for node_ref in node_refs {
        let _ = write!(out, "    const {v}", v = node_ref_var(node_ref.id));
        if imported_node_ref_ids.contains(&node_ref.id) {
            out.push_str(" = options.nodeRefs?.[");
            let _ = write!(out, "{}", node_ref.id.0);
            let v = node_ref_var(node_ref.id);
            out.push_str("];\n");
            let _ = writeln!(
                out,
                "    if ({v} == null || typeof {v} !== \"object\" || !(\"current\" in {v})) {{"
            );
            out.push_str("        throw new Error(\"linked NodeRef alias ");
            let _ = write!(out, "{}", node_ref.id.0);
            out.push_str(" was not supplied\");\n    }\n");
        } else {
            out.push_str(" = { current: null };\n");
        }
        // Register cleanup before any DOM lookup or binding can fail. The
        // hydration runtime drains this resource stack on partial-init
        // rollback and owner disposal. Because later listeners/effects are
        // pushed after this closure, reverse-order teardown releases them
        // before the author-visible handle is cleared.
        let _ = writeln!(
            out,
            "    __rsjs_disposers.push(() => {{ {v}.current = null; }});",
            v = node_ref_var(node_ref.id)
        );
    }
}

fn emit_disposer_setup(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_node_ref_ids: &[NodeRefId],
    reachable_query_ids: &[QueryId],
    owner: DisposerOwner,
) {
    if !island_has_disposers(
        island,
        ctx,
        reachable_signal_ids,
        reachable_node_ref_ids,
        reachable_query_ids,
    ) {
        return;
    }
    if owner == DisposerOwner::Fragment {
        out.push_str("    const __rsjs_disposers = [];\n");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisposerOwner {
    HydrationRuntime,
    Fragment,
}

fn emit_disposer_return(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_node_ref_ids: &[NodeRefId],
    reachable_query_ids: &[QueryId],
    is_fragment: bool,
) {
    if !island_has_disposers(
        island,
        ctx,
        reachable_signal_ids,
        reachable_node_ref_ids,
        reachable_query_ids,
    ) {
        if is_fragment {
            out.push_str("    return () => {};\n");
        }
        return;
    }
    out.push_str("    return () => dispose_all(__rsjs_disposers, \"rsjs bindings\");\n");
}

fn island_has_disposers(
    island: &Island,
    ctx: &EmitContext,
    reachable_signal_ids: &[SignalId],
    reachable_node_ref_ids: &[NodeRefId],
    reachable_query_ids: &[QueryId],
) -> bool {
    island.node_refs.iter().any(|node_ref| {
        ctx.includes_node_ref(node_ref.id) && reachable_node_ref_ids.contains(&node_ref.id)
    }) || island
        .component_fragment_mounts
        .iter()
        .any(|mount| mount.requires_parent_runtime())
        || !island.intervals.is_empty()
        || island
            .lists
            .iter()
            .any(|list| ctx.includes_node(list.container))
        || island.handlers.iter().any(|handler| {
            handler.debounce_ms().is_some()
                && handler_is_bound_to_included_node(island, ctx, handler.id)
        })
        || island.signals.iter().any(|sig| {
            sig.derived.is_some()
                && ctx.includes_signal(sig.id)
                && reachable_signal_ids.contains(&sig.id)
        })
        || !reachable_query_ids.is_empty()
        || island_has_live_query_keys(island, reachable_query_ids)
        || island
            .bindings
            .text_bindings
            .keys()
            .any(|node| ctx.includes_node(*node))
        || island
            .bindings
            .input_bindings
            .keys()
            .any(|node| ctx.includes_node(*node))
        || island
            .bindings
            .attr_bindings
            .values()
            .any(|binding| ctx.includes_node(binding.node))
        || island
            .bindings
            .event_bindings
            .keys()
            .any(|(node, _)| ctx.includes_node(*node))
        || island
            .bindings
            .conditional_sections
            .keys()
            .any(|parent| ctx.includes_node(*parent))
}

/// Emit parent-side mount calls for reusable component fragments.
fn emit_component_fragment_mounts(
    out: &mut String,
    island: &Island,
    ctx: &EmitContext,
    reachable_mounts: &[(NodeId, String)],
) {
    if reachable_mounts.is_empty() {
        return;
    }

    let reachable: HashSet<(u32, &str)> = reachable_mounts
        .iter()
        .map(|(root, asset_id)| (root.0, asset_id.as_str()))
        .collect();
    let mut mounts: Vec<_> = island
        .component_fragment_mounts
        .iter()
        .filter(|mount| reachable.contains(&(mount.root.0, mount.asset_id.as_str())))
        .collect();
    mounts.sort_by(|a, b| a.root.0.cmp(&b.root.0).then(a.asset_id.cmp(&b.asset_id)));

    for mount in mounts {
        // A single view-tree node can render many DOM roots when its component
        // expression sits inside a server-side `for` loop. Every instance
        // carries the same mount marker, so mount the complete selector result
        // rather than hydrating only the first matching child.
        out.push('\n');
        let root_var = component_fragment_root_var(mount.root);
        let roots_var = component_fragment_roots_var(mount.root);
        let _ = write!(
            out,
            "    const {roots_var} = root.querySelectorAll('[{marker}=\"",
            marker = MARKER_COMPONENT_MOUNT
        );
        let _ = write!(out, "{}", mount.root.0);
        out.push_str("\"]');\n");

        let _ = writeln!(out, "    for (const {root_var} of {roots_var}) {{");

        let restore_callbacks_var = format!("__rsjs_restore_callbacks_{}", mount.root.0);
        let previous_callbacks_var = format!("__rsjs_previous_callbacks_{}", mount.root.0);
        let had_callbacks_var = format!("__rsjs_had_callbacks_{}", mount.root.0);
        let callbacks_restored_var = format!("__rsjs_callbacks_restored_{}", mount.root.0);

        let _ = writeln!(out, "        let {restore_callbacks_var} = () => {{}};");
        out.push_str("        try {\n");

        // The callback table belongs to this exact mount occurrence. Always
        // replace it, including with `{}`, so a Some -> None rehydrate cannot
        // inherit a stale callback from an earlier parent instance. Register
        // restoration before the fallible child mount and make it idempotent:
        // the catch path restores immediately, while normal owner teardown
        // restores after the child fragment disposer.
        let _ = writeln!(out, "        if ({root_var} != null) {{");
        let _ = writeln!(
            out,
            "            const {had_callbacks_var} = Object.prototype.hasOwnProperty.call({root_var}, \"__rsjs_callbacks\");"
        );
        let _ = writeln!(
            out,
            "            const {previous_callbacks_var} = {root_var}.__rsjs_callbacks;"
        );
        let _ = writeln!(out, "            let {callbacks_restored_var} = false;");
        let _ = writeln!(out, "            {restore_callbacks_var} = () => {{");
        let _ = writeln!(out, "                if ({callbacks_restored_var}) return;");
        let _ = writeln!(out, "                {callbacks_restored_var} = true;");
        let _ = writeln!(
            out,
            "                if ({had_callbacks_var}) {root_var}.__rsjs_callbacks = {previous_callbacks_var};"
        );
        let _ = writeln!(
            out,
            "                else delete {root_var}.__rsjs_callbacks;"
        );
        out.push_str("            };\n");
        let _ = writeln!(
            out,
            "            __rsjs_disposers.push({restore_callbacks_var});"
        );
        let _ = writeln!(out, "            {root_var}.__rsjs_callbacks = {{");
        for callback in &mount.callbacks {
            let _ = writeln!(
                out,
                "            {prop}: (__payload) => {{",
                prop = callback_slot_prop(callback.slot)
            );
            emit_client_block_js_with_context_and_indent(
                out,
                callback.body_ir(),
                &ctx.client_emit_context(),
                4,
            )
            .expect("Client IR v3 should emit component callback adapter bodies");
            out.push_str("            },\n");
        }
        out.push_str("            };\n        }\n");

        let _ = write!(
            out,
            "        const {disposer} = {alias}",
            disposer = component_fragment_disposer_var(mount.root),
            alias = component_fragment_import_alias(&mount.asset_id)
        );
        out.push('(');
        out.push_str(&root_var);
        if !mount.signal_aliases.is_empty() {
            out.push_str(", { signals: { ");
            let mut aliases = mount.signal_aliases.clone();
            aliases.sort_by_key(|alias| alias.child.0);
            for (idx, alias) in aliases.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{}", alias.child.0);
                let _ = write!(out, ": {sig}", sig = signal_var(alias.parent));
            }
            out.push_str(" } }");
        }
        out.push_str(");\n");
        let disposer = component_fragment_disposer_var(mount.root);
        let _ = writeln!(out, "        if (typeof {disposer} === \"function\") {{");
        let _ = writeln!(out, "            __rsjs_disposers.push({disposer});");
        out.push_str("        }\n    } catch (__rsjs_fragment_error) {\n");
        let _ = writeln!(out, "        {restore_callbacks_var}();");
        let _ = writeln!(
            out,
            "        report_hydration_error(\"component fragment {asset}\", __rsjs_fragment_error);",
            asset = JsStr(&mount.asset_id)
        );
        out.push_str("        }\n    }\n");
    }
}

fn component_fragment_disposer_var(root: NodeId) -> String {
    format!("__rsjs_fragment_disposer_{}", root.0)
}

fn component_fragment_roots_var(root: NodeId) -> String {
    format!("__rsjs_component_roots_{}", root.0)
}

/// Emit `const __node_<id> = walk_path(...); bind_text(...)`
/// for each text binding, sorted by [`NodeId`] for determinism.
fn emit_text_bindings(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    let type_env = TypeEnv::for_island(island);
    let mut entries: Vec<(NodeId, ExprId)> = island
        .bindings
        .text_bindings
        .iter()
        .filter(|(node, _)| ctx.includes_node(**node))
        .map(|(node, expr)| (*node, *expr))
        .collect();
    entries.sort_by_key(|(node, _)| node.0);

    for (node, expr_id) in entries {
        out.push('\n');
        let deps = collect_reactive_deps(&island.exprs, expr_id);
        let rust_float_text = expression_uses_rust_float_text(island, &type_env, expr_id);
        if node_is_in_for_loop(&island.view_tree, node) {
            let node_name = node_var(node);
            let _ = write!(
                out,
                "    for (const {node_name} of root.querySelectorAll('[{marker}=\"",
                marker = MARKER_BIND
            );
            let value = format!("{}", node.0);
            let _ = writeln!(out, "{value}\"]')) {{", value = JsStr(&value));

            out.push_str("        ");
            emit_bind_text_call(
                out,
                island,
                expr_id,
                &deps,
                &node_name,
                Some(&node_name),
                rust_float_text,
                ctx,
            );
            out.push_str("    }\n");
        } else {
            emit_node_lookup(out, node, MARKER_BIND, declared, island, ctx);
            let node_name = node_var(node);
            out.push_str(INDENT);
            emit_bind_text_call(
                out,
                island,
                expr_id,
                &deps,
                &node_name,
                None,
                rust_float_text,
                ctx,
            );
        }
    }
}

/// Whether a text expression has an actual Rust f32/f64 value shape.
///
/// `ClientType::F64` alone is intentionally not enough evidence: a value can
/// acquire an f64-shaped representation through a generic/runtime path without
/// a statically proven Rust `Display` source. Walk the expression's value
/// sources to distinguish float-producing paths from Number-shaped values with
/// a different text-formatting contract.
fn expression_uses_rust_float_text(
    island: &Island,
    type_env: &TypeEnv<'_>,
    expr_id: ExprId,
) -> bool {
    expression_has_rust_float_source(
        island,
        type_env,
        island.exprs.client_expr(expr_id),
        &mut HashSet::new(),
    )
}

fn expression_has_rust_float_source(
    island: &Island,
    type_env: &TypeEnv<'_>,
    expr: &ClientExpr,
    visiting_signals: &mut HashSet<SignalId>,
) -> bool {
    if !matches!(type_env.type_of(expr), ClientType::F32 | ClientType::F64) {
        return false;
    }

    match expr {
        ClientExpr::Literal(Value::Float(_)) => true,
        ClientExpr::SignalRead(id) => {
            let Some(signal) = island.signals.get(id.0 as usize) else {
                return false;
            };
            if let Some(derived) = signal.derived {
                if !visiting_signals.insert(*id) {
                    return false;
                }
                let result = expression_has_rust_float_source(
                    island,
                    type_env,
                    island.exprs.client_expr(derived),
                    visiting_signals,
                );
                visiting_signals.remove(id);
                result
            } else {
                matches!(signal.ty, ClientType::F32 | ClientType::F64)
            }
        }
        ClientExpr::QueryRead {
            id,
            accessor: QueryAccessor::Data,
        } => matches!(
            type_env.query_data_type(*id),
            ClientType::F32 | ClientType::F64
        ),
        ClientExpr::QueryRead {
            accessor: QueryAccessor::DownloadPercent,
            ..
        } => true,
        ClientExpr::BrowserRead(_)
        | ClientExpr::EventRead(_)
        | ClientExpr::EventCollectionRead { .. }
        | ClientExpr::DomRead { .. }
        | ClientExpr::NodeRefRead { .. }
        | ClientExpr::Project { .. }
        | ClientExpr::AsType { .. } => true,
        ClientExpr::Unary { expr, .. } => {
            expression_has_rust_float_source(island, type_env, expr, visiting_signals)
        }
        ClientExpr::Binary { left, right, .. } => {
            expression_has_rust_float_source(island, type_env, left, visiting_signals)
                || expression_has_rust_float_source(island, type_env, right, visiting_signals)
        }
        ClientExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expression_has_rust_float_source(island, type_env, then_expr, visiting_signals)
                && expression_has_rust_float_source(island, type_env, else_expr, visiting_signals)
        }
        ClientExpr::Coalesce { target, fallback } => {
            matches!(fallback, Value::Float(_))
                && expression_has_rust_float_source(island, type_env, target, visiting_signals)
        }
        _ => false,
    }
}

fn emit_bind_text_call(
    out: &mut String,
    island: &Island,
    expr_id: ExprId,
    deps: &ReactiveDeps,
    node_name: &str,
    node_context: Option<&str>,
    rust_float_text: bool,
    ctx: &EmitContext,
) {
    let _ = write!(out, "__rsjs_disposers.push(bind_text({node_name}, () => ");
    emit_arena_expr_with_node(out, island, expr_id, node_context, ctx);
    out.push_str(", ");
    emit_dep_array(out, &deps.signals);
    let has_extra_handles = !deps.queries.is_empty() || !deps.mutations.is_empty();
    if has_extra_handles {
        out.push_str(", ");
        emit_handle_array(out, deps);
    }
    if rust_float_text {
        if !has_extra_handles {
            out.push_str(", []");
        }
        out.push_str(", \"rust-float\"");
    }
    out.push_str("));\n");
}

/// Assign author-declared node refs to their matched DOM elements.
fn emit_node_ref_bindings(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
    reachable_node_ref_ids: &[NodeRefId],
) {
    let mut entries: Vec<_> = island
        .bindings
        .node_ref_bindings
        .values()
        .filter(|binding| {
            ctx.includes_node(binding.node)
                && ctx.includes_node_ref(binding.node_ref)
                && reachable_node_ref_ids.contains(&binding.node_ref)
        })
        .collect();
    entries.sort_by_key(|binding| binding.node.0);

    for binding in entries {
        out.push('\n');
        emit_node_lookup(out, binding.node, MARKER_REF, declared, island, ctx);
        let _ = writeln!(
            out,
            "    {node_ref}.current = {node};",
            node_ref = node_ref_var(binding.node_ref),
            node = node_var(binding.node)
        );
    }
}

/// Emit event-binding wiring.
///
/// For non-submit events we use *event delegation*: one
/// `bind_delegated_events(root, "<event>", new Map([...]))` call per
/// distinct event-name, regardless of how many handlers share that
/// event-name on this island. The map keys are stringified `NodeId`
/// values matching the `data-rsjs-handler` attribute the SSR layer
/// emits, and the values are the per-handler functions defined in
/// `emit_handlers`. No `walk_path` lookup is emitted for delegated
/// handlers: dispatch happens at click-time via
/// `event.target.closest('[data-rsjs-handler]')`.
///
/// `submit` keeps its per-form `bind_submit(node, handler)` shape.
/// `bind_submit` calls `event.preventDefault()` before the handler
/// fires so the browser doesn't navigate, and the form-element
/// reference is needed to bind to. Forms are typically singletons in
/// an island, so the per-listener cost is not a concern.
///
/// Repeated submit forms and global-event owners use
/// `querySelectorAll('[data-rsjs-handler=...]')` and bind every rendered
/// instance. Non-repeated submit/global owners keep the faster static
/// `walk_path` lookup.
///
/// Output order is deterministic: delegated entries are emitted first
/// (sorted by event-name, then by NodeId within the map literal), then
/// document/window/submit bindings sorted by NodeId.
fn emit_event_bindings(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    if island.bindings.event_bindings.is_empty() {
        return;
    }

    // Bucket by event-name. For ordinary non-submit events we'll emit
    // one delegated registration per bucket; for submit and global
    // events we keep the per-node shape because the runtime needs a concrete
    // source element to resolve the registration target and payload owner.
    let mut delegated: std::collections::BTreeMap<&'static str, Vec<(NodeId, EventHandlerId)>> =
        std::collections::BTreeMap::new();
    let mut submits: Vec<(NodeId, EventHandlerId)> = Vec::new();
    let mut document_events: Vec<(NodeId, EventName, EventHandlerId)> = Vec::new();
    let mut window_events: Vec<(NodeId, EventName, EventHandlerId)> = Vec::new();

    for ((node, event), handler) in island.bindings.event_bindings.iter() {
        if !ctx.includes_node(*node) {
            continue;
        }
        if matches!(event, EventName::Submit) {
            submits.push((*node, *handler));
        } else if event.is_document_event() {
            document_events.push((*node, *event, *handler));
        } else if event.is_window_event() {
            window_events.push((*node, *event, *handler));
        } else {
            delegated
                .entry(event.as_dom_name())
                .or_default()
                .push((*node, *handler));
        }
    }

    // Stable textual sort within each bucket so the emitted Map
    // entries are deterministic regardless of HashMap iteration order.
    for entries in delegated.values_mut() {
        entries.sort_by_key(|(node, _)| node.0);
    }
    submits.sort_by_key(|(node, _)| node.0);
    document_events.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then(a.1.as_attr_name().cmp(b.1.as_attr_name()))
    });
    window_events.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then(a.1.as_attr_name().cmp(b.1.as_attr_name()))
    });

    // Delegated registrations: one `bind_delegated_events` call per
    // event-name. The Map is built inline; entries are
    // `["<NodeId>", __handler_<idx>]` pairs. We emit the Map literal
    // across multiple lines when the bucket has >1 entries so the
    // emitted JS stays diffable.
    for (event_name, entries) in &delegated {
        out.push('\n');
        let _ = write!(
            out,
            "    __rsjs_disposers.push(bind_delegated_events(root, \"{event_name}\", new Map(["
        );
        if entries.len() == 1 {
            let (node, handler) = entries[0];
            out.push_str("[\"");
            let _ = write!(out, "{}", node.0);
            let _ = writeln!(out, "\", {handler}]])));", handler = handler_var(handler));
        } else {
            out.push('\n');
            for (node, handler) in entries {
                out.push_str("        [\"");
                let _ = write!(out, "{}", node.0);
                let _ = writeln!(out, "\", {handler}],", handler = handler_var(*handler));
            }
            out.push_str("    ])));\n");
        }
    }

    // Document-level registrations: attach to the element's owner document,
    // expose that Document as the rsjs current target, and retain the source
    // element as the compiler-owned payload owner.
    // Loop-rendered owners share one marker value, so bind every
    // matching rendered instance.
    for (node, event, handler) in &document_events {
        emit_global_event_binding(
            out,
            "bind_document_event",
            *node,
            *event,
            *handler,
            declared,
            island,
            ctx,
        );
    }

    // Window-level registrations: attach to ownerDocument.defaultView, expose
    // that Window as the rsjs current target, and retain the source element as
    // the compiler-owned payload owner.
    // Loop-rendered owners share one marker value, so bind every
    // matching rendered instance.
    for (node, event, handler) in &window_events {
        emit_global_event_binding(
            out,
            "bind_window_event",
            *node,
            *event,
            *handler,
            declared,
            island,
            ctx,
        );
    }

    // Submit bindings: keep the per-form `bind_submit(node, handler)`
    // shape. Stable forms use `walk_path`; loop-rendered forms share a
    // marker value, so bind every matching form instance.
    for (node, handler) in submits {
        out.push('\n');
        let node_name = node_var(node);
        if node_is_in_for_loop(&island.view_tree, node) {
            let _ = write!(
                out,
                "    for (const {node_name} of root.querySelectorAll('[{marker}=\"",
                marker = MARKER_HANDLER
            );
            let value = format!("{}", node.0);
            let _ = writeln!(out, "{value}\"]')) {{", value = JsStr(&value));
            let _ = writeln!(
                out,
                "        __rsjs_disposers.push(bind_submit({node_name}, {handler}));",
                handler = handler_var(handler)
            );
            out.push_str("    }\n");
        } else {
            emit_node_lookup(out, node, MARKER_HANDLER, declared, island, ctx);
            let _ = writeln!(
                out,
                "    __rsjs_disposers.push(bind_submit({node_name}, {handler}));",
                handler = handler_var(handler)
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_global_event_binding(
    out: &mut String,
    bind_fn: &str,
    node: NodeId,
    event: EventName,
    handler: EventHandlerId,
    declared: &mut HashSet<NodeId>,
    island: &Island,
    ctx: &EmitContext,
) {
    let node_name = node_var(node);
    if node_is_in_for_loop(&island.view_tree, node) {
        out.push('\n');
        let _ = write!(
            out,
            "    for (const {node_name} of root.querySelectorAll('[{marker}=\"",
            marker = MARKER_HANDLER
        );
        let value = format!("{}", node.0);
        let _ = writeln!(out, "{value}\"]')) {{", value = JsStr(&value));
        let _ = write!(out, "        __rsjs_disposers.push({bind_fn}");
        out.push('(');
        let _ = writeln!(
            out,
            "{node_name}, \"{event}\", {handler}));",
            event = event.as_dom_name(),
            handler = handler_var(handler)
        );
        out.push_str("    }\n");
    } else {
        emit_node_lookup(out, node, MARKER_HANDLER, declared, island, ctx);
        let _ = write!(out, "    __rsjs_disposers.push({bind_fn}");
        out.push('(');
        let _ = writeln!(
            out,
            "{node_name}, \"{event}\", {handler}));",
            event = event.as_dom_name(),
            handler = handler_var(handler)
        );
    }
}

/// Emit a `bind_input(node, "<property>", signal)` call for each input
/// binding, sorted by [`NodeId`] for determinism. Wires a two-way
/// binding: writes to the input update the signal, signal updates
/// re-render the bound DOM property.
///
/// Repeated form controls use `querySelectorAll` against their shared
/// input marker and bind every rendered control instance to the same
/// signal. Non-repeated controls keep the faster static `walk_path`
/// lookup.
fn emit_input_bindings(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    let mut entries: Vec<(NodeId, &InputBinding)> = island
        .bindings
        .input_bindings
        .iter()
        .filter(|(node, _)| ctx.includes_node(**node))
        .map(|(node, ib)| (*node, ib))
        .collect();
    entries.sort_by_key(|(node, _)| node.0);

    for (node, binding) in entries {
        let marker = match binding.kind {
            InputKind::Value => MARKER_INPUT_VALUE,
            InputKind::Checked => MARKER_INPUT_CHECKED,
            InputKind::Selected => MARKER_INPUT_SELECTED,
        };
        out.push('\n');
        let node_name = node_var(node);
        if node_is_in_for_loop(&island.view_tree, node) {
            let _ = write!(
                out,
                "    for (const {node_name} of root.querySelectorAll('[{marker}=\""
            );
            let value = format!("{}", node.0);
            let _ = writeln!(out, "{value}\"]')) {{", value = JsStr(&value));
            out.push_str("        ");
            emit_bind_input_call(out, &node_name, binding, ctx);
            out.push_str("    }\n");
        } else {
            emit_node_lookup(out, node, marker, declared, island, ctx);
            out.push_str(INDENT);
            emit_bind_input_call(out, &node_name, binding, ctx);
        }
    }
}

fn emit_bind_input_call(
    out: &mut String,
    node_name: &str,
    binding: &InputBinding,
    ctx: &EmitContext,
) {
    let _ = write!(
        out,
        "__rsjs_disposers.push(bind_input({node_name}, \"{property}\", {signal}",
        property = binding.kind.as_dom_property(),
        signal = signal_var(binding.signal)
    );
    if ctx.root_mode == RootMode::FragmentRoot {
        out.push_str(", undefined, options.preserveDirtyControls !== false");
    }
    out.push_str("));\n");
}

/// Emit a `bind_attr(node, name, kind, getter, deps)` call for each
/// dynamic attribute binding, sorted by node then attribute name.
fn emit_attr_bindings(
    out: &mut String,
    island: &Island,
    declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    let mut entries: Vec<&AttrBinding> = island
        .bindings
        .attr_bindings
        .values()
        .filter(|binding| ctx.includes_node(binding.node))
        .collect();
    entries.sort_by(|a, b| a.node.0.cmp(&b.node.0).then(a.name.cmp(&b.name)));

    for binding in entries {
        out.push('\n');
        let deps = collect_reactive_deps(&island.exprs, binding.expr);
        if node_is_in_for_loop(&island.view_tree, binding.node) {
            let node_name = node_var(binding.node);
            let _ = write!(
                out,
                "    for (const {node_name} of root.querySelectorAll('[{marker}=\"",
                marker = MARKER_ATTR
            );
            let value = format!("{}", binding.node.0);
            let _ = writeln!(out, "{value}\"]')) {{", value = JsStr(&value));

            out.push_str("        ");
            emit_bind_attr_call(
                out,
                island,
                binding,
                &deps,
                &node_name,
                Some(&node_name),
                ctx,
            );
            out.push_str("    }\n");
        } else {
            emit_node_lookup(out, binding.node, MARKER_ATTR, declared, island, ctx);
            let node_name = node_var(binding.node);
            out.push_str(INDENT);
            emit_bind_attr_call(out, island, binding, &deps, &node_name, None, ctx);
        }
    }
}

fn emit_bind_attr_call(
    out: &mut String,
    island: &Island,
    binding: &AttrBinding,
    deps: &ReactiveDeps,
    node_name: &str,
    node_context: Option<&str>,
    ctx: &EmitContext,
) {
    let _ = write!(
        out,
        "__rsjs_disposers.push(bind_attr({node_name}, \"{name}\", \"{kind}\", () => ",
        name = JsStr(&binding.name),
        kind = binding.kind.as_runtime_kind()
    );
    emit_arena_expr_with_node(out, island, binding.expr, node_context, ctx);
    out.push_str(", ");
    emit_dep_array(out, &deps.signals);
    if !deps.queries.is_empty() || !deps.mutations.is_empty() {
        out.push_str(", ");
        emit_handle_array(out, deps);
    }
    out.push_str("));\n");
}

/// Emit a `bind_conditional(...)` call for each conditional section,
/// sorted by parent [`NodeId`] for determinism.
///
/// Output shape (one section, parent N, predicate reading signal S,
/// then-children nodes \[T1, T2\], else-children nodes \[E1\]):
///
/// ```js
/// const __cond_node_N_then_0 = walk_path(root, [0, ...], "data-rsjs-cond-then", "N:0");
/// const __cond_node_N_then_1 = walk_path(root, [0, ...], "data-rsjs-cond-then", "N:1");
/// const __cond_node_N_else_0 = walk_path(root, [0, ...], "data-rsjs-cond-else", "N:0");
/// bind_conditional(
///     () => (__sig_S.get() > 5),
///     [__sig_S],
///     [__cond_node_N_then_0, __cond_node_N_then_1],
///     [__cond_node_N_else_0],
/// );
/// ```
///
/// The runtime sees the resolved DOM nodes plus the predicate getter
/// and dep list; it does not need to know the parent's id. The
/// per-branch index (the `:0` / `:1` suffix) keeps the markers
/// unambiguous when a branch contains multiple top-level nodes.
fn emit_conditional_sections(
    out: &mut String,
    island: &Island,
    _declared: &mut HashSet<NodeId>,
    ctx: &EmitContext,
) {
    let mut entries: Vec<(NodeId, &[ConditionalSection])> = island
        .bindings
        .conditional_sections
        .iter()
        .filter(|(parent, _)| ctx.includes_node(**parent))
        .map(|(parent, sections)| (*parent, sections.as_slice()))
        .collect();
    entries.sort_by_key(|(parent, _)| parent.0);

    for (parent, sections) in entries {
        let mut then_offset = 0usize;
        let mut else_offset = 0usize;
        for section in sections {
            out.push('\n');

            let then_start = then_offset;
            let else_start = else_offset;

            if node_is_in_for_loop(&island.view_tree, parent) {
                emit_repeated_conditional_section(
                    out, island, parent, section, then_start, else_start, ctx,
                );
                then_offset += section.then_children.len();
                else_offset += section.else_children.len();
                continue;
            }

            // Resolve every branch node via the index walker. Use a
            // stable, descriptive local name so the emitted JS is
            // greppable.
            for (idx, child) in section.then_children.iter().enumerate() {
                emit_branch_node_lookup(out, parent, *child, "then", then_start + idx, island, ctx);
            }
            for (idx, child) in section.else_children.iter().enumerate() {
                emit_branch_node_lookup(out, parent, *child, "else", else_start + idx, island, ctx);
            }

            let deps = collect_reactive_deps(&island.exprs, section.condition);

            out.push_str("    __rsjs_disposers.push(bind_conditional(\n        () => ");
            emit_arena_expr(out, island, section.condition, ctx);
            out.push_str(",\n        ");
            emit_dep_array(out, &deps.signals);
            out.push_str(",\n        ");
            emit_node_list(out, parent, then_start, section.then_children.len(), "then");
            out.push_str(",\n        ");
            emit_node_list(out, parent, else_start, section.else_children.len(), "else");
            out.push_str(",\n");

            // Round-3 review #4: surface query/mutation handles too so the
            // conditional re-evaluates when those state slots change.
            // Position-wise the runtime sees this as the optional fifth
            // argument; absent bindings keep the four-arg form.
            if !deps.queries.is_empty() || !deps.mutations.is_empty() {
                out.push_str("        ");
                emit_handle_array(out, &deps);
                out.push_str(",\n");
            }

            out.push_str("    ));\n");

            then_offset += section.then_children.len();
            else_offset += section.else_children.len();
        }
    }
}

// proa-perf-ok(scope): trusted low-level JavaScript source emitter, not SSR HTML rendering
fn emit_repeated_conditional_section(
    out: &mut String,
    island: &Island,
    parent: NodeId,
    section: &ConditionalSection,
    then_start: usize,
    else_start: usize,
    ctx: &EmitContext,
) {
    for (idx, _) in section.then_children.iter().enumerate() {
        emit_repeated_branch_node_lookup(out, parent, "then", then_start + idx);
    }
    for (idx, _) in section.else_children.iter().enumerate() {
        emit_repeated_branch_node_lookup(out, parent, "else", else_start + idx);
    }

    let key = repeated_conditional_key(parent, then_start, else_start);
    let count_var = format!("__cond_count_{key}");
    let index_var = format!("__cond_i_{key}");
    let context_var = format!("__cond_ctx_{key}");
    let deps = collect_reactive_deps(&island.exprs, section.condition);

    let _ = write!(out, "    const {count_var} = Math.max(0");
    emit_repeated_branch_lengths(out, parent, then_start, section.then_children.len(), "then");
    emit_repeated_branch_lengths(out, parent, else_start, section.else_children.len(), "else");
    out.push_str(");\n");
    let _ = writeln!(
        out,
        "    for (let {index_var} = 0; {index_var} < {count_var}; {index_var}++) {{"
    );
    let _ = write!(out, "        const {context_var} = ");
    emit_repeated_conditional_context(
        out,
        parent,
        then_start,
        section.then_children.len(),
        else_start,
        section.else_children.len(),
        &index_var,
    );
    out.push_str(";\n        __rsjs_disposers.push(bind_conditional(\n            () => ");
    emit_arena_expr_with_node(out, island, section.condition, Some(&context_var), ctx);
    out.push_str(",\n            ");
    emit_dep_array(out, &deps.signals);
    out.push_str(",\n            ");
    emit_repeated_node_list(
        out,
        parent,
        then_start,
        section.then_children.len(),
        "then",
        &index_var,
    );
    out.push_str(",\n            ");
    emit_repeated_node_list(
        out,
        parent,
        else_start,
        section.else_children.len(),
        "else",
        &index_var,
    );
    out.push_str(",\n");

    if !deps.queries.is_empty() || !deps.mutations.is_empty() {
        out.push_str("            ");
        emit_handle_array(out, &deps);
        out.push_str(",\n");
    }

    out.push_str("        ));\n    }\n");
}

fn emit_repeated_branch_node_lookup(out: &mut String, parent: NodeId, branch: &str, idx: usize) {
    let attr = format!("data-rsjs-cond-{branch}");
    let value = format!("{}:{idx}", parent.0);
    let _ = writeln!(
        out,
        "    const {v} = Array.from(root.querySelectorAll('[{attr}=\"{value}\"]'));",
        v = cond_branch_list_var(parent, branch, idx),
        attr = JsStr(&attr),
        value = JsStr(&value),
    );
}

fn emit_repeated_branch_lengths(
    out: &mut String,
    parent: NodeId,
    start: usize,
    len: usize,
    branch: &str,
) {
    for i in 0..len {
        let _ = write!(
            out,
            ", {v}.length",
            v = cond_branch_list_var(parent, branch, start + i)
        );
    }
}

fn emit_repeated_conditional_context(
    out: &mut String,
    parent: NodeId,
    then_start: usize,
    then_len: usize,
    else_start: usize,
    else_len: usize,
    index_var: &str,
) {
    let mut emitted = false;
    out.push('(');
    for i in 0..then_len {
        if emitted {
            out.push_str(" ?? ");
        }
        out.push_str(&cond_branch_list_var(parent, "then", then_start + i));
        out.push('[');
        out.push_str(index_var);
        out.push(']');
        emitted = true;
    }
    for i in 0..else_len {
        if emitted {
            out.push_str(" ?? ");
        }
        out.push_str(&cond_branch_list_var(parent, "else", else_start + i));
        out.push('[');
        out.push_str(index_var);
        out.push(']');
        emitted = true;
    }
    if !emitted {
        out.push_str("null");
    }
    out.push_str(")?.parentElement ?? null");
}

fn emit_repeated_node_list(
    out: &mut String,
    parent: NodeId,
    start: usize,
    len: usize,
    branch: &str,
    index_var: &str,
) {
    out.push('[');
    for i in 0..len {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&cond_branch_list_var(parent, branch, start + i));
        out.push('[');
        out.push_str(index_var);
        out.push(']');
    }
    out.push(']');
}

/// Emit `[__rsjs_query_<id>, ..., __rsjs_mutation_<id>, ...]` for the
/// query/mutation handles a binding observes. Order: queries first
/// (sorted by id), then mutations (sorted by id), matching
/// [`ReactiveDeps`]'s first-seen ordering on the input but
/// deterministic across the union.
fn emit_handle_array(out: &mut String, deps: &ReactiveDeps) {
    out.push('[');
    let mut first = true;
    for q in &deps.queries {
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push_str(&query_var(*q));
    }
    for m in &deps.mutations {
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push_str(&mutation_var(*m));
    }
    out.push(']');
}

/// Emit a single `const __cond_node_<parent>_<branch>_<idx> = walk_path(root, ...);`
/// line for one branch element of a conditional section.
///
/// Resolution uses the static index path to the child element (an element child
/// of `parent`); the runtime verifies the expected
/// `data-rsjs-cond-<branch>` marker and falls back to an owner-scoped marker
/// lookup on mismatch.
fn emit_branch_node_lookup(
    out: &mut String,
    parent: NodeId,
    child: NodeId,
    branch: &str,
    idx: usize,
    island: &Island,
    ctx: &EmitContext,
) {
    let attr = format!("data-rsjs-cond-{branch}");
    let value = format!("{}:{idx}", parent.0);
    let _ = write!(
        out,
        "    const {v} = ",
        v = cond_branch_var(parent, branch, idx)
    );
    emit_walk_call(out, &island.view_tree, child, &attr, &value, ctx.root_mode);
    out.push_str(";\n");
}

/// Emit `[a, b, c]` for one branch's resolved-node locals, or `[]` if
/// the branch is empty.
fn emit_node_list(out: &mut String, parent: NodeId, start: usize, len: usize, branch: &str) {
    out.push('[');
    for i in 0..len {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&cond_branch_var(parent, branch, start + i));
    }
    out.push(']');
}

/// Local name for the resolved DOM node of a single conditional
/// branch element.
fn cond_branch_var(parent: NodeId, branch: &str, idx: usize) -> String {
    format!("__cond_node_{}_{}_{}", parent.0, branch, idx)
}

fn cond_branch_list_var(parent: NodeId, branch: &str, idx: usize) -> String {
    format!("__cond_nodes_{}_{}_{}", parent.0, branch, idx)
}

fn repeated_conditional_key(parent: NodeId, then_start: usize, else_start: usize) -> String {
    format!("{}_{}_{}", parent.0, then_start, else_start)
}

/// Emit `const __node_<id> = walk_path(root, [<index path>], "<attr>", "<id>");`
/// at most once per [`NodeId`].
///
/// Round-3 review #6: a node with multiple bindings (e.g. an
/// element with both an event handler and a text binding, or a
/// node with two events) routes through this helper from each
/// binding kind. Without dedup the same `const __node_<id>` line
/// emits twice and the JS engine rejects the redeclared `const`.
/// Track declared NodeIds in `declared` and skip subsequent uses;
/// the first emission wins (its `<attr>` selector is whichever
/// binding kind ran first, but querySelector's match semantics
/// don't depend on which marker attribute was queried as long as
/// the element actually carries it). emit_rust ensures every
/// marker attribute on a node is present, so any selector resolves
/// to the same DOM node.
fn emit_node_lookup(
    out: &mut String,
    node: NodeId,
    attr: &str,
    declared: &mut HashSet<NodeId>,
    island: &Island,
    ctx: &EmitContext,
) {
    if !declared.insert(node) {
        return;
    }
    let value = format!("{}", node.0);
    let _ = write!(out, "    const {v} = ", v = node_var(node));
    emit_walk_call(out, &island.view_tree, node, attr, &value, ctx.root_mode);
    out.push_str(";\n");
}

/// Emit a `walk_path(root, [path], "marker_attr", "marker_value")`
/// expression. Falls back to a root-inclusive selector lookup
/// (`root.matches(...) ? root : root.querySelector(...)`) when the IR
/// view tree has no statically walkable path to `node` (for-loop
/// trees, or a logic bug surfaced as a stable lookup rather than a
/// missing const).
fn emit_walk_call(
    out: &mut String,
    view_tree: &ViewTreeRef,
    node: NodeId,
    marker_attr: &str,
    marker_value: &str,
    root_mode: RootMode,
) {
    match compute_index_path(view_tree, node) {
        Some(path) => {
            out.push_str("walk_path(root, ");
            out.push_str(&format_index_path(&path, root_mode));
            out.push_str(", \"");
            out.push_str(marker_attr);
            out.push_str("\", \"");
            out.push_str(&js_string_inner(marker_value));
            out.push_str("\")");
        }
        None => {
            // No statically walkable path from root to `node`: trees
            // containing a for-loop bail out of index paths entirely
            // (child indices shift as list items re-render), and IR
            // construction bugs land here too. The fallback must
            // consider `root` itself before searching below it: a
            // component fragment receives its own marked root element
            // as `root`, and `querySelector` matches descendants only,
            // so a plain lookup can never resolve the fragment root.
            // For island modules `root` is the `<rsjs-island>` wrapper,
            // which never carries binding markers, so the `matches`
            // arm is false and behavior is unchanged.
            let _ = write!(
                out,
                "(root.matches('[{attr}=\"{value}\"]') ? root : root.querySelector('[{attr}=\"{value}\"]'))",
                attr = marker_attr,
                value = JsStr(marker_value),
            );
        }
    }
}

/// Whether the island contains any binding kind that needs to resolve
/// a DOM node reference. Used to decide whether to import `walk_path`
/// from the runtime.
///
/// Ordinary non-submit event bindings are excluded: as of OPTIMIZATIONS
/// item #2 they route through `bind_delegated_events`, which dispatches
/// via `event.target.closest('[data-rsjs-handler]')` and does not need a
/// `walk_path` lookup. Repeated input and submit bindings are also
/// excluded because they resolve via `querySelectorAll`. Stable form
/// bindings and document/window-level events still need the lookup
/// because their runtime helpers expose a concrete source element as
/// the rsjs current target.
fn island_needs_node_walk(
    island: &Island,
    ctx: &EmitContext,
    reachable_node_ref_ids: &[NodeRefId],
) -> bool {
    let needs_for_events = island.bindings.event_bindings.keys().any(|(node, ev)| {
        ctx.includes_node(*node)
            && ((matches!(ev, EventName::Submit) && !node_is_in_for_loop(&island.view_tree, *node))
                || ((ev.is_document_event() || ev.is_window_event())
                    && !node_is_in_for_loop(&island.view_tree, *node)))
    });
    island
        .bindings
        .text_bindings
        .keys()
        .any(|node| ctx.includes_node(*node))
        || needs_for_events
        || island
            .bindings
            .input_bindings
            .keys()
            .any(|node| ctx.includes_node(*node) && !node_is_in_for_loop(&island.view_tree, *node))
        || island
            .bindings
            .attr_bindings
            .values()
            .any(|binding| ctx.includes_node(binding.node))
        || island.bindings.node_ref_bindings.values().any(|binding| {
            ctx.includes_node(binding.node)
                && ctx.includes_node_ref(binding.node_ref)
                && reachable_node_ref_ids.contains(&binding.node_ref)
        })
        || island
            .bindings
            .conditional_sections
            .keys()
            .any(|node| ctx.includes_node(*node) && !node_is_in_for_loop(&island.view_tree, *node))
        || island
            .component_fragment_mounts
            .iter()
            .any(|mount| mount.requires_parent_runtime())
}

/// Emit a JS literal for a [`Value`].
fn emit_value(out: &mut String, value: &Value) {
    match value {
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        // BigInt literal: keeps exact integer semantics past 2^53.
        Value::BigInt(n) => {
            let _ = write!(out, "{n}n");
        }
        Value::Float(f) => emit_float(out, *f),
        Value::String(s) => {
            out.push('"');
            out.push_str(&js_string_inner(s));
            out.push('"');
        }
        Value::Null => out.push_str("null"),
    }
}

/// Emit a finite or non-finite f64 in a way that round-trips through
/// JS. NaN and infinities use the global identifiers because JS has
/// no float literal for them.
fn emit_float(out: &mut String, f: f64) {
    if f.is_nan() {
        out.push_str("NaN");
    } else if f.is_infinite() {
        out.push_str(if f.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        });
    } else {
        // Rust's default {} formatting for f64 produces a numeric
        // literal that JS parses identically; force a decimal point
        // so integers-as-floats stay distinguishable to a human
        // reader (e.g. `1.0` rather than `1`).
        let s = format!("{f}");
        out.push_str(&s);
        if !s.contains('.') && !s.contains('e') && !s.contains('E') {
            out.push_str(".0");
        }
    }
}

/// Emit an arena expression as a JS sub-expression.
///
/// Template binding getters emit the Client IR v3 mirror, so the binding
/// path exercises the same typed expression backend as handlers and helper
/// modules.
fn emit_arena_expr(out: &mut String, island: &Island, expr_id: ExprId, ctx: &EmitContext) {
    emit_arena_expr_with_node(out, island, expr_id, None, ctx);
}

fn emit_request_config_expr(out: &mut String, island: &Island, expr_id: ExprId, ctx: &EmitContext) {
    if matches!(
        island.exprs.client_expr(expr_id),
        ClientExpr::Literal(Value::String(_))
    ) {
        emit_arena_expr(out, island, expr_id, ctx);
    } else {
        out.push_str("() => ");
        emit_arena_expr(out, island, expr_id, ctx);
    }
}

fn emit_query_key_expr(out: &mut String, island: &Island, expr_id: ExprId, ctx: &EmitContext) {
    match island.exprs.client_expr(expr_id) {
        ClientExpr::QueryKey(_) => emit_arena_expr(out, island, expr_id, ctx),
        _ => {
            out.push('[');
            emit_arena_expr(out, island, expr_id, ctx);
            out.push(']');
        }
    }
}

/// Emit an arena expression with an optional repeated-node context.
fn emit_arena_expr_with_node(
    out: &mut String,
    island: &Island,
    expr_id: ExprId,
    node_context: Option<&str>,
    emit_ctx: &EmitContext,
) {
    let mut client_ctx = emit_ctx.client_emit_context();
    client_ctx.node_name = node_context.map(str::to_string);
    emit_client_expr_js_with_context(out, island.exprs.client_expr(expr_id), &client_ctx)
        .expect("arena Client IR v3 expression should emit");
}

/// Generated identifier for a query handle local.
pub(crate) fn query_var(id: crate::types::QueryId) -> String {
    format!("__rsjs_query_{}", id.0)
}

fn query_registration_var(id: QueryId) -> String {
    format!("__rsjs_query_registration_{}", id.0)
}

fn query_key_watcher_var(id: QueryId) -> String {
    format!("__rsjs_query_key_watcher_{}", id.0)
}

/// Generated identifier for a mutation handle local.
pub(crate) fn mutation_var(id: crate::types::MutationId) -> String {
    format!("__rsjs_mutation_{}", id.0)
}

/// True iff `s` is a syntactically valid (ASCII) JavaScript
/// identifier: starts with `[A-Za-z_$]` and continues with
/// `[A-Za-z0-9_$]*`. Used as a debug-time sanity check before
/// embedding `ClientExpr::Call` callee names directly into JS source.
fn is_valid_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Compute the JS-side export name for a client-module `ClientExpr::Call`'s callee.
///
/// `<module>` is the user-written bundle path (`crate::pricing`,
/// `self::pricing`, `other_crate::shared`). `<name>` is the helper
/// function name. The return value joins the two with `__` after
/// replacing every `::` in the module path with `__`. This keeps the
/// emitted JS uniquely-named even when two bundles export
/// helpers with the same local name (e.g. `pricing__subtotal` vs
/// `tax__subtotal`).
///
/// # Contract (round-3 review #10)
///
/// Both `module` (last segment after `::`) and `name` are embedded
/// directly into JS source as identifiers. Callers must ensure they
/// were produced by `rsjs_macros::client_attr` /
/// `client_module_attr`, which validate that helper / module names
/// are non-raw Rust identifiers. A debug assertion here catches a
/// hand-built IR (or a future regression) that violates the
/// contract; release builds skip the check for zero overhead.
pub fn bundle_export_name(module: &str, name: &str) -> String {
    // Use only the *last segment* of the user-written import path
    // as the bundle's identifier. The `#[rsjs::client_module]` macro
    // knows only the local module name (e.g. `pricing`); without
    // last-segment-only resolution, legacy descriptors spelling the same
    // bundle as `self::pricing` and `crate::pricing` would disagree on the
    // bundle filename and the function ident,
    // breaking the import. The collision risk (two different bundles
    // both ending in `pricing`) is the consuming user's problem to
    // resolve via aliasing.
    let last = module.rsplit("::").next().unwrap_or(module);
    let combined = format!("{last}__{name}");
    debug_assert!(
        is_valid_js_ident(&combined),
        "ClientExpr::Call produced an invalid JS identifier `{combined}`; \
         the macro layer should have rejected non-ident module / helper names"
    );
    combined
}

fn client_module_namespace(module: &str) -> String {
    format!("__rsjs_client_module_{}", module.replace("::", "__"))
}

fn client_module_asset_name(module: &str) -> &str {
    module.rsplit("::").next().unwrap_or(module)
}

/// Stable exported mount function name for a component fragment module.
pub fn component_fragment_export_name(asset_id: &str) -> String {
    let mut out = String::from("mount_");
    for ch in asset_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

fn component_fragment_import_alias(asset_id: &str) -> String {
    let mut out = String::from("__rsjs_mount_");
    for ch in asset_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

fn component_fragment_root_var(root: NodeId) -> String {
    format!("__rsjs_component_root_{}", root.0)
}

/// Emit the dep list as a JS array of signal references, in the
/// order [`collect_deps`] returned them.
fn emit_dep_array(out: &mut String, deps: &[SignalId]) {
    out.push('[');
    for (i, id) in deps.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&signal_var(*id));
    }
    out.push(']');
}

/// Generated identifier for a signal.
fn signal_var(id: SignalId) -> String {
    format!("__sig_{}", id.0)
}

fn node_ref_var(id: NodeRefId) -> String {
    format!("__ref_{}", id.0)
}

/// Generated identifier for a handler.
fn handler_var(id: EventHandlerId) -> String {
    format!("__handler_{}", id.0)
}

/// Generated property name for a callback slot.
fn callback_slot_prop(id: CallbackSlotId) -> String {
    format!("__callback_{}", id.0)
}

/// Generated identifier for a DOM-resolved node.
fn node_var(id: NodeId) -> String {
    format!("__node_{}", id.0)
}

/// Escape `s` so it can be embedded inside a JS double-quoted string
/// literal. The caller is responsible for the surrounding quotes.
///
/// Public so future emitters / tests can produce identical escaping.
pub fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    out.push_str(&js_string_inner(s));
    out.push('"');
    out
}

/// Zero-allocation `Display` adapter that streams the same escapes as
/// [`js_string_inner`] directly into the formatter, so `write!` call
/// sites can interpolate an escaped value without building an
/// intermediate `String`. This is the emitter-side analogue of the
/// `text!` intrinsic's direct-to-buffer writes in the HTML macros.
pub struct JsStr<'a>(pub &'a str);

impl std::fmt::Display for JsStr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for c in self.0.chars() {
            match c {
                '\\' => f.write_str("\\\\")?,
                '"' => f.write_str("\\\"")?,
                '\n' => f.write_str("\\n")?,
                '\r' => f.write_str("\\r")?,
                '\t' => f.write_str("\\t")?,
                '\u{08}' => f.write_str("\\b")?,
                '\u{0c}' => f.write_str("\\f")?,
                // U+2028 / U+2029 must be escaped because JS parsers
                // treat them as line terminators inside string literals
                // in pre-ES2019 contexts; safe to emit unconditionally.
                '\u{2028}' => f.write_str("\\u2028")?,
                '\u{2029}' => f.write_str("\\u2029")?,
                c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

/// Escape `s` for embedding in a JS double-quoted string. Returns the
/// escaped contents WITHOUT surrounding quotes.
fn js_string_inner(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let _ = write!(out, "{}", JsStr(s));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        ClientBinaryOp, ClientBlock, ClientExpr, ClientFunctionRef, ClientNodeRefEffect,
        ClientParam, ClientStmt,
    };

    fn linked_test_key(byte: u8) -> crate::LinkedHelperKey {
        crate::LinkedHelperKey::from_canonical_bytes(vec![byte]).unwrap()
    }

    fn linked_test_call(edge: u32) -> ClientExpr {
        ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(edge),
                authored_path: "calendar::__client_cell_serial".to_owned(),
                name: "__client_cell_serial".to_owned(),
            },
            vec![
                ClientExpr::int(202_608),
                ClientExpr::bool(edge.is_multiple_of(2)),
                ClientExpr::int(edge as i64),
            ],
        )
    }

    fn calendar_shaped_helper_island() -> Island {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let body = ClientBlock::from_stmts(
            (0..14)
                .map(|edge| ClientStmt::Expr(linked_test_call(edge)))
                .collect::<Vec<_>>(),
        );
        let handler = builder.handler_from_ir(body);
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        builder.build()
    }

    fn calendar_shaped_helper_program(extra_edge: Option<u32>) -> LinkedHelperProgram {
        let key = linked_test_key(1);
        let mut roots = (0..14)
            .map(|edge| {
                crate::LinkedHelperCallBinding::new(HelperCallEdgeId::new(edge), key.clone())
            })
            .collect::<Vec<_>>();
        if let Some(edge) = extra_edge {
            roots.push(crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(edge),
                key.clone(),
            ));
        }
        LinkedHelperProgram::new(
            roots,
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![
                    ClientParam::new("view_ym", ClientType::U32),
                    ClientParam::new("sunday_start", ClientType::Bool),
                    ClientParam::new("index", ClientType::U32),
                ],
                ClientType::U32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::binary(
                    ClientBinaryOp::Checked {
                        op: crate::CheckedIntegerOp::Add,
                        domain: crate::IntegerDomain::U32,
                    },
                    ClientExpr::local(crate::ClientLocalId(0)),
                    ClientExpr::local(crate::ClientLocalId(2)),
                )))]),
                vec![],
            )],
        )
        .unwrap()
    }

    fn deterministic_helper_island() -> Island {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let calls: Vec<_> = (0..2)
            .map(|edge| {
                ClientStmt::Expr(ClientExpr::call(
                    ClientFunctionRef::Helper {
                        edge: HelperCallEdgeId::new(edge),
                        authored_path: format!("crate::helper_{edge}"),
                        name: format!("helper_{edge}"),
                    },
                    vec![],
                ))
            })
            .collect();
        let handler = builder.handler_from_ir(ClientBlock::from_stmts(calls));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        builder.build()
    }

    fn deterministic_helper_program(reverse: bool) -> LinkedHelperProgram {
        let first = linked_test_key(1);
        let second = linked_test_key(2);
        let mut roots = vec![
            crate::LinkedHelperCallBinding::new(HelperCallEdgeId::new(0), first.clone()),
            crate::LinkedHelperCallBinding::new(HelperCallEdgeId::new(1), second.clone()),
        ];
        let mut definitions = vec![
            crate::LinkedHelperDefinition::new(
                first,
                vec![],
                ClientType::I32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(1)))]),
                vec![],
            ),
            crate::LinkedHelperDefinition::new(
                second,
                vec![],
                ClientType::I32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(2)))]),
                vec![],
            ),
        ];
        if reverse {
            roots.reverse();
            definitions.reverse();
        }
        LinkedHelperProgram::new(roots, definitions).unwrap()
    }

    #[test]
    fn js_string_escapes_quotes_and_backslashes() {
        assert_eq!(js_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn js_string_escapes_control_chars() {
        // Input: newline, tab, carriage return, then U+0001 (a non-printable
        // control char that is not one of the named escapes).
        let input = "\n\t\r\x01";
        let expected = "\"\\n\\t\\r\\u0001\"";
        assert_eq!(js_string(input), expected);
    }

    #[test]
    fn emit_returns_empty_for_server_only_island() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        b.server_only_node(root, "StaticPanel");
        b.set_root(root);
        let island = b.build();

        assert_eq!(emit(&island, "StaticPanel"), "");
        assert_eq!(emit_fragment(&island, "staticpanel_1234"), "");
    }

    #[test]
    fn linked_static_fragment_is_a_runtime_free_noop_module() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        b.server_only_node(root, "StaticPanel");
        b.set_root(root);
        let island = b.build();

        assert_eq!(
            emit_fragment_with_linked_exports(&island, "staticpanel_1234", &[], &[], &[]),
            "export function mount_staticpanel_1234(_root, _options = {}) {}\n"
        );
    }

    #[test]
    fn final_link_emits_calendar_shaped_helper_once_and_resolves_all_edges() {
        let island = calendar_shaped_helper_island();
        let descriptor = emit(&island, "calendar");
        assert_eq!(
            descriptor.matches("__rsjs_unlinked_helper_edge_").count(),
            14
        );

        let linked =
            try_emit_linked(&island, "calendar", &calendar_shaped_helper_program(None)).unwrap();

        assert_eq!(linked.matches("function __rsjs_helper_0(").count(), 1);
        assert_eq!(linked.matches("__rsjs_helper_0(").count(), 15);
        assert!(!linked.contains("__rsjs_unlinked_"));
        assert!(linked.starts_with("import { "));
        assert!(linked.lines().next().unwrap().contains("checked_int_add"));
        assert!(
            linked.contains("function __rsjs_helper_0(__rsjs_arg_0, __rsjs_arg_1, __rsjs_arg_2)")
        );
        assert!(linked.contains("checked_int_add(__rsjs_arg_0, __rsjs_arg_2, \"u32\")"));
    }

    #[test]
    fn final_linked_javascript_is_independent_of_program_input_order() {
        let island = deterministic_helper_island();
        let forward = try_emit_linked(
            &island,
            "deterministic_helpers",
            &deterministic_helper_program(false),
        )
        .unwrap();
        let reversed = try_emit_linked(
            &island,
            "deterministic_helpers",
            &deterministic_helper_program(true),
        )
        .unwrap();

        assert_eq!(forward, reversed);
        assert_eq!(forward.matches("function __rsjs_helper_").count(), 2);
    }

    #[test]
    fn final_link_rejects_reserved_runtime_function_reference() {
        use crate::builder::IslandBuilder;

        let reference = format!(
            "globalThis.{prefix}helper_edge_0",
            prefix = crate::UNLINKED_HELPER_IDENTIFIER_PREFIX
        );
        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let handler = builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::call(ClientFunctionRef::Runtime(reference.clone()), vec![]),
        )]));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();
        let program = LinkedHelperProgram::new(vec![], vec![]).unwrap();

        let error = try_emit_linked(&island, "forged_runtime", &program).unwrap_err();
        assert_eq!(
            error,
            LinkedHelperEmitError::Client(ClientEmitError::ReservedUnlinkedFunctionRef(reference))
        );
    }

    #[test]
    fn final_link_rejects_missing_and_extra_root_bindings() {
        let island = calendar_shaped_helper_island();
        let key = linked_test_key(1);
        let missing = LinkedHelperProgram::new(
            (0..13)
                .map(|edge| {
                    crate::LinkedHelperCallBinding::new(HelperCallEdgeId::new(edge), key.clone())
                })
                .collect(),
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![ClientParam::new("index", ClientType::I32)],
                ClientType::I32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                    crate::ClientLocalId(0),
                )))]),
                vec![],
            )],
        )
        .unwrap();
        assert!(matches!(
            try_emit_linked(&island, "calendar", &missing),
            Err(LinkedHelperEmitError::MissingRootBinding { edge })
                if edge == HelperCallEdgeId::new(13)
        ));

        let extra = calendar_shaped_helper_program(Some(99));
        assert!(matches!(
            try_emit_linked(&island, "calendar", &extra),
            Err(LinkedHelperEmitError::ExtraRootBinding { edge })
                if edge == HelperCallEdgeId::new(99)
        ));

        let key = linked_test_key(2);
        let wrong_arity = LinkedHelperProgram::new(
            (0..14)
                .map(|edge| {
                    crate::LinkedHelperCallBinding::new(HelperCallEdgeId::new(edge), key.clone())
                })
                .collect(),
            vec![crate::LinkedHelperDefinition::new(
                key.clone(),
                vec![
                    ClientParam::new("left", ClientType::I32),
                    ClientParam::new("right", ClientType::I32),
                ],
                ClientType::I32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                    crate::ClientLocalId(0),
                )))]),
                vec![],
            )],
        )
        .unwrap();
        assert!(matches!(
            try_emit_linked(&island, "calendar", &wrong_arity),
            Err(LinkedHelperEmitError::RootCallArityMismatch {
                edge,
                target,
                expected: 2,
                actual: 3,
            }) if edge == HelperCallEdgeId::new(0) && target == key
        ));
    }

    #[test]
    fn final_link_rejects_wrong_root_helper_argument_type() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(0),
                authored_path: "calendar::__client_cell_serial".to_owned(),
                name: "__client_cell_serial".to_owned(),
            },
            vec![
                ClientExpr::string("not-a-year-month"),
                ClientExpr::bool(false),
                ClientExpr::int(4),
            ],
        );
        let handler =
            builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(call)]));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();

        let key = linked_test_key(4);
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                key.clone(),
            )],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![
                    ClientParam::new("view_ym", ClientType::U32),
                    ClientParam::new("sunday_start", ClientType::Bool),
                    ClientParam::new("index", ClientType::U32),
                ],
                ClientType::U32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                    crate::ClientLocalId(0),
                )))]),
                vec![],
            )],
        )
        .unwrap();
        let error = try_emit_linked(&island, "wrong_argument", &program).unwrap_err();
        assert!(matches!(
            error,
            LinkedHelperEmitError::InvalidRootType(source)
                if matches!(
                    source.as_ref(),
                    crate::analysis::ClientHelperBodyTypeError::TypeMismatch {
                        site: crate::analysis::ClientHelperTypeSite::HelperArgument { edge, index: 0 },
                        expected: ClientType::U32,
                        actual: crate::analysis::ClientHelperInferredType::Exact(ClientType::String),
                    } if *edge == HelperCallEdgeId::new(0)
                )
        ));
    }

    #[test]
    fn final_link_specializes_wide_root_literals_through_option_and_vec() {
        use crate::builder::IslandBuilder;

        let edge = HelperCallEdgeId::new(0);
        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge,
                authored_path: "crate::consume_wide_literals".to_owned(),
                name: "consume_wide_literals".to_owned(),
            },
            vec![
                ClientExpr::int(-7),
                ClientExpr::int(8),
                ClientExpr::ArrayLit(vec![Value::Int(9), Value::Null, Value::Int(10)]),
                ClientExpr::int(11),
            ],
        );
        let handler =
            builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(call)]));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();

        let key = linked_test_key(31);
        let nested_wide = ClientType::option(ClientType::vec(ClientType::option(ClientType::I64)));
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(edge, key.clone())],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![
                    ClientParam::new("signed", ClientType::I64),
                    ClientParam::new("unsigned", ClientType::U64),
                    ClientParam::new("nested", nested_wide),
                    ClientParam::new("narrow", ClientType::I32),
                ],
                ClientType::Unit,
                ClientBlock::from_stmts(vec![ClientStmt::Return(None)]),
                vec![],
            )],
        )
        .unwrap();

        let js = try_emit_linked(&island, "wide_root_literals", &program).unwrap();
        assert!(
            js.contains("__rsjs_helper_0(-7n, 8n, [9n, null, 10n], 11)"),
            "the resolved parameter signature must specialize every contextual wide literal:\n{js}"
        );
        assert!(!js.contains("__rsjs_unlinked_"));
    }

    #[test]
    fn final_link_rejects_negative_contextual_u64_literal_before_emission() {
        use crate::builder::IslandBuilder;

        let edge = HelperCallEdgeId::new(0);
        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        let call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge,
                authored_path: "crate::consume_u64".to_owned(),
                name: "consume_u64".to_owned(),
            },
            vec![ClientExpr::int(-1)],
        );
        let handler =
            builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(call)]));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);

        let key = linked_test_key(34);
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(edge, key.clone())],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![ClientParam::new("value", ClientType::U64)],
                ClientType::Unit,
                ClientBlock::from_stmts(vec![ClientStmt::Return(None)]),
                vec![],
            )],
        )
        .unwrap();

        let error = try_emit_linked(&builder.build(), "negative_u64", &program).unwrap_err();
        assert!(matches!(
            error,
            LinkedHelperEmitError::InvalidRootType(source)
                if matches!(
                    source.as_ref(),
                    crate::analysis::ClientHelperBodyTypeError::TypeMismatch {
                        site: crate::analysis::ClientHelperTypeSite::HelperArgument {
                            edge: actual_edge,
                            index: 0,
                        },
                        expected: ClientType::U64,
                        actual: crate::analysis::ClientHelperInferredType::NumberIntegerLiteral {
                            min: -1,
                            max: -1,
                        },
                    } if *actual_edge == edge
                )
        ));
    }

    #[test]
    fn final_link_specializes_wide_literals_in_transitive_helper_calls() {
        use crate::builder::IslandBuilder;

        let target = linked_test_key(32);
        let caller = linked_test_key(33);
        let nested_edge = HelperCallEdgeId::new(0);
        let nested_call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: nested_edge,
                authored_path: "crate::wide_identity".to_owned(),
                name: "wide_identity".to_owned(),
            },
            vec![ClientExpr::int(41)],
        );
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                caller.clone(),
            )],
            vec![
                crate::LinkedHelperDefinition::new(
                    target.clone(),
                    vec![ClientParam::new("value", ClientType::I64)],
                    ClientType::I64,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                        crate::ClientLocalId(0),
                    )))]),
                    vec![],
                ),
                crate::LinkedHelperDefinition::new(
                    caller,
                    vec![],
                    ClientType::I64,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(nested_call))]),
                    vec![crate::LinkedHelperCallBinding::new(nested_edge, target)],
                ),
            ],
        )
        .unwrap();

        let mut builder = IslandBuilder::new();
        let root = builder.element("output", None);
        let expression = builder.expr(ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(0),
                authored_path: "crate::caller".to_owned(),
                name: "caller".to_owned(),
            },
            vec![],
        ));
        builder.bind_text(root, expression);
        builder.set_root(root);

        let js = try_emit_linked(&builder.build(), "wide_nested_call", &program).unwrap();
        assert!(
            js.contains("return __rsjs_helper_0(41n);"),
            "helper-to-helper calls need the same signature-directed specialization as roots:\n{js}"
        );
    }

    #[test]
    fn final_link_requires_a_durable_domain_proof_for_node_payload_arguments() {
        use crate::builder::IslandBuilder;
        use crate::AttrBindingKind;

        fn payload_island(index: ClientExpr) -> Island {
            let mut builder = IslandBuilder::new();
            let root = builder.element("div", None);
            let repeated = builder.for_loop(root, "", "", "idx", "0_u32..42_u32", "");
            let button = builder.element("button", Some(repeated));
            builder.payload_attr(button, "data-rsjs-payload-p0", "idx");
            let expression = builder.expr(ClientExpr::call(
                ClientFunctionRef::Helper {
                    edge: HelperCallEdgeId::new(0),
                    authored_path: "calendar::__client_cell_serial".to_owned(),
                    name: "__client_cell_serial".to_owned(),
                },
                vec![ClientExpr::int(202_608), ClientExpr::bool(false), index],
            ));
            builder.bind_attr(button, "data-index", AttrBindingKind::Value, expression);
            builder.set_root(root);
            builder.build()
        }

        let key = linked_test_key(6);
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                key.clone(),
            )],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![
                    ClientParam::new("view_ym", ClientType::U32),
                    ClientParam::new("sunday_start", ClientType::Bool),
                    ClientParam::new("index", ClientType::U32),
                ],
                ClientType::U32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                    crate::ClientLocalId(2),
                )))]),
                vec![],
            )],
        )
        .unwrap();

        let payload = || ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p0".to_owned(),
            tokens: "idx".to_owned(),
        };
        let error = try_emit_linked(&payload_island(payload()), "untyped_payload", &program)
            .expect_err("JSON transport alone must not prove a u32 source value");
        assert!(matches!(
            error,
            LinkedHelperEmitError::InvalidRootType(source)
                if matches!(
                    source.as_ref(),
                    crate::analysis::ClientHelperBodyTypeError::TypeMismatch {
                        site: crate::analysis::ClientHelperTypeSite::HelperArgument { edge, index: 2 },
                        expected: ClientType::U32,
                        actual: crate::analysis::ClientHelperInferredType::Exact(ClientType::Unknown),
                    } if *edge == HelperCallEdgeId::new(0)
                )
        ));

        let checked_payload = ClientExpr::binary(
            ClientBinaryOp::Checked {
                op: crate::CheckedIntegerOp::Add,
                domain: crate::IntegerDomain::U32,
            },
            payload(),
            ClientExpr::int(0),
        );
        let js = try_emit_linked(&payload_island(checked_payload), "typed_payload", &program)
            .expect("the checked identity is a durable exact-domain proof");
        assert!(
            js.contains("checked_int_add(") && js.contains("JSON.parse("),
            "typed payload lost its checked identity during emission:\n{js}"
        );
        assert!(!js.contains("__rsjs_unlinked_"));
    }

    #[test]
    fn final_link_rejects_helper_return_in_wrong_root_context() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let selected = builder.signal_typed(Value::Int(0), ClientType::U32);
        let root = builder.element("button", None);
        let call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(0),
                authored_path: "calendar::label".to_owned(),
                name: "label".to_owned(),
            },
            vec![ClientExpr::int(4)],
        );
        let handler =
            builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::SignalSet {
                id: selected,
                value: call,
            }]));
        builder.bind_event(root, EventName::Click, handler);
        builder.set_root(root);
        let island = builder.build();

        let key = linked_test_key(5);
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                key.clone(),
            )],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![ClientParam::new("index", ClientType::U32)],
                ClientType::String,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string("day")))]),
                vec![],
            )],
        )
        .unwrap();

        let error = try_emit_linked(&island, "wrong_result", &program).unwrap_err();
        assert!(matches!(
            error,
            LinkedHelperEmitError::InvalidRootType(source)
                if matches!(
                    source.as_ref(),
                    crate::analysis::ClientHelperBodyTypeError::TypeMismatch {
                        site: crate::analysis::ClientHelperTypeSite::SignalSet(id),
                        expected: ClientType::U32,
                        actual: crate::analysis::ClientHelperInferredType::Exact(ClientType::String),
                    } if *id == selected
                )
        ));
    }

    #[test]
    fn final_link_deduplicates_identical_root_edge_observations() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let root = builder.element("div", None);
        let first = builder.element("span", Some(root));
        let second = builder.element("span", Some(root));
        let expression = builder.expr(ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(0),
                authored_path: "crate::label".to_owned(),
                name: "label".to_owned(),
            },
            vec![],
        ));
        builder.bind_text(first, expression);
        builder.bind_text(second, expression);
        builder.set_root(root);
        let island = builder.build();

        let key = linked_test_key(3);
        let program = LinkedHelperProgram::new(
            vec![crate::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                key.clone(),
            )],
            vec![crate::LinkedHelperDefinition::new(
                key,
                vec![],
                ClientType::String,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string(
                    "label",
                )))]),
                vec![],
            )],
        )
        .unwrap();

        let js = try_emit_linked(&island, "shared_expr", &program).unwrap();
        // One declaration plus the two consumers of the same arena ExprId.
        assert_eq!(js.matches("__rsjs_helper_0()").count(), 3);
        assert!(!js.contains("__rsjs_unlinked_"));
    }

    #[test]
    fn linked_fragment_exports_forced_signals_and_payload_handlers() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let local_only = b.signal(Value::Int(1));
        let root = b.element("button", None);
        let local_text = b.expr(ClientExpr::SignalRead(local_only));
        b.text_node(root, local_text);
        b.set_root(root);
        let island = b.build();
        let fragment = emit_fragment_with_linked_exports(
            &island,
            "linked_counter",
            &[count],
            &[FragmentPayloadHandlerExport {
                producer_id: 9,
                payload_arity: 1,
                body: ClientBlock::from_stmts(vec![ClientStmt::SignalSet {
                    id: count,
                    value: ClientExpr::CallbackPayload,
                }]),
            }],
            &[],
        );

        assert!(fragment.contains(
            "const __sig_0 = __rsjs_signal_overrides[0] ?? \
             (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0) ? \
             signal(__rsjs_signal_seeds[0]) : signal(signal_initial("
        ));
        assert!(fragment.contains("options.expose.signals[0] = __sig_0;"));
        assert!(!fragment.contains("options.expose.signals[1]"));
        assert!(fragment.contains("options.expose.payloadHandlers[9] = (__payload) => {"));
        assert!(fragment.contains("__sig_0.set(__payload);"));
    }

    #[test]
    fn linked_fragment_payload_handler_emits_browser_reads_and_writes_directly() {
        use crate::builder::IslandBuilder;
        use crate::{ClientBrowserEffect, ClientBrowserRead, LocationField};

        let mut b = IslandBuilder::new();
        let root = b.element("button", None);
        b.set_root(root);
        let island = b.build();
        let fragment = emit_fragment_with_linked_exports(
            &island,
            "browser_callback",
            &[],
            &[FragmentPayloadHandlerExport {
                producer_id: 4,
                payload_arity: 0,
                body: ClientBlock::from_stmts(vec![
                    ClientStmt::Expr(ClientExpr::BrowserRead(ClientBrowserRead::Location(
                        LocationField::Pathname,
                    ))),
                    ClientStmt::BrowserEffect(ClientBrowserEffect::WindowPrint),
                ]),
            }],
            &[],
        );

        assert!(fragment.contains("options.expose.payloadHandlers[4] = (__payload) => {"));
        assert!(fragment.contains("window.location.pathname"));
        assert!(fragment.contains("window.print?.();"));
    }

    #[test]
    fn fragment_keeps_dependency_free_bindings_as_browser_work() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let root = builder.element("span", None);
        let constant = builder.expr(ClientExpr::string("client constant"));
        builder.bind_text(root, constant);
        builder.set_root(root);

        let fragment = emit_fragment(&builder.build(), "constant_binding");
        assert!(
            fragment.contains("export function mount_constant_binding(root, options = {})")
                && fragment.contains("bind_text("),
            "a dependency-free binding is still executable browser work:\n{fragment}"
        );
    }

    #[test]
    fn fragment_keeps_dom_only_node_ref_bindings_reachable() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let node_ref = builder.node_ref();
        let root = builder.element("input", None);
        builder.bind_node_ref(root, node_ref);
        builder.set_root(root);

        let fragment = emit_fragment(&builder.build(), "dom_only_node_ref");
        assert!(
            fragment.contains("const __ref_0 = { current: null };")
                && fragment.contains("__ref_0.current = __node_"),
            "a DOM binding is itself sufficient to retain and bind its node-ref object:\n{fragment}"
        );
    }

    #[test]
    fn linked_payload_roots_supply_query_and_numeric_runtime_imports() {
        use crate::builder::IslandBuilder;
        use crate::JsonPointer;

        let mut builder = IslandBuilder::new();
        let root = builder.element("button", None);
        builder.set_root(root);
        let payload = FragmentPayloadHandlerExport {
            producer_id: 7,
            payload_arity: 0,
            body: ClientBlock::from_stmts(vec![
                ClientStmt::Expr(ClientExpr::Project {
                    target: Box::new(ClientExpr::Literal(Value::Null)),
                    path: JsonPointer::new("/value"),
                }),
                ClientStmt::Expr(ClientExpr::binary(
                    ClientBinaryOp::Checked {
                        op: crate::CheckedIntegerOp::Add,
                        domain: crate::IntegerDomain::I32,
                    },
                    ClientExpr::int(1),
                    ClientExpr::int(2),
                )),
            ]),
        };

        let fragment = emit_fragment_with_linked_exports(
            &builder.build(),
            "forced_runtime_imports",
            &[],
            &[payload],
            &[],
        );
        assert!(
            fragment.contains("checked_int_add")
                && fragment.contains("from \"/rsjs-runtime.js\"")
                && fragment.contains("from \"/rsjs-runtime-query.js\""),
            "forced payload code must bring every runtime symbol it emits into scope:\n{fragment}"
        );
    }

    #[test]
    fn final_fragment_resolver_observes_forced_derived_helper_closure_once() {
        use crate::builder::IslandBuilder;

        let mut builder = IslandBuilder::new();
        let leaf = builder.signal(Value::Int(4));
        let edge = HelperCallEdgeId::new(11);
        let derived_body = builder.expr(ClientExpr::call(
            ClientFunctionRef::Helper {
                edge,
                authored_path: "crate::latent_serial".to_owned(),
                name: "latent_serial".to_owned(),
            },
            vec![ClientExpr::SignalRead(leaf)],
        ));
        let derived = builder.memo(derived_body);
        let root = builder.element("section", None);
        builder.set_root(root);
        let island = builder.build();

        let key = linked_test_key(9);
        let helper = crate::LinkedHelperDefinition::new(
            key.clone(),
            vec![ClientParam::new("value", ClientType::I32)],
            ClientType::I32,
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                crate::ClientLocalId(0),
            )))]),
            vec![],
        );
        let mut resolver_calls = 0;
        let fragment = try_emit_fragment_resolving_helpers_and_node_refs(
            &island,
            "latent_helper",
            &[derived],
            &[],
            &[],
            &[],
            &[],
            |exact_edges| {
                resolver_calls += 1;
                assert_eq!(exact_edges, &[edge]);
                LinkedHelperProgram::new(
                    vec![crate::LinkedHelperCallBinding::new(edge, key)],
                    vec![helper],
                )
                .map_err(|error| error.to_string())
            },
        )
        .unwrap();

        assert_eq!(resolver_calls, 1);
        assert!(
            fragment.contains("const __sig_0 = __rsjs_signal_overrides[0]"),
            "{fragment}"
        );
        assert!(fragment.contains("const __sig_1 = memo("), "{fragment}");
        assert!(fragment.contains("function __rsjs_helper_0("), "{fragment}");
        assert!(!fragment.contains("__rsjs_unlinked_"), "{fragment}");
    }

    #[test]
    fn linked_fragment_node_refs_alias_exact_objects_without_fallback_allocation() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let node_ref = b.node_ref();
        let input = b.element("input", None);
        b.bind_node_ref(input, node_ref);
        b.set_root(input);
        let island = b.build();

        let fragment = emit_fragment_with_linked_node_refs(
            &island,
            "linked_input_ref",
            &[],
            &[],
            &[],
            &[node_ref],
            &[node_ref],
        );

        assert!(fragment.contains("const __ref_0 = options.nodeRefs?.[0];"));
        assert!(fragment.contains(
            "if (__ref_0 == null || typeof __ref_0 !== \"object\" || !(\"current\" in __ref_0))"
        ));
        assert!(fragment.contains("options.expose.nodeRefs[0] = __ref_0;"));
        assert!(
            !fragment.contains("const __ref_0 = { current: null };"),
            "an imported handle must never silently allocate a second ref object"
        );
        let cleanup = fragment
            .find("__rsjs_disposers.push(() => { __ref_0.current = null; });")
            .expect("the shared ref must be cleared during reverse-order owner teardown");
        let binding = fragment
            .find("__ref_0.current = __node_")
            .expect("the child fragment must bind its local DOM target");
        assert!(
            cleanup < binding,
            "cleanup registration must precede binding so partial initialization also clears the ref"
        );
    }

    #[test]
    fn linked_fragment_exports_exact_conditional_lifecycle_control() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let visible = b.signal(Value::Bool(false));
        let predicate = b.expr(ClientExpr::SignalRead(visible));
        let root = b.element("section", None);
        b.set_root(root);
        let island = b.build();

        let fragment = emit_fragment_with_linked_exports(
            &island,
            "conditional_parent",
            &[visible],
            &[],
            &[FragmentLifecycleControlExport {
                control_id: 3,
                producer_expression_id: predicate,
            }],
        );

        assert!(fragment.contains("options.expose?.lifecycleControls != null"));
        assert!(fragment.contains("options.expose.lifecycleControls[3] = {"));
        assert!(fragment.contains("get: () => !!(__sig_0.get()),"));
        assert!(fragment.contains("deps: [__sig_0],"));
        assert!(fragment.contains("handles: [],"));
    }

    #[test]
    fn linked_fragment_input_binding_distinguishes_first_mount_from_remount() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let value = b.signal(Value::String("server".to_owned()));
        let root = b.element("input", None);
        b.bind_input(root, InputKind::Value, value);
        b.set_root(root);
        let island = b.build();

        let fragment = emit_fragment_with_linked_exports(&island, "input_child", &[], &[], &[]);
        assert!(fragment
            .contains(", \"value\", __sig_0, undefined, options.preserveDirtyControls !== false)"));

        let standalone = emit(&island, "input_child");
        assert!(!standalone.contains("preserveDirtyControls"));
    }

    #[test]
    fn emit_returns_empty_for_unreachable_client_declarations() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let dead = b.signal(Value::String("secret".to_string()));
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            dead,
            ClientExpr::string("still-dead"),
        )]));
        let root = b.element("section", None);
        b.set_root(root);
        let island = b.build();

        assert_eq!(emit(&island, "DeadDeclarations"), "");
        assert_eq!(emit_fragment(&island, "deaddeclarations_1234"), "");
    }

    #[test]
    fn fragment_imports_and_mounts_use_reachable_fragment_edges() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        let first = b.element("div", Some(root));
        let second = b.element("div", Some(root));
        b.component_fragment_mount(
            first,
            "child_a".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.component_fragment_mount(
            second,
            "child_b".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        b.set_root(root);
        let island = b.build();
        let reachable = vec![(first, "child_a".to_string())];

        let imports = component_fragment_import_lines(&reachable);
        assert!(imports.contains("mount_child_a"));
        assert!(!imports.contains("child_b"));

        let mut mounts = String::new();
        emit_component_fragment_mounts(
            &mut mounts,
            &island,
            &EmitContext::island(&island),
            &reachable,
        );
        assert!(mounts.contains("__rsjs_mount_child_a("));
        assert!(mounts.contains(
            "const __rsjs_component_roots_1 = root.querySelectorAll('[data-rsjs-component-mount=\"1\"]');"
        ));
        assert!(mounts.contains("for (const __rsjs_component_root_1 of __rsjs_component_roots_1)"));
        assert!(!mounts.contains("__rsjs_mount_child_b("));
        assert!(!mounts.contains("data-rsjs-component-mount=\"2\""));
    }

    #[test]
    fn emit_value_handles_each_variant() {
        for (v, expected) in [
            (Value::Bool(true), "true"),
            (Value::Bool(false), "false"),
            (Value::Int(42), "42"),
            (Value::Int(-7), "-7"),
            (Value::Null, "null"),
        ] {
            let mut out = String::new();
            emit_value(&mut out, &v);
            assert_eq!(out, expected, "mismatch for {v:?}");
        }
    }

    #[test]
    fn emit_value_formats_floats_with_decimal_point() {
        let mut out = String::new();
        emit_value(&mut out, &Value::Float(1.0));
        assert_eq!(out, "1.0");

        out.clear();
        emit_value(&mut out, &Value::Float(2.5));
        assert_eq!(out, "2.5");
    }

    #[test]
    fn emit_marks_only_proven_float_text_for_rust_infinity_spelling() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let float = b.signal_typed(Value::Float(0.0), ClientType::F64);
        let integer = b.signal(Value::Int(0));
        let float_read = b.expr(ClientExpr::SignalRead(float));
        let integer_read = b.expr(ClientExpr::SignalRead(integer));
        let root = b.element("div", None);
        let float_node = b.element("span", Some(root));
        let integer_node = b.element("span", Some(root));
        b.bind_text(float_node, float_read);
        b.bind_text(integer_node, integer_read);
        b.set_root(root);

        let js = emit(&b.build(), "FloatText");

        assert!(
            js.contains("bind_text(__node_1, () => __sig_0.get(), [__sig_0], [], \"rust-float\")"),
            "proven f64 text should request Rust non-finite spelling; got:\n{js}"
        );
        assert!(
            js.contains("bind_text(__node_2, () => __sig_1.get(), [__sig_1])"),
            "integer text should retain generic stringification; got:\n{js}"
        );
    }

    #[test]
    fn emit_recursively_decodes_integer_leaves_in_compound_signal_seeds() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let maybe = b.signal_typed(Value::Null, ClientType::option(ClientType::I64));
        let read = b.expr(ClientExpr::SignalRead(maybe));
        let root = b.element("div", None);
        let node = b.element("span", Some(root));
        b.bind_text(node, read);
        b.set_root(root);

        let js = emit(&b.build(), "OptionalWide");

        assert!(
            js.contains("signal_initial_typed"),
            "compound fixed-width seed should import its typed decoder; got:\n{js}"
        );
        assert!(
            js.contains(
                "signal(signal_initial_typed(__rsjs_signal_initials, 0, null, {\"kind\":\"option\",\"inner\":{\"kind\":\"i64\"}}))"
            ),
            "Option<i64> should carry its recursive seed schema; got:\n{js}"
        );
        assert!(
            !js.contains("signal_initial(__rsjs_signal_initials, 0, null)"),
            "typed compound seeds must not bypass recursive decoding; got:\n{js}"
        );
    }

    #[test]
    fn emit_lowers_string_transforms_and_predicates() {
        use crate::client::emit_client_expr_js;
        use crate::client::{StringPredicateOp, StringTransformOp};

        let out = emit_client_expr_js(&ClientExpr::StringTransform {
            op: StringTransformOp::ToUppercase,
            target: Box::new(ClientExpr::SignalRead(SignalId(0))),
        })
        .unwrap();
        assert_eq!(out, "(String((__sig_0.get()) ?? \"\").toUpperCase())");

        let out = emit_client_expr_js(&ClientExpr::StringTransform {
            op: StringTransformOp::Trim,
            target: Box::new(ClientExpr::SignalRead(SignalId(0))),
        })
        .unwrap();
        assert_eq!(
            out,
            "(String((__sig_0.get()) ?? \"\").replace(/^[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+|[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+$/g, \"\"))"
        );

        let out = emit_client_expr_js(&ClientExpr::StringPredicate {
            op: StringPredicateOp::StartsWith,
            haystack: Box::new(ClientExpr::SignalRead(SignalId(0))),
            needle: Box::new(ClientExpr::Literal(Value::String("ab".to_string()))),
        })
        .unwrap();
        assert_eq!(
            out,
            "(String((__sig_0.get()) ?? \"\").startsWith(String((\"ab\") ?? \"\")))"
        );

        let out = emit_client_expr_js(&ClientExpr::StringPredicate {
            op: StringPredicateOp::EndsWith,
            haystack: Box::new(ClientExpr::SignalRead(SignalId(0))),
            needle: Box::new(ClientExpr::Literal(Value::String("yz".to_string()))),
        })
        .unwrap();
        assert_eq!(
            out,
            "(String((__sig_0.get()) ?? \"\").endsWith(String((\"yz\") ?? \"\")))"
        );
    }

    #[test]
    fn emit_lowers_mutations_and_run_mutation_handler_statements() {
        use crate::builder::IslandBuilder;
        use crate::reactive::{InvalidationStrategy, MutationOptions};
        let mut b = IslandBuilder::new();
        let cart = b.signal(Value::String("[]".to_string()));
        let mkey = b.expr(ClientExpr::Literal(Value::String(
            "update_cart".to_string(),
        )));
        let mserver = b.expr(ClientExpr::Literal(Value::Null));
        let mclient = b.expr(ClientExpr::Literal(Value::String("/api/cart".to_string())));
        let inv = b.expr(ClientExpr::Literal(Value::String("cart".to_string())));
        let mid = b.mutation(
            mkey,
            mserver,
            mclient,
            vec![inv],
            MutationOptions {
                retry_count: 2,
                on_success_invalidate_strategy: InvalidationStrategy::Refetch,
            },
        );
        let h = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
            mutation: mid,
            args: vec![ClientExpr::SignalRead(cart)],
        }]));
        let root = b.element("div", None);
        let btn = b.element("button", Some(root));
        b.bind_event(btn, crate::bindings::EventName::Click, h);
        b.set_root(root);
        let island = b.build();
        let js = emit(&island, "UpdateCart");

        // Imports register_mutation (and crucially NOT register_query,
        // since this island has no queries).
        assert!(
            js.contains("import { rsjs, register_mutation }"),
            "missing register_mutation import; got:\n{js}"
        );
        // Registration call shape.
        assert!(
            js.contains("const __rsjs_mutation_0 = register_mutation({"),
            "missing mutation 0 registration; got:\n{js}"
        );
        assert!(
            js.contains(r#"key: ["update_cart"]"#),
            "missing key shape; got:\n{js}"
        );
        assert!(
            js.contains("root,"),
            "missing root context for mutation metadata lookup; got:\n{js}"
        );
        assert!(
            js.contains(r#"runner: "/api/cart""#),
            "missing runner shape; got:\n{js}"
        );
        assert!(
            js.contains(r#"invalidates: [["cart"]]"#),
            "missing invalidates shape; got:\n{js}"
        );
        assert!(
            js.contains("retry_count: 2"),
            "missing retry policy; got:\n{js}"
        );
        assert!(
            js.contains(r#"on_success_invalidate_strategy: "refetch""#),
            "missing strategy; got:\n{js}"
        );
        // Handler body lowers RunMutation with a single arg to
        // `handle.run(arg)` (single-arg runtime contract per Round-4
        // review #2; previously emitted `handle.run([arg])`).
        assert!(
            js.contains("__rsjs_mutation_0.run(__sig_0.get())"),
            "missing RunMutation lowering; got:\n{js}"
        );
    }

    #[test]
    fn emit_skips_unreachable_query_and_mutation_declarations() {
        use crate::bindings::EventName;
        use crate::builder::IslandBuilder;
        use crate::reactive::{MutationOptions, QueryOptions};

        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::Literal(Value::String("dead".to_string())));
        let url = b.expr(ClientExpr::Literal(Value::String("/api/dead".to_string())));
        b.query(key, key, url, QueryOptions::default());
        b.mutation(key, url, url, vec![key], MutationOptions::default());
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(Vec::new()));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "DeadDeclarations");

        assert!(
            js.contains("bind_delegated_events"),
            "event binding should still hydrate; got:\n{js}"
        );
        assert!(
            !js.contains("register_query")
                && !js.contains("register_mutation")
                && !js.contains("__rsjs_query_0")
                && !js.contains("__rsjs_mutation_0")
                && !js.contains("/rsjs-runtime-query.js"),
            "unreachable query/mutation declarations must not ship JS runtime state; got:\n{js}"
        );
    }

    #[test]
    fn emit_imports_only_reachable_client_modules() {
        use crate::builder::IslandBuilder;
        use crate::client::ClientFunctionRef;

        let mut b = IslandBuilder::new();
        b.add_client_module_import("crate::money".to_string());
        b.add_client_module_import("crate::dates".to_string());
        let formatted = b.expr(ClientExpr::call(
            ClientFunctionRef::Import {
                module: "crate::money".to_string(),
                name: "format_usd".to_string(),
            },
            vec![ClientExpr::int(42)],
        ));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, formatted);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "MoneyLabel");

        assert!(
            js.contains("./money.js")
                && js.contains("__rsjs_client_module_crate__money.money__format_usd"),
            "reachable client module call should import and call money bundle; got:\n{js}"
        );
        assert!(
            !js.contains("./dates.js") && !js.contains("__rsjs_client_module_crate__dates"),
            "unused client module declaration must not ship; got:\n{js}"
        );
    }

    #[test]
    fn emit_skips_unbound_handler_declarations() {
        use crate::bindings::EventName;
        use crate::builder::IslandBuilder;
        use crate::client::ClientFunctionRef;

        let mut b = IslandBuilder::new();
        b.add_client_module_import("crate::money".to_string());
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let bound = b.handler_from_ir(ClientBlock::from_stmts(Vec::new()));
        let unused = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::call(
                ClientFunctionRef::Import {
                    module: "crate::money".to_string(),
                    name: "format_usd".to_string(),
                },
                vec![ClientExpr::int(42)],
            ),
        )]));
        b.bind_event(button, EventName::Click, bound);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "DeadHandler");

        assert!(
            js.contains("function __handler_0"),
            "bound handler should be emitted; got:\n{js}"
        );
        assert!(
            !js.contains(&format!("function __handler_{}", unused.0))
                && !js.contains("format_usd")
                && !js.contains("./money.js"),
            "unused handler body and client module must not ship; got:\n{js}"
        );
    }

    #[test]
    fn emit_skips_unreachable_node_ref_declarations() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let node_ref = b.node_ref();
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
            id: node_ref,
            effect: ClientNodeRefEffect::Focus,
        }]));
        let live = b.signal(Value::Int(7));
        let live_read = b.expr(ClientExpr::signal_read(live));
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, live_read);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "DeadRef");

        assert!(
            js.contains("__sig_0.get()"),
            "live text binding should still hydrate; got:\n{js}"
        );
        assert!(
            !js.contains("__ref_0"),
            "unreachable node-ref handle/effect must not ship; got:\n{js}"
        );
    }

    #[test]
    fn emit_keeps_node_refs_from_bound_handlers() {
        use crate::bindings::EventName;
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let node_ref = b.node_ref();
        let root = b.element("div", None);
        let button = b.element("button", Some(root));
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
            id: node_ref,
            effect: ClientNodeRefEffect::Focus,
        }]));
        b.bind_event(button, EventName::Click, handler);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "LiveRef");

        assert!(
            js.contains("const __ref_0 = { current: null };"),
            "reachable node-ref handle should be declared; got:\n{js}"
        );
        assert!(
            js.contains("__rsjs_disposers.push(() => { __ref_0.current = null; });"),
            "every reachable node-ref handle needs owner-scoped cleanup; got:\n{js}"
        );
        assert!(
            js.contains("__ref_0.current?.focus?.();"),
            "bound handler should keep node-ref effect; got:\n{js}"
        );
    }

    #[test]
    fn emit_registers_node_ref_cleanup_before_fallible_dom_binding() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        let first_ref = b.node_ref();
        let second_ref = b.node_ref();
        let root = b.element("section", None);
        let first_node = b.element("input", Some(root));
        let second_node = b.element("button", Some(root));
        b.bind_node_ref(first_node, first_ref);
        b.bind_node_ref(second_node, second_ref);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "BoundRefs");
        let first_declaration = js
            .find("const __ref_0 = { current: null };")
            .expect("first reachable ref is declared");
        let first_cleanup = js
            .find("__rsjs_disposers.push(() => { __ref_0.current = null; });")
            .expect("first reachable ref is registered for cleanup");
        let second_cleanup = js
            .find("__rsjs_disposers.push(() => { __ref_1.current = null; });")
            .expect("second reachable ref is registered for cleanup");
        let first_assignment = js
            .find("__ref_0.current = __node_1;")
            .expect("first reachable ref is bound to its DOM node");

        assert!(
            first_declaration < first_cleanup
                && first_cleanup < second_cleanup
                && second_cleanup < first_assignment,
            "cleanup must be registered deterministically before any DOM lookup/binding can fail; \
             the runtime then unwinds ref 1 before ref 0:\n{js}"
        );
        assert!(
            js.contains("dispose_all")
                && js.contains("return () => dispose_all(__rsjs_disposers, \"rsjs bindings\");"),
            "a live node ref must keep the generated owner cleanup path; got:\n{js}"
        );

        let fragment = emit_fragment(&island, "bound_refs_fragment");
        assert!(
            fragment.contains("const __rsjs_disposers = [];")
                && fragment.contains("__rsjs_disposers.push(() => { __ref_0.current = null; });")
                && fragment
                    .contains("return () => dispose_all(__rsjs_disposers, \"rsjs bindings\");")
                && fragment
                    .contains("dispose_all(__rsjs_disposers, \"rsjs fragment init rollback\");"),
            "standalone fragments must own, roll back, and return their node-ref cleanup stack; \
             got:\n{fragment}"
        );
    }

    #[test]
    fn unreachable_node_refs_do_not_request_disposer_support() {
        use crate::builder::IslandBuilder;

        let mut b = IslandBuilder::new();
        b.node_ref();
        let root = b.element("section", None);
        b.set_root(root);
        let island = b.build();
        let ctx = EmitContext::island(&island);

        assert!(
            !island_has_disposers(&island, &ctx, &[], &[], &[]),
            "an unreachable declaration is not an owner resource"
        );
        let imports = runtime_import_line(
            &island,
            RuntimeImportMode::Island,
            &ctx,
            &[],
            &[],
            &[],
            &BTreeSet::new(),
        );
        assert!(
            !imports.contains("dispose_all"),
            "an unreachable node ref must not pull disposer support into the runtime import: \
             {imports}"
        );
        assert_eq!(
            emit(&island, "DeadRefOnly"),
            "",
            "an otherwise static island with a dead node-ref declaration emits no module"
        );
    }

    #[test]
    fn emit_skips_unreachable_signal_declarations() {
        use crate::builder::IslandBuilder;

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

        let js = emit(&island, "LiveOnly");

        assert!(
            js.contains("const __sig_1") && js.contains("__sig_1.get()"),
            "reachable signal should still be emitted and read; got:\n{js}"
        );
        assert!(
            !js.contains("__sig_0") && !js.contains("secret") && !js.contains("still-dead"),
            "unreachable signal declaration and unbound writer must not ship; got:\n{js}"
        );
    }

    #[test]
    fn emit_keeps_signal_deps_from_reachable_query_metadata() {
        use crate::builder::IslandBuilder;
        use crate::client::QueryAccessor;
        use crate::reactive::QueryOptions;

        let mut b = IslandBuilder::new();
        let symbol = b.signal(Value::String("AAPL".to_string()));
        let key = b.expr(ClientExpr::QueryKey(vec![
            ClientExpr::string("stock"),
            ClientExpr::signal_read(symbol),
            ClientExpr::signal_read(symbol),
        ]));
        let url = b.expr(ClientExpr::StringToLowercase {
            target: Box::new(ClientExpr::signal_read(symbol)),
        });
        let query = b.query(key, key, url, QueryOptions::default());
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, query_read);
        b.set_root(root);
        let island = b.build();

        for js in [
            emit(&island, "StockQuery"),
            emit_fragment(&island, "StockQuery"),
        ] {
            assert!(
                js.contains("const __sig_0")
                    && js.contains("register_query")
                    && js.contains("__sig_0.get()"),
                "query key signal dependency must be emitted with reachable query metadata; got:\n{js}"
            );
            assert!(
                js.contains(
                    "import { rsjs, dispose_query, register_query, rebind_query } from \
                     \"/rsjs-runtime-query.js\";"
                ),
                "live query keys must import rebind_query; got:\n{js}"
            );
            assert!(
                js.contains(
                    "const __rsjs_query_registration_0 = () => ({\n\
                     \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}island: \"StockQuery\","
                ) && js.contains("key: [\"stock\", __sig_0.get(), __sig_0.get()],")
                    && js.contains("live_key: true,")
                    && js.contains("fetcher: (String((__sig_0.get()) ?? \"\").toLowerCase()),"),
                "live key and request descriptor must be reevaluated atomically; got:\n{js}"
            );
            assert!(
                !js.contains("fetcher: () =>"),
                "a live request descriptor must be snapshotted per key, not read \
                 lazily by an old cache entry; got:\n{js}"
            );
            assert!(
                js.contains(
                    "const __rsjs_query_key_watcher_0 = memo(() => {\n\
                     \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}rebind_query(\
                     __rsjs_query_0, __rsjs_query_registration_0());"
                ) && js.contains("}, [__sig_0]);")
                    && js.contains("__rsjs_disposers.push(__rsjs_query_key_watcher_0._dispose);"),
                "live query keys need one coalesced, disposer-owned watcher; got:\n{js}"
            );
        }
    }

    #[test]
    fn emit_keeps_static_query_registration_byte_lean() {
        use crate::builder::IslandBuilder;
        use crate::client::QueryAccessor;
        use crate::reactive::QueryOptions;

        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::QueryKey(vec![ClientExpr::string("stock")]));
        let url = b.expr(ClientExpr::string("/api/stocks"));
        let query = b.query(key, key, url, QueryOptions::default());
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, query_read);
        b.set_root(root);
        let island = b.build();

        let js = emit(&island, "StaticStockQuery");

        assert!(
            js.contains("const __rsjs_query_0 = register_query({")
                && js.contains("__rsjs_disposers.push(() => dispose_query(__rsjs_query_0));"),
            "static query should retain direct registration; got:\n{js}"
        );
        assert!(
            !js.contains("rebind_query")
                && !js.contains("__rsjs_query_registration_0")
                && !js.contains("__rsjs_query_key_watcher_0"),
            "static query should not pay for live-key machinery; got:\n{js}"
        );
    }

    #[test]
    fn fragment_query_metadata_uses_seed_root_while_dom_bindings_use_fragment_root() {
        use crate::builder::IslandBuilder;
        use crate::client::QueryAccessor;
        use crate::reactive::QueryOptions;

        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::QueryKey(vec![ClientExpr::string("stock")]));
        let url = b.expr(ClientExpr::string("/api/stocks"));
        let query = b.query(key, key, url, QueryOptions::default());
        let query_read = b.expr(ClientExpr::QueryRead {
            id: query,
            accessor: QueryAccessor::Data,
        });
        let root = b.element("section", None);
        let output = b.element("output", Some(root));
        b.bind_text(output, query_read);
        b.set_root(root);
        let island = b.build();

        let js = emit_fragment(&island, "stock_fragment");

        assert!(
            js.contains(
                "const __rsjs_query_0 = register_query({\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}island: \"stock_fragment\",\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}query_id: 0,\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}root: options.seedRoot ?? root,"
            ),
            "a linked fragment must resolve adjacent query metadata from its candidate wrapper; \
             got:\n{js}"
        );
        assert!(
            js.contains("__rsjs_disposers.push(() => dispose_query(__rsjs_query_0));"),
            "a fragment instance must release its query observer on every unmount; got:\n{js}"
        );
        assert!(
            js.contains("walk_path(root, [0], \"data-rsjs-bind\", \"1\")"),
            "the query's DOM binding must stay scoped to the fragment's exact inner root; got:\n{js}"
        );
        assert!(
            !js.contains("walk_path(options.seedRoot ?? root"),
            "the outer seed wrapper must never become the fragment's DOM authority; got:\n{js}"
        );
    }

    #[test]
    fn fragment_mutation_metadata_uses_seed_root_while_bindings_and_cleanup_stay_local() {
        use crate::builder::IslandBuilder;
        use crate::client::MutationAccessor;
        use crate::reactive::{InvalidationStrategy, MutationOptions};

        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::QueryKey(vec![ClientExpr::string("save")]));
        let server_runner = b.expr(ClientExpr::Unit);
        let client_runner = b.expr(ClientExpr::string("/api/save"));
        let invalidates = b.expr(ClientExpr::QueryKey(vec![ClientExpr::string("results")]));
        let mutation = b.mutation(
            key,
            server_runner,
            client_runner,
            vec![invalidates],
            MutationOptions {
                retry_count: 2,
                on_success_invalidate_strategy: InvalidationStrategy::Reset,
            },
        );
        let pending = b.expr(ClientExpr::MutationRead {
            id: mutation,
            accessor: MutationAccessor::IsPending,
        });
        let root = b.element("section", None);
        let output = b.element("output", Some(root));
        b.bind_text(output, pending);
        b.set_root(root);
        let island = b.build();

        let js = emit_fragment(&island, "save_fragment");

        assert!(
            js.contains(
                "const __rsjs_mutation_0 = register_mutation({\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}island: \"save_fragment\",\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}mutation_id: 0,\n\
                 \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}root: options.seedRoot ?? root,"
            ),
            "a linked fragment must resolve adjacent mutation metadata from its candidate wrapper; got:\n{js}"
        );
        assert!(
            js.contains("runner: \"/api/save\"")
                && js.contains("invalidates: [[\"results\"]]")
                && js.contains("retry_count: 2")
                && js.contains("on_success_invalidate_strategy: \"reset\""),
            "the producer-authored mutation request, invalidations, and options must remain exact; got:\n{js}"
        );
        assert!(
            js.contains("walk_path(root, [0], \"data-rsjs-bind\", \"1\")")
                && js.contains("__rsjs_disposers.push("),
            "mutation observation and its disposer must stay scoped to the fragment DOM; got:\n{js}"
        );
        assert!(
            !js.contains("walk_path(options.seedRoot ?? root"),
            "the outer seed wrapper must never become the fragment's DOM authority; got:\n{js}"
        );
    }

    #[test]
    fn emit_lowers_mutation_read_accessor() {
        use crate::builder::IslandBuilder;
        use crate::client::MutationAccessor;
        use crate::reactive::{InvalidationStrategy, MutationOptions};
        let mut b = IslandBuilder::new();
        let mkey = b.expr(ClientExpr::Literal(Value::String("k".to_string())));
        let mserver = b.expr(ClientExpr::Literal(Value::Null));
        let mclient = b.expr(ClientExpr::Literal(Value::String("/api".to_string())));
        let mid = b.mutation(
            mkey,
            mserver,
            mclient,
            Vec::new(),
            MutationOptions {
                retry_count: 0,
                on_success_invalidate_strategy: InvalidationStrategy::Refetch,
            },
        );
        // Bind a span's text content to mutation.is_pending().
        let pending = b.expr(ClientExpr::MutationRead {
            id: mid,
            accessor: MutationAccessor::IsPending,
        });
        let root = b.element("div", None);
        let span = b.element("span", Some(root));
        b.bind_text(span, pending);
        b.set_root(root);
        let island = b.build();
        let js = emit(&island, "UpdateCart");
        assert!(
            js.contains("__rsjs_mutation_0.is_pending()"),
            "missing MutationRead accessor lowering; got:\n{js}"
        );
    }

    #[test]
    fn emit_lowers_event_collection_reads() {
        use crate::builder::IslandBuilder;
        use crate::client::{EventCollection, EventCollectionField};
        use crate::reactive::ConsoleLevel;

        let mut b = IslandBuilder::new();
        let h = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Console {
            level: ConsoleLevel::Log,
            args: vec![
                ClientExpr::EventCollectionLen(EventCollection::ChangedTouches),
                ClientExpr::EventCollectionRead {
                    collection: EventCollection::ChangedTouches,
                    index: Box::new(ClientExpr::int(1)),
                    field: EventCollectionField::TouchClientX,
                },
                ClientExpr::EventCollectionRead {
                    collection: EventCollection::DataTransferItems,
                    index: Box::new(ClientExpr::int(0)),
                    field: EventCollectionField::DataTransferItemKind,
                },
            ],
        }]));
        let root = b.element("div", None);
        let btn = b.element("button", Some(root));
        b.bind_event(btn, crate::bindings::EventName::Click, h);
        b.set_root(root);
        let island = b.build();
        let js = emit(&island, "Collections");

        assert!(
            js.contains(
                "console.log((event.changedTouches?.length ?? 0), (event.changedTouches?.[Math.trunc(Number(1) || 0)]?.clientX ?? 0), (event.dataTransfer?.items?.[Math.trunc(Number(0) || 0)]?.kind ?? \"\"));"
            ),
            "missing event collection lowering; got:\n{js}"
        );
    }

    #[test]
    fn emit_is_deterministic_under_hashmap_iteration() {
        // Build a small island and emit it many times; the byte
        // string must match exactly. This catches HashMap iteration
        // order leaks.
        use crate::builder::IslandBuilder;
        let mut b = IslandBuilder::new();
        let s0 = b.signal(Value::Int(0));
        let s1 = b.signal(Value::Int(0));
        let e0 = b.expr(ClientExpr::SignalRead(s0));
        let e1 = b.expr(ClientExpr::SignalRead(s1));
        let root = b.element("div", None);
        let n0 = b.element("span", Some(root));
        let n1 = b.element("span", Some(root));
        let n2 = b.element("span", Some(root));
        let n3 = b.element("span", Some(root));
        b.bind_text(n0, e0);
        b.bind_text(n1, e1);
        b.bind_text(n2, e0);
        b.bind_text(n3, e1);
        b.set_root(root);
        let island = b.build();

        let first = emit(&island, "T");
        for _ in 0..32 {
            assert_eq!(emit(&island, "T"), first);
        }
    }

    /// `MutationOptions::optimistic_update` was removed (option 2 of
    /// the M9.5 plan) because emit_js never serialised it AND the
    /// runtime applied the same optimistic value to every invalidated
    /// key, which is incorrect for any non-trivial mutation. The
    /// emitted register_mutation() options object must therefore not
    /// mention the field at all so client code never reads stale
    /// half-wired contracts.
    #[test]
    fn emit_does_not_serialise_optimistic_update_field() {
        use crate::builder::IslandBuilder;
        use crate::reactive::{InvalidationStrategy, MutationOptions};
        let mut b = IslandBuilder::new();
        let cart = b.signal(Value::String("[]".to_string()));
        let mkey = b.expr(ClientExpr::Literal(Value::String("k".to_string())));
        let mserver = b.expr(ClientExpr::Literal(Value::Null));
        let mclient = b.expr(ClientExpr::Literal(Value::String("/api".to_string())));
        let inv = b.expr(ClientExpr::Literal(Value::String("cart".to_string())));
        let mid = b.mutation(
            mkey,
            mserver,
            mclient,
            vec![inv],
            MutationOptions {
                retry_count: 0,
                on_success_invalidate_strategy: InvalidationStrategy::Refetch,
            },
        );
        let h = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
            mutation: mid,
            args: vec![ClientExpr::SignalRead(cart)],
        }]));
        let root = b.element("div", None);
        let btn = b.element("button", Some(root));
        b.bind_event(btn, crate::bindings::EventName::Click, h);
        b.set_root(root);
        let island = b.build();
        let js = emit(&island, "M");
        assert!(
            !js.contains("optimistic_update"),
            "emit_js must not emit `optimistic_update` until M9.5; got:\n{js}"
        );
    }

    /// Round-3 review #6 regression. A node that hosts both a text
    /// binding and an event binding (e.g.
    /// `<button on_click={...}>{count.get()}</button>`) used to
    /// emit two `const __node_<id> = ...` lines and the JS engine
    /// rejected the redeclared `const`. emit_js now dedups the
    /// declaration via a per-emit "declared" set; the const appears
    /// at most once.
    #[test]
    fn emits_each_const_node_at_most_once_across_binding_kinds() {
        use crate::builder::IslandBuilder;
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let h = b.handler_from_ir(ClientBlock::new());
        let root = b.element("div", None);
        let btn = b.element("button", Some(root));
        // text binding on the button
        let read_expr = b.expr(ClientExpr::SignalRead(count));
        b.bind_text(btn, read_expr);
        // event binding on the SAME node
        b.bind_event(btn, crate::bindings::EventName::Click, h);
        b.set_root(root);
        let island = b.build();
        let js = emit(&island, "samenode");
        let needle = format!("const {} = ", node_var(btn));
        let occurrences = js.match_indices(&needle).count();
        assert_eq!(
            occurrences, 1,
            "expected exactly one `const __node_<id>` line for the multi-binding node; got {occurrences} in:\n{js}"
        );
    }
}
