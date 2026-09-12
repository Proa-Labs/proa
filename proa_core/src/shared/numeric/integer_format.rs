// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Allocation-free Rust-compatible formatting for primitive integers.

use core::fmt;

use crate::render::DataLoader;
use crate::shared::escape::{
    escape_attr_value_for_dialect, escape_text_for_dialect, EscapeDialect,
};
use crate::shared::mode::ContentMode;
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::web::WebContext;

const ALIGN_NONE: u8 = 0;
const ALIGN_LEFT: u8 = 1;
const ALIGN_RIGHT: u8 = 2;
const ALIGN_CENTER: u8 = 3;

const KIND_DISPLAY: u8 = 0;
const KIND_BINARY: u8 = 1;
const KIND_OCTAL: u8 = 2;
const KIND_LOWER_HEX: u8 = 3;
const KIND_UPPER_HEX: u8 = 4;
const KIND_LOWER_EXP: u8 = 5;
const KIND_UPPER_EXP: u8 = 6;

const INTEGER_BODY_LEN: usize = 128;
const COMPACT_OUTPUT_LEN: usize = 40;
const REPEAT_CHUNK_LEN: usize = 64;

/// A literal Rust formatting specification parsed once by `text!`.
///
/// The numeric fields deliberately use compact integer tags so the proc macro
/// does not need to name internal enums in generated application code.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct __ProaIntegerFormatPlan {
    fill: char,
    align: u8,
    sign_plus: bool,
    alternate: bool,
    zero_pad: bool,
    width: usize,
    precision: Option<usize>,
    kind: u8,
}

impl __ProaIntegerFormatPlan {
    #[doc(hidden)]
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub const fn __proa_new(
        fill: char,
        align: u8,
        sign_plus: bool,
        alternate: bool,
        zero_pad: bool,
        width: usize,
        precision: Option<usize>,
        kind: u8,
    ) -> Self {
        Self {
            fill,
            align,
            sign_plus,
            alternate,
            zero_pad,
            width,
            precision,
            kind,
        }
    }
}

/// Stable autoref dispatch used by `text!` to select a primitive integer
/// encoder without excluding arbitrary Rust formatting implementations.
#[doc(hidden)]
pub trait __ProaWrapInteger<T> {
    type Output: __ProaIntegerFormatValue;

    fn __proa_wrap_integer(
        self,
        plan: __ProaIntegerFormatPlan,
        fallback: fn(&T, &mut dyn fmt::Write) -> fmt::Result,
    ) -> Self::Output;
}

/// Rendering contract shared by specialized primitive values and the exact
/// `core::fmt` fallback generated at the macro call site.
#[doc(hidden)]
pub trait __ProaIntegerFormatValue {
    fn __proa_render_integer_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError>;

    fn __proa_render_integer_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError>;
}

#[doc(hidden)]
pub struct __ProaFastInteger<T> {
    value: T,
    plan: __ProaIntegerFormatPlan,
}

#[doc(hidden)]
pub struct __ProaFallbackInteger<T> {
    value: T,
    format: fn(&T, &mut dyn fmt::Write) -> fmt::Result,
}

impl<T> __ProaWrapInteger<T> for &mut &mut Option<T> {
    type Output = __ProaFallbackInteger<T>;

    #[inline(always)]
    fn __proa_wrap_integer(
        self,
        _plan: __ProaIntegerFormatPlan,
        format: fn(&T, &mut dyn fmt::Write) -> fmt::Result,
    ) -> Self::Output {
        __ProaFallbackInteger {
            value: (**self)
                .take()
                .expect("text! formatted argument initialized"),
            format,
        }
    }
}

impl<T> __ProaIntegerFormatValue for __ProaFallbackInteger<T> {
    #[inline(always)]
    fn __proa_render_integer_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        crate::shared::text::__proa_render_fmt_text_with(&self.value, self.format, cx)
    }

    #[inline(always)]
    fn __proa_render_integer_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        crate::shared::attr_value::__proa_render_fmt_attr_with(&self.value, self.format, cx)
    }
}

/// Operations needed after autoref dispatch. Implementations preserve the
/// original primitive type so common decimal widths still monomorphize to the
/// corresponding `itoa_lut` encoder rather than widening every value to u128.
#[doc(hidden)]
pub trait __ProaPrimitiveInteger: Copy {
    fn __proa_is_negative(self) -> bool;
    fn __proa_magnitude(self) -> u128;
    fn __proa_radix_value(self) -> u128;
    fn __proa_write_magnitude(self, output: &mut [u8; 39]) -> usize;
}

macro_rules! impl_unsigned_integer {
    ($ty:ty, $write:ident, $len:literal) => {
        impl __ProaPrimitiveInteger for $ty {
            #[inline(always)]
            fn __proa_is_negative(self) -> bool {
                false
            }

            #[inline(always)]
            fn __proa_magnitude(self) -> u128 {
                self as u128
            }

            #[inline(always)]
            fn __proa_radix_value(self) -> u128 {
                self as u128
            }

            #[inline(always)]
            fn __proa_write_magnitude(self, output: &mut [u8; 39]) -> usize {
                let output: &mut [u8; $len] = (&mut output[..$len])
                    .try_into()
                    .expect("primitive integer digit buffer has exact capacity");
                itoa_lut::$write(self, output)
            }
        }
    };
}

macro_rules! impl_signed_integer {
    ($ty:ty, $unsigned:ty, $write:ident, $len:literal) => {
        impl __ProaPrimitiveInteger for $ty {
            #[inline(always)]
            fn __proa_is_negative(self) -> bool {
                self < 0
            }

            #[inline(always)]
            fn __proa_magnitude(self) -> u128 {
                self.unsigned_abs() as u128
            }

            #[inline(always)]
            fn __proa_radix_value(self) -> u128 {
                self as $unsigned as u128
            }

            #[inline(always)]
            fn __proa_write_magnitude(self, output: &mut [u8; 39]) -> usize {
                let output: &mut [u8; $len] = (&mut output[..$len])
                    .try_into()
                    .expect("primitive integer digit buffer has exact capacity");
                itoa_lut::$write(self.unsigned_abs(), output)
            }
        }
    };
}

impl_unsigned_integer!(u8, write_u8, 3);
impl_unsigned_integer!(u16, write_u16, 5);
impl_unsigned_integer!(u32, write_u32, 10);
impl_unsigned_integer!(u64, write_u64, 20);
impl_unsigned_integer!(u128, write_u128, 39);
impl_signed_integer!(i8, u8, write_u8, 3);
impl_signed_integer!(i16, u16, write_u16, 5);
impl_signed_integer!(i32, u32, write_u32, 10);
impl_signed_integer!(i64, u64, write_u64, 20);
impl_signed_integer!(i128, u128, write_u128, 39);

impl __ProaPrimitiveInteger for usize {
    #[inline(always)]
    fn __proa_is_negative(self) -> bool {
        false
    }

    #[inline(always)]
    fn __proa_magnitude(self) -> u128 {
        self as u128
    }

    #[inline(always)]
    fn __proa_radix_value(self) -> u128 {
        self as u128
    }

    #[inline(always)]
    fn __proa_write_magnitude(self, output: &mut [u8; 39]) -> usize {
        let output: &mut [u8; 20] = (&mut output[..20])
            .try_into()
            .expect("usize digit buffer has exact capacity");
        itoa_lut::write_u64(self as u64, output)
    }
}

impl __ProaPrimitiveInteger for isize {
    #[inline(always)]
    fn __proa_is_negative(self) -> bool {
        self < 0
    }

    #[inline(always)]
    fn __proa_magnitude(self) -> u128 {
        self.unsigned_abs() as u128
    }

    #[inline(always)]
    fn __proa_radix_value(self) -> u128 {
        self as usize as u128
    }

    #[inline(always)]
    fn __proa_write_magnitude(self, output: &mut [u8; 39]) -> usize {
        let output: &mut [u8; 20] = (&mut output[..20])
            .try_into()
            .expect("isize digit buffer has exact capacity");
        itoa_lut::write_u64(self.unsigned_abs() as u64, output)
    }
}

macro_rules! impl_integer_dispatch {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl __ProaWrapInteger<$ty> for &mut Option<$ty> {
                type Output = __ProaFastInteger<$ty>;

                #[inline(always)]
                fn __proa_wrap_integer(
                    self,
                    plan: __ProaIntegerFormatPlan,
                    _fallback: fn(&$ty, &mut dyn fmt::Write) -> fmt::Result,
                ) -> Self::Output {
                    __ProaFastInteger {
                        value: self
                            .take()
                            .expect("text! formatted integer argument initialized"),
                        plan,
                    }
                }
            }

            impl<'a> __ProaWrapInteger<&'a $ty> for &mut Option<&'a $ty> {
                type Output = __ProaFastInteger<$ty>;

                #[inline(always)]
                fn __proa_wrap_integer(
                    self,
                    plan: __ProaIntegerFormatPlan,
                    _fallback: fn(&&'a $ty, &mut dyn fmt::Write) -> fmt::Result,
                ) -> Self::Output {
                    __ProaFastInteger {
                        value: *self
                            .take()
                            .expect("text! formatted integer argument initialized"),
                        plan,
                    }
                }
            }

            impl<'a> __ProaWrapInteger<&'a mut $ty> for &mut Option<&'a mut $ty> {
                type Output = __ProaFastInteger<$ty>;

                #[inline(always)]
                fn __proa_wrap_integer(
                    self,
                    plan: __ProaIntegerFormatPlan,
                    _fallback: fn(&&'a mut $ty, &mut dyn fmt::Write) -> fmt::Result,
                ) -> Self::Output {
                    __ProaFastInteger {
                        value: *self
                            .take()
                            .expect("text! formatted integer argument initialized"),
                        plan,
                    }
                }
            }
        )+
    };
}

impl_integer_dispatch!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize,);

impl<T: __ProaPrimitiveInteger> __ProaIntegerFormatValue for __ProaFastInteger<T> {
    #[inline(always)]
    fn __proa_render_integer_text<B: WriteBuf, L: DataLoader>(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let mode = cx.current_mode();
        let dialect = cx.__proa_current_escape_dialect();
        render_integer(
            self.value,
            self.plan,
            &mut cx.out,
            FillTarget::Text { mode, dialect },
        )
    }

    #[inline(always)]
    fn __proa_render_integer_attr<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let dialect = cx.__proa_current_escape_dialect();
        render_integer(
            self.value,
            self.plan,
            &mut cx.out,
            FillTarget::Attr { dialect },
        )
    }
}

#[derive(Clone, Copy)]
enum FillTarget {
    Text {
        mode: ContentMode,
        dialect: EscapeDialect,
    },
    Attr {
        dialect: EscapeDialect,
    },
}

struct FormattedInteger {
    sign: Option<u8>,
    prefix: &'static [u8],
    head: [u8; INTEGER_BODY_LEN],
    head_len: usize,
    zeroes: usize,
    tail: [u8; 3],
    tail_len: usize,
}

impl FormattedInteger {
    #[inline(always)]
    fn body_len(&self) -> usize {
        self.head_len
            .saturating_add(self.zeroes)
            .saturating_add(self.tail_len)
    }

    #[inline(always)]
    fn content_len(&self) -> usize {
        usize::from(self.sign.is_some())
            .saturating_add(self.prefix.len())
            .saturating_add(self.body_len())
    }
}

#[derive(Clone, Copy)]
struct Layout {
    left_fill: usize,
    internal_zeroes: usize,
    right_fill: usize,
    fill: char,
}

#[inline(always)]
fn render_integer<T: __ProaPrimitiveInteger, B: WriteBuf>(
    value: T,
    plan: __ProaIntegerFormatPlan,
    output: &mut B,
    target: FillTarget,
) -> Result<(), WriteError> {
    if plan.kind == KIND_DISPLAY {
        return render_decimal(value, plan, output, target);
    }

    let formatted = format_integer(value, plan);
    let layout = layout(formatted.content_len(), plan);
    let total_chars = formatted
        .content_len()
        .saturating_add(layout.left_fill)
        .saturating_add(layout.internal_zeroes)
        .saturating_add(layout.right_fill);

    if is_unescaped_fill(layout.fill) && total_chars <= COMPACT_OUTPUT_LEN {
        return output.extend_with(total_chars, |destination| {
            write_compact(&formatted, layout, destination)
        });
    }

    write_fill(output, layout.fill, layout.left_fill, target)?;
    if let Some(sign) = formatted.sign {
        output.push(sign)?;
    }
    output.extend(formatted.prefix)?;
    write_repeated_byte(output, b'0', layout.internal_zeroes)?;
    output.extend(&formatted.head[..formatted.head_len])?;
    write_repeated_byte(output, b'0', formatted.zeroes)?;
    output.extend(&formatted.tail[..formatted.tail_len])?;
    write_fill(output, layout.fill, layout.right_fill, target)
}

#[inline(always)]
fn render_decimal<T: __ProaPrimitiveInteger, B: WriteBuf>(
    value: T,
    plan: __ProaIntegerFormatPlan,
    output: &mut B,
    target: FillTarget,
) -> Result<(), WriteError> {
    let magnitude = value.__proa_magnitude();
    if plan.zero_pad && !value.__proa_is_negative() && !plan.sign_plus {
        let limit = match plan.width {
            2 => 100,
            3 => 1_000,
            4 => 10_000,
            _ => 0,
        };
        if limit != 0 && magnitude < limit {
            return output.extend_with(plan.width, |destination| {
                let mut remaining = magnitude as u32;
                for slot in destination[..plan.width].iter_mut().rev() {
                    *slot = b'0' + (remaining % 10) as u8;
                    remaining /= 10;
                }
                plan.width
            });
        }
    }

    let mut digits = [0u8; 39];
    let digit_len = value.__proa_write_magnitude(&mut digits);
    let sign = if value.__proa_is_negative() {
        Some(b'-')
    } else if plan.sign_plus {
        Some(b'+')
    } else {
        None
    };
    let content_len = digit_len + usize::from(sign.is_some());
    let layout = layout(content_len, plan);
    let total_chars = content_len
        .saturating_add(layout.left_fill)
        .saturating_add(layout.internal_zeroes)
        .saturating_add(layout.right_fill);

    if is_unescaped_fill(layout.fill) && total_chars <= COMPACT_OUTPUT_LEN {
        return output.extend_with(total_chars, |destination| {
            let mut cursor = 0;
            destination[..layout.left_fill].fill(layout.fill as u8);
            cursor += layout.left_fill;
            if let Some(sign) = sign {
                destination[cursor] = sign;
                cursor += 1;
            }
            destination[cursor..cursor + layout.internal_zeroes].fill(b'0');
            cursor += layout.internal_zeroes;
            destination[cursor..cursor + digit_len].copy_from_slice(&digits[..digit_len]);
            cursor += digit_len;
            destination[cursor..cursor + layout.right_fill].fill(layout.fill as u8);
            cursor + layout.right_fill
        });
    }

    write_fill(output, layout.fill, layout.left_fill, target)?;
    if let Some(sign) = sign {
        output.push(sign)?;
    }
    write_repeated_byte(output, b'0', layout.internal_zeroes)?;
    output.extend(&digits[..digit_len])?;
    write_fill(output, layout.fill, layout.right_fill, target)
}

#[inline(always)]
fn layout(content_len: usize, plan: __ProaIntegerFormatPlan) -> Layout {
    let padding = plan.width.saturating_sub(content_len);
    if padding != 0 && plan.zero_pad {
        return Layout {
            left_fill: 0,
            internal_zeroes: padding,
            right_fill: 0,
            fill: '0',
        };
    }

    let (left_fill, right_fill) = match plan.align {
        ALIGN_LEFT => (0, padding),
        ALIGN_CENTER => (padding / 2, padding - padding / 2),
        ALIGN_RIGHT | ALIGN_NONE => (padding, 0),
        _ => {
            debug_assert!(false, "unknown integer alignment tag");
            (padding, 0)
        }
    };
    Layout {
        left_fill,
        internal_zeroes: 0,
        right_fill,
        fill: plan.fill,
    }
}

#[inline(always)]
fn write_compact(formatted: &FormattedInteger, layout: Layout, destination: &mut [u8]) -> usize {
    let mut cursor = 0;
    destination[cursor..cursor + layout.left_fill].fill(layout.fill as u8);
    cursor += layout.left_fill;
    if let Some(sign) = formatted.sign {
        destination[cursor] = sign;
        cursor += 1;
    }
    destination[cursor..cursor + formatted.prefix.len()].copy_from_slice(formatted.prefix);
    cursor += formatted.prefix.len();
    destination[cursor..cursor + layout.internal_zeroes].fill(b'0');
    cursor += layout.internal_zeroes;
    destination[cursor..cursor + formatted.head_len]
        .copy_from_slice(&formatted.head[..formatted.head_len]);
    cursor += formatted.head_len;
    destination[cursor..cursor + formatted.zeroes].fill(b'0');
    cursor += formatted.zeroes;
    destination[cursor..cursor + formatted.tail_len]
        .copy_from_slice(&formatted.tail[..formatted.tail_len]);
    cursor += formatted.tail_len;
    destination[cursor..cursor + layout.right_fill].fill(layout.fill as u8);
    cursor + layout.right_fill
}

#[inline]
fn write_repeated_byte<B: WriteBuf>(
    output: &mut B,
    byte: u8,
    mut count: usize,
) -> Result<(), WriteError> {
    if count == 0 {
        return Ok(());
    }
    let chunk = [byte; REPEAT_CHUNK_LEN];
    while count >= REPEAT_CHUNK_LEN {
        output.extend(&chunk)?;
        count -= REPEAT_CHUNK_LEN;
    }
    output.extend(&chunk[..count])
}

#[inline]
fn write_fill<B: WriteBuf>(
    output: &mut B,
    fill: char,
    mut count: usize,
    target: FillTarget,
) -> Result<(), WriteError> {
    if count == 0 {
        return Ok(());
    }
    if is_unescaped_fill(fill) {
        return write_repeated_byte(output, fill as u8, count);
    }

    let mut encoded = [0u8; 4];
    let encoded = fill.encode_utf8(&mut encoded).as_bytes();
    let repetitions = (256 / encoded.len()).max(1);
    let mut chunk = [0u8; 256];
    for index in 0..repetitions {
        let start = index * encoded.len();
        chunk[start..start + encoded.len()].copy_from_slice(encoded);
    }
    while count != 0 {
        let current = count.min(repetitions);
        let byte_len = current * encoded.len();
        let text = core::str::from_utf8(&chunk[..byte_len])
            .expect("fill chunk consists of complete UTF-8 character encodings");
        match target {
            FillTarget::Text { mode, dialect } => {
                escape_text_for_dialect(output, text, mode, dialect)?;
            }
            FillTarget::Attr { dialect } => {
                escape_attr_value_for_dialect(output, text, dialect)?;
            }
        }
        count -= current;
    }
    Ok(())
}

#[inline(always)]
fn is_unescaped_fill(fill: char) -> bool {
    fill == ' ' || (fill.is_ascii_graphic() && !matches!(fill, '&' | '<' | '>' | '"' | '\''))
}

#[inline(always)]
fn format_integer<T: __ProaPrimitiveInteger>(
    value: T,
    plan: __ProaIntegerFormatPlan,
) -> FormattedInteger {
    let radix = matches!(
        plan.kind,
        KIND_BINARY | KIND_OCTAL | KIND_LOWER_HEX | KIND_UPPER_HEX
    );
    let negative = !radix && value.__proa_is_negative();
    let sign = if negative {
        Some(b'-')
    } else if plan.sign_plus {
        Some(b'+')
    } else {
        None
    };
    let prefix = if plan.alternate {
        match plan.kind {
            KIND_BINARY => b"0b" as &'static [u8],
            KIND_OCTAL => b"0o",
            KIND_LOWER_HEX | KIND_UPPER_HEX => b"0x",
            _ => b"",
        }
    } else {
        b""
    };
    let mut formatted = FormattedInteger {
        sign,
        prefix,
        head: [0; INTEGER_BODY_LEN],
        head_len: 0,
        zeroes: 0,
        tail: [0; 3],
        tail_len: 0,
    };

    match plan.kind {
        KIND_DISPLAY => {
            let digits: &mut [u8; 39] = (&mut formatted.head[..39])
                .try_into()
                .expect("integer body has room for decimal digits");
            formatted.head_len = value.__proa_write_magnitude(digits);
        }
        KIND_BINARY => {
            formatted.head_len =
                write_radix(value.__proa_radix_value(), 1, b"01", &mut formatted.head);
        }
        KIND_OCTAL => {
            formatted.head_len = write_radix(
                value.__proa_radix_value(),
                3,
                b"01234567",
                &mut formatted.head,
            );
        }
        KIND_LOWER_HEX => {
            formatted.head_len = write_radix(
                value.__proa_radix_value(),
                4,
                b"0123456789abcdef",
                &mut formatted.head,
            );
        }
        KIND_UPPER_HEX => {
            formatted.head_len = write_radix(
                value.__proa_radix_value(),
                4,
                b"0123456789ABCDEF",
                &mut formatted.head,
            );
        }
        KIND_LOWER_EXP | KIND_UPPER_EXP => {
            write_scientific(
                value.__proa_magnitude(),
                plan.precision,
                if plan.kind == KIND_LOWER_EXP {
                    b'e'
                } else {
                    b'E'
                },
                &mut formatted,
            );
        }
        _ => {
            debug_assert!(false, "unknown integer format kind tag");
            formatted.head[0] = b'0';
            formatted.head_len = 1;
        }
    }
    formatted
}

#[inline]
fn write_radix(value: u128, bits: u32, digits: &[u8], output: &mut [u8; 128]) -> usize {
    if value == 0 {
        output[0] = b'0';
        return 1;
    }
    let significant_bits = u128::BITS - value.leading_zeros();
    let count = significant_bits.div_ceil(bits) as usize;
    let mask = (1u128 << bits) - 1;
    for (index, slot) in output[..count].iter_mut().enumerate() {
        let shift = (count - index - 1) as u32 * bits;
        *slot = digits[((value >> shift) & mask) as usize];
    }
    count
}

/// Port of `core::fmt::num`'s integer exponential algorithm. Precision means
/// fraction digits, trailing zeroes are removed when it is absent, and
/// discarded digits use round-half-to-even.
#[inline]
fn write_scientific(
    value: u128,
    precision: Option<usize>,
    letter_e: u8,
    formatted: &mut FormattedInteger,
) {
    let mut exponent = value.checked_ilog10().unwrap_or(0) as usize;
    let mut coefficient_precision = exponent;
    let mut coefficient = value;

    let more_precision = match precision {
        None => {
            while coefficient_precision != 0 && coefficient.is_multiple_of(10) {
                coefficient /= 10;
                coefficient_precision -= 1;
            }
            0
        }
        Some(requested) if requested >= coefficient_precision => requested - coefficient_precision,
        Some(requested) => {
            let less_precision = coefficient_precision - requested;
            let scale = 10u128
                .checked_pow(less_precision as u32)
                .expect("integer scientific precision scale is bounded by u128 digits");
            let floor = coefficient / scale;
            let over = coefficient % scale;
            let half = scale / 2;
            let round_up = over > half || (over == half && floor & 1 != 0);
            coefficient = floor + u128::from(round_up);
            coefficient_precision = requested;

            if round_up && coefficient.checked_ilog10().unwrap_or(0) as usize > requested {
                coefficient /= 10;
                exponent += 1;
            }
            0
        }
    };

    let mut digits = [0u8; 39];
    let digit_len = itoa_lut::write_u128(coefficient, &mut digits);
    formatted.head[0] = digits[0];
    formatted.head_len = 1;
    if coefficient_precision != 0 || more_precision != 0 {
        formatted.head[1] = b'.';
        formatted.head_len = 2;
        let fraction_start = digit_len.saturating_sub(coefficient_precision);
        let fraction = &digits[fraction_start..digit_len];
        formatted.head[2..2 + fraction.len()].copy_from_slice(fraction);
        formatted.head_len += fraction.len();
        if fraction.len() < coefficient_precision {
            let missing = coefficient_precision - fraction.len();
            formatted
                .head
                .copy_within(2..formatted.head_len, 2 + missing);
            formatted.head[2..2 + missing].fill(b'0');
            formatted.head_len += missing;
        }
    }
    formatted.zeroes = more_precision;
    formatted.tail[0] = letter_e;
    if exponent < 10 {
        formatted.tail[1] = b'0' + exponent as u8;
        formatted.tail_len = 2;
    } else {
        formatted.tail[1] = b'0' + (exponent / 10) as u8;
        formatted.tail[2] = b'0' + (exponent % 10) as u8;
        formatted.tail_len = 3;
    }
}
