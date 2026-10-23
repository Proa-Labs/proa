#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compare the precomputed main implementation with request-local metadata.

Run from any directory. Startup setup is excluded in both versions. The HTTP
case exercises each revision's normal framework integration; head_only gives
the old implementation its best case: an immutable, already resolved snapshot.
Both render the same document. Allocation counts and context/future
sizes are printed alongside Criterion timings. Requires Cargo's dependencies.
"""

import argparse
import pathlib
import shutil
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(*args, cwd=ROOT):
    subprocess.run(args, cwd=cwd, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-ref", default="ae1f3b2")
    args = parser.parse_args()
    revision = subprocess.check_output(
        ["git", "rev-parse", "--verify", args.baseline_ref + "^{commit}"],
        cwd=ROOT, text=True,
    ).strip()
    target = ROOT / "target"
    # Same-version crates in different worktrees must not overwrite each
    # other's build artifacts. Only Criterion's measurements are shared.
    baseline_target = target / "metadata-static-main-build"
    with tempfile.TemporaryDirectory(prefix="proa-static-metadata-") as directory:
        baseline = pathlib.Path(directory) / "main"
        run("git", "worktree", "add", "--detach", str(baseline), revision)
        manifest = baseline / "proa_bench/Cargo.toml"
        lock = baseline / "Cargo.lock"
        originals = {path: path.read_bytes() for path in [manifest, lock]}
        created = []
        try:
            for name in ["benches/metadata_static.rs", "fixtures/metadata_static_legacy.rs.txt"]:
                destination = baseline / "proa_bench" / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / "proa_bench" / name, destination)
                created.append(destination)
            harness = created[0]
            harness.write_text(harness.read_text().replace(
                "metadata_static_current.rs.txt", "metadata_static_legacy.rs.txt"))
            manifest.write_text(manifest.read_text().replace(
                "[dependencies]", "[dependencies]\nproa_framework_core = { workspace = true }")
                + '\n[[bench]]\nname = "metadata_static"\nharness = false\n')
            run("cargo", "bench", "--target-dir", str(baseline_target), "-p", "proa_bench",
                "--bench", "metadata_static", "--", "--noplot", "--save-baseline",
                "metadata-static-main", cwd=baseline)
        finally:
            for path, original in originals.items():
                path.write_bytes(original)
            for path in created:
                path.unlink()
            fixtures = baseline / "proa_bench/fixtures"
            if fixtures.is_dir() and not any(fixtures.iterdir()):
                fixtures.rmdir()
            run("git", "worktree", "remove", str(baseline))
    for source in (baseline_target / "criterion/static_metadata").glob("*/metadata-static-main"):
        destination = target / "criterion/static_metadata" / source.parent.name / source.name
        shutil.copytree(source, destination, dirs_exist_ok=True)
    run("cargo", "bench", "--locked", "--target-dir", str(target), "-p", "proa_bench",
        "--bench", "metadata_static", "--", "--noplot", "--baseline", "metadata-static-main")


if __name__ == "__main__":
    main()
