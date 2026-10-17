# Sentry OpenTelemetry Logs SSR benchmark

Measured August 27, 2026 on an 8-vCPU AMD EPYC-Milan VPS with Node in
production mode and the workspace Rust release profile (`opt-level=3`, thin
LTO, `codegen-units=1`). Each renderer was pinned to one vCPU.

Both renderers produce a complete 400-row document. Each row includes the
collapsed log summary and its structured two-column attribute tree. CSS
serialization is timed; file reads, dependency loading, process startup, and
client hydration are not.

The default showcase renders 400 server rows and progressively reveals them in
40-row batches as the log viewport scrolls.

- Warmup: 100 renders
- Measurement: 300 renders
- Reported values: median of five independent benchmark processes
- React: `NODE_ENV=production`, React 19 `renderToString`
- Proa: a fresh owned `RenderBuffer` converted to `String`
- Comparison document: hydration-independent SSR application DOM; each
  framework's client hydration transport is outside the timed output

| Renderer | Mean | p50 | p99 | HTML bytes |
|---|---:|---:|---:|---:|
| React `renderToString` | 26,405.54 µs | 25,360.62 µs | 54,314.93 µs | 875,774 |
| Proa fresh-owned document | 157.48 µs | 155.13 µs | 175.53 µs | 874,540 |

The parity-controlled p50 ratio is **163.48x** (displayed as **163x** on the
landing page).

The previous version of this benchmark timed Proa into a reused `Vec` while
React returned a fresh owned string. That mixed ownership models and understated
the Proa number. The benchmark now makes both timed paths return fresh owned
documents and explicitly primes the Rust allocator to model a warm,
long-lived server process.

## Full-document parity

The benchmark is guarded by the same Chromium full-document parity checker as
the Airbnb and Nike comparisons. It requires matching document mode, doctype,
element structure, attributes, normalized text nodes, and rendered text. The
400-row Proa and React documents both normalize to SHA-256
`67be317135ed94443841fe0791cf9fb7653c473ad6d68b46e1b02eb9cb162d95`.
Raw byte counts differ because of serializer-only artifacts such as React text
boundary comments and equivalent entity spellings.

Reproduce from the repository root:

```bash
cd proa_demo/react-ssr-shared
pnpm install --frozen-lockfile --ignore-scripts
pnpm exec playwright install chromium
pnpm run parity:sentry-otel

cd ../..
BENCH_ROWS=400 BENCH_WARMUP=100 BENCH_ITERATIONS=300 \
  cargo run --release --locked -p proa_showcase --no-default-features \
  --features sentry-otel \
  --bin sentry_otel_bench

cd proa_demo/react-ssr-shared
NODE_ENV=production BENCH_ROWS=400 BENCH_WARMUP=100 BENCH_ITERATIONS=300 \
  pnpm exec tsx bench-sentry-otel.tsx
```
