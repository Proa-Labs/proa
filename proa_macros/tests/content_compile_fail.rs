// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[test]
fn content_markers_reject_invalid_syntax_and_type_check_omitted_values() {
    trybuild::TestCases::new().compile_fail("tests/ui/content/*.rs");
}
