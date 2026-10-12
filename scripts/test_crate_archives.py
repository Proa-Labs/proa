#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import tempfile
import unittest
from pathlib import Path

from check_crate_archives import check_source_paths


class IncludeBoundaryTests(unittest.TestCase):
    def test_embedded_scaffold_code_is_data_but_real_include_still_requires_archive_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "lib.rs"
            source.write_text('const TEMPLATE: &str = r###"include_str!("missing-template.rs");\n#[path = "generated.rs"] mod generated;"###;\nconst REAL: &str = include_str!("missing-real.txt");\n')
            problems = check_source_paths("example", root, source, {"lib.rs"})
            self.assertEqual(len(problems), 1)
            self.assertIn("missing-real.txt", problems[0])
            self.assertIn(":3:", problems[0])


if __name__ == "__main__":
    unittest.main()
