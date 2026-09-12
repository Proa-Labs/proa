// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! File selection flags shared by the fmt, lint, check, and chunks commands.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct FileSelectionArgs {
    /// Rust files or directories to scan. Defaults to the current directory.
    pub(crate) paths: Vec<PathBuf>,
    /// Scan changed Rust files. Defaults to diffing against HEAD unless --base is provided.
    #[arg(long)]
    pub(crate) changed: bool,
    /// Scan staged Rust files.
    #[arg(long)]
    pub(crate) staged: bool,
    /// Base ref for --changed.
    #[arg(long)]
    pub(crate) base: Option<String>,
}

impl FileSelectionArgs {
    pub(crate) fn uses_git_selection(&self) -> bool {
        self.staged || self.changed || self.base.is_some()
    }
}

pub(crate) fn selected_paths(args: &FileSelectionArgs) -> Result<Vec<PathBuf>> {
    let changed = args.changed || args.base.is_some();
    if args.staged && changed {
        bail!("--staged cannot be combined with --changed or --base");
    }
    if (args.staged || changed) && !args.paths.is_empty() {
        bail!("explicit paths cannot be combined with --staged, --changed, or --base");
    }
    if args.staged || changed {
        crate::source::find_changed_rust_files(args.base.as_deref(), args.staged)
    } else {
        Ok(args.paths.clone())
    }
}
