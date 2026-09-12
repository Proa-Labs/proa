// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const PROA_REGISTRY_INDEX: &str = "sparse+https://registry.proa.so/sparse/";

pub fn configure_trybuild_registry() {
    // Trybuild generates temporary crates under CARGO_TARGET_DIR. When that
    // directory is outside this checkout, Cargo cannot discover the
    // repository-local .cargo/config.toml by walking parent directories.
    // Supplying the public index URL keeps versioned path dependencies
    // resolvable without introducing a credential or requiring a fixed target
    // directory.
    std::env::set_var("CARGO_REGISTRIES_PROA_INDEX", PROA_REGISTRY_INDEX);
    // Trybuild's `pass` fixtures are ordinary temporary binaries rather than
    // `cfg(test)` targets. Compile them as deterministic descriptor units:
    // that is the phase whose typing/ABI diagnostics these fixtures exercise,
    // and it avoids pretending their linked binaries are deployable output.
    std::env::set_var("RSJS_DESCRIPTOR_PHASE", "1");
    std::env::set_var("RSJS_CARGO_UNIT_ID", "01".repeat(32));
}
