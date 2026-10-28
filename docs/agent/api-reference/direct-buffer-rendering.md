# Why components write directly to the output buffer

**Proa renders a component while its local data is alive and returns completion,
rather than requiring it to return a retained tree of borrowed children.** This
keeps ordinary Rust ownership and control flow usable inside `render`.

This records the rationale for the current contract, revisited during metadata
and head-rendering design work in October 2026. It does not establish when the
original decision was made or rule out a different ownership model in a future
API.

```rust
use proa_core::{WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

fn render_label(
    cx: &mut WebContext,
    title: String,
    compact: bool,
) -> Result<(), WriteError> {
    if compact {
        html_sync! { <h1>{title.as_str()}</h1> }.render(cx)
    } else {
        html_sync! { <article><h2>{title.as_str()}</h2></article> }.render(cx)
    }
}
```

Both branches return `Result<(), WriteError>`. Each template finishes using its
borrow before `title` is dropped. Async rendering preserves the same lifetime
relationship inside the rendering future, including across awaits.

## Ownership: retaining an owner and its borrowed children

A returned template may borrow data supplied by its caller. The difficulty is
returning children that borrow data **created or owned inside** the preparation
function:

```rust,compile_fail
struct Label<'a>(&'a str);

struct Prepared<'a> {
    title: String,
    child: Label<'a>,
}

fn prepare(title: String) -> Prepared<'static> {
    let child = Label(title.as_str());
    Prepared { title, child }
}
```

This attempts to return a self-referential value: `child` borrows `title`, while
the returned value also owns `title`. Ordinary safe Rust rejects moving that
owner while borrowed and returning the local borrow (`E0505` / `E0515`). Changing
`'static` to an unconstrained lifetime parameter does not supply an owner that
lives outside the function.

Adding `move` to a closure around an already-borrowed template does not repair
the borrow. Nor does putting that template in a `Box`: heap storage alone does
not establish the required ownership and lifetime relationship.

An **owning closure can work** if it captures the source data first and creates
and renders the borrowed children *inside its invocation*. Other possible
strategies include owned/shared child data, a scoped continuation, or a carefully
designed pinned representation. These are explicit ownership strategies, not a
mechanical change from `.render(cx)` to returning its input.

## Control flow: different branches return different concrete types

`impl WebRenderSync` names one concrete return type for the entire function.
Each dynamic `html_sync!` invocation creates a distinct closure type, so this
ordinary Rust branch fails with `E0308`:

```rust,compile_fail
use proa_core::WebRenderSync;
use proa_macros::html_sync;

fn label(title: &str, compact: bool) -> impl WebRenderSync + '_ {
    if compact {
        html_sync! { <h1>{title}</h1> }
    } else {
        html_sync! { <p>{title}</p> }
    }
}
```

Supporting this return shape needs a common representation: an enum, type
erasure, or a compiler transformation. Another solution is one owning render
closure whose branches each render immediately and return the same `Result`.
Existing `boxed_sync_render` / `boxed_render` adapters already provide explicit
erasure for eligible owned components; their bounds and allocation costs are
documented in [render traits](render-traits.md#genuinely-runtime-selected-children).

This is not a claim that every pair of templates has different types. Fully
static templates can share `StaticHtml`; dynamic `html!` already uses an erased
`AsyncHtml` wrapper. Branches *inside* one template also differ from returning
two separate template values.

## What direct rendering does and does not imply

The macros already return renderable values. Dynamic `html_sync!` produces an
`OptimizedHtml` containing a render closure; calling `.render(cx)` executes it.
That value exposes a byte-writing operation, not an inspectable child tree.
Returning it or boxing it does not let a head-discovery pass see children hidden
inside arbitrary Rust control flow without executing that code.

Direct output also keeps static byte consolidation and generic synchronous
dispatch without requiring a retained node allocation per component. It does
**not** prove a returning-view design would be twice as slow, nor that every
current rendering path is allocation-free. The writer can allocate output
pages, application code can allocate, and dynamic async templates already use
boxed closures and futures. Measure any proposed replacement against the
existing path.

Writing to a buffer is separate from sending bytes over HTTP. The response
adapter decides whether output is buffered or streamed; a write does not imply
that bytes have already reached the client.

Before replacing the contract, demonstrate local borrowed children, divergent
branches, async lifetimes and cancellation, and the resulting allocation and
delivery costs. A closure or `Box<dyn ...>` may be part of that solution; neither
by itself provides child discovery.

## Source and executable evidence

- [`WebRenderSync` and `WebRender`](../../../proa_core/src/render/web_render.rs):
  completion-oriented signatures and compile-fail doctests for both constraints.
- [`OptimizedHtml` and `AsyncHtml`](../../../proa_core/src/shared/html_node.rs):
  current closure storage and execution.
- [Template emission](../../../proa_macros/src/static_consolidation/emit_tokens.rs):
  static consolidation and generated render closures.

Run the relevant doctests with `cargo test -p proa_core --doc render::web_render`.
