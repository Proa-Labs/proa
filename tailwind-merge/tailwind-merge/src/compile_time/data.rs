// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]
#![allow(unused_imports)]

#[derive(Clone, Copy)]
pub struct ClassEntry {
    pub class: &'static str,
    pub group_id: u16,
}

#[derive(Clone, Copy)]
pub struct PrefixValidator {
    pub prefix: &'static str,
    pub requires_dash: bool,
    pub validator: ValidatorKind,
    pub group_id: u16,
}

#[derive(Clone, Copy)]
pub struct BareValidator {
    pub validator: ValidatorKind,
    pub group_id: u16,
}

#[derive(Clone, Copy)]
pub struct ConflictSet {
    pub group_id: u16,
    pub conflicts: &'static [u16],
    pub postfix_conflicts: &'static [u16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidatorKind {
    Any,
    AnyNonArbitrary,
    Number,
    Integer,
    Percent,
    Fraction,
    TshirtSize,
    ArbitraryValue,
    ArbitraryVariable,
    ArbitraryLength,
    ArbitraryNumber,
    ArbitrarySize,
    ArbitraryPosition,
    ArbitraryImage,
    ArbitraryShadow,
    ArbitraryVariableLength,
    ArbitraryVariableNumber,
    ArbitraryVariableSize,
    ArbitraryVariablePosition,
    ArbitraryVariableImage,
    ArbitraryVariableShadow,
    ArbitraryVariableFamilyName,
}

use super::const_utils::{
    cmp_str, ends_with_char, has_suffix, slice_from, slice_range, slice_to, starts_with_char,
    strip_prefix,
};

include!(concat!(env!("OUT_DIR"), "/generated_data.rs"));

const TSHIRT_SUFFIXES: [&str; 5] = ["xs", "sm", "md", "lg", "xl"];

pub const fn lookup_class(base: &str) -> Option<u16> {
    let mut low = 0usize;
    let mut high = CLASS_ENTRIES.len();

    while low < high {
        let mid = (low + high) / 2;
        let entry = CLASS_ENTRIES[mid];
        let cmp = cmp_str(base, entry.class);
        if cmp == 0 {
            return Some(entry.group_id);
        } else if cmp < 0 {
            high = mid;
        } else {
            low = mid + 1;
        }
    }

    let mut idx = 0usize;
    while idx < PREFIX_VALIDATORS.len() {
        let entry = PREFIX_VALIDATORS[idx];
        if entry.prefix.is_empty() {
            if validator_matches(entry.validator, base) {
                return Some(entry.group_id);
            }
        } else if let Some(remainder) = strip_prefix(base, entry.prefix) {
            let value = if entry.requires_dash {
                strip_dash(remainder)
            } else {
                Some(remainder)
            };

            if let Some(value) = value {
                if validator_matches(entry.validator, value) {
                    return Some(entry.group_id);
                }
            }
        }
        idx += 1;
    }

    idx = 0;
    while idx < BARE_VALIDATORS.len() {
        let entry = BARE_VALIDATORS[idx];
        if validator_matches(entry.validator, base) {
            return Some(entry.group_id);
        }
        idx += 1;
    }

    None
}

pub const fn class_conflicts(group_id: u16, has_postfix_modifier: bool) -> &'static [u16] {
    let mut idx = 0;
    while idx < CONFLICT_SETS.len() {
        let set = CONFLICT_SETS[idx];
        if set.group_id == group_id {
            if has_postfix_modifier && !set.postfix_conflicts.is_empty() {
                return set.postfix_conflicts;
            }
            return set.conflicts;
        }
        idx += 1;
    }
    &[]
}

const fn validator_matches(kind: ValidatorKind, value: &str) -> bool {
    match kind {
        ValidatorKind::Any => true,
        ValidatorKind::AnyNonArbitrary => {
            !starts_with_char(value, '[') && !starts_with_char(value, '(')
        }
        ValidatorKind::Number => is_number(value),
        ValidatorKind::Integer => is_integer(value),
        ValidatorKind::Percent => is_percent(value),
        ValidatorKind::Fraction => is_fraction(value),
        ValidatorKind::TshirtSize => matches_tshirt_size(value),
        ValidatorKind::ArbitraryValue
        | ValidatorKind::ArbitraryLength
        | ValidatorKind::ArbitraryNumber
        | ValidatorKind::ArbitrarySize
        | ValidatorKind::ArbitraryPosition
        | ValidatorKind::ArbitraryImage
        | ValidatorKind::ArbitraryShadow => is_arbitrary_bracket(value),
        ValidatorKind::ArbitraryVariable
        | ValidatorKind::ArbitraryVariableLength
        | ValidatorKind::ArbitraryVariableNumber
        | ValidatorKind::ArbitraryVariableSize
        | ValidatorKind::ArbitraryVariablePosition
        | ValidatorKind::ArbitraryVariableImage
        | ValidatorKind::ArbitraryVariableShadow
        | ValidatorKind::ArbitraryVariableFamilyName => is_arbitrary_paren(value),
    }
}

const fn strip_dash(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes[0] != b'-' {
        return None;
    }
    Some(slice_from(value, 1))
}

const fn is_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut idx = 0;
    if bytes[idx] == b'-' {
        idx += 1;
        if idx == bytes.len() {
            return false;
        }
    }

    let mut saw_digit = false;
    let mut saw_dot = false;
    while idx < bytes.len() {
        let b = bytes[idx];
        if b == b'.' {
            if saw_dot {
                return false;
            }
            saw_dot = true;
        } else if is_ascii_digit(b) {
            saw_digit = true;
        } else {
            return false;
        }
        idx += 1;
    }

    saw_digit
}

const fn is_integer(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut idx = 0;
    if bytes[idx] == b'-' {
        idx += 1;
        if idx == bytes.len() {
            return false;
        }
    }

    while idx < bytes.len() {
        if !is_ascii_digit(bytes[idx]) {
            return false;
        }
        idx += 1;
    }

    true
}

const fn is_percent(value: &str) -> bool {
    if !ends_with_char(value, '%') {
        return false;
    }
    let len = value.len();
    if len == 1 {
        return false;
    }
    is_number(slice_to(value, len - 1))
}

const fn is_fraction(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut idx = 0;
    let mut slash_idx: isize = -1;
    while idx < bytes.len() {
        if bytes[idx] == b'/' {
            if slash_idx != -1 {
                return false;
            }
            slash_idx = idx as isize;
        }
        idx += 1;
    }

    if slash_idx <= 0 || slash_idx as usize == bytes.len() - 1 {
        return false;
    }

    let numerator = slice_range(value, 0, slash_idx as usize);
    let denominator = slice_from(value, (slash_idx as usize) + 1);

    if numerator.is_empty() || denominator.is_empty() {
        return false;
    }

    let numerator_bytes = numerator.as_bytes();
    let mut n_idx = 0;
    if numerator_bytes[n_idx] == b'-' {
        n_idx += 1;
        if n_idx == numerator_bytes.len() {
            return false;
        }
    }
    while n_idx < numerator_bytes.len() {
        if !is_ascii_digit(numerator_bytes[n_idx]) {
            return false;
        }
        n_idx += 1;
    }

    let denominator_bytes = denominator.as_bytes();
    if denominator_bytes.is_empty() {
        return false;
    }
    let mut d_idx = 0;
    while d_idx < denominator_bytes.len() {
        if !is_ascii_digit(denominator_bytes[d_idx]) {
            return false;
        }
        d_idx += 1;
    }

    true
}

const fn matches_tshirt_size(value: &str) -> bool {
    let mut idx = 0;
    while idx < TSHIRT_SUFFIXES.len() {
        let suffix = TSHIRT_SUFFIXES[idx];
        if has_suffix(value, suffix) {
            let prefix_len = value.len() - suffix.len();
            if prefix_len == 0 {
                return true;
            }
            let prefix = slice_to(value, prefix_len);
            if is_number(prefix) {
                return true;
            }
        }
        idx += 1;
    }
    false
}

const fn is_arbitrary_bracket(value: &str) -> bool {
    value.len() >= 2 && starts_with_char(value, '[') && ends_with_char(value, ']')
}

const fn is_arbitrary_paren(value: &str) -> bool {
    value.len() >= 2 && starts_with_char(value, '(') && ends_with_char(value, ')')
}

const fn is_ascii_digit(byte: u8) -> bool {
    byte >= b'0' && byte <= b'9'
}
