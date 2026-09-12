#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise the public/private boundary with isolated Git repositories."""

from __future__ import annotations

import contextlib
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from scripts import check_repository_boundary as boundary  # noqa: E402


class LocalKnowledgeBaseBoundary(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.git("init", "--quiet")
        self.write(".gitignore", ".local/\n")
        self.write("docs/README.md", "[Guide](guide.md)\n")
        self.write("docs/guide.md", "# Current guide\n")
        self.write(".local/knowledge-base/notes.md", "Private fixture contents\n")
        self.git("add", "--", ".gitignore", "docs")
        self.root_patch = patch.object(boundary, "REPO_ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def git(self, *args: str) -> None:
        subprocess.run(
            ["git", *args], cwd=self.root, check=True, capture_output=True
        )

    def write(self, relative: str, contents: str | bytes) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents.encode() if isinstance(contents, str) else contents)

    def check(self) -> tuple[int, str]:
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            status = boundary.main()
        return status, output.getvalue()

    def test_ignored_notes_do_not_enter_public_inventory(self) -> None:
        self.assertFalse(any(boundary.is_local_only(p) for p in boundary.candidate_files()))
        status, output = self.check()
        self.assertEqual(status, 0, output)

    def test_forced_git_add_of_binary_private_file_is_rejected(self) -> None:
        self.write(".local/knowledge-base/attachment.bin", b"\x00private payload")
        self.git("add", "--force", "--", ".local/knowledge-base/attachment.bin")
        status, output = self.check()
        self.assertEqual(status, 1)
        self.assertIn("local-only content must not be tracked", output)
        self.assertNotIn("private payload", output)

    def test_public_symlinks_to_private_content_are_rejected(self) -> None:
        for target in ("notes.md", "missing.md"):
            with self.subTest(target=target):
                link = self.root / "docs/leak.md"
                link.symlink_to(f"../.local/knowledge-base/{target}")
                status, output = self.check()
                self.assertEqual(status, 1)
                self.assertIn("symlink targets local-only content", output)
                link.unlink()

    def test_rust_cannot_embed_ignored_notes(self) -> None:
        self.write(
            "src/lib.rs",
            'const NOTES: &[u8] = include_bytes!("../.local/knowledge-base/notes.md");\n',
        )
        status, output = self.check()
        self.assertEqual(status, 1)
        self.assertIn("include path", output)
        self.assertIn("targets local-only content", output)

    def test_public_markdown_cannot_link_to_private_notes(self) -> None:
        self.write("docs/guide.md", "[Notes](../.local/knowledge-base/notes.md)\n")
        status, output = self.check()
        self.assertEqual(status, 1)
        self.assertIn("local documentation link", output)
        self.assertIn("targets local-only content", output)

    def test_cargo_cannot_depend_on_local_only_paths(self) -> None:
        self.write(
            "Cargo.toml",
            '[workspace]\nmembers = [".local/private-crate"]\n'
            '[workspace.dependencies]\nprivate = { path = ".local/private-crate" }\n',
        )
        status, output = self.check()
        self.assertEqual(status, 1)
        self.assertIn("dependency 'private' targets local-only content", output)
        self.assertIn("workspace member targets local-only content", output)


if __name__ == "__main__":
    unittest.main()
