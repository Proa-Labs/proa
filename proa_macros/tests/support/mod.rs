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
}
