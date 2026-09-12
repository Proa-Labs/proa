// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JavaScript number/string coercion semantics.
//!
//! The server evaluator and the JS emitters run the same IR, so numeric
//! coercion, comparison, and stringification must match ECMAScript. Every
//! deliberate divergence is documented in `SEMANTICS.md`.

use crate::value::Value;

/// JS `ToNumber`.
pub fn to_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::String(s) => str_to_number(s),
        // ToPrimitive(array/object) yields a string; objects always give
        // "[object Object]" (NaN), arrays join their elements.
        Value::Array(_) | Value::Object(_) => str_to_number(&display(value)),
    }
}

/// JS `String(value)`.
pub fn display(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => number_to_string(*f),
        Value::String(s) => s.to_string(),
        Value::Array(items) => {
            // Array.prototype.toString == join(","); null elements become "".
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !matches!(item, Value::Null) {
                    out.push_str(&display(item));
                }
            }
            out
        }
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// JS `ToString` applied to a number (`Number::toString` / `String(n)`).
///
/// Matches ECMAScript shortest-round-trip formatting in the positional
/// range and uses exponent notation outside `[1e-6, 1e21)` like JS does.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == 0.0 {
        return "0".to_string(); // covers -0 as well
    }
    let abs = n.abs();
    if !(1e-6..1e21).contains(&abs) {
        // JS exponent form: "1e+21", "1.5e-7". Rust's `{:e}` produces
        // "1e21" / "1.5e-7"; add the '+' for non-negative exponents.
        let s = format!("{n:e}");
        if let Some(pos) = s.find('e') {
            let (mantissa, exp) = s.split_at(pos);
            let exp = &exp[1..];
            if exp.starts_with('-') {
                return format!("{mantissa}e{exp}");
            }
            return format!("{mantissa}e+{exp}");
        }
        return s;
    }
    // Positional range: Rust Display is shortest-round-trip and never uses
    // exponent notation, matching JS output here.
    format!("{n}")
}

/// Whitespace JS trims and skips when converting strings to numbers.
///
/// Divergence from Rust: JS also strips U+FEFF (ZWNBSP) but not U+0085;
/// see `SEMANTICS.md`.
pub fn is_js_whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{0085}') || c == '\u{FEFF}'
}

pub fn trim_js(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// JS `StringToNumber` (the `Number("...")` conversion, whole string).
pub fn str_to_number(s: &str) -> f64 {
    let t = trim_js(s);
    if t.is_empty() {
        return 0.0;
    }
    // Non-decimal literals (no sign allowed in JS)
    for (prefixes, radix) in [(["0x", "0X"], 16u32), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        for prefix in prefixes {
            if let Some(rest) = t.strip_prefix(prefix) {
                return parse_radix(rest, radix);
            }
        }
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    if is_decimal_literal(body) {
        // Safe: the grammar we validated is a subset of what Rust accepts.
        sign * body.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn parse_radix(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut acc = 0.0f64;
    for c in digits.chars() {
        match c.to_digit(radix) {
            Some(d) => acc = acc * radix as f64 + d as f64,
            None => return f64::NAN,
        }
    }
    acc
}

/// JS `StrUnsignedDecimalLiteral` (without Infinity): digits with optional
/// fraction and exponent, or a fraction-only form like ".5".
fn is_decimal_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let int_digits = count_digits(bytes, i);
    i += int_digits;
    let mut frac_digits = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        frac_digits = count_digits(bytes, i);
        i += frac_digits;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let exp_digits = count_digits(bytes, i);
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == bytes.len()
}

fn count_digits(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count()
}

/// JS `parseInt(s, 10)`: leading whitespace, optional sign, leading digits.
pub fn parse_int_10(s: &str) -> f64 {
    let t = s.trim_start_matches(is_js_whitespace);
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits = count_digits(body.as_bytes(), 0);
    if digits == 0 {
        return f64::NAN;
    }
    sign * body[..digits].parse::<f64>().unwrap_or(f64::NAN)
}

/// JS `parseFloat(s)`: longest prefix matching a decimal literal
/// (including a signed `Infinity`).
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_js_whitespace);
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.starts_with("Infinity") {
        return sign * f64::INFINITY;
    }
    let len = decimal_prefix_len(body);
    if len == 0 {
        return f64::NAN;
    }
    sign * body[..len].parse::<f64>().unwrap_or(f64::NAN)
}

/// Length of the longest prefix of `s` that is a decimal literal.
fn decimal_prefix_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    let int_digits = count_digits(bytes, i);
    i += int_digits;
    let mut frac_digits = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        let frac = count_digits(bytes, i + 1);
        if int_digits > 0 || frac > 0 {
            i += 1 + frac;
            frac_digits = frac;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return 0;
    }
    // Optional exponent; only consumed if complete.
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_digits = count_digits(bytes, j);
        if exp_digits > 0 {
            i = j + exp_digits;
        }
    }
    i
}

/// JS `Math.round`: half-up toward positive infinity.
///
/// Implemented against the exact midpoint rather than `floor(x + 0.5)`,
/// which misrounds values like 0.49999999999999994 (the addition itself
/// rounds up to 1.0). For non-integral x, `floor(x)` is below 2^52 in
/// magnitude, so `floor(x) + 0.5` is exact.
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() || x.fract() == 0.0 {
        return x;
    }
    let f = x.floor();
    if x < f + 0.5 {
        f
    } else {
        f + 1.0
    }
}

/// JS `Math.min` for two values: NaN-propagating, -0 < +0.
pub fn js_min2(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b {
        a
    } else if b < a {
        b
    } else if a.is_sign_negative() {
        a
    } else {
        b
    }
}

/// JS `Math.max` for two values: NaN-propagating, +0 > -0.
pub fn js_max2(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b {
        a
    } else if b > a {
        b
    } else if a.is_sign_positive() {
        a
    } else {
        b
    }
}

/// String length in UTF-16 code units (JS `.length`).
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Lexicographic UTF-16 code unit comparison (JS `<` on strings).
pub fn utf16_lt(a: &str, b: &str) -> bool {
    a.encode_utf16().lt(b.encode_utf16())
}

/// Slice of a string by UTF-16 code unit range (lone surrogates become
/// U+FFFD; see `SEMANTICS.md`).
pub fn utf16_slice(s: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = s
        .encode_utf16()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Single UTF-16 code unit as a string (JS `s[i]`), if in range.
pub fn utf16_at(s: &str, index: usize) -> Option<String> {
    s.encode_utf16()
        .nth(index)
        .map(|unit| String::from_utf16_lossy(&[unit]))
}
