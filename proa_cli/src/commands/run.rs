// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa run <name>` runs a project script. Bare `proa run` runs a linked binary.

use std::ffi::OsString;

use anyhow::{ensure, Context, Result};
use clap::Args;

use super::{linked::LinkedExecuteArgs, script, task};
use crate::{config, lifecycle, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    #[command(flatten)]
    cargo: LinkedExecuteArgs,
    /// List project scripts and their lifecycle use.
    #[arg(long)]
    list: bool,
    /// Run a named task even if its inputs and outputs are up to date.
    #[arg(long)]
    force: bool,
    /// Script name and its arguments, or just binary arguments after `--`.
    #[arg(value_name = "SCRIPT", trailing_var_arg = true)]
    arguments: Vec<OsString>,
}

pub(crate) fn run(args: RunArgs, raw: &[OsString]) -> Result<()> {
    // Clap discards the separator when it precedes the first positional.
    // Preserve the distinction between `run dev` and binary `run -- dev`.
    let before = raw.len().saturating_sub(args.arguments.len());
    let binary_arguments = before > 0 && raw[before - 1] == "--";
    let script = args.arguments.split_first().filter(|_| !binary_arguments);
    if args.list || script.is_some() {
        ensure!(
            !args.cargo.selects_binary(),
            "binary selection flags belong to bare `proa run`; put script flags after the script name"
        );
        let root = resolve_config_root(args.cargo.root)?;
        let project = config::load_project_config(&root)?;
        if args.list {
            ensure!(
                script.is_none() && !args.force && args.arguments.is_empty(),
                "use `proa run --list` without a script or arguments"
            );
            task::list(&project);
            return Ok(());
        }
        let (name, arguments) = script.expect("checked script name");
        let name = name.to_str().context("script name must be UTF-8")?;
        if let Some((_, source)) =
            script::shell_entries(&project.scripts).find(|(script, _)| *script == name)
        {
            ensure!(
                !args.force,
                "`--force` applies to named tasks with cached outputs"
            );
            return script::run(&root, name, source, arguments);
        }
        let arguments = arguments
            .strip_prefix(&[OsString::from("--")])
            .unwrap_or(arguments);
        return task::run_named(&root, &project, name, args.force, arguments);
    }
    ensure!(!args.force, "`--force` requires a named task");
    lifecycle::run(lifecycle::RunOptions {
        root: args.cargo.root,
        profile: args.cargo.profile,
        package: args.cargo.package,
        bin: args.cargo.bin,
        target: args.cargo.target,
        features: args.cargo.features,
        all_features: args.cargo.all_features,
        no_default_features: args.cargo.no_default_features,
        locked: args.cargo.locked,
        offline: args.cargo.offline,
        arguments: args
            .arguments
            .into_iter()
            .map(|arg| {
                arg.into_string().map_err(|arg| {
                    anyhow::anyhow!("binary argument `{}` must be UTF-8", arg.to_string_lossy())
                })
            })
            .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::cli::Cli;
    use crate::commands::Command;

    #[test]
    fn linked_run_accepts_exact_cargo_selection_and_runtime_arguments() {
        let cli = Cli::try_parse_from([
            "proa",
            "run",
            "--root",
            "/tmp/demo",
            "-p",
            "demo",
            "--bin",
            "server",
            "--profile",
            "release-fast",
            "--target",
            "aarch64-apple-darwin",
            "--features",
            "hydration,metrics",
            "--locked",
            "--offline",
            "--",
            "--seed",
            "42",
        ])
        .unwrap();

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.cargo.root, Some(PathBuf::from("/tmp/demo")));
        assert_eq!(args.cargo.package.as_deref(), Some("demo"));
        assert_eq!(args.cargo.bin.as_deref(), Some("server"));
        assert_eq!(args.cargo.profile.as_deref(), Some("release-fast"));
        assert_eq!(args.cargo.target.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(args.cargo.features, ["hydration", "metrics"]);
        assert!(args.cargo.locked);
        assert!(args.cargo.offline);
        assert_eq!(args.arguments, ["--seed", "42"]);
    }
}
