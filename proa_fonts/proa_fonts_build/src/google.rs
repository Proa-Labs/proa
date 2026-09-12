// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Google Fonts `css2` resolution: URL building, response parsing, and (behind
//! the `fetch` feature) the blocking fetcher the collector uses for
//! `google_font!` declarations.
//!
//! The parsing half is pure and always compiled; only [`resolve`] performs
//! network I/O. Google serves already-subset WOFF2 per `unicode-range`, so a
//! subset selection chooses which files to vend, not a glyph-subsetting step.

use proa_fonts::request::FontRequest;
use proa_fonts::{FontStyle, FontWeight};

use crate::error::BuildError;
use crate::FaceInput;

/// Browser-like UA so Google Fonts serves WOFF2 (and not TTF for old agents).
pub const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// One `@font-face` block parsed from Google's CSS response.
#[derive(Debug, Clone, PartialEq)]
pub struct GoogleFace {
    pub subset: String,
    pub style: FontStyle,
    pub weight: FontWeight,
    pub url: String,
    pub unicode_range: String,
}

/// Fetch and resolve a Google declaration into the face inputs to vend.
#[cfg(feature = "fetch")]
pub fn resolve(req: &FontRequest) -> Result<Vec<FaceInput>, BuildError> {
    let url = css_url(req);
    let client = reqwest::blocking::Client::builder()
        .user_agent(BROWSER_UA)
        .build()
        .map_err(|e| BuildError::Fetch(e.to_string()))?;
    let css = client
        .get(&url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::text)
        .map_err(|e| BuildError::Fetch(format!("{url}: {e}")))?;

    let faces = select_faces(req, &css)?;

    let mut out = Vec::with_capacity(faces.len());
    for face in faces {
        let bytes = client
            .get(&face.url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| BuildError::Fetch(format!("{}: {e}", face.url)))?
            .bytes()
            .map_err(|e| BuildError::Fetch(format!("{}: {e}", face.url)))?
            .to_vec();
        out.push(face_input(req, &face, bytes));
    }
    Ok(out)
}

/// Filter the parsed CSS down to the faces the declaration selects.
pub fn select_faces(req: &FontRequest, css: &str) -> Result<Vec<GoogleFace>, BuildError> {
    let mut faces = parse_google_css(css);
    let wanted = wanted_subsets(req);
    faces.retain(|f| wanted.iter().any(|w| w == &f.subset));
    if faces.is_empty() {
        return Err(BuildError::Fetch(format!(
            "no faces matched subsets {wanted:?} in the Google Fonts response for `{}`",
            req.family
        )));
    }
    Ok(faces)
}

/// Build the [`FaceInput`] for one fetched face.
pub fn face_input(req: &FontRequest, face: &GoogleFace, bytes: Vec<u8>) -> FaceInput {
    let slug = crate::family_slug(&req.family, '-');
    let stem = format!(
        "{slug}-{}-{}-{}",
        face.subset,
        weight_label(face.weight),
        style_label(face.style)
    );
    FaceInput {
        bytes,
        weight: face.weight,
        style: face.style,
        display: crate::parse_display(req.display.as_deref()),
        unicode_range: Some(face.unicode_range.clone()),
        preload: should_preload(req, face),
        stem,
    }
}

/// Whether a face gets a `<link rel=preload>`: an explicit `preload = [...]`
/// list matches against `<subset>-<weight>-<style>` tails; the default is the
/// primary subset's upright 400 (or upright variable) face.
pub fn should_preload(req: &FontRequest, face: &GoogleFace) -> bool {
    if !req.preload.is_empty() {
        let tail = format!(
            "{}-{}-{}",
            face.subset,
            weight_label(face.weight),
            style_label(face.style)
        );
        return req
            .preload
            .iter()
            .any(|p| p == &tail || tail.contains(p.as_str()));
    }
    let wanted = wanted_subsets(req);
    let primary_subset = wanted.first().map_or("latin", String::as_str);
    face.subset == primary_subset
        && face.style == FontStyle::Normal
        && matches!(
            face.weight,
            FontWeight::Fixed(400) | FontWeight::Variable { .. }
        )
}

/// The subsets a declaration selects; defaults to latin.
pub fn wanted_subsets(req: &FontRequest) -> Vec<String> {
    if req.subsets.is_empty() {
        vec!["latin".to_string()]
    } else {
        req.subsets.clone()
    }
}

/// Build the Google Fonts `css2` request URL for a declaration.
pub fn css_url(req: &FontRequest) -> String {
    let family = req.family.replace(' ', "+");
    let italic = req.styles.contains(&FontStyle::Italic);
    let mut weights = req.weights.clone();
    if weights.is_empty() {
        weights.push(400);
    }
    weights.sort_unstable();
    weights.dedup();

    let axis = if req.variable_font {
        let range = "100..900";
        if italic {
            format!(":ital,wght@0,{range};1,{range}")
        } else {
            format!(":wght@{range}")
        }
    } else if italic {
        let tuples: Vec<String> = weights
            .iter()
            .flat_map(|w| [format!("0,{w}"), format!("1,{w}")])
            .collect();
        format!(":ital,wght@{}", tuples.join(";"))
    } else {
        let list: Vec<String> = weights.iter().map(u16::to_string).collect();
        format!(":wght@{}", list.join(";"))
    };

    let display = req.display.as_deref().unwrap_or("swap");
    format!("https://fonts.googleapis.com/css2?family={family}{axis}&display={display}")
}

/// Parse the `@font-face` blocks out of a Google Fonts CSS response, tagging
/// each with the subset named in the `/* subset */` comment before it.
pub fn parse_google_css(css: &str) -> Vec<GoogleFace> {
    let bytes = css.as_bytes();
    let mut faces = Vec::new();
    let mut subset = String::from("latin");
    let mut i = 0;
    while i < bytes.len() {
        if css[i..].starts_with("/*") {
            if let Some(end) = css[i + 2..].find("*/") {
                subset = css[i + 2..i + 2 + end].trim().to_string();
                i += 2 + end + 2;
                continue;
            }
        }
        if css[i..].starts_with("@font-face") {
            if let Some(open) = css[i..].find('{') {
                let block_start = i + open + 1;
                if let Some(close) = css[block_start..].find('}') {
                    let block = &css[block_start..block_start + close];
                    if let Some(face) = parse_font_face_block(block, &subset) {
                        faces.push(face);
                    }
                    i = block_start + close + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    faces
}

fn parse_font_face_block(block: &str, subset: &str) -> Option<GoogleFace> {
    let mut style = FontStyle::Normal;
    let mut weight = None;
    let mut url = None;
    let mut unicode_range = None;
    for decl in block.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "font-style"
                if v.eq_ignore_ascii_case("italic") || v.eq_ignore_ascii_case("oblique") =>
            {
                style = FontStyle::Italic;
            }
            "font-weight" => weight = parse_weight(v),
            "src" => url = extract_url(v),
            "unicode-range" => unicode_range = Some(v.to_string()),
            _ => {}
        }
    }
    Some(GoogleFace {
        subset: subset.to_string(),
        style,
        weight: weight?,
        url: url?,
        unicode_range: unicode_range.unwrap_or_default(),
    })
}

fn parse_weight(v: &str) -> Option<FontWeight> {
    let parts: Vec<&str> = v.split_whitespace().collect();
    match parts.as_slice() {
        [single] => single.parse::<u16>().ok().map(FontWeight::Fixed),
        [min, max] => Some(FontWeight::Variable {
            min: min.parse().ok()?,
            max: max.parse().ok()?,
        }),
        _ => None,
    }
}

fn extract_url(src: &str) -> Option<String> {
    let start = src.find("url(")? + 4;
    let end = src[start..].find(')')? + start;
    let raw = src[start..end]
        .trim()
        .trim_matches(|c| c == '\'' || c == '"');
    Some(raw.to_string())
}

/// Stable weight token used in stems and the lock, e.g. `400` or `variable`.
pub fn weight_label(weight: FontWeight) -> String {
    match weight {
        FontWeight::Fixed(w) => w.to_string(),
        FontWeight::Variable { .. } => "variable".to_string(),
    }
}

/// Stable style token used in stems and the lock.
pub fn style_label(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CSS: &str = "\
/* cyrillic */
@font-face {
  font-family: 'Geist';
  font-style: normal;
  font-weight: 400;
  font-display: swap;
  src: url(https://fonts.gstatic.com/s/geist/v1/cyrillic-400.woff2) format('woff2');
  unicode-range: U+0301, U+0400-045F;
}
/* latin */
@font-face {
  font-family: 'Geist';
  font-style: normal;
  font-weight: 400;
  font-display: swap;
  src: url(https://fonts.gstatic.com/s/geist/v1/latin-400.woff2) format('woff2');
  unicode-range: U+0000-00FF, U+0131;
}
/* latin */
@font-face {
  font-family: 'Geist';
  font-style: italic;
  font-weight: 100 900;
  src: url(https://fonts.gstatic.com/s/geist/v1/latin-var-italic.woff2) format('woff2');
  unicode-range: U+0000-00FF;
}
";

    #[test]
    fn parses_google_css_blocks_with_subsets() {
        let faces = parse_google_css(SAMPLE_CSS);
        assert_eq!(faces.len(), 3);

        assert_eq!(faces[0].subset, "cyrillic");
        assert_eq!(faces[0].style, FontStyle::Normal);
        assert_eq!(faces[0].weight, FontWeight::Fixed(400));

        assert_eq!(faces[1].subset, "latin");
        assert_eq!(
            faces[1].url,
            "https://fonts.gstatic.com/s/geist/v1/latin-400.woff2"
        );
        assert_eq!(faces[1].unicode_range, "U+0000-00FF, U+0131");

        assert_eq!(faces[2].style, FontStyle::Italic);
        assert_eq!(faces[2].weight, FontWeight::Variable { min: 100, max: 900 });
    }

    #[test]
    fn css_url_encodes_axis_variants() {
        let mut req = FontRequest::google("Geist Mono");
        req.weights = vec![700, 400, 400];
        assert_eq!(
            css_url(&req),
            "https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400;700&display=swap"
        );

        req.styles = vec![FontStyle::Normal, FontStyle::Italic];
        assert_eq!(
            css_url(&req),
            "https://fonts.googleapis.com/css2?family=Geist+Mono:ital,wght@0,400;1,400;0,700;1,700&display=swap"
        );

        req.variable_font = true;
        req.styles = vec![FontStyle::Normal];
        assert_eq!(
            css_url(&req),
            "https://fonts.googleapis.com/css2?family=Geist+Mono:wght@100..900&display=swap"
        );
    }

    #[test]
    fn extract_url_strips_wrapper_and_quotes() {
        assert_eq!(
            extract_url("url(https://x/y.woff2) format('woff2')").as_deref(),
            Some("https://x/y.woff2")
        );
        assert_eq!(
            extract_url("url('https://x/y.woff2')").as_deref(),
            Some("https://x/y.woff2")
        );
    }

    #[test]
    fn explicit_preload_tails_override_the_default() {
        let face = |subset: &str, weight, style| GoogleFace {
            subset: subset.to_string(),
            style,
            weight,
            url: String::new(),
            unicode_range: String::new(),
        };

        let mut req = FontRequest::google("Geist");
        req.subsets = vec!["latin".into(), "cyrillic".into()];
        // Default: primary subset, upright 400 only.
        assert!(should_preload(
            &req,
            &face("latin", FontWeight::Fixed(400), FontStyle::Normal)
        ));
        assert!(!should_preload(
            &req,
            &face("cyrillic", FontWeight::Fixed(400), FontStyle::Normal)
        ));
        assert!(!should_preload(
            &req,
            &face("latin", FontWeight::Fixed(700), FontStyle::Normal)
        ));

        req.preload = vec!["cyrillic-700-normal".into()];
        assert!(should_preload(
            &req,
            &face("cyrillic", FontWeight::Fixed(700), FontStyle::Normal)
        ));
        assert!(!should_preload(
            &req,
            &face("latin", FontWeight::Fixed(400), FontStyle::Normal)
        ));
    }
}
