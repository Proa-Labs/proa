// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Top-level [`Island`] struct: the owner of all rsjs side-tables.
//!
//! An island is the unit of compilation: one struct plus one
//! `#[rsjs(component...)]` render-trait impl lower to one `Island`. The
//! struct owns every signal, every handler, every binding, and the
//! expression arena that the bindings reference.
//!
//! Expressions and handler bodies are Client IR v3 only: the arena
//! stores [`ClientExpr`] and handler-like declarations store
//! [`ClientBlock`]. The legacy v1 expression IR and its conversion
//! compatibility layer have been deleted; every
//! analysis and emitter walks v3 directly.
//!
//! The "view tree" is a placeholder in v0.1. The real lifted
//! `html_sync!` parse tree lands in M7; until then, the builder
//! constructs a small in-memory [`ViewTree`] whose only purpose is
//! to provide stable [`NodeId`]s for the side-tables to key off of.
//! The shape is intentionally minimal: enough to round-trip the
//! Counter fixture (a root with an ordered list of children, each
//! either an element or a text-binding placeholder), nothing more.

use crate::bindings::Bindings;
use crate::client::{ClientBlock, ClientExpr};
use crate::lists::ListDecl;
use crate::reactive::{
    CallbackSlot, EventHandler, IntervalDecl, MutationDecl, NodeRefDecl, QueryDecl,
    SharedStoreDecl, SignalDecl,
};
use crate::types::{CallbackSlotId, EventHandlerId, ExprId, NodeId, NodeRefId, SignalId};

/// Redacted source location for diagnostics and compiler sidecars.
///
/// Line and column values are one-based and refer to the source expression that
/// produced the IR fact. Emitters must not use this metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceAnchor {
    pub start_line: usize,
    pub start_column: usize,
}

/// An expression arena indexed by [`ExprId`].
///
/// The single storage is Client IR v3 ([`ClientExpr`]); the
/// bindings tables key into the arena so that text bindings,
/// attribute bindings, and conditional predicates can all share the
/// same expression by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExprArena {
    client_exprs: Vec<ClientExpr>,
}

impl ExprArena {
    /// Construct an empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an expression and return its id. Ids are dense and
    /// monotonic, starting at `0`.
    pub fn alloc(&mut self, client_expr: ClientExpr) -> ExprId {
        let id = ExprId(self.client_exprs.len() as u32);
        self.client_exprs.push(client_expr);
        id
    }

    /// Borrow the Client IR v3 expression with the given id.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this arena.
    pub fn client_expr(&self, id: ExprId) -> &ClientExpr {
        &self.client_exprs[id.0 as usize]
    }

    /// Mutably iterate every Client IR v3 expression in arena order.
    ///
    /// This is primarily used by descriptor-only normalization passes that
    /// rewrite server payload tokens without changing expression ids.
    pub fn iter_client_exprs_mut(&mut self) -> impl Iterator<Item = &mut ClientExpr> {
        self.client_exprs.iter_mut()
    }

    /// Number of expressions in the arena.
    pub fn len(&self) -> usize {
        self.client_exprs.len()
    }

    /// Whether the arena holds no expressions.
    pub fn is_empty(&self) -> bool {
        self.client_exprs.is_empty()
    }

    /// Iterate every Client IR v3 expression in the arena in id order.
    ///
    /// Used by callers that need a structural pass over the IR (e.g.
    /// `emit_js` deciding whether to import the runtime projection
    /// helpers). The iterator does not yield ids; callers needing
    /// stable ids should index via `len()` + `client_expr()` instead.
    pub fn iter_client_exprs(&self) -> impl Iterator<Item = &ClientExpr> {
        self.client_exprs.iter()
    }
}

/// Client-side trigger used by generated island modules.
///
/// `Immediate` preserves the original rsjs behavior: the island module calls
/// the runtime's `hydrate(...)` entry point as soon as the DOM is ready. The
/// deferred variants map directly to cancellable runtime strategy entry points.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum HydrationStrategy {
    /// Hydrate as soon as the DOM is ready.
    #[default]
    Immediate,
    /// Hydrate when the island root first intersects the viewport.
    Visible,
    /// Hydrate during an idle callback.
    Idle,
    /// Hydrate on the first interaction with the island root.
    Interaction { event: Option<String> },
    /// Hydrate once a media query matches.
    Media { query: String },
}

/// Redacted provenance for server-rendered template surfaces.
///
/// This is intentionally syntax/provenance metadata, not type information.
/// It lets diagnostics explain why a slot or attr stayed SSR without
/// serializing the raw runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ServerSurfaceSource {
    /// The author used an explicit `rsjs::server_only(...)` or
    /// `rsjs::server_attr(...)` boundary.
    #[default]
    ExplicitBoundary,
    /// The author used an explicit server boundary around a value the
    /// frontend could classify as SSR without the wrapper.
    ExplicitSimpleSsrBoundary,
    /// The frontend inferred SSR for a simple ordinary local or field/path
    /// projection.
    InferredSimpleSsr,
    /// The frontend retained an arbitrary Rust expression as opaque SSR after
    /// proving only that its visible syntax does not capture a known client
    /// value or primitive. Its type, purity, and DOM cardinality remain
    /// unknown.
    InferredOpaqueSsr,
    /// The frontend preserved a direct composed component struct literal for
    /// rustc-resolved semantic linking.
    ///
    /// During descriptor compilation this node is ordinary server rendering,
    /// but a successful final link replaces the site with one compiler-owned
    /// exact-one wrapper. This provenance is intentionally distinct from both
    /// arbitrary opaque SSR and an explicit independent `rsjs::island(...)`
    /// boundary.
    DirectComponentSite,
    /// The surface is part of a keyed-list item template.
    KeyedListItem,
    /// The author used an explicit `rsjs::island(...)` boundary.
    ExplicitIslandBoundary,
}

impl ServerSurfaceSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitBoundary => "explicit_boundary",
            Self::ExplicitSimpleSsrBoundary => "explicit_simple_ssr_boundary",
            Self::InferredSimpleSsr => "inferred_simple_ssr",
            Self::InferredOpaqueSsr => "inferred_opaque_ssr",
            Self::DirectComponentSite => "direct_component_site",
            Self::KeyedListItem => "keyed_list_item",
            Self::ExplicitIslandBoundary => "explicit_island_boundary",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::ExplicitBoundary => "explicit SSR boundary",
            Self::ExplicitSimpleSsrBoundary => "explicit wrapper around simple SSR projection",
            Self::InferredSimpleSsr => "inferred simple SSR projection",
            Self::InferredOpaqueSsr => "inferred opaque SSR expression",
            Self::DirectComponentSite => "rustc-resolved direct component site",
            Self::KeyedListItem => "keyed-list item SSR template",
            Self::ExplicitIslandBoundary => "explicit island boundary",
        }
    }
}

pub type ServerSurfaceAnchor = SourceAnchor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAttr {
    pub name: String,
    pub tokens: String,
    pub source: ServerSurfaceSource,
    pub source_anchor: Option<ServerSurfaceAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerOnlyNode {
    pub tokens: String,
    pub source: ServerSurfaceSource,
    pub source_anchor: Option<ServerSurfaceAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadAttr {
    pub name: String,
    pub tokens: String,
    pub label: Option<String>,
    pub source_anchor: Option<SourceAnchor>,
}

/// A node in the v0.1 placeholder view tree.
///
/// The shape covers what the M1 Counter fixture needs:
///
/// - [`ViewNode::Element`] for tag nodes (the surrounding `<div>`,
///   the two `<button>`s, the `<span>`).
/// - [`ViewNode::TextBindingSlot`] for the position inside the
///   `<span>` whose text content is driven by a reactive expression.
/// - [`ViewNode::StaticText`] for literal text children (e.g.
///   `"Submit"` inside a `<button>`).
/// - [`ViewNode::ServerOnly`] for explicit `rsjs::server_only(...)`
///   escape hatches that render on the server but have no client IR.
///
/// M7 replaces this with a reference into the lifted `html_sync!`
/// parse tree. At that point `ViewNode` either disappears or
/// becomes a thin index into the parse tree; the side-tables
/// (`Bindings`, `signals`, `handlers`) do not change shape.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ViewNode {
    /// An HTML element. The `tag` is the element name (e.g.
    /// `"button"`); `children` are this element's child node ids
    /// in source order. `static_attrs` carries authored attributes
    /// that the rsjs analyser does not consume itself (i.e. anything
    /// other than `on_*`, `bind_*`); each entry is a `(name, value)`
    /// pair preserved verbatim from the source so emit_rust can
    /// thread them into the generated `html_sync!` body. `server_attrs`
    /// carries explicit `rsjs::server_attr(...)` dynamic attributes that
    /// render on the server but have no client binding. `payload_attrs`
    /// carries rsjs-generated JSON payload attributes for loop-local event
    /// handler payloads.
    Element {
        /// The HTML tag name.
        tag: String,
        /// Ordered list of child node ids.
        children: Vec<NodeId>,
        /// Author-written static HTML attributes that the rsjs
        /// analyser passes through unchanged (e.g. `class="error"`,
        /// `aria-label="..."`, `id="foo"`, `type="submit"`). Entries
        /// preserve source order. Boolean attributes (`disabled`)
        /// are stored with an empty-string value.
        static_attrs: Vec<(String, String)>,
        /// Server-only dynamic attributes. The token string is the full
        /// `rsjs::server_attr(...)` call so macro-side descriptor inlining can
        /// substitute child `self.field` props before emit_rust extracts the
        /// inner expression. Source metadata is redacted provenance for
        /// diagnostics and strategy sidecars.
        server_attrs: Vec<ServerAttr>,
        /// Framework-generated dynamic payload attributes. The value is a
        /// Rust expression token string that must implement
        /// `rsjs::ClientEncode`; emit_rust serializes it as JSON at render
        /// time and the JS event handler reads it back with `JSON.parse`.
        payload_attrs: Vec<PayloadAttr>,
    },
    /// A text-binding slot: the text content here is supplied by
    /// the expression in [`Bindings::text_bindings`] keyed at this
    /// node's id. The slot itself carries no static text.
    TextBindingSlot,
    /// Literal static text from the source (e.g. `"Submit"` inside
    /// `<button>Submit</button>`). The string is stored verbatim;
    /// emit_rust writes it into the generated HTML, which handles
    /// escaping at the `html_sync!` layer.
    StaticText(String),
    /// Opaque server-rendered expression from an explicit
    /// `rsjs::server_only(...)` slot. This is intentionally a token
    /// string rather than a [`ClientExpr`]: the JS emitter must ignore it,
    /// and the Rust emitter splices it back into the generated
    /// `html_sync!` body as ordinary Proa server content.
    ServerOnly(ServerOnlyNode),
    /// A server-rendered template `for` loop whose repeated body is still
    /// represented in the view tree.
    ///
    /// `attrs`, `label`, `pat`, `expr`, and `body_prefix` are token strings
    /// captured from the source `ExprForLoop`. The macro-side Rust emitter
    /// reparses them into the generated `html_sync!` / `html!` body so the
    /// server render preserves the user's loop shape. The `children` are the
    /// analysed nodes from the loop body's tail `html_sync!` / `html!`
    /// invocation, which lets delegated event handlers be lowered even though
    /// the DOM repeats at runtime.
    ForLoop {
        /// Outer attributes on the `for` expression.
        attrs: String,
        /// Optional loop label, including the trailing colon when present.
        label: String,
        /// The source loop pattern.
        pat: String,
        /// The iterator expression after `in`.
        expr: String,
        /// Statements before the loop body's tail HTML macro.
        body_prefix: String,
        /// Analysed top-level body nodes.
        children: Vec<NodeId>,
    },
}

/// The v0.1 placeholder view tree.
///
/// The tree is a flat arena of [`ViewNode`]s plus a single root
/// pointer. NodeIds are dense indices into the arena; the `root`
/// field identifies the entry node.
///
/// This type is replaced in M7 by a reference to the lifted
/// `html_sync!` parse tree; the rest of the IR is structured so
/// that swap is a localised change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewTree {
    nodes: Vec<ViewNode>,
    root: Option<NodeId>,
}

impl ViewTree {
    /// Construct an empty tree with no root.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node and return its id. Ids are dense and monotonic.
    pub fn alloc(&mut self, node: ViewNode) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// Borrow the node with the given id.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this tree.
    pub fn get(&self, id: NodeId) -> &ViewNode {
        &self.nodes[id.0 as usize]
    }

    /// Mutably borrow the node with the given id.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this tree.
    pub fn get_mut(&mut self, id: NodeId) -> &mut ViewNode {
        &mut self.nodes[id.0 as usize]
    }

    /// The root node, if one has been set.
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Set the root node.
    pub fn set_root(&mut self, id: NodeId) {
        self.root = Some(id);
    }

    /// Number of nodes in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree holds no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// A reference to the view tree owned by an [`Island`].
///
/// In v0.1 this is just an inline [`ViewTree`]; in M7 it becomes a
/// reference into the lifted parse tree owned by `rsjs_macros`. The
/// indirection exists today so the rest of the IR can use a stable
/// name (`ViewTreeRef`) for "wherever the structural HTML lives,"
/// without spreading an in-progress migration through every
/// side-table.
pub type ViewTreeRef = ViewTree;

/// A callback adapter installed by a parent island before mounting a
/// reusable component fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentFragmentCallback {
    /// Child callback slot this adapter satisfies.
    pub slot: CallbackSlotId,
    /// Cached Client IR v3 body for parent-side callback adapter emission.
    pub(crate) body_ir: ClientBlock,
}

impl ComponentFragmentCallback {
    /// Construct a parent-side callback adapter from Client IR v3.
    pub fn from_ir(slot: CallbackSlotId, body_ir: ClientBlock) -> Self {
        Self { slot, body_ir }
    }

    /// Return this callback adapter's cached Client IR v3 body.
    pub fn body_ir(&self) -> &ClientBlock {
        &self.body_ir
    }
}

/// Alias a child component signal to a parent-owned signal.
///
/// This preserves prop-signal semantics when a child fragment is
/// mounted instead of fully inlined: child bindings subscribe directly
/// to the parent signal object, so parent updates still refresh the
/// child DOM.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentFragmentSignalAlias {
    /// Signal id used inside the child fragment module.
    pub child: SignalId,
    /// Signal id owned by the parent island.
    pub parent: SignalId,
    /// Child reactive prop field that introduced this alias, when known.
    pub prop_name: Option<String>,
    /// Parent call-site expression that supplied the reactive prop, when known.
    pub source_anchor: Option<SourceAnchor>,
}

/// Alias a copied child component node to its parent-island node id.
///
/// The parent owns the copied view-tree nodes for SSR, but the reusable
/// component fragment JS is emitted against the child's original node ids.
/// SSR therefore needs to render child-local marker values on parent-owned
/// nodes so the fragment chunk can resolve its bindings after mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentFragmentNodeAlias {
    /// Node id used inside the child fragment module.
    pub child: NodeId,
    /// Node id allocated in the parent island's copied view tree.
    pub parent: NodeId,
}

/// Runtime mount of a reusable component fragment inside a parent island.
///
/// The parent still carries the copied child view tree and bindings so the
/// Rust emitter can write correct SSR HTML and hydration markers. The JS
/// emitter treats the owned ids as fragment-owned and skips emitting the
/// duplicate child binding code in the parent module; instead it imports
/// the fragment chunk and emits one mount call.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentFragmentMount {
    /// Root DOM node of the copied child component subtree in the parent
    /// view tree.
    pub root: NodeId,
    /// Child component fragment asset id.
    pub asset_id: String,
    /// Parent-side callback adapters keyed by the child slot id.
    pub callbacks: Vec<ComponentFragmentCallback>,
    /// Child signal ids that should use parent signal cells.
    pub signal_aliases: Vec<ComponentFragmentSignalAlias>,
    /// Child-to-parent node aliases for SSR hydration markers.
    pub node_aliases: Vec<ComponentFragmentNodeAlias>,
    /// Nodes copied from the child component descriptor.
    pub owned_nodes: Vec<NodeId>,
    /// Signals copied from the child component descriptor for SSR
    /// initial-value computation. Parent JS skips these declarations.
    pub owned_signals: Vec<SignalId>,
    /// Handlers copied from the child component descriptor for marker
    /// bookkeeping. Parent JS skips these declarations.
    pub owned_handlers: Vec<EventHandlerId>,
    /// Node refs copied from the child component descriptor. Parent JS skips
    /// these declarations when the child fragment is mounted as a shared chunk.
    pub owned_node_refs: Vec<NodeRefId>,
}

impl ComponentFragmentMount {
    /// Whether this mount requires the parent island's JavaScript module.
    ///
    /// Descriptor schema v4 component fragments are parent-mounted assets:
    /// `emit_fragment(...)` exports a mount function, but it does not register
    /// a self-hydrating island wrapper or record a response-scoped child asset.
    /// Therefore even a self-contained hydrated child currently needs parent JS
    /// to import and invoke the fragment. A future independent-boundary mode can
    /// make this conditional once the compiler can emit the child's wrapper,
    /// seed blobs, and `cx.rsjs_island_used(child_asset)` from the parent SSR
    /// path.
    pub fn requires_parent_runtime(&self) -> bool {
        true
    }
}

/// The top-level rsjs IR for a single island.
///
/// All side-tables are owned here; the bindings reference signal /
/// handler / expression / node ids that resolve inside this struct.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Island {
    /// Page-scoped shared-state handles acquired by this island.
    pub shared_stores: Vec<SharedStoreDecl>,
    /// Reactive signals declared in the island. Indexed by their
    /// position in the vector; `signals[i].id == SignalId(i as u32)`
    /// is the invariant the builder maintains.
    pub signals: Vec<SignalDecl>,
    /// Event handlers declared in the island. Indexed analogously
    /// to `signals`.
    pub handlers: Vec<EventHandler>,
    /// Reusable-component callback slots referenced by handler
    /// bodies. Indexed analogously to `signals`.
    pub callback_slots: Vec<CallbackSlot>,
    /// DOM node refs declared in the island. Indexed analogously to
    /// `signals`.
    pub node_refs: Vec<NodeRefDecl>,
    /// Per-NodeId side-tables (text bindings, event bindings).
    pub bindings: Bindings,
    /// Arena holding every reactive expression referenced by the
    /// bindings.
    pub exprs: ExprArena,
    /// Reference to the view tree (placeholder in v0.1, lifted
    /// parse tree in M7).
    pub view_tree: ViewTreeRef,
    /// Whether the authored component uses `proa_core::WebRender` rather
    /// than `proa_core::WebRenderSync`. The Rust emitter preserves that
    /// trait choice while rewriting the render body through
    /// `proa_macros::html!` or `proa_macros::html_sync!`.
    ///
    /// In M5 the flag is set by the [`crate::IslandBuilder`] (the
    /// `availability_probe` golden exercises the async path); in M7
    /// the parser derives it from the canonical authored render signature.
    pub is_async: bool,
    /// Reachable client-module bundles selected by linking.
    ///
    /// New source does not declare a component allowlist. This field remains
    /// in the wire schema for linked bundle planning and backward-compatible
    /// descriptor decoding.
    pub client_module_imports: Vec<String>,
    /// Client-side trigger used by the generated island module. Defaults
    /// to [`HydrationStrategy::Immediate`] for backwards compatibility.
    pub hydration_strategy: HydrationStrategy,
    /// Asset ids of descriptor-backed rsjs component fragments that
    /// were composed into this island.
    ///
    /// Today the macro still inlines the child IR for correctness,
    /// especially for callback props that update parent-owned signals.
    /// This side table is the first shared-fragment asset-graph layer:
    /// it records which reusable component chunks the island depends on
    /// so the manifest and bundler can dedupe them across parent island
    /// types before hydration switches to direct fragment calls.
    pub component_fragment_imports: Vec<String>,
    /// Concrete component-fragment mount calls inside this island.
    pub component_fragment_mounts: Vec<ComponentFragmentMount>,
    /// Reactive queries declared in the island. Populated by M8's
    /// `query(...)` builder API and (post-M8) the parser. Indexed
    /// analogously to `signals` / `handlers`: `queries[i].id ==
    /// QueryId(i as u32)`.
    pub queries: Vec<QueryDecl>,
    /// Reactive mutations declared in the island. Populated by M9's
    /// `mutation(...)` builder API and (post-M9) the parser.
    /// Indexed analogously to `queries`: `mutations[i].id ==
    /// MutationId(i as u32)`.
    pub mutations: Vec<MutationDecl>,
    /// Interval timers declared in the island prelude via
    /// `rsjs::interval(ms, || ...);`. Intervals carry no id: nothing
    /// else in the IR references them, so they are emitted (and walked
    /// by analyses) in declaration order.
    pub intervals: Vec<IntervalDecl>,
    /// Keyed lists declared in the island body via a template `for`
    /// loop whose iterator is a signal read and whose body root
    /// carries `key={...}`. Lists carry no id: nothing else in the IR
    /// references them, so they are emitted (and walked by analyses)
    /// in declaration order. See [`crate::lists::ListDecl`].
    pub lists: Vec<ListDecl>,
}

impl Island {
    /// Construct an empty island.
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::SignalId;

    #[test]
    fn expr_arena_accepts_explicit_client_ir_v3_form() {
        let mut arena = ExprArena::new();
        let expr = arena.alloc(ClientExpr::Field {
            base: Box::new(ClientExpr::SignalRead(SignalId(0))),
            name: "value".to_string(),
        });

        assert!(matches!(
            arena.client_expr(expr),
            ClientExpr::Field { name, .. } if name == "value"
        ));
    }
}
