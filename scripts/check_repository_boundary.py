#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Reject dependencies and source assumptions that escape this repository."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

try:
    import tomllib
except ModuleNotFoundError as error:  # pragma: no cover - Python < 3.11
    raise SystemExit("check_repository_boundary.py requires Python 3.11+") from error


REPO_ROOT = Path(__file__).resolve().parent.parent
IGNORED_PARTS = {".git", ".local", "node_modules", "target"}
SELF = Path(__file__).resolve()

FORBIDDEN_TEXT = (
    (
        "sibling checkout path",
        re.compile(
            r"(?:^|[\"'=(\s])(?:\.\./)+(?:proa-ui|wellformed)(?:[/\"'\s)]|$)|"
            r"\bPROA_UI_ROOT\b|path:\s*proa-ui(?:\s|$)|Checkout proa-ui"
        ),
    ),
    (
        "developer-machine path",
        # `runner` is the fixed account name inside GitHub Actions and ARC
        # runner images, so `/home/runner/...` in a container spec or Dockerfile
        # names the CI image, not a maintainer's machine.
        re.compile(
            r"(?:file://)?/(?:Users|home)/(?![$%{<]|runner/)[^/\s]+/|"
            r"[A-Za-z]:\\Users\\[^\\\s]+\\|~/Desktop/"
        ),
    ),
)

FORBIDDEN_PUBLIC_TERMS = re.compile(
    r"\b(?:Page VM|Document VM|proa_page_vm|proa_document)\b"
)
LITERAL_INCLUDE = re.compile(
    r"\binclude_(?:str|bytes)!\s*\(\s*\"(?P<path>[^\"\\\r\n]+)\"\s*\)"
)
INLINE_MARKDOWN_LINK = re.compile(
    r"!?\[[^\]\r\n]*\]\(\s*(?P<target><[^>\r\n]+>|[^)\s\r\n]+)"
)
REFERENCE_MARKDOWN_LINK = re.compile(
    r"^\s*\[[^\]\r\n]+\]:\s*(?P<target><[^>\r\n]+>|\S+)",
    re.MULTILINE,
)
NON_FILE_LINK_PREFIXES = (
    "#",
    "/",
    "http://",
    "https://",
    "mailto:",
    "data:",
    "javascript:",
    "app://",
)
TERM_EXCEPTIONS = {
    Path("docs/open-source-boundary.md"),
}
# Files whose `include_str!` / `include_bytes!` literals are template data for a
# generated project rather than includes this repository performs. Their paths
# resolve inside the scaffolded project, so they cannot be checked here.
INCLUDE_EXCEPTIONS = {
    Path("proa_cli/src/site_scaffold/templates/mod.rs"),
}


def is_inside_repo(path: Path) -> bool:
    try:
        path.relative_to(REPO_ROOT)
    except ValueError:
        return False
    return True


def is_local_only(path: Path) -> bool:
    return is_inside_repo(path) and ".local" in path.relative_to(REPO_ROOT).parts


def candidate_files() -> list[Path]:
    result = subprocess.run(
        [
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
    )
    if result.returncode == 0:
        relative_paths = [
            Path(raw.decode("utf-8", errors="surrogateescape"))
            for raw in result.stdout.split(b"\0")
            if raw
        ]
        candidates = [REPO_ROOT / path for path in relative_paths]
        return [
            path for path in candidates if path.exists() or path.is_symlink()
        ]

    return [
        path
        for path in REPO_ROOT.rglob("*")
        if path.is_file() and not IGNORED_PARTS.intersection(path.parts)
    ]


def dependency_tables(value: object):
    if not isinstance(value, dict):
        return
    for key, child in value.items():
        if key in {"dependencies", "dev-dependencies", "build-dependencies"}:
            if isinstance(child, dict):
                yield child
        elif isinstance(child, dict):
            yield from dependency_tables(child)


def check_manifest(manifest: Path) -> list[str]:
    problems: list[str] = []
    try:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        return [f"{manifest.relative_to(REPO_ROOT)}: cannot parse manifest: {error}"]

    for table in dependency_tables(data):
        for dependency, specification in table.items():
            if not isinstance(specification, dict) or "path" not in specification:
                continue
            raw_path = specification["path"]
            if not isinstance(raw_path, str):
                problems.append(
                    f"{manifest.relative_to(REPO_ROOT)}: dependency "
                    f"{dependency!r} has a non-string path"
                )
                continue
            resolved = (manifest.parent / raw_path).resolve(strict=False)
            if is_local_only(resolved):
                problems.append(
                    f"{manifest.relative_to(REPO_ROOT)}: dependency "
                    f"{dependency!r} targets local-only content via {raw_path!r}"
                )
            elif not is_inside_repo(resolved):
                problems.append(
                    f"{manifest.relative_to(REPO_ROOT)}: dependency "
                    f"{dependency!r} escapes the repository via {raw_path!r}"
                )
            elif not resolved.exists():
                problems.append(
                    f"{manifest.relative_to(REPO_ROOT)}: dependency "
                    f"{dependency!r} points to missing path {raw_path!r}"
                )

    workspace = data.get("workspace")
    if isinstance(workspace, dict):
        for key in ("members", "exclude"):
            entries = workspace.get(key, [])
            if not isinstance(entries, list):
                continue
            for entry in entries:
                if not isinstance(entry, str):
                    continue
                resolved = (manifest.parent / entry).resolve(strict=False)
                if is_local_only(resolved):
                    problems.append(
                        f"{manifest.relative_to(REPO_ROOT)}: workspace {key[:-1]} "
                        f"targets local-only content via {entry!r}"
                    )
                elif not is_inside_repo(resolved):
                    problems.append(
                        f"{manifest.relative_to(REPO_ROOT)}: workspace {key[:-1]} "
                        f"escapes the repository via {entry!r}"
                    )

    return problems


def read_text(path: Path) -> str | None:
    try:
        raw = path.read_bytes()
    except OSError:
        return None
    if b"\0" in raw:
        return None
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return None


def markdown_without_fenced_code(text: str) -> str:
    output: list[str] = []
    fence: str | None = None
    for line in text.splitlines(keepends=True):
        stripped = line.lstrip()
        marker = "```" if stripped.startswith("```") else "~~~" if stripped.startswith("~~~") else None
        if marker is not None:
            if fence is None:
                fence = marker
            elif fence == marker:
                fence = None
            output.append("\n")
        elif fence is None:
            output.append(line)
        else:
            output.append("\n")
    return "".join(output)


def check_markdown_links(
    path: Path, text: str, repository_paths: set[Path]
) -> list[str]:
    problems: list[str] = []
    searchable = markdown_without_fenced_code(text)
    relative = path.relative_to(REPO_ROOT)
    for pattern in (INLINE_MARKDOWN_LINK, REFERENCE_MARKDOWN_LINK):
        for match in pattern.finditer(searchable):
            target = match.group("target").strip("<>")
            if not target or target.startswith(NON_FILE_LINK_PREFIXES):
                continue
            target = unquote(target.split("#", 1)[0].split("?", 1)[0])
            if not target or any(marker in target for marker in ("{", "}", "<", ">")):
                continue
            resolved = (path.parent / target).resolve(strict=False)
            if is_local_only(resolved):
                line = searchable.count("\n", 0, match.start()) + 1
                problems.append(
                    f"{relative}:{line}: local documentation link {target!r} "
                    "targets local-only content"
                )
                continue
            if not is_inside_repo(resolved):
                line = searchable.count("\n", 0, match.start()) + 1
                problems.append(
                    f"{relative}:{line}: local documentation link {target!r} "
                    "escapes the repository"
                )
                continue
            if resolved in repository_paths:
                continue
            line = searchable.count("\n", 0, match.start()) + 1
            problems.append(
                f"{relative}:{line}: local documentation link {target!r} "
                "is missing from the repository checkout"
            )
    return problems


def main() -> int:
    problems: list[str] = []
    files = candidate_files()
    if (REPO_ROOT / "package.json").is_file():
        if not (REPO_ROOT / "pnpm-lock.yaml").is_file():
            problems.append(
                "package.json: browser acceptance dependencies require a "
                "checked-in pnpm-lock.yaml"
            )
        for stray in ("package-lock.json", "yarn.lock", "bun.lockb"):
            if (REPO_ROOT / stray).is_file():
                problems.append(
                    f"{stray}: this repository resolves Node dependencies with "
                    "pnpm; remove the competing lockfile"
                )
    repository_paths: set[Path] = {REPO_ROOT}
    for candidate in files:
        if not candidate.exists():
            continue
        resolved_candidate = candidate.resolve(strict=False)
        if (
            is_local_only(candidate)
            or is_local_only(resolved_candidate)
            or not is_inside_repo(resolved_candidate)
        ):
            continue
        repository_paths.add(resolved_candidate)
        parent = resolved_candidate.parent
        while is_inside_repo(parent):
            repository_paths.add(parent)
            if parent == REPO_ROOT:
                break
            parent = parent.parent

    for path in files:
        relative = path.relative_to(REPO_ROOT)
        if is_local_only(path):
            problems.append(f"{relative}: local-only content must not be tracked")
            continue
        resolved = path.resolve(strict=False)
        if is_local_only(resolved):
            problems.append(f"{relative}: symlink targets local-only content")
            continue
        if not path.exists():
            continue
        if path.is_symlink() and not is_inside_repo(resolved):
            problems.append(f"{relative}: symlink resolves outside the repository")
            continue
        if path.name == "Cargo.toml":
            problems.extend(check_manifest(path))
        if resolved == SELF:
            continue
        text = read_text(path)
        if text is None:
            continue
        if path.suffix == ".rs" and relative not in INCLUDE_EXCEPTIONS:
            for include_match in LITERAL_INCLUDE.finditer(text):
                include_path = include_match.group("path")
                resolved_include = (path.parent / include_path).resolve(strict=False)
                line = text.count("\n", 0, include_match.start()) + 1
                if is_local_only(resolved_include):
                    problems.append(
                        f"{relative}:{line}: include path {include_path!r} "
                        "targets local-only content"
                    )
                elif not is_inside_repo(resolved_include):
                    problems.append(
                        f"{relative}:{line}: include path {include_path!r} "
                        "escapes the repository"
                    )
                elif not resolved_include.exists():
                    problems.append(
                        f"{relative}:{line}: include path {include_path!r} is missing"
                    )
        if path.suffix in {".md", ".mdx"}:
            problems.extend(check_markdown_links(path, text, repository_paths))
        for label, pattern in FORBIDDEN_TEXT:
            match = pattern.search(text)
            if match:
                line = text.count("\n", 0, match.start()) + 1
                problems.append(f"{relative}:{line}: contains a {label}")
        if relative not in TERM_EXCEPTIONS:
            match = FORBIDDEN_PUBLIC_TERMS.search(text)
            if match:
                line = text.count("\n", 0, match.start()) + 1
                problems.append(
                    f"{relative}:{line}: exposes downstream implementation term "
                    f"{match.group(0)!r}"
                )

    if problems:
        print("repository boundary violations:", file=sys.stderr)
        for problem in sorted(set(problems)):
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(
        f"repository boundary OK ({len(files)} source files; "
        "all Cargo paths stay inside the checkout)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
