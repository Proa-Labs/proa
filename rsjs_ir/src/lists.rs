// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Keyed list declarations: client-side reconciliation over a
//! `Vec`-valued signal.
//!
//! A keyed list is authored as a template `for` loop whose iterator is
//! a signal read and whose body root carries a required `key={...}`
//! attribute:
//!
//! ```text
//! <ul>
//!     {for item in items.get() {
//!         html_sync! { <li key={item}>{item}</li> }
//!     }}
//! </ul>
//! ```
//!
//! The declaration is intentionally self-contained: the v0.1 keyed-list
//! body grammar is restricted to static markup, item-derived attributes,
//! and text slots reading the loop variable (or one of its fields), so
//! the item template lives here as a small standalone tree instead of
//! weaving new expression variants through the shared
//! [`crate::client::ClientExpr`] surface. The SSR half still renders through
//! the view tree's
//! [`crate::island::ViewNode::ForLoop`] replay; this declaration is what
//! the JS emitter lowers to a runtime `bind_list(...)` call with
//! `key` / `create` / `adopt` callbacks.

use crate::types::{ExprId, NodeId, SignalId};

/// Reactive source that drives a keyed list.
///
/// Existing signal-authored lists keep the direct [`SignalId`] shape so
/// older analyzer paths can preserve the exact `__sig_N.get()` getter
/// emission. Query-backed and memo list sources use an arena
/// expression instead; dependency collection then discovers any signal,
/// query, or mutation handles read by that expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListSource {
    /// A `Vec`-valued signal, e.g. `items.get()`.
    Signal(SignalId),
    /// A `Vec`-valued arena expression, e.g. `query.data().rows`.
    Expr(ExprId),
}

impl ListSource {
    pub const fn signal(id: SignalId) -> Self {
        Self::Signal(id)
    }

    pub const fn expr(id: ExprId) -> Self {
        Self::Expr(id)
    }
}

/// An expression over the current loop item, usable as a list key or a
/// text slot. This is the entire v0.1 item-expression grammar: the
/// item itself (`Vec<String>` / `Vec<i64>` elements) or a single named
/// field on it (`Vec<T: ClientProps>` elements).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListItemExpr {
    /// The loop variable itself, e.g. `key={item}` / `{item}`.
    Item,
    /// A named field on the loop variable, e.g. `key={row.id}` /
    /// `{row.title}`.
    ItemField(String),
}

impl ListItemExpr {
    /// Lower to the JavaScript expression evaluated against the
    /// runtime's `item` callback parameter.
    pub fn js(&self) -> String {
        match self {
            ListItemExpr::Item => "item".to_string(),
            ListItemExpr::ItemField(field) => format!("item.{field}"),
        }
    }
}

/// One node of a keyed-list item template, mirrored on the client as
/// imperative `document.createElement` / `setAttribute` /
/// `textContent` calls.
///
/// Invariant maintained by the frontend: an element's children are
/// either all [`ListTemplateNode::Element`]s, or all text-level nodes
/// ([`ListTemplateNode::Text`] / [`ListTemplateNode::Slot`]). Mixing
/// the two is rejected at lowering time so the per-item update
/// function can re-apply a slot-bearing element's full `textContent`
/// (static prefix plus slot values) without tracking individual DOM
/// text nodes, and so SSR-adopted nodes can be re-resolved through
/// `Element.children` index walks (which skip text nodes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListTemplateNode {
    /// An HTML element with static and item-derived attributes.
    Element {
        /// Tag name, e.g. `"li"`.
        tag: String,
        /// Author-written static attributes, in source order. Boolean
        /// attributes are stored with an empty value.
        static_attrs: Vec<(String, String)>,
        /// Dynamic attributes whose value reads the current loop item
        /// directly (or one field on it), e.g. `href={row.url}`.
        dynamic_attrs: Vec<(String, ListItemExpr)>,
        /// Child template nodes (see the type-level invariant).
        children: Vec<ListTemplateNode>,
    },
    /// Literal static text.
    Text(String),
    /// A dynamic text slot reading the loop item.
    Slot(ListItemExpr),
}

impl ListTemplateNode {
    /// Whether this subtree contains at least one dynamic slot.
    pub fn has_slot(&self) -> bool {
        match self {
            ListTemplateNode::Slot(_) => true,
            ListTemplateNode::Text(_) => false,
            ListTemplateNode::Element {
                dynamic_attrs,
                children,
                ..
            } => !dynamic_attrs.is_empty() || children.iter().any(Self::has_slot),
        }
    }
}

/// Declaration of one keyed list inside an island.
///
/// SSR renders the initial items through the view tree's `ForLoop`
/// replay (each item root carrying `data-rsjs-key`); the JS emitter
/// lowers this declaration to one `bind_list(container, getter,
/// { key, create, adopt, deps })` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDecl {
    /// The container element whose children the runtime `bind_list`
    /// owns. Carries the `data-rsjs-list="<NodeId>"` marker; the
    /// frontend guarantees the loop is its only child.
    pub container: NodeId,
    /// The [`crate::island::ViewNode::ForLoop`] node that replays the
    /// loop during SSR.
    pub loop_node: NodeId,
    /// The analysed item-template root element in the view tree
    /// (a child of `loop_node`); emit_rust attaches the SSR
    /// `data-rsjs-key` attribute here.
    pub item_root: NodeId,
    /// The `Vec`-valued reactive source driving the list.
    pub source: ListSource,
    /// The required `key={...}` expression over the loop item.
    pub key: ListItemExpr,
    /// Rust tokens of the key expression as the user wrote it
    /// (`item`, `row.id`); emit_rust splices these into the SSR
    /// `data-rsjs-key` attribute so the server and client stringify
    /// the same value.
    pub key_tokens: String,
    /// The client-side item template.
    pub item: ListTemplateNode,
}
