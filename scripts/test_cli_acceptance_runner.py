#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Check parallel acceptance orchestration without compiling fixture apps."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
CARGO_STUB = '''#!/usr/bin/env python3
import json, os, shutil, sys, time
from pathlib import Path
# Serves as the `cargo` stub and, copied into a target directory by the
# prelude, as the `proa` CLI snapshot the example shards execute directly.
program = Path(sys.argv[0]).name
args = sys.argv[1:]
if program == "cargo" and args[0] == "metadata":
    print(os.environ["TEST_METADATA"])
elif program == "cargo" and args[0] == "test" and "--no-run" in args:
    # The prelude: compile without running, leaving the CLI in debug/.
    debug = Path(os.environ.get("CARGO_TARGET_DIR", "target")).resolve() / "debug"
    debug.mkdir(parents=True, exist_ok=True)
    for name in ("proa", "cargo-proa"):
        shutil.copy2(sys.argv[0], debug / name)
        (debug / name).chmod(0o755)
elif args[0] in ("run", "test"):
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    target.mkdir(parents=True, exist_ok=True)
    # Fail if concurrent consumers that may build or execute binaries share
    # a namespace. A `cargo test` whose target directory the prelude already
    # populated builds nothing and only runs tests, so several of those may
    # share it. Keep the marker until every shard has started so overlap is
    # deterministic.
    if program == "cargo" and args[0] == "test":
        consumer = "test-" + args[args.index("--test") + 1]
        builder = not (target / "debug" / "proa").exists()
    else:
        consumer = args[-3]
        builder = True
    marker = target / "active-consumer"
    if builder:
        try:
            marker.touch(exist_ok=False)
        except FileExistsError:
            sys.exit("parallel shards share an executable namespace: " + str(target))
    elif marker.exists():
        sys.exit("a test shard shares a namespace with a builder: " + str(target))
    state = Path(os.environ["TEST_STATE"])
    (state / consumer).write_text(str(target) + "\\n" + sys.argv[0])
    deadline = time.monotonic() + 20
    while len(list(state.iterdir())) < 4 and time.monotonic() < deadline:
        time.sleep(0.01)
    if len(list(state.iterdir())) < 4:
        sys.exit("not all shards started")
    if consumer == os.environ.get("TEST_FAIL_CONSUMER"):
        sys.exit(23)
elif program != "cargo" or args[0] not in ("fetch", "build"):
    sys.exit("unexpected command: " + program + " " + repr(args))
'''


class AcceptanceRunnerTests(unittest.TestCase):
    def run_shards(self, custom_target=None, fail_consumer=None):
        with tempfile.TemporaryDirectory(prefix="proa acceptance runner ") as temporary:
            checkout = Path(temporary).resolve()
            script = checkout / "run.sh"
            shutil.copy2(ROOT / "scripts/run_proa_cli_acceptance.sh", script)
            scripts = checkout / "scripts"
            scripts.mkdir()
            # The generator owns its own end-to-end linked test. This test
            # isolates the runner's shard and executable namespaces.
            (scripts / "check_origin_scaffold.py").write_text(
                "import subprocess\nsubprocess.run(['cargo', 'run', '--', 'test', "
                "'-p', 'origin-atlas', '--test', 'atlas'], check=True)\n")
            tools = checkout / "bin"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text(CARGO_STUB)
            cargo.chmod(0o755)
            sleep = tools / "sleep"
            sleep.write_text("#!/usr/bin/env python3\nimport time\ntime.sleep(0.05)\n")
            sleep.chmod(0o755)
            state = checkout / "state"
            state.mkdir()
            metadata = {"packages": [{"name": "proa-cli", "targets": [
                {"name": path.stem, "kind": ["test"]}
                for path in (ROOT / "proa_cli/tests").glob("*.rs")
            ]}]}
            env = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                   "TEST_METADATA": json.dumps(metadata), "TEST_STATE": str(state)}
            env.pop("CARGO_TARGET_DIR", None)
            if custom_target is not None:
                env["CARGO_TARGET_DIR"] = custom_target
            if fail_consumer:
                env["TEST_FAIL_CONSUMER"] = fail_consumer
            result = subprocess.run(["bash", str(script), "pr"], cwd=checkout,
                                    env=env, capture_output=True, text=True, timeout=60)
            records = {path.name: path.read_text().split("\n") for path in state.iterdir()}
            paths = {name: Path(record[0]) for name, record in records.items()}
            executables = {name: Path(record[1]) for name, record in records.items()}
            expected_root = (checkout / (custom_target or "target") / "acceptance-shards").resolve()
            halves = {name for name in paths if name.startswith("test-")}
            self.assertEqual(len(halves), 2, result.stdout + result.stderr)
            self.assertEqual(set(paths) - halves, {"proa_webmcp_pizza", "origin-atlas"},
                             result.stdout + result.stderr)
            # The two proa-cli halves share the prebuilt pr-1 directory; the
            # example shards each get their own.
            self.assertEqual({paths[name] for name in halves}, {expected_root / "pr-1"})
            self.assertEqual(paths["proa_webmcp_pizza"], expected_root / "pr-2")
            self.assertEqual(paths["origin-atlas"], expected_root / "pr-3")
            # The pizza shard runs the prelude's snapshot, not a build of its own;
            # the snapshot sits outside every shard's Cargo target directory.
            self.assertEqual(executables["proa_webmcp_pizza"], expected_root / "cli" / "proa")
            self.assertTrue(all(executables[name].name == "cargo" for name in halves))
            return result

    def test_parallel_consumers_have_separate_default_target_directories(self):
        result = self.run_shards()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_relative_and_absolute_custom_targets_with_spaces_are_respected(self):
        with tempfile.TemporaryDirectory(prefix="proa target ") as target:
            for directory in ("custom target", str(Path(target).resolve())):
                with self.subTest(directory=directory):
                    result = self.run_shards(custom_target=directory)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_failed_shard_still_fails_the_aggregate(self):
        result = self.run_shards(fail_consumer="proa_webmcp_pizza")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pr-2: FAILED (exit 23)", result.stdout)
        self.assertIn("pr-1a: ok", result.stdout)
        self.assertIn("pr-1b: ok", result.stdout)
        self.assertIn("pr-3: ok", result.stdout)


if __name__ == "__main__":
    unittest.main()
