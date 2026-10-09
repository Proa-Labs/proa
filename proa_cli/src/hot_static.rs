// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Explicit opt-in development templates; incompatible changes use Cargo.

use anyhow::{bail, Context, Result};
use proa_dsl::dev_hot::{HotInput, StaticManifest};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
use syn::{spanned::Spanned, visit::Visit};

pub(crate) struct HotState {
    path: PathBuf,
    shapes: BTreeMap<PathBuf, String>,
    staged: Option<PathBuf>,
}

impl HotState {
    pub(crate) fn prepare(root: &Path) -> Result<Self> {
        let (shapes, manifest) = scan(root)?;
        if !root.join("src").is_dir() {
            eprintln!(
                "--hot-static scans `src/` under the dev root, which {} does not have; every edit will use the linked rebuild",
                root.display()
            );
        }
        let directory = root.join("target/proa-hot-static").join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&directory)?;
        let path = directory.join("manifest.json");
        publish(&path, &manifest)?;
        Ok(Self {
            path,
            shapes,
            staged: None,
        })
    }

    pub(crate) fn candidate(&self, root: &Path) -> Result<Self> {
        let (shapes, manifest) = scan(root)?;
        let staged = self.path.with_extension(format!(
            "candidate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::write(&staged, serde_json::to_vec(&manifest)?)?;
        Ok(Self {
            path: self.path.clone(),
            shapes,
            staged: Some(staged),
        })
    }

    pub(crate) fn activate(&mut self) -> Result<()> {
        if let Some(staged) = &self.staged {
            fs::rename(staged, &self.path)?;
        }
        self.staged = None;
        Ok(())
    }

    pub(crate) fn environment(&self) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([(
            "PROA_DEV_HOT_STATIC".into(),
            self.path.as_os_str().to_owned(),
        )])
    }

    pub(crate) fn try_update(&self, root: &Path, changed: &[PathBuf]) -> Result<bool> {
        if changed.is_empty()
            || changed
                .iter()
                .any(|path| !self.shapes.contains_key(&root.join(path)))
        {
            return Ok(false);
        }
        let (shapes, manifest) = scan(root)?;
        if shapes != self.shapes {
            return Ok(false);
        }
        publish(&self.path, &manifest)?;
        Ok(true)
    }
}

impl Drop for HotState {
    fn drop(&mut self) {
        if let Some(staged) = &self.staged {
            let _ = fs::remove_file(staged);
        }
    }
}

fn publish(path: &Path, manifest: &StaticManifest) -> Result<()> {
    let bytes = serde_json::to_vec(manifest)?;
    let temporary = path.with_extension("new");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn scan(root: &Path) -> Result<(BTreeMap<PathBuf, String>, StaticManifest)> {
    let mut shapes = BTreeMap::new();
    let mut manifest = StaticManifest {
        schema: 1,
        ..Default::default()
    };
    let sources = root.join("src");
    // A workspace root (`cargo run -p member`) has no package sources of its
    // own. That is not an error: it has no hot templates, so every edit takes
    // the ordinary Cargo rebuild.
    if !sources.is_dir() {
        return Ok((shapes, manifest));
    }
    for entry in walkdir::WalkDir::new(sources).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension() != Some(OsStr::new("rs")) {
            continue;
        }
        let path = entry.path().to_path_buf();
        let source = fs::read_to_string(&path)?;
        let (shape, templates) = scan_file(&source)
            .with_context(|| format!("scan hot templates in {}", path.display()))?;
        for (id, template) in templates {
            if manifest.templates.insert(id.clone(), template).is_some() {
                bail!("duplicate hot template ID `{id}`");
            }
        }
        shapes.insert(path, shape);
    }
    Ok((shapes, manifest))
}

fn scan_file(
    source: &str,
) -> Result<(String, BTreeMap<String, proa_dsl::dev_hot::StaticTemplate>)> {
    struct Collector<'a> {
        source: &'a str,
        templates: BTreeMap<String, proa_dsl::dev_hot::StaticTemplate>,
        replacements: Vec<(usize, usize, String)>,
        error: Option<anyhow::Error>,
    }
    impl<'a> Visit<'a> for Collector<'a> {
        fn visit_macro(&mut self, node: &'a syn::Macro) {
            let names = node
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>();
            if names.last().map(String::as_str) != Some("html_hot_sync") {
                return;
            }
            if names.len() > 1 && names != ["proa_macros", "html_hot_sync"] {
                return;
            }
            let result = (|| -> Result<()> {
                let input: HotInput = syn::parse2(node.tokens.clone())?;
                let template = proa_dsl::dev_hot::compile(&input).map_err(anyhow::Error::msg)?;
                let map = crate::source::SourceMap::new(self.source);
                let span = node.tokens.span();
                let start = map
                    .byte_for_line_column(span.start().line, span.start().column)
                    .context("missing hot template start")?;
                let end = map
                    .byte_for_line_column(span.end().line, span.end().column)
                    .context("missing hot template end")?;
                self.replacements.push((
                    start,
                    end,
                    serde_json::to_string(&(input.id.value(), &template.shape))?,
                ));
                if self.templates.insert(input.id.value(), template).is_some() {
                    bail!("duplicate hot template ID");
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.error = Some(error);
            }
        }
    }
    let file = syn::parse_file(source)?;
    let mut collector = Collector {
        source,
        templates: BTreeMap::new(),
        replacements: Vec::new(),
        error: None,
    };
    collector.visit_file(&file);
    if let Some(error) = collector.error {
        return Err(error);
    }
    collector
        .replacements
        .sort_by_key(|replacement| replacement.0);
    let mut shape = String::new();
    let mut position = 0;
    for (start, end, replacement) in collector.replacements {
        shape.push_str(&source[position..start]);
        shape.push_str(&replacement);
        position = end;
    }
    shape.push_str(&source[position..]);
    Ok((shape, collector.templates))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_without_sources_has_no_hot_templates_instead_of_failing() {
        let temp = tempfile::tempdir().unwrap();
        let state = HotState::prepare(temp.path()).unwrap();
        assert!(state.shapes.is_empty());
        assert!(!state
            .try_update(temp.path(), &["member/src/main.rs".into()])
            .unwrap());
        assert!(state.candidate(temp.path()).unwrap().shapes.is_empty());
    }

    #[test]
    fn static_run_layout_changes_require_a_real_rebuild() {
        let source = r#"fn page() { html_hot_sync! { "page"; <p>{self.first}"middle"{self.second}""{self.third}</p> }; }"#;
        let (shape, templates) = scan_file(source).unwrap();
        let moved = source
            .replace("\"middle\"", "\"\"")
            .replace("{self.second}\"\"", "{self.second}\"moved\"");
        let (moved_shape, moved_templates) = scan_file(&moved).unwrap();
        // An equal count does not guarantee that slots occur between the
        // same Rust expressions. Publishing this as hot data would put text
        // before the wrong child in the already-compiled renderer.
        assert_eq!(
            templates["page"].slots.len(),
            moved_templates["page"].slots.len()
        );
        assert_ne!(shape, moved_shape);
        let erased = source.replace("\"middle\"", "\"\"");
        assert_ne!(shape, scan_file(&erased).unwrap().0);

        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        let path = temp.path().join("src/main.rs");
        fs::write(&path, source).unwrap();
        let state = HotState::prepare(temp.path()).unwrap();
        let published = fs::read(&state.path).unwrap();
        for changed in [moved, erased] {
            fs::write(&path, changed).unwrap();
            assert!(!state
                .try_update(temp.path(), &["src/main.rs".into()])
                .unwrap());
            assert_eq!(fs::read(&state.path).unwrap(), published);
        }
    }

    #[test]
    fn static_edits_preserve_shape_but_rust_markup_and_handler_edits_do_not() {
        let source = r#"fn page(cx: &mut Context) { html_hot_sync! { "page"; <p class="a">"Before"{self.value}</p> }.render(cx); }"#;
        let (shape, templates) = scan_file(source).unwrap();
        let (next_shape, next_templates) = scan_file(
            &source
                .replace("Before", "<&After")
                .replace("class=\"a\"", "class=\"b\""),
        )
        .unwrap();
        assert_eq!(shape, next_shape);
        assert_ne!(templates["page"].slots, next_templates["page"].slots);
        assert!(
            String::from_utf8_lossy(&next_templates["page"].slots[0]).contains("&lt;&amp;After")
        );
        for changed in [
            source.replace("self.value", "self.other"),
            source.replace("<p ", "<div ").replace("</p>", "</div>"),
            source.replace(".render(cx)", ".render(other)"),
        ] {
            assert_ne!(shape, scan_file(&changed).unwrap().0);
        }
        assert!(scan_file(
            r#"fn page() { html_hot_sync! { "x"; <a href="javascript:alert(1)">"x"</a> }; }"#
        )
        .is_err());
    }

    #[test]
    fn rust_string_slots_and_same_shape_other_files_require_a_real_rebuild() {
        let source = r#"fn page() { html_hot_sync! { "page"; <p>{text("before")}</p> }; }"#;
        assert_ne!(
            scan_file(source).unwrap().0,
            scan_file(&source.replace("before", "after")).unwrap().0
        );
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        let source_path = temp.path().join("src/main.rs");
        fs::write(&source_path, source).unwrap();
        let state = HotState::prepare(temp.path()).unwrap();
        assert!(!state
            .try_update(temp.path(), &[temp.path().join("Cargo.toml")])
            .unwrap());
        fs::write(&source_path, source.replace("before", "after")).unwrap();
        let published = fs::read(&state.path).unwrap();
        assert!(!state.try_update(temp.path(), &[source_path]).unwrap());
        assert_eq!(fs::read(&state.path).unwrap(), published);
    }
}

#[cfg(test)]
mod watch_path_tests {
    use super::*;
    #[test]
    fn watcher_relative_paths_publish_compatible_updates_atomically() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        let path = temp.path().join("src/main.rs");
        let source = r#"fn page() { html_hot_sync! { "page"; <p>"before"{self.value}</p> }; }"#;
        fs::write(&path, source).unwrap();
        let state = HotState::prepare(temp.path()).unwrap();
        let original = fs::read(&state.path).unwrap();
        fs::write(&path, source.replace("before", "after")).unwrap();
        assert!(state
            .try_update(temp.path(), &["src/main.rs".into()])
            .unwrap());
        assert_ne!(fs::read(&state.path).unwrap(), original);
        fs::write(&path, source.replace("self.value", "self.other")).unwrap();
        assert!(!state
            .try_update(temp.path(), &["src/main.rs".into()])
            .unwrap());
    }
}

#[cfg(test)]
mod generation_tests {
    use super::*;
    #[test]
    fn candidate_builds_reuse_the_compile_environment_and_cannot_publish_before_success() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        let path = temp.path().join("src/main.rs");
        let source = r#"fn page() { html_hot_sync! { "page"; <p>"old"{self.value}</p> }; }"#;
        fs::write(&path, source).unwrap();
        let active = HotState::prepare(temp.path()).unwrap();
        let old_bytes = fs::read(&active.path).unwrap();
        fs::write(
            &path,
            source
                .replace("old", "new")
                .replace("self.value", "self.other"),
        )
        .unwrap();
        let mut candidate = active.candidate(temp.path()).unwrap();
        assert_eq!(active.environment(), candidate.environment());
        assert_eq!(fs::read(&active.path).unwrap(), old_bytes);
        candidate.activate().unwrap();
        assert_ne!(fs::read(&active.path).unwrap(), old_bytes);
    }
}
