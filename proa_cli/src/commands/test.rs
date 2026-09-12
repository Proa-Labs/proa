// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa test`: execute application tests through the linked RSJS pipeline with
//! an isolated database lifecycle.

use anyhow::Result;
use clap::Args;

use super::linked::LinkedExecuteArgs;
use crate::lifecycle;

#[derive(Debug, Args)]
pub(crate) struct TestArgs {
    #[command(flatten)]
    cargo: LinkedExecuteArgs,
    /// Run one exact integration-test target instead of the application suite.
    #[arg(long)]
    test: Option<String>,
    /// Re-run tests when project or migration inputs change.
    #[arg(long)]
    watch: bool,
    /// Preserve the isolated SQLite database when a test run fails.
    #[arg(long)]
    keep_database: bool,
    /// Arguments forwarded unchanged to the selected test harness after Cargo's `--`.
    #[arg(last = true)]
    arguments: Vec<String>,
}

pub(crate) fn run(args: TestArgs) -> Result<()> {
    lifecycle::test(lifecycle::TestOptions {
        root: args.cargo.root,
        profile: args.cargo.profile,
        package: args.cargo.package,
        bin: args.cargo.bin,
        test: args.test,
        target: args.cargo.target,
        features: args.cargo.features,
        all_features: args.cargo.all_features,
        no_default_features: args.cargo.no_default_features,
        locked: args.cargo.locked,
        offline: args.cargo.offline,
        watch: args.watch,
        keep_database: args.keep_database,
        arguments: args.arguments,
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;
    use crate::commands::Command;

    #[test]
    fn linked_test_accepts_one_exact_test_target_and_harness_arguments() {
        let cli = Cli::try_parse_from([
            "proa",
            "test",
            "-p",
            "demo",
            "--test",
            "http",
            "--no-default-features",
            "--",
            "--nocapture",
            "linked_plan",
        ])
        .unwrap();

        let Command::Test(args) = cli.command else {
            panic!("expected test command");
        };
        assert_eq!(args.cargo.package.as_deref(), Some("demo"));
        assert_eq!(args.cargo.bin, None);
        assert_eq!(args.test.as_deref(), Some("http"));
        assert!(args.cargo.no_default_features);
        assert!(!args.watch);
        assert!(!args.keep_database);
        assert_eq!(args.arguments, ["--nocapture", "linked_plan"]);
    }

    #[test]
    fn test_accepts_database_lifecycle_flags() {
        let cli = Cli::try_parse_from([
            "proa",
            "test",
            "--bin",
            "demo",
            "--watch",
            "--keep-database",
        ])
        .unwrap();

        let Command::Test(args) = cli.command else {
            panic!("expected test command");
        };
        assert!(args.watch);
        assert!(args.keep_database);
    }
}
