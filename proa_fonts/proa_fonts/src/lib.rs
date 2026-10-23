// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Self-hosted, metric-matched web fonts for Proa.
//!
//! `proa_fonts` is the *runtime* half of Proa's font story. It defines a small
//! set of `const`-constructible descriptor types ([`Font`], [`FontFace`]) and a
//! zero-allocation `<head>` renderer (`Font::head`). The *build* half (asset
//! fetching, content-hashing, fallback-metric computation, and code generation)
//! lives in `proa_fonts_build`: a `build.rs` collector scans the crate for
//! `google_font!` / `local_font!` declarations, vendors the files, and emits
//! `const FONT_<id>: Font` items that the macros expand to references of.
//!
//! # Why this exists
//!
//! The valuable part of Next.js' `next/font` is not the class name: it is that
//! fonts are optimized, **self-hosted**, **preloaded**, and given
//! **metric-compatible fallbacks**, all at build time, with **no runtime
//! request to a third party** and **no JavaScript**. `proa_fonts` brings that
//! to Proa's SSR pipeline.
//!
//! A generated [`Font`] carries:
//! - the vended, content-hashed woff2 URLs ([`FontFace`]),
//! - a pre-rendered stylesheet ([`Font::css`]) containing the real `@font-face`
//!   rules, a synthesized metric-matched fallback `@font-face`, and the utility
//!   classes that apply them, and
//! - the generated identifiers a page assigns: a [class name](Font::class_name),
//!   a [CSS variable](Font::variable), and the full [family stack](Font::family_stack).
//!
//! # Usage in a document layout
//!
//! Assign the font on `<html>` (via its class name or variable) and flush its
//! `<head>` markup where you build the document head:
//!
//! ```ignore
//! use proa_fonts::{Font, google_font};
//!
//! // Declared in code; the build collector vendors it and generates the const
//! // this expands to a reference of:
//! static GEIST: Font =
//!     google_font!("Geist", subsets = [latin], weights = [400], variable = "--font-geist");
//!
//! fn render_head(cx: &mut proa_core::ctx::Ctx) {
//!     GEIST.head(cx).unwrap(); // <link rel=preload …> + <style> @font-face …
//! }
//!
//! // On the <html> element:
//! //   class={GEIST.class_name()}   // sets font-family directly, or
//! //   class={GEIST.variable()}     // sets `--font-geist` for your own CSS
//! ```
//!
//! `Font::head` emits preload hints and a `<style>` block only; it never
//! allocates and is generic over the output buffer, so it composes inside any
//! `WebRenderSync` head. The stylesheet is developer-controlled generated
//! content. Its static segments are preserved, with the request's base path
//! inserted into known face URLs when the application is mounted under a prefix.
//!
//! The default `render` feature enables these rendering methods. Build scripts
//! can disable default features to use descriptors and request
//! IDs without compiling the renderer. The default `macros` feature re-exports
//! `google_font!` and `local_font!`; disable it when only descriptors are needed.

pub mod request;
pub use proa_fonts_types::FontStyle;

/// Declare fonts through the same crate as their runtime descriptors.
#[cfg(feature = "macros")]
pub use proa_fonts_macros::{google_font, local_font};

#[cfg(feature = "render")]
use proa_core::{StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};

/// Web-served font container format. Proa prefers `Woff2` everywhere (it is the
/// smallest broadly-supported format, and the only one Google Fonts serves to
/// modern browsers). Raw sfnt variants exist so a locally-supplied `.ttf`/`.otf`
/// can be vended as-is without a WOFF2 *encoder* in the build pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFormat {
    /// WOFF2 (`font/woff2`, CSS `format("woff2")`).
    Woff2,
    /// Raw TrueType sfnt (`font/ttf`, CSS `format("truetype")`).
    Truetype,
    /// Raw OpenType/CFF sfnt (`font/otf`, CSS `format("opentype")`).
    Opentype,
}

impl FontFormat {
    /// The `type` attribute value for a `<link rel="preload" as="font">` hint.
    #[inline]
    pub const fn mime(self) -> &'static str {
        match self {
            FontFormat::Woff2 => "font/woff2",
            FontFormat::Truetype => "font/ttf",
            FontFormat::Opentype => "font/otf",
        }
    }

    /// The token used inside a CSS `src: url(…) format("…")` descriptor.
    #[inline]
    pub const fn css_format(self) -> &'static str {
        match self {
            FontFormat::Woff2 => "woff2",
            FontFormat::Truetype => "truetype",
            FontFormat::Opentype => "opentype",
        }
    }

    /// The file extension (without dot) Proa vends this format under.
    #[inline]
    pub const fn extension(self) -> &'static str {
        match self {
            FontFormat::Woff2 => "woff2",
            FontFormat::Truetype => "ttf",
            FontFormat::Opentype => "otf",
        }
    }
}

/// Weight coverage of a single `@font-face` file: a fixed weight, or the
/// `[min, max]` range covered by a variable font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// A single static weight, e.g. `Fixed(400)`.
    Fixed(u16),
    /// An inclusive variable-font weight range, e.g. `Variable { min: 100, max: 900 }`.
    Variable { min: u16, max: u16 },
}

/// How a face renders while its font file is still loading. Baked into the
/// generated stylesheet's `font-display` descriptor; retained on the descriptor
/// for introspection and tooling. See the CSS Fonts spec for the swap-period
/// semantics of each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontDisplay {
    Auto,
    Block,
    /// No block period, infinite swap period. The default Proa vends: the
    /// metric-matched fallback shows immediately and swaps with no layout shift.
    #[default]
    Swap,
    Fallback,
    Optional,
}

impl FontDisplay {
    /// The CSS `font-display` value.
    #[inline]
    pub const fn css(self) -> &'static str {
        match self {
            FontDisplay::Auto => "auto",
            FontDisplay::Block => "block",
            FontDisplay::Swap => "swap",
            FontDisplay::Fallback => "fallback",
            FontDisplay::Optional => "optional",
        }
    }
}

/// One self-hosted font file plus the identity of the `@font-face` it backs.
///
/// A face carries only what the runtime needs (a vended URL to preload and its
/// container format) plus stable identity metadata (weight, style) for tooling
/// and revalidation. The full `@font-face` rule (including `font-display` and
/// `unicode-range`) is baked into [`Font::css`] at generation time, so there is
/// a single source of truth for the emitted stylesheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFace {
    /// Root-absolute, content-hashed URL of the vended file, e.g.
    /// `"/fonts/geist-latin-400-normal.9f2c1a3b.woff2"`. Always leading-slash
    /// absolute so it is immune to `<base href>` rewriting.
    pub url: &'static str,
    /// Container format of the vended file.
    pub format: FontFormat,
    /// Weight coverage of this file.
    pub weight: FontWeight,
    /// Style axis of this file.
    pub style: FontStyle,
    /// Whether to emit a `<link rel="preload">` for this file. Preload the
    /// small set of faces above the fold (typically the primary latin normal
    /// weight); preloading every subset/weight wastes bandwidth.
    pub preload: bool,
}

impl FontFace {
    /// A non-preloaded WOFF2 face at a fixed weight and normal style. Handy for
    /// hand-authored fixtures and secondary faces; generated modules construct
    /// [`FontFace`] literally.
    #[inline]
    pub const fn woff2(url: &'static str, weight: u16) -> Self {
        Self {
            url,
            format: FontFormat::Woff2,
            weight: FontWeight::Fixed(weight),
            style: FontStyle::Normal,
            preload: false,
        }
    }

    /// Returns a copy with `preload` set to `true`.
    #[inline]
    pub const fn preloaded(mut self) -> Self {
        self.preload = true;
        self
    }
}

/// A generated, self-hosted font family: its vended files, pre-rendered
/// stylesheet, and the identifiers a page uses to apply it.
///
/// Instances are produced by the build-time collector (`proa_fonts_build`) as
/// `const FONT_<id>` items that `google_font!` / `local_font!` calls reference.
/// Construct them by hand only in tests or fixtures. Every field is `&'static`,
/// so a `Font` is `Copy` and free to reference from any render path.
#[derive(Debug, Clone, Copy)]
pub struct Font {
    /// The primary `@font-face` family name, e.g. `"Geist"`.
    pub family: &'static str,
    /// A stable, collision-resistant class name that applies this font's
    /// `font-family`, e.g. `"__proa_font_geist_9f2c1a3b"`. Assign it to an
    /// element to opt that subtree into the font directly.
    pub class_name: &'static str,
    /// A CSS custom-property name, including the leading `--`, e.g.
    /// `"--font-geist"`. The generated stylesheet defines a class that sets
    /// this variable so downstream CSS can reference `var(--font-geist)`.
    pub variable: &'static str,
    /// The full `font-family` stack, e.g.
    /// `"'Geist', 'Geist Fallback', system-ui, sans-serif"`. This is what both
    /// the class and the variable resolve to.
    pub family_stack: &'static str,
    /// The vended files backing this family, used to emit preload hints and to
    /// revalidate the on-disk assets against the lockfile.
    pub faces: &'static [FontFace],
    /// The complete, pre-rendered stylesheet: real `@font-face` rules, the
    /// synthesized metric-matched fallback `@font-face`, and the utility classes
    /// named by [`class_name`](Font::class_name) and [`variable`](Font::variable).
    /// Developer-controlled generated content. [`style`](Font::style) preserves
    /// its bytes apart from prefixing known face URLs for the request's mount.
    pub css: &'static str,
}

impl Font {
    /// The `@font-face` family name.
    #[inline]
    pub const fn family(&self) -> &'static str {
        self.family
    }

    /// The class name that applies this font's `font-family` directly.
    #[inline]
    pub const fn class_name(&self) -> &'static str {
        self.class_name
    }

    /// The CSS custom-property name (with leading `--`) this font defines.
    #[inline]
    pub const fn variable(&self) -> &'static str {
        self.variable
    }

    /// The full `font-family` stack the class and variable resolve to.
    #[inline]
    pub const fn family_stack(&self) -> &'static str {
        self.family_stack
    }

    /// The vended files backing this family.
    #[inline]
    pub const fn faces(&self) -> &'static [FontFace] {
        self.faces
    }

    /// Emit `<link rel="preload">` hints for every face marked
    /// [`preload`](FontFace::preload).
    ///
    /// Font preloads are always fetched in CORS-anonymous mode, so the hint
    /// carries `crossorigin` even for same-origin files; omitting it would
    /// cause the browser to fetch the file twice.
    #[cfg(feature = "render")]
    pub fn preload_links<B: WriteBuf>(
        &self,
        cx: &mut WebContext<impl proa_core::DataLoader, B>,
    ) -> Result<(), WriteError> {
        for face in self.faces {
            if !face.preload {
                continue;
            }
            StaticRaw("<link rel=\"preload\" as=\"font\" type=\"").render(cx)?;
            StaticRaw(face.format.mime()).render(cx)?;
            StaticRaw("\" href=\"").render(cx)?;
            proa_core::shared::attr_value::AttrValue::render_attr_value(
                &cx.request().url(face.url),
                cx,
            )?;
            StaticRaw("\" crossorigin=\"anonymous\">").render(cx)?;
        }
        Ok(())
    }

    /// Emit the `<style>` element carrying this font's generated stylesheet
    /// (real `@font-face` rules, the metric-matched fallback, and the utility
    /// classes).
    #[cfg(feature = "render")]
    pub fn style<B: WriteBuf>(
        &self,
        cx: &mut WebContext<impl proa_core::DataLoader, B>,
    ) -> Result<(), WriteError> {
        StaticRaw("<style>").render(cx)?;
        if cx.request().base_path().as_str().is_empty() {
            StaticRaw(self.css).render(cx)?;
        } else {
            // proa-perf-ok: share the prefix while writing CSS without allocating a stylesheet.
            let base_path = cx.request().base_path().clone();
            let mut rest = self.css;
            // The font compiler emits src:url(<face.url>). Only substitute
            // known face URLs, preserving fallbacks and unrelated CSS bytes.
            while let Some(start) = rest.find("src:url(") {
                let split = start + "src:url(".len();
                let Some(end) = rest[split..].find(')') else {
                    break;
                };
                let url = &rest[split..split + end];
                cx.output_mut().extend(rest[..split].as_bytes())?;
                if url.starts_with('/')
                    && !url.starts_with("//")
                    && self.faces.iter().any(|face| face.url == url)
                {
                    write_css_url_prefix(cx.output_mut(), base_path.as_str())?;
                }
                cx.output_mut().extend(url.as_bytes())?;
                rest = &rest[split + end..];
            }
            cx.output_mut().extend(rest.as_bytes())?;
        }
        StaticRaw("</style>").render(cx)
    }

    /// Emit everything this font contributes to the document `<head>`: its
    /// preload hints followed by its `<style>` block. Call it once, high in the
    /// head, from your document layout.
    ///
    /// Allocation-free and generic over the output buffer, so it composes inside
    /// any `WebRenderSync` head builder.
    /// Both preload links and generated CSS face URLs follow the base path on
    /// `cx.request()`. Render with the page's context so each mount stays local.
    #[cfg(feature = "render")]
    pub fn head<B: WriteBuf>(
        &self,
        cx: &mut WebContext<impl proa_core::DataLoader, B>,
    ) -> Result<(), WriteError> {
        self.preload_links(cx)?;
        self.style(cx)
    }
}

// Escape an unquoted CSS url() token, including '<' so a mount containing
// '</style>' cannot terminate the enclosing HTML raw-text element. CSS hex
// escapes decode to the original URL before the browser resolves it.
#[cfg(feature = "render")]
fn write_css_url_prefix<B: WriteBuf>(out: &mut B, prefix: &str) -> Result<(), WriteError> {
    let mut start = 0;
    for (index, byte) in prefix.bytes().enumerate() {
        let escaped = match byte {
            b' ' => "\\20 ",
            b'\"' => "\\22 ",
            b'\'' => "\\27 ",
            b'(' => "\\28 ",
            b')' => "\\29 ",
            b'<' => "\\3c ",
            b'>' => "\\3e ",
            b'\\' => "\\5c ",
            _ => continue,
        };
        out.extend(prefix[start..index].as_bytes())?;
        out.extend(escaped.as_bytes())?;
        start = index + 1;
    }
    out.extend(prefix[start..].as_bytes())
}

#[cfg(all(test, feature = "render"))]
mod tests {
    use super::*;
    #[cfg(feature = "render")]
    use proa_core::{RenderBuffer, WebContext};

    /// A structurally realistic fixture standing in for a generated module: two
    /// latin faces (regular preloaded, italic not) plus a representative
    /// stylesheet with a real `@font-face`, a metric-matched fallback, and the
    /// two utility classes.
    const FIXTURE_CSS: &str = concat!(
        "@font-face{font-family:'Geist';font-style:normal;font-weight:400;",
        "font-display:swap;src:url(/fonts/geist-latin-400-normal.9f2c1a3b.woff2) format('woff2');",
        "unicode-range:U+0000-00FF}",
        "@font-face{font-family:'Geist Fallback';src:local('Arial');",
        "ascent-override:95.02%;descent-override:24.02%;line-gap-override:0.00%;size-adjust:101.00%}",
        ".__proa_font_geist_9f2c1a3b{font-family:'Geist','Geist Fallback',system-ui,sans-serif}",
        ":root{--font-geist:'Geist','Geist Fallback',system-ui,sans-serif}",
    );

    const FIXTURE_FACES: &[FontFace] = &[
        FontFace {
            url: "/fonts/geist-latin-400-normal.9f2c1a3b.woff2",
            format: FontFormat::Woff2,
            weight: FontWeight::Fixed(400),
            style: FontStyle::Normal,
            preload: true,
        },
        FontFace {
            url: "/fonts/geist-latin-400-italic.7a1b2c3d.woff2",
            format: FontFormat::Woff2,
            weight: FontWeight::Fixed(400),
            style: FontStyle::Italic,
            preload: false,
        },
    ];

    const FIXTURE: Font = Font {
        family: "Geist",
        class_name: "__proa_font_geist_9f2c1a3b",
        variable: "--font-geist",
        family_stack: "'Geist','Geist Fallback',system-ui,sans-serif",
        faces: FIXTURE_FACES,
        css: FIXTURE_CSS,
    };

    fn render(f: impl FnOnce(&mut WebContext<::proa_core::NoLoader, RenderBuffer>)) -> String {
        let mut cx = WebContext::new();
        f(&mut cx);
        cx.into_output().into_string()
    }

    #[test]
    fn head_emits_preload_only_for_preloaded_faces() {
        let html = render(|cx| FIXTURE.head(cx).unwrap());

        // Regular face is preloaded with the required font-preload attributes.
        assert!(html.contains(
            "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" \
             href=\"/fonts/geist-latin-400-normal.9f2c1a3b.woff2\" crossorigin=\"anonymous\">"
        ));
        // Italic face is not preloaded, so no preload hint mentions it.
        assert!(!html.contains("geist-latin-400-italic"));
    }

    #[test]
    fn font_preloads_and_css_follow_each_requests_mount() {
        for prefix in ["/apps/demo", "/user/a%40b/proxy/3000", "/preview/other"] {
            let mut cx = WebContext::new();
            cx.request_mut()
                .set_base_path(proa_core::BasePath::new(prefix).unwrap());
            FIXTURE.head(&mut cx).unwrap();
            let html = cx.into_output().into_string();
            assert!(html.contains(&format!("href=\"{prefix}{}\"", FIXTURE_FACES[0].url)));
            assert!(html.contains(&format!("src:url({prefix}{})", FIXTURE_FACES[0].url)));
            assert!(!html.contains("href=\"/fonts/"));
            assert!(!html.contains("src:url(/fonts/"));
            // Metric fallbacks and utility rules retain their generated bytes.
            assert!(html.contains("src:local("));
            assert!(html.contains(":root{--font-geist:"));
        }
    }

    #[test]
    fn mount_is_escaped_for_html_attributes_and_css_url_tokens() {
        let mut cx = WebContext::new();
        cx.request_mut()
            .set_base_path(proa_core::BasePath::new("/apps/\"'()").unwrap());
        FIXTURE.head(&mut cx).unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("href=\"/apps/&quot;&#39;()/fonts/"), "{html}");
        assert!(
            html.contains(r"src:url(/apps/\22 \27 \28 \29 /fonts/"),
            "{html}"
        );
        let mut css = Vec::new();
        write_css_url_prefix(&mut css, "/apps/</style>").unwrap();
        assert_eq!(css, br"/apps/\3c /style\3e ");
    }

    #[test]
    fn head_wraps_css_in_a_single_style_element() {
        let html = render(|cx| FIXTURE.head(cx).unwrap());

        assert_eq!(html.matches("<style>").count(), 1);
        assert_eq!(html.matches("</style>").count(), 1);
        assert!(html.contains("@font-face"));
        assert!(html.contains(FIXTURE_CSS));
        // Both utility classes ship: the direct class and the variable class.
        assert!(html.contains(".__proa_font_geist_9f2c1a3b{font-family:"));
        assert!(html.contains("--font-geist:"));
        // The metric-matched fallback is present so swap causes no layout shift.
        assert!(html.contains("'Geist Fallback'"));
        assert!(html.contains("size-adjust:"));
    }

    #[test]
    fn preload_links_and_style_split_the_head_output() {
        let links = render(|cx| FIXTURE.preload_links(cx).unwrap());
        let style = render(|cx| FIXTURE.style(cx).unwrap());

        assert!(links.contains("rel=\"preload\""));
        assert!(!links.contains("<style>"));

        assert!(style.starts_with("<style>"));
        assert!(style.ends_with("</style>"));
        assert!(!style.contains("rel=\"preload\""));

        // head() is exactly preload_links() followed by style().
        let head = render(|cx| FIXTURE.head(cx).unwrap());
        assert_eq!(head, format!("{links}{style}"));
    }

    #[test]
    fn accessors_expose_generated_identifiers() {
        assert_eq!(FIXTURE.family(), "Geist");
        assert_eq!(FIXTURE.class_name(), "__proa_font_geist_9f2c1a3b");
        assert_eq!(FIXTURE.variable(), "--font-geist");
        assert_eq!(
            FIXTURE.family_stack(),
            "'Geist','Geist Fallback',system-ui,sans-serif"
        );
        assert_eq!(FIXTURE.faces().len(), 2);
    }

    #[test]
    fn face_constructors_default_to_non_preloaded_woff2() {
        let face = FontFace::woff2("/fonts/x.woff2", 500);
        assert_eq!(face.format, FontFormat::Woff2);
        assert_eq!(face.weight, FontWeight::Fixed(500));
        assert_eq!(face.style, FontStyle::Normal);
        assert!(!face.preload);
        assert!(face.preloaded().preload);
    }

    #[test]
    fn a_font_with_no_preloaded_faces_emits_no_links() {
        const CSS: &str = "@font-face{font-family:'X';src:url(/fonts/x.woff2) format('woff2')}";
        const FACES: &[FontFace] = &[FontFace::woff2("/fonts/x.woff2", 400)];
        const F: Font = Font {
            family: "X",
            class_name: "__proa_font_x",
            variable: "--font-x",
            family_stack: "'X',sans-serif",
            faces: FACES,
            css: CSS,
        };

        let html = render(|cx| F.head(cx).unwrap());
        assert!(!html.contains("rel=\"preload\""));
        assert!(html.contains("<style>"));
        assert!(html.contains(CSS));
    }
}
