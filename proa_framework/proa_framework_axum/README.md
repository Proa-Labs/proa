# proa_framework_axum

Axum/Tokio adapter that wires Proa's rendering core (`proa_core`,
`proa_macros`) to an HTTP server. It handles routing, island hydration, static
assets, and metadata layouts behind a single Axum router.

This crate is the glue between a page component and an HTTP response. If you
are writing a component, start with `proa_core` and `proa_macros`. If you are
wiring a component into an Axum server, you want this crate. If you are
adapting Proa to a different server framework or async runtime, use
[`proa_framework_core`](../proa_framework_core/README.md) instead.

## How it relates to proa_framework_core

```
proa_core / proa_stream         render engine: bytes into caller-owned buffers
            |
proa_framework_core             contracts: routes, metadata, manifests, response staging
            |
proa_framework_axum  <- you are here  axum Router, stream executors, static assets, hardening
```

The split is deliberate: `proa_framework_core` defines what a route,
manifest, and response *are*, with no dependency on axum or (with default
features off) any async runtime. This crate is the tokio + axum
implementation of those contracts, and the layer that owns everything
IO-shaped: sockets, threads, clocks, channels. The clearest example is
streaming: Proa permits render futures to be `!Send`, so the response contract
preserves `FrameworkStreamFactory::Send | Local`. Send streams go straight to
Axum. Local streams are admitted to a small persistent pinned-worker pool with
bounded queued/active work and byte-bounded per-response transport. Both paths
prepare their first item asynchronously before the service returns the response
head. A different adapter can replace exactly that execution layer and reuse
everything in `proa_framework_core` unchanged. See the
[`Send` and runtime-portability decision](../proa_framework_core/README.md#send-policy-and-runtime-portability).
The rule that keeps the boundary honest: if a change needs a timer, a spawn,
or a socket, it belongs here, never in the engine crates, and CI enforces this
by failing if tokio appears in the engine crates' no-default-features
dependency trees.

## Modules

| Module | Purpose |
|--------|---------|
| `route` | `Route`, `RouteResponse`, layouts, metadata cascade, route context layer |
| `static_service` | Serves embedded static assets (`EmbeddedStaticService`) |
| `readiness` | Generic, timeout-bounded dependency probes for `/readyz` |
| `server` | Signal-aware Axum accept loop with bounded graceful draining |

## Route responses

`RouteResponse` is an opaque deferred-render plan. Its constructors expose
five useful capabilities:

| Constructor | Use |
|-------------|-----|
| `ssr(component)` | Render a `WebRenderSync` component into a buffered HTML response. |
| `ssr_async(component)` | Drive a `WebRender` component to completion, then emit buffered HTML. |
| `ssr_stream(component)` | Stream an async component, including `Suspense` boundaries. |
| `ssr_async_send(component)` | Buffer a compiler-proved `WebRenderSend` graph. |
| `ssr_stream_send(component)` | Stream a compiler-proved `WebRenderSend` graph directly on Axum/Tokio. |

Delivery mode and capability remain outside the public representation. All SSR
paths ship baseline security headers by default (`X-Content-Type-Options:
nosn`, `Content-Type: text/html; charset=utf-8`).

Return Axum's `Html`, redirects, JSON, or other ordinary responses directly
when no Proa render is needed. Mount client-rendered SPAs through Axum router
composition.

For async pages, use `RouteResponse::ssr_async(component)`. For streamed async
pages with explicit fallback boundaries, use
`RouteResponse::ssr_stream(component)` plus `proa_stream::Suspense`. These
compatible paths allow local futures and use the bounded pinned-worker pool for
streaming. If the full tree implements `WebRenderSend`, pair `html_send!` with
`ssr_async_send(...)` or `ssr_stream_send(...)`; the latter bypasses the local
pool. Sync components can opt into the local async path with
`proa_core::to_async(...)`, but `RouteResponse::ssr(...)` remains the preferred
path for normal sync pages.

Untimed Suspense children are concurrent futures owned by the response driver,
not one Tokio task per boundary. Timed children are supervised in tasks so
their deadlines still fire while the body is backpressured. Same-turn
completions share one transport batch, and nested children are admitted only
after their parent's replacement reaches the wire.

## Defining a page

A route is an async handler returning a `RouteResponse`; pass the
component itself so the framework renders it in one pass. Rendering to a
`String` first and then returning it as HTML adds an avoidable copy. The normal
`ssr` path owns the standard reusable `RenderBuffer`,
so application pages customize only their loader:

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_framework_axum::RouteResponse;
use proa_macros::html_sync;

struct HomePage;

impl<L: DataLoader> WebRenderSync<L> for HomePage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <main class="container mx-auto p-8">
                <h1 class="text-4xl font-bold">"Welcome to Proa"</h1>
            </main>
        }
        .render(cx)
    }
}

pub async fn handler() -> RouteResponse {
    RouteResponse::ssr(HomePage)
}
```

Wire it into a route table with a layout and a render mode, then hand the
result to the builder below:

```rust
use axum::routing::get;
use proa_framework_axum::{router_from_spec, Route, RouteMode};

let (router, computed_metadata) = router_from_spec(vec![Route::layout(
    "",
    &AppLayout,
    vec![Route::page("/", "Home", get(handler)).with_route_mode(RouteMode::Server)],
)]);
```

## Composing a server

```rust
use std::time::Duration;
use proa_framework_axum::FrameworkBuilder;

let app = FrameworkBuilder::new()
    .with_dynamic_routes(my_router())
    .with_static_service(proa_framework_axum::standalone_static_router("dist"))
    .build();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
proa_framework_axum::serve(listener, app, Duration::from_secs(30)).await?;
```

`FrameworkBuilder::build()` returns an `axum::Router`, so anything downstream of it (middleware, nesting under another router, tracing, CORS) works via the standard Axum / tower ecosystem.

### Rendering without a server

`RouteResponse::render_to_bytes` finalizes one response into a complete HTML
document with no listener, no island manifest, and no async runtime. It is
available on this crate's `RouteResponse` and on `proa_framework_core`'s, so a
static renderer can skip Axum and tokio entirely. Static exports and snapshot
tests get the same managed head and RSJS runtime tags a served route would.
Async plans render buffered, a non-2xx status is an error, and no route context
is installed, so pages that read route metadata, route params, or the base
path should go through the built router with `oneshot` instead:

```rust,ignore
let html = RouteResponse::ssr(RootDocument { children: HomePage })
    .with_head(HeadConfig::default())
    .render_to_bytes()?;
std::fs::write("output/index.html", html)?;
```

`proa_framework_axum::serve` waits for Ctrl-C or SIGTERM, immediately stops
accepting new connections, and starts the supplied drain deadline at that
moment. Accepted connections may finish normally during the drain window. At
the deadline, the helper aborts the remaining connection tasks; dropping a
Proa streaming body also cancels its pending render stream and stalled
`Suspense` boundaries. The default used by generated sites is
`DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT` (30 seconds), and applications can pass a
different duration directly.

These lifecycle guarantees belong to this serve helper. An application that
calls `axum::serve` itself owns its signal handling, deadline, task tracking,
and streamed-body cancellation policy.

### Sitemaps and robots.txt

Configure discovery endpoints on the same builder so they share its public origin
and mount prefix:

```rust
use proa_framework_axum::{BasePath, FrameworkBuilder, RobotsTxt, SitemapUrl};

let app = FrameworkBuilder::new()
    .with_base_path(BasePath::new("/apps/demo")?)
    .with_public_origin("https://example.com")?
    .with_sitemap([SitemapUrl::new("/")?, SitemapUrl::new("/pricing")?])
    .with_robots(RobotsTxt::new().disallow("/admin")?)
    .try_build()?;
```

The sitemap contains `https://example.com/apps/demo/pricing`; robots.txt contains
`Disallow: /apps/demo/admin` and advertises the prefixed sitemap automatically.
Both endpoints are opt-in and rendered once at startup using the final builder
configuration. `try_build()` returns discovery configuration errors; `build()`
panics on them. Custom/dynamic handlers can still use the standalone document and
response helpers.

Absolute sitemap and index entries must resolve inside the final base path
directory. `try_build()` returns `DiscoveryError::OutsideBasePath` for an
out-of-mount URL, including one that escapes through literal or encoded `..`.
Local paths cannot traverse above the application root. Discovery URLs reject
fragments, controls, and raw excluded URI characters; percent-encode URL data
before passing it to the API. Robots sitemap links may still use other origins.

Incoming routes remain `/sitemap.xml` and `/robots.txt`. A prefix-stripping proxy
must expose `/robots.txt` at the origin root in addition to forwarding mounted app
paths. Do not register overlapping custom discovery handlers on this builder.

See the [complete application example](../../proa_marketing/src/docs/framework/sitemaps.mdx#run-an-application)
for application route setup, environment configuration, listener binding, and `serve()`.
Sitemap entries are explicit; registering a page route does not add it automatically.

Use `.with_sitemaps(files)` for multiple documents. Each
`SitemapFile::new("products-0", urls)?` is served at `/sitemap-products-0.xml`;
`/sitemap.xml` becomes an index linking to the documents, and robots.txt
advertises that index. Supply an awaited application generator with
`.with_sitemaps(generate_sitemaps(&db).await?)`. Generation and rendering happen
once at startup; requests serve cached bytes. The last `with_sitemap` or
`with_sitemaps` call replaces the previous configuration.

IDs must be unique, nonempty ASCII letters/digits/`-`/`_`; an empty file list is
an error. Each sitemap and index is limited to 50,000 entries and 50 MiB of
uncompressed XML. Use smaller batches for long URLs. For application-managed
refresh or dynamic responses, use `SitemapIndex` and `sitemap_index_xml` alongside
the existing standalone sitemap helpers.

[Robots.txt](../../proa_marketing/src/docs/framework/robots.mdx) uses the same
application-owned async generation pattern:
`.with_robots(generate_robots(&db).await?)`. Generate a `RobotsTxt` at startup;
requests serve its cached text, with the final builder mount applied once and
an automatic sitemap/index link. There is one robots.txt per origin.

`RobotsTxt::group_agents(["Applebot", "Bingbot"])?` shares rules across bots.
Per-group options include `.crawl_delay(Duration::from_millis(1500))` for exact
seconds and `.directive("Request-Rate", "10/1m")?` for crawler extensions.
`.host("https://example.com")?` sets an optional explicit host; repeated
`.sitemap(...)` calls add sitemap links. Extension support depends on the bot;
Google ignores crawl delay. Application-wide rules cover the bare mount and
its query variants; literal `*`/`$` in a mount are escaped independently of rule
operators. Custom directives reject comments, control
characters, malformed names, and reserved fields, while permitting spaces in
values. Only allow/disallow rules receive the base path.

User-agent tokens follow [RFC 9309 §2.2.1](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.1).
The builder and response helper reject robots documents above
[Google's 500 KiB limit](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#file-format).
Other adapters can use `RobotsTxt::try_render_txt()` for the same check.

### Liveness and readiness

Liveness and readiness should not share dependency policy:

- `/healthz` answers whether the process is alive. Keep it cheap and independent
  of databases, caches, and APIs; a liveness failure usually tells an
  orchestrator to restart the process.
- `/readyz` answers whether this process should receive application traffic.
  Check only dependencies required to serve that traffic; a readiness failure
  should remove the instance from service without forcing a restart.

The generated site mounts the cheap `/healthz` liveness endpoint. It
intentionally does not mount an always-ready `/readyz`: that would claim
dependency readiness without checking a dependency. Add an honest application
probe by merging a `Readiness` router:

```rust,ignore
use std::time::Duration;
use proa_framework_axum::Readiness;

let database_readiness = Readiness::new("database", {
    let pool = pool.clone();
    move || {
        let pool = pool.clone();
        async move {
            // Replace this with the cheapest operation that proves the
            // application's required database path can accept work.
            pool.acquire().await.map(drop)
        }
    }
})
.with_timeout(Duration::from_secs(1));

let app = app.merge(database_readiness.into_router());
```

`into_router()` mounts GET and HEAD `/readyz`; `into_route()` returns a
`MethodRouter` for a custom path or nested router. Compose several required
dependencies inside one async probe using the application's desired
short-circuiting or concurrency policy.

Every request runs a fresh probe, so failures are not sticky and simultaneous
requests do not serialize behind shared framework state. Success is exactly
`200` with `ready\n`; probe failure or timeout is exactly `503` with
`not ready\n`. Both outcomes are `text/plain; charset=utf-8` and carry
`Cache-Control: no-store`. Internal errors and timeout durations are written to
structured logs, never to the public body. The default deadline is
`DEFAULT_READINESS_TIMEOUT` (2 seconds).

The framework does not spawn probe tasks. If the handler request future is
dropped, its timeout and probe futures are dropped too. Dependency clients
should therefore use cancellation-safe async operations.

### Strict script Content Security Policy

Turn it on with one builder call:

```rust,ignore
let app = FrameworkBuilder::new()
    .with_strict_csp()
    .build();
```

Each request gets a fresh 128-bit nonce, installed as a
`proa_framework_axum::CspNonce` request extension before Proa's route context runs,
and the response carries the matching policy:

```text
default-src 'self'; script-src 'nonce-…' 'strict-dynamic';
style-src 'self' 'unsafe-inline'; object-src 'none';
base-uri 'self'; frame-ancestors 'none'
```

Proa applies that nonce to rsjs state, import maps, module bootstraps, React
props, streaming runtime tags, and every out-of-order boundary swap. Raw
`<script>` tags you write by hand still need their own `nonce` attribute, read
from `cx.csp_nonce()`.

`CspPolicy` adjusts the non-script directives and supports a report-only
rollout:

```rust,ignore
use proa_framework_axum::CspPolicy;

let app = FrameworkBuilder::new()
    .with_csp_policy(
        CspPolicy::strict()
            .with_directive("img-src", "'self' data: https://cdn.example")
            .report_only(true),
    )
    .build();
```

`script-src` is deliberately not settable through `with_directive`, since
replacing it would drop the nonce; use `allow_script_source` to append extra
sources. The layer leaves an existing `Content-Security-Policy` header alone,
so a single handler can set its own policy.

In CSP Level 3 browsers, `'strict-dynamic'` makes the nonce, not a broad
same-origin or CDN allowlist, the root of script trust, including dependencies
loaded by a trusted module. Prefer self-hosting production dependencies even
though the default React runtime currently resolves through `esm.sh`. Adding
`'self'` or `https://esm.sh` as compatibility fallbacks permits any matching
script in browsers that do not enforce `'strict-dynamic'`, so treat that as a
deliberate security tradeoff rather than the strict policy.

Do not reuse a nonce across responses or substitute a request ID for
cryptographic randomness. The example keeps inline styles enabled because Proa
applications may intentionally render style attributes; remove that exception
when the application uses classes and external stylesheets exclusively. Script
execution does not require `'unsafe-inline'`.

## Security posture

The framework applies these baseline controls:

- **CRLF / header injection** — all response headers use Axum's typed `HeaderValue`; no user-controlled strings flow into header sinks.
- **SSRF** — no endpoint fetches URLs supplied via query/header.
- **Middleware-trust bypass** — no custom internal header (`X-Proa-*`, `X-Forwarded-*`) is trusted for authorization.
- **Default security headers** — `X-Content-Type-Options: nosniff` and explicit UTF-8 charset are injected on every SSR response (`route/response.rs`, `zero_copy_body.rs`).

## Testing

```
cargo test -p proa_framework_axum --lib
```

Lib tests cover the layout/metadata composition path. Framework integration tests live in this crate's `tests/` directory.
