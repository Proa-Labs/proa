#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compare downloaded benchmark directories, including runtime regressions."""

import argparse
import json
from pathlib import Path
import re


def summarize(directory):
    stages = [json.loads(line) for line in (directory / "stages.jsonl").read_text().splitlines()]
    if any(stage["exit_code"] for stage in stages):
        raise ValueError(f"{directory}: benchmark contains a failed build")
    builds = {}
    for label in ("cold", "noop", "link-change"):
        selected = [stage for stage in stages if stage["stage"].startswith(label + "-")]
        if len(selected) != 4:
            raise ValueError(f"{directory}: incomplete {label} benchmark")
        log = (directory / f"{label}-linked.log").read_text()
        phases = []
        for minutes, seconds in re.findall(r"Finished .*?profile.*? in (?:(\d+)m )?(\d+(?:\.\d+)?)s", log):
            phases.append(round(int(minutes or 0) * 60 + float(seconds), 3))
        if len(phases) != 2:
            raise ValueError(f"{directory}: expected completed descriptor and final phases for {label}")
        reuse = re.search(r"descriptor (\d+)/(\d+) Cargo artifacts reused; final (\d+)/(\d+) reused", log)
        if reuse is None:
            raise ValueError(f"{directory}: missing artifact reuse report for {label}")
        resources = dict(line.strip().split(": ", 1) for line in
                         (directory / f"{label}-linked.resources").read_text().splitlines() if ": " in line)
        builds[label] = {"seconds": round(sum(stage["seconds"] for stage in selected), 3),
                         "descriptor_seconds": phases[0], "final_seconds": phases[1],
                         "descriptor_reuse": f"{reuse[1]}/{reuse[2]}",
                         "final_reuse": f"{reuse[3]}/{reuse[4]}",
                         "peak_rss_mib": round(float(resources["Maximum resident set size (kbytes)"]) / 1024, 1),
                         "cpu_seconds": round(float(resources["User time (seconds)"])
                                              + float(resources["System time (seconds)"]), 3)}
    return {"directory": str(directory), "builds": builds,
            "artifact": json.loads((directory / "link-change-build.json").read_text()),
            "runtime": json.loads((directory / "runtime.json").read_text())}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidates", type=Path, nargs="+")
    args = parser.parse_args()
    baseline = summarize(args.baseline)
    candidates = []
    for path in args.candidates:
        candidate = summarize(path)
        candidate["median_ssr_change_percent"] = {
            page: round((sample["median_ssr_us"] / baseline["runtime"][page]["median_ssr_us"] - 1) * 100, 2)
            for page, sample in candidate["runtime"].items()
        }
        candidates.append(candidate)
    print(json.dumps({"baseline": baseline, "candidates": candidates}, indent=2))
