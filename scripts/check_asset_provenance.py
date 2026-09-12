#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Require provenance for redistributed binary and captured fixture assets."""

from __future__ import annotations

import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "docs" / "asset-provenance.md"
IGNORED_PARTS = {".git", "node_modules", "target"}

BINARY_EXTENSIONS = {
    ".gif",
    ".ico",
    ".jpeg",
    ".jpg",
    ".mov",
    ".mp4",
    ".otf",
    ".pdf",
    ".png",
    ".ttf",
    ".webm",
    ".webp",
    ".woff",
    ".woff2",
    ".zip",
}
TEXT_EXTENSIONS = {
    ".css",
    ".html",
    ".js",
    ".json",
    ".marko",
    ".md",
    ".mdx",
    ".mjs",
    ".rs",
    ".stpl",
    ".ts",
    ".tsx",
}
FORBIDDEN_REMOTE_ORIGINS = (
    "a0.muscache.example",
    "rover.ebay.com",
    "secureir.ebaystatic.com",
)
# Keep these as exact host-to-file grants. Adding a new retained remote reference
# must identify both its origin and the specific fixture that needs it.
RETAINED_REMOTE_ORIGIN_PATHS: dict[str, frozenset[str]] = {
    "a0.muscache.com": frozenset(
        {
            "proa_demo/react-ssr-shared/src/data.ts",
            "proa_demo/react-ssr-shared/src/listing.tsx",
            "proa_showcase/src/airbnb/parity.rs",
            "proa_showcase/src/airbnb/shared.css",
        }
    ),
    "a.espncdn.com": frozenset(
        {"proa_showcase/src/espn/data/summary-401695777.json"}
    ),
    "github.githubassets.com": frozenset(
        {
            "proa_demo/react-ssr-shared/src/github-pr-diff.tsx",
            "proa_demo/src/github_pr_diff.rs",
        }
    ),
    "i.ebayimg.com": frozenset(
        {
            "proa_demo/marko-server/page_srp_real.marko",
            "proa_demo/react-ssr/page.tsx",
            "proa_showcase/src/ebay/data/shell_post.html",
            "proa_showcase/src/ebay/data/shell_pre.html",
            "proa_showcase/src/ebay/srp.rs",
        }
    ),
    "ir.ebaystatic.com": frozenset(
        {
            "proa_demo/marko-server/page_srp_real.marko",
            "proa_demo/react-ssr/chrome.generated.tsx",
            "proa_showcase/src/ebay/data/shell_post.html",
            "proa_showcase/src/ebay/data/shell_pre.html",
            "proa_showcase/src/ebay/data/srp_chrome_reference.html",
            "proa_showcase/src/ebay/srp_chrome_real.rs",
        }
    ),
    "site.api.espn.com": frozenset({"proa_showcase/src/espn/data/CAPTURE.md"}),
}
SCANNED_PREFIXES = ("proa_bench/", "proa_demo/", "proa_marketing/", "proa_showcase/")


def repository_files() -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )
    if result.returncode == 0:
        return sorted(
            path
            for path in result.stdout.decode().split("\0")
            if path and (ROOT / path).is_file()
        )

    return sorted(
        path.relative_to(ROOT).as_posix()
        for path in ROOT.rglob("*")
        if path.is_file() and not IGNORED_PARTS.intersection(path.parts)
    )


def manifest_patterns() -> list[str]:
    text = MANIFEST.read_text(encoding="utf-8")
    return re.findall(r"^\|\s*`([^`]+)`\s*\|", text, flags=re.MULTILINE)


def is_fixture_asset(path: str) -> bool:
    suffix = Path(path).suffix.lower()
    if path == "proa_demo/showcase-props.json":
        return True
    return (
        path.startswith("proa_showcase/src/")
        and "/data/" in path
        and suffix in {".css", ".html", ".json"}
    )


def allows_retained_remote_origin(path: str, origin: str) -> bool:
    return path in RETAINED_REMOTE_ORIGIN_PATHS.get(origin, ())


def main() -> int:
    files = repository_files()
    file_set = set(files)
    patterns = manifest_patterns()
    errors: list[str] = []
    scanned_text: dict[str, str] = {}

    if not patterns:
        errors.append(f"{MANIFEST.relative_to(ROOT)} has no manifest path rows")

    for pattern in patterns:
        if not any(fnmatch.fnmatch(path, pattern) for path in files):
            errors.append(f"stale provenance manifest pattern: {pattern}")

    for path in files:
        suffix = Path(path).suffix.lower()
        if suffix in BINARY_EXTENSIONS or is_fixture_asset(path):
            if not any(fnmatch.fnmatch(path, pattern) for pattern in patterns):
                errors.append(f"missing provenance manifest entry: {path}")

        if suffix in TEXT_EXTENSIONS and path.startswith(SCANNED_PREFIXES):
            text = (ROOT / path).read_text(encoding="utf-8", errors="ignore")
            scanned_text[path] = text
            for origin in FORBIDDEN_REMOTE_ORIGINS:
                if origin in text:
                    line = text[: text.index(origin)].count("\n") + 1
                    errors.append(f"forbidden remote fixture origin in {path}:{line}: {origin}")
            for origin in RETAINED_REMOTE_ORIGIN_PATHS:
                if origin in text and not allows_retained_remote_origin(path, origin):
                    line = text[: text.index(origin)].count("\n") + 1
                    errors.append(
                        f"unmanifested remote fixture origin in {path}:{line}: {origin}"
                    )

    for origin, allowed_paths in RETAINED_REMOTE_ORIGIN_PATHS.items():
        for path in sorted(allowed_paths):
            if path not in file_set:
                errors.append(f"stale remote fixture allowance: {origin} in {path}")
            elif origin not in scanned_text.get(path, ""):
                errors.append(f"unused remote fixture allowance: {origin} in {path}")

    if errors:
        print("Asset provenance check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(
        f"Asset provenance check passed "
        f"({len(patterns)} manifest patterns, {len(files)} repository files)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
