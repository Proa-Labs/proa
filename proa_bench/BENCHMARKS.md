# Proa internal performance benchmarks

This crate contains Proa-only regression guards and diagnostic fixtures that
should evolve with the framework implementation. Cross-framework and
cross-language comparisons, parity gates, external adapters, and historical
results live in
[`Proa-Labs/proa-benchmarks`](https://github.com/Proa-Labs/proa-benchmarks).

Keeping that boundary prevents ordinary Proa workspace Clippy and test jobs
from compiling Leptos, Dioxus, Yew, Sailfish, Askama, Topcoat, and the
non-Rust comparison toolchains.

## Durable render gate

`render_memcpy_floor` compares a complete Proa render with a cache-hot memcpy
of the exact same bytes:

```bash
cargo bench --locked -p proa_bench --bench render_memcpy_floor
```

The Criterion identifiers remain stable for the performance-report policy:

- `render_memcpy_floor/proa_html_safetext_stack`
- `render_memcpy_floor/memcpy_page_floor`

Set `PROA_PERF_FIXTURE_OUTPUT` to retain the rendered bytes outside the timed
loop. The benchmark asserts byte identity before measuring either row.

## Component façade guard

`component_facade` compares `#[derive(ProaComponent)]` with an equivalent
authored `WebRenderSync` component:

```bash
cargo bench --locked -p proa_bench --bench component_facade
```

All rows render the same concrete child and byte-identical HTML into a reused
stack `WriteCursor`. Its preflight rejects output drift, render-path heap
allocation, loss of static dispatch, or component layout drift. Treat those
contracts—not a workstation-local point estimate—as the durable evidence.

## Streaming and buffer regressions

The remaining Criterion targets isolate implementation behavior within Proa:

```bash
cargo bench --locked -p proa_bench --bench streaming_buffer_comparison
cargo bench --locked -p proa_bench --bench streaming_pure_render
cargo bench --locked -p proa_bench --bench suspense_boundary_scaling
cargo bench --locked -p proa_bench --bench in_order_streaming_pages
cargo bench --locked -p proa_bench --bench context_parts_page
```

Other focused targets cover closure overhead, sync-path coverage, authored
component structure, legacy button paths, and complex control flow. Run a
target on a quiet host and retain the complete Criterion output whenever a
performance claim depends on it.

For an HTTP-body chunk timeline rather than a Criterion microbenchmark:

```bash
cargo run --locked --release -p proa_bench --bin stream_trace -- --iterations 25
```

Generated trace output stays under the ignored `proa_bench/results/`
directory.

## Suspense adapter architecture benchmark

`suspense_adapter_bench` measures the real Axum adapter from router dispatch
through complete body collection. It records response-ready, first-byte, and
completion latency for buffered rendering and 0, 1, 8, 32, and 128 immediate
out-of-order boundaries at multiple runtime worker and concurrency counts:

```bash
cargo run --locked --release -p proa_bench --bin suspense_adapter_bench -- \
  --iterations 10000 \
  --warmup 250 \
  --repeats 3 \
  --workers 1,4 \
  --concurrency 1,32,128
```

Ordinary runs write ignored JSON under `proa_bench/results/`. Architecture
changes may retain a named, commit-specific result under
`proa_bench/baselines/` so the before/after comparison remains reviewable.
