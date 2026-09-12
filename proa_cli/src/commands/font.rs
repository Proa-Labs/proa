// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa font`: vendor and inspect self-hosted, metric-matched fonts
//! (proa_fonts). Fonts are declared in code with `google_font!` /
//! `local_font!`; this command runs the build collector outside a build.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::font;
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct FontArgs {
    #[command(subcommand)]
    command: FontCommand,
}

#[derive(Debug, Subcommand)]
enum FontCommand {
    /// Vendor and lock every font declared in the project's source, or with
    /// --offline verify the lock and vendored files instead (the CI gate).
    Sync(FontSyncArgs),
    /// List fonts recorded in proa-fonts.lock.
    List(FontListArgs),
}

#[derive(Debug, Args)]
struct FontSyncArgs {
    /// Project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Verify only: exit non-zero instead of fetching when a declaration is
    /// not satisfied by proa-fonts.lock and the vendored files.
    #[arg(long)]
    offline: bool,
    /// Vendored-files directory. Defaults to public/fonts; must match the
    /// build script's CollectOptions when that is customized.
    #[arg(long, value_name = "DIR")]
    fonts_dir: Option<PathBuf>,
    /// URL prefix referenced by the generated CSS. Defaults to /fonts.
    #[arg(long, value_name = "PREFIX")]
    url_prefix: Option<String>,
}

#[derive(Debug, Args)]
struct FontListArgs {
    /// Project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit stable JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: FontArgs) -> Result<()> {
    match args.command {
        FontCommand::Sync(a) => font::sync(font::SyncOptions {
            root: resolve_config_root(a.root)?,
            offline: a.offline,
            fonts_dir: a.fonts_dir,
            url_prefix: a.url_prefix,
        }),
        FontCommand::List(a) => font::list(resolve_config_root(a.root)?, a.json),
    }
}
