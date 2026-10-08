# rsjs

Compile interactive Proa components written in Rust into server-rendered HTML
and small, route-planned JavaScript.

## Status

RSJS is actively developed and already supports signals, event handlers,
bindings, queries, mutations, typed shared state, browser helpers, and linked
component composition. The supported client language is intentionally smaller
than general Rust.

Use these as the current sources of truth:

- [`STATUS.md`](./STATUS.md) tracks implemented, partial, and unsupported work.
- [`KNOWN_LIMITATIONS.md`](./KNOWN_LIMITATIONS.md) lists user-visible gaps.
- [The RSJS authoring guide](../docs/agent/rsjs-islands.md) documents the
  canonical component shapes.
- [The slot grammar](../docs/agent/api-reference/slot-grammar.md) defines what
  may appear inside analyzed template slots.
- The [passing](../rsjs_macros/tests/ui/conformance/pass) and
  [failing](../rsjs_macros/tests/ui/conformance/fail) conformance fixtures are
  the executable language contract.

## What RSJS does

An RSJS component is a normal Proa render-trait implementation. It renders HTML
on the server through the same `WebContext` as every other component. The RSJS
attribute additionally analyzes browser-reachable work and emits the client
graph needed to make that HTML interactive.

- `#[rsjs(component)]` makes a component available to linked RSJS composition
  without creating browser work by itself.
- `#[rsjs(component, client)]` permits local browser origins such as signals,
  handlers, queries, mutations, shared-state acquisition, node refs, and
  browser APIs.
- An analyzed component with no reachable browser roots remains server-only and
  emits no island wrapper or JavaScript.
- The body is still `html_sync!` or `html!`; there is no separate `rsjs!` body
  macro.

The ordinary traits are `WebRenderSync<L>` and `WebRender<L>`. Their omitted
writer parameter defaults to the reusable `RenderBuffer` owned by
`WebContext<L>`, so normal components do not carry an output lifetime or name a
writer. Low-level integrations that genuinely need a custom writer may use the
advanced `WebRenderSync<L, B>` / `WebRender<L, B>` and `WebContext<L, B>` forms.

Use `signal(initial)` for writable state and `memo(|| expression)` for a
read-only cached derivation. Memo dependencies are inferred by the compiler;
there is no manual dependency array. Input writes schedule batched
recomputation, and unchanged results suppress downstream notifications.
See the [signals reference](../docs/agent/api-reference/signals.md) for details.

Event attributes can debounce a closure with
`on_input={rsjs::debounce(|event| query.set(event.value()), 300)}`. The handler
comes first and the integer literal delay comes second. Debounce is trailing
edge and pending calls are canceled on island teardown.

## Quick start

```rust,ignore
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{event_handler, rsjs, signal};
use rsjs::web::dom;

pub struct Counter {
    pub initial: i32,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(self.initial);
        let decrement = event_handler(|_: rsjs::MouseEvent<dom::HtmlButtonElement>| {
            count.update(|value| value - 1);
        });
        let increment = event_handler(|_: rsjs::MouseEvent<dom::HtmlButtonElement>| {
            count.update(|value| value + 1);
        });

        html_sync! {
            <div data-counter>
                <button type="button" on_click={decrement()}>"-"</button>
                <span>{count.get()}</span>
                <button type="button" on_click={increment()}>"+"</button>
            </div>
        }
        .render(cx)
    }
}
```

Named handlers are the preferred production shape: define them immediately
before the template, then invoke them in `on_*` attributes. Inline closures
remain useful for small components.

`rsjs::web::dom` contains the generated canonical element markers shared by
typed events and node refs. They are zero-sized compiler handles that lower to
direct JavaScript; using them does not add WASM. The older `rsjs::elem::*`
spellings remain compatibility aliases.

Inside an event handler, `event.current_target()` may be kept in a local and
used for generated scalar DOM reads:

```rust,ignore
let input = event.current_target();
let value = input.value();
let width = input.client_width();
```

The marker controls method availability in rustc. Current-target handles stay
inside their handler and are read-only; use `NodeRef<E>` for supported DOM
effects. Local handlers use element markers. `on_document_*` handlers use
`dom::Document`, and `on_window_*` handlers use `dom::Window`; those exact
global handles intentionally have no element-only methods. The default
`dom::Any` event marker is accepted only for element-local bindings, so global
handlers must annotate their exact target.

## Rust-authored client objects and JavaScript classes

`#[rsjs::client]` on an inline module lowers a closed Rust struct/impl graph to
nominal JavaScript records and functions. This is useful for browser logic that
benefits from constructors, typed fields, and methods but does not need to
cross a JavaScript API boundary. Public free functions remain the
transport-safe scalar roots; object values cannot escape into signals, props,
payloads, queries, or mutations.

When JavaScript itself needs `new`, `instanceof`, and persistent per-instance
state, opt into a real ESM class boundary:

```rust,ignore
#[rsjs::client(class)]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn new(count: i32) -> Self {
            Self { count }
        }

        pub fn increment(&mut self, amount: i32) {
            self.count += amount;
        }

        pub fn value(&self) -> i32 {
            self.count
        }
    }
}
```

This emits `export class Counter` with private JavaScript state. Public
borrowed receiver methods become class methods; private methods remain internal
lowered functions. The supported language is intentionally closed: named
scalar fields, typed constructors, borrowed methods, scalar locals, checked
arithmetic, assignment, and `if`/`else`. Generics, inheritance, consuming
receivers, arbitrary crate calls, async methods, and `Drop` are rejected rather
than translated with different semantics.

Class modules are deployment assets, not implicit island dependencies. They
appear in `rsjs::manifest().client_classes`; use
`rsjs::client_class_filename(module_path)` or resolve
`RsjsAssetRef::ClientClass` through the application's asset resolver, and
import the resulting ESM URL explicitly. When the runtime is not served at its
canonical root URL, pass the class source through
`rsjs::rewrite_runtime_import_specifiers(...)` with the resolved runtime URLs.
JavaScript owns the instance lifetime and calls any authored cleanup method
explicitly.

User-defined components use Proa's struct-construction syntax inside slots:

```rust,ignore
html_sync! {
    <main>
        <h1>"Dashboard"</h1>
        {Counter { initial: 0 }}
    </main>
}
```

Plain HTML elements keep tag syntax. A direct analyzed child component is a
linkable component edge; use `rsjs::island(child)` only when the child should
be an independent island boundary.

## Build and inspect

RSJS applications use two Rust compilations separated by semantic linking.
The first compilation produces typed descriptors, `cargo proa` resolves the
reachable client graph and final owners, and the final compilation emits the
attested server artifact and its route-planned assets.

```bash
# Source-level formatting and diagnostics for an edited component
cargo proa fmt --check --verify path/to/component.rs
cargo proa lint path/to/component.rs --deny warnings

# Full linked diagnostics and a deployable build
cargo proa check
cargo proa build

# Explain why a component hydrated, stayed server-only, or pulled in a feature
cargo proa rsjs explain Counter
```

Plain `cargo check` and rust-analyzer remain useful for ordinary Rust typing and
provisional descriptor compilation. A deployable RSJS application must use a
linked lifecycle command such as `cargo proa build`, `run`, `test`, or `dev`.

## Sync and async rendering

Use `WebRenderSync` for ordinary local browser state and whenever every server
value needed for the initial render is already available. Use `WebRender` only
when rendering must await server work. Async render methods return
`RenderOutcome::pending(...)`; `async fn render` is not the RSJS trait shape.

```rust,ignore
use std::future::Future;

use proa_core::{
    DataLoader, LoadKey, RenderOutcome, WebContext, WebRender, WriteError,
};
use proa_macros::html;
use rsjs::{rsjs, signal};

pub struct UserCard {
    pub user_id: u64,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for UserCard {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let user = cx
                .load::<User>(LoadKey::typed("user", self.user_id))
                .await?;
            let expanded = signal(false);

            html! {
                <article data-user-card>
                    <h3>{user.name}</h3>
                    <button
                        type="button"
                        on_click={|_| expanded.set(!expanded.get())}
                    >
                        "Details"
                    </button>
                    <div hidden={!expanded.get()}>{user.bio}</div>
                </article>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
```

`query_with_initial(...)` may be declared in a sync component when its initial
serializable value is already available. If the initial value needs a loader,
load it through `cx` in an async prelude and then create the query. Mutations
use the async render surface. Do not turn ordinary SSR data into a query unless
the browser needs to observe, refetch, invalidate, or bind it.

## Props and composition

Inbound values have explicit timing:

- A direct field read such as `{self.label}` is `LiveRead` participation. It
  stays current when a linked parent supplies a live value and remains plain
  SSR when no live producer exists.
- `signal(self.value)` is `SeedOnce`: it creates child-owned state from the
  value present when that occurrence mounts. Later parent updates do not reset
  it.
- `rsjs::server_only(self.value)` and `rsjs::server_attr(self.value)` are
  intentional per-render snapshots. Linking rejects a live value at that
  boundary instead of silently freezing it.
- A child updates parent-owned state through an `rsjs::Handler<T>` callback.
  Callback props cannot cross an explicit `rsjs::island(...)` boundary.

Direct analyzed children are linked by Rust identity. Imports, renamed imports,
re-exports, and canonical paths work across crates. Browser-connected children
can fuse into one final owner; a server-only occurrence does not ship client
code merely because its component could participate elsewhere.

## Signals and client arithmetic

```rust,ignore
let count = signal(0_i64);
count.get();
count.set(42_i64);
count.update(|value| value + 1_i64);
```

Client arithmetic supports `i32`, `u32`, `i64`, and `u64`. The 32-bit domains
are Number-backed; the 64-bit domains are always BigInt-backed. Operations
preserve the Rust domain and reject overflow, invalid division or remainder,
and mixed-domain arithmetic before mutation. Other integer widths may still be
transported by boundary codecs, but they are not supported client arithmetic
domains. See [`STATUS.md`](./STATUS.md#numeric-contract) for the complete
contract and current edge cases.

## Queries and mutations

Queries and mutations provide page-scoped asynchronous data ownership. Rust
components declare their structural keys and static request descriptors;
rendered initial query values are dehydrated into the response and reused by
every observer of the same key. Reachability planning drops unused declarations
and includes the optional query runtime only when rendered client work needs it.

Use tuple-backed structural keys in Rust so mixed key-part types remain natural:

```rust,ignore
let price = rsjs::query_with_initial(
    rsjs::QueryKey::new(("stock", symbol, page)),
    initial_price,
    "/api/stocks/AAPL",
    rsjs::QueryOptions::default(),
);

let refresh = rsjs::event_handler(|_: rsjs::Event| price.refetch());
let invalidate = rsjs::event_handler(|_: rsjs::Event| price.invalidate());
```

Use `rsjs::FetchRequest` when a request needs a method, headers, body,
credentials, timeout, or non-JSON response handling. The authoring guide has
complete [query and mutation examples](../docs/agent/rsjs-islands.md#shape-4-async-island-with-dataloader-prelude).

## Typed state shared by independent islands

Island-local signals are the default. Independent island roots that need the
same synchronous client-owned UI state can share a typed page-scoped provider:

```rust,ignore
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

`WorkbenchChildren` must be one direct analyzed component; its render tree may
contain multiple consuming islands. The current provider grammar does not
accept an `html_sync!` fragment as `children`.

Each client consumer acquires the nearest provider in its prelude:

```rust,ignore
let workbench = rsjs::shared::<WorkbenchState>();
let button_bg = workbench.button_bg();
let is_green = rsjs::memo(|| button_bg.get() == "#2f6b4f");
```

The provider emits no DOM wrapper. Each top-level field is an independent cell;
nested providers use nearest-provider semantics, and late-hydrating islands see
the current live value. Use `rsjs::batch(...)` to group related writes into one
DOM flush. Prefer queries or mutations when the source of truth is asynchronous
or server-owned.

Every provider field, signal seed, handler payload, query key/seed, mutation
metadata value, and node-local payload that reaches the browser is public page
data. Never put secrets or authorization-only values in those surfaces. Use
`cargo proa lint --min-severity info` and `cargo proa rsjs explain` to audit why
data crossed the client boundary.

## Page and asset integration

Render an island exactly like any other Proa component:

```rust,ignore
html_sync! {
    <html>
        <body>
            {Counter { initial: 0 }}
        </body>
    </html>
}
```

That is all application page code needs. `proa_framework_axum` snapshots the
response's `RsjsRenderArtifacts` and emits the shared `PageRuntimePlan`
automatically across sync, async, and streaming response paths.
Pages and components must not look up manifest entries, construct `/_rsjs/...`
URLs, call `cx.rsjs_islands_used()`, or append runtime `<script>` tags.

The planner includes only assets required by the rendered graph, preserves
deferred hydration strategies, emits query and shared-state seed tables before
activation, and emits exact module preloads. A pure server-only result records
nothing and therefore adds no RSJS runtime.

Custom framework adapters operate at the response boundary. They snapshot
`cx.rsjs_artifacts_snapshot()`, pass the rendered islands to
`Manifest::page_runtime_plan(...)` (or `page_runtime_plan_with_urls(...)` for
hashed/CDN URLs), and emit the artifact seed tables before any consuming owner
can activate. Streaming adapters must preserve artifact deltas and emission
ordering. See [Wiring the island into a page](../docs/agent/rsjs-islands.md#wiring-the-island-into-a-page)
before implementing a custom adapter.

## Cross-runtime consumers

The page-scoped query and mutation caches are available to React, Vue, Svelte,
Solid, or application-owned JavaScript through the query runtime:

```ts
import { rsjs } from '/rsjs-runtime-query.js';

const cart = rsjs.query<Cart>(['cart', userId]);
const unsubscribeCart = cart.subscribe((value) => {
    console.log('cart updated', value);
});

const updateCart = rsjs.mutation<UpdateCart, Cart>(['update_cart', userId]);
const unsubscribeMutation = updateCart.subscribe((state) => {
    console.log('mutation state', state.status);
});

await updateCart.run({ itemId, quantity: 2 });
cart.invalidate();
```

Both handles expose `subscribe` and `getSnapshot`, so they adapt directly to
React's `useSyncExternalStore` and equivalent store APIs. Their TypeScript
contract lives in
[`runtime/rsjs-runtime-query.d.ts`](./runtime/rsjs-runtime-query.d.ts).

RSJS seeds its cache from response metadata in the browser. It does not
currently provide a server-side preseed API for a separate non-RSJS renderer
such as Next.js or Remix. Those adapters must either render a server placeholder
until mount or arrange their own matching server snapshot; otherwise the other
renderer can produce a hydration mismatch.

## Architecture

RSJS uses one typed pipeline for server output and browser behavior:

```text
authored WebRenderSync / WebRender implementation
        |
        v
descriptor compilation
  - parse and lower the Proa template
  - preserve the typed SSR implementation
  - emit component/helper edges and latent client crossings
        |
        v
cargo proa semantic link
  - resolve Rust identities and reachability
  - partition final owners and choose activation strategies
  - write a content-addressed link plan
        |
        v
final Rust compilation
  - emit final wrappers, fragments, serializers, and client modules
  - register the exact manifest assets under the linked PlanId
        |
        v
framework response planning
  - include only artifacts recorded by the rendered response
  - order runtime assets, seed tables, preloads, and stream deltas
```

The main crates are:

- `rsjs`: the author-facing primitives, runtime assets, manifest, and response
  planning contracts.
- `rsjs_macros`: render-impl analysis and Rust code generation.
- `rsjs_ir`: typed client IR, optimization, and JavaScript emission.
- `proa_cli`: descriptor compilation, semantic linking, and final-build
  orchestration for `cargo proa`.

RSJS emits a small, constrained set of JavaScript patterns directly rather than
depending on a general-purpose JavaScript AST. Manifest entries expose raw,
unminified sources; production minification and compression are framework
adapter responsibilities. For measured snapshots and their methodology
caveats, see [`OPTIMIZATIONS.md`](./OPTIMIZATIONS.md); do not treat old
unqualified bundle-size claims as a current guarantee.

## Naming

`rsjs` means Rust source to JavaScript. The crate and attribute family share the
same short lowercase name, while component bodies remain ordinary Proa
`html_sync!` or `html!` templates.
