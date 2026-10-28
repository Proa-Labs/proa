// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let docs_dir = manifest_dir.join("src/docs");

    println!("cargo:rerun-if-changed=build.rs");
    emit_rerun_for_dir(&docs_dir).expect("track docs content");

    let asset_version = static_asset_version(&manifest_dir).expect("compute static asset version");
    println!("cargo:rustc-env=MARKETING_ASSET_VERSION={asset_version}");

    let markdown = proa_docs_build::MarkdownPipeline::default()
        .code_highlighting(proa_docs_build::CodeHighlight::new())
        .mdx(proa_docs_build::MdxConfig::proa());

    let config = proa_docs_build::Config::new().source(
        proa_docs_build::DocsSource::new(docs_dir)
            .out_dir_name("docs")
            .base_path("/docs")
            .site_name("Proa Docs")
            .markdown(markdown),
    );

    proa_docs_build::compile(config);
}

fn emit_rerun_for_dir(dir: &Path) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            emit_rerun_for_dir(&path)?;
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Ok(())
}

fn static_asset_version(manifest_dir: &Path) -> io::Result<String> {
    let mut hash = 0xcbf29ce484222325u64;
    for rel in [
        "public/og.jpg",
        "public/apple-touch-icon.png",
        "public/styles.css",
        "public/proa-ui-rail.css",
        "public/docs.js",
        "public/search.mjs",
        "public/agent-tools.mjs",
        "public/search-index.mjs",
        "public/vendor/orama.mjs",
    ] {
        let path = manifest_dir.join(rel);
        println!("cargo:rerun-if-changed={}", path.display());
        hash = fnv1a(hash, rel.as_bytes());
        hash = fnv1a(hash, &fs::read(path)?);
    }
    Ok(format!("{hash:016x}"))
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
