# Proa agent reference

Entry point for coding agents (Claude Code, Codex, etc.) authoring local render
components, islands, or pages in this workspace. Optimized for
**copy-paste-and-adapt**: every section ends in a link to a real example file an
agent can crib from.

If you have read [`CLAUDE.md`](../../CLAUDE.md) at the repo root, the material
here is the same shape but deeper, and grouped so an agent can land on one page
per task rather than skimming the whole thing.

## Decision tree

| You want to ... | Start here |
|---|---|
| Add a local render component | [`components.md`](components.md) |
| Add an RSJS-analyzed/composable component (with or without browser origins) | [`rsjs-islands.md`](rsjs-islands.md) |
| Pick `WebRenderSync` vs `WebRender` | [`api-reference/render-traits.md`](api-reference/render-traits.md) |
| Understand the `Send` / local-future streaming policy | [`../../proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability`](../../proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability) |
| Pick `md_sync!` vs `md!` | [`api-reference/render-traits.md#markdown-has-the-same-split`](api-reference/render-traits.md#markdown-has-the-same-split) |
| Read or write a signal inside an island | [`api-reference/signals.md`](api-reference/signals.md) |
| Share typed page state across independent islands | [`api-reference/signals.md#observability-across-islands`](api-reference/signals.md#observability-across-islands) |
| Know what's allowed inside `{ ... }` slots in `html_sync!` | [`api-reference/slot-grammar.md`](api-reference/slot-grammar.md) |
| Look up data-attribute / class / variant conventions | [`api-reference/conventions.md`](api-reference/conventions.md) |
| Find the recipe for a specific common task | [`cookbook.md`](cookbook.md) |
| Diagnose a compile error from `html_sync!` or `#[rsjs(...)]` | [`footguns.md`](footguns.md) |

The `proa_ui` component library is a downstream integration. Changes to that
library, its catalog, or its recipes belong in the component library's owning
repository and should follow that repository's agent instructions.

## Core mental model in three sentences

Proa compiles HTML and Markdown templates into byte sequences at compile time,
so SSR is allocation-free on the hot path. Custom HTML components normally
implement `WebRenderSync<L>` and return
`html_sync! { ... }.render(cx)`; components that genuinely await implement the
real `WebRender<L>` signature. The default writer is the `RenderBuffer` owned by
`WebContext<L>`; the extra `B` parameter is only for advanced custom writers.
Markdown components implement
`MdRenderSync<L>` (or its async-capable counterpart) and use the matching
macro pair. Analyzed components put `#[rsjs(component)]` on that real trait
impl, adding `client` when they originate browser behavior. Those components
compile to per-component vanilla JS and a small shared runtime, so the page
ships no client framework.

These are **compiled templates**: Rust-authored templates checked and lowered
during the application build. A **runtime template** is source supplied as data
and compiled or validated after the host has been built. The
runtime-template/document system is a downstream integration and is not
required for this workspace.

## Where the example fixtures live

Agents should treat these as the canonical reference shapes. Read before writing:

| Pattern | File | What it shows |
|---|---|---|
| Explicit custom render component | [`proa_core/tests/landing_page_example.rs`](../../proa_core/tests/landing_page_example.rs) | Authored `WebRenderSync` implementation and template composition |
| Declarative single-tag component | [`proa_macros/tests/proa_component_launch.rs`](../../proa_macros/tests/proa_component_launch.rs) | `#[derive(ProaComponent)]`, builders, children, and async bridge generation |
| Generic-children declarative component | [`proa_macros/tests/proa_component.rs`](../../proa_macros/tests/proa_component.rs) | `Card<C = ()>` and derive-backed generic component patterns |
| Single-signal rsjs island | [`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs) | The minimum working island |
| Multi-signal island + conditionals + bind_value | [`rsjs_macros/tests/parser_user_form.rs`](../../rsjs_macros/tests/parser_user_form.rs) | Forms, validation, multiple `if` branches |
| Typed state shared across independent islands | [`rsjs/examples/shared_workbench.rs`](../../rsjs/examples/shared_workbench.rs) | `SharedStateProvider` scope, `shared::<T>()`, field cells, memo values, and batched writes |
| Async analyzed component | [`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs) | Canonical `WebRender` signature and `RenderOutcome::pending(async move { ... })` |
| Real-world rsjs island shipping in production | [`proa_demo/src/ebay_srp_islands.rs`](../../proa_demo/src/ebay_srp_islands.rs) | `WatchHeart` (one signal) + `FilterChips` (multi-signal toolbar) |
| Server-rendered values | [`api-reference/slot-grammar.md`](api-reference/slot-grammar.md) | Prelude values render normally; `server_only(...)` is a compatibility escape hatch |

## Conventions one-liner

- All HTML element names with hyphens render as hyphens (`my-button`, `<rsjs-island>`).
- Tag names that are Rust keywords (`<use>`, `<for>`, `<type>`) work; the parser uses `Ident::parse_any`.
- Components add `data-component` to their root element and `data-variant={variant.as_str()}` if they have variants.
- Hot-path styling uses static class strings and `ClassList<N>`
  (stack-allocated, no heap).
- Use `WebRenderSync` by default; switch to `WebRender` only when rendering must
  `.await` (for example, to load a query's initial value) or while mutation
  declarations remain on the async surface.
- Use `md_sync!` inside `MdRenderSync`; use `md!` inside async-capable `MdRender`.
  Author either as one multiline `r###"..."###` Markdown template by default;
  its source newlines render directly and `{expression}` composes typed children.
- Before finishing Rust render changes, run `python3 scripts/proa_perf_lint.py`.

## When stuck

1. Search the workspace for `#[rsjs(component` or `WebRenderSync` to see how others did it.
2. Read [`footguns.md`](footguns.md). Most "weird compile error" cases are documented there with the fix.
3. The compile errors from `#[rsjs(...)]` are intentionally agent-quality and
   tell you the rule + the fix. Read them carefully before guessing.
