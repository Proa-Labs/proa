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
import json, os, sys, time
from pathlib import Path
args = sys.argv[1:]
if args[0] == "metadata":
    print(os.environ["TEST_METADATA"])
elif args[0] in ("run", "test"):
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    target.mkdir(parents=True, exist_ok=True)
    # Fail if concurrent Cargo consumers share a binary namespace. Keep the
    # marker until every shard has started so overlap is deterministic.
    marker = target / "active-consumer"
    try:
        marker.touch(exist_ok=False)
    except FileExistsError:
        sys.exit("parallel shards share an executable namespace: " + str(target))
    consumer = "test" if args[0] == "test" else args[-3]
    state = Path(os.environ["TEST_STATE"])
    (state / consumer).write_text(str(target))
    deadline = time.monotonic() + 5
    while len(list(state.iterdir())) < 3 and time.monotonic() < deadline:
        time.sleep(0.01)
    if len(list(state.iterdir())) < 3:
        sys.exit("not all shards started")
    if consumer == os.environ.get("TEST_FAIL_CONSUMER"):
        sys.exit(23)
elif args[0] != "fetch":
    sys.exit("unexpected cargo command: " + repr(args))
'''


class AcceptanceRunnerTests(unittest.TestCase):
    def run_shards(self, custom_target=None, fail_consumer=None):
        with tempfile.TemporaryDirectory(prefix="proa acceptance runner ") as temporary:
            checkout = Path(temporary).resolve()
            script = checkout / "run.sh"
            shutil.copy2(ROOT / "scripts/run_proa_cli_acceptance.sh", script)
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
                                    env=env, capture_output=True, text=True, timeout=15)
            paths = {path.name: Path(path.read_text()) for path in state.iterdir()}
            expected_root = (checkout / (custom_target or "target") / "acceptance-shards").resolve()
            self.assertEqual(set(paths), {"test", "proa_webmcp_pizza", "proa_origin_routing_atlas"},
                             result.stdout + result.stderr)
            self.assertEqual(len(set(paths.values())), 3, paths)
            self.assertEqual(set(paths.values()), {expected_root / name for name in ("pr-1", "pr-2", "pr-3")})
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
        self.assertIn("pr-1: ok", result.stdout)
        self.assertIn("pr-3: ok", result.stdout)


if __name__ == "__main__":
    unittest.main()
