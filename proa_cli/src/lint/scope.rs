// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn is_test_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str() == Some("tests"))
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
