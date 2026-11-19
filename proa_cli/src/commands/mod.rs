// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One module per `proa` subcommand. Each module owns its clap argument
//! types and a `run` entry point; this module owns the [`Command`] tree and
//! dispatches parsed arguments to the right subcommand.

mod add;
mod app_generate;
mod build;
mod check;
mod chunks;
mod completions;
mod config;
pub(crate) mod db;
mod deploy;
mod dev;
mod fmt;
mod font;
mod generate;
mod init;
mod linked;
mod lint;
mod new;
mod registry;
mod rsjs;
mod run;
pub(crate) mod script;
mod selection;
mod site;
mod start;
pub(crate) mod task;
mod test;
mod ui;
mod update;

use std::ffi::OsString;

use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Update the installed Proa CLI and Cargo subcommand together.
    Update(update::UpdateArgs),
    #[command(name = "install-self", hide = true)]
    InstallSelf(update::InstallSelfArgs),
    /// Initialize proa.config.rs and proa.lock.json in an existing project.
    Init(init::InitArgs),
    /// Inspect and validate proa.config.rs.
    Config(config::ConfigArgs),
    /// Add an application capability to an existing Proa project.
    Add(add::AddArgs),
    /// Create a new Proa project.
    New(new::NewArgs),
    /// Watch the project and restart its linked development server.
    Dev(dev::DevArgs),
    /// Deploy through transparent provider-native workflows.
    Deploy(deploy::DeployArgs),
    /// Run the built-in build for one linked binary and its assets.
    Build(build::BuildArgs),
    /// Create, migrate, inspect, reset, or remove the configured database.
    Db(db::DbArgs),
    /// Run a project script, or compile and run an application binary with no script name.
    Run(run::RunArgs),
    /// Start an already-built release without compiling or watching.
    Start(start::StartArgs),
    /// Run application tests with linked compilation and isolated database state.
    Test(test::TestArgs),
    /// Generate files inside an existing Proa project.
    Generate(generate::GenerateArgs),
    /// Inspect configured component registries.
    Registry(registry::RegistryArgs),
    /// Install source components from the Proa UI registries.
    Ui(ui::UiArgs),
    /// Inspect rsjs compiler analysis artifacts.
    Rsjs(rsjs::RsjsArgs),
    /// Inspect and validate a Proa site.
    Site(site::SiteArgs),
    /// Format HTML, XML, and raw Markdown template macro bodies.
    Fmt(fmt::FmtArgs),
    /// Run Proa template lints.
    Lint(lint::LintArgs),
    /// Run verified format/lint checks and, for a whole project, the linked RSJS Cargo check.
    Check(check::CheckArgs),
    /// Generate shell completions for the proa binary.
    Completions(completions::CompletionsArgs),
    /// Inspect template consolidation: static runs, slot boundaries, and
    /// suggested reserve hints for html!/html_sync! bodies.
    Chunks(chunks::ChunksArgs),
    /// Vendor and inspect self-hosted, metric-matched fonts declared in code (proa_fonts).
    Font(font::FontArgs),
}

pub(crate) fn dispatch(command: Command, arguments: &[OsString]) -> Result<()> {
    match command {
        Command::Update(args) => update::run(args),
        Command::InstallSelf(args) => update::install_self(args),
        Command::Init(args) => init::run(args),
        Command::Config(args) => config::run(args),
        Command::Add(args) => add::run(args),
        Command::New(args) => new::run(args),
        Command::Dev(args) => dev::run(args),
        Command::Deploy(args) => deploy::run(args),
        Command::Build(args) => build::run(args),
        Command::Db(args) => db::run(args),
        Command::Run(args) => run::run(args, arguments),
        Command::Start(args) => start::run(args),
        Command::Test(args) => test::run(args),
        Command::Generate(args) => generate::run(args),
        Command::Registry(args) => registry::run(args),
        Command::Ui(args) => ui::run(args),
        Command::Rsjs(args) => rsjs::run(args),
        Command::Site(args) => site::run(args),
        Command::Fmt(args) => fmt::run(args),
        Command::Lint(args) => lint::run(args),
        Command::Check(args) => check::run(args),
        Command::Completions(args) => completions::run(args),
        Command::Chunks(args) => chunks::run(args),
        Command::Font(args) => font::run(args),
    }
}
