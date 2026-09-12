#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Regression tests for the Criterion-to-Polars performance report."""

from __future__ import annotations

import io
import json
import pathlib
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest import mock

import polars as pl

import perf_report


RENDER = "render_memcpy_floor/proa_html_safetext_stack"
MEMCPY = "render_memcpy_floor/memcpy_page_floor"
FIXTURE = b"x" * 4096


def write_criterion_result(
    root: pathlib.Path,
    benchmark: str,
    *,
    typical_ns: float,
    ci_low_ns: float,
    ci_high_ns: float,
    output_bytes: int | None = 4096,
    round_names: tuple[str, ...] = ("round-1", "round-2"),
    include_slope: bool = True,
    sample_size: int = 60,
) -> None:
    def estimate(point: float) -> dict[str, object]:
        return {
            "confidence_interval": {
                "confidence_level": 0.95,
                "lower_bound": ci_low_ns,
                "upper_bound": ci_high_ns,
            },
            "point_estimate": point,
            "standard_error": 0.5,
        }

    for round_name in round_names:
        result_dir = root / round_name / benchmark / "new"
        result_dir.mkdir(parents=True, exist_ok=True)
        estimates = {
            "mean": estimate(typical_ns if not include_slope else typical_ns * 9)
        }
        if include_slope:
            estimates["slope"] = estimate(typical_ns)
        metadata: dict[str, object] = {"full_id": benchmark}
        if output_bytes is not None:
            metadata["throughput"] = {"Bytes": output_bytes}
        iterations = [float(index + 1) for index in range(sample_size)]
        sample = {
            "sampling_mode": "Linear",
            "iters": iterations,
            "times": [
                typical_ns * iteration * (0.99 + (index % 5) * 0.005)
                for index, iteration in enumerate(iterations)
            ],
        }
        (result_dir / "estimates.json").write_text(json.dumps(estimates))
        (result_dir / "benchmark.json").write_text(json.dumps(metadata))
        (result_dir / "sample.json").write_text(json.dumps(sample))


def benchmark_config(
    fixture_version: int = 1,
    approved_rebaseline_version: int | None = None,
    rebaseline_max_render_to_memcpy: float = 20.0,
) -> pl.DataFrame:
    if approved_rebaseline_version is None:
        approved_rebaseline_version = fixture_version
    return pl.DataFrame(
        [
            {
                "id": "standard_page",
                "render": RENDER,
                "memcpy": MEMCPY,
                "fixture_version": fixture_version,
                "approved_rebaseline_version": approved_rebaseline_version,
                "rebaseline_max_render_to_memcpy": rebaseline_max_render_to_memcpy,
                "warn_percent": 8.0,
                "fail_percent": 15.0,
                "output_policy": "exact",
                "expected_rounds": 2,
                "minimum_sample_count": 120,
            }
        ]
    )


def modified_policy_config() -> pl.DataFrame:
    return pl.DataFrame(
        [
            {
                "id": "standard_page",
                "render": "candidate/renamed_render",
                "memcpy": "candidate/renamed_memcpy",
                "fixture_version": 2,
                "approved_rebaseline_version": 999,
                "rebaseline_max_render_to_memcpy": 999_999.0,
                "warn_percent": 9_998.0,
                "fail_percent": 9_999.0,
                "output_policy": "candidate-policy",
                "expected_rounds": 99,
                "minimum_sample_count": 9_999,
            }
        ]
    )


class PerfReportTests(unittest.TestCase):
    def test_config_requires_durable_baseline_state_epoch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory, "benchmarks.toml")
            path.write_text(
                f"""\
schema_version = 1

[[benchmarks]]
id = "standard_page"
render = "{RENDER}"
memcpy = "{MEMCPY}"
fixture_version = 1
approved_rebaseline_version = 1
rebaseline_max_render_to_memcpy = 20.0
warn_percent = 8.0
fail_percent = 15.0
output_policy = "exact"
expected_rounds = 2
minimum_sample_count = 120
"""
            )
            with self.assertRaisesRegex(ValueError, "baseline state epoch"):
                perf_report.load_config(path)

    def test_config_requires_a_positive_finite_rebaseline_budget(self) -> None:
        for value in ("0.0", "-1.0", "nan"):
            with self.subTest(value=value), tempfile.TemporaryDirectory() as directory:
                path = pathlib.Path(directory, "benchmarks.toml")
                path.write_text(
                    f"""\
schema_version = 1
baseline_state_epoch = 1

[[benchmarks]]
id = "standard_page"
render = "{RENDER}"
memcpy = "{MEMCPY}"
fixture_version = 1
approved_rebaseline_version = 1
rebaseline_max_render_to_memcpy = {value}
warn_percent = 8.0
fail_percent = 15.0
output_policy = "exact"
expected_rounds = 2
minimum_sample_count = 120
"""
                )
                with self.assertRaisesRegex(ValueError, "positive finite ratio"):
                    perf_report.load_config(path)

    def test_candidate_cannot_replace_base_policy_or_benchmark_mapping(self) -> None:
        config = perf_report.comparison_config(
            benchmark_config(fixture_version=1), modified_policy_config()
        ).row(0, named=True)

        self.assertEqual(config["render"], RENDER)
        self.assertEqual(config["memcpy"], MEMCPY)
        self.assertEqual(config["warn_percent"], 8.0)
        self.assertEqual(config["fail_percent"], 15.0)
        self.assertEqual(config["output_policy"], "exact")
        self.assertEqual(config["expected_rounds"], 2)
        self.assertEqual(config["minimum_sample_count"], 120)
        self.assertEqual(config["approved_rebaseline_version"], 1)
        self.assertEqual(config["rebaseline_max_render_to_memcpy"], 20.0)
        self.assertEqual(config["base_fixture_version"], 1)
        self.assertEqual(config["head_fixture_version"], 2)

    def compare_roots(
        self,
        base: pathlib.Path,
        head: pathlib.Path,
        *,
        base_fixture: bytes = FIXTURE,
        head_fixture: bytes = FIXTURE,
        base_fixture_version: int = 1,
        head_fixture_version: int = 1,
        base_approved_rebaseline_version: int | None = None,
    ) -> dict[str, object]:
        base_fixture_path = base / "fixture.html"
        head_fixture_path = head / "fixture.html"
        base_fixture_path.parent.mkdir(parents=True, exist_ok=True)
        head_fixture_path.parent.mkdir(parents=True, exist_ok=True)
        base_fixture_path.write_bytes(base_fixture)
        head_fixture_path.write_bytes(head_fixture)
        observations = pl.concat(
            [
                perf_report.load_criterion(base, "base", "base-sha"),
                perf_report.load_criterion(head, "head", "head-sha"),
            ],
            how="vertical_relaxed",
        )
        samples = pl.concat(
            [
                perf_report.load_samples(base, "base", "base-sha"),
                perf_report.load_samples(head, "head", "head-sha"),
            ],
            how="vertical_relaxed",
        )
        fixtures = pl.DataFrame(
            [
                perf_report.fixture_metadata(base_fixture_path, "base", "base-sha"),
                perf_report.fixture_metadata(head_fixture_path, "head", "head-sha"),
            ]
        )
        config = perf_report.comparison_config(
            benchmark_config(
                base_fixture_version, base_approved_rebaseline_version
            ),
            benchmark_config(head_fixture_version),
        )
        rows = perf_report.compare(observations, samples, config, fixtures).to_dicts()
        self.assertEqual(len(rows), 1)
        return rows[0]

    def test_stable_render_reports_memcpy_efficiency_and_distribution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=103, ci_low_ns=101, ci_high_ns=105
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "ok")
        self.assertAlmostEqual(row["latency_change_percent"], 3.0)
        self.assertAlmostEqual(row["head_render_to_memcpy"], 10.3)
        self.assertAlmostEqual(row["efficiency_percent"], 100 / 10.3)
        self.assertGreater(row["head_render_sample_p95_ns"], 0)
        self.assertEqual(row["head_render_typical_estimators"], "slope")

    def test_mean_is_used_when_criterion_does_not_emit_slope(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            for revision in (base, head):
                write_criterion_result(
                    revision,
                    RENDER,
                    typical_ns=100,
                    ci_low_ns=98,
                    ci_high_ns=102,
                    include_slope=False,
                )
                write_criterion_result(
                    revision,
                    MEMCPY,
                    typical_ns=10,
                    ci_low_ns=9,
                    ci_high_ns=11,
                    include_slope=False,
                )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "ok")
        self.assertEqual(row["head_render_typical_estimators"], "mean")
        self.assertEqual(row["head_render_typical_ns"], 100)

    def test_substantial_regression_requires_latency_ratio_and_separated_ci(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=125, ci_low_ns=122, ci_high_ns=128
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "regression")
        self.assertEqual(row["confidence_intervals_separated"], True)
        self.assertAlmostEqual(row["latency_change_percent"], 25.0)
        self.assertAlmostEqual(row["ratio_change_percent"], 25.0)

    def test_changed_exact_fixture_is_a_hard_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            base_fixture = b"a" * 4096
            head_fixture = b"b" * 8192
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head,
                RENDER,
                typical_ns=100,
                ci_low_ns=98,
                ci_high_ns=102,
                output_bytes=8192,
            )
            write_criterion_result(
                head,
                MEMCPY,
                typical_ns=10,
                ci_low_ns=9,
                ci_high_ns=11,
                output_bytes=8192,
            )

            row = self.compare_roots(
                base,
                head,
                base_fixture=base_fixture,
                head_fixture=head_fixture,
            )

        self.assertEqual(row["status"], "fixture_changed")
        self.assertEqual(row["output_compatible"], False)

    def test_same_size_hash_drift_without_version_bump_is_a_hard_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            for revision in (base, head):
                write_criterion_result(
                    revision, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
                )
                write_criterion_result(
                    revision, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
                )

            row = self.compare_roots(
                base,
                head,
                base_fixture=b"a" * len(FIXTURE),
                head_fixture=b"b" * len(FIXTURE),
            )

        self.assertEqual(row["status"], "fixture_changed")
        self.assertEqual(row["fixture_identity_changed"], True)
        self.assertEqual(row["output_compatible"], False)

    def test_version_bump_makes_an_intentional_fixture_change_visible(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            for revision in (base, head):
                write_criterion_result(
                    revision, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
                )
                write_criterion_result(
                    revision, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
                )

            row = self.compare_roots(
                base,
                head,
                base_fixture=b"a" * len(FIXTURE),
                head_fixture=b"b" * len(FIXTURE),
                base_fixture_version=1,
                head_fixture_version=2,
                base_approved_rebaseline_version=2,
            )

        self.assertEqual(row["status"], "fixture_rebaselined")
        self.assertEqual(row["fixture_version_bumped"], True)
        self.assertEqual(row["output_compatible"], False)
        self.assertEqual(row["fixture_rebaseline_approved"], True)
        self.assertEqual(row["rebaseline_budget_exceeded"], False)

    def test_approved_fixture_change_cannot_waive_tenfold_slowdown(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=1_000, ci_low_ns=990, ci_high_ns=1_010
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )

            row = self.compare_roots(
                base,
                head,
                base_fixture=b"a" * len(FIXTURE),
                head_fixture=b"b" * len(FIXTURE),
                base_fixture_version=1,
                head_fixture_version=2,
                base_approved_rebaseline_version=2,
            )

        self.assertEqual(row["fixture_rebaseline_approved"], True)
        self.assertEqual(row["head_render_to_memcpy"], 100.0)
        self.assertEqual(row["rebaseline_max_render_to_memcpy"], 20.0)
        self.assertEqual(row["rebaseline_budget_exceeded"], True)
        self.assertEqual(row["status"], "regression")

    def test_candidate_cannot_self_approve_fixture_rebaseline(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=1_000, ci_low_ns=990, ci_high_ns=1_010
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            row = self.compare_roots(
                base,
                head,
                base_fixture=b"a" * len(FIXTURE),
                head_fixture=b"b" * len(FIXTURE),
                base_fixture_version=1,
                head_fixture_version=2,
            )

        self.assertEqual(row["status"], "rebaseline_unapproved")
        self.assertEqual(row["fixture_rebaseline_approved"], False)

    def test_partial_round_or_sample_upload_is_not_comparable(self) -> None:
        for round_names, sample_size in ((('round-1',), 60), (('round-1', 'round-2'), 20)):
            with self.subTest(round_names=round_names, sample_size=sample_size):
                with tempfile.TemporaryDirectory() as directory:
                    root = pathlib.Path(directory)
                    base, head = root / "base", root / "head"
                    for revision in (base, head):
                        for benchmark, typical, low, high in (
                            (RENDER, 100, 98, 102),
                            (MEMCPY, 10, 9, 11),
                        ):
                            write_criterion_result(
                                revision,
                                benchmark,
                                typical_ns=typical,
                                ci_low_ns=low,
                                ci_high_ns=high,
                                round_names=round_names,
                                sample_size=sample_size,
                            )
                    row = self.compare_roots(base, head)
                self.assertEqual(row["status"], "not_comparable")

    def test_candidate_benchmark_ids_must_match_trusted_base(self) -> None:
        renamed = benchmark_config().with_columns(
            pl.lit("renamed").alias("id")
        )
        with self.assertRaisesRegex(ValueError, "benchmark IDs differ"):
            perf_report.comparison_config(benchmark_config(), renamed)

    def test_version_bump_cannot_waive_regression_without_fixture_change(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=130, ci_low_ns=128, ci_high_ns=132
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )

            row = self.compare_roots(
                base, head, base_fixture_version=1, head_fixture_version=2
            )

        self.assertEqual(row["status"], "not_comparable")
        self.assertEqual(row["fixture_identity_changed"], False)

    def test_missing_base_memcpy_is_not_silently_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                head, RENDER, typical_ns=103, ci_low_ns=101, ci_high_ns=105
            )
            write_criterion_result(
                head, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "not_comparable")

    def test_missing_head_measurement_is_not_comparable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=103, ci_low_ns=101, ci_high_ns=105
            )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "not_comparable")

    def test_missing_throughput_metadata_changes_fixture_status(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head = root / "base", root / "head"
            for revision in (base, head):
                write_criterion_result(
                    revision,
                    RENDER,
                    typical_ns=100,
                    ci_low_ns=98,
                    ci_high_ns=102,
                    output_bytes=None,
                )
                write_criterion_result(
                    revision,
                    MEMCPY,
                    typical_ns=10,
                    ci_low_ns=9,
                    ci_high_ns=11,
                    output_bytes=None,
                )

            row = self.compare_roots(base, head)

        self.assertEqual(row["status"], "fixture_changed")

    def test_main_writes_polars_artifacts_and_fails_incomplete_data(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            base, head, output = root / "base", root / "head", root / "output"
            base_fixture, head_fixture = root / "base.html", root / "head.html"
            config = root / "benchmarks.toml"
            github_output = root / "github-output"
            base_fixture.write_bytes(FIXTURE)
            head_fixture.write_bytes(FIXTURE)
            config.write_text(
                f"""\
schema_version = 1
baseline_state_epoch = 1

[[benchmarks]]
id = "standard_page"
render = "{RENDER}"
memcpy = "{MEMCPY}"
fixture_version = 1
approved_rebaseline_version = 1
rebaseline_max_render_to_memcpy = 20.0
warn_percent = 8.0
fail_percent = 15.0
output_policy = "exact"
expected_rounds = 2
minimum_sample_count = 120
"""
            )
            write_criterion_result(
                base, RENDER, typical_ns=100, ci_low_ns=98, ci_high_ns=102
            )
            write_criterion_result(
                base, MEMCPY, typical_ns=10, ci_low_ns=9, ci_high_ns=11
            )
            write_criterion_result(
                head, RENDER, typical_ns=103, ci_low_ns=101, ci_high_ns=105
            )
            argv = [
                "perf_report.py",
                "--base",
                str(base),
                "--head",
                str(head),
                "--base-fixture",
                str(base_fixture),
                "--head-fixture",
                str(head_fixture),
                "--base-config",
                str(config),
                "--config",
                str(config),
                "--output",
                str(output),
                "--base-sha",
                "base-sha",
                "--head-sha",
                "head-sha",
            ]

            with (
                mock.patch("sys.argv", argv),
                mock.patch.dict(
                    "os.environ", {"GITHUB_OUTPUT": str(github_output)}, clear=False
                ),
                redirect_stdout(io.StringIO()),
            ):
                exit_code = perf_report.main()

            summary = json.loads((output / "summary.json").read_text())

            self.assertEqual(exit_code, 0)
            self.assertEqual(summary["status"], "failure")
            self.assertEqual(summary["failures"], 1)
            self.assertEqual(summary["benchmarks"][0]["status"], "not_comparable")
            self.assertTrue((output / "observations.parquet").is_file())
            self.assertTrue((output / "samples.parquet").is_file())
            self.assertTrue((output / "fixtures.json").is_file())
            self.assertTrue((output / "comparison.csv").is_file())
            self.assertIn("status=failure", github_output.read_text())


if __name__ == "__main__":
    unittest.main()
