#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Tests for durable GitHub performance alerts."""

from __future__ import annotations

import io
import json
import pathlib
import sys
import tempfile
import unittest
import urllib.error
from typing import Any


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import perf_alert


SHA = "a" * 40
BASE_SHA = "b" * 40
REPOSITORY = "proa-labs/proa"
RUN_ID = 123
RUN_URL = f"https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}"


def config(**changes: Any) -> perf_alert.AlertConfig:
    values = {
        "repository": REPOSITORY,
        "workflow_path": ".github/workflows/performance.yml",
        "run_url": RUN_URL,
        "run_id": RUN_ID,
        "run_attempt": 1,
        "sha": SHA,
        "event": "push",
        "workflow_conclusion": "success",
        "mention": "@hariria",
    }
    values.update(changes)
    return perf_alert.AlertConfig(**values)


def row(status: str = "ok") -> dict[str, Any]:
    return {
        "id": "standard_page",
        "status": status,
        "base_render_typical_ns": 100.0,
        "head_render_typical_ns": 120.0,
        "latency_change_percent": 20.0,
        "head_render_ci_low_ns": 118.0,
        "head_render_ci_high_ns": 122.0,
        "head_render_sample_p95_ns": 125.0,
        "head_memcpy_typical_ns": 10.0,
        "head_render_to_memcpy": 12.0,
        "rebaseline_max_render_to_memcpy": 20.0,
        "rebaseline_budget_exceeded": False,
        "ratio_change_percent": 20.0,
        "efficiency_percent": 8.33,
        "warn_percent": 8.0,
        "fail_percent": 15.0,
    }


def report(*statuses: str) -> perf_alert.Report:
    rows = tuple(row(status) for status in statuses)
    failures = sum(item["status"] in perf_alert.HARD_STATUSES for item in rows)
    warnings = sum(item["status"] in perf_alert.WARNING_STATUSES for item in rows)
    status = "failure" if failures else "warning" if warnings else "success"
    return perf_alert.Report(status, failures, warnings, rows, BASE_SHA, SHA)


def baseline_issue(
    sha: str = BASE_SHA,
    number: int = 88,
    run_id: int = 100,
    *,
    user: str = "github-actions[bot]",
) -> dict[str, Any]:
    return {
        "number": number,
        "title": perf_alert.BASELINE_STATE_TITLE,
        "body": (
            f"{perf_alert.BASELINE_STATE_MARKER}\n"
            f"<!-- proa-performance-baseline:{sha}:{run_id}:1 -->"
        ),
        "state": "open",
        "user": {"login": user},
    }


def issue(kind: str, number: int, run_id: int = 100, state: str = "open") -> dict[str, Any]:
    return {
        "number": number,
        "title": perf_alert.ALERTS[kind]["title"],
        "body": (
            f"{perf_alert.ALERTS[kind]['marker']}\n"
            f"<!-- proa-performance-alert-run:{run_id}:1 -->"
        ),
        "state": state,
        "user": {"login": "github-actions[bot]"},
    }


class FakeApi:
    def __init__(
        self,
        issues: list[dict[str, Any]] | None = None,
        *,
        current: bool = True,
    ) -> None:
        self._issues = list(issues or [])
        self.current = current
        self.created: list[tuple[str, str]] = []
        self.updated: list[tuple[int, str, str]] = []
        self.comments: list[tuple[int, str]] = []

    def authoritative_run_is_current(self, _config: perf_alert.AlertConfig) -> bool:
        return self.current

    def issues(self, _repository: str) -> list[dict[str, Any]]:
        return self._issues

    def create_issue(self, _repository: str, title: str, body: str) -> None:
        self.created.append((title, body))

    def update_issue(
        self, _repository: str, number: int, body: str, *, state: str
    ) -> None:
        self.updated.append((number, state, body))

    def comment(self, _repository: str, number: int, body: str) -> None:
        self.comments.append((number, body))


class SequenceGitHubApi(perf_alert.GitHubApi):
    def __init__(self, outcomes: list[Any]) -> None:
        self.outcomes = list(outcomes)
        self.calls: list[tuple[str, str]] = []
        self.sleeps: list[float] = []
        super().__init__("token", sleep=self.sleeps.append)

    def _request_json_once(
        self,
        method: str,
        endpoint: str,
        *,
        params: dict[str, str] | None = None,
        payload: dict[str, Any] | None = None,
    ) -> Any:
        del params, payload
        self.calls.append((method, endpoint))
        outcome = self.outcomes.pop(0)
        if isinstance(outcome, Exception):
            raise outcome
        return outcome


class PerformanceAlertTests(unittest.TestCase):
    def test_resolver_uses_exact_bot_managed_baseline(self) -> None:
        api = FakeApi([baseline_issue()])
        self.assertEqual(
            perf_alert.resolve_healthy_baseline(
                api, REPOSITORY, "c" * 40, require_state=True
            ),
            BASE_SHA,
        )

    def test_resolver_fails_closed_after_bootstrap_epoch(self) -> None:
        api = FakeApi(
            [baseline_issue(user="attacker")]
        )
        with self.assertRaisesRegex(perf_alert.AlertError, "missing after bootstrap"):
            perf_alert.resolve_healthy_baseline(
                api, REPOSITORY, "c" * 40, require_state=True
            )
        self.assertEqual(
            perf_alert.resolve_healthy_baseline(
                api, REPOSITORY, "c" * 40, require_state=False
            ),
            "c" * 40,
        )

    def test_resolver_rejects_ambiguous_bot_state(self) -> None:
        api = FakeApi([baseline_issue(number=88), baseline_issue(number=89)])
        with self.assertRaisesRegex(perf_alert.AlertError, "multiple bot-managed"):
            perf_alert.resolve_healthy_baseline(
                api, REPOSITORY, "c" * 40, require_state=True
            )

    def test_secondary_rate_limit_response_is_classified_as_retryable(self) -> None:
        class SecondaryLimitOpener:
            @staticmethod
            def open(request: Any, timeout: int) -> Any:
                del timeout
                raise urllib.error.HTTPError(
                    request.full_url,
                    403,
                    "Forbidden",
                    {},
                    io.BytesIO(b'{"message":"You have exceeded a secondary rate limit."}'),
                )

        api = perf_alert.GitHubApi("token", opener=SecondaryLimitOpener())
        with self.assertRaises(perf_alert.RetryableGitHubError):
            api._request_json_once("GET", "/repos/proa-labs/proa")

    def test_retry_after_and_primary_reset_headers_are_respected(self) -> None:
        api = perf_alert.GitHubApi("token", now=lambda: 100.0)
        self.assertEqual(api._retry_after({"Retry-After": "7"}), 7.0)
        self.assertEqual(
            api._retry_after(
                {"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "111"}
            ),
            11.0,
        )

    def test_retryable_get_and_patch_use_bounded_retry_after(self) -> None:
        for method in ("GET", "PATCH"):
            with self.subTest(method=method):
                api = SequenceGitHubApi(
                    [
                        perf_alert.RetryableGitHubError(
                            "secondary rate limit", retry_after=2.5
                        ),
                        {"ok": True},
                    ]
                )
                result = api.request_json(method, "/repos/proa-labs/proa")
                self.assertEqual(result, {"ok": True})
                self.assertEqual(len(api.calls), 2)
                self.assertEqual(api.sleeps, [2.5])

    def test_retry_wait_beyond_budget_fails_without_early_retry(self) -> None:
        api = SequenceGitHubApi(
            [
                perf_alert.RetryableGitHubError(
                    "primary rate limit", retry_after=perf_alert.MAX_RETRY_SECONDS + 1
                ),
                {"must_not_be_reached": True},
            ]
        )
        with self.assertRaises(perf_alert.RetryableGitHubError):
            api.request_json("GET", "/repos/proa-labs/proa")
        self.assertEqual(len(api.calls), 1)
        self.assertEqual(api.sleeps, [])

    def test_transient_retries_stop_at_the_bounded_attempt_limit(self) -> None:
        api = SequenceGitHubApi(
            [
                perf_alert.RetryableGitHubError("temporary outage")
                for _ in range(perf_alert.MAX_API_ATTEMPTS)
            ]
        )
        with self.assertRaises(perf_alert.RetryableGitHubError):
            api.request_json("GET", "/repos/proa-labs/proa")
        self.assertEqual(len(api.calls), perf_alert.MAX_API_ATTEMPTS)
        self.assertEqual(api.sleeps, [1.0, 2.0, 4.0])

    def test_request_layer_never_blindly_retries_post(self) -> None:
        api = SequenceGitHubApi(
            [
                perf_alert.RetryableGitHubError("ambiguous timeout"),
                {"duplicate": True},
            ]
        )
        with self.assertRaises(perf_alert.RetryableGitHubError):
            api.request_json("POST", "/repos/proa-labs/proa/issues", payload={})
        self.assertEqual(len(api.calls), 1)
        self.assertEqual(api.sleeps, [])

    def test_create_issue_reconciles_ambiguous_post_before_retrying(self) -> None:
        title = "Performance regression detected"
        body = "bot-managed body"

        class AmbiguousCreateApi(perf_alert.GitHubApi):
            def __init__(self) -> None:
                self.remote_issues: list[dict[str, Any]] = []
                self.post_calls = 0
                self.sleeps: list[float] = []
                super().__init__("token", sleep=self.sleeps.append)

            def issues(self, _repository: str) -> list[dict[str, Any]]:
                return self.remote_issues

            def _request_json_once(
                self,
                method: str,
                endpoint: str,
                *,
                params: dict[str, str] | None = None,
                payload: dict[str, Any] | None = None,
            ) -> Any:
                del endpoint, params
                self.assert_post(method, payload)
                self.post_calls += 1
                self.remote_issues.append(
                    {
                        "title": title,
                        "body": body,
                        "user": {"login": "github-actions[bot]"},
                    }
                )
                raise perf_alert.RetryableGitHubError("response was lost")

            @staticmethod
            def assert_post(method: str, payload: dict[str, Any] | None) -> None:
                if method != "POST" or payload != {"title": title, "body": body}:
                    raise AssertionError("unexpected create-issue request")

        api = AmbiguousCreateApi()
        api.create_issue(REPOSITORY, title, body)
        self.assertEqual(api.post_calls, 1)
        self.assertEqual(api.sleeps, [1.0])

    def test_existing_bot_comment_makes_post_idempotent_across_reruns(self) -> None:
        body = "durable reconciliation comment"

        class ExistingCommentApi(perf_alert.GitHubApi):
            def __init__(self) -> None:
                self.post_calls = 0
                super().__init__("token", sleep=lambda _delay: None)

            def comments(
                self, _repository: str, _number: int
            ) -> list[dict[str, Any]]:
                return [
                    {
                        "body": body,
                        "user": {"login": "github-actions[bot]"},
                    }
                ]

            def _request_json_once(self, *args: Any, **kwargs: Any) -> Any:
                del args, kwargs
                self.post_calls += 1
                raise AssertionError("existing comment must suppress POST")

        api = ExistingCommentApi()
        api.comment(REPOSITORY, 17, body)
        self.assertEqual(api.post_calls, 0)

    def test_regression_creates_issue_with_durable_evidence_and_mention(self) -> None:
        api = FakeApi()
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("regression")
        )
        self.assertEqual(result, "created:regression")
        title, body = api.created[0]
        self.assertEqual(title, "Performance regression detected")
        self.assertIn("@hariria", body)
        self.assertIn("120.00 ns", body)
        self.assertIn("10.00 ns", body)
        self.assertIn("20.00%", body)

    def test_regression_retains_last_healthy_baseline(self) -> None:
        api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="failure"),
            report("regression"),
        )
        baseline_update = next(
            body for number, state, body in api.updated if number == 88 and state == "open"
        )
        self.assertIn(f"baseline:{BASE_SHA}:", baseline_update)
        self.assertNotIn(f"baseline:{SHA}:", baseline_update)

    def test_warning_does_not_ratchet_baseline_but_healthy_push_does(self) -> None:
        warning_api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(warning_api, config(), report("warning"))
        warning_body = next(
            body for number, _state, body in warning_api.updated if number == 88
        )
        self.assertIn(f"baseline:{BASE_SHA}:", warning_body)
        self.assertEqual(warning_api.comments, [])

        healthy_api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(healthy_api, config(), report("ok"))
        healthy_body = next(
            body for number, _state, body in healthy_api.updated if number == 88
        )
        self.assertIn(f"baseline:{SHA}:", healthy_body)
        self.assertTrue(any(number == 88 for number, _body in healthy_api.comments))

    def test_schedule_never_ratchets_baseline(self) -> None:
        api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(api, config(event="schedule"), report("ok"))
        body = next(body for number, _state, body in api.updated if number == 88)
        self.assertIn(f"baseline:{BASE_SHA}:", body)
        self.assertEqual(api.comments, [])

    def test_failed_workflow_never_ratchets_baseline(self) -> None:
        api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="failure"),
            report("ok"),
        )
        body = next(body for number, _state, body in api.updated if number == 88)
        self.assertIn(f"baseline:{BASE_SHA}:", body)
        self.assertEqual(api.comments, [])

    def test_approved_fixture_rebaseline_establishes_new_anchor(self) -> None:
        api = FakeApi([baseline_issue()])
        perf_alert.reconcile_alert(api, config(), report("fixture_rebaselined"))
        body = next(body for number, _state, body in api.updated if number == 88)
        self.assertIn(f"baseline:{SHA}:", body)

    def test_over_budget_approved_rebaseline_retains_healthy_anchor(self) -> None:
        over_budget = row("regression")
        over_budget["head_render_typical_ns"] = 1_000.0
        over_budget["head_render_to_memcpy"] = 100.0
        over_budget["ratio_change_percent"] = 900.0
        over_budget["rebaseline_budget_exceeded"] = True
        rebaseline_regression = perf_alert.Report(
            "failure", 1, 0, (over_budget,), BASE_SHA, SHA
        )
        api = FakeApi([baseline_issue()])

        result = perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="failure"),
            rebaseline_regression,
        )

        self.assertIn("created:regression", result)
        body = next(body for number, _state, body in api.updated if number == 88)
        self.assertIn(f"baseline:{BASE_SHA}:", body)
        self.assertNotIn(f"baseline:{SHA}:", body)

    def test_immediate_predecessor_cannot_false_recover_durable_alert(self) -> None:
        regression = issue("regression", 17)
        api = FakeApi([regression, baseline_issue()])
        # This is the formerly dangerous H -> A regression -> unchanged B
        # report: B looks healthy only because it was compared with A, while
        # the durable state still records H.
        invalid_recovery = perf_alert.Report(
            "success",
            0,
            0,
            (row("ok"),),
            SHA,
            "c" * 40,
        )
        result = perf_alert.reconcile_alert(
            api,
            config(
                sha="c" * 40,
                run_url=f"https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}",
            ),
            invalid_recovery,
        )
        self.assertEqual(result, "created:infrastructure")
        self.assertFalse(
            any(number == 17 and state == "closed" for number, state, _body in api.updated)
        )
        self.assertFalse(any(number == 88 for number, _state, _body in api.updated))

    def test_integrity_failure_is_not_mislabeled_as_latency_regression(self) -> None:
        api = FakeApi()
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("fixture_changed")
        )
        self.assertEqual(result, "created:integrity")
        self.assertEqual(api.created[0][0], "Performance baseline could not be compared")

    def test_missing_report_creates_infrastructure_issue(self) -> None:
        api = FakeApi([issue("regression", 17), issue("integrity", 18)])
        result = perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="failure"),
            None,
            "summary missing",
        )
        self.assertEqual(result, "created:infrastructure")
        self.assertEqual(api.updated, [])

    def test_integrity_failure_does_not_claim_latency_recovery(self) -> None:
        api = FakeApi([issue("regression", 17)])
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("fixture_changed")
        )
        self.assertEqual(result, "created:integrity")
        self.assertFalse(any(number == 17 for number, _state, _body in api.updated))

    def test_rebaseline_does_not_claim_latency_recovery(self) -> None:
        api = FakeApi([issue("regression", 17), issue("integrity", 18)])
        result = perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="success"),
            report("fixture_rebaselined"),
        )
        self.assertIn("closed:integrity", result)
        self.assertFalse(any(number == 17 for number, _state, _body in api.updated))

    def test_unapproved_rebaseline_creates_integrity_alert(self) -> None:
        api = FakeApi()
        result = perf_alert.reconcile_alert(
            api,
            config(workflow_conclusion="failure"),
            report("rebaseline_unapproved"),
        )
        self.assertEqual(result, "created:integrity")

    def test_recovery_closes_existing_alert_and_advances_run_state(self) -> None:
        api = FakeApi([issue("regression", 17)])
        result = perf_alert.reconcile_alert(api, config(), report("ok"))
        self.assertEqual(result, "closed:regression")
        self.assertEqual(api.updated[0][0:2], (17, "closed"))
        self.assertIn(f"run:{RUN_ID}:1", api.updated[0][2])

    def test_public_marker_spoof_is_ignored(self) -> None:
        spoof = issue("regression", 17)
        spoof["user"] = {"login": "attacker"}
        api = FakeApi([spoof])
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("regression")
        )
        self.assertEqual(result, "created:regression")
        self.assertEqual(api.updated, [])

    def test_stale_authoritative_run_does_not_mutate_issues(self) -> None:
        api = FakeApi(current=False)
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("regression")
        )
        self.assertEqual(result, "ignored_stale")
        self.assertEqual(api.created, [])

    def test_recorded_newer_run_prevents_out_of_order_recovery(self) -> None:
        api = FakeApi([issue("regression", 17, run_id=999)])
        result = perf_alert.reconcile_alert(api, config(), report("ok"))
        self.assertEqual(result, "ignored_stale")
        self.assertEqual(api.updated, [])

    def test_duplicate_bot_issues_are_deduplicated_without_wedging(self) -> None:
        api = FakeApi([issue("regression", 17), issue("regression", 18, run_id=90)])
        result = perf_alert.reconcile_alert(
            api, config(workflow_conclusion="failure"), report("regression")
        )
        self.assertIn("deduplicated:regression", result)
        self.assertTrue(any(number == 18 and state == "closed" for number, state, _ in api.updated))
        duplicate_body = next(body for number, _state, body in api.updated if number == 18)
        self.assertIn("Closed as a duplicate", duplicate_body)
        self.assertIn("does not indicate", duplicate_body)
        self.assertNotIn("Resolved by", duplicate_body)

    def test_summary_validation_rejects_wrong_sha_and_aggregate_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory, "summary.json")
            payload = {
                "schema_version": 1,
                "base_sha": BASE_SHA,
                "head_sha": "b" * 40,
                "status": "success",
                "failures": 0,
                "warnings": 0,
                "benchmarks": [row("ok")],
            }
            path.write_text(json.dumps(payload))
            with self.assertRaisesRegex(perf_alert.AlertError, "different revision"):
                perf_alert.load_report(path, SHA)
            payload["head_sha"] = SHA
            payload["failures"] = 1
            payload["status"] = "failure"
            path.write_text(json.dumps(payload))
            with self.assertRaisesRegex(perf_alert.AlertError, "aggregate counts"):
                perf_alert.load_report(path, SHA)

    def test_invalid_event_path_and_mention_are_rejected(self) -> None:
        for invalid in (
            config(event="pull_request"),
            config(workflow_path="performance.yml"),
            config(mention="@bad user"),
        ):
            with self.subTest(config=invalid), self.assertRaises(perf_alert.AlertError):
                perf_alert.validate_config(invalid)

    def test_workflow_path_accepts_documented_ref_suffix(self) -> None:
        expected = ".github/workflows/performance.yml"
        self.assertTrue(perf_alert.workflow_path_matches(expected, expected))
        self.assertTrue(perf_alert.workflow_path_matches(f"{expected}@main", expected))
        self.assertFalse(perf_alert.workflow_path_matches(f"{expected}@", expected))


if __name__ == "__main__":
    unittest.main()
