# proa_framework_core

The framework-agnostic contract layer between Proa's render engine and any
HTTP server. Code in this crate knows how a Proa route turns into a
response: route tables, metadata cascade, island manifests, response
staging, streaming finalization. It does not know which server, runtime, or
transport delivers that response. There is no axum here, no sockets, no
threads, no clocks, and (with the `rt-tokio` feature disabled) no tokio
anywhere in the dependency tree.

## Where it sits

```
proa_core / proa_stream         render engine: bytes into caller-owned buffers
            |
proa_framework_core             contracts: routes, metadata, manifests, response staging
            |
proa_framework_axum (axum + tokio)   the shipped adapter: sockets, threads, clocks
your adapter (anything else)    implements the same contracts
```

If you are building an app on axum, depend on `proa_framework_axum` and you get
all of this re-exported. Depend on this crate directly when you are writing
an adapter for a different framework or runtime.

## `Send` policy and runtime portability

This is an intentional contract decision:

- `WebRender::render(...)` does **not** require its returned future to be
  `Send`. A component may capture request-local `Rc` state, and current render
  contexts use `Rc`/`RefCell` internally. Adding a blanket `Send` bound would
  reject those components and strengthen the public component contract for the
  needs of one class of server adapter.
- Proa's runtime independence comes from keeping Tokio, Axum, and other
  executor/transport APIs out of this contract layer. It does not come from
  requiring streams to be `!Send`. `Send` is a standard Rust capability, and a
  `Send` future can still be polled by a single-threaded or non-Tokio executor.
- Therefore `!Send` is **permitted, not required**. Code at an erased boundary
  should preserve a render stream's `Send` capability when it has one. An HTTP
  adapter should poll a `Send`-capable stream directly when its body API allows
  that, and use a local-executor bridge only for a genuinely `!Send` stream.
- `Send` does not request threads or parallel execution. On a one-worker
  runtime, a `Send` future still executes on that one worker; the marker only
  permits safe movement between threads.

`FrameworkBody` has exactly two wire shapes: `Full(Bytes)` and
`Stream(FrameworkStreamFactory)`. The nested factory is either `Send` or
`Local`. Axum polls a compiler-proved `Send` stream directly and reserves its
bounded pinned-worker bridge for `Local`. Existing `WebRender` route
constructors remain on the compatible `Local` path until their complete render
graph carries a stronger proof.

The proof is expressed by `WebRenderSend` / `WebRenderSendSync`,
`SendWebContext`, and `html_send!`. Route adapters preserve it with
`ssr_async_send(...)` and `ssr_stream_send(...)`; no runtime test or unsafe
cast upgrades an ordinary `WebRender` tree. In out-of-order mode, ordinary
boundaries are polled concurrently inside the response stream. A runtime
adapter may supervise timed boundaries in tasks so deadlines keep progressing
when transport backpressure stops body polling.

Do not make this crate Tokio-specific, blanket-replace request-local `Rc` state
with `Arc<Mutex<_>>`, or silently remove support for local futures merely to
satisfy a server body's `Send` bound. Keep ownership cheap in the renderer and
express the stronger capability only where the complete render graph can
actually uphold it.

## Modules

| Module | What it defines |
|---|---|
| `route_spec` | `RouteSpec` / `ResolvedRouteSpec` route tables, `RouteMode` (static vs dynamic), static-path enumeration, metadata map computation |
| `route_snapshot` | `FrameworkRouteSnapshot` and route-pattern parameter extraction |
| `metadata` | The `Metadata` trait and `CascadeValue` cascade (titles, meta tags) resolved per route |
| `response` | The opaque `RouteResponse` render plan, `FrameworkStreamFactory`, and portable finalizers for buffered and streamed responses |
| `manifest` | `IslandManifest`, asset entries, and hydration strategy config produced at build time |
| `bootstrap` | Renders the island bootstrap script for the islands a page actually used |

## Writing an adapter

An adapter owns everything IO-shaped. The contract surface it implements:

1. Map the incoming request to a `proa_core` request context and side
   channels (`ResponseContext`, `HeadContext`, and friends).
2. Render. Sync pages need no runtime at all; async pages need any
   executor's `block_on`; streaming routes produce a
   `FrameworkStreamFactory`.
3. Match `FrameworkStreamFactory::Send` and `::Local`. Poll the former
   directly. Keep the latter pinned after invoking its transferable unopened
   factory. Before committing response headers, asynchronously obtain its
   first item so an early render error can still become an HTTP 500.
4. For buffered renders, copy the staged `ResponseContext` status and headers
   into the native response after rendering. A streaming adapter must finalize
   the native response head before it opens the body factory; streamed render
   contexts therefore reject status/header mutation.
5. Serve the manifest's assets and emit the bootstrap script for used
   islands.

### A minimal adapter, end to end

A buffered sync route on a hand-rolled server, with no axum and no
runtime. `HomePage` is any ordinary `WebRenderSync<L>` component using the
default owned writer; the tuple stands in for whatever response type your
server uses. `RenderBuffer` is the
standard reusable buffer: it chains fixed-size pages on overflow, so a
render never reallocates or copies what it has already written:

```rust
use proa_core::WebContext;

fn handle_request() -> (u16, Vec<(String, Vec<u8>)>, Vec<u8>) {
    // 1. Render through the standard reusable buffer. The context carries the
    //    staged side channels (response, head, telemetry) with it, and
    //    components write to them mid-render via cx.response().
    let mut cx = WebContext::new();
    let staged = cx.response().clone(); // clones alias the same channel
    HomePage.render(&mut cx).expect("render failed");
    let buf = cx.into_output();

    // 2. Copy the staged status and headers into the native response.
    let status = staged.status().map(|s| s.as_u16()).unwrap_or(200);
    let headers = staged
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
        .collect();

    // 3. Ship the body. Scatter-gather servers write `buf.slices()` out
    //    directly (writev-style, no flattening); simple ones flatten once.
    (status, headers, buf.to_vec())
}
```

Route matching comes from `route_spec` / `route_snapshot`
(`extract_route_params` maps a pattern like `/users/:id` onto a request
path), async pages add an executor's `block_on`, and streaming routes replace
step 1 with a `FrameworkStreamFactory` driven according to its nested
capability above.

## Features

- `rt-tokio` (default): forwarded to `proa_stream` so prepared loads spawn
  eagerly on the ambient tokio runtime. Disable it for runtime-agnostic
  builds; this crate's own code never references a runtime either way, and
  CI enforces that the no-default-features dependency tree stays
  tokio-free.
