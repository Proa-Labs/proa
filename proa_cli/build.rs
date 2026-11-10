// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, env, fs, path::Path};

fn manifest(path: &Path) -> toml::Value {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path)
        .expect("read Cargo manifest")
        .parse()
        .expect("parse Cargo manifest")
}

fn main() {
    let root = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo package root");
    let root = Path::new(&root);
    let package = manifest(&root.join("Cargo.toml"));
    let mut workspace = None;
    let mut versions = BTreeMap::new();
    for (name, dependency) in package["dev-dependencies"]
        .as_table()
        .expect("scaffold dependencies")
    {
        if !name.starts_with("proa_") && name != "proa" && name != "rsjs" {
            continue;
        }
        let dependency = if dependency.get("workspace").and_then(toml::Value::as_bool) == Some(true)
        {
            let workspace = workspace.get_or_insert_with(|| {
                root.ancestors()
                    .skip(1)
                    .filter_map(|parent| {
                        let path = parent.join("Cargo.toml");
                        path.is_file().then(|| manifest(&path))
                    })
                    .find(|value| value.get("workspace").is_some())
                    .expect("Cargo workspace")
            });
            &workspace["workspace"]["dependencies"][name]
        } else {
            dependency
        };
        let version = dependency
            .as_str()
            .or_else(|| dependency.get("version").and_then(toml::Value::as_str))
            .expect("scaffold dependency version");
        versions.insert(name.clone(), version.to_owned());
    }
    let output = Path::new(&env::var_os("OUT_DIR").expect("Cargo build output"))
        .join("scaffold-versions.toml");
    fs::write(
        output,
        toml::to_string(&versions).expect("serialize scaffold versions"),
    )
    .expect("write scaffold versions");
}
