#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path
import tomllib
import unittest

from check_origin_scaffold import workspace_manifest


class WorkspaceManifestTests(unittest.TestCase):
    def test_repeated_dependencies_keep_each_sections_options(self):
        source = '''[package]
name = "example"
version = "0.1.0"
[dependencies]
proa_core = { version = "0.4.0", features = ["html-to-markdown"] }
reqwest = { version = "0.12", features = ["json"] }
[build-dependencies]
proa_core = { version = "0.4.0", default-features = false }
[dev-dependencies]
proa_core = { version = "0.4.0", features = ["fragment-ast"] }
reqwest = { version = "0.12", features = ["blocking"] }
'''
        package_path = Path("/workspace/proa_core")
        manifest = tomllib.loads(workspace_manifest(
            source,
            {"proa_core": package_path},
            {"reqwest": {"version": "0.12.24", "default-features": False,
                         "features": ["rustls-tls"]}},
        ))
        self.assertEqual(manifest["dependencies"]["proa_core"],
                         {"path": str(package_path), "features": ["html-to-markdown"]})
        self.assertEqual(manifest["build-dependencies"]["proa_core"],
                         {"path": str(package_path), "default-features": False})
        self.assertEqual(manifest["dev-dependencies"]["proa_core"],
                         {"path": str(package_path), "features": ["fragment-ast"]})
        self.assertEqual(manifest["dependencies"]["reqwest"],
                         {"version": "0.12.24", "default-features": False,
                          "features": ["json", "rustls-tls"]})
        self.assertEqual(manifest["dev-dependencies"]["reqwest"],
                         {"version": "0.12.24", "default-features": False,
                          "features": ["blocking", "rustls-tls"]})


if __name__ == "__main__":
    unittest.main()
