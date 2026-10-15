# Measuring compile times

Use `proa build` to measure the complete linked application pipeline. Use
`cargo check` to isolate compiler and type-checking costs. Record the revision,
toolchain, profile, enabled features, and machine when comparing results.

Development builds use limited debug metadata (`debug = 1`) to reduce code
generation, linking, and artifact-verification work. Source-line backtraces
remain available. For full type and variable information in a debugger, set
`CARGO_PROFILE_DEV_DEBUG=2`; use `CARGO_PROFILE_TEST_DEBUG=2` for test binaries.
These settings do not change application optimization levels or release profiles.

## Repeated edit probes

Run the benchmark driver from the repository root in a dedicated worktree:

```sh
cargo build --locked -p proa-cli --bins
python3 scripts/benchmark_compile_times.py --label check --repetitions 5
python3 scripts/benchmark_compile_times.py --label linked --repetitions 5 -- \
  target/debug/proa build --root . -p proa_marketing --bin proa_marketing \
  --profile dev --locked --offline --no-check --no-assets
```

The linked command measures compilation with asset building and Proa source
checks skipped. Validate those separately. Deployable RSJS applications use
the linked build pipeline; ordinary `cargo check` remains useful for checking
Rust without producing a deployable executable.

The driver requires Python 3.10 or newer and clean versions of its probe inputs.
Use `--case` repeatedly to select a subset:

| Case | Change |
|---|---|
| `noop` | Run the command with unchanged source |
| `page` | Change homepage metadata in a Rust function body |
| `docs-prose` | Append a paragraph to a documentation page |
| `docs-code` | Append a new Rust fence to a documentation page |
| `core` | Change a rendering-core error message in a function body |

The tooling probes use the CLI build command:

```sh
python3 scripts/benchmark_compile_times.py --label cli --repetitions 5 \
  --case cli-command --case cli-lint --case cli-engine -- \
  cargo build --locked -p proa-cli --bins --timings
```

These probes change a diagnostic inside a command, the lint engine, or the
linked compiler engine. The driver recognizes the corresponding source paths
in revisions before the tooling crates were extracted.

Each case discards a warmup, then measures the requested number of edits.
Fresh per-run markers prevent a previous trial's cached results from
satisfying an edit probe. Compare medians and ranges, and repeat a baseline
when machine load or run order may affect the result.

To compare revisions, run the same driver in each worktree after building that
revision's CLI. For a revision without the driver, invoke the script by its
path in the newer checkout while keeping the older worktree as the working
directory. Use separate labels and the same build command and environment.

`target/compile-time-benchmark/<label>/` contains the command, compiler and
platform metadata, per-run logs, samples, summary statistics, and recovery
copies of source files. The driver restores source after success, command
failure, Ctrl-C, or SIGTERM. After a forced kill or machine interruption,
restore the files from `originals/` before continuing. Run no other builds or
source edits in that worktree during a measurement.

## Inspecting compiler units

```sh
cargo build --locked -p proa-cli --bins --timings
cargo check --locked -p rsjs_macros -p proa_macros --timings
```

Cargo writes HTML reports under `target/cargo-timings/`. A fresh target
directory measures clean compilation; an existing directory measures the work
invalidated since its previous build. The
[Cargo timing guide](https://doc.rust-lang.org/cargo/reference/timings.html)
explains the unit graph and compiler concurrency information. Assess clean,
development, release, and linked application builds separately.

## Compile work avoided by the workspace

The CLI command layer, source tools (`proa_source_tools`), and linked build
engine (`proa_build`) compile separately. A command edit reuses both engines;
an edit to either engine reuses the other. Existing `proa_cli` module paths
remain available through re-exports.

Font descriptors and request identities are available without the default
`proa_fonts/render` feature. Build collectors and font macros use that smaller
surface; applications enable rendering. Likewise, RSJS macros and runtime code
use the transport wire ABI without the default `rsjs_transport/extract` feature,
which enables object-file readers for build tools. Runtime rendering changes
therefore do not invalidate font tooling, and object extraction changes do not
invalidate application macros.

Tokio and Tower HTTP features belong to each consuming crate rather than a
workspace-wide `full` feature. Keep normal dependencies separate from those
needed only by examples, tests, or benchmarks. Check crates individually when
changing features; workspace feature unification can hide a missing feature.

Linked builds also compare the exact serialized descriptor-cache indexes and
snapshots before rewriting them. Identical records keep their existing files;
changed records still use atomic replacement and durable synchronization.
This avoids repeating hundreds of file and directory syncs on a warm build.
Cargo freshness, source/environment/dependency checks, and exact descriptor
object validation still run on every invocation.

Final RSJS macro expansion caches validated component and helper topology.
Each cache retains only its last exact plan per expansion thread. Every expansion still reads the current plan file and verifies its
expected SHA-256 commitment. Parsed topology is reused only for identical bytes
and plan identity; changed plans are validated again, and validation errors are
not cached. Definition selection still uses the current Cargo unit and record
identity. This avoids repeatedly parsing and validating a whole application
plan for each component or helper.

The template compiler and RSJS IR do not depend on the renderer's implementation.
`proa_html_policy` holds the allocation-free URL policy shared by `proa_dsl`
and `proa_core`. The renderer re-exports the existing validator path. Changing
rendering internals can therefore reuse the compiler crates.

The docs compiler stores disposable syntax-highlight results under
`OUT_DIR/.proa-docs-highlight-cache/`. Entries depend on code bytes, language,
and the content hash of the actual compiler executable, including its embedded
grammars. Compiler changes invalidate the cache automatically. Unavailable or
malformed entries fall back to highlighting; writes publish complete entries
atomically. `cargo clean` removes the cache.

Only highlighted fence output is reused. Markdown parsing, page metadata,
navigation, search, link validation, and removal of deleted pages still run
on every docs compilation. Filename and selected-line markup are applied after
the cached highlighting, so changing fence metadata remains effective.

Linked builds still hash artifact and input bytes for verification. SHA-256
uses runtime hardware detection on supported CPUs with a portable software
fallback; see the [RustCrypto backend documentation](https://docs.rs/sha2/0.11.0/sha2/#backends).
The development profile optimizes these hashing dependencies without changing
application optimization settings or requiring a native CPU target.
