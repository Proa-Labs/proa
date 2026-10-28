---
name: add-component
description: Scaffold or modify an opaque Proa Rust render component owned by the current crate or application. Use for component structs, generic children, variants, ClassList styling, and render tests. Use the island skill when RSJS composition analysis or browser behavior is required.
---

# Add a Proa render component

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

Read [`docs/agent/components.md`](../../../docs/agent/components.md) before
editing. This skill targets components owned by this standalone repository or
by the current application. It does not assume `proa_ui`, a component catalog,
a theme module, or a neighboring checkout.

If the user explicitly asks for a component in a downstream component-library
repository, follow that repository's own instructions.

Use the island workflow for application browser behavior. Prefer RSJS state,
typed handlers, bindings, and client helpers over executable JavaScript strings,
inline scripts, or hand-written JavaScript assets. Raw JavaScript is a last
resort for a verified, documented RSJS gap; follow the [RSJS-first policy](../../../docs/agent/rsjs-islands.md#prefer-rsjs-for-browser-behavior).

## Workflow

1. Find the owning crate and its nearest component.
2. Keep a small component in one module. Use
   `components/<name>/{mod.rs,recipe.rs,<name>.rs}` only when variants or tests
   justify the split.
3. Implement `WebRenderSync<L>` explicitly and use `html_sync!` by default.
   Add `WebRenderSync` bounds only to generic values rendered as children.
   Use `proa_core::to_async(...)` when a sync child must compose inside
   `html!`. Implement the real `WebRender<L>` signature only when rendering
   awaits.
   `WebContext<L>` owns the default `RenderBuffer`; reserve `B: WriteBuf` for
   advanced custom-writer integrations.
4. Use generic `C = ()` plus `Option<C>` for children.
5. Store compile-time classes in static strings and assemble conditional
   fragments with `ClassList<N>`.
6. Add `data-component="<kebab-name>"` to the root. Add `data-variant` for a
   caller-selectable variant.
7. Test dynamic input, escaping, variants, children, optional fields, and
   stable accessibility relationships.

If the component must participate in a parent's live RSJS graph, switch to the
island workflow and put `#[rsjs(component)]` on this real trait impl even when
its own strategy is ServerOnly. Add `client` only when it originates browser
behavior.

## Performance and security

- Every byte should write directly to the output buffer.
- Do not call `format!`, `.to_string()`, `.join()`, or `push_str` in `render`.
- Do not render a child into an intermediate `String`.
- Dynamic text and attributes are escaped by default.
- Never pass user content to `UnsafeRaw`.
- Pass browser values through typed RSJS props, signals, and bindings. For a
  justified raw JavaScript bridge, transport dynamic values as data rather
  than interpolating executable source. Keep dynamic CSS data out of raw
  `<style>` text too.
- Render numeric and boolean values directly.

## Minimal component

```rust
use proa_core::{
    ClassList, DataLoader, WebContext, WebRenderSync, WriteError,
};
use proa_macros::html_sync;

pub struct Notice<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<L, C> WebRenderSync<L> for Notice<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new()
            .add("rounded-md border px-4 py-3")
            .add_opt(self.class);

        html_sync! {
            <aside data-component="notice" class={classes}>
                {self.children}
            </aside>
        }
        .render(cx)
    }
}
```

## Verify

```bash
cargo proa fmt --check --verify <changed-rust-paths>
cargo proa lint <changed-rust-paths> --deny warnings
cargo check -p <owning-package>
cargo test -p <owning-package> --lib
python3 scripts/proa_perf_lint.py
```
