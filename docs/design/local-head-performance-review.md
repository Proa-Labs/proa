# Local routing and metadata performance review

The local benchmark suite exposed avoidable request-path and metadata-patch
allocations. Both were removed. The measured renderer workloads remain within
local sampling variation, and the real HTTP captures have no response errors or
load-generator failures. This is **not** a universal zero-overhead claim: tiny
raw/dynamic requests retain about 20–53 ns of additional routing work.

[Raw evidence](evidence/head-local-benchmarks-performance.json) includes every
process sample, paired bootstrap intervals, compiler/checkout/source/binary
hashes, output parity hashes, and HTTP resource and generator diagnostics.
[Reproduction](../../proa_bench/probes/managed_requests/README.md) uses the actual
local `proa-benchmarks` fixtures with temporary Cargo patches.

## Scope and controls

- Before the latest tweaks: `1bde343a`.
- Initial implementation containing the regressions: `a5da47c9`.
- Fixed implementation: the tracked source and binary hashes in the evidence;
  measured before committing the performance fixes.
- Extra renderer/storage control: local main `ee29048e`.
- Benchmarks source: `ec96618`; unrelated local preview changes were preserved.
- Same Rust 1.95 compiler and resolved dependencies for before/candidate. Main
  uses its own frozen Proa dependency graph. Actual suite
  release binaries use its default release profile; request/native probes use
  opt-level 3, thin LTO and one codegen unit. No competing compiler jobs ran
  during measurement. HTTP uses one Tokio worker; CPU affinity is not pinned.

## Regressions found and fixed

The capture adapter originally constructed an async extractor and cloned/moved
the ready service for every request. A tiny plain route went from 26 to 28
allocations and was about 6.5% slower; an endpoint route added three allocations
and was about 6.9% slower. Static and ordinary named paths now use synchronous
preparation and the original ready-service path. Encoded captures, wildcards and
unusual URI rewrites retain Axum's decoding path.

The original bulk metadata operation cloned all head fields for each patch.
Inputs are now validated first, including URL resolution, graph retention and
slot limits, then applied under the same lock. Shared defaults retain their
normal copy-on-write behavior. The entire patch still rejects invalid input
without changing existing fields.

## Complete in-process requests

Medians across 15 alternating process pairs, each timing 100,000 warmed complete
requests. Parentheses show per-request allocation calls. All versions produce
identical response SHA256 hashes. Request construction and body collection are
included in both versions.

| Case | Before | Initial | Fixed | Fixed vs before |
| --- | ---: | ---: | ---: | ---: |
| Plain Axum, tiny response | 0.895 µs (26) | 0.960 µs (28) | 0.914 µs (26) | +2.2% |
| Proa endpoint, tiny response | 1.187 µs (34) | 1.283 µs (37) | 1.194 µs (34) | +0.7% |
| Named parameter endpoint | 1.945 µs (48) | 2.050 µs (52) | 1.998 µs (48) | +2.7% |
| Managed head, individual setters | 2.874 µs (60) | 2.937 µs (62) | 2.816 µs (60) | −2.0% |
| Managed head, bulk metadata | — | 3.084 µs (65) | 2.883 µs (59) | +0.3% vs baseline setters |

Initial and final captures are separate alternating runs; do not subtract their
raw absolute times as if they were simultaneous. Final allocation **bytes** also
match the baseline for all four existing cases. Bulk metadata uses one fewer
allocation and 11 fewer allocated bytes than baseline individual setters.

Paired-median 95% bootstrap intervals for final request latency are +0.5…+3.7%
(plain), −1.7…+2.7% (endpoint), +1.2…+4.0% (named parameter), and −2.7…−0.4%
(managed individual setters). The remaining tiny-route cost is measurable; it
must not be described as zero overhead. Bulk vs baseline setters has an
interval of −0.8…+1.7%.

## Canonical renderer workloads

Unmodified benchmark adapters, fresh owned output, nine alternating process
triplets and exact output parity. Values are the median process P50. The paired
bootstrap intervals include no change for all three workloads.

| Workload | Main | Before | Fixed | Fixed vs before |
| --- | ---: | ---: | ---: | ---: |
| 32-card page | 2.000 µs | 2.000 µs | 2.041 µs | +2.05% |
| 96-product grid | 26.083 µs | 25.625 µs | 25.750 µs | +0.49% |
| 1,000-row table | 253.917 µs | 255.542 µs | 254.792 µs | −0.29% |

The card-page difference is one approximately 41 ns timer tick. Paired-median
95% intervals are −2.1…+2.1%, −0.6…+0.7%, and −0.8…+0.6%, respectively.

## Real HTTP at equal load

Existing benchmark server, load driver and SHA256 response validator. Three
alternating pairs per workload, one-second pre-roll plus five-second windows.
All **91,800 responses** passed validation; all 18 phases passed generator CPU
and lag limits. Completion throughput matched the offered 2,000/500/50 RPS.

| Workload | Before P50 | Fixed P50 | Before CPU/request | Fixed CPU/request |
| --- | ---: | ---: | ---: | ---: |
| Cards | 0.100 ms | 0.096 ms | 0.0296 ms | 0.0276 ms |
| Products | 0.213 ms | 0.212 ms | 0.0914 ms | 0.0877 ms |
| Table | 1.022 ms | 0.951 ms | 0.6993 ms | 0.7006 ms |

These are fixed-load local measurements, not saturation-capacity claims.

## Native storage and remaining limits

Against the pre-tweak baseline, ordinary local/Send component, ErrorBoundary,
and Suspense allocation counts/bytes, future sizes, output bytes and poll counts
match exactly. The 19-sample native timing medians differ by −0.5…+0.7%.

Against main, local ErrorBoundary and Suspense retain the previously documented
**8 extra bytes per boxed allocation**, with no additional allocation count.
This predates the latest tweaks and is unchanged here. Ordinary component cases
use the same allocation counts and fewer bytes than main in these probes.
Managed metadata intentionally has its own preparation cost. Encoded/wildcard
capture latency, the full ORIGIN application, maximum HTTP capacity, cold start
and browser hydration were not separately timed in this review.

## Correctness checks

Core and framework tests pass, including atomic patch rejection at the metadata
slot limit, invalid URLs/templates/media, inherited fields, decoded and Unicode
slugs, nested inner context layers, URI rewriting and streaming. A regression
test verifies that the static context path calls the ready service without
cloning it. Formatting, source lint, allocation lint and workspace checking also
passed.
