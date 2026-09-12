// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Attribute value rendering trait
//!
//! This module provides the `AttrValue` trait which allows different types
//! to be used as HTML attribute values in the `html!` macro.

use super::write_buf::{WriteBuf, WriteError};
use crate::render::DataLoader;
use crate::render::WebContext;
use core::fmt;

const DISPLAY_URL_ATTR_PROBE_CAP: usize = 2048;

/// Trait for types that can be rendered as HTML attribute values.
pub trait AttrValue {
    /// Returns true if the attribute should be rendered at all.
    fn should_render(&self) -> bool;

    /// Returns true if this is a boolean attribute (no value, just presence).
    fn is_boolean_attr(&self) -> bool {
        false
    }

    /// Expose the value as a string slice for URL-scheme validation, when the
    /// value is a plain string-like attribute value.
    ///
    /// Returns `None` for values that can never carry a URL (booleans, class
    /// lists) or that the developer has explicitly marked safe (`SafeAttr`).
    /// `html!` calls this only for URL-bearing attribute names (`href`, `src`,
    /// `action`, ...) so dynamic values get the same `javascript:` / `vbscript:`
    /// / executable-`data:` scheme block that static literals get at compile
    /// time and that the runtime `Elem` builder applies. See
    /// [`render_url_attr`](AttrValue::render_url_attr).
    #[inline(always)]
    fn url_str(&self) -> Option<&str> {
        None
    }

    /// Render the attribute value to the buffer.
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError>;

    /// Render the complete attribute (name + value + closing quote) in one call.
    #[inline(always)]
    fn render_attr<B: WriteBuf, L: DataLoader>(
        &self,
        name_prefix: &'static [u8],
        name_bool: &'static [u8],
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        if self.should_render() {
            if self.is_boolean_attr() {
                cx.out.extend_static(name_bool)?;
            } else {
                cx.out.extend_static(name_prefix)?;
                self.render_attr_value(cx)?;
                cx.out.extend(b"\"")?;
            }
        }
        Ok(())
    }

    /// Render a URL-bearing attribute (`href`, `src`, `action`, ...), blocking
    /// dangerous URL schemes before they reach the output.
    ///
    /// `html!` emits this instead of [`render_attr`](AttrValue::render_attr)
    /// for URL attribute names. If [`url_str`](AttrValue::url_str) yields a
    /// value whose scheme is `javascript:`, `vbscript:`, or an executable
    /// `data:` type, the attribute is neutralized: in debug builds it trips a
    /// `debug_assert!` so the mistake is caught in tests, and in release builds
    /// it renders with an empty value rather than the dangerous URL. This
    /// closes the gap where a static `href="javascript:..."` is a compile error
    /// but a dynamic `href={user_value}` would otherwise pass through unchecked.
    /// Mirrors the guard in `proa_core::shared::elem::write_attr`.
    #[inline(always)]
    fn render_url_attr<B: WriteBuf, L: DataLoader>(
        &self,
        name_prefix: &'static [u8],
        name_bool: &'static [u8],
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        if let Some(url) = self.url_str() {
            if crate::shared::attr::has_dangerous_url_scheme(url) {
                debug_assert!(
                    false,
                    "dangerous URL scheme in a URL attribute value: `javascript:`, \
                     `vbscript:`, and executable `data:` URIs are blocked to prevent \
                     Cross-Site Scripting (XSS). Use a safe scheme (https:, /, #) or \
                     route the value through a typed helper."
                );
                if self.should_render() {
                    cx.out.extend_static(name_prefix)?;
                    cx.out.extend_static(b"\"")?;
                }
                return Ok(());
            }
        }
        self.render_attr(name_prefix, name_bool, cx)
    }
}

// Implementation for &str - always renders, escapes the value
impl AttrValue for &str {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn url_str(&self) -> Option<&str> {
        Some(self)
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        crate::shared::escape::escape_attr_value_for_dialect(&mut cx.out, self, dialect)
    }
}

// Implementation for String - always renders, escapes the value.
impl AttrValue for String {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn url_str(&self) -> Option<&str> {
        Some(self.as_str())
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        crate::shared::escape::escape_attr_value_for_dialect(&mut cx.out, self.as_str(), dialect)
    }
}

// Implementation for Option<&str> - renders only if Some
impl AttrValue for Option<&str> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.is_some()
    }

    #[inline(always)]
    fn url_str(&self) -> Option<&str> {
        *self
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        if let Some(s) = self {
            let dialect = cx.__proa_current_escape_dialect();
            crate::shared::escape::escape_attr_value_for_dialect(&mut cx.out, s, dialect)?;
        }
        Ok(())
    }
}

// Implementation for bool - boolean attribute (present/absent)
impl AttrValue for bool {
    #[inline(always)]
    fn should_render(&self) -> bool {
        *self
    }

    #[inline(always)]
    fn is_boolean_attr(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        _cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        Ok(())
    }
}

// Implementation for ClassList - renders class segments directly
impl<const N: usize> AttrValue for super::class_list::ClassList<N> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        !self.is_empty()
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        use crate::WebRenderSync;
        WebRenderSync::render(*self, cx)
    }
}

/// A pre-verified safe attribute value that skips runtime escaping.
pub struct SafeAttr(pub &'static str);

impl AttrValue for SafeAttr {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        cx.out.extend_static(self.0.as_bytes())
    }
}

/// Render a `Display` value as a normal quoted attribute value.
///
/// This is intentionally distinct from the blanket `bool` implementation:
/// `disabled={false}` should omit a boolean HTML attribute, but
/// `aria-expanded={display_attr(false)}` must render as
/// `aria-expanded="false"`.
pub struct DisplayAttr<T>(pub T);

pub const fn display_attr<T>(value: T) -> DisplayAttr<T> {
    DisplayAttr(value)
}

impl<T: fmt::Display> AttrValue for DisplayAttr<T> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        __proa_render_fmt_attr(format_args!("{}", &self.0), cx)
    }

    #[inline(always)]
    fn render_url_attr<B: WriteBuf, L: DataLoader>(
        &self,
        name_prefix: &'static [u8],
        _name_bool: &'static [u8],
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let mut rendered = heapless::String::<DISPLAY_URL_ATTR_PROBE_CAP>::new();
        if fmt::write(&mut rendered, format_args!("{}", &self.0)).is_err() {
            debug_assert!(
                false,
                "formatted URL attribute value exceeded the inline validation buffer"
            );
            return render_empty_attr(name_prefix, cx);
        }

        if crate::shared::attr::has_dangerous_url_scheme(rendered.as_str()) {
            debug_assert!(
                false,
                "dangerous URL scheme in a URL attribute value: `javascript:`, \
                 `vbscript:`, and executable `data:` URIs are blocked to prevent \
                 Cross-Site Scripting (XSS). Use a safe scheme (https:, /, #) or \
                 route the value through a typed helper."
            );
            return render_empty_attr(name_prefix, cx);
        }

        cx.out.extend_static(name_prefix)?;
        let dialect = cx.__proa_current_escape_dialect();
        crate::shared::escape::escape_attr_value_for_dialect(
            &mut cx.out,
            rendered.as_str(),
            dialect,
        )?;
        cx.out.extend(b"\"")
    }
}

#[inline(always)]
fn render_empty_attr<B: WriteBuf, L: DataLoader>(
    name_prefix: &'static [u8],
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    cx.out.extend_static(name_prefix)?;
    cx.out.extend_static(b"\"")
}

#[doc(hidden)]
#[inline(always)]
pub fn __proa_render_fmt_attr<B: WriteBuf, L: DataLoader>(
    args: fmt::Arguments<'_>,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    let mut writer = EscapingAttrWriter { cx, err: None };
    match fmt::write(&mut writer, args) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer.err.unwrap_or(WriteError::InvalidAttribute(
            "failed to format attribute value",
        ))),
    }
}

/// Attribute-escaping counterpart to
/// [`crate::shared::text::__proa_render_fmt_text_with`].
#[doc(hidden)]
#[inline(always)]
pub fn __proa_render_fmt_attr_with<T, B: WriteBuf, L: DataLoader>(
    value: &T,
    format: fn(&T, &mut dyn fmt::Write) -> fmt::Result,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    let mut writer = EscapingAttrWriter { cx, err: None };
    match format(value, &mut writer) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer.err.unwrap_or(WriteError::InvalidAttribute(
            "failed to format attribute value",
        ))),
    }
}

struct EscapingAttrWriter<'cx, B: WriteBuf, L: DataLoader> {
    cx: &'cx mut WebContext<L, B>,
    err: Option<WriteError>,
}

impl<B: WriteBuf, L: DataLoader> fmt::Write for EscapingAttrWriter<'_, B, L> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.err.is_some() {
            return Err(fmt::Error);
        }

        let dialect = self.cx.__proa_current_escape_dialect();
        match crate::shared::escape::escape_attr_value_for_dialect(&mut self.cx.out, s, dialect) {
            Ok(()) => Ok(()),
            Err(err) => {
                self.err = Some(err);
                Err(fmt::Error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_always_renders() {
        let s: &str = "hello";
        assert!(s.should_render());
        assert!(!s.is_boolean_attr());
    }

    #[test]
    fn string_always_renders() {
        let s = String::from("hello");
        assert!(s.should_render());
        assert!(!s.is_boolean_attr());
    }

    #[test]
    fn option_str_conditional() {
        let some: Option<&str> = Some("hello");
        let none: Option<&str> = None;

        assert!(some.should_render());
        assert!(!none.should_render());
    }

    #[test]
    fn bool_is_boolean_attr() {
        assert!(true.should_render());
        assert!(!false.should_render());
        assert!(true.is_boolean_attr());
        assert!(false.is_boolean_attr());
    }

    #[test]
    fn class_list_renders_when_non_empty() {
        let empty = super::super::class_list::ClassList::<3>::new();
        let non_empty = super::super::class_list::ClassList::<3>::new().add("foo");

        assert!(!empty.should_render());
        assert!(non_empty.should_render());
        assert!(!non_empty.is_boolean_attr());
    }

    #[test]
    fn display_attr_renders_bool_as_quoted_value() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        display_attr(false)
            .render_attr(b" aria-expanded=\"", b" aria-expanded", &mut cx)
            .unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), r#" aria-expanded="false""#);
    }

    #[test]
    fn display_attr_escapes_formatted_chunks() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        display_attr("<panel \"open\">")
            .render_attr(b" data-state=\"", b" data-state", &mut cx)
            .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#" data-state="&lt;panel &quot;open&quot;&gt;""#
        );
    }

    #[test]
    fn display_attr_url_attr_renders_safe_value() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        display_attr("https://example.com/<path>?q=\"x\"")
            .render_url_attr(b" href=\"", b" href", &mut cx)
            .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#" href="https://example.com/&lt;path&gt;?q=&quot;x&quot;""#
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn display_attr_url_attr_blocks_javascript_scheme_in_debug() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _ = display_attr("javascript:alert(1)").render_url_attr(b" href=\"", b" href", &mut cx);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn display_attr_url_attr_neutralizes_dangerous_schemes_in_release() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        display_attr("javascript:alert(1)")
            .render_url_attr(b" href=\"", b" href", &mut cx)
            .unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), r#" href="""#);
    }

    #[test]
    fn fmt_attr_helper_streams_through_attr_escaping() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        __proa_render_fmt_attr(format_args!("{} {:.1}", "<panel \"open\">", 1.25), &mut cx)
            .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#"&lt;panel &quot;open&quot;&gt; 1.2"#
        );
    }

    #[test]
    fn url_str_exposed_only_for_string_like_values() {
        assert_eq!("https://x".url_str(), Some("https://x"));
        assert_eq!(String::from("https://x").url_str(), Some("https://x"));
        assert_eq!(Some("https://x").url_str(), Some("https://x"));
        assert_eq!(None::<&str>.url_str(), None);
        assert_eq!(true.url_str(), None);
        assert_eq!(SafeAttr("javascript:ok").url_str(), None);
    }

    #[test]
    fn url_attr_renders_safe_scheme() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        "https://example.com/path?q=1"
            .render_url_attr(b" href=\"", b" href", &mut cx)
            .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#" href="https://example.com/path?q=1""#
        );
    }

    #[test]
    fn url_attr_allows_inline_image_data_uri() {
        // Non-executable data: URIs (inline images) are legitimate and must
        // still render through the URL-attr path.
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        "data:image/png;base64,iVBORw0KGgo="
            .render_url_attr(b" src=\"", b" src", &mut cx)
            .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#" src="data:image/png;base64,iVBORw0KGgo=""#
        );
    }

    // In debug builds the guard trips a `debug_assert!` so the mistake surfaces
    // in tests; in release builds it neutralizes the attribute to an empty
    // value. Both behaviors are asserted below, gated on `debug_assertions`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn url_attr_blocks_javascript_scheme_in_debug() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let _ = "javascript:alert(document.cookie)".render_url_attr(b" href=\"", b" href", &mut cx);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn url_attr_neutralizes_dangerous_schemes_in_release() {
        for dangerous in [
            "javascript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "  JavaScript:alert(1)",
        ] {
            let mut out = Vec::new();
            let mut cx = WebContext::with_buffer(&mut out);
            dangerous
                .render_url_attr(b" href=\"", b" href", &mut cx)
                .unwrap();
            let rendered = String::from_utf8(out).unwrap();
            assert_eq!(rendered, r#" href="""#, "input was {dangerous:?}");
        }
    }
}
