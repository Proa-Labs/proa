// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::mode::ContentMode;
use crate::shared::write_buf::{WriteBuf, WriteError};
use memchr::{memchr, memchr2, memchr3};

pub mod policy;

pub use policy::{EscapeDialect, EscapePolicy, HtmlPolicy, NullBytePolicy, XmlPolicy};

mod sealed {
    pub trait Sealed {}
}

/// Write a segment of source text to `buf`, applying the active dialect
/// policy for NBSP and NULL bytes.
///
/// `segment` must start and end on character boundaries of the source text.
/// Runs are only cut at a NUL byte or around a complete U+00A0, so every run
/// handed to the buffer is valid UTF-8 on its own.
#[inline(always)]
#[allow(unsafe_code)]
fn emit_with_policy<S, P>(buf: &mut S, segment: &[u8]) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
    P: EscapePolicy,
{
    // `start` is the first byte not yet written; `scan` is where the search
    // for the next special byte resumes.
    let mut start = 0;
    let mut scan = 0;
    while scan < segment.len() {
        let needle_hit = if P::ESCAPE_NBSP {
            memchr2(0x00, 0xC2, &segment[scan..])
        } else {
            memchr(0x00, &segment[scan..])
        };

        let Some(pos) = needle_hit else {
            break;
        };

        let abs = scan + pos;
        if segment[abs] == 0x00 {
            if abs > start {
                // SAFETY: cut before an ASCII NUL.
                unsafe { emit_source_run(buf, &segment[start..abs]) }?;
            }
            match P::NULL_BYTE {
                NullBytePolicy::ReplaceWithReplacementChar => {
                    buf.extend_static_str("\u{FFFD}")?;
                }
                NullBytePolicy::Reject => {
                    return Err(WriteError::InvalidCharForDialect);
                }
            }
            start = abs + 1;
            scan = start;
        } else {
            debug_assert!(P::ESCAPE_NBSP);
            if abs + 1 < segment.len() && segment[abs + 1] == 0xA0 {
                if abs > start {
                    // SAFETY: cut before the lead byte of U+00A0.
                    unsafe { emit_source_run(buf, &segment[start..abs]) }?;
                }
                buf.extend_static_str("&nbsp;")?;
                start = abs + 2;
                scan = start;
            } else {
                // Some other U+0080..U+00BF character. Leave it in the pending
                // run instead of ending a write on its lone lead byte.
                scan = abs + 1;
            }
        }
    }
    if start < segment.len() {
        // SAFETY: `start` is 0 or just past a complete character, and the
        // segment ends on a character boundary.
        unsafe { emit_source_run(buf, &segment[start..]) }?;
    }
    Ok(())
}

/// Pass a run of the source text through unchanged.
///
/// The escaper scans bytes, so its runs are byte slices of the `&str` being
/// escaped. Every cut point is either an ASCII byte or the end of a complete
/// character, so each run is valid UTF-8 on its own and needs no validation.
///
/// # Safety
///
/// `run` must be a sub-slice of a `&str` that starts and ends on character
/// boundaries.
#[inline(always)]
#[allow(unsafe_code)]
unsafe fn emit_source_run<S>(buf: &mut S, run: &[u8]) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
{
    debug_assert!(core::str::from_utf8(run).is_ok());
    // SAFETY: guaranteed by the caller.
    buf.extend_str(unsafe { core::str::from_utf8_unchecked(run) })
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text<B: WriteBuf>(buf: &mut B, s: &str, mode: ContentMode) -> Result<(), WriteError> {
    escape_text_in::<HtmlPolicy, B>(buf, s, mode)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_for_dialect<B: WriteBuf>(
    buf: &mut B,
    s: &str,
    mode: ContentMode,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    match dialect {
        EscapeDialect::Html => escape_text_in::<HtmlPolicy, B>(buf, s, mode),
        EscapeDialect::Xml => escape_text_xml_in_mode(buf, s, mode),
    }
}

/// The XML arm of [`escape_text_for_dialect`].
///
/// The dialect is only known at run time, so every dynamic text value carries
/// both arms. HTML is the common one and stays inline; this one is a call, so
/// that it adds a few bytes to each value's code instead of a second escaper.
#[inline(never)]
fn escape_text_xml_in_mode<B: WriteBuf>(
    buf: &mut B,
    s: &str,
    mode: ContentMode,
) -> Result<(), WriteError> {
    escape_text_in::<XmlPolicy, B>(buf, s, mode)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
    mode: ContentMode,
) -> Result<(), WriteError> {
    match mode {
        ContentMode::Html => escape_text_normal_in::<P, B>(buf, s),
        ContentMode::Script | ContentMode::Style => {
            if !P::HAS_RAW_TEXT_ELEMENTS {
                return Err(WriteError::RawTextUnsupportedForDialect);
            }
            match mode {
                ContentMode::Script => escape_text_with_sentinel_in::<P, B>(buf, s, b"script"),
                ContentMode::Style => escape_text_with_sentinel_in::<P, B>(buf, s, b"style"),
                ContentMode::Html => unreachable!(),
            }
        }
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_xml<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_text_normal_in::<XmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_attr_value_in::<HtmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value_for_dialect<B: WriteBuf>(
    buf: &mut B,
    s: &str,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    match dialect {
        EscapeDialect::Html => escape_attr_value_in::<HtmlPolicy, B>(buf, s),
        EscapeDialect::Xml => escape_attr_value_xml_call(buf, s),
    }
}

/// Write a whole attribute: `name_prefix` (` name="`), the escaped `value`,
/// and the closing quote.
///
/// A value that needs no escaping goes to the buffer together with the text
/// around it, as one append. Everything else is a call, so that the code each
/// dynamic attribute adds to a template stays small.
#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub(crate) fn write_attr_for_dialect<B: WriteBuf>(
    buf: &mut B,
    name_prefix: &'static str,
    value: &str,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    if matches!(dialect, EscapeDialect::Html)
        && buf.__append_if_clean(name_prefix, value, "\"", attr_classes::<HtmlPolicy>())?
    {
        return Ok(());
    }
    write_attr_escaping(buf, name_prefix, value, dialect)
}

/// Everything [`write_attr_for_dialect`] does not append itself: a value with
/// something to escape, a long value, a value at the end of a page, and XML.
#[inline(never)]
fn write_attr_escaping<B: WriteBuf>(
    buf: &mut B,
    name_prefix: &'static str,
    value: &str,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    buf.extend_static_str(name_prefix)?;
    match dialect {
        EscapeDialect::Html => {
            escape_scan::<_, HtmlPolicy>(buf, value, attr_classes::<HtmlPolicy>())?
        }
        EscapeDialect::Xml => escape_scan::<_, XmlPolicy>(buf, value, attr_classes::<XmlPolicy>())?,
    }
    buf.extend_str("\"")
}

/// The XML arm of [`escape_attr_value_for_dialect`], a call for the same
/// reason as [`escape_text_xml_in_mode`].
#[inline(never)]
fn escape_attr_value_xml_call<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_attr_value_in::<XmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    if buf.__append_if_clean("", s, "", attr_classes::<P>())? {
        return Ok(());
    }
    escape_attr_value_scan::<_, P>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value_xml<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_attr_value_in::<XmlPolicy, B>(buf, s)
}

/// The kinds of byte an escaper acts on.
///
/// Each kind is one bit, so a set of kinds is the OR of their values.
#[repr(u8)]
enum ByteClass {
    /// `&`, `<`, `>` or NUL, which every escaper rewrites or rejects.
    Markup = 1,
    /// `"` or `'`, which attribute values escape.
    Quote = 2,
    /// 0xC2, the first byte of U+00A0, which HTML rewrites.
    NbspLead = 4,
}

/// The [`ByteClass`] bit of every byte value. Zero means every escaper copies
/// the byte through unchanged.
///
/// The entries are plain bytes, not `ByteClass` values, so that a lookup
/// compiles to the same instructions as a table of integer constants.
pub(crate) const BYTE_CLASS: [u8; 256] = {
    let mut table = [0; 256];
    table[0x00] = ByteClass::Markup as u8;
    table[b'&' as usize] = ByteClass::Markup as u8;
    table[b'<' as usize] = ByteClass::Markup as u8;
    table[b'>' as usize] = ByteClass::Markup as u8;
    table[b'"' as usize] = ByteClass::Quote as u8;
    table[b'\'' as usize] = ByteClass::Quote as u8;
    table[0xC2] = ByteClass::NbspLead as u8;
    table
};

/// The set of byte classes that text escaping under `P` does anything with.
#[inline(always)]
const fn text_classes<P: EscapePolicy>() -> u8 {
    if P::ESCAPE_NBSP {
        ByteClass::Markup as u8 | ByteClass::NbspLead as u8
    } else {
        ByteClass::Markup as u8
    }
}

/// The set of byte classes that attribute-value escaping under `P` does
/// anything with.
#[inline(always)]
const fn attr_classes<P: EscapePolicy>() -> u8 {
    text_classes::<P>() | ByteClass::Quote as u8
}

/// Bytes examined in one step of a scan.
pub(crate) const CHUNK: usize = 16;

/// Whether any of 16 bytes has a class in a set, checked with one vector
/// comparison per group of special bytes instead of one table lookup per byte.
///
/// Every set of classes the escapers use holds [`ByteClass::Markup`], so these
/// functions look for the markup bytes unconditionally and for the other two
/// classes on request.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
pub(crate) mod chunk {
    use super::{ByteClass, CHUNK};
    use core::arch::x86_64::{
        __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };

    #[inline(always)]
    #[allow(unsafe_code)]
    pub(crate) fn has_class(chunk: &[u8; CHUNK], classes: u8) -> bool {
        debug_assert!(classes & ByteClass::Markup as u8 != 0);
        // SAFETY: this module is only compiled when SSE2 is enabled for the
        // whole build, and the load reads exactly the 16 bytes `chunk` borrows.
        unsafe {
            let bytes = _mm_loadu_si128(chunk.as_ptr().cast());
            let equals =
                |value: __m128i, byte: u8| _mm_cmpeq_epi8(value, _mm_set1_epi8(byte as i8));
            let with_bit = |bit: u8| _mm_or_si128(bytes, _mm_set1_epi8(bit as i8));
            // `<` is 0x3C and `>` is 0x3E. Setting bit 1 turns both into `>`
            // and nothing else into it, so one comparison finds either.
            let mut hit = _mm_or_si128(equals(with_bit(0x02), b'>'), equals(bytes, 0x00));
            hit = if classes & ByteClass::Quote as u8 != 0 {
                // `"` is 0x22 and `&` is 0x26, which bit 2 pairs the same way.
                let quote_or_amp = _mm_or_si128(equals(with_bit(0x04), b'&'), equals(bytes, b'\''));
                _mm_or_si128(hit, quote_or_amp)
            } else {
                _mm_or_si128(hit, equals(bytes, b'&'))
            };
            if classes & ByteClass::NbspLead as u8 != 0 {
                hit = _mm_or_si128(hit, equals(bytes, 0xC2));
            }
            _mm_movemask_epi8(hit) != 0
        }
    }
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub(crate) mod chunk {
    use super::{ByteClass, CHUNK};
    use core::arch::aarch64::{uint8x16_t, vceqq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8, vorrq_u8};

    #[inline(always)]
    #[allow(unsafe_code)]
    pub(crate) fn has_class(chunk: &[u8; CHUNK], classes: u8) -> bool {
        debug_assert!(classes & ByteClass::Markup as u8 != 0);
        // SAFETY: this module is only compiled when NEON is enabled for the
        // whole build, and the load reads exactly the 16 bytes `chunk` borrows.
        unsafe {
            let bytes = vld1q_u8(chunk.as_ptr());
            let equals = |value: uint8x16_t, byte: u8| vceqq_u8(value, vdupq_n_u8(byte));
            let with_bit = |bit: u8| vorrq_u8(bytes, vdupq_n_u8(bit));
            // `<` is 0x3C and `>` is 0x3E. Setting bit 1 turns both into `>`
            // and nothing else into it, so one comparison finds either.
            let mut hit = vorrq_u8(equals(with_bit(0x02), b'>'), equals(bytes, 0x00));
            hit = if classes & ByteClass::Quote as u8 != 0 {
                // `"` is 0x22 and `&` is 0x26, which bit 2 pairs the same way.
                let quote_or_amp = vorrq_u8(equals(with_bit(0x04), b'&'), equals(bytes, b'\''));
                vorrq_u8(hit, quote_or_amp)
            } else {
                vorrq_u8(hit, equals(bytes, b'&'))
            };
            if classes & ByteClass::NbspLead as u8 != 0 {
                hit = vorrq_u8(hit, equals(bytes, 0xC2));
            }
            vmaxvq_u8(hit) != 0
        }
    }
}

#[cfg(not(any(
    all(target_arch = "x86_64", target_feature = "sse2"),
    all(target_arch = "aarch64", target_feature = "neon"),
)))]
pub(crate) mod chunk {
    use super::{BYTE_CLASS, CHUNK};

    #[inline(always)]
    pub(crate) fn has_class(chunk: &[u8; CHUNK], classes: u8) -> bool {
        chunk
            .iter()
            .any(|&byte| BYTE_CLASS[byte as usize] & classes != 0)
    }
}

/// Whether any byte of `bytes` has a class in `classes`, so that escaping it
/// would do something other than copy it through.
///
/// 16 bytes or more are compared a chunk at a time. The last comparison is the
/// chunk that ends with the input, which overlaps the one before it unless
/// the length is a multiple of 16. Reading a byte twice does no harm, because
/// the answer is only whether some byte is special; where it is, and what
/// replaces it, is [`escape_scan`]'s job. Shorter inputs are looked up a byte
/// at a time.
#[inline(always)]
pub(crate) fn has_class(bytes: &[u8], classes: u8) -> bool {
    if let Some(last) = bytes.last_chunk::<CHUNK>() {
        let (chunks, _) = bytes.as_chunks::<CHUNK>();
        return chunks.iter().any(|chunk| chunk::has_class(chunk, classes))
            || chunk::has_class(last, classes);
    }
    bytes
        .iter()
        .any(|&byte| BYTE_CLASS[byte as usize] & classes != 0)
}

/// Escape an attribute value that was not appended as it is.
///
/// Not inlined: a value's own code is [`WriteBuf::__append_if_clean`], and
/// every value that needs more than that shares this function.
#[inline(never)]
fn escape_attr_value_scan<S, P>(buf: &mut S, s: &str) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
    P: EscapePolicy,
{
    escape_scan::<_, P>(buf, s, attr_classes::<P>())
}

#[cfg(test)]
fn escape_text_normal<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_text_normal_in::<HtmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
fn escape_text_normal_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    if buf.__append_if_clean("", s, "", text_classes::<P>())? {
        return Ok(());
    }
    escape_text_normal_scan::<_, P>(buf, s)
}

/// Escape element text that was not appended as it is. Not inlined, for the
/// same reason as [`escape_attr_value_scan`].
#[inline(never)]
fn escape_text_normal_scan<S, P>(buf: &mut S, s: &str) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
    P: EscapePolicy,
{
    escape_scan::<_, P>(buf, s, text_classes::<P>())
}

/// Escape `s`, replacing every byte whose class is in `classes`.
///
/// One pass: chunks with nothing to replace are stepped over whole, and the
/// bytes of a chunk that has something are looked up one at a time. Source
/// text between replacements is written in runs as long as possible, so a
/// value with nothing to replace is still a single write.
#[inline(always)]
#[allow(unsafe_code)]
fn escape_scan<S, P>(buf: &mut S, s: &str, classes: u8) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
    P: EscapePolicy,
{
    let bytes = s.as_bytes();
    // `bytes[..written]` is in `buf` already, as itself or as replacements.
    // It always ends on a character boundary: it only ever moves to just past
    // an ASCII byte or just past a complete U+00A0.
    let mut written = 0;
    let mut at = 0;
    while at < bytes.len() {
        let stop = match bytes[at..].first_chunk::<CHUNK>() {
            Some(chunk) if !chunk::has_class(chunk, classes) => {
                at += CHUNK;
                continue;
            }
            Some(_) => at + CHUNK,
            None => bytes.len(),
        };
        while at < stop {
            let special = at;
            let byte = bytes[special];
            at += 1;
            if BYTE_CLASS[byte as usize] & classes == 0 {
                continue;
            }
            let replacement = match byte {
                b'&' => Some("&amp;"),
                b'<' => Some("&lt;"),
                b'>' => Some("&gt;"),
                b'"' => Some("&quot;"),
                b'\'' => Some("&#39;"),
                0x00 => match P::NULL_BYTE {
                    NullBytePolicy::ReplaceWithReplacementChar => Some("\u{FFFD}"),
                    NullBytePolicy::Reject => None,
                },
                // The one classified byte left is 0xC2. It starts U+00A0 when
                // 0xA0 follows; before any other byte it starts some other
                // character in U+0080..U+00BF, which passes through.
                _ if bytes.get(at) == Some(&0xA0) => {
                    at += 1;
                    Some("&nbsp;")
                }
                _ => continue,
            };
            if special > written {
                // SAFETY: `written` is a character boundary (see above), and
                // `special` is one because it holds an ASCII byte or the
                // first byte of a two-byte character.
                unsafe { emit_source_run(buf, &bytes[written..special]) }?;
            }
            let Some(replacement) = replacement else {
                return Err(WriteError::InvalidCharForDialect);
            };
            buf.extend_static_str(replacement)?;
            written = at;
        }
    }
    if written < bytes.len() {
        // SAFETY: `written` is a character boundary (see above), and the run
        // ends where `s` does.
        unsafe { emit_source_run(buf, &bytes[written..]) }?;
    }
    Ok(())
}

/// Escape text inside a raw-text element (`<script>` / `<style>`).
///
/// `tag` is the lowercase element name (`b"script"` or `b"style"`). The
/// function masks three byte sequences that could transition the HTML
/// tokenizer out of the raw-text state or back into tag mode, covering the
/// Go `html/template` CVE series (CVE-2023-24538, CVE-2023-39318,
/// CVE-2023-39319) of script-context escape bypasses:
///
/// - `</tag` followed by a tag-ending char (`>`, whitespace, `/`) — closes
///   the raw-text element (HTML5 §13.2.5.17). Masked to `<\/tag…`.
/// - `<tag` followed by a tag-ending char — transitions into
///   script-data-escaped / script-data-double-escaped (HTML5 §13.2.5.27).
///   Masked to `<\tag…`.
/// - `<!--` — opens the script-data-escaped state (HTML5 §13.2.5.18).
///   Masked to `<\!--`.
///
/// `-->` does NOT need its own mask: every bare `>` byte in the input is
/// escaped to `&gt;`, so the byte sequence `-->` can never appear in the
/// output. `>` escape + `<!--` mask together make the `-->` tokenizer
/// transition unreachable (see HTML5 §13.2.5.22 — you only observe `-->`
/// once script-data-escaped state is entered, which requires an unmasked
/// `<!--`).
///
/// All masks insert a single `\` byte. In a JS string literal, `\x` where
/// `x` is not a recognized escape is interpreted as `x` — so the decoded
/// string value is unchanged. Outside a string, interpolation inside
/// `<script>` already triggers the Gap B compile-time warning, so the
/// developer has been steered away from that path entirely.
///
/// CVE review Gap P. Complements Gap B (compile-time warning).
///
/// Not inlined: dynamic text inside `<script>` or `<style>` is rare, and the
/// content mode is only known at run time, so every dynamic text value would
/// otherwise carry a copy of this loop.
#[inline(never)]
#[allow(unsafe_code)]
fn escape_text_with_sentinel_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
    tag: &[u8],
) -> Result<(), WriteError> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut flush_start = 0;

    while i < bytes.len() {
        let rest = &bytes[i..];

        // Fast path: scan ahead for the next byte that requires handling.
        // Every `&`, `<`, `>` is escaped to its HTML entity, and `<` can
        // additionally trigger a sentinel mask. Bytes not in that set pass
        // through unchanged and get flushed when the next trigger is found.
        let Some(off) = memchr3(b'&', b'<', b'>', rest) else {
            // No more trigger bytes — fall through to the final flush.
            break;
        };
        i += off;

        // i now points at a trigger byte. Check sentinel masks first.
        if bytes[i] == b'<' {
            // `</tag` closing tag (case-insensitive), with tag-end lookahead.
            if starts_with_close_tag(&bytes[i..], tag) {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("<\\/")?;
                let tag_end = i + 2 + tag.len();
                // SAFETY: the matched tag name is ASCII.
                unsafe { emit_source_run(buf, &bytes[i + 2..tag_end]) }?;
                i = tag_end;
                flush_start = i;
                continue;
            }
            // `<tag` opening tag (case-insensitive), with tag-end lookahead.
            if starts_with_open_tag(&bytes[i..], tag) {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("<\\")?;
                let tag_end = i + 1 + tag.len();
                // SAFETY: the matched tag name is ASCII.
                unsafe { emit_source_run(buf, &bytes[i + 1..tag_end]) }?;
                i = tag_end;
                flush_start = i;
                continue;
            }
            // `<!--` HTML comment start.
            if bytes[i..].starts_with(b"<!--") {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("<\\!--")?;
                i += 4;
                flush_start = i;
                continue;
            }
        }

        // Regular HTML escape for `&`, `<`, `>`.
        match bytes[i] {
            b'&' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("&amp;")?;
            }
            b'<' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("&lt;")?;
            }
            b'>' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static_str("&gt;")?;
            }
            _ => unreachable!(),
        }
        i += 1;
        flush_start = i;
    }

    // Final flush of any trailing pass-through region.
    if flush_start < bytes.len() {
        emit_with_policy::<_, P>(buf, &bytes[flush_start..])?;
    }
    Ok(())
}

/// Flush the pass-through region `[start, end)` into `buf`.
#[inline(always)]
fn flush_impl<S, P>(buf: &mut S, bytes: &[u8], start: usize, end: usize) -> Result<(), WriteError>
where
    S: ?Sized + WriteBuf,
    P: EscapePolicy,
{
    if start < end {
        emit_with_policy::<_, P>(buf, &bytes[start..end])
    } else {
        Ok(())
    }
}

/// `</tag` followed by a tag-ending character (space, tab, LF, CR, FF, `/`,
/// `>`, or EOF). Matches HTML5 §13.2.5.17 end-tag recognition in raw-text
/// element states — the tokenizer accepts any of these as the delimiter
/// after the tag name, so the escaper must recognize them all.
#[inline]
fn starts_with_close_tag(input: &[u8], tag: &[u8]) -> bool {
    if input.len() < 2 + tag.len() {
        return false;
    }
    if &input[..2] != b"</" {
        return false;
    }
    if !input[2..2 + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    match input.get(2 + tag.len()) {
        None => true,
        Some(&c) => is_tag_end_char(c),
    }
}

/// `<tag` followed by a tag-ending character.
#[inline]
fn starts_with_open_tag(input: &[u8], tag: &[u8]) -> bool {
    if input.len() < 1 + tag.len() {
        return false;
    }
    if input[0] != b'<' {
        return false;
    }
    if !input[1..1 + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    match input.get(1 + tag.len()) {
        None => true,
        Some(&c) => is_tag_end_char(c),
    }
}

#[inline(always)]
fn is_tag_end_char(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'\n' | b'\r' | 0x0C /* FF */ | b'/' | b'>'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::ContentMode;
    use proptest::prelude::*;

    fn any_string() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..256)
            .prop_map(|chars| chars.into_iter().collect::<String>())
    }

    /// A biased generator that mixes arbitrary characters with occasional
    /// injected sentinel-adjacent substrings. Random `any::<char>()` runs
    /// almost never produce `</script` or `<!--` by chance, so we inject
    /// them explicitly to exercise the mask paths that matter.
    fn any_string_with_sentinel_fragments() -> impl Strategy<Value = String> {
        let fragments: Vec<&'static str> = vec![
            "</script>",
            "</script ",
            "</SCRIPT>",
            "</script/",
            "<script>",
            "<script ",
            "<SCRIPT src=x>",
            "<!--",
            "-->",
            "<!-->",
            "<\\/script>",
            "<\\script>",
            "<\\!--",
            "--\\>",
            "a",
            "-",
            "\u{00A0}",
            "\0",
            ">",
        ];
        proptest::collection::vec(proptest::sample::select(fragments), 0..24)
            .prop_map(|parts| parts.concat())
    }

    fn basic_unescape(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut i = 0;
        let bytes = input.as_bytes();
        while i < bytes.len() {
            // Reverse any `<\X` sentinel mask (covers `<\/script`, `<\/style`,
            // `<\script`, `<\style`, `<\!--`). The escape layer guarantees
            // that `<\` only appears in its output as the product of masking
            // because every bare `<` in the input is escaped to `&lt;`.
            if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                out.push('<');
                i += 2;
                continue;
            }
            if bytes[i] == b'&' {
                if input[i..].starts_with("&amp;") {
                    out.push('&');
                    i += 5;
                    continue;
                }
                if input[i..].starts_with("&lt;") {
                    out.push('<');
                    i += 4;
                    continue;
                }
                if input[i..].starts_with("&gt;") {
                    out.push('>');
                    i += 4;
                    continue;
                }
                if input[i..].starts_with("&quot;") {
                    out.push('"');
                    i += 6;
                    continue;
                }
                if input[i..].starts_with("&#39;") {
                    out.push('\'');
                    i += 5;
                    continue;
                }
                if input[i..].starts_with("&nbsp;") {
                    out.push('\u{00A0}');
                    i += 6;
                    continue;
                }
            }
            let ch = input[i..].chars().next().expect("valid UTF-8");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// NULL→FFFD is lossy, so roundtrip comparisons use a normalized original.
    fn normalize_nulls(s: &str) -> String {
        s.replace('\0', "\u{FFFD}")
    }

    /// Assert that the escaped output is safe to embed inside a `<script>`
    /// raw-text element. None of the following HTML5 tokenizer state
    /// transitions must be reachable in the byte stream:
    ///
    /// - `</script` followed by a tag-end char (§13.2.5.17) closes the
    ///   element.
    /// - `<script` followed by a tag-end char (§13.2.5.19) transitions
    ///   into script-data-escaped-double state.
    /// - `<!--` (§13.2.5.18) opens script-data-escaped state.
    /// - `-->` (§13.2.5.22) closes script-data-escaped state.
    ///
    /// All four must appear ONLY in their masked forms after escaping.
    fn assert_script_mode_safe(escaped: &str) {
        assert_no_raw_tag_close(escaped, "script");
        assert_no_raw_tag_open(escaped, "script");
        assert!(
            !escaped.contains("<!--"),
            "raw <!-- in script-mode output: {escaped:?}"
        );
        assert!(
            !escaped.contains("-->"),
            "raw --> in script-mode output: {escaped:?}"
        );
    }

    fn assert_style_mode_safe(escaped: &str) {
        assert_no_raw_tag_close(escaped, "style");
        assert_no_raw_tag_open(escaped, "style");
        assert!(
            !escaped.contains("<!--"),
            "raw <!-- in style-mode output: {escaped:?}"
        );
        assert!(
            !escaped.contains("-->"),
            "raw --> in style-mode output: {escaped:?}"
        );
    }

    /// Check that `</tag` followed by a tag-end char does not appear
    /// anywhere in the output. The scan is case-insensitive, matching
    /// HTML5 end-tag recognition.
    fn assert_no_raw_tag_close(escaped: &str, tag: &str) {
        let needle = format!("</{tag}");
        let lower = escaped.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut pos = 0;
        while let Some(off) = lower[pos..].find(&needle) {
            let start = pos + off;
            let after = bytes.get(start + needle.len()).copied();
            if is_html5_tag_end(after) {
                panic!("raw </{tag}<end-char> in output at byte {start}: {escaped:?}");
            }
            pos = start + 1;
        }
    }

    /// Same for `<tag` open-tag start.
    fn assert_no_raw_tag_open(escaped: &str, tag: &str) {
        let needle = format!("<{tag}");
        let lower = escaped.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut pos = 0;
        while let Some(off) = lower[pos..].find(&needle) {
            let start = pos + off;
            // Skip `</tag` — that's the close-tag case, already checked.
            if start > 0
                && bytes.get(start - 1) == Some(&b'/')
                && bytes.get(start.wrapping_sub(2)) == Some(&b'<')
            {
                pos = start + 1;
                continue;
            }
            let after = bytes.get(start + needle.len()).copied();
            if is_html5_tag_end(after) {
                panic!("raw <{tag}<end-char> in output at byte {start}: {escaped:?}");
            }
            pos = start + 1;
        }
    }

    fn is_html5_tag_end(c: Option<u8>) -> bool {
        match c {
            None => true,
            Some(b) => matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | b'/' | b'>'),
        }
    }

    proptest! {
        #[test]
        fn text_html_roundtrips(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Html).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_script_masks_sentinel(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Script).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_script_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_style_masks_sentinel(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Style).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_style_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        /// Same two properties, but exercised against a biased generator
        /// that frequently injects `</script`, `<!--`, `-->`, etc. The
        /// unbiased test almost never generates these substrings so the
        /// Gap P mask paths would otherwise go unexercised under proptest.
        #[test]
        fn text_script_sentinel_biased(original in any_string_with_sentinel_fragments()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Script).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_script_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_style_sentinel_biased(original in any_string_with_sentinel_fragments()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Style).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_style_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn attr_roundtrips(original in any_string()) {
            let mut buf = Vec::new();
            escape_attr_value(&mut buf, &original).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            prop_assert!(!escaped.contains('"'));
            prop_assert!(!escaped.contains('\''));
            prop_assert!(!escaped.contains('<'));
            prop_assert!(!escaped.contains('>'));
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }
    }

    #[test]
    fn nbsp_only_text() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00A0}").unwrap();
        assert_eq!(buf, b"&nbsp;");
    }

    #[test]
    fn nbsp_mixed_with_html_chars() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "a\u{00A0}<b>\u{00A0}&c").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "a&nbsp;&lt;b&gt;&nbsp;&amp;c"
        );
    }

    #[test]
    fn nbsp_consecutive() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00A0}\u{00A0}\u{00A0}").unwrap();
        assert_eq!(buf, b"&nbsp;&nbsp;&nbsp;");
    }

    #[test]
    fn nbsp_0xc2_not_followed_by_0xa0() {
        // U+00BF = [0xC2, 0xBF] — should NOT be escaped
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00BF}").unwrap();
        assert_eq!(buf, "\u{00BF}".as_bytes());
    }

    #[test]
    fn nbsp_in_attr_value() {
        let mut buf = Vec::new();
        escape_attr_value(&mut buf, "hello\u{00A0}world").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "hello&nbsp;world");
    }

    #[test]
    fn nbsp_in_script_sentinel() {
        let mut buf = Vec::new();
        escape_text(&mut buf, "\u{00A0}", ContentMode::Script).unwrap();
        assert_eq!(buf, b"&nbsp;");
    }

    #[test]
    fn null_replaced_with_fffd_in_text() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "a\0b").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{FFFD}b");
    }

    #[test]
    fn null_replaced_with_fffd_in_attr() {
        let mut buf = Vec::new();
        escape_attr_value(&mut buf, "a\0b").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{FFFD}b");
    }

    #[test]
    fn null_only() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\0").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\u{FFFD}");
    }

    #[test]
    fn null_and_nbsp_mixed() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\0\u{00A0}\0").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\u{FFFD}&nbsp;\u{FFFD}");
    }

    #[test]
    fn xml_text_preserves_nbsp_and_escapes_markup() {
        let mut buf = Vec::new();
        escape_text_xml(&mut buf, "a\u{00A0}<b>&c").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{00A0}&lt;b&gt;&amp;c");
    }

    #[test]
    fn xml_attr_preserves_nbsp_and_escapes_quotes() {
        let mut buf = Vec::new();
        escape_attr_value_xml(&mut buf, "a\u{00A0}\"b'c<d>&").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "a\u{00A0}&quot;b&#39;c&lt;d&gt;&amp;"
        );
    }

    #[test]
    fn xml_text_rejects_null() {
        let mut buf = Vec::new();
        let err = escape_text_xml(&mut buf, "a\0b").unwrap_err();
        assert_eq!(err, WriteError::InvalidCharForDialect);
    }

    #[test]
    fn xml_attr_rejects_null() {
        let mut buf = Vec::new();
        let err = escape_attr_value_xml(&mut buf, "a\0b").unwrap_err();
        assert_eq!(err, WriteError::InvalidCharForDialect);
    }

    #[test]
    fn xml_policy_rejects_raw_text_modes() {
        let mut buf = Vec::new();
        let err =
            escape_text_in::<XmlPolicy, _>(&mut buf, "body", ContentMode::Script).unwrap_err();
        assert_eq!(err, WriteError::RawTextUnsupportedForDialect);
    }

    // -----------------------------------------------------------------
    // CVE review Gap P — script/style sentinel completeness.
    // Each test covers a Go html/template CVE precedent (CVE-2023-39318,
    // CVE-2023-39319, CVE-2023-24538) plus the new close-tag-with-attrs
    // case.
    // -----------------------------------------------------------------

    fn script_escape(s: &str) -> String {
        let mut buf = Vec::new();
        escape_text(&mut buf, s, ContentMode::Script).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn style_escape(s: &str) -> String {
        let mut buf = Vec::new();
        escape_text(&mut buf, s, ContentMode::Style).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn script_sentinel_exact_close_tag() {
        let out = script_escape("</script>");
        assert!(!out.contains("</script>"));
        assert!(out.contains(r"<\/script"));
    }

    #[test]
    fn script_sentinel_close_tag_with_whitespace() {
        // CVE-2023-39319 shape: `</script ` with trailing whitespace
        // (attribute list) is still a valid end tag per HTML5 §13.2.5.17.
        let out = script_escape("</script foo=bar>");
        assert!(!out.contains("</script"));
        assert!(out.contains(r"<\/script"));
    }

    #[test]
    fn script_sentinel_close_tag_case_insensitive() {
        let out = script_escape("</SCRIPT>");
        assert!(!out.to_ascii_lowercase().contains("</script"));
        assert!(out.contains(r"<\/SCRIPT"));
    }

    #[test]
    fn script_sentinel_open_tag_case_insensitive() {
        // CVE-2023-39318: nested `<script` in script-data-escaped state.
        let out = script_escape("<script src=x>");
        // The HTML tokenizer's script-data-double-escape transition needs
        // `<script` followed by a tag-end char. Masking to `<\script` breaks
        // the transition without corrupting JS string semantics.
        assert!(!out.contains("<script"));
        assert!(out.contains(r"<\script"));
    }

    #[test]
    fn script_sentinel_open_tag_not_masked_when_tag_name_continues() {
        // `<scripted` is NOT a script start-tag (per tokenizer); it must
        // pass through via the `<` → `&lt;` path unchanged.
        let out = script_escape("<scripted>");
        assert!(!out.contains(r"<\scripted"));
        assert!(out.contains("&lt;scripted"));
    }

    #[test]
    fn script_sentinel_html_comment_start() {
        // CVE-2023-39318 precedent: `<!--` inside a JS string transitions
        // into script-data-escaped state.
        let out = script_escape("<!-- comment -->");
        assert!(!out.contains("<!--"));
        assert!(out.contains(r"<\!--"));
    }

    #[test]
    fn script_sentinel_comment_end_unreachable_via_gt_escape() {
        // No explicit `-->` mask is needed: the trailing `>` is always
        // escaped to `&gt;`, so `-->` can never appear in the output even
        // without an `<!--` mask (which we still mask as a belt-and-braces
        // measure in `script_sentinel_html_comment_start`).
        let out = script_escape("<!-- comment -->");
        assert!(!out.contains("-->"));
        // The trailing `>` becomes `&gt;`.
        assert!(out.contains("--&gt;"));
    }

    #[test]
    fn script_sentinel_bare_dash_not_masked() {
        // `&nbsp;` escape in `emit_with_nbsp_null_escape` doesn't touch the
        // `-` byte, but `&` itself is a trigger — assert the output is what
        // we expect byte-for-byte (spaces pass through, `-` passes through).
        let out = script_escape("a - b");
        assert_eq!(out, "a - b");
    }

    #[test]
    fn style_sentinel_covers_style_tag() {
        let out = style_escape("</style>");
        assert!(!out.contains("</style>"));
        assert!(out.contains(r"<\/style"));
    }

    #[test]
    fn style_sentinel_covers_nested_style_open() {
        let out = style_escape("<style>");
        assert!(!out.contains("<style>"));
        assert!(out.contains(r"<\style"));
    }

    #[test]
    fn script_mode_does_not_mask_style_tag() {
        // Only the active tag name is treated as a sentinel.
        let out = script_escape("</style>");
        assert!(out.contains("&lt;/style"));
    }

    #[test]
    fn script_sentinel_all_patterns_combined() {
        let input = "a<script>x</script>b<!--c-->d";
        let out = script_escape(input);
        // None of the raw tokens survive.
        assert!(!out.contains("<script>"));
        assert!(!out.contains("</script>"));
        assert!(!out.contains("<!--"));
        assert!(!out.contains("-->"));
        // `<script`, `</script`, `<!--` show up in masked form.
        assert!(out.contains(r"<\script"));
        assert!(out.contains(r"<\/script"));
        assert!(out.contains(r"<\!--"));
        // `-->` is unreachable because the trailing `>` is always escaped.
        assert!(out.contains("--&gt;"));
    }

    /// Records each write an escaper makes, so a test can tell one write from
    /// two, and no write from an empty one.
    #[derive(Debug, Default, PartialEq)]
    struct RecordedWrites(Vec<String>);

    impl WriteBuf for RecordedWrites {
        fn capacity(&self) -> usize {
            usize::MAX
        }

        fn len(&self) -> usize {
            self.0.iter().map(String::len).sum()
        }

        fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
            self.0.push(text.to_owned());
            Ok(())
        }

        fn as_slice(&self) -> &[u8] {
            unimplemented!("tests read the recorded writes")
        }

        fn to_vec(&self) -> Vec<u8> {
            self.0.concat().into_bytes()
        }

        fn clear(&mut self) {
            self.0.clear();
        }
    }

    /// The escaping rules applied one character at a time, with no scanning,
    /// for the escapers to be checked against.
    fn escaped_by_rule<P: EscapePolicy>(input: &str, attr: bool) -> Result<String, WriteError> {
        let mut out = String::new();
        for character in input.chars() {
            match character {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' if attr => out.push_str("&quot;"),
                '\'' if attr => out.push_str("&#39;"),
                '\0' => match P::NULL_BYTE {
                    NullBytePolicy::ReplaceWithReplacementChar => out.push('\u{FFFD}'),
                    NullBytePolicy::Reject => return Err(WriteError::InvalidCharForDialect),
                },
                '\u{00A0}' if P::ESCAPE_NBSP => out.push_str("&nbsp;"),
                other => out.push(other),
            }
        }
        Ok(out)
    }

    /// Escape `input` the three ways a value can be escaped, and check each
    /// against [`escaped_by_rule`]:
    ///
    /// - through the entry point into a sink with no append of its own, which
    ///   checks the value and then writes or scans it;
    /// - through the scanning loop alone;
    /// - through the entry point into a page writer, which checks short values
    ///   as it copies them. Its pages are 48 bytes and it starts `lead` bytes
    ///   into one, for each of `leads`, so values land on the end of a page.
    fn assert_escapes_by_rule<P: EscapePolicy>(input: &str, attr: bool, leads: &[usize]) {
        let expected = escaped_by_rule::<P>(input, attr);
        let context = || format!("attr={attr}, input={input:?}");

        let mut entry = RecordedWrites::default();
        let entered = if attr {
            escape_attr_value_in::<P, _>(&mut entry, input)
        } else {
            escape_text_normal_in::<P, _>(&mut entry, input)
        };
        let mut scan = RecordedWrites::default();
        let scanned = if attr {
            escape_attr_value_scan::<_, P>(&mut scan, input)
        } else {
            escape_text_normal_scan::<_, P>(&mut scan, input)
        };
        match &expected {
            Ok(expected) => {
                assert_eq!(entered, Ok(()), "{}", context());
                assert_eq!(scanned, Ok(()), "{}", context());
                assert_eq!(&entry.0.concat(), expected, "{}", context());
                assert_eq!(&scan.0.concat(), expected, "{}", context());
                // A value that needs nothing is one write, or none when empty.
                if expected == input {
                    let writes = usize::from(!input.is_empty());
                    assert_eq!(entry.0.len(), writes, "{}", context());
                    assert_eq!(scan.0.len(), writes, "{}", context());
                }
            }
            Err(error) => {
                assert_eq!(entered.as_ref(), Err(error), "{}", context());
                assert_eq!(scanned.as_ref(), Err(error), "{}", context());
            }
        }

        for &lead in leads {
            let mut pages = crate::shared::write_buf::ChainedBumpWriter::<48>::new();
            WriteBuf::extend_str(&mut pages, &"-".repeat(lead)).unwrap();
            let paged = if attr {
                escape_attr_value_in::<P, _>(&mut pages, input)
            } else {
                escape_text_normal_in::<P, _>(&mut pages, input)
            };
            match &expected {
                Ok(expected) => {
                    assert_eq!(paged, Ok(()), "lead={lead}, {}", context());
                    let written = String::from_utf8(pages.to_vec()).unwrap();
                    assert_eq!(&written[lead..], expected, "lead={lead}, {}", context());
                }
                Err(error) => {
                    assert_eq!(paged.as_ref(), Err(error), "lead={lead}, {}", context())
                }
            }
        }
    }

    fn assert_escapes_by_rule_everywhere(input: &str, leads: &[usize]) {
        for attr in [false, true] {
            assert_escapes_by_rule::<HtmlPolicy>(input, attr, leads);
            assert_escapes_by_rule::<XmlPolicy>(input, attr, leads);
        }
    }

    #[test]
    fn empty_input_makes_no_write() {
        assert_escapes_by_rule_everywhere("", &[0, 47]);
    }

    #[test]
    fn every_character_escapes_by_rule_at_every_length() {
        // U+0000..=U+07FF holds every ASCII byte and every two-byte lead byte,
        // so it covers each special character, U+00A0, and the other
        // characters that start with 0xC2. The prefix lengths put the
        // character in each of the short-value cases (1 to 3 bytes, 4 to 7,
        // 8 to 15), on both sides of every 16-byte chunk boundary up to 64,
        // and past the 64-byte limit of the copying check.
        let characters =
            (0..=0x7FF)
                .filter_map(char::from_u32)
                .chain(['\u{20AC}', '\u{FFFD}', '\u{1F600}']);
        let prefix_lengths = [
            0, 1, 2, 3, 4, 7, 8, 15, 16, 17, 31, 32, 33, 47, 48, 63, 64, 65, 80,
        ];
        for character in characters {
            for prefix_length in prefix_lengths {
                for suffix in ["", "z"] {
                    let input = format!("{}{character}{suffix}", "p".repeat(prefix_length));
                    assert_escapes_by_rule_everywhere(&input, &[0, 41]);
                }
            }
        }
    }

    #[test]
    fn values_escape_by_rule_at_every_offset_into_a_page() {
        let every_offset: Vec<usize> = (0..48).collect();
        for value in [
            "x",
            "card",
            "a-clean-value",
            "seventeen-bytes-x",
            "a clean value that is longer than one whole 48-byte page of output",
            "&",
            "a<b",
            "say \"hi\" & 'bye'",
            "caf\u{00E9}\u{00A0}au\u{00A0}lait",
            "nul\0byte",
            "a value over sixty-four bytes long with one <special> character in the middle",
        ] {
            assert_escapes_by_rule_everywhere(value, &every_offset);
        }
    }

    #[test]
    fn attribute_is_written_whole_or_escaped() {
        for (value, expected) in [
            ("", " class=\"\""),
            ("card", " class=\"card\""),
            ("a \"quoted\" card", " class=\"a &quot;quoted&quot; card\""),
            (
                "flex items-center gap-2 rounded-md border px-3 py-1 text-sm font-medium shadow-sm",
                " class=\"flex items-center gap-2 rounded-md border px-3 py-1 text-sm font-medium shadow-sm\"",
            ),
        ] {
            // Every offset into a page, so the attribute also straddles one.
            for lead in 0..48 {
                let mut pages = crate::shared::write_buf::ChainedBumpWriter::<48>::new();
                WriteBuf::extend_str(&mut pages, &"-".repeat(lead)).unwrap();
                write_attr_for_dialect(&mut pages, " class=\"", value, EscapeDialect::Html)
                    .unwrap();
                let written = String::from_utf8(pages.to_vec()).unwrap();
                assert_eq!(&written[lead..], expected, "lead={lead}");
            }
            let mut plain = Vec::new();
            write_attr_for_dialect(&mut plain, " class=\"", value, EscapeDialect::Html).unwrap();
            assert_eq!(String::from_utf8(plain).unwrap(), expected);
        }

        let mut xml = Vec::new();
        write_attr_for_dialect(&mut xml, " title=\"", "a\u{00A0}<b>", EscapeDialect::Xml).unwrap();
        assert_eq!(
            String::from_utf8(xml).unwrap(),
            " title=\"a\u{00A0}&lt;b&gt;\""
        );
        let mut xml = Vec::new();
        assert_eq!(
            write_attr_for_dialect(&mut xml, " title=\"", "a\0b", EscapeDialect::Xml),
            Err(WriteError::InvalidCharForDialect)
        );
    }

    #[test]
    fn chunk_check_agrees_with_the_class_table() {
        let markup = ByteClass::Markup as u8;
        let quote = ByteClass::Quote as u8;
        let nbsp = ByteClass::NbspLead as u8;
        for classes in [markup, markup | nbsp, markup | quote, markup | quote | nbsp] {
            assert!(!chunk::has_class(&[b'a'; CHUNK], classes));
            for byte in 0..=u8::MAX {
                for position in 0..CHUNK {
                    let mut bytes = [b'a'; CHUNK];
                    bytes[position] = byte;
                    assert_eq!(
                        chunk::has_class(&bytes, classes),
                        BYTE_CLASS[byte as usize] & classes != 0,
                        "byte={byte:#04x}, position={position}, classes={classes:#05b}"
                    );
                }
            }
        }
    }

    proptest! {
        #[test]
        fn strings_escape_by_rule(
            characters in proptest::collection::vec(any::<char>(), 0..96),
            specials in proptest::collection::vec(
                proptest::sample::select(vec!['&', '<', '>', '"', '\'', '\0', '\u{00A0}', '\u{00A9}']),
                0..6,
            ),
        ) {
            // Interleave so that several special characters land in one
            // string, which arbitrary characters alone almost never produce.
            let mut input = String::new();
            let mut specials = specials.into_iter();
            for character in characters {
                input.push(character);
                input.extend(specials.next());
            }
            input.extend(specials);
            assert_escapes_by_rule_everywhere(&input, &[0, 23, 41]);
        }
    }
}
