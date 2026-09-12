# Proa Macros

Ergonomic HTML generation macros for the Proa SSR framework.

## Overview

Proa exposes two HTML macros:

- `html_sync!` for sync `WebRenderSync` components
- `html!` for async-capable `WebRender` components and async roots

Both macros still consolidate static HTML at compile time and generate optimized byte writes. The split exists so sync components keep the original zero-overhead path while async components can compose futures without forcing heap allocation everywhere.

## Which Macro To Use

### `html_sync!`

Use `html_sync!` in normal sync components:

```rust
use proa_core::{WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct Page {
    pub title: &'static str,
}

impl WebRenderSync for Page {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <div class="container">
                <h1>{self.title}</h1>
                <p>"Welcome to Proa"</p>
            </div>
        }
        .render(cx)
    }
}
```

### `html!`

Use `html!` in async-capable components:

```rust
use std::future::Future;

use proa_core::{to_async, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa_macros::{html, html_sync};

pub struct Header;

impl WebRenderSync for Header {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <header>"Header"</header> }.render(cx)
    }
}

pub struct AsyncPage;

impl WebRender for AsyncPage {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            html! {
                <main>
                    {to_async(Header)}
                    <p>"Loaded asynchronously"</p>
                </main>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
```

## Sync Children In Async Trees

Built-in text and fragment types already implement `WebRender`, so ordinary interpolations continue to work in `html!`.

Custom sync components do not get an automatic blanket lift. Wrap them explicitly:

```rust
{proa_core::to_async(MySyncComponent { ... })}
```

That keeps the trait system coherent while still making mixed trees possible.

## Supported Syntax

Both macros support the same HTML syntax:

- HTML elements: `<div>`, `<span>`, `<img />`, and so on
- string literals as text nodes
- dynamic attributes like `class={classes}`
- boolean attributes like `disabled`
- Rust expressions inside `{...}`
- nested Proa fragments
- compile-time validation for invalid HTML and accessibility issues

## Notes

- `html_sync!` composes `WebRenderSync` children directly and is the preferred default for existing UI code.
- `html!` drives child expressions through `proa_core::render_child(...)`, so it can await async children.
- Neither macro allocates intermediate `String`s for static markup.
- Use `UnsafeRaw` only for trusted pre-rendered HTML.

See the [render-trait guide](../docs/agent/api-reference/render-traits.md) for
async composition and `proa_core` for the trait definitions.
