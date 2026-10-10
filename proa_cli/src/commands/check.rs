// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa check`: run verified format/lint checks and, for a whole project,
//! the linked RSJS Cargo check.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;

use super::selection::{selected_paths, FileSelectionArgs};
use crate::fmt;
use crate::lifecycle;
use crate::lint;
use crate::output::{has_errors, has_warnings, print_diagnostics};
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    #[command(flatten)]
    files: FileSelectionArgs,
    /// Proa project root for the linked Cargo check. Defaults to the nearest root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Run the complete configured project validation pipeline.
    #[arg(long)]
    all: bool,
    /// Cargo profile override, e.g. release-fast.
    #[arg(long)]
    profile: Option<String>,
    /// Cargo package to check.
    #[arg(short = 'p', long)]
    package: Option<String>,
    /// Cargo binary to check.
    #[arg(long)]
    bin: Option<String>,
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
    /// Run only the source formatter and lints, without the linked Cargo check.
    #[arg(long)]
    source_only: bool,
    /// Report lint warnings without failing the check (errors and
    /// formatting drift still fail).
    #[arg(long)]
    allow_warnings: bool,
    /// Include Rust test files and #[cfg(test)] modules.
    #[arg(long)]
    include_tests: bool,
    /// Use compiler-emitted target/rsjs/strategy sidecars for rsjs strategy lints.
    #[arg(long)]
    use_rsjs_strategy_cache: bool,
    /// Error when a matching rsjs strategy sidecar is missing or ambiguous.
    #[arg(long, requires = "use_rsjs_strategy_cache")]
    require_fresh_strategy_cache: bool,
}

pub(crate) fn run(args: CheckArgs) -> Result<()> {
    let uses_file_selection = !args.files.paths.is_empty() || args.files.uses_git_selection();
    if args.all && uses_file_selection {
        bail!("--all cannot be combined with file selection flags");
    }
    let run_linked_check = !args.source_only && !uses_file_selection;
    if !run_linked_check
        && (args.profile.is_some()
            || args.package.is_some()
            || args.bin.is_some()
            || args.target.is_some()
            || !args.features.is_empty()
            || args.all_features
            || args.no_default_features
            || args.locked
            || args.offline)
    {
        bail!(
            "Cargo target/profile/feature flags require a whole-project linked check; remove the file selection or --source-only"
        );
    }

    let project_root = if run_linked_check || args.root.is_some() {
        Some(resolve_config_root(args.root.clone())?)
    } else {
        None
    };
    let paths = if uses_file_selection {
        selected_paths(&args.files)?
    } else if let Some(root) = &project_root {
        let src = root.join("src");
        if src.exists() {
            vec![src]
        } else {
            Vec::new()
        }
    } else {
        selected_paths(&args.files)?
    };
    let rsjs_strategy_cache = if args.use_rsjs_strategy_cache {
        let root = match &project_root {
            Some(root) => root.clone(),
            None => resolve_config_root(args.root.clone())?,
        };
        Some(lint::RsjsStrategyCacheOptions {
            root,
            require_fresh: args.require_fresh_strategy_cache,
        })
    } else {
        None
    };
    let format_summary = if args.files.uses_git_selection() && paths.is_empty() {
        fmt::FormatSummary::default()
    } else {
        fmt::format_paths_with_options(&paths, true, true)?
    };
    let diagnostics = if args.files.uses_git_selection() && paths.is_empty() {
        Vec::new()
    } else {
        lint::lint_paths(
            &paths,
            lint::LintOptions {
                include_tests: args.include_tests,
                rsjs_strategy_cache,
                content: lint::content::ContentOptions {
                    root: args.root.clone(),
                    ..Default::default()
                },
            },
        )?
    };

    print_diagnostics(&format_summary.diagnostics, false, false)?;
    print_diagnostics(&diagnostics, false, false)?;

    if !format_summary.changed.is_empty() {
        for path in &format_summary.changed {
            eprintln!("{}: Proa macro formatting differs", path.display());
        }
    }

    if !format_summary.changed.is_empty()
        || has_errors(&format_summary.diagnostics)
        || has_errors(&diagnostics)
        || (!args.allow_warnings && has_warnings(&diagnostics))
    {
        bail!("Proa check failed");
    }

    if run_linked_check {
        if args.all {
            let root = project_root
                .as_ref()
                .expect("whole-project checks resolve a project root");
            let site = crate::scaffold::site_check(Some(root.clone()))?;
            if !site.errors.is_empty() {
                for error in site.errors {
                    eprintln!("error: {error}");
                }
                bail!("Proa site check failed");
            }
            lifecycle::build_assets_for_project(root)?;
        }
        lifecycle::check(lifecycle::CheckOptions {
            root: project_root.clone(),
            profile: args.profile.clone(),
            package: args.package.clone(),
            bin: args.bin.clone(),
            target: args.target.clone(),
            features: args.features.clone(),
            all_features: args.all_features,
            no_default_features: args.no_default_features,
            locked: args.locked,
            offline: args.offline,
        })?;
        if args.all {
            lifecycle::test(lifecycle::TestOptions {
                root: project_root,
                profile: args.profile,
                package: args.package,
                bin: args.bin,
                test: None,
                target: args.target,
                features: args.features,
                all_features: args.all_features,
                no_default_features: args.no_default_features,
                locked: args.locked,
                offline: args.offline,
                watch: false,
                keep_database: false,
                arguments: Vec::new(),
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::cli::Cli;
    use crate::commands::Command;

    #[test]
    fn check_accepts_rsjs_strategy_cache_flags() {
        let cli = Cli::try_parse_from([
            "proa",
            "check",
            "src/page.rs",
            "--use-rsjs-strategy-cache",
            "--require-fresh-strategy-cache",
        ])
        .unwrap();

        let Command::Check(args) = cli.command else {
            panic!("expected check command");
        };
        assert_eq!(args.files.paths, [PathBuf::from("src/page.rs")]);
        assert!(args.use_rsjs_strategy_cache);
        assert!(args.require_fresh_strategy_cache);
    }

    #[test]
    fn project_check_accepts_exact_cargo_selection() {
        let cli = Cli::try_parse_from([
            "proa",
            "check",
            "--root",
            "/tmp/demo",
            "-p",
            "demo",
            "--bin",
            "server",
            "--target",
            "aarch64-apple-darwin",
            "--features",
            "hydration,metrics",
            "--no-default-features",
            "--locked",
            "--offline",
        ])
        .unwrap();

        let Command::Check(args) = cli.command else {
            panic!("expected check command");
        };
        assert_eq!(args.root, Some(PathBuf::from("/tmp/demo")));
        assert_eq!(args.package.as_deref(), Some("demo"));
        assert_eq!(args.bin.as_deref(), Some("server"));
        assert_eq!(args.target.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(args.features, ["hydration", "metrics"]);
        assert!(args.no_default_features);
        assert!(args.locked);
        assert!(args.offline);
        assert!(!args.source_only);
    }
}
