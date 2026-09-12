#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Decide whether a pull request needs the linked acceptance suite.

The linked acceptance shards (scripts/run_proa_cli_acceptance.sh) exercise
proa-cli, the RSJS pipeline, and the example applications. None of that
depends on the marketing site, the documentation, the deployment notes, or
the agent instructions, so a pull request that touches only those paths skips
the suite. Everything else runs it, and so does anything this script cannot
classify: an API error, an unknown event, or a pull request too large to list.

CI usage: writes ``linked=true`` or ``linked=false`` to ``$GITHUB_OUTPUT`` and
explains the decision on stdout. Needs ``GITHUB_TOKEN`` with pull-requests
read access when ``GITHUB_EVENT_NAME`` is ``pull_request``.
"""

from __future__ import annotations

import fnmatch
import json
import os
import posixpath
import sys
import urllib.request
from collections.abc import Iterable

# Paths whose changes cannot alter what the linked suite builds or asserts.
# Keep this list narrow and directory-shaped: a pattern that matched inside a
# crate that the suite compiles would silently drop coverage.
SKIPPABLE_PATTERNS: tuple[str, ...] = (
    "proa_marketing/**",
    "docs/**",
    "deploy/**",
    ".claude/**",
    "**/*.md",
    "**/*.mdx",
)

# The GitHub API lists at most this many files for one pull request. Beyond
# it the listing is incomplete, so the suite runs.
MAX_LISTED_FILES = 3000
PAGE_SIZE = 100


def matches(path: str, pattern: str) -> bool:
    """Match a repository-relative path against one skippable pattern."""
    normalized = posixpath.normpath(path)
    if pattern.endswith("/**"):
        prefix = pattern[: -len("/**")]
        return normalized == prefix or normalized.startswith(prefix + "/")
    if pattern.startswith("**/"):
        return fnmatch.fnmatchcase(posixpath.basename(normalized), pattern[len("**/") :])
    return fnmatch.fnmatchcase(normalized, pattern)


def is_skippable(path: str) -> bool:
    return any(matches(path, pattern) for pattern in SKIPPABLE_PATTERNS)


def needs_linked_acceptance(paths: Iterable[str]) -> bool:
    """True unless every changed path is skippable. An empty list runs the suite."""
    listed = list(paths)
    if not listed:
        return True
    return not all(is_skippable(path) for path in listed)


def changed_files(repo: str, number: str, token: str, api_url: str) -> list[str]:
    """Every path a pull request touches, including the old name of a rename."""
    files: list[str] = []
    page = 1
    while True:
        url = f"{api_url}/repos/{repo}/pulls/{number}/files?per_page={PAGE_SIZE}&page={page}"
        request = urllib.request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "User-Agent": "proa-ci-linked-acceptance-paths",
                "X-GitHub-Api-Version": "2022-11-28",
            },
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            batch = json.load(response)
        if not isinstance(batch, list):
            raise RuntimeError(f"unexpected response shape from {url}")
        for entry in batch:
            files.append(entry["filename"])
            previous = entry.get("previous_filename")
            if previous:
                files.append(previous)
        if len(batch) < PAGE_SIZE:
            break
        page += 1
        if page * PAGE_SIZE > MAX_LISTED_FILES:
            raise RuntimeError(f"pull request lists more than {MAX_LISTED_FILES} files")
    return files


def emit(needed: bool, reason: str, output_path: str | None) -> None:
    value = "true" if needed else "false"
    print(f"linked acceptance {'runs' if needed else 'is skipped'}: {reason}")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write(f"linked={value}\n")


def main() -> int:
    output_path = os.environ.get("GITHUB_OUTPUT")
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    if event != "pull_request":
        emit(True, f"event is {event or 'unknown'}, not a pull request", output_path)
        return 0

    try:
        files = changed_files(
            repo=os.environ["GITHUB_REPOSITORY"],
            number=os.environ["PR_NUMBER"],
            token=os.environ["GITHUB_TOKEN"],
            api_url=os.environ.get("GITHUB_API_URL", "https://api.github.com"),
        )
    except Exception as error:  # noqa: BLE001 - any failure means "run the suite"
        emit(True, f"could not list the pull request's files ({error})", output_path)
        return 0

    forcing = sorted(path for path in files if not is_skippable(path))
    if not files:
        emit(True, "the pull request lists no changed files", output_path)
    elif forcing:
        shown = ", ".join(forcing[:5])
        more = f" and {len(forcing) - 5} more" if len(forcing) > 5 else ""
        emit(True, f"{len(forcing)} of {len(files)} changed paths need it ({shown}{more})", output_path)
    else:
        emit(False, f"all {len(files)} changed paths are documentation, marketing, or deployment content", output_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
