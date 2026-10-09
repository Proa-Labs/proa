# Authoring rsjs islands

An RSJS component is a normal Proa `WebRenderSync` or `WebRender` trait impl
marked for analysis. Use `#[rsjs(component)]` when the component only
participates in an enclosing client graph. Add `client` when it declares local
signals, handlers, timers, queries, mutations, shared-state acquisition, node
refs, or browser work. Every form renders HTML on the server. When
reachable browser work exists, semantic linking forms final owners and compiles
their reachable graphs to vanilla-JS modules baked into the binary and served
with the shared RSJS runtime. In v2, an annotated impl with no reachable client
roots is classified as `ServerOnly` and emits no owner module.

This is the agent-facing authoring guide. For a quick intro, see
[`rsjs/README.md`](../../rsjs/README.md); the accepted analyzed-component
signatures live in
[`rsjs_macros/src/parse.rs`](../../rsjs_macros/src/parse.rs).

## Anatomy of a working island

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

pub struct Counter;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <div>
                <button on_click={|_| count.update(|c| c - 1)}>"-"</button>
                <span>{count.get()}</span>
                <button on_click={|_| count.update(|c| c + 1)}>"+"</button>
            </div>
        }
        .render(cx)
    }
}
```

The trait impl, method signature, context parameter, result type, and final
`.render(cx)` call are ordinary authored Rust. `component` asks RSJS to analyze
that implementation. `client` grants permission to originate browser behavior;
it does not force unused code to ship. Bare `#[rsjs]`, inherent impl shorthand,
and `async fn render` sugar are rejected.

The omitted writer parameter defaults to the owned `RenderBuffer`. Keep this
one-loader-generic shape for normal islands; the later generic-island example
shows the advanced custom-writer form when it is genuinely required.

Because this impl has reachable signal reads and click handlers, RSJS:
1. Wraps the rendered output in `<rsjs-island data-rsjs-island-root="counter_<hash>">...</rsjs-island>`.
2. Adds `data-rsjs-handler="N"` and `data-rsjs-bind="N"` markers so the runtime can find each handler/binding target on hydration.
3. Emits the final owner's reachable JS module, registered into the manifest at link time via `linkme`.
4. Emits a `<script type="application/json" data-rsjs-island="...">{"signals":["0"]}</script>` seed so the client runtime can rebuild the exact `i64` signal state.

The entire `data-rsjs-*` attribute namespace is compiler-owned. Do not author
those attributes yourself; use an application namespace such as `data-app-*`.
The analyzer rejects collisions so occurrence-specific marker elision cannot
silently remove application data.

Reference: [`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs).

## When not to use an island

Do not add an RSJS attribute to an opaque component that only needs server
rendering. Keep it as a regular authored `WebRenderSync` / `html_sync!`
component. Use `#[rsjs(component)]` only when the compiler must analyze the
component as a participant in composition. Add `client` when it declares a
client origin such as `signal`, `on_*`, `bind_*`,
`query_with_initial`, `mutation`, `shared::<T>()`, `node_ref`,
`rsjs::interval`, or a browser helper.

A pure `memo` derived only from inbound live values is participation, not a
new origin. A memo that depends on locally originated state or a browser
capability inherits that dependency's origin requirement.

Ordinary non-reactive Rust expressions inside an analyzed template stay opaque
SSR automatically. Calls such as `render_markdown(document)` and
`UnsafeRaw(svg)` do not need a `server_only(...)` wrapper when their visible
arguments contain no RSJS state, handler, or browser primitive. Dynamic
attributes follow the same rule. Inbound fields are different: direct reads
such as `self.label` are latent `LiveRead` participation, and opaque calls that
depend on an inbound value fail closed if a live caller would otherwise be
silently snapshotted. A value implementing `ClientEncode` or coming from a
`ClientProps`-derived type also does not make an island necessary by itself; it
only crosses to the browser when final linking reaches a client-visible use.

Opaque child output has unknown DOM cardinality, but it can still appear
directly before a browser-owned sibling:

```rust
html_sync! {
    <main>
        {render_markdown(document)}
        <button on_click={increment}>{count.get()}</button>
    </main>
}
```

The generated client path speculatively uses the fast positional lookup,
verifies the compiler-owned marker it lands on, and falls back to an
owner-scoped marker lookup when the opaque output emitted zero or multiple DOM
nodes. No server buffering or HTML parsing is required. A static host such as
`<article>` can still be useful for document semantics or styling, but it is
not required for hydration correctness.

`cargo proa lint` reports `rsjs/unnecessary-island` when an analyzed render
method has no detected client-side rsjs primitives. Treat that as an
optimization prompt, not proof that `#[rsjs(component)]` is unnecessary: a
ServerOnly component can still need its descriptor to participate in parent
composition. Remove the attribute only when the type is intentionally opaque
SSR and does not need compiler composition; do not wrap every child in
`server_only(...)` merely to retain it. The source lint can prove plainly named
client declarations dead when they never reach the template or a reachable
handler graph, and reports those as `rsjs/dead-client-primitive`.
The v2 compiler's IR strategy pass remains the authority for full reachability,
descriptor-backed children, query/mutation metadata, and emitted assets.
Compiler-backed linting can use the v2 strategy result when available; stale or
missing strategy artifacts fall back to source analysis.

In the v2 compiler model, the decision is usage-driven. Client-capable types and
`ClientEncode` bounds only mean a value is allowed to cross into browser state;
they do not force hydration. Hydration starts from reachable client uses such as
handlers, bindings, signal reads/writes, query or mutation effects, browser
helpers, client helper calls, and descriptor-declared reactive or callback
fields. A direct inbound field read can participate as `LiveRead`, but remains
plain SSR when its final occurrence has no browser-reachable producer.

Quick boundary rules:

| Code shape | Result |
| --- | --- |
| No RSJS attribute | Opaque SSR component; no client analysis. |
| `#[rsjs(component)]` | Analyzed participant that may compose into an enclosing client graph but cannot originate local browser behavior. |
| `#[rsjs(component, client)]` | Origin-capable analyzed component; reachable browser work determines emitted JS. |
| `signal(...)`, `memo(...)`, `query_with_initial(...)`, `mutation(...)`, or `node_ref(...)` declared but unused | Can be dropped by v2 reachability; do not rely on side effects there. |
| `on_*`, `bind_*`, reachable signal/query/mutation/node-ref use, browser helper, or client helper call | Browser JS is required. |
| Reachable call to a `#[rsjs::client]` helper | The helper and its reachable helper dependencies ship; an uncalled helper does not. |
| Helper call inside a callback-adapter payload that crosses a component-owner boundary | Rejected with `rsjs/linked-payload-handler-unsupported-dependency` until that ABI transports typed helper edges. Ordinary component handlers and reactive expressions are supported. |
| `ClientEncode` / `ClientProps` on a value | Permission to cross, not a reason to hydrate. |
| Ordinary call/value with no inbound or local client dependency | Inferred opaque SSR; no client IR. |
| `rsjs::server_only(...)` / `rsjs::server_attr(...)` | Explicit per-render snapshot boundary, also retained as a compatibility escape hatch. |

Implementation status: linked builds resolve standalone helper definitions and
calls through rustc-selected relocation targets, carry each reachable helper's
relocation-resolved Client IR into the content-addressed final plan, and emit
one private JavaScript function per reachable helper in each caller fragment.
Final emission fails if any symbolic helper edge remains unresolved.
Component-level `clients(...)` allowlists have been removed; using one produces
a focused migration diagnostic.

Treat every client-reaching position as public page data. Do not pass secrets,
database handles, session internals, or server-only capability objects through
signal initializers, handler payloads, query/mutation metadata, node payloads,
or descriptor reactive props. If data should remain server-private,
keep it in an opaque SSR component or use `server_only(...)` /
`server_attr(...)` for an intentional per-render snapshot. A live composed
caller is rejected at that boundary.

For review-heavy code, run lint with info diagnostics to see
`rsjs/client-crossing` records. Those diagnostics should explain which value
crosses, why it crosses, and where it is emitted, without printing the runtime
value. Suppressing the diagnostic does not stop the value from shipping; it only
hides the local warning.

Reachable client-crossing values must satisfy the crossing surface's codec
contract: signal seeds, handler payloads, node payloads, and descriptor
reactive props use `rsjs::ClientEncode`, while query initial values use the
query serialization path. Values that only appear in SSR/server-only positions
do not need a client codec. Server-loaded data from `cx.load(...).await?`
follows the same rule: rendering it as SSR text keeps it server-side, while
passing it to `signal(...)`, `query_with_initial(...)`, a handler payload, a
node payload, or a descriptor reactive prop serializes it into public browser
state.

Do not rely on side effects inside unused client declarations. In v2, declarations
such as `signal(...)`, `memo(...)`, `event_handler(...)`, `node_ref(...)`,
`query_with_initial(...)`, or `mutation(...)` can be dropped when the IR proves
no reachable browser work uses them. Put required server work in ordinary Rust
prelude code.
`cargo proa lint` also reports plainly unused named `signal(...)` /
`.into_signal()`, `memo(...)`, `rsjs::handler(...)`,
`rsjs::event_handler(...)`, `rsjs::node_ref(...)`, query declarations
(`query(...)`, `query_with_options(...)`, `query_with_initial(...)`), and
mutation declarations (`mutation(...)`, `mutation_with_options(...)`) as
`rsjs/dead-client-primitive`, because some of these locals may be inlined away
before they become compiler IR declarations.

Ordinary prelude code is still server code and may run even if a later client
declaration is proven dead, so avoid doing expensive server work solely to feed a
speculative client primitive.

The tolerant frontend preserves arbitrary ordinary server statements, `?`,
early returns, drop/evaluation order, the authored `.render(cx)` call, and async
poll boundaries for both server-only and hydrated components. Compiler-owned
declarations are replaced at their original lexical positions, and only the
final `html_sync!` / `html!` invocation is exchanged for the analyzed template.
An opaque server statement may not read a signal, query, mutation, node ref,
handler, client-derived alias, or browser helper: those operations must remain
inside syntax RSJS can lower and receive
`rsjs/template-client-state-in-opaque-server-statement` or
`rsjs/template-browser-operation-in-opaque-server-statement` otherwise.

The current explicit-trait frontend still accepts one analyzed render site: the
method (or canonical `RenderOutcome::pending(async move { ... })` future) ends
in one `html_sync!` / `html!` expression. Put genuinely alternative render
sites in separate components until multi-site linking is specified.

Do not expect rsjs to auto-convert an island to client-only rendering. If code
cannot produce SSR, it should fail with a diagnostic today. Future client-only
support needs an explicit opt-in such as `render = "client"` plus an
intentional fallback/no-JS policy; silent empty SSR should not be the default.
The parser currently reserves and rejects `render = "client"`, `client_only`,
and `fallback = ...` until that policy is implemented.

## Migrating Existing Islands to v2 Strategy

For each analyzed component, classify the code by authoring permission and
reachable browser work, not by prop types:

1. If the render body only emits static/server markup and does not participate
   in client composition, remove the RSJS attribute and keep
   it a regular `WebRenderSync` component. Do not wrap the whole tree in
   `server_only(...)` just to keep the attribute.
2. Use participant-only `#[rsjs(component)]` for an analyzed child that
   receives graph edges but declares no local origins. Use
   `#[rsjs(component, client)]` when it declares origins.
3. Remove routine `server_only(...)` / `server_attr(...)` wrappers around
   proven ordinary server locals. Retain an explicit boundary when the
   component contract intentionally snapshots an inbound value, or as a
   compatibility escape for syntax the frontend cannot classify. Opaque
   zero-to-many-node output can remain direct: hydration verifies its fast
   positional path and falls back to an owner-scoped marker lookup.
4. If a client primitive is declared but unused, move any required server work
   into ordinary prelude Rust. v2 may drop unused `signal`, `memo`,
   handler, query, mutation, or node-ref declarations.
5. If a utility helper only formats SSR text, call it normally in the template
   or prelude. When a pure, origin-free calculation must rerun
   after hydration, annotate that function once with `#[rsjs::client]`;
   `#[rsjs::client_module]` is optional grouping rather than eligibility.
6. Run lint with info diagnostics when reviewing data exposure:
   `cargo proa lint --min-severity info`. New `rsjs/client-crossing` records are
   public page data and should be reviewed deliberately. CI can fail on that
   specific audit code with `cargo proa lint --min-severity info --deny rsjs/client-crossing`.
7. Verify output shape. Server-only results should have no `<rsjs-island>`
   wrapper, no `data-rsjs-island-root`, no seed scripts, no manifest entry, and
   no page runtime tags. Hydrated results should ship only the reachable helper
   bundles, query/mutation runtime, and fragment assets they actually use.

## Debugging Strategy Decisions

Use strategy diagnostics when an island ships unexpected JS or stops shipping
JS after v2 reachability pruning.

`cargo proa lint --min-severity info` is the review path for public data
crossings. It should report `rsjs/client-crossing` with a redacted label,
surface, and reason, but suppressing that diagnostic does not change what ships
to the browser. The current compiler-backed strategy-cache path reports
reachable signal seed, handler payload, query key, and query initial-value
crossings, plus node payload, query fetcher, mutation key, mutation runner,
mutation invalidation key, and descriptor reactive prop crossings. Raw helper
arguments and mutation call arguments are not listed as separate public-data
crossings; when those expressions read server-derived data, the underlying
signal seed, payload attr, query/mutation metadata, or descriptor prop crossing
is reported instead.

For compiler-backed v2 diagnostics from a previous build, consume strategy
sidecars:

```bash
cargo proa lint path/to/file.rs --use-rsjs-strategy-cache
cargo proa check path/to/file.rs --use-rsjs-strategy-cache
```

This makes `rsjs/unnecessary-island` follow the compiler strategy result when a
valid sidecar matches the impl. It also enables sidecar-only info diagnostics
such as transitional `rsjs/unused-client-module` records for helper groups that
did not ship and `rsjs/component-fragment-mount` for parents hydrating through the
current schema-v4 parent-mounted child fragment path,
`rsjs/dead-client-primitive` for declared but unreachable
signals, memo cells, handlers, node refs, queries, or mutations, plus anchored
`rsjs/unnecessary-server-only` warnings for wrappers the compiler proved were
simple SSR projections. Missing or ambiguous sidecars fall back to the
conservative source lint by default. In strict mode they are errors:

```bash
cargo proa lint --use-rsjs-strategy-cache --require-fresh-strategy-cache
cargo proa check --use-rsjs-strategy-cache --require-fresh-strategy-cache
```

Current cache matching first uses `input_fingerprint` plus source anchors when
the sidecar has them, then falls back to exact self type and unique struct name
for older sidecars. Shared analysis and descriptor-hash freshness are v2
follow-ups.

When v2 strategy sidecars are enabled, use:

```bash
cargo proa rsjs explain target/rsjs/strategy/<analysis-id>.json
```

The selector can also be an analysis id, runtime asset id, struct name, or
struct path when the sidecar is under `target/rsjs/strategy`. Use the JSON form
for tooling:

```bash
cargo proa rsjs explain <analysis-id-or-struct-name> --json
```

For audit tooling that only needs public data exposure, use the normalized
crossing report:

```bash
cargo proa rsjs crossings --json
cargo proa rsjs crossings <analysis-id-or-struct-name> --json
```

The report is sidecar-backed in the current CLI. It contains redacted crossing
records with component ids, surfaces, requirements, optional source anchors, and
no serialized values.

The explanation should answer:

- `Why did this hydrate?` Look at reachable roots: `on_*`, `bind_*`, signal
  reads/writes, query or mutation observations, mutation runs, node refs,
  browser reads/effects, client helper calls, or descriptor child host
  coupling. Browser helpers are reported by stable capability labels such as
  `location.pathname` or `clipboard.write_text`, not by runtime values.
- `Which client helper bundles shipped?` The transitional explanation format
  lists `declared_client_modules`, `client_modules`, and
  `unused_client_modules` separately from roots. Only groups containing
  reachable `#[rsjs::client]` helpers should ship. These legacy field names do
  not make component-level allowlists part of the accepted API.
- `Which shared runtime features shipped?` The explanation lists
  `runtime_features` such as `query` or `mutation` only when reachable
  query/mutation work needs that browser runtime code.
- `Which descriptor fragment assets shipped?` The explanation lists
  `component_fragment_imports` for reachable parent-mounted descriptor
  fragments. Server-only child descriptors do not appear there.
- `Why did this emit no JS?` A `ServerOnly` result means there are no reachable
  client roots. The missing wrapper, seeds, manifest entry, runtime asset, and
  page runtime tags are intentional.
- `Did this prop make the island client-rendered?` Props and `ClientEncode`
  bounds only permit crossing. The explanation should point at the use site
  that crossed, such as a signal initializer, handler payload, node payload, or
  descriptor reactive prop. Reachable signal initializers appear in the redacted `crossings`
  list as `signal_initial_value` records with a `signal_seed` surface;
  reachable handler payloads appear as `handler_payload` records with a
  `handler_payload_attr` surface; reachable node-local payloads appear as
  `node_payload` records with a `node_payload_attr` surface; reachable query
  keys appear as `query_key` records with a `query_metadata_blob` surface;
  reachable query fetcher configs appear as `query_fetcher` records with a
  `query_metadata_blob` surface and `request_config` requirement;
  reachable query initial values appear as `query_initial_value` records with a
  `query_seed_blob` surface; reachable mutation keys appear as `mutation_key`
  records with a `mutation_metadata_blob` surface; reachable mutation runner
  configs appear as `mutation_runner` records with a `mutation_metadata_blob`
  surface and `request_config` requirement; reachable mutation invalidation keys
  appear as `mutation_invalidation_key` records with the same mutation metadata
  surface. Client helper calls appear
  in roots and shipped bundles; their argument data is reported by the actual
  signal, payload, query, or descriptor crossing surface that supplied it.
  Reachable fragment-backed child reactive props appear as
  `descriptor_reactive_prop` records with a `descriptor_reactive_prop` surface
  and, when available, a redacted source anchor for the parent prop expression.
  Payload attrs that are not read by reachable handler or node-local Client IR
  are not emitted.
- `Why did this slot stay SSR?` Template decisions should explain simple
  server slots and attrs. Strategy sidecars expose a redacted
  `server_surfaces` list with SSR slot/attr kind, node, attr name when
  applicable, source label such as `inferred_simple_ssr`,
  `explicit_simple_ssr_boundary`, or `explicit_boundary`, optional source
  line/column when the surface came from the current template, and reason, not
  raw source tokens or runtime values.
- `Why did this slot hydrate?` Template decisions should name the client
  lowering reason, such as a signal read, binding, handler payload, browser
  helper, client helper call, descriptor host coupling, or the current
  schema-v4 parent-mounted descriptor fragment path.
- `Did an unused primitive still run?` Dead client declarations can be dropped.
  Move required server side effects into ordinary prelude Rust.
- `Why was my deferred strategy ignored?`
  `#[rsjs(component, client, strategy = "...")]` only
  controls when browser JS hydrates. If the strategy result is `ServerOnly`,
  `cargo proa rsjs explain` reports the requested strategy as ignored because
  no wrapper or JS asset is emitted.

Do not infer strategy by checking whether a JS file or manifest entry exists.
Use a strategy sidecar or `cargo proa rsjs explain`. Strict cache mode rejects
missing, ambiguous, or stale-fingerprint sidecars. Full cross-crate descriptor
freshness still needs descriptor hashes and shared analysis.

## Canonical client handlers

Prefer defining event handlers as named locals immediately before `html!` /
`html_sync!`, then invoking those locals from `on_*` attributes. This keeps the
template mostly structural, gives the handler a meaningful name, and makes IDE
completion work in normal Rust code before the template macro starts.

Use `rsjs::event_handler(...)` for DOM event attributes. Its first closure
parameter is the event, and it may take one payload argument that you pass from
the attribute invocation:

```rust
use rsjs::web::dom;

let refresh_on_click = rsjs::event_handler(|_: rsjs::Event| {
    price.refetch();
});
let pick_on_click = rsjs::event_handler(|event: rsjs::MouseEvent<dom::HtmlButtonElement>, symbol: &'static str| {
    console::log(event.closest_attr("data-symbol"), symbol);
});
let update_on_input = rsjs::event_handler(|event: rsjs::InputEvent<dom::HtmlInputElement>| {
    let input = event.current_target();
    console::log(input.value(), input.get_attribute("aria-label"));
});

html! {
    <button type="button" on_click={refresh_on_click()}>"Refresh"</button>
    <button type="button" data-symbol={symbol} on_click={pick_on_click(symbol)}>"Pick"</button>
    <input value={symbol} on_input={update_on_input()} />
}
```

Inline `on_*={|event| ...}` closures are still supported for small examples, but
new production-style islands should prefer named handler locals.

Typed event generics model the handler's `currentTarget`, not the raw bubbling
`target`. Local bindings use element targets. Prefer the generated canonical
markers under `rsjs::web::dom`, such as
`rsjs::MouseEvent<dom::HtmlButtonElement>` and
`rsjs::InputEvent<dom::HtmlInputElement>`. The same
`dom::HtmlInputElement` marker is valid in
`rsjs::node_ref::<dom::HtmlInputElement>()`. The older `rsjs::elem::Button`
and `rsjs::elem::Input` names remain compatibility aliases of those exact
types.

The typed-handle form supports both direct reads and handler-local bindings:

```rust
let input = event.current_target();
let value = input.value();
let label = input.get_attribute("aria-label");
let width = input.client_width();
```

The available methods come from the generated element capabilities. Rustc
therefore rejects, for example,
`MouseEvent<dom::HtmlButtonElement>::current_target().checked()`. A
current-target handle is intentionally handler-scoped: use it only as the
receiver of generated scalar reads. It cannot be stored in a signal, passed to
a callback/client helper, serialized, or used for DOM effects. Use a typed
`NodeRef<E>` when the handler needs to mutate an element.

For `on_document_*`, annotate the event with `dom::Document`; for
`on_window_*`, use `dom::Window`:

```rust
let close_on_escape = rsjs::event_handler(
    |event: rsjs::KeyboardEvent<dom::Document>| {
        if event.key() == "Escape" {
            open.set(false);
        }
    },
);
let measure_on_resize = rsjs::event_handler(
    |_: rsjs::Event<dom::Window>| width.set(panel.rect_width()),
);
```

Their `current_target()` handles represent the actual `Document` and `Window`
used for registration. Those markers are not elements, so element-only reads
such as `.id()`, `.value()`, attributes, and geometry are unavailable. Read
the bubbling target through event accessors, or use a typed `NodeRef<E>` for a
specific owner element. Compiler-owned repeated-handler payloads still resolve
from the source element internally; that does not change `current_target()`.

RSJS rustc-checks the typed event against both the `on_*` event family and its
concrete binding target. For example,
`MouseEvent<dom::HtmlInputElement>` is rejected
on `<input on_input=...>`, as is `InputEvent<dom::HtmlButtonElement>`. Use the
base `rsjs::Event` type and/or its default `dom::Any` marker only when an
element-local handler intentionally needs a broader contract. Global handlers
must name `dom::Document` or `dom::Window`; leaving the event generic at its
`dom::Any` default is rejected so element-only reads cannot silently target a
global object.

Add `strategy = "..."` to the component attribute when hydration should be
deferred. The
default is immediate/eager hydration. Supported values are `"visible"`, `"idle"`,
`"interaction"` or `"interaction:<event>"`, and `"media:<query>"`:

```rust
#[rsjs(component, client, strategy = "visible")]
impl<L: DataLoader> WebRenderSync<L> for BelowFoldCounter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        /* signal + html_sync! body ending in `.render(cx)` */
    }
}
```

## The seven shapes you will write

### Shape 1: single-signal toggle

When you have one boolean or numeric signal driving one UI state. This is the
"watch heart," "show/hide drawer," "expand/collapse" pattern.

```rust
pub struct WatchHeart;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for WatchHeart {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let watching = signal(false);
        html_sync! {
            <button type="button"
                    aria-label="watch listing"
                    on_click={|_| watching.set(!watching.get())}>
                <span class="icon-on" aria-hidden="true">
                    {if watching.get() {
                        html_sync! { <svg><use href="#icon-heart-filled"/></svg> }
                    }}
                </span>
                <span class="icon-off" aria-hidden="true">
                    {if !watching.get() {
                        html_sync! { <svg><use href="#icon-heart-outline"/></svg> }
                    }}
                </span>
            </button>
        }
        .render(cx)
    }
}
```

Reference: [`proa_demo/src/ebay_srp_islands.rs`](../../proa_demo/src/ebay_srp_islands.rs)
(`WatchHeart`).

### Shape 2: multi-signal island with conditionals

When you have several signals on one island (a form, a multi-toggle toolbar,
etc.). Each signal toggles independently; conditional sections gate their own
visibility.

```rust
pub struct UserForm;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for UserForm {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let username = signal(String::new());
        let email = signal(String::new());
        let submitting = signal(false);
        html_sync! {
            <form on_submit={|_| submitting.set(true)}>
                <input bind_value={username}/>
                <div>
                    {if !username.is_non_empty() {
                        html_sync! { <p class="error">"Username required"</p> }
                    }}
                </div>
                <input bind_value={email}/>
                <div>
                    {if email.get() == String::new() {
                        html_sync! { <p class="error">"Email required"</p> }
                    }}
                </div>
                <button type="submit">"Submit"</button>
            </form>
        }
        .render(cx)
    }
}
```

Reference: [`rsjs_macros/tests/parser_user_form.rs`](../../rsjs_macros/tests/parser_user_form.rs).

The seed for this island is `{"signals":["","",false]}` because all three
signals are reachable. In v2, unreachable signal values are not serialized; if
a higher reachable SignalId needs alignment, the dead slot is emitted as
`null`.

### Shape 3: server-only opaque content inside an island

Non-reactive calls and values remain ordinary server rendering inside an
analyzed component. They do not need a marker function or a dedicated static
host. If the result emits zero or multiple nodes, hydration verifies the
speculative positional path and falls back to an owner-scoped marker lookup for
any later browser-owned target.

```rust
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for IconButton {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let pressed = signal(false);
        html_sync! {
            <button on_click={|_| pressed.set(!pressed.get())}>
                <span class="icon">
                    {proa_core::UnsafeRaw("<svg>...</svg>")}
                </span>
            </button>
        }
        .render(cx)
    }
}
```

Constraints on an inferred opaque server expression:
- Cannot capture or read a `signal(...)` (compile error: "cannot capture reactive signal").
- Cannot declare/read client primitives such as `signal(...)`,
  `memo(...)`, `query_with_initial(...)`, `mutation(...)`,
  `node_ref(...)`, `handler(...)`, `event_handler(...)`, or
  `interval(...)` inside the wrapped expression.
- Cannot call rsjs browser helpers such as `rsjs::window::inner_width()`,
  `rsjs::location::pathname()`, or `rsjs::navigator::language()` inside the
  wrapped expression.
- Renders once on the server; the runtime treats the subtree as opaque.

Reference: [`rsjs/src/lib.rs`](../../rsjs/src/lib.rs) (the `server_only` doc) and
the `rsjs_macros/tests/ui/conformance/fail/36_server_only_signal_capture.rs`
failing test.

### Shape 4: async island with dataloader prelude

When the render needs to `.await` (most commonly `cx.load::<T>(id).await` for a
dataloader fetch), implement the real `WebRender` method and return
`RenderOutcome::pending(async move { ... })`.

```rust
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
                <article>
                    <h3>{user.name}</h3>
                    <button on_click={|_| expanded.set(!expanded.get())}>
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

Reference: [`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs)
(`AsyncServerPreludeChild`).

`query_with_initial(...)` works on `WebRenderSync` when its serializable initial
value is already available. Use async `WebRender` when producing that initial
value requires `.await`; mutations are async-only. For server-fetched data,
load the initial value through the normal server `cx`/`DataLoader` path, then
pass that value to `rsjs::query_with_initial(...)`.
The macro serializes the effective key into the adjacent per-island query blob
and records the value in the framework's page-level query seed table, while the
browser receives only that JSON plus the static client request descriptor.
Initial server output also evaluates query accessors/projections from that same
value for text bindings, dynamic attributes, and conditional branch visibility.
Prefer direct typed field access such as `price.data().value` or
`price.data().market.bid`; `.at("/path")` remains an escape hatch for dynamic
JSON-shaped data.
Use `rsjs::QueryKey::new((...))` for structural cache keys. Literal single
string keys still work as a backwards-compatible one-part key. Runtime
initializers also serialize the per-instance query key, so
`query_with_initial(...)` keys can use server locals such as
`symbol.as_str()`; the client fetch descriptor itself is still static.
Prefer tuple keys in Rust, even though the browser runtime stores query keys as
arrays to match React Query. Rust arrays require every element to have the same
type, so `["stock", symbol, page]` stops working as soon as `page` is numeric.
Tuples naturally support mixed key parts, e.g. `("stock", symbol, page)`, and
rsjs serializes them to the same structural JS array shape at runtime.
`rsjs::server_fetcher(|cx| async move { ... })` is not supported. `cx` is
already available in the pending render future, and the direct server prelude
load is the canonical shape. Mutation registrations now also pass the hydrated
root so the browser runtime can read the adjacent
`data-rsjs-island-mutations` metadata blob. That
blob includes static-server-safe fields and can serialize server-local mutation
keys and invalidation keys at render time when they cannot be lowered into
browser JS. Signal-backed mutation keys and invalidation keys still come from
the generated island JS fallback. Async parent owners can link child component
fragments whose queries/mutations use static descriptor-safe keys, fetchers,
runners, and invalidations; the fused owner owns the remapped query/mutation ids
and metadata. Child queries that rely on `query_with_initial(...)` also carry their
server initializer metadata through the descriptor, so an async parent can
dehydrate the remapped child query under the parent's query id. Child mutation
metadata with server-local keys or
invalidations is also carried through the descriptor and serialized under the
parent's mutation id.
Use a literal URL for the common client refetch case, or `rsjs::FetchRequest`
when the browser request needs method, headers, body, credentials, timeout, or
non-JSON response decoding. `query_with_initial(...)` is the only Rust query
declaration surface; there is no separate static-JSON query form.
Event handlers can call `query.refetch()` to fetch immediately or
`query.invalidate()` to mark the cache stale and refetch when the query is
observed.

Do not use `query_with_initial(...)` just to fetch SSR data. If the browser does
not need to observe, refetch, invalidate, or bind the value, keep it as ordinary
`cx.load(...).await?` prelude data and render it through SSR. In v2, unreachable
query/mutation declarations can be dropped along with their metadata and runtime
imports.

```rust
use rsjs::elem;

#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for StockTicker {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let draft = signal(String::new());
            let symbol = "AAPL";
            let initial_price: StockPrice = cx
                .load_json(LoadKey::named("stock", symbol))
                .await?;
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", symbol)),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    headers: &[("Accept", "application/json")],
                    timeout_ms: Some(5_000),
                    ..Default::default()
                },
                rsjs::QueryOptions {
                    interval_ms: Some(5_000),
                    refetch_on_focus: true,
                    download_progress: true,
                    stream_text: false,
                    retry_count: 3,
                    stale_time_ms: 2_000,
                },
            );
            let save = rsjs::mutation::<String, ()>(
                rsjs::QueryKey::new(("update_cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    credentials: Some(rsjs::FetchCredentials::SameOrigin),
                    response: Some(rsjs::ResponseKind::Empty),
                    headers: &[("Content-Type", "application/json")],
                    timeout_ms: Some(5_000),
                    ..Default::default()
                },
                &[rsjs::QueryKey::new(("stock", "AAPL"))],
            );
            let refresh_on_click =
                rsjs::event_handler(|_: rsjs::MouseEvent<elem::Button>| price.refetch());
            let invalidate_on_click =
                rsjs::event_handler(|_: rsjs::MouseEvent<elem::Button>| price.invalidate());
            let save_on_click =
                rsjs::event_handler(|_: rsjs::MouseEvent<elem::Button>| save.run(draft.get()));

            html! {
                <div>
                    <span>{price.data().value}</span>
                    <span>{price.download_loaded()}</span>
                    <span>{price.download_percent()}</span>
                    <span hidden={!save.is_pending()}>"Saving"</span>
                    <button on_click={refresh_on_click()}>"Refresh"</button>
                    <button on_click={invalidate_on_click()}>"Invalidate"</button>
                    <button on_click={save_on_click()}>"Save"</button>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
```

`query_with_initial(...)` may use `WebRenderSync` when its initial value is
already available. If that value must be loaded asynchronously, use
`WebRender` + `html!`. Mutation declarations still require the async render
surface.

### Shape 5: generic island with lifetime / type / const params

When the struct needs to borrow lifetime-bound data, take a type parameter, or
parameterise on a const, put those generics on the real render-trait impl.

```rust
pub struct GenericIsland<'a, T, const N: usize> {
    pub label: &'a T,
}

#[rsjs(component, client)]
impl<'a, T, const N: usize, Buf, Loader> WebRenderSync<Loader, Buf>
    for GenericIsland<'a, T, N>
where
    for<'b> &'b T: LabelRef,
    Buf: WriteBuf,
    Loader: DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let label = self.label.label_ref();   // prelude let
        let count = signal(0_i64);
        html_sync! {
            <div data-kind="generic">
                <span>{count.get()}</span>
                {label}
            </div>
        }
        .render(cx)
    }
}
```

Things to know:
- Buffer and loader generics can be named for the surrounding code and carry
  specialized bounds when needed.
- HRTBs (`for<'b> &'b T: LabelRef`) on the impl-level `where` clause work.
- Async components use the canonical `WebRender` signature. Application
  lifetime/type/const generics belong on the impl header; the render method
  carries the trait's render/output lifetimes.

Reference: [`rsjs_macros/tests/parser_generics.rs`](../../rsjs_macros/tests/parser_generics.rs).

### Shape 6: reusable rsjs component with callback props

When a child component should reuse markup but let the parent own state, give
the child an `rsjs::Handler<T>` field and call it from the child's handler body.
A direct child occurrence connected by a live prop or callback edge is linked
into the same final owner. The linker imports the child's compiled fragment; it
does not macro-copy the child's graph into the parent.

```rust
pub struct SearchInput {
    pub label: &'static str,
    pub value: String,
    pub on_change: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for SearchInput {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let on_input = rsjs::event_handler(
            |event: rsjs::InputEvent<rsjs::elem::Input>| {
                self.on_change.call(event.current_value());
            },
        );
        html_sync! {
            <label>
                {self.label}
                <input value={self.value} on_input={on_input()}/>
            </label>
        }
        .render(cx)
    }
}

pub struct SearchPanel;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for SearchPanel {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let query = signal(String::new());
        let on_change = rsjs::handler(|next: String| query.set(next));

        html_sync! {
            <section>
                {SearchInput {
                    label: "Search",
                    value: query.get(),
                    on_change,
                }}
            </section>
        }
        .render(cx)
    }
}
```

Current constraints:
- Required handler props must be local `rsjs::handler(...)` values or
  `rsjs::noop_handler()`.
- Optional handler props use `Option<rsjs::Handler<T>>`; child components call
  `self.on_change.call_if_some(value)` after importing
  `rsjs::OptionalHandlerExt`, and parent literals pass `Some(on_change)` or
  `None`.
- `signal(self.value)` is `SeedOnce`: it creates child-owned state from the
  value present when that occurrence mounts. Later parent writes do not reset
  it.
- Direct reads such as `{self.label}` and `class={self.class}` are `LiveRead`
  participation. They produce no JavaScript for a server-only occurrence, but
  remain current when a linked parent supplies a live value.
- An exact alias such as `let label = self.label;` preserves that inbound
  provenance; it is not a way to silently freeze a live prop.
- Use `rsjs::server_only(self.label)` or
  `rsjs::server_attr(self.class)` only when the API intentionally promises a
  per-render snapshot. Linking rejects a live parent value at that boundary.
- A child cannot mutate an inbound `LiveRead` directly. Emit a typed callback
  to the parent, or use `signal(self.value)` when the child deliberately owns
  independent local state.
- Template loops in child components can repeat event handlers, including
  submit handlers and `on_document_*` / `on_window_*` handlers, input bindings,
  text bindings, dynamic attributes, and conditional sections.
- When a loop-local value is used in text, attributes, or handler payloads,
  rsjs serializes that per-node payload into marker attributes and decodes it
  during hydration. The loop's server source can be a descriptor server prop,
  but the reachable payload attr itself is public browser data and is reported
  as a client crossing.
- Sync parents cannot fuse an async child occurrence into their render path.
  Async parents can link descriptor-backed async children, including child
  query/mutation components and awaited server prelude values represented by
  server-only fragment slots.
- Child components with richer runtime-only behavior can still use
  `rsjs::island(...)` as an explicit boundary.
- Child-local prelude lets used by inferred server props,
  `rsjs::server_attr(...)`, loop iterators, loop prefixes, and payload attrs are
  recorded in the compiled component fragment for semantic linking.
- Direct component edges resolve through Rust identity. Ordinary imports,
  renamed imports, re-exports, and canonical paths work in same-crate and
  cross-crate composition.

Reference:
[`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs)
(`ComponentCallbackInput`, `ComponentCallbackValue`) for descriptor-backed
cross-component callbacks.

Calendar-shaped example:

```rust
pub struct DatePicker {
    pub selected: u8,
    pub on_change: rsjs::Handler<u8>,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for DatePicker {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let selected = signal(self.selected);
        let on_day_click = rsjs::event_handler(|event: rsjs::Event, day: u8| {
            event.prevent_default();
            console::log(day);
            self.on_change.call(day);
        });

        html_sync! {
            <div data-date-picker>
                {for day in 1_u8..=31_u8 {
                    html_sync! {
                        <button
                            type="button"
                            data_selected={selected.get() == day}
                            on_click={on_day_click(day)}
                        >
                            {day}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct BookingForm;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for BookingForm {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let selected_day = signal(15_u8);
        let on_change = rsjs::handler(|day: u8| selected_day.set(day));

        html_sync! {
            <section>
                {DatePicker {
                    selected: selected_day.get(),
                    on_change,
                }}
                <p>"Selected day: " {selected_day.get()}</p>
            </section>
        }
        .render(cx)
    }
}
```

### Shape 7: explicit child island boundary

Descriptor-backed RSJS components participate in the enclosing linked graph by
default. Use `rsjs::island(...)` only when the child should hydrate as an
independent nested island.

```rust
html_sync! {
    <section>
        {rsjs::island(SearchInput {
            label: "Standalone search",
            value: String::new(),
            on_change: rsjs::noop_handler(),
        })}
    </section>
}
```

Boundary rule: parent `rsjs::handler(...)` values cannot cross
`rsjs::island(...)`. If the child should update parent state, render it directly
so the live edge remains inside one owner. If the child should be independent,
give it independent state or pass `rsjs::noop_handler()`.

Direct descriptor-backed children currently compose by usage:

- A child occurrence whose fragment uses only server props remains SSR and does
  not form a browser owner.
- Live props, callbacks, and reactive lifecycle edges fuse the connected
  occurrences into one owner; a live graph edge never crosses an owner boundary.
- Disconnected client subgraphs may remain separate owners and keep separate
  activation strategies. `rsjs::island(child)` forces that cut.
- If occurrences must fuse but request incompatible explicit activation
  strategies, linking fails with a focused diagnostic. An unconstrained
  occurrence inherits the constrained owner's strategy; a fused owner with no
  constraint defaults to eager activation. A server-only result forms no owner,
  so its requested strategy is ignored.
- A hydrated descriptor fragment mounted from the parent requires parent JS.
- Imported, renamed, and re-exported component paths resolve through rustc's
  selected component ABI rather than token spelling.

## The Signal API

See [`api-reference/signals.md`](api-reference/signals.md) for the full reference.
Quickref:

```rust
let count = signal(0_i64);
count.get()              // read in reactive position; auto-tracks
count.set(42)            // imperative replace; use in handler bodies
count.update(|c| c + 1)  // closure update; use in handler bodies
```

Inside an `on_*` handler body the closure `|_| count.update(|c| c + 1)` is the
canonical handler shape.

## Typed state shared by independent islands

`signal(...)` is island-local. For synchronous UI state that must be read or
written by separate island roots, use a typed page-scoped provider:

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

`WorkbenchChildren` is one direct analyzed component whose render tree may
contain `ThemePanel`, `CanvasPreview`, and `Inspector`. The v1 provider grammar
requires one direct component struct literal in `children`; wrap a group of
siblings in that component rather than passing an `html_sync!` fragment.

Consumers acquire the nearest provider inside their own
`#[rsjs(component, client)]` prelude:

```rust
let workbench = rsjs::shared::<WorkbenchState>();
let button_bg = workbench.button_bg();
let is_green = rsjs::memo(|| button_bg.get() == "#2f6b4f");
```

The provider is lexical during SSR but does not render a DOM node. SSR records
the exact provider scope in each island's adjacent hydration blob; the page
runtime then gives every island bound to that scope the same field cells. Each
top-level struct field invalidates independently. Nested providers use
nearest-provider semantics, sibling providers do not share values, and an
island that hydrates later observes current live state.

The state struct can be defined in another module or crate. HIR records the
type path and field accessor syntax; generated Rust asks the type's
`SharedState` metadata for its protocol key/schema and lets rustc validate the
typed handle. The explicit namespaced `type_key` is the browser/server ABI;
module paths and `TypeId` are not protocol identities.

Use `rsjs::batch(|| { ... })` for several related writes. Values are
synchronously readable while DOM effects remain deduplicated into one
microtask flush.

Provider declaration is server-side lexical topology, emits no DOM wrapper,
and does not by itself originate browser work. `rsjs::shared::<T>()` is the
client origin and therefore belongs in a `#[rsjs(component, client)]` consumer.
In v1, the provider must be statically present, `state` must be a named
`SharedState` struct literal, and `children` must be exactly one direct
component struct literal. Do not put provider topology in loops, conditions, or
deferred regions. A provider seed cannot read an enclosing live value: use a
static server seed, make SharedState the source of truth, or update the shared
field explicitly from a handler/effect. The same `type_key` with a different
schema ABI fails before owner activation.

This does not widen DOM authority. An island can react inside its own rendered
root; arbitrary `document` queries, computed-style inspection, and mutations
of foreign server-rendered nodes still require an explicit tool/browser
capability layer.

Use queries/mutations instead when the source of truth is asynchronous or
server-owned and needs loading, error, staleness, invalidation, or refetch
semantics.

## What's allowed inside `{ ... }` slots

See [`api-reference/slot-grammar.md`](api-reference/slot-grammar.md). The
core surface is:

- Literal values (`42`, `"hello"`, `true`)
- Ordinary server locals render once when their dependency closure contains no
  inbound or local client value
- Inbound field reads such as `self.title` participate as `LiveRead`; exact
  aliases and projections retain that provenance
- Signal reads (`count.get()`, `username.is_non_empty()`)
- Conditional sections (`{if pred { html_sync! { ... } }}`) — at most one `{if}` per parent element
- Single-arg closures in `on_*` slots (`{|_| signal.update(|v| !v)}`)
- Ordinary renderable values constructed in the prelude for opaque SSR
- `rsjs::island(Component { ... })` for explicit nested island boundaries
- Calls to functions annotated with `#[rsjs::client]`; normal Rust imports,
  aliases, and qualified paths are valid

A direct struct literal in an analyzed slot is a component edge and must
implement the generated component ABI. Construct an arbitrary non-component
renderer in the prelude and render its local instead. An unbound direct path
such as `{Badge}` or `{crate::ui::Badge}` is the corresponding unit-component
edge; normal imports, renamed imports, and re-exports work. A tracked prelude
or loop local remains an ordinary rendered value. An opaque expression is
accepted only while no browser-reachable dependency crosses it; RSJS fails
closed rather than freezing a live value. `server_only(...)` remains an
explicit snapshot/compatibility boundary, not the normal route.

Annotate each portable, origin-free helper once with `#[rsjs::client]`. A
reachable call creates its helper edge automatically, while an uncalled helper
does not ship. These helpers cannot acquire state, perform I/O, or hide browser
capabilities; use the corresponding compiler-known operations for effects.
The current standalone-helper ABI accepts ordinary synchronous Rust-ABI free
functions with fixed, plain-identifier parameters. Parameters may use `bool`,
`i32`/`u32`/`i64`/`u64`, `f64`, `String`, `&str`, and the supported
`Option<T>`/`Vec<T>`/borrowed-slice wrappers; returns must be owned (use
`String`, not `&str`, and `Vec<T>`, not a slice). `f32` is rejected until the
client IR can preserve Rust's per-operation f32 rounding. `const`, `unsafe`,
`extern`, variadic, generic call turbofish, and non-plain parameter patterns are
rejected rather than silently changing the Rust contract.

Helper-local scalars may use `let mut` plus direct or checked compound
assignment, including inside statement-level `if` / `else` branches. The
generated server evaluator and browser module use the same stable local type
and checked integer domain.

Helper bodies may also use ephemeral named-field structs as local value
objects. Construction, scalar field reads, and field assignment on a `let mut`
local lower to plain JavaScript objects and own-property operations; the same
body is evaluated with an internal object value during SSR. The object itself
cannot be a helper parameter, return value, signal value, callback payload, or
helper-call argument, so it never crosses a serialization boundary. Keep the
public helper signature in the scalar/`Option`/`Vec` set:

```rust
struct MenuState {
    open: bool,
    label: &'static str,
}

#[rsjs::client]
fn toggle_menu() -> bool {
    let mut menu = MenuState {
        open: false,
        label: "Menu",
    };
    menu.open = !menu.open;
    menu.open && menu.label == "Menu"
}
```

Give numeric field seeds an explicit supported width (`0_i32`, `0_u32`,
`0_i64`, or `0_u64`) or initialize them from an already typed helper value.
The macro cannot inspect the surrounding struct declaration's resolved field
type, so it rejects numeric field operations whose Number/BigInt and overflow
domain would otherwise be guessed. Checked `+=`, `-=`, `*=`, `/=`, and `%=`
are supported on fields with a proven domain.

For a closed class-like authoring surface, apply `#[rsjs::client]` to an inline
module. The module must contain at least one named-field struct and one public
transport-safe helper root. Public roots are marked as client helpers
automatically. Direct `new(...) -> Self` constructors and borrowed read/mutation
methods with supported typed value arguments become private relocation-linked
helper definitions with an explicit object receiver. Every method argument is
evaluated once by ordinary function-call semantics.
Method bodies may use checked scalar calculations, scalar `let` / `let mut`
locals (including lexically scoped branch locals), assignment-oriented `if` /
`else` branches and, on `&mut self`, named-field mutation. A reachable method
body is emitted once even when several public roots call it:

```rust
#[rsjs::client]
pub mod menu_logic {
    pub struct Menu {
        open: bool,
    }

    impl Menu {
        pub fn new(open: bool) -> Self {
            Self { open }
        }

        pub fn set_open(&mut self, enabled: bool) {
            if enabled {
                let next = !self.open;
                self.open = next;
            } else {
                self.open = false;
            }
        }

        pub fn is_open(&self) -> bool {
            self.open
        }
    }

    pub fn toggled() -> bool {
        let mut menu = Menu::new(false);
        menu.set_open(true);
        menu.is_open()
    }
}
```

In this default mode, constructors and methods are plain private JavaScript
functions over nominally checked object records; an original-body source
checker still makes rustc validate the authored constructor and method calls.
Statementful and argument-bearing read methods may be nested in public root
expressions. Loops, general `match`, object escape, tuple/index fields,
nested-field assignment, and whole-object
reassignment remain unsupported. Branch locals retain Rust lexical visibility;
using one after its branch is rejected before JavaScript emission. Passing an
object to an ordinary function is likewise rejected, so only compiler-generated
constructor, field, and borrowed-method edges can carry the nominal object.
Compare and transform scalar fields; direct object equality is rejected because
JavaScript identity equality would not preserve Rust value semantics.
An authored by-value alias is lowered as a fresh record. This preserves Rust's
independent `Copy` behavior when a copied object and its source are both used;
the retained source-checking body still lets rustc reject an illegal reuse of a
non-`Copy` move.

When application-owned JavaScript actually needs `new`, `instanceof`, or a
class API for a third-party integration, opt in with
`#[rsjs::client(class)]`:

```rust
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
            let next: i32 = self.count + amount;
            self.count = next;
        }

        pub fn value(&self) -> i32 {
            self.count
        }
    }
}
```

Every public struct in such a module becomes an ESM `export class`; it must
have one public `new(...) -> Self`. Public borrowed receiver methods become
class methods, while private inherent methods remain private lowered
functions. The generated class owns a JavaScript private field containing the
same nominal record used by linked helpers, so state persists across calls and
does not leak through enumerable properties. Method-to-method calls remain in
the closed class module. External helper calls are rejected because this asset
does not use the component relocation linker.

`rsjs::manifest().client_classes` lists the generated modules and exports.
`rsjs::client_class_filename(module_path)` returns the deployment filename;
`rsjs::write_manifest_assets(...)` writes it with the other RSJS assets. The
JavaScript application owns each instance's lifetime: retain it for as long as
the integration needs it and call an authored cleanup method explicitly.
Trait impls, including `Drop`, are rejected so cleanup cannot disappear during
lowering. This class ABI is not an implicit hydration-scoped controller and is
never serialized into a signal, prop, or payload.

`#[rsjs::client_module]` may group helpers for bundling or namespace validation,
but is not required and does not grant eligibility. Prefer small domain groups
such as money formatting, date formatting, or upload CSV preview helpers over a
broad shared utilities group.

For utility crates, prefer a server-first split:

- Put rich date, money, CSV, JSON, and locale logic in ordinary Rust modules
  and render the result through SSR when it does not need to update in the
  browser.
- Annotate only small, pure browser-needed functions with `#[rsjs::client]`,
  such as money display, relative date, CSV preview, or generated
  form-validation helpers. Group them with `#[rsjs::client_module]` only when
  bundling or namespace validation benefits from it.
- Keep client helper inputs client-shaped: cents/minor units, ISO timestamps,
  enum tags, short strings, or JSON Pointer projections. Do not make a simple
  label pull a full parser, decimal library, or timezone database into the
  page.
- Treat server and browser helpers as separate explicit surfaces. A normal Rust
  `format_money(...)` helper is SSR-only; a reachable
  `money_display::format_usd_cents(...)` helper call is what imports browser JS.
- Do not hide broad dependencies behind common bundles. If a feature needs an
  Intl wrapper, timezone table, decimal parser, CSV parser, or JSON editor
  helper, put that in a named opt-in bundle so ordinary SSR pages stay small.
- Published utility bundles should expose an obvious size/capability profile.
  Today, `cargo proa rsjs explain` shows which reachable helper bundle shipped
  and which declared bundles were unused.

## When the island has props

Inbound values have explicit temporal meaning:

1. **Render or project the field directly** when the child should observe the
   current parent value. `{self.label}`, `class={self.class}`, and an exact
   alias such as `let label = self.label;` are `LiveRead` participation.
2. **Pass the field to `signal(...)`** when it should seed child-owned state.
   `signal(self.initial)` is `SeedOnce`; later parent changes do not reset the
   local cell.
3. **Wrap the field in `server_only(...)` or `server_attr(...)`** only when the
   component contract deliberately requires a server snapshot. A live caller
   is rejected instead of being silently frozen.

For generic components, rsjs should not require `T: ClientEncode` just because
`T` is a prop type. The codec bound appears only when final linking makes that
value browser-reachable—for example through `LiveRead`, `SeedOnce`, a handler
payload, or a query/mutation value. A proven server-only occurrence keeps its
ordinary Rust rendering path and needs no browser codec.

A reachable `#[rsjs::client]` helper call is also client-reactive IR. If the
helper needs a struct field, seed that field first
(`let cents = signal(self.cents);`) and pass `cents.get()` to the helper.
Calling `money::format(self.cents)` directly from a template slot is rejected
because `self.cents` is still server-owned there.

```rust
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for ProductChip {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let label = self.label;             // inbound LiveRead alias
        let active = signal(self.initial);  // SeedOnce child-owned state
        html_sync! {
            <button on_click={|_| active.set(!active.get())}>
                {label}
            </button>
        }
        .render(cx)
    }
}
```

The macro tracks provenance through these locals automatically; you do not tag
them. A static caller still produces static HTML for `label`, while a linked
live caller observes the current value without resetting `active`.

## Wiring the island into a page

In a parent template, an island is invoked the same way as any Proa component:
struct construction inside an `html_sync!` slot.

```rust
html_sync! {
    <html>
        <body>
            { Counter }
            { WatchHeart }
            { ProductChip { label: "Sale" } }
        </body>
    </html>
}
```

That is all page code needs to do. `proa_framework_axum` snapshots the response's
recorded `RsjsRenderArtifacts` after rendering and emits the shared
`rsjs::PageRuntimePlan` automatically across sync, async, and
streaming response paths. Pages and components must not read
`cx.rsjs_islands_used()` or append runtime tags themselves.

The shared planner keeps interaction islands eager, defers visible/idle/media
islands, scopes the query runtime to the eager graph, includes reachable client
modules and component fragments, and emits exact module preloads. A pure
`ServerOnly` result records nothing and therefore emits no runtime. If a
server-only parent renders an independent hydrated child, the child records its
own artifact and only that child's assets are planned.

Custom framework adapters should snapshot `cx.rsjs_artifacts_snapshot()` at the
response boundary and pass `artifacts.rendered_islands()` to
`Manifest::page_runtime_plan(...)`. Use
`Manifest::page_runtime_plan_with_urls(...)` with an
`RsjsAssetUrlResolver` for hashed or CDN URLs. They must also emit
`artifacts.query_seed_table_json()` as the inert `rsjs-query-seed-table` before
the plan when it is present, and
`artifacts.shared_state_seed_table_json()` as the inert
`rsjs-shared-state-seed-table` before any consuming owner can activate. The
resolver changes response-plan URLs only:
hashed/CDN asset serving must also rewrite the generated ESM sources' runtime
and dependency import specifiers to those resolved URLs, or preserve stable
canonical aliases. Cached fragments carry and replay their complete
`RsjsRenderArtifacts`; streamed boundaries carry committed artifact deltas and
share one `PageRuntimeEmissionState`. Low-level out-of-order adapters emit each
boundary's query-seed and plan-delta metadata immediately before its swap
frame; the frame dispatches `proa:boundary-ready`, so appending metadata after
it is too late.

## Verifying your island

```bash
# Compiles + runs the island's render in isolation
cargo test -p <your-crate> --lib

# Inspect the generated SSR HTML byte-for-byte
let html = proa_core::render_to_string(MyIsland);
# Expected shape:
#   <rsjs-island data-rsjs-island-root="myisland_<hash>">
#     ...with data-rsjs-handler="N", data-rsjs-bind="N", data-rsjs-cond-then="N:0" markers...
#   </rsjs-island>
#   <script type="application/json" data-rsjs-island="myisland_<hash>">
#     {"signals":[<initial-values>]}
#   </script>
```

If the SSR shape is right, hydration is too: the runtime and per-island JS are
generated mechanically from the same IR.

## Common errors

See [`footguns.md`](footguns.md) for the full gallery. The top three:

1. **`X is not a recognised IR primitive or linked #[rsjs::client] helper`** — A browser-reachable value crossed an opaque operation. Keep the operation server-only in the prelude, or annotate a portable helper with `#[rsjs::client]`.
2. **`dynamic href attributes are not supported`** — Keep island URLs static or render vetted server-side values with `rsjs::server_attr(...)`.
3. **`expected identifier, found keyword` inside `<svg>`** — Pre-existing bug, fixed: keyword tag names like `<use>` now parse. If you still see this, your `proa_dsl` is stale.
