---
name: proa-component
description: Author or modify optimized Proa Rust render components in this workspace or in an application. Use for component structs, generic children, variants, ClassList styling, render implementations, component tests, or Proa CLI diagnostics. For browser state, events, hydration, or client validation, use the proa-island skill.
---

# Proa Component

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

For Markdown implementations, follow the [Markdown authoring skill](../proa-markdown/SKILL.md):
write document-shaped raw templates and nested raw Markdown fragments, with
ordinary Rust control flow inside explicit `{ ... }` slots.

Use this workflow for components owned by the current Proa crate or
application. Do not assume a component-library repository, catalog, theme
module, or sibling checkout.

The downstream `proa_ui` library owns its own component and catalog workflow.
When work is explicitly scoped there, follow that repository's skill instead.

Use the island workflow for application browser behavior. Prefer RSJS state,
typed handlers, bindings, and client helpers over executable JavaScript strings,
inline scripts, or hand-written JavaScript assets. Raw JavaScript is a last
resort for a verified, documented RSJS gap; follow the [RSJS-first policy](../../../docs/agent/rsjs-islands.md#prefer-rsjs-for-browser-behavior).

## Workflow

1. Read `docs/agent/components.md` and the nearest component in the owning
   crate. Use `proa_core/tests/landing_page_example.rs` as the explicit-render
   reference.
2. Decide whether the component is opaque SSR, participant-only, or
   origin-capable.
   Deliberately opaque server-rendered components stay in this workflow.
   Components that must participate in RSJS parent composition use the
   `proa-island` workflow even when they emit no JavaScript; use
   `#[rsjs(component)]` for participant-only analysis and add `client` only for
   browser origins.
3. Author an explicit `WebRenderSync<L>` implementation with
   `html_sync!` by default. Add render bounds only to generic values that
   actually appear as children. Use `proa_core::to_async(...)` when a sync
   child must compose inside `html!`. Author the real `WebRender<L>`
   signature only when rendering genuinely awaits.
   `WebContext<L>` owns the default `RenderBuffer`; keep `B: WriteBuf` for an
   explicitly advanced custom-writer integration.
4. Keep static classes in `&'static str` constants. Assemble a fixed number of
   conditional fragments with `ClassList<N>`.
5. Give the root a stable `data-component` attribute. Add `data-variant` when
   callers can select a variant.
6. Add focused render tests for dynamic input, escaping, branch selection,
   attributes, child composition, and ARIA/id contracts.
7. Run:

```bash
cargo proa fmt --check --verify <changed-rust-paths>
cargo proa lint <changed-rust-paths> --deny warnings
cargo check -p <owning-package>
cargo test -p <owning-package> --lib
python3 scripts/proa_perf_lint.py
```

## Hard rules

- For a managed-head HTML document, declare the selected Page explicitly in
  its render-trait impl, even when it has no `render_head` hook:
  `const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::PAGE;`.
  Keep the same role on any local/Send/sync implementations of that Page.
  Ancestor layouts with head hooks must declare `ComponentRole::LAYOUT`.
  Author async hooks as native `async fn render_head(...) -> Result<(), WriteError>`;
  no attribute or handwritten async block is needed. Run
  `cargo proa lint <changed-paths> --deny warnings`: `render/head-role-required`
  rejects overrides with missing roles or an explicit `COMPONENT` role. This is
  a source check for async hooks; Rust cannot detect a default-method override.
  Forwarded/application role constants must resolve to `LAYOUT` or `PAGE`.
  Sync hooks retain a code-generation assertion, so run `cargo build` too;
  `cargo check` alone does not instantiate it. See the
  [managed Page boundary](../../../docs/agent/components.md#managed-html-page-boundary).
- Verify managed Pages through their real document/route composition: one Page
  and one head outlet, layout defaults overridden by the Page, and publication
  before pending body work. Missing roles must surface as errors; do not accept
  a raw body-only render test as proof of correct managed-head integration.
- Do not use `format!`, `String`, `.to_string()`, `.to_owned()`, `.collect()`,
  `.join()`, `Vec::new`, or `push_str` in a render hot path.
- Render numbers and booleans directly.
- Put single-target literal text directly in templates: `"Save"`, not `{"Save"}`.
  Copy shared by HTML and Markdown belongs in shared content fields.
- Never use `UnsafeRaw` for user-provided content.
- Do not render a child to an intermediate `String`; compose typed children.
- Treat linter findings as API or render-shape feedback before suppressing
  them.

## Minimal shape

```rust
use proa_core::{
    ClassList, DataLoader, WebContext, WebRenderSync, WriteError,
};
use proa_macros::html_sync;

pub struct Component<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<L, C> WebRenderSync<L> for Component<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new()
            .add("rounded-md border p-4")
            .add_opt(self.class);
        html_sync! {
            <div data-component="component" class={classes}>
                {self.children}
            </div>
        }
        .render(cx)
    }
}
```
