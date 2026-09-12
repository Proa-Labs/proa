# proa_stream

Concurrent data loading and streaming server-side rendering for Proa.

`proa_stream` sits between the render traits in `proa_core` and an HTTP
adapter. It provides request-scoped load deduplication, explicit `Suspense`
boundaries, in-order Markdown streaming, and out-of-order HTML boundary
framing. It does not own routing, sockets, or an HTTP runtime.

```text
proa_core                      components, render contexts, load keys
    |
proa_stream                    data coordination and streaming drivers
    |
proa_framework_core           framework-neutral response contracts
    |
proa_framework_axum           Axum and Tokio transport adapter
```

Application code normally uses this crate through
`proa_framework_axum::RouteResponse`. Authors of another framework adapter can
drive the lower-level APIs directly.

## What it provides

- `Suspense<C, Fb>` renders useful fallback HTML in the shell and resolves the
  child behind a streaming boundary.
- `DataResolver<L>` shares repeated same-key loads for one request. It is a
  single-flight and memoization layer, not a cross-request cache.
- `PreparedLoader<L>` starts known loads before an in-order render reaches
  them, preserving concurrent data fetching without reordering output.
- `CollectLoadKeys` gives route values an explicit way to declare those known
  preload keys.
- Out-of-order drivers schedule independent boundaries, honor priority and
  deadlines, and keep rendered RSJS artifacts attached to the boundary that
  produced them.
- The in-order Markdown driver streams typed Markdown without browser-side
  swapping.
- Boundary framing and the small `$proa` swap runtime support CSP nonces and
  safely transport arbitrary UTF-8 HTML.

## Application quick start

Wrap independently slow content in `Suspense`, then return a streaming route
response:

```rust,ignore
use proa_macros::{html, html_sync};
use proa_stream::Suspense;

html! {
    <main>
        <h1>"Product"</h1>
        {Suspense::new(
            Reviews { product_id: self.product_id },
            html_sync! { <div class="skeleton">"Loading reviews..."</div> },
        )}
        {Suspense::new(
            Stock { product_id: self.product_id },
            html_sync! { <div class="skeleton">"Loading stock..."</div> },
        )}
    </main>
}
```

At the route boundary, install one request-scoped resolver around the
application loader:

```rust,ignore
use std::sync::Arc;

use proa_framework_axum::RouteResponse;
use proa_stream::DataResolver;

let loader = DataResolver::request_scope(Arc::clone(&state.loader));

RouteResponse::ssr_stream_with_loader(
    ProductPage { product_id },
    loader,
)
```

Components request data through their `WebContext`:

```rust,ignore
let product: Product = cx.load_json(product_key(self.product_id)).await?;
```

Every component that requests the same `LoadKey` receives the same in-flight
or resolved result during that request. The application loader still owns
origin access, retries, and caching across requests.

## Streaming modes

`Suspense` adapts to the render mode selected by the response:

| Mode | Wire behavior | Browser runtime | `priority` / `timeout` |
|---|---|---|---|
| Buffered or static HTML | Child renders inline | None | Ignored |
| `ChunkedInOrder` HTML | Child content streams in document order | None | Ignored |
| `ChunkedOutOfOrder` HTML | Fallback ships in the shell; child swaps in when ready | Small `$proa` runtime | Honored |
| Markdown | Child content streams in document order | None | Ignored |

Out-of-order is the default for `RouteResponse::ssr_stream`. A fast boundary
may arrive before an earlier slow boundary. When several boundaries become
ready in the same driver turn, they are ordered by `Critical`, `Optional`,
then `Defer`, followed by boundary id.

```rust,ignore
use std::time::Duration;

use proa_core::Priority;
use proa_stream::Suspense;

Suspense {
    child: Recommendations { product_id },
    fallback: LoadingRecommendations,
    priority: Priority::Defer,
    timeout: Some(Duration::from_millis(250)),
}
```

An out-of-order timeout cancels the child and leaves its fallback in place.
The timeout options are deliberately ignored by in-order and buffered modes:
canceling after inline bytes have been emitted could leave partial markup on
the wire.

## In-order preloading

In-order output cannot pass a slow boundary, so start predictable loads before
rendering. `CollectLoadKeys` keeps that discovery explicit:

```rust,ignore
use proa_core::LoadKey;
use proa_stream::CollectLoadKeys;

impl CollectLoadKeys for ProductPage {
    fn collect_load_keys(&self, keys: &mut Vec<LoadKey>) {
        keys.push(product_key(self.product_id));
        keys.push(reviews_key(self.product_id));
        keys.push(stock_key(self.product_id));
    }
}
```

Framework adapters can feed `page.load_keys()` into their preload response
API. Typed integrations can compose the layers directly:

```rust,ignore
use std::sync::Arc;

use proa_core::RenderMode;
use proa_framework_axum::RouteResponse;
use proa_stream::{CollectLoadKeys, DataResolver, PreparedLoader};

let page = ProductPage { product_id };
let keys = page.load_keys();
let resolver = DataResolver::request_scope(Arc::clone(&state.loader));
let loader = Arc::new(PreparedLoader::new_typed(resolver, keys));

RouteResponse::ssr_stream_with_loader(page, loader)
    .with_render_mode(RenderMode::ChunkedInOrder)
```

The load path is then:

```text
WebContext
    -> PreparedLoader       known keys start before rendering
    -> DataResolver         one load per key for this request
    -> application loader   origin fetches and cross-request cache policy
```

`LoadHints` are forwarded to the application loader but are not part of
deduplication identity. If different requests must produce different payloads,
encode that difference in the `LoadKey`.

## Runtime feature

The default `rt-tokio` feature makes `PreparedLoader` spawn known loads eagerly
on the ambient Tokio runtime and gives the out-of-order driver a Tokio clock
for elapsed-time deadlines.

```toml
[dependencies]
proa_stream = { version = "0.3.0", git = "https://github.com/Proa-Labs/proa.git" }
```

For a runtime-agnostic dependency:

```toml
[dependencies]
proa_stream = { version = "0.3.0", git = "https://github.com/Proa-Labs/proa.git", default-features = false }
```

Without `rt-tokio`, prepared loads begin lazily when first awaited and overlap
through cooperative polling. The portable boundary scheduler exposes
`next_deadline` and `expire` so an adapter can supply its own clock. Its
convenience `next_event` conservatively expires configured timeouts at
admission because the crate has no clock adapter in that configuration.

## Writing a framework adapter

Normal applications should let `proa_framework_axum` handle framing. Adapter
authors use `StreamContext` and
`drain_pending_boundaries_with_nonce_and_artifacts` to drain detached HTML
boundaries.

Each `OutOfOrderBoundaryChunk` contains:

- the boundary id;
- the complete transport frame in `bytes`; and
- the RSJS islands, query seeds, and shared-state seeds committed by that
  boundary.

Serialize the artifact metadata immediately before `chunk.bytes`. The swap
frame dispatches `proa:boundary-ready`, so metadata sent afterward arrives too
late for hydration. Use the nonce-aware drain and `write_inline_runtime_tag`
when the response has a Content Security Policy nonce.

The byte-only drain helpers are deprecated because discarding boundary
artifacts breaks deferred interactive content.

Markdown adapters use `render_md_in_order_to_flusher`. The supplied
`StreamChunkFlusher` receives ordered chunks while the root `RenderBuffer`
recycles its pages. `render_md_in_order_to_vec` is a convenient test and
non-streaming integration helper.

## Development

From the workspace root:

```bash
cargo test -p proa_stream
cargo check -p proa_stream --no-default-features
cargo run -p proa_stream --example boundary_framing_fixture -- c3RyZWFtLW5vbmNl
```

## Further reading

- [Streaming SSR guide](../proa_marketing/src/docs/core/streaming-ssr.mdx)
- [Data loading guide](../proa_marketing/src/docs/framework/data-loading.mdx)
- [Render traits and async composition](../docs/agent/api-reference/render-traits.md)
- [`proa_framework_core`](../proa_framework/proa_framework_core/README.md)
- [`proa_framework_axum`](../proa_framework/proa_framework_axum/README.md)

Licensed under the [Apache License 2.0](../LICENSE).
