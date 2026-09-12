// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa new`: create application-first Proa projects.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};

use crate::{
    capabilities::{
        CapabilityOverrides, DatabaseChoice, DeployChoice, StarterChoice, StylingChoice, UiChoice,
    },
    scaffold::DryRunFormat,
    site_scaffold,
};

#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    #[command(subcommand)]
    command: NewCommand,
}

#[derive(Debug, Subcommand)]
enum NewCommand {
    /// Create a production-shaped Proa site.
    Site(NewSiteArgs),
}

#[derive(Debug, Args)]
struct NewSiteArgs {
    /// Full destination path. Its final segment becomes the package name.
    path: PathBuf,
    /// Product preset: app, marketing, docs, minimal, or origin (`basic` aliases app).
    #[arg(long, default_value = "app")]
    template: String,
    /// Override the preset's database capability.
    #[arg(long, value_enum)]
    database: Option<DatabaseChoice>,
    /// Override the preset's UI capability.
    #[arg(long, value_enum)]
    ui: Option<UiChoice>,
    /// Override the preset's styling capability.
    #[arg(long, value_enum)]
    styling: Option<StylingChoice>,
    /// Override the preset's starter content.
    #[arg(long, value_enum)]
    starter: Option<StarterChoice>,
    /// Add a portable or provider-specific deployment capability.
    #[arg(long, value_enum)]
    deploy: Option<DeployChoice>,
    /// Human-facing display title (does not change the package name).
    #[arg(long)]
    title: Option<String>,
    /// Deprecated parent-directory alias; the positional path is canonical.
    #[arg(long)]
    destination: Option<PathBuf>,
    /// Generate the recommended VS Code workspace files.
    #[arg(long, conflicts_with = "no_vscode")]
    vscode: bool,
    /// Omit the entire .vscode directory.
    #[arg(long)]
    no_vscode: bool,
    /// Generate AGENTS.md (enabled by default).
    #[arg(long, conflicts_with = "no_agents_md")]
    agents_md: bool,
    /// Omit AGENTS.md.
    #[arg(long)]
    no_agents_md: bool,
    /// Compatibility alias for --template minimal --starter blank.
    #[arg(long)]
    empty: bool,
    /// Compatibility alias for --styling tailwind.
    #[arg(long, conflicts_with = "no_tailwind")]
    tailwind: bool,
    /// Compatibility alias for --styling css.
    #[arg(long)]
    no_tailwind: bool,
    /// Do not initialize a Git repository.
    #[arg(long)]
    no_git: bool,
    /// Compile and run the full local validation after committing the project.
    #[arg(long)]
    verify: bool,
    /// Enter `proa dev` after committing the project.
    #[arg(long)]
    start: bool,
    /// Start the project and open its local URL.
    #[arg(long)]
    open: bool,
    /// Forbid registry, tool, and Cargo network access.
    #[arg(long)]
    offline: bool,
    /// Resolve and print the complete plan without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Emit machine-readable JSON output.
    #[arg(long)]
    json: bool,
    /// Accept recommended defaults; never implies verify, start, open, or deploy.
    #[arg(long)]
    yes: bool,
}

pub(crate) fn run(args: NewArgs) -> Result<()> {
    match args.command {
        NewCommand::Site(args) => run_site(args),
    }
}

fn run_site(mut args: NewSiteArgs) -> Result<()> {
    if args.tailwind && args.styling.is_some() {
        bail!("--tailwind cannot be combined with --styling");
    }
    if args.no_tailwind && args.styling.is_some() {
        bail!("--no-tailwind cannot be combined with --styling");
    }
    if args.empty && (args.template != "app" || args.starter.is_some()) {
        bail!("--empty cannot be combined with an explicit template or starter");
    }
    if args.empty {
        args.template = "minimal".to_string();
        args.starter = Some(StarterChoice::Blank);
    }
    if args.tailwind {
        args.styling = Some(StylingChoice::Tailwind);
    }
    if args.no_tailwind {
        args.styling = Some(StylingChoice::Css);
    }
    let dry_run = args.dry_run.then_some(if args.json {
        DryRunFormat::Json
    } else {
        DryRunFormat::Text
    });
    let _recommended_defaults = args.yes;

    site_scaffold::new_site(site_scaffold::NewSiteOptions {
        path: args.path,
        destination: args.destination,
        title: args.title,
        template: args.template,
        overrides: CapabilityOverrides {
            database: args.database,
            ui: args.ui,
            styling: args.styling,
            starter: args.starter,
            deploy: args.deploy,
        },
        vscode: args.vscode || !args.no_vscode,
        agents_md: args.agents_md || !args.no_agents_md,
        no_git: args.no_git,
        verify: args.verify,
        start: args.start || args.open,
        open: args.open,
        offline: args.offline,
        dry_run,
        json: args.json,
    })
}
