// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[test]
fn component_authoring_diagnostics() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/proa_component_fail/*.rs");
}
