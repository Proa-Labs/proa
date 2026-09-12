#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Tests for the linked-acceptance path classifier used by ci.yml."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

# Runnable both as `python3 scripts/test_linked_acceptance_paths.py` and
# through unittest discovery from the repository root.
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from scripts.linked_acceptance_paths import (  # noqa: E402
    is_skippable,
    needs_linked_acceptance,
)


class SkippablePaths(unittest.TestCase):
    def test_marketing_docs_deploy_and_agent_notes_are_skippable(self) -> None:
        for path in (
            "proa_marketing/src/docs/framework/page-caching.mdx",
            "proa_marketing/public/search.mjs",
            "proa_marketing/src/layout.rs",
            "docs/agent/components.md",
            "deploy/gcp-runners/README.md",
            "deploy/coolify-control-proxy/index.js",
            ".claude/skills/add-island/SKILL.md",
            "README.md",
            "proa_cli/README.md",
            "CLAUDE.md",
        ):
            with self.subTest(path=path):
                self.assertTrue(is_skippable(path))

    def test_code_manifests_and_workflows_are_not_skippable(self) -> None:
        for path in (
            "proa_cli/src/lifecycle.rs",
            "Cargo.lock",
            "Cargo.toml",
            "examples/origin-routing-atlas/tests/atlas/golden.rs",
            ".github/workflows/ci.yml",
            "scripts/run_proa_cli_acceptance.sh",
            "rsjs_macros/src/lib.rs",
            "proa_docs/proa_docs_build/src/search.rs",
            # A crate README is fine, a markdown-named source file is not.
            "proa_core/src/markdown.rs",
            # Prefix look-alikes must not match the directory patterns.
            "proa_marketing_core/src/lib.rs",
            "docs_build/src/lib.rs",
        ):
            with self.subTest(path=path):
                self.assertFalse(is_skippable(path))

    def test_traversal_in_paths_is_normalized_before_matching(self) -> None:
        self.assertFalse(is_skippable("docs/../proa_cli/src/main.rs"))
        self.assertTrue(is_skippable("proa_marketing/./src/docs/index.mdx"))


class SuiteDecision(unittest.TestCase):
    def test_docs_only_change_skips(self) -> None:
        self.assertFalse(
            needs_linked_acceptance(
                ["proa_marketing/src/docs/meta.json", "docs/agent/cookbook.md", "README.md"]
            )
        )

    def test_one_code_path_forces_the_suite(self) -> None:
        self.assertTrue(
            needs_linked_acceptance(["proa_marketing/src/docs/meta.json", "proa_cli/src/lint.rs"])
        )

    def test_empty_listing_is_conservative(self) -> None:
        self.assertTrue(needs_linked_acceptance([]))

    def test_rename_out_of_skippable_directory_counts_both_names(self) -> None:
        # changed_files() appends previous_filename for renames; a move from
        # docs/ into a crate must run the suite because of the new location.
        self.assertTrue(needs_linked_acceptance(["docs/old.md", "proa_core/src/new.rs"]))


if __name__ == "__main__":
    unittest.main()
