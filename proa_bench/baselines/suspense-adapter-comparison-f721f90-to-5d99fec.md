# Suspense adapter architecture comparison

This report compares the durable pre-architecture result in
`suspense-adapter-before-f721f90.json` with the post-architecture result in
`suspense-adapter-after-5d99fec.json`.

Both runs used the same Apple M4 Max host, Rust 1.95.0, release profile,
10,000 requests per repeat, 250 warmup requests, three repeats, Tokio worker
counts 1 and 4, concurrency 1/32/128, and immediately ready tiny boundaries.
Values below are the median of the three reported per-repeat percentiles or
throughput values. The client is an in-process fast body collector; these are
framework-overhead measurements, not production capacity estimates.

## Direct `Send` path versus the old per-response bridge

| Workers | Concurrency | Boundaries | Before p50 | Direct p50 | p50 speedup | Before p99 | Direct p99 | Successful throughput | Errors before -> after |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 0 | 13.46 us | 2.25 us | 6.0x | 27.88 us | 2.79 us | 70,169 -> 437,166 rps | 0 -> 0 |
| 1 | 1 | 32 | 40.96 us | 21.12 us | 1.9x | 57.25 us | 26.71 us | 23,597 -> 47,262 rps | 0 -> 0 |
| 1 | 1 | 128 | 129.04 us | 77.33 us | 1.7x | 155.29 us | 89.42 us | 7,669 -> 12,911 rps | 0 -> 0 |
| 1 | 128 | 0 | 160.79 us | 2.21 us | 72.8x | 28.70 ms | 2.75 us | 86,545 -> 441,504 rps | 0 -> 0 |
| 1 | 128 | 32 | 3.335 ms | 21.12 us | 157.9x | 4.153 ms | 26.67 us | 40,592 -> 47,243 rps | 0 -> 0 |
| 1 | 128 | 128 | 25.03 ms | 77.00 us | 325.0x | 73.05 ms | 89.54 us | 2,037 -> 12,967 rps | 29,409 -> 0 |
| 4 | 128 | 0 | 421.58 us | 4.12 us | 102.2x | 12.76 ms | 6.33 us | 77,697 -> 911,809 rps | 0 -> 0 |
| 4 | 128 | 32 | 1.538 ms | 31.25 us | 49.2x | 3.759 ms | 52.42 us | 73,680 -> 123,922 rps | 0 -> 0 |
| 4 | 128 | 128 | 7.865 ms | 115.42 us | 68.1x | 19.87 ms | 175.46 us | 9,392 -> 33,998 rps | 27,760 -> 0 |

The old 128-boundary/concurrency-128 cases failed 29,409 of 30,000 attempts
with one worker (98.0%) and 27,760 of 30,000 with four workers (92.5%). Both
the bounded Local path and direct Send path completed every measured request
in the post-architecture run. Before-run latency and successful throughput for
those two rows describe only the small successful subset.

## Bounded Local compatibility path

The Local path is primarily the compatibility and overload-safety path. It
uses a persistent pool whose worker count matches the benchmark's Tokio worker
count, bounded admission, and byte/chunk-bounded cross-thread transport. The
old bridge could escape a nominal one-worker runtime through Tokio's much
larger blocking pool, so low-percentile results are not expected to improve in
every saturated case.

Representative results:

| Workers | Concurrency | Boundaries | Before p50 | Local p50 | Before p99 | Local p99 | Errors before -> after |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 32 | 40.96 us | 38.79 us | 57.25 us | 55.12 us | 0 -> 0 |
| 1 | 128 | 32 | 3.335 ms | 5.036 ms | 4.153 ms | 5.327 ms | 0 -> 0 |
| 1 | 128 | 128 | 25.03 ms | 13.21 ms | 73.05 ms | 13.66 ms | 29,409 -> 0 |
| 4 | 128 | 0 | 421.58 us | 565.79 us | 12.76 ms | 625.21 us | 0 -> 0 |
| 4 | 128 | 32 | 1.538 ms | 1.697 ms | 3.759 ms | 2.126 ms | 0 -> 0 |
| 4 | 128 | 128 | 7.865 ms | 4.733 ms | 19.87 ms | 5.663 ms | 27,760 -> 0 |

This is the intended trade: finite resource use and controlled tails for
legacy local futures, with the direct Send route available when the complete
render graph carries the stronger compiler proof.

## Boundary batching

For immediately ready boundaries, the old transport emitted the shell plus
one body chunk per boundary. The post-architecture driver emits the shell plus
one same-poll batch:

| Boundaries | Before chunks | After Local chunks | After Send chunks |
|---:|---:|---:|---:|
| 0 | 1 | 1 | 1 |
| 1 | 2 | 2 | 2 |
| 8 | 9 | 2 | 2 |
| 32 | 33 | 2 | 2 |
| 128 | 129 | 2 | 2 |

No timer or artificial batching delay is used; only work already ready in the
same driver poll is combined.

## Interpretation

The direct Send path removes the per-response runtime, blocking-pool job,
startup blocking receive, and cross-thread chunk bridge. The measured
improvement is large enough to clear the architecture's central performance
goal, especially at concurrency. The bounded Local result also clears the
progress/safety goal: all 1.08 million Local/control attempts in the new matrix
completed without an error, including the saturation cases that failed in the
old architecture.

The benchmark intentionally does not include sockets, TLS, delayed data,
large boundary bodies, slow clients, RSS, allocator counts, or OS context
switch measurements. Correctness tests separately cover first-item failure,
queued and active cancellation, byte-bound enforcement, backpressured timeout
progress, nested parent-before-child order, CSP, and request-state isolation.
