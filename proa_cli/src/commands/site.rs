// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa site`: inspect and validate a Proa site.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};

use crate::scaffold;

#[derive(Debug, Args)]
pub(crate) struct SiteArgs {
    #[command(subcommand)]
    command: SiteCommand,
}

#[derive(Debug, Subcommand)]
enum SiteCommand {
    /// Validate the opinionated Proa site structure.
    Check(SiteCheckArgs),
    /// Print discovered page routes.
    Routes(SiteRoutesArgs),
    /// Print a compact application capability dashboard.
    Status(SiteCheckArgs),
    /// Explain one owned route in Proa terms.
    Inspect(SiteInspectArgs),
    /// Print site diagnostics.
    Doctor(SiteCheckArgs),
}

#[derive(Debug, Args)]
struct SiteCheckArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SiteRoutesArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON route data.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SiteInspectArgs {
    /// Canonical route path to inspect.
    path: String,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON route data.
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: SiteArgs) -> Result<()> {
    match args.command {
        SiteCommand::Check(args) | SiteCommand::Doctor(args) => {
            let report = scaffold::site_check(args.root)?;
            scaffold::print_site_check(&report, args.json)?;
            if !report.errors.is_empty() {
                bail!("Proa site check failed");
            }
            Ok(())
        }
        SiteCommand::Routes(args) => {
            let routes = scaffold::site_routes(args.root)?;
            scaffold::print_routes(&routes, args.json)
        }
        SiteCommand::Status(args) => print_status(args.root, args.json),
        SiteCommand::Inspect(args) => print_inspect(args),
    }
}

fn print_status(root: Option<PathBuf>, json: bool) -> Result<()> {
    let root = scaffold::resolve_root(root)?;
    let config = crate::config::load_project_config(&root)?;
    let lock: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        root.join(crate::config::LOCK_FILE),
    )?)?;
    let application = config.application.as_ref();
    let database_config = config.database.as_ref();
    let package = application
        .map(|app| app.package.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Proa site");
    let template = application
        .map(|app| app.template.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("custom");
    let starter = application
        .map(|app| app.starter.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("custom");
    let database = database_config
        .map(|db| db.driver.as_str())
        .unwrap_or("none");
    let database_integration = database_config
        .map(|db| db.integration.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("integrated");
    let styling = config
        .styling
        .as_ref()
        .map(|style| style.kind.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("css");
    let markdown = config
        .representations
        .as_ref()
        .and_then(|formats| formats.markdown.as_ref())
        .is_some_and(|format| format.enabled);
    let components = lock
        .get("components")
        .and_then(serde_json::Value::as_object)
        .map_or(0, serde_json::Map::len);
    let value = serde_json::json!({
        "package": package,
        "template": template,
        "starter": starter,
        "database": database,
        "databaseIntegration": database_integration,
        "markdown": markdown,
        "components": components,
        "styling": styling
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{package}");
        println!("  Template      {template} · {starter}");
        println!(
            "  Routes        {}",
            if markdown {
                "1 page · 2 endpoints · 1 Markdown alias"
            } else {
                "1 page · 2 endpoints"
            }
        );
        println!(
            "  Representations {}",
            if markdown {
                "HTML + Markdown · Accept negotiation enabled"
            } else {
                "HTML"
            }
        );
        println!("  Database      {database}");
        println!(
            "  Data          {}",
            if database == "sqlite" && database_integration == "manual" {
                "manual application-state wiring pending"
            } else if database == "sqlite" {
                "AppLoader · request-local dedupe enabled"
            } else {
                "none"
            }
        );
        println!("  UI            {components} vendored components");
        println!("  Styling       {styling}");
    }
    Ok(())
}

fn print_inspect(args: SiteInspectArgs) -> Result<()> {
    let root = scaffold::resolve_root(args.root)?;
    let routes = scaffold::site_routes(Some(root.clone()))?;
    let route = routes
        .iter()
        .find(|route| route.path == args.path)
        .ok_or_else(|| {
            anyhow::anyhow!("route `{}` is not in the owned route registry", args.path)
        })?;
    let home = std::fs::read_to_string(root.join("src/pages/home.rs")).unwrap_or_default();
    let consumers = ["DatabaseBadge", "DatabaseCard"]
        .into_iter()
        .filter(|name| home.contains(name))
        .collect::<Vec<_>>();
    let markdown = home.contains("MdRenderSync");
    let value = serde_json::json!({
        "method": route.method,
        "path": route.path,
        "source": route.file,
        "kind": route.kind,
        "representations": route.representations,
        "markdownTrait": markdown.then_some("MdRenderSync"),
        "loaderKey": (!consumers.is_empty()).then_some("system:database-status"),
        "consumers": consumers,
        "uniqueStaticKeys": usize::from(!consumers.is_empty()),
        "clientJavaScript": "none"
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("Route       {} {}", route.method, route.path);
        println!("Source      {}", route.file.display());
        println!("Kind        {}", route.kind);
        println!("Represent.  {}", route.representations);
        if markdown {
            println!("Markdown    /index.md · MdRenderSync · no HTML shell");
            println!("Discovery   Link rel=alternate · Vary: Accept");
        }
        if !consumers.is_empty() {
            println!("Data        system:database-status");
            println!("Consumers   {}", consumers.join(" · "));
            println!(
                "Dedupe      {} consumers · 1 unique static key",
                consumers.len()
            );
        }
        println!("Client JS   none");
    }
    Ok(())
}
