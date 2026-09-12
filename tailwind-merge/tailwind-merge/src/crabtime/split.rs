// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use core::{slice, str};

/// Result of a constexpr whitespace split with fixed capacity.
#[derive(Clone, Copy)]
pub struct SplitWhitespaceResult<const N: usize> {
    parts: [&'static str; N],
}

impl<const N: usize> SplitWhitespaceResult<N> {
    /// Consumes the result and returns the fixed array of segments. Unused
    /// slots contain empty strings.
    pub const fn into_array(self) -> [&'static str; N] {
        self.parts
    }
}

/// Split a string on ASCII whitespace in a `const fn` context.
///
/// The function records at most `N` segments; additional segments are ignored.
/// Unused entries in the output are initialised to the empty string. Whitespace
/// is considered any of: space, tab, carriage-return, newline or form-feed.
pub const fn split_whitespace<const N: usize>(input: &'static str) -> SplitWhitespaceResult<N> {
    let mut parts: [&'static str; N] = [""; N];
    let bytes = input.as_bytes();
    let mut idx = 0;
    let mut count = 0;

    while idx < bytes.len() {
        while idx < bytes.len() && is_whitespace(bytes[idx]) {
            idx += 1;
        }

        if idx >= bytes.len() {
            break;
        }

        let start = idx;
        while idx < bytes.len() && !is_whitespace(bytes[idx]) {
            idx += 1;
        }
        let end = idx;

        if count < N {
            parts[count] = slice_range(input, start, end);
            count += 1;
        } else {
            break;
        }
    }

    let _ = count; // keep the compiler aware that we tracked the number of segments.
    SplitWhitespaceResult { parts }
}

const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | 0x0C)
}

const fn slice_range(value: &str, start: usize, end: usize) -> &'static str {
    unsafe {
        let ptr = value.as_ptr().add(start);
        let len = end - start;
        str::from_utf8_unchecked(slice::from_raw_parts(ptr, len))
    }
}

#[cfg(test)]
mod tests {
    use super::split_whitespace;

    const SPLIT: [&str; 4] = split_whitespace::<4>("  px-1\tpx-2\npx-3  ").into_array();

    #[test]
    fn splits_whitespace_in_const_context() {
        assert_eq!(SPLIT, ["px-1", "px-2", "px-3", ""]);
    }

    #[test]
    fn ignores_excess_tokens() {
        const OUT: [&str; 2] = split_whitespace::<2>("a b c").into_array();
        assert_eq!(OUT, ["a", "b"]);
    }

    #[test]
    fn trailing_whitespace_is_ignored() {
        const OUT: [&str; 3] = split_whitespace::<3>("a b ").into_array();
        assert_eq!(OUT, ["a", "b", ""]);
    }
}
