# Development rebuild investigation

Branch `perf/dev-rebuild-pipeline` starts at fetched `origin/main` (`ab6e5cd`).
The worktree is separate from the original checkout. Experiments were run on
2026-10-03 on an Apple M4 Max, macOS 26.5.2, with assets skipped. Rust 1.95 is
the repository's pinned compiler; the compiler matrix separately tests current
stable 1.99 and a pinned nightly. See [implementation details](IMPLEMENTATION.md).

## Measured outcomes

The latest complete architecture meets the **2–3 second warm rebuild target**
at its upper edge on the measured machine: 2.987s template / 2.964s function
edits, using ordinary compiled templates and the normal dev profile. One of
six samples took 3.079s, so this is typical latency rather than a strict limit
on every edit. See [the follow-up and its limits](PIZZA_TARGET.md).

| Warm save to changed HTTP response | Main baseline | Recorded branch | Checkpoint |
| --- | ---: | ---: | --- |
| Pizza template edit, dev, 300ms debounce | 6.97s | 2.99s | Hashing/watcher |
| Pizza template edit, dev, actual 200ms CLI default | — | 2.89s | Hashing/watcher |
| Pizza Rust function-body edit, dev, actual CLI default | — | 2.91s | Hashing/watcher |
| Pizza template edit, dev, actual CLI default | — | 3.035s | Complete unit projections |
| Pizza Rust function-body edit, dev, actual CLI default | — | 3.037s | Complete unit projections |
| Pizza template edit, dev, actual CLI default | — | 2.993s | Unit derivation cache |
| Pizza Rust function-body edit, dev, actual CLI default | — | 3.033s | Unit derivation cache |
| Pizza template edit, dev, actual CLI default | — | 3.035s | Warm wrapper reuse |
| Pizza Rust function-body edit, dev, actual CLI default | — | 2.993s | Warm wrapper reuse |
| Pizza template edit, dev, actual CLI default | — | 2.987s | Parallel publication / retained buffers (latest) |
| Pizza Rust function-body edit, dev, actual CLI default | — | 2.964s | Parallel publication / retained buffers (latest) |
| Standalone 24-route consumer, dev, 300ms debounce | 6.91s | 4.00s | Initial investigation |
| Explicit hot-static text edit, nodebug, 100ms debounce | — | 0.13s | Initial investigation |

Each number is a median of three serialized edits with warmed caches and a
release-built CLI. The Pizza application was copied to `/tmp` with workspace
inheritance resolved and Proa dependencies linked to this branch. Assets are
skipped in all trials; browser navigation, hydration, and paint are not measured.
The standalone and hot-static rows were not remeasured for this follow-up.

![Main and current defaults, with debounce settings explicit](diagrams/pizza-current-waterfall.png)

The initial branch measured 4.85s; a fresh software-hashing control measured
4.67s. Accelerated hashing alone measured 3.10s at the original 300ms debounce.
Parallel observed-input validation and faster observation of graceful server
exit brought the next checkpoint to 3.02s. The hashing/watcher same-settings run is
2.99s; the default quiet window is now 200ms and measured 2.89s. All six default
template/function samples were below three seconds.

![Current Pizza cache/link/setup breakdown](diagrams/pizza-current-detailed-waterfall.png)

Object hashing, observed source/extern validation, and transport-manifest
construction dominated the earlier setup stage. Semantic planning itself took
less than 1ms. The CLI now uses runtime-dispatched SHA-256 on AArch64 while
preserving every hash domain and current-byte validation. Cargo-stage timings
also include wrapper validation; they are not isolated rustc CPU measurements.

The opt-in static-update measurement uses a different edit path, profile, and
debounce. Its rendering overhead and supported syntax are documented in
[HOT_STATIC.md](HOT_STATIC.md). The ordinary Pizza trials do not use it.

## What we tried, in priority order

1. **Cache publication and the actual app waterfall.** Byte-identical durable
   indexes now retain their existing publication. Changed publications preserve
   atomic replacement/durability and the existing cache lease. Symlink and
   corrupted/interrupted-cache checks remain active. This accounts for most of
   the improvement; it remains useful with any future compiler.
2. **Duplicate validation and extraction.** Retain immutable validated object
   bytes and both hash domains through manifest construction and extraction.
   Reuse paired dependency hashes within a cycle. A bounded in-process record
   cache avoids reparsing exact unchanged generations after fresh validation.
3. **Incremental descriptor compilation.** Preserve Cargo's incremental flags
   and codegen partitioning when collecting complete rlibs or native host linker
   object inputs. Partial-edit/removal tests retain unused witnesses across
   multiple codegen units. Unsupported transports keep a conservative complete
   single-object carrier. In the dev-profile fixture, incremental descriptors
   reduce the Cargo stage but archive validation offsets much of that gain;
   there is no demonstrated additional watcher speedup over the cache fix.
4. **Narrow final invalidation.** Stable runtime coordinates, fragment/owner
   generations, and complete unit projections now replace global plan dependencies
   in semantic-plan expansions. The real Cargo fixture keeps unrelated B fresh
   after an A edit and unused root insertion, and refreshes consumers after shared
   helper/child changes. HTML, manifest imports, generated JavaScript, and current
   dep-info agree; exact full-plan/root validation remains. See
   [the architecture and acceptance evidence](INVALIDATION.md).
5. **Upstream compiler comparison.** Run isolated, serialized configurations
   for the repository compiler, current stable, pinned nightly, next solver,
   parallel frontend, and Cranelift. Results and failures are retained in
   [the compiler comparison](COMPILERS.md).
6. **Scoped static updates.** The remaining full-build latency justifies an
   explicit `html_hot_sync!` / `proa dev --hot-static` prototype. HTTP checks cover
   escaping, static attributes, dynamic fallback, failed builds, and repair.
   [Usage and limits](HOT_STATIC.md) describe its deliberately narrow scope.

## Next decisions

1. Review/land the verified cache, complete descriptor collection, and unit-plan
   invalidation work. The complete architecture now meets the typical warm
   Pizza target; preserve the corruption checks and runtime/reuse acceptance
   fixture as these changes evolve.
2. Harden/measure complete descriptor carriers on more targets and larger apps.
   Do not infer a broad end-to-end gain from the smaller descriptor Cargo stage.
   Model multiple exact artifacts per extern and use Cargo's public build-script
   observations before adopting the experimental nightly output layouts.
3. Seek more margin below 3s from representative workloads and the measured
   remaining stages. Fresh executable first-launch overhead explains much of
   the restart stage on this Mac; repeated application startup is only a few
   milliseconds. Cargo dependency crates remain the reuse boundary, so Pizza's
   single component crate benefits less from narrowing invalidation.
4. Keep static updates explicit while measuring rendering overhead and fixing
   watcher-session cold builds and source discovery. Broaden supported templates
   only from representative use and correctness evidence.

New and existing applications receive these defaults through the CLI. Scaffolded
dev profiles already inherit Cargo's incremental default; no extra manifest
flags or automatic hot-static opt-in are needed.

## Learning and reproduction

Cargo's [incremental profile setting](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)
controls reuse inside rebuilt crates. Cargo separately skips whole fresh crates.
The compiler guide's [incremental overview](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
and [detailed dependency algorithm](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
explain dependency tracking and result fingerprints. Our collector uses public
rlib/linker outputs rather than inspecting rustc's private incremental directory.

```sh
cargo build -p proa-cli --bin cargo-proa --release
python3 scripts/clone_example_probe.py --help
python3 scripts/compile_time_probe.py --help
python3 scripts/dev_latency_probe.py --help
python3 scripts/compiler_matrix_probe.py --help
```

Raw numerical evidence is in [evidence/](evidence/); SVG waterfalls in
[diagrams/](diagrams/) can be exported. [Historical prototype notes](HISTORICAL.md)
retain the earlier `c3cbaee` investigation and isolated cache experiment; their
estimates and old measurements are not the final branch results above.
