// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for compile-time error messages
// Run with: cargo test compile_fail

mod support;

#[test]
fn ui_tests() {
    support::configure_trybuild_registry();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail/*.rs");
}
