// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod support;

#[test]
fn markdown_sync_async_boundary_is_diagnostic() {
    support::configure_trybuild_registry();
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/md_fail/*.rs");
}
