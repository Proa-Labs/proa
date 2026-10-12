#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""
Publish selected Proa workspace crates to crates.io after ownership review.

Usage:
    cargo login  # or configure CARGO_REGISTRY_TOKEN through your secret manager
    scripts/publish.py [--dry-run] [--skip-published] [--only crate1,crate2]

Dry runs execute the policy/archive preflight and print the dependency order;
they do not invoke `cargo publish`, because unpublished in-workspace
dependencies cannot all be registry-verified before the bootstrap begins. This
script intentionally does not discover credentials from another checkout or
from deployment infrastructure.
See docs/registry.md for the repository-side release policy.
"""

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from email.utils import parsedate_to_datetime

REPO_ROOT = Path(__file__).resolve().parent.parent
REGISTRY = "crates-io"
REGISTRY_URL = "https://crates.io"
INDEX_POLL_SECONDS = 5
INDEX_TIMEOUT_SECONDS = 30 * 60


def check_release_preflight() -> int:
    """Verify registry policy plus the exact crate archives before publishing."""
    for script in ("check_publish_policy.py", "check_crate_archives.py"):
        result = subprocess.run(
            [sys.executable, str(REPO_ROOT / "scripts" / script)],
            cwd=REPO_ROOT,
        )
        if result.returncode != 0:
            return result.returncode
    return 0


def discover_crates():
    """Return publishable root-workspace crates from Cargo's locked metadata."""
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    crates: dict[str, tuple[Path, str, list[str]]] = {}
    for package in json.loads(result.stdout)["packages"]:
        if package.get("publish") != [REGISTRY]:
            continue
        dependencies = [
            dependency["name"]
            for dependency in package.get("dependencies", [])
            if dependency.get("kind") != "dev" and dependency.get("path") is not None
        ]
        manifest = Path(package["manifest_path"])
        crates[package["name"]] = (
            manifest.parent,
            package["version"],
            dependencies,
        )
    return crates


def topo_sort(crates: dict) -> list[str]:
    """Topological sort of the dep graph; deps come before dependents."""
    ordered: list[str] = []
    visited: set[str] = set()
    visiting: set[str] = set()

    def visit(n: str):
        if n in visited:
            return
        if n in visiting:
            raise ValueError(f"dependency cycle detected at {n}")
        visiting.add(n)
        for dep in crates.get(n, (None, None, []))[2]:
            if dep in crates:
                visit(dep)
        visiting.discard(n)
        visited.add(n)
        ordered.append(n)

    # Scaffolds emitted by the CLI depend on libraries beyond the CLI's own
    # build graph. Publish those libraries before making the CLI installable.
    for name in crates:
        if name != "proa-cli":
            visit(name)
    if "proa-cli" in crates:
        visit("proa-cli")
    return ordered


def list_remote_versions(names: list[str]) -> dict[str, set[str]]:
    """Read exact published versions, including older versions, without a token."""
    published: dict[str, set[str]] = {}
    for name in names:
        request = urllib.request.Request(
            f"{REGISTRY_URL}/api/v1/crates/{urllib.parse.quote(name, safe='')}",
            headers={"User-Agent": "proa-publish (https://github.com/Proa-Labs/proa)"},
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                data = json.loads(response.read())
                published[name] = {version["num"] for version in data["versions"]}
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
            published[name] = set()
    return published


def publish_one(name: str, no_verify: bool = False) -> tuple[int, str]:
    """Run cargo publish; return (returncode, classified_status).

    classified_status is one of:
      "ok"            - publish succeeded
      "already"       - already published at this version (treat as skip)
      "error"         - real failure; caller should bail
    """
    if name == "proa-cli":
        preflight = subprocess.run(
            ["cargo", "test", "--locked", "-p", name, "--lib"],
            cwd=REPO_ROOT,
        )
        if preflight.returncode:
            return preflight.returncode, "error"
    cmd = ["cargo", "publish", "--registry", REGISTRY, "-p", name]
    if no_verify:
        cmd.append("--no-verify")
    print(f"\n=== publishing {name} ===")
    print(f"$ {' '.join(cmd)}")
    while True:
        proc = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        retry = rate_limit_retry_at(proc.stdout + proc.stderr) if proc.returncode else None
        if retry is None:
            break
        print(f"crates.io rate limit: retrying {name} after the server's requested time")
        while (remaining := retry - time.time()) > 0:
            print(f"rate-limit wait: {name}, {int(remaining) + 1}s remaining", flush=True)
            time.sleep(min(60, remaining))
    if proc.returncode == 0:
        return 0, "ok"
    # Detect "already published at this version" so re-runs are idempotent.
    combined = proc.stdout + proc.stderr
    already_markers = [
        "is already uploaded",
        "already exists on",
        "already published version",
    ]
    if any(marker in combined for marker in already_markers):
        return proc.returncode, "already"
    return proc.returncode, "error"


def rate_limit_retry_at(output: str) -> float | None:
    """Honor crates.io's explicit retry date; never retry an unrelated failure."""
    if "status 429 Too Many Requests" not in output:
        return None
    match = re.search(r"Please try again after (.+? GMT)", output)
    if match is None:
        return None
    try:
        return parsedate_to_datetime(match.group(1)).timestamp() + 2
    except (ValueError, TypeError, OverflowError):
        return None


def wait_until_available(name: str, version: str) -> bool:
    """Wait until Cargo can resolve an uploaded version from the sparse index."""
    deadline = time.monotonic() + INDEX_TIMEOUT_SECONDS
    package = f"{name}@{version}"
    while time.monotonic() < deadline:
        result = subprocess.run(
            ["cargo", "info", "--registry", REGISTRY, package],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            print(f"registry index now resolves {package}")
            return True
        print(
            f"waiting for {package} to become visible in the {REGISTRY} registry..."
        )
        time.sleep(INDEX_POLL_SECONDS)
    print(
        f"ERROR: {package} did not become visible within "
        f"{INDEX_TIMEOUT_SECONDS // 60} minutes",
        file=sys.stderr,
    )
    return False


def main():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="run preflight and print the publish plan without invoking Cargo publish",
    )
    parser.add_argument(
        "--skip-published",
        action="store_true",
        help="skip crates already on the registry at the current version",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="pass --no-verify to cargo publish (skip local rebuild verification)",
    )
    parser.add_argument(
        "--only",
        help="comma-separated list of crate names to publish (others skipped)",
    )
    args = parser.parse_args()

    preflight_rc = check_release_preflight()
    if preflight_rc != 0:
        return preflight_rc

    crates = discover_crates()
    order = topo_sort(crates)

    if args.only:
        wanted = {name.strip() for name in args.only.split(",") if name.strip()}
        missing = wanted - set(order)
        if missing:
            print(f"WARN: --only requested but not in workspace or not publishable: {sorted(missing)}", file=sys.stderr)
        order = [n for n in order if n in wanted]

    print(f"Publish order ({len(order)} crates):")
    for n in order:
        print(f"  {n} v{crates[n][1]}")

    if not order:
        print("Nothing to publish.")
        return 0

    if args.dry_run:
        print(
            "\nDry run complete: preflight passed; no uploads or Cargo publish "
            "verification were attempted."
        )
        return 0

    # Cargo resolves its standard credential providers (including cargo login
    # and CARGO_REGISTRY_TOKEN). Do not read or print publisher credentials here.
    if args.skip_published:
        try:
            published = list_remote_versions(order)
        except (OSError, ValueError, KeyError) as error:
            print(f"ERROR: cannot check crates.io versions: {error}", file=sys.stderr)
            return 1
        before = len(order)
        order = [n for n in order if crates[n][1] not in published.get(n, set())]
        skipped = before - len(order)
        if skipped:
            print(f"\nSkipping {skipped} crate(s) already at target version on registry")
        if not order:
            print("All target versions already published.")
            return 0

    stats = {"ok": 0, "already": 0, "error": 0}
    for i, n in enumerate(order, 1):
        print(f"\n[{i}/{len(order)}] {n}")
        rc, status = publish_one(n, no_verify=args.no_verify)
        stats[status] += 1
        if status == "already":
            print(f"NOTE: {n} already on registry at this version; continuing.")
        elif status == "error":
            print(f"ERROR: cargo publish failed for {n}", file=sys.stderr)
            return rc
        if status == "ok":
            if not wait_until_available(n, crates[n][1]):
                return 1

    print(f"\nDone. published={stats['ok']} already={stats['already']} errors={stats['error']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
