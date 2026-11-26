#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Select affected Cargo packages and execute the existing Rust CI phases."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import subprocess
import time
from pathlib import Path, PurePosixPath

from linked_acceptance_paths import changed_files

ROOT = Path(__file__).resolve().parents[1]
LIB_EXCLUDE = {
    "proa_demo",
    "proa_marketing",
    "proa_showcase_web",
    "proa_webmcp_pizza",
}
INTEGRATION_EXCLUDE = LIB_EXCLUDE | {"proa_showcase", "proa-cli"}
LINT_PACKAGES = {
    "proa_facade_example",
    "proa_core",
    "proa_stream",
    "proa_framework_core",
    "proa_framework_axum",
    "proa_marketing",
    "proa-cli",
    "proa_webmcp_pizza",
}
FEATURE_CHECKS = [
    (["proa"], ["--no-default-features"]),
    (["proa"], ["--all-features"]),
    (["proa_stream"], []),
    (["proa_framework_axum"], []),
    (["proa_framework_core"], ["--no-default-features"]),
    (["proa_fonts", "rsjs_transport"], ["--no-default-features"]),
    (["proa_macros", "proa_dsl"], ["--no-default-features"]),
]


def affected(metadata: dict, paths: list[str]) -> set[str] | None:
    """None means full validation; unknown/shared inputs never skip coverage."""
    if not paths:
        return None
    root = Path(metadata["workspace_root"])
    packages = {p["name"]: p for p in metadata["packages"]}
    owners = sorted(
        (
            (Path(p["manifest_path"]).parent.relative_to(root).as_posix(), p["name"])
            for p in packages.values()
        ),
        key=lambda pair: len(pair[0]),
        reverse=True,
    )
    selected = set()
    for path in paths:
        parts = PurePosixPath(path).parts
        if not parts or ".." in parts or path.startswith("/") or ".cargo" in parts:
            return None
        if PurePosixPath(path).name in {
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "rust-toolchain",
        }:
            return None
        owner = next(
            (name for folder, name in owners if path.startswith(folder + "/")), None
        )
        if owner is None:
            return None
        selected.add(owner)
    dependents = {name: set() for name in packages}
    for package in packages.values():
        # metadata includes optional, target-specific, build and dev dependencies.
        for dependency in package["dependencies"]:
            name = dependency["name"]
            if name in packages and dependency.get("path"):
                dependents[name].add(package["name"])
    # Runtime fixture reads and generated Cargo manifests are not Cargo edges.
    runtime_consumers = {
        "proa_macros": {"proa_source_tools", "rsjs_transport"},
        "mcp-incident-desk": {"proa_source_tools"},
        "proa_core": {"rsjs_transport"},
        "rsjs": {"rsjs_transport"},
        "proa_docs": {"proa_docs_build"},
    }
    for provider, consumers in runtime_consumers.items():
        if provider in dependents:
            dependents[provider].update(consumers & packages.keys())
    # Quickstart acceptance extracts examples from this page, outside Cargo's graph.
    if "proa_marketing/src/docs/index.mdx" in paths:
        selected.add("proa-cli")
    pending = list(selected)
    while pending:
        for name in dependents[pending.pop()] - selected:
            selected.add(name)
            pending.append(name)
    return selected


def make_plan(metadata: dict, paths: list[str] | None) -> dict:
    selected = affected(metadata, paths) if paths is not None else None
    names = {p["name"] for p in metadata["packages"]}
    return {
        "full": selected is None,
        "packages": sorted(names if selected is None else selected),
        "libraries": sorted(
            p["name"]
            for p in metadata["packages"]
            if any(
                kind in {"lib", "proc-macro"}
                for t in p["targets"]
                for kind in t["kind"]
            )
        ),
        "integration": sorted(
            p["name"]
            for p in metadata["packages"]
            if any("test" in t["kind"] for t in p["targets"])
        ),
    }


def selection(plan: dict, excluded: set[str], eligible: list[str]) -> list[str]:
    if plan["full"]:
        return ["--workspace"] + [
            arg for name in sorted(excluded) for arg in ("--exclude", name)
        ]
    names = sorted(set(plan["packages"]) & set(eligible) - excluded)
    return [arg for name in names for arg in ("-p", name)]


def commands(phase: str, plan: dict) -> list[list[str]]:
    packages = set(plan["packages"])
    cargo = ["cargo", phase, "--locked", "--timings"]
    if phase == "clippy":
        return [
            cargo
            + selection(plan, set(), plan["packages"])
            + ["--all-targets", "--", "-A", "clippy::all", "-D", "clippy::correctness"]
        ]
    if phase == "features":
        return [
            ["cargo", "check", "--locked", "--timings"]
            + [arg for p in names for arg in ("-p", p)]
            + flags
            + ["--lib"]
            for names, flags in FEATURE_CHECKS
            if packages.intersection(names)
        ]
    if phase == "libraries":
        args = selection(plan, LIB_EXCLUDE, plan["libraries"])
        return (
            [["cargo", "test", "--locked", "--timings", *args, "--lib"]] if args else []
        )
    if phase == "integration":
        args = selection(plan, INTEGRATION_EXCLUDE, plan["integration"])
        features = (
            ["--features", "proa_core/html-to-markdown"]
            if "proa_core" in packages
            else []
        )
        result = (
            [
                [
                    "cargo",
                    "test",
                    "--locked",
                    "--timings",
                    *args,
                    *features,
                    "--test",
                    "*",
                ]
            ]
            if args
            else []
        )
        if "proa_core" in packages:
            result.append(
                [
                    "cargo",
                    "test",
                    "-p",
                    "proa_core",
                    "--features",
                    "html-to-markdown",
                    "--locked",
                    "--doc",
                ]
            )
        return result
    raise ValueError(phase)


def source_timestamps(state: Path, restore: bool) -> None:
    """Retain Cargo freshness only for tracked files with identical bytes/mode."""
    try:
        previous = json.loads(state.read_text()) if restore and state.exists() else {}
    except (OSError, ValueError):
        previous = {}  # A missing/corrupt cache must cause a rebuild, not a skip.
    if not isinstance(previous, dict):
        previous = {}
    paths = (
        subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT)
        .decode()
        .split("\0")
    )
    links = {
        p: os.readlink(ROOT / p) for p in filter(None, paths) if (ROOT / p).is_symlink()
    }
    files = {p for p in filter(None, paths) if p not in links and (ROOT / p).is_file()}
    old_files = previous.get("files", {})
    # New/deleted inputs and changed symlinks can alter module resolution even
    # when existing source bytes match. Let Cargo see the fresh checkout then.
    if restore and (
        not isinstance(old_files, dict)
        or set(old_files) != files
        or previous.get("links") != links
    ):
        print(
            "Source layout changed or cache metadata unavailable; keeping fresh timestamps."
        )
        return
    current = {}
    restored = 0
    for relative in sorted(files):
        path = ROOT / relative
        before = path.stat()
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        after = path.stat()
        if (before.st_mtime_ns, before.st_size) != (after.st_mtime_ns, after.st_size):
            continue
        old = old_files.get(relative, {})
        if restore:
            if (
                isinstance(old, dict)
                and old.get("sha256") == digest
                and old.get("mode") == before.st_mode
                and isinstance(old.get("mtime_ns"), int)
            ):
                os.utime(path, ns=(before.st_atime_ns, old["mtime_ns"]))
                restored += 1
        else:
            current[relative] = {
                "sha256": digest,
                "mode": before.st_mode,
                "mtime_ns": before.st_mtime_ns,
            }
    if restore:
        print(f"Reused timestamps for {restored} unchanged tracked files.")
    else:
        state.parent.mkdir(parents=True, exist_ok=True)
        state.write_text(json.dumps({"files": current, "links": links}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    phases = parser.add_subparsers(dest="phase", required=True)
    for phase in (
        "plan",
        "clippy",
        "features",
        "libraries",
        "integration",
        "restore-mtimes",
        "save-mtimes",
    ):
        command = phases.add_parser(phase)
        command.add_argument(
            "--state" if phase.endswith("mtimes") else "--plan",
            dest="data",
            type=Path,
            required=True,
        )
    args = parser.parse_args()
    if args.phase in {"restore-mtimes", "save-mtimes"}:
        source_timestamps(args.data, restore=args.phase == "restore-mtimes")
        return
    if args.phase == "plan":
        paths = None
        if os.environ.get("GITHUB_EVENT_NAME") == "pull_request":
            try:
                paths = changed_files(
                    os.environ["GITHUB_REPOSITORY"],
                    os.environ["PR_NUMBER"],
                    os.environ["GITHUB_TOKEN"],
                    os.environ.get("GITHUB_API_URL", "https://api.github.com"),
                )
            except Exception as error:  # noqa: BLE001 - uncertainty must select the full suite
                print(f"Cannot classify changed paths; using full validation: {error}")
        metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--no-deps",
                    "--all-features",
                    "--format-version",
                    "1",
                ],
                cwd=ROOT,
            )
        )
        plan = make_plan(metadata, paths)
        args.data.write_text(json.dumps(plan))
        print(
            f"Rust validation: {'full workspace' if plan['full'] else 'affected packages'}: {', '.join(plan['packages'])}"
        )
        if plan["full"] and paths is not None:
            print(
                "Shared/unclassified inputs require full validation; changed paths include: "
                + ", ".join(paths[:10])
            )
        if output := os.environ.get("GITHUB_OUTPUT"):
            packages = set(plan["packages"])
            flags = {
                "cli": bool(packages & LINT_PACKAGES),
                "cli-tests": "proa-cli" in packages,
                "bindings": "rsjs_web" in packages,
                "rsjs": any(p.startswith("rsjs") for p in packages),
            }
            with open(output, "a") as stream:
                stream.writelines(
                    f"{key}={str(value).lower()}\n" for key, value in flags.items()
                )
        return
    plan = json.loads(args.data.read_text())
    for command in commands(args.phase, plan):
        print("$ " + shlex.join(command), flush=True)
        started = time.monotonic()
        subprocess.run(command, cwd=ROOT, check=True)
        print(
            f"{args.phase}: {time.monotonic() - started:.1f}s (includes nested builds during tests)",
            flush=True,
        )


if __name__ == "__main__":
    main()
