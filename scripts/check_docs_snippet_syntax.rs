// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use quote::ToTokens;
use std::{env, fmt::Write, fs, path::PathBuf, process::ExitCode};
use syn::{UseTree, visit::Visit};

#[derive(Default)]
struct Imports {
    app: String,
    config: String,
    count: usize,
}

impl Imports {
    fn collect(&mut self, path: &mut Vec<String>, tree: &UseTree) {
        match tree {
            UseTree::Group(group) => {
                for child in &group.items {
                    self.collect(path, child);
                }
            }
            UseTree::Path(child) => {
                path.push(child.ident.to_string());
                self.collect(path, &child.tree);
                path.pop();
            }
            leaf => {
                let Some(root) = path.first() else { return };
                if root != "rsjs" && root != "proa" && !root.starts_with("proa_") {
                    return;
                }
                // Project configuration gets its own proa::config facade from the CLI.
                let is_config = root == "proa"
                    && (path.get(1).is_some_and(|p| p == "config")
                        || (path.len() == 1
                            && matches!(leaf, UseTree::Name(item) if item.ident == "config")));
                let out = if is_config {
                    &mut self.config
                } else {
                    &mut self.app
                };
                // Blocks keep aliases scoped to their original declaration, even across excerpts.
                writeln!(
                    out,
                    "const _: () = {{ use {}::{{{}}}; }};",
                    path.join("::"),
                    leaf.to_token_stream()
                )
                .unwrap();
                self.count += 1;
            }
        }
    }
}

impl<'ast> Visit<'ast> for Imports {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.collect(&mut Vec::new(), &item.tree);
    }
}

fn check_excerpt(source: &str, imports: &mut Imports) -> syn::Result<()> {
    let file_error = match syn::parse_file(source) {
        Ok(file) => {
            imports.visit_file(&file);
            return Ok(());
        }
        Err(error) => error,
    };
    // Documentation also shows function bodies, trait items and configuration fields.
    let body = if source.trim_start().starts_with('.') {
        format!("{{ receiver{source} }}")
    } else {
        format!("{{ {source} }}")
    };
    if let Ok(block) = syn::parse_str::<syn::Block>(&body) {
        imports.visit_block(&block);
    } else if let Ok(item) = syn::parse_str::<syn::TraitItem>(source) {
        imports.visit_trait_item(&item);
    } else if let Ok(expr) = syn::parse_str::<syn::ExprStruct>(&format!("Config {{ {source} }}")) {
        imports.visit_expr_struct(&expr);
    } else {
        return Err(file_error);
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut success = true;
    let mut args = env::args_os().skip(1);
    let directory = args.next().expect("directory of extracted Rust fences");
    let output = PathBuf::from(args.next().expect("output directory for public imports"));
    let mut paths: Vec<_> = fs::read_dir(directory)
        .expect("read snippets directory")
        .map(|entry| entry.expect("read snippet entry").path())
        .collect();
    paths.sort();
    let mut imports = Imports::default();
    for path in paths {
        let source = fs::read_to_string(&path).expect("read extracted Rust fence");
        let file_name = path.file_name().unwrap().to_string_lossy();
        writeln!(imports.app, "// {file_name}").unwrap();
        writeln!(imports.config, "// {file_name}").unwrap();
        if let Err(error) = check_excerpt(&source, &mut imports) {
            eprintln!("{file_name}: {error}");
            success = false;
        }
    }
    if success {
        fs::write(output.join("app-imports.rs"), imports.app).expect("write application imports");
        fs::write(output.join("config-imports.rs"), imports.config).expect("write config imports");
        println!(
            "Found {} public imports to check against the candidate",
            imports.count
        );
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
