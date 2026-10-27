# Claude Code Instructions for the Proa workspace

Proa is a Rust SSR template engine that compiles HTML templates into optimized byte sequences at compile time. It renders 10-20x faster than traditional templating engines through static consolidation and zero-allocation patterns.

## Agent reference

For deep authoring guidance, read [`docs/agent/`](docs/agent/index.md). It is
the single source of truth for local render-component and island authoring in
this workspace. The `proa_ui` component library is a downstream integration;
component-library and catalog changes belong in its owning repository.

| Task | Doc |
|---|---|
| Add a local render component | [`docs/agent/components.md`](docs/agent/components.md) |
| Render HTML, Markdown, XML, text, or JSON | [`docs/agent/document-rendering.md`](docs/agent/document-rendering.md) |
| Add an analyzed `#[rsjs(component...)]` component | [`docs/agent/rsjs-islands.md`](docs/agent/rsjs-islands.md) |
| Recipe for a specific common task | [`docs/agent/cookbook.md`](docs/agent/cookbook.md) |
| Diagnose a `#[rsjs(...)]` / `html_sync!` compile error | [`docs/agent/footguns.md`](docs/agent/footguns.md) |
| Signal API (`.get` / `.set` / `.update`) | [`docs/agent/api-reference/signals.md`](docs/agent/api-reference/signals.md) |
| `WebRenderSync` vs `WebRender` decision | [`docs/agent/api-reference/render-traits.md`](docs/agent/api-reference/render-traits.md) |
| `Send` / local-future streaming decision | [`proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability`](proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability) |
| What's allowed inside `{ ... }` slots | [`docs/agent/api-reference/slot-grammar.md`](docs/agent/api-reference/slot-grammar.md) |
| Naming, classes, data attributes, conventions | [`docs/agent/api-reference/conventions.md`](docs/agent/api-reference/conventions.md) |

## Claude Code skills

Project-scoped Claude Code skills live at
[`.claude/skills/<name>/SKILL.md`](.claude/skills/README.md) and are
auto-discovered for anyone running `claude` in this repo. Workspace-focused
skills include `/add-component` (local render-component scaffold),
`/add-island` (rsjs island scaffold), `/test-component` (render and browser
testing), and `/proa-perf-review` (render-path allocation review). See
[`.claude/skills/README.md`](.claude/skills/README.md) for the authoring
convention and how to add a new skill.

## Workspace layout

Most crates live at the repo root (`./`). The framework workspace root
`Cargo.toml` is at the repo root. Treat this checkout as a standalone
workspace: commands and documentation must not assume a neighboring repository.

This workspace implements **compiled templates**, meaning Rust-authored
templates checked and lowered during the application build. **Runtime
templates** are source supplied as data and compiled or validated after the
host is built. The runtime-template/document system and the `proa_ui` component
library are downstream integrations with their own repositories. Neither is a
workspace prerequisite.

Multi-crate subsystems are grouped in a parent directory named after the
subsystem, with each crate in a subdirectory matching its package name.
Package names are unchanged by the grouping, so `cargo` invocations and
`use` paths are unaffected; only filesystem paths differ.

| Group directory | Crates inside |
|---|---|
| `proa_framework/` | `proa_framework_axum`, `proa_framework_core` |
| `proa_fonts/` | `proa_fonts`, `proa_fonts_build`, `proa_fonts_macros`, `proa_fonts_parse` |
| `proa_docs/` | `proa_docs`, `proa_docs_build` |
| `tailwind-merge/` | `tailwind-merge`, `tailwind-merge-macro` |
| `const_json/` | `const_json`, `const_json_macros` |

So `proa_framework_axum`'s sources are at `proa_framework/proa_framework_axum/src/`. The
`rsjs_*` crates are not yet grouped.

Runnable reference applications live under `examples/` and are workspace
members: `examples/origin-routing-atlas`, `examples/webmcp-pizza`, and
`examples/mcp-incident-desk`. They are `publish = false` and exercise the
public API the way an application would, so they are the best place to read a
complete page, loader, and layout in context.

| Crate | Purpose |
|-------|---------|
| `proa_core` | Rendering primitives: `WebRenderSync`, `WebRender`, `WriteBuf`, `ClassList`, HTML elements |
| `proa_macros` | Proc macros: `html!`, `html_sync!`, `#[derive(ProaRecipe)]`, `#[derive(ProaVariant)]`, `#[derive(ProaComponent)]`, `#[derive(Action)]`, `#[island]` |
| `proa_framework_axum` | Axum/Tokio adapter with routing, static serving, and HTTP response integration |
| `proa_ir` | Intermediate representation for reactive attrs, bindings, and lower-level client/runtime glue |
| `proa_actions` | Action data structures for event handlers |
| `proa_codegen` | TypeScript code generation from Rust components |
| `proa_react` | React/JSX interop |
| `proa_fonts` | Runtime for self-hosted, metric-matched web fonts: `Font`/`FontFace`, zero-alloc `Font::head` |
| `proa_fonts_build` | Build-time font pipeline: parse metrics, compute fallbacks, content-hash, generate `proa_fonts::Font` modules (drives `cargo proa font`) |
| `proa_bench` | Proa-only render and streaming regression benchmarks |

## Core patterns

### Document authoring

Use the matching macros for substantial authored output: `html_sync!`/`html!`,
`md_sync!`/`md!`, `xml_sync!`/`xml!`, and `const_json!` for literal JSON. Use typed
`Serialize` models for runtime JSON. This applies to handlers, error pages, and
cached/startup documents too. HTML and Markdown representations belong on the
same page/component types, with shared data and typed children. See
[`docs/agent/document-rendering.md`](docs/agent/document-rendering.md) for format
selection, escaping, source-data boundaries, and verification.

### Render traits

Application sync components normally implement `WebRenderSync<L>`. The full
trait definition is:

```rust
pub trait WebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}
```

Async-capable components normally implement `WebRender<L>`. The full trait
definition is:

```rust
pub trait WebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>;
}
```

Use `WebRenderSync` by default. Use `WebRender` only for components that need to `.await`.
The `B` parameter is the advanced custom-writer escape hatch. Application
components normally omit it and implement `WebRenderSync<L>` or `WebRender<L>`;
that selects the default owned `RenderBuffer` and leaves only the loader
customizable.

### `html_sync!` and `html!`

`html_sync!` is the sync fast path for `WebRenderSync` components:

```rust
html_sync! {
    <div class="flex gap-2">
        <span>{some_variable}</span>
        "Static text"
    </div>
}
```

`html!` is the async-capable macro for `WebRender` components and async roots. If you need to place a custom sync component inside an async `html!` tree, wrap it with `proa_core::to_async(...)`.

### ClassList (zero-allocation styling)

Stack-allocated fixed-size class array:

```rust
let classes = ClassList::<3>::new()
    .add(base::BUTTON_BASE)
    .add(variant.classes())
    .add_opt(self.class);
```

### Static class constants

Keep classes known at compile time in a single static string:

```rust
pub const BUTTON_BASE: &str = "inline-flex items-center rounded-md text-sm";
```

### Fonts (proa_fonts)

Self-host web fonts instead of hitting Google at runtime: content-hashed files, a metric-matched fallback (no layout shift on swap), preload hints, zero JS. Fonts are declared in code with `google_font!` / `local_font!` from `proa_fonts_macros`, e.g. `static SANS: Font = google_font!("Geist", subsets = [latin], weights = [400, 700], variable = "--font-sans");`. Each declaration produces a `proa_fonts::Font` rendered in `<head>` with one allocation-free call `SANS.head(cx)?` (apply `class={SANS.class_name()}` on `<html>`, or reference `var(--font-...)`).

A `build.rs` calling `proa_fonts_build::collect::run(&CollectOptions::from_build_env()?)` scans for the macros, vendors into `public/fonts`, records the result in `proa-fonts.lock`, and generates the consts into `OUT_DIR` (Google fetching is behind the default-on `fetch` feature). Needs one boilerplate line: `mod __proa_fonts_generated { include!(concat!(env!("OUT_DIR"), "/proa_fonts.rs")); }`. Vendoring is lock-gated: unchanged declarations with intact vendored files (sha256 + size verified) regenerate offline from disk; commit `proa-fonts.lock` and `public/fonts` so builds are offline and reproducible. `cargo proa font sync` runs the same collector outside a build; `--offline` is strictly verify-only for all sources (no writes, fails with "proa-fonts.lock is stale" on any lock or file drift; the CI gate), and plain `sync` under `CARGO_NET_OFFLINE=true` also verifies only. During a `cargo build` with `CARGO_NET_OFFLINE=true`, local fonts still vendor; only needed Google fetches fail actionably. `font add`/`font remove` and `proa-fonts.toml` no longer exist.

Never add a runtime `fonts.googleapis.com` `<link>` or a hand-written `@font-face`. See [`docs/framework/fonts`](proa_marketing/src/docs/framework/fonts.mdx).

## Local component structure

For components owned by an application or crate in this workspace, prefer this
file layout:

```
components/<name>/
├── mod.rs          # Re-exports
├── recipe.rs       # Variant enums, class constants, recipe struct
└── <name>.rs       # Component struct + WebRenderSync/WebRender impl + tests
```

### recipe.rs pattern

```rust
use proa_macros::ProaRecipe;

mod base {
    pub const COMPONENT_BASE: &str = "class1 class2";
}

mod variants {
    pub const VARIANT_DEFAULT: &str = "bg-primary text-primary-foreground";
    pub const VARIANT_SECONDARY: &str = "bg-secondary text-secondary-foreground";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "variant")]
pub enum ComponentVariant {
    #[default]
    Default,
    Secondary,
}

impl ComponentVariant {
    pub const fn classes(self) -> &'static str {
        match self {
            Self::Default => variants::VARIANT_DEFAULT,
            Self::Secondary => variants::VARIANT_SECONDARY,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Secondary => "secondary",
        }
    }
}

#[derive(ProaRecipe)]
pub struct ComponentRecipe {
    base: &'static str,
    variant_default: Option<ComponentVariant>,
}

pub fn component_classes(variant: ComponentVariant) -> proa_core::ClassList<3> {
    use proa_core::ClassList;
    ClassList::new()
        .add(base::COMPONENT_BASE)
        .add(variant.classes())
}
```

### component.rs pattern

```rust
use super::recipe::{component_classes, ComponentVariant};
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct Component<C = ()> {
    pub variant: ComponentVariant,
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<C> Default for Component<C> {
    fn default() -> Self {
        Self {
            variant: ComponentVariant::Default,
            class: None,
            children: None,
        }
    }
}

impl<L, C> WebRenderSync<L> for Component<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = component_classes(self.variant).add_opt(self.class);
        html_sync! {
            <div data-component data-variant={self.variant.as_str()} class={classes}>
                {self.children}
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::text::text;

    #[test]
    fn component_renders() {
        let mut cx = WebContext::new();
        Component { variant: ComponentVariant::Default, class: None, children: Some(text("Hi")) }
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("data-component"));
    }
}
```

For async components, use `WebRender` plus `html!`. Wrap sync children with `proa_core::to_async(...)` when they need to participate in async composition. Use `proa_core::await_component(...)` when a future resolves to a renderable child.

### Registration checklist (when adding a local component)

1. Create `components/<name>/mod.rs`, `recipe.rs`, `<name>.rs`
2. Add `pub mod <name>;` + re-exports to `components/mod.rs`
3. Add types to `pub use components::{...}` in `lib.rs`

The downstream component library has its own registration and catalog workflow.
Follow the instructions in its owning repository when changing it.

## Building

```bash
# All commands run from repo root
cd /path/to/proa  # repo root

# Format
cargo fmt --all

# Check the standalone workspace
cargo check --workspace --all-targets

# Test under plain cargo. The RSJS plain-cargo-build guard makes any test
# binary that links island rlibs from a *dependency* fail at link time
# (same-crate lib tests are fine: the guard is cfg'd out under test). The
# island demo crates are exercised through `cargo proa test` instead.
cargo test --workspace --lib --exclude proa_demo --exclude proa_marketing --exclude proa_showcase_web --exclude proa-lean-server
cargo test --workspace --tests --exclude proa_demo --exclude proa_marketing --exclude proa_showcase_web --exclude proa-lean-server --exclude proa_showcase

# Perf lint changed Rust render paths (regex-based, fast)
python3 scripts/proa_perf_lint.py

# AST-deep template lint, formatter, and consolidation inspector
cargo proa lint                  # perf + correctness lints (warnings+ shown; --min-severity info for all); parallel across files
cargo proa lint --changed        # only files changed vs HEAD (also on fmt/check/chunks)
cargo proa fmt                   # canonical macro layout; render-preserving verify runs before every write
cargo proa fmt --check           # no writes; prints a unified diff and exits non-zero on drift
cargo proa check --source-only --allow-warnings # workspace CI gate; errors/drift fail. Without --source-only the linked RSJS Cargo check runs and needs one exact -p/--bin target
cargo proa config validate       # proa.config.json: unknown keys, empty commands, bad asset kinds
cargo proa chunks proa_showcase/src # static-run / dynamic-slot report

# Fonts: declared in code via google_font!/local_font!, vendored by build.rs (see docs/framework/fonts)
cargo proa font sync             # vendor + lock every declared font outside a build
cargo proa font sync --offline   # strict verify, no writes; fails on any drift (CI gate)
cargo proa font list             # fonts recorded in proa-fonts.lock (--json includes measured metrics)

# Run the marketing/docs application. Plain `cargo run -p proa_marketing`
# does NOT work: it links proa_demo, whose cdylib is rejected by the RSJS
# plain-cargo-build guard. Go through the linked pipeline instead.
cargo proa run -p proa_marketing --bin proa_marketing
scripts/watch-marketing.sh       # dev loop: rebuild and restart on change
```

## Conventions

- **Data attributes**: Every component renders `data-<component>` on its root element (e.g., `data-badge`, `data-dialog-root`, `data-card`)
- **Variant attributes**: Components with variants add `data-variant={variant.as_str()}`
- **Optional classes**: All components accept `class: Option<&'static str>` for customization
- **Generic children**: Components use `C = ()` default generic for children
- **Const constructors**: Provide `pub const fn new() -> Self` where possible
- **No heap in hot path**: Recipes and class computation use stack-allocated `ClassList<N>` and `heapless` collections
- **Perf lint**: Before finishing Rust render changes, run `python3 scripts/proa_perf_lint.py` and `cargo proa lint` (AST-deep: catches render-copy composition, UnsafeRaw misuse, allocation patterns). Fix findings or add `// proa-perf-ok: reason` (line) / `// proa-perf-ok(scope): reason` (whole item) only for intentional non-hot-path allocations.
- **Page composition**: Never render a fragment to a `String` and splice it into another render (`perf/render-copy`). Compose with generic component slots so the whole page renders in one pass.
- **Prefer the Proa HTML macros for HTML generation**: Use `html_sync!` in sync `WebRenderSync` components and `html!` in async `WebRender` components instead of `format!()` or manual `WriteBuf` calls. The macros handle escaping automatically, consolidate static segments at compile time, and avoid intermediate `String` allocations. Use `UnsafeRaw` only for pre-rendered HTML content that must not be double-escaped.
- **Styling ownership**: Proa does not require Tailwind or a particular theme module; follow the owning crate's design-token conventions
- **Error handling**: `thiserror::Error` for error enums
- **Logging**: Use `tracing`

## Writing style (applies to all prose in this workspace)

- **No em dashes.** Never use em dashes in any prose Claude writes or suggests, including drafts, edits, shortened versions, and inline commentary. Use commas, parentheses, colons, semicolons, or sentence breaks instead.
