// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! First-class rendering support for all primitive numeric types.
//!
//! Implements [`crate::WebRenderSync`], [`AttrValue`], [`crate::MdRenderSync`],
//! and [`crate::MdRender`] for all 14 Rust primitive numeric types using
//! [`itoa_lut`] (integers) and [`zmij`] (floats) for zero-allocation,
//! zero-escape formatting. Literal `text!` integer format plans additionally
//! cover width, alignment/fill, signs, radix prefixes, and scientific notation
//! without routing primitive values through `core::fmt`.
//!
//! Integer rendering bypasses the `itoa_lut::Buffer` API entirely, writing
//! digits directly into a sized stack array and extending the output buffer
//! with raw bytes — no `&str` conversion, no UTF-8 validation.
//!
//! Digits, minus signs, decimal points, and `e` are all HTML-safe, so no
//! escaping pass is needed.
use core::fmt;

use crate::md::MdContext;
use crate::render::DataLoader;
use crate::render::{MdRender, MdRenderSync, RenderOutcome, WebRenderSync};
use crate::shared::attr_value::AttrValue;
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::web::WebContext;

mod integer_format;

#[doc(hidden)]
pub use integer_format::{__ProaIntegerFormatPlan, __ProaIntegerFormatValue, __ProaWrapInteger};

/// Write Zmij's shortest representation while preserving Ryu's historical
/// spelling for positive exponents (`e12`, not `e+12`).
#[inline(always)]
fn write_shortest<B: WriteBuf>(out: &mut B, formatted: &str) -> Result<(), WriteError> {
    let bytes = formatted.as_bytes();
    if let Some(plus) = memchr::memchr(b'+', bytes) {
        debug_assert!(plus > 0 && bytes[plus - 1] == b'e');
        out.extend_with(bytes.len() - 1, |dest| {
            dest[..plus].copy_from_slice(&bytes[..plus]);
            dest[plus..bytes.len() - 1].copy_from_slice(&bytes[plus + 1..]);
            bytes.len() - 1
        })
    } else {
        out.extend(bytes)
    }
}

#[inline(always)]
fn write_shortest_md<B: WriteBuf>(
    cx: &mut MdContext<'_, B>,
    formatted: &str,
) -> Result<(), WriteError> {
    let bytes = formatted.as_bytes();
    if let Some(plus) = memchr::memchr(b'+', bytes) {
        debug_assert!(plus > 0 && bytes[plus - 1] == b'e');
        cx.write_numeric_with(bytes.len() - 1, |dest| {
            dest[..plus].copy_from_slice(&bytes[..plus]);
            dest[plus..bytes.len() - 1].copy_from_slice(&bytes[plus + 1..]);
            bytes.len() - 1
        })
    } else {
        cx.write_numeric_str(formatted)
    }
}
macro_rules! impl_integer_render {
    ($unsigned:ty, $write_fn:ident, $max_len:expr) => {
        impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for $unsigned {
            #[inline(always)]
            fn render(
                self,
                cx: &mut WebContext<L, B>,
            ) -> Result<(), crate::shared::write_buf::WriteError> {
                let v = self;
                cx.out.extend_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl AttrValue for $unsigned {
            #[inline(always)]
            fn should_render(&self) -> bool {
                true
            }
            #[inline(always)]
            fn render_attr_value<B: WriteBuf, L: DataLoader>(
                &self,
                cx: &mut WebContext<L, B>,
            ) -> Result<(), crate::shared::write_buf::WriteError> {
                let v = *self;
                cx.out.extend_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl<B: WriteBuf> MdRenderSync<B> for $unsigned {
            #[inline(always)]
            fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
                let v = self;
                cx.write_numeric_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl<B: WriteBuf> MdRender<B> for $unsigned {
            fn render_md<'cx, 'out>(
                self,
                cx: &'cx mut MdContext<'out, B>,
            ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
            where
                Self: 'cx,
                'out: 'cx,
            {
                RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
                    MdRenderSync::render_md(self, cx),
                )
            }
        }
    };
}
macro_rules! impl_signed_render {
    ($signed:ty, $write_fn:ident, $max_len:expr) => {
        impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for $signed {
            #[inline(always)]
            fn render(
                self,
                cx: &mut WebContext<L, B>,
            ) -> Result<(), crate::shared::write_buf::WriteError> {
                let v = self;
                cx.out.extend_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl AttrValue for $signed {
            #[inline(always)]
            fn should_render(&self) -> bool {
                true
            }
            #[inline(always)]
            fn render_attr_value<B: WriteBuf, L: DataLoader>(
                &self,
                cx: &mut WebContext<L, B>,
            ) -> Result<(), crate::shared::write_buf::WriteError> {
                let v = *self;
                cx.out.extend_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl<B: WriteBuf> MdRenderSync<B> for $signed {
            #[inline(always)]
            fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
                let v = self;
                cx.write_numeric_with($max_len, |buf| {
                    itoa_lut::$write_fn(v, buf.try_into().unwrap())
                })
            }
        }
        impl<B: WriteBuf> MdRender<B> for $signed {
            fn render_md<'cx, 'out>(
                self,
                cx: &'cx mut MdContext<'out, B>,
            ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
            where
                Self: 'cx,
                'out: 'cx,
            {
                RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
                    MdRenderSync::render_md(self, cx),
                )
            }
        }
    };
}
impl_integer_render!(u8, write_u8, 3);
impl_integer_render!(u16, write_u16, 5);
impl_integer_render!(u32, write_u32, 10);
impl_integer_render!(u64, write_u64, 20);
impl_integer_render!(u128, write_u128, 39);
impl_signed_render!(i8, write_i8, 4);
impl_signed_render!(i16, write_i16, 6);
impl_signed_render!(i32, write_i32, 11);
impl_signed_render!(i64, write_i64, 21);
impl_signed_render!(i128, write_i128, 40);
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for usize {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        <u64 as WebRenderSync<L, B>>::render(self as u64, cx)
    }
}
impl AttrValue for usize {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        (*self as u64).render_attr_value(cx)
    }
}
impl<B: WriteBuf> MdRenderSync<B> for usize {
    #[inline(always)]
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        MdRenderSync::render_md(self as u64, cx)
    }
}
impl<B: WriteBuf> MdRender<B> for usize {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for isize {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        <i64 as WebRenderSync<L, B>>::render(self as i64, cx)
    }
}
impl AttrValue for isize {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), crate::shared::write_buf::WriteError> {
        (*self as i64).render_attr_value(cx)
    }
}
impl<B: WriteBuf> MdRenderSync<B> for isize {
    #[inline(always)]
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        MdRenderSync::render_md(self as i64, cx)
    }
}
impl<B: WriteBuf> MdRender<B> for isize {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}
macro_rules! impl_float_render {
    ($($ty:ty),+) => {
        $(
            impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for $ty {
                #[inline(always)]
                fn render(
                    self,
                    cx: &mut WebContext<L, B>,
                ) -> Result<(), crate::shared::write_buf::WriteError> {
                    let mut buf = zmij::Buffer::new();
                    write_shortest(&mut cx.out, buf.format(self))
                }
            }
            impl AttrValue for $ty {
                #[inline(always)]
                fn should_render(&self) -> bool { true }
                #[inline(always)]
                fn render_attr_value<B: WriteBuf, L: DataLoader>(
                    &self,
                    cx: &mut WebContext<L, B>,
                ) -> Result<(), crate::shared::write_buf::WriteError> {
                    let mut buf = zmij::Buffer::new();
                    write_shortest(&mut cx.out, buf.format(*self))
                }
            }
            impl<B: WriteBuf> MdRenderSync<B> for $ty {
                #[inline(always)]
                fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
                    let mut buf = zmij::Buffer::new();
                    write_shortest_md(cx, buf.format(self))
                }
            }
            impl<B: WriteBuf> MdRender<B> for $ty {
                fn render_md<'cx, 'out>(
                    self,
                    cx: &'cx mut MdContext<'out, B>,
                ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
                    RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
                        MdRenderSync::render_md(self, cx),
                    )
                }
            }
        )+
    };
}
impl_float_render!(f32, f64);

const FIXED_FORMAT_BUFFER_LEN: usize = 40;
const MAX_FAST_FIXED_PRECISION: usize = 32;

#[derive(Clone, Copy)]
struct FixedDecimal {
    magnitude: u128,
    precision: usize,
    negative: bool,
}

impl FixedDecimal {
    #[inline(always)]
    fn output_len(self) -> usize {
        let digits = decimal_digits(self.magnitude);
        let body = if self.precision == 0 {
            digits
        } else if digits > self.precision {
            digits + 1
        } else {
            self.precision + 2
        };
        body + usize::from(self.negative)
    }
}

#[inline(always)]
fn decimal_digits(value: u128) -> usize {
    if value == 0 {
        1
    } else {
        value.ilog10() as usize + 1
    }
}

#[inline(always)]
const fn pow5(exponent: usize) -> u128 {
    let mut value = 1u128;
    let mut index = 0;
    while index < exponent {
        value *= 5;
        index += 1;
    }
    value
}

/// Convert a finite f64 into the exactly rounded integer represented by a
/// fixed-precision decimal. The arithmetic mirrors `value * 10^precision`
/// without doing a floating-point multiply, so halfway cases retain Rust's
/// round-to-even behavior.
#[inline]
fn fixed_decimal(value: f64, precision: usize) -> Option<FixedDecimal> {
    if !value.is_finite() || precision > MAX_FAST_FIXED_PRECISION {
        return None;
    }

    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let absolute = bits & !(1u64 << 63);
    let encoded_exponent = ((absolute >> 52) & 0x7ff) as i32;
    let fraction = absolute & ((1u64 << 52) - 1);
    let (significand, binary_exponent) = if encoded_exponent == 0 {
        (fraction, -1074)
    } else {
        ((1u64 << 52) | fraction, encoded_exponent - 1075)
    };

    let product = (significand as u128).checked_mul(pow5(precision))?;
    let shift = binary_exponent + precision as i32;
    let magnitude = if shift >= 0 {
        let shift = shift as u32;
        if shift >= u128::BITS || product > (u128::MAX >> shift) {
            return None;
        }
        product << shift
    } else {
        let shift = (-shift) as u32;
        if shift > u128::BITS {
            0
        } else if shift == u128::BITS {
            u128::from(product > (1u128 << 127))
        } else {
            let quotient = product >> shift;
            let remainder = product & ((1u128 << shift) - 1);
            let halfway = 1u128 << (shift - 1);
            let round_up = remainder > halfway || (remainder == halfway && quotient & 1 != 0);
            quotient.checked_add(u128::from(round_up))?
        }
    };

    let fixed = FixedDecimal {
        magnitude,
        precision,
        negative,
    };
    (fixed.output_len() <= FIXED_FORMAT_BUFFER_LEN).then_some(fixed)
}

#[inline]
fn write_fixed_decimal(value: FixedDecimal, output: &mut [u8; FIXED_FORMAT_BUFFER_LEN]) -> usize {
    let sign_len = usize::from(value.negative);
    let digits_buf: &mut [u8; 39] = (&mut output[sign_len..sign_len + 39])
        .try_into()
        .expect("fixed numeric buffer has room for u128 digits");
    let digits = itoa_lut::write_u128(value.magnitude, digits_buf);

    let len = if value.precision == 0 {
        sign_len + digits
    } else if digits > value.precision {
        let decimal_point = sign_len + digits - value.precision;
        output.copy_within(decimal_point..sign_len + digits, decimal_point + 1);
        output[decimal_point] = b'.';
        sign_len + digits + 1
    } else {
        let digits_start = sign_len + 2 + value.precision - digits;
        output.copy_within(sign_len..sign_len + digits, digits_start);
        output[sign_len] = b'0';
        output[sign_len + 1] = b'.';
        output[sign_len + 2..digits_start].fill(b'0');
        sign_len + value.precision + 2
    };

    if value.negative {
        output[0] = b'-';
    }
    len
}

struct NumericFmtWriter<'a, B: WriteBuf> {
    output: &'a mut B,
    error: Option<WriteError>,
}

impl<B: WriteBuf> fmt::Write for NumericFmtWriter<'_, B> {
    #[inline]
    fn write_str(&mut self, value: &str) -> fmt::Result {
        match self.output.extend(value.as_bytes()) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.error = Some(error);
                Err(fmt::Error)
            }
        }
    }
}

#[inline]
fn render_std_fixed<T: fmt::Display, B: WriteBuf>(
    value: T,
    precision: usize,
    output: &mut B,
) -> Result<(), WriteError> {
    let mut writer = NumericFmtWriter {
        output,
        error: None,
    };
    match fmt::write(&mut writer, format_args!("{:.*}", precision, value)) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer.error.unwrap_or(WriteError::InvalidAttribute(
            "failed to format numeric value",
        ))),
    }
}

#[inline(always)]
fn render_fixed_f64<B: WriteBuf>(
    value: f64,
    precision: usize,
    output: &mut B,
) -> Result<(), WriteError> {
    if let Some(fixed) = fixed_decimal(value, precision) {
        let mut buffer = [0u8; FIXED_FORMAT_BUFFER_LEN];
        let len = write_fixed_decimal(fixed, &mut buffer);
        output.extend(&buffer[..len])
    } else {
        render_std_fixed(value, precision, output)
    }
}

#[inline(always)]
fn render_fixed_f32<B: WriteBuf>(
    value: f32,
    precision: usize,
    output: &mut B,
) -> Result<(), WriteError> {
    if let Some(fixed) = fixed_decimal(value as f64, precision) {
        let mut buffer = [0u8; FIXED_FORMAT_BUFFER_LEN];
        let len = write_fixed_decimal(fixed, &mut buffer);
        output.extend(&buffer[..len])
    } else {
        render_std_fixed(value, precision, output)
    }
}

/// Stable autoref dispatch used by `text!` to select primitive float
/// formatting without excluding arbitrary `Display` implementations.
///
/// The macro starts with `&mut Option<T>`. Exact receiver implementations win
/// for `f32` and `f64`; every other `Display` type resolves one autoref deeper
/// to `&mut &mut Option<T>`. Keeping the value in an `Option` lets either
/// selected implementation move it into its wrapper without cloning it.
#[doc(hidden)]
pub trait __ProaWrapFixed {
    type Output<const PRECISION: usize>: __ProaFixedFormatValue<PRECISION>;

    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION>;
}

#[doc(hidden)]
pub struct __ProaFastFixedF64<const PRECISION: usize>(f64);

#[doc(hidden)]
pub struct __ProaFastFixedF32<const PRECISION: usize>(f32);

#[doc(hidden)]
pub struct __ProaDisplayFixed<T, const PRECISION: usize>(T);

impl __ProaWrapFixed for &mut Option<f64> {
    type Output<const PRECISION: usize> = __ProaFastFixedF64<PRECISION>;

    #[inline(always)]
    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION> {
        __ProaFastFixedF64(self.take().expect("text! fixed argument initialized"))
    }
}

impl __ProaWrapFixed for &mut Option<f32> {
    type Output<const PRECISION: usize> = __ProaFastFixedF32<PRECISION>;

    #[inline(always)]
    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION> {
        __ProaFastFixedF32(self.take().expect("text! fixed argument initialized"))
    }
}

impl __ProaWrapFixed for &mut Option<&f64> {
    type Output<const PRECISION: usize> = __ProaFastFixedF64<PRECISION>;

    #[inline(always)]
    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION> {
        __ProaFastFixedF64(*self.take().expect("text! fixed argument initialized"))
    }
}

impl __ProaWrapFixed for &mut Option<&f32> {
    type Output<const PRECISION: usize> = __ProaFastFixedF32<PRECISION>;

    #[inline(always)]
    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION> {
        __ProaFastFixedF32(*self.take().expect("text! fixed argument initialized"))
    }
}

impl<T: fmt::Display> __ProaWrapFixed for &mut &mut Option<T> {
    type Output<const PRECISION: usize> = __ProaDisplayFixed<T, PRECISION>;

    #[inline(always)]
    fn __proa_wrap_fixed<const PRECISION: usize>(self) -> Self::Output<PRECISION> {
        __ProaDisplayFixed((**self).take().expect("text! fixed argument initialized"))
    }
}

/// Rendering contract implemented by the specialized and generic fixed-value
/// wrappers constructed by `text!`.
#[doc(hidden)]
pub trait __ProaFixedFormatValue<const PRECISION: usize> {
    fn __proa_render_fixed_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError>;

    fn __proa_render_fixed_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError>;
}

impl<const PRECISION: usize> __ProaFixedFormatValue<PRECISION> for __ProaFastFixedF64<PRECISION> {
    #[inline(always)]
    fn __proa_render_fixed_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        render_fixed_f64(self.0, PRECISION, &mut cx.out)
    }

    #[inline(always)]
    fn __proa_render_fixed_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        render_fixed_f64(self.0, PRECISION, &mut cx.out)
    }
}

impl<const PRECISION: usize> __ProaFixedFormatValue<PRECISION> for __ProaFastFixedF32<PRECISION> {
    #[inline(always)]
    fn __proa_render_fixed_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        render_fixed_f32(self.0, PRECISION, &mut cx.out)
    }

    #[inline(always)]
    fn __proa_render_fixed_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        render_fixed_f32(self.0, PRECISION, &mut cx.out)
    }
}

impl<T: fmt::Display, const PRECISION: usize> __ProaFixedFormatValue<PRECISION>
    for __ProaDisplayFixed<T, PRECISION>
{
    #[inline(always)]
    fn __proa_render_fixed_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        crate::shared::text::__proa_render_fmt_text(format_args!("{:.*}", PRECISION, self.0), cx)
    }

    #[inline(always)]
    fn __proa_render_fixed_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        crate::shared::attr_value::__proa_render_fmt_attr(
            format_args!("{:.*}", PRECISION, &self.0),
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::attr_value::AttrValue;
    fn render_web<T: WebRenderSync<::proa_core::NoLoader, Vec<u8>>>(val: T) -> String {
        let mut cx = WebContext::with_buffer(Vec::new());
        val.render(&mut cx).unwrap();
        String::from_utf8(cx.into_output()).unwrap()
    }
    fn render_attr<T: AttrValue>(val: &T) -> String {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        val.render_attr_value(&mut cx).unwrap();
        String::from_utf8(buf).unwrap()
    }
    fn render_md<T: MdRenderSync<Vec<u8>>>(val: T) -> String {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        MdRenderSync::render_md(val, &mut cx).unwrap();
        String::from_utf8(buf).unwrap()
    }
    fn render_fixed64(val: f64, precision: usize) -> String {
        let mut buf = Vec::new();
        render_fixed_f64(val, precision, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }
    fn render_fixed32(val: f32, precision: usize) -> String {
        let mut buf = Vec::new();
        render_fixed_f32(val, precision, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }
    #[test]
    fn u32_zero() {
        assert_eq!(render_web(0u32), "0");
    }
    #[test]
    fn u32_max() {
        assert_eq!(render_web(u32::MAX), "4294967295");
    }
    #[test]
    fn i32_min() {
        assert_eq!(render_web(i32::MIN), "-2147483648");
    }
    #[test]
    fn i32_negative() {
        assert_eq!(render_web(-42i32), "-42");
    }
    #[test]
    fn u64_max() {
        assert_eq!(render_web(u64::MAX), "18446744073709551615");
    }
    #[test]
    fn u8_boundary() {
        assert_eq!(render_web(0u8), "0");
        assert_eq!(render_web(255u8), "255");
    }
    #[test]
    fn usize_render() {
        assert_eq!(render_web(42usize), "42");
    }
    #[test]
    fn f64_pi() {
        assert_eq!(render_web(std::f64::consts::PI), "3.141592653589793");
    }
    #[test]
    fn f64_nan() {
        assert_eq!(render_web(f64::NAN), "NaN");
    }
    #[test]
    fn f64_inf() {
        assert_eq!(render_web(f64::INFINITY), "inf");
    }
    #[test]
    fn f64_neg_inf() {
        assert_eq!(render_web(f64::NEG_INFINITY), "-inf");
    }
    #[test]
    fn f32_basic() {
        assert_eq!(render_web(1.5f32), "1.5");
    }
    #[test]
    fn shortest_positive_exponent_preserves_ryu_spelling() {
        assert_eq!(render_web(1e40_f64), "1e40");
        assert_eq!(render_attr(&1e40_f64), "1e40");
        assert_eq!(render_md(1e40_f64), "1e40");
    }
    #[test]
    fn fixed_float_edge_cases_match_std() {
        let values = [
            0.0,
            -0.0,
            0.125,
            0.375,
            1.005,
            9.999,
            -9.999,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for value in values {
            for precision in [0, 1, 2, 3, 6, 9, 15, 32, 33, 100] {
                assert_eq!(
                    render_fixed64(value, precision),
                    format!("{:.*}", precision, value),
                    "value={value:?}, precision={precision}",
                );
            }
        }
    }
    #[test]
    fn fast_fixed_f64_matches_std_across_random_bits() {
        let mut bits = 0x6a09_e667_f3bc_c909u64;
        let mut checked = 0usize;
        for _ in 0..50_000 {
            bits = bits
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let value = f64::from_bits(bits);
            for precision in [0, 1, 2, 6, 9, 15, 32] {
                if fixed_decimal(value, precision).is_some() {
                    assert_eq!(
                        render_fixed64(value, precision),
                        format!("{:.*}", precision, value),
                        "bits={bits:#018x}, precision={precision}",
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked > 50_000,
            "insufficient fast-path coverage: {checked}"
        );
    }
    #[test]
    fn fast_fixed_f32_matches_std_across_random_bits() {
        let mut bits = 0x243f_6a88u32;
        let mut checked = 0usize;
        for _ in 0..25_000 {
            bits = bits.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
            let value = f32::from_bits(bits);
            for precision in [0, 1, 2, 6, 9, 15, 32] {
                if fixed_decimal(value as f64, precision).is_some() {
                    assert_eq!(
                        render_fixed32(value, precision),
                        format!("{:.*}", precision, value),
                        "bits={bits:#010x}, precision={precision}",
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked > 50_000,
            "insufficient fast-path coverage: {checked}"
        );
    }
    #[test]
    fn integer_attr_always_renders() {
        assert!(42u32.should_render());
        assert!(0u32.should_render());
        assert!((-1i32).should_render());
        assert!(!42u32.is_boolean_attr());
    }
    #[test]
    fn float_attr_always_renders() {
        assert!(2.5f64.should_render());
        assert!(!2.5f64.is_boolean_attr());
    }
    #[test]
    fn web_and_attr_match_integer() {
        let val = 12345u32;
        assert_eq!(render_web(val), render_attr(&val));
    }
    #[test]
    fn web_and_attr_match_negative() {
        let val = -99i64;
        assert_eq!(render_web(val), render_attr(&val));
    }
    #[test]
    fn web_and_attr_match_float() {
        let val = 2.75f64;
        assert_eq!(render_web(val), render_attr(&val));
    }
    #[test]
    fn md_matches_web_integer() {
        assert_eq!(render_md(42u32), render_web(42u32));
    }
    #[test]
    fn md_matches_web_float() {
        assert_eq!(render_md(2.5f64), render_web(2.5f64));
    }
    #[test]
    fn md_negative() {
        assert_eq!(render_md(-1i32), "-1");
    }
}
