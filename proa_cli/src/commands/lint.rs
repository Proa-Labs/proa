// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa lint`: run Proa template lints.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;

use super::selection::{selected_paths, FileSelectionArgs};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lint;
use crate::output::{has_errors, has_warnings, print_diagnostics};
use crate::util::resolve_config_root;

#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    files: FileSelectionArgs,
    /// Project root for content counterpart discovery and RSJS strategy sidecars.
    /// Defaults to the nearest Cargo workspace/package or proa.config.json.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Exit non-zero when warnings are found.
    #[arg(long)]
    deny_warnings: bool,
    /// Exit non-zero when warnings or a diagnostic code is found.
    #[arg(long, value_name = "LEVEL_OR_CODE")]
    deny: Vec<String>,
    /// Emit stable JSON diagnostics for editors and agents.
    #[arg(long)]
    json: bool,
    /// Minimum severity to print: error, warning, or info. Text output
    /// defaults to `warning`; --json/--github default to `info` (everything).
    #[arg(long, value_name = "LEVEL")]
    min_severity: Option<String>,
    /// Emit GitHub Actions annotations.
    #[arg(long)]
    github: bool,
    /// Include Rust test files and #[cfg(test)] modules.
    #[arg(long)]
    include_tests: bool,
    /// Show paired, unpaired, and whole-view-exempt content status (info).
    #[arg(long)]
    content_summary: bool,
    /// Use compiler-emitted target/rsjs/strategy sidecars for rsjs strategy lints.
    #[arg(long)]
    use_rsjs_strategy_cache: bool,
    /// Error when a matching rsjs strategy sidecar is missing or ambiguous.
    #[arg(long, requires = "use_rsjs_strategy_cache")]
    require_fresh_strategy_cache: bool,
}

pub(crate) fn run(args: LintArgs) -> Result<()> {
    let paths = selected_paths(&args.files)?;
    let rsjs_strategy_cache = if args.use_rsjs_strategy_cache {
        Some(lint::RsjsStrategyCacheOptions {
            root: resolve_config_root(args.root.clone())?,
            require_fresh: args.require_fresh_strategy_cache,
        })
    } else {
        None
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
                    report_status: args.content_summary,
                    ..Default::default()
                },
            },
        )?
    };

    let min_severity = resolve_min_severity(&args)?;
    let shown: Vec<_> = diagnostics
        .iter()
        .filter(|diag| {
            diag.severity.rank() >= min_severity.rank()
                || args.deny.iter().any(|code| code == diag.code)
        })
        .cloned()
        .collect();
    print_diagnostics(&shown, args.json, args.github)?;
    let hidden = diagnostics.len() - shown.len();
    if hidden > 0 && !args.json && !args.github {
        eprintln!(
            "({hidden} lower-severity diagnostics hidden; pass `--min-severity info` to show them)"
        );
    }

    // Exit status considers everything found, not just what was printed.
    if has_errors(&diagnostics) || has_denied_lint(&args, &diagnostics) {
        bail!("Proa lint failed");
    }

    Ok(())
}

fn resolve_min_severity(args: &LintArgs) -> Result<Severity> {
    match args.min_severity.as_deref() {
        Some("error" | "errors") => Ok(Severity::Error),
        Some("warning" | "warnings") => Ok(Severity::Warning),
        Some("info" | "all") => Ok(Severity::Info),
        Some(other) => {
            bail!("unknown --min-severity level `{other}` (use error, warning, or info)")
        }
        // Machine outputs default to everything; humans default to warning+.
        None if args.json || args.github => Ok(Severity::Info),
        None => Ok(Severity::Warning),
    }
}

fn deny_warnings(args: &LintArgs) -> bool {
    args.deny_warnings
        || args
            .deny
            .iter()
            .any(|level| matches!(level.as_str(), "warning" | "warnings"))
}

fn has_denied_lint(args: &LintArgs, diagnostics: &[Diagnostic]) -> bool {
    if deny_warnings(args) && has_warnings(diagnostics) {
        return true;
    }

    diagnostics.iter().any(|diag| {
        args.deny
            .iter()
            .any(|denied| denied != "warning" && denied != "warnings" && denied == diag.code)
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;

    use super::*;
    use crate::cli::Cli;
    use crate::commands::Command;

    #[test]
    fn lint_accepts_an_explicit_strategy_cache_root() {
        let cli = Cli::try_parse_from([
            "proa",
            "lint",
            "src/page.rs",
            "--root",
            "/tmp/demo",
            "--use-rsjs-strategy-cache",
        ])
        .unwrap();

        let Command::Lint(args) = cli.command else {
            panic!("expected lint command");
        };
        assert_eq!(args.files.paths, [PathBuf::from("src/page.rs")]);
        assert_eq!(args.root, Some(PathBuf::from("/tmp/demo")));
        assert!(args.use_rsjs_strategy_cache);
    }

    #[test]
    fn lint_deny_accepts_specific_diagnostic_codes() {
        let args = LintArgs {
            files: FileSelectionArgs {
                paths: Vec::new(),
                changed: false,
                staged: false,
                base: None,
            },
            root: None,
            deny_warnings: false,
            deny: vec!["rsjs/client-crossing".to_string()],
            json: false,
            min_severity: None,
            github: false,
            include_tests: false,
            content_summary: false,
            use_rsjs_strategy_cache: false,
            require_fresh_strategy_cache: false,
        };
        let diagnostics = vec![
            Diagnostic::new(
                Path::new("src/page.rs"),
                4,
                9,
                Severity::Info,
                "render/advisory",
                "advisory",
            ),
            Diagnostic::new(
                Path::new("src/page.rs"),
                5,
                13,
                Severity::Info,
                "rsjs/client-crossing",
                "public crossing",
            ),
        ];

        assert!(has_denied_lint(&args, &diagnostics));

        let mut unrelated = args;
        unrelated.deny = vec!["rsjs/unused-client-module".to_string()];
        assert!(!has_denied_lint(&unrelated, &diagnostics));
    }

    #[test]
    fn lint_deny_warnings_keeps_existing_severity_behavior() {
        let args = LintArgs {
            files: FileSelectionArgs {
                paths: Vec::new(),
                changed: false,
                staged: false,
                base: None,
            },
            root: None,
            deny_warnings: false,
            deny: vec!["warnings".to_string()],
            json: false,
            min_severity: None,
            github: false,
            include_tests: false,
            content_summary: false,
            use_rsjs_strategy_cache: false,
            require_fresh_strategy_cache: false,
        };
        let diagnostics = vec![Diagnostic::new(
            Path::new("src/page.rs"),
            4,
            9,
            Severity::Warning,
            "render/advisory",
            "warning",
        )];

        assert!(has_denied_lint(&args, &diagnostics));
    }
}
