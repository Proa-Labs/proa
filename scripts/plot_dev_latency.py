#!/usr/bin/env python3
"""Render an observed dev rebuild waterfall (requires matplotlib)."""

import argparse
import json
import re
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", type=Path, nargs="+")
    parser.add_argument("--labels", nargs="+")
    parser.add_argument("--detailed", action="store_true", help="Expand cache/setup using PROA_BUILD_TIMINGS events")
    parser.add_argument("--output", type=Path, required=True, help="Output prefix, without an extension")
    args = parser.parse_args()
    labels = args.labels or [path.stem for path in args.inputs]
    if len(labels) != len(args.inputs):
        parser.error("Supply one label per input")
    stages = [
        ("Watch + debounce", "save", "detected", "#64748b"),
        ("Prepare build", "detected", "prepared", "#94a3b8"),
        ("Descriptor Cargo", "prepared", "descriptor_finished", "#3b82f6"),
        ("Cache, link + final setup", "descriptor_finished", "final_compiling", "#e97829"),
        ("Final compile + link", "final_compiling", "final_finished", "#2563eb"),
        ("Validate + snapshot", "final_finished", "validated", "#a78bfa"),
        ("Restart + HTTP response", "validated", "response", "#10b981"),
    ]
    fig, axes = plt.subplots(1, len(args.inputs), figsize=(8 * len(args.inputs), 10 if args.detailed else 5.7), squeeze=False)
    total_max = 0
    for path in args.inputs:
        data = json.loads(path.read_text())
        total_max = max(total_max, max(sample["boundaries"]["response"] for sample in data["samples"]))
    for ax, path, label in zip(axes[0], args.inputs, labels):
        data = json.loads(path.read_text())
        ordered = sorted(data["samples"], key=lambda sample: sample["boundaries"]["response"])
        selected = ordered[len(ordered) // 2]
        b = selected["boundaries"]
        selected_stages = stages
        if args.detailed:
            detected = [event for event in data["events"] if "change stabilized; rebuilding" in event["line"]]
            start = detected[selected["sample"]]["time"] - b["detected"]
            for event in data["events"]:
                timing = re.fullmatch(r"rsjs timing: (.*): ([0-9.]+) ms", event["line"])
                if timing and start <= event["time"] <= start + b["response"]:
                    b[timing[1]] = event["time"] - start
            details = [
                ("Artifact indexes", "cache artifact indexes"),
                ("Publish rebuilt objects", "cache persist rebuilt objects"),
                ("Load + hash objects", "cache load and hash objects"),
                ("Validate sources + externs", "cache validate observed inputs and externs"),
                ("Validate artifact joins", "cache validate artifact joins"),
                ("Publish indexes + cleanup", "cache publish indexes and cleanup"),
                ("Transport manifest", "transport manifest and root selection"),
                ("Transport graph", "transport graph"),
                ("Final-unit selection", "transport final-unit selection"),
                ("ABI adaptation", "ABI adaptation"),
                ("Semantic planning", "semantic planning"),
                ("Plan publication", "semantic plan publication"),
                ("Final unit map", "final request validation and unit map"),
                ("Final Cargo setup", "final Cargo setup"),
            ]
            missing = [key for _, key in details if key not in b]
            if missing:
                parser.error(f"{path} lacks timing events: {missing}")
            selected_stages = stages[:3]
            previous = "descriptor_finished"
            for name, key in details:
                selected_stages.append((name, previous, key, "#e97829"))
                previous = key
            # Cargo startup/wrapper validation occurs before its first Compiling line.
            selected_stages.append(("Cargo startup + wrapper", previous, "final_compiling", "#f59e0b"))
            selected_stages.extend(stages[4:])
        if "hot_published" in b:
            selected_stages = [
                ("Watch, validate + publish", "save", "hot_published", "#64748b"),
                ("Changed HTTP response", "hot_published", "response", "#10b981"),
            ]
        for row, (name, start, end, color) in enumerate(selected_stages):
            duration = b[end] - b[start]
            ax.barh(row, duration, left=b[start], height=0.65, color=color)
            ax.text(b[end] + 0.04, row, f"{duration:.2f}s", va="center", fontsize=10)
            if row > 0:
                ax.plot([b[start], b[start]], [row - 0.65, row], color="#cbd5e1", linewidth=1)
        total = b["response"]
        ax.barh(len(selected_stages), total, height=0.65, color="#0f172a")
        ax.text(total + 0.04, len(selected_stages), f"{total:.2f}s", va="center", fontweight="bold")
        ax.set_yticks(range(len(selected_stages) + 1), [stage[0] for stage in selected_stages] + ["Total"])
        ax.invert_yaxis()
        ax.axvspan(2.0, 3.0, color="#16a34a", alpha=0.09)
        ax.axvline(3.0, color="#16a34a", linestyle="--", linewidth=1.4)
        ax.text(2.5, -0.65, "2–3s target", color="#15803d", ha="center", fontsize=10)
        ax.set_xlim(0, total_max + 0.65)
        ax.set_xlabel("Seconds since saving a template")
        ax.set_title(label, loc="left", fontsize=16, fontweight="bold", pad=28)
        ax.grid(axis="x", color="#e2e8f0", linewidth=0.6)
        ax.set_axisbelow(True)
        for spine in ax.spines.values():
            spine.set_visible(False)
    fig.text(0.02, 0.02, "M4 Max · Rust 1.95 · warm caches · assets skipped\n"
             "Each panel shows the median observed cycle of three. Stages include wrappers/setup.\n"
             "The endpoint is an updated HTTP response; browser paint is not measured.", fontsize=9, color="#475569")
    fig.tight_layout(rect=(0, 0.10, 1, 1))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for extension in ("svg", "png"):
        destination = args.output.with_suffix("." + extension)
        fig.savefig(destination, dpi=160, facecolor="white")
        if extension == "svg":
            # Matplotlib emits insignificant spaces at path-data line ends.
            destination.write_text("\n".join(line.rstrip() for line in destination.read_text().splitlines()) + "\n")
        print(destination)


if __name__ == "__main__":
    main()
