// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::BTreeSet;

pub(crate) fn is_test_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str() == Some("tests"))
        || is_cfg_test_module_file(path)
}

/// Whether `path` is a module file that its parent declares with
/// `#[cfg(test)] mod name;` (for example `src/assets/tests.rs`), or a module
/// nested inside one. Inline `#[cfg(test)] mod tests { .. }` blocks are
/// covered by [`cfg_test_module_line_ranges`] instead.
fn is_cfg_test_module_file(path: &Path) -> bool {
    let (Some(stem), Some(dir)) = (path.file_stem().and_then(|s| s.to_str()), path.parent()) else {
        return false;
    };
    let (name, module_dir) = match stem {
        "lib" | "main" => return false,
        "mod" => match (dir.file_name().and_then(|n| n.to_str()), dir.parent()) {
            (Some(name), Some(parent)) => (name, parent),
            _ => return false,
        },
        name => (name, dir),
    };
    // `name` is declared by the module that owns `module_dir`: `<dir>.rs`,
    // `<dir>/mod.rs`, or a crate root beside it.
    let mut parents = vec![
        module_dir.join("mod.rs"),
        module_dir.join("lib.rs"),
        module_dir.join("main.rs"),
    ];
    if let (Some(dir_name), Some(outer)) = (
        module_dir.file_name().and_then(|n| n.to_str()),
        module_dir.parent(),
    ) {
        parents.push(outer.join(format!("{dir_name}.rs")));
    }
    parents.iter().any(|parent| {
        cfg_test_module_names(parent).is_some_and(|names| names.contains(name))
            || (parent.is_file() && is_cfg_test_module_file(parent))
    })
}

/// Out-of-line modules that `parent` declares under `#[cfg(test)]`.
///
/// Every lint pass asks about every file, so each parent is read once and
/// cached until its modification time or length changes (the editor server
/// is long-lived).
fn cfg_test_module_names(parent: &Path) -> Option<std::sync::Arc<BTreeSet<String>>> {
    use std::sync::{Arc, Mutex, OnceLock};
    type Stamp = (std::time::SystemTime, u64);
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (Stamp, Arc<BTreeSet<String>>)>>> =
        OnceLock::new();

    let metadata = std::fs::metadata(parent).ok()?;
    let stamp = (metadata.modified().ok()?, metadata.len());
    let cache = CACHE.get_or_init(Default::default);
    let lock = || {
        cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    };
    if let Some((cached, names)) = lock().get(parent) {
        if *cached == stamp {
            return Some(Arc::clone(names));
        }
    }
    let names = Arc::new(cfg_test_module_declarations(
        &std::fs::read_to_string(parent).ok()?,
    ));
    lock().insert(parent.to_path_buf(), (stamp, Arc::clone(&names)));
    Some(names)
}

/// Names of out-of-line modules declared under exactly `#[cfg(test)]`, the
/// attribute [`cfg_test_module_line_ranges`] accepts, on the same line or on
/// attribute lines just above the declaration.
fn cfg_test_module_declarations(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut cfg_test_pending = false;
    for line in source.lines() {
        let mut rest = line.trim();
        while let Some(attribute) = rest.strip_prefix("#[") {
            let Some(end) = attribute.find(']') else {
                break;
            };
            let compact: String = attribute[..end]
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect();
            cfg_test_pending |= compact == "cfg(test)";
            rest = attribute[end + 1..].trim_start();
        }
        if rest.is_empty() || rest.starts_with("//") {
            continue;
        }
        if cfg_test_pending {
            if let Some(name) = declared_module(rest) {
                names.insert(name.to_owned());
            }
        }
        cfg_test_pending = false;
    }
    names
}

/// The name in `mod name;` with an optional visibility, such as
/// `pub(crate) mod name;`.
fn declared_module(item: &str) -> Option<&str> {
    let mut rest = item;
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if let Some(scoped) = rest.strip_prefix('(') {
            rest = scoped[scoped.find(')')? + 1..].trim_start();
        }
    }
    let after = rest.strip_prefix("mod")?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let after = after.trim_start();
    let end = after
        .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .unwrap_or(after.len());
    (end > 0 && after[end..].trim_start().starts_with(';')).then(|| &after[..end])
}

pub(crate) fn cfg_test_module_line_ranges(source: &str) -> Vec<RangeInclusive<usize>> {
    let mut ranges = Vec::new();
    let mut cfg_test_pending = false;
    let mut active_start = None;
    let mut depth = 0isize;

    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let stripped = line.trim();

        if active_start.is_some() {
            depth += brace_delta(line);
            if depth <= 0 {
                ranges.push(active_start.take().unwrap()..=line_no);
                depth = 0;
            }
            continue;
        }

        if stripped.starts_with("#[cfg(test)]") {
            cfg_test_pending = true;
            continue;
        }

        if cfg_test_pending && stripped.contains("mod tests") {
            if line.contains('{') {
                active_start = Some(line_no);
                depth = brace_delta(line).max(1);
                if depth <= 0 {
                    ranges.push(line_no..=line_no);
                    active_start = None;
                    depth = 0;
                }
            }
            cfg_test_pending = false;
            continue;
        }

        if !stripped.is_empty() && !stripped.starts_with("#[") {
            cfg_test_pending = false;
        }
    }

    if let Some(start) = active_start {
        ranges.push(start..=source.lines().count().max(start));
    }

    ranges
}

pub(crate) fn brace_delta(line: &str) -> isize {
    line.bytes().fold(0, |acc, byte| match byte {
        b'{' => acc + 1,
        b'}' => acc - 1,
        _ => acc,
    })
}

pub(crate) fn line_in_ranges(line: usize, ranges: &[RangeInclusive<usize>]) -> bool {
    ranges.iter().any(|range| range.contains(&line))
}

pub(crate) fn dedupe_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    // Key by borrowed message to avoid a String clone per diagnostic.
    let mut seen = HashSet::new();
    let keep: Vec<bool> = diagnostics
        .iter()
        .map(|diag| seen.insert((diag.line, diag.column, diag.code, diag.message.as_str())))
        .collect();
    drop(seen);
    let mut keep = keep.into_iter();
    diagnostics.retain(|_| keep.next().unwrap_or(true));
}
