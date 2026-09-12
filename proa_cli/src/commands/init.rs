// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa init`: write proa.config.json and proa.lock.json into an existing
//! project.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::config;

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Project root to initialize. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Overwrite existing proa.config.json and proa.lock.json.
    #[arg(long)]
    force: bool,
    /// Print the owned files that would be created without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit JSON with written file paths and status.
    #[arg(long)]
    json: bool,
    /// Accept safe defaults. This never adds application capabilities.
    #[arg(long)]
    yes: bool,
}

pub(crate) fn run(args: InitArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => root,
        None => std::env::current_dir()?,
    };
    if !root.join("Cargo.toml").is_file() {
        anyhow::bail!("`proa init` requires an existing Rust project with Cargo.toml");
    }
    let _accept_defaults = args.yes;
    if args.dry_run {
        let files = [config::CONFIG_FILE, config::LOCK_FILE]
            .into_iter()
            .filter(|name| args.force || !root.join(name).exists())
            .collect::<Vec<_>>();
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "operationId": "project.init",
                    "commandVersion": 1,
                    "projectRoot": root,
                    "plannedFilesystemOperations": files.iter().map(|name| serde_json::json!({"operation":"create","path":name})).collect::<Vec<_>>(),
                    "configurationChanges": [],
                    "warnings": [],
                    "result": "planned"
                }))?
            );
        } else {
            println!("Plan {}", root.display());
            for file in files {
                println!("  create  {file}");
            }
        }
        return Ok(());
    }
    let report = config::init_project_files(&root, args.force)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Proa project root: {}", report.root.display());
        print_init_file_status(
            config::CONFIG_FILE,
            &report.config_path,
            report.wrote_config,
        );
        print_init_file_status(config::LOCK_FILE, &report.lock_path, report.wrote_lock);
    }

    Ok(())
}

fn print_init_file_status(name: &str, path: &std::path::Path, wrote: bool) {
    let status = if wrote { "created" } else { "exists" };
    println!("{status:>7} {name} ({})", path.display());
}
