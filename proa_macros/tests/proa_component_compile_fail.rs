// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod support;

#[test]
fn component_authoring_diagnostics() {
    support::configure_trybuild_registry();
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/proa_component_fail/*.rs");
}
