// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Start a previously built release artifact.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use clap::Args;
use proa_assets::{MANIFEST_FILE, OUTPUT_ENV};

use crate::{config, lifecycle, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct StartArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    /// Exact release executable path. Relative paths resolve from the project root.
    #[arg(long)]
    bin: Option<PathBuf>,
    /// Published public catalog directory. Relative paths resolve from the project root.
    #[arg(long, value_name = "DIR")]
    public_assets: Option<PathBuf>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    port: Option<u16>,
}

#[derive(serde::Deserialize)]
struct CargoTargetMetadata {
    target_directory: PathBuf,
}

fn resolve_target_dir(root: &Path) -> PathBuf {
    let manifest_path = root.join("Cargo.toml");
    if manifest_path.is_file() {
        let cargo = env::var_os("CARGO")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "cargo".into());
        if let Ok(output) = Command::new(cargo)
            .current_dir(root)
            .args([
                "metadata",
                "--format-version=1",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest_path)
            .output()
        {
            if output.status.success() {
                if let Ok(metadata) = serde_json::from_slice::<CargoTargetMetadata>(&output.stdout)
                {
                    if metadata.target_directory.is_absolute() {
                        return metadata.target_directory;
                    }
                    return root.join(metadata.target_directory);
                }
            }
        }
    }
    match env::var_os("CARGO_TARGET_DIR").filter(|value| !value.is_empty()) {
        Some(dir) => {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        }
        None => root.join("target"),
    }
}

fn paths_match(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

pub(crate) fn run(args: StartArgs) -> Result<()> {
    let root = std::path::absolute(resolve_config_root(args.root)?)
        .context("resolve the release project root")?;
    let target_dir = resolve_target_dir(&root);
    let manifest = lifecycle::load_start_manifest(&target_dir);
    let selected = if let Some(path) = args.bin {
        path
    } else {
        let project = config::load_project_config(&root)?;
        let configured_binary = project
            .application
            .map(|application| application.binary)
            .filter(|name| !name.is_empty())
            .or_else(|| crate::util::cargo_package_name(&root))
            .unwrap_or_else(|| "app".into());
        let mut filename = std::ffi::OsString::from(&configured_binary);
        let suffix = std::env::consts::EXE_SUFFIX;
        if !suffix.is_empty() && !configured_binary.to_ascii_lowercase().ends_with(suffix) {
            filename.push(suffix);
        }
        let default_release = target_dir.join("release").join(filename);
        if default_release.is_file() {
            default_release
        } else if let Some(recorded) = manifest.as_ref().and_then(|manifest| {
            manifest.entries.iter().rev().find(|entry| {
                entry.target_triple.is_none()
                    && entry.binary == configured_binary
                    && entry.executable.is_file()
            })
        }) {
            recorded.executable.clone()
        } else {
            default_release
        }
    };
    let executable = if selected.is_absolute() {
        selected
    } else {
        root.join(selected)
    };
    if !executable.is_file() {
        bail!(
            "release executable not found at {}\nRun `proa run build` (or `proa build --artifact-out \"{}\"`) first, or select an exported executable with `proa start --bin <path>`",
            executable.display(),
            executable.display()
        );
    }
    let mut command = Command::new(&executable);
    command.current_dir(&root);
    if let Some(directory) = args.public_assets {
        let directory = root.join(directory);
        if !directory.join(MANIFEST_FILE).is_file() {
            bail!(
                "public asset catalog is missing at {}; pass the directory reported by proa build",
                directory.display()
            );
        }
        command.env(OUTPUT_ENV, directory);
    } else if env::var_os(OUTPUT_ENV).is_none() {
        if let Some(directory) = manifest.as_ref().and_then(|manifest| {
            manifest
                .entries
                .iter()
                .rev()
                .find(|entry| paths_match(&entry.executable, &executable))
                .and_then(|entry| entry.public_assets.as_deref())
        }) {
            if directory.join(MANIFEST_FILE).is_file() {
                command.env(OUTPUT_ENV, directory);
            }
        }
    }
    if let Some(host) = args.host {
        command.env("PROA_HOST", host);
    }
    if let Some(port) = args.port {
        command.env("PORT", port.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("start {}", executable.display()))?;
    if !status.success() {
        bail!("release application exited with {status}");
    }
    Ok(())
}
