#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Server-side SSR load test for the fully dynamic eBay SRP.
#
# Measures p50/p95/p99 latency and throughput with oha at c=1 (single user,
# pure server processing time on loopback) and c=8 (light concurrency, where
# synchronous renderers queue). Run each engine first:
#
#   proa:   PORT=5002 target/release/proa-playground
#   react:  cd proa_demo/react-ssr  && NODE_ENV=production PORT=5401 node --import tsx server.tsx
#   next:   cd proa_demo/next-ssr   && pnpm run build && NODE_ENV=production ./node_modules/.bin/next start -p 5402
#
# Usage: ./ssr-loadtest.sh [n]   (items per page: 60, 120, 240)
set -euo pipefail

N="${1:-60}"
DUR="${BENCH_DURATION:-8s}"
PROA="${PROA_URL:-http://127.0.0.1:5002/showcase/ebay/proa-rsjs}"
REACT="${REACT_URL:-http://127.0.0.1:5401/showcase/ebay/react}"
NEXT="${NEXT_URL:-http://127.0.0.1:5402/showcase/ebay/next}"
MARKO="${MARKO_URL:-http://127.0.0.1:5300/showcase/ebay/marko}"

if ! command -v oha >/dev/null; then
  echo "oha not found (cargo install oha)" >&2
  exit 1
fi

run() {
  local label="$1" url="$2" c="$3"
  env -u NO_COLOR oha --no-tui -z "$DUR" -c "$c" --output-format json "$url" 2>/dev/null | node -e '
    let d = "";
    process.stdin.on("data", (x) => (d += x)).on("end", () => {
      const j = JSON.parse(d);
      const p = j.latencyPercentiles || {};
      const ms = (x) => (x * 1000).toFixed(2).padStart(9);
      console.log(
        process.argv[1].padEnd(16),
        ("c=" + process.argv[2]).padEnd(4),
        "p50" + ms(p.p50) + "ms",
        "p95" + ms(p.p95) + "ms",
        "p99" + ms(p.p99) + "ms",
        "rps " + j.summary.requestsPerSec.toFixed(1).padStart(8),
        "ok " + (100 * j.summary.successRate).toFixed(0) + "%",
      );
    });
  ' "$label" "$c"
}

echo "=== fully dynamic eBay SRP, n=$N items per page, ${DUR} per cell ==="
for c in 1 8; do
  run "proa-rsjs" "$PROA?n=$N" "$c"
  run "marko 6" "$MARKO?n=$N" "$c"
  run "react 19" "$REACT?n=$N" "$c"
  run "next 16" "$NEXT?n=$N" "$c"
done
