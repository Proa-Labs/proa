// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Start a previously built release artifact.

use std::{path::PathBuf, process::Command};

use anyhow::{bail, Context, Result};
use clap::Args;
use proa_build::assets::{MANIFEST_FILE, OUTPUT_ENV};

use crate::{config, util::resolve_config_root};

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

pub(crate) fn run(args: StartArgs) -> Result<()> {
    let root = std::path::absolute(resolve_config_root(args.root)?)
        .context("resolve the release project root")?;
    let selected = if let Some(path) = args.bin {
        path
    } else {
        let project = config::load_project_config(&root)?;
        let configured_binary = project
            .application
            .as_ref()
            .and_then(|application| {
                [&application.binary, &application.package]
                    .into_iter()
                    .find(|name| !name.is_empty())
                    .map(String::as_str)
            })
            .unwrap_or("app");
        let mut filename = std::ffi::OsString::from(configured_binary);
        let suffix = std::env::consts::EXE_SUFFIX;
        if !suffix.is_empty() && !configured_binary.to_ascii_lowercase().ends_with(suffix) {
            filename.push(suffix);
        }
        PathBuf::from("target").join("release").join(filename)
    };
    let executable = if selected.is_absolute() {
        selected
    } else {
        root.join(selected)
    };
    if !executable.is_file() {
        bail!(
            "release executable not found at {}\nExport it first with `proa build --artifact-out \"{}\"`, or select an exported executable with `proa start --bin <path>`",
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
