// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Fast, safe integer-to-ASCII conversion using a digit-pair lookup table.
//!
//! This crate converts integers to their decimal string representation without
//! any heap allocation and without `unsafe` code. It is a drop-in replacement
//! for the [`itoa`](https://crates.io/crates/itoa) crate with a compatible API.
//!
//! # Performance
//!
//! The core technique is a 200-byte lookup table that maps each value 0–99 to
//! its two-digit ASCII representation. Numbers are decomposed front-to-back in
//! chunks of 2 digits (÷100), 4 digits (÷10,000), or 8 digits (÷100,000,000),
//! and each chunk is written via a 2-byte `copy_from_slice` from the table.
//!
//! All write functions accept fixed-size array references (e.g. `&mut [u8; 20]`
//! for `u64`), which lets the compiler prove all indices are in-bounds and
//! **elide every bounds check** — giving us the speed of raw pointer writes
//! with full safety guarantees.
//!
//! # Usage
//!
//! ```
//! let mut buf = itoa_lut::Buffer::new();
//! let s: &str = buf.format(12345u32);
//! assert_eq!(s, "12345");
//! ```
//!
//! # Supported types
//!
//! All 12 Rust primitive integer types: `u8`, `u16`, `u32`, `u64`, `u128`,
//! `usize`, `i8`, `i16`, `i32`, `i64`, `i128`, `isize`.

#![no_std]
// The only unsafe usage is `from_utf8_unchecked` in Buffer::format, which is
// sound because all write functions produce only ASCII bytes (0x30..=0x39 for
// digits, 0x2D for '-').

/// A 200-byte lookup table mapping each value 0–99 to its two-digit ASCII
/// representation. Entry `n` occupies bytes `[2n, 2n+1]`.
///
/// For example, entry 42 is at bytes [84, 85] = [b'4', b'2'].
const LUT: &[u8; 200] = b"\
0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";

/// Returns the two ASCII bytes for a value in 0–99.
///
/// # Correctness argument
///
/// `n` is in `[0, 99]`, so `i = n * 2` is in `[0, 198]`, and `i + 1` is in
/// `[1, 199]`. Both are valid indices into `LUT` (length 200).
#[inline(always)]
fn pair(n: u32) -> [u8; 2] {
    let i = (n as usize) << 1;
    [LUT[i], LUT[i + 1]]
}

// ────────────────────────────────────────────────────────────
// Building blocks: write variable-length head and fixed-width tails
// ────────────────────────────────────────────────────────────

/// Writes 1–4 digits of `n` (where `n < 10,000`) to `buf`, returning the
/// number of bytes written.
///
/// # Correctness argument
///
/// The caller guarantees `n < 10,000`. Every division by 100 yields quotients
/// and remainders in `[0, 99]`, which are valid `pair()` inputs. The maximum
/// write offset is `buf[2..4]`, so any `buf` of length ≥ 4 is sufficient.
#[inline(always)]
fn write4(n: u32, buf: &mut [u8]) -> usize {
    debug_assert!(n < 10_000);
    if n < 100 {
        if n < 10 {
            buf[0] = n as u8 + b'0';
            1
        } else {
            let p = pair(n);
            buf[..2].copy_from_slice(&p);
            2
        }
    } else {
        let (hi, lo) = (n / 100, n % 100);
        if n < 1000 {
            buf[0] = hi as u8 + b'0';
            let p = pair(lo);
            buf[1..3].copy_from_slice(&p);
            3
        } else {
            let p1 = pair(hi);
            let p2 = pair(lo);
            buf[..2].copy_from_slice(&p1);
            buf[2..4].copy_from_slice(&p2);
            4
        }
    }
}

/// Writes exactly 4 zero-padded digits (for fixed-width tail groups).
///
/// # Correctness argument
///
/// `n < 10,000`, so `n / 100` and `n % 100` are both in `[0, 99]`.
/// Writes to `buf[0..4]`.
#[inline(always)]
fn write4_pad(n: u32, buf: &mut [u8]) {
    debug_assert!(n < 10_000);
    let (hi, lo) = (n / 100, n % 100);
    let p1 = pair(hi);
    let p2 = pair(lo);
    buf[..2].copy_from_slice(&p1);
    buf[2..4].copy_from_slice(&p2);
}

/// Writes exactly 8 zero-padded digits (for fixed-width tail groups).
///
/// # Correctness argument
///
/// `n < 100,000,000`, so `n / 10,000` and `n % 10,000` are both in
/// `[0, 9999]`, valid inputs for `write4_pad`. Writes to `buf[0..8]`.
#[inline(always)]
fn write8_pad(n: u32, buf: &mut [u8]) {
    debug_assert!(n < 100_000_000);
    let (hi, lo) = (n / 10_000, n % 10_000);
    write4_pad(hi, buf);
    write4_pad(lo, &mut buf[4..]);
}

// ────────────────────────────────────────────────────────────
// Per-type write functions
// ────────────────────────────────────────────────────────────

/// Writes a `u8` to `buf`. Returns the number of bytes written (1–3).
///
/// # Correctness argument
///
/// `u8::MAX = 255`, which is 3 digits. The buffer is exactly `[u8; 3]`.
/// For `n < 10`: writes 1 byte. For `n < 100`: 2 bytes via `pair()`.
/// For `n >= 100`: `n / 100` is in `[1, 2]` (single digit), `n % 100`
/// is in `[0, 55]`, valid for `pair()`. Writes 3 bytes.
#[inline]
pub fn write_u8(n: u8, buf: &mut [u8; 3]) -> usize {
    if n < 10 {
        buf[0] = n + b'0';
        1
    } else if n < 100 {
        let p = pair(n as u32);
        buf[..2].copy_from_slice(&p);
        2
    } else {
        let (hi, lo) = (n / 100, n % 100);
        buf[0] = hi + b'0';
        let p = pair(lo as u32);
        buf[1..3].copy_from_slice(&p);
        3
    }
}

/// Writes a `u16` to `buf`. Returns the number of bytes written (1–5).
///
/// # Correctness argument
///
/// `u16::MAX = 65535`, which is 5 digits. The buffer is exactly `[u8; 5]`.
/// Decomposition by ÷100 and ÷10,000 keeps all quotients and remainders
/// in `[0, 99]` or `[0, 9]`, valid for `pair()` or single-digit write.
#[inline]
pub fn write_u16(n: u16, buf: &mut [u8; 5]) -> usize {
    if n < 100 {
        if n < 10 {
            buf[0] = n as u8 + b'0';
            1
        } else {
            let p = pair(n as u32);
            buf[..2].copy_from_slice(&p);
            2
        }
    } else if n < 10_000 {
        let (hi, lo) = (n / 100, n % 100);
        if n < 1000 {
            buf[0] = hi as u8 + b'0';
            let p = pair(lo as u32);
            buf[1..3].copy_from_slice(&p);
            3
        } else {
            let p1 = pair(hi as u32);
            let p2 = pair(lo as u32);
            buf[..2].copy_from_slice(&p1);
            buf[2..4].copy_from_slice(&p2);
            4
        }
    } else {
        let (a, rem) = (n / 10_000, n % 10_000);
        let (b, c) = (rem / 100, rem % 100);
        buf[0] = a as u8 + b'0';
        let p1 = pair(b as u32);
        let p2 = pair(c as u32);
        buf[1..3].copy_from_slice(&p1);
        buf[3..5].copy_from_slice(&p2);
        5
    }
}

/// Writes a `u32` to `buf`. Returns the number of bytes written (1–10).
///
/// # Correctness argument
///
/// `u32::MAX = 4,294,967,295`, which is 10 digits. The buffer is `[u8; 10]`.
/// The number is decomposed into at most three groups:
///   - Leading 1–2 digits (from `n / 100,000,000`), max 42
///   - Middle 4 digits (from `(n / 10,000) % 10,000`)
///   - Trailing 4 digits (from `n % 10,000`)
///
/// Each group uses `write4`/`write4_pad`/`write8_pad`, which only access
/// the first 4 or 8 bytes of their slice argument.
#[inline]
pub fn write_u32(n: u32, buf: &mut [u8; 10]) -> usize {
    if n < 10_000 {
        write4(n, buf)
    } else if n < 100_000_000 {
        let (hi, lo) = (n / 10_000, n % 10_000);
        let l = write4(hi, buf);
        write4_pad(lo, &mut buf[l..]);
        l + 4
    } else {
        let (hi, lo) = (n / 100_000_000, n % 100_000_000);
        let l = if hi >= 10 {
            let p = pair(hi);
            buf[..2].copy_from_slice(&p);
            2
        } else {
            buf[0] = hi as u8 + b'0';
            1
        };
        write8_pad(lo, &mut buf[l..]);
        l + 8
    }
}

/// Writes a `u64` to `buf`. Returns the number of bytes written (1–20).
///
/// # Correctness argument
///
/// `u64::MAX = 18,446,744,073,709,551,615`, which is 20 digits. The buffer
/// is `[u8; 20]`. The number is decomposed into groups of at most 8 digits:
///   - `n < 10,000`: 1–4 digits via `write4`
///   - `n < 100,000,000`: 5–8 digits via `write4` + `write4_pad`
///   - `n < 10^16`: 9–16 digits via `write4` (+ `write4_pad`) + `write8_pad`
///   - `n >= 10^16`: 17–20 digits via `write4` + two `write8_pad`
///
/// The maximum write position is `buf[l + 16]` where `l ≤ 4`, so
/// `l + 16 ≤ 20`, which fits in the buffer.
#[inline]
pub fn write_u64(n: u64, buf: &mut [u8; 20]) -> usize {
    if n < 10_000 {
        write4(n as u32, buf)
    } else if n < 100_000_000 {
        let (hi, lo) = (n / 10_000, n % 10_000);
        let l = write4(hi as u32, buf);
        write4_pad(lo as u32, &mut buf[l..]);
        l + 4
    } else if n < 10_000_000_000_000_000 {
        let (hi, lo) = (n / 100_000_000, n % 100_000_000);
        let hi = hi as u32;
        let lo = lo as u32;
        let l = if hi < 10_000 {
            write4(hi, buf)
        } else {
            let (h2, l2) = (hi / 10_000, hi % 10_000);
            let l = write4(h2, buf);
            write4_pad(l2, &mut buf[l..]);
            l + 4
        };
        write8_pad(lo, &mut buf[l..]);
        l + 8
    } else {
        let (hi, lo) = (n / 10_000_000_000_000_000, n % 10_000_000_000_000_000);
        let (mid, lo) = ((lo / 100_000_000) as u32, (lo % 100_000_000) as u32);
        let l = write4(hi as u32, buf);
        write8_pad(mid, &mut buf[l..]);
        write8_pad(lo, &mut buf[l + 8..]);
        l + 16
    }
}

/// Writes a `u128` to `buf`. Returns the number of bytes written (1–39).
///
/// Values that fit in `u64` are delegated to `write_u64`. Larger values are
/// decomposed using Granlund-Montgomery division by 10^16.
#[inline]
pub fn write_u128(n: u128, buf: &mut [u8; 39]) -> usize {
    if n <= u64::MAX as u128 {
        let inner: &mut [u8; 20] = (&mut buf[..20]).try_into().unwrap();
        write_u64(n as u64, inner)
    } else {
        write_u128_big(n, buf)
    }
}

fn write_u128_big(n: u128, buf: &mut [u8; 39]) -> usize {
    const POW_10_8: u64 = 100_000_000;
    const POW_10_16: u64 = 10_000_000_000_000_000;

    debug_assert!(n > u64::MAX as u128);

    let mut result = [0u32; 5];

    let quot = {
        const DIV_FACTOR: u128 = 76_624_777_043_294_442_917_917_351_357_515_459_181;
        const DIV_SHIFT: u32 = 51;
        let q = u128_mulhi(n, DIV_FACTOR) >> DIV_SHIFT;
        debug_assert_eq!(q, n / POW_10_16 as u128);
        q
    };
    let rem = (n - quot * POW_10_16 as u128) as u64;
    result[1] = (rem / POW_10_8) as u32;
    result[0] = (rem % POW_10_8) as u32;

    let (top_idx, top_val) = if quot >= POW_10_16 as u128 {
        let q2 = (quot >> 16) as u64 / (POW_10_16 >> 16);
        let r2 = (quot - POW_10_16 as u128 * q2 as u128) as u64;
        result[3] = (r2 / POW_10_8) as u32;
        result[2] = (r2 % POW_10_8) as u32;
        result[4] = q2 as u32;
        (4, result[4])
    } else if (quot as u64) >= POW_10_8 {
        result[3] = ((quot as u64) / POW_10_8) as u32;
        result[2] = ((quot as u64) % POW_10_8) as u32;
        (3, result[3])
    } else {
        result[2] = quot as u32;
        (2, result[2])
    };

    let mut tmp = [0u8; 10];
    let l = write_u32(top_val, &mut tmp);
    buf[..l].copy_from_slice(&tmp[..l]);
    let mut pos = l;

    for i in (0..top_idx).rev() {
        write8_pad(result[i], &mut buf[pos..]);
        pos += 8;
    }

    pos
}

#[inline]
fn u128_mulhi(x: u128, y: u128) -> u128 {
    let x_lo = x as u64;
    let x_hi = (x >> 64) as u64;
    let y_lo = y as u64;
    let y_hi = (y >> 64) as u64;

    let carry = (x_lo as u128 * y_lo as u128) >> 64;
    let m = x_lo as u128 * y_hi as u128 + carry;
    let high1 = m >> 64;
    let m_lo = m as u64;
    let high2 = (x_hi as u128 * y_lo as u128 + m_lo as u128) >> 64;

    x_hi as u128 * y_hi as u128 + high1 + high2
}

// ────────────────────────────────────────────────────────────
// Signed wrappers
// ────────────────────────────────────────────────────────────

/// Writes an `i8` to `buf`. Returns the number of bytes written (1–4).
#[inline]
pub fn write_i8(n: i8, buf: &mut [u8; 4]) -> usize {
    if n < 0 {
        buf[0] = b'-';
        let abs = (!(n as u8)).wrapping_add(1);
        let inner: &mut [u8; 3] = (&mut buf[1..4]).try_into().unwrap();
        1 + write_u8(abs, inner)
    } else {
        let inner: &mut [u8; 3] = (&mut buf[..3]).try_into().unwrap();
        write_u8(n as u8, inner)
    }
}

/// Writes an `i16` to `buf`. Returns the number of bytes written (1–6).
#[inline]
pub fn write_i16(n: i16, buf: &mut [u8; 6]) -> usize {
    if n < 0 {
        buf[0] = b'-';
        let abs = (!(n as u16)).wrapping_add(1);
        let inner: &mut [u8; 5] = (&mut buf[1..6]).try_into().unwrap();
        1 + write_u16(abs, inner)
    } else {
        let inner: &mut [u8; 5] = (&mut buf[..5]).try_into().unwrap();
        write_u16(n as u16, inner)
    }
}

/// Writes an `i32` to `buf`. Returns the number of bytes written (1–11).
#[inline]
pub fn write_i32(n: i32, buf: &mut [u8; 11]) -> usize {
    if n < 0 {
        buf[0] = b'-';
        let abs = (!(n as u32)).wrapping_add(1);
        let inner: &mut [u8; 10] = (&mut buf[1..11]).try_into().unwrap();
        1 + write_u32(abs, inner)
    } else {
        let inner: &mut [u8; 10] = (&mut buf[..10]).try_into().unwrap();
        write_u32(n as u32, inner)
    }
}

/// Writes an `i64` to `buf`. Returns the number of bytes written (1–20).
#[inline]
pub fn write_i64(n: i64, buf: &mut [u8; 21]) -> usize {
    if n < 0 {
        buf[0] = b'-';
        let abs = (!(n as u64)).wrapping_add(1);
        let inner: &mut [u8; 20] = (&mut buf[1..21]).try_into().unwrap();
        1 + write_u64(abs, inner)
    } else {
        let inner: &mut [u8; 20] = (&mut buf[..20]).try_into().unwrap();
        write_u64(n as u64, inner)
    }
}

/// Writes an `i128` to `buf`. Returns the number of bytes written (1–40).
#[inline]
pub fn write_i128(n: i128, buf: &mut [u8; 40]) -> usize {
    if n < 0 {
        buf[0] = b'-';
        let abs = (!(n as u128)).wrapping_add(1);
        let inner: &mut [u8; 39] = (&mut buf[1..40]).try_into().unwrap();
        1 + write_u128(abs, inner)
    } else {
        let inner: &mut [u8; 39] = (&mut buf[..39]).try_into().unwrap();
        write_u128(n as u128, inner)
    }
}

// ────────────────────────────────────────────────────────────
// itoa-compatible Buffer API
// ────────────────────────────────────────────────────────────

/// A stack-allocated buffer for formatting integers as decimal strings.
///
/// This is API-compatible with `itoa::Buffer` — call [`Buffer::format`] to
/// write an integer and get back a `&str`.
///
/// ```
/// let mut buf = itoa_lut::Buffer::new();
/// assert_eq!(buf.format(42u32), "42");
/// assert_eq!(buf.format(-1i64), "-1");
/// assert_eq!(buf.format(u128::MAX), "340282366920938463463374607431768211455");
/// ```
pub struct Buffer {
    bytes: [u8; 40],
}

impl Buffer {
    /// Creates a new buffer. The contents are uninitialized (but safe).
    #[inline]
    pub const fn new() -> Self {
        Self { bytes: [0u8; 40] }
    }

    /// Formats an integer into the buffer and returns the result as a `&str`.
    #[inline]
    pub fn format<I: Integer>(&mut self, value: I) -> &str {
        value.write_to_buf(self)
    }
}

impl Default for Buffer {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Trait implemented for all supported integer types.
///
/// This is sealed — it cannot be implemented outside this crate.
pub trait Integer: private::Sealed {
    #[doc(hidden)]
    fn write_to_buf(self, buf: &mut Buffer) -> &str;
}

mod private {
    pub trait Sealed {}
}

macro_rules! impl_integer {
    (unsigned $ty:ty, $write_fn:ident, $max_len:expr) => {
        impl private::Sealed for $ty {}
        impl Integer for $ty {
            #[inline]
            fn write_to_buf(self, buf: &mut Buffer) -> &str {
                let b: &mut [u8; $max_len] = (&mut buf.bytes[..$max_len]).try_into().unwrap();
                let len = $write_fn(self, b);
                // SAFETY: write functions only produce ASCII digits (0x30..=0x39),
                // which are valid single-byte UTF-8 code points.
                unsafe { core::str::from_utf8_unchecked(&buf.bytes[..len]) }
            }
        }
    };
    (signed $ty:ty, $write_fn:ident, $max_len:expr) => {
        impl private::Sealed for $ty {}
        impl Integer for $ty {
            #[inline]
            fn write_to_buf(self, buf: &mut Buffer) -> &str {
                let b: &mut [u8; $max_len] = (&mut buf.bytes[..$max_len]).try_into().unwrap();
                let len = $write_fn(self, b);
                // SAFETY: write functions only produce ASCII digits and '-' (0x2D),
                // which are valid single-byte UTF-8 code points.
                unsafe { core::str::from_utf8_unchecked(&buf.bytes[..len]) }
            }
        }
    };
}

impl_integer!(unsigned u8, write_u8, 3);
impl_integer!(unsigned u16, write_u16, 5);
impl_integer!(unsigned u32, write_u32, 10);
impl_integer!(unsigned u64, write_u64, 20);
impl_integer!(unsigned u128, write_u128, 39);
impl_integer!(signed i8, write_i8, 4);
impl_integer!(signed i16, write_i16, 6);
impl_integer!(signed i32, write_i32, 11);
impl_integer!(signed i64, write_i64, 21);
impl_integer!(signed i128, write_i128, 40);

// usize/isize delegate to the pointer-width equivalent
impl private::Sealed for usize {}
impl Integer for usize {
    #[inline]
    fn write_to_buf(self, buf: &mut Buffer) -> &str {
        (self as u64).write_to_buf(buf)
    }
}

impl private::Sealed for isize {}
impl Integer for isize {
    #[inline]
    fn write_to_buf(self, buf: &mut Buffer) -> &str {
        (self as i64).write_to_buf(buf)
    }
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
extern crate alloc;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // ── Exhaustive small-type tests ──

    #[test]
    fn u8_exhaustive() {
        let mut buf = Buffer::new();
        for n in 0..=u8::MAX {
            assert_eq!(buf.format(n), &n.to_string(), "u8 failed for {n}");
        }
    }

    #[test]
    fn i8_exhaustive() {
        let mut buf = Buffer::new();
        for n in i8::MIN..=i8::MAX {
            assert_eq!(buf.format(n), &n.to_string(), "i8 failed for {n}");
        }
    }

    #[test]
    fn u16_exhaustive() {
        let mut buf = Buffer::new();
        for n in 0..=u16::MAX {
            assert_eq!(buf.format(n), &n.to_string(), "u16 failed for {n}");
        }
    }

    #[test]
    fn i16_exhaustive() {
        let mut buf = Buffer::new();
        for n in i16::MIN..=i16::MAX {
            assert_eq!(buf.format(n), &n.to_string(), "i16 failed for {n}");
        }
    }

    // ── Boundary tests for larger types ──

    #[test]
    fn u32_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[u32] = &[
            0,
            1,
            9,
            10,
            99,
            100,
            999,
            1000,
            9999,
            10000,
            99999,
            100000,
            999999,
            1000000,
            9999999,
            10000000,
            99999999,
            100000000,
            999999999,
            1000000000,
            u32::MAX,
        ];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "u32 failed for {n}");
        }
    }

    #[test]
    fn i32_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[i32] = &[0, 1, -1, 42, -42, i32::MAX, i32::MIN];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "i32 failed for {n}");
        }
    }

    #[test]
    fn u64_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[u64] = &[
            0,
            1,
            9,
            10,
            99,
            100,
            9999,
            10000,
            99999999,
            100000000,
            999999999999999,
            1000000000000000,
            9999999999999999,
            10000000000000000,
            u64::MAX,
        ];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "u64 failed for {n}");
        }
    }

    #[test]
    fn i64_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[i64] = &[0, 1, -1, 42, -42, i64::MAX, i64::MIN];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "i64 failed for {n}");
        }
    }

    #[test]
    fn u128_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[u128] = &[
            0,
            1,
            u64::MAX as u128,
            u64::MAX as u128 + 1,
            u128::MAX / 2,
            u128::MAX,
        ];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "u128 failed for {n}");
        }
    }

    #[test]
    fn i128_boundaries() {
        let mut buf = Buffer::new();
        let cases: &[i128] = &[0, 1, -1, i128::MAX, i128::MIN];
        for &n in cases {
            assert_eq!(buf.format(n), &n.to_string(), "i128 failed for {n}");
        }
    }

    #[test]
    fn usize_and_isize() {
        let mut buf = Buffer::new();
        assert_eq!(buf.format(0usize), "0");
        assert_eq!(buf.format(usize::MAX), &usize::MAX.to_string());
        assert_eq!(buf.format(-1isize), "-1");
        assert_eq!(buf.format(isize::MIN), &isize::MIN.to_string());
    }

    // ── Property: every power-of-10 boundary ──

    #[test]
    fn u64_every_power_of_10() {
        let mut buf = Buffer::new();
        let mut pow = 1u64;
        loop {
            for &delta in &[0u64, 1] {
                let n = pow.saturating_sub(delta);
                assert_eq!(buf.format(n), &n.to_string(), "u64 pow10 failed for {n}");
            }
            if pow > u64::MAX / 10 {
                break;
            }
            pow *= 10;
        }
    }

    // ── Property: Buffer is reusable ──

    #[test]
    fn buffer_reuse() {
        let mut buf = Buffer::new();
        assert_eq!(buf.format(42u32), "42");
        assert_eq!(buf.format(-99i64), "-99");
        assert_eq!(buf.format(u128::MAX), &u128::MAX.to_string());
        assert_eq!(buf.format(0u8), "0");
    }
}
