// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa run`: compile and run one exact Proa binary through the linked RSJS
//! pipeline.

use anyhow::Result;
use clap::Args;

use super::linked::LinkedExecuteArgs;
use crate::lifecycle;

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    #[command(flatten)]
    cargo: LinkedExecuteArgs,
    /// Arguments forwarded unchanged to the selected binary after Cargo's `--`.
    #[arg(last = true)]
    arguments: Vec<String>,
}

pub(crate) fn run(args: RunArgs) -> Result<()> {
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
        arguments: args.arguments,
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
