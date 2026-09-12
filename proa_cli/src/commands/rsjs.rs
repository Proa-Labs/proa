// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa rsjs`: inspect rsjs compiler analysis artifacts.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::rsjs;
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct RsjsArgs {
    #[command(subcommand)]
    command: RsjsCommand,
}

#[derive(Debug, Subcommand)]
enum RsjsCommand {
    /// Explain why an rsjs impl emitted browser JS or stayed server-only.
    Explain(RsjsExplainArgs),
    /// Print redacted public data crossing records from rsjs strategy sidecars.
    Crossings(RsjsCrossingsArgs),
}

#[derive(Debug, Args)]
struct RsjsExplainArgs {
    /// Strategy sidecar path, analysis id, runtime asset id, struct name, or struct path.
    selector: String,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit stable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RsjsCrossingsArgs {
    /// Optional strategy sidecar path, analysis id, runtime asset id, struct name, or struct path.
    selector: Option<String>,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit stable JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: RsjsArgs) -> Result<()> {
    match args.command {
        RsjsCommand::Explain(args) => {
            let root = resolve_config_root(args.root)?;
            let explanation = rsjs::explain_strategy(&root, &args.selector)?;
            rsjs::print_strategy_explanation(&explanation, args.json)
        }
        RsjsCommand::Crossings(args) => {
            let root = resolve_config_root(args.root)?;
            let report = rsjs::crossing_report(&root, args.selector.as_deref())?;
            rsjs::print_crossing_report(&report, args.json)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;
    use crate::commands::Command;

    use super::RsjsCommand;

    #[test]
    fn rsjs_crossings_accepts_selector_and_json() {
        let cli = Cli::try_parse_from(["proa", "rsjs", "crossings", "Counter", "--json"]).unwrap();

        let Command::Rsjs(args) = cli.command else {
            panic!("expected rsjs command");
        };
        let RsjsCommand::Crossings(args) = args.command else {
            panic!("expected rsjs crossings command");
        };
        assert_eq!(args.selector.as_deref(), Some("Counter"));
        assert!(args.json);
    }
}
