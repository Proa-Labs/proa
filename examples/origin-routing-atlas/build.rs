// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=proa-fonts.lock");
    let fonts =
        proa_fonts_build::collect::CollectOptions::from_build_env().expect("cargo build env");
    if let Err(error) = proa_fonts_build::collect::run(&fonts) {
        panic!("proa fonts: {error}");
    }
}
