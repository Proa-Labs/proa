// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Primitive identifier types used to key the side-tables.
//!
//! Each identifier is a thin newtype around a `u32`. They are
//! deliberately not interchangeable: a `NodeId` cannot be passed
//! where a `SignalId` is expected, even though they share the same
//! representation. This is the v0.1 surface; more identifier types
//! (e.g. `MemoId`, `QueryId`) land in later milestones.
//!
//! All identifiers are dense, monotonic indices: the builder hands
//! them out in allocation order starting at `0`. This invariant lets
//! the emitters and analyses use `Vec<T>`-backed storage when they
//! want random access (see [`crate::island::ExprArena`]) without
//! reaching for a hashmap.

/// Identifier for a [`crate::reactive::SignalDecl`] inside an
/// [`crate::island::Island`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub u32);

/// Identifier for a page-scoped shared-state acquisition inside an island.
///
/// Multiple [`SignalId`] declarations may reference fields of the same store.
/// At hydration time this id indexes the island's resolved provider bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SharedStoreId(pub u32);

/// Identifier for an [`crate::reactive::EventHandler`] inside an
/// [`crate::island::Island`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventHandlerId(pub u32);

/// Identifier for a callback slot declared by a reusable component.
///
/// Callback slots are not DOM event handlers. They are named places
/// where an inlined child component can invoke parent-owned behavior
/// (`self.on_change(value)`). The parent component inliner rewrites
/// these ids to concrete [`EventHandlerId`]s once callback props are
/// wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallbackSlotId(pub u32);

/// Identifier for a [`crate::reactive::NodeRefDecl`] inside an
/// [`crate::island::Island`].
///
/// Node refs are authoring handles declared with `node_ref::<T>()` and
/// bound to DOM elements via `ref={handle}`. They follow the same dense,
/// monotonic allocation discipline as signals and handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeRefId(pub u32);

/// Identifier for a node in the lifted `html_sync!` parse tree.
///
/// Compiler frontends assign these ids to nodes in the lifted tree. The
/// programmatic builder follows the same dense allocation convention for
/// hand-constructed fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Identifier for an [`crate::client::ClientExpr`] inside an
/// [`crate::island::ExprArena`].
///
/// Expressions are stored in an arena (a `Vec<Expr>`) and referenced
/// by index. This keeps the `Expr` enum's `Box`-ed children compact
/// and makes shared subtrees (e.g. multiple text bindings reading
/// the same signal) cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// Identifier for a local binding inside an event-handler body.
///
/// Handler locals are scoped by the generated JavaScript function/block, not
/// by the island side tables. The macro frontend allocates ids for stable
/// emitted names (`__local_<id>`) and descriptor remapping rewrites
/// child-component local ids when inlining reusable rsjs components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Identifier for a [`crate::reactive::QueryDecl`] inside an
/// [`crate::island::Island`].
///
/// M8: queries are reactive cache cells keyed by a structural array
/// (`["stock", id]`). Each query declaration in an island gets a
/// dense, monotonic id used by [`crate::client::ClientExpr::QueryRead`] and
/// the JS runtime to address its cache entry. Shares the same dense-
/// id discipline as [`SignalId`] / [`EventHandlerId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryId(pub u32);

/// Identifier for a [`crate::reactive::MutationDecl`] inside an
/// [`crate::island::Island`].
///
/// M9: mutations are imperative cache writes keyed by a structural
/// array (`["update_cart", userId]`). Each mutation declaration in
/// an island gets a dense, monotonic id used by
/// [`crate::client::ClientExpr::MutationRead`], by the
/// [`crate::client::ClientStmt::RunMutation`] handler statement, and
/// by the JS runtime to address the per-mutation
/// state slot. Shares the same dense-id discipline as
/// [`QueryId`] / [`SignalId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MutationId(pub u32);
