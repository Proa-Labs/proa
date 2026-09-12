#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Header normalization must not change code or take over upstream notices."""

from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts import copyright_headers as headers  # noqa: E402


class Normalization(unittest.TestCase):
    def test_rust_attributes_and_docs_remain_in_order(self) -> None:
        source = b'#![no_std]\n//! Crate docs.\npub fn run() {}\n'
        result = headers.normalize(Path("src/lib.rs"), source)
        self.assertTrue(result.endswith(source))
        self.assertTrue(result.startswith(b"// Copyright Proa Labs, Inc.\n"))
        self.assertEqual(headers.normalize(Path("src/lib.rs"), result), result)

    def test_legacy_holder_and_year_are_normalized_without_touching_code(self) -> None:
        source = (
            b"// Copyright (c) 2024 The rsjs Authors. All rights reserved.\n"
            b"// SPDX-License-Identifier: Apache-2.0\n\n"
            b'const TEXT: &str = "Copyright The rsjs Authors.";\n'
        )
        result = headers.normalize(Path("src/lib.rs"), source)
        self.assertEqual(result.count(b"// Copyright"), 1)
        self.assertTrue(result.endswith(source.split(b"\n\n", 1)[1]))

    def test_legacy_notice_after_module_docs_is_not_left_behind(self) -> None:
        source = (
            b"#![allow(dead_code)]\n//! Module description.\n\n// Copyright The rsjs Authors.\n"
            b"// SPDX-License-Identifier: Apache-2.0\n\nuse std::fmt;\n"
        )
        result = headers.normalize(Path("src/lib.rs"), source)
        self.assertNotIn(b"The rsjs Authors", result)
        self.assertIn(b"//! Module description.", result)
        self.assertTrue(result.endswith(b"use std::fmt;\n"))
        self.assertEqual(result.count(b"SPDX-License-Identifier:"), 1)
        self.assertNotIn(b"\n\n\n", result)

    def test_shebang_encoding_bom_crlf_are_preserved(self) -> None:
        source = '\ufeff#!/usr/bin/env python3\r\n# coding: utf-8\r\nprint("ok")\r\n'.encode()
        result = headers.normalize(Path("scripts/tool.py"), source)
        self.assertTrue(result.startswith('\ufeff#!/usr/bin/env python3\r\n# coding: utf-8\r\n'.encode()))
        self.assertTrue(result.endswith(b'print("ok")\r\n'))
        self.assertNotIn(b"\n", result.replace(b"\r\n", b""))
        self.assertEqual(headers.normalize(Path("scripts/tool.py"), result), result)

    def test_second_line_encoding_cookie_stays_valid(self) -> None:
        source = b'# Tool description.\n# coding: utf-8\nprint("ok")\n'
        result = headers.normalize(Path("tool.py"), source)
        self.assertEqual(result.splitlines()[:2], source.splitlines()[:2])

    def test_css_charset_precedes_block_comment_header(self) -> None:
        source = b'@charset "UTF-8";\nbody { color: red; }\n'
        result = headers.normalize(Path("style.css"), source)
        self.assertTrue(result.startswith(b'@charset "UTF-8";\n/* Copyright Proa Labs, Inc. */\n'))
        self.assertTrue(result.endswith(b'body { color: red; }\n'))
        self.assertEqual(headers.normalize(Path("style.css"), result), result)

    def test_docker_parser_directives_remain_active(self) -> None:
        for name in ("Dockerfile", "deploy/app.Dockerfile"):
            for directives in (
                b"# syntax=docker/dockerfile:1.7\n",
                b"# syntax=docker/dockerfile:1\n# escape=`\n# check=error=true\n",
                b"  # SyNtAx = docker/dockerfile:1\r\n",
            ):
                with self.subTest(name=name, directives=directives):
                    newline = b"\r\n" if b"\r\n" in directives else b"\n"
                    body = b"FROM scratch" + newline
                    result = headers.normalize(Path(name), directives + newline + body)
                    self.assertTrue(result.startswith(directives))
                    self.assertTrue(result.endswith(body))
                    self.assertEqual(headers.normalize(Path(name), result), result)

    def test_docker_directive_like_comments_are_not_promoted(self) -> None:
        for prefix in (b"# Build description.\n", b"\n", b"FROM scratch\n"):
            source = prefix + b"# syntax=not-an-active-directive\n"
            result = headers.normalize(Path("Dockerfile"), source)
            self.assertTrue(result.startswith(b"# Copyright"))
            self.assertTrue(result.endswith(source.lstrip(b"\n")))

    def test_unknown_holders_and_licenses_require_review(self) -> None:
        for source in (
            b"// Copyright Another Author\nlet x = 1;\n",
            b"// SPDX-License-Identifier: MIT\nlet x = 1;\n",
            b"/*\n * Copyright Another Author\n */\nlet x = 1;\n",
            b"// Licensed under the MIT License\nlet x = 1;\n",
        ):
            with self.subTest(source=source), self.assertRaises(ValueError):
                headers.normalize(Path("file.js"), source)

    def test_data_formats_and_fixtures_are_not_rewritten(self) -> None:
        for name in ("data.json", "README.md", "page.html", "page.marko"):
            self.assertEqual(headers.normalize(Path(name), b"fixture"), b"fixture")
        for name in (
            ".local/notes.rs", "node_modules/pkg/index.js",
            "examples/site/src/components/_proa/button.rs",
            "rsjs_macros/tests/ui/fail/case.rs", "rsjs_macros/golden/case.rs",
            "proa_macros/tests/md_fail/case.rs", "rsjs_ir/tests/a.expected.js",
            "editors/vscode/test/fixture/src/lib.rs", "proa_marketing/public/vendor/orama.mjs",
            "proa_cli/src/site_scaffold/templates/origin/src/components/dialog/mod.rs",
        ):
            with self.subTest(name=name):
                self.assertTrue(headers.excluded(Path(name)))


class GitScope(unittest.TestCase):
    def test_scaffold_sources_and_workspace_copies_are_normalized_together(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            pairs = (
                (
                    "scripts/watch-lock.sh",
                    "proa_cli/templates/watch-lock.sh",
                    b"#!/usr/bin/env bash\necho ok\n",
                ),
                (
                    "examples/origin-routing-atlas/src/main.rs",
                    "proa_cli/src/site_scaffold/templates/origin/src/main.rs",
                    b"fn main() {}\n",
                ),
            )
            for workspace, template, source in pairs:
                for name in (workspace, template):
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(source)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(headers.run(root, True), 0)
                self.assertEqual(headers.run(root, False), 0)
            for workspace, template, source in pairs:
                result = (root / workspace).read_bytes()
                self.assertNotEqual(result, source)
                self.assertEqual(result, (root / template).read_bytes())

    def test_deleted_or_symlinked_catalog_locks_are_not_read(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            private = root / ".local"
            private.mkdir()
            (private / "proa.lock.json").write_text("not public JSON")
            (root / "alias").symlink_to(private, target_is_directory=True)
            self.assertEqual(headers.source_installed_files(root, [
                Path("deleted/proa.lock.json"), Path("alias/proa.lock.json")
            ]), set())

    def test_generated_header_requires_fixing_the_generator(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            generated = root / "rsjs_web/src/generated.rs"
            generated.parent.mkdir(parents=True)
            original = "// Copyright The rsjs Authors.\n// SPDX-License-Identifier: Apache-2.0\n"
            generated.write_text(original)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(headers.run(root, True), 1)
            self.assertEqual(generated.read_text(), original)

    def test_catalog_installed_source_keeps_its_original_license(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            original = "// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.\n// SPDX-License-Identifier: MIT OR Apache-2.0\n"
            (root / "button.rs").write_text(original)
            (root / "proa.lock.json").write_text(json.dumps({
                "components": {"@proa/button": {"installedFiles": ["button.rs"]}}
            }))
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(headers.run(root, True), 0)
            self.assertEqual((root / "button.rs").read_text(), original)

    def test_fix_skips_ignored_and_symlinked_content_and_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            (root / ".gitignore").write_text(".local/\n")
            (root / ".local").mkdir()
            private = root / ".local/private.rs"
            private.write_text("private data\n")
            (root / "link.rs").symlink_to(private)
            (root / "lib.rs").write_text("fn example() {}\n")
            (root / "lib.rs").chmod(0o755)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(headers.run(root, False), 1)
                self.assertEqual(headers.run(root, True), 0)
                self.assertEqual(headers.run(root, False), 0)
            self.assertEqual(private.read_text(), "private data\n")
            self.assertEqual((root / "lib.rs").stat().st_mode & 0o777, 0o755)

    def test_ambiguous_notice_prevents_partial_writes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            (root / "a.rs").write_text("fn example() {}\n")
            (root / "z.rs").write_text("// Copyright Another Author\n")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(headers.run(root, True), 1)
            self.assertEqual((root / "a.rs").read_text(), "fn example() {}\n")


if __name__ == "__main__":
    unittest.main()
