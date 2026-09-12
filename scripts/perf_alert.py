#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Maintain durable GitHub issues for trusted performance workflow runs."""

from __future__ import annotations

import argparse
import json
import logging
import math
import os
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Callable


API_ROOT = "https://api.github.com"
API_VERSION = "2022-11-28"
MAX_JSON_BYTES = 2 * 1_048_576
MAX_ERROR_BYTES = 64 * 1_024
MAX_API_ATTEMPTS = 4
BASE_RETRY_SECONDS = 1.0
MAX_RETRY_SECONDS = 60.0
RETRYABLE_HTTP_STATUSES = {408, 425, 429, 500, 502, 503, 504}
REPOSITORY_RE = re.compile(r"[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}\Z")
SHA_RE = re.compile(r"[0-9a-fA-F]{40}\Z")
WORKFLOW_PATH_RE = re.compile(r"\.github/workflows/[A-Za-z0-9_.-]+\.ya?ml\Z")
MENTION_RE = re.compile(r"(?:@[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?)?\Z")
BENCHMARK_ID_RE = re.compile(r"[A-Za-z0-9_.:/-]{1,160}\Z")
STATE_RE = re.compile(r"<!-- proa-performance-alert-run:(\d+):(\d+) -->")
BASELINE_STATE_RE = re.compile(
    r"<!-- proa-performance-baseline:([0-9a-fA-F]{40}):(\d+):(\d+) -->"
)
TRUSTED_EVENTS = {"push", "schedule"}
CONCLUSIONS = {
    "success",
    "failure",
    "cancelled",
    "timed_out",
    "action_required",
    "stale",
    "skipped",
    "neutral",
}
BENCHMARK_STATUSES = {
    "ok",
    "warning",
    "regression",
    "not_comparable",
    "fixture_changed",
    "fixture_rebaselined",
    "rebaseline_unapproved",
}
HARD_STATUSES = {
    "regression",
    "not_comparable",
    "fixture_changed",
    "rebaseline_unapproved",
}
WARNING_STATUSES = {"warning", "fixture_rebaselined"}
ALERTS = {
    "regression": {
        "title": "Performance regression detected",
        "marker": "<!-- proa-performance-alert:regression -->",
    },
    "integrity": {
        "title": "Performance baseline could not be compared",
        "marker": "<!-- proa-performance-alert:integrity -->",
    },
    "infrastructure": {
        "title": "Performance monitoring failed",
        "marker": "<!-- proa-performance-alert:infrastructure -->",
    },
}
BASELINE_STATE_TITLE = "Performance healthy baseline state"
BASELINE_STATE_MARKER = "<!-- proa-performance-baseline-state -->"

LOG = logging.getLogger("proa.perf_alert")


class AlertError(RuntimeError):
    """The performance alert could not be reconciled safely."""


class RetryableGitHubError(AlertError):
    """A transient or rate-limited GitHub API failure."""

    def __init__(self, message: str, retry_after: float | None = None) -> None:
        super().__init__(message)
        self.retry_after = retry_after


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args: Any, **_kwargs: Any) -> None:
        return None


HTTP_OPENER = urllib.request.build_opener(RejectRedirects)


@dataclass(frozen=True)
class AlertConfig:
    repository: str
    workflow_path: str
    run_url: str
    run_id: int
    run_attempt: int
    sha: str
    event: str
    workflow_conclusion: str
    mention: str = ""

    @property
    def run_key(self) -> tuple[int, int]:
        return self.run_id, self.run_attempt


@dataclass(frozen=True)
class Report:
    status: str
    failures: int
    warnings: int
    benchmarks: tuple[dict[str, Any], ...]
    base_sha: str
    head_sha: str


@dataclass(frozen=True)
class BaselineState:
    sha: str
    run_id: int
    run_attempt: int
    issue_number: int
    issue_state: str

    @property
    def run_key(self) -> tuple[int, int]:
        return self.run_id, self.run_attempt


def workflow_path_matches(actual: Any, expected: str) -> bool:
    if not isinstance(actual, str):
        return False
    path, separator, ref = actual.partition("@")
    return path == expected and (not separator or (bool(ref) and "@" not in ref))


def validate_config(config: AlertConfig) -> None:
    if not REPOSITORY_RE.fullmatch(config.repository):
        raise AlertError("repository must use OWNER/REPO syntax")
    if not WORKFLOW_PATH_RE.fullmatch(config.workflow_path):
        raise AlertError("workflow path must name a workflow YAML file")
    if not SHA_RE.fullmatch(config.sha):
        raise AlertError("sha must be a full 40-character Git commit")
    if config.event not in TRUSTED_EVENTS:
        raise AlertError("alerts are restricted to trusted push and schedule events")
    if config.workflow_conclusion not in CONCLUSIONS:
        raise AlertError("workflow conclusion is invalid")
    if config.run_id <= 0 or config.run_attempt <= 0:
        raise AlertError("run ID and attempt must be positive")
    expected_url = (
        f"https://github.com/{config.repository}/actions/runs/{config.run_id}"
    )
    if config.run_url != expected_url:
        raise AlertError("run URL does not identify the configured workflow run")
    if not MENTION_RE.fullmatch(config.mention):
        raise AlertError("mention must be empty or one GitHub @login")


class GitHubApi:
    def __init__(
        self,
        token: str,
        *,
        sleep: Callable[[float], None] = time.sleep,
        now: Callable[[], float] = time.time,
        opener: Any = HTTP_OPENER,
    ) -> None:
        if not token or "\n" in token or "\r" in token:
            raise AlertError("GitHub token is missing or malformed")
        self.token = token
        self._sleep = sleep
        self._now = now
        self._opener = opener

    def _retry_after(self, headers: Any) -> float | None:
        retry_after = headers.get("Retry-After") if headers is not None else None
        if retry_after is not None:
            try:
                value = float(retry_after)
            except (TypeError, ValueError):
                value = -1.0
            if value >= 0:
                return value

        remaining = (
            headers.get("X-RateLimit-Remaining") if headers is not None else None
        )
        reset = headers.get("X-RateLimit-Reset") if headers is not None else None
        if remaining == "0" and reset is not None:
            try:
                return max(0.0, float(reset) - self._now())
            except (TypeError, ValueError):
                return None
        return None

    @staticmethod
    def _backoff(error: RetryableGitHubError, attempt: int) -> float | None:
        if error.retry_after is not None:
            # Do not retry sooner than GitHub requested. If the requested wait
            # would exceed this short observer's retry budget, fail cleanly and
            # let a workflow rerun reconcile the durable state later.
            if error.retry_after > MAX_RETRY_SECONDS:
                return None
            return max(0.0, error.retry_after)
        return min(BASE_RETRY_SECONDS * (2 ** (attempt - 1)), MAX_RETRY_SECONDS)

    def _request_json_once(
        self,
        method: str,
        endpoint: str,
        *,
        params: dict[str, str] | None = None,
        payload: dict[str, Any] | None = None,
    ) -> Any:
        query = "" if not params else "?" + urllib.parse.urlencode(params)
        body = None
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": API_VERSION,
            "User-Agent": "proa-performance-alert/2",
        }
        if payload is not None:
            body = json.dumps(payload, separators=(",", ":")).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            f"{API_ROOT}{endpoint}{query}",
            data=body,
            headers=headers,
            method=method,
        )
        try:
            with self._opener.open(request, timeout=30) as response:
                raw = response.read(MAX_JSON_BYTES + 1)
        except urllib.error.HTTPError as error:
            status = error.code
            response_headers = error.headers
            try:
                error_body = error.read(MAX_ERROR_BYTES + 1)
            finally:
                error.close()
            error_text = error_body[:MAX_ERROR_BYTES].decode(
                "utf-8", errors="replace"
            ).lower()
            retry_after = self._retry_after(response_headers)
            remaining = (
                response_headers.get("X-RateLimit-Remaining")
                if response_headers is not None
                else None
            )
            if (
                status in RETRYABLE_HTTP_STATUSES
                or retry_after is not None
                or (status == 403 and remaining == "0")
                or (status == 403 and "secondary rate limit" in error_text)
            ):
                raise RetryableGitHubError(
                    f"GitHub API temporarily returned HTTP {status}", retry_after
                ) from error
            raise AlertError(f"GitHub API returned HTTP {status}") from error
        except (urllib.error.URLError, TimeoutError) as error:
            raise RetryableGitHubError("GitHub API request failed transiently") from error
        if len(raw) > MAX_JSON_BYTES:
            raise AlertError("GitHub API response exceeded the size limit")
        if not raw:
            return None
        try:
            return json.loads(raw)
        except json.JSONDecodeError as error:
            raise AlertError("GitHub API returned invalid JSON") from error

    def request_json(
        self,
        method: str,
        endpoint: str,
        *,
        params: dict[str, str] | None = None,
        payload: dict[str, Any] | None = None,
    ) -> Any:
        attempts = MAX_API_ATTEMPTS if method in {"GET", "PATCH"} else 1
        for attempt in range(1, attempts + 1):
            try:
                return self._request_json_once(
                    method, endpoint, params=params, payload=payload
                )
            except RetryableGitHubError as error:
                delay = self._backoff(error, attempt)
                if attempt == attempts or delay is None:
                    raise
                LOG.warning(
                    "github_api method=%s endpoint=%s attempt=%d/%d retry_in=%.1fs",
                    method,
                    endpoint,
                    attempt,
                    attempts,
                    delay,
                )
                self._sleep(delay)
        raise AssertionError("unreachable GitHub API retry state")

    def paginated_list(
        self, endpoint: str, *, params: dict[str, str] | None = None
    ) -> list[dict[str, Any]]:
        values: list[dict[str, Any]] = []
        for page in range(1, 21):
            page_params = dict(params or {})
            page_params.update({"per_page": "100", "page": str(page)})
            payload = self.request_json("GET", endpoint, params=page_params)
            if not isinstance(payload, list) or not all(
                isinstance(value, dict) for value in payload
            ):
                raise AlertError("GitHub returned a malformed paginated list")
            values.extend(payload)
            if len(payload) < 100:
                return values
        raise AlertError("GitHub returned too many result pages")

    def authoritative_run_is_current(self, config: AlertConfig) -> bool:
        run = self.request_json(
            "GET", f"/repos/{config.repository}/actions/runs/{config.run_id}"
        )
        if not isinstance(run, dict):
            raise AlertError("GitHub returned a malformed workflow run")
        repository = run.get("repository")
        repository_name = (
            repository.get("full_name") if isinstance(repository, dict) else None
        )
        workflow_id = run.get("workflow_id")
        if (
            not isinstance(workflow_id, int)
            or workflow_id <= 0
            or run.get("id") != config.run_id
            or run.get("run_attempt", 1) != config.run_attempt
            or not workflow_path_matches(run.get("path"), config.workflow_path)
            or str(run.get("head_sha", "")).lower() != config.sha.lower()
            or run.get("head_branch") != "main"
            or run.get("event") != config.event
            or run.get("status") != "completed"
            or run.get("conclusion") != config.workflow_conclusion
            or repository_name != config.repository
        ):
            raise AlertError("triggering workflow run identity did not attest")

        payload = self.request_json(
            "GET",
            f"/repos/{config.repository}/actions/workflows/{workflow_id}/runs",
            params={"branch": "main", "per_page": "100"},
        )
        runs = payload.get("workflow_runs") if isinstance(payload, dict) else None
        if not isinstance(runs, list):
            raise AlertError("GitHub returned a malformed workflow-run list")
        keys: list[tuple[int, int]] = []
        for candidate in runs:
            if not isinstance(candidate, dict):
                continue
            candidate_repo = candidate.get("repository")
            candidate_repo_name = (
                candidate_repo.get("full_name")
                if isinstance(candidate_repo, dict)
                else None
            )
            run_id = candidate.get("id")
            attempt = candidate.get("run_attempt", 1)
            if (
                isinstance(run_id, int)
                and isinstance(attempt, int)
                and candidate.get("event") in TRUSTED_EVENTS
                and candidate.get("head_branch") == "main"
                and workflow_path_matches(candidate.get("path"), config.workflow_path)
                and candidate_repo_name == config.repository
            ):
                keys.append((run_id, attempt))
        if not keys or config.run_key not in keys:
            raise AlertError("triggering workflow run was absent from its workflow history")
        return config.run_key == max(keys)

    def issues(self, repository: str) -> list[dict[str, Any]]:
        return self.paginated_list(
            f"/repos/{repository}/issues",
            params={"state": "all", "sort": "updated", "direction": "desc"},
        )

    def comments(self, repository: str, number: int) -> list[dict[str, Any]]:
        return self.paginated_list(f"/repos/{repository}/issues/{number}/comments")

    def _post_idempotently(
        self,
        endpoint: str,
        payload: dict[str, Any],
        *,
        operation: str,
        already_applied: Callable[[], bool],
    ) -> None:
        # This preflight also makes a complete observer rerun idempotent after a
        # prior process created the resource but failed before advancing state.
        if already_applied():
            return

        for attempt in range(1, MAX_API_ATTEMPTS + 1):
            try:
                self.request_json("POST", endpoint, payload=payload)
                return
            except RetryableGitHubError as error:
                delay = self._backoff(error, attempt)
                if delay is None:
                    raise
                LOG.warning(
                    "github_api operation=%s attempt=%d/%d reconcile_in=%.1fs",
                    operation,
                    attempt,
                    MAX_API_ATTEMPTS,
                    delay,
                )
                # POST outcomes can be ambiguous after a timeout or 5xx. Wait
                # before probing so GitHub's eventually visible issue state has
                # a chance to settle, and only retry when the exact bot-authored
                # resource is still absent.
                self._sleep(delay)
                if already_applied():
                    return
                if attempt == MAX_API_ATTEMPTS:
                    raise
        raise AssertionError("unreachable GitHub API POST retry state")

    def create_issue(self, repository: str, title: str, body: str) -> None:
        def already_applied() -> bool:
            return any(
                issue.get("title") == title
                and issue.get("body") == body
                and "pull_request" not in issue
                and isinstance(issue.get("user"), dict)
                and issue["user"].get("login") == "github-actions[bot]"
                for issue in self.issues(repository)
            )

        self._post_idempotently(
            f"/repos/{repository}/issues",
            {"title": title, "body": body},
            operation="create_issue",
            already_applied=already_applied,
        )

    def update_issue(
        self, repository: str, number: int, body: str, *, state: str
    ) -> None:
        payload: dict[str, Any] = {"body": body, "state": state}
        if state == "closed":
            payload["state_reason"] = "completed"
        self.request_json(
            "PATCH", f"/repos/{repository}/issues/{number}", payload=payload
        )

    def comment(self, repository: str, number: int, body: str) -> None:
        def already_applied() -> bool:
            return any(
                comment.get("body") == body
                and isinstance(comment.get("user"), dict)
                and comment["user"].get("login") == "github-actions[bot]"
                for comment in self.comments(repository, number)
            )

        self._post_idempotently(
            f"/repos/{repository}/issues/{number}/comments",
            {"body": body},
            operation="create_comment",
            already_applied=already_applied,
        )


def finite_number(value: Any, field: str) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise AlertError(f"performance report field {field} is not numeric")
    number = float(value)
    if not math.isfinite(number):
        raise AlertError(f"performance report field {field} is not finite")
    return number


def load_report(path: pathlib.Path, expected_sha: str) -> Report:
    if not path.is_file():
        raise AlertError("performance summary artifact is missing")
    if path.stat().st_size > MAX_JSON_BYTES:
        raise AlertError("performance summary artifact exceeded the size limit")
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise AlertError("performance summary artifact is not valid JSON") from error
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise AlertError("performance summary has an unsupported schema")
    if str(payload.get("head_sha", "")).lower() != expected_sha.lower():
        raise AlertError("performance summary belongs to a different revision")
    base_sha = payload.get("base_sha")
    head_sha = payload.get("head_sha")
    if (
        not isinstance(base_sha, str)
        or not SHA_RE.fullmatch(base_sha)
        or not isinstance(head_sha, str)
        or not SHA_RE.fullmatch(head_sha)
    ):
        raise AlertError("performance summary base/head revision is invalid")
    status = payload.get("status")
    failures = payload.get("failures")
    warnings = payload.get("warnings")
    benchmarks = payload.get("benchmarks")
    if status not in {"success", "warning", "failure"}:
        raise AlertError("performance summary status is invalid")
    if (
        not isinstance(failures, int)
        or isinstance(failures, bool)
        or failures < 0
        or not isinstance(warnings, int)
        or isinstance(warnings, bool)
        or warnings < 0
        or not isinstance(benchmarks, list)
        or not benchmarks
        or len(benchmarks) > 100
    ):
        raise AlertError("performance summary counts or benchmarks are invalid")

    normalized: list[dict[str, Any]] = []
    for row in benchmarks:
        if not isinstance(row, dict):
            raise AlertError("performance benchmark row is malformed")
        benchmark_id = row.get("id")
        benchmark_status = row.get("status")
        if (
            not isinstance(benchmark_id, str)
            or not BENCHMARK_ID_RE.fullmatch(benchmark_id)
            or benchmark_status not in BENCHMARK_STATUSES
        ):
            raise AlertError("performance benchmark identity or status is invalid")
        copied = dict(row)
        for field in (
            "base_render_typical_ns",
            "head_render_typical_ns",
            "latency_change_percent",
            "head_render_ci_low_ns",
            "head_render_ci_high_ns",
            "head_render_sample_p95_ns",
            "head_memcpy_typical_ns",
            "head_render_to_memcpy",
            "ratio_change_percent",
            "efficiency_percent",
            "rebaseline_max_render_to_memcpy",
            "warn_percent",
            "fail_percent",
        ):
            copied[field] = finite_number(row.get(field), field)
        if (
            copied["rebaseline_max_render_to_memcpy"] is None
            or copied["rebaseline_max_render_to_memcpy"] <= 0
            or not isinstance(row.get("rebaseline_budget_exceeded"), bool)
        ):
            raise AlertError("performance rebaseline budget evidence is invalid")
        copied["rebaseline_budget_exceeded"] = row["rebaseline_budget_exceeded"]
        normalized.append(copied)

    computed_failures = sum(row["status"] in HARD_STATUSES for row in normalized)
    computed_warnings = sum(row["status"] in WARNING_STATUSES for row in normalized)
    computed_status = (
        "failure" if computed_failures else "warning" if computed_warnings else "success"
    )
    if (failures, warnings, status) != (
        computed_failures,
        computed_warnings,
        computed_status,
    ):
        raise AlertError("performance summary aggregate counts do not match its rows")
    return Report(
        status,
        failures,
        warnings,
        tuple(normalized),
        base_sha.lower(),
        head_sha.lower(),
    )


def desired_alerts(
    config: AlertConfig, report: Report | None, report_error: str | None
) -> set[str]:
    if report_error or report is None:
        return {"infrastructure"}
    statuses = {row["status"] for row in report.benchmarks}
    desired: set[str] = set()
    if "regression" in statuses:
        desired.add("regression")
    if statuses.intersection(
        {"not_comparable", "fixture_changed", "rebaseline_unapproved"}
    ):
        desired.add("integrity")
    expected_conclusion = "failure" if report.failures else "success"
    if config.workflow_conclusion != expected_conclusion:
        desired.add("infrastructure")
    return desired


def resolvable_alerts(config: AlertConfig, report: Report | None) -> set[str]:
    """Return alert kinds for which this run carries affirmative recovery proof."""

    if report is None:
        return set()
    statuses = {row["status"] for row in report.benchmarks}
    resolved: set[str] = set()
    # A changed/rebaselined/non-comparable fixture cannot prove that a prior
    # latency regression recovered; it measured a different or invalid page.
    if not statuses.intersection(
        {
            "regression",
            "not_comparable",
            "fixture_changed",
            "fixture_rebaselined",
            "rebaseline_unapproved",
        }
    ):
        resolved.add("regression")
    if not statuses.intersection(
        {"not_comparable", "fixture_changed", "rebaseline_unapproved"}
    ):
        resolved.add("integrity")
    expected_conclusion = "failure" if report.failures else "success"
    if config.workflow_conclusion == expected_conclusion:
        resolved.add("infrastructure")
    return resolved


def issue_state(issue: dict[str, Any], kind: str) -> tuple[int, int] | None:
    metadata = ALERTS[kind]
    user = issue.get("user")
    body = issue.get("body")
    if (
        issue.get("title") != metadata["title"]
        or not isinstance(user, dict)
        or user.get("login") != "github-actions[bot]"
        or not isinstance(body, str)
        or metadata["marker"] not in body
    ):
        return None
    match = STATE_RE.search(body)
    if not match:
        return None
    return int(match.group(1)), int(match.group(2))


def managed_issues(issues: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    managed = {kind: [] for kind in ALERTS}
    for issue in issues:
        if "pull_request" in issue:
            continue
        for kind in ALERTS:
            state = issue_state(issue, kind)
            number = issue.get("number")
            if state is not None and isinstance(number, int) and number > 0:
                copied = dict(issue)
                copied["_run_key"] = state
                managed[kind].append(copied)
    return managed


def baseline_state_from_issue(issue: dict[str, Any]) -> BaselineState | None:
    """Parse only the exact bot-authored issue used as durable baseline state."""

    user = issue.get("user")
    body = issue.get("body")
    number = issue.get("number")
    state = issue.get("state")
    if (
        issue.get("title") != BASELINE_STATE_TITLE
        or "pull_request" in issue
        or not isinstance(user, dict)
        or user.get("login") != "github-actions[bot]"
        or not isinstance(body, str)
        or BASELINE_STATE_MARKER not in body
        or not isinstance(number, int)
        or number <= 0
        or state not in {"open", "closed"}
    ):
        return None
    matches = BASELINE_STATE_RE.findall(body)
    if len(matches) != 1:
        return None
    sha, run_id, run_attempt = matches[0]
    return BaselineState(
        sha.lower(), int(run_id), int(run_attempt), number, state
    )


def baseline_states(issues: list[dict[str, Any]]) -> list[BaselineState]:
    return [
        state
        for issue in issues
        if (state := baseline_state_from_issue(issue)) is not None
    ]


def resolve_healthy_baseline(
    api: GitHubApi,
    repository: str,
    fallback_sha: str,
    *,
    require_state: bool,
    attempts: int = 1,
    retry_seconds: float = 0.0,
    sleep: Callable[[float], None] = time.sleep,
) -> str:
    """Resolve the bot-managed healthy anchor, failing closed after bootstrap.

    The fallback is accepted only while introducing a new baseline-state epoch.
    Once the epoch exists in the trusted base, absence or ambiguity must stop the
    benchmark instead of silently reverting to the immediately previous commit.
    """

    if not REPOSITORY_RE.fullmatch(repository):
        raise AlertError("repository must use OWNER/REPO syntax")
    if not SHA_RE.fullmatch(fallback_sha):
        raise AlertError("fallback baseline must be a full 40-character Git commit")
    if attempts <= 0 or retry_seconds < 0:
        raise AlertError("baseline resolution retry settings are invalid")

    for attempt in range(1, attempts + 1):
        states = baseline_states(api.issues(repository))
        if len(states) > 1:
            raise AlertError("multiple bot-managed healthy baseline states exist")
        if states:
            LOG.info(
                "performance_baseline action=resolved sha=%s run_id=%d attempt=%d",
                states[0].sha,
                states[0].run_id,
                states[0].run_attempt,
            )
            return states[0].sha
        if not require_state:
            LOG.warning(
                "performance_baseline action=bootstrap sha=%s", fallback_sha.lower()
            )
            return fallback_sha.lower()
        if attempt < attempts:
            LOG.warning(
                "performance_baseline action=waiting attempt=%d/%d retry_in=%.1fs",
                attempt,
                attempts,
                retry_seconds,
            )
            sleep(retry_seconds)
    raise AlertError(
        "bot-managed healthy baseline state is missing after bootstrap epoch"
    )


def baseline_body(config: AlertConfig, sha: str, report: Report) -> str:
    return "\n".join(
        [
            BASELINE_STATE_MARKER,
            f"<!-- proa-performance-baseline:{sha}:{config.run_id}:{config.run_attempt} -->",
            "",
            "## Trusted performance comparison anchor",
            "",
            f"The next main or scheduled benchmark must compare against `{sha}`.",
            "",
            f"- Last observed revision: `{report.head_sha}`",
            f"- Last comparison base: `{report.base_sha}`",
            f"- Last result: `{report.status}`",
            f"- [Workflow run]({config.run_url})",
            "",
            (
                "This bot-managed state prevents an unchanged slow revision from "
                "becoming healthy merely because it was compared with the immediately "
                "preceding regression. Warning and failed runs retain the prior anchor."
            ),
        ]
    )


def baseline_transition(
    config: AlertConfig,
    report: Report | None,
    issues: list[dict[str, Any]],
) -> tuple[BaselineState | None, str | None, str | None]:
    """Return existing state, next SHA, and any integrity error."""

    if report is None:
        return None, None, None
    states = baseline_states(issues)
    if len(states) > 1:
        return None, None, "multiple bot-managed healthy baseline states exist"
    existing = states[0] if states else None
    if existing is not None and report.base_sha.lower() != existing.sha:
        return (
            existing,
            None,
            "performance report did not use the recorded healthy baseline",
        )
    if existing is not None and existing.run_key > config.run_key:
        return existing, None, "healthy baseline state belongs to a newer run"

    current = existing.sha if existing is not None else report.base_sha.lower()
    statuses = {row["status"] for row in report.benchmarks}
    may_advance = (
        config.event == "push"
        and config.workflow_conclusion == "success"
        and bool(statuses)
        and statuses.issubset({"ok", "fixture_rebaselined"})
    )
    return existing, report.head_sha.lower() if may_advance else current, None


def persist_baseline_transition(
    api: GitHubApi,
    config: AlertConfig,
    report: Report,
    existing: BaselineState | None,
    next_sha: str,
) -> None:
    body = baseline_body(config, next_sha, report)
    if existing is None:
        api.create_issue(config.repository, BASELINE_STATE_TITLE, body)
        LOG.info("performance_baseline action=initialized sha=%s", next_sha)
        return

    if existing.sha != next_sha:
        api.comment(
            config.repository,
            existing.issue_number,
            (
                f"Healthy baseline advanced from `{existing.sha}` to `{next_sha}` "
                f"by [run {config.run_id}]({config.run_url})."
            ),
        )
    api.update_issue(
        config.repository, existing.issue_number, body, state="open"
    )
    LOG.info(
        "performance_baseline action=%s sha=%s",
        "advanced" if existing.sha != next_sha else "retained",
        next_sha,
    )


def fmt(value: Any, suffix: str = "") -> str:
    return "—" if value is None else f"{float(value):,.2f}{suffix}"


def evidence_table(report: Report | None) -> str:
    if report is None:
        return "No valid structured report was available."
    lines = [
        "| Benchmark | Status | Base | Head | Change | P95 | memcpy | Render/memcpy | Rebaseline ceiling | Ratio change | Efficiency |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in report.benchmarks:
        lines.append(
            "| `{id}` | {status} | {base} ns | {head} ns | {change} | {p95} ns | "
            "{memcpy} ns | {ratio}× | {ceiling}× | {ratio_change} | {efficiency} |".format(
                id=row["id"],
                status=row["status"],
                base=fmt(row.get("base_render_typical_ns")),
                head=fmt(row.get("head_render_typical_ns")),
                change=fmt(row.get("latency_change_percent"), "%"),
                p95=fmt(row.get("head_render_sample_p95_ns")),
                memcpy=fmt(row.get("head_memcpy_typical_ns")),
                ratio=fmt(row.get("head_render_to_memcpy")),
                ceiling=fmt(row.get("rebaseline_max_render_to_memcpy")),
                ratio_change=fmt(row.get("ratio_change_percent"), "%"),
                efficiency=fmt(row.get("efficiency_percent"), "%"),
            )
        )
    return "\n".join(lines)


def alert_body(
    config: AlertConfig,
    kind: str,
    report: Report | None,
    report_error: str | None,
) -> str:
    metadata = ALERTS[kind]
    descriptions = {
        "regression": (
            "At least one benchmark crossed the substantial latency threshold "
            "or the trusted rebaseline efficiency ceiling."
        ),
        "integrity": "At least one fixture changed unexpectedly or could not be compared.",
        "infrastructure": "The monitor did not produce a trustworthy completed result.",
    }
    mention = f"{config.mention}\n\n" if config.mention else ""
    error = f"\n\nMonitor detail: `{report_error}`" if report_error else ""
    return "\n".join(
        [
            str(metadata["marker"]),
            f"<!-- proa-performance-alert-run:{config.run_id}:{config.run_attempt} -->",
            "",
            f"{mention}## {metadata['title']}",
            "",
            descriptions[kind] + error,
            "",
            f"- Revision: `{config.sha}`",
            f"- Trigger: `{config.event}`",
            f"- Workflow conclusion: `{config.workflow_conclusion}`",
            f"- [Workflow run and retained artifacts]({config.run_url})",
            "",
            evidence_table(report),
            "",
            "This issue body preserves the key latency and empirical memcpy-floor evidence "
            "after the downloadable artifact expires.",
        ]
    )


def recovery_body(config: AlertConfig, kind: str, report: Report | None) -> str:
    metadata = ALERTS[kind]
    return "\n".join(
        [
            str(metadata["marker"]),
            f"<!-- proa-performance-alert-run:{config.run_id}:{config.run_attempt} -->",
            "",
            f"## {metadata['title']} — resolved",
            "",
            f"Resolved by `{config.sha}` from [this workflow run]({config.run_url}).",
            "",
            evidence_table(report),
        ]
    )


def duplicate_body(
    config: AlertConfig,
    kind: str,
    primary_number: int,
    report: Report | None,
) -> str:
    metadata = ALERTS[kind]
    return "\n".join(
        [
            str(metadata["marker"]),
            f"<!-- proa-performance-alert-run:{config.run_id}:{config.run_attempt} -->",
            "",
            f"## {metadata['title']} — duplicate",
            "",
            f"Closed as a duplicate of the bot-managed alert in #{primary_number}.",
            "This consolidation does not indicate that the underlying benchmark recovered.",
            "",
            f"Observed while reconciling [this workflow run]({config.run_url}).",
            "",
            evidence_table(report),
        ]
    )


def reconcile_alert(
    api: GitHubApi,
    config: AlertConfig,
    report: Report | None,
    report_error: str | None = None,
    *,
    log: Callable[[str], None] = LOG.info,
) -> str:
    validate_config(config)
    if not api.authoritative_run_is_current(config):
        log(
            f"performance_alert run_id={config.run_id} attempt={config.run_attempt} "
            "action=ignored_stale"
        )
        return "ignored_stale"

    repository_issues = api.issues(config.repository)
    baseline_existing, baseline_next, baseline_error = baseline_transition(
        config, report, repository_issues
    )
    effective_report_error = report_error or baseline_error
    existing = managed_issues(repository_issues)
    all_keys = [
        issue["_run_key"]
        for issues in existing.values()
        for issue in issues
    ]
    if all_keys and config.run_key < max(all_keys):
        log("performance_alert action=ignored_older_than_recorded_state")
        return "ignored_stale"

    desired = desired_alerts(config, report, effective_report_error)
    resolvable = (
        set()
        if effective_report_error is not None
        else resolvable_alerts(config, report)
    )
    actions: list[str] = []
    for kind, issues in existing.items():
        issues.sort(key=lambda issue: (issue["_run_key"], issue["number"]), reverse=True)
        primary = issues[0] if issues else None
        if kind in desired:
            body = alert_body(config, kind, report, effective_report_error)
            if primary is None:
                api.create_issue(config.repository, str(ALERTS[kind]["title"]), body)
                actions.append(f"created:{kind}")
            elif config.run_key >= primary["_run_key"]:
                number = int(primary["number"])
                if config.run_key > primary["_run_key"]:
                    api.comment(config.repository, number, body)
                api.update_issue(config.repository, number, body, state="open")
                actions.append(f"updated:{kind}")
        elif (
            kind in resolvable
            and primary is not None
            and primary.get("state") == "open"
            and config.run_key >= primary["_run_key"]
        ):
            number = int(primary["number"])
            body = recovery_body(config, kind, report)
            api.comment(config.repository, number, body)
            api.update_issue(config.repository, number, body, state="closed")
            actions.append(f"closed:{kind}")

        for duplicate in issues[1:]:
            if duplicate.get("state") != "open" or config.run_key < duplicate["_run_key"]:
                continue
            number = int(duplicate["number"])
            primary_number = int(primary["number"])
            body = duplicate_body(config, kind, primary_number, report)
            api.comment(config.repository, number, body)
            api.update_issue(config.repository, number, body, state="closed")
            actions.append(f"deduplicated:{kind}")

    if (
        report is not None
        and effective_report_error is None
        and baseline_next is not None
    ):
        persist_baseline_transition(
            api,
            config,
            report,
            baseline_existing,
            baseline_next,
        )

    if not actions:
        actions.append("healthy" if not desired else "idempotent")
    result = ",".join(actions)
    log(
        f"performance_alert run_id={config.run_id} attempt={config.run_attempt} "
        f"desired={','.join(sorted(desired)) or 'none'} "
        f"resolvable={','.join(sorted(resolvable)) or 'none'} action={result}"
    )
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow-path", required=True)
    parser.add_argument("--run-url", required=True)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--event", required=True)
    parser.add_argument("--workflow-conclusion", required=True)
    parser.add_argument("--summary", type=pathlib.Path, required=True)
    parser.add_argument("--mention", default="")
    return parser.parse_args()


def parse_baseline_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Resolve the durable healthy performance baseline"
    )
    parser.add_argument("resolve-baseline", nargs="?")
    parser.add_argument("--repository", required=True)
    parser.add_argument("--fallback-sha", required=True)
    parser.add_argument("--require-state", action="store_true")
    parser.add_argument("--attempts", type=int, default=1)
    parser.add_argument("--retry-seconds", type=float, default=0.0)
    return parser.parse_args()


def append_step_summary(action: str) -> None:
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with pathlib.Path(path).open("a", encoding="utf-8") as summary:
            summary.write(f"## Performance alert\n\nReconciliation: `{action}`.\n")


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "resolve-baseline":
        args = parse_baseline_args()
        logging.basicConfig(
            level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s"
        )
        try:
            baseline = resolve_healthy_baseline(
                GitHubApi(os.environ.get("GITHUB_TOKEN", "")),
                args.repository,
                args.fallback_sha,
                require_state=args.require_state,
                attempts=args.attempts,
                retry_seconds=args.retry_seconds,
            )
        except AlertError as error:
            print(f"performance baseline resolution failed: {error}", file=sys.stderr)
            return 1
        print(baseline)
        return 0

    args = parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    config = AlertConfig(
        repository=args.repository,
        workflow_path=args.workflow_path,
        run_url=args.run_url,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        sha=args.sha,
        event=args.event,
        workflow_conclusion=args.workflow_conclusion,
        mention=args.mention,
    )
    report: Report | None = None
    report_error: str | None = None
    try:
        validate_config(config)
        report = load_report(args.summary, config.sha)
    except AlertError as error:
        report_error = str(error)
    try:
        action = reconcile_alert(
            GitHubApi(os.environ.get("GITHUB_TOKEN", "")),
            config,
            report,
            report_error,
        )
    except AlertError as error:
        print(f"performance alert failed: {error}", file=sys.stderr)
        return 1
    append_step_summary(action)
    return 0


if __name__ == "__main__":
    sys.exit(main())
