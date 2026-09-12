// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stylesheet assembly: turn resolved faces and computed fallback overrides
//! into the exact CSS embedded in a generated [`proa_fonts::Font`].
//!
//! Output is deterministic and compact (one rule per line, minimal
//! whitespace). The rendered order is: every real `@font-face`, then the
//! metric-matched fallback `@font-face`, then the direct-application class,
//! then the `:root` custom-property token.

use std::fmt::Write as _;

use proa_fonts::{FontDisplay, FontFormat, FontStyle, FontWeight};

/// A resolved, vended `@font-face` to emit.
pub struct CssFace<'a> {
    pub family: &'a str,
    pub url: &'a str,
    pub format: FontFormat,
    pub weight: FontWeight,
    pub style: FontStyle,
    pub display: FontDisplay,
    pub unicode_range: Option<&'a str>,
}

/// The synthesized metric-matched fallback `@font-face`.
pub struct FallbackFace<'a> {
    /// Synthesized family name, e.g. `"Geist Fallback"`.
    pub family: &'a str,
    /// `local(...)` face the browser already has, e.g. `"Arial"`.
    pub local: &'a str,
    pub ascent_override: &'a str,
    pub descent_override: &'a str,
    pub line_gap_override: &'a str,
    pub size_adjust: &'a str,
}

/// The utility rules that apply the family.
pub struct Utility<'a> {
    /// Class name applied to opt an element into the font directly.
    pub class_name: &'a str,
    /// CSS custom-property name (with leading `--`) defined on `:root`.
    pub variable: &'a str,
    /// Full `font-family` stack both rules resolve to.
    pub family_stack: &'a str,
}

/// Assemble the complete stylesheet string.
pub fn stylesheet(
    faces: &[CssFace<'_>],
    fallback: &FallbackFace<'_>,
    utility: &Utility<'_>,
) -> String {
    let mut css = String::new();

    for face in faces {
        let _ = write!(
            css,
            "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};\
             font-display:{display};src:url({url}) format('{format}')",
            family = face.family,
            style = face.style.css(),
            weight = weight_css(face.weight),
            display = face.display.css(),
            url = face.url,
            format = face.format.css_format(),
        );
        if let Some(range) = face.unicode_range {
            let _ = write!(css, ";unicode-range:{range}");
        }
        css.push_str("}\n");
    }

    let _ = writeln!(
        css,
        "@font-face{{font-family:'{family}';src:local('{local}');\
         ascent-override:{ascent};descent-override:{descent};\
         line-gap-override:{line_gap};size-adjust:{size_adjust}}}",
        family = fallback.family,
        local = fallback.local,
        ascent = fallback.ascent_override,
        descent = fallback.descent_override,
        line_gap = fallback.line_gap_override,
        size_adjust = fallback.size_adjust,
    );

    let _ = write!(
        css,
        ".{class}{{font-family:{stack}}}\n:root{{{var}:{stack}}}",
        class = utility.class_name,
        var = utility.variable,
        stack = utility.family_stack,
    );

    css
}

/// Render a [`FontWeight`] as a CSS `font-weight` value: a single number for a
/// static weight, or a `min max` range for a variable font.
pub fn weight_css(weight: FontWeight) -> String {
    match weight {
        FontWeight::Fixed(w) => w.to_string(),
        FontWeight::Variable { min, max } => format!("{min} {max}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_faces_fallback_class_and_root_token_in_order() {
        let faces = [CssFace {
            family: "Geist",
            url: "/fonts/geist-latin-400-normal.9f2c1a3b.woff2",
            format: FontFormat::Woff2,
            weight: FontWeight::Fixed(400),
            style: FontStyle::Normal,
            display: FontDisplay::Swap,
            unicode_range: Some("U+0000-00FF"),
        }];
        let fallback = FallbackFace {
            family: "Geist Fallback",
            local: "Arial",
            ascent_override: "95.02%",
            descent_override: "24.02%",
            line_gap_override: "0.00%",
            size_adjust: "101.00%",
        };
        let utility = Utility {
            class_name: "__proa_font_geist_9f2c1a3b",
            variable: "--font-geist",
            family_stack: "'Geist','Geist Fallback',system-ui,sans-serif",
        };

        let css = stylesheet(&faces, &fallback, &utility);

        let face_at = css
            .find("@font-face{font-family:'Geist';font-style:normal")
            .unwrap();
        let fallback_at = css.find("@font-face{font-family:'Geist Fallback'").unwrap();
        let class_at = css.find(".__proa_font_geist_9f2c1a3b{").unwrap();
        let root_at = css.find(":root{--font-geist:").unwrap();
        assert!(face_at < fallback_at, "real face precedes fallback");
        assert!(fallback_at < class_at, "fallback precedes utility class");
        assert!(class_at < root_at, "class precedes :root token");

        assert!(
            css.contains("src:url(/fonts/geist-latin-400-normal.9f2c1a3b.woff2) format('woff2')")
        );
        assert!(css.contains("unicode-range:U+0000-00FF}"));
        assert!(css.contains("size-adjust:101.00%}"));
        assert!(css.contains(
            ".__proa_font_geist_9f2c1a3b{font-family:'Geist','Geist Fallback',system-ui,sans-serif}"
        ));
        assert!(css.contains(":root{--font-geist:'Geist','Geist Fallback',system-ui,sans-serif}"));
    }

    #[test]
    fn variable_font_weight_renders_as_range() {
        assert_eq!(weight_css(FontWeight::Fixed(500)), "500");
        assert_eq!(
            weight_css(FontWeight::Variable { min: 100, max: 900 }),
            "100 900"
        );
    }

    #[test]
    fn omits_unicode_range_when_absent() {
        let faces = [CssFace {
            family: "Brand",
            url: "/fonts/brand.abcd1234.woff2",
            format: FontFormat::Woff2,
            weight: FontWeight::Variable { min: 100, max: 900 },
            style: FontStyle::Normal,
            display: FontDisplay::Swap,
            unicode_range: None,
        }];
        let fallback = FallbackFace {
            family: "Brand Fallback",
            local: "Arial",
            ascent_override: "90.00%",
            descent_override: "20.00%",
            line_gap_override: "0.00%",
            size_adjust: "100.00%",
        };
        let utility = Utility {
            class_name: "__proa_font_brand_abcd1234",
            variable: "--font-brand",
            family_stack: "'Brand','Brand Fallback',sans-serif",
        };
        let css = stylesheet(&faces, &fallback, &utility);
        assert!(!css.contains("unicode-range"));
        assert!(css.contains("font-weight:100 900"));
    }
}
