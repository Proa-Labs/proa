// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub fn configure_trybuild_descriptor_phase() {
    // Trybuild's `pass` fixtures are ordinary temporary binaries rather than
    // `cfg(test)` targets. Compile them as deterministic descriptor units:
    // that is the phase whose typing/ABI diagnostics these fixtures exercise,
    // and it avoids pretending their linked binaries are deployable output.
    std::env::set_var("RSJS_DESCRIPTOR_PHASE", "1");
    std::env::set_var("RSJS_CARGO_UNIT_ID", "01".repeat(32));
}
