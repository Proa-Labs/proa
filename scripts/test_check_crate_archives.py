# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import tempfile
import unittest
from pathlib import Path

from check_crate_archives import check_source_paths


class SourcePathTests(unittest.TestCase):
    def check(self, source: str, archive: set[str]) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = root / "src" / "lib.rs"
            path.parent.mkdir()
            path.write_text(source)
            return check_source_paths("fixture", root, path, archive)

    def test_generated_source_and_comments_are_not_archive_dependencies(self):
        source = '''
const GENERATED: &str = r###"
include_str!("../app.rs");
#[path = "../generated.rs"] mod generated;
"###;
// include_bytes!("../../outside.bin");
/* nested /* comment */ include_str!("../../outside.txt"); */
const BYTE_TEMPLATE: &[u8] = br##"include_str!("../../outside.txt")"##;
'''
        self.assertEqual(self.check(source, {"src/lib.rs"}), [])

    def test_real_paths_after_literals_are_still_checked(self):
        source = '''
const QUOTE: char = '"';
fn with_lifetime<'a>(value: &'a str) {}
const TEXT: &str = r#"include_str!("../../ignored.txt")"#;
const REAL: &str = include_str!("../missing.txt");
#[path = "../../outside.rs"] mod outside;
'''
        problems = self.check(source, {"src/lib.rs"})
        self.assertEqual(len(problems), 2)
        self.assertIn("missing.txt", problems[0])
        self.assertIn(":5:", problems[0])
        self.assertIn("escapes the crate archive", problems[1])

    def test_packaged_include_is_accepted(self):
        self.assertEqual(
            self.check('include_str!("../README.md");', {"src/lib.rs", "README.md"}),
            [],
        )


if __name__ == "__main__":
    unittest.main()
