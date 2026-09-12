// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use core::ops::Index;

/// Stack-allocated, grow-only JSON buffer.
///
/// This structure exposes only `const fn` methods so it can be used from
/// compile-time generated serializers.
pub struct JsonBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Default for JsonBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> JsonBuffer<N> {
    /// Create an empty buffer.
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    /// Current number of bytes written.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Maximum capacity in bytes.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Whether the buffer is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Push a single byte, asserting capacity.
    pub const fn push_byte(&mut self, byte: u8) {
        assert!(self.len < N, "const_json buffer overflow");
        self.buf[self.len] = byte;
        self.len += 1;
    }

    /// Append a raw UTF-8 literal.
    pub const fn push_lit(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut idx = 0;
        let len = bytes.len();
        while idx < len {
            self.push_byte(bytes[idx]);
            idx += 1;
        }
    }

    /// Append a string value with JSON escaping.
    pub const fn push_escaped(&mut self, value: &str) {
        let bytes = value.as_bytes();
        let mut idx = 0;
        let len = bytes.len();
        while idx < len {
            let b = bytes[idx];
            match b {
                b'"' => {
                    self.push_byte(b'\\');
                    self.push_byte(b'"');
                }
                b'\\' => {
                    self.push_byte(b'\\');
                    self.push_byte(b'\\');
                }
                b'\n' => {
                    self.push_byte(b'\\');
                    self.push_byte(b'n');
                }
                b'\r' => {
                    self.push_byte(b'\\');
                    self.push_byte(b'r');
                }
                b'\t' => {
                    self.push_byte(b'\\');
                    self.push_byte(b't');
                }
                0x20..=0x7E => {
                    self.push_byte(b);
                }
                _ => {
                    // Fallback to \u00XX escaping for other control bytes.
                    self.push_byte(b'\\');
                    self.push_byte(b'u');
                    self.push_byte(b'0');
                    self.push_byte(b'0');
                    let hi = (b >> 4) & 0x0F;
                    let lo = b & 0x0F;
                    self.push_byte(hex_digit(hi));
                    self.push_byte(hex_digit(lo));
                }
            }
            idx += 1;
        }
    }

    /// View the written bytes as a slice.
    pub const fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.buf.as_ptr(), self.len) }
    }

    /// Interpret the contents as a UTF-8 string.
    pub const fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(self.as_bytes()) }
    }

    /// Move the buffer out, returning the backing array and length.
    pub const fn into_inner(self) -> ([u8; N], usize) {
        (self.buf, self.len)
    }

    /// Reset the buffer to empty without altering allocation.
    pub const fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> Index<usize> for JsonBuffer<N> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.buf[index]
    }
}

const fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + (nibble - 10),
        _ => b'0',
    }
}
