# Compiler-error gallery

Each entry: **the error you'll see** → **what triggered it** → **how to fix**.
Compiled from the rsjs analyser, html_sync! parser, and patterns we hit
authoring real islands and components.

If you hit an error not in this list, the diagnostic itself is the canonical
authority — rsjs/proa errors are intentionally written to be agent-quality
(they describe the constraint and point at the fix). Read them carefully
before guessing.

---

## `#[rsjs(component)]` (rsjs_macros)

### "X is not a recognised IR primitive or linked `#[rsjs::client]` helper"

**Triggered by:** a browser-reachable value flows through a function or
construction the analyzer cannot lower or link. An opaque server expression is
allowed when it has no live dependency; it becomes an error when accepting it
would silently snapshot reactive state.

```rust
// ✗ wrong
{ format_label(count.get()) }
```

**Fix options:**
1. If the value is server-only, compute it before the analyzed template and
   render the local:
   ```rust
   let label = format_label(self.initial_count);
   html_sync! { <span>{label}</span> }.render(cx)
   ```
2. If it is portable, origin-free client computation, annotate the helper
   definition once:
   ```rust
   #[rsjs::client]
   pub fn subtotal(price: i64, quantity: i64) -> i64 {
       price * quantity
   }
   ```
Reachable calls are linked automatically, including through normal Rust
imports and aliases. `#[rsjs::client_module]` is optional grouping; the
consuming component does not declare an allowlist. Source lint accepts a
statically spelled helper path provisionally because it cannot resolve imports;
the semantic linker rejects the edge if the resolved function lacks
`#[rsjs::client]`.

Using removed component-level `clients(...)` syntax produces
`rsjs/removed-clients-allowlist` in `cargo proa lint` and a matching compiler
migration diagnostic.

---

### "event current-target marker `Any` does not match binding target `Document`"

**Triggered by:** leaving the event target generic at its `dom::Any` default
on an `on_document_*` or `on_window_*` binding. It can also appear when an
element marker, such as `dom::HtmlDivElement`, is used for a global handler.
The event generic describes the actual `current_target()`, not the template
element that owns the global registration.

```rust
// Wrong: MouseEvent defaults to the element-only dom::Any marker.
on_document_click={|event: rsjs::MouseEvent| {
    let _ = event.current_target_attr("data-menu-id");
}}
```

**Fix:** parameterize the event with the exact global target:

```rust
use rsjs::web::dom;

on_document_click={|event: rsjs::MouseEvent<dom::Document>| {
    if event.closest_attr("data-menu-id") == "main" {
        // Read the bubbling target through event accessors.
    }
}}
on_window_resize={|_: rsjs::Event<dom::Window>| {
    width.set(rsjs::window::inner_width());
}}
```

`CurrentTarget<dom::Document>` and `CurrentTarget<dom::Window>` intentionally
do not expose element attributes, ids, values, or geometry. Use bubbling-target
event accessors or a typed `NodeRef<E>` when element access is required.

---

### "`rsjs::server_only(...)` cannot capture reactive signal `name`"

**Triggered by:** referencing a `signal(...)` local from inside a `server_only`
slot.

```rust
let count = signal(0);
{ server_only(format!("count is {}", count.get())) }   // ✗ captures count
```

**Fix:** `server_only` is a compatibility boundary for opaque SSR content that
runs once on the server and stays untouched on the client. If the value needs
to update, render the reactive read normally:

```rust
<span>{count.get()}</span>            // ✓ regular reactive read
```

Current source lint may report this as `rsjs/unsupported-capture`. In v2,
the same compatibility code also covers hidden client primitives or browser
helpers inside opaque SSR boundaries. The canonical code should be
`rsjs/template-reactive-capture-in-server-only`.

---

### "`rsjs/template-client-state-in-opaque-server-statement`"

**Triggered by:** ordinary server control flow or an opaque statement reads a
compiler-owned client local:

```rust
let count = rsjs::signal(0_i64);
if count.get() > 0 {
    println!("client state");
}
```

Ordinary server statements themselves are supported and remain in source
order, including `?`, early returns, and statements around `await`. The failure
is the hidden client-state read: RSJS cannot make arbitrary server control flow
reactive in the browser.

**Fix:** move the read into the analyzed template, a handler, an effect, or
another supported client expression. Keep the surrounding ordinary server
work in the prelude.

The parallel
`rsjs/template-browser-operation-in-opaque-server-statement` diagnostic means
a visibly named browser helper was called from ordinary server code; move that
call into a supported client position.

---

### "rsjs only supports unary `!` in reactive expressions"

**Triggered by:** any other unary operator (`-`, `*`, etc.) in a reactive
predicate.

**Fix:** rewrite using `BinOp` operators (which the IR does support):
arithmetic comparisons, `==`, `!=`, `<`, `>`, etc.

---

### "`#[rsjs]` requires an explicit `component` marker"

**Triggered by:** writing bare `#[rsjs]` or an option-only attribute such as
`#[rsjs(strategy = "visible")]`.

**Fix:** mark the real render-trait impl as an analyzed component. Add
`client` if it originates browser behavior:

```rust
#[rsjs(component, client, strategy = "visible")]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        html_sync! {
            <button on_click={|_| count.update(|n| n + 1)}>{count.get()}</button>
        }
        .render(cx)
    }
}
```

### "`#[rsjs(component)]` requires a real `WebRenderSync` or `WebRender` trait impl"

**Triggered by:** placing the attribute on an inherent impl:

```rust
#[rsjs(component, client)]
impl Counter {
    fn render(self) { /* ... */ }
}
```

**Fix:** author the complete trait impl, context parameter, return type, and
final `.render(cx)` chain. RSJS analyzes that Rust implementation; it does not
generate the trait plumbing.

### "`client` is only valid with `component`"

**Triggered by:** writing `#[rsjs(client)]`.

**Fix:** write `#[rsjs(component, client)]` on a real render-trait impl. Use
participant-only `#[rsjs(component)]` only when the component does not declare
local signals, handlers, timers, queries, mutations, shared-state acquisition,
node refs, or browser work.

### "`async fn render` is not supported"

**Triggered by:** using the removed async-function sugar on `WebRender`.

**Fix:** author the trait's real signature and return one pending render:

```rust
#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for AsyncIsland {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let data = cx.load::<Data>(self.key).await?;
            html! { <output>{data}</output> }.render(cx).resolve().await
        })
    }
}
```

---

### "`mutation(...)` requires an async `WebRender` island"

**Triggered by:** `rsjs::mutation(...)` inside a `WebRenderSync` impl.

**Fix:** change `impl WebRenderSync<...>` to `impl WebRender<...>`, use
`html!`, and author the canonical `RenderOutcome::pending(async move { ... })`
body shown above. The macro picks sync versus async from the real trait method
shape; nothing is auto-switched.

`query_with_initial(...)` is different: it may stay in `WebRenderSync` when the
initial value is already available. Switch to `WebRender` only when computing
that initial value actually needs `.await`.

---

### "expected identifier, found keyword `use`" (or `for`, `type`, `match`, etc.)

**Should not occur.** The proa_dsl parser uses `Ident::parse_any`, which
accepts keyword-shaped tag names (`<use>`, `<for>`, `<type>`).

If you see this, your local `proa_dsl` is stale. Run `cargo clean -p proa_dsl
&& cargo build` to pick up the keyword-tag fix.

---

## `html_sync!` (proa_macros)

### "expected element name, found ..."

**Triggered by:** trying to use a tag name with characters other than
`[A-Za-z0-9_-]`. Tag names with hyphens are spelled with `-` directly
(`<my-button>`); underscores in source map to hyphens in output.

---

### "void element cannot have children"

**Triggered by:** giving a child to a self-closing element like `<input>`,
`<br>`, `<img>`, `<meta>`, etc.

**Fix:** use `<input ... />` (self-closing) or remove the children. The list
of void elements is the standard HTML spec list; `is_void_element` in
[`proa_dsl/src/parse.rs`](../../proa_dsl/src/parse.rs) is canonical.

---

## Component / `WebRenderSync` traits (proa_core)

### "the trait bound `MyComponent: WebRenderSync<L>` is not satisfied"

**Most common cause:** a rendered generic child lacks a matching loader bound,
or the component implemented `WebRender` when the caller expected
`WebRenderSync`.

```rust
impl<L, C> WebRenderSync<L> for MyComponent<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="my-component">{self.children}</div>
        }
        .render(cx)
    }
}
```

For a declarative single-element component,
`#[derive(ProaComponent)]` can generate the trait and generic-child bounds.
Custom, multi-root, or prelude-heavy components implement the render trait
directly. Constrain only types that flow into child positions; ordinary state
generics do not need render bounds. `WebContext<L>` uses the default owned
`RenderBuffer`; only a deliberately custom-writer component needs a second
`B: WriteBuf` parameter.

---

### "cannot find `to_async` in module `proa_core`"

**Triggered by:** placing a `WebRenderSync` value as a slot child inside an
`html!` (async) tree without the `to_async` adapter.

**Fix:**
```rust
html! {
    <div>{ proa_core::to_async(my_sync_component) }</div>
}
```

If `my_sync_component` uses `#[derive(ProaComponent)]`, the
`#[component(..., async_render)]` opt-in generates its zero-cost async bridge
and removes the call-site adapter. Explicit custom components use
`proa_core::to_async(...)`.

---

## Markdown render traits (proa_core)

### "`AsyncMd<...>: MdRenderSync<B>` is not satisfied"

**Triggered by:** using async-capable `md!` inside an `MdRenderSync` component
or passing its dynamic output to a synchronous Markdown renderer.

**Fix:** use `md_sync!` for a synchronous component:

```rust
md_sync! { r###"
# {title}
"###
}
.render_md(cx)
```

If the component really needs async children, implement `MdRender` and drive
the `md!` output with `render_md_child(...).await` or `.resolve().await`.

### "`MyChild: MdRender<B>` is not satisfied" inside `md!`

**Triggered by:** interpolating a custom sync-only `MdRenderSync` child into
the async-capable macro.

**Fix:**

```rust
md! { r###"
Result: {proa_core::to_md_async(my_sync_child)}
"###
}
```

The inverse is impossible: an async `MdRender` child cannot be placed inside
`md_sync!`.

---

## Component conventions

### "Component renders without `data-component` attribute"

Not a compile error, a convention violation. Add `data-component` to your
component's root element so DOM-level testing tools can locate it.

```rust
html_sync! {
    <div data-component="my-component" class={classes}>
        ...
    </div>
}
```

If your component has variants, also add `data-variant={variant.as_str()}`.
See [`api-reference/conventions.md`](api-reference/conventions.md).

---

## Shared-state runtime errors (not compiler errors)

Nothing in this section is a compile-time diagnostic: `cargo check` and
`cargo build` pass clean. The first three errors are `WriteError`s raised by
`WebContext` while the server renders the page, so they surface on the first
request that renders the offending region (typically as a 500 from the
handler), not at build time. The last group is thrown by the browser runtime
during hydration.

### Compile time: "rsjs/shared-state-division-width"

Fixed-width integer division needs the complete Rust domain, not merely the
browser representation. `i32`, `u32`, `i64`, and `u64` differ in range and
signed `MIN / -1` is an overflow. A shared field's domain is not available from
its accessor tokens, so direct shared operands require an explicit turbofish:

```rust,ignore
let per_item = rsjs::div_int::<u64>(total.get(), count.get()); // checked u64
let average  = rsjs::div_f64(total.get(), count.get());        // fractional f64
```

Without the turbofish, `div_int` is accepted only when both operands
independently prove the same supported domain. A typed literal beside a `Dyn`
shared read is not enough; write `rsjs::div_int::<i32>(value.get(), 2_i32)`.
Shared reads require the explicit primitive width at every `div_int` call;
earlier specialization does not make a later bare helper call valid. Use the
primitive type itself (or `core::primitive::*`/`std::primitive::*`), not a
reference, wrapper, alias, or lookalike path.
The generated server type witness also rejects an explicit width that disagrees
with the actual shared-state field.

Declaration order is never the cause of a shared-state compile error: the state
struct may be defined anywhere, in any crate, reached by any spelling.

### SSR render-time: "rsjs/shared-state-missing-provider"

**Raised when:** the server renders an island that calls
`rsjs::shared::<T>()`, but no matching
`SharedStateProvider { state: T { ... }, children: ... }` is active around
that render region. A missing provider cannot always be proven at macro
expansion, so the build succeeds and the first request fails.

The diagnostic names the island, Rust type, protocol `type_key`, and provider
syntax. Put one provider at the page/region owner; do not thread the generated
handle through every component prop.

### SSR render-time: "rsjs/shared-state-schema-mismatch" or "type-key-conflict"

**Raised when:** at request time, the provider and consumer on the rendered
page were compiled against different schemas for the same
`#[rsjs(type_key = "...")]`, or two Rust structs claim one key. Rebuild all
consumers together after changing a field, and give unrelated schemas distinct
namespaced keys. `TypeId`, source paths, and module paths are not protocol
identities.

### SSR render-time: "rsjs/shared-state-late-stream-provider"

**Raised when:** while streaming a response, a provider is created inside a
deferred boundary after the shell's shared-state seed table was emitted. Move
the provider above the boundary. A deferred island may consume an outer
provider because its render context captures the active provider stack; it
cannot introduce a new page scope after publication.

### Browser `rsjs/shared-state-*` hydration errors

These structured runtime errors indicate stale or malformed page metadata, not
ordinary application control flow:

- `abi-mismatch`, `schema-mismatch`, or `scope-collision`: the page and island
  assets were built against incompatible runtimes/schemas. Rebuild and deploy
  them atomically, then reload the page.
- `missing-binding`, `missing-scope`, or `missing-field`: an island's adjacent
  state blob no longer agrees with the page seed table. Check that response
  rewriting/caching did not mix HTML from different builds, and that a
  hand-assembled page actually embedded the seed-table script (framework
  responses embed it automatically; manual embedding must include it, see
  `rsjs/README.md`).
- `invalid-table`, `invalid-seed`, `duplicate-scope`, or
  `seed-decode-failed`: the page seed script was corrupted or emitted twice.
- `mixed-width-arithmetic`: a legacy/schema-less runtime path combined a
  BigInt-backed value with a fractional Number, which has no exact BigInt
  image. New fixed-width integer arithmetic is domain-checked before emission.
  If fractional behavior is intentional, use `rsjs::div_f64(a, b)` so both
  sides deliberately become Numbers.

Each thrown `RsjsSharedStateError` has a stable `.code` and structured
`.details`; the complete seed table is validated before any store is installed,
so a bad response cannot leave a partially initialized page registry.

---

## When the rendered HTML is wrong

### Expected `<rsjs-island>` markup is missing

If the component has no reachable client roots, v2 classifies it as
`ServerOnly` and intentionally emits no wrapper, seed script, runtime asset, or
page runtime tags. Use `cargo proa rsjs explain ...` or
`cargo proa lint --use-rsjs-strategy-cache` to confirm whether this is the
compiler strategy. If browser behavior is intended, add a reachable client root
such as an `on_*` handler, `bind_*`, signal read/write, query/mutation use,
node ref, browser helper, or reachable `#[rsjs::client]` helper call.

### `<rsjs-island>` and seed `<script>` are present, but click does nothing

**Most likely:** the per-island JS isn't reaching the browser. Check:

1. The page response includes `<script type="module" src="/rsjs-runtime.js">`
   or a deferred bootstrap script for the island's hydration strategy.
   `proa_framework_axum` adds this automatically from the response's
   `RsjsRenderArtifacts`; a custom adapter must use
   `Manifest::page_runtime_plan(...)` at its response boundary.
2. There's a `<script type="module" src="/rsjs/island/<asset_id>.js">` for *your specific island* (asset_id matches the SSR `<rsjs-island data-rsjs-island-root="...">`).
3. The browser console isn't reporting a 404 on either of those URLs.
4. If the page is loaded from a `blob:` URL or a different origin than the asset server, the asset server needs CORS headers (`Access-Control-Allow-Origin: *` for dev).

### Initial render shows the wrong conditional branch

**Most likely:** the SSR pre-applied `style="display:none"` to the inactive
branch correctly, but `bind_conditional` in the runtime is restoring `none`
instead of `''` on show. This was fixed in `rsjs/runtime/rsjs-runtime.js`
(line 408 region) — make sure your runtime is up to date.

### Multiple handlers in one island fire the wrong action

**Most likely:** you're on a stale `rsjs_macros` from before the
`data-rsjs-handler` NodeId fix (`382d4d0`). Symptom: clicking button A
toggles signal B's state. Update `rsjs_macros`.

---

## CLI quick checks

```bash
# Verify all rsjs unit + integration tests still pass
cargo test -p rsjs_macros -p rsjs --all-targets

# Run the macro on a single test fixture and dump the generated impl
cargo expand -p <your-crate> --lib <module>::<MyIsland>

# Run the playground and inspect a real island's SSR HTML + per-island JS
cargo proa run -p proa_demo --bin proa-playground --features server
# then: open http://localhost:5000/playground/ebay/ebay-srp
```
