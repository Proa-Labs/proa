// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Frontend expression shape: the analyzer-internal AST for reactive
//! expressions.
//!
//! This is *not* an rsjs IR. The single expression IR is Client IR v3
//! ([`ClientExpr`](rsjs_ir::client::ClientExpr)); islands, analyses, and
//! emitters only ever see `ClientExpr`. `FrontendExpr` exists so the
//! analyzer (`reactive_expr`, `handlers`, `queries`, `browser`) can lower
//! `syn` trees into a small builder-friendly shape before the one-way,
//! construction-time conversion in
//! [`client_lowering::lower_handler_ir_expr_to_client_expr`]
//! (super::client_lowering) produces the `ClientExpr` stored at the
//! [`rsjs_ir::IslandBuilder::expr`] arena boundary.
//!
//! Notable differences from `ClientExpr`:
//!
//! - No expression-level signal writes: `signal.set(...)` and
//!   `signal.update(...)` are statements in Client IR v3, and the
//!   analyzer lowers them straight to `ClientStmt` shapes.
//! - No `Unary` / `Field` / `Index` / general `Call` variants: the
//!   analyzer encodes those source forms through the constrained
//!   variants below (or rejects them with a diagnostic).

use rsjs_ir::abi::HelperCallEdgeId;
use rsjs_ir::{
    AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventValue, JsonPointer, LocalId, LocationField, MutationAccessor,
    MutationId, NodeRefAccessor, NodeRefId, QueryAccessor, QueryId, SignalId, StorageValueType,
    StringPredicateOp, StringTransformOp, Value, WindowField,
};

/// A reactive expression at the v0.1 surface.
///
/// See the module-level docs for what is and isn't included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrontendExpr {
    /// A literal value. Compiles to a JS literal on the client.
    Literal(Value),

    /// A literal array of scalar values. Compiles to a JS array
    /// literal (`["a", "b"]`) on the client.
    ///
    /// v0.1 surface: produced only for `signal.set(vec![...])` inside
    /// event handlers, where every element is a literal (the keyed-list
    /// milestone's way of writing a `Vec`-valued signal). Elements are
    /// [`Value`]s rather than sub-expressions on purpose: it keeps the
    /// variant out of every recursive walk until a use case demands
    /// computed elements.
    ArrayLit(Vec<Value>),

    /// Read the current value of a signal. Lowers to a tracked
    /// `signal.get()` call in JS, registering the surrounding
    /// effect as a subscriber.
    SignalRead(SignalId),

    /// Read a handler-local binding declared by `let name = expr;`.
    ///
    /// Locals are only produced for event-handler bodies. They are not stored
    /// in the island expression arena for reactive text/attribute bindings.
    Local(LocalId),

    /// Read a whitelisted field from the DOM event currently being
    /// handled. These expressions are only valid inside event-handler
    /// bodies; the macro frontend enforces that by only constructing
    /// them when lowering the handler's event parameter.
    EventValue(EventValue),

    /// The compiler-selected element on which the active handler was
    /// declared. This opaque handle may only be kept in a handler local and
    /// used as the receiver of a generated scalar DOM read.
    EventCurrentTarget,

    /// Read the length of a whitelisted event-scoped browser collection.
    ///
    /// This is the first resource/collection-shaped primitive for the
    /// richer P0 browser API surface. It keeps raw DOM collection objects
    /// out of user-authored rsjs code while still allowing components to
    /// reason about multiple touches/files/items.
    EventCollectionLen(EventCollection),

    /// Read a scalar field from an indexed event-scoped browser collection.
    ///
    /// The `index` expression is lowered through the normal rsjs expression
    /// path and coerced by the JS emitter to an integer array index. Missing
    /// collections, out-of-range indices, and absent fields fall back to the
    /// field's typed empty value.
    EventCollectionRead {
        /// Event collection being inspected.
        collection: EventCollection,
        /// Index expression used to select the item.
        index: Box<FrontendExpr>,
        /// Scalar field read from the selected item.
        field: EventCollectionField,
    },

    /// Read a whitelisted value from a bound DOM node ref.
    NodeRefRead {
        /// Node ref being read.
        id: NodeRefId,
        /// Which DOM projection to read.
        accessor: NodeRefAccessor,
    },

    /// Read a scalar DOM projection from an opaque compiler handle.
    DomRead {
        /// Handler-local current-target handle being inspected.
        target: Box<FrontendExpr>,
        /// Generated DOM projection selected by the shared registry.
        accessor: NodeRefAccessor,
    },

    /// Test whether the active DOM event target is inside or outside a
    /// bound node ref.
    ///
    /// Constructed from `event.target_inside(root)` and
    /// `event.target_outside(root)` inside handler expressions. This
    /// keeps outside-click logic in the constrained IR instead of
    /// forcing component authors back to raw JavaScript.
    EventTargetWithinNodeRef {
        /// Node ref whose current element is used as the containment root.
        id: NodeRefId,
        /// `true` for `target_inside`, `false` for `target_outside`.
        inside: bool,
    },

    /// Read a static attribute from the DOM event target, from the
    /// nearest ancestor carrying that attribute, or from the matched
    /// delegated handler element.
    EventAttribute {
        /// Which DOM node to read from.
        source: EventAttributeSource,
        /// Static HTML attribute name.
        attr: String,
    },

    /// Read a whitelisted browser-global value. These expressions are
    /// intentionally narrow: browser state is observable client-side
    /// data, so each supported read gets an explicit variant instead
    /// of opening arbitrary `window` / `document` access.
    BrowserRead(BrowserRead),

    /// Read a server-encoded JSON payload from the DOM element that
    /// handled the current delegated event.
    ///
    /// The macro frontend emits this for loop-local handler payloads
    /// such as `self.on_change(day.date)` inside a template `for` loop.
    /// The server render writes the payload into `attr` on each repeated
    /// element; the JS handler finds the closest matching element and
    /// `JSON.parse`s the attribute value at event time.
    EventPayload {
        /// Attribute name containing the JSON payload.
        attr: String,
    },

    /// Payload passed from a child component fragment callback into
    /// the parent callback adapter.
    ///
    /// This variant is not author-facing syntax. The macro constructs
    /// it when lowering `rsjs::handler(|payload| ...)` props for a
    /// reusable component fragment. The JS emitter lowers it to the
    /// generated callback function's `__payload` parameter.
    CallbackPayload,

    /// Read a server-encoded JSON payload from the DOM node currently being
    /// bound by a repeated text/attribute binding.
    ///
    /// Unlike [`FrontendExpr::EventPayload`], this is not tied to an event. The JS
    /// emitter supplies the repeated node variable while wiring each element
    /// returned by `querySelectorAll`.
    NodePayload {
        /// Attribute name containing the JSON payload.
        attr: String,
        /// Original Rust expression tokens. The Rust emitter reparses these
        /// for SSR initial-value computation.
        tokens: String,
    },

    /// A binary operator over two reactive sub-expressions. Numeric
    /// arithmetic and comparisons are over numbers; logical
    /// operators are short-circuiting over booleans.
    BinaryOp {
        /// The operator.
        op: BinOp,
        /// Left-hand operand.
        lhs: Box<FrontendExpr>,
        /// Right-hand operand.
        rhs: Box<FrontendExpr>,
    },

    /// Checked unary negation in one proven signed fixed-width domain.
    UnaryNeg {
        domain: rsjs_ir::IntegerDomain,
        expr: Box<FrontendExpr>,
    },

    /// Value-level conditional expression. Lowers to a JavaScript
    /// ternary expression and to a normal Rust `if` expression during
    /// SSR initial-value emission.
    Conditional {
        /// Predicate expression.
        condition: Box<FrontendExpr>,
        /// Expression returned when the predicate is truthy.
        then_expr: Box<FrontendExpr>,
        /// Expression returned when the predicate is falsy.
        else_expr: Box<FrontendExpr>,
    },

    /// Non-emptiness predicate. Lowers to a JS check that returns
    /// `true` when `target` resolves to a non-empty string, a non-
    /// empty array, or a plain object with at least one own property.
    /// `null` / `undefined` count as empty. Booleans and numbers also
    /// count as empty (they have no notion of length); the predicate
    /// is intended for the M5 form fixture's `field.is_non_empty()`
    /// pattern, which always targets a string-typed signal.
    ///
    /// The input accepts any supported expression rather than only a
    /// projection chain, so callers can pass a [`FrontendExpr::SignalRead`]
    /// directly.
    IsNonEmpty {
        /// Sub-expression whose result is checked for non-emptiness.
        target: Box<FrontendExpr>,
    },

    /// Lowercase a string-like expression. Lowers to
    /// `String(target ?? "").toLowerCase()` in JS so nullish values
    /// become an empty string instead of throwing.
    StringToLowercase {
        /// Sub-expression whose string form is lowercased.
        target: Box<FrontendExpr>,
    },

    /// Check whether one string-like expression contains another.
    /// Lowers to `String(haystack ?? "").includes(String(needle ?? ""))`.
    StringContains {
        /// Sub-expression to search in.
        haystack: Box<FrontendExpr>,
        /// Sub-expression to search for.
        needle: Box<FrontendExpr>,
    },

    /// Apply a unary string transform (uppercase, trim) to a
    /// string-like expression. Lowers to
    /// `String(target ?? "").<method>()` in JS so nullish values
    /// become an empty string instead of throwing.
    StringTransform {
        /// Which transform to apply.
        op: StringTransformOp,
        /// Sub-expression whose string form is transformed.
        target: Box<FrontendExpr>,
    },

    /// Check a string-like expression against a prefix/suffix
    /// predicate. Lowers to
    /// `String(haystack ?? "").<method>(String(needle ?? ""))`.
    StringPredicate {
        /// Which predicate to apply.
        op: StringPredicateOp,
        /// Sub-expression to test.
        haystack: Box<FrontendExpr>,
        /// Prefix or suffix sub-expression.
        needle: Box<FrontendExpr>,
    },

    /// Built-in browser encoding / binary transform.
    ///
    /// This is intentionally a narrow unary primitive rather than
    /// arbitrary `TextEncoder` / `TextDecoder` access. WebAuthn,
    /// WebCrypto, and non-JSON fetch bodies need byte/string codecs,
    /// but rsjs still wants a clear, typed surface with predictable
    /// JS output.
    Encoding {
        /// Which codec operation to apply.
        op: EncodingOp,
        /// Input expression.
        value: Box<FrontendExpr>,
    },

    /// Structural query/mutation cache key parts.
    ///
    /// This is emitted as a JavaScript array literal and is intended for the
    /// `rsjs::QueryKey::new((...))` authoring surface. Legacy string keys stay
    /// represented as a normal [`FrontendExpr::Literal`] and are wrapped by the query
    /// emitter as a one-part structural key.
    QueryKey(Vec<FrontendExpr>),

    /// Read a value from a query's cache entry. The `accessor`
    /// selects which projection of the query state we want
    /// ([`QueryAccessor::Data`], [`QueryAccessor::Status`],
    /// [`QueryAccessor::IsLoading`], etc.).
    ///
    /// M8 surface: lowers to `__rsjs_query_<id>.<accessor>()` in
    /// JS and to a hand-built initial-value lookup on the server
    /// (see `rsjs_macros::emit_rust`).
    QueryRead {
        /// Query whose cache entry is being read.
        id: QueryId,
        /// Which projection of the query's state to return.
        accessor: QueryAccessor,
    },

    /// JSON-Pointer projection over a query result (or other
    /// JSON-shaped value). The path is parsed from a JSON
    /// Pointer string (RFC 6901) at IR-construction time.
    ///
    /// Lowers to a per-segment walk in JS (`v == null ? <empty>
    /// : v[seg]`); never panics on shape mismatch.
    Project {
        /// The expression producing the value to project into.
        target: Box<FrontendExpr>,
        /// The JSON Pointer path.
        path: JsonPointer,
    },

    /// Type-asserting projection terminator. Returns the projected
    /// value cast to `ty`, or a type-appropriate empty value if
    /// the projection is missing or shape-mismatched.
    AsType {
        /// The expression to cast.
        target: Box<FrontendExpr>,
        /// Which type to assert.
        ty: AsType,
    },

    /// Non-null / non-undefined predicate. Lowers to a JS check
    /// that returns `true` only when the projected value is
    /// present (not `null`, not `undefined`, not missing).
    IsPresent {
        /// Sub-expression whose result is checked for presence.
        target: Box<FrontendExpr>,
    },

    /// Structural equality against a literal `Value`. Distinct
    /// from [`FrontendExpr::BinaryOp`] with [`BinOp::Eq`]: this variant is
    /// the projection-chain terminator that takes a [`Value`]
    /// literal directly (matching `.eq(value)` in the authoring
    /// shape).
    Eq {
        /// Sub-expression to compare.
        target: Box<FrontendExpr>,
        /// Literal value to compare against.
        value: Value,
    },

    /// Inverse of [`FrontendExpr::Eq`]: structural inequality against a
    /// literal.
    Neq {
        /// Sub-expression to compare.
        target: Box<FrontendExpr>,
        /// Literal value to compare against.
        value: Value,
    },

    /// Explicit fallback for projections. Returns the projected
    /// value if present-and-correct-shape, otherwise the
    /// `fallback` literal.
    Coalesce {
        /// Sub-expression to evaluate.
        target: Box<FrontendExpr>,
        /// Literal fallback when the projection is missing or
        /// shape-mismatched.
        fallback: Value,
    },

    /// Read a value from a mutation's state slot. The `accessor`
    /// selects which projection of the mutation state we want
    /// ([`MutationAccessor::Status`], [`MutationAccessor::IsPending`],
    /// [`MutationAccessor::Data`], etc.).
    ///
    /// M9 surface: lowers to `__rsjs_mutation_<id>.<accessor>()` in
    /// JS. There is no server-side dehydrated state for mutations
    /// (mutations always boot in the `idle` state on hydration); the
    /// emit_rust pass renders nothing for `MutationRead` reads
    /// during initial-value computation, falling back to the typed
    /// default.
    MutationRead {
        /// Mutation whose state slot is being read.
        id: MutationId,
        /// Which projection of the mutation's state to return.
        accessor: MutationAccessor,
    },

    /// Direct `text!` formatting in a render slot; SSR keeps the streaming macro.
    Text {
        template: String,
        plan: String,
        args: Vec<FrontendExpr>,
        attribute: bool,
    },

    /// A call to a `#[rsjs::client]` helper.
    ///
    /// Every lowered source occurrence receives its own dense `edge`. The
    /// analyzer retains `authored_path` exactly so generated Rust can let rustc
    /// resolve same-module names, imports, renamed imports, and re-exports.
    ///
    /// Helper eligibility comes from the resolved definition's
    /// `#[rsjs::client]` annotation.
    ClientCall {
        /// Dense source call-site id inside this component descriptor.
        edge: HelperCallEdgeId,
        /// Canonical token spelling of the authored Rust call path.
        authored_path: String,
        /// The helper function name (the trailing path segment of
        /// the call site).
        name: String,
        /// Argument expressions in source order.
        args: Vec<FrontendExpr>,
    },
}

/// Whitelisted browser-global reads exposed to reactive expressions and
/// handler bodies.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BrowserRead {
    /// Read a numeric field from `window`.
    Window(WindowField),
    /// Read a string field from `window.location`.
    Location(LocationField),
    /// Read an item from `localStorage` / `sessionStorage`, returning
    /// `fallback` when the key is missing or the storage area throws.
    Storage {
        /// Storage area to read from.
        area: BrowserStorageArea,
        /// How the stored string should be interpreted.
        value_type: StorageValueType,
        /// Storage key expression.
        key: Box<FrontendExpr>,
        /// Fallback value. `None` means empty string.
        fallback: Option<Box<FrontendExpr>>,
    },
    /// Read one value from the current page URL's query string.
    ///
    /// Lowers to `new URLSearchParams(window.location.search).get(key)`.
    /// `fallback` is returned when the key is missing, URLSearchParams is
    /// unavailable, or the browser throws.
    UrlSearchParam {
        /// Query-string key expression.
        key: Box<FrontendExpr>,
        /// Fallback value. `None` means empty string.
        fallback: Option<Box<FrontendExpr>>,
    },
    /// A read declared in `rsjs_ir::browser_registry` with a
    /// [`HelperLowering::DeclaredRead`] lowering: the registry entry
    /// carries the JS template, so no dedicated variant is needed.
    ///
    /// [`HelperLowering::DeclaredRead`]: rsjs_ir::browser_registry::HelperLowering::DeclaredRead
    Declared {
        /// Stable registry id (`<module>.<method>`).
        helper: &'static str,
        /// Lowered reactive arguments in source order.
        args: Vec<FrontendExpr>,
    },
}

/// The operators allowed inside [`FrontendExpr::BinaryOp`] in v0.1.
///
/// Arithmetic operators are over numbers only. Comparison operators
/// are over numbers (or booleans for `Eq`/`Neq`). Logical operators
/// are short-circuiting over booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BinOp {
    /// Fixed-width Rust integer arithmetic with deterministic checked
    /// semantics in both generated Rust and JavaScript.
    Checked {
        op: rsjs_ir::CheckedIntegerOp,
        domain: rsjs_ir::IntegerDomain,
    },
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `/` over integer operands. Rust integer division truncates
    /// toward zero, but the bare JS `/` operator produces a fractional
    /// Number, so the analyzer selects this variant when both operands
    /// infer as integers and the JS emitter wraps the quotient in
    /// `Math.trunc(...)`. Server-side Rust emission is a plain `/`
    /// (Rust truncates natively).
    DivInt,
    /// `%`
    Rem,
    /// `==`
    Eq,
    /// `!=`
    Neq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `&&` (short-circuiting)
    And,
    /// `||` (short-circuiting)
    Or,
    /// Float `/` with a representation-pinned operand. Lowers to `num_div`.
    /// This is produced only by the explicit `rsjs::div_f64(...)` boundary.
    DynDiv,
    /// `==` with a representation-pinned operand. Lowers to `num_eq`, because
    /// a bare `===` is `false` for `5n === 5` while Rust `5 == 5` is `true`.
    DynEq,
    /// `!=` with a representation-pinned operand. Lowers to `num_ne`.
    DynNe,
}
