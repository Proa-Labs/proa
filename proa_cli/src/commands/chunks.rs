// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa chunks`: inspect template consolidation for `html!` / `html_sync!`
//! bodies.

use anyhow::Result;
use clap::Args;

use super::selection::{selected_paths, FileSelectionArgs};
use crate::chunks;

#[derive(Debug, Args)]
pub(crate) struct ChunksArgs {
    #[command(flatten)]
    files: FileSelectionArgs,
    /// Emit stable JSON reports.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: ChunksArgs) -> Result<()> {
    let paths = selected_paths(&args.files)?;
    let reports = chunks::analyze_paths(&paths)?;
    chunks::print_reports(&reports, args.json)
}
