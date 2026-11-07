#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Regression coverage for affected-package selection and Rust CI commands."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rust_ci import ROOT, affected, commands, main, make_plan, source_timestamps


def package(name, folder, dependencies=(), kinds=("lib", "test")):
    return {
        "name": name,
        "manifest_path": f"/repo/{folder}/Cargo.toml",
        "dependencies": [
            {"name": d, "path": f"/repo/{d}", "kind": kind} for d, kind in dependencies
        ],
        "targets": [{"kind": [kind]} for kind in kinds],
    }


def metadata():
    return {
        "workspace_root": "/repo",
        "packages": [
            package("proa_core", "proa_core"),
            package(
                "proa_macros",
                "proa_macros",
                [("proa_core", "dev")],
                ("proc-macro", "test"),
            ),
            package("proa_source_tools", "proa_source_tools"),
            package(
                "proa-cli",
                "proa_cli",
                [("proa_macros", "dev"), ("proa_source_tools", None)],
            ),
            package("proa_og", "proa_og"),
            package(
                "proa_framework_axum",
                "proa_framework/proa_framework_axum",
                [("proa_core", "build")],
            ),
            package(
                "mcp-incident-desk", "examples/mcp-incident-desk", [("proa_core", None)]
            ),
            package("proa_marketing", "proa_marketing", [("proa_core", None)]),
        ],
    }


class RustSelectionTests(unittest.TestCase):
    def test_leaf_edit_does_not_select_unrelated_tests(self):
        self.assertEqual(affected(metadata(), ["proa_og/src/lib.rs"]), {"proa_og"})
        self.assertEqual(affected(metadata(), ["proa_cli/src/main.rs"]), {"proa-cli"})

    def test_build_dev_and_runtime_input_dependents_are_transitive(self):
        selected = affected(metadata(), ["proa_core/src/lib.rs"])
        self.assertEqual(
            selected, {p["name"] for p in metadata()["packages"]} - {"proa_og"}
        )
        for path in [
            "proa_macros/tests/content_coverage.rs",
            "examples/mcp-incident-desk/src/lib.rs",
        ]:
            self.assertTrue(
                {"proa_source_tools", "proa-cli"} <= affected(metadata(), [path])
            )

    def test_nested_builds_and_sibling_source_reads_select_their_test_consumers(self):
        graph = metadata()
        graph["packages"].extend(
            [
                package("rsjs", "rsjs"),
                package("rsjs_transport", "rsjs_transport"),
                package("proa_docs", "proa_docs/proa_docs"),
                package("proa_docs_build", "proa_docs/proa_docs_build"),
            ]
        )
        for path in (
            "proa_core/src/lib.rs",
            "proa_macros/src/lib.rs",
            "rsjs/src/lib.rs",
        ):
            self.assertIn("rsjs_transport", affected(graph, [path]))
        self.assertIn(
            "proa_docs_build", affected(graph, ["proa_docs/proa_docs/src/icons.rs"])
        )

    def test_nested_grouped_crate_and_non_rust_fixture_ownership(self):
        self.assertEqual(
            affected(
                metadata(),
                ["proa_framework/proa_framework_axum/tests/golden/home.html"],
            ),
            {"proa_framework_axum"},
        )
        self.assertIn(
            "proa_macros", affected(metadata(), ["proa_macros/tests/ui/fail.stderr"])
        )

    def test_unknown_shared_inputs_and_empty_listing_run_everything(self):
        for paths in [
            [],
            ["Cargo.lock"],
            ["proa_core/Cargo.toml"],
            ["rust-toolchain.toml"],
            [".github/workflows/ci.yml"],
            ["scripts/rust_ci.py"],
            ["docs/guide.md"],
            ["proa_og/../proa_core/src/lib.rs"],
            ["deleted_crate/src/lib.rs"],
            ["proa_og/src/lib.rs", "unknown/file"],
        ]:
            with self.subTest(paths=paths):
                self.assertIsNone(affected(metadata(), paths))

    def test_quickstart_document_also_selects_cli(self):
        self.assertEqual(
            affected(metadata(), ["proa_marketing/src/docs/index.mdx"]),
            {"proa_marketing", "proa-cli"},
        )

    def test_dependency_cycles_terminate(self):
        graph = metadata()
        graph["packages"][0]["dependencies"].append(
            {"name": "proa_macros", "path": "/repo/proa_macros", "kind": "dev"}
        )
        self.assertIn("proa_core", affected(graph, ["proa_macros/src/lib.rs"]))

    def test_api_failure_falls_back_to_full_validation(self):
        with tempfile.TemporaryDirectory() as raw:
            plan = Path(raw) / "plan.json"
            env = {
                "GITHUB_EVENT_NAME": "pull_request",
                "GITHUB_REPOSITORY": "test/proa",
                "PR_NUMBER": "1",
                "GITHUB_TOKEN": "fixture",
            }
            with (
                patch.dict(os.environ, env),
                patch("sys.argv", ["rust_ci.py", "plan", "--plan", str(plan)]),
                patch(
                    "rust_ci.changed_files", side_effect=RuntimeError("API unavailable")
                ),
                patch(
                    "rust_ci.subprocess.check_output",
                    return_value=json.dumps(metadata()).encode(),
                ),
            ):
                main()
            self.assertTrue(json.loads(plan.read_text())["full"])

    def test_selected_commands_keep_existing_exclusions_and_docs(self):
        plan = make_plan(metadata(), None)
        library = commands("libraries", plan)[0]
        integration = commands("integration", plan)
        self.assertEqual(library[library.index("proa_marketing") - 1], "--exclude")
        self.assertEqual(
            integration[0][integration[0].index("proa-cli") - 1], "--exclude"
        )
        self.assertIn("--workspace", library)
        self.assertIn("--doc", integration[1])
        self.assertIn("--all-targets", commands("clippy", plan)[0])
        self.assertIn("clippy::correctness", commands("clippy", plan)[0])

    def test_leaf_selection_does_not_run_core_docs_or_feature_matrix(self):
        plan = make_plan(metadata(), ["proa_og/src/lib.rs"])
        self.assertEqual(commands("features", plan), [])
        self.assertEqual(len(commands("integration", plan)), 1)
        self.assertNotIn("--doc", commands("integration", plan)[0])
        self.assertNotIn("--features", commands("integration", plan)[0])
        self.assertEqual(commands("clippy", plan)[0].count("-p"), 1)


class CargoFreshnessTests(unittest.TestCase):
    def test_unchanged_sources_stay_fresh_but_dependency_edits_rebuild(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            shutil.copy2(ROOT / "rust-toolchain.toml", root / "rust-toolchain.toml")
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["dep", "app"]\nresolver = "2"\n'
            )
            (root / ".gitignore").write_text("/target/\n")
            for name in ("dep", "app"):
                (root / name / "src").mkdir(parents=True)
                manifest = (
                    f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2021"\n'
                )
                if name == "app":
                    manifest += '[dependencies]\ndep = { path = "../dep" }\n'
                (root / name / "Cargo.toml").write_text(manifest)
            source = root / "dep/src/lib.rs"
            source.write_text("pub fn value() -> u32 { 1 }\n")
            (root / "app/src/main.rs").write_text(
                'fn main() { println!("{}", dep::value()); }\n'
            )
            environment = dict(
                os.environ, CARGO_TARGET_DIR=str(root / "target"), CARGO_INCREMENTAL="0"
            )

            def build():
                result = subprocess.run(
                    ["cargo", "build", "--offline", "--message-format=json"],
                    cwd=root,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=True,
                )
                messages = [json.loads(line) for line in result.stdout.splitlines()]
                return {
                    message["target"]["name"]: message["fresh"]
                    for message in messages
                    if message["reason"] == "compiler-artifact"
                }

            self.assertFalse(build()["dep"])
            subprocess.run(["git", "init", "--quiet"], cwd=root, check=True)
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            state = root / "target/source-state.json"
            with patch("rust_ci.ROOT", root):
                source_timestamps(state, restore=False)
                before = source.stat().st_mtime_ns
                os.utime(source, ns=(before + 2_000_000_000, before + 2_000_000_000))
                source_timestamps(state, restore=True)
                self.assertEqual(source.stat().st_mtime_ns, before)
                self.assertTrue(
                    all(build().values()),
                    "unchanged dependencies and app must stay fresh",
                )
                source.write_text("pub fn value() -> u32 { 2 }\n")
                changed = source.stat().st_mtime_ns
                source_timestamps(state, restore=True)
                self.assertEqual(source.stat().st_mtime_ns, changed)
                self.assertFalse(build()["dep"], "changed dependencies must rebuild")
                binary = root / "target/debug/app"
                self.assertEqual(
                    subprocess.check_output([str(binary)], text=True).strip(), "2"
                )
                source_timestamps(state, restore=False)
                # Simulate another checkout plus a newly tracked source. Even
                # unchanged files must retain fresh mtimes in this case.
                before = source.stat().st_mtime_ns
                os.utime(source, ns=(before + 2_000_000_000, before + 2_000_000_000))
                (root / "dep/src/new.rs").write_text("// new module-resolution input\n")
                subprocess.run(["git", "add", "."], cwd=root, check=True)
                source_timestamps(state, restore=True)
                self.assertNotEqual(source.stat().st_mtime_ns, before)


if __name__ == "__main__":
    unittest.main()
