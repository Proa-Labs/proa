# Lean benchmark page (Proa vs Marko vs React)

This directory hosts a deliberately small comparison page: three servers render
the same page with `N` `WatchHeart`-shaped toggles. There are no analytics,
fonts, product images, or third-party scripts. Framework runtime and hydration
code are therefore the only JavaScript shipped by each page.

Use this benchmark to isolate framework overhead. Use the synthetic marketplace
harness in `proa_demo/bench/run.mjs` for a denser, application-shaped workload.
Its historical `ebay-rsjs` route/configuration identifier is retained only for
compatibility.

## Servers

| Port | Engine | Source |
|---:|---|---|
| 5501 | Proa | `proa-server/` |
| 5502 | Marko | `marko-server/` |
| 5503 | React | `react-server/` |

Every server exposes:

- `GET /` for the page. `?n=N` controls the widget count and defaults to 60.
- `GET /health` for a small JSON health response.
- Only the runtime and per-widget JavaScript required by its framework.

## Running

```bash
# Proa, port 5501
cargo build --release -p proa-lean-server
target/release/proa-lean-server

# Marko, port 5502
cd proa_demo/bench/lean-page/marko-server
PORT=5502 node server.mjs

# React, port 5503
cd proa_demo/bench/lean-page/react-server
PORT=5503 node --import tsx server.tsx
```

The Marko and React packages reuse dependencies installed in
`proa_demo/marko-server` and `proa_demo/react-ssr`. Run `npm install` in those
parent directories first when necessary.

## Measure wire size

```bash
cd proa_demo/bench/lean-page
node lean-bundle-size.mjs
```

## Run the browser benchmark

```bash
cd proa_demo/bench
BENCH_RUNS=5 BENCH_CPU=4x BENCH_NETWORK=wifi,4g BENCH_CACHE=cold \
  node lean-page/lean-run.mjs
```

Supported variables:

- `BENCH_RUNS` (default 5)
- `BENCH_CPU`: `unthrottled`, `4x`, or `6x`
- `BENCH_NETWORK`: `wifi`, `4g`, `fast3g`, or `slow3g`
- `BENCH_CACHE`: `cold` or `warm`
- `LEAN_N` (default 60)

Do not copy old result tables into this file as evergreen claims. Record the
revision, environment, commands, and raw result artifacts for every published
comparison so future fixture/runtime changes cannot make the numbers
misleading.

## File layout

```text
lean-page/
├── README.md
├── proa-server/
│   ├── Cargo.toml
│   └── src/main.rs
├── marko-server/
│   ├── lean.marko
│   ├── package.json
│   └── server.mjs
├── react-server/
│   ├── client.tsx
│   ├── page.tsx
│   ├── package.json
│   ├── server.tsx
│   └── tsconfig.json
├── lean-bundle-size.mjs
└── lean-run.mjs
```

The Proa server reuses the existing `WatchHeart` component and filters runtime
assets to that component. Marko and React use their native equivalents. All
three honor the same `?n=N` range so comparisons exercise matching widget
counts.
