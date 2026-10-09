---
name: proa-island
description: "Author or modify optimized Proa #[rsjs(component...)] islands. Use when adding browser interactivity, hydration strategies, signals, named event handlers, typed events, bind_value/bind_checked forms, wellformed client validation, query/mutation islands, client modules, or debugging rsjs/html_sync compile errors with Proa CLI lint feedback."
---

# Proa Island

## Overview

Use this workflow for Proa interactive islands. Keep the server render path sync
when a query's initial value is already available. Use the async render path for
mutations, an awaited query initial value, or another real await. Let the Proa
CLI linter guide the island shape before treating a compile error as a macro
problem.

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
