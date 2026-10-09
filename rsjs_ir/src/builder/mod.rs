// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Ergonomic constructors for hand-building an [`Island`].
//!
//! The builder is the hand-author frontend: it lets tests (and
//! programmatic frontends) construct an IR without going through the
//! macro pipeline. It is deliberately thin: every method either
//! allocates a new id and returns it, or wires an existing id into a
//! side-table.
//!
//! v0.1 (M1-M9) surface:
//!
//! - [`IslandBuilder::signal`] — declare a new signal.
//! - [`IslandBuilder::expr`] — push an expression into the arena.
//! - [`IslandBuilder::handler_from_ir`] — declare a new event handler from
//!   Client IR v3.
//! - [`IslandBuilder::element`] — append an element view node.
//! - [`IslandBuilder::text_node`] — append a text-binding slot
//!   (and wire its binding into the side-table in one step).
//! - [`IslandBuilder::server_only_node`] — append an explicit
//!   server-rendered opaque child that has no client-side IR.
//! - [`IslandBuilder::bind_text`] — wire an existing node's text
//!   content to an expression.
//! - [`IslandBuilder::bind_event`] — wire a DOM event on a node to
//!   a handler.
//! - [`IslandBuilder::bind_input`] — wire a `bind_value` /
//!   `bind_checked` / `bind_selected` two-way binding (M5).
//! - [`IslandBuilder::bind_attr`] — wire a dynamic one-way HTML
//!   attribute binding.
//! - [`IslandBuilder::conditional_section`] — register an `if`
//!   block's then/else branches under a parent (M4).
//! - [`IslandBuilder::query`] — declare a server-driven query (M8).
//! - [`IslandBuilder::mutation`] — declare a mutation handler (M9).
//! - [`IslandBuilder::add_client_module_import`] — record a bundle
//!   path the island depends on (M7e).
//! - [`IslandBuilder::set_hydration_strategy`] — choose the client
//!   hydration trigger for generated island modules.
//! - [`IslandBuilder::add_component_fragment_import`] — record a
//!   descriptor-backed child component fragment the island composes.
//! - [`IslandBuilder::component_fragment_mount`] — record a concrete
//!   parent-to-fragment mount call.
//! - [`IslandBuilder::set_async`] — flag the island as async.
//! - [`IslandBuilder::set_root`] — designate the view tree's root.
//! - [`IslandBuilder::build`] — finalise into an [`Island`].
//!
//! The builder does not validate cross-references (e.g. it does not
//! check that an [`ExprId`] passed to `bind_text` originated from
//! this builder's arena). Misuse panics later, when an emitter or
//! analysis dereferences the bad id; that is acceptable for the
//! test-author frontend. The macro frontend (`rsjs_macros::analyze`)
//! reaches the same builder via syntactic validation.

use crate::bindings::{
    AttrBinding, AttrBindingKind, Bindings, ConditionalSection, EventName, InputBinding, InputKind,
    NodeRefBinding,
};
use crate::client::{ClientBlock, ClientExpr, ClientType, IntegerDomain};
use crate::island::{
    ComponentFragmentCallback, ComponentFragmentMount, ComponentFragmentNodeAlias,
    ComponentFragmentSignalAlias, ExprArena, HydrationStrategy, Island, PayloadAttr, ServerAttr,
    ServerOnlyNode, ServerSurfaceAnchor, ServerSurfaceSource, SourceAnchor, ViewNode, ViewTree,
};
use crate::lists::ListDecl;
use crate::reactive::{
    CallbackSlot, EventHandler, IntervalDecl, MutationDecl, MutationOptions, NodeRefDecl,
    QueryDecl, QueryOptions, SharedFieldRef, SharedStoreDecl, SignalDecl,
};
use crate::types::{
    CallbackSlotId, EventHandlerId, ExprId, MutationId, NodeId, NodeRefId, QueryId, SharedStoreId,
    SignalId,
};
use crate::value::Value;

/// Fluent builder for an [`Island`].
///
/// Construct one with [`IslandBuilder::new`], allocate signals /
/// expressions / handlers / nodes via the typed helper methods,
/// wire them together with `bind_*` calls, and finalise with
/// [`IslandBuilder::build`]. See the crate-level docs for an
/// example.
#[derive(Debug, Default)]
pub struct IslandBuilder {
    shared_stores: Vec<SharedStoreDecl>,
    signals: Vec<SignalDecl>,
    handlers: Vec<EventHandler>,
    callback_slots: Vec<CallbackSlot>,
    node_refs: Vec<NodeRefDecl>,
    bindings: Bindings,
    exprs: ExprArena,
    view_tree: ViewTree,
    is_async: bool,
    client_module_imports: Vec<String>,
    hydration_strategy: HydrationStrategy,
    component_fragment_imports: Vec<String>,
    component_fragment_mounts: Vec<ComponentFragmentMount>,
    queries: Vec<QueryDecl>,
    mutations: Vec<MutationDecl>,
    intervals: Vec<IntervalDecl>,
    lists: Vec<ListDecl>,
}

impl IslandBuilder {
    /// Construct an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a new signal with the given initial value. Returns
    /// the freshly allocated [`SignalId`].
    ///
    /// The declared [`ClientType`] is inferred from the literal seed
    /// via [`ClientType::of_value`]. Use [`IslandBuilder::signal_typed`]
    /// when the caller knows a more precise (or honestly less precise)
    /// shape, e.g. `Unknown` for runtime initializers.
    pub fn signal(&mut self, init: Value) -> SignalId {
        let ty = ClientType::of_value(&init);
        self.signal_typed(init, ty)
    }

    /// Declare a new signal with the given initial value and declared
    /// client-side value shape. Returns the freshly allocated
    /// [`SignalId`].
    pub fn signal_typed(&mut self, init: Value, ty: ClientType) -> SignalId {
        let id = SignalId(self.signals.len() as u32);
        self.signals.push(SignalDecl {
            id,
            source_name: None,
            source_anchor: None,
            init,
            derived: None,
            ty,
            integer_domain: None,
            shared: None,
        });
        id
    }

    /// Declare a new memo (derived) cell whose value is the arena
    /// expression `body` re-evaluated whenever the signals it reads
    /// change. Returns the freshly allocated [`SignalId`]; see
    /// [`crate::reactive::SignalDecl::derived`] for why derived cells
    /// share the signal id space.
    pub fn memo(&mut self, body: ExprId) -> SignalId {
        let id = SignalId(self.signals.len() as u32);
        self.signals.push(SignalDecl {
            id,
            source_name: None,
            source_anchor: None,
            init: Value::Null,
            derived: Some(body),
            // Backfilled from the derived body by the whole-island
            // typing pass once the island is fully constructed; see
            // `SignalDecl::ty`.
            ty: ClientType::Unknown,
            integer_domain: None,
            shared: None,
        });
        id
    }

    /// Declare one typed page-scoped shared-state acquisition.
    pub fn shared_store(
        &mut self,
        source_name: String,
        rust_type: String,
        source_anchor: Option<SourceAnchor>,
    ) -> SharedStoreId {
        let id = SharedStoreId(self.shared_stores.len() as u32);
        self.shared_stores.push(SharedStoreDecl {
            id,
            source_name,
            rust_type,
            source_anchor,
        });
        id
    }

    /// Declare a field cell from a page-scoped shared store.
    pub fn shared_field(
        &mut self,
        store: SharedStoreId,
        field: String,
        ty: ClientType,
    ) -> SignalId {
        let id = SignalId(self.signals.len() as u32);
        self.signals.push(SignalDecl {
            id,
            source_name: None,
            source_anchor: None,
            init: Value::Null,
            derived: None,
            ty,
            integer_domain: None,
            shared: Some(SharedFieldRef { store, field }),
        });
        id
    }

    /// Attach diagnostic source metadata to a signal declaration.
    pub fn set_signal_source(
        &mut self,
        id: SignalId,
        source_name: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        if let Some(signal) = self.signals.get_mut(id.0 as usize) {
            signal.source_name = source_name;
            signal.source_anchor = source_anchor;
        }
    }

    /// Overwrite the declared client-side value shape of an
    /// already-declared signal. Used to backfill memo (derived)
    /// cell types from the whole-island typing pass, and by the
    /// analyzer when a later fact sharpens a declaration.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not allocated by this builder.
    pub fn set_signal_ty(&mut self, id: SignalId, ty: ClientType) {
        self.signals[id.0 as usize].ty = ty;
    }

    /// Declared client-side value shape of an already-declared signal,
    /// or `None` for an id this builder never allocated.
    pub fn signal_ty(&self, id: SignalId) -> Option<&ClientType> {
        self.signals.get(id.0 as usize).map(|decl| &decl.ty)
    }

    /// Attach the proven Rust fixed-width integer domain to a signal.
    pub fn set_signal_integer_domain(
        &mut self,
        id: SignalId,
        integer_domain: Option<IntegerDomain>,
    ) {
        self.signals[id.0 as usize].integer_domain = integer_domain;
    }

    /// Proven Rust fixed-width integer domain of an already-declared signal.
    pub fn signal_integer_domain(&self, id: SignalId) -> Option<IntegerDomain> {
        self.signals
            .get(id.0 as usize)
            .and_then(|decl| decl.integer_domain)
    }

    /// Mark an already-declared signal as a memo (derived) cell.
    ///
    /// Used by the component inliner: child signal declarations are
    /// copied before the child expression arena, so a child memo
    /// cell's remapped body id only becomes known after the arena copy.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not allocated by this builder.
    pub fn set_signal_derived(&mut self, id: SignalId, body: ExprId) {
        self.signals[id.0 as usize].derived = Some(body);
    }

    /// Signal declarations made so far, in id order. Together with
    /// [`IslandBuilder::expr_arena`] this is the context the
    /// whole-island typing pass
    /// ([`crate::analysis::typing::TypeEnv`]) needs while the island
    /// is still under construction.
    pub fn signal_decls(&self) -> &[SignalDecl] {
        &self.signals
    }

    /// The expression arena as built so far. See
    /// [`IslandBuilder::signal_decls`].
    pub fn expr_arena(&self) -> &ExprArena {
        &self.exprs
    }

    /// Push an expression into the arena. Returns the freshly
    /// allocated [`ExprId`].
    pub fn expr(&mut self, client_expr: ClientExpr) -> ExprId {
        self.exprs.alloc(client_expr)
    }

    /// Declare a new event handler from an already-lowered Client IR v3 block.
    pub fn handler_from_ir(&mut self, body_ir: ClientBlock) -> EventHandlerId {
        let id = EventHandlerId(self.handlers.len() as u32);
        self.handlers.push(EventHandler::from_ir(id, body_ir));
        id
    }

    /// Declare a new debounced event handler from an already-lowered
    /// Client IR v3 block. The emitted JS wraps the body with the
    /// runtime's `debounce(fn, delay_ms)` (trailing edge) and pushes the
    /// wrapper's `_dispose` into the island disposer list.
    pub fn debounce_handler_from_ir(&mut self, ms: u64, body_ir: ClientBlock) -> EventHandlerId {
        let id = EventHandlerId(self.handlers.len() as u32);
        self.handlers
            .push(EventHandler::from_ir_debounced(id, ms, body_ir));
        id
    }

    /// Attach diagnostic source metadata to an event handler declaration.
    pub fn set_handler_source(
        &mut self,
        id: EventHandlerId,
        source_name: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        if let Some(handler) = self.handlers.get_mut(id.0 as usize) {
            handler.source_name = source_name;
            handler.source_anchor = source_anchor;
        }
    }

    /// Declare an island-lifetime interval timer running `body_ir`
    /// every `ms` milliseconds. Intervals carry no id: nothing else in
    /// the IR references them.
    pub fn interval(&mut self, ms: u64, body_ir: ClientBlock) {
        self.intervals.push(IntervalDecl::from_ir(ms, body_ir));
    }

    /// Declare a keyed list: client-side reconciliation of a
    /// container's children over a `Vec`-valued signal. The frontend
    /// is responsible for having already allocated the referenced
    /// container / loop / item-root view nodes and the driving signal;
    /// see [`crate::lists::ListDecl`] for the field contracts.
    pub fn keyed_list(&mut self, decl: ListDecl) {
        self.lists.push(decl);
    }

    /// Declare or reuse a named callback slot.
    ///
    /// The analyser calls this for source shapes like
    /// `self.on_change(value)`. Reusing the same field returns the
    /// existing id so repeated invocations share one slot.
    pub fn callback_slot(&mut self, name: &str) -> CallbackSlotId {
        if let Some(slot) = self.callback_slots.iter().find(|slot| slot.name == name) {
            return slot.id;
        }
        let id = CallbackSlotId(self.callback_slots.len() as u32);
        self.callback_slots.push(CallbackSlot {
            id,
            name: name.to_string(),
        });
        id
    }

    /// Declare a new DOM node ref.
    pub fn node_ref(&mut self) -> NodeRefId {
        let id = NodeRefId(self.node_refs.len() as u32);
        self.node_refs.push(NodeRefDecl {
            id,
            source_name: None,
            source_anchor: None,
        });
        id
    }

    /// Attach diagnostic source metadata to a node-ref declaration.
    pub fn set_node_ref_source(
        &mut self,
        id: NodeRefId,
        source_name: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        if let Some(node_ref) = self.node_refs.get_mut(id.0 as usize) {
            node_ref.source_name = source_name;
            node_ref.source_anchor = source_anchor;
        }
    }

    /// Append an element node to the view tree. If `parent` is
    /// `Some(id)`, the new node is also pushed onto that parent's
    /// child list. Returns the freshly allocated [`NodeId`].
    ///
    /// # Panics
    ///
    /// Panics if `parent` is `Some(id)` and the parent node is not
    /// an [`ViewNode::Element`] (text-binding slots cannot have
    /// children).
    pub fn element(&mut self, tag: &str, parent: Option<NodeId>) -> NodeId {
        let id = self.view_tree.alloc(ViewNode::Element {
            tag: tag.to_string(),
            children: Vec::new(),
            static_attrs: Vec::new(),
            server_attrs: Vec::new(),
            payload_attrs: Vec::new(),
        });
        if let Some(parent_id) = parent {
            self.attach(parent_id, id);
        }
        id
    }

    /// Attach a static HTML attribute to an element node. Used by
    /// the analyser for attributes that rsjs itself does not consume
    /// (anything other than `on_*` / `bind_*`); emit_rust threads
    /// these straight into the generated `html_sync!` body so author
    /// attributes like `class="error"`, `aria-label="..."`, `id="foo"`,
    /// `type="submit"` survive the round trip.
    ///
    /// Boolean attributes are stored with an empty `value`.
    ///
    /// # Panics
    ///
    /// Panics if `node` is not an [`ViewNode::Element`].
    pub fn static_attr(&mut self, node: NodeId, name: &str, value: &str) {
        match self.view_tree.get_mut(node) {
            ViewNode::Element { static_attrs, .. } => {
                static_attrs.push((name.to_string(), value.to_string()));
            }
            _ => panic!("static_attr requires an Element node ({node:?})"),
        }
    }

    /// Attach an explicit server-only dynamic attribute to an element node.
    ///
    /// The value is stored as the full source token string for
    /// `rsjs::server_attr(...)`. The Rust emitter reparses it and splices the
    /// inner expression into the generated `html_sync!` body; the JS emitter
    /// ignores it because it has no client-side binding.
    pub fn server_attr(&mut self, node: NodeId, name: &str, tokens: &str) {
        self.server_attr_with_source(node, name, tokens, ServerSurfaceSource::ExplicitBoundary);
    }

    /// Attach a server-only dynamic attribute to an element node with
    /// redacted source metadata.
    pub fn server_attr_with_source(
        &mut self,
        node: NodeId,
        name: &str,
        tokens: &str,
        source: ServerSurfaceSource,
    ) {
        self.server_attr_with_source_anchor(node, name, tokens, source, None);
    }

    /// Attach a server-only dynamic attribute to an element node with
    /// redacted source metadata and a diagnostic source location.
    pub fn server_attr_with_source_anchor(
        &mut self,
        node: NodeId,
        name: &str,
        tokens: &str,
        source: ServerSurfaceSource,
        source_anchor: Option<ServerSurfaceAnchor>,
    ) {
        match self.view_tree.get_mut(node) {
            ViewNode::Element { server_attrs, .. } => {
                server_attrs.push(ServerAttr {
                    name: name.to_string(),
                    tokens: tokens.to_string(),
                    source,
                    source_anchor,
                });
            }
            _ => panic!("server_attr requires an Element node ({node:?})"),
        }
    }

    /// Attach an rsjs-generated dynamic payload attribute to an element node.
    ///
    /// Payload attrs are rendered as JSON strings on the server and consumed by
    /// delegated event handlers on the client. `tokens` is the Rust expression
    /// to encode.
    pub fn payload_attr(&mut self, node: NodeId, name: &str, tokens: &str) {
        self.payload_attr_with_source(node, name, tokens, None, None);
    }

    /// Attach an rsjs-generated dynamic payload attribute with redacted
    /// diagnostic metadata.
    pub fn payload_attr_with_source(
        &mut self,
        node: NodeId,
        name: &str,
        tokens: &str,
        label: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        match self.view_tree.get_mut(node) {
            ViewNode::Element { payload_attrs, .. } => {
                payload_attrs.push(PayloadAttr {
                    name: name.to_string(),
                    tokens: tokens.to_string(),
                    label,
                    source_anchor,
                });
            }
            _ => panic!("payload_attr requires an Element node ({node:?})"),
        }
    }

    /// Return the compiler-owned payload attribute already carrying `tokens`
    /// on `node`.
    ///
    /// A repeated template expression may feed several bindings or handlers on
    /// the same element. Those consumers must share one encoded DOM value: the
    /// expression is evaluated in one loop environment and every read observes
    /// the same immutable server snapshot. Keeping this lookup in the builder
    /// also prevents frontend fallback paths from depending on attribute-name
    /// allocation details.
    pub fn payload_attr_for_tokens(&self, node: NodeId, tokens: &str) -> Option<String> {
        match self.view_tree.get(node) {
            ViewNode::Element { payload_attrs, .. } => payload_attrs
                .iter()
                .find(|attr| attr.tokens == tokens)
                .map(|attr| attr.name.clone()),
            _ => None,
        }
    }

    /// Append a literal static text child under `parent`. Used by
    /// the analyser for `Node::Text` children of an element (e.g.
    /// `"Submit"` inside `<button>Submit</button>`). emit_rust writes
    /// the text into the generated `html_sync!` body as a string
    /// literal child; the macro takes care of HTML escaping.
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not an [`ViewNode::Element`].
    pub fn text_node_static(&mut self, parent: NodeId, text: &str) -> NodeId {
        let id = self.view_tree.alloc(ViewNode::StaticText(text.to_string()));
        self.attach(parent, id);
        id
    }

    /// Append an explicit server-only child under `parent`.
    ///
    /// The token string is produced by the proc-macro frontend from a
    /// `rsjs::server_only(...)` expression. It is only consumed by the
    /// Rust emitter; JS emission, dependency collection, and hydration
    /// markers intentionally ignore this node.
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not an [`ViewNode::Element`].
    pub fn server_only_node(&mut self, parent: NodeId, tokens: &str) -> NodeId {
        self.server_only_node_with_source(parent, tokens, ServerSurfaceSource::ExplicitBoundary)
    }

    /// Append a server-only child under `parent` with redacted source metadata.
    pub fn server_only_node_with_source(
        &mut self,
        parent: NodeId,
        tokens: &str,
        source: ServerSurfaceSource,
    ) -> NodeId {
        self.server_only_node_with_source_anchor(parent, tokens, source, None)
    }

    /// Append a server-only child under `parent` with redacted source metadata
    /// and a diagnostic source location.
    pub fn server_only_node_with_source_anchor(
        &mut self,
        parent: NodeId,
        tokens: &str,
        source: ServerSurfaceSource,
        source_anchor: Option<ServerSurfaceAnchor>,
    ) -> NodeId {
        let id = self.view_tree.alloc(ViewNode::ServerOnly(ServerOnlyNode {
            tokens: tokens.to_string(),
            source,
            source_anchor,
        }));
        self.attach(parent, id);
        id
    }

    /// Append a server-rendered template `for` loop under `parent`.
    ///
    /// The loop header and prefix statements are stored as token strings so
    /// the macro-side Rust emitter can reconstruct the original server loop
    /// while the analysed body children remain addressable for event handler
    /// metadata.
    pub fn for_loop(
        &mut self,
        parent: NodeId,
        attrs: &str,
        label: &str,
        pat: &str,
        expr: &str,
        body_prefix: &str,
    ) -> NodeId {
        let id = self.view_tree.alloc(ViewNode::ForLoop {
            attrs: attrs.to_string(),
            label: label.to_string(),
            pat: pat.to_string(),
            expr: expr.to_string(),
            body_prefix: body_prefix.to_string(),
            children: Vec::new(),
        });
        self.attach(parent, id);
        id
    }

    /// Append a text-binding slot under `parent` and wire its text
    /// content to `expr`. Returns the freshly allocated [`NodeId`].
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not an [`ViewNode::Element`].
    pub fn text_node(&mut self, parent: NodeId, expr: ExprId) -> NodeId {
        let id = self.view_tree.alloc(ViewNode::TextBindingSlot);
        self.attach(parent, id);
        self.bindings.text_bindings.insert(id, expr);
        id
    }

    /// Wire an existing node's text content to an expression.
    /// Useful when the node was created via [`Self::element`] (e.g.
    /// because it has children of its own) and only later receives
    /// a text binding.
    pub fn bind_text(&mut self, node: NodeId, expr: ExprId) {
        self.bindings.text_bindings.insert(node, expr);
    }

    /// Wire a DOM event on a node to a handler.
    pub fn bind_event(&mut self, node: NodeId, event: EventName, handler: EventHandlerId) {
        self.bindings.event_bindings.insert((node, event), handler);
    }

    /// Register a conditional view section under `parent`.
    ///
    /// `condition` must be the [`ExprId`] of a boolean-valued
    /// expression (typically a [`ClientExpr::Binary`] over a
    /// [`ClientExpr::SignalRead`]). `then_children` and `else_children` are
    /// children of `parent` whose visibility the runtime will toggle
    /// as `condition` changes. Both lists must reference NodeIds that
    /// already exist in `parent`'s children list; the builder does
    /// not reorganise the view tree, it only annotates it.
    ///
    /// Pass an empty `else_children` for the `if`-only shape.
    ///
    pub fn conditional_section(
        &mut self,
        parent: NodeId,
        condition: ExprId,
        then_children: Vec<NodeId>,
        else_children: Vec<NodeId>,
    ) {
        let section = self.reserve_conditional_section(parent, condition);
        self.complete_conditional_section(parent, section, then_children, else_children);
    }

    /// Reserve a stable per-parent conditional-section ordinal.
    ///
    /// This compiler-facing two-step form lets an analyzer assign the outer
    /// section before recursively lowering nested branch content. Call
    /// [`Self::complete_conditional_section`] exactly once with the returned
    /// ordinal.
    #[doc(hidden)]
    pub fn reserve_conditional_section(&mut self, parent: NodeId, condition: ExprId) -> usize {
        let sections = self
            .bindings
            .conditional_sections
            .entry(parent)
            .or_default();
        let ordinal = sections.len();
        sections.push(ConditionalSection {
            condition,
            then_children: Vec::new(),
            else_children: Vec::new(),
        });
        ordinal
    }

    /// Fill one section previously created by
    /// [`Self::reserve_conditional_section`].
    #[doc(hidden)]
    pub fn complete_conditional_section(
        &mut self,
        parent: NodeId,
        ordinal: usize,
        then_children: Vec<NodeId>,
        else_children: Vec<NodeId>,
    ) {
        let section = self
            .bindings
            .conditional_sections
            .get_mut(&parent)
            .and_then(|sections| sections.get_mut(ordinal))
            .expect("a conditional section must be reserved before it is completed");
        section.then_children = then_children;
        section.else_children = else_children;
    }

    /// Wire a two-way input binding on `node`.
    ///
    /// `kind` selects which DOM property the runtime mirrors; `signal`
    /// is the signal whose value the input reflects. The signal's
    /// initial value also drives the SSR-time output (the `value=`
    /// attribute or the `checked` flag).
    ///
    /// # Panics
    ///
    /// Panics if an input binding is already registered for `node`
    /// (one input binding per element in v0.1; nothing in the M5
    /// fixture motivates multiple bindings on a single node).
    pub fn bind_input(&mut self, node: NodeId, kind: InputKind, signal: SignalId) {
        let prev = self
            .bindings
            .input_bindings
            .insert(node, InputBinding { node, kind, signal });
        if prev.is_some() {
            panic!("input binding already registered for node {node:?}");
        }
    }

    /// Wire a one-way dynamic attribute binding on `node`.
    ///
    /// # Panics
    ///
    /// Panics if the same `(node, name)` binding is registered twice.
    pub fn bind_attr(&mut self, node: NodeId, name: &str, kind: AttrBindingKind, expr: ExprId) {
        let key = (node, name.to_string());
        let prev = self.bindings.attr_bindings.insert(
            key,
            AttrBinding {
                node,
                name: name.to_string(),
                kind,
                expr,
            },
        );
        if prev.is_some() {
            panic!("attribute binding `{name}` already registered for node {node:?}");
        }
    }

    /// Bind an element node to an author-declared node ref.
    ///
    /// # Panics
    ///
    /// Panics if `node` already has a node-ref binding or if `node_ref` is
    /// already bound to another element. A node ref denotes one stable DOM
    /// target; silently assigning it more than once would make the last
    /// `NodeId` win and would be branch/order dependent.
    pub fn bind_node_ref(&mut self, node: NodeId, node_ref: NodeRefId) {
        if let Some(existing) = self
            .bindings
            .node_ref_bindings
            .values()
            .find(|binding| binding.node_ref == node_ref)
        {
            panic!(
                "node ref {node_ref:?} is already bound to node {:?}",
                existing.node
            );
        }
        let prev = self
            .bindings
            .node_ref_bindings
            .insert(node, NodeRefBinding { node, node_ref });
        if prev.is_some() {
            panic!("node ref binding already registered for node {node:?}");
        }
    }

    /// Mark this island as using the async-capable render path. When set,
    /// [`Island::is_async`] becomes `true` and Rust emission uses
    /// `proa_macros::html!`. The default is sync.
    ///
    /// The parser sets this from the canonical authored render signature;
    /// hand-built IR fixtures call it directly.
    pub fn set_async(&mut self, is_async: bool) {
        self.is_async = is_async;
    }

    /// Record one transitional bundle import from a decoded or linked
    /// descriptor. New source frontends do not expose component-level bundle
    /// allowlists.
    pub fn add_client_module_import(&mut self, path: String) {
        self.client_module_imports.push(path);
    }

    /// Choose the client-side trigger used by the generated island module.
    pub fn set_hydration_strategy(&mut self, strategy: HydrationStrategy) {
        self.hydration_strategy = strategy;
    }

    /// Record a descriptor-backed reusable component fragment dependency.
    ///
    /// `asset_id` is the child component's rsjs asset id, matching the
    /// component-fragment manifest entry emitted by that child's own
    /// `#[rsjs(component...)]` impl. Duplicates are ignored so two uses of the same
    /// child component in one parent still produce one dependency edge.
    pub fn add_component_fragment_import(&mut self, asset_id: String) {
        if !self.component_fragment_imports.contains(&asset_id) {
            self.component_fragment_imports.push(asset_id);
        }
    }

    /// Record a concrete reusable component-fragment mount.
    #[allow(clippy::too_many_arguments)]
    pub fn component_fragment_mount(
        &mut self,
        root: NodeId,
        asset_id: String,
        callbacks: Vec<ComponentFragmentCallback>,
        signal_aliases: Vec<ComponentFragmentSignalAlias>,
        node_aliases: Vec<ComponentFragmentNodeAlias>,
        owned_nodes: Vec<NodeId>,
        owned_signals: Vec<SignalId>,
        owned_handlers: Vec<EventHandlerId>,
        owned_node_refs: Vec<NodeRefId>,
    ) {
        self.add_component_fragment_import(asset_id.clone());
        self.component_fragment_mounts.push(ComponentFragmentMount {
            root,
            asset_id,
            callbacks,
            signal_aliases,
            node_aliases,
            owned_nodes,
            owned_signals,
            owned_handlers,
            owned_node_refs,
        });
    }

    /// Number of nodes currently allocated in the view tree.
    pub fn view_tree_len(&self) -> usize {
        self.view_tree.len()
    }

    /// Borrow the bindings accumulated so far.
    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// Borrow an already-allocated view node. Used by frontends for
    /// post-pass structural validation (e.g. the keyed-list
    /// only-child container check).
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this builder.
    pub fn view_node(&self, id: NodeId) -> &ViewNode {
        self.view_tree.get(id)
    }

    /// Designate `id` as the view tree's root.
    pub fn set_root(&mut self, id: NodeId) {
        self.view_tree.set_root(id);
    }

    /// Declare a new M8 query.
    ///
    /// `key_expr` is the [`ExprId`] of the structural cache key
    /// expression (typically an [`ClientExpr::Literal`] in v0.1
    /// fixtures). `server_initial` and `client_fetcher` are
    /// opaque [`ExprId`]s the emitters lower at codegen time;
    /// hand-built fixtures pass [`ClientExpr::Literal`] of the desired
    /// initial value. `options` is the per-query refetch policy.
    ///
    /// Returns the freshly allocated [`QueryId`].
    pub fn query(
        &mut self,
        key_expr: ExprId,
        server_initial: ExprId,
        client_fetcher: ExprId,
        options: QueryOptions,
    ) -> QueryId {
        self.query_typed(
            key_expr,
            server_initial,
            client_fetcher,
            ClientType::Unknown,
            options,
        )
    }

    /// Declare a new M8 query with a known client-side data shape.
    ///
    /// See [`Self::query`] for the common untyped form. Macro-built
    /// islands use this when `let q: rsjs::Query<T> = ...` exposes a
    /// syntactic result type.
    pub fn query_typed(
        &mut self,
        key_expr: ExprId,
        server_initial: ExprId,
        client_fetcher: ExprId,
        data_ty: ClientType,
        options: QueryOptions,
    ) -> QueryId {
        let id = QueryId(self.queries.len() as u32);
        self.queries.push(QueryDecl {
            id,
            source_name: None,
            source_anchor: None,
            key_expr,
            server_initial,
            client_fetcher,
            data_ty,
            options,
        });
        id
    }

    /// Attach diagnostic source metadata to a query declaration.
    pub fn set_query_source(
        &mut self,
        id: QueryId,
        source_name: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        if let Some(query) = self.queries.get_mut(id.0 as usize) {
            query.source_name = source_name;
            query.source_anchor = source_anchor;
        }
    }

    /// Query declarations made so far, in id order.
    pub fn query_decls(&self) -> &[QueryDecl] {
        &self.queries
    }

    /// Append an already-built [`QueryDecl`] to the island. Used by
    /// the macro frontend (M8) when the parser has already
    /// allocated the QueryId; tests prefer [`Self::query`].
    ///
    /// # Panics
    ///
    /// Panics if `decl.id` does not match the next dense slot in
    /// the queries vector. The dense-id invariant matches the one
    /// signals and handlers maintain.
    pub fn add_query(&mut self, decl: QueryDecl) {
        let expected = QueryId(self.queries.len() as u32);
        assert_eq!(
            decl.id, expected,
            "QueryDecl::id must equal the next dense slot ({expected:?}); got {:?}",
            decl.id
        );
        self.queries.push(decl);
    }

    /// Declare a new M9 mutation.
    ///
    /// `key_expr` is the [`ExprId`] of the structural mutation key
    /// expression. `server_runner` and `client_runner` are opaque
    /// [`ExprId`]s the emitters lower at codegen time. `invalidates`
    /// is the list of QueryKey expression ids the mutation refetches
    /// on success. `options` is the per-mutation policy.
    ///
    /// Returns the freshly allocated [`MutationId`].
    pub fn mutation(
        &mut self,
        key_expr: ExprId,
        server_runner: ExprId,
        client_runner: ExprId,
        invalidates: Vec<ExprId>,
        options: MutationOptions,
    ) -> MutationId {
        let id = MutationId(self.mutations.len() as u32);
        self.mutations.push(MutationDecl {
            id,
            source_name: None,
            source_anchor: None,
            key_expr,
            server_runner,
            client_runner,
            invalidates,
            options,
        });
        id
    }

    /// Attach diagnostic source metadata to a mutation declaration.
    pub fn set_mutation_source(
        &mut self,
        id: MutationId,
        source_name: Option<String>,
        source_anchor: Option<SourceAnchor>,
    ) {
        if let Some(mutation) = self.mutations.get_mut(id.0 as usize) {
            mutation.source_name = source_name;
            mutation.source_anchor = source_anchor;
        }
    }

    /// Append an already-built [`MutationDecl`] to the island.
    /// Mirrors [`Self::add_query`] for the (post-M9) parser frontend.
    ///
    /// # Panics
    ///
    /// Panics if `decl.id` does not match the next dense slot in
    /// the mutations vector.
    pub fn add_mutation(&mut self, decl: MutationDecl) {
        let expected = MutationId(self.mutations.len() as u32);
        assert_eq!(
            decl.id, expected,
            "MutationDecl::id must equal the next dense slot ({expected:?}); got {:?}",
            decl.id
        );
        self.mutations.push(decl);
    }

    /// Finalise the builder into an [`Island`]. The builder is
    /// consumed.
    pub fn build(self) -> Island {
        Island {
            shared_stores: self.shared_stores,
            signals: self.signals,
            handlers: self.handlers,
            callback_slots: self.callback_slots,
            node_refs: self.node_refs,
            bindings: self.bindings,
            exprs: self.exprs,
            view_tree: self.view_tree,
            is_async: self.is_async,
            client_module_imports: self.client_module_imports,
            hydration_strategy: self.hydration_strategy,
            component_fragment_imports: self.component_fragment_imports,
            component_fragment_mounts: self.component_fragment_mounts,
            queries: self.queries,
            mutations: self.mutations,
            intervals: self.intervals,
            lists: self.lists,
        }
    }

    /// Internal helper: append `child` to `parent`'s child list.
    fn attach(&mut self, parent: NodeId, child: NodeId) {
        match self.view_tree.get_mut(parent) {
            ViewNode::Element { children, .. } | ViewNode::ForLoop { children, .. } => {
                children.push(child)
            }
            ViewNode::TextBindingSlot => {
                panic!("cannot attach a child to a text-binding slot ({parent:?})");
            }
            ViewNode::StaticText(_) => {
                panic!("cannot attach a child to a static-text node ({parent:?})");
            }
            ViewNode::ServerOnly(_) => {
                panic!("cannot attach a child to a server-only node ({parent:?})");
            }
        }
    }
}
