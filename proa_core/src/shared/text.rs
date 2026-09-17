// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::render::DataLoader;
use crate::{
    md::MdContext,
    render::{MdRender, MdRenderSync, RenderOutcome, WebContext, WebRender, WebRenderSync},
    shared::escape::escape_text_for_dialect,
    shared::write_buf::{WriteBuf, WriteError},
};
use core::fmt;
/// Text node wrapper using proper escaping.
pub struct Text<'a>(pub &'a str);
impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Text<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        let mode = cx.current_mode();
        let dialect = cx.__proa_current_escape_dialect();
        escape_text_for_dialect(&mut cx.out, self.0, mode, dialect)
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async composition bridge
impl<'a, L: DataLoader, B: WriteBuf> WebRender<L, B> for Text<'a> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
impl<'a, B: WriteBuf> MdRenderSync<B> for Text<'a> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_text(self.0)
    }
}
impl<'a, B: WriteBuf> MdRender<B> for Text<'a> {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
#[inline(always)]
pub fn text(s: &str) -> Text<'_> {
    Text(s)
}

#[doc(hidden)]
#[inline(always)]
pub fn __proa_render_fmt_text<B: WriteBuf, L: DataLoader>(
    args: fmt::Arguments<'_>,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    let mut writer = EscapingTextWriter { cx, err: None };
    match fmt::write(&mut writer, args) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer
            .err
            .unwrap_or(WriteError::InvalidAttribute("failed to format text"))),
    }
}

/// Run a macro-generated formatting callback directly against Proa's escaping
/// writer. This is the generic half of the primitive numeric autoref dispatch:
/// specialized values never call it, while arbitrary formatting traits retain
/// the exact `format_args!` expression emitted by `text!`.
#[doc(hidden)]
#[inline(always)]
pub fn __proa_render_fmt_text_with<T, B: WriteBuf, L: DataLoader>(
    value: &T,
    format: fn(&T, &mut dyn fmt::Write) -> fmt::Result,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    let mut writer = EscapingTextWriter { cx, err: None };
    match format(value, &mut writer) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer
            .err
            .unwrap_or(WriteError::InvalidAttribute("failed to format text"))),
    }
}

struct EscapingTextWriter<'cx, B: WriteBuf, L: DataLoader> {
    cx: &'cx mut WebContext<L, B>,
    err: Option<WriteError>,
}

impl<L: DataLoader, B: WriteBuf> fmt::Write for EscapingTextWriter<'_, B, L> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.err.is_some() {
            return Err(fmt::Error);
        }

        let mode = self.cx.current_mode();
        let dialect = self.cx.__proa_current_escape_dialect();
        match escape_text_for_dialect(&mut self.cx.out, s, mode, dialect) {
            Ok(()) => Ok(()),
            Err(err) => {
                self.err = Some(err);
                Err(fmt::Error)
            }
        }
    }
}
/// Raw HTML passthrough that **bypasses all escaping**.
///
/// # Security Warning
///
/// This type injects its content directly into the HTML output with **no
/// encoding or sanitization**. Passing user-controlled or untrusted data
/// through `UnsafeRaw` creates a direct Cross-Site Scripting (XSS)
/// vulnerability — the most common web application security flaw
/// (OWASP Top 10, A03:2021 — Injection).
///
/// ## OWASP guidance
///
/// The OWASP XSS Prevention Cheat Sheet (Rule #1) states: "Never insert
/// untrusted data except in allowed locations", and specifically warns
/// against inserting unsanitized HTML.
///
/// <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
///
/// If user-authored HTML is required, sanitize it **before** passing it to
/// `UnsafeRaw` using a dedicated allowlist-based sanitizer:
/// - Rust: [ammonia](https://crates.io/crates/ammonia)
/// - Client-side: [DOMPurify](https://github.com/cure53/DOMPurify)
///
/// Prefer [`StaticRaw`] when the content is a compile-time constant, as it
/// eliminates the possibility of runtime injection entirely.
///
/// ## Second-order contexts and dangerous tags
///
/// A few HTML contexts re-parse content after the initial HTML parse and
/// require sanitizers that understand the inner grammar — attribute escaping
/// alone is not enough:
/// - `<svg>` / `<math>` / `<foreignObject>` subtrees: browsers re-parse
///   content in a different namespace, and historical DOMPurify bypasses
///   (2019–2020) showed that HTML-escaped content can become executable
///   after the namespace flip. The `html!` macro emits a compile-time
///   warning when `UnsafeRaw` appears inside these containers.
/// - `<iframe srcdoc="…">`: the attribute value parses as HTML inside the
///   iframe.
/// - `<base href="…">`: a single injected `<base>` rewrites every relative
///   URL on the page, including `<script src="./island.js">`. If your app
///   serves any island assets via relative URLs, an attacker who lands a
///   `<base>` tag via `UnsafeRaw` can redirect them to an attacker origin.
///   DOMPurify excludes `<base>` from its default allowlist for the same
///   reason. Prefer absolute-path URLs (leading `/`) for island and static
///   asset references, which are immune to `<base href>` rewriting.
///
/// # Example
///
/// ```ignore
/// // SAFE: developer-controlled constant
/// UnsafeRaw("<br>").render(cx);
///
/// // DANGEROUS: never do this with user input
/// // UnsafeRaw(&user_input).render(cx); // XSS!
/// ```
pub struct UnsafeRaw<'a>(pub &'a str);
impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for UnsafeRaw<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        cx.out.extend(self.0.as_bytes())
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async bridge for trusted raw output
impl<'a, L: DataLoader, B: WriteBuf> WebRender<L, B> for UnsafeRaw<'a> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
/// Raw HTML/SVG is meaningless in markdown — renders nothing.
impl<'a, B: WriteBuf> MdRenderSync<B> for UnsafeRaw<'a> {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
impl<'a, B: WriteBuf> MdRender<B> for UnsafeRaw<'a> {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
/// Static raw HTML passthrough — zero-copy on scatter-gather buffers.
///
/// Like [`UnsafeRaw`] but constrained to `&'static str`, allowing
/// [`WriteBuf::extend_static`] to store a pointer instead of copying bytes.
/// Use this for `const` SVG strings, inline HTML fragments, and other
/// compile-time-known markup.
///
/// # Security
///
/// Because the content must be a compile-time constant (`&'static str`),
/// `StaticRaw` is inherently safer than [`UnsafeRaw`] — there is no path
/// for user-controlled data to reach it at runtime. This makes it the
/// preferred choice for embedding trusted markup (SVG icons, static HTML
/// fragments, etc.) without escaping overhead.
///
/// Per OWASP XSS Prevention Cheat Sheet (Rule #1): raw HTML is acceptable
/// when the content is developer-controlled and verified at authoring time,
/// which the `'static` lifetime constraint enforces.
///
/// <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
pub struct StaticRaw(pub &'static str);
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticRaw {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        cx.out.extend_static(self.0.as_bytes())
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async bridge for static raw output
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for StaticRaw {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
impl<B: WriteBuf> MdRenderSync<B> for StaticRaw {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
impl<B: WriteBuf> MdRender<B> for StaticRaw {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
/// Impl WebRender for &str - allows dynamic text in html_optimized! blocks
/// The text is escaped at runtime.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for &str {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        let mode = cx.current_mode();
        let dialect = cx.__proa_current_escape_dialect();
        escape_text_for_dialect(&mut cx.out, self, mode, dialect)
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async composition bridge
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for &str {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
/// Impl WebRender for String - allows dynamic text from format!() etc.
/// The text is escaped at runtime.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for String {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        let mode = cx.current_mode();
        let dialect = cx.__proa_current_escape_dialect();
        escape_text_for_dialect(&mut cx.out, &self, mode, dialect)
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async composition bridge
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for String {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
impl<B: WriteBuf> MdRender<B> for &str {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
impl<B: WriteBuf> MdRender<B> for String {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
impl<B: WriteBuf> MdRender<B> for &String {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
/// Pre-verified safe text that doesn't need escaping.
/// Use `safe_text!("literal")` macro to create this safely at compile time.
///
/// For dynamic strings known to be safe, use `SafeText::from_trusted(s)`.
///
/// SAFETY: The contained string must NOT contain: `<`, `>`, `&`, `"`, or `'`
/// Using this with unsafe content will result in XSS vulnerabilities.
pub struct SafeText<'a>(pub &'a str);
impl<'a> SafeText<'a> {
    /// Create a SafeText from a trusted dynamic string.
    ///
    /// # Safety
    /// The caller must ensure the string does NOT contain `<`, `>`, `&`, `"`, or `'`.
    /// Passing unsafe content will result in XSS vulnerabilities.
    ///
    /// # Example
    /// ```ignore
    /// // Only use for strings you KNOW are safe (e.g., from a controlled set)
    /// let title = match idx {
    ///     0 => "Latency",
    ///     _ => "Throughput",
    /// };
    /// SafeText::from_trusted(title).render(cx);
    /// ```
    #[inline(always)]
    pub const fn from_trusted(s: &'a str) -> Self {
        SafeText(s)
    }
}
impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SafeText<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        cx.out.extend(self.0.as_bytes())
    }
}
// proa-perf-ok(scope): intentional immediate sync-to-async composition bridge
impl<'a, L: DataLoader, B: WriteBuf> WebRender<L, B> for SafeText<'a> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<
        impl core::future::Future<Output = Result<(), crate::shared::write_buf::WriteError>>,
    > {
        RenderOutcome::<std::future::Ready<Result<(), crate::shared::write_buf::WriteError>>>::Done(
            WebRenderSync::render(self, cx),
        )
    }
}
/// SafeText in markdown — the text is HTML-safe but may contain markdown chars,
/// so we still escape for markdown.
impl<'a, B: WriteBuf> MdRenderSync<B> for SafeText<'a> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_text(self.0)
    }
}
impl<'a, B: WriteBuf> MdRender<B> for SafeText<'a> {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
/// Create a SafeText from a string literal, verified at compile time.
///
/// This macro checks that the string contains no characters that need escaping
/// (`<`, `>`, `&`, `"`, `'`) and produces a `SafeText` that bypasses the
/// runtime escape scanning.
///
/// # Example
/// ```
/// use proa_core::{safe_text, WebRender};
///
/// // This compiles - no special characters
/// let t = safe_text!("Hello World");
///
/// // This would fail to compile - contains '<'
/// // let t = safe_text!("<script>");
/// ```
/// Suppress compile-time performance hints for this expression.
///
/// Wrapping an expression in `perf_ok()` tells the `html!` macro's performance
/// scanner to skip warnings for allocations like `format!()`, `.to_string()`,
/// etc. The expression is returned unchanged — this function compiles away to
/// nothing.
///
/// Use when you've intentionally chosen an allocation in the render path
/// (prototyping, rare code paths, complex formatting). Visible in code review
/// so others can ask "why is this allocation here?"
///
/// # Example
///
/// ```ignore
/// use proa_core::perf_ok;
///
/// // This would normally produce a cargo warning:
/// html! { <span>{format!("${}", price)}</span> }
///
/// // Silenced:
/// html! { <span>{perf_ok(format!("${}", price))}</span> }
/// ```
#[inline(always)]
pub fn perf_ok<T>(val: T) -> T {
    val
}

/// Suppress the `html!` macro's dynamic `style=` warning for a reviewed value.
///
/// Use this for inline styles assembled from trusted, developer-owned pieces
/// that cannot reasonably be expressed as a static attribute or class.
#[inline(always)]
pub fn style_ok<T>(val: T) -> T {
    val
}

/// Suppress the `html!` macro's raw-text interpolation warning for a reviewed value.
///
/// Intended for `<title>` and `<textarea>` content where interpolation is
/// deliberate and does not rely on tag parsing.
#[inline(always)]
pub fn raw_text_ok<T>(val: T) -> T {
    val
}

/// Suppress the `html!` macro's foreign-content `UnsafeRaw` warning for reviewed SVG/MathML.
///
/// Use only when the inserted markup is developer-controlled or has already
/// been sanitized with a namespace-aware sanitizer.
#[inline(always)]
pub fn foreign_content_ok<T>(val: T) -> T {
    val
}

#[macro_export]
macro_rules! safe_text {
    ($s:literal) => {{
        const _: () = {
            const S: &str = $s;
            const fn check_safe(s: &str) -> bool {
                let bytes = s.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'<' || b == b'>' || b == b'&' || b == b'"' || b == b'\'' {
                        return false;
                    }
                    i += 1;
                }
                true
            }
            assert!(
                check_safe(S),
                "safe_text! string contains characters that need escaping: <, >, &, \", or '"
            );
        };
        $crate::SafeText($s)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_text_helper_streams_through_text_escaping() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        __proa_render_fmt_text(
            format_args!("{:.2} {}", 12.345_f64, "<tag & value>"),
            &mut cx,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "12.35 &lt;tag &amp; value&gt;"
        );
    }
}
