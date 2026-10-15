#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compare two release binaries on one worker, alternating measurement order."""

import argparse
import hashlib
import json
from pathlib import Path
import statistics

from benchmark import runtime


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("artifacts", type=Path, help="shared static/showcase asset directory")
    parser.add_argument("report", type=Path)
    parser.add_argument("--rounds", type=int, choices=range(2, 11), default=3)
    args = parser.parse_args()
    args.report.mkdir(parents=True, exist_ok=False)
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    hashes = {}
    for name, path in binaries.items():
        with path.open("rb") as binary:
            hashes[name] = hashlib.file_digest(binary, "sha256").hexdigest()
    samples = {name: [] for name in binaries}
    for round_index in range(args.rounds):
        order = list(binaries) if round_index % 2 == 0 else list(reversed(binaries))
        for name in order:
            report = args.report / f"round-{round_index + 1}-{name}"
            report.mkdir()
            runtime(binaries[name], args.artifacts.resolve(), report)
            samples[name].append(json.loads((report / "runtime.json").read_text()))
    medians = {
        name: {page: statistics.median(sample[page]["median_ssr_us"] for sample in rounds)
               for page in rounds[0]}
        for name, rounds in samples.items()
    }
    result = {
        "binary_sha256": hashes, "rounds": args.rounds,
        "samples_per_page_per_round": 1000, "median_ssr_us": medians,
        "candidate_change_percent": {
            page: round((value / medians["baseline"][page] - 1) * 100, 2)
            for page, value in medians["candidate"].items()
        },
    }
    (args.report / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
