// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TypeScriptDocgen {
    pub enabled: bool,
    pub tsconfig_path: PathBuf,
    pub base_path: PathBuf,
    pub cache_dir: PathBuf,
    pub component_name: String,
    pub hide_internal: bool,
    pub annotations: Vec<Annotation>,
    pub command: Option<String>,
}

impl Default for TypeScriptDocgen {
    fn default() -> Self {
        Self {
            enabled: false,
            tsconfig_path: PathBuf::from("tsconfig.json"),
            base_path: PathBuf::from("."),
            cache_dir: PathBuf::from("target/proa-docs/typescript"),
            component_name: "auto-type-table".to_string(),
            hide_internal: true,
            annotations: vec![
                Annotation::Internal("@internal".to_string()),
                Annotation::DisplayType("@fumadocsType".to_string()),
                Annotation::DisplayTypeFallback("@remarks".to_string()),
                Annotation::Href("@fumadocsHref".to_string()),
            ],
            command: None,
        }
    }
}

impl TypeScriptDocgen {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tsconfig_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.tsconfig_path = path.into();
        self
    }

    pub fn base_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.base_path = path.into();
        self
    }

    pub fn cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_dir = path.into();
        self
    }

    pub fn component_name(mut self, name: impl Into<String>) -> Self {
        self.component_name = name.into();
        self
    }

    pub fn hide_internal(mut self, hide_internal: bool) -> Self {
        self.hide_internal = hide_internal;
        self
    }

    pub fn annotation(mut self, annotation: Annotation) -> Self {
        self.annotations.push(annotation);
        self
    }

    pub fn command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Internal(String),
    DisplayType(String),
    DisplayTypeFallback(String),
    Href(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeTableRequest {
    pub source_markdown_path: PathBuf,
    pub path: Option<String>,
    pub name: Option<String>,
    pub inline_type: Option<String>,
}
