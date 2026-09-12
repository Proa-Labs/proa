// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Command-line entry point: `cargo proa` argument normalization, the rustc
//! wrapper dispatch used by linked RSJS builds, and parsing into
//! [`crate::commands::Command`].

use anyhow::Result;
use clap::Parser;

use crate::commands;
use crate::rsjs::build as rsjs_build;

#[derive(Debug, Parser)]
#[command(name = "proa")]
#[command(version)]
#[command(about = "Developer tooling for Proa templates")]
pub struct Cli {
    #[command(subcommand)]
    pub(crate) command: commands::Command,
}

/// Run the `proa` / `cargo proa` command-line interface.
pub fn run() -> Result<()> {
    if rsjs_build::rustc_wrapper_dispatch_requested() {
        let status = match rsjs_build::run_rustc_wrapper_from_env(std::env::args_os()) {
            Ok(status) => status,
            Err(error) => {
                if rsjs_build::emit_rustc_wrapper_diagnostic(&error).is_err() {
                    return Err(error.into());
                }
                std::process::exit(1);
            }
        };
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        return Ok(());
    }

    let args = normalize_cargo_subcommand_args(std::env::args_os());
    let cli = Cli::parse_from(args);
    commands::dispatch(cli.command)
}

fn normalize_cargo_subcommand_args<I>(args: I) -> Vec<std::ffi::OsString>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let mut args: Vec<_> = args.into_iter().collect();
    let invoked_as_cargo_proa = args
        .first()
        .and_then(|arg| std::path::Path::new(arg).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "cargo-proa");

    if invoked_as_cargo_proa
        && args
            .get(1)
            .and_then(|arg| arg.to_str())
            .is_some_and(|arg| arg == "proa")
    {
        args.remove(1);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_cargo_subcommand_name() {
        let args = normalize_cargo_subcommand_args([
            "cargo-proa".into(),
            "proa".into(),
            "fmt".into(),
            "--check".into(),
        ]);
        let rendered: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert_eq!(rendered, ["cargo-proa", "fmt", "--check"]);
    }
}
