#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compile identical probes against two already-built release trees; retain evidence."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("baseline", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--timing-samples", type=int, default=9)
args = parser.parse_args()
probes = Path(__file__).resolve().parent
results = {"compiler": subprocess.check_output(["rustc", "-Vv"], text=True), "sources": {}}
with tempfile.TemporaryDirectory(prefix="proa-head-perf-") as directory:
    binaries = {}
    for name in ["async_boundary_storage", "suspense_storage_body", "head_free_components", "head_free_send"]:
        source = probes / f"{name}.rs"
        results["sources"][source.name] = hashlib.sha256(source.read_bytes()).hexdigest()
        for label, root in [("baseline", args.baseline), ("candidate", args.candidate)]:
            root = root.resolve()
            target = root / "target/release"
            command = ["rustc", "--edition=2021", "-O", "-C", "lto=thin", "-C", "codegen-units=1", "-L", f"dependency={target}/deps"]
            for crate in ["proa_core", "proa_stream"]:
                command += ["--extern", f"{crate}={target}/lib{crate}.rlib"]
            if name.startswith("head_free_"):
                dynamic = next(p for p in target.glob("libproa_macros.*") if p.suffix in (".dylib", ".so", ".dll"))
                command += ["--extern", f"proa_macros={dynamic}"]
            binary = Path(directory) / f"{label}-{name}"
            subprocess.run(command + [str(source), "-o", str(binary)], check=True)
            binaries[label, name] = binary
            entry = results.setdefault(label, {})
            libraries = [target/"libproa_core.rlib",target/"libproa_stream.rlib"]
            libraries.extend(p for p in target.glob("libproa_macros.*") if p.suffix in (".dylib", ".so", ".dll"))
            entry["artifacts_sha256"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in libraries}
            entry["dirty_diff_sha256"] = hashlib.sha256(subprocess.check_output(["git", "-C", str(root), "diff", "HEAD"])).hexdigest()
            entry["revision"] = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
            entry[name] = subprocess.check_output([str(binary)], text=True)
    samples = {label: {str(i): [] for i in range(4)} for label in ("baseline", "candidate")}
    for sample in range(args.timing_samples):
        order = ["baseline", "candidate"] if sample % 2 == 0 else ["candidate", "baseline"]
        for label in order:
            output = subprocess.check_output([str(binaries[label, "head_free_components"]), "--timing"], text=True)
            for line in output.splitlines():
                fields = dict(part.split("=") for part in line.split())
                samples[label][fields["case"]].append(float(fields["ns_per_render"]))
    results["latency_ns"] = samples
    results["latency_medians_ns"] = {label: {case: statistics.median(values) for case, values in cases.items()} for label, cases in samples.items()} if args.timing_samples else {}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(results, indent=2)+"\n")
print(json.dumps({k: results[k] for k in ("baseline", "candidate", "latency_medians_ns")}, indent=2))
