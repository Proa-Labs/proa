# ESPN Gamecast SSR benchmark

Measured July 10, 2026 on arm64 macOS 26.5.1 with Node 23.2.0 and
Rust 1.95.0.

Both renderers produce the complete page document from static typed fixture
data. The measured page includes the scoreboard, all three tab panels, both
teams' batting and pitching tables, play-by-play, win probability, recap,
sidebar, and inline CSS. File reads, process startup, dependency loading, and
fixture capture/parsing are outside the timed region.

- Warmup: 1,000 renders
- Measurement: 10,000 renders
- Reported values: median of three independent benchmark processes
- React: `NODE_ENV=production`, React 19 `renderToString`
- Proa: workspace `release` profile (`opt-level=3`, thin LTO,
  `codegen-units=1`)

| Renderer | Mean | p50 | p99 | HTML bytes |
|---|---:|---:|---:|---:|
| React `renderToString` | 137.94 µs | 130.33 µs | 262.33 µs | 33,739 |
| Proa fresh output | 5.74 µs | 5.71 µs | 7.17 µs | 33,241 |
| Proa reused `Vec` | 5.66 µs | 5.58 µs | 7.04 µs | 33,241 |

Against the allocation-inclusive Proa render, React was **24.03× slower** by
mean latency and **22.83× slower** at p50. Reusing the response buffer increased
the mean difference to **24.37×**.

Reproduce from the repository root:

```bash
cargo build --release -p proa_showcase --bin espn_bench
BENCH_WARMUP=1000 BENCH_ITERATIONS=10000 \
  target/release/espn_bench

cd proa_demo/react-ssr-shared
pnpm install --frozen-lockfile
NODE_ENV=production BENCH_WARMUP=1000 BENCH_ITERATIONS=10000 \
  pnpm exec tsx bench-espn.tsx
```
