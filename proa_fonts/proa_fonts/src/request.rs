// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The declaration a `google_font!` / `local_font!` call represents, plus the
//! stable id derived from it.
//!
//! This type is the contract shared by two sides that never see each other's
//! code: the proc macro (which expands a call site to a reference to a
//! generated const) and the build-time collector (which scans source for those
//! calls, vendors the fonts, and emits the consts). Both construct a
//! [`FontRequest`] from the same tokens and call [`FontRequest::const_ident`];
//! because the id is a pure function of the normalized declaration, the
//! reference and the definition always agree.
//!
//! The id is derived from the *declaration*, not the font bytes (which do not
//! exist at macro-expansion time). The generated [`Font`](crate::Font)'s own
//! class name is derived separately from the vended bytes.

use crate::FontStyle;

/// Where a declared font comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSource {
    /// Fetched from Google Fonts by family name.
    Google,
    /// Vended from local files listed in [`FontRequest::files`].
    Local,
}

impl RequestSource {
    const fn as_str(self) -> &'static str {
        match self {
            RequestSource::Google => "google",
            RequestSource::Local => "local",
        }
    }
}

/// Generic category, selecting the metric-matched fallback and CSS suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Generic {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl Generic {
    /// The CSS generic keyword ending the family stack.
    pub const fn css_generic(self) -> &'static str {
        match self {
            Generic::Sans => "sans-serif",
            Generic::Serif => "serif",
            Generic::Mono => "monospace",
        }
    }

    /// Stable lowercase token used in the id and in config.
    pub const fn as_str(self) -> &'static str {
        match self {
            Generic::Sans => "sans",
            Generic::Serif => "serif",
            Generic::Mono => "mono",
        }
    }
}

/// A font declaration, as written at a `google_font!` / `local_font!` call
/// site. Identity is fully determined by its normalized fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRequest {
    pub source: RequestSource,
    pub family: String,
    pub generic: Generic,
    pub subsets: Vec<String>,
    pub weights: Vec<u16>,
    pub styles: Vec<FontStyle>,
    pub variable_font: bool,
    pub variable: Option<String>,
    pub files: Vec<String>,
    pub display: Option<String>,
    pub fallback_local: Option<String>,
    pub extra_stack: Vec<String>,
    /// Face tails to `<link rel=preload>`, e.g. `["latin-400-normal"]`. Empty
    /// selects the default: for Google sources the primary subset's upright
    /// 400 (or upright variable) face, for local files the upright faces.
    pub preload: Vec<String>,
    /// SPDX or free-text license recorded in the lock, e.g. `"OFL-1.1"`.
    pub license: Option<String>,
}

impl FontRequest {
    /// A minimal Google request: one family, latin, weight 400, normal.
    pub fn google(family: impl Into<String>) -> Self {
        Self {
            source: RequestSource::Google,
            family: family.into(),
            generic: Generic::Sans,
            subsets: Vec::new(),
            weights: Vec::new(),
            styles: Vec::new(),
            variable_font: false,
            variable: None,
            files: Vec::new(),
            display: None,
            fallback_local: None,
            extra_stack: Vec::new(),
            preload: Vec::new(),
            license: None,
        }
    }

    /// A local request from one or more files.
    pub fn local(family: impl Into<String>, files: Vec<String>) -> Self {
        Self {
            source: RequestSource::Local,
            files,
            ..Self::google(family)
        }
    }

    /// A canonical, normalized string uniquely describing this declaration.
    /// Order-insensitive fields are sorted so cosmetic reordering at the call
    /// site does not change the id.
    pub fn canonical_key(&self) -> String {
        const US: char = '\u{1f}'; // unit separator, cannot appear in the inputs
        let mut subsets: Vec<String> = self
            .subsets
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        subsets.sort();
        subsets.dedup();

        let mut weights = self.weights.clone();
        weights.sort_unstable();
        weights.dedup();

        let mut styles: Vec<&str> = self
            .styles
            .iter()
            .map(|s| match s {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
            })
            .collect();
        styles.sort_unstable();
        styles.dedup();

        // `files` order is meaningful: it drives face order, CSS order, and
        // the primary-face tie-break, so it is NOT sorted; reordering the
        // list is a declaration change.
        let files = self.files.clone();

        let mut extra = self.extra_stack.clone();
        // extra_stack order is meaningful (it is a CSS fallback order), so it is
        // NOT sorted; join as-is.
        let _ = &mut extra;

        let mut key = String::new();
        key.push_str(self.source.as_str());
        key.push(US);
        key.push_str(self.family.trim());
        key.push(US);
        key.push_str(self.generic.as_str());
        key.push(US);
        key.push_str(&subsets.join(","));
        key.push(US);
        key.push_str(
            &weights
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        key.push(US);
        key.push_str(&styles.join(","));
        key.push(US);
        key.push(if self.variable_font { '1' } else { '0' });
        key.push(US);
        key.push_str(self.variable.as_deref().unwrap_or(""));
        key.push(US);
        key.push_str(&files.join(","));
        key.push(US);
        key.push_str(self.display.as_deref().unwrap_or(""));
        key.push(US);
        key.push_str(self.fallback_local.as_deref().unwrap_or(""));
        key.push(US);
        key.push_str(&extra.join(","));

        let mut preload = self.preload.clone();
        preload.sort();
        preload.dedup();
        key.push(US);
        key.push_str(&preload.join(","));
        key.push(US);
        key.push_str(self.license.as_deref().unwrap_or(""));
        key
    }

    /// A stable 8-hex id for this declaration. Identical on both the macro and
    /// collector sides for the same declaration.
    pub fn request_id(&self) -> String {
        let hash = fnv1a64(self.canonical_key().as_bytes());
        format!("{:08x}", (hash & 0xffff_ffff) as u32)
    }

    /// The generated const identifier both sides use, e.g. `FONT_9F2C1A3B`.
    /// Upper-cased so it satisfies Rust's `non_upper_case_globals` lint.
    pub fn const_ident(&self) -> String {
        format!("FONT_{}", self.request_id().to_ascii_uppercase())
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic() {
        let a = FontRequest::google("Geist");
        let b = FontRequest::google("Geist");
        assert_eq!(a.request_id(), b.request_id());
        assert_eq!(
            a.const_ident(),
            format!("FONT_{}", a.request_id().to_ascii_uppercase())
        );
    }

    #[test]
    fn id_is_insensitive_to_cosmetic_order() {
        let mut a = FontRequest::google("Geist");
        a.subsets = vec!["latin".into(), "latin-ext".into()];
        a.weights = vec![700, 400];
        a.styles = vec![FontStyle::Italic, FontStyle::Normal];

        let mut b = FontRequest::google("Geist");
        b.subsets = vec!["latin-ext".into(), "latin".into()];
        b.weights = vec![400, 700, 400];
        b.styles = vec![FontStyle::Normal, FontStyle::Italic];

        assert_eq!(a.request_id(), b.request_id());
    }

    #[test]
    fn id_changes_with_meaningful_fields() {
        let base = FontRequest::google("Geist");
        let mut diff_family = base.clone();
        diff_family.family = "Geist Mono".into();
        let mut diff_weight = base.clone();
        diff_weight.weights = vec![700];
        let mut diff_var = base.clone();
        diff_var.variable = Some("--font-sans".into());
        let mut diff_extra = base.clone();
        diff_extra.extra_stack = vec!["system-ui".into()];

        let ids = [
            base.request_id(),
            diff_family.request_id(),
            diff_weight.request_id(),
            diff_var.request_id(),
            diff_extra.request_id(),
        ];
        // All distinct.
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j], "ids {i} and {j} collided");
            }
        }
    }

    #[test]
    fn files_order_is_significant() {
        let mut a = FontRequest::local("Brand", vec!["a.woff2".into(), "b.woff2".into()]);
        let mut b = FontRequest::local("Brand", vec!["b.woff2".into(), "a.woff2".into()]);
        a.variable = Some("--font-brand".into());
        b.variable = Some("--font-brand".into());
        assert_ne!(a.request_id(), b.request_id());
    }

    #[test]
    fn extra_stack_order_is_significant() {
        let mut a = FontRequest::google("Geist");
        a.extra_stack = vec!["system-ui".into(), "Arial".into()];
        let mut b = FontRequest::google("Geist");
        b.extra_stack = vec!["Arial".into(), "system-ui".into()];
        assert_ne!(a.request_id(), b.request_id());
    }

    #[test]
    fn const_ident_is_a_valid_rust_identifier() {
        let id = FontRequest::google("Geist").const_ident();
        assert!(id.starts_with("FONT_"));
        assert!(id[5..].chars().all(|c| c.is_ascii_hexdigit()));
    }
}
