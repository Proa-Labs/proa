// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::sync::Arc;

use crate::shared::attr_value::AttrValue;
use crate::{DataLoader, WebContext, WriteBuf, WriteError};

/// A validated Content Security Policy nonce for one rendered request.
///
/// The accepted alphabet matches CSP's `base64-value` grammar, including the
/// URL-safe `-` and `_` variants and up to two trailing padding characters.
/// Validation lets Proa write the value directly into a quoted HTML attribute
/// without allocating or relying on callers to escape it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CspNonce(Arc<str>);

impl CspNonce {
    /// Validate and own a CSP nonce.
    pub fn new(value: impl Into<Arc<str>>) -> Result<Self, InvalidCspNonce> {
        let value = value.into();
        if is_valid_csp_nonce(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidCspNonce)
        }
    }

    /// Return the nonce exactly as it should appear in a CSP header.
    #[inline(always)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Append ` nonce="…"` to an HTML start tag.
    ///
    /// Construction-time validation proves that `self` contains no HTML
    /// delimiters, so this path is allocation-free.
    #[inline(always)]
    pub fn write_html_attr<B: WriteBuf>(&self, out: &mut B) -> Result<(), WriteError> {
        out.extend_static(b" nonce=\"")?;
        out.extend(self.0.as_bytes())?;
        out.extend_static(b"\"")
    }
}

impl AsRef<str> for CspNonce {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

// proa-perf-ok(scope): construction validates an alphabet with no HTML
// delimiters, so direct attribute emission is safe and allocation-free
impl AttrValue for CspNonce {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend(self.0.as_bytes())
    }
}

/// Optional nonce attribute adapter used by Proa's generated script tags.
///
/// This is deliberately a distinct type instead of an `AttrValue`
/// implementation for `Option<&CspNonce>`. Multiple concrete `Option<T>`
/// attribute implementations make an untyped `None` in ordinary component
/// props ambiguous during template expansion.
#[derive(Clone, Copy, Debug)]
pub struct CspNonceAttr<'a>(Option<&'a CspNonce>);

/// Adapt an optional request nonce for use in an `html!`/`html_sync!`
/// attribute.
#[inline(always)]
pub const fn csp_nonce_attr(nonce: Option<&CspNonce>) -> CspNonceAttr<'_> {
    CspNonceAttr(nonce)
}

// proa-perf-ok(scope): the contained nonce has the same validated alphabet
impl AttrValue for CspNonceAttr<'_> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.0.is_some()
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        if let Some(nonce) = self.0 {
            cx.out.extend(nonce.0.as_bytes())?;
        }
        Ok(())
    }
}

/// Error returned when a CSP nonce is not a valid base64 value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCspNonce;

impl fmt::Display for InvalidCspNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "CSP nonce must be a non-empty base64 value using ASCII letters, digits, +, /, -, _, and at most two trailing = characters",
        )
    }
}

impl std::error::Error for InvalidCspNonce {}

fn is_valid_csp_nonce(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let content_len = bytes
        .iter()
        .position(|byte| *byte == b'=')
        .unwrap_or(bytes.len());
    if content_len == 0 || bytes.len().saturating_sub(content_len) > 2 {
        return false;
    }
    if !bytes[content_len..].iter().all(|byte| *byte == b'=') {
        return false;
    }
    bytes[..content_len]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'+' | b'/' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_standard_and_url_safe_base64_values() {
        for value in ["abcDEF012+/", "abcDEF012-_", "YWJjZA==", "YQ="] {
            assert_eq!(CspNonce::new(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_empty_malformed_and_html_values() {
        for value in ["", "=", "abc===", "ab=c", "abc def", "abc\"", "abc<script>"] {
            assert!(
                CspNonce::new(value).is_err(),
                "{value:?} should be rejected"
            );
        }
    }

    #[test]
    fn writes_a_complete_nonce_attribute() {
        let nonce = CspNonce::new("YWJjZA==").unwrap();
        let mut out = Vec::new();
        nonce.write_html_attr(&mut out).unwrap();
        assert_eq!(out, br#" nonce="YWJjZA==""#);
    }

    #[test]
    fn optional_attribute_adapter_renders_only_present_nonces() {
        let nonce = CspNonce::new("YWJjZA==").unwrap();
        let present = csp_nonce_attr(Some(&nonce));
        let absent = csp_nonce_attr(None);

        assert!(present.should_render());
        assert!(!absent.should_render());

        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        present
            .render_attr(b" nonce=\"", b" nonce", &mut cx)
            .unwrap();
        absent
            .render_attr(b" nonce=\"", b" nonce", &mut cx)
            .unwrap();
        assert_eq!(out, br#" nonce="YWJjZA==""#);
    }
}
