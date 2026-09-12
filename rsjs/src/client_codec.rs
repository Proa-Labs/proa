// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Client-boundary codec traits and metadata vocabulary.
//!
//! Types that cross the Rust/JS boundary inside an RSJS-analyzed component
//! (signal initial values, query/mutation arguments, handler captures,
//! and custom-element props) must implement [`ClientEncode`] and
//! [`ClientDecode`]. These are *not* `serde::Serialize` /
//! `Deserialize`; they are a parallel codec specifically for the
//! rsjs hydration / handler-capture wire format.
//!
//! # Why a separate trait
//!
//! `serde::Serialize` is observable across the whole ecosystem. If
//! deriving the rsjs codec generated a `Serialize` impl, the type
//! would suddenly serialise wide integers as JSON strings everywhere,
//! breaking unrelated JSON-RPC / API endpoints. The rsjs codec is
//! scoped to rsjs's hydration code paths and never leaks beyond them.
//!
//! # Wire format
//!
//! - `bool` → JSON `true` / `false`.
//! - 32-bit and narrower integers → JSON number.
//! - `f32`, `f64` → JSON number.
//! - `i64`, `u64`, `i128`, `u128`, `isize`, `usize` → **JSON string**
//!   (the BigInt convention: preserves precision past 2^53 on the
//!   client side, where these decode to `BigInt`).
//! - `String`, `str`, and references to string-like values → JSON string with the standard escape rules
//!   (`\"`, `\\`, `\n`, `\r`, `\t`, `\b`, `\f`, and `\u00XX` for
//!   control codepoints below `0x20`).
//! - `Option<T>` → JSON `null` for `None`, `T`'s shape for `Some`.
//! - `Vec<T>` and borrowed slices → JSON array of `T`'s shape.
//! - Struct with `#[derive(rsjs::ClientProps)]` → JSON object,
//!   fields in declaration order.
//! - Fieldless enum with `#[derive(rsjs::ClientProps)]` → JSON string
//!   containing the exact Rust variant name. Variant order and numeric
//!   discriminants do not affect the wire value.
//!
//! # Sibling-const convention
//!
//! Every `#[derive(ClientProps)]` emits a sibling const named
//! `__rsjs_codec_meta_<StructName>` of type
//! `&'static ClientPropsMeta`. Downstream macros (`#[rsjs::client]`,
//! `#[rsjs(component...)]`, query/mutation lowering) look this up by Rust name
//! resolution rather than by introspecting trait associated consts,
//! because proc macros run before type resolution and cannot read
//! associated consts.

use core::fmt::Write as _;

/// Function generated for every `#[rsjs::client]` helper that evaluates
/// the helper body against already-captured server arguments.
pub type ClientHelperServerEval =
    fn(args: &[(&'static str, crate::__runtime::ClientArg)]) -> crate::__runtime::ClientValue;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Encode `self` to the rsjs wire format (a valid JSON value).
///
/// Custom string encoders should use [`write_json_string`]. Island seed
/// emission additionally escapes HTML script-data hazards at the final output
/// boundary, including for custom implementations.
pub trait ClientEncode {
    /// Append the wire-format JSON for `self` to `out`.
    fn encode_into(&self, out: &mut String);

    /// Encode `self` to a fresh string. Convenience wrapper.
    fn encode(&self) -> String {
        let mut out = String::new();
        self.encode_into(&mut out);
        out
    }
}

/// Decode a value of `Self` from the rsjs wire format.
pub trait ClientDecode: Sized {
    /// Parse `s` as the wire-format JSON for `Self`.
    ///
    /// On success, returns `(value, bytes_consumed)`. The two-value
    /// shape lets aggregate decoders (struct, `Vec`, `Option`) chain
    /// child parses without re-scanning. `decode` is the
    /// whole-string convenience wrapper that errors on trailing
    /// content.
    fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError>;

    /// Decode `s` in full. Errors if any non-whitespace content
    /// remains after the value.
    fn decode(s: &str) -> Result<Self, ClientDecodeError> {
        let (value, consumed) = Self::decode_partial(s)?;
        let rest = &s[consumed..];
        if rest
            .bytes()
            .any(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            return Err(ClientDecodeError {
                msg: "trailing content after rsjs codec value",
            });
        }
        Ok(value)
    }
}

/// Error returned by [`ClientDecode`] when the input does not match
/// the expected wire-format shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientDecodeError {
    /// Static description of what went wrong. Pointer-equal across
    /// instances of the same error site, suitable for embedding in
    /// generated code.
    pub msg: &'static str,
}

impl core::fmt::Display for ClientDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("rsjs client decode error: ")?;
        f.write_str(self.msg)
    }
}

impl std::error::Error for ClientDecodeError {}

// ---------------------------------------------------------------------------
// Metadata vocabulary (closed)
// ---------------------------------------------------------------------------

/// The closed vocabulary of field types the rsjs codec supports.
///
/// This vocabulary is the contract between the derive (`rsjs_macros`),
/// the client codegen (`rsjs_ir::emit_js`), and the JS runtime. Adding
/// a new variant requires a paired runtime change.
#[derive(Debug, Clone, Copy)]
pub enum ClientPropsFieldType {
    /// Rust `bool`.
    Bool,
    /// Narrow signed integer types: `i8`, `i16`, `i32`. JSON number.
    I32,
    /// Narrow unsigned integer types: `u8`, `u16`, `u32`. JSON number.
    U32,
    /// Wide signed integer: `i64`, `i128`, `isize`. JSON string.
    I64,
    /// Wide unsigned integer: `u64`, `u128`, `usize`. JSON string.
    U64,
    /// Rust `f32`. JSON number.
    F32,
    /// Rust `f64`. JSON number.
    F64,
    /// `String`. JSON string.
    String,
    /// `Option<T>`. JSON `null` or `T`'s shape.
    OptionOf(&'static ClientPropsFieldType),
    /// `Vec<T>`. JSON array.
    VecOf(&'static ClientPropsFieldType),
    /// Another `ClientProps`-derived type (object struct or tagged enum).
    NestedClientProps(&'static ClientPropsMeta),
}

/// Top-level wire shape generated by `#[derive(ClientProps)]`.
///
/// A fieldless Rust enum is represented as a tagged JSON string. The listed
/// tags are the exact Rust variant identifiers and form part of the generated
/// client schema.
#[derive(Debug, Clone, Copy)]
pub enum ClientPropsShape {
    /// A named-field Rust struct encoded as a JSON object.
    Object,
    /// A fieldless Rust enum encoded as one of the declared JSON strings.
    TaggedString {
        /// Accepted variant tags in source declaration order.
        variants: &'static [&'static str],
    },
}

/// One field of a `ClientProps`-derived struct.
#[derive(Debug, Clone, Copy)]
pub struct ClientPropsField {
    /// The Rust field name.
    pub name: &'static str,
    /// The field's type in the closed vocabulary.
    pub ty: &'static ClientPropsFieldType,
}

/// Sibling metadata for a `ClientProps`-derived struct or fieldless enum.
///
/// Emitted as a `pub const __rsjs_codec_meta_<TypeName>:
/// &'static ClientPropsMeta` next to every `#[derive(ClientProps)]`.
#[derive(Debug, Clone, Copy)]
pub struct ClientPropsMeta {
    /// The Rust type name, e.g. `"UserDraft"` or `"FulfillmentMode"`.
    pub name: &'static str,
    /// The type's generated client wire shape.
    pub shape: ClientPropsShape,
    /// The struct's fields, in declaration order.
    ///
    /// This is empty for [`ClientPropsShape::TaggedString`].
    pub fields: &'static [ClientPropsField],
}

/// Per-helper metadata emitted alongside every `#[rsjs::client]`
/// function.
///
/// `META: &'static ClientHelperMeta` is exposed on each generated
/// `__rsjs_client_<name>_meta` impl so M7e's `#[rsjs::client_module]`
/// attribute can collect references into a bundle without redeclaring
/// the consts. The manifest collector builds bundle JS through
/// [`Self::body_ir_json`], and server render evaluates helpers through the
/// generated [`Self::server_eval`] function.
///
/// All fields are `'static`: the struct exists at compile time and is
/// referenced from sibling consts, never owned. Bundle JS is emitted from
/// [`Self::body_ir_json`], while SSR uses the generated metadata evaluator for
/// the supported primitive helper subset.
#[derive(Debug, Clone, Copy)]
pub struct ClientHelperMeta {
    /// The helper's Rust function name (e.g. `"subtotal"`).
    pub name: &'static str,
    /// Each parameter's `(name, ty_name)` in declaration order. The
    /// type name is the v0.1 primitive token the `#[rsjs::client]`
    /// attribute recognised (`"i64"`, `"String"`, `"&str"`, etc.).
    pub params: &'static [(&'static str, &'static str)],
    /// The helper's return type as a v0.1 primitive token, e.g.
    /// `"i64"` or `"bool"`.
    pub return_ty: &'static str,
    /// Version of the syn-free helper IR stored in [`Self::body_ir_json`].
    pub body_ir_version: u32,
    /// Lowered helper body in the Client IR v3-compatible JSON shape.
    pub body_ir_json: &'static str,
    /// Server-side evaluator for this helper's currently supported
    /// primitive expression subset.
    pub server_eval: ClientHelperServerEval,
}

/// Per-bundle metadata emitted alongside every
/// `#[rsjs::client_module]`-marked module.
///
/// Sibling-const convention: a module annotated with
/// `#[rsjs::client_module] pub mod <name> { ... }` exposes a
/// `pub const __rsjs_client_module_<name>_meta:
/// &'static ClientModuleMeta` next to the module's items. Helper calls use
/// ordinary Rust name resolution and the relocation-backed linker; the module
/// metadata only controls optional grouping. M7g's manifest collector reads
/// the helpers list to enumerate the bundle's contents; M7e's
/// analyser uses `module_path` to validate calls.
#[derive(Debug, Clone, Copy)]
pub struct ClientModuleMeta {
    /// The bundle's canonical Rust path string, exactly as the
    /// authoring crate sees it (e.g. `"crate::helpers::pricing"`).
    /// The string is informational: rustc's normal name resolution
    /// validates the path; this field carries it for diagnostics and
    /// for the JS bundle's filename derivation.
    pub module_path: &'static str,
    /// References to the per-fn metadata structs of every
    /// `#[rsjs::client]` helper inside the module, in source order.
    pub helpers: &'static [&'static ClientHelperMeta],
}

/// Resolve a helper call path to the JS export name used inside a
/// client-module bundle.
///
/// `callee` is the Rust path captured in helper metadata, such as
/// `"subtotal"`, `"pricing::subtotal"`, or
/// `"self::pricing::subtotal"`. The returned name matches the bundle
/// convention used by island JS imports: `<module>__<helper>`.
pub fn client_helper_export_name(callee: &str, bundle_safe_name: Option<&str>) -> String {
    let mut segments = callee.rsplit("::");
    let helper = segments.next().unwrap_or(callee);
    match segments.next() {
        Some(module) => format!("{module}__{helper}"),
        None => bundle_safe_name
            .map(|module| format!("{module}__{helper}"))
            .unwrap_or_else(|| helper.to_string()),
    }
}

// ---------------------------------------------------------------------------
// JSON helpers (shared between primitive impls and the derive output).
// ---------------------------------------------------------------------------

/// Append a JSON-escaped string literal (with surrounding quotes) to `out`.
///
/// Public for use from `#[derive(ClientProps)]`-emitted code; users
/// should not call this directly.
#[doc(hidden)]
pub fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // HTML/JS-context escapes: the encoded JSON is shipped inside
            // <script type="application/json"> blocks, so any '<', '>', '&',
            // U+2028, or U+2029 in a string value could break out of the
            // script tag or be interpreted as a JS line terminator.
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Skip ASCII whitespace at `s[start..]`, returning the new offset.
#[doc(hidden)]
pub fn skip_ws(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = start;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Parse a JSON string starting at `s[start..]`. Returns the decoded
/// string and the offset *after* the closing quote.
#[doc(hidden)]
pub fn parse_json_string(s: &str, start: usize) -> Result<(String, usize), ClientDecodeError> {
    let bytes = s.as_bytes();
    if start >= bytes.len() || bytes[start] != b'"' {
        return Err(ClientDecodeError {
            msg: "expected '\"' at start of JSON string",
        });
    }
    let mut out = String::new();
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return Ok((out, i + 1));
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return Err(ClientDecodeError {
                    msg: "unterminated escape sequence in JSON string",
                });
            }
            match bytes[i] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'b' => out.push('\u{08}'),
                b'f' => out.push('\u{0c}'),
                b'u' => {
                    if i + 4 >= bytes.len() {
                        return Err(ClientDecodeError {
                            msg: "truncated \\uXXXX escape in JSON string",
                        });
                    }
                    let hex = match core::str::from_utf8(&bytes[i + 1..i + 5]) {
                        Ok(h) => h,
                        Err(_) => {
                            return Err(ClientDecodeError {
                                msg: "non-ASCII bytes in \\uXXXX escape",
                            });
                        }
                    };
                    let cp = match u32::from_str_radix(hex, 16) {
                        Ok(v) => v,
                        Err(_) => {
                            return Err(ClientDecodeError {
                                msg: "invalid hex digits in \\uXXXX escape",
                            });
                        }
                    };
                    i += 4;
                    // Round-3 review #4: RFC 8259 §7 surrogate-pair
                    // handling. A high surrogate (`0xD800..=0xDBFF`)
                    // must be followed by `\u` + a low surrogate
                    // (`0xDC00..=0xDFFF`); the pair encodes a single
                    // supplementary-plane codepoint. Without this,
                    // strings like ``  decoded as
                    // "lone surrogate is not a Unicode scalar value".
                    if (0xD800..=0xDBFF).contains(&cp) {
                        // Need the next 6 bytes: `\uXXXX`.
                        if i + 6 >= bytes.len() || bytes[i + 1] != b'\\' || bytes[i + 2] != b'u' {
                            return Err(ClientDecodeError {
                                msg: "high surrogate in \\uXXXX escape not followed by \\uXXXX low surrogate",
                            });
                        }
                        let low_hex = match core::str::from_utf8(&bytes[i + 3..i + 7]) {
                            Ok(h) => h,
                            Err(_) => {
                                return Err(ClientDecodeError {
                                    msg: "non-ASCII bytes in low-surrogate \\uXXXX escape",
                                });
                            }
                        };
                        let low = match u32::from_str_radix(low_hex, 16) {
                            Ok(v) => v,
                            Err(_) => {
                                return Err(ClientDecodeError {
                                    msg: "invalid hex digits in low-surrogate \\uXXXX escape",
                                });
                            }
                        };
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(ClientDecodeError {
                                msg: "high surrogate in \\uXXXX escape not followed by valid low surrogate",
                            });
                        }
                        let combined = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                        match char::from_u32(combined) {
                            Some(c) => out.push(c),
                            None => {
                                return Err(ClientDecodeError {
                                    msg:
                                        "decoded surrogate pair is not a valid Unicode scalar value",
                                });
                            }
                        }
                        // Consume `\uXXXX` (6 bytes) of the low surrogate.
                        i += 6;
                    } else if (0xDC00..=0xDFFF).contains(&cp) {
                        return Err(ClientDecodeError {
                            msg: "lone low-surrogate \\uXXXX escape in JSON string",
                        });
                    } else {
                        match char::from_u32(cp) {
                            Some(c) => out.push(c),
                            None => {
                                return Err(ClientDecodeError {
                                    msg: "\\uXXXX escape is not a valid Unicode scalar value",
                                });
                            }
                        }
                    }
                }
                _ => {
                    return Err(ClientDecodeError {
                        msg: "unknown escape sequence in JSON string",
                    });
                }
            }
            i += 1;
            continue;
        }
        // RFC 8259 §7: control bytes in `0x00..=0x1F` MUST be escaped
        // as `\uXXXX` inside a JSON string. Reject literal control
        // bytes here (round-3 review #3) so the decoder is symmetric
        // with the encoder, which already escapes the same range.
        if b < 0x20 {
            return Err(ClientDecodeError {
                msg: "rsjs::ClientDecode: unescaped control character (U+00XX) in JSON string; control bytes must be escaped as \\uXXXX per RFC 8259 §7",
            });
        }
        // Copy through the next UTF-8 codepoint.
        let ch_start = i;
        let width = utf8_char_width(b);
        if width == 0 || ch_start + width > bytes.len() {
            return Err(ClientDecodeError {
                msg: "invalid UTF-8 in JSON string",
            });
        }
        let slice = &s[ch_start..ch_start + width];
        out.push_str(slice);
        i += width;
    }
    Err(ClientDecodeError {
        msg: "unterminated JSON string",
    })
}

/// Width of a UTF-8 leading byte, or 0 for a continuation/invalid byte.
fn utf8_char_width(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC2 {
        0
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else if b < 0xF8 {
        4
    } else {
        0
    }
}

/// Scan a JSON number literal starting at `s[start..]`. Returns the
/// `(text_slice, end_offset)` of the number; the caller parses with
/// `from_str`.
#[doc(hidden)]
pub fn scan_json_number(s: &str, start: usize) -> Result<(&str, usize), ClientDecodeError> {
    let bytes = s.as_bytes();
    let mut i = start;
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == int_start {
        return Err(ClientDecodeError {
            msg: "expected digit in JSON number",
        });
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    Ok((&s[start..i], i))
}

/// Match a literal string at `s[start..]`. Returns the offset past the
/// match, or an error.
///
/// Round-3 review #5: compares bytes directly via `s.as_bytes()[..]`
/// instead of `&s[start..start + lit.len()]`, so a hostile caller
/// passing a multi-byte literal can't trigger a non-codepoint-boundary
/// slice panic. The literals this helper sees in practice are the
/// JSON keywords `null`, `true`, `false` (all pure ASCII), but the
/// byte compare is the safer default and has the same cost.
#[doc(hidden)]
pub fn expect_literal(s: &str, start: usize, lit: &str) -> Result<usize, ClientDecodeError> {
    let bytes = s.as_bytes();
    let lit_bytes = lit.as_bytes();
    if bytes.len() < start + lit_bytes.len() || &bytes[start..start + lit_bytes.len()] != lit_bytes
    {
        return Err(ClientDecodeError {
            msg: "expected JSON literal",
        });
    }
    Ok(start + lit_bytes.len())
}

/// Match a single-byte token at `s[start..]`.
#[doc(hidden)]
pub fn expect_byte(s: &str, start: usize, b: u8) -> Result<usize, ClientDecodeError> {
    let bytes = s.as_bytes();
    if start >= bytes.len() || bytes[start] != b {
        return Err(ClientDecodeError {
            msg: "expected JSON delimiter",
        });
    }
    Ok(start + 1)
}

// ---------------------------------------------------------------------------
// Primitive impls
// ---------------------------------------------------------------------------

impl ClientEncode for bool {
    fn encode_into(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl ClientDecode for bool {
    fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
        let i = skip_ws(s, 0);
        if let Ok(end) = expect_literal(s, i, "true") {
            return Ok((true, end));
        }
        if let Ok(end) = expect_literal(s, i, "false") {
            return Ok((false, end));
        }
        Err(ClientDecodeError {
            msg: "expected 'true' or 'false'",
        })
    }
}

macro_rules! impl_narrow_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl ClientEncode for $t {
                fn encode_into(&self, out: &mut String) {
                    let _ = write!(out, "{}", self);
                }
            }
            impl ClientDecode for $t {
                fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
                    let i = skip_ws(s, 0);
                    let (text, end) = scan_json_number(s, i)?;
                    let v = text.parse::<$t>().map_err(|_| ClientDecodeError {
                        msg: concat!("number out of range for ", stringify!($t)),
                    })?;
                    Ok((v, end))
                }
            }
        )*
    };
}

impl_narrow_int!(i8, i16, i32, u8, u16, u32);

macro_rules! impl_wide_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl ClientEncode for $t {
                fn encode_into(&self, out: &mut String) {
                    out.push('"');
                    let _ = write!(out, "{}", self);
                    out.push('"');
                }
            }
            impl ClientDecode for $t {
                fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
                    let i = skip_ws(s, 0);
                    let (text, end) = parse_json_string(s, i)?;
                    let v = text.parse::<$t>().map_err(|_| ClientDecodeError {
                        msg: concat!("wide-integer string out of range for ", stringify!($t)),
                    })?;
                    Ok((v, end))
                }
            }
        )*
    };
}

impl_wide_int!(i64, u64, i128, u128, isize, usize);

macro_rules! impl_float {
    ($($t:ty),* $(,)?) => {
        $(
            impl ClientEncode for $t {
                fn encode_into(&self, out: &mut String) {
                    // Round-3 review #9: NaN, +Infinity, and -Infinity
                    // are valid f32/f64 values but not valid JSON. The
                    // pre-fix encoder formatted them as `NaN` / `inf` /
                    // `-inf`, which produces invalid JSON the JS-side
                    // decoder rejects. JSON has no representation for
                    // non-finite floats; the convention used by
                    // Python's `json` module, JavaScript's
                    // `JSON.stringify`, and most other JSON serialisers
                    // is to encode them as `null`. We follow that
                    // convention. Decoding a `null` back into an f64
                    // would normally fail, but the codec routes nulls
                    // through `Option<T>` (or rejects them on a
                    // mismatch), so the encoded payload is safely
                    // round-trippable through `Option<f64>` and clear-
                    // cut on shape mismatch through bare `f64`.
                    if self.is_nan() || self.is_infinite() {
                        out.push_str("null");
                    } else {
                        let _ = write!(out, "{}", self);
                    }
                }
            }
            impl ClientDecode for $t {
                fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
                    let i = skip_ws(s, 0);
                    let (text, end) = scan_json_number(s, i)?;
                    let v = text.parse::<$t>().map_err(|_| ClientDecodeError {
                        msg: concat!("invalid floating-point literal for ", stringify!($t)),
                    })?;
                    Ok((v, end))
                }
            }
        )*
    };
}

impl_float!(f32, f64);

impl ClientEncode for String {
    fn encode_into(&self, out: &mut String) {
        write_json_string(out, self.as_str());
    }
}

impl ClientEncode for str {
    fn encode_into(&self, out: &mut String) {
        write_json_string(out, self);
    }
}

impl ClientDecode for String {
    fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
        let i = skip_ws(s, 0);
        parse_json_string(s, i)
    }
}

impl<T: ClientEncode + ?Sized> ClientEncode for &T {
    fn encode_into(&self, out: &mut String) {
        (**self).encode_into(out);
    }
}

impl<T: ClientEncode> ClientEncode for Option<T> {
    fn encode_into(&self, out: &mut String) {
        match self {
            None => out.push_str("null"),
            Some(v) => v.encode_into(out),
        }
    }
}

impl<T: ClientDecode> ClientDecode for Option<T> {
    fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
        let i = skip_ws(s, 0);
        if let Ok(end) = expect_literal(s, i, "null") {
            return Ok((None, end));
        }
        let (v, end) = T::decode_partial(&s[i..])?;
        Ok((Some(v), i + end))
    }
}

impl<T: ClientEncode> ClientEncode for Vec<T> {
    fn encode_into(&self, out: &mut String) {
        out.push('[');
        for (idx, item) in self.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            item.encode_into(out);
        }
        out.push(']');
    }
}

impl<T: ClientEncode> ClientEncode for [T] {
    fn encode_into(&self, out: &mut String) {
        out.push('[');
        for (idx, item) in self.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            item.encode_into(out);
        }
        out.push(']');
    }
}

impl<T: ClientDecode> ClientDecode for Vec<T> {
    fn decode_partial(s: &str) -> Result<(Self, usize), ClientDecodeError> {
        let mut i = skip_ws(s, 0);
        i = expect_byte(s, i, b'[')?;
        i = skip_ws(s, i);
        let mut out = Vec::new();
        if s.as_bytes().get(i) == Some(&b']') {
            return Ok((out, i + 1));
        }
        loop {
            let (item, consumed) = T::decode_partial(&s[i..])?;
            out.push(item);
            i += consumed;
            i = skip_ws(s, i);
            match s.as_bytes().get(i) {
                Some(&b',') => {
                    i = skip_ws(s, i + 1);
                }
                Some(&b']') => return Ok((out, i + 1)),
                _ => {
                    return Err(ClientDecodeError {
                        msg: "expected ',' or ']' in JSON array",
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_roundtrip() {
        assert_eq!(true.encode(), "true");
        assert_eq!(false.encode(), "false");
        assert!(<bool as ClientDecode>::decode("true").unwrap());
        assert!(!<bool as ClientDecode>::decode("false").unwrap());
    }

    #[test]
    fn narrow_int_roundtrip() {
        assert_eq!(42i32.encode(), "42");
        assert_eq!((-5i32).encode(), "-5");
        assert_eq!(<i32 as ClientDecode>::decode("42").unwrap(), 42);
        assert_eq!(<u32 as ClientDecode>::decode("99").unwrap(), 99);
    }

    #[test]
    fn float_nan_and_infinity_encode_as_json_null() {
        // Round-3 review #9 regression. JSON has no representation
        // for non-finite floats; following the
        // JSON.stringify / Python json convention, NaN and
        // ±Infinity encode as `null`. This keeps the wire payload
        // valid JSON and matches how JS-side parsers treat these
        // values when they round-trip.
        assert_eq!(f64::NAN.encode(), "null");
        assert_eq!(f64::INFINITY.encode(), "null");
        assert_eq!(f64::NEG_INFINITY.encode(), "null");
        assert_eq!(f32::NAN.encode(), "null");
        assert_eq!(f32::INFINITY.encode(), "null");
        assert_eq!(f32::NEG_INFINITY.encode(), "null");
        // Finite floats stay byte-stable.
        assert_eq!(1.5f64.encode(), "1.5");
        assert_eq!((-0.0f64).encode(), "-0");
    }

    #[test]
    fn wide_int_uses_string() {
        let v: u64 = 12345678901234567890;
        let s = v.encode();
        assert_eq!(s, "\"12345678901234567890\"");
        assert_eq!(<u64 as ClientDecode>::decode(&s).unwrap(), v);
    }

    #[test]
    fn string_escapes() {
        let s = "hi \"there\"\n\\".to_string();
        let enc = s.encode();
        assert_eq!(enc, "\"hi \\\"there\\\"\\n\\\\\"");
        let back: String = ClientDecode::decode(&enc).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn option_some_none() {
        let some: Option<i32> = Some(7);
        let none: Option<i32> = None;
        assert_eq!(some.encode(), "7");
        assert_eq!(none.encode(), "null");
        assert_eq!(<Option<i32>>::decode("7").unwrap(), Some(7));
        assert_eq!(<Option<i32>>::decode("null").unwrap(), None);
    }

    #[test]
    fn vec_roundtrip() {
        let v: Vec<i32> = vec![1, 2, 3];
        assert_eq!(v.encode(), "[1,2,3]");
        let back: Vec<i32> = ClientDecode::decode("[1,2,3]").unwrap();
        assert_eq!(back, vec![1, 2, 3]);
        let empty: Vec<i32> = ClientDecode::decode("[]").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn unicode_escape() {
        let raw = "\"\\u0041\"";
        let s: String = ClientDecode::decode(raw).unwrap();
        assert_eq!(s, "A");
    }

    #[test]
    fn rejects_trailing() {
        let r: Result<i32, _> = ClientDecode::decode("1 trailing");
        assert!(r.is_err());
    }

    #[test]
    fn script_tag_breakout_is_escaped() {
        let raw = "</script><script>alert(1)</script>".to_string();
        let enc = raw.encode();
        // Output must not contain literal "</script>" so the JSON blob is
        // safe inside <script type="application/json"> elements.
        assert!(!enc.contains("</script>"));
        assert!(!enc.contains("<"));
        assert!(!enc.contains(">"));
        // HTML-significant runes are unicode-escaped.
        assert!(enc.contains("\\u003c"));
        assert!(enc.contains("\\u003e"));
        // Decoder accepts \uXXXX so we still round-trip.
        let back: String = ClientDecode::decode(&enc).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn line_and_paragraph_separators_are_escaped() {
        let raw = "a\u{2028}b\u{2029}c".to_string();
        let enc = raw.encode();
        assert!(!enc.contains('\u{2028}'));
        assert!(!enc.contains('\u{2029}'));
        assert!(enc.contains("\\u2028"));
        assert!(enc.contains("\\u2029"));
        let back: String = ClientDecode::decode(&enc).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn surrogate_pair_round_trips() {
        // Round-3 review #4. RFC 8259 §7 surrogate-pair handling. A
        // `` (😀, U+1F600) escape decodes to the
        // supplementary-plane codepoint via the high+low surrogate
        // combination 0x10000 + ((0xD83D - 0xD800) << 10) +
        // (0xDE00 - 0xDC00).
        let raw = "\"\\uD83D\\uDE00\"";
        let s: String = ClientDecode::decode(raw).expect("valid surrogate pair must decode");
        assert_eq!(s, "\u{1F600}");
        // Lone high surrogate without a low pair must error.
        let lone_high = "\"\\uD83D\"";
        assert!(
            <String as ClientDecode>::decode(lone_high).is_err(),
            "lone high surrogate must error",
        );
        // Lone low surrogate must error.
        let lone_low = "\"\\uDE00\"";
        assert!(
            <String as ClientDecode>::decode(lone_low).is_err(),
            "lone low surrogate must error",
        );
        // High surrogate followed by a non-low \\uXXXX must error.
        let bad_pair = "\"\\uD83D\\u0041\"";
        assert!(
            <String as ClientDecode>::decode(bad_pair).is_err(),
            "high surrogate not followed by valid low surrogate must error",
        );
    }

    #[test]
    fn rejects_unescaped_control_byte_in_string() {
        // Round-3 review #3. RFC 8259 §7: control bytes in
        // `0x00..=0x1F` must be escaped as `\uXXXX` inside a JSON
        // string. The decoder must reject literal control bytes
        // rather than copying them through.
        let raw = "\"a\u{0001}b\"";
        let r: Result<String, _> = ClientDecode::decode(raw);
        assert!(
            r.is_err(),
            "decoder must reject literal control byte (U+0001), got: {:?}",
            r
        );
        // The escaped form must still round-trip.
        let escaped = "\"a\\u0001b\"";
        let s: String = ClientDecode::decode(escaped).expect("escaped form decodes");
        assert_eq!(s, "a\u{0001}b");
    }

    #[test]
    fn ampersand_is_escaped() {
        let raw = "Tom & Jerry".to_string();
        let enc = raw.encode();
        assert!(!enc.contains('&'));
        assert!(enc.contains("\\u0026"));
        let back: String = ClientDecode::decode(&enc).unwrap();
        assert_eq!(back, raw);
    }
}
