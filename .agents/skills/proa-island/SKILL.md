---
name: proa-island
description: "Author or modify optimized Proa #[rsjs(component...)] islands. Use when adding browser interactivity, hydration strategies, signals, named event handlers, typed events, bind_value/bind_checked forms, wellformed client validation, query/mutation islands, client modules, or debugging rsjs/html_sync compile errors with Proa CLI lint feedback."
---

# Proa Island

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

## Browser behavior defaults

Use RSJS for application browser behavior whenever supported. Check existing
bindings, client helpers, and integration components before writing raw
JavaScript. A compile error or a shorter script is not evidence that RSJS
cannot do the job. Only a necessary, documented gap justifies a minimal raw
JavaScript bridge; follow the [RSJS-first policy](../../../docs/agent/rsjs-islands.md#prefer-rsjs-for-browser-behavior).

## Overview

Use this workflow for Proa interactive islands. Keep the server render path sync
when a query's initial value is already available. Use the async render path for
mutations, an awaited query initial value, or another real await. Let the Proa
CLI linter guide the island shape before treating a compile error as a macro
problem.

For a managed-head document, put the selected Page boundary on an ordinary SSR
Page wrapper: `const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::PAGE;`.
It is required even if that Page inherits all metadata. Ancestor SSR wrappers
with head hooks declare `ComponentRole::LAYOUT`; body islands keep the default
component role. A real head hook without a Layout/Page role fails a compile-time
assertion during code generation; verify it with `cargo build`. The current
`#[rsjs(component...)]` analyzer accepts a render-only impl, so do not add the
role constant or head hook inside that analyzed impl. Compose the island inside
the Page wrapper instead. See the
[managed Page boundary](../../../docs/agent/components.md#managed-html-page-boundary).

## Workflow

1. Read `docs/agent/rsjs-islands.md` for authoring shape.
2. Read `docs/agent/footguns.md` when a `#[rsjs(...)]` or `html_sync!` compile error appears.
3. Always author the real render trait impl. Use
   `#[rsjs(component, client)]` when the component declares local signals,
   handlers, timers, queries, mutations, shared-state acquisition, node refs,
   or browser work. Use `#[rsjs(component)]` only for analyzed components that
   participate in a parent client graph without originating browser behavior.
   Keep ordinary local browser state and queries whose initial value is already
   available on `WebRenderSync<L>`. Mutations, awaited query initials, and
   real awaits use the canonical `WebRender<L>` /
   `RenderOutcome::pending(async move { ... })` form; `async fn render` sugar
   is not supported. `WebContext<L>` owns the default `RenderBuffer`; use the
   second writer generic only in deliberately low-level integrations.
4. Prefer named handler locals over inline closures in production code:
   `let on_input = rsjs::event_handler(|event: rsjs::InputEvent<rsjs::elem::Input>| { ... });`.
   Invoke with `on_input={on_input()}`.
5. Use typed events when the handler needs element-specific data:
   `MouseEvent<elem::Button>`, `InputEvent<elem::Input>`, and
   `event.current_value()` / `event.current_checked()`.
6. Use `signal(...)`, `.get()`, `.set(...)`, `.update(|x| expr)`, and memo
   cells exactly as documented in `docs/agent/api-reference/signals.md`.
   Signal receivers must stay bare identifiers.
7. Ordinary dependency-free locals and opaque calls remain SSR. Direct inbound
   field/path reads are latent `LiveRead` participation: a linked live parent
   value stays current, while `signal(self.value)` samples once for child-owned
   state and a live query-key use rebinds its observer. Use
   `rsjs::server_attr(...)` or `rsjs::server_only(...)` only for an intentional
   per-render snapshot or opaque SSR boundary; do not capture reactive signals,
   declare client primitives, or call rsjs browser helpers through those escape
   hatches.
8. For Wellformed-backed forms, generate the form module with
   `wellformed_macros::form_schema!` and use generated helpers such as
   `normalize_email`, `error_email`, and `valid_email` for client-side
   validation before POSTing. Each generated pure helper must expose one
   `#[rsjs::client]` definition; reachable calls link automatically.
   `#[rsjs::client_module]` is optional grouping, and the consuming component
   does not declare `clients(...)`. Still validate on the server with the same
   schema. The semantic linker resolves reachable same-crate and cross-crate
   helper calls through normal Rust imports, aliases, and re-exports.
9. Run the Proa CLI workflow on changed island files:

```bash
cargo proa fmt --check --verify <changed-rust-paths>
cargo proa lint <changed-rust-paths> --deny warnings
cargo proa check <changed-rust-paths>
```

If the linter reports `rsjs/...`, `format/...`, accessibility, render
composition, or allocation diagnostics, revise the island and rerun. Typical
fixes are: move logic into a named handler or standalone `#[rsjs::client]` helper,
wrap server-only content explicitly, replace unsupported slot expressions with
supported reactive expressions, or switch an unnecessary async render back to
`WebRenderSync`.

10. Run crate verification after the CLI loop:

```bash
cargo check -p <crate>
cargo test -p <crate> --lib
python3 scripts/proa_perf_lint.py
```

For a downstream repository, run its own documented workspace commands.

## Minimal Shape

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{event_handler, rsjs, signal};

pub struct Counter {
    pub initial: i64,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(self.initial);
        let decrement = event_handler(|_: rsjs::Event| count.update(|c| c - 1));
        let increment = event_handler(|_: rsjs::Event| count.update(|c| c + 1));

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

The trait impl is ordinary authored Rust. The attribute analyzes that impl and
generates its client artifact when browser work is reachable; it does not
invent the render signature or return type.
