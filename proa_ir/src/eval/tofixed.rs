// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `Number.prototype.toFixed` semantics for `Expr::FormatNumber`.
//!
//! The ECMAScript spec picks the integer n closest to `x * 10^d`, choosing
//! the LARGER n on exact ties (round half away from zero on the magnitude).
//! Rust's `format!("{:.d}")` rounds ties to even on the exact binary value,
//! so it disagrees on exactly representable ties like `0.125`. To match JS
//! we expand the double to its exact decimal digits (a finite expansion,
//! at most ~1074 fraction digits) and round half-up on the digit string.

use super::jsnum::number_to_string;

/// Maximum fraction digits `toFixed` accepts (spec range is 0..=100).
pub const MAX_FRACTION_DIGITS: u8 = 100;

/// Exceeds the longest possible exact decimal expansion of an f64
/// fraction (~1074 digits for subnormals), so `format!` yields the exact
/// digits, zero padded.
const EXACT_PRECISION: usize = 1100;

/// JS `x.toFixed(digits)`. `digits` must be `<= MAX_FRACTION_DIGITS`
/// (callers validate; JS throws a RangeError).
pub fn to_fixed(x: f64, digits: u8) -> String {
    debug_assert!(digits <= MAX_FRACTION_DIGITS);
    if x.is_nan() {
        return "NaN".to_string();
    }
    // Spec: |x| >= 1e21 (and Infinity) falls back to regular ToString.
    if !x.is_finite() || x.abs() >= 1e21 {
        return number_to_string(x);
    }
    let negative = x < 0.0; // strictly: -0.0 gets no sign, matching JS
    let abs = x.abs();
    let d = digits as usize;

    let exact = format!("{abs:.precision$}", precision = EXACT_PRECISION);
    let dot = exact.find('.').expect("fixed formatting always has a dot");
    let mut int_len = dot;
    let mut buf: Vec<u8> = exact[..dot]
        .bytes()
        .chain(exact[dot + 1..dot + 1 + d].bytes())
        .collect();

    // Round half away from zero on the magnitude: the first dropped digit
    // decides (>= 5 rounds up; an exact tie has that digit == 5 with only
    // zeros after, which also rounds up).
    if exact.as_bytes()[dot + 1 + d] >= b'5' {
        let mut carry = true;
        for digit in buf.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            buf.insert(0, b'1');
            int_len += 1;
        }
    }

    let mut out = String::with_capacity(buf.len() + 2);
    if negative {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&buf[..int_len]).expect("ascii digits"));
    if d > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(&buf[int_len..]).expect("ascii digits"));
    }
    out
}
