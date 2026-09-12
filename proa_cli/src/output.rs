// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Diagnostic presentation shared by the command layer: human-readable
//! terminal output, stable JSON, and GitHub Actions annotations.

use anyhow::Result;
use serde::Serialize;

use crate::diagnostic::{Diagnostic, Severity};

pub(crate) fn print_diagnostics(
    diagnostics: &[Diagnostic],
    json: bool,
    github: bool,
) -> Result<()> {
    if json {
        let items: Vec<_> = diagnostics.iter().map(DiagnosticJson::from).collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if github {
        for diagnostic in diagnostics {
            println!("{}", github_annotation(diagnostic));
        }
    } else {
        for diagnostic in diagnostics {
            eprint!("{diagnostic}");
        }
    }
    Ok(())
}

pub(crate) fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diag| diag.severity == Severity::Error)
}

pub(crate) fn has_warnings(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diag| diag.severity == Severity::Warning)
}

#[derive(Serialize)]
struct DiagnosticJson<'a> {
    path: String,
    line: usize,
    column: usize,
    severity: Severity,
    code: &'static str,
    message: &'a str,
    help: Option<&'a str>,
}

impl<'a> From<&'a Diagnostic> for DiagnosticJson<'a> {
    fn from(diagnostic: &'a Diagnostic) -> Self {
        Self {
            path: diagnostic.path.display().to_string(),
            line: diagnostic.line,
            column: diagnostic.column,
            severity: diagnostic.severity,
            code: diagnostic.code,
            message: &diagnostic.message,
            help: diagnostic.help.as_deref(),
        }
    }
}

fn github_annotation(diagnostic: &Diagnostic) -> String {
    let level = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    format!(
        "::{level} file={},line={},col={},title={}::{}",
        github_escape_property(&diagnostic.path.display().to_string()),
        diagnostic.line,
        diagnostic.column,
        github_escape_property(diagnostic.code),
        github_escape_message(&diagnostic.message)
    )
}

fn github_escape_property(value: &str) -> String {
    github_escape_message(value)
        .replace(',', "%2C")
        .replace(':', "%3A")
}

fn github_escape_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn github_annotation_maps_info_to_notice() {
        let diagnostic = Diagnostic::new(
            Path::new("src/example.rs"),
            4,
            9,
            Severity::Info,
            "render/advisory",
            "advisory message",
        );

        assert!(github_annotation(&diagnostic).starts_with("::notice "));
    }
}
