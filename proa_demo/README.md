# proa_demo

Runnable demo and comparison harness for Proa.

This crate owns executable concerns:

- `run-playground.sh` and the `proa-playground` server.
- FFI exports used by external harnesses.
- Framework comparison servers and clients under `react-ssr`, `next-ssr`,
  `marko-server`, and related shared packages.
- Demo-only rsjs wiring such as `src/ebay_srp_islands.rs` and
  `src/ebay_srp_filter_runtime.rs`.

Reusable page fixtures belong in `proa_showcase`. The compatibility modules
`src/ebay_srp*.rs` intentionally re-export the eBay fixture modules from
`proa_showcase` so older playground and benchmark call sites keep working
while the ownership boundary stays clean.
