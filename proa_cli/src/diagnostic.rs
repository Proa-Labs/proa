// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

/// Authored source range. Lines and Unicode scalar columns are one-based;
/// the end is exclusive (display widths and editor UTF-16 offsets differ).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRange {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceRange {
    pub fn from_span(path: &Path, span: proc_macro2::Span) -> Self {
        let start = span.start();
        let end = span.end();
        Self {
            path: path.to_owned(),
            line: start.line.max(1),
            column: start.column + 1,
            end_line: end.line.max(1),
            end_column: end.column + 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceLine {
    pub number: usize,
    pub text: String,
}

/// Evidence retained with a diagnostic, so output never needs to reread a file.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLabel {
    pub range: SourceRange,
    pub message: String,
    pub primary: bool,
    pub source: Vec<SourceLine>,
}

impl SourceLabel {
    pub fn new(
        range: SourceRange,
        source: &str,
        message: impl Into<String>,
        primary: bool,
    ) -> Self {
        let lines: Vec<_> = source
            .lines()
            .enumerate()
            .filter_map(|(index, text)| {
                let number = index + 1;
                let near_start = number >= range.line.saturating_sub(1)
                    && number <= range.line.saturating_add(2);
                let near_end = number >= range.end_line.saturating_sub(1)
                    && number <= range.end_line.saturating_add(1);
                (near_start || near_end).then(|| SourceLine {
                    number,
                    text: text.to_owned(),
                })
            })
            .collect();
        Self {
            range,
            message: message.into(),
            primary,
            source: lines,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SuggestionApplicability {
    MachineApplicable,
    MaybeIncorrect,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub message: String,
    pub range: SourceRange,
    pub replacement: String,
    pub applicability: SuggestionApplicability,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    /// Ordering rank for severity filtering (higher = more severe).
    pub const fn rank(self) -> u8 {
        match self {
            Self::Error => 2,
            Self::Warning => 1,
            Self::Info => 0,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

impl Serialize for Severity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub help: Option<String>,
    pub labels: Vec<SourceLabel>,
    pub notes: Vec<String>,
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    pub fn new(
        path: impl AsRef<Path>,
        line: usize,
        column: usize,
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            line,
            column,
            severity,
            code,
            message: message.into(),
            help: None,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_label(mut self, label: SourceLabel) -> Self {
        self.labels.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.labels.is_empty() {
            writeln!(
                f,
                "{}[{}]: {}",
                self.severity.as_str(),
                self.code,
                self.message
            )?;
            for label in &self.labels {
                render_label(f, label)?;
            }
            for note in &self.notes {
                writeln!(f, "   = note: {note}")?;
            }
            if let Some(help) = &self.help {
                writeln!(f, "   = help: {help}")?;
            }
            for suggestion in &self.suggestions {
                writeln!(f, "   = help: {}", suggestion.message)?;
                for line in suggestion.replacement.lines() {
                    writeln!(f, "           {line}")?;
                }
            }
            return Ok(());
        }
        writeln!(
            f,
            "{}:{}:{}: {}[{}]: {}",
            self.path.display(),
            self.line,
            self.column,
            self.severity.as_str(),
            self.code,
            self.message
        )?;
        if let Some(help) = &self.help {
            writeln!(f, "  help: {help}")?;
        }
        for note in &self.notes {
            writeln!(f, "  note: {note}")?;
        }
        for suggestion in &self.suggestions {
            writeln!(
                f,
                "  help: {}: {}",
                suggestion.message, suggestion.replacement
            )?;
        }
        Ok(())
    }
}

fn render_label(f: &mut fmt::Formatter<'_>, label: &SourceLabel) -> fmt::Result {
    let range = &label.range;
    let arrow = if label.primary { "-->" } else { ":::" };
    writeln!(
        f,
        "  {arrow} {}:{}:{}",
        crate::source::display_path(&range.path).display(),
        range.line,
        range.column
    )?;
    let width = label
        .source
        .last()
        .map_or(range.line, |line| line.number)
        .to_string()
        .len();
    writeln!(f, " {:width$} |", "")?;
    let mut previous = None;
    for line in &label.source {
        if previous.is_some_and(|number| line.number > number + 1) {
            writeln!(f, " {:width$} | ...", "")?;
        }
        writeln!(f, " {:>width$} | {}", line.number, line.text)?;
        if line.number == range.line {
            let column = range
                .column
                .saturating_sub(1)
                .min(line.text.chars().count());
            let authored_prefix: String = line.text.chars().take(column).collect();
            let prefix: String = authored_prefix
                .split_inclusive('\t')
                .map(|segment| {
                    let text = segment.trim_end_matches('\t');
                    let mut spaces = " ".repeat(text.width());
                    if segment.ends_with('\t') {
                        spaces.push('\t');
                    }
                    spaces
                })
                .collect();
            let available = line.text.chars().count().saturating_sub(column).max(1);
            let length = if range.end_line == range.line {
                range
                    .end_column
                    .saturating_sub(range.column)
                    .max(1)
                    .min(available)
            } else {
                available
            };
            let highlighted: String = line.text.chars().skip(column).take(length).collect();
            let underline = if label.primary { '^' } else { '-' }
                .to_string()
                .repeat(highlighted.width().max(1));
            writeln!(f, " {:width$} | {prefix}{underline} {}", "", label.message)?;
        }
        previous = Some(line.number);
    }
    if !label.source.iter().any(|line| line.number == range.line) {
        writeln!(f, " {:width$} | {}", "", label.message)?;
    }
    writeln!(f, " {:width$} |", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_rank_orders_error_over_warning_over_info() {
        assert!(Severity::Error.rank() > Severity::Warning.rank());
        assert!(Severity::Warning.rank() > Severity::Info.rank());
    }

    #[test]
    fn severity_serializes_as_lowercase_string() {
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Error).unwrap(),
            "\"error\""
        );
    }

    #[test]
    fn display_includes_location_code_and_help() {
        let diagnostic = Diagnostic::new(
            Path::new("src/page.rs"),
            4,
            9,
            Severity::Warning,
            "perf/vec",
            "Vec in a render path",
        )
        .with_help("use stack storage");

        let rendered = diagnostic.to_string();
        assert_eq!(
            rendered,
            "src/page.rs:4:9: warning[perf/vec]: Vec in a render path\n  help: use stack storage\n"
        );
    }

    #[test]
    fn display_omits_help_line_when_absent() {
        let diagnostic = Diagnostic::new(
            Path::new("src/page.rs"),
            1,
            1,
            Severity::Info,
            "render/advisory",
            "advisory",
        );

        assert_eq!(
            diagnostic.to_string(),
            "src/page.rs:1:1: info[render/advisory]: advisory\n"
        );
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    #[test]
    fn source_frames_have_primary_related_evidence_and_repairs() {
        let range = SourceRange {
            path: "src/product.md.rs".into(),
            line: 2,
            column: 5,
            end_line: 2,
            end_column: 13,
        };
        let related = SourceRange {
            path: "src/product.html.rs".into(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 8,
        };
        let diag = Diagnostic::new(
            &range.path,
            2,
            5,
            Severity::Error,
            "render/content-coverage",
            "Product Markdown omits RestockNotice",
        )
        .with_label(SourceLabel::new(
            range.clone(),
            "fn render_md() {\n    md_sync! {}\n}",
            "the notice is absent here",
            true,
        ))
        .with_label(SourceLabel::new(
            related,
            "notice.render(cx)",
            "rendered in HTML",
            false,
        ))
        .with_note("constructing a child does not render it")
        .with_suggestion(Suggestion {
            message: "acknowledge an intentional omission inside render_md".into(),
            range,
            replacement: "proa_macros::omit!(RestockNotice);".into(),
            applicability: SuggestionApplicability::MaybeIncorrect,
        });
        assert_eq!(diag.to_string(),"error[render/content-coverage]: Product Markdown omits RestockNotice\n  --> src/product.md.rs:2:5\n   |\n 1 | fn render_md() {\n 2 |     md_sync! {}\n   |     ^^^^^^^^ the notice is absent here\n 3 | }\n   |\n  ::: src/product.html.rs:1:1\n   |\n 1 | notice.render(cx)\n   | ------- rendered in HTML\n   |\n   = note: constructing a child does not render it\n   = help: acknowledge an intentional omission inside render_md\n           proa_macros::omit!(RestockNotice);\n");
    }
    #[test]
    fn rust_span_columns_are_unicode_scalar_offsets() {
        let expr: syn::Expr = syn::parse_str("(\"🦀\", value)").unwrap();
        let syn::Expr::Tuple(tuple) = expr else {
            panic!()
        };
        let range = SourceRange::from_span(
            Path::new("test.rs"),
            syn::spanned::Spanned::span(&tuple.elems[1]),
        );
        assert_eq!(range.column, 7);
        assert_eq!(range.end_column, 12);
    }
}

#[cfg(test)]
mod unicode_frame_tests {
    use super::*;
    #[test]
    fn source_underlines_use_display_width_after_wide_characters() {
        let r = SourceRange {
            path: "md.rs".into(),
            line: 1,
            column: 3,
            end_line: 1,
            end_column: 8,
        };
        let d = Diagnostic::new("md.rs", 1, 3, Severity::Info, "test/frame", "frame")
            .with_label(SourceLabel::new(r, "🦀 value", "value", true));
        assert!(d.to_string().contains("   |    ^^^^^ value"));
    }
}
