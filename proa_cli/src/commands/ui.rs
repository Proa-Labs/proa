// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa ui`: install source components from the Proa UI registries.

use std::{fs, path::PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand};
use sha2::{Digest, Sha256};

use crate::ui;
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct UiArgs {
    #[command(subcommand)]
    command: UiCommand,
}

#[derive(Debug, Subcommand)]
enum UiCommand {
    /// Search the bundled starter catalog.
    Search(UiSearchArgs),
    /// Resolve and inspect components without writing them.
    View(UiViewArgs),
    /// Resolve, verify, and transactionally install a UI component or block.
    Add(UiAddArgs),
    /// Show installed components and local modification state.
    Installed(UiInstalledArgs),
    /// Show source-owned component drift.
    Diff(UiDiffArgs),
}

#[derive(Debug, Args)]
struct UiSearchArgs {
    query: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UiViewArgs {
    #[arg(required = true)]
    components: Vec<String>,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UiAddArgs {
    /// Component or block slug, or a qualified name such as @proa/button.
    components: Vec<String>,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Registry alias: @proa or @proa-pro.
    #[arg(long)]
    registry: Option<String>,
    /// Resolve and verify the complete transaction without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Reinstall an item that is already recorded in proa.lock.json,
    /// overwriting its files. Use after deleting installed files or changing
    /// where `@components`, `@assets`, and `paths` place them.
    #[arg(long)]
    force: bool,
    /// Emit a machine-readable install report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UiInstalledArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UiDiffArgs {
    component: Option<String>,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: UiArgs) -> Result<()> {
    match args.command {
        UiCommand::Search(args) => search(args),
        UiCommand::View(args) => {
            let root = resolve_config_root(args.root)?;
            for component in args.components {
                let report = ui::add(ui::AddOptions {
                    root: root.clone(),
                    component,
                    registry: args.registry.clone(),
                    dry_run: true,
                    force: false,
                })?;
                ui::print_add_report(&report, args.json)?;
            }
            Ok(())
        }
        UiCommand::Add(args) => {
            let root = resolve_config_root(args.root)?;
            for component in args.components {
                let report = ui::add(ui::AddOptions {
                    root: root.clone(),
                    component,
                    registry: args.registry.clone(),
                    dry_run: args.dry_run,
                    force: args.force,
                })?;
                ui::print_add_report(&report, args.json)?;
            }
            Ok(())
        }
        UiCommand::Installed(args) => installed(args.root, args.json, None),
        UiCommand::Diff(args) => installed(args.root, args.json, args.component.as_deref()),
    }
}

fn search(args: UiSearchArgs) -> Result<()> {
    let catalog = [
        ("badge", "Compact status and category label"),
        ("button", "Button and link actions"),
        ("card", "Generic content card"),
    ];
    let query = args.query.unwrap_or_default().to_ascii_lowercase();
    let matches: Vec<_> = catalog
        .into_iter()
        .filter(|(name, description)| {
            query.is_empty()
                || name.contains(&query)
                || description.to_ascii_lowercase().contains(&query)
        })
        .collect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&matches)?);
    } else {
        for (name, description) in matches {
            println!("{name:<18} {description}");
        }
    }
    Ok(())
}

fn installed(root: Option<PathBuf>, json: bool, filter: Option<&str>) -> Result<()> {
    let root = resolve_config_root(root)?;
    let source = fs::read_to_string(root.join(crate::config::LOCK_FILE))?;
    let value: serde_json::Value = serde_json::from_str(&source)?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut reports = Vec::new();
    for (name, component) in components {
        if filter
            .is_some_and(|filter| name != filter && name.trim_start_matches("@proa/") != filter)
        {
            continue;
        }
        let mut state = "current";
        let mut changed = Vec::new();
        if let Some(files) = component
            .get("files")
            .and_then(serde_json::Value::as_object)
        {
            for (relative, expected) in files {
                let file = root.join(relative);
                let actual = fs::read(&file)
                    .ok()
                    .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
                if actual.as_deref() != expected.as_str() {
                    state = if file.exists() { "modified" } else { "missing" };
                    changed.push(relative.clone());
                }
            }
        } else if let Some(files) = component
            .get("installedFiles")
            .and_then(serde_json::Value::as_array)
        {
            for relative in files.iter().filter_map(serde_json::Value::as_str) {
                if !root.join(relative).exists() {
                    state = "missing";
                    changed.push(relative.to_string());
                }
            }
        }
        reports.push(serde_json::json!({
            "component": name,
            "version": component.get("version").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
            "state": state,
            "changedFiles": changed
        }));
    }
    if filter.is_some() && reports.is_empty() {
        anyhow::bail!("component is not installed");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for report in reports {
            println!(
                "{:<28} {:<10} {}",
                report["component"].as_str().unwrap_or("unknown"),
                report["state"].as_str().unwrap_or("unknown"),
                report["version"].as_str().unwrap_or("unknown")
            );
            for file in report["changedFiles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
            {
                println!("  {file}");
            }
        }
    }
    Ok(())
}
