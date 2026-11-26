#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Detect allocation-prone patterns in Proa render hot paths.

The lint is intentionally conservative: in a Git checkout it checks Rust files
that changed by default; in a source archive without Git metadata it scans all
Rust files. It only reports allocation patterns inside `render` methods or
inside `html!` / `html_sync!` macro bodies. Test fixtures are skipped unless
`--include-tests` is passed. Suppress an intentional finding with
`// proa-perf-ok: <reason>` on the same line or the previous line.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

IGNORED_DIRS = {
    ".git",
    "target",
    "node_modules",
}

IGNORED_PREFIXES = (
    pathlib.Path(".claude/worktrees"),
)


@dataclass(frozen=True)
class Rule:
    code: str
    pattern: re.Pattern[str]
    message: str
    replacement: str


@dataclass(frozen=True)
class Finding:
    path: pathlib.Path
    line_no: int
    code: str
    line: str
    message: str
    replacement: str
    context: str


RULES = [
    Rule(
        "format",
        re.compile(r"\bformat!\s*\("),
        "format! allocates a String in the render path",
        'split text into static and dynamic segments, for example: "$"{price}" / night"',
    ),
    Rule(
        "serde-json-to-string",
        re.compile(r"\bserde_json::to_string(?:_pretty)?\s*\("),
        "serde_json::to_string allocates in the render path",
        "precompute serialized data before render or write a dedicated renderable wrapper",
    ),
    Rule(
        "render-to-string",
        re.compile(r"\brender_to_string\s*\("),
        "render_to_string allocates an intermediate String",
        "render the child directly into the existing WebContext",
    ),
    Rule(
        "string-new",
        re.compile(r"\bString::new\s*\("),
        "String::new creates a heap-backed value in the render path",
        "use Option<&str>, static strings, or move allocation to construction time",
    ),
    Rule(
        "string-from",
        re.compile(r"\bString::from\s*\("),
        "String::from allocates in the render path",
        "use the source &str directly when possible",
    ),
    Rule(
        "string-with-capacity",
        re.compile(r"\bString::with_capacity\s*\("),
        "String::with_capacity allocates in the render path",
        "write directly to the Proa output buffer or precompute outside render",
    ),
    Rule(
        "vec-new",
        re.compile(r"\bVec::new\s*\("),
        "Vec::new creates a heap collection in the render path",
        "iterate source data directly or use a stack-allocated fixed container",
    ),
    Rule(
        "vec-with-capacity",
        re.compile(r"\bVec::with_capacity\s*\("),
        "Vec::with_capacity allocates in the render path",
        "render directly into the existing buffer or use stack storage",
    ),
    Rule(
        "vec-macro",
        re.compile(r"\bvec!\s*\["),
        "vec! allocates in the render path",
        "use a fixed array/slice or iterate source data directly",
    ),
    Rule(
        "to-string",
        re.compile(r"\.to_string\s*\("),
        ".to_string allocates in the render path",
        "pass primitives and &str directly; Proa renders numeric types without conversion",
    ),
    Rule(
        "to-owned",
        re.compile(r"\.to_owned\s*\("),
        ".to_owned allocates in the render path",
        "borrow the existing value or move ownership before render",
    ),
    Rule(
        "collect",
        re.compile(r"\.collect(?:\s*::\s*<[^>]+>)?\s*\("),
        ".collect allocates a collection in the render path",
        "iterate directly instead of collecting first",
    ),
    Rule(
        "join",
        re.compile(r"\.join\s*\("),
        ".join allocates a String in the render path",
        "use ClassList/tw_join for classes or render pieces directly",
    ),
    Rule(
        "push-str",
        re.compile(r"\.push_str\s*\("),
        "push_str usually means an intermediate String is being assembled",
        "write directly to cx/out or use html_sync static consolidation",
    ),
    Rule(
        "string-push",
        re.compile(r"\.push\s*\("),
        "push usually means an intermediate String or Vec is being assembled",
        "write directly to cx/out or use stack storage",
    ),
]


def run_git(args: list[str]) -> list[str]:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=REPO_ROOT,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except subprocess.CalledProcessError:
        return []
    return [line for line in result.stdout.splitlines() if line]


def is_git_checkout() -> bool:
    result = subprocess.run(
        ["git", "rev-parse", "--is-inside-work-tree"],
        cwd=REPO_ROOT,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return result.returncode == 0 and result.stdout.strip() == "true"


def rel_path(path: pathlib.Path) -> pathlib.Path:
    absolute = path if path.is_absolute() else REPO_ROOT / path
    return absolute.resolve().relative_to(REPO_ROOT)


def should_scan_path(path: pathlib.Path) -> bool:
    rel = rel_path(path)
    if rel.suffix != ".rs":
        return False
    if any(part in IGNORED_DIRS for part in rel.parts):
        return False
    return not any(rel == prefix or prefix in rel.parents for prefix in IGNORED_PREFIXES)


def git_changed_files(base: str | None, staged: bool) -> list[pathlib.Path]:
    if staged:
        names = run_git(["diff", "--cached", "--name-only", "--diff-filter=ACMRTUXB", "--", "*.rs"])
    else:
        diff_args = ["diff", "--name-only", "--diff-filter=ACMRTUXB"]
        if base:
            diff_args.append(base)
        else:
            diff_args.append("HEAD")
        diff_args.extend(["--", "*.rs"])
        names = run_git(diff_args)
        names.extend(run_git(["ls-files", "--others", "--exclude-standard", "--", "*.rs"]))

    seen: set[pathlib.Path] = set()
    paths: list[pathlib.Path] = []
    for name in names:
        path = pathlib.Path(name)
        if path in seen:
            continue
        seen.add(path)
        if (REPO_ROOT / path).exists() and should_scan_path(path):
            paths.append(path)
    return paths


def all_rust_files() -> list[pathlib.Path]:
    return sorted(
        rel_path(path)
        for path in REPO_ROOT.rglob("*.rs")
        if should_scan_path(path)
    )


def count_braces(line: str) -> int:
    return line.count("{") - line.count("}")


def mask_rust_non_code(lines: list[str]) -> list[str]:
    """Replace strings and comments with spaces while preserving positions.

    The perf lint is intentionally lexical, but brace depth is security-like:
    one `}` inside a format string or comment must not end an event-handler or
    render scope early. Keeping every line/column intact lets existing regex
    matches continue to report the original source location.
    """
    masked_lines: list[str] = []
    block_comment_depth = 0
    string_quote: str | None = None
    raw_hashes: int | None = None

    for line in lines:
        masked = list(line)
        index = 0
        while index < len(line):
            if block_comment_depth > 0:
                masked[index] = " "
                if line.startswith("/*", index):
                    if index + 1 < len(masked):
                        masked[index + 1] = " "
                    block_comment_depth += 1
                    index += 2
                elif line.startswith("*/", index):
                    if index + 1 < len(masked):
                        masked[index + 1] = " "
                    block_comment_depth -= 1
                    index += 2
                else:
                    index += 1
                continue

            if raw_hashes is not None:
                masked[index] = " "
                closing = '"' + ("#" * raw_hashes)
                if line.startswith(closing, index):
                    for offset in range(len(closing)):
                        if index + offset < len(masked):
                            masked[index + offset] = " "
                    index += len(closing)
                    raw_hashes = None
                else:
                    index += 1
                continue

            if string_quote is not None:
                masked[index] = " "
                if line[index] == "\\":
                    if index + 1 < len(masked):
                        masked[index + 1] = " "
                    index += 2
                elif line[index] == string_quote:
                    string_quote = None
                    index += 1
                else:
                    index += 1
                continue

            if line.startswith("//", index):
                for rest in range(index, len(masked)):
                    masked[rest] = " "
                break
            if line.startswith("/*", index):
                masked[index] = " "
                if index + 1 < len(masked):
                    masked[index + 1] = " "
                block_comment_depth = 1
                index += 2
                continue

            raw_start = None
            for prefix in ("br", "rb", "r"):
                if not line.startswith(prefix, index):
                    continue
                cursor = index + len(prefix)
                while cursor < len(line) and line[cursor] == "#":
                    cursor += 1
                if cursor < len(line) and line[cursor] == '"':
                    before = line[index - 1] if index > 0 else ""
                    if not (before.isalnum() or before == "_"):
                        raw_start = (cursor, cursor - index - len(prefix))
                        break
            if raw_start is not None:
                quote_index, hashes = raw_start
                for offset in range(index, quote_index + 1):
                    masked[offset] = " "
                raw_hashes = hashes
                index = quote_index + 1
                continue

            if line[index] == '"':
                masked[index] = " "
                string_quote = '"'
                index += 1
                continue

            if line[index] == "'":
                # Treat this as a char literal only when a closing quote is
                # present on the line. Lifetimes such as `'a` remain code.
                cursor = index + 1
                escaped = False
                closing = None
                while cursor < len(line):
                    char = line[cursor]
                    if not escaped and char == "'":
                        closing = cursor
                        break
                    if not escaped and char.isspace():
                        break
                    if not escaped and char == "\\":
                        escaped = True
                    else:
                        escaped = False
                    cursor += 1
                if closing is not None:
                    for offset in range(index, closing + 1):
                        masked[offset] = " "
                    index = closing + 1
                    continue

            index += 1

        masked_lines.append("".join(masked))
    return masked_lines


def is_suppressed(line: str, previous_line: str) -> bool:
    return (
        "proa-perf-ok" in line
        or "proa-perf-ok" in previous_line
        or "perf_ok(" in line
    )


# A bare or `rsjs::`-rooted call, never a method call or another crate's path,
# matching the RSJS dialect and `cargo proa lint`.
RSJS_CALL_PREFIX = r"(?<![\w:.])(?:(?:::)?rsjs::)?"

STRING_NEW = r"(?:(?:::)?std::string::)?String::new\s*\(\s*\)"
VEC_NEW = r"(?:(?:::)?(?:std|alloc)::vec::)?Vec::new\s*\(\s*\)"

EMPTY_SIGNAL_SEED = re.compile(
    RSJS_CALL_PREFIX
    + r"signal\s*(?:::\s*<[^;{}]*?>\s*)?\(\s*("
    + STRING_NEW
    + "|"
    + VEC_NEW
    + r")\s*,?\s*\)"
)


def empty_signal_seed_spans(code_lines: list[str]) -> list[tuple[int, int]]:
    """Return offsets of `signal(String::new())` / `signal::<Vec<T>>(Vec::new())` seeds.

    Neither constructor allocates, and RSJS needs the owned type so handlers
    can write runtime values into the signal. Formatting, a trailing comma, a
    turbofish, and a `std::` path do not matter. Offsets index the masked
    lines joined with newlines.
    """
    text = "\n".join(code_lines)
    return [seed.span(1) for seed in EMPTY_SIGNAL_SEED.finditer(text)]


# An attribute that makes RSJS analyze the item it is on, mirroring
# `rsjs_line_ranges` in `proa_source_tools`.
RSJS_ITEM_ATTRIBUTE = re.compile(
    r"#\s*\[\s*(?:::)?(?:\w+\s*::\s*)*(?:rsjs|client|client_module)\s*[(\]]"
)


def rsjs_item_spans(code_lines: list[str]) -> list[tuple[int, int]]:
    """Return offsets of items RSJS analyzes, from the attribute to the closing `}`.

    Covers `#[rsjs(...)]` impls and functions, `#[client]` functions, and
    client modules. Offsets match `empty_signal_seed_spans`.
    """
    text = "\n".join(code_lines)
    spans: list[tuple[int, int]] = []
    for attribute in RSJS_ITEM_ATTRIBUTE.finditer(text):
        index = text.index("[", attribute.start())
        depth = 0
        # Skip the rest of the attribute, then the item header up to its body.
        # A `;` outside parentheses or brackets ends a bodiless item.
        body_start = None
        while index < len(text):
            char = text[index]
            if char in "([":
                depth += 1
            elif char in ")]":
                depth -= 1
            elif char == ";" and depth == 0:
                break
            elif char == "{" and depth == 0:
                body_start = index
                break
            index += 1
        if body_start is None:
            continue
        depth = 0
        for index in range(body_start, len(text)):
            if text[index] == "{":
                depth += 1
            elif text[index] == "}":
                depth -= 1
                if depth == 0:
                    spans.append((attribute.start(), index + 1))
                    break
        else:
            spans.append((attribute.start(), len(text)))
    return spans


CLIENT_CLOSURE_CALL_OPEN = re.compile(
    RSJS_CALL_PREFIX + r"(?:event_handler|handler|interval|debounce)\s*\("
)
CLOSURE_HEAD = re.compile(r"\s*(?:move\s+)?\|[^|]*\|")


def client_closure_call_spans(code_lines: list[str]) -> list[tuple[int, int]]:
    """Return offsets of closure arguments to browser-only RSJS primitive calls.

    Inside an item RSJS analyzes, RSJS removes `event_handler`/`handler`
    declarations and `interval` timers from the server program, and
    `debounce` only appears in event attributes, so closures passed to them
    never run during SSR. Elsewhere the same names are ordinary functions and
    stay linted, as do other arguments such as an `interval` period. Offsets
    match `empty_signal_seed_spans`.
    """
    text = "\n".join(code_lines)
    rsjs_items = rsjs_item_spans(code_lines)
    spans: list[tuple[int, int]] = []
    for call in CLIENT_CLOSURE_CALL_OPEN.finditer(text):
        if not any(start <= call.start() < end for start, end in rsjs_items):
            continue
        index = call.end()
        depth = 1
        closure_start = None
        at_argument_start = True
        while index < len(text):
            if at_argument_start:
                at_argument_start = False
                head = CLOSURE_HEAD.match(text, index)
                if head is not None:
                    # Skip the parameter list so commas between typed
                    # parameters do not end the argument.
                    closure_start = index
                    index = head.end()
                    continue
            char = text[index]
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
            if depth == 0 or (depth == 1 and char == ","):
                if closure_start is not None:
                    spans.append((closure_start, index))
                    closure_start = None
                if depth == 0:
                    break
                at_argument_start = True
            index += 1
        if closure_start is not None:
            spans.append((closure_start, len(text)))
    return spans


SHARED_STATE_PROVIDER_OPEN = re.compile(
    r"(?<![\w:])(?:(?:::)?rsjs::)?SharedStateProvider\s*(?:::\s*<[^;{}]*>\s*)?\{"
)
STATE_FIELD = re.compile(r"(?<!\w)state\s*:(?!:)")
STRING_LITERAL = r'"(?:[^"\\]|\\[\s\S])*"'


def seed_string_literal(delimiter: str) -> str:
    """Match a quoted string or a raw string with the same hashes at both ends."""
    raw = rf'r(?P<{delimiter}>#*)"(?:(?!"(?P={delimiter}))[\s\S])*"(?P={delimiter})'
    return rf"(?:{STRING_LITERAL}|{raw})"


# A struct field initialized with an owned literal or empty value, matching
# `expr_is_shared_state_owned_seed` in `proa_source_tools`.
OWNED_SEED_FIELD = re.compile(
    r"[{,]\s*[A-Za-z_]\w*\s*:\s*("
    + r"(?:(?:::)?std::string::)?String::(?:new|default)\s*\(\s*\)"
    + "|"
    + VEC_NEW
    + r"|(?:(?:::)?std::string::)?String::from\s*\(\s*"
    + seed_string_literal("from_hashes")
    + r"\s*,?\s*\)|"
    + seed_string_literal("receiver_hashes")
    + r"\s*\.\s*to_(?:string|owned)\s*\(\s*\)"
    + r")\s*(?=[,}])"
)


def shared_state_seed_spans(lines: list[str], code_lines: list[str]) -> list[tuple[int, int]]:
    """Return offsets of owned literal or empty fields in `SharedStateProvider` seeds.

    `#[derive(rsjs::SharedState)]` rejects borrowed fields, so a field of the
    `state: ...` seed may be `"mode".to_string()`, `"mode".to_owned()`,
    `String::from("mode")`, `String::new()`, `String::default()`, or
    `Vec::new()`. Any other seed expression runs on every SSR render and stays
    linted. Offsets match `client_closure_call_spans`.
    """
    text = "\n".join(code_lines)
    # Literal receivers are blank in the masked text, so seed fields are
    # matched against the original source at the same offsets.
    source = "\n".join(lines)
    spans: list[tuple[int, int]] = []
    for start, end in shared_state_value_spans(text):
        spans.extend(field.span(1) for field in OWNED_SEED_FIELD.finditer(source, start, end))
    return spans


def shared_state_value_spans(text: str) -> list[tuple[int, int]]:
    """Return offsets of each `SharedStateProvider { state: ... }` value in masked text."""
    spans: list[tuple[int, int]] = []
    for provider in SHARED_STATE_PROVIDER_OPEN.finditer(text):
        depth = 0
        start = None
        for index in range(provider.end() - 1, len(text)):
            char = text[index]
            if depth == 1 and start is None:
                field = STATE_FIELD.match(text, index)
                if field is not None:
                    start = field.end()
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
            if start is not None and (depth == 0 or (depth == 1 and char == ",")):
                spans.append((start, index))
                break
            if depth == 0:
                break
    return spans


EVENT_HANDLER_OPEN = re.compile(r"\bon_[a-zA-Z0-9_]+\s*=\s*\{")
EVENT_HANDLER_CLOSURE_OPEN = re.compile(r"\bon_[a-zA-Z0-9_]+\s*=\s*\{\s*(?:move\s+)?\|")


def is_client_event_handler_expression(line: str, match_start: int) -> bool:
    """Return true when an allocation sits inside an `on_*={|...| ...}` closure.

    Event closures are compiled into browser JavaScript and are not executed by
    the SSR render path. Proa's formatter commonly keeps the full attribute on
    one line, so treating these as server allocations is a false positive.

    The handler must still be open at the match position: braces are balanced
    from the handler's opening `{` up to the match, so server-side attribute
    expressions that follow a closed `on_*={|...|}` handler on the same line
    are still linted.
    """
    for event in EVENT_HANDLER_OPEN.finditer(line, 0, match_start):
        opening = event.end() - 1  # the pattern always ends at the `{`
        depth = 0
        closed = False
        for ch in line[opening:match_start]:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    closed = True
                    break
        if closed:
            continue
        if "|" in line[opening + 1 : match_start]:
            return True
    return False


def client_event_handler_depth_after(line: str, current_depth: int) -> int:
    """Track multiline `on_*={|...| ...}` expressions for SSR perf linting.

    Braces are balanced only within handler expressions themselves. Once a
    handler closes, the remainder of the line is scanned for the next handler
    opening, so `{` characters that belong to later server-side attribute
    expressions on the same line are never credited to the handler.
    """
    depth = current_depth
    pos = 0
    while True:
        while depth > 0 and pos < len(line):
            ch = line[pos]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            pos += 1
        if depth > 0:
            return depth
        event = EVENT_HANDLER_CLOSURE_OPEN.search(line, pos)
        if event is None:
            return 0
        pos = line.index("{", event.start(), event.end()) + 1
        depth = 1


def carried_handler_close_index(line: str, depth: int) -> int:
    """Index just past the `}` that closes a handler carried in from earlier lines.

    Returns 0 when no handler depth was carried in, and len(line) when the
    handler stays open past the end of this line. Rule matches at or beyond the
    returned index are back on the SSR render path and must be linted.
    """
    if depth <= 0:
        return 0
    for index, ch in enumerate(line):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return index + 1
    return len(line)


def is_comment_line(stripped: str) -> bool:
    return (
        stripped.startswith("//")
        or stripped.startswith("/*")
        or stripped.startswith("*")
    )


def scan_file(path: pathlib.Path, include_tests: bool) -> list[Finding]:
    if not include_tests and "tests" in path.parts:
        return []
    if not include_tests and path.name == "tests.rs":
        return []

    full_path = REPO_ROOT / path
    try:
        lines = full_path.read_text(encoding="utf-8").splitlines()
    except UnicodeDecodeError:
        lines = full_path.read_text(encoding="utf-8", errors="replace").splitlines()

    code_lines = mask_rust_non_code(lines)
    exempt_spans = (
        client_closure_call_spans(code_lines)
        + shared_state_seed_spans(lines, code_lines)
        + empty_signal_seed_spans(code_lines)
    )
    line_starts = [0]
    for code_line in code_lines:
        line_starts.append(line_starts[-1] + len(code_line) + 1)
    findings: list[Finding] = []
    previous_line = ""
    cfg_test_pending = False
    test_depth = 0
    render_pending = False
    render_depth = 0
    macro_depth = 0
    client_event_handler_depth = 0

    for index, (line, code_line) in enumerate(zip(lines, code_lines), start=1):
        stripped = line.strip()
        is_comment = not code_line.strip() and bool(stripped)
        delta = count_braces(code_line)

        if stripped.startswith("#[cfg(test)]"):
            cfg_test_pending = True

        if cfg_test_pending and re.search(r"\bmod\s+tests\b", code_line):
            test_depth = max(delta, 1)
            cfg_test_pending = False
            previous_line = line
            continue

        in_test = test_depth > 0
        if in_test and not include_tests:
            test_depth += delta
            if test_depth <= 0:
                test_depth = 0
            previous_line = line
            continue

        if not is_comment and re.search(r"\bfn\s+render\b", code_line):
            render_pending = True

        if render_pending and not is_comment and "{" in code_line:
            render_depth = max(delta, 1)
            render_pending = False
        elif render_depth > 0:
            render_depth += delta
            if render_depth <= 0:
                render_depth = 0

        starts_macro = (
            not is_comment
            and (
                re.search(r"\bhtml_sync!\s*[{(]", code_line) is not None
                or re.search(r"\bhtml!\s*[{(]", code_line) is not None
            )
        )
        if starts_macro:
            macro_depth += max(delta, 1)
        elif macro_depth > 0:
            macro_depth += delta
            if macro_depth <= 0:
                macro_depth = 0

        in_render = render_pending or render_depth > 0
        in_macro = macro_depth > 0 or starts_macro
        handler_close_index = carried_handler_close_index(
            code_line, client_event_handler_depth
        )
        if (
            (in_render or in_macro)
            and not is_comment
            and not is_suppressed(line, previous_line)
        ):
            context = "render" if in_render else "html macro"
            for rule in RULES:
                for match in rule.pattern.finditer(code_line):
                    if match.start() < handler_close_index:
                        continue
                    if is_client_event_handler_expression(code_line, match.start()):
                        continue
                    offset = line_starts[index - 1] + match.start()
                    if any(start <= offset < end for start, end in exempt_spans):
                        continue
                    findings.append(
                        Finding(
                            path=path,
                            line_no=index,
                            code=rule.code,
                            line=stripped,
                            message=rule.message,
                            replacement=rule.replacement,
                            context=context,
                        )
                    )
                    break

        client_event_handler_depth = client_event_handler_depth_after(
            code_line, client_event_handler_depth
        )
        previous_line = line

    return findings


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Detect allocation-prone patterns in changed Proa Rust render paths."
    )
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--all", action="store_true", help="scan all tracked Rust source files")
    scope.add_argument("--changed", action="store_true", help="scan changed Rust files")
    scope.add_argument("--staged", action="store_true", help="scan staged Rust files")
    parser.add_argument("--base", help="git base ref for --changed, default: HEAD")
    parser.add_argument(
        "--include-tests",
        action="store_true",
        help="also scan #[cfg(test)] modules and integration-test fixtures",
    )
    parser.add_argument("paths", nargs="*", help="specific files or directories to scan")
    return parser.parse_args()


def paths_from_args(args: argparse.Namespace) -> list[pathlib.Path]:
    if args.paths:
        paths: list[pathlib.Path] = []
        for raw in args.paths:
            path = pathlib.Path(raw)
            full = path if path.is_absolute() else REPO_ROOT / path
            if full.is_dir():
                paths.extend(rel_path(p) for p in full.rglob("*.rs") if should_scan_path(p))
            elif full.exists() and should_scan_path(full):
                paths.append(rel_path(full))
        return sorted(set(paths))

    if args.all:
        return all_rust_files()
    if args.staged:
        if not is_git_checkout():
            raise SystemExit("--staged requires a Git checkout")
        return git_changed_files(args.base, staged=True)
    if args.changed or args.base:
        if not is_git_checkout():
            raise SystemExit("--changed and --base require a Git checkout")
        return git_changed_files(args.base, staged=False)
    if not is_git_checkout():
        return all_rust_files()
    return git_changed_files(args.base, staged=False)


def main() -> int:
    args = parse_args()
    paths = paths_from_args(args)
    if not paths:
        print("proa-perf-lint: no Rust files to scan")
        return 0

    findings: list[Finding] = []
    for path in paths:
        findings.extend(scan_file(path, include_tests=args.include_tests))

    if not findings:
        print(f"proa-perf-lint: scanned {len(paths)} Rust file(s), no render-path allocation smells found")
        return 0

    print(f"proa-perf-lint: found {len(findings)} issue(s) in {len(paths)} Rust file(s)")
    print()
    for finding in findings:
        print(f"{finding.path}:{finding.line_no}: {finding.code} [{finding.context}]")
        print(f"  {finding.message}")
        print(f"  fix: {finding.replacement}")
        print(f"  code: {finding.line}")
        print()

    print("If an allocation is intentional and not on the SSR hot path, add:")
    print("  // proa-perf-ok: short reason")
    print("on the same line or the line immediately before it.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
