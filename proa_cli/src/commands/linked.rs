// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cargo target-selection flags shared by the linked `run` and `test`
//! commands.

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct LinkedExecuteArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    /// Cargo profile override, e.g. release-fast.
    #[arg(long)]
    pub(crate) profile: Option<String>,
    /// Exact Cargo package containing the selected target.
    #[arg(short = 'p', long)]
    pub(crate) package: Option<String>,
    /// Exact Cargo binary target.
    #[arg(long)]
    pub(crate) bin: Option<String>,
    /// Cargo target triple for both descriptor and final compilation.
    #[arg(long)]
    pub(crate) target: Option<String>,
    /// Cargo features enabled for both descriptor and final compilation.
    #[arg(long, value_delimiter = ',')]
    pub(crate) features: Vec<String>,
    /// Enable every Cargo feature.
    #[arg(long)]
    pub(crate) all_features: bool,
    /// Disable Cargo default features.
    #[arg(long)]
    pub(crate) no_default_features: bool,
    /// Require Cargo.lock to remain unchanged.
    #[arg(long)]
    pub(crate) locked: bool,
    /// Disable Cargo network access.
    #[arg(long)]
    pub(crate) offline: bool,
}
