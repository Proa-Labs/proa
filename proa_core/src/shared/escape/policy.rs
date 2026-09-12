// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::sealed;

/// Policy controlling Proa's escape rules at the byte-loop level.
///
/// This trait is sealed because escape rules are a security boundary. New
/// dialects must be added in-tree and receive a focused security review.
pub trait EscapePolicy: sealed::Sealed {
    /// Rewrite U+00A0 (NBSP) in text and attribute content.
    ///
    /// HTML emits `&nbsp;`; XML 1.0 has no named NBSP entity without a DTD, so
    /// XML lets the UTF-8 byte sequence pass through.
    const ESCAPE_NBSP: bool;

    /// What to do with U+0000 (NULL) bytes in input.
    const NULL_BYTE: NullBytePolicy;

    /// Whether the dialect has raw-text element states (`<script>`, `<style>`).
    ///
    /// HTML supports these states. XML does not, so XML rejects script/style
    /// content-mode escaping.
    const HAS_RAW_TEXT_ELEMENTS: bool;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum EscapeDialect {
    Html,
    Xml,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum NullBytePolicy {
    /// Replace with U+FFFD. HTML default.
    ReplaceWithReplacementChar,
    /// Return `WriteError::InvalidCharForDialect`. XML default.
    Reject,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct HtmlPolicy;

impl sealed::Sealed for HtmlPolicy {}

impl EscapePolicy for HtmlPolicy {
    const ESCAPE_NBSP: bool = true;
    const NULL_BYTE: NullBytePolicy = NullBytePolicy::ReplaceWithReplacementChar;
    const HAS_RAW_TEXT_ELEMENTS: bool = true;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct XmlPolicy;

impl sealed::Sealed for XmlPolicy {}

impl EscapePolicy for XmlPolicy {
    const ESCAPE_NBSP: bool = false;
    const NULL_BYTE: NullBytePolicy = NullBytePolicy::Reject;
    const HAS_RAW_TEXT_ELEMENTS: bool = false;
}
