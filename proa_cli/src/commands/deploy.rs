// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Thin deployment orchestration over provider-native workflows.

use std::{path::PathBuf, process::Command};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use crate::{config, scaffold, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct DeployArgs {
    #[command(subcommand)]
    command: DeployCommand,
}

#[derive(Debug, Subcommand)]
enum DeployCommand {
    /// Upload through Railway's official CLI.
    Railway(RailwayArgs),
    /// Run a blocking deployment command, then invoke the configured CDN invalidation.
    Run(cdn::RunArgs),
    /// Validate and explain the Coolify deployment contract.
    Coolify(CoolifyArgs),
}

#[derive(Debug, Args)]
struct RailwayArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    environment: Option<String>,
    #[arg(long)]
    detach: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct CoolifyArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    check: bool,
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: DeployArgs) -> Result<()> {
    match args.command {
        DeployCommand::Railway(args) => railway(args),
        DeployCommand::Run(args) => cdn::run(args),
        DeployCommand::Coolify(args) => coolify(args),
    }
}

fn railway(args: RailwayArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    require_provider(&root, "railway")?;
    let report = scaffold::site_check(Some(root.clone()))?;
    if !report.errors.is_empty() {
        bail!(
            "site check failed before deployment: {}",
            report.errors.join("; ")
        );
    }
    let available = Command::new("railway").arg("--version").output().context(
        "Railway CLI is not installed; install it from Railway's official documentation",
    )?;
    if !available.status.success() {
        bail!("Railway CLI is unavailable");
    }
    let linked = Command::new("railway")
        .arg("status")
        .current_dir(&root)
        .status()?;
    if !linked.success() {
        bail!("this directory is not linked to Railway; run `railway init` and review the project/service choices first");
    }
    let mut command = Command::new("railway");
    command.arg("up").current_dir(&root);
    if let Some(environment) = args.environment {
        command.args(["--environment", &environment]);
    }
    if args.detach {
        command.arg("--detach");
    }
    if args.yes {
        command.arg("--yes");
    }
    let status = command.status()?;
    if !status.success() {
        bail!("`railway up` failed with {status}");
    }
    println!("Railway upload complete\nVerify /readyz and attach a backed-up /app/data volume before relying on SQLite in production.");
    Ok(())
}

fn coolify(args: CoolifyArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    require_provider(&root, "coolify")?;
    let required = [
        "Dockerfile",
        ".dockerignore",
        "coolify.env.example",
        "COOLIFY.md",
    ];
    let missing: Vec<_> = required
        .into_iter()
        .filter(|file| !root.join(file).exists())
        .collect();
    let site = scaffold::site_check(Some(root.clone()))?;
    let ok = missing.is_empty() && site.errors.is_empty();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"provider":"coolify","ready":ok,"missing":missing,"siteErrors":site.errors,"health":"/healthz","readiness":"/readyz","sqliteVolume":"/app/data"})
            )?
        );
    } else {
        println!(
            "Coolify deployment {}",
            if ok { "ready" } else { "needs attention" }
        );
        for file in &missing {
            println!("  missing {file}");
        }
        println!("  health /healthz · readiness /readyz · persistent SQLite volume /app/data · one replica");
    }
    if args.check && !ok {
        bail!("Coolify deployment checks failed");
    }
    Ok(())
}

fn require_provider(root: &std::path::Path, expected: &str) -> Result<()> {
    let project = config::load_project_config(root)?;
    let actual = project
        .deploy
        .as_ref()
        .and_then(|deploy| deploy.get("provider"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("none");
    if actual != expected {
        bail!("deployment capability `{expected}` is not configured\nAdd it with: proa add deploy {expected}");
    }
    Ok(())
}

mod cdn;
