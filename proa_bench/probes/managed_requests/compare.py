#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Build identical Proa benchmark fixtures against local checkouts and compare.

No files in the supplied benchmarks checkout are edited. Builds use a detached
copy, local Cargo patches, the same lockfiles, and separate retained binaries.
Compiler jobs never overlap the alternating timing samples.
"""
import argparse
import hashlib
import json
import shutil
import statistics
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRATES = {
    "proa_core": "proa_core",
    "proa_macros": "proa_macros",
    "proa_stream": "proa_stream",
    "proa_framework_axum": "proa_framework/proa_framework_axum",
    "proa_showcase": "proa_showcase",
    "rsjs": "rsjs",
}
SUITE_BINS = ["representative_suite_bench", "proa_page_server", "proa_suspense_server"]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command, cwd=None, log=None):
    if log:
        with log.open("w") as stream:
            subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, check=True)
    else:
        return subprocess.check_output(command, cwd=cwd, text=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--benchmarks", type=Path, required=True)
    parser.add_argument("--main", type=Path)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--http", action="store_true")
    args = parser.parse_args()
    roots = {"before": args.baseline.resolve(), "fixed": args.candidate.resolve()}
    if args.main:
        roots["main"] = args.main.resolve()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=False)
    (work / "bin").mkdir()
    bench = work / "benchmarks"
    execute(["git", "-C", str(args.benchmarks.resolve()), "worktree", "add", "--detach", str(bench), "HEAD"])
    for label, root in roots.items():
        patch = '[patch."https://github.com/Proa-Labs/proa"]\n'
        patch += "".join(name + " = { path = " + json.dumps(str(root / rel)) + " }\n" for name, rel in CRATES.items())
        (work / (label + "-patch.toml")).write_text(patch)
    # Frozen linked resolutions from the captured comparison, shared by all builds.
    shutil.copy2(HERE / "suite.lock", bench / "Cargo.lock")
    suite_locks = {}
    for label in roots:
        shutil.copy2(HERE / "suite.lock", bench / "Cargo.lock")
        if label == "main":
            execute(["git", "apply", "--unidiff-zero", str(HERE / "main-suite.patch")], bench)
        command = ["cargo", "build", "--release", "--locked", "--features", "topcoat,http-load", "--config", str(work / (label + "-patch.toml")), "--target-dir", str(work / "suite-target")]
        for binary in SUITE_BINS:
            command += ["--bin", binary]
        execute(command, bench, work / (label + "-suite-build.log"))
        suite_locks[label] = digest(bench / "Cargo.lock")
        for binary in SUITE_BINS:
            shutil.copy2(work / "suite-target/release" / binary, work / "bin" / (label + "-" + binary))

    probe = work / "request-probe"
    (probe / "src").mkdir(parents=True)
    shutil.copy2(HERE / "request.rs", probe / "src/main.rs")
    shutil.copy2(HERE / "request.toml", probe / "Cargo.toml")
    shutil.copy2(HERE / "request.lock", probe / "Cargo.lock")
    for label, source in [("before", "before"), ("fixed", "fixed"), ("bulk", "fixed")]:
        command = ["cargo", "build", "--release", "--locked", "--manifest-path", str(probe / "Cargo.toml"), "--config", str(work / (source + "-patch.toml")), "--target-dir", str(work / "request-target")]
        if label == "bulk":
            command += ["--features", "bulk"]
        execute(command, log=work / (label + "-request-build.log"))
        shutil.copy2(work / "request-target/release/proa-request-perf", work / "bin" / (label + "-final-request"))
    for label in ["before", "fixed"]:
        execute(["cargo", "build", "-p", "proa_core", "-p", "proa_macros", "-p", "proa_stream", "--release", "--locked"], roots[label], work / (label + "-core-build.log"))

    provenance = {"compiler": execute(["rustc", "-Vv"]), "request_source_sha256": digest(HERE / "request.rs"), "benchmark_revision": execute(["git", "rev-parse", "HEAD"], bench).strip(), "roots": {}}
    for label, root in roots.items():
        provenance["roots"][label] = {"revision": execute(["git", "rev-parse", "HEAD"], root).strip(), "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"], cwd=root)).hexdigest()}
    provenance["binaries"] = {p.name: digest(p) for p in (work / "bin").iterdir()}
    provenance["suite_lock_sha256_by_control"] = suite_locks
    provenance["request_lock_sha256"] = digest(probe / "Cargo.lock")
    (work / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")

    requests = {}
    for case in ["empty_raw", "empty_spec", "dynamic_spec", "managed"]:
        samples = {label: [] for label in (["before", "fixed", "bulk"] if case == "managed" else ["before", "fixed"])}
        for repeat in range(15):
            order = list(samples)
            if repeat % 2:
                order.reverse()
            for label in order:
                samples[label].append(json.loads(execute([str(work / "bin" / (label + "-final-request")), case, "100000"])))
        assert len({s["sha256"] for batch in samples.values() for s in batch}) == 1, "request output parity"
        requests[case] = samples
        (work / "requests.json").write_text(json.dumps(requests, indent=2) + "\n")
        print(case, {label: statistics.median(s["ns_per_request"] for s in batch) for label, batch in samples.items()}, flush=True)

    rendering = {}
    labels = ["main", "before", "fixed"] if args.main else ["before", "fixed"]
    for workload in ["cards", "products", "table"]:
        hashes, samples = {}, {label: [] for label in labels}
        for label in labels:
            dump = work / (label + "-" + workload + ".html")
            execute([str(work / "bin" / (label + "-representative_suite_bench")), "--framework", "proa", "--workload", workload, "--dump", str(dump), "--dump-only"])
            hashes[label] = digest(dump)
        assert len(set(hashes.values())) == 1, "canonical output parity"
        for repeat in range(9):
            for label in (labels if repeat % 2 == 0 else list(reversed(labels))):
                output = work / "render-sample.json"
                execute([str(work / "bin" / (label + "-representative_suite_bench")), "--framework", "proa", "--workload", workload, "--warmup", "500", "--iterations", "3000", "--out", str(output)])
                samples[label].append(json.loads(output.read_text()))
        rendering[workload] = {"sha256": hashes, "samples": samples}
        (work / "rendering.json").write_text(json.dumps(rendering, indent=2) + "\n")
        print(workload, {label: statistics.median(s["p50_us"] for s in batch) for label, batch in samples.items()}, flush=True)
    execute(["python3", str(roots["fixed"] / "proa_bench/probes/compare_head.py"), str(roots["before"]), str(roots["fixed"]), "--output", str(work / "native.json"), "--timing-samples", "19"], log=work / "native.log")
    if args.http:
        execute(["node", str(HERE / "http.mjs"), str(work), str(args.benchmarks.resolve())], log=work / "http.log")
    print("Raw results and binaries retained at", work)


if __name__ == "__main__":
    main()
