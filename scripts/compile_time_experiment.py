#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure a temporary RSJS build-engine crate extraction in a dedicated worktree.

Uses only the Python standard library. Run from the repository root. Relevant
source files must be clean. Manifests and source are restored even on failure;
the recovery copies, logs, Cargo timing units, and results stay under target/.
Do not run another Cargo process in the worktree during the experiment.
"""

from __future__ import annotations

import argparse
import json
import re
import signal
import subprocess
import time
from pathlib import Path


MODULES = (
    "abi_adapter", "build", "cache", "final_compile", "legacy_graph", "link",
    "pipeline", "semantic_link",
)
VISIBILITY = {
    "build": (("fn", "emit_rustc_wrapper_diagnostic"),),
    "cache": (("const", "DESCRIPTOR_CACHE_SCHEMA_VERSION"),),
    "final_compile": (("fn", "snapshot_wrapper_executable"),),
    "pipeline": (
        ("enum", "LinkedCargoStage"),
        ("fn", "run_linked_cargo_cached_with_progress"),
    ),
}
CASES = (
    ("cli", "proa_cli/src/chunks.rs", "failed to read {}"),
    ("engine", "proa_cli/src/rsjs/pipeline.rs",
     "linked Cargo test requires exactly one descriptor-only"),
)


def run(command: list[str]) -> str:
    return subprocess.check_output(command, text=True).strip()


def experiment(repetitions: int, output: Path) -> None:
    root = Path.cwd()
    output.mkdir(parents=True, exist_ok=True)
    files = ["Cargo.toml", "Cargo.lock", "proa_cli/Cargo.toml",
             "proa_cli/src/rsjs/mod.rs"]
    files += [f"proa_cli/src/rsjs/{name}.rs" for name in MODULES]
    files += [path for _, path, _ in CASES]
    files = sorted(set(files))
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", *files], check=True)
    originals = {name: (root / name).read_bytes() for name in files}
    backup = output / "originals"
    for name, contents in originals.items():
        dest = backup / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(contents)

    metadata = {
        "revision": run(["git", "rev-parse", "HEAD"]),
        "rustc": run(["rustc", "-Vv"]),
        "cargo": run(["cargo", "-V"]),
        "repetitions": repetitions,
        "cases": CASES,
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    rows: list[dict] = []
    target = Path(json.loads(run([
        "cargo", "metadata", "--locked", "--format-version", "1", "--no-deps",
    ]))["target_directory"])

    def restore() -> None:
        for name, contents in originals.items():
            (root / name).write_bytes(contents)

    def cargo(phase: str, mode: str, case: str, iteration: int) -> None:
        command = ["cargo", mode, "--locked", "-p", "proa-cli", "--bins", "--timings"]
        label = f"{phase}-{mode}-{case}-{iteration}"
        started = time.monotonic()
        proc = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True)
        elapsed = time.monotonic() - started
        (output / f"{label}.log").write_text(proc.stdout)
        if proc.returncode:
            raise RuntimeError(f"{label} failed; see {output / (label + '.log')}")
        report = (target / "cargo-timings/cargo-timing.html").read_text()
        match = re.search(r"const UNIT_DATA = (\[.*?\]);", report, re.S)
        if match is None:
            raise RuntimeError("Cargo timing report format changed")
        units = json.loads(match[1])
        row = dict(phase=phase, mode=mode, case=case, iteration=iteration,
                   elapsed=elapsed, units=units)
        rows.append(row)
        (output / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{label}: {elapsed:.3f}s", flush=True)

    def measure(phase: str) -> None:
        for mode in ("build", "check"):
            cargo(phase, mode, "warmup", 0)
            for i in range(repetitions):
                cargo(phase, mode, "noop", i)
            for case, name, needle in CASES:
                path = root / name
                contents = path.read_text()
                if contents.count(needle) != 1:
                    raise RuntimeError(f"Probe anchor changed in {name}")
                try:
                    # Discard the first changed build to warm the item's caches.
                    for i in range(-1, repetitions):
                        path.write_text(contents.replace(
                            needle, needle + f" [compile-probe-{phase}-{i}]"))
                        cargo(phase, mode, case, i)
                finally:
                    path.write_text(contents)

    def extract() -> None:
        spike = output / "rsjs_build_spike"
        (spike / "src").mkdir(parents=True, exist_ok=True)
        manifest = '''[package]
name = "rsjs_build_spike"
version = "0.0.0"
edition = "2021"
publish = false
[dependencies]
object = { workspace = true }
rayon = "1.10"
rsjs_ir = { workspace = true }
rsjs_transport = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
syn = { workspace = true, features = ["full"] }
thiserror = { workspace = true }
[dev-dependencies]
tempfile = { workspace = true }
'''
        (spike / "Cargo.toml").write_text(manifest)
        modules = "".join(
            f"#[path = {json.dumps(str(root / 'proa_cli/src/rsjs' / (m + '.rs')))}]\n"
            f"pub mod {m};\n" for m in MODULES
        )
        (spike / "src/lib.rs").write_text("pub mod rsjs {\n" + modules + "}\n")
        path = root / "Cargo.toml"
        path.write_text(path.read_text().replace(
            "members = [", f"members = [\n    {json.dumps(str(spike))},", 1))
        path = root / "proa_cli/Cargo.toml"
        path.write_text(path.read_text().replace(
            "[dependencies]", "[dependencies]\nrsjs_build_spike = { path = "
            + json.dumps(str(spike)) + " }", 1))
        path = root / "proa_cli/src/rsjs/mod.rs"
        contents = path.read_text()
        for module in MODULES:
            contents = re.sub(
                rf"pub(?:\(crate\))? mod {module};",
                f"pub use rsjs_build_spike::rsjs::{module};", contents)
        path.write_text(contents)
        for module, names in VISIBILITY.items():
            path = root / f"proa_cli/src/rsjs/{module}.rs"
            contents = path.read_text()
            for kind, name in names:
                old = f"pub(crate) {kind} {name}"
                if contents.count(old) != 1:
                    raise RuntimeError(f"Visibility anchor changed: {old}")
                contents = contents.replace(old, f"pub {kind} {name}")
            path.write_text(contents)
        # A module loaded through an explicit path resolves external child
        # modules differently from an ordinary `mod name;` declaration.
        for module in MODULES:
            path = root / f"proa_cli/src/rsjs/{module}.rs"
            contents = path.read_text()
            if "mod tests;" in contents:
                tests = root / f"proa_cli/src/rsjs/{module}/tests.rs"
                contents = contents.replace(
                    "mod tests;", f"#[path = {json.dumps(str(tests))}]\nmod tests;")
                path.write_text(contents)
        # --no-deps metadata does not update Cargo.lock. Resolve just this new
        # path dependency without updating the existing registry dependencies.
        proc = subprocess.run(["cargo", "check", "--offline", "-p", "proa-cli", "--bins"],
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (output / "split-resolve.log").write_text(proc.stdout)
        proc.check_returncode()

    try:
        measure("baseline")
        extract()
        measure("split")
        # Test the extracted engine independently before restoring the workspace.
        proc = subprocess.run(["cargo", "test", "--locked", "-p", "rsjs_build_spike",
                               "--lib"], stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True)
        (output / "split-engine-tests.log").write_text(proc.stdout)
        proc.check_returncode()
        restore()
        measure("restored")
    finally:
        restore()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--output", type=Path,
                        default=Path("target/compile-time-experiment"))
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    # Let finally restore files when a supervised run receives SIGTERM.
    def interrupted(_signum: int, _frame: object) -> None:
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupted)
    experiment(args.repetitions, args.output.resolve())


if __name__ == "__main__":
    main()
