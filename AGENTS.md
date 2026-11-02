# Codex Instructions for the Proa workspace

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
| Why rendering writes directly to a buffer: ownership, self-reference, and branch types | [`docs/agent/api-reference/direct-buffer-rendering.md`](docs/agent/api-reference/direct-buffer-rendering.md) |
| `Send` / local-future streaming decision | [`proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability`](proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability) |
| What's allowed inside `{ ... }` slots | [`docs/agent/api-reference/slot-grammar.md`](docs/agent/api-reference/slot-grammar.md) |
| Naming, classes, data attributes, conventions | [`docs/agent/api-reference/conventions.md`](docs/agent/api-reference/conventions.md) |

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
Package names and Rust import paths are unchanged; only filesystem paths differ.

| Group directory | Crates inside |
|---|---|
| `proa_framework/` | `proa_framework_axum`, `proa_framework_core` |
| `proa_fonts/` | `proa_fonts`, `proa_fonts_build`, `proa_fonts_macros`, `proa_fonts_parse` |
| `proa_docs/` | `proa_docs`, `proa_docs_build` |
| `tailwind-merge/` | `tailwind-merge`, `tailwind-merge-macro` |
| `const_json/` | `const_json`, `const_json_macros` |

For example, the `proa_framework_axum` package sources are under
`proa_framework/proa_framework_axum/src/`. The `rsjs_*` crates are not grouped.

| Crate | Purpose |
|-------|---------|
| `proa_html_policy` | Allocation-free URL policy shared by compilers and the renderer, without rendering dependencies |
| `proa_build` | Linked RSJS compiler orchestration, descriptor caches, and semantic linking |
| `proa_source_tools` | Source discovery, linting, formatting, and artifact inspection |
| `proa_core` | Rendering primitives: `WebRenderSync`, `WebRender`, `WriteBuf`, `ClassList`, HTML elements |
| `proa_macros` | Proc macros: `html!`, `html_sync!`, `md!`, `md_sync!`, `#[derive(ProaRecipe)]`, `#[derive(ProaVariant)]`, `#[derive(ProaComponent)]`, `#[derive(Action)]`, `#[island]` |
| `proa_stream` | Streaming SSR substrate: loaders, stream handles, `Suspense`, out-of-order driver |
| `proa_framework_axum` | Axum/Tokio adapter with routing, static serving, and HTTP response integration |
| `proa_ir` | Intermediate representation for reactive attrs, bindings, and lower-level client/runtime glue |
| `proa_actions` | Action data structures for event handlers |
| `proa_codegen` | TypeScript code generation from Rust components |
| `proa_react` | React/JSX interop |
| `proa_framework_core` | Server/runtime-agnostic framework core: manifest, bootstrap, portable response/body finalizers |
| `proa_bench` | Proa-only render and streaming regression benchmarks |
| `rsjs_web` | Generated zero-cost DOM markers, capabilities, and event-interface witnesses for RSJS JavaScript lowering |

## Core patterns

### Document authoring

Use the matching macros for substantial authored output: `html_sync!`/`html!`,
`md_sync!`/`md!`, `xml_sync!`/`xml!`, and `const_json!` for literal JSON. Use typed
`Serialize` models for runtime JSON. This applies to handlers, error pages, and
cached/startup documents too. HTML and Markdown representations belong on the
same page/component module, with copy, labels, URLs, and collections defined
once in shared props or a content constructor. Compose dedicated Markdown
sections for the core page structure; do not duplicate prose in both templates,
create a separate agent-facing page, or default to converting the HTML tree.
Preserve HTML copy and `PageMeta` unless editorial changes are explicitly
requested. See
[`docs/agent/document-rendering.md`](docs/agent/document-rendering.md) for format
selection, escaping, source-data boundaries, and verification.

### Browser behavior

Use RSJS for application browser behavior whenever supported: typed signals,
event handlers, bindings, node refs, and client helpers. Do not default to
JavaScript strings, inline scripts, or hand-written JavaScript assets. Raw
JavaScript requires a necessary, verified capability gap, a nearby explanation,
and a minimal bridge. See the
[RSJS-first policy](docs/agent/rsjs-islands.md#prefer-rsjs-for-browser-behavior)
for the decision process and scope.

### Render traits

Application sync components normally implement `WebRenderSync<L>`. Its body
method signature is:

```rust
pub trait WebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}
```

Async-capable components normally implement `WebRender<L>`. Its body method
signature is:

```rust
pub trait WebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>;
}
```

Use `WebRenderSync` by default. Use `WebRender` when the body or metadata hook
needs to `.await`. Async head hooks use native
`async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError>`.
Declare `const ROLE: ComponentRole = ComponentRole::PAGE` on the selected Page,
including inherited-only Pages; ancestor metadata providers declare `LAYOUT`.
Run `cargo proa lint <changed-paths> --deny warnings` to catch head overrides
with missing roles or an explicit `COMPONENT` role. Async role validation is a
source check; sync hooks also retain a code-generation assertion.
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

Markdown follows the same split: use `md_sync!` in `MdRenderSync` components
and async-capable `md!` in `MdRender` components. Lift a custom synchronous
Markdown child into `md!` with `proa_core::to_md_async(...)`.

Author Markdown as one multiline raw template, using three hashes by default:

```rust
md_sync! { r###"
    # {self.title}

    {self.body}
    "###
}
```

Source newlines are output newlines, so do not add `"\n"` or `"\n\n"` between
ordinary blocks. `{expression}` composes a typed child. Braces inside fenced
code blocks and inline code spans stay literal. Increase the raw-string hash
count only if the Markdown itself contains the exact closing sequence `"###`.
Outside code spans and fences, write `{{` or `}}` for a literal brace.
Text outside slots is always Markdown; each `{ ... }` slot is Rust. Use
ordinary Rust `if`, `for`, and `match` there, and re-enter Markdown through a
nested macro. Prefer ordinary one-line strings for short fragments, such as
`md_sync! { "- {item}\n" }` and `md_sync! { "**In stock**" }`. Preserve the
row-ending `\n`. Use raw templates for substantial multiline fragments, with
fewer hashes inside an outer raw literal. Never remove the explicit slot boundary around control flow.
See [the Markdown skill](.agents/skills/proa-markdown/SKILL.md) for a complete
loop/branch example and the exceptions for code fences and exact separators.
The token composition form (`md_sync! { "# " { title } }`) remains supported
for generated code and cases that need the expressions visible as Rust tokens.
Run `proa fmt` to normalize raw-template indentation and keep the closing macro
brace aligned with the invocation; `proa lint` enforces the same layout.

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
cargo test --workspace --no-fail-fast

# Perf lint changed Rust render paths
python3 scripts/proa_perf_lint.py

# Run the marketing/docs application
cargo proa run -p proa_marketing --bin proa_marketing
```

## Conventions

- **Data attributes**: Every component renders `data-<component>` on its root element (e.g., `data-badge`, `data-dialog-root`, `data-card`)
- **Variant attributes**: Components with variants add `data-variant={variant.as_str()}`
- **Optional classes**: All components accept `class: Option<&'static str>` for customization
- **Generic children**: Components use `C = ()` default generic for children
- **Const constructors**: Provide `pub const fn new() -> Self` where possible
- **No heap in hot path**: Recipes and class computation use stack-allocated `ClassList<N>` and `heapless` collections
- **Perf lint**: Before finishing Rust render changes, run `python3 scripts/proa_perf_lint.py`. Fix findings or add `// proa-perf-ok: reason` only for intentional non-hot-path allocations.
- **Styling ownership**: Proa does not require Tailwind or a particular theme module; follow the owning crate's design-token conventions
- **Error handling**: `thiserror::Error` for error enums
- **Logging**: Use `tracing`
- **Render tests**: Do not assert that output contains prose already hard-coded
  in the renderer under test. That only pins copy. Assert behavior instead:
  dynamic inputs, escaping, branch selection, element counts, attributes,
  accessibility contracts, asset registration, or complete output when every
  byte is the contract.
