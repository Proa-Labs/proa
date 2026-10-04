# Development rebuild performance

This branch reduces ordinary warm Pizza rebuilds from about 7s to roughly 2.8s.
Measurements were made on 2026-10-03 on an Apple M4 Max running macOS 26.5.2
and Rust 1.95, with a release-built CLI and skipped assets.

| Save to changed HTTP response | Median | Samples (seconds) |
| --- | ---: | --- |
| Main: template, 300ms debounce | 6.974s | 7.027, 6.974, 6.844 |
| Branch: template, default 200ms debounce | 2.987s | 2.987, 3.079, 2.932 |
| Branch: Rust function, default 200ms debounce | 2.964s | 2.925, 2.964, 2.965 |

Each case uses three serialized edits with warm caches, ordinary compiled
templates and the normal dev profile. Startup, assets and browser hydration/paint
are excluded. The example was copied to `/tmp` and linked to the corresponding
checkout. The branch results were measured at code commit `26e6529`; follow-up descriptor trials appear below; the final combination with the
subsequent hot-static fixes has not been separately rebenchmarked. One sample
exceeded 3s, so these are typical latencies on one machine, not a strict bound.

## Descriptor-size follow-up (2026-10-04)

| Experiment | Before | After | Decision |
| --- | ---: | ---: | --- |
| Omit descriptor debug info | 2.996s | 2.801s | Keep: 196ms saved, full final-app debug retained |
| Share source hashes within validation | 2.717s | 2.723s | Discard: no established end-to-end gain |
| Optimize host build dependencies | 2.742–2.809s controls | 2.709s | Keep optional: first startup rises from 37s to 72s |

The descriptor trial uses six warm edits per variant with unchanged normal-dev
settings and 200ms debounce. Instrumented file reads fall from 1.76 to 1.20 GiB
per edit; bytes fed to hashes fall from 2.90 to 1.88 GiB. These are warm file
reads, not measured SSD traffic, and exclude compiler-internal I/O and native
carrier copying. The largest descriptor archive falls from 99MB to 27MB. The
final app retains exactly the same 5.65MB of debug sections.

The reuse trial randomized 12 adjacent before/after pairs using one binary and
namespace. Its source-cache implementation was removed. The smaller change
that skips an unused input-domain hash for link-only artifacts remains; the
combined trial reduced manifest-stage medians from 55ms to 38ms, without
establishing an end-to-end win. Hash algorithms and cache identities stay intact.

The build-dependency trial uses six edits per profile, bracketed by six normal-dev
edits before and after. Its modest gain does not justify changing `proa new`
defaults. Descriptor no-debug is a CLI policy and requires no app manifest
change. Policy version 6 intentionally populates a fresh descriptor namespace
on first use, retaining subsequent incremental reuse.

The figure uses representative edits from separate trials; its lower panel
shows median substage times from three newer traced edits. The main remaining
costs are the descriptor and final compiler passes. Detailed profiling stays in
the investigation checkout and is excluded from this PR.

![Pizza waterfall and remaining cache work](diagrams/pizza-current-waterfall.png)

[Compact measurements](evidence/pizza-measurements.json) retain all sample
boundaries, current stage durations, CLI digest and source-record hashes.
Intermediate experiment logs and duplicate figures are omitted from this PR.
The original pipeline logs remain in Git revision `1f2ece6`; follow-up raw
traces stay outside the PR.

## What changed

1. Avoid rewriting identical durable cache indexes; retain atomic publication,
   leases and corruption checks for changed records.
2. Reuse freshly validated carrier bytes and parsed records, deduplicate input
   hashing, use accelerated compatible SHA-256, and publish independent records
   in parallel. The earlier waterfall identified validation and transport work
   as the major setup costs; semantic planning takes less than 1ms.
3. Preserve incremental compilation and multiple codegen units when the CLI can
   collect complete descriptor carriers. Archive validation offsets some compiler
   savings; the descriptor stage alone does not predict end-to-end speedup.
4. Track complete per-unit plan projections and stable runtime/asset identities,
   allowing unrelated final crates to stay fresh. Cargo crate dependencies remain
   the reuse boundary, so Pizza's single component crate benefits less here.
5. Reduce the default quiet window to 200ms and observe graceful server exit
   sooner. `--debounce-ms 300` retains the previous quiet window.

[Implementation](IMPLEMENTATION.md) describes cache/carrier changes;
[invalidation](INVALIDATION.md) explains the dependency contract; the
[author review](REVIEW.md) records compatibility breaks, exact checks and merge
risks. All checks passed on `07f516b`; the latest combined changes must also pass
CI before merging. A coordinated CLI/macro/runtime release is required.

The optional [static-update prototype](HOT_STATIC.md) uses an explicit macro
and dev flag. Its separate 0.129s trial used a different profile and 100ms
quiet window; it does not contribute to the ordinary Pizza results above.
Scaffolded dev profiles already inherit Cargo's incremental default, so
`proa new` needs no additional manifest flags or automatic prototype opt-in.

## Reproduce

Build the release CLI, then copy an example with
`python3 scripts/clone_example_probe.py webmcp-pizza --output /tmp/proa-pizza-probe`.
Place `EDIT_MARKER_A` in a template's HTTP-visible text in the copied `src/lib.rs`.
Run the probes serially; the watcher probe restores its source afterward.

```sh
cargo build -p proa-cli --bin cargo-proa --release
PROA_BUILD_TIMINGS=1 python3 scripts/dev_latency_probe.py \
  --fixture /tmp/proa-pizza-probe --cli target/release/cargo-proa \
  --source src/lib.rs --url-path / --profile dev
```

Probe output stays in the fixture's `target/probe-logs/`; do not commit the
compiler logs or intermediate plots. `scripts/compile_time_probe.py` provides
isolated build-only trials, and `scripts/hot_static_acceptance.py` checks the
prototype's HTTP behavior.

Cargo's [incremental profile setting](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)
controls reuse within rebuilt crates; Cargo separately skips whole fresh crates.
The [compiler's incremental overview](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
explains the query dependency model. Upstream compiler experiments did not
establish a warm-build win from the next solver on this workload; their detailed
investigation remains outside the implementation diff.
