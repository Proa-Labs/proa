// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let mut success = true;
    let directory = env::args_os()
        .nth(1)
        .expect("directory of extracted Rust fences");
    let mut paths: Vec<_> = fs::read_dir(directory)
        .expect("read snippets directory")
        .map(|entry| entry.expect("read snippet entry").path())
        .collect();
    paths.sort();
    for path in paths {
        let source = fs::read_to_string(&path).expect("read extracted Rust fence");
        if let Err(error) = syn::parse_file(&source) {
            // Documentation also shows function bodies, trait items and method chains.
            let body = if source.trim_start().starts_with('.') {
                format!("{{ receiver{source} }}")
            } else {
                format!("{{ {source} }}")
            };
            if syn::parse_str::<syn::Block>(&body).is_err()
                && syn::parse_str::<syn::TraitItem>(&source).is_err()
                && syn::parse_str::<syn::ExprStruct>(&format!("Config {{ {source} }}")).is_err()
            {
                eprintln!("{}: {error}", path.file_name().unwrap().to_string_lossy());
                success = false;
            }
        }
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
