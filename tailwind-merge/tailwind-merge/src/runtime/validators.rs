// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use once_cell::sync::Lazy;
use regex::{Regex, RegexBuilder};

pub type ValidatorFn = fn(&str) -> bool;

macro_rules! validator_map {
    ($( $name:ident ),+ $(,)?) => {
        pub fn validator_by_name(name: &str) -> Option<ValidatorFn> {
            match name {
                $( stringify!($name) => Some($name), )+
                _ => None,
            }
        }
    };
}

static ARBITRARY_VALUE_REGEX: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^\[(?:(\w[\w-]*):)?(.+)\]$")
        .case_insensitive(true)
        .build()
        .expect("valid arbitrary value regex")
});

static ARBITRARY_VARIABLE_REGEX: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^\((?:(\w[\w-]*):)?(.+)\)$")
        .case_insensitive(true)
        .build()
        .expect("valid arbitrary variable regex")
});

static FRACTION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d+/\d+$").expect("valid fraction regex"));

static TSHIRT_UNIT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+(\.\d+)?)?(xs|sm|md|lg|xl)$").expect("valid tshirt size regex"));

static LENGTH_UNIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\d+(%|px|r?em|[sdl]?v([hwib]|min|max)|pt|pc|in|cm|mm|cap|ch|ex|r?lh|cq(w|h|i|b|min|max))|\b(calc|min|max|clamp)\(.+\)|^0$",
    )
    .expect("valid length unit regex")
});

static COLOR_FUNCTION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(rgba?|hsla?|hwb|(ok)?(lab|lch)|color-mix)\(.+\)$")
        .expect("valid color function regex")
});

static SHADOW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(inset_)?-?((\d+)?\.?(\d+)[a-z]+|0)_-?((\d+)?\.?(\d+)[a-z]+|0)")
        .expect("valid shadow regex")
});

static IMAGE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(url|image|image-set|cross-fade|element|(repeating-)?(linear|radial|conic)-gradient)\(.+\)$",
    )
    .expect("valid image regex")
});

pub fn is_fraction(value: &str) -> bool {
    FRACTION_REGEX.is_match(value)
}

pub fn is_number(value: &str) -> bool {
    !value.is_empty() && value.parse::<f64>().map(|_| true).unwrap_or(false)
}

pub fn is_integer(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }

    match value.parse::<f64>() {
        Ok(number) => number.fract() == 0.0,
        Err(_) => false,
    }
}

pub fn is_percent(value: &str) -> bool {
    value.strip_suffix('%').map(is_number).unwrap_or(false)
}

pub fn is_tshirt_size(value: &str) -> bool {
    TSHIRT_UNIT_REGEX.is_match(value)
}

pub fn is_any(_: &str) -> bool {
    true
}

pub fn is_any_non_arbitrary(value: &str) -> bool {
    !is_arbitrary_value(value) && !is_arbitrary_variable(value)
}

pub fn is_arbitrary_size(value: &str) -> bool {
    get_is_arbitrary_value(value, is_label_size, is_never)
}

pub fn is_arbitrary_value(value: &str) -> bool {
    ARBITRARY_VALUE_REGEX.is_match(value)
}

pub fn is_arbitrary_length(value: &str) -> bool {
    get_is_arbitrary_value(value, is_label_length, is_length_only)
}

pub fn is_arbitrary_number(value: &str) -> bool {
    get_is_arbitrary_value(value, is_label_number, is_number)
}

pub fn is_arbitrary_position(value: &str) -> bool {
    get_is_arbitrary_value(value, is_label_position, is_never)
}

pub fn is_arbitrary_image(value: &str) -> bool {
    get_is_arbitrary_value(value, is_label_image, is_image)
}

pub fn is_arbitrary_shadow(value: &str) -> bool {
    if let Some(captures) = ARBITRARY_VALUE_REGEX.captures(value) {
        if let Some(label_match) = captures.get(1) {
            if !is_label_shadow(label_match.as_str()) {
                return false;
            }
        }

        if let Some(value_match) = captures.get(2) {
            return is_shadow(value_match.as_str());
        }
    }
    false
}

pub fn is_arbitrary_variable(value: &str) -> bool {
    ARBITRARY_VARIABLE_REGEX.is_match(value)
}

pub fn is_arbitrary_variable_length(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_length, false)
}

pub fn is_arbitrary_variable_family_name(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_family_name, false)
}

pub fn is_arbitrary_variable_position(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_position, false)
}

pub fn is_arbitrary_variable_size(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_size, false)
}

pub fn is_arbitrary_variable_image(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_image, false)
}

pub fn is_arbitrary_variable_shadow(value: &str) -> bool {
    get_is_arbitrary_variable(value, is_label_shadow, true)
}

fn is_length_only(value: &str) -> bool {
    LENGTH_UNIT_REGEX.is_match(value) && !COLOR_FUNCTION_REGEX.is_match(value)
}

fn is_never(_: &str) -> bool {
    false
}

fn is_shadow(value: &str) -> bool {
    SHADOW_REGEX.is_match(value)
}

fn is_image(value: &str) -> bool {
    IMAGE_REGEX.is_match(value)
}

fn get_is_arbitrary_value(
    value: &str,
    test_label: fn(&str) -> bool,
    test_value: fn(&str) -> bool,
) -> bool {
    if let Some(captures) = ARBITRARY_VALUE_REGEX.captures(value) {
        if let Some(label_match) = captures.get(1) {
            return test_label(label_match.as_str());
        }
        if let Some(value_match) = captures.get(2) {
            return test_value(value_match.as_str());
        }
    }
    false
}

fn get_is_arbitrary_variable(
    value: &str,
    test_label: fn(&str) -> bool,
    should_match_no_label: bool,
) -> bool {
    if let Some(captures) = ARBITRARY_VARIABLE_REGEX.captures(value) {
        if let Some(label) = captures.get(1) {
            return test_label(label.as_str());
        }
        return should_match_no_label;
    }
    false
}

fn is_label_position(label: &str) -> bool {
    label.eq_ignore_ascii_case("position") || label.eq_ignore_ascii_case("percentage")
}

fn is_label_image(label: &str) -> bool {
    label.eq_ignore_ascii_case("image") || label.eq_ignore_ascii_case("url")
}

fn is_label_size(label: &str) -> bool {
    label.eq_ignore_ascii_case("length")
        || label.eq_ignore_ascii_case("size")
        || label.eq_ignore_ascii_case("bg-size")
}

fn is_label_length(label: &str) -> bool {
    label.eq_ignore_ascii_case("length")
}

fn is_label_number(label: &str) -> bool {
    label.eq_ignore_ascii_case("number")
}

fn is_label_family_name(label: &str) -> bool {
    label.eq_ignore_ascii_case("family-name")
}

fn is_label_shadow(label: &str) -> bool {
    label.eq_ignore_ascii_case("shadow")
}

validator_map!(
    is_any,
    is_any_non_arbitrary,
    is_arbitrary_image,
    is_arbitrary_length,
    is_arbitrary_number,
    is_arbitrary_position,
    is_arbitrary_shadow,
    is_arbitrary_size,
    is_arbitrary_value,
    is_arbitrary_variable,
    is_arbitrary_variable_family_name,
    is_arbitrary_variable_image,
    is_arbitrary_variable_length,
    is_arbitrary_variable_position,
    is_arbitrary_variable_shadow,
    is_arbitrary_variable_size,
    is_fraction,
    is_integer,
    is_number,
    is_percent,
    is_tshirt_size,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_requires_number() {
        assert!(is_percent("10%"));
        assert!(!is_percent("%"));
    }

    #[test]
    fn arbitrary_shadow_requires_shadow_values() {
        assert!(is_arbitrary_shadow("[shadow:0_0_2px_black]"));
        assert!(!is_arbitrary_shadow("[shadow:foo]"));
    }
}
