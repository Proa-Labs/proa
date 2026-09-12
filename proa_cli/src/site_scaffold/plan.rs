// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub path: PathBuf,
    pub bytes: usize,
    #[serde(skip)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePlan {
    pub destination: PathBuf,
    pub files: Vec<PlannedFile>,
    pub capabilities: Vec<String>,
}

impl SitePlan {
    pub fn new(destination: PathBuf, capabilities: Vec<String>) -> Self {
        Self {
            destination,
            files: Vec::new(),
            capabilities,
        }
    }

    pub fn write(&mut self, relative: impl AsRef<Path>, content: impl Into<String>) -> Result<()> {
        let relative = relative.as_ref();
        validate_relative(relative)?;
        if self.files.iter().any(|file| file.path == relative) {
            bail!("scaffold planned `{}` more than once", relative.display());
        }
        let content = content.into();
        self.files.push(PlannedFile {
            path: relative.to_path_buf(),
            bytes: content.len(),
            content,
        });
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        for required in [
            "Cargo.toml",
            "src/main.rs",
            "proa.config.json",
            "proa.lock.json",
            "public/styles.css",
        ] {
            if !self
                .files
                .iter()
                .any(|file| file.path == Path::new(required))
            {
                bail!("scaffold plan is missing required file `{required}`");
            }
        }
        for file in &self.files {
            if file.content.contains("__PACKAGE__")
                || file.content.contains("__TITLE__")
                || file.content.contains("__DATABASE_URL__")
            {
                bail!(
                    "scaffold template placeholder remained in `{}`",
                    file.path.display()
                );
            }
            if file.path.extension().and_then(|value| value.to_str()) == Some("json") {
                serde_json::from_str::<serde_json::Value>(&file.content)
                    .with_context(|| format!("validate generated {}", file.path.display()))?;
            }
            if file.path.extension().and_then(|value| value.to_str()) == Some("rs") {
                syn::parse_file(&file.content)
                    .with_context(|| format!("parse generated {}", file.path.display()))?;
            }
        }
        Ok(())
    }
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.is_absolute() || path.as_os_str().is_empty() {
        bail!("planned file paths must be nonempty and relative");
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!("planned file path escapes the project: {}", path.display());
    }
    Ok(())
}
