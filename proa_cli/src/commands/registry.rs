// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa registry`: inspect configured component registries.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::config;
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct RegistryArgs {
    #[command(subcommand)]
    command: RegistryCommand,
}

#[derive(Debug, Subcommand)]
enum RegistryCommand {
    /// Print component registry aliases from proa.config.json.
    List(RegistryListArgs),
}

#[derive(Debug, Args)]
struct RegistryListArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON registry aliases.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: RegistryArgs) -> Result<()> {
    match args.command {
        RegistryCommand::List(args) => {
            let root = resolve_config_root(args.root)?;
            let config = config::load_project_config(&root)?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&config.registries)?);
            } else {
                println!("Proa registries for {}", root.display());
                for (alias, template) in config.registries {
                    println!("{alias:>10}  {template}");
                }
            }

            Ok(())
        }
    }
}
