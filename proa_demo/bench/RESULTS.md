# Proa SSR benchmark results

This harness compares Proa, Marko 6, React 19 SSR, and Next.js on two
workloads:

- a lean page that isolates framework and hydration overhead;
- a content-dense eBay search-results page with a refinement rail,
  nested product cards, ratings, optional badges, prices, seller metadata,
  pagination, and a multi-column footer.

The eBay route keeps its historical `ebay-srp` identifier so existing scripts
and result filenames continue to work. It combines deterministic listing props
with the restored captured eBay SRP chrome documented in
`proa_showcase/src/ebay/data/`.

## Result status

The branded eBay fixture was restored in August 2026 after an unrelated
debranding rewrite. Numbers from the temporary generic marketplace fixture
describe a different document and are intentionally not reproduced here. Do
not compare runs without first confirming the fixture revision and output byte
count.

Run the current matrix again before publishing eBay numbers. A valid
report must record:

- the exact commit and dirty-worktree state;
- machine model, CPU, memory, operating system, and browser version;
- engine and package versions;
- item count, concurrency, warmup, iteration count, and response byte count;
- raw per-run samples, not only aggregates;
- whether the measurement is pure render time, loopback HTTP latency, browser
  script time, or wire size.

## eBay workload

All engines receive the same deterministic listing data. The default view
renders 60 cards; scaling runs use 240 cards. Each card includes:

- a dynamically assembled eBay item link and captured image URL;
- an escaped title with realistic punctuation and reserved HTML characters;
- a multi-part subtitle;
- optional caption and discount branches;
- filled, half-filled, and empty rating icons;
- current and optional original prices;
- buying format, delivery, location, watcher count, and seller feedback.

The full-document variants also render the query, result count, captured navigation,
search controls, five facet groups, result controls, pagination, and footer on
every request. The legacy shell-splice microbenchmark remains useful for
comparing contiguous and scatter/gather buffers against the same captured
document shape. Browser measurements must account for the capture's external
image and vendor-script URLs; pure SSR and wire-size measurements do not fetch
those resources.

## Reading results

Keep these categories separate:

- **Pure SSR render** measures template/runtime work into an in-memory buffer.
- **Loopback HTTP** adds routing, request parsing, response construction, and
  socket writes.
- **Browser script time** adds client bundle parse/evaluation and hydration.
- **Wire size** depends on minification, serialization, and compression.

A result from one category is not evidence for another. Response bytes must be
reported alongside latency: a framework rendering less markup is not a fair
speed comparison.

Ratios are usually more portable than absolute times, but only when runs use
the same fixture, machine, process lifecycle, and output contract. Restart
long-lived Node processes between recorded matrices and inspect variance before
quoting a headline.

## Lean benchmark

The lean page renders 60 watch-heart toggles on minimal HTML. It is the best
workload for isolating framework runtime and hydration overhead, but it does not
model a production application's data access, business logic, or third-party
widgets.

For browser runs:

- use a production build with source maps and development branches disabled;
- clear browser cache and cookies between cold-cache samples;
- record the CPU-throttle and network profile;
- collect at least 30 samples per cell and publish median and p95;
- save raw CDP metrics so outliers and confidence intervals can be recomputed.

## Open methodology work

- [ ] Run the full current eBay matrix at 60 and 240 cards.
- [ ] Save raw latency samples and output hashes for every engine.
- [ ] Automate DOM/attribute parity checks across Proa, Marko, React, and Next.
- [ ] Record exact machine and runtime versions in result artifacts.
- [ ] Expand the lean workload to 1, 60, and 300 interactive widgets.
- [ ] Automate production-bundle checks for development-only React paths,
      source maps, and debugging hooks.
- [ ] Run at least 30 browser samples per cell and report confidence intervals.

Until these are complete, treat locally produced numbers as engineering
diagnostics, not public claims.

## Reproducing

```bash
# Build the Proa playground.
cargo build --release -p proa_demo --bin proa-playground --features server

# Start the eBay servers.
target/release/proa-playground &
cd proa_demo/react-ssr && PORT=5401 node --import tsx server.tsx &

# Start lean servers on separate ports.
target/release/proa-lean-server &
cd proa_demo/bench/lean-page/marko-server && PORT=5502 node server.mjs &
cd proa_demo/bench/lean-page/react-server && PORT=5503 node --import tsx server.tsx &

# Measure bytes and browser behavior.
node proa_demo/bundle_size.mjs
node proa_demo/bench/lean-page/lean-bundle-size.mjs
cd proa_demo/bench && node run.mjs
cd proa_demo/bench/lean-page && node lean-run.mjs
```

## See also

- [`rsjs/OPTIMIZATIONS.md`](../../rsjs/OPTIMIZATIONS.md)
- [`lean-page/README.md`](lean-page/README.md)
- [`proa_demo/bundle_size.mjs`](../bundle_size.mjs)
