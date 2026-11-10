#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from contextlib import redirect_stderr, redirect_stdout
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import call, patch

from check_registry_install import registry_metadata


class RegistryMetadataTests(unittest.TestCase):
    def setUp(self):
        self.run = self.enterContext(patch("check_registry_install.subprocess.run"))
        self.sleep = self.enterContext(patch("check_registry_install.time.sleep"))
        self.enterContext(redirect_stderr(io.StringIO()))
        self.enterContext(redirect_stdout(io.StringIO()))

    def test_missing_proa_versions_retry_only_dependency_resolution(self):
        for message in (
            'error: failed to select a version for the requirement `proa = "^0.4.0"`',
            'error: no matching package named `proa` found',
            'error: failed to select a version for the requirement `proa_core = "^0.4.0"`',
            'error: no matching package named `proa_html_policy` found',
            'error: no matching package named `rsjs` found',
        ):
            with self.subTest(message=message):
                self.run.reset_mock()
                self.sleep.reset_mock()
                self.run.side_effect = [
                    subprocess.CompletedProcess([], 101, "", message),
                    subprocess.CompletedProcess([], 0, '{"packages": []}', ""),
                ]
                self.assertEqual(registry_metadata(Path("site"), {}), {"packages": []})
                self.assertEqual(self.run.call_count, 2)
                self.assertEqual(self.run.call_args.args[0],
                                 ["cargo", "metadata", "--format-version", "1"])
                self.sleep.assert_called_once_with(5)

    def test_unavailable_version_stops_after_three_attempts(self):
        self.run.return_value = subprocess.CompletedProcess(
            [], 101, "", 'error: no matching package named `proa_core` found')
        with self.assertRaises(subprocess.CalledProcessError):
            registry_metadata(Path("site"), {})
        self.assertEqual(self.run.call_count, 3)
        self.assertEqual(self.sleep.call_args_list, [call(5), call(10)])

    def test_other_resolution_failures_stop_without_retrying(self):
        for message in (
            'error: failed to select a version for `proa_core`: feature does not exist',
            'error: no matching package named `unrelated_dependency` found',
            'error: no matching package named `proactive` found',
            'error: failed to download from the registry',
        ):
            with self.subTest(message=message):
                self.run.reset_mock()
                self.run.return_value = subprocess.CompletedProcess([], 101, "", message)
                with self.assertRaises(subprocess.CalledProcessError):
                    registry_metadata(Path("site"), {})
                self.run.assert_called_once()
                self.sleep.assert_not_called()


if __name__ == "__main__":
    unittest.main()
