// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for compile-time error messages
// Run with: cargo test compile_fail

#[test]
fn ui_tests() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail/*.rs");
}
