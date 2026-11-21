// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa dev`: build one exact linked server and restart it from persistent
//! caches when files change.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::lifecycle;

#[derive(Debug, Args)]
pub(crate) struct DevArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Host passed to the server as PROA_HOST and used for fallback readiness.
    /// Defaults to the PROA_HOST environment variable, then HOST when it is an
    /// IP address or localhost, then 127.0.0.1.
    #[arg(long)]
    host: Option<String>,
    /// Port value passed to the dev command as PORT. Defaults to the PORT
    /// environment variable, then 3000.
    #[arg(long)]
    port: Option<u16>,
    /// Also watch this file or directory. Repeat to add multiple paths.
    #[arg(long, value_name = "PATH")]
    watch: Vec<PathBuf>,
    /// Open the resolved local URL in the default browser once the server is ready.
    #[arg(long)]
    open: bool,
    /// Disable Cargo and managed-tool network access.
    #[arg(long)]
    offline: bool,
    /// Skip asset commands and public catalog publication.
    #[arg(long)]
    no_assets: bool,
    /// Enable experimental html_hot_sync! static text/attribute updates.
    #[arg(long)]
    hot_static: bool,
    /// Run once without watching for file changes.
    #[arg(long)]
    no_restart: bool,
    /// Do not reload connected browsers after a rebuild.
    #[arg(long)]
    no_reload: bool,
    /// Restart the server for stylesheet changes instead of swapping
    /// stylesheets in place. Use this when stylesheets are compiled into the
    /// binary rather than served from disk.
    #[arg(long)]
    no_css_hot_swap: bool,
    /// Port for the live-reload event stream. Defaults to one the operating
    /// system assigns.
    #[arg(long, value_name = "PORT")]
    reload_port: Option<u16>,
    /// Quiet window a change must survive before a rebuild starts. Defaults to 200.
    #[arg(long, value_name = "MILLISECONDS")]
    debounce_ms: Option<u64>,
    /// Print the detailed build log instead of the interactive summary.
    /// Piped output is always detailed.
    #[arg(long)]
    verbose: bool,
    /// Override the exact Cargo run root. Server arguments may follow Cargo's `--`.
    /// Example: -- cargo run --bin app -- --seed demo.
    #[arg(last = true)]
    command: Vec<String>,
}

pub(crate) fn run(args: DevArgs) -> Result<()> {
    let options = lifecycle::DevOptions {
        root: args.root,
        host: args.host,
        port: args.port,
        watch: args.watch,
        open: args.open,
        offline: args.offline,
        no_assets: args.no_assets,
        hot_static: args.hot_static,
        no_restart: args.no_restart,
        no_reload: args.no_reload,
        no_css_hot_swap: args.no_css_hot_swap,
        reload_port: args.reload_port,
        debounce_ms: args.debounce_ms,
        verbose: args.verbose,
        command: args.command,
    };
    lifecycle::dev(options)
}
