# Managed-head regression probes

Compile the **same probe source** against main and the implementation. The baseline
must contain the same native APIs and use the same Rust compiler/profile.

In each checkout:

```sh
cargo build -p proa_core -p proa_stream -p proa_macros --release
```

From the candidate checkout:

```sh
python3 proa_bench/probes/compare_head.py /path/to/main . \
  --output docs/design/evidence/head-native-performance.json \
  --timing-samples 19
```

The runner compiles with optimization, thin LTO and one codegen unit. It saves
compiler identity, probe hashes, linked-library hashes, checkout revisions, tracked
diff hashes, raw allocation/size/poll/output results and every latency sample.
The candidate is uncommitted during development; identical HEAD revisions do not
mean identical libraries. Artifact hashes identify the actual builds.

| Probe | What it checks |
| --- | --- |
| `head_free_components.rs` | No-hook leaf, macro-composed leaf, pending child, 32-child loop; WebContext, SendWebContext and WriteError sizes |
| `head_free_send.rs` | Same composition/storage cases for Send macros (storage measured; timings not included) |
| `async_boundary_storage.rs` | Ready, pending and recovered native ErrorBoundary futures and allocations |
| `suspense_storage_body.rs` | Native local/Send Suspense, static/in-order, ready/pending |

Latency uses 3,000,000 warm-buffer renders per case, alternating process order.
The counting allocator remains enabled in both builds. Keep other compiler jobs
idle during measurement. Nanosecond microbenchmarks are sensitive to code layout,
CPU frequency and the host workload: retain raw results and do not call matching
allocation counts a proof of matching latency.

These probes do **not** measure network TTFB, full router/middleware throughput,
managed-head serialization cost or end-to-end request context construction. Managed
mode deliberately pays for metadata state, validation and publication. Native
component boundaries must not gain hook allocations or larger futures.
