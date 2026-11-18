// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa config`: inspect and validate proa.config.rs.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};

use crate::config;
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Compile and validate project configuration.
    Validate(ConfigInspectArgs),
    /// Print the effective configuration with defaults filled in.
    Show(ConfigInspectArgs),
}

#[derive(Debug, Args)]
struct ConfigInspectArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommand::Validate(args) => {
            let root = resolve_config_root(args.root)?;
            let validation = config::validate_project_config(&root)?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&validation)?);
            } else {
                match &validation.config_path {
                    Some(path) => println!("Validating {}", path.display()),
                    None => println!("Validating inferred config for {}", root.display()),
                }
                for warning in &validation.warnings {
                    println!("warning: {warning}");
                }
                for error in &validation.errors {
                    println!("error: {error}");
                }
                if validation.warnings.is_empty() && validation.errors.is_empty() {
                    println!("Project config is valid");
                }
            }

            if !validation.errors.is_empty() {
                bail!("Proa config validation failed");
            }
            Ok(())
        }
        ConfigCommand::Show(args) => {
            let root = resolve_config_root(args.root)?;
            let config = config::load_project_config(&root)?;
            println!("{}", serde_json::to_string_pretty(&config)?);
            Ok(())
        }
    }
}
