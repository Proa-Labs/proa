// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa completions`: generate shell completions for the proa binary.

use anyhow::Result;
use clap::Args;

use crate::cli::Cli;

#[derive(Debug, Args)]
pub(crate) struct CompletionsArgs {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    shell: clap_complete::Shell,
}

pub(crate) fn run(args: CompletionsArgs) -> Result<()> {
    let mut command = <Cli as clap::CommandFactory>::command();
    clap_complete::generate(args.shell, &mut command, "proa", &mut std::io::stdout());
    Ok(())
}
