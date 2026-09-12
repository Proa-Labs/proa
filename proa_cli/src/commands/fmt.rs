// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa fmt`: format HTML, XML, and raw Markdown template macro bodies.

use anyhow::{bail, Result};
use clap::Args;

use super::selection::{selected_paths, FileSelectionArgs};
use crate::fmt;
use crate::output::{has_errors, print_diagnostics};

#[derive(Debug, Args)]
pub(crate) struct FmtArgs {
    #[command(flatten)]
    files: FileSelectionArgs,
    /// Do not write files; print a diff and exit non-zero if formatting
    /// differs.
    #[arg(long)]
    check: bool,
    /// Reparse output and verify the template tree is structurally identical
    /// (render-preserving). Always on when writing; this flag adds the pass
    /// to --check runs too.
    #[arg(long)]
    verify: bool,
    /// Skip the render-preserving verification before rewriting files.
    #[arg(long, conflicts_with = "verify")]
    no_verify: bool,
}

pub(crate) fn run(args: FmtArgs) -> Result<()> {
    let paths = selected_paths(&args.files)?;
    let summary = if args.files.uses_git_selection() && paths.is_empty() {
        fmt::FormatSummary::default()
    } else {
        // Writes always verify render preservation unless explicitly opted
        // out; check mode only pays for the pass when asked.
        let verify = if args.check {
            args.verify
        } else {
            !args.no_verify
        };
        fmt::format_paths_with_options(&paths, args.check, verify)?
    };
    print_diagnostics(&summary.diagnostics, false, false)?;

    if args.check && !summary.changed.is_empty() {
        for (path, diff) in &summary.diffs {
            println!("--- {}", path.display());
            print!("{diff}");
        }
        for path in &summary.changed {
            eprintln!("{}: Proa macro formatting differs", path.display());
        }
        bail!("Proa formatting check failed");
    }

    if has_errors(&summary.diagnostics) {
        bail!("Proa formatting failed");
    }

    Ok(())
}
