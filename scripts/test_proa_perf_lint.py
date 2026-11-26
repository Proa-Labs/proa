#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Regression tests for proa_perf_lint client event handler suppression.

The lint must skip allocations inside `on_*={|...| ...}` closures (they compile
to browser JavaScript) while still flagging allocations in server-side
expressions on the same line, before, after, or between handlers. RSJS
browser-closure primitives are exempt only inside items RSJS analyzes.

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


def render_fixture(body_lines: list[str], rsjs: bool = False) -> str:
    """Wrap template lines in a render fn + html_sync! so the lint scans them.

    With `rsjs`, the impl carries `#[rsjs(component, client)]`.
    """
    lines = [
        "struct Widget;",
        "",
        *(["#[rsjs(component, client)]"] if rsjs else []),
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
    def scan(self, body_lines: list[str], rsjs: bool = False) -> list:
        return self.scan_source(render_fixture(body_lines, rsjs))

    def scan_source(self, source: str) -> list:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fixture.rs"
            self.assertNotIn("tests", path.parts)
            path.write_text(source, encoding="utf-8")
            return proa_perf_lint.scan_file(path, include_tests=False)

    def codes(self, body_lines: list[str], rsjs: bool = False) -> list[str]:
        return [finding.code for finding in self.scan(body_lines, rsjs)]

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

    def test_empty_signal_seeds_are_not_flagged(self):
        findings = self.scan(
            [
                "let draft = rsjs::signal(String::new());",
                "let rows = signal::<Vec<Row>>( Vec::new() );",
            ]
        )
        self.assertEqual(findings, [])

    def test_other_empty_constructors_are_still_flagged(self):
        codes = self.codes(
            [
                "let draft = rsjs::signal(String::new() + suffix);",
                "let label = String::new();",
                "let rows = Vec::new();",
            ]
        )
        self.assertEqual(codes.count("string-new"), 2)
        self.assertEqual(codes.count("vec-new"), 1)

    def test_browser_only_rsjs_closures_are_suppressed(self):
        findings = self.scan(
            [
                "let choose = rsjs::event_handler(",
                "    |_: rsjs::MouseEvent<rsjs::elem::Button>| {",
                '        rsjs::batch(|| mode.set("edit".to_string()));',
                "    },",
                ");",
                "let pick = handler(|value: u32| mode.set(value.to_string()));",
                'rsjs::interval(1_000, || mode.set(format!("{}", 1)));',
                "<input on_input={rsjs::debounce(|e| q.set(e.value().to_string()), 300)} />",
            ],
            rsjs=True,
        )
        self.assertEqual(findings, [])

    def test_server_code_around_browser_closures_is_flagged(self):
        codes = self.codes(
            [
                'let pick = rsjs::handler(|v: u32| m.set(v.to_string())); let s = "a".to_string();',
                "let label = rsjs::memo(|| mode.get().to_string());",
                "let other = my_handler(|| x.to_string());",
                "let other = pricing::handler(|| x.to_string());",
            ],
            rsjs=True,
        )
        self.assertEqual(codes.count("to-string"), 4)

    def test_server_arguments_and_methods_near_browser_closures_are_flagged(self):
        codes = self.codes(
            [
                'let row = self.handler(format!("row {}", id));',
                "rsjs::interval(",
                '    period(format!("{}", ms)),',
                '    || tick.set(format!("{}", 1)),',
                ");",
                "let pick = rsjs::event_handler(",
                "    |event: rsjs::MouseEvent<elem::Button>, value: &'static str| {",
                '        selected.set(format!("{}", value));',
                "    },",
                ");",
            ],
            rsjs=True,
        )
        self.assertEqual(codes.count("format"), 2)

    def test_formatted_empty_signal_seeds_are_not_flagged(self):
        findings = self.scan(
            [
                "let draft = rsjs::signal(",
                "    String::new(),",
                ");",
                "let rows = signal::<Vec<Row>>(",
                "    Vec::new()",
                ");",
            ]
        )
        self.assertEqual(findings, [])

    def test_method_and_foreign_signal_seeds_are_flagged(self):
        codes = self.codes(
            [
                "let draft = self.signal(String::new());",
                "let draft = state::signal(String::new());",
            ]
        )
        self.assertEqual(codes.count("string-new"), 2)

    def test_shared_state_provider_seed_is_suppressed(self):
        codes = self.codes(
            [
                "{rsjs::SharedStateProvider {",
                "    state: WorkbenchState {",
                '        mode: "select".to_string(),',
                '        tags: vec!["a".to_string()],',
                "    },",
                '    children: Panel { title: "Tools".to_string() },',
                "}}",
            ]
        )
        # Only the owned literal field is a seed; `vec![..]` allocates.
        self.assertEqual(codes, ["vec-macro", "to-string", "to-string"])

    def test_shared_state_provider_seed_exempts_only_owned_literal_fields(self):
        # Mirrors `shared_state_provider_seed_exempts_only_owned_literal_initializers`
        # in proa_source_tools.
        findings = self.scan(
            [
                "{rsjs::SharedStateProvider::<WorkbenchState, Panel> {",
                "    state: WorkbenchState {",
                '        mode: "select".to_string(),',
                '        accent: String::from("#3366cc"),',
                '        tone: "warm".to_owned(),',
                "        query: String::new(),",
                "        draft: String::default(),",
                "        rows: Vec::new(),",
                "        filters: Filters { owner: std::string::String::new() },",
                '        title: format!("{} ({})", self.name, self.count),',
                "        tags: self.tags.iter().cloned().collect(),",
                "        json: serde_json::to_string(&self.rows).unwrap(),",
                '        preview: render_to_string(Panel { title: "x" }),',
                "        name: String::from(self.name),",
                "        label: self.label.to_string(),",
                "    },",
                '    children: Panel { title: "Tools" },',
                "}}",
            ],
            rsjs=True,
        )
        self.assertEqual(
            [(finding.line_no, finding.code) for finding in findings],
            [
                (16, "format"),
                (17, "collect"),
                (18, "serde-json-to-string"),
                (19, "render-to-string"),
                (20, "string-from"),
                (21, "to-string"),
            ],
        )

    def test_shared_state_seed_accepts_raw_strings_and_trailing_commas(self):
        self.assertEqual(
            self.codes(
                [
                    "{rsjs::SharedStateProvider {",
                    "    state: WorkbenchState {",
                    '        mode: r#"select"#.to_string(),',
                    '        accent: String::from("blue",),',
                    '        tone: String::from(r##"a "# b"##, ),',
                    '        plain: r"plain".to_owned(),',
                    '        name: String::from(self.name,),',
                    '        title: r#"prefix "#.to_string() + self.title,',
                    "    },",
                    "    children: Panel,",
                    "}}",
                ],
                rsjs=True,
            ),
            ["string-from", "to-string"],
        )

    def test_browser_closure_names_outside_rsjs_items_are_flagged(self):
        # Mirrors `browser_closure_names_outside_rsjs_items_are_server_calls`
        # in proa_source_tools: without `#[rsjs]` these are ordinary calls.
        body = [
            'let label = handler(|| format!("{} items", self.count));',
            "let other = rsjs::handler(|| self.count.to_string());",
            'rsjs::interval(1_000, || tick.set(format!("{}", 1)));',
        ]
        self.assertEqual(self.codes(body), ["format", "to-string", "format"])
        self.assertEqual(self.codes(body, rsjs=True), [])

    def test_rsjs_item_spans_cover_attributed_items_only(self):
        source = "\n".join(
            [
                "#[derive(Clone)]",
                "struct Plain;",
                "#[rsjs::rsjs(component, client)]",
                "#[allow(dead_code)]",
                "impl<L: DataLoader> WebRenderSync<L> for Island { x() }",
                "#[rsjs::client_module]",
                "mod client;",
                "#[client]",
                "fn browser(rows: [u8; 4]) { y() }",
                "#[serde(client)]",
                "fn server() { z() }",
            ]
        )
        lines = source.split("\n")
        spans = proa_perf_lint.rsjs_item_spans(proa_perf_lint.mask_rust_non_code(lines))
        covered = [source[start:end].splitlines()[-1] for start, end in spans]
        self.assertEqual(
            covered,
            [
                "impl<L: DataLoader> WebRenderSync<L> for Island { x() }",
                "fn browser(rows: [u8; 4]) { y() }",
            ],
        )

    def test_rsjs_seed_and_closure_spellings_match_cargo_proa_lint(self):
        # Mirrors `rsjs_seed_and_closure_spellings_match_the_regex_lint` in
        # proa_source_tools.
        source = "\n".join(
            [
                "#[rsjs(component, client)]",
                "impl<L: DataLoader> WebRenderSync<L> for Panel {",
                "    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {",
                "        let mode = rsjs::signal(String::new());",
                "        let pairs = signal::<Vec<(u32, String)>>(Vec::new());",
                "        let named = rsjs::signal::<std::string::String>(std::string::String::new());",
                "        html_sync! {",
                "            <div>",
                "                <button on_click={move |_| {",
                '                    mode.set("inline".to_string());',
                '                }}>"Inline"</button>',
                "                {rsjs::SharedStateProvider::<WorkbenchState, Tools> {",
                "                    state: WorkbenchState {",
                '                        mode: "select".to_string(),',
                "                    },",
                "                    children: Tools {},",
                "                }}",
                "            </div>",
                "        }",
                "        .render(cx)",
                "    }",
                "}",
            ]
        )
        self.assertEqual(self.scan_source(source + "\n"), [])

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
