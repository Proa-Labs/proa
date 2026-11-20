// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Project-root discovery shared by commands that operate on a Proa project.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::config;

pub(crate) fn cargo_package_name(root: &Path) -> Option<String> {
    let source = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let manifest = source.parse::<toml::Value>().ok()?;
    Some(manifest.get("package")?.get("name")?.as_str()?.to_owned())
}

pub(crate) fn resolve_config_root(root: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root);
    }

    resolve_config_root_from(std::env::current_dir()?)
}

fn resolve_config_root_from(mut current: PathBuf) -> Result<PathBuf> {
    loop {
        // A virtual Cargo workspace deliberately has no `src/` directory,
        // but it still owns the shared target directory where RSJS strategy
        // sidecars are emitted. Treat every Cargo manifest as a valid nearest
        // project boundary; package manifests continue to work as before.
        if config::project_config_path(&current).is_some() || current.join("Cargo.toml").is_file() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("could not find a Proa project root or proa.config.rs (pass --root)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_root_discovery_accepts_virtual_cargo_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let nested = temp.path().join("member").join("src");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            resolve_config_root_from(nested).unwrap(),
            temp.path().to_path_buf()
        );
    }
}
