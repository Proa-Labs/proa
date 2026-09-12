// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Read tsconfig.json for TypeScript path aliases.
//!
//! Walks up from `CARGO_MANIFEST_DIR` looking for tsconfig.json or package.json,
//! then extracts `compilerOptions.baseUrl` and `compilerOptions.paths`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resolved configuration for finding TSX files.
#[derive(Debug, Clone)]
pub struct TsxResolveConfig {
    /// Base directory for resolving relative paths (tsconfig's dir + baseUrl).
    pub base_dir: PathBuf,
    /// Path aliases from tsconfig `compilerOptions.paths`.
    /// Key is the alias pattern (e.g. `@/*`), value is the replacement patterns.
    pub aliases: HashMap<String, Vec<String>>,
}

/// Load the resolve config by finding tsconfig.json.
///
/// Walks up from `start_dir` looking for `tsconfig.json`, then falls back to
/// looking for `package.json` (same directory likely has tsconfig).
pub fn load_resolve_config(start_dir: &Path) -> Result<TsxResolveConfig, String> {
    let tsconfig_path = find_tsconfig(start_dir).ok_or_else(|| {
        format!(
            "Could not find tsconfig.json searching upward from {:?}",
            start_dir
        )
    })?;

    parse_tsconfig(&tsconfig_path)
}

/// Find tsconfig.json by walking up from the given directory.
fn find_tsconfig(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();

    loop {
        let candidate = dir.join("tsconfig.json");
        if candidate.exists() {
            return Some(candidate);
        }

        // Also check if package.json exists here (likely project root)
        // but keep looking for tsconfig specifically
        if !dir.pop() {
            break;
        }
    }

    None
}

/// Parse tsconfig.json and extract baseUrl + paths.
fn parse_tsconfig(path: &Path) -> Result<TsxResolveConfig, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse {:?}: {}", path, e))?;

    let tsconfig_dir = path.parent().unwrap_or(Path::new("."));

    let compiler_opts = json.get("compilerOptions");

    // Extract baseUrl (default ".")
    let base_url = compiler_opts
        .and_then(|co| co.get("baseUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let base_dir = tsconfig_dir.join(base_url);

    // Extract paths
    let mut aliases = HashMap::new();

    if let Some(paths) = compiler_opts
        .and_then(|co| co.get("paths"))
        .and_then(|v| v.as_object())
    {
        for (key, value) in paths {
            if let Some(arr) = value.as_array() {
                let replacements: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                aliases.insert(key.clone(), replacements);
            }
        }
    }

    Ok(TsxResolveConfig { base_dir, aliases })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tsconfig_basic() {
        let dir = std::env::temp_dir().join("proa_tsconfig_test");
        std::fs::create_dir_all(&dir).unwrap();

        let tsconfig = dir.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@/*": ["src/*"],
                        "@components/*": ["src/components/*"]
                    }
                }
            }"#,
        )
        .unwrap();

        let config = parse_tsconfig(&tsconfig).unwrap();
        assert_eq!(config.base_dir, dir.join("."));
        assert_eq!(config.aliases.len(), 2);
        assert_eq!(config.aliases["@/*"], vec!["src/*"]);
        assert_eq!(config.aliases["@components/*"], vec!["src/components/*"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_tsconfig_no_paths() {
        let dir = std::env::temp_dir().join("proa_tsconfig_test_nopaths");
        std::fs::create_dir_all(&dir).unwrap();

        let tsconfig = dir.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{ "compilerOptions": { "target": "es2020" } }"#,
        )
        .unwrap();

        let config = parse_tsconfig(&tsconfig).unwrap();
        assert!(config.aliases.is_empty());
        assert_eq!(config.base_dir, dir.join("."));

        std::fs::remove_dir_all(&dir).ok();
    }
}
