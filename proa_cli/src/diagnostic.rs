// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

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
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        Ok(())
    }
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
