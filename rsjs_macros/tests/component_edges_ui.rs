// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Focused diagnostics for the direct-component relocation surface.

mod support;

#[test]
fn direct_component_edge_diagnostics() {
    support::configure_trybuild_registry();
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/component_edges/direct_struct_requires_component_abi.rs");
}
