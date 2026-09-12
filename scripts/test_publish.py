#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import io
import json
import unittest
import urllib.error
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


if __name__ == "__main__":
    unittest.main()
