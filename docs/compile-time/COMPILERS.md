# Upstream compiler comparison

These are local three-sample warm-edit medians on the 24-route consumer, with
its own `nodebug` profile and isolated Cargo targets. Build-only includes process
startup/orchestration; it excludes watcher debounce and browser paint. Every
configuration ran serially, without another test/build compilation competing.

## Linked Proa pipeline: supported stable compilers

| Compiler | SSR warm build | Split RSJS-island warm SSR-page edit | No-op SSR build |
| --- | ---: | ---: | ---: |
| Repository Rust 1.95 | 2.499s | 2.602s | 1.536s |
| Current stable Rust 1.99 | 2.696s | 2.740s | 1.716s |

The island descriptor remains fresh after the page edit; its final crate still
rebuilds because the global plan changes. The separate island handler edit is
also recorded, but has only one sample per compiler. See
`evidence/compiler-matrix-initial.json` and its configuration metadata.

Rust 1.99 was released on October 1, 2026; it is the current stable for this
October 3 experiment. This fixture does not demonstrate a warm-rebuild gain
from upgrading the compiler. It is not a claim that Rust 1.99 is generally
slower. [Official release announcement](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/).

## Nightly compatibility findings

The August 26 nightly (`1.100.0-nightly`, `787af2b8c`) fails the current Proa
wrapper even with default flags: Cargo supplies separate `.rlib` and `.rmeta`
inputs for one extern name, and the exact dependency map currently rejects
multiple artifacts. Solver, frontend, and backend trials fail at the same gate;
their failures are not evidence of a compiler-feature regression.

The August 1 nightly (`1.99.0-nightly`, `ad3d0bc14`) fails a different existing
wrapper assumption: locating build-script environment declarations beside
`OUT_DIR`. Its linked feature trials consequently produce no latency result.
See `evidence/compiler-matrix-aug01-linked.json` and
[the architecture implications](INVALIDATION.md).

Both failures remain fail-closed. Supporting these Cargo layouts requires exact
multi-artifact dependency roles and an authoritative build-script observation
handoff; dropping metadata inputs or skipping environment validation would
make the benchmark run by weakening the build contract.

## Compiler-only SSR isolation

Direct Cargo SSR builds succeed, allowing feature comparisons without the
wrapper. The direct RSJS-island attempt correctly fails Proa's existing
plain-Cargo deployability guard; it was not bypassed. The table uses only the
completed SSR samples before that guard. Each has three serial warmed edits,
with raw records retained in `evidence/compiler-direct-ssr.json`; subsequent
guard diagnostics are in `evidence/compiler-direct-island-guard.json`.

| Compiler/configuration | Warm SSR edit | No-op | First build (one sample) |
| --- | ---: | ---: | ---: |
| Rust 1.95, LLVM | 0.668s | 0.431s | 18.668s |
| Rust 1.99, LLVM | 0.759s | 0.321s | 17.493s |
| August 1 nightly, LLVM | 0.723s | 0.331s | 19.929s |
| Same nightly, `-Znext-solver=globally` | 0.718s | 0.327s | 18.494s |
| Same nightly, `-Zthreads=4` | 0.743s | 0.456s | 14.289s |
| Same nightly, Cranelift + `-Cpanic=abort` | 0.829s | 0.366s | 17.698s |

The next solver's ~5ms warm difference is too small to treat as a demonstrated
gain. Four frontend threads show a lower first-build observation but no warm
gain; the first build has only one sample. Cranelift is supported on this macOS
AArch64 host, and the matching preview component was installed. Panic abort
avoids its documented macOS unwinding limitation, so the backend comparison
also changes panic semantics. It does not win this warm workload.
[Cranelift support and limitations](https://github.com/rust-lang/rustc_codegen_cranelift).

The next-solver flag selects a different trait solver; it is not a general
compile-time acceleration switch. The compiler guide documents frontend
parallelism separately from concurrent codegen units. Its parallel page warns
that portions are outdated, so the exact tested flags/compiler versions above
are part of the evidence. [Next-solver flag](https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/next-solver.html),
[parallel compiler guide](https://rustc-dev-guide.rust-lang.org/parallel-rustc.html).

## Decision

Keep experimental solver/frontend/backend settings out of generated defaults.
The framework cache savings are independent of these compiler features.
Complete incremental carriers let Proa consume Cargo/rustc reuse while retaining
all descriptors. The next architecture work is stable owner/unit plan inputs
and Cargo output-layout compatibility, with broader compiler comparisons after
those protocols are supported. Do not predict the user's complete Rust rebuild
from a direct Cargo SSR timing or promise a speedup from future compiler merges.

The direct SSR commands can be reproduced without the expected island failure:

```sh
python3 scripts/compiler_matrix_probe.py --fixture /tmp/proa-dev-perf-compiler-matrix \
  --cli target/release/cargo-proa --cases direct-ssr --suffix=-ssr
```
