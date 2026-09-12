#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Check or normalize copyright headers in first-party source and configuration.

Only Git-visible files are eligible. --fix never replaces an unknown copyright
holder or license; third-party notices and byte-sensitive fixtures are excluded.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OWNER = "Proa Labs, Inc."
LICENSE = "Apache-2.0"
SLASH_SUFFIXES = {".rs", ".js", ".mjs", ".ts", ".tsx", ".jsx"}
HASH_SUFFIXES = {".py", ".sh", ".ps1", ".toml", ".yaml", ".yml"}
BLOCK_SUFFIXES = {".css", ".c", ".h"}
GENERATED_HEADERS = {Path("rsjs_web/src/generated.rs")}

# These retain their governing license or inherit the root license without
# inserting comments into generated output or test data.
EXCLUSIONS = {
    "third-party/source-installed material": (
        "*/components/_proa/*", "*/public/proa/proa-ui/*",
        "proa_marketing/public/vendor/*",
        # Older catalog copies not recorded in this example's install lock.
        "examples/origin-routing-atlas/src/components/dialog/*",
        "examples/origin-routing-atlas/src/components/theme_toggle/*",
        "proa_cli/src/site_scaffold/templates/origin/src/components/dialog/*",
        "proa_cli/src/site_scaffold/templates/origin/src/components/theme_toggle/*",
    ),
    "compiler/editor fixtures and exact-output snapshots": (
        "*/tests/ui/*", "*/tests/*_fail/*", "*/tests/fixtures/*",
        "*/test/fixture/*", "*/tests/golden/*", "*/golden/*",
        "rsjs_conformance/snapshots/*", "rsjs_ir/tests/*.expected.js",
    ),
    "generated assets and captured page data": (
        "*/pnpm-lock.yaml", "pnpm-lock.yaml",
        "proa_demo/include/proa.h",
        "proa_actions/examples/user_profile.contract.ts",
        "proa_actions/examples/user_profile.runtime.ts",
        "proa_demo/react-ssr/chrome.generated.tsx",
        "proa_demo/react-ssr-shared/src/github-pr-diff-fixtures.ts",
        "proa_demo/src/github_pr_diff_fixtures.rs",
        "proa_showcase/src/ebay/srp_chrome_real.rs",
        "proa_showcase/src/ebay/data/*", "proa_showcase_web/public/assets/*",
        "*/public/styles.css", "proa_marketing/public/docs.js",
        "proa_marketing/public/search-index.mjs", "proa_marketing/public/proa-ui-rail.css",
    ),
}
LEGACY_OWNERS = {"The rsjs Authors.", "The Proa Authors.", OWNER}
COPYRIGHT = re.compile(
    r"^(?:Copyright(?:\s+(?:\(c\)|©))?|SPDX-FileCopyrightText:)\s+"
    r"(?:(?:\d{4}(?:-\d{4})?)\s+)?(?P<owner>.+?)\s*$"
)
ENCODING = re.compile(r"^[ \t]*#.*?coding[:=][ \t]*[-\w.]+")
DOCKER_DIRECTIVE = re.compile(r"^[ \t]*#[ \t]*(?:syntax|escape|check)[ \t]*=", re.I)


def excluded(path: Path) -> bool:
    if any(part in {".git", ".local", "node_modules", "target"} for part in path.parts):
        return True
    return any(
        fnmatch.fnmatchcase(path.as_posix(), pattern)
        for patterns in EXCLUSIONS.values()
        for pattern in patterns
    )


def is_dockerfile(path: Path) -> bool:
    return path.name == "Dockerfile" or path.name.endswith(".Dockerfile")


def style(path: Path) -> tuple[str, str] | None:
    if path.suffix in SLASH_SUFFIXES:
        return "// ", ""
    if (
        path.suffix in HASH_SUFFIXES
        or is_dockerfile(path)
        or path.as_posix() == "rsjs_web/schema/bindings.txt"
    ):
        return "# ", ""
    if path.suffix in BLOCK_SUFFIXES:
        return "/* ", " */"
    return None


def comment_text(line: str) -> str | None:
    text = line.strip()
    if text.startswith(("//!", "///", "#![", "#[")):
        return None
    if text.startswith("//"):
        return text[2:].strip()
    if text.startswith("#"):
        return text[1:].strip()
    if text.startswith("/*") and text.endswith("*/"):
        return text[2:-2].strip()
    return None


def normalize(path: Path, raw: bytes) -> bytes:
    """Change only short license notices; preserve file preambles and EOLs."""
    markers = style(path)
    if markers is None:
        return raw
    text = raw.decode("utf-8")
    bom = "\ufeff" if text.startswith("\ufeff") else ""
    text = text.removeprefix(bom)
    newline = "\r\n" if "\r\n" in text else "\n"
    lines = text.splitlines(keepends=True)
    start = 0
    if is_dockerfile(path):
        # Docker stops recognizing parser directives at the first ordinary
        # comment or blank line, so all leading directives must precede us.
        while start < len(lines) and DOCKER_DIRECTIVE.match(lines[start]):
            start += 1
    if lines and lines[0].startswith("#!") and not lines[0].startswith("#!["):
        start = 1
    if path.suffix == ".py":
        # Python encoding cookies can occupy either of the first two lines.
        for index in range(start, min(2, len(lines))):
            if ENCODING.match(lines[index]):
                start = index + 1
    if path.suffix == ".css" and start < len(lines) and lines[start].startswith("@charset "):
        start += 1

    remove = set()
    block = False
    for index in range(start, len(lines)):
        stripped = lines[index].strip()
        if not stripped:
            continue
        if stripped.startswith(("//!", "///")):
            # Older files place their license after the module documentation.
            continue
        if path.suffix == ".rs" and stripped.startswith("#![") and stripped.endswith("]"):
            continue
        payload = comment_text(lines[index])
        if payload is None:
            if stripped.startswith("/*") or block:
                block = not stripped.endswith("*/")
                # Do not rewrite multiline third-party license blocks.
                if re.search(r"copyright|licen[cs]e", stripped, re.I):
                    raise ValueError("review the existing multiline license notice")
                continue
            break
        copyright_match = COPYRIGHT.match(payload)
        if copyright_match:
            holder = copyright_match["owner"].removesuffix(" All rights reserved.")
            if holder not in LEGACY_OWNERS:
                raise ValueError(f"preserve and review existing holder {holder!r}")
            remove.add(index)
        elif payload.startswith("SPDX-License-Identifier:"):
            license_id = payload.partition(":")[2].strip()
            if license_id != LICENSE:
                raise ValueError(f"preserve and review existing license {license_id!r}")
            remove.add(index)
        elif re.search(r"\b(?:licensed under|(?:MIT|BSD|GNU|Apache) licen[cs]e)\b", payload, re.I):
            raise ValueError("preserve and review the existing license notice")

    # Removing a notice between two comment sections should leave one blank
    # separator, without introducing a rustfmt-only follow-up change.
    notice_lines = set(remove)
    for first in sorted(notice_lines):
        if first - 1 in notice_lines:
            continue
        end = first
        while end in notice_lines:
            end += 1
        if first > start and not lines[first - 1].strip() and end < len(lines) and not lines[end].strip():
            remove.add(end)

    prefix = "".join(lines[:start])
    if prefix and not prefix.endswith(("\n", "\r")):
        prefix += newline
    body = "".join(line for index, line in enumerate(lines[start:], start) if index not in remove)
    body = body.lstrip("\r\n")
    opening, closing = markers
    header = (
        f"{opening}Copyright {OWNER}{closing}{newline}"
        f"{opening}SPDX-License-Identifier: {LICENSE}{closing}{newline}{newline}"
    )
    return (bom + prefix + header + body).encode("utf-8")


def repository_files(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root, check=True, capture_output=True,
    )
    return sorted({Path(name.decode()) for name in result.stdout.split(b"\0") if name})


def regular_file(root: Path, relative: Path) -> bool:
    path = root / relative
    return (
        path.is_file()
        and not path.is_symlink()
        and not any((root / parent).is_symlink() for parent in relative.parents)
    )


def source_installed_files(root: Path, files: list[Path]) -> set[Path]:
    """Preserve catalog source and its own licenses, as recorded by proa ui add."""
    installed = set()
    for lock in files:
        if lock.name != "proa.lock.json" or excluded(lock):
            continue
        if not regular_file(root, lock):
            continue
        data = json.loads((root / lock).read_text())
        for component in data.get("components", {}).values():
            for name in component.get("installedFiles", []):
                path = Path(name)
                if path.is_absolute() or ".." in path.parts:
                    raise ValueError(f"{lock}: installed file must stay inside the application")
                installed.add(lock.parent / path)
    return installed


def run(root: Path, fix: bool) -> int:
    updates = []
    errors = []
    checked = 0
    files = repository_files(root)
    installed = source_installed_files(root, files)
    for relative in files:
        path = root / relative
        if excluded(relative) or relative in installed or style(relative) is None:
            continue
        # Never follow a symlink into private notes or another checkout.
        if not regular_file(root, relative):
            continue
        checked += 1
        try:
            raw = path.read_bytes()
            updated = normalize(relative, raw)
        except (OSError, UnicodeError, ValueError) as error:
            errors.append(f"{relative}: {error}")
            continue
        if updated != raw:
            if relative in GENERATED_HEADERS:
                errors.append(f"{relative}: fix the generator's copyright header and regenerate this file")
            else:
                updates.append((path, raw, updated))

    # Review ambiguities before making any edits, so --fix is safe to retry.
    if errors:
        for error in errors:
            print(error)
        return 1
    if fix:
        for path, original, updated in updates:
            if path.read_bytes() != original:
                raise RuntimeError(f"{path.relative_to(root)} changed during normalization")
            path.write_bytes(updated)
        print(f"Copyright headers: normalized {len(updates)} of {checked} first-party files.")
        return 0
    if updates:
        for path, _, _ in updates:
            print(f"{path.relative_to(root)}: missing or inconsistent copyright/license header")
        print(f"Run python3 scripts/copyright_headers.py --fix ({len(updates)} files).")
        return 1
    print(f"Copyright headers OK ({checked} first-party files).")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="check without writing (default)")
    mode.add_argument("--fix", action="store_true", help="normalize known first-party notices")
    args = parser.parse_args()
    return run(ROOT, args.fix)


if __name__ == "__main__":
    raise SystemExit(main())
