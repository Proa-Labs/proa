// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Start a previously built release artifact.

use std::{path::PathBuf, process::Command};

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::{config, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct StartArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    /// Exact release executable path. Relative paths resolve from the project root.
    #[arg(long)]
    bin: Option<PathBuf>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    port: Option<u16>,
}

pub(crate) fn run(args: StartArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    let project = config::load_project_config(&root)?;
    let configured_binary = project
        .application
        .as_ref()
        .and_then(|application| application.get("binary"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            project
                .application
                .as_ref()
                .and_then(|application| application.get("package"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("app");
    let selected = args.bin.unwrap_or_else(|| {
        let mut filename = std::ffi::OsString::from(configured_binary);
        filename.push(std::env::consts::EXE_SUFFIX);
        PathBuf::from("target").join("release").join(filename)
    });
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
