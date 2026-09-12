#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "gen_srp_chrome", SCRIPT_DIR / "gen_srp_chrome.py"
)
GENERATOR = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(GENERATOR)


class CaptureSanitizerTests(unittest.TestCase):
    def test_removes_executable_hooks_without_disturbing_visual_markup(self):
        source = """<!DOCTYPE html><html><head>
<script src="https://tracker.example/live.js"></script>
<link rel="preload" as="script" href="https://tracker.example/live.js">
<link rel="modulepreload" href="https://tracker.example/module.js">
<link rel="dns-prefetch" href="https://tracker.example">
<link rel="preconnect" href="https://tracker.example">
<link rel="compression-dictionary" href="https://tracker.example/dictionary">
<link rel="stylesheet" href="https://static.example/safe.css">
<link rel="canonical" href="https://static.example/safe-page">
<meta http-equiv="refresh" content="0;url=https://tracker.example">
<meta name="referrer" content="unsafe-url">
</head><body onload="steal()">
<iframe src="https://tracker.example"><script>steal()</script></iframe>
<object data="https://tracker.example/object"></object>
<embed src="https://tracker.example/plugin">
<noscript><img src="https://tracker.example/pixel"></noscript>
<svg><foreignObject><button onclick='steal()'>unsafe</button></foreignObject></svg>
<a href="javascript:steal()" ping="https://tracker.example/ping">unsafe</a>
<a href="data:text/html,%3Cscript%3Esteal()%3C/script%3E">unsafe data URL</a>
<a href="javascript-is-a-plain-slug">safe relative URL</a>
<img src="https://tracker.example/1x1.gif" width="1" height="1">
<img src="https://static.example/product.webp" width="500" height="500"
     onerror="steal()">
<div data-track="{&quot;parentrq&quot;:&quot;0123456789abcdef0123456789abcdef&quot;,
                 &quot;pageci&quot;:&quot;01234567-89ab-cdef-0123-456789abcdef&quot;}"></div>
<div data-pageci="fedcba98-7654-3210-fedc-ba9876543210"></div>
<div data-track='{"payload":"{\\"parentrq\\":\\"abcdef0123456789abcdef0123456789\\"}"}'></div>
<div data-token="fixture-csrf-token-do-not-use-0000">visual content</div>
</body></html>"""

        sanitized = GENERATOR.sanitize_capture_html(source)

        self.assertEqual(GENERATOR.executable_capture_hooks(sanitized), [])
        self.assertNotIn("tracker.example", sanitized)
        self.assertIn("https://static.example/safe.css", sanitized)
        self.assertIn("https://static.example/safe-page", sanitized)
        self.assertIn('href="javascript-is-a-plain-slug"', sanitized)
        self.assertIn("https://static.example/product.webp", sanitized)
        self.assertIn("visual content", sanitized)
        self.assertIn("fixture-csrf-token-do-not-use-0000", sanitized)
        self.assertIn("fixture-parentrq-do-not-use-0000", sanitized)
        self.assertIn("fixture-pageci-do-not-use-0000000000", sanitized)
        self.assertNotIn("0123456789abcdef0123456789abcdef", sanitized)
        self.assertNotIn("01234567-89ab-cdef-0123-456789abcdef", sanitized)
        self.assertNotIn("fedcba98-7654-3210-fedc-ba9876543210", sanitized)
        self.assertNotIn("abcdef0123456789abcdef0123456789", sanitized)
        self.assertEqual(GENERATOR.unplaceholdered_capture_identifiers(sanitized), [])
        self.assertIn('<meta name="referrer" content="no-referrer">', sanitized)

    def test_committed_capture_fixtures_are_already_inert(self):
        for path in GENERATOR.CAPTURE_FIXTURES:
            with self.subTest(path=path.name):
                source = path.read_text()
                self.assertEqual(GENERATOR.sanitize_capture_html(source), source)
                self.assertEqual(GENERATOR.executable_capture_hooks(source), [])
                self.assertEqual(
                    GENERATOR.unplaceholdered_capture_identifiers(source), []
                )


if __name__ == "__main__":
    unittest.main()
