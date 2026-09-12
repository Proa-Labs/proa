# proa-bench

Browser-driven timing harness for the proa-rsjs vs Marko 6 vs React 19
vs Next.js SSR comparison. Sibling to `proa_demo/bundle_size.mjs` (which
measures bytes-on-the-wire). This harness measures what actually happens
when a browser loads the page: TTFB, FCP, LCP, a TTI proxy, script CPU
cost, and total first-party transfer.

## Prerequisites

All companion servers for the selected page must be running before you run the bench.
The harness preflights each URL and aborts with a hint if any are
unreachable.

1. **proa playground** (defaults to `:5000`, falls through to 5001+):

    ```sh
    cargo run --release -p proa_demo --bin proa-playground --features server
    ```

2. **Marko 6 SSR** (default `:5300`): auto-spawned by the proa
   playground. If it isn't up, check the playground's stdout for boot
   errors from the marko-server child process.

3. **React 19 SSR** (default `:5401`): start manually:

    ```sh
    PORT=5401 node --import tsx \
        ../react-ssr/server.tsx
    ```

4. **Next.js SSR** (default `:5402`): build once, then start the
   production server:

    ```sh
    cd ../next-ssr
    npm install
    npm run build
    PORT=5402 npm run start
    ```

## Install + run

```sh
cd proa_demo/bench
npm install                       # pulls playwright + chromium
npx playwright install chromium   # only needed once per machine
node run.mjs
```

## Configuration (env vars)

| Var           | Default            | Notes                                                                |
| ------------- | ------------------ | -------------------------------------------------------------------- |
| `PROA_PORT`   | `5000`             | proa playground port                                                 |
| `MARKO_PORT`  | `5300`             | marko-server port                                                    |
| `HONO_PORT`   | `5100`             | Hono React companion port for Nike pages                             |
| `REACT_PORT`  | `5401`             | react-ssr companion port                                             |
| `NEXT_PORT`   | `5402`             | Next.js production server port                                       |
| `BENCH_RUNS`  | `5`                | runs per (engine × cpu × cache) cell                                 |
| `BENCH_CPU`   | `unthrottled,4x,6x`| comma list; subset of `{unthrottled, 4x, 6x}` (CDP throttling rates) |
| `BENCH_CACHE` | `cold,warm`        | comma list; subset of `{cold, warm}`                                 |
| `BENCH_PAGE`  | `ebay-rsjs`        | one of `{ebay-rsjs, nike-catalog, nike-pdp}`                         |
| `BENCH_ENGINES` | all for page     | comma list of engine names for the selected page                     |

Example: a quick smoke run that only exercises the cold path under 4x
throttling:

```sh
BENCH_RUNS=3 BENCH_CPU=4x BENCH_CACHE=cold node run.mjs
```

Run the Nike catalog page against Proa, Hono React, and Next:

```sh
BENCH_PAGE=nike-catalog BENCH_RUNS=3 BENCH_CPU=4x BENCH_CACHE=cold node run.mjs
```

Run only Proa vs Next on the eBay page:

```sh
BENCH_ENGINES='proa-rsjs,next 16' BENCH_RUNS=5 BENCH_CPU=4x BENCH_CACHE=cold node run.mjs
```

## What gets measured

Per run, captured from inside the page via `PerformanceObserver`:

| Metric          | Source                                                                                   |
| --------------- | ---------------------------------------------------------------------------------------- |
| **TTFB**        | `PerformanceNavigationTiming.responseStart`                                              |
| **FCP**         | `PerformancePaintTiming` entry `first-contentful-paint`                                  |
| **LCP**         | latest entry from observer `type: "largest-contentful-paint"` (settles for 3s post-load) |
| **TTI**         | heuristic: earliest moment after FCP with 2s of long-task quietness                      |
| **scriptCPU**   | sum over first-party script resources of `(duration − network slice)`                    |
| **transferred** | navigation `transferSize` + first-party resource `transferSize` summed                   |

Each measurement is repeated `BENCH_RUNS` times in fresh
`BrowserContext`s; results are aggregated as median + p95.

## CPU + cache profiles

CPU throttling is applied via CDP `Emulation.setCPUThrottlingRate`
*before* the navigation, so SSR HTML parsing also pays the cost
(this is what real low-end phones experience):

- `unthrottled` (rate=1): control
- `4x` (rate=4): ~midrange phone
- `6x` (rate=6): ~low-end phone

Cache:

- `cold`: `Network.clearBrowserCache` + `clearBrowserCookies` +
  `setCacheDisabled(true)` before the run
- `warm`: load once, sleep 500ms, then load again with
  `setCacheDisabled(false)` for the measured load

## Output

A section table per (CPU, cache) cell:

```
═══ CPU=4x  cache=cold  (n=5 runs/engine) ═══

engine        TTFB med    FCP med    LCP med    TTI med    scriptCPU   bytes     LCP p95
─────────── ────────── ────────── ────────── ────────── ─────────── ──────── ──────────
proa-rsjs       42ms     180ms      210ms      215ms       15.2ms     45.1KB    230ms
marko 6         50ms     220ms      260ms      280ms       45.0ms     78.2KB    295ms
react 19        55ms     310ms      420ms      650ms      180.4ms    142.0KB    480ms
next 16         80ms     390ms      520ms      780ms      240.0ms    210.0KB    610ms
```

Followed by a winners-by-FCP callout and (if any) a list of run
errors.

## Limitations

- **LCP volatility.** LCP can jump if a hero image's request order
  changes between cold and warm loads. The 3s settle window catches
  most candidates, but the p95 column will reflect any tail entries.
- **TTI is a proxy.** No browser exposes a canonical TTI. The
  `longtask` quietness heuristic (2s window) gives a stable signal
  for hydration-heavy pages but undercounts work hidden in
  microtasks.
- **scriptCPU is a proxy.** The Performance spec doesn't separate
  parse + eval from network time. We compute
  `resource.duration − (responseEnd − startTime)` which approximates
  parse + eval, but cached resources will report near-zero here even
  though they still cost CPU to evaluate.
- **Single-machine variance.** All three engines run on localhost; if
  another process pegs the CPU mid-run, numbers drift. Re-run if any
  engine looks 2× off its expected value.
- **Headless chromium only.** Numbers are not directly comparable to
  Lighthouse Mobile or WebPageTest, which add network throttling on
  top. This harness keeps network at LAN speed and isolates the CPU
  variable.

## See also

- `../bundle_size.mjs` — bytes-on-the-wire (raw, gzip-9, brotli-11)
  measurement. Run it alongside this harness for a complete picture.
