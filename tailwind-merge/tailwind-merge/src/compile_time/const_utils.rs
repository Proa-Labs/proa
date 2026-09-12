// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use core::{slice, str};

pub const fn starts_with_char(value: &str, ch: char) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty() && bytes[0] == ch as u8
}

pub const fn ends_with_char(value: &str, ch: char) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty() && bytes[bytes.len() - 1] == ch as u8
}

pub const fn has_prefix(value: &str, prefix: &str) -> bool {
    let value_bytes = value.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    if prefix_bytes.len() > value_bytes.len() {
        return false;
    }

    let mut idx = 0;
    while idx < prefix_bytes.len() {
        if value_bytes[idx] != prefix_bytes[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

pub const fn has_suffix(value: &str, suffix: &str) -> bool {
    let value_bytes = value.as_bytes();
    let suffix_bytes = suffix.as_bytes();
    if suffix_bytes.len() > value_bytes.len() {
        return false;
    }

    let offset = value_bytes.len() - suffix_bytes.len();
    let mut idx = 0;
    while idx < suffix_bytes.len() {
        if value_bytes[offset + idx] != suffix_bytes[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

pub const fn strip_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if has_prefix(value, prefix) {
        Some(slice_from(value, prefix.len()))
    } else {
        None
    }
}

pub const fn slice_from(value: &str, start: usize) -> &str {
    slice_range(value, start, value.len())
}

pub const fn slice_to(value: &str, end: usize) -> &str {
    slice_range(value, 0, end)
}

pub const fn slice_range(value: &str, start: usize, end: usize) -> &str {
    unsafe {
        let ptr = value.as_ptr().add(start);
        let len = end - start;
        str::from_utf8_unchecked(slice::from_raw_parts(ptr, len))
    }
}

pub const fn cmp_str(left: &str, right: &str) -> i8 {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut idx = 0;
    while idx < left_bytes.len() && idx < right_bytes.len() {
        let l = left_bytes[idx];
        let r = right_bytes[idx];
        if l < r {
            return -1;
        }
        if l > r {
            return 1;
        }
        idx += 1;
    }

    if left_bytes.len() < right_bytes.len() {
        -1
    } else if left_bytes.len() > right_bytes.len() {
        1
    } else {
        0
    }
}

pub const fn str_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut idx = 0;
    while idx < left_bytes.len() {
        if left_bytes[idx] != right_bytes[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

pub const fn slice_prefix<T, const N: usize>(array: &[T; N], len: usize) -> &[T] {
    unsafe { slice::from_raw_parts(array.as_ptr(), len) }
}

pub const fn slice_prefix_mut<T, const N: usize>(array: &mut [T; N], len: usize) -> &mut [T] {
    unsafe { slice::from_raw_parts_mut(array.as_mut_ptr(), len) }
}
