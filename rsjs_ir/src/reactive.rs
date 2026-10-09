// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reactive declarations: signals, event handlers, queries, and
//! mutations.
//!
//! v0.1 ships **declarations only**. There are intentionally no
//! dependency fields: a signal does not store its dependents, an
//! event handler does not store the signals it reads or writes.
//! Dependency information is computed on demand by analyses that
//! walk the [`crate::client::ClientExpr`] tree (`analysis::dep_collector`).
//! Treating dependencies as derived rather than source-of-truth
//! keeps the IR cheaper to construct, harder to get out of sync,
//! and easier for passes to mutate. Signal dependencies are derived data,
//! never source-of-truth IR.
//!
//! Handler-like declarations ([`EventHandler`], [`IntervalDecl`],
//! [`crate::island::ComponentFragmentCallback`]) store their bodies as
//! Client IR v3 [`crate::client::ClientBlock`]s. Imperative actions
//! (signal writes, browser effects, console logging, query cache
//! actions, mutations) and structured control flow live on
//! [`crate::client::ClientStmt`]; there is no other statement IR.

use crate::client::{ClientBlock, ClientType, IntegerDomain};
use crate::island::SourceAnchor;
use crate::types::{
    CallbackSlotId, EventHandlerId, ExprId, MutationId, NodeRefId, QueryId, SharedStoreId, SignalId,
};
use crate::value::Value;

/// Declaration of a reactive signal.
///
/// The `init` value is captured server-side (where it ends up
/// embedded in the rendered HTML for hydration) and is the initial
/// value handed to the client-side runtime when the island boots.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalDecl {
    /// Identifier the rest of the IR uses to reference this signal.
    pub id: SignalId,
    /// Optional source-level local name, used only for redacted diagnostics.
    pub source_name: Option<String>,
    /// Optional source location of the `signal(...)` call, used only for
    /// diagnostics and sidecars.
    pub source_anchor: Option<SourceAnchor>,
    /// Initial value at island construction time.
    pub init: Value,
    /// Body expression of a memo (derived) cell, or `None` for a
    /// plain leaf signal.
    ///
    /// Design note: memo cells deliberately share the [`SignalId`]
    /// space with plain signals instead of introducing a parallel
    /// `MemoId`. Every existing consumer (dependency collection,
    /// binding dep arrays, `__sig_<id>` JS naming, component-inline
    /// signal remapping) keys off `SignalId`, and the runtime's
    /// `memo()` cell exposes the same `_subscribe` interface as a
    /// signal, so a derived cell slots into all of that wiring
    /// unchanged. The only places that must distinguish the two are
    /// the emitters: the JS emitter declares derived cells with
    /// `memo(() => <body>, [<deps>])` instead of
    /// `signal(signal_initial(...))`, and the seed-blob writers emit a
    /// `null` placeholder at a derived cell's index (the blob array is
    /// indexed by `SignalId`, so positions must stay aligned, but a
    /// derived value is never seeded; it is recomputed from the seeded
    /// source signals).
    pub derived: Option<ExprId>,
    /// Declared client-side value shape of this signal.
    ///
    /// This is the runtime representation on the client as determined by the
    /// Rust source domain. `i32`/`u32` signals use integral JavaScript Numbers;
    /// `i64`/`u64` signals always use BigInt, including small values such as
    /// `2_i64`. Wide runtime initializers cross the hydration boundary as
    /// canonical decimal strings and decode to BigInt before entering the
    /// signal. The analyzer fills this from literal suffixes, typed
    /// `Signal<T>` bindings, and explicit `signal::<T>(...)` arguments;
    /// [`ClientType::Unknown`] remains the honest fallback only when no
    /// supported client domain can be proven.
    ///
    /// For a memo (derived) cell this is the type of the derived
    /// body, backfilled by the whole-island typing pass
    /// (`crate::analysis::typing`) once the body expression exists in
    /// the arena.
    pub ty: ClientType,
    /// Rust fixed-width integer domain for checked client arithmetic.
    ///
    /// This is separate from [`Self::ty`]: `ty` describes the JavaScript
    /// representation, while this field records the source width and
    /// signedness needed to enforce deterministic overflow, division, and
    /// remainder behavior. `None` means the macro could not prove a supported
    /// domain; arithmetic over such a signal must be rejected rather than
    /// lowered to native JavaScript Number behavior.
    pub integer_domain: Option<IntegerDomain>,
    /// Page-scoped store field backing this signal, or `None` when the signal
    /// is owned by the island itself.
    ///
    /// Shared fields deliberately occupy the ordinary [`SignalId`] space so
    /// dependency collection, memo cells, bindings, and handler writes do
    /// not need a parallel reactive graph.
    pub shared: Option<SharedFieldRef>,
}

/// One `rsjs::shared::<T>()` acquisition in an island prelude.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStoreDecl {
    pub id: SharedStoreId,
    /// Source-level local name, used for generated Rust and diagnostics.
    pub source_name: String,
    /// Canonical Rust type tokens. The IR remains independent of `syn`; the
    /// Rust emitter retains the original `syn::Type` separately.
    pub rust_type: String,
    pub source_anchor: Option<SourceAnchor>,
}

/// Metadata connecting a signal declaration to one top-level shared-state
/// field cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFieldRef {
    pub store: SharedStoreId,
    pub field: String,
}

/// Declaration of a client-side event handler.
///
/// Each handler is a freestanding Client IR v3 block invoked by name from the
/// runtime when the bound DOM event fires; the runtime supplies referenced
/// signals through the handler's captured client environment.
#[derive(Debug, Clone, PartialEq)]
pub struct EventHandler {
    /// Identifier the rest of the IR uses to reference this handler.
    pub id: EventHandlerId,
    /// Optional source-level local or event name, used only for redacted
    /// diagnostics.
    pub source_name: Option<String>,
    /// Optional source location of the handler declaration or event attribute,
    /// used only for diagnostics and sidecars.
    pub source_anchor: Option<SourceAnchor>,
    /// Cached Client IR v3 body for analysis and JS emission.
    pub(crate) body_ir: ClientBlock,
    /// Trailing-edge debounce window in milliseconds, or `None` for an
    /// ordinary handler.
    ///
    /// When set, the JS emitter declares the handler as `const
    /// __handler_<id> = debounce((event) => { ... }, <delay_ms>);` and pushes
    /// the wrapper's `_dispose` into the island disposer list so a
    /// removed island never fires a pending trailing call. Delegation
    /// tables reference the wrapped function by the same
    /// `__handler_<id>` name, so nothing downstream of the declaration
    /// changes shape.
    pub(crate) debounce_ms: Option<u64>,
}

impl EventHandler {
    /// Construct an event handler from Client IR v3.
    pub fn from_ir(id: EventHandlerId, body_ir: ClientBlock) -> Self {
        Self {
            id,
            source_name: None,
            source_anchor: None,
            body_ir,
            debounce_ms: None,
        }
    }

    /// Construct a debounced event handler from Client IR v3. The
    /// emitted JS wraps the body with the runtime's `debounce(fn, delay_ms)`
    /// and ties the wrapper's `_dispose` to island teardown.
    pub fn from_ir_debounced(id: EventHandlerId, ms: u64, body_ir: ClientBlock) -> Self {
        Self {
            id,
            source_name: None,
            source_anchor: None,
            body_ir,
            debounce_ms: Some(ms),
        }
    }

    /// Return this handler's cached Client IR v3 body.
    pub fn body_ir(&self) -> &ClientBlock {
        &self.body_ir
    }

    /// Return this handler's trailing-edge debounce window, if any.
    pub fn debounce_ms(&self) -> Option<u64> {
        self.debounce_ms
    }
}

/// Declaration of an island-lifetime interval timer.
///
/// Declared in source as a prelude statement
/// `rsjs::interval(ms, || ...);`. The body uses the event-handler
/// statement grammar minus the event parameter (there is no event: the
/// runtime invokes the callback on a timer tick). The JS emitter lowers
/// each declaration to
/// `__rsjs_disposers.push(interval(<ms>, () => { ... }));` so island
/// teardown clears the timer.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalDecl {
    /// Tick period in milliseconds.
    pub ms: u64,
    /// Client IR v3 body executed on every tick.
    pub(crate) body_ir: ClientBlock,
}

impl IntervalDecl {
    /// Construct an interval declaration from Client IR v3.
    pub fn from_ir(ms: u64, body_ir: ClientBlock) -> Self {
        Self { ms, body_ir }
    }

    /// Return this interval's cached Client IR v3 body.
    pub fn body_ir(&self) -> &ClientBlock {
        &self.body_ir
    }
}

/// Callback slot declared by reusable rsjs component syntax.
///
/// The source-level shape is a direct call on a handler-looking self
/// field, e.g. `self.on_change(value)`. A standalone island has no
/// parent callback table, so the JS emitter lowers these as optional
/// no-op calls. The component inliner will later rewrite each slot
/// invocation to the parent handler id supplied for that callback
/// prop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackSlot {
    /// Identifier the rest of the IR uses to reference this slot.
    pub id: CallbackSlotId,
    /// Source field name without the `self.` prefix, e.g. `on_change`.
    pub name: String,
}

/// Declaration of a client-side DOM node reference.
///
/// A node ref is declared in source with `let input =
/// node_ref::<HtmlInputElement>();` and bound in the template with
/// `ref={input}`. The JS emitter lowers it to a tiny `{ current }`
/// handle whose current value is assigned during hydration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRefDecl {
    /// Identifier the rest of the IR uses to reference this node ref.
    pub id: NodeRefId,
    /// Optional source-level local name, used only for redacted diagnostics.
    pub source_name: Option<String>,
    /// Optional source location of the `node_ref(...)` call, used only for
    /// diagnostics and sidecars.
    pub source_anchor: Option<SourceAnchor>,
}

/// History behavior for URL query-string mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UrlHistoryMode {
    /// Replace the current history entry.
    Replace,
    /// Push a new history entry.
    Push,
}

/// Event-scoped file list used by
/// [`crate::client::ClientBrowserEffect::EventFileTextInto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventFileSource {
    /// `event.target.files`.
    Target,
    /// `event.dataTransfer.files`.
    DataTransfer,
}

/// Browser storage area targeted by
/// [`crate::client::ClientBrowserEffect`] storage helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageArea {
    /// `window.localStorage`.
    Local,
    /// `window.sessionStorage`.
    Session,
}

/// Built-in browser console methods exposed from handler bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConsoleLevel {
    /// `console.log(...)`.
    Log,
    /// `console.info(...)`.
    Info,
    /// `console.debug(...)`.
    Debug,
    /// `console.warn(...)`.
    Warn,
    /// `console.error(...)`.
    Error,
    /// `console.table(...)`.
    Table,
    /// `console.time(...)`.
    Time,
    /// `console.timeEnd(...)`.
    TimeEnd,
    /// `console.group(...)`.
    Group,
    /// `console.groupEnd(...)`.
    GroupEnd,
}

/// Declaration of an M8 reactive query.
///
/// A query is a reactive cache cell keyed by a structural array
/// (`["stock", id]`). The server-side initial value is embedded in
/// the page during SSR; the client fetcher runs after hydration on
/// the configured policy (interval, focus regain, manual `refetch()`).
///
/// The `key_expr`, `server_initial`, and `client_fetcher` fields are
/// [`ExprId`]s into the island's [`crate::island::ExprArena`]. v0.1
/// (M8) parks the initial-value contract behind opaque expression
/// ids: hand-built fixtures store an [`crate::client::ClientExpr::Literal`] in
/// `server_initial` and the emitter treats that literal as the
/// dehydrated initial value.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryDecl {
    /// Identifier the rest of the IR uses to reference this query.
    pub id: QueryId,
    /// Optional source-level local name, used only for redacted diagnostics.
    pub source_name: Option<String>,
    /// Optional source location of the query declaration, used only for
    /// diagnostics and sidecars.
    pub source_anchor: Option<SourceAnchor>,
    /// Expression producing the structural cache key. Typically an
    /// [`crate::client::ClientExpr::Literal`] of `Value::String("stock:AAPL")` in v0.1
    /// fixtures; M8.5+ extends this to allow signal-derived keys
    /// (`["stock", symbol_signal]`).
    pub key_expr: ExprId,
    /// Server-side initial-value expression. v0.1 contract: this
    /// must lower to a [`crate::value::Value`] through
    /// [`crate::client::ClientExpr::Literal`] for hand-built fixtures and tests. Macro
    /// generated `query_with_initial(...)` values are serialized
    /// directly from render code before dehydration.
    pub server_initial: ExprId,
    /// Client-side fetcher metadata. v0.1 stores it as an opaque
    /// [`ExprId`] in the arena: hand-built fixtures use
    /// [`crate::client::ClientExpr::Literal`] of `Value::String("/api/url")` to encode a
    /// trivial GET URL, and the JS emitter renders it as the
    /// fetcher function the runtime calls.
    pub client_fetcher: ExprId,
    /// Declared client-side value shape returned by
    /// [`crate::client::QueryAccessor::Data`].
    ///
    /// Older hand-built fixtures leave this as [`ClientType::Unknown`]. The
    /// macro frontend fills it from `let q: rsjs::Query<T> = ...` when the
    /// Rust type is syntactically visible, and future ClientProps metadata
    /// integration can populate full structural object fields.
    pub data_ty: ClientType,
    /// Refetch / staleness / retry policy.
    pub options: QueryOptions,
}

/// Per-query refetch and retry policy.
///
/// The four canonical knobs are polling, focus refetch, retry on error, and
/// retry delay. This intentionally small contract is not intended to mirror
/// every TanStack Query option.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryOptions {
    /// Refetch interval in milliseconds. `None` disables polling.
    ///
    /// Round-3 review #16: narrowed from `Option<u64>` to
    /// `Option<u32>`. `u32::MAX` ms is ~49 days; nobody polls that
    /// rarely, and the JS lowering writes the value into a literal
    /// `Number` (which loses precision past 2^53). `u32` keeps both
    /// ends honest.
    pub interval_ms: Option<u32>,
    /// Whether to refetch when the window regains focus.
    pub refetch_on_focus: bool,
    /// Whether the client runtime should read the response body with
    /// a stream reader and publish download progress metadata on the
    /// query handle.
    pub download_progress: bool,
    /// Whether a text response should update `query.data()` as
    /// chunks arrive. Only valid for client fetchers whose response
    /// decoder is `text`.
    pub stream_text: bool,
    /// Number of retries on a failed fetch before surfacing the
    /// error to the cache.
    pub retry_count: u32,
    /// Time after which the cache entry is considered stale (in
    /// milliseconds since `data_updated_at`). `0` means never stale.
    ///
    /// Round-3 review #16: narrowed from `u64` to `u32` for the
    /// same reasons as `interval_ms` above.
    pub stale_time_ms: u32,
}

impl QueryDecl {
    /// Structural equality on queries: same id, same arena ids,
    /// same options. Mirrors the dedup-pass discipline used for
    /// signals and handlers.
    pub fn structurally_eq(&self, other: &QueryDecl) -> bool {
        self.id == other.id
            && self.source_name == other.source_name
            && self.source_anchor == other.source_anchor
            && self.key_expr == other.key_expr
            && self.server_initial == other.server_initial
            && self.client_fetcher == other.client_fetcher
            && self.data_ty == other.data_ty
            && self.options == other.options
    }
}

/// Declaration of an M9 reactive mutation.
///
/// A mutation is an imperative cache write keyed by a structural
/// array (`["update_cart", userId]`). Unlike a query (which fetches
/// data on a refetch policy), a mutation is fired explicitly by an
/// event handler via [`crate::client::ClientStmt::RunMutation`]. The mutation
/// posts to its `client_runner` URL with the user-supplied args and,
/// on success, walks the configured `invalidates` list and refetches
/// every cache entry whose key matches.
///
/// The `key_expr`, `server_runner`, `client_runner`, and the
/// per-element `invalidates` field are [`ExprId`]s into the island's
/// [`crate::island::ExprArena`]. v0.1 (M9) parks the actual runner
/// contract behind opaque expression ids: hand-built fixtures store
/// an [`crate::client::ClientExpr::Literal`] in `client_runner` and the JS runtime treats
/// that literal as the URL+method config. The full server-runner
/// API (a closure that closes over `cx`) lands alongside the M8.5
/// fetcher contract.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationDecl {
    /// Identifier the rest of the IR uses to reference this mutation.
    pub id: MutationId,
    /// Optional source-level local name, used only for redacted diagnostics.
    pub source_name: Option<String>,
    /// Optional source location of the mutation declaration, used only for
    /// diagnostics and sidecars.
    pub source_anchor: Option<SourceAnchor>,
    /// Expression producing the structural mutation key. Typically an
    /// [`crate::client::ClientExpr::Literal`] of `Value::String("update_cart")` in v0.1
    /// fixtures.
    pub key_expr: ExprId,
    /// Server-side runner expression (a stub in v0.1; M9.5+ replaces
    /// with a real cross-crate runner closure that closes over `cx`).
    pub server_runner: ExprId,
    /// Client-side runner metadata. v0.1 stores it as an opaque
    /// [`ExprId`] in the arena: hand-built fixtures use
    /// [`crate::client::ClientExpr::Literal`] of `Value::String("/api/url")` to encode the
    /// POST endpoint, and the JS emitter renders it as the runner
    /// config the runtime calls. M9.5+ extends to the full
    /// `ClientFetcher` struct (method=Post/Put/Delete, body, headers).
    pub client_runner: ExprId,
    /// Query keys to invalidate on successful mutation. Each element
    /// is an [`ExprId`] of a key expression with the same shape as
    /// [`QueryDecl::key_expr`]. The runtime prefix-matches every
    /// cache entry against this list and refetches the survivors.
    pub invalidates: Vec<ExprId>,
    /// Per-mutation retry / optimistic-update / invalidation policy.
    pub options: MutationOptions,
}

/// Per-mutation policy.
///
/// The canonical knobs are the retry budget and invalidation strategy.
///
/// Optimistic updates were carried as an `optimistic_update:
/// Option<ExprId>` field in earlier drafts but were never lowered by
/// `emit_js`, AND the runtime applied the same optimistic value to
/// every invalidated key (incorrect for any non-trivial mutation
/// where each invalidation target needs its own optimistic shape).
/// Rather than ship a half-wired contract, the field is removed and
/// proper optimistic-update support (per-key shape + rollback
/// semantics) lands in M9.5. TODO(M9.5).
#[derive(Debug, Clone, PartialEq)]
pub struct MutationOptions {
    /// Number of retries on a failed run before surfacing the
    /// error to the mutation state slot.
    pub retry_count: u32,
    /// What to do with invalidated cache entries on success.
    pub on_success_invalidate_strategy: InvalidationStrategy,
}

/// What to do with cache entries the mutation invalidates on success.
///
/// `Refetch` is the default and matches tanstack-query semantics:
/// every matching entry refetches its data via its configured client
/// fetcher. `Reset` is the more aggressive choice: every matching
/// entry is reset to the `idle` state and any subscribers see
/// `undefined` until a fresh fetch completes; useful when the
/// previous data is known to be invalid (e.g. user logged out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidationStrategy {
    /// Refetch every matching cache entry. Default.
    Refetch,
    /// Reset every matching cache entry to `idle`.
    Reset,
}

impl Default for MutationOptions {
    fn default() -> Self {
        Self {
            retry_count: 0,
            on_success_invalidate_strategy: InvalidationStrategy::Refetch,
        }
    }
}

impl MutationDecl {
    /// Structural equality on mutations: same id, same arena ids,
    /// same options. Mirrors the dedup-pass discipline used for
    /// signals, handlers, and queries.
    pub fn structurally_eq(&self, other: &MutationDecl) -> bool {
        self.id == other.id
            && self.source_name == other.source_name
            && self.source_anchor == other.source_anchor
            && self.key_expr == other.key_expr
            && self.server_runner == other.server_runner
            && self.client_runner == other.client_runner
            && self.invalidates == other.invalidates
            && self.options == other.options
    }
}
