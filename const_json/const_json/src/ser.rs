// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Minimal JSON serialization helpers used by the `ConstJson` derive macro.

use crate::JsonBuffer;

/// Write the beginning `{` of an object.
pub const fn begin_object<const N: usize>(buf: &mut JsonBuffer<N>) {
    buf.push_byte(b'{');
}

/// Write the closing `}` of an object.
pub const fn end_object<const N: usize>(buf: &mut JsonBuffer<N>) {
    buf.push_byte(b'}');
}

/// Write the beginning `[` of an array.
pub const fn begin_array<const N: usize>(buf: &mut JsonBuffer<N>) {
    buf.push_byte(b'[');
}

/// Write the closing `]` of an array.
pub const fn end_array<const N: usize>(buf: &mut JsonBuffer<N>) {
    buf.push_byte(b']');
}

/// Insert a field separator `,`.
pub const fn field_separator<const N: usize>(buf: &mut JsonBuffer<N>) {
    buf.push_byte(b',');
}

/// Write a quoted JSON key followed by a colon.
pub const fn write_key<const N: usize>(buf: &mut JsonBuffer<N>, key: &str) {
    buf.push_byte(b'"');
    buf.push_escaped(key);
    buf.push_byte(b'"');
    buf.push_byte(b':');
}

/// Write a JSON string value (including surrounding quotes).
pub const fn write_string<const N: usize>(buf: &mut JsonBuffer<N>, value: &str) {
    buf.push_byte(b'"');
    buf.push_escaped(value);
    buf.push_byte(b'"');
}

/// Write a boolean JSON value.
pub const fn write_bool<const N: usize>(buf: &mut JsonBuffer<N>, value: bool) {
    if value {
        buf.push_lit("true");
    } else {
        buf.push_lit("false");
    }
}

/// Write an unsigned integer as decimal digits.
pub const fn write_unsigned<const N: usize>(buf: &mut JsonBuffer<N>, mut value: u128) {
    if value == 0 {
        buf.push_byte(b'0');
        return;
    }

    let mut tmp = [0u8; 39];
    let mut len = 0usize;
    while value > 0 {
        let digit = (value % 10) as u8;
        tmp[len] = digit;
        len += 1;
        value /= 10;
    }

    while len > 0 {
        len -= 1;
        buf.push_byte(b'0' + tmp[len]);
    }
}

/// Write a signed integer as decimal digits.
pub const fn write_signed<const N: usize>(buf: &mut JsonBuffer<N>, value: i128) {
    if value < 0 {
        buf.push_byte(b'-');
        let magnitude = value.unsigned_abs();
        write_unsigned(buf, magnitude);
    } else {
        write_unsigned(buf, value as u128);
    }
}
