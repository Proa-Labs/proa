// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa build`: build assets and compile one exact Proa binary through the
//! linked RSJS pipeline.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::lifecycle;

#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    /// Proa project root. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Cargo profile override, e.g. release-fast.
    #[arg(long)]
    profile: Option<String>,
    /// Cargo package to build.
    #[arg(short = 'p', long)]
    package: Option<String>,
    /// Cargo binary to build.
    #[arg(long)]
    bin: Option<String>,
    /// Exact Cargo example target to build.
    #[arg(long)]
    example: Option<String>,
    /// Cargo target triple for both descriptor and final compilation.
    #[arg(long)]
    target: Option<String>,
    /// Cargo features enabled for both descriptor and final compilation.
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,
    /// Enable every Cargo feature.
    #[arg(long)]
    all_features: bool,
    /// Disable Cargo default features.
    #[arg(long)]
    no_default_features: bool,
    /// Require Cargo.lock to remain unchanged.
    #[arg(long)]
    locked: bool,
    /// Disable Cargo network access.
    #[arg(long)]
    offline: bool,
    /// Skip Proa format/lint/site checks.
    #[arg(long)]
    no_check: bool,
    /// Skip configured asset builds.
    #[arg(long)]
    no_assets: bool,
    /// Copy the attested final executable to this path after a successful linked build.
    /// Relative paths are resolved from the Proa project root. For warm rebuilds, prefer
    /// the Cargo target directory or a path outside the selected package source tree.
    #[arg(long, value_name = "PATH")]
    artifact_out: Option<PathBuf>,
    /// Override the configured Cargo build command. Custom non-Cargo commands are not supported.
    /// Example: -- cargo build --release -p app --bin app.
    #[arg(last = true)]
    command: Vec<String>,
}

pub(crate) fn run(args: BuildArgs) -> Result<()> {
    lifecycle::build(lifecycle::BuildOptions {
        root: args.root,
        profile: args.profile,
        package: args.package,
        bin: args.bin,
        example: args.example,
        target: args.target,
        features: args.features,
        all_features: args.all_features,
        no_default_features: args.no_default_features,
        locked: args.locked,
        offline: args.offline,
        no_check: args.no_check,
        no_assets: args.no_assets,
        artifact_out: args.artifact_out,
        command: args.command,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::cli::Cli;
    use crate::commands::Command;

    #[test]
    fn build_accepts_an_explicit_artifact_output() {
        let cli = Cli::try_parse_from([
            "proa",
            "build",
            "--root",
            "/tmp/demo",
            "--artifact-out",
            "dist/server",
            "--package",
            "demo",
            "--bin",
            "server",
        ])
        .unwrap();

        let Command::Build(args) = cli.command else {
            panic!("expected build command");
        };
        assert_eq!(args.root, Some(PathBuf::from("/tmp/demo")));
        assert_eq!(args.artifact_out, Some(PathBuf::from("dist/server")));
        assert_eq!(args.package.as_deref(), Some("demo"));
        assert_eq!(args.bin.as_deref(), Some("server"));
    }
}
