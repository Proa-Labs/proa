# Render traits: sync, local async, and `Send` async

Proa has a compatibility pair for ordinary component trees and an opt-in
`Send` pair for render graphs that may migrate between executor threads. Pick
the surface explicitly; the choice is not auto-detected.

## The compatibility traits

```rust
// Sync. The fast path. Use this by default.
pub trait WebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}

// Async-capable. The render method returns a `RenderOutcome` that may resolve
// either synchronously (`Done(r)`) or via a future (`Pending(fut)`).
pub trait WebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>;
}
```

`B` is the advanced custom-writer parameter. Because it defaults to the owned
`RenderBuffer`, ordinary application components customize only `L` and use
`WebRenderSync<L>` / `WebRender<L>` with `WebContext<L>`.

The future returned by `WebRender` intentionally has no blanket `Send` bound.
That permits local futures without tying the rendering core to a particular
executor. It does not mean every stream must be `!Send`, nor that a server
adapter should bridge a stream that is otherwise `Send`-capable. See
[`Send` policy and runtime portability](../../../proa_framework/proa_framework_core/README.md#send-policy-and-runtime-portability)
for the architectural decision and adapter rule.

## The opt-in `Send` traits

Use the companion surface when the complete render graph is send-safe:

```rust
pub trait WebRenderSendSync<
    L: DataLoader = NoLoader,
    B: WriteBuf + Send = RenderBuffer,
>: Send {
    fn render_send_sync(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> Result<(), WriteError>;
}

pub trait WebRenderSend<
    L: DataLoader = NoLoader,
    B: WriteBuf + Send = RenderBuffer,
>: Send {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send>;
}
```

`SendWebContext` keeps request state and detached boundary artifacts in
send-safe, request-owned storage. `WebRenderSend` is a real compiler proof: the
component value, every pending continuation, the writer, and every child must
all satisfy the stronger contract. It does not spawn work or promise parallel
execution by itself.

## Decision rule

| Your component | Pick |
|---|---|
| Pure markup, no I/O, no `.await` | `WebRenderSync` |
| Reads `cx.load::<T>(id).await` (dataloader) | `WebRender` |
| Calls `query_with_initial(...)` with an already-available initial value | `WebRenderSync` |
| Must `.await` to produce a query's initial value | `WebRender` |
| Calls `mutation(...)` inside an analyzed component | `WebRender` (mutation declarations remain on the async surface) |
| Composes a child that is async | `WebRender`, OR keep sync and use `proa_core::to_async(child)` if you can isolate the async leaf |
| Complete async tree is `Send` and should stream directly through a multithreaded adapter | `WebRenderSend` + `html_send!` |
| Send tree contains a send-safe synchronous child | `WebRenderSendSync`, then `proa_core::to_send(child)` |

## Default: `WebRenderSync`

Almost every static component is sync. Catalog pages, application components,
and the `Card<C>` family normally implement `WebRenderSync`.

```rust
impl<L, C> WebRenderSync<L> for Card<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div class={card_classes(self.variant)}>{self.children}</div>
        }
        .render(cx)
    }
}
```

The authored bounds make the component's contract visible to rustc and IDEs.
Apply render bounds only to generic template children; ordinary state generics
do not need them.

## When you actually need `WebRender`

The most common reason is a dataloader fetch. A server-only component uses the
authored trait directly, with no RSJS attribute:

```rust
impl<L: DataLoader> WebRender<L> for UserCard {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let user = cx
                .load::<User>(LoadKey::typed("user", self.user_id))
                .await?;
            html! { <div>{user.name}</div> }.render(cx).resolve().await
        })
    }
}
```

This is the only supported async authoring shape. `async fn render` sugar is
intentionally rejected. If the same component participates in RSJS analysis,
put `#[rsjs(component)]` on the trait impl; add `client` only if it originates
browser behavior.

## Mixing sync and async children

In an `html!` (async) tree, all children must be `WebRender`. To embed a
`WebRenderSync` child:

```rust
use proa_core::to_async;

html! {
    <div>
        { to_async(my_sync_component) }
    </div>
}
```

`to_async` is a zero-cost adapter: it implements `WebRender` by delegating to
the inner `WebRenderSync`'s sync render and wrapping the result in
`RenderOutcome::Done`.

For single-element components generated with `#[derive(ProaComponent)]`, add
`async_render` to the component attribute:

```rust
#[derive(proa_macros::ProaComponent)]
#[component(tag = "aside", async_render)]
struct Notice<C = ()> {
    #[component(children)]
    children: Option<C>,
}

impl<C> Notice<C> {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}
```

The derive can prove that the generated component is synchronous, so it emits
the same immediate `WebRender` bridge for that concrete type. Such components
compose directly in `html!` without a call-site `to_async(...)`. Explicit
hand-written `WebRenderSync` implementations use the adapter.

In an `html_sync!` (sync) tree, all children must be `WebRenderSync`. There is
no inverse adapter (you cannot embed an async child in a sync tree).

## Authoring a compiler-proved `Send` tree

`html_send!` mirrors `html!`, but it composes `WebRenderSend` children through
`SendWebContext` and requires its generated future to be `Send`:

```rust
impl<L: DataLoader> WebRenderSend<L> for SearchPage {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        html_send! {
            <main>
                {SearchResults { query: self.query }}
            </main>
        }
        .render_send(cx)
    }
}
```

Use `to_send(...)` for a child that implements `WebRenderSendSync`, and
`await_send_component(...)` for a `Send` future that produces a
`WebRenderSend` child. `to_send_static(...)` is narrower: it adapts an existing
`WebRenderSync` value only when that value is output-only and does not inspect
request state, register artifacts, or install stream callbacks.

At an Axum route boundary, choose `ssr_async_send(...)` for buffered delivery
or `ssr_stream_send(...)` for direct streaming. Their typed-loader variants
end in `_with_loader`. Ordinary `ssr_async(...)` and `ssr_stream(...)` retain
support for local futures and therefore use the bounded local executor when
streaming.

## Genuinely runtime-selected children

Keep normal component trees generic: that is the static-dispatch,
allocation-free path. When a plugin registry, runtime catalog, or data-driven
branch truly needs to select between unrelated concrete component types, use
the explicitly boxed escape hatches:

```rust
use proa_core::{boxed_render, boxed_sync_render};

let sync_child = if use_card {
    boxed_sync_render(CardView { /* ... */ })
} else {
    boxed_sync_render(TableView { /* ... */ })
};

html_sync! { <main>{sync_child}</main> }
```

`boxed_sync_render(...)` returns `BoxedWebRenderSync<L, B>` and composes in
either `html_sync!` or `html!`. `boxed_render(...)` returns
`BoxedWebRender<L, B>` for unrelated async-capable children:

```rust
let async_child = if use_local {
    boxed_render(LocalResults { /* ... */ })
} else {
    boxed_render(RemoteResults { /* ... */ })
};

html! { <main>{async_child}</main> }
```

The surrounding render context normally infers `B` and `L`. The selected
component must be `Send + 'static`. Each wrapper stores it in one `Box`
(a non-zero-sized component normally incurs one heap allocation) and renders
through a vtable call. An async erased component also boxes its pending future
at that boundary, and polling that future uses dynamic dispatch; dropping the
pending render drops that future. The future is allowed to be `!Send`, just
like an ordinary `WebRender` future. `boxed_sync_render(...)` takes the
immediate `RenderOutcome::Done` path in an async tree and does not box a
future.

Root helpers grow their output as needed. Advanced adapters choose the writer
that matches their ownership and streaming requirements instead of carrying
per-component allocation hints.

These costs and restrictions are deliberately opt-in and do not change
generic components, `#[derive(ProaComponent)]`, or ordinary `html_sync!`
trees.

## Markdown has the same split

Markdown rendering uses the same `WebContext<L, B>` as HTML. Both Markdown
traits default to `L = NoLoader` and `B = RenderBuffer`; application components
usually customize only `L`. Loaders, response metadata, metering, and streaming
state are shared, while the chosen trait controls representation and escaping.

Markdown rendering deliberately mirrors HTML:

| Markdown component | Template macro | Child trait |
|---|---|---|
| `MdRenderSync<L>` | `md_sync!` | `MdRenderSync<L>` |
| `MdRender<L>` | `md!` | `MdRender<L>` |

Use `md_sync!` by default:

```rust
impl<L: DataLoader> MdRenderSync<L> for Article {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        md_sync! { r###"
            # {self.title}

            {self.body}
            "###
        }
        .render_md(cx)
    }
}
```

Use `md!` when a child may await:

```rust
impl<L: DataLoader> MdRender<L> for Article {
    fn render_md(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::Pending(async move {
            proa_core::render_md_child(
                cx,
                md! { r###"
                    # {self.title}

                    {self.async_body}
                    "###
                },
            )
            .await
        })
    }
}
```

Primitive strings and numbers implement both Markdown traits. A custom
sync-only child placed inside `md!` must be lifted explicitly:

```rust
md! { r###"
Result: {proa_core::to_md_async(sync_child)}
"###
}
```

An async child cannot be placed inside `md_sync!`.

The default authoring form is one multiline raw template delimited with three
hashes: `md_sync! { r###"..."### }` or `md! { r###"..."### }`. Its source
newlines are the rendered Markdown newlines, while `{expression}` composes a
typed child and template `for` / `if` blocks provide control flow. Common Rust
source indentation is stripped while deeper relative Markdown indentation is
preserved. Braces inside fenced code blocks and inline code spans stay literal;
outside code, double a literal brace as `{{` or `}}`. Increase the hash count
only when the Markdown contains the exact closing sequence `"###`.

The expression-visible token form remains supported for generated templates:
`md_sync! { "# " { title } }`. In that form, a new top-level source line
establishes a context-aware Markdown block boundary.

### Convert existing HTML components to Markdown

Enable the optional converter on the core dependency:

```toml
proa_core = { version = "0.3.0", features = ["html-to-markdown"] }
```

A synchronous HTML component can delegate its Markdown implementation without
maintaining a second template:

```rust
impl<L: DataLoader> MdRenderSync<L> for Article {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        proa_core::to_markdown(self).render_md(cx)
    }
}
```

`to_markdown(value)` is an explicit adapter, not a blanket trait implementation.
It always calls `WebRenderSync<L>` on the wrapped value, so delegation does not
recurse into `render_md`. The source component uses its default owned
`RenderBuffer`; the destination Markdown context can use any `WriteBuf`.

You can also wrap an HTML fragment directly, without defining a component:

```rust
md_sync! { r###"
    # Overview

    {proa_core::to_markdown(html_sync! {
        <p>"Compiled templates with " <strong>"fast rendering"</strong> "."</p>
    })}

    {custom_markdown_child}
    "###
}
```

The converter uses ATX headings, fenced code, inline links, dash lists, and
GFM tables. Simple and headerless grids retain all cells; tables with spans or
block content use a list of rows and cells. See the [element coverage and edge
cases](html-to-markdown.md) for the full conversion policies and limits.
Containers and CSS are dropped; metadata elements, scripts, noscript,
styles, templates, SVG, and explicitly hidden/aria-hidden elements are omitted.
CSS visibility is not evaluated. The result has no leading or trailing
newline, so put separators in the surrounding template as shown above. Inline
fragments can be interpolated inline.

Capture preserves the live loader and request state, including request-state
mutations made by the HTML renderer. HTML escaping, Markdown formatting state,
and streaming hooks are isolated and restored even on errors or panics.
Capture/conversion errors do not append partial HTML to the Markdown output;
final Markdown writes still obey the destination writer's capacity and metering.

This is a buffered, allocating conversion: it builds temporary HTML, a DOM, and
a Markdown string. It is not the zero-allocation Markdown fast path and does
not support async HTML sources. The synchronous adapter implements `MdRender`
as an immediate completion, so it also composes directly inside async `md!`.

Child components inside the converted subtree use their HTML representations,
even when they implement a Markdown trait. To preserve a custom Markdown child,
compose it beside the converted fragment as shown above. Semantics inferred
from CSS or interactions cannot be recovered; keep handwritten Markdown for
components that need different headings, summaries, or omitted controls.

Runnable example: [`proa_core/examples/html_to_markdown.rs`](../../../proa_core/examples/html_to_markdown.rs).

## Inside an analyzed component

The `#[rsjs(component)]` macro respects whichever trait you wrote. Add
`client` when the component originates browser behavior. A
`query_with_initial(...)` declaration is valid in `WebRenderSync` when its
initial value is already available. If producing that value needs
`cx.load(...).await`, choose `WebRender` and `html!`; the macro never silently
switches traits.

## Performance notes

- `WebRenderSync` is **the** fast path: no future allocation, no `RenderOutcome` wrapper, no async dispatch overhead. Stick with it whenever possible.
- An impl that returns `RenderOutcome::Done` costs nothing beyond the match arm. `to_async(sync_child)` is exactly that shape.
- **The macro, not the trait, is what allocates.** An `html!` (or `md!`) block with any dynamic content emits `async_html_from_fn(Box::new(|cx| Box::pin(async move { .. })))`: one heap allocation for the erased closure, one for the future, and dynamically dispatched polls, whether or not the body ever awaits. `html_sync!` emits a monomorphized `OptimizedHtml<B, L, F>` with an `#[inline(always)]` render and no allocation. So an async component whose template happens to be static still pays twice per node.
- Prefer `html_sync!` for every subtree that has no async child, even inside a `WebRender` impl that awaits elsewhere. Wrap the result in `to_async(..)` at the seam if it composes into an async tree.

### Lints that enforce this

`cargo proa lint` reports four related findings:

| Code | Fires on |
|---|---|
| `render/async-unneeded` | a `WebRender` impl whose body never awaits |
| `render/md-async-unneeded` | the `MdRender` equivalent |
| `perf/async-html-static-subtree` | an `html!` whose whole subtree is synchronous |
| `perf/async-md-static-subtree` | an `md!` with no async interpolation |

The impl-level rules stay quiet when the answer is not knowable from the source: a generic parameter bound by `WebRender`/`MdRender`, a body that completes through `RenderOutcome::Done`, a body that forwards a foreign `RenderOutcome` (`Pending(<call>)` or a tail `render*(..)` call), or a template holding a bare unit-struct child. Suppress a remaining true positive you intend to keep with `// proa-perf-ok: reason`.

## See also

- [`proa_core/src/render/web_render.rs`](../../../proa_core/src/render/web_render.rs) — actual trait definitions.
- [`proa_core/src/render/outcome.rs`](../../../proa_core/src/render/outcome.rs) — `RenderOutcome` and `.resolve()`.
- [`rsjs_macros/src/parse.rs`](../../../rsjs_macros/src/parse.rs) — accepted analyzed-component signatures.
