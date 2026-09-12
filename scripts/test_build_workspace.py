#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
SYNC_SCRIPT = REPOSITORY_ROOT / "scripts" / "sync_build_context.py"
WATCH_SCRIPT = REPOSITORY_ROOT / "scripts" / "watch-marketing.sh"
WATCH_LOCK_SCRIPT = REPOSITORY_ROOT / "scripts" / "watch-lock.sh"
SCAFFOLD_WATCH_LOCK_TEMPLATE = (
    REPOSITORY_ROOT / "proa_cli" / "templates" / "watch-lock.sh"
)
SCAFFOLD_DEV_TEMPLATE = REPOSITORY_ROOT / "proa_cli" / "templates" / "dev.sh"


def write_file(path: Path, contents: str, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents)
    path.chmod(mode)


def set_mtime(path: Path, nanoseconds: int, *, follow_symlinks: bool = True) -> None:
    os.utime(
        path,
        ns=(nanoseconds, nanoseconds),
        follow_symlinks=follow_symlinks,
    )


class BuildContextSyncTests(unittest.TestCase):
    def run_sync(
        self, incoming: Path, cached: Path, *, expect_success: bool = True
    ) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            [sys.executable, str(SYNC_SCRIPT), str(incoming), str(cached)],
            check=False,
            capture_output=True,
            text=True,
        )
        if expect_success and result.returncode != 0:
            self.fail(f"sync failed:\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}")
        return result

    def test_sync_preserves_only_content_and_mode_identical_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            incoming = root / "incoming"
            cached = root / "cached"
            incoming.mkdir()
            cached.mkdir()

            initial_ns = 1_700_000_000_000_000_000
            fresh_ns = initial_ns + 10_000_000_000

            write_file(incoming / "same.rs", "unchanged")
            write_file(incoming / "changed.rs", "old-value")
            write_file(incoming / "mode.sh", "#!/bin/sh\n")
            write_file(incoming / "deleted.txt", "delete me")
            write_file(incoming / "kind", "was a file")
            write_file(incoming / "reverse-kind" / "nested.txt", "was a directory")
            write_file(incoming / "links" / "target-a", "a")
            write_file(incoming / "links" / "target-b", "b")
            os.symlink("target-a", incoming / "links" / "stable")
            os.symlink("target-a", incoming / "links" / "changed")
            os.symlink("missing-a", incoming / "links" / "dangling")
            write_file(incoming / "links" / "file-to-link", "was a file")
            os.symlink("target-a", incoming / "links" / "link-to-file")
            for path in incoming.rglob("*"):
                set_mtime(path, initial_ns, follow_symlinks=not path.is_symlink())

            self.run_sync(incoming, cached)
            same_before = (cached / "same.rs").stat()
            stable_link_before = (cached / "links" / "stable").lstat()
            changed_link_before = (cached / "links" / "changed").lstat()
            dangling_link_before = (cached / "links" / "dangling").lstat()
            kind_before = (cached / "kind").lstat()
            reverse_kind_before = (cached / "reverse-kind").lstat()
            file_to_link_before = (cached / "links" / "file-to-link").lstat()
            link_to_file_before = (cached / "links" / "link-to-file").lstat()

            # Simulate a fresh checkout: unchanged bytes receive a new incoming
            # mtime, while real content, mode, shape, and symlink changes occur
            # alongside additions and deletions.
            set_mtime(incoming / "same.rs", fresh_ns)
            set_mtime(incoming / "links" / "stable", fresh_ns, follow_symlinks=False)
            write_file(incoming / "changed.rs", "new-value")
            set_mtime(incoming / "changed.rs", fresh_ns)
            (incoming / "mode.sh").chmod(0o755)
            set_mtime(incoming / "mode.sh", fresh_ns)
            (incoming / "deleted.txt").unlink()
            (incoming / "kind").unlink()
            write_file(incoming / "kind" / "nested.txt", "now a directory")
            shutil.rmtree(incoming / "reverse-kind")
            write_file(incoming / "reverse-kind", "now a file")
            set_mtime(incoming / "kind", initial_ns)
            set_mtime(incoming / "kind" / "nested.txt", initial_ns)
            set_mtime(incoming / "reverse-kind", initial_ns)
            write_file(incoming / "added.txt", "new")
            (incoming / "links" / "changed").unlink()
            os.symlink("target-b", incoming / "links" / "changed")
            set_mtime(incoming / "links" / "changed", fresh_ns, follow_symlinks=False)
            (incoming / "links" / "dangling").unlink()
            os.symlink("missing-b", incoming / "links" / "dangling")
            set_mtime(incoming / "links" / "dangling", fresh_ns, follow_symlinks=False)
            (incoming / "links" / "file-to-link").unlink()
            os.symlink("target-b", incoming / "links" / "file-to-link")
            set_mtime(incoming / "links" / "file-to-link", initial_ns, follow_symlinks=False)
            (incoming / "links" / "link-to-file").unlink()
            write_file(incoming / "links" / "link-to-file", "now a file")
            set_mtime(incoming / "links" / "link-to-file", initial_ns)

            # Cargo's real target cache is a sibling mount. Any accidental
            # target tree inside the cached source workspace must be deleted.
            write_file(cached / "target" / "sentinel", "cargo cache")
            write_file(cached / "proa_marketing" / "public" / "styles.css", "generated")
            styles_before = (cached / "proa_marketing" / "public" / "styles.css").stat()
            write_file(incoming / "proa_marketing" / "public" / "styles.css", "generated")

            self.run_sync(incoming, cached)

            same_after = (cached / "same.rs").stat()
            self.assertEqual(same_after.st_ino, same_before.st_ino)
            self.assertEqual(same_after.st_mtime_ns, same_before.st_mtime_ns)
            self.assertEqual((incoming / "same.rs").stat().st_mtime_ns, fresh_ns)
            self.assertEqual((cached / "changed.rs").read_text(), "new-value")
            self.assertGreater(
                (cached / "changed.rs").stat().st_mtime_ns,
                same_before.st_mtime_ns,
            )
            self.assertEqual(stat.S_IMODE((cached / "mode.sh").stat().st_mode), 0o755)
            self.assertGreater(
                (cached / "mode.sh").stat().st_mtime_ns,
                same_before.st_mtime_ns,
            )
            self.assertFalse((cached / "deleted.txt").exists())
            self.assertEqual((cached / "kind" / "nested.txt").read_text(), "now a directory")
            self.assertEqual((cached / "reverse-kind").read_text(), "now a file")
            self.assertGreater((cached / "kind").stat().st_mtime_ns, kind_before.st_mtime_ns)
            self.assertGreater(
                (cached / "reverse-kind").stat().st_mtime_ns,
                reverse_kind_before.st_mtime_ns,
            )
            self.assertEqual((cached / "added.txt").read_text(), "new")
            self.assertEqual(os.readlink(cached / "links" / "stable"), "target-a")
            self.assertEqual(
                (cached / "links" / "stable").lstat().st_mtime_ns,
                stable_link_before.st_mtime_ns,
            )
            self.assertEqual(os.readlink(cached / "links" / "changed"), "target-b")
            self.assertEqual(os.readlink(cached / "links" / "dangling"), "missing-b")
            self.assertGreater(
                (cached / "links" / "changed").lstat().st_mtime_ns,
                changed_link_before.st_mtime_ns,
            )
            self.assertGreater(
                (cached / "links" / "dangling").lstat().st_mtime_ns,
                dangling_link_before.st_mtime_ns,
            )
            self.assertEqual(os.readlink(cached / "links" / "file-to-link"), "target-b")
            self.assertGreater(
                (cached / "links" / "file-to-link").lstat().st_mtime_ns,
                file_to_link_before.st_mtime_ns,
            )
            self.assertEqual((cached / "links" / "link-to-file").read_text(), "now a file")
            self.assertGreater(
                (cached / "links" / "link-to-file").stat().st_mtime_ns,
                link_to_file_before.st_mtime_ns,
            )
            self.assertFalse((cached / "target").exists())
            self.assertEqual(
                (cached / "proa_marketing" / "public" / "styles.css").read_text(),
                "generated",
            )
            styles_after = (cached / "proa_marketing" / "public" / "styles.css").stat()
            self.assertEqual(styles_after.st_ino, styles_before.st_ino)
            self.assertEqual(styles_after.st_mtime_ns, styles_before.st_mtime_ns)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO creation is unavailable")
    def test_sync_rejects_special_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            incoming = root / "incoming"
            cached = root / "cached"
            incoming.mkdir()
            cached.mkdir()
            os.mkfifo(incoming / "pipe")

            result = self.run_sync(incoming, cached, expect_success=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported special file", result.stderr)


class ScaffoldWatchScriptTests(unittest.TestCase):
    def test_scaffold_uses_the_hardened_workspace_lock_helper(self) -> None:
        self.assertEqual(
            SCAFFOLD_WATCH_LOCK_TEMPLATE.read_bytes(),
            WATCH_LOCK_SCRIPT.read_bytes(),
        )

    @staticmethod
    def run_dev_wrapper(
        root: Path, *arguments: str, settings: dict[str, str] | None = None
    ) -> tuple[subprocess.CompletedProcess[str], list[str], Path]:
        """Run the generated dev.sh against a fake `proa` and capture its argv."""
        scripts = root / "scripts"
        fake_bin = root / "fake-bin"
        state = root / "state"
        scripts.mkdir(parents=True)
        state.mkdir()
        shutil.copyfile(SCAFFOLD_DEV_TEMPLATE, scripts / "dev.sh")
        shutil.copyfile(SCAFFOLD_WATCH_LOCK_TEMPLATE, scripts / "watch-lock.sh")
        write_file(
            fake_bin / "proa",
            """#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$CARGO_TARGET_DIR" > "$PROA_TEST_STATE/target"
printf '%s\n' "$@" > "$PROA_TEST_STATE/args"
""",
            mode=0o755,
        )

        environment = os.environ.copy()
        environment.pop("CARGO_TARGET_DIR", None)
        environment.pop("PROA_DEV_CARGO_TARGET_DIR", None)
        for name in (
            "PROA_DEV_RELOAD",
            "PROA_DEV_RELOAD_PORT",
            "PROA_DEV_CSS_HOT_SWAP",
            "PROA_DEV_DEBOUNCE_MS",
        ):
            environment.pop(name, None)
        environment.update(
            {
                "PATH": f"{fake_bin}{os.pathsep}{environment['PATH']}",
                "PROA_TEST_STATE": str(state),
                "PROA_WATCH_LOCK_ROOT": str(root / "locks"),
            }
        )
        environment.update(settings or {})

        result = subprocess.run(
            ["bash", str(scripts / "dev.sh"), *arguments],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=environment,
        )
        args_file = state / "args"
        forwarded = (
            args_file.read_text().splitlines() if args_file.exists() else []
        )
        return result, forwarded, state

    def test_dev_wrapper_isolates_cargo_and_forwards_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory) / "site"
            result, forwarded, state = self.run_dev_wrapper(root, "--port", "4321")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                (state / "target").read_text().strip(),
                str(root / "target" / "watch" / "dev"),
            )
            # The debounce window is always passed so the script, not the
            # binary, is the one place its default is written down. Caller
            # arguments still come last and therefore still win.
            self.assertEqual(
                forwarded,
                ["dev", "--root", str(root), "--debounce-ms", "300", "--port", "4321"],
            )
            self.assertFalse((root / "locks" / "dev.lock").exists())

    def test_dev_wrapper_maps_live_reload_settings_onto_flags(self) -> None:
        cases = [
            ({"PROA_DEV_RELOAD": "off"}, "--no-reload", True),
            ({"PROA_DEV_RELOAD": "0"}, "--no-reload", True),
            ({"PROA_DEV_RELOAD": "false"}, "--no-reload", True),
            ({"PROA_DEV_RELOAD": "NO"}, "--no-reload", True),
            ({"PROA_DEV_RELOAD": "on"}, "--no-reload", False),
            ({"PROA_DEV_CSS_HOT_SWAP": "off"}, "--no-css-hot-swap", True),
            ({"PROA_DEV_CSS_HOT_SWAP": "on"}, "--no-css-hot-swap", False),
        ]
        for settings, flag, expected in cases:
            with self.subTest(settings=settings):
                with tempfile.TemporaryDirectory() as temporary_directory:
                    root = Path(temporary_directory) / "site"
                    result, forwarded, _ = self.run_dev_wrapper(root, settings=settings)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(flag in forwarded, expected, forwarded)

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory) / "site"
            result, forwarded, _ = self.run_dev_wrapper(
                root,
                settings={
                    "PROA_DEV_RELOAD_PORT": "4500",
                    "PROA_DEV_DEBOUNCE_MS": "750",
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                forwarded,
                [
                    "dev",
                    "--root",
                    str(root),
                    "--reload-port",
                    "4500",
                    "--debounce-ms",
                    "750",
                ],
            )

    def test_dev_wrapper_reports_unusable_settings_instead_of_passing_them_on(
        self,
    ) -> None:
        # A number that is not a number would otherwise reach clap and surface
        # as a usage dump that never names the variable at fault.
        for name in ("PROA_DEV_RELOAD_PORT", "PROA_DEV_DEBOUNCE_MS"):
            with self.subTest(setting=name):
                with tempfile.TemporaryDirectory() as temporary_directory:
                    root = Path(temporary_directory) / "site"
                    result, forwarded, _ = self.run_dev_wrapper(
                        root, settings={name: "soon"}
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(name, result.stderr)
                    self.assertEqual(forwarded, [], "the dev server should not start")

        # An unrecognized on/off value keeps the documented default and says so,
        # rather than silently reading as "off".
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory) / "site"
            result, forwarded, _ = self.run_dev_wrapper(
                root, settings={"PROA_DEV_RELOAD": "maybe"}
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("PROA_DEV_RELOAD", result.stderr)
            self.assertNotIn("--no-reload", forwarded)


class StableStylesheetTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        scripts = self.root / "scripts"
        scripts.mkdir()
        shutil.copy2(WATCH_SCRIPT, scripts / WATCH_SCRIPT.name)
        shutil.copy2(WATCH_LOCK_SCRIPT, scripts / WATCH_LOCK_SCRIPT.name)
        write_file(
            self.root / "proa_marketing" / "styles" / "tailwind.config.cjs",
            "module.exports = {};",
        )
        write_file(
            self.root / "proa_marketing" / "styles" / "input.css",
            "@tailwind utilities;",
        )
        (self.root / "proa_marketing" / "public").mkdir()
        self.styles = self.root / "proa_marketing" / "public" / "styles.css"
        self.tailwind = self.root / "fake-tailwind"
        write_file(
            self.tailwind,
            """#!/usr/bin/env bash
set -euo pipefail
output=
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-o" ]]; then
    output="$2"
    shift 2
  else
    shift
  fi
done
[[ -n "$output" ]]
printf '%s' "${FAKE_CSS_CONTENT:-}" > "$output"
if [[ "${FAKE_TAILWIND_FAIL:-0}" == "1" ]]; then
  exit 17
fi
""",
            mode=0o755,
        )

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def run_build(self, css: str, *, fail: bool = False) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment.update(
            {
                "FAKE_CSS_CONTENT": css,
                "FAKE_TAILWIND_FAIL": "1" if fail else "0",
                "PROA_MARKETING_BUILD_CSS_ONLY": "1",
                "TAILWIND_BIN": str(self.tailwind),
            }
        )
        return subprocess.run(
            ["bash", str(self.root / "scripts" / "watch-marketing.sh")],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )

    def test_identical_output_keeps_inode_and_mtime(self) -> None:
        write_file(self.styles, "same css")
        set_mtime(self.styles, 1_700_000_000_000_000_000)
        before = self.styles.stat()

        result = self.run_build("same css")
        self.assertEqual(result.returncode, 0, result.stderr)
        after = self.styles.stat()
        self.assertEqual(after.st_ino, before.st_ino)
        self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)

    def test_changed_output_is_replaced_and_failure_keeps_last_success(self) -> None:
        write_file(self.styles, "old css")
        before = self.styles.stat()

        result = self.run_build("new css")
        self.assertEqual(result.returncode, 0, result.stderr)
        after = self.styles.stat()
        self.assertNotEqual(after.st_ino, before.st_ino)
        self.assertEqual(self.styles.read_text(), "new css")

        failed = self.run_build("partial css", fail=True)
        self.assertEqual(failed.returncode, 17)
        self.assertEqual(self.styles.read_text(), "new css")
        self.assertEqual(list(self.styles.parent.glob("styles.css.tmp.*")), [])

    def test_watcher_ignores_atomic_stylesheet_temporary_files(self) -> None:
        fake_bin = self.root / "fake-bin"
        cargo_args = self.root / "cargo-args"
        write_file(
            fake_bin / "cargo",
            """#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 2 && "$1" == "watch" && "$2" == "--version" ]]; then
  exit 0
fi
printf '%s\n' "$@" > "$FAKE_CARGO_ARGS"
""",
            mode=0o755,
        )
        write_file(fake_bin / "lsof", "#!/usr/bin/env bash\nexit 1\n", mode=0o755)
        ignored_sentinel = self.styles.parent / "styles.css.tmp.existing"
        write_file(ignored_sentinel, "incomplete css")

        environment = os.environ.copy()
        environment.pop("PROA_MARKETING_BUILD_CSS_ONLY", None)
        environment.pop("PROA_MARKETING_WATCH_RUN", None)
        environment.update(
            {
                "FAKE_CARGO_ARGS": str(cargo_args),
                "FAKE_CSS_CONTENT": "generated css",
                "FAKE_TAILWIND_FAIL": "0",
                "PATH": f"{fake_bin}{os.pathsep}{environment['PATH']}",
                "PROA_WATCH_LOCK_ROOT": str(self.root / "locks"),
                "TAILWIND_BIN": str(self.tailwind),
            }
        )
        result = subprocess.run(
            ["bash", str(self.root / "scripts" / "watch-marketing.sh")],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=environment,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = cargo_args.read_text().splitlines()
        self.assertEqual(
            arguments.count("proa_marketing/public/styles.css.tmp.*"), 1
        )
        self.assertNotIn("proa_marketing/public/styles.css.tmp.existing", arguments)


@unittest.skipUnless(hasattr(os, "killpg"), "process groups are unavailable")
class WatchLockSignalTests(unittest.TestCase):
    def test_sigint_stops_stubborn_watcher_group_and_releases_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            child_script = root / "stubborn-child"
            supervisor_script = root / "watch-supervisor"
            write_file(
                child_script,
                """#!/usr/bin/env bash
set -euo pipefail
state_dir="$1"
trap '' HUP INT TERM
printf '%s\n' "$$" > "$state_dir/child.pid"
bash -c '
  set -euo pipefail
  state_dir="$1"
  trap "" HUP INT TERM
  printf "%s\\n" "$$" > "$state_dir/grandchild.pid"
  while :; do sleep 10; done
' bash "$state_dir" &
wait
""",
                mode=0o755,
            )
            write_file(
                supervisor_script,
                """#!/usr/bin/env bash
set -euo pipefail
source "$1"
export PROA_WATCH_LOCK_ROOT="$2/locks"
proa_watch_lock_acquire test "$2"
proa_watch_run "$2/stubborn-child" "$2"
""",
                mode=0o755,
            )

            process = subprocess.Popen(
                ["bash", str(supervisor_script), str(WATCH_LOCK_SCRIPT), str(root)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                start_new_session=True,
            )
            child_pid: int | None = None
            try:
                pid_paths = (root / "child.pid", root / "grandchild.pid")
                deadline = time.monotonic() + 3
                while not all(path.exists() for path in pid_paths):
                    if process.poll() is not None:
                        stdout, stderr = process.communicate()
                        self.fail(
                            "watch supervisor exited before its children started:\n"
                            f"stdout:\n{stdout}\nstderr:\n{stderr}"
                        )
                    if time.monotonic() >= deadline:
                        self.fail("watch supervisor did not start its children")
                    time.sleep(0.01)

                child_pid = int(pid_paths[0].read_text().strip())
                grandchild_pid = int(pid_paths[1].read_text().strip())
                started = time.monotonic()
                process.send_signal(signal.SIGINT)
                stdout, stderr = process.communicate(timeout=3)
                elapsed = time.monotonic() - started

                self.assertEqual(
                    process.returncode,
                    130,
                    f"stdout:\n{stdout}\nstderr:\n{stderr}",
                )
                self.assertLess(elapsed, 2)
                self.assertFalse((root / "locks" / "test.lock").exists())

                for pid in (child_pid, grandchild_pid):
                    state = subprocess.run(
                        ["ps", "-o", "stat=", "-p", str(pid)],
                        check=False,
                        capture_output=True,
                        text=True,
                    ).stdout.strip()
                    self.assertTrue(not state or state.startswith("Z"), state)
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                if child_pid is not None:
                    try:
                        os.killpg(child_pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass


if __name__ == "__main__":
    unittest.main()
