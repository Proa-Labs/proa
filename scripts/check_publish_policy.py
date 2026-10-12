#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Validate crates.io and release-plz wiring for workspace crates."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ALLOWED_REGISTRY = "crates-io"
REPO_ROOT = Path(__file__).resolve().parent.parent
RELEASE_PLZ_CONFIG = REPO_ROOT / "release-plz.toml"
RELEASE_PLZ_SCRIPT = REPO_ROOT / "scripts" / "release-plz.sh"
WORKSPACE_MANIFEST = REPO_ROOT / "Cargo.toml"
STANDALONE_BOOTSTRAP_BASELINES = {
    "proa_const_json": "0.1.2",
    "proa_const_json_macros": "0.1.2",
    "itoa_lut": "0.1.2",
    "proa_actions": "0.3.0",
    "proa_codegen": "0.2.0",
    "proa_core": "0.3.0",
    "proa_docs": "0.2.0",
    "proa_docs_build": "0.2.0",
    "proa_dsl": "0.1.3",
    "proa_framework_axum": "0.3.0",
    "proa_framework_core": "0.3.0",
    "proa_ir": "0.1.2",
    "proa_macros": "0.3.0",
    "proa_og": "0.1.2",
    "proa_react": "0.3.0",
    "proa_stream": "0.3.0",
    "rsjs": "0.2.0",
    "rsjs_ir": "0.2.0",
    "rsjs_macros": "0.2.0",
    "rsjs_web": "0.2.0",
    "proa_tailwind_merge": "0.1.2",
    "proa_tailwind_merge_macro": "0.1.2",
}


def stable_semver(version: str) -> tuple[int, int, int] | None:
    """Parse the stable three-part versions used by the bootstrap policy."""
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if match is None:
        return None
    return tuple(int(part) for part in match.groups())


def check_bootstrap_package_minimums(
    packages: list[dict], publishable: set[str], errors: list[str]
) -> None:
    """Ensure immutable pre-standalone registry versions cannot be reused."""
    packages_by_name = {package["name"]: package for package in packages}
    for name, baseline in STANDALONE_BOOTSTRAP_BASELINES.items():
        package = packages_by_name.get(name)
        if package is None or name not in publishable:
            errors.append(
                f"{name}: standalone bootstrap package is missing or not publishable"
            )
            continue
        current = package["version"]
        parsed_current = stable_semver(current)
        parsed_baseline = stable_semver(baseline)
        if parsed_current is None or parsed_baseline is None:
            errors.append(
                f"{name}: bootstrap versions must be stable x.y.z values; "
                f"got current {current!r}, baseline {baseline!r}"
            )
        elif parsed_current < parsed_baseline:
            errors.append(
                f"{name}: package version {current} predates the standalone "
                f"bootstrap minimum {baseline}"
            )


def load_workspace_packages() -> list[dict]:
    metadata = subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
    )
    return json.loads(metadata.stdout)["packages"]


def load_release_plz_config() -> dict:
    with RELEASE_PLZ_CONFIG.open("rb") as f:
        return tomllib.load(f)


def load_workspace_rust_version() -> str | None:
    with WORKSPACE_MANIFEST.open("rb") as f:
        manifest = tomllib.load(f)
    return manifest.get("workspace", {}).get("package", {}).get("rust-version")


def release_plz_packages(config: dict) -> set[str]:
    packages = config.get("package", [])
    return {package["name"] for package in packages if package.get("release") is True}


def check_bootstrap_dependency_minimums(
    packages: list[dict], publishable: set[str], errors: list[str]
) -> None:
    """Keep packaged manifests from resolving pre-standalone dependencies."""
    for package in packages:
        if package["name"] not in publishable:
            continue
        for dependency in package.get("dependencies", []):
            if dependency.get("kind") == "dev" or dependency.get("path") is None:
                continue
            dependency_name = dependency["name"]
            baseline = STANDALONE_BOOTSTRAP_BASELINES.get(dependency_name)
            if baseline is None:
                continue
            requirement = dependency.get("req")
            requirement_floor = (
                stable_semver(requirement[1:])
                if isinstance(requirement, str) and requirement.startswith("^")
                else None
            )
            if (
                requirement_floor is None
                or requirement_floor < stable_semver(baseline)
            ):
                errors.append(
                    f"{package['name']}: path dependency {dependency_name!r} must "
                    f"use a caret requirement at or above {baseline} for the "
                    f"standalone bootstrap; got {requirement!r}"
                )


def check_publishable_msrv(
    packages: list[dict], publishable: set[str], errors: list[str]
) -> None:
    """Require every release archive to advertise the workspace MSRV."""
    expected = load_workspace_rust_version()
    if not isinstance(expected, str) or not expected:
        errors.append(
            f"{WORKSPACE_MANIFEST}: workspace.package.rust-version must be set"
        )
        return

    for package in packages:
        if package["name"] not in publishable:
            continue
        actual = package.get("rust_version")
        if actual != expected:
            errors.append(
                f"{package['name']}: publishable crate must inherit rust-version "
                f"{expected}; Cargo metadata reports {actual!r}"
            )


def check_release_plz_script(errors: list[str]) -> None:
    if not RELEASE_PLZ_SCRIPT.exists():
        errors.append(f"missing release-plz script: {RELEASE_PLZ_SCRIPT}")
        return

    script = RELEASE_PLZ_SCRIPT.read_text()
    required_fragments = [
        "RELEASE_PLZ_VERSION",
        f'REGISTRY="{ALLOWED_REGISTRY}"',
        "scripts/check_publish_policy.py",
        "scripts/check_crate_archives.py",
        'exec release-plz "$command" --config "$CONFIG" --registry "$REGISTRY"',
        "CARGO_REGISTRY_TOKEN",
        "GIT_TOKEN",
    ]
    for fragment in required_fragments:
        if fragment not in script:
            errors.append(
                f"{RELEASE_PLZ_SCRIPT}: missing required release-plz wiring `{fragment}`"
            )


def main() -> int:
    packages = load_workspace_packages()

    errors: list[str] = []
    publishable: set[str] = set()
    for package in sorted(packages, key=lambda p: p["name"]):
        publish = package.get("publish")
        name = package["name"]
        manifest = package["manifest_path"]

        if publish == [ALLOWED_REGISTRY]:
            publishable.add(name)
            continue

        if publish == []:
            continue

        if publish is None:
            errors.append(f"{name}: missing publish field in {manifest}")
        else:
            errors.append(
                f"{name}: publish must be ['{ALLOWED_REGISTRY}'] or [] in {manifest}; got {publish!r}"
            )

    for package in packages:
        for dependency in package.get("dependencies", []):
            if dependency.get("registry") is not None:
                errors.append(
                    f"{package['name']}: {dependency['name']} names an alternative registry"
                )

    check_bootstrap_package_minimums(packages, publishable, errors)
    check_bootstrap_dependency_minimums(packages, publishable, errors)
    check_publishable_msrv(packages, publishable, errors)

    if RELEASE_PLZ_CONFIG.exists():
        release_config = load_release_plz_config()
        release_packages = release_plz_packages(release_config)
        missing = sorted(publishable - release_packages)
        extra = sorted(release_packages - publishable)
        if missing:
            errors.append(
                f"{RELEASE_PLZ_CONFIG}: publishable crates missing release=true entries: {', '.join(missing)}"
            )
        if extra:
            errors.append(
                f"{RELEASE_PLZ_CONFIG}: release=true entries are not publishable crates.io crates: {', '.join(extra)}"
            )
        if release_config.get("workspace", {}).get("publish_no_verify") is not False:
            errors.append(
                f"{RELEASE_PLZ_CONFIG}: workspace.publish_no_verify must be false "
                "so Cargo verifies every release archive"
            )
        if release_config.get("workspace", {}).get("semver_check") is not True:
            errors.append(
                f"{RELEASE_PLZ_CONFIG}: workspace.semver_check must be true "
                "for normal automated releases"
            )
    else:
        errors.append(f"missing release-plz config: {RELEASE_PLZ_CONFIG}")

    check_release_plz_script(errors)

    if errors:
        print(
            "publish policy violation: crates.io release flow is not fully wired",
            file=sys.stderr,
        )
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "Set publish.workspace = true for crates.io crates, publish = false for local crates, "
            "and keep release-plz.toml aligned.",
            file=sys.stderr,
        )
        return 1

    print(
        f"publish policy ok: {len(publishable)} crate(s) restricted to '{ALLOWED_REGISTRY}' "
        "and wired into release-plz"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
