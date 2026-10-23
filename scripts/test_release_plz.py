#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise release orchestration without credentials, uploads, or GitHub writes."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[1]
WRAPPER = ROOT / "scripts/release-plz.sh"
REVISION = "a" * 40
TOKEN_NAMES = ("GIT_TOKEN", "GITHUB_TOKEN", "GH_TOKEN", "CARGO_REGISTRY_TOKEN", "CARGO_REGISTRIES_CRATES_IO_TOKEN")
TOOL_STUB = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
tool = Path(sys.argv[0]).name
args = sys.argv[1:]
state_path = Path(os.environ["TEST_TOOL_STATE"])
state = json.loads(state_path.read_text())
if tool in ("release-plz", "cargo-semver-checks") and args == ["--version"]:
    version = state.get(tool)
    if version is None:
        sys.exit(1)
    print(tool + " " + version)
    sys.exit(0)
with Path(os.environ["TEST_INVOCATIONS"]).open("a") as log:
    log.write(json.dumps({"tool": tool, "args": args, "cwd": os.getcwd(),
        "credentials": {name: bool(os.environ.get(name)) for name in
            ("GIT_TOKEN", "GITHUB_TOKEN", "GH_TOKEN", "CARGO_REGISTRY_TOKEN", "CARGO_REGISTRIES_CRATES_IO_TOKEN")}}) + "\n")
if tool == "git":
    if args == ["rev-parse", "HEAD"]:
        print(os.environ["TEST_HEAD"])
    elif args == ["status", "--porcelain"]:
        print(os.environ.get("TEST_DIRTY", ""), end="")
    elif args == ["remote", "get-url", "origin"]:
        print(os.environ.get("TEST_ORIGIN", "https://github.com/example/proa.git"))
    else:
        sys.exit("unexpected git arguments: " + repr(args))
elif tool == "gh":
    assert "repos/example/proa/git/ref/heads/main" in args, args
    print(os.environ.get("TEST_MAIN", os.environ["TEST_HEAD"]))
elif tool == "cargo":
    if args[0].startswith("+"):
        args = args[1:]
    assert args[0] == "install", args
    state[args[1]] = args[args.index("--version") + 1]
    state_path.write_text(json.dumps(state))
elif tool == "rustup":
    assert args[:2] == ["toolchain", "install"], args
elif tool.startswith("check_"):
    if os.environ.get("TEST_FAIL_PREFLIGHT") == tool:
        sys.exit(1)
elif tool != "release-plz":
    sys.exit("unexpected tool: " + tool)
'''


class ReleaseFlowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="proa release flow ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        scripts = self.root / "scripts"
        scripts.mkdir()
        shutil.copy2(WRAPPER, scripts / WRAPPER.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("release-plz", "cargo-semver-checks", "git", "gh", "cargo", "rustup"):
            self.write_stub(self.bin / name)
        for name in ("check_publish_policy.py", "check_crate_archives.py"):
            self.write_stub(scripts / name)
        self.log = self.root / "invocations.jsonl"
        self.state = self.root / "tool-versions.json"
        text = WRAPPER.read_text()
        self.versions = {
            "release-plz": re.search(r'^RELEASE_PLZ_VERSION="([^"]+)"', text, re.M)[1],
            "cargo-semver-checks": re.search(r'^SEMVER_CHECKS_VERSION="([^"]+)"', text, re.M)[1],
        }
        self.state.write_text(json.dumps(self.versions))
        self.toolchain = re.search(r'^RELEASE_TOOLS_TOOLCHAIN="([^"]+)"', text, re.M)[1]

    @staticmethod
    def write_stub(path):
        path.write_text(TOOL_STUB)
        path.chmod(0o755)

    def run_wrapper(self, *arguments, **overrides):
        env = {key: value for key, value in os.environ.items() if key not in TOKEN_NAMES}
        env.update({
            "PATH": str(self.bin) + os.pathsep + env["PATH"],
            "TEST_HEAD": REVISION,
            "TEST_INVOCATIONS": str(self.log),
            "TEST_TOOL_STATE": str(self.state),
            "GIT_TOKEN": "fixture-git-token",
            "CARGO_REGISTRY_TOKEN": "fixture-registry-token",
        })
        for name in ("PROA_RELEASE_EXPECTED_SHA", "PROA_RELEASE_REPOSITORY", "GITHUB_REPOSITORY"):
            env.pop(name, None)
        env.update(overrides)
        # A different cwd and spaces in the checkout catch quoting/path drift.
        return subprocess.run(
            ["bash", str(self.root / "scripts/release-plz.sh"), *arguments],
            cwd=self.bin, env=env, capture_output=True, text=True,
        )

    def calls(self, tool=None):
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return [call for call in calls if tool is None or call["tool"] == tool]

    def assert_passed(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_check_runs_both_preflights_without_credentials_or_remote_operations(self):
        self.assert_passed(self.run_wrapper("check", GH_TOKEN="fixture-gh-token",
                                           CARGO_REGISTRIES_CRATES_IO_TOKEN="fixture-alternate-token"))
        self.assertEqual([c["tool"] for c in self.calls()],
                         ["check_publish_policy.py", "check_crate_archives.py"])
        self.assertTrue(all(not any(c["credentials"].values()) for c in self.calls()))

    def test_install_pins_both_tools_and_withholds_credentials_from_cargo(self):
        self.state.write_text("{}")
        self.assert_passed(self.run_wrapper("install"))
        self.assertEqual(json.loads(self.state.read_text()), self.versions)
        self.assertEqual(len(self.calls("rustup")), 1)
        self.assertIn(self.toolchain, self.calls("rustup")[0]["args"])
        self.assertFalse(any(self.calls("rustup")[0]["credentials"].values()))
        for call in self.calls("cargo"):
            self.assertEqual(call["args"][0], "+" + self.toolchain)
            self.assertIn("--locked", call["args"])
            self.assertFalse(any(call["credentials"].values()))

    def test_version_drift_and_missing_semver_checker_fail_before_preflight(self):
        for tool in self.versions:
            with self.subTest(tool=tool):
                self.state.write_text(json.dumps({**self.versions, tool: "0.0.0"}))
                result = self.run_wrapper("check")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(tool, result.stderr)
                self.assertEqual(self.calls(), [])

    def test_preflight_failure_prevents_publication_and_source_api_access(self):
        result = self.run_wrapper("release", TEST_FAIL_PREFLIGHT="check_crate_archives.py")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls("gh"), [])
        self.assertEqual(self.calls("release-plz"), [])
        self.assertTrue(all(not any(c["credentials"].values()) for c in self.calls()))

    def test_live_release_has_the_same_source_guard_locally_and_on_ci(self):
        for actions in ("false", "true"):
            with self.subTest(actions=actions):
                result = self.run_wrapper("release", GITHUB_ACTIONS=actions, TEST_MAIN="b" * 40)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("stale", result.stderr)
                self.assertEqual(self.calls("release-plz"), [])

    def test_expected_sha_attests_the_actual_checkout(self):
        result = self.run_wrapper("release", PROA_RELEASE_EXPECTED_SHA="b" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("PROA_RELEASE_EXPECTED_SHA", result.stderr)
        self.assertEqual(self.calls("gh"), [])
        self.assertEqual(self.calls("release-plz"), [])

    def test_invalid_main_api_response_cannot_authorize_publication(self):
        result = self.run_wrapper("release", TEST_MAIN="not-a-commit")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls("release-plz"), [])

    def test_ssh_origin_and_case_insensitive_repository_identity_work_locally(self):
        for origin in ("git@github.com:Example/Proa.git", "ssh://git@github.com/Example/Proa.git"):
            with self.subTest(origin=origin):
                self.assert_passed(self.run_wrapper(
                    "release-pr", TEST_ORIGIN=origin, PROA_RELEASE_REPOSITORY="example/proa",
                ))

    def test_dirty_checkout_and_wrong_repository_cannot_open_release_prs(self):
        for overrides in ({"TEST_DIRTY": "?? unexpected.txt\n"},
                          {"PROA_RELEASE_REPOSITORY": "elsewhere/proa"}):
            with self.subTest(overrides=overrides):
                result = self.run_wrapper("release-pr", **overrides)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls("gh"), [])
                self.assertEqual(self.calls("release-plz"), [])

    def test_release_and_release_pr_share_config_and_restore_only_required_tokens(self):
        for command in ("release", "release-pr"):
            with self.subTest(command=command):
                self.assert_passed(self.run_wrapper(command))
                call = self.calls("release-plz")[-1]
                self.assertEqual(call["args"], [command, "--config", str(self.root / "release-plz.toml"),
                                                 "--registry", "crates-io"])
                self.assertTrue(call["credentials"]["GIT_TOKEN"])
                self.assertEqual(call["credentials"]["CARGO_REGISTRY_TOKEN"], command == "release")
                self.assertEqual(call["cwd"], str(self.root))

    def test_both_dry_run_spellings_withhold_registry_credentials_and_allow_branches(self):
        for arguments in (("release-dry-run",), ("release", "--dry-run")):
            with self.subTest(arguments=arguments):
                self.assert_passed(self.run_wrapper(*arguments, TEST_MAIN="b" * 40))
                call = self.calls("release-plz")[-1]
                self.assertIn("--dry-run", call["args"])
                self.assertEqual(call["args"][0], "release")
                self.assertFalse(call["credentials"]["CARGO_REGISTRY_TOKEN"])
                self.assertEqual(self.calls("gh"), [])

    def test_missing_live_credentials_cannot_start_a_release(self):
        for missing in ("GIT_TOKEN", "CARGO_REGISTRY_TOKEN"):
            with self.subTest(missing=missing):
                self.assertNotEqual(self.run_wrapper("release", **{missing: ""}).returncode, 0)
                self.assertEqual(self.calls("release-plz"), [])

    def test_standard_token_aliases_are_normalized_for_live_publication(self):
        self.assert_passed(self.run_wrapper(
            "release", GIT_TOKEN="", GITHUB_TOKEN="fixture-git-token",
            CARGO_REGISTRY_TOKEN="", CARGO_REGISTRIES_CRATES_IO_TOKEN="fixture-registry-token",
        ))
        credentials = self.calls("release-plz")[-1]["credentials"]
        self.assertTrue(credentials["GIT_TOKEN"])
        self.assertTrue(credentials["CARGO_REGISTRY_TOKEN"])
        self.assertFalse(credentials["GITHUB_TOKEN"])
        self.assertFalse(credentials["CARGO_REGISTRIES_CRATES_IO_TOKEN"])


class ReleaseContractTests(unittest.TestCase):
    def test_cli_release_page_has_one_owner_and_its_tag_still_triggers_binary_builds(self):
        config = tomllib.loads((ROOT / "release-plz.toml").read_text())
        cli = next(p for p in config["package"] if p["name"] == "proa-cli")
        self.assertTrue(config["workspace"]["release_always"])
        self.assertFalse(cli["git_release_enable"])
        self.assertTrue(cli["git_tag_enable"])
        tag = cli.get("git_tag_name", config["workspace"]["git_tag_name"])
        self.assertEqual(tag.replace("{{ package }}", "proa-cli").replace("{{ version }}", "1.2.3"),
                         "proa-cli-v1.2.3")
        self.assertIn('- "proa-cli-v*"', (ROOT / ".github/workflows/release-cli.yml").read_text())

    def test_manual_workflow_calls_the_shared_wrapper_instead_of_duplicating_release_logic(self):
        workflow = (ROOT / ".github/workflows/release-plz.yml").read_text()
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("default: check", workflow)
        self.assertIn("fetch-depth: 0", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("if: github.ref == 'refs/heads/main'", workflow)
        self.assertIn("run: scripts/release-plz.sh install", workflow)
        self.assertIn('run: scripts/release-plz.sh "$RELEASE_COMMAND"', workflow)
        for command in ("check", "release-pr", "release-dry-run", "release"):
            self.assertIn("          - " + command + "\n", workflow)
        self.assertIn("secrets.RELEASE_GITHUB_TOKEN", workflow)
        self.assertNotIn("cargo install release-plz", workflow)


if __name__ == "__main__":
    unittest.main()
