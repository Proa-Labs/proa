#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure real edits with any Cargo or Proa build command in a dedicated worktree.

Run from the repository root. The selected probe inputs must be clean. Their
contents are restored on completion, failure, Ctrl-C, or SIGTERM. Recovery
copies and timing records remain in the output directory after a forced kill.
Do not run concurrent builds or edit the probe inputs during a measurement.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import signal
import statistics
import subprocess
import time
import uuid
from pathlib import Path


PROBES = {
    "noop": None,
    "page": "proa_marketing/src/pages/home.rs",
    "docs-prose": "proa_marketing/src/docs/index.mdx",
    "docs-code": "proa_marketing/src/docs/index.mdx",
    "core": "proa_core/src/shared/write_buf.rs",
    "cli-command": "proa_cli/src/commands/fmt.rs",
    "cli-lint": "proa_source_tools/src/lint/mod.rs",
    "cli-engine": "proa_build/src/pipeline.rs",
}


DEFAULT_CASES = ("noop", "page", "docs-prose", "docs-code", "core")
LEGACY_PROBES = {
    "cli-lint": "proa_cli/src/lint/mod.rs",
    "cli-engine": "proa_cli/src/rsjs/pipeline.rs",
}
CLI_ANCHORS = {
    "cli-command": "Proa formatting check failed",
    "cli-lint": "failed to read {}",
    "cli-engine": "linked Cargo test requires exactly one descriptor-only",
}


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def edited_source(case: str, source: str, nonce: str, iteration: int) -> str:
    marker = f"compile-probe-{nonce}-{iteration}"
    if case == "docs-prose":
        return source + f"\n\nCompile time probe {marker}.\n"
    if case == "docs-code":
        return source + (
            f"\n\n```rust\n// {marker}\n"
            f"fn compile_probe() -> u64 {{ {iteration + 2} }}\n```\n"
        )
    if case in CLI_ANCHORS:
        needle = CLI_ANCHORS[case]
        replacement = f"{needle} [{marker}]"
    elif case == "core":
        needle = 'write!(f, "stream closed")'
        replacement = f'write!(f, "stream closed [{marker}]")'
    elif case == "page":
        needle = "Proa — The next era of web interfaces. Built in Rust."
        replacement = f"{needle} [{marker}]"
    else:
        raise ValueError(f"Unknown edit probe: {case}")
    if source.count(needle) != 1:
        raise ValueError(f"The {case} probe anchor changed")
    return source.replace(needle, replacement)


def benchmark(args: argparse.Namespace) -> None:
    root = Path.cwd()
    if not (root / "proa_core/Cargo.toml").is_file():
        raise ValueError("Run from the Proa repository root")
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        command = ["cargo", "check", "--locked", "-p", "proa_marketing", "--timings"]
    cases = args.case or list(DEFAULT_CASES)
    paths = {case: PROBES[case] for case in cases}
    for case, name in paths.items():
        if name is not None and not (root / name).exists() and case in LEGACY_PROBES:
            paths[case] = LEGACY_PROBES[case]
    output = args.output.resolve() / args.label
    output.mkdir(parents=True, exist_ok=True)
    (output / "summary.json").unlink(missing_ok=True)
    names = sorted({name for name in paths.values() if name is not None})
    if names:
        subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", *names], check=True)
    originals = {name: (root / name).read_bytes() for name in names}
    for name, source in originals.items():
        backup = output / "originals" / name
        backup.parent.mkdir(parents=True, exist_ok=True)
        backup.write_bytes(source)
    nonce = uuid.uuid4().hex
    metadata = {
        "revision": git("rev-parse", "HEAD"),
        "diff": git("diff", "--stat", "HEAD"),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "platform": platform.platform(),
        "logical_cpus": os.cpu_count(),
        "environment": {
            key: os.environ.get(key) for key in (
                "CARGO_TARGET_DIR", "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS",
                "RUSTC_WRAPPER", "RUSTFLAGS",
                "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG",
            )
        },
        "command": command,
        "repetitions": args.repetitions,
        "nonce": nonce,
        "input_sha256": {
            name: hashlib.sha256(source).hexdigest() for name, source in originals.items()
        },
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    results = []
    try:
        for case in cases:
            name = paths[case]
            try:
                # The first run warms the command and the edited item's caches.
                for iteration in range(-1, args.repetitions):
                    if name is not None:
                        (root / name).write_text(edited_source(
                            case, originals[name].decode(), nonce, iteration,
                        ))
                    label = f"{case}-{iteration}"
                    started = time.monotonic()
                    with (output / f"{label}.log").open("w") as log:
                        proc = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
                    elapsed = time.monotonic() - started
                    results.append({
                        "case": case, "iteration": iteration,
                        "elapsed": elapsed, "exit_code": proc.returncode,
                    })
                    (output / "samples.json").write_text(json.dumps(results, indent=2) + "\n")
                    if proc.returncode:
                        raise RuntimeError(f"{label} failed: see {output / (label + '.log')}")
                    print(f"{case} {iteration}: {elapsed:.3f}s", flush=True)
            finally:
                if name is not None:
                    (root / name).write_bytes(originals[name])
    finally:
        for name, source in originals.items():
            (root / name).write_bytes(source)
    summary = []
    for case in cases:
        values = [r["elapsed"] for r in results if r["case"] == case and r["iteration"] >= 0]
        row = {
            "case": case, "median": statistics.median(values),
            "minimum": min(values), "maximum": max(values), "count": len(values),
        }
        summary.append(row)
        print(f"{case}: median {row['median']:.3f}s [{min(values):.3f}, {max(values):.3f}]")
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", action="append", choices=PROBES)
    parser.add_argument("--label", required=True)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--output", type=Path, default=Path("target/compile-time-benchmark"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")

    def interrupted(_signum: int, _frame: object) -> None:
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    benchmark(args)


if __name__ == "__main__":
    main()
