# Signals API

Reactive primitives inside RSJS-analyzed components. A component that declares
local state or another browser origin uses `#[rsjs(component, client)]`; a
component that only participates in an enclosing live graph can use
`#[rsjs(component)]`. `signal(initial)` creates writable `Signal<T>` state;
`memo(|| expr)` creates a read-only, cached `Memo<T>` derived value. Outside an
RSJS-analyzed body these are server-side placeholders — the macro lifts the
reactive declarations to IR before browser runtime.

## Quickref

```rust
use rsjs::{memo, signal, Memo, Signal};

let count: Signal<i64> = signal(0_i64);
let doubled: Memo<i64> = memo(|| count.get() * 2_i64);
let open = signal(false);

// In a reactive position (inside html_sync!):
{count.get()}                                  // tracked read; updates trigger re-render
{doubled.get()}                                // derived read; deps are tracked through memo

// In a handler body (the closure on an `on_*` slot):
on_click={|_| count.set(42)}                   // imperative replace
on_click={|_| count.update(|c| c + 1)}         // closure update
on_click={|_| open.set(!open.get())}           // bool toggle
```

## When to use which

| Method | Use case | Notes |
|---|---|---|
| `signal(initial)` | Declare reactive state at the top of an island's render fn | Reachable signals receive deterministic ids from declaration order; unused declarations can be removed by linking |
| `memo(\|\| expr)` | Cache a read-only value derived from live inputs | Dependencies are inferred; no derived seed value is serialized |
| `.get()` | Read in any reactive position (text slot, attribute, conditional predicate) | Auto-tracks the surrounding effect; updates re-run the effect |
| `.set(value)` | Replace the signal's value with a new one | Use in handler bodies; not in render-time computation |
| `.update(\|prev\| next)` | Compute new value from current value | Equivalent to `signal.set(closure(signal.get()))`; preferred when next depends on previous |
| `batch(\|\| { ... })` | Group several synchronous writes | Values change immediately; dependent DOM effects flush once in the next microtask |

## Memo dependencies and caching

Write `memo(|| expression)` without a manual dependency array. The compiler
walks the lowered expression, deduplicates its signal reads, and emits the
dependency list for the runtime. Reads in both arms of a supported conditional
are included. Pure client helpers receive live values through their arguments;
those argument reads contribute dependencies too.

```rust
let total = memo(|| price.get() * quantity.get());
// Generated JavaScript subscribes to both price and quantity:
// const total = memo(() => ..., [price, quantity]);
```

The browser evaluates the memo at creation and caches the current result.
Source writes schedule recomputation in the microtask flush; several writes
in one synchronous turn coalesce. Reads before that flush still return the
previous result. Subscribers run only when the new result differs according
to `Object.is`. This stores one current result, not a cache of past input
combinations. Teardown unsubscribes the memo from its inputs.

Use a memo when multiple consumers should share a cached derivation or when
unchanged derived results should suppress downstream work. An inline reactive
expression such as `{price.get() * quantity.get()}`, or a supported reactive
prelude alias, can derive a value without creating a shared cached cell.
Memo closures must be one supported reactive expression; put intermediate
locals in a pure `#[rsjs::client]` helper and call it from that expression.

## Debouncing event-driven writes

`rsjs::debounce(handler, delay_ms)` wraps a closure directly in an `on_*`
attribute. It waits for the trailing edge, passes the latest event, and cancels
pending work on island teardown:

```rust
<input on_input={rsjs::debounce(|event| query.set(event.value()), 300)} />
```

The first argument must be a closure and the delay must be an integer literal.
A named-handler reference or runtime delay expression is not supported in this
analyzed form. `rsjs::interval(ms, || ...)` keeps its delay-first order.

## Initial-value rules

The value passed to `signal(...)` is evaluated during SSR and encoded into the
seed script. Literal values stay compile-time encoded; runtime expressions use
`rsjs::ClientEncode`.

```rust
signal(0_i64)                  // integer
signal(0.0_f64)                // float
signal(false)                  // bool
signal(String::new())          // empty string
signal("hello".to_string())    // owned literal string
signal(self.initial)           // runtime field value
signal(build_initial_state())  // runtime expression; must impl ClientEncode
```

An initializer is a one-time server seed, so it cannot read an earlier client
cell:

```rust
let count = signal(1_i64);
let doubled = memo(|| count.get() * 2_i64); // live derived value
```

Writing `signal(count.get() * 2_i64)` is rejected with
`rsjs/signal-seed-client-dependency`; use an ordinary server value when the
value is truly a seed, or `memo` when it should follow another signal.

Runtime expressions that do not implement `ClientEncode` fail at the generated
impl with a normal rustc trait-bound error. If an integer expression is
ambiguous, add a suffix or turbofish, e.g. `signal::<i32>(count_from_url())`.

Typed fieldless enums can cross without converting the authoring model to
`String`:

```rust
#[derive(rsjs::ClientProps)]
enum FulfillmentMode {
    Pickup,
    Delivery,
}

signal(self.mode) // wire value is "Pickup" or "Delivery"
```

The exact variant identifier is the stable JSON string tag; numeric
discriminants are not serialized, and renaming a variant changes the wire
schema. Data-carrying variants remain a derive-time error. Enum path patterns
such as `FulfillmentMode::Pickup` are not yet supported in client-handler
`match` lowering, so typed transport and enum-pattern client control flow are
separate capabilities.

## Reading in conditional predicates

The IR understands a fixed grammar of reactive expressions. For predicates:

```rust
{if count.get() == 0          { html_sync! { ... } }}    // BinOp::Eq
{if count.get() != 0          { html_sync! { ... } }}    // BinOp::Neq
{if count.get() < 10          { html_sync! { ... } }}    // ordering
{if !active.get()             { html_sync! { ... } }}    // unary !
{if username.is_non_empty()   { html_sync! { ... } }}    // recognised method
```

`!signal.get()` lowers to `signal.get() == false` in the IR. Other unary
operators are not supported.

## Writing in handler bodies

Handler bodies are tighter: only specific shapes lower correctly. The two
canonical shapes:

```rust
// Replace
on_click={|_| count.set(42)}
on_click={|_| count.set(!watching.get())}      // bool toggle via set + get

// Update via closure
on_click={|_| count.update(|c| c + 1)}
on_click={|_| count.update(|v| !v)}            // bool toggle via update
```

A multi-statement handler body may use the supported handler statement grammar.
Use `batch` when several writes are one logical transaction:

```rust
on_click={|_| rsjs::batch(|| {
    mode.set(WorkbenchMode::Edit);
    selected_node.set(None);
    revision.update(|revision| revision + 1);
})}
```

Reads inside and immediately after the batch observe the new values. Equal
writes use the runtime's `Object.is` check and do not notify subscribers.
Nested batches share the outer boundary, including subscribers in other owners
that acquire the same SharedState scope. A thrown callback preserves completed
writes, restores scheduler depth in `finally`, schedules one outer flush, and
rethrows. Batch callbacks must be synchronous and cannot return a promise.

## What the seed looks like in SSR

Each reachable `signal(initial)` declaration contributes to the compiler-owned
seed layout. Surviving signals retain deterministic declaration order; an
unused declaration does not ship, except that a dead position may remain
`null` when a higher id must keep its alignment. For an owner with three
reachable signals declared as
`signal(false), signal(0_i64), signal(String::new())`:

```html
<script type="application/json" data-rsjs-island="myisland_<hash>">
  {"signals":[false,"0",""]}
</script>
```

The canonical decimal string preserves the `i64` exactly; typed hydration
validates it and reconstructs a BigInt, even for zero. The runtime reconstructs
each `Signal` with its SSR-time value, so the client never starts mid-flight.

## Observability across islands

Each island has its own signal namespace. `signal(...)` declarations in
`IslandA` are not visible to `IslandB`, even if they share an asset id.

For synchronous client-owned UI state shared by independent islands, derive a
typed schema and place a provider around the owning page region:

```rust
#[derive(rsjs::SharedState)]
#[rsjs(type_key = "acme.workbench")]
pub struct WorkbenchState {
    pub mode: String,
    pub button_bg: String,
    pub selected_node: Option<String>,
}

html_sync! {
    {rsjs::SharedStateProvider {
        state: WorkbenchState {
            mode: "select".into(),
            button_bg: "#2f6b4f".into(),
            selected_node: None,
        },
        children: WorkbenchChildren {},
    }}
}
```

`WorkbenchChildren` is one direct analyzed component that may render the three
consumer components internally. In v1, `state` must be a named `SharedState`
struct literal and `children` must be exactly one direct component struct
literal. A provider must be statically present; do not place provider topology
in a loop, condition, or deferred region.

Declaration order does not matter. The `SharedState` definition may sit above
or below the RSJS-analyzed impls that use it, in a later module, or in another
crate (including one with a `build.rs`), and it may be reached through a bare
`use`, a re-export, or an alias. Nothing about a shared-state island depends on
proc-macro expansion order or on the contents of `target/`.

That is possible because the macro never resolves a shared field's Rust type.
It cannot: a proc macro sees tokens, not types. The page seed preserves the
wire representation, but fixed-width arithmetic still needs explicit source
evidence for Rust range and signedness. A suffix or typed boundary can
specialize shared `.set(...)` and `.update(...)`; untyped shared arithmetic is
rejected instead of becoming representation-only arithmetic.

Dynamic ranges inside client handlers follow the same rule. Both bounds must
prove one matching `i32`, `u32`, `i64`, or `u64` domain (an integer literal may
take the other bound's proven domain). Client IR v3 carries that exact domain
on the range statement, so the induction variable keeps its Rust width and
signedness when it is passed to a standalone `#[rsjs::client]` helper.

For integer division over direct shared reads, provide the exact domain:

```rust
let per_item = rsjs::div_int::<u64>(total.get(), count.get());
```

This emits checked `u64` division. Division by zero traps before mutation, and
`::<i32>`/`::<i64>` also preserve Rust's `MIN / -1` overflow. The turbofish may
be omitted only when both operands independently prove the same supported
domain; a shared operand always requires it at every call boundary, regardless
of earlier helper uses. The type argument must be the primitive itself (or its
canonical `core::primitive::*`/`std::primitive::*` path), not a reference,
wrapper, alias, or lookalike path. Use `rsjs::div_f64(a, b)` only when
conversion to fractional Number semantics is intentional.

Each consuming island acquires the nearest provider ambiently, with no prop
threading and no shared component tree:

```rust
let workbench = rsjs::shared::<WorkbenchState>();
let button_bg = workbench.button_bg();
let green = rsjs::memo(|| button_bg.get() == "#2f6b4f");

html_sync! {
    <button on_click={|_| button_bg.set("#2f6b4f".to_string())}>
        {if green.get() { "green" } else { "other" }}
    </button>
}
```

Note the conditional around `green.get()`: a `bool` memo is read in
predicate position (`if`, or an attribute value), not as a bare text slot,
because `bool` does not implement `WebRenderSync`. String and numeric
memos may be rendered directly as text slots.

Every top-level struct field is a separate page-runtime cell. A memo that
reads `button_bg` does not subscribe to `selected_node`. Direct chains such as
`workbench.button_bg().get()` and `workbench.button_bg().set(...)` are also
supported; binding a field to a local is useful when it is read repeatedly.

Provider semantics:

- The nearest provider of the same `type_key` wins.
- A parent and child may both provide the same schema. This is intentional
  shadowing, not a duplicate-seed error; leaving the child restores the parent.
- Sibling/nested providers are independent scopes.
- All islands under one resolved scope acquire the same field-cell objects.
- Late-hydrating islands see the current live value, never a stale SSR copy.
- The provider emits no DOM wrapper.
- A provider must be above a deferred streaming boundary so its seed is in the
  shell before hydration.
- Provider declaration is server-side lexical topology and is not itself a
  client origin. `rsjs::shared::<T>()` is the origin and requires a
  `#[rsjs(component, client)]` consumer.
- A provider initializer is a seed, not ongoing synchronization. It cannot read
  an enclosing live value; initialize from static server data or update shared
  state explicitly from a handler/effect.
- Two declarations with the same `type_key` but different schema ABIs fail
  before any owner activates.
- Stores belong to the current response/navigation generation. Page teardown
  disposes their field subscriptions, pending work, and owner references; a
  later page never reuses a prior page's provider scope id.
- A provider serializes every struct field into page HTML, including fields no
  island currently reads. Treat the full schema as public client data; do not
  store secrets or server-only authorization values in `SharedState`.

Use shared state for synchronous UI ownership: selected tools, tabs, drafts,
inspector selection, or theme tokens. Use queries/mutations for asynchronous,
server-owned, stale/refetchable data. A shared field remains subject to the
island DOM boundary: sharing a value does not grant access to arbitrary foreign
DOM nodes.

## Related references

- Source-of-truth for IR `Expr` variants:
  [`rsjs_ir/src/client/expr.rs`](../../../rsjs_ir/src/client/expr.rs).
- Stub `Signal<T>` used by tests for type-checking: [`rsjs_macros/tests/parser_counter.rs:29-44`](../../../rsjs_macros/tests/parser_counter.rs).
- Real-world examples: [`proa_demo/src/ebay_srp_islands.rs`](../../../proa_demo/src/ebay_srp_islands.rs).
