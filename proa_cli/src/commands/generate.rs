// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa generate`: generate files inside an existing Proa project.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::scaffold;

#[derive(Debug, Args)]
pub(crate) struct GenerateArgs {
    #[command(subcommand)]
    command: GenerateCommand,
}

#[derive(Debug, Subcommand)]
enum GenerateCommand {
    /// Generate an ordered SQLx migration pair.
    Migration(GenerateMigrationArgs),
    /// Generate and register a typed GET page.
    Page(GeneratePageArgs),
    /// Generate and register a non-page Axum endpoint.
    Endpoint(GenerateEndpointArgs),
    /// Generate an ordinary SQLx model plus migration.
    Model(GenerateModelArgs),
    /// Generate a reusable render component under src/components.
    Component(GenerateComponentArgs),
    /// Generate a shared page layout under src/layouts.
    Layout(GenerateLayoutArgs),
    /// Generate a typed data module under src/static_data.
    Data(GenerateDataArgs),
}

#[derive(Debug, Args)]
struct GeneratePageArgs {
    /// Canonical route path, such as about or blog/:slug.
    path: String,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    layout: Option<String>,
    #[arg(long, conflicts_with = "no_markdown")]
    markdown: bool,
    #[arg(long)]
    no_markdown: bool,
    #[arg(long)]
    no_test: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct GenerateEndpointArgs {
    path: String,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long, value_enum)]
    method: EndpointMethod,
    #[arg(long, value_enum)]
    response: EndpointResponse,
    #[arg(long)]
    no_test: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum EndpointMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum EndpointResponse {
    Json,
    Redirect,
    Empty,
    Html,
}

#[derive(Debug, Args)]
struct GenerateModelArgs {
    name: String,
    #[arg(required = true)]
    fields: Vec<String>,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct GenerateMigrationArgs {
    /// Lowercase snake-case migration name.
    name: String,
    #[arg(long)]
    root: Option<PathBuf>,
    /// Create one irreversible .sql file instead of an up/down pair.
    #[arg(long)]
    simple: bool,
    /// Open the generated files with VISUAL or EDITOR.
    #[arg(long)]
    open: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct GenerateComponentArgs {
    /// Component name, optionally with folders, e.g. pricing/feature-row.
    name: String,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Use components/<name>/mod.rs instead of one file.
    #[arg(long)]
    folder: bool,
    /// Generate a recipe.rs file next to the component.
    #[arg(long)]
    recipe: bool,
    /// Include generic children support.
    #[arg(long)]
    children: bool,
    /// Generate the async-capable WebRender trait instead of WebRenderSync.
    #[arg(long = "async")]
    asynchronous: bool,
    /// Add a semantic MdRenderSync implementation.
    #[arg(long)]
    markdown: bool,
    /// Do not generate a render test.
    #[arg(long = "no-test")]
    no_test: bool,
    /// Print planned changes without writing.
    #[arg(long, value_name = "FORMAT", num_args = 0..=1, default_missing_value = "text")]
    dry_run: Option<scaffold::DryRunFormat>,
    /// Emit the dry-run plan as JSON.
    #[arg(long)]
    json: bool,
    /// Overwrite generated files if they already exist.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct GenerateLayoutArgs {
    /// Layout name.
    name: String,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Generate a full document shell.
    #[arg(long)]
    document: bool,
    /// Generate a section-level wrapper.
    #[arg(long)]
    section: bool,
    /// Move the generated document-root marker from the current root layout.
    #[arg(long)]
    replace_root: bool,
    /// Print planned changes without writing.
    #[arg(long, value_name = "FORMAT", num_args = 0..=1, default_missing_value = "text")]
    dry_run: Option<scaffold::DryRunFormat>,
    /// Emit the dry-run plan as JSON.
    #[arg(long)]
    json: bool,
    /// Overwrite generated files if they already exist.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct GenerateDataArgs {
    /// Data module name.
    name: String,
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Print planned changes without writing.
    #[arg(long, value_name = "FORMAT", num_args = 0..=1, default_missing_value = "text")]
    dry_run: Option<scaffold::DryRunFormat>,
    /// Emit the dry-run plan as JSON.
    #[arg(long)]
    json: bool,
    /// Overwrite generated files if they already exist.
    #[arg(long)]
    force: bool,
}

pub(crate) fn run(args: GenerateArgs) -> Result<()> {
    match args.command {
        GenerateCommand::Migration(args) => super::app_generate::migration(
            args.name,
            args.root,
            args.simple,
            args.open,
            args.dry_run,
            args.json,
        ),
        GenerateCommand::Page(args) => super::app_generate::page(
            args.path,
            args.root,
            args.layout,
            args.markdown,
            args.no_markdown,
            args.no_test,
            args.dry_run,
            args.json,
        ),
        GenerateCommand::Endpoint(args) => super::app_generate::endpoint(
            args.path,
            args.root,
            match args.method {
                EndpointMethod::Get => "get",
                EndpointMethod::Post => "post",
                EndpointMethod::Put => "put",
                EndpointMethod::Patch => "patch",
                EndpointMethod::Delete => "delete",
            },
            match args.response {
                EndpointResponse::Json => "json",
                EndpointResponse::Redirect => "redirect",
                EndpointResponse::Empty => "empty",
                EndpointResponse::Html => "html",
            },
            args.no_test,
            args.dry_run,
            args.json,
        ),
        GenerateCommand::Model(args) => {
            super::app_generate::model(args.name, args.fields, args.root, args.dry_run, args.json)
        }
        GenerateCommand::Component(args) => {
            scaffold::generate_component(scaffold::GenerateComponentOptions {
                name: args.name,
                root: args.root,
                folder: args.folder,
                recipe: args.recipe,
                children: args.children,
                asynchronous: args.asynchronous,
                markdown: args.markdown,
                no_test: args.no_test,
                dry_run: json_dry_run(args.dry_run, args.json),
                force: args.force,
            })
        }
        GenerateCommand::Layout(args) => {
            scaffold::generate_layout(scaffold::GenerateLayoutOptions {
                name: args.name,
                root: args.root,
                document: args.document,
                section: args.section,
                replace_root: args.replace_root,
                dry_run: json_dry_run(args.dry_run, args.json),
                force: args.force,
            })
        }
        GenerateCommand::Data(args) => scaffold::generate_data(scaffold::GenerateDataOptions {
            name: args.name,
            root: args.root,
            dry_run: json_dry_run(args.dry_run, args.json),
            force: args.force,
        }),
    }
}

fn json_dry_run(
    dry_run: Option<scaffold::DryRunFormat>,
    json: bool,
) -> Option<scaffold::DryRunFormat> {
    dry_run.map(|format| {
        if json {
            scaffold::DryRunFormat::Json
        } else {
            format
        }
    })
}
