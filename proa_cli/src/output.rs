// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Diagnostic presentation shared by the command layer: human-readable
//! terminal output, stable JSON, and GitHub Actions annotations.

use anyhow::Result;
use serde::Serialize;

use crate::diagnostic::{Diagnostic, Severity, SourceLabel, Suggestion};

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: &'a Vec<SourceLabel>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: &'a Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: &'a Vec<Suggestion>,
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
            labels: &diagnostic.labels,
            notes: &diagnostic.notes,
            suggestions: &diagnostic.suggestions,
        }
    }
}

fn github_annotation(diagnostic: &Diagnostic) -> String {
    let level = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    let mut message = diagnostic.message.clone();
    for label in &diagnostic.labels {
        message.push_str(&format!(
            "\n{}:{}:{}: {}",
            crate::source::display_path(&label.range.path).display(),
            label.range.line,
            label.range.column,
            label.message
        ));
    }
    for note in &diagnostic.notes {
        message.push_str(&format!("\nnote: {note}"));
    }
    if let Some(help) = &diagnostic.help {
        message.push_str(&format!("\nhelp: {help}"));
    }
    for suggestion in &diagnostic.suggestions {
        message.push_str(&format!(
            "\nhelp: {}\n{}",
            suggestion.message, suggestion.replacement
        ));
    }
    format!(
        "::{level} file={},line={},col={},title={}::{}",
        github_escape_property(
            &crate::source::display_path(&diagnostic.path)
                .display()
                .to_string()
        ),
        diagnostic.line,
        diagnostic.column,
        github_escape_property(diagnostic.code),
        github_escape_message(&message)
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

#[cfg(test)]
mod content_output_tests {
    use super::*;
    use crate::diagnostic::{SourceRange, SuggestionApplicability};
    #[test]
    fn machine_outputs_preserve_paired_evidence_and_suggestions() {
        let primary = SourceRange {
            path: "src/md.rs".into(),
            line: 3,
            column: 4,
            end_line: 3,
            end_column: 10,
        };
        let related = SourceRange {
            path: "src/html.rs".into(),
            line: 8,
            column: 9,
            end_line: 8,
            end_column: 20,
        };
        let d = Diagnostic::new(
            &primary.path,
            3,
            4,
            Severity::Info,
            "render/content-coverage",
            "Product omits Notice",
        )
        .with_label(SourceLabel::new(
            primary.clone(),
            "\n\n   md! {}",
            "missing here",
            true,
        ))
        .with_label(SourceLabel::new(
            related,
            "\n\n\n\n\n\n\n        Notice",
            "HTML child",
            false,
        ))
        .with_note("construction is not output")
        .with_help("add the child")
        .with_suggestion(Suggestion {
            message: "intentional omission".into(),
            range: primary,
            replacement: "omit!(Notice);".into(),
            applicability: SuggestionApplicability::MaybeIncorrect,
        });
        let json = serde_json::to_value(DiagnosticJson::from(&d)).unwrap();
        assert_eq!(json["labels"][1]["range"]["path"], "src/html.rs");
        assert_eq!(json["labels"][0]["range"]["end_column"], 10);
        assert_eq!(json["suggestions"][0]["applicability"], "maybe-incorrect");
        assert_eq!(json["notes"][0], "construction is not output");
        let github = github_annotation(&d);
        assert!(github.contains("src/html.rs:8:9: HTML child"));
        assert!(github.contains("%0Anote: construction is not output"));
        assert!(github.contains("omit!(Notice);"));
    }
}
