# Authoring components

This guide covers render components owned by this repository or by an
application built with Proa. It does not assume a component library, catalog,
theme system, or neighboring checkout.

The `proa_ui` design-system library is a downstream consumer of Proa. Its
components and catalog follow the instructions in that repository.

Before authoring a page, discovery document, or error response, read
[`document-rendering.md`](document-rendering.md). It covers format-specific
macros and shared HTML/Markdown implementations, including cached documents.

## Managed HTML Page boundary

Every managed-head HTML document needs one explicitly declared Page. Put this
constant in the selected Page's `WebRender` or `WebRenderSync` impl, and in any
corresponding Send render impl:

```rust
const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::PAGE;
```

This is required even when the Page has no head hook and inherits all metadata.
Ancestor layouts implementing `render_head` declare
`const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;`.
Only the Page seals metadata before its body; a head hook alone
does not declare a Page. Ordinary `COMPONENT` values skip head invocation entirely.
Manual documents without managed-head configuration do not require this role.
A component declaring `LAYOUT` or `PAGE` renders only inside a managed document,
even with the default no-op hook. Layouts must precede the selected Page and
cannot appear after sealing or inside Suspense/captures/fallbacks. This rule is
the same for sync, async, Send and adapted rendering. Plain
`WebContext::new()`, `render_to_string`, or `.without_head()` fails with
`HeadError::ManualMode`. Keep visual wrappers on the default `COMPONENT` role
when they need to render inside a Page or in manual mode.

Test through the document's actual managed route, including an outlet and an
explicit Page, and verify head publication while body work remains pending.
Calling `render` directly exercises only the body and bypasses lifecycle dispatch.
Run `cargo proa lint <changed-paths> --deny warnings` for changed hooks.
`render/head-role-required` rejects an override with a missing role declaration
or an explicit `COMPONENT` role. Native async hooks cannot expose override
presence to Rust, so the async contract relies on this source check. Forwarded
roles and application constants are not evaluated by the lint; ensure they resolve
to `LAYOUT` or `PAGE`. Synchronous hooks also retain a compiler assertion during
code generation; run `cargo build`, since `cargo check` does not instantiate it.

A Page with neither a hook nor a declared role remains indistinguishable from an
ordinary component. Choosing `LAYOUT` for the selected Page also leaves no Page
boundary. These cases leave preparation unsealed: a completed render fails with
`MissingPage` or `OrphanProvider`, while pending body work can wait until the
preparation deadline (30 seconds by default in the Axum streaming adapter).

Keep interactive islands inside an ordinary SSR Page wrapper. The current
`#[rsjs(component...)]` analyzer accepts only the render method in its impl;
put the Page role and head hook on the wrapper rather than the analyzed island.
See [managed document head](api-reference/render-traits.md#managed-document-head)
for dispatch, inheritance, and hook signatures.

## Pick the render path

Use `WebRenderSync` and `html_sync!` unless rendering must await data. Author
the render trait explicitly:

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct Notice<C = ()> {
    pub tone: NoticeTone,
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<L, C> WebRenderSync<L> for Notice<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = notice_classes(self.tone).add_opt(self.class);
        html_sync! {
            <aside
                data-component="notice"
                data-variant={self.tone.as_str()}
                class={classes}
            >
                {self.children}
            </aside>
        }
        .render(cx)
    }
}
```

`WebContext<L>` owns the default reusable `RenderBuffer`. Keep the second
writer generic off ordinary components; add `B: WriteBuf` and use
`WebContext<L, B>` only for an integration that deliberately supports custom
writers.

Use `proa_core::to_async(component)` when this sync component must compose
inside an `html!` tree. Implement `WebRender` with its real
`RenderOutcome<impl Future>` signature only when the component itself needs
`.await`.

Inside a template's `if`, `for`, or `match` slot, leave the final child as an
expression without `.render(cx)?;`:

```rust
{if show {
    Child { value }
}}
```

The macro renders that final expression and propagates errors. Explicit calls
are still needed in ordinary Rust code, in closures, or before later statements
in the same block. Keep the outer `.render(cx)` that executes the whole template.

For a declarative component that maps directly to one HTML tag,
`#[derive(ProaComponent)]` remains available. Custom or multi-element renderers
use the explicit traits above.

An undecorated render-trait impl is deliberately opaque to RSJS. If a reusable
component must participate in a parent's live client graph, annotate this same
real trait impl with `#[rsjs(component)]` even when it emits no JavaScript.
Add `client` only when the component originates browser behavior.

Read
[`api-reference/render-traits.md`](api-reference/render-traits.md) before
choosing the async path.

## File layout

Small application components can live in one module. For a component with
variants and tests, prefer:

```text
src/components/notice/
├── mod.rs
├── notice.rs
└── recipe.rs
```

- `mod.rs` declares modules and re-exports public types.
- `notice.rs` owns the component, render implementation, and focused tests.
- `recipe.rs` owns variant enums, static class constants, and class selection.

Register the module only in the crate that owns it. There is no framework-wide
catalog registration step.

## Variants and classes

Keep class selection outside the HTML tree and allocation-free:

```rust
use proa_core::ClassList;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoticeTone {
    #[default]
    Neutral,
    Positive,
    Critical,
}

impl NoticeTone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Positive => "positive",
            Self::Critical => "critical",
        }
    }

    const fn classes(self) -> &'static str {
        match self {
            Self::Neutral => "border-zinc-300 bg-zinc-50 text-zinc-950",
            Self::Positive => "border-emerald-300 bg-emerald-50 text-emerald-950",
            Self::Critical => "border-red-300 bg-red-50 text-red-950",
        }
    }
}

const NOTICE_BASE: &str = "rounded-md border px-4 py-3";

pub const fn notice_classes(tone: NoticeTone) -> ClassList<2> {
    ClassList::new().add(NOTICE_BASE).add(tone.classes())
}
```

Use a static string for classes known at compile time and `ClassList<N>` for a
fixed number of conditional fragments. Do not use `format!`, `String`,
`.join`, or `push_str` inside `render`.

If an application defines a compile-time class concatenation helper, it may use
that helper for its own constants. Proa itself does not require a particular
CSS framework or theme-token module.

## Children

Use `C = ()` plus `Option<C>` for zero-cost generic children:

```rust
pub struct Card<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl Default for Card<()> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
```

Add `WebRenderSync<L>` bounds only to generic types that flow into child
positions. For multiple structural regions:

```rust
pub struct Shell<H = (), C = (), F = ()> {
    pub header: Option<H>,
    pub children: Option<C>,
    pub footer: Option<F>,
}

impl<L, H, C, F> WebRenderSync<L> for Shell<H, C, F>
where
    L: DataLoader,
    H: WebRenderSync<L>,
    C: WebRenderSync<L>,
    F: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div data-component="shell">
                <header>{self.header}</header>
                <main>{self.children}</main>
                <footer>{self.footer}</footer>
            </div>
        }
        .render(cx)
    }
}
```

## Root attributes

Application components should expose a stable root hook:

- `data-component="<kebab-name>"` identifies the component.
- `data-variant="<token>"` identifies a public visual or behavioral variant.
- `class: Option<&'static str>` lets a caller add classes without allocating.

These attributes make DOM tests and integrations independent of generated or
utility classes.

## Testing

Render the component through the normal buffer and assert public invariants:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::text::text;
    use proa_core::{WebContext, WebRenderSync};

    #[test]
    fn notice_renders_variant_children_and_custom_class() {
        let mut cx = WebContext::new();

        Notice {
            tone: NoticeTone::Positive,
            class: Some("mt-4"),
            children: Some(text("<saved>")),
        }
        .render(&mut cx)
        .unwrap();

        let html = cx.into_output().into_string();
        assert!(html.contains(r#"data-component="notice""#));
        assert!(html.contains(r#"data-variant="positive""#));
        assert!(html.contains("mt-4"));
        assert!(html.contains("&lt;saved&gt;"));
        assert!(!html.contains("<saved>"));
    }
}
```

Test every variant mapping, optional field in both states, user-controlled text
escaping, and stable ARIA/id relationships. Snapshot whole pages only when
structural diffs are useful and deterministic.

## Verification

From the repository root:

```bash
cargo proa fmt --check --verify path/to/changed.rs
cargo proa lint path/to/changed.rs --deny warnings
cargo check -p <owning-package>
cargo test -p <owning-package> --lib
python3 scripts/proa_perf_lint.py
```

See also:

- [`cookbook.md#7-generic-children-component`](cookbook.md#7-generic-children-component)
- [`api-reference/conventions.md`](api-reference/conventions.md)
- [`api-reference/slot-grammar.md`](api-reference/slot-grammar.md)
- [`../../proa_core/tests/landing_page_example.rs`](../../proa_core/tests/landing_page_example.rs)
- [`../../proa_macros/tests/proa_component_launch.rs`](../../proa_macros/tests/proa_component_launch.rs) for the separate declarative `#[derive(ProaComponent)]` path
