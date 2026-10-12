#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Verify that publishable crate archives are standalone and licensed."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
PUBLISH_REGISTRY = "crates-io"
CANONICAL_LICENSE = REPO_ROOT / "LICENSE"
ARCHIVE_LICENSE = "LICENSE"
EXPECTED_REPOSITORY = "https://github.com/proa-labs/proa"
EXPECTED_HOMEPAGE = "https://proa.so"
PLACEHOLDER_DESCRIPTION = "Proa server-rendering framework workspace crate"
LICENSE_TEXT_MARKERS = (
    "Apache License",
    "Version 2.0, January 2004",
    "END OF TERMS AND CONDITIONS",
    "Copyright 2026 Proa Labs, Inc.",
)
LITERAL_INCLUDE = re.compile(
    r"\binclude_(?:str|bytes)!\s*\(\s*\"(?P<path>[^\"\\\r\n]+)\"\s*\)"
)
PATH_ATTRIBUTE = re.compile(
    r"#\s*\[\s*path\s*=\s*\"(?P<path>[^\"\\\r\n]+)\"\s*\]"
)
RAW_STRING = re.compile(r'(?:br|cr|r)(#*)"')
CHAR_LITERAL = re.compile(
    r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'"
)


def workspace_packages() -> list[dict]:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--no-deps",
        ],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)["packages"]


def archive_files(package: dict) -> set[str]:
    result = subprocess.run(
        [
            "cargo",
            "package",
            "--package",
            package["name"],
            "--allow-dirty",
            "--locked",
            "--list",
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise RuntimeError(f"could not list archive: {detail}")
    return {line.strip() for line in result.stdout.splitlines() if line.strip()}


def inside(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def check_license(
    package: dict,
    archive: set[str],
    canonical_license: bytes,
) -> list[str]:
    package_name = package["name"]
    problems: list[str] = []

    if package.get("license") is not None:
        problems.append(
            f"{package_name}: inherit only the workspace license-file; got "
            f"license {package.get('license')!r}"
        )

    manifest = Path(package["manifest_path"]).resolve()
    license_file = package.get("license_file")
    if not isinstance(license_file, str):
        problems.append(
            f"{package_name}: must inherit the workspace license-file"
        )
    else:
        package_license = (manifest.parent / license_file).resolve()
        if package_license != CANONICAL_LICENSE.resolve():
            problems.append(
                f"{package_name}: license-file resolves to {package_license}, "
                f"expected {CANONICAL_LICENSE}"
            )
        else:
            try:
                package_license_text = package_license.read_bytes()
            except OSError as error:
                problems.append(
                    f"{package_name}: could not read {package_license}: {error}"
                )
            else:
                if package_license_text != canonical_license:
                    problems.append(
                        f"{package_name}: {package_license} does not exactly match "
                        f"the canonical {CANONICAL_LICENSE}"
                    )

    if ARCHIVE_LICENSE not in archive:
        problems.append(
            f"{package_name}: crate archive does not include "
            f"the canonical {ARCHIVE_LICENSE} file"
        )

    return problems


def check_package_metadata(package: dict) -> list[str]:
    package_name = package["name"]
    problems: list[str] = []
    if package.get("repository") != EXPECTED_REPOSITORY:
        problems.append(
            f"{package_name}: repository metadata must be "
            f"{EXPECTED_REPOSITORY!r}; got {package.get('repository')!r}"
        )
    if package.get("homepage") != EXPECTED_HOMEPAGE:
        problems.append(
            f"{package_name}: homepage metadata must be "
            f"{EXPECTED_HOMEPAGE!r}; got {package.get('homepage')!r}"
        )
    description = package.get("description")
    if not description or description == PLACEHOLDER_DESCRIPTION:
        problems.append(
            f"{package_name}: replace the workspace placeholder with a "
            "crate-specific public description"
        )
    return problems


def rust_code_mask(text: str) -> str:
    """Keep source offsets while masking literals and nested Rust comments.

    Scaffold generators contain entire Rust files in strings. Their includes
    belong to the generated application, not to the generator's archive.
    """
    masked = list(text)
    index = 0
    while index < len(text):
        start = index
        if text.startswith("//", index):
            end = text.find("\n", index)
            index = len(text) if end < 0 else end
        elif text.startswith("/*", index):
            index += 2
            depth = 1
            while index < len(text) and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
        elif raw := RAW_STRING.match(text, index):
            closing = '"' + raw.group(1)
            end = text.find(closing, raw.end())
            index = len(text) if end < 0 else end + len(closing)
        elif text[index] == '"':
            index += 1
            while index < len(text):
                if text[index] == "\\":
                    index += 2
                elif text[index] == '"':
                    index += 1
                    break
                else:
                    index += 1
        elif char := CHAR_LITERAL.match(text, index):
            index = char.end()
        else:
            index += 1
            continue
        for position in range(start, min(index, len(text))):
            if text[position] != "\n":
                masked[position] = " "
    return "".join(masked)


def check_source_paths(
    package_name: str,
    package_root: Path,
    source: Path,
    archive: set[str],
) -> list[str]:
    try:
        text = source.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return []

    code = rust_code_mask(text)
    problems: list[str] = []
    for kind, pattern in (
        ("include", LITERAL_INCLUDE),
        ("path attribute", PATH_ATTRIBUTE),
    ):
        for match in pattern.finditer(text):
            if code[match.start()] == " ":
                continue
            raw_path = match.group("path")
            target = (source.parent / raw_path).resolve(strict=False)
            line = text.count("\n", 0, match.start()) + 1
            source_relative = source.relative_to(package_root)
            if not inside(target, package_root):
                problems.append(
                    f"{package_name}:{source_relative}:{line}: {kind} "
                    f"{raw_path!r} escapes the crate archive"
                )
                continue
            target_relative = target.relative_to(package_root).as_posix()
            if target_relative not in archive:
                problems.append(
                    f"{package_name}:{source_relative}:{line}: {kind} "
                    f"{raw_path!r} targets {target_relative!r}, which is absent "
                    "from the crate archive"
                )
    return problems


def main() -> int:
    problems: list[str] = []
    checked = 0

    try:
        canonical_license = CANONICAL_LICENSE.read_bytes()
        license_text = canonical_license.decode("utf-8")
    except (OSError, UnicodeError) as error:
        problems.append(f"could not read canonical {CANONICAL_LICENSE}: {error}")
        canonical_license = b""
        license_text = ""
    for marker in LICENSE_TEXT_MARKERS:
        if marker not in license_text:
            problems.append(
                f"{CANONICAL_LICENSE}: missing expected Apache-2.0 text {marker!r}"
            )

    for package in sorted(workspace_packages(), key=lambda item: item["name"]):
        if package.get("publish") != [PUBLISH_REGISTRY]:
            continue
        checked += 1
        manifest = Path(package["manifest_path"]).resolve()
        package_root = manifest.parent
        try:
            archive = archive_files(package)
        except RuntimeError as error:
            problems.append(f"{package['name']}: {error}")
            continue

        problems.extend(check_license(package, archive, canonical_license))
        problems.extend(check_package_metadata(package))

        for relative in sorted(archive):
            if not relative.endswith(".rs"):
                continue
            source = package_root / relative
            if source.is_file():
                problems.extend(
                    check_source_paths(
                        package["name"],
                        package_root,
                        source,
                        archive,
                    )
                )

    if problems:
        print("crate archive boundary violations:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(f"crate archive boundaries OK ({checked} publishable crates)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
