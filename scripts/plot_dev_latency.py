#!/usr/bin/env python3
"""Render an observed dev rebuild waterfall (requires matplotlib)."""

import argparse
import json
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", type=Path, nargs="+")
    parser.add_argument("--labels", nargs="+")
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
    fig, axes = plt.subplots(1, len(args.inputs), figsize=(8 * len(args.inputs), 5.7), squeeze=False)
    total_max = 0
    for path in args.inputs:
        data = json.loads(path.read_text())
        total_max = max(total_max, max(sample["boundaries"]["response"] for sample in data["samples"]))
    for ax, path, label in zip(axes[0], args.inputs, labels):
        data = json.loads(path.read_text())
        ordered = sorted(data["samples"], key=lambda sample: sample["boundaries"]["response"])
        selected = ordered[len(ordered) // 2]
        b = selected["boundaries"]
        for row, (name, start, end, color) in enumerate(stages):
            duration = b[end] - b[start]
            ax.barh(row, duration, left=b[start], height=0.65, color=color)
            ax.text(b[end] + 0.04, row, f"{duration:.2f}s", va="center", fontsize=10)
            if row > 0:
                ax.plot([b[start], b[start]], [row - 0.65, row], color="#cbd5e1", linewidth=1)
        total = b["response"]
        ax.barh(len(stages), total, height=0.65, color="#0f172a")
        ax.text(total + 0.04, len(stages), f"{total:.2f}s", va="center", fontweight="bold")
        ax.set_yticks(range(len(stages) + 1), [stage[0] for stage in stages] + ["Total"])
        ax.invert_yaxis()
        ax.axvline(1.0, color="#dc2626", linestyle="--", linewidth=1.4)
        ax.text(1.0, -0.65, "1s goal", color="#dc2626", ha="center", fontsize=10)
        ax.set_xlim(0, total_max + 0.65)
        ax.set_xlabel("Seconds since saving a template")
        ax.set_title(label, loc="left", fontsize=16, fontweight="bold", pad=28)
        ax.grid(axis="x", color="#e2e8f0", linewidth=0.6)
        ax.set_axisbelow(True)
        for spine in ax.spines.values():
            spine.set_visible(False)
    fig.text(0.02, 0.02, "24-route standalone app · M4 Max · Rust 1.95 · warm caches · assets skipped\n"
             "Each panel shows the median observed cycle of three. Stages include wrappers/setup.\n"
             "The endpoint is an updated HTTP response; browser paint is not measured.", fontsize=9, color="#475569")
    fig.tight_layout(rect=(0, 0.10, 1, 1))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for extension in ("svg", "png"):
        destination = args.output.with_suffix("." + extension)
        fig.savefig(destination, dpi=160, facecolor="white")
        print(destination)


if __name__ == "__main__":
    main()
