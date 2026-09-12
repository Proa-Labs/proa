#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Compare Criterion base/head results and emit observable Polars reports."""

from __future__ import annotations

import argparse
import hashlib
import json
import logging
import math
import os
import pathlib
import sys
import tomllib
from typing import Any

import polars as pl


LOG = logging.getLogger("proa.perf")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", type=pathlib.Path, required=True)
    parser.add_argument("--head", type=pathlib.Path, required=True)
    parser.add_argument("--base-fixture", type=pathlib.Path, required=True)
    parser.add_argument("--head-fixture", type=pathlib.Path, required=True)
    parser.add_argument("--base-config", type=pathlib.Path, required=True)
    parser.add_argument("--config", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--base-sha", default="unknown")
    parser.add_argument("--head-sha", default="unknown")
    return parser.parse_args()


def throughput_bytes(payload: dict[str, Any]) -> int | None:
    throughput = payload.get("throughput")
    if not isinstance(throughput, dict):
        return None
    value = throughput.get("Bytes")
    return int(value) if value is not None else None


def benchmark_id(metadata: dict[str, Any], path: pathlib.Path) -> str:
    full_id = metadata.get("full_id")
    if isinstance(full_id, str) and full_id:
        return full_id
    # Criterion writes <group>/<function>/new/{benchmark,estimates,sample}.json.
    return "/".join(path.parts[-4:-2])


def round_id(root: pathlib.Path, path: pathlib.Path) -> str:
    relative = path.relative_to(root)
    return relative.parts[0] if relative.parts[0].startswith("round-") else "round-1"


def typical_estimate(estimates: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    # Criterion describes slope as the typical value for linear sampling. Flat
    # sampling does not emit it, so mean is the documented compatible fallback.
    for name in ("slope", "mean"):
        estimate = estimates.get(name)
        if isinstance(estimate, dict):
            point = float(estimate["point_estimate"])
            if math.isfinite(point) and point > 0:
                return name, estimate
    raise ValueError("Criterion result has neither a valid slope nor mean estimate")


def load_criterion(root: pathlib.Path, revision: str, sha: str) -> pl.DataFrame:
    rows: list[dict[str, Any]] = []
    for estimates_path in sorted(root.glob("**/new/estimates.json")):
        benchmark_path = estimates_path.with_name("benchmark.json")
        if not benchmark_path.is_file():
            raise FileNotFoundError(f"missing Criterion metadata: {benchmark_path}")

        estimates = json.loads(estimates_path.read_text())
        metadata = json.loads(benchmark_path.read_text())
        estimator, estimate = typical_estimate(estimates)
        confidence = estimate["confidence_interval"]
        row = {
            "revision": revision,
            "sha": sha,
            "round": round_id(root, estimates_path),
            "benchmark": benchmark_id(metadata, estimates_path),
            "typical_estimator": estimator,
            "typical_ns": float(estimate["point_estimate"]),
            "std_error_ns": float(estimate["standard_error"]),
            "ci_low_ns": float(confidence["lower_bound"]),
            "ci_high_ns": float(confidence["upper_bound"]),
            "output_bytes": throughput_bytes(metadata),
        }
        rows.append(row)
        LOG.info("criterion_observation %s", json.dumps(row, sort_keys=True))

    if not rows:
        raise RuntimeError(f"no Criterion estimates found below {root}")
    return pl.DataFrame(rows)


def load_samples(root: pathlib.Path, revision: str, sha: str) -> pl.DataFrame:
    rows: list[dict[str, Any]] = []
    for sample_path in sorted(root.glob("**/new/sample.json")):
        benchmark_path = sample_path.with_name("benchmark.json")
        if not benchmark_path.is_file():
            raise FileNotFoundError(f"missing Criterion metadata: {benchmark_path}")
        sample = json.loads(sample_path.read_text())
        metadata = json.loads(benchmark_path.read_text())
        iterations = sample.get("iters", [])
        elapsed = sample.get("times", [])
        if not iterations or len(iterations) != len(elapsed):
            raise ValueError(f"invalid Criterion sample arrays: {sample_path}")
        for index, (iteration_count, elapsed_ns) in enumerate(
            zip(iterations, elapsed, strict=True)
        ):
            iteration_count = float(iteration_count)
            elapsed_ns = float(elapsed_ns)
            if iteration_count <= 0 or elapsed_ns < 0:
                raise ValueError(f"invalid Criterion sample value: {sample_path}")
            rows.append(
                {
                    "revision": revision,
                    "sha": sha,
                    "round": round_id(root, sample_path),
                    "benchmark": benchmark_id(metadata, sample_path),
                    "sample_index": index,
                    "iterations": iteration_count,
                    "elapsed_ns": elapsed_ns,
                    "latency_ns": elapsed_ns / iteration_count,
                }
            )

    if not rows:
        raise RuntimeError(f"no Criterion samples found below {root}")
    frame = pl.DataFrame(rows)
    LOG.info(
        "criterion_samples revisions=%s rows=%d benchmarks=%d",
        revision,
        frame.height,
        frame["benchmark"].n_unique(),
    )
    return frame


def fixture_metadata(path: pathlib.Path, revision: str, sha: str) -> dict[str, Any]:
    if not path.is_file():
        raise FileNotFoundError(f"missing exact rendered fixture: {path}")
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as fixture:
        while chunk := fixture.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    row = {
        "revision": revision,
        "sha": sha,
        "fixture_path": str(path),
        "fixture_size_bytes": size,
        "fixture_sha256": digest.hexdigest(),
    }
    LOG.info("fixture_identity %s", json.dumps(row, sort_keys=True))
    return row


def load_config(path: pathlib.Path) -> pl.DataFrame:
    payload = tomllib.loads(path.read_text())
    if payload.get("schema_version") != 1:
        raise ValueError("unsupported performance benchmark schema")
    if payload.get("baseline_state_epoch") != 1:
        raise ValueError("unsupported or missing performance baseline state epoch")
    rows = payload.get("benchmarks", [])
    if not rows:
        raise ValueError("performance benchmark config contains no rows")
    frame = pl.DataFrame(rows)
    required = {
        "id",
        "render",
        "memcpy",
        "fixture_version",
        "approved_rebaseline_version",
        "rebaseline_max_render_to_memcpy",
        "warn_percent",
        "fail_percent",
        "output_policy",
        "expected_rounds",
        "minimum_sample_count",
    }
    missing = sorted(required.difference(frame.columns))
    if missing:
        raise ValueError(f"performance benchmark config is missing: {', '.join(missing)}")
    if frame["id"].n_unique() != frame.height:
        raise ValueError("performance benchmark ids must be unique")
    if frame.filter(pl.col("output_policy") != "exact").height:
        raise ValueError("performance fixture output_policy must be 'exact'")
    if frame.filter(pl.col("fixture_version") <= 0).height:
        raise ValueError("performance fixture_version must be a positive integer")
    if frame.filter(
        (pl.col("approved_rebaseline_version") < pl.col("fixture_version"))
        | (
            pl.col("approved_rebaseline_version")
            > pl.col("fixture_version") + 1
        )
    ).height:
        raise ValueError(
            "approved_rebaseline_version must equal fixture_version or its next version"
        )
    if frame.filter(
        ~pl.col("rebaseline_max_render_to_memcpy").is_finite()
        | (pl.col("rebaseline_max_render_to_memcpy") <= 0)
    ).height:
        raise ValueError(
            "rebaseline_max_render_to_memcpy must be a positive finite ratio"
        )
    if frame.filter(
        (pl.col("warn_percent") <= 0)
        | (pl.col("fail_percent") <= pl.col("warn_percent"))
    ).height:
        raise ValueError("performance thresholds must satisfy 0 < warn_percent < fail_percent")
    if frame.filter(
        (pl.col("expected_rounds") <= 0)
        | (pl.col("minimum_sample_count") < pl.col("expected_rounds"))
    ).height:
        raise ValueError(
            "performance completeness must use positive rounds and at least one sample per round"
        )
    return frame


def comparison_config(base: pl.DataFrame, head: pl.DataFrame) -> pl.DataFrame:
    """Use trusted-base policy while admitting an explicit head fixture bump.

    A candidate must not be able to raise its own thresholds, rename the timed
    rows, or delete a required benchmark to pass the gate. The base contract is
    therefore authoritative for every policy field. Only ``fixture_version``
    comes from the head, where an increase makes intentional output changes
    visible rather than silently comparing different pages.
    """

    base_ids = set(base["id"].to_list())
    head_ids = set(head["id"].to_list())
    if base_ids != head_ids:
        missing = sorted(base_ids - head_ids)
        unexpected = sorted(head_ids - base_ids)
        raise ValueError(
            "candidate performance benchmark IDs differ from trusted base "
            f"(missing={missing}, unexpected={unexpected})"
        )

    head_versions = head.select(
        "id", pl.col("fixture_version").alias("head_fixture_version")
    )
    return base.rename({"fixture_version": "base_fixture_version"}).join(
        head_versions, on="id", how="left", validate="1:1"
    )


def summarized_observations(
    observations: pl.DataFrame, samples: pl.DataFrame, revision: str
) -> pl.DataFrame:
    estimates = (
        observations.filter(pl.col("revision") == revision)
        .group_by("benchmark")
        .agg(
            pl.col("typical_ns").median(),
            pl.col("std_error_ns").median(),
            # Use the outer confidence bounds across rounds. This is
            # deliberately conservative when deciding intervals are separated.
            pl.col("ci_low_ns").min(),
            pl.col("ci_high_ns").max(),
            pl.col("output_bytes").drop_nulls().first(),
            pl.col("output_bytes").drop_nulls().n_unique().alias("output_variants"),
            pl.col("round").n_unique().alias("rounds"),
            pl.col("typical_estimator").unique().sort().str.join("+").alias(
                "typical_estimators"
            ),
        )
    )
    distribution = (
        samples.filter(pl.col("revision") == revision)
        .group_by("benchmark")
        .agg(
            pl.len().alias("sample_count"),
            pl.col("latency_ns").quantile(0.50, interpolation="linear").alias(
                "sample_p50_ns"
            ),
            pl.col("latency_ns").quantile(0.95, interpolation="linear").alias(
                "sample_p95_ns"
            ),
            pl.col("latency_ns").quantile(0.99, interpolation="linear").alias(
                "sample_p99_ns"
            ),
        )
    )
    return estimates.join(distribution, on="benchmark", how="left")


def prefixed_observations(
    observations: pl.DataFrame,
    samples: pl.DataFrame,
    revision: str,
    benchmark_column: str,
    prefix: str,
) -> pl.DataFrame:
    summary = summarized_observations(observations, samples, revision)
    return summary.select(
        pl.col("benchmark").alias(benchmark_column),
        *[
            pl.col(column).alias(f"{prefix}_{column}")
            for column in summary.columns
            if column != "benchmark"
        ],
    )


def compare(
    observations: pl.DataFrame,
    samples: pl.DataFrame,
    config: pl.DataFrame,
    fixtures: pl.DataFrame,
) -> pl.DataFrame:
    result = config
    for revision in ("base", "head"):
        result = result.join(
            prefixed_observations(
                observations, samples, revision, "render", f"{revision}_render"
            ),
            on="render",
            how="left",
        )
        result = result.join(
            prefixed_observations(
                observations, samples, revision, "memcpy", f"{revision}_memcpy"
            ),
            on="memcpy",
            how="left",
        )
        fixture = fixtures.filter(pl.col("revision") == revision).row(
            0, named=True
        )
        result = result.with_columns(
            pl.lit(fixture["fixture_sha256"]).alias(
                f"{revision}_fixture_sha256"
            ),
            pl.lit(fixture["fixture_size_bytes"]).alias(
                f"{revision}_fixture_size_bytes"
            ),
        )

    result = result.with_columns(
        (
            (pl.col("head_render_typical_ns") / pl.col("base_render_typical_ns") - 1.0)
            * 100.0
        ).alias("latency_change_percent"),
        (pl.col("base_render_typical_ns") / pl.col("base_memcpy_typical_ns")).alias(
            "base_render_to_memcpy"
        ),
        (pl.col("head_render_typical_ns") / pl.col("head_memcpy_typical_ns")).alias(
            "head_render_to_memcpy"
        ),
        (pl.col("head_render_typical_ns") - pl.col("head_memcpy_typical_ns")).alias(
            "head_overhead_ns"
        ),
        (1_000_000_000.0 / pl.col("head_render_typical_ns")).alias(
            "actual_renders_per_second"
        ),
        (1_000_000_000.0 / pl.col("head_memcpy_typical_ns")).alias(
            "ideal_renders_per_second"
        ),
        (
            100.0
            * pl.col("head_memcpy_typical_ns")
            / pl.col("head_render_typical_ns")
        ).alias("efficiency_percent"),
        (
            pl.col("head_render_ci_low_ns") > pl.col("base_render_ci_high_ns")
        ).alias("confidence_intervals_separated"),
        (
            (pl.col("output_policy") != pl.lit("exact"))
            | (
                (pl.col("base_fixture_size_bytes") > 0)
                & (
                    pl.col("base_fixture_size_bytes")
                    == pl.col("head_fixture_size_bytes")
                )
                & (
                    pl.col("base_fixture_sha256") == pl.col("head_fixture_sha256")
                )
                & (pl.col("base_render_output_variants") == 1)
                & (pl.col("head_render_output_variants") == 1)
                & (pl.col("base_memcpy_output_variants") == 1)
                & (pl.col("head_memcpy_output_variants") == 1)
                & (
                    pl.col("base_render_output_bytes")
                    == pl.col("base_fixture_size_bytes")
                )
                & (
                    pl.col("head_render_output_bytes")
                    == pl.col("head_fixture_size_bytes")
                )
                & (
                    pl.col("base_memcpy_output_bytes")
                    == pl.col("base_fixture_size_bytes")
                )
                & (
                    pl.col("head_memcpy_output_bytes")
                    == pl.col("head_fixture_size_bytes")
                )
            )
        ).alias("output_compatible"),
        (
            (pl.col("head_fixture_version") > pl.col("base_fixture_version"))
            & (pl.col("base_fixture_version") > 0)
        ).alias("fixture_version_bumped"),
        (
            (pl.col("base_fixture_size_bytes") != pl.col("head_fixture_size_bytes"))
            | (pl.col("base_fixture_sha256") != pl.col("head_fixture_sha256"))
        ).alias("fixture_identity_changed"),
        (
            pl.col("base_fixture_version").is_not_null()
            & (pl.col("base_fixture_version") > 0)
            & (pl.col("head_fixture_version") > 0)
            & (pl.col("head_fixture_version") >= pl.col("base_fixture_version"))
        ).alias("fixture_version_valid"),
        (
            (pl.col("head_fixture_version") > pl.col("base_fixture_version"))
            & (
                pl.col("head_fixture_version")
                == pl.col("approved_rebaseline_version")
            )
            & (
                pl.col("approved_rebaseline_version")
                == pl.col("base_fixture_version") + 1
            )
        ).alias("fixture_rebaseline_approved"),
    ).with_columns(
        (
            (pl.col("head_render_to_memcpy") / pl.col("base_render_to_memcpy") - 1.0)
            * 100.0
        ).alias("ratio_change_percent")
    ).with_columns(
        (
            pl.col("fixture_rebaseline_approved")
            & pl.col("fixture_identity_changed")
            & (
                pl.col("head_render_to_memcpy")
                > pl.col("rebaseline_max_render_to_memcpy")
            )
        ).alias("rebaseline_budget_exceeded")
    )

    missing_render = pl.col("base_render_typical_ns").is_null() | pl.col(
        "head_render_typical_ns"
    ).is_null()
    missing_floor = pl.col("base_memcpy_typical_ns").is_null() | pl.col(
        "head_memcpy_typical_ns"
    ).is_null()
    incomplete_measurement = (
        pl.col("base_render_sample_count").is_null()
        | pl.col("head_render_sample_count").is_null()
        | pl.col("base_memcpy_sample_count").is_null()
        | pl.col("head_memcpy_sample_count").is_null()
        | pl.col("base_render_rounds").is_null()
        | pl.col("head_render_rounds").is_null()
        | pl.col("base_memcpy_rounds").is_null()
        | pl.col("head_memcpy_rounds").is_null()
        | (pl.col("base_render_rounds") != pl.col("expected_rounds"))
        | (pl.col("head_render_rounds") != pl.col("expected_rounds"))
        | (pl.col("base_memcpy_rounds") != pl.col("expected_rounds"))
        | (pl.col("head_memcpy_rounds") != pl.col("expected_rounds"))
        | (
            pl.col("base_render_sample_count")
            < pl.col("minimum_sample_count")
        )
        | (
            pl.col("head_render_sample_count")
            < pl.col("minimum_sample_count")
        )
        | (
            pl.col("base_memcpy_sample_count")
            < pl.col("minimum_sample_count")
        )
        | (
            pl.col("head_memcpy_sample_count")
            < pl.col("minimum_sample_count")
        )
    )
    substantial = (
        (pl.col("latency_change_percent") >= pl.col("fail_percent"))
        & (pl.col("ratio_change_percent") >= pl.col("fail_percent"))
        & pl.col("confidence_intervals_separated")
    )
    warning = (pl.col("latency_change_percent") >= pl.col("warn_percent")) | (
        pl.col("ratio_change_percent") >= pl.col("warn_percent")
    )

    return result.with_columns(
        pl.when(missing_render | missing_floor | incomplete_measurement)
        .then(pl.lit("not_comparable"))
        .when(~pl.col("fixture_version_valid"))
        .then(pl.lit("not_comparable"))
        # An intentional fixture change is mergeable only when an earlier,
        # fixture-neutral change on the trusted base approved exactly the next
        # version. A candidate cannot approve its own rebaseline.
        .when(
            pl.col("fixture_version_bumped")
            & pl.col("fixture_identity_changed")
            & pl.col("fixture_rebaseline_approved")
            & pl.col("rebaseline_budget_exceeded")
        )
        .then(pl.lit("regression"))
        .when(
            pl.col("fixture_version_bumped")
            & pl.col("fixture_identity_changed")
            & pl.col("fixture_rebaseline_approved")
        )
        .then(pl.lit("fixture_rebaselined"))
        .when(pl.col("fixture_version_bumped") & pl.col("fixture_identity_changed"))
        .then(pl.lit("rebaseline_unapproved"))
        # A version-only bump could otherwise waive an ordinary latency
        # regression. Require the captured page identity to actually change.
        .when(pl.col("fixture_version_bumped"))
        .then(pl.lit("not_comparable"))
        .when(~pl.col("output_compatible"))
        .then(pl.lit("fixture_changed"))
        .when(substantial)
        .then(pl.lit("regression"))
        .when(warning)
        .then(pl.lit("warning"))
        .otherwise(pl.lit("ok"))
        .alias("status")
    )


def fmt_number(value: Any, digits: int = 2) -> str:
    if value is None:
        return "—"
    return f"{float(value):,.{digits}f}"


def markdown_report(frame: pl.DataFrame, base_sha: str, head_sha: str) -> str:
    lines = [
        "## Render performance",
        "",
        f"Base `{base_sha[:12]}` → head `{head_sha[:12]}` on the same runner.",
        (
            "Typical latency uses Criterion's slope estimate, falling back to "
            "mean for flat sampling."
        ),
        (
            "The memcpy figure is an empirical cache-hot copy floor for the "
            "exact rendered bytes."
        ),
        "Fixture identity is enforced with SHA-256 and byte size.",
        "",
    ]
    for row in frame.to_dicts():
        lines.append(
            "- `{id}` fixture v{base_version} → v{head_version}; "
            "base `{base_hash}` ({base_size:,} B), head `{head_hash}` ({head_size:,} B)".format(
                id=row["id"],
                base_version=row["base_fixture_version"],
                head_version=row["head_fixture_version"],
                base_hash=row["base_fixture_sha256"][:12],
                head_hash=row["head_fixture_sha256"][:12],
                base_size=row["base_fixture_size_bytes"],
                head_size=row["head_fixture_size_bytes"],
            )
        )
    lines.extend(
        [
            "",
            (
                "| Fixture | Base typical | Head typical | Change | Head p95 | "
                "memcpy floor | Render / memcpy | Rebaseline ceiling | "
                "Efficiency | Status |"
            ),
            "|---|---:|---:|---:|---:|---:|---:|---:|---:|---|",
        ]
    )
    for row in frame.to_dicts():
        change = (
            "—"
            if row["status"] in {"fixture_rebaselined", "rebaseline_unapproved"}
            else f"{fmt_number(row.get('latency_change_percent'))}%"
        )
        lines.append(
            (
                "| {id} | {base} ns | {head} ns | {change} | {p95} ns | "
                "{floor} ns | {ratio}× | {ceiling}× | {efficiency}% | "
                "{status} |"
            ).format(
                id=row["id"],
                base=fmt_number(row.get("base_render_typical_ns")),
                head=fmt_number(row.get("head_render_typical_ns")),
                change=change,
                p95=fmt_number(row.get("head_render_sample_p95_ns")),
                floor=fmt_number(row.get("head_memcpy_typical_ns")),
                ratio=fmt_number(row.get("head_render_to_memcpy")),
                ceiling=fmt_number(row.get("rebaseline_max_render_to_memcpy")),
                efficiency=fmt_number(row.get("efficiency_percent")),
                status=row["status"],
            )
        )
    lines.extend(
        [
            "",
            (
                "A regression requires latency and render/memcpy ratio to exceed "
                "the configured substantial threshold, with conservatively "
                "non-overlapping confidence intervals. An approved fixture "
                "rebaseline also fails when its absolute render/memcpy ratio "
                "exceeds the trusted rebaseline ceiling."
            ),
            (
                "`fixture_changed`, `rebaseline_unapproved`, and "
                "`not_comparable` are hard failures: a changed page or "
                "incomplete baseline must not hide a latency regression."
            ),
            (
                "`fixture_rebaselined` skips the latency comparison only when "
                "the trusted base pre-approved that exact next fixture version "
                "and the head render/memcpy ratio remains within the trusted "
                "rebaseline budget."
            ),
            "",
        ]
    )
    return "\n".join(lines)


def emit_github_output(name: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write(f"{name}={value}\n")


def main() -> int:
    args = parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    args.output.mkdir(parents=True, exist_ok=True)

    observations = pl.concat(
        [
            load_criterion(args.base, "base", args.base_sha),
            load_criterion(args.head, "head", args.head_sha),
        ],
        how="vertical_relaxed",
    )
    samples = pl.concat(
        [
            load_samples(args.base, "base", args.base_sha),
            load_samples(args.head, "head", args.head_sha),
        ],
        how="vertical_relaxed",
    )
    fixtures = pl.DataFrame(
        [
            fixture_metadata(args.base_fixture, "base", args.base_sha),
            fixture_metadata(args.head_fixture, "head", args.head_sha),
        ]
    )
    comparison = compare(
        observations,
        samples,
        comparison_config(load_config(args.base_config), load_config(args.config)),
        fixtures,
    )

    observations.write_parquet(args.output / "observations.parquet")
    observations.write_csv(args.output / "observations.csv")
    samples.write_parquet(args.output / "samples.parquet")
    samples.write_csv(args.output / "samples.csv")
    fixtures.write_parquet(args.output / "fixtures.parquet")
    fixtures.write_csv(args.output / "fixtures.csv")
    (args.output / "fixtures.json").write_text(
        json.dumps(fixtures.to_dicts(), indent=2) + "\n"
    )
    comparison.write_parquet(args.output / "comparison.parquet")
    comparison.write_csv(args.output / "comparison.csv")

    rows = comparison.to_dicts()
    failure_statuses = {
        "regression",
        "not_comparable",
        "fixture_changed",
        "rebaseline_unapproved",
    }
    failures = sum(row["status"] in failure_statuses for row in rows)
    warnings = sum(row["status"] in {"warning", "fixture_rebaselined"} for row in rows)
    status = "failure" if failures else "warning" if warnings else "success"
    summary = {
        "schema_version": 1,
        "base_sha": args.base_sha,
        "head_sha": args.head_sha,
        "status": status,
        "failures": failures,
        "warnings": warnings,
        "fixtures": fixtures.to_dicts(),
        "benchmarks": rows,
    }
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    markdown = markdown_report(comparison, args.base_sha, args.head_sha)
    (args.output / "summary.md").write_text(markdown)

    for row in rows:
        annotation = "error" if row["status"] in failure_statuses else "warning"
        if row["status"] != "ok":
            if row.get("rebaseline_budget_exceeded"):
                detail = (
                    f"approved fixture v{row['base_fixture_version']} → "
                    f"v{row['head_fixture_version']} measured "
                    f"{fmt_number(row.get('head_render_to_memcpy'))}x the memcpy "
                    "floor, above the trusted "
                    f"{fmt_number(row.get('rebaseline_max_render_to_memcpy'))}x "
                    "rebaseline budget"
                )
            elif row["status"] in {"fixture_rebaselined", "rebaseline_unapproved"}:
                detail = (
                    f"fixture v{row['base_fixture_version']} → "
                    f"v{row['head_fixture_version']}; exact bytes changed and latency "
                    "comparison was skipped; "
                    f"trusted approval={row['fixture_rebaseline_approved']}"
                )
            else:
                detail = (
                    f"{fmt_number(row.get('latency_change_percent'))}% latency change, "
                    f"{fmt_number(row.get('head_render_to_memcpy'))}x memcpy floor"
                )
            print(
                f"::{annotation} title=Performance {row['status']}::{row['id']}: "
                f"{detail}"
            )
        LOG.info("comparison %s", json.dumps(row, sort_keys=True, default=str))

    emit_github_output("status", status)
    emit_github_output("failures", str(failures))
    emit_github_output("warnings", str(warnings))
    print(markdown)
    return 0


if __name__ == "__main__":
    sys.exit(main())
