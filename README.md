<div align="center">

<img src="assets/logo.svg" width="96" height="96" alt="Proa" />

# Proa

Proa is building the next generation of interface infrastructure for the web.
It enables you to create extremely fast, scalable applications and services in
Rust by bringing compiled components, streaming, data
loading, and fine-grained interactive islands into one framework.

[![Axum adapter](https://img.shields.io/badge/proa__framework__axum-0.3.0-DD7627?style=flat-square&logo=rust&logoColor=white)](proa_framework/proa_framework_axum/README.md)
[![Core](https://img.shields.io/badge/proa__core-0.3.0-DD7627?style=flat-square&logo=rust&logoColor=white)](proa_core)
[![Macros](https://img.shields.io/badge/proa__macros-0.3.0-DD7627?style=flat-square&logo=rust&logoColor=white)](proa_macros/README.md)
[![RSJS](https://img.shields.io/badge/rsjs-0.2.0-DD7627?style=flat-square&logo=rust&logoColor=white)](rsjs/README.md)
[![License](https://img.shields.io/badge/license-Apache--2.0-DD7627?style=flat-square)](LICENSE)
[![X](https://img.shields.io/badge/X-DD7627?style=flat-square&logo=x&logoColor=white)](https://x.com/drewhariri)
[![Reddit](https://img.shields.io/badge/Reddit-DD7627?style=flat-square&logo=reddit&logoColor=white)](https://www.reddit.com/r/proa_rs/)

[**Get started**](https://proa.so/docs) ·
[**Explore apps**](https://proa.so/ecosystem/apps#reference-showcases) ·
[See the benchmarks](https://github.com/Proa-Labs/proa-benchmarks)

</div>

---

## Next-generation interface infrastructure

The web's interface layer was designed around a person opening one page at a
time in a browser. The next web must also serve crawlers and software agents
that read and act across hundreds of interfaces in parallel, at massive scale.

Proa's larger vision is one typed, compiled interface layer that produces the
right representation for whoever asks: complete HTML for people, targeted
behavior for browsers, and dense machine-readable output for agents.

This repository is the foundation of that vision: a high-performance server
renderer, a typed component model, fine-grained browser interactivity,
streaming, data loading, and the framework machinery needed to run them in
production.

## Start in five minutes

On macOS or Linux:

```bash
curl -fsSL https://proa.so/install.sh | sh
```

On Windows, run this in PowerShell:

```powershell
irm https://proa.so/install.ps1 | iex
```

Open a new terminal, then create a site:

```bash
proa new site hello-proa --template marketing --docs proa_docs --tailwind --yes
cd hello-proa
proa dev
```

Follow the [full quickstart](https://proa.so/docs) to add a
route, inspect the generated project, and run Proa's source-aware checks.


## Your first page is a typed Rust component

Use `WebRenderSync` and `html_sync!` for the normal fast path:

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

struct Greeting {
    name: String,
}

impl<L: DataLoader> WebRenderSync<L> for Greeting {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <main class="mx-auto max-w-3xl px-6 py-16">
                <h1 class="text-4xl font-semibold">"Hello, "{self.name}</h1>
                <p class="mt-4 text-zinc-600">"Rendered by Proa."</p>
            </main>
        }
        .render(cx)
    }
}
```

The macro validates the template at compile time, consolidates adjacent static
markup, and preserves the correct escaping behavior for dynamic slots.

When a component genuinely needs to await work, switch to `WebRender` and
`html!`. Markdown follows the same split with `MdRenderSync` / `md_sync!` and
`MdRender` / `md!`.

## Add interactivity where it belongs

An RSJS island is still a normal server component. Add signals and event
handlers, and Proa emits the server-rendered HTML plus the JavaScript required
for that island:

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

struct Counter {
    initial: i32,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(self.initial);

        html_sync! {
            <button
                data-counter
                type="button"
                on_click={|_| count.update(|value| value + 1)}
            >
                "Count: "{count.get()}
            </button>
        }
        .render(cx)
    }
}
```

The browser starts from the HTML the server already rendered. It does not
re-run the server component tree to make the page interactive. See the
[RSJS guide](rsjs/README.md) for composition, queries, shared state, forms, and
current limitations.

## Technical Approach

We treat web rendering as a serialization problem, not a component-tree
reconstruction problem. A server should turn application state into useful HTML
as directly as possible: **emit as much HTML and as little JavaScript as
possible, as fast as possible.**

JavaScript is expensive. The browser must download it, parse it, compile it, and
execute it—often on the main thread. That is expensive on desktop and worse on
mobile CPUs; it can take [6-10x longer](https://github.com/GoogleChrome/lighthouse/blob/main/docs/throttling.md#benchmarking-cpu-power) to execute on lower tier android devices.
It also consumes memory and battery. Proa treats every byte of
client code as work that must justify itself. For more, see [Addy Osmani's "The Cost of JavaScript"](https://www.youtube.com/watch?v=X9eRLElSW1c)

### Performance by construction

We can reason about rendering performance from first principles.

The theoretical minimum is a fully static page: no branching, no formatting, no allocation, no component traversal. The page already exists as bytes!

That gives us a useful design target: a fast dynamic renderer should approach the cost of serving a static byte buffer.

Proa is designed around that principle. The compiler tries to move as much work as possible out of the render path:

1. Make static work actually static.
`html! {}` and the Proa compiler identify HTML that can be determined ahead of time and lower it to static byte sequences. The remaining dynamic code is compiled to native code, where LLVM can inline calls, eliminate dead work, unroll loops, and optimize across component boundaries.

2. Write directly to the output.
Proa avoids intermediate render trees, temporary strings, unnecessary buffer allocations, and extra copies. Dynamic values are written into the final output stream as directly as possible.

The result is a renderer whose cost is increasingly proportional to the amount of the page that is genuinely dynamic—not to the size or complexity of the component tree.


Synchronous rendering remains the default, monomorphized fast path. Async work,
streaming, heap allocation, and custom writers are explicit choices for the
parts of an application that need them.

The same compiler-first principle reaches beyond the render function. Proa's
documentation pipeline can resolve routes, validate links, build search data,
highlight code, and turn math into MathML ahead of time. Its font pipeline
self-hosts and content-hashes font files and generates metric-matched fallbacks.
If work can be proven once during the build, it should not be repeated on every
request or in every browser.

### Ship only what the route uses

During server rendering, Proa records the hydrated islands that the route
actually reached and derives the response's asset plan from that graph.
Server-only components emit no island JavaScript. Interactive routes load only
their rendered island modules, add the query and mutation runtime only when it
is used, and preload only reachable client dependencies. Islands can hydrate
immediately, when visible, while idle, on first interaction, or when a media
query matches. The JavaScript shipped by a page scales with that page's
behavior—not with the size of the application.

### Load data without hidden waterfalls

Components request data by stable keys through a request-scoped resolver, so
repeated reads share one in-flight load instead of duplicating database or API
work. Known loads can start before rendering, while independent `Suspense`
boundaries resolve concurrently and stream behind useful fallbacks with
explicit priority and timeout behavior. Components that do not await data stay
on the synchronous path.

### One interface, more than one representation

A Proa component is a typed Rust value, not a browser-only description of a
DOM tree. Matched HTML and Markdown render traits let the same interface model
produce a complete page for a person and a dense text representation for a
crawler or agent. The docs system already emits per-page Markdown and an
`/llms-full.txt` artifact alongside its HTML output.

### Keep the engine portable and the server production-ready

Proa separates rendering and route contracts from sockets, threads, clocks,
and a specific async runtime. The shipped Axum adapter adds the operational
pieces at the boundary: buffered, async, and streaming responses;
strict per-request nonce CSP; honest readiness probes; and bounded graceful
draining that cancels unfinished streams. A different server adapter can reuse
the same route, manifest, response, and streaming contracts.

## Benchmarks

For more benchmarks, see
[Proa-Labs/proa-benchmarks](https://github.com/Proa-Labs/proa-benchmarks).

[![Sustained responses per second, CPU per response, and peak memory for Proa, React, Marko, and Go html/template serving three pages over HTTP on one core](assets/benchmarks/21-canonical-pages-http.svg)](assets/benchmarks/21-canonical-pages-http.svg)

[![Seventeen renderers across three workloads on a logarithmic axis of render time relative to Proa](assets/benchmarks/01-canonical-scoreboard.svg)](assets/benchmarks/01-canonical-scoreboard.svg)


[![Responses completed per second, latency distribution, and peak memory for Proa, Marko, and React streaming the product grid through Suspense over HTTP on one core](assets/benchmarks/20-suspense-load.svg)](assets/benchmarks/20-suspense-load.svg)

**How fast does a fresh process answer?** Spawn to first complete response, the
cost of every autoscaling event and cold instance: Proa answered in 8.63 ms
holding 4.5 MiB, against 69.0 ms and 75.9 MiB for React, 57.0 ms and 63.0 MiB
for Marko, and 15.2 ms and 12.5 MiB for Go.

[![Median, P95, and slowest time from process spawn to first complete product-grid response, with memory after that response](assets/benchmarks/22-cold-start.svg)](assets/benchmarks/22-cold-start.svg)

The parity gate, the one-core rule, the load generator's self-checks, what is
deliberately left outside each measurement, every raw JSON capture, and the
commands to reproduce all of it on your own hardware are documented in
[Proa-Labs/proa-benchmarks](https://github.com/Proa-Labs/proa-benchmarks).

## Proa in production

- [latency.so](https://latency.so): a server-side rendering speed
  leaderboard, server-rendered with Proa.

## Explore the project

| If you want to… | Start here |
|---|---|
| Build your first Proa site | [Getting started](https://proa.so/docs) |
| Understand the benchmark claims | [Benchmarks](#benchmarks) and the [full evidence repository](https://github.com/Proa-Labs/proa-benchmarks) |
| Author server-rendered components | [`proa_core`](proa_core) and [`proa_macros`](proa_macros/README.md) |
| Build interactive islands | [RSJS](rsjs/README.md) |
| Load data and stream independent boundaries | [`proa_stream`](proa_stream) and the [data-loading guide](https://proa.so/docs/framework/data-loading) |
| Integrate routing, security, and Axum | [`proa_framework_axum`](proa_framework/proa_framework_axum/README.md) |
| Adapt Proa to another HTTP server or runtime | [`proa_framework_core`](proa_framework/proa_framework_core/README.md) |
| Compile documentation and self-hosted fonts | [`proa_docs`](proa_docs) and [`proa_fonts`](proa_fonts) |
| Embed rendering in an existing service | [FFI guide](https://proa.so/docs/ffi) |
| Contribute to Proa | [Contributing guide](CONTRIBUTING.md) |
| Give a coding agent project context | [Agent authoring guide](docs/agent/index.md) |

## Repository development

Prerequisites:

- Rust 1.93.0 or newer
- Python 3 for repository lint scripts
- Node.js 22 or newer for browser conformance tests and JavaScript comparison
  engines

Run the workspace checks from the repository root:

```bash
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast
python3 scripts/proa_perf_lint.py
```

Run the marketing site and documentation locally with:

```bash
cargo run -p proa_marketing
```

This repository contains the compiled-template engine and its first-party
framework, tooling, examples, and RSJS compiler. Runtime templates and the
`proa_ui` component library are downstream projects with independent release
processes.

Security issues should be reported privately according to
[`SECURITY.md`](SECURITY.md). Generated and captured assets are documented in
[`GENERATED.md`](GENERATED.md).

## License

Proa is licensed under the Apache License, Version 2.0. See
[`LICENSE`](LICENSE), copyright 2026 Proa Labs, Inc.

Commercial use, including inside closed-source products, is permitted with no
separate agreement. See [`LICENSING.md`](LICENSING.md) for details or contact
`legal@proa.so`. Third-party attributions are collected in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
