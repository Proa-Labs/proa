// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time font pipeline for Proa.
//!
//! Given font bytes (raw sfnt or WOFF2) and a [`FontSpec`], [`generate_font`]:
//! 1. content-hashes each file and names it `{stem}.{hash8}.{ext}`,
//! 2. parses the primary face's metrics ([`metrics`]),
//! 3. synthesizes a metric-matched fallback `@font-face` ([`metrics::compute_fallback`]),
//! 4. assembles the stylesheet ([`css`]), and
//! 5. emits a `const FONT: Font = …` module ([`emit`]) targeting [`proa_fonts`].
//!
//! [`generate_font`] performs no I/O and no network access: callers hand it
//! bytes and write its outputs, keeping the pipeline pure and unit-testable.
//! The build-time entry point is [`collect`]: it scans a crate for
//! `google_font!` / `local_font!` declarations, vendors their faces (fetching
//! Google sources through [`google`] when the default `fetch` feature is on),
//! records the result in `proa-fonts.lock`, and emits the generated consts.
//! Unchanged declarations whose vendored files are intact regenerate from disk
//! with no network at all.

pub mod collect;
pub mod config;
pub mod css;
pub mod emit;
pub mod error;
pub mod google;
pub mod hash;
pub mod metrics;
pub mod woff2;

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

pub use error::BuildError;
pub use metrics::{FallbackMetrics, FallbackOverrides, FontMetrics};
pub use proa_fonts::{FontDisplay, FontFormat, FontStyle, FontWeight};

/// Generic font category, selecting the metric-matched fallback face and the
/// trailing CSS generic keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenericFamily {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl From<proa_fonts::request::Generic> for GenericFamily {
    fn from(g: proa_fonts::request::Generic) -> Self {
        use proa_fonts::request::Generic;
        match g {
            Generic::Sans => GenericFamily::Sans,
            Generic::Serif => GenericFamily::Serif,
            Generic::Mono => GenericFamily::Mono,
        }
    }
}

impl GenericFamily {
    /// Published metrics of this category's canonical local fallback.
    pub fn fallback_metrics(self) -> &'static FallbackMetrics {
        match self {
            GenericFamily::Sans => &metrics::ARIAL,
            GenericFamily::Serif => &metrics::TIMES_NEW_ROMAN,
            GenericFamily::Mono => &metrics::COURIER_NEW,
        }
    }

    /// The CSS generic keyword ending the family stack.
    pub fn css_generic(self) -> &'static str {
        match self {
            GenericFamily::Sans => "sans-serif",
            GenericFamily::Serif => "serif",
            GenericFamily::Mono => "monospace",
        }
    }
}

/// One font file to vend, plus the identity of the `@font-face` it backs.
#[derive(Debug, Clone)]
pub struct FaceInput {
    /// Raw font bytes: sfnt (`.ttf`/`.otf`) or WOFF2.
    pub bytes: Vec<u8>,
    pub weight: FontWeight,
    pub style: FontStyle,
    pub display: FontDisplay,
    /// Optional `unicode-range` descriptor (e.g. a Google subset range).
    pub unicode_range: Option<String>,
    /// Whether to emit a `<link rel=preload>` for this file.
    pub preload: bool,
    /// Filename stem, e.g. `"geist-latin-400-normal"`. The content hash and
    /// extension are appended.
    pub stem: String,
}

/// A font to generate: its identity, its faces, and how to name and stack it.
#[derive(Debug, Clone)]
pub struct FontSpec {
    /// Display / `@font-face` family name, e.g. `"Geist"`.
    pub family: String,
    /// Generic category driving the fallback and CSS suffix.
    pub generic: GenericFamily,
    /// Override the `local(...)` fallback face name (defaults to the generic's).
    pub fallback_local: Option<String>,
    /// Extra families appended after the fallback, before the generic keyword.
    pub extra_stack: Vec<String>,
    /// CSS custom-property name to expose (derived from `family` when `None`).
    pub variable: Option<String>,
    /// Root-absolute URL prefix for vended files, e.g. `"/fonts"`.
    pub url_prefix: String,
    /// The faces to vend. Must be non-empty.
    pub faces: Vec<FaceInput>,
}

/// A resolved, vended face: its hashed filename/URL plus rendering metadata.
#[derive(Debug, Clone)]
pub struct ResolvedFace {
    pub filename: String,
    pub url: String,
    /// Full SHA-256 of the vended bytes, recorded in the lock.
    pub sha256_hex: String,
    pub format: FontFormat,
    pub weight: FontWeight,
    pub style: FontStyle,
    pub display: FontDisplay,
    pub unicode_range: Option<String>,
    pub preload: bool,
    /// The bytes to write to the public fonts directory.
    pub bytes: Vec<u8>,
}

/// The complete result of generating one font.
#[derive(Debug, Clone)]
pub struct GeneratedFont {
    /// `SCREAMING_SNAKE` const name, e.g. `"GEIST"`.
    pub const_name: String,
    /// Lowercase id slug, e.g. `"geist"`.
    pub slug: String,
    pub family: String,
    /// Synthesized fallback family name, e.g. `"Geist Fallback"`.
    pub fallback_family: String,
    pub class_name: String,
    pub variable: String,
    pub family_stack: String,
    pub css: String,
    /// The generated Rust module source.
    pub module_source: String,
    pub faces: Vec<ResolvedFace>,
    pub metrics: FontMetrics,
    pub fallback: FallbackOverrides,
}

/// Run the full generation pipeline for one font.
pub fn generate_font(spec: &FontSpec) -> Result<GeneratedFont, BuildError> {
    if spec.faces.is_empty() {
        return Err(BuildError::NoFaces);
    }

    // 1. Resolve each face: detect its container, hash it, name it.
    let mut resolved = Vec::with_capacity(spec.faces.len());
    for face in &spec.faces {
        let format = detect_format(&face.bytes)?;
        let hash8 = hash::content_hash8(&face.bytes);
        let sha = hash::sha256_hex(&face.bytes);
        let filename = format!("{}.{}.{}", face.stem, hash8, format.extension());
        let url = join_url(&spec.url_prefix, &filename);
        resolved.push(ResolvedFace {
            filename,
            url,
            sha256_hex: sha,
            format,
            weight: face.weight,
            style: face.style,
            display: face.display,
            unicode_range: face.unicode_range.clone(),
            preload: face.preload,
            bytes: face.bytes.clone(),
        });
    }

    // 2. Parse metrics from the primary face (normal style, nearest to 400).
    let primary = &resolved[pick_primary(&resolved)];
    let sfnt = sfnt_bytes(&primary.bytes, primary.format)?;
    let metrics = metrics::parse_sfnt_metrics(&sfnt)?;

    // 3. Synthesize the metric-matched fallback.
    let mut fallback = metrics::compute_fallback(&metrics, spec.generic.fallback_metrics());
    if let Some(local) = &spec.fallback_local {
        fallback.local = local.clone();
    }

    // 4. Names and the family stack.
    let slug_us = slug(&spec.family, '_');
    let slug_dash = slug(&spec.family, '-');
    let const_name = slug_us.to_uppercase();
    let id = hash::short_hash(&naming_seed(&spec.family, &resolved));
    let class_name = format!("__proa_font_{slug_us}_{id}");
    let variable = spec
        .variable
        .clone()
        .unwrap_or_else(|| format!("--font-{slug_dash}"));
    let fallback_family = format!("{} Fallback", spec.family);
    let family_stack = build_stack(
        &spec.family,
        &fallback_family,
        &spec.extra_stack,
        spec.generic,
    );

    // 5. Stylesheet.
    let css_faces: Vec<css::CssFace<'_>> = resolved
        .iter()
        .map(|f| css::CssFace {
            family: &spec.family,
            url: &f.url,
            format: f.format,
            weight: f.weight,
            style: f.style,
            display: f.display,
            unicode_range: f.unicode_range.as_deref(),
        })
        .collect();
    let css_fallback = css::FallbackFace {
        family: &fallback_family,
        local: &fallback.local,
        ascent_override: &fallback.ascent_override,
        descent_override: &fallback.descent_override,
        line_gap_override: &fallback.line_gap_override,
        size_adjust: &fallback.size_adjust,
    };
    let utility = css::Utility {
        class_name: &class_name,
        variable: &variable,
        family_stack: &family_stack,
    };
    let css = css::stylesheet(&css_faces, &css_fallback, &utility);

    // 6. Emit the module.
    let module_source = emit::module(&emit::ModuleView {
        const_name: &const_name,
        family: &spec.family,
        class_name: &class_name,
        variable: &variable,
        family_stack: &family_stack,
        faces: &resolved,
        css: &css,
    });

    Ok(GeneratedFont {
        const_name,
        slug: slug_us,
        family: spec.family.clone(),
        fallback_family,
        class_name,
        variable,
        family_stack,
        css,
        module_source,
        faces: resolved,
        metrics,
        fallback,
    })
}

/// Sniff the font container from its first four bytes.
fn detect_format(bytes: &[u8]) -> Result<FontFormat, BuildError> {
    let sig = bytes.get(0..4).ok_or(BuildError::UnsupportedContainer)?;
    match sig {
        b"wOF2" => Ok(FontFormat::Woff2),
        b"OTTO" => Ok(FontFormat::Opentype),
        b"true" | b"typ1" | b"ttcf" => Ok(FontFormat::Truetype),
        [0x00, 0x01, 0x00, 0x00] => Ok(FontFormat::Truetype),
        _ => Err(BuildError::UnsupportedContainer),
    }
}

/// Obtain sfnt bytes for metric parsing, decoding WOFF2 when needed.
fn sfnt_bytes(bytes: &[u8], format: FontFormat) -> Result<Cow<'_, [u8]>, BuildError> {
    match format {
        FontFormat::Woff2 => Ok(Cow::Owned(woff2::to_sfnt_for_metrics(bytes)?)),
        FontFormat::Truetype | FontFormat::Opentype => Ok(Cow::Borrowed(bytes)),
    }
}

/// Index of the face whose metrics best represent the family: normal style and
/// the weight nearest 400.
fn pick_primary(faces: &[ResolvedFace]) -> usize {
    faces
        .iter()
        .enumerate()
        .min_by_key(|(_, f)| {
            let style_penalty = if f.style == FontStyle::Normal {
                0
            } else {
                1000
            };
            let w = representative_weight(f.weight);
            style_penalty + (w - 400).abs()
        })
        .map_or(0, |(i, _)| i)
}

fn representative_weight(weight: FontWeight) -> i32 {
    match weight {
        FontWeight::Fixed(w) => i32::from(w),
        FontWeight::Variable { min, max } => {
            if (min..=max).contains(&400) {
                400
            } else if 400 < i32::from(min) {
                i32::from(min)
            } else {
                i32::from(max)
            }
        }
    }
}

fn naming_seed(family: &str, faces: &[ResolvedFace]) -> String {
    let mut seed = String::from(family);
    for f in faces {
        seed.push('\0');
        seed.push_str(&f.sha256_hex);
    }
    seed
}

fn build_stack(
    family: &str,
    fallback_family: &str,
    extra: &[String],
    generic: GenericFamily,
) -> String {
    let mut stack = format!("'{family}','{fallback_family}'");
    for e in extra {
        stack.push(',');
        stack.push_str(e);
    }
    stack.push(',');
    stack.push_str(generic.css_generic());
    stack
}

/// A lowercase identifier slug: alphanumerics preserved, every other run
/// collapsed to a single `sep`, with no leading/trailing separators.
pub(crate) fn family_slug(name: &str, sep: char) -> String {
    slug(name, sep)
}

/// Parse a `font-display` keyword, defaulting to `swap`.
pub(crate) fn parse_display(display: Option<&str>) -> FontDisplay {
    match display.map(str::to_ascii_lowercase).as_deref() {
        Some("auto") => FontDisplay::Auto,
        Some("block") => FontDisplay::Block,
        Some("fallback") => FontDisplay::Fallback,
        Some("optional") => FontDisplay::Optional,
        _ => FontDisplay::Swap,
    }
}

fn slug(name: &str, sep: char) -> String {
    let mut out = String::new();
    let mut pending_sep = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push(sep);
            }
            out.push(c.to_ascii_lowercase());
            pending_sep = false;
        } else {
            pending_sep = true;
        }
    }
    out
}

fn join_url(prefix: &str, filename: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ttf_face(
        stem: &str,
        weight: u16,
        style: FontStyle,
        preload: bool,
        bytes: Vec<u8>,
    ) -> FaceInput {
        FaceInput {
            bytes,
            weight: FontWeight::Fixed(weight),
            style,
            display: FontDisplay::Swap,
            unicode_range: None,
            preload,
            stem: stem.to_string(),
        }
    }

    fn system_ttf() -> Option<Vec<u8>> {
        [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Geneva.ttf",
            "/System/Library/Fonts/Monaco.ttf",
        ]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| std::fs::read(p).unwrap())
    }

    #[test]
    fn empty_spec_is_rejected() {
        let spec = FontSpec {
            family: "Geist".into(),
            generic: GenericFamily::Sans,
            fallback_local: None,
            extra_stack: vec![],
            variable: None,
            url_prefix: "/fonts".into(),
            faces: vec![],
        };
        assert!(matches!(generate_font(&spec), Err(BuildError::NoFaces)));
    }

    #[test]
    fn slug_collapses_and_lowercases() {
        assert_eq!(slug("Geist", '_'), "geist");
        assert_eq!(slug("Geist Mono", '_'), "geist_mono");
        assert_eq!(slug("Geist Mono", '-'), "geist-mono");
        assert_eq!(slug("  IBM Plex  Sans ", '-'), "ibm-plex-sans");
    }

    #[test]
    fn join_url_normalizes_slashes() {
        assert_eq!(join_url("/fonts", "a.woff2"), "/fonts/a.woff2");
        assert_eq!(join_url("/fonts/", "a.woff2"), "/fonts/a.woff2");
    }

    #[test]
    fn detects_common_containers() {
        assert_eq!(detect_format(b"wOF2....").unwrap(), FontFormat::Woff2);
        assert_eq!(detect_format(b"OTTO....").unwrap(), FontFormat::Opentype);
        assert_eq!(
            detect_format(&[0, 1, 0, 0, 9]).unwrap(),
            FontFormat::Truetype
        );
        assert!(matches!(
            detect_format(b"wOFF"),
            Err(BuildError::UnsupportedContainer)
        ));
    }

    #[test]
    fn primary_face_prefers_normal_400() {
        let faces = vec![
            ResolvedFace {
                filename: "a".into(),
                url: "a".into(),
                sha256_hex: "1".into(),
                format: FontFormat::Woff2,
                weight: FontWeight::Fixed(700),
                style: FontStyle::Normal,
                display: FontDisplay::Swap,
                unicode_range: None,
                preload: false,
                bytes: vec![],
            },
            ResolvedFace {
                filename: "b".into(),
                url: "b".into(),
                sha256_hex: "2".into(),
                format: FontFormat::Woff2,
                weight: FontWeight::Fixed(400),
                style: FontStyle::Normal,
                display: FontDisplay::Swap,
                unicode_range: None,
                preload: false,
                bytes: vec![],
            },
            ResolvedFace {
                filename: "c".into(),
                url: "c".into(),
                sha256_hex: "3".into(),
                format: FontFormat::Woff2,
                weight: FontWeight::Fixed(400),
                style: FontStyle::Italic,
                display: FontDisplay::Swap,
                unicode_range: None,
                preload: false,
                bytes: vec![],
            },
        ];
        assert_eq!(pick_primary(&faces), 1);
    }

    // End-to-end generation using a real system font (skipped when absent).
    #[test]
    fn generate_font_end_to_end_with_system_font() {
        let Some(bytes) = system_ttf() else {
            eprintln!("no system font; skipping end-to-end generation test");
            return;
        };

        let spec = FontSpec {
            family: "Test Sans".into(),
            generic: GenericFamily::Sans,
            fallback_local: None,
            extra_stack: vec!["system-ui".into()],
            variable: None,
            url_prefix: "/fonts".into(),
            faces: vec![ttf_face(
                "test-sans-400-normal",
                400,
                FontStyle::Normal,
                true,
                bytes,
            )],
        };

        let gen = generate_font(&spec).unwrap();

        assert_eq!(gen.const_name, "TEST_SANS");
        assert_eq!(gen.slug, "test_sans");
        assert_eq!(gen.variable, "--font-test-sans");
        assert!(gen.class_name.starts_with("__proa_font_test_sans_"));
        assert_eq!(gen.faces.len(), 1);
        // Truetype system font is vended as-is.
        assert_eq!(gen.faces[0].format, FontFormat::Truetype);
        assert!(gen.faces[0].filename.ends_with(".ttf"));

        // Module + CSS are internally consistent.
        assert!(gen.module_source.contains("pub const TEST_SANS: Font"));
        assert!(gen.module_source.contains(&gen.class_name));
        assert!(gen.css.contains("@font-face{font-family:'Test Sans'"));
        assert!(gen
            .css
            .contains("@font-face{font-family:'Test Sans Fallback'"));
        assert!(gen.css.contains("size-adjust:"));
        assert!(gen.css.contains(&format!(":root{{{}:", gen.variable)));
        assert!(gen.metrics.units_per_em > 0);
    }
}
