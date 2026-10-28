---
name: add-island
description: Scaffold a new rsjs island (interactive client-hydrated component) following docs/agent/rsjs-islands.md. Use when the user asks to add interactivity, hydration, signals, bind_value forms, or an RSJS-analyzed render impl.
---

# Add an rsjs island

For authored HTML, Markdown, XML, plain-text, or JSON documents, read
[`docs/agent/document-rendering.md`](../../../docs/agent/document-rendering.md).
Use the matching macro or typed serializer in handlers and cached documents as
well as components. Give relevant page types an `MdRenderSync` implementation;
do not substitute parallel Markdown files, string replacement, or hand-built
markup. Keep runtime JSON typed; use `const_json!` for literal JSON.
For multiple render targets, read the guide's shared-content workflow first.
Keep both implementations with the owning page/component, not in a separate
agent-facing page. Put copy, labels, URLs, and collections in shared props or
one content constructor before writing either renderer. Compose explicit
Markdown sections for the core page structure; do not duplicate literals or
convert the entire HTML view. Preserve existing copy and `PageMeta`. The
linter is a backstop, not proof that separately written strings stay in sync.

When the user wants interactivity — a click handler, a signal-driven
update, a bind_value form input, anything that needs JS in the browser —
write a complete `#[rsjs(component, client)] impl WebRenderSync<L>` block
on a struct. You author the trait, method signature, return type, context
parameter, and final `.render(cx)` call. The attribute analyzes that body so
the server renders HTML and a companion JS bundle can hydrate the same shape.

Full authoring guide:
[`docs/agent/rsjs-islands.md`](../../../docs/agent/rsjs-islands.md).
Compile-error catalog: [`docs/agent/footguns.md`](../../../docs/agent/footguns.md).
Signal API surface: [`docs/agent/api-reference/signals.md`](../../../docs/agent/api-reference/signals.md).

## Browser behavior defaults

Use RSJS for application browser behavior whenever supported. Check existing
bindings, client helpers, and integration components before writing raw
JavaScript. A compile error or a shorter script is not evidence that RSJS
cannot do the job. Only a necessary, documented gap justifies a minimal raw
JavaScript bridge; follow the [RSJS-first policy](../../../docs/agent/rsjs-islands.md#prefer-rsjs-for-browser-behavior).

## Minimal island

```rust
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

pub struct Counter { pub initial: i64 }

#[rsjs(component, client)]
impl<L: proa_core::DataLoader>
    proa_core::WebRenderSync<L> for Counter
{
    fn render(self, cx: &mut proa_core::WebContext<L>)
        -> Result<(), proa_core::WriteError>
    {
        let count = signal(self.initial);
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

The attribute expansion:

- Preserves the authored `WebRenderSync` contract for SSR.
- Compiles the body to a per-island JS bundle.
- Registers `&'static IslandManifestEntry` in `ISLAND_REGISTRY` via linkme
  for the framework adapter to discover at startup.

## Signal API

```rust
let count = signal(0_i64);     // initial value (carried into the browser)
let n = count.get();            // read (subscribes the surrounding render slot)
count.set(5);                   // replace
count.update(|c| c + 1);        // functional update
```

Update closures must be **single-expression**: `|c| c + 1` works,
`|c| { let n = c + 1; n }` does not. The macro analyser rejects
block-bodied closures with intermediate statements. Rewrite as a
chained expression, or compute the value before the call and use
`set`.

When a signal initializer comes from `self.<field>`, it crosses the
SSR/hydrate boundary as a one-time seed for that child occurrence. Later parent
updates do not reset the child-owned signal. A direct `self.<field>` read in
text, an attribute, or a branch is different: it can remain a live inbound
value when the parent supplies one.

## Allowed inside analyzed render bodies

- `signal(...)`, `.get()`, `.set(...)`, `.update(|c| <single-expr>)`.
- Named `rsjs::event_handler(...)` locals and supported inline event closures.
- Simple expressions in `{ ... }` slots. Server `.await` belongs only inside
  the pending future returned by a canonical `WebRender` implementation.
- `on_click` / `on_input` / `on_submit` / `bind_value` / `bind_checked` /
  `bind_selected` event handlers and form bindings.
- Supported `if`/`else`, `match`, local bindings, and bounded loops in handler
  bodies, as documented in `docs/agent/rsjs-islands.md`.
- Loops over compile-time-known data.

## Not allowed inside analyzed render bodies

- Block-bodied update closures with intermediate `let` statements.
- Arbitrary Rust inside handlers beyond the documented client-language subset.
- `async fn render` sugar. Use the real `WebRender` signature and return
  `RenderOutcome::pending(async move { ... })`.
- Runtime method dispatch on types the macro analyser cannot see through.
- `cx.something()` from inside an event handler — `cx` is server-only.
  Capture values into the closure instead.

When in doubt, write smaller expressions. The macro is intentionally
conservative; expanding what's allowed has been a recurring source of
bugs.

## Mutations vs queries vs local state

- **Local state**: `signal(...)`. Lives in the browser, doesn't talk
  to the server. Use for UI like open/closed, count, toggle.
- **Queries**: load the initial value in the pending server render, then declare
  `query_with_initial(QueryKey::new((...)), initial, FetchRequest { ... },
  QueryOptions { ... })`. Reachable query observers share the page cache.
- **Mutations**: declare `mutation(QueryKey::new((...)), FetchRequest { ... },
  invalidation_keys)` in the same canonical async `WebRender` shape.

## Event delegation + microtask batching

The rsjs runtime uses event delegation (one listener per event name
per island root) and microtask-batched signal updates. Implications:

- Many islands on a page = constant-size listener count.
- `signal.update()` called N times synchronously triggers ONE
  re-render, not N. Don't structure code around manual batching.
- Reachability keeps each island payload scoped to the browser behavior it
  actually uses. Measure generated output instead of relying on fixed byte
  counts, which change as the compiler and runtime evolve.

See [`rsjs/OPTIMIZATIONS.md`](../../../rsjs/OPTIMIZATIONS.md) for
what's done and what's queued (hydration strategies, signal_array,
inline tiny islands).

## Common mistakes

- **Forgetting to bring `proa_core::WebContext` / `WebRenderSync` into scope**
  at the impl site. The macro needs them.
- **Using `let mut` in render for state.** Use `signal(...)` instead;
  `let mut` only mutates server-side and won't survive the hydrate.
- **Calling `cx.foo()` from inside an event handler closure.** `cx` is
  server-only. Capture data as a value into the closure.
- **Block-bodied update closures** (`|c| { let x = c + 1; x }`) — error.
  Rewrite as `|c| c + 1` or compute outside and `set`.
- **Unsupported arbitrary Rust in handler bodies.** Use the documented
  statement/control-flow subset or move portable computation into a function
  annotated once with `#[rsjs::client]`. `#[rsjs::client_module]` is optional
  grouping; consumers do not declare a `clients(...)` allowlist.
- **Writing `async fn render`.** Async analyzed components implement the real
  `WebRender` signature and return
  `RenderOutcome::pending(async move { ... })`.

## html_sync! conventions inside islands

All the macro essentials from `/add-component` still apply:

- Text in double quotes (`"foo"`), expressions in `{...}`.
- Underscores in attribute names → dashes (`hx_post` → `hx-post`).
- `Option<&str>` attrs auto-omit when None.
- Boolean attrs render as `attr=""`.
- Performance: no `format!()`, use inline segments and pre-computed
  strings (see `/add-component` § Performance for details).
- Security: automatic XSS escape, `javascript:` URL blocking,
  `target="_blank"` auto-rel.
- Accessibility: `<img>` needs `alt`, ARIA roles validated, etc.
  (compile-time errors).

## Verification

```bash
python3 scripts/proa_perf_lint.py                     # changed Rust render paths
cargo check -p <crate>                            # types + macro expansion
cargo test  -p <crate> --lib                      # any per-island tests
cargo run   -p proa_demo                          # browse the rendered island
# Inspect compiled per-island JS:
curl http://localhost:3001/rsjs/island/<asset_id>.<hash>.js \
  | tee /tmp/island.js
```

The island gets its compiled JS embedded in the binary at link time
via `linkme::distributed_slice`. The framework adapter
(`proa_demo/src/playground.rs::serve_rsjs_*` is the reference) serves
it from memory at `/rsjs/island/<asset_id>.<hash>.js` with brotli +
`Cache-Control: immutable`.

Fix any `scripts/proa_perf_lint.py` finding before reporting done. Suppress
with `// proa-perf-ok: reason` only when an allocation is deliberate and outside
the SSR hot path.

## See also

- [`docs/agent/rsjs-islands.md`](../../../docs/agent/rsjs-islands.md) —
  long-form authoring guide.
- [`docs/agent/api-reference/signals.md`](../../../docs/agent/api-reference/signals.md) —
  signal API.
- [`docs/agent/api-reference/slot-grammar.md`](../../../docs/agent/api-reference/slot-grammar.md) —
  what's allowed inside `{...}`.
- [`docs/agent/footguns.md`](../../../docs/agent/footguns.md) —
  compile-error catalog with fixes.
- [`rsjs/OPTIMIZATIONS.md`](../../../rsjs/OPTIMIZATIONS.md) — perf queue.
