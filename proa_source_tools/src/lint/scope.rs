// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::BTreeMap;

pub(crate) fn is_test_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str() == Some("tests"))
        || is_cfg_test_module_file(path)
}

/// Whether `path` is a module file compiled only for tests: every parent that
/// declares it does so under `#[cfg(test)]` (for example `#[cfg(test)] mod
/// tests;` for `src/assets/tests.rs`) or is itself test-only. A file that any
/// production module also declares ships, so it stays production code.
/// Inline `#[cfg(test)] mod tests { .. }` blocks are covered by
/// [`cfg_test_module_line_ranges`] instead.
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
    // `<dir>/mod.rs`, or a crate root beside it (a package can have both
    // `lib.rs` and `main.rs`).
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
    let mut test_owner = false;
    for parent in &parents {
        let Some(test_only) = module_declarations(parent).and_then(|d| d.names.get(name).copied())
        else {
            continue;
        };
        if test_only || is_cfg_test_module_file(parent) {
            test_owner = true;
        } else {
            return false;
        }
    }
    test_owner || is_cfg_test_path_module_file(path, dir)
}

/// Whether a sibling declares `path` only through `#[cfg(test)] #[path =
/// "file.rs"] mod name;`. A top-level `#[path]` resolves against the
/// declaring file's directory. Consulted only when no ordinary declaration
/// owns the file.
fn is_cfg_test_path_module_file(path: &Path, dir: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut test_owner = false;
    for sibling in entries.flatten().map(|entry| entry.path()) {
        if sibling == path || sibling.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let Some(test_only) =
            module_declarations(&sibling).and_then(|d| d.paths.get(file_name).copied())
        else {
            continue;
        };
        if test_only || is_cfg_test_module_file(&sibling) {
            test_owner = true;
        } else {
            return false;
        }
    }
    test_owner
}

/// Out-of-line `mod name;` items declared at the top level of one file,
/// mapped to whether every declaration is `#[cfg(test)]`. Nested inline
/// modules own other directories and are left out.
#[derive(Debug, Default)]
struct ModuleDeclarations {
    /// Declarations without `#[path]`, by module name.
    names: BTreeMap<String, bool>,
    /// `#[path = "..."]` declarations, by path value.
    paths: BTreeMap<String, bool>,
}

fn parse_module_declarations(source: &str) -> ModuleDeclarations {
    let mut declarations = ModuleDeclarations::default();
    for (name, test_only, path) in crate::source::top_level_module_declarations(source) {
        let map = match path {
            Some(path) => declarations.paths.entry(path),
            None => declarations.names.entry(name),
        };
        map.and_modify(|all_test| *all_test &= test_only)
            .or_insert(test_only);
    }
    declarations
}

thread_local! {
    /// Unsaved editor buffers for the snapshot being linted on this thread.
    static SOURCE_OVERRIDES: std::cell::RefCell<
        HashMap<PathBuf, (String, Option<std::sync::Arc<ModuleDeclarations>>)>,
    > = Default::default();
}

/// While alive, test-module checks on this thread read the given files from
/// memory instead of disk, so an unsaved edit to a `mod` declaration takes
/// effect in the same snapshot. The previous overrides return on drop.
pub(crate) struct SourceOverrides {
    previous: HashMap<PathBuf, (String, Option<std::sync::Arc<ModuleDeclarations>>)>,
}

impl SourceOverrides {
    pub(crate) fn install(sources: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        let overrides = sources
            .into_iter()
            .map(|(path, source)| (path, (source, None)))
            .collect();
        let previous = SOURCE_OVERRIDES.with(|cell| cell.replace(overrides));
        Self { previous }
    }
}

impl Drop for SourceOverrides {
    fn drop(&mut self) {
        let previous = std::mem::take(&mut self.previous);
        SOURCE_OVERRIDES.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// Module declarations in `parent`, from an editor override or from disk.
///
/// Every lint pass asks about every file, so a file on disk is parsed once and
/// cached until its modification time or length changes (the editor server
/// is long-lived).
fn module_declarations(parent: &Path) -> Option<std::sync::Arc<ModuleDeclarations>> {
    use std::sync::{Arc, Mutex, OnceLock};
    type Stamp = (std::time::SystemTime, u64);
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (Stamp, Arc<ModuleDeclarations>)>>> =
        OnceLock::new();

    if let Some(declarations) = SOURCE_OVERRIDES.with(|cell| {
        let mut overrides = cell.borrow_mut();
        let (source, parsed) = overrides.get_mut(parent)?;
        Some(Arc::clone(parsed.get_or_insert_with(|| {
            Arc::new(parse_module_declarations(source))
        })))
    }) {
        return Some(declarations);
    }

    let metadata = std::fs::metadata(parent).ok()?;
    let stamp = (metadata.modified().ok()?, metadata.len());
    let cache = CACHE.get_or_init(Default::default);
    let lock = || {
        cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    };
    if let Some((cached, declarations)) = lock().get(parent) {
        if *cached == stamp {
            return Some(Arc::clone(declarations));
        }
    }
    let declarations = Arc::new(parse_module_declarations(
        &std::fs::read_to_string(parent).ok()?,
    ));
    lock().insert(parent.to_path_buf(), (stamp, Arc::clone(&declarations)));
    Some(declarations)
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
        .map(|diag| {
            seen.insert((
                diag.path.as_path(),
                diag.line,
                diag.column,
                diag.code,
                diag.message.as_str(),
            ))
        })
        .collect();
    drop(seen);
    let mut keep = keep.into_iter();
    diagnostics.retain(|_| keep.next().unwrap_or(true));
}
