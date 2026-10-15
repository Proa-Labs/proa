// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Disposable memoization of the syntax highlighter, never of page compilation.

use base16ct::HexDisplay;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::syntax_highlight::HighlightedLine;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) struct HighlightCache {
    root: PathBuf,
}

#[derive(Deserialize, Serialize)]
struct Entry {
    key: String,
    lines: Option<Vec<HighlightedLine>>,
}

impl HighlightCache {
    pub(crate) fn new(root: &Path) -> Option<Self> {
        // Build scripts statically contain the highlighter and its grammars.
        // Hash their actual executable, not a package version or timestamp:
        // editing compiler code or changing dependencies must invalidate hits
        // even when Cargo reuses the same OUT_DIR and package version.
        let executable = File::open(std::env::current_exe().ok()?).ok()?;
        Self::for_compiler(root, executable).ok()
    }

    fn for_compiler(root: &Path, mut compiler: impl Read) -> io::Result<Self> {
        let mut hash = Sha256::new();
        hash.update(b"proa.docs.highlight-cache.v1\0");
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = compiler.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let root = root.join(format!("{:x}", HexDisplay(&hash.finalize())));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub(crate) fn get_or_compute(
        &self,
        code: &str,
        language: &str,
        compute: impl FnOnce() -> Result<Option<Vec<HighlightedLine>>, syntect::Error>,
    ) -> Result<Option<Vec<HighlightedLine>>, syntect::Error> {
        let key = Self::key(code, language);
        let path = self.root.join(format!("{key}.json"));
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(entry) = serde_json::from_slice::<Entry>(&bytes) {
                if entry.key == key {
                    return Ok(entry.lines);
                }
            }
        }

        // Parse failures retain the normal warning/fallback behavior and are
        // retried on the next build. Cache I/O can never make a build fail.
        let lines = compute()?;
        let entry = Entry { key, lines };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            self.store(&path, &bytes);
        }
        Ok(entry.lines)
    }

    fn key(code: &str, language: &str) -> String {
        let mut hash = Sha256::new();
        hash.update((language.len() as u64).to_le_bytes());
        hash.update(language.as_bytes());
        hash.update(code.as_bytes());
        format!("{:x}", HexDisplay(&hash.finalize()))
    }

    fn store(&self, path: &Path, bytes: &[u8]) {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = self
            .root
            .join(format!(".{}-{nonce}.tmp", std::process::id()));
        let Ok(mut file) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        else {
            // A collision does not give us ownership of an existing file.
            return;
        };
        let written = file.write_all(bytes);
        drop(file);
        if written.and_then(|()| fs::rename(&temporary, path)).is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(html: &str) -> Option<Vec<HighlightedLine>> {
        Some(vec![HighlightedLine {
            html: html.to_string(),
            ending: "\r\n".to_string(),
        }])
    }

    #[test]
    fn persisted_hits_skip_highlighting_and_preserve_line_endings() {
        let temp = tempfile::tempdir().unwrap();
        let cache = HighlightCache::for_compiler(temp.path(), &b"compiler-a"[..]).unwrap();
        let expected = lines("<span>escaped &lt;code&gt;</span>");
        assert_eq!(
            cache
                .get_or_compute("code", "rust", || Ok(expected.clone()))
                .unwrap(),
            expected
        );

        let reopened = HighlightCache::for_compiler(temp.path(), &b"compiler-a"[..]).unwrap();
        assert_eq!(
            reopened
                .get_or_compute("code", "rust", || panic!("cache miss"))
                .unwrap(),
            expected
        );
    }

    #[test]
    fn code_language_and_compiler_changes_invalidate_entries() {
        let temp = tempfile::tempdir().unwrap();
        let cache = HighlightCache::for_compiler(temp.path(), &b"compiler-a"[..]).unwrap();
        cache
            .get_or_compute("old", "rust", || Ok(lines("old")))
            .unwrap();
        assert_eq!(
            cache
                .get_or_compute("new", "rust", || Ok(lines("new")))
                .unwrap(),
            lines("new")
        );
        assert_eq!(
            cache.get_or_compute("old", "text", || Ok(None)).unwrap(),
            None
        );
        let rebuilt = HighlightCache::for_compiler(temp.path(), &b"compiler-b"[..]).unwrap();
        assert_eq!(
            rebuilt
                .get_or_compute("old", "rust", || Ok(lines("rebuilt")))
                .unwrap(),
            lines("rebuilt")
        );
    }

    #[test]
    fn corrupt_or_misplaced_entries_are_recomputed() {
        let temp = tempfile::tempdir().unwrap();
        let cache = HighlightCache::for_compiler(temp.path(), &b"compiler"[..]).unwrap();
        let path = cache
            .root
            .join(format!("{}.json", HighlightCache::key("code", "rust")));
        fs::write(&path, "{truncated").unwrap();
        assert_eq!(
            cache
                .get_or_compute("code", "rust", || Ok(lines("fresh")))
                .unwrap(),
            lines("fresh")
        );
        fs::write(&path, br#"{"key":"different-input","lines":null}"#).unwrap();
        assert_eq!(
            cache
                .get_or_compute("code", "rust", || Ok(lines("fresh")))
                .unwrap(),
            lines("fresh")
        );
    }

    #[test]
    fn unavailable_cache_does_not_change_the_computed_result() {
        let temp = tempfile::tempdir().unwrap();
        let cache = HighlightCache::for_compiler(temp.path(), &b"compiler"[..]).unwrap();
        fs::remove_dir(&cache.root).unwrap();
        fs::write(&cache.root, "not a directory").unwrap();
        assert_eq!(
            cache
                .get_or_compute("code", "rust", || Ok(lines("fresh")))
                .unwrap(),
            lines("fresh")
        );
    }

    #[test]
    fn concurrent_publishers_leave_a_complete_cache_entry() {
        let temp = tempfile::tempdir().unwrap();
        let cache = std::sync::Arc::new(
            HighlightCache::for_compiler(temp.path(), &b"compiler"[..]).unwrap(),
        );
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    assert_eq!(
                        cache
                            .get_or_compute("code", "rust", || Ok(lines("fresh")))
                            .unwrap(),
                        lines("fresh")
                    );
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            cache
                .get_or_compute("code", "rust", || panic!("cache miss"))
                .unwrap(),
            lines("fresh")
        );
    }
}
