#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import io
import json
import unittest
import urllib.error
from subprocess import CompletedProcess
from unittest.mock import patch

import publish


class PublishedVersionsTests(unittest.TestCase):
    def test_reads_all_versions_without_sending_credentials(self):
        payload = {"versions": [{"num": "0.2.0"}, {"num": "0.1.0"}]}
        with patch("publish.urllib.request.urlopen", return_value=io.BytesIO(json.dumps(payload).encode())) as request:
            self.assertEqual(publish.list_remote_versions(["example"]),
                             {"example": {"0.1.0", "0.2.0"}})
        sent = request.call_args.args[0]
        self.assertEqual(sent.full_url, "https://crates.io/api/v1/crates/example")
        self.assertIsNone(sent.get_header("Authorization"))

    def test_missing_crate_has_no_published_versions(self):
        error = urllib.error.HTTPError("https://crates.io", 404, "missing", {}, None)
        with patch("publish.urllib.request.urlopen", side_effect=error):
            self.assertEqual(publish.list_remote_versions(["unpublished"]), {"unpublished": set()})

    def test_lookup_failure_does_not_become_permission_to_upload(self):
        error = urllib.error.HTTPError("https://crates.io", 429, "rate limited", {}, None)
        with patch("publish.urllib.request.urlopen", side_effect=error):
            with self.assertRaises(urllib.error.HTTPError):
                publish.list_remote_versions(["example"])

    def test_dependencies_are_published_before_dependents(self):
        crates = {"app": (None, "1.0.0", ["core"]), "core": (None, "1.0.0", [])}
        self.assertEqual(publish.topo_sort(crates), ["core", "app"])

    def test_cli_follows_the_libraries_its_scaffolds_emit(self):
        crates = {"proa-cli": (None, "1.0.0", ["core"]),
                  "framework": (None, "1.0.0", ["core"]),
                  "core": (None, "1.0.0", [])}
        self.assertEqual(publish.topo_sort(crates), ["core", "framework", "proa-cli"])

    def test_publish_honors_the_registry_retry_date(self):
        message = ("status 429 Too Many Requests: Please try again after "
                   "Mon, 12 Oct 2026 16:09:51 GMT")
        retry = publish.rate_limit_retry_at(message)
        with patch("publish.subprocess.run", side_effect=[
            CompletedProcess([], 101, "", message), CompletedProcess([], 0, "Uploaded", "")
        ]) as command, patch("publish.time.time", side_effect=[retry - 5, retry]), \
                patch("publish.time.sleep") as sleep:
            self.assertEqual(publish.publish_one("example"), (0, "ok"))
        self.assertEqual(command.call_count, 2)
        sleep.assert_called_once_with(5)

    def test_authentication_and_unrecognized_limits_do_not_retry(self):
        for message in ("authentication failed", "status 429 Too Many Requests",
                        "Please try again after Mon, 12 Oct 2026 16:09:51 GMT",
                        "status 429 Too Many Requests: Please try again after invalid GMT"):
            self.assertIsNone(publish.rate_limit_retry_at(message))

    def test_cli_is_not_uploaded_when_its_library_checks_fail(self):
        with patch("publish.subprocess.run", return_value=CompletedProcess([], 101)) as command:
            self.assertEqual(publish.publish_one("proa-cli"), (101, "error"))
        command.assert_called_once()
        self.assertEqual(command.call_args.args[0],
                         ["cargo", "test", "--locked", "-p", "proa-cli", "--lib"])


if __name__ == "__main__":
    unittest.main()
