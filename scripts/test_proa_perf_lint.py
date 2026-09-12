#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Regression tests for proa_perf_lint client event handler suppression.

The lint must skip allocations inside `on_*={|...| ...}` closures (they compile
to browser JavaScript) while still flagging allocations in server-side
expressions on the same line, before, after, or between handlers.

Run with: python3 scripts/test_proa_perf_lint.py
"""

from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest

SCRIPTS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS_DIR))

import proa_perf_lint  # noqa: E402


def render_fixture(body_lines: list[str]) -> str:
    """Wrap template lines in a render fn + html_sync! so the lint scans them."""
    lines = [
        "struct Widget;",
        "",
        "impl Widget {",
        "    fn render(self) {",
        "        html_sync! {",
    ]
    lines.extend("            " + body_line for body_line in body_lines)
    lines.extend(
        [
            "        }",
            "    }",
            "}",
        ]
    )
    return "\n".join(lines) + "\n"


class ScanFixtureTest(unittest.TestCase):
    def scan(self, body_lines: list[str]) -> list:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fixture.rs"
            self.assertNotIn("tests", path.parts)
            path.write_text(render_fixture(body_lines), encoding="utf-8")
            return proa_perf_lint.scan_file(path, include_tests=False)

    def codes(self, body_lines: list[str]) -> list[str]:
        return [finding.code for finding in self.scan(body_lines)]

    # Sanity check: the fixture harness reports server allocations at all.
    def test_allocation_without_handler_is_flagged(self):
        codes = self.codes(['<span label={format!("n={}", n)}>"x"</span>'])
        self.assertIn("format", codes)

    # F7: an allocation after a closed same-line handler is server-side code.
    def test_allocation_after_closed_same_line_handler_is_flagged(self):
        codes = self.codes(
            [
                "<button on_click={|| count.set(0)}"
                ' label={format!("clicked {} times", n)}>"go"</button>'
            ]
        )
        self.assertIn("format", codes)

    # A real allocation inside a genuine same-line handler stays suppressed.
    def test_allocation_inside_same_line_handler_is_suppressed(self):
        findings = self.scan(
            ['<button on_click={|| log.push(format!("clicked {} times", n))}>"go"</button>']
        )
        self.assertEqual(findings, [])

    # F23: a `{` from a later server attribute must not be credited to the
    # handler; the next-line allocation runs on the SSR hot path.
    def test_server_attribute_brace_not_credited_to_handler(self):
        codes = self.codes(
            [
                "<input on_input={|e| value.set(e)} class={",
                'format!("input-{}", self.kind)',
                "} />",
            ]
        )
        self.assertIn("format", codes)

    # F23: an allocation after the braces closing a multiline handler is
    # server-side code and must be scanned.
    def test_allocation_on_multiline_handler_close_line_is_flagged(self):
        codes = self.codes(
            [
                "<button on_click={|| {",
                "count.set(count.get() + 1);",
                '}}>{format!("clicks: {}", self.count)}</button>',
            ]
        )
        self.assertIn("format", codes)

    # A multiline handler still suppresses allocations in its interior.
    def test_multiline_handler_interior_is_suppressed(self):
        findings = self.scan(
            [
                "<button on_click={|| {",
                'let label = format!("clicked {} times", count.get());',
                "send(label);",
                '}}>"go"</button>',
            ]
        )
        self.assertEqual(findings, [])

    # A second handler that opens after an earlier one closed on the same line
    # is still tracked across lines.
    def test_second_handler_on_same_line_still_suppresses(self):
        findings = self.scan(
            [
                "<input on_focus={|| mark()} on_input={|e| {",
                'value.set(format!("v{}", e));',
                "}} />",
            ]
        )
        self.assertEqual(findings, [])

    def test_open_brace_inside_string_does_not_hide_following_server_allocation(self):
        codes = self.codes(
            [
                '<button on_click={|| log("{")}'
                ' label={format!("server {}", n)}>"go"</button>'
            ]
        )
        self.assertIn("format", codes)

    def test_close_braces_inside_handler_strings_and_comments_do_not_end_it(self):
        findings = self.scan(
            [
                "<button on_click={|| {",
                'let marker = r#"}}"#; // }} must remain lexical trivia',
                'log(format!("client {}", marker));',
                "}} />",
            ]
        )
        self.assertEqual(findings, [])

    def test_comment_brace_does_not_keep_closed_handler_open(self):
        codes = self.codes(
            [
                "<button on_click={|| mark()} // {",
                ' label={format!("server {}", n)}>"go"</button>',
            ]
        )
        self.assertIn("format", codes)


class HelperTest(unittest.TestCase):
    def test_non_code_mask_preserves_columns_and_removes_braces(self):
        line = 'let x = r#"{ hidden }"#; /* } */ run({ value }); // {'
        masked = proa_perf_lint.mask_rust_non_code([line])[0]
        self.assertEqual(len(masked), len(line))
        self.assertEqual(proa_perf_lint.count_braces(masked), 0)
        self.assertIn("run({ value })", masked)

    def test_depth_after_does_not_credit_server_attribute_brace(self):
        line = "<input on_input={|e| value.set(e)} class={"
        self.assertEqual(proa_perf_lint.client_event_handler_depth_after(line, 0), 0)

    def test_depth_after_tracks_multiline_handler_open(self):
        self.assertEqual(
            proa_perf_lint.client_event_handler_depth_after("<button on_click={|| {", 0),
            2,
        )

    def test_depth_after_closes_carried_handler(self):
        line = '}}>{format!("clicks: {}", self.count)}</button>'
        self.assertEqual(proa_perf_lint.client_event_handler_depth_after(line, 2), 0)

    def test_closed_handler_prefix_is_not_client_expression(self):
        line = (
            "<button on_click={|| count.set(0)}"
            ' label={format!("clicked {} times", n)}>"go"</button>'
        )
        match_start = line.index("format!")
        self.assertFalse(
            proa_perf_lint.is_client_event_handler_expression(line, match_start)
        )

    def test_open_handler_match_is_client_expression(self):
        line = '<button on_click={|| log.push(format!("x{}", n))}>"go"</button>'
        match_start = line.index("format!")
        self.assertTrue(
            proa_perf_lint.is_client_event_handler_expression(line, match_start)
        )

    def test_carried_handler_close_index(self):
        line = '}}>{format!("clicks: {}", self.count)}</button>'
        self.assertEqual(proa_perf_lint.carried_handler_close_index(line, 2), 2)
        self.assertEqual(proa_perf_lint.carried_handler_close_index(line, 0), 0)
        self.assertEqual(
            proa_perf_lint.carried_handler_close_index("count.set(1);", 2),
            len("count.set(1);"),
        )


if __name__ == "__main__":
    unittest.main()
