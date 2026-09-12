// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `proa-fonts.lock` schema.
//!
//! Font declarations live in code (`google_font!` / `local_font!`); the lock
//! records the resolved, content-hashed result of vendoring them so rebuilds
//! are reproducible and offline: checksums, byte sizes, per-face metadata
//! sufficient to regenerate the `Font` const from the vendored bytes, computed
//! metrics, and the fallback overrides. The collector
//! ([`crate::collect`]) reads it to skip fetching for unchanged declarations
//! and rewrites it when declarations change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Top-level `proa-fonts.lock`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FontsLock {
    #[serde(default)]
    pub fonts: BTreeMap<String, LockEntry>,
}

/// The resolved, reproducible result for one declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockEntry {
    pub family: String,
    /// The declaration fingerprint this entry satisfies
    /// ([`proa_fonts::request::FontRequest::request_id`]). An entry whose id no
    /// longer matches any scanned declaration is stale and gets cleaned up.
    #[serde(default)]
    pub request_id: String,
    /// The generated const identifier, e.g. `"FONT_9F2C1A3B"`.
    pub const_name: String,
    pub class_name: String,
    pub css_variable: String,
    pub family_stack: String,
    pub files: Vec<LockFile>,
    pub metrics: LockMetrics,
    pub fallback: LockFallback,
    #[serde(default)]
    pub license: Option<String>,
}

/// One vended file's integrity record plus the face metadata needed to
/// regenerate its `@font-face` from the vendored bytes without refetching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFile {
    pub filename: String,
    pub url: String,
    /// Full SHA-256 of the vended bytes.
    pub sha256: String,
    pub bytes: u64,
    /// Filename stem before the content hash, e.g. `"geist-latin-400-normal"`.
    #[serde(default)]
    pub stem: String,
    /// `"400"`-style fixed weight or `"min..max"` for a variable face.
    #[serde(default)]
    pub weight: String,
    /// `"normal"` or `"italic"`.
    #[serde(default)]
    pub style: String,
    /// `font-display` keyword.
    #[serde(default)]
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unicode_range: Option<String>,
    #[serde(default)]
    pub preload: bool,
}

/// The metrics parsed from the primary face, recorded for auditability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockMetrics {
    pub units_per_em: u16,
    pub ascent: i32,
    pub descent: i32,
    pub line_gap: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_width_avg: Option<f64>,
}

/// The synthesized fallback overrides, recorded for auditability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFallback {
    pub local: String,
    pub ascent_override: String,
    pub descent_override: String,
    pub line_gap_override: String,
    pub size_adjust: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_round_trips_through_toml() {
        let mut fonts = BTreeMap::new();
        fonts.insert(
            "geist_9f2c1a3b".to_string(),
            LockEntry {
                family: "Geist".to_string(),
                request_id: "9f2c1a3b".to_string(),
                const_name: "FONT_9F2C1A3B".to_string(),
                class_name: "__proa_font_geist_9f2c1a3b".to_string(),
                css_variable: "--font-geist".to_string(),
                family_stack: "'Geist','Geist Fallback',system-ui,sans-serif".to_string(),
                files: vec![LockFile {
                    filename: "geist-latin-400-normal.9f2c1a3b.woff2".to_string(),
                    url: "/fonts/geist-latin-400-normal.9f2c1a3b.woff2".to_string(),
                    sha256: "9f2c1a3b".to_string(),
                    bytes: 12345,
                    stem: "geist-latin-400-normal".to_string(),
                    weight: "400".to_string(),
                    style: "normal".to_string(),
                    display: "swap".to_string(),
                    unicode_range: Some("U+0000-00FF".to_string()),
                    preload: true,
                }],
                metrics: LockMetrics {
                    units_per_em: 1000,
                    ascent: 950,
                    descent: -250,
                    line_gap: 0,
                    x_width_avg: Some(512.5),
                },
                fallback: LockFallback {
                    local: "Arial".to_string(),
                    ascent_override: "95.02%".to_string(),
                    descent_override: "24.02%".to_string(),
                    line_gap_override: "0.00%".to_string(),
                    size_adjust: "101.00%".to_string(),
                },
                license: Some("OFL-1.1".to_string()),
            },
        );
        let lock = FontsLock { fonts };

        let serialized = toml::to_string(&lock).unwrap();
        let parsed: FontsLock = toml::from_str(&serialized).unwrap();
        assert_eq!(lock, parsed);
    }

    #[test]
    fn entries_missing_v2_metadata_still_parse() {
        // A pre-metadata lock parses; empty request_id / face metadata marks
        // the entry stale so the collector re-resolves it.
        let legacy = r#"
            [fonts.geist]
            family = "Geist"
            const_name = "GEIST"
            class_name = "__proa_font_geist_9f2c1a3b"
            css_variable = "--font-geist"
            family_stack = "'Geist',sans-serif"

            [[fonts.geist.files]]
            filename = "geist-latin-400-normal.9f2c1a3b.woff2"
            url = "/fonts/geist-latin-400-normal.9f2c1a3b.woff2"
            sha256 = "9f2c1a3b"
            bytes = 12345

            [fonts.geist.metrics]
            units_per_em = 1000
            ascent = 950
            descent = -250
            line_gap = 0

            [fonts.geist.fallback]
            local = "Arial"
            ascent_override = "95.02%"
            descent_override = "24.02%"
            line_gap_override = "0.00%"
            size_adjust = "101.00%"
        "#;
        let lock: FontsLock = toml::from_str(legacy).unwrap();
        let entry = &lock.fonts["geist"];
        assert!(entry.request_id.is_empty());
        assert!(entry.files[0].stem.is_empty());
    }
}
