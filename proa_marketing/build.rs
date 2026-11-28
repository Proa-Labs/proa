// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let docs_dir = manifest_dir.join("src/docs");
    let blog_dir = manifest_dir.join("src/blog");

    println!("cargo:rerun-if-changed=build.rs");
    emit_rerun_for_dir(&docs_dir).expect("track docs content");
    emit_rerun_for_dir(&blog_dir).expect("track blog content");

    let markdown = proa_docs_build::MarkdownPipeline::default()
        .code_highlighting(proa_docs_build::CodeHighlight::new())
        .mdx(proa_docs_build::MdxConfig::proa());

    let config = proa_docs_build::Config::new()
        .source(
            proa_docs_build::DocsSource::new(docs_dir)
                .out_dir_name("docs")
                .base_path("/docs")
                .site_name("Proa Docs")
                .markdown(markdown.clone()),
        )
        .source(
            proa_docs_build::DocsSource::new(blog_dir)
                .out_dir_name("blog")
                .base_path("/blog")
                .site_name("Proa Blog")
                .schema(
                    proa_docs_build::PageSchema::new()
                        .required_string("title")
                        .required_string("description")
                        .required_string("author")
                        .required_string("date"),
                )
                .search(proa_docs_build::SearchConfig::disabled())
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
