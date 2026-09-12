// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! TSX file resolution logic.
//!
//! Resolves island component names to TSX file paths using:
//! - Direct `path = "..."` specification
//! - Module `module = "..."` resolution via tsconfig aliases
//! - Auto-discovery by scanning the base directory

use std::path::{Path, PathBuf};

use crate::tsconfig::TsxResolveConfig;

/// Extensions to try when resolving modules.
const EXTENSIONS: &[&str] = &[".tsx", ".ts", ".jsx", ".js"];

/// Resolve a TSX file path for an island component.
///
/// Priority:
/// 1. `explicit_path` — resolve directly relative to macro invocation site
/// 2. `module` — resolve via tsconfig aliases
/// 3. Auto-discover by scanning base_dir
pub fn resolve_tsx_path(
    component_name: &str,
    explicit_path: Option<&str>,
    module: Option<&str>,
    config: &TsxResolveConfig,
    invocation_dir: &Path,
) -> Result<PathBuf, String> {
    // 1. Explicit path
    if let Some(path) = explicit_path {
        let resolved = invocation_dir.join(path);
        if resolved.exists() {
            return Ok(resolved);
        }
        // Also try relative to base_dir
        let from_base = config.base_dir.join(path);
        if from_base.exists() {
            return Ok(from_base);
        }
        return Err(format!(
            "Island `{}`: explicit path `{}` not found (tried {:?} and {:?})",
            component_name, path, resolved, from_base
        ));
    }

    // 2. Module resolution
    if let Some(module_path) = module {
        return resolve_module(component_name, module_path, config);
    }

    // 3. Auto-discover
    auto_discover(component_name, config)
}

/// Resolve a module specifier using tsconfig aliases.
fn resolve_module(
    component_name: &str,
    module_path: &str,
    config: &TsxResolveConfig,
) -> Result<PathBuf, String> {
    // Try to match against aliases
    for (pattern, replacements) in &config.aliases {
        if let Some(resolved) = try_alias_match(module_path, pattern, replacements, config) {
            return Ok(resolved);
        }
    }

    // Try as a direct relative path from base_dir
    if let Some(resolved) = try_with_extensions(&config.base_dir.join(module_path)) {
        return Ok(resolved);
    }

    // Try as a directory with index file
    let dir_path = config.base_dir.join(module_path);
    if dir_path.is_dir() {
        if let Some(resolved) = try_index_file(&dir_path) {
            return Ok(resolved);
        }
    }

    Err(format!(
        "Island `{}`: could not resolve module `{}`",
        component_name, module_path
    ))
}

/// Try to match a module path against a tsconfig alias pattern.
fn try_alias_match(
    module_path: &str,
    pattern: &str,
    replacements: &[String],
    config: &TsxResolveConfig,
) -> Option<PathBuf> {
    // Pattern is like "@/*" or "@components/*"
    // If pattern ends with *, it's a prefix match
    if let Some(prefix) = pattern.strip_suffix('*') {
        if let Some(rest) = module_path.strip_prefix(prefix) {
            for replacement in replacements {
                if let Some(rep_prefix) = replacement.strip_suffix('*') {
                    let candidate = config.base_dir.join(format!("{}{}", rep_prefix, rest));
                    if let Some(resolved) = try_with_extensions(&candidate) {
                        return Some(resolved);
                    }
                    // Try as directory with index
                    if candidate.is_dir() {
                        if let Some(resolved) = try_index_file(&candidate) {
                            return Some(resolved);
                        }
                    }
                }
            }
        }
    } else {
        // Exact match
        if module_path == pattern {
            for replacement in replacements {
                let candidate = config.base_dir.join(replacement);
                if let Some(resolved) = try_with_extensions(&candidate) {
                    return Some(resolved);
                }
            }
        }
    }

    None
}

/// Try appending each extension to a path.
fn try_with_extensions(base: &Path) -> Option<PathBuf> {
    // First try the path as-is
    if base.exists() && base.is_file() {
        return Some(base.to_path_buf());
    }

    // Try each extension
    for ext in EXTENSIONS {
        let with_ext = base.with_extension(ext.trim_start_matches('.'));
        if with_ext.exists() {
            return Some(with_ext);
        }
    }

    None
}

/// Try to find an index file in a directory.
fn try_index_file(dir: &Path) -> Option<PathBuf> {
    for ext in EXTENSIONS {
        let index = dir.join(format!("index{}", ext));
        if index.exists() {
            return Some(index);
        }
    }
    None
}

/// Auto-discover a component's TSX file by scanning base_dir.
fn auto_discover(component_name: &str, config: &TsxResolveConfig) -> Result<PathBuf, String> {
    if !config.base_dir.exists() {
        return Err(format!(
            "Island `{}`: base directory {:?} does not exist",
            component_name, config.base_dir
        ));
    }

    // Quick path: check for {Name}.tsx directly in common locations
    let quick_candidates = [
        config.base_dir.join(format!("{}.tsx", component_name)),
        config
            .base_dir
            .join(format!("components/{}.tsx", component_name)),
        config
            .base_dir
            .join(format!("islands/{}.tsx", component_name)),
        config.base_dir.join(format!("src/{}.tsx", component_name)),
        config
            .base_dir
            .join(format!("src/components/{}.tsx", component_name)),
        config
            .base_dir
            .join(format!("src/islands/{}.tsx", component_name)),
    ];

    for candidate in &quick_candidates {
        if candidate.exists() && file_exports_component(candidate, component_name) {
            return Ok(candidate.clone());
        }
    }

    // Full scan: walk base_dir recursively looking for matching exports
    let mut matches = Vec::new();
    scan_directory(&config.base_dir, component_name, &mut matches);

    match matches.len() {
        0 => Err(format!(
            "Island `{}`: no TSX file found exporting `{}` in {:?}",
            component_name, component_name, config.base_dir
        )),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(format!(
            "Island `{}`: ambiguous — multiple files export `{}`: {:?}. Use `module = \"...\"` or `path = \"...\"` to disambiguate.",
            component_name, component_name, matches
        )),
    }
}

/// Recursively scan a directory for TSX/TS files that export a component.
fn scan_directory(dir: &Path, component_name: &str, matches: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip node_modules, hidden dirs, dist/build
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "dist" || name == "build"
            {
                continue;
            }
            scan_directory(&path, component_name, matches);
        } else if is_tsx_file(&path) && file_exports_component(&path, component_name) {
            matches.push(path);
        }
    }
}

/// Check if a file path has a TSX/TS extension.
fn is_tsx_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| matches!(ext, "tsx" | "ts" | "jsx" | "js"))
}

/// Quick check if a file exports a component with the given name.
///
/// Looks for patterns like:
/// - `export default function Name`
/// - `export function Name`
/// - `export default Name`
/// - `export const Name`
fn file_exports_component(path: &Path, component_name: &str) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let patterns = [
        format!("export default function {}", component_name),
        format!("export function {}", component_name),
        format!("export default {}", component_name),
        format!("export const {}", component_name),
    ];

    for pattern in &patterns {
        if let Some(pos) = content.find(pattern.as_str()) {
            // Verify it's a word boundary after the name
            let after = pos + pattern.len();
            if after >= content.len() {
                return true;
            }
            let next_char = content.as_bytes()[after];
            if !next_char.is_ascii_alphanumeric() && next_char != b'_' {
                return true;
            }
        }
    }

    // Also check for filename match (Name.tsx exports Name)
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if stem == component_name && content.contains("export") {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(base_dir: &Path) -> TsxResolveConfig {
        TsxResolveConfig {
            base_dir: base_dir.to_path_buf(),
            aliases: HashMap::new(),
        }
    }

    #[test]
    fn test_try_with_extensions() {
        let dir = std::env::temp_dir().join("proa_resolve_ext_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Counter.tsx"),
            "export default function Counter() {}",
        )
        .unwrap();

        let result = try_with_extensions(&dir.join("Counter"));
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("Counter.tsx"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_auto_discover() {
        let dir = std::env::temp_dir().join("proa_resolve_discover_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("components")).unwrap();
        std::fs::write(
            dir.join("components/Counter.tsx"),
            "export default function Counter(props: CounterProps) { return <div />; }",
        )
        .unwrap();

        let config = make_config(&dir);
        let result = auto_discover("Counter", &config);
        assert!(result.is_ok(), "auto_discover failed: {:?}", result.err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_explicit_path() {
        let dir = std::env::temp_dir().join("proa_resolve_explicit_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("MyComponent.tsx"),
            "export default function MyComponent() {}",
        )
        .unwrap();

        let config = make_config(&dir);
        let result = resolve_tsx_path("MyComponent", Some("MyComponent.tsx"), None, &config, &dir);
        assert!(result.is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
