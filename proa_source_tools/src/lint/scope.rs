// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

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
        std::fs::read_to_string(parent).is_ok_and(|source| {
            declares_cfg_test_module(&source, name) || is_cfg_test_module_file(parent)
        })
    })
}

/// Whether `source` declares the out-of-line module `name` under exactly
/// `#[cfg(test)]`, the same attribute [`cfg_test_module_line_ranges`] accepts,
/// on the same line or on attribute lines just above it.
fn declares_cfg_test_module(source: &str, name: &str) -> bool {
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
        if cfg_test_pending && declares_module(rest, name) {
            return true;
        }
        cfg_test_pending = false;
    }
    false
}

/// `mod name;` with an optional visibility, such as `pub(crate) mod name;`.
fn declares_module(item: &str, name: &str) -> bool {
    let mut rest = item;
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if let Some(scoped) = rest.strip_prefix('(') {
            let Some(end) = scoped.find(')') else {
                return false;
            };
            rest = scoped[end + 1..].trim_start();
        }
    }
    rest.strip_prefix("mod")
        .filter(|after| after.starts_with(char::is_whitespace))
        .and_then(|after| after.trim_start().strip_prefix(name))
        .is_some_and(|after| after.trim_start().starts_with(';'))
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
