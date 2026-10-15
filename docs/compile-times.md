# Measuring compile times

Use Cargo timings to distinguish dependency compilation, crate compilation,
and linking before changing crate boundaries:

```bash
cargo build --locked -p proa-cli --bins --timings
cargo check --locked -p rsjs_macros -p proa_macros --timings
```

Cargo writes an HTML report under `target/cargo-timings/`. A fresh target
directory measures a clean build; an existing directory measures the work
invalidated since its previous build. Record the compiler version, profile,
target, enabled features, and command when comparing results. See the
[Cargo timing documentation](https://doc.rust-lang.org/cargo/reference/timings.html)
for the unit graph and compiler concurrency information.

## Testing an RSJS build engine extraction

`scripts/compile_time_experiment.py` measures a specific proposed boundary:
moving the CLI's descriptor build, cache, linking, ABI adaptation, and final
compilation modules into a separate library crate. Inspection stays in the
CLI because it depends on the linter's source fingerprinting.

Run the experiment in a dedicated worktree, with Python 3.10 or newer and the
repository's pinned Rust toolchain:

```bash
python3 scripts/compile_time_experiment.py --repetitions 5
```

The script requires clean versions of the source files it edits. It temporarily
changes workspace manifests, module declarations, and five internal visibility
boundaries. An unpublished crate under `target/compile-time-experiment/` compiles
the existing source files through path attributes; the CLI re-exports those
modules. No source files are moved.

Each of the baseline, split, and restored phases measures `cargo build` and
`cargo check` for both CLI binaries. It measures unchanged builds, a diagnostic
string edit in `chunks.rs`, and a validation string edit in the RSJS pipeline.
It warms each command and changed item before collecting samples, and tests
the extracted engine's library tests before the restored phase. Compare medians
and ranges, and verify the restored phase agrees with the baseline.

The output directory contains:

- `metadata.json`: revision, toolchain, and probe definitions.
- `results.json`: wall times and the full Cargo timing units for each run.
- Per-run logs, including the extracted engine's test output.
- `originals/`: recovery copies of every file the experiment can edit.

Source and manifests are restored on normal completion, build failure,
Ctrl-C, or SIGTERM. After a forced process kill or machine interruption,
restore the files from `originals/` before continuing. Run no other Cargo
commands or source edits in the worktree during the experiment.

This experiment measures warmed development builds for two specific edits.
Assess clean builds, release builds, application builds, and edits to shared
dependencies separately. A crate split can avoid rebuilding unaffected code
while adding another compilation step when the extracted code itself changes.
