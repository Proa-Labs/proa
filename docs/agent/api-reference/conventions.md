# Conventions

The non-negotiable house rules. Every component / island / page in this repo
should match these.

## Component file structure

```
src/components/<name>/
├── mod.rs          # Re-exports: pub use ...
├── recipe.rs       # Variants enums + class constants + recipe struct
└── <name>.rs       # The component struct + WebRenderSync/WebRender impl + tests
```

Small application components may stay in one file. The split above is useful
when a component has several variants or enough tests to obscure its render
implementation. There is no global catalog registration step.

## Data attributes on the root element

Every component's root element gets `data-component="<kebab-name>"`:

```rust
html_sync! {
    <div data-component="my-card" class={classes}>...</div>
}
```

Components with variants additionally get `data-variant="<variant.as_str()>"`:

```rust
html_sync! {
    <div
        data-component="my-card"
        data-variant={variant.as_str()}
        class={classes}>...</div>
}
```

Why: this lets DOM-level testing tools and integrators locate the component
and its current variant without reading classes.

`data-rsjs-*` is reserved for compiler-generated hydration, owner, payload,
list, and state markers. Application components must use another prefix, such
as `data-app-*`; analyzed templates reject authored attributes in the reserved
namespace.

## Variant enums

Variant enums normally implement `Default`, `Copy`, `Eq`, and `Hash`.
`ProaVariant` is available when a downstream code generator needs a named
variant mapping:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "variant")]
pub enum ButtonVariant {
    #[default]
    Default,
    Secondary,
    Destructive,
    Ghost,
}
```

Each variant exposes:

```rust
impl ButtonVariant {
    pub const fn classes(self) -> &'static str { ... }   // one static class string
    pub const fn as_str(self) -> &'static str { ... }    // kebab-case for data-variant
}
```

## Class lists

Keep static classes in string constants and assemble a fixed number of
conditional fragments with `ClassList`.

### Static class constants

```rust
pub const BUTTON_BASE: &str =
    "inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors";
```

The compiler stores this as one `&'static str`; there is no runtime
concatenation.

### `ClassList<N>` for stack-allocated assembly

```rust
let classes = ClassList::<3>::new()
    .add(base::BUTTON_BASE)
    .add(variant.classes())
    .add_opt(self.class);
```

`N` is the maximum slot count (chosen at compile time per call site). No heap
allocation. Iterates as a single space-separated `&str` when written to the
output buffer.

Don't reach for `format!` or `String::push_str` for class assembly. Those
allocate per render.

## Optional class prop

Every component takes an optional `class: Option<&'static str>` so callers can
add their own classes:

```rust
pub struct Button<C = ()> {
    pub variant: ButtonVariant,
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
```

Threaded through with `.add_opt(self.class)`.

## Generic children

Components that take children use the `C = ()` default-generic pattern:

```rust
pub struct Card<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<L, C> WebRenderSync<L> for Card<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="card" class={card_classes(self.class)}>
                {self.children}
            </div>
        }
        .render(cx)
    }
}
```

Defaulting `C = ()` lets `Card { ..Default::default() }` work without children.
The `Option<C>` wrapper lets callers write `Card { children: Some(...), ... }`.

## Const constructors

Where possible, components expose `pub const fn new() -> Self` so they
compose into other `const` contexts:

```rust
impl Button<()> {
    pub const fn new() -> Self {
        Self {
            variant: ButtonVariant::Default,
            class: None,
            children: None,
        }
    }
}
```

## Design tokens

Applications may centralize design tokens in their own module:

```rust
use crate::theme::{bg, rounded, text};

pub const CARD_BASE: &str = "p-4";

pub const fn card_classes() -> ClassList<4> {
    ClassList::new()
        .add(CARD_BASE)
        .add(rounded::LG)
        .add(bg::BACKGROUND)
        .add(text::FOREGROUND)
}
```

Proa does not require a particular CSS framework or token module. Follow the
owning application's styling conventions and keep class computation
allocation-free.

## Application URLs

Compose unprefixed application paths with `cx.url("/path")` so links,
forms, and asset attributes follow the deployment's runtime base path. Compute
the URL before calling `.render(cx)` to avoid overlapping context borrows:

```rust
let home = cx.url("/");
html_sync! { <a href={home}>"Home"</a> }.render(cx)
```

The returned attribute value allocates no intermediate string. Plain URL strings
stay unchanged; the helper passes through external URLs, protocol-relative URLs,
document-relative paths, queries, and fragments. Use the same helper in HTML and
Markdown renderers so content coverage can match their URLs. `cx.request()`
provides incoming request metadata; construct outgoing URLs with `cx.url(...)`.
Only clone `cx.request().base_path()` when a loop or closure needs independent
access while rendering; explain that shared, allocation-free clone with a
`proa-perf-ok` comment. In Axum handlers, compose redirects with
`proa_framework_axum::app_url("/")`.

The prefix affects outgoing URLs, not incoming routing. `Font::head(cx)`,
compiled docs article URL attributes, shell navigation, and search artifacts
share it; pass the same context to fonts and use `response_with_base_path` in
custom docs handlers. Linked query/mutation runtimes compose endpoint URLs
automatically. Managed CSS/JS services compose recognized URL syntax, leaving
ordinary strings unchanged; custom browser clients can import `app_url` from
`/rsjs-runtime.js`. Use `BasePathHtml` for trusted compiled HTML whose URL
attributes should follow the mount. See the
[deployment coverage contract](../../../proa_marketing/src/docs/framework/deployment.mdx#coverage-and-limitations).

## Error handling

- `thiserror::Error` for error enums (no `anyhow` in component code).
- Components return `Result<(), WriteError>`, propagating with `?` from inner renders.

## Logging

Use `tracing` (not `log`, not `eprintln!`).

## Writing style

This applies to all prose Claude or Codex writes in this workspace:

- **No em dashes.** Use commas, parentheses, colons, semicolons, or sentence breaks.
- Direct over decorative; avoid "elegant," "robust," "powerful."
- Cite file paths with `path/to/file.rs:line` so they're navigable.

## See also

- [`CLAUDE.md`](../../../CLAUDE.md) — top-level conventions overview.
- [`../components.md`](../components.md) — complete local component workflow.
- [`../../../proa_core/tests/landing_page_example.rs`](../../../proa_core/tests/landing_page_example.rs) — checked explicit render-component example.
- [`../../../proa_macros/tests/proa_component_launch.rs`](../../../proa_macros/tests/proa_component_launch.rs) — checked declarative `#[derive(ProaComponent)]` examples.
