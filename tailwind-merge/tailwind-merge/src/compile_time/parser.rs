// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::const_utils::{
    ends_with_char, slice_from, slice_prefix, slice_prefix_mut, slice_range, slice_to,
    starts_with_char,
};

#[derive(Clone, Copy)]
pub struct ParsedClass<const N: usize> {
    modifiers: [&'static str; N],
    modifier_count: usize,
    pub base: &'static str,
    pub has_important: bool,
    pub has_postfix: bool,
    pub is_external: bool,
}

impl<const N: usize> ParsedClass<N> {
    pub const fn parse(class_name: &'static str) -> Self {
        let bytes = class_name.as_bytes();
        let mut modifiers = [""; N];
        let mut modifier_count = 0usize;

        let mut bracket_depth = 0isize;
        let mut paren_depth = 0isize;
        let mut modifier_start = 0usize;
        let mut postfix_offset: Option<usize> = None;

        let mut idx = 0usize;
        while idx < bytes.len() {
            let ch = bytes[idx] as char;
            if bracket_depth == 0 && paren_depth == 0 {
                if ch == ':' {
                    if idx > modifier_start && modifier_count < N {
                        modifiers[modifier_count] = slice_range(class_name, modifier_start, idx);
                        modifier_count += 1;
                    }
                    modifier_start = idx + 1;
                } else if ch == '/' {
                    postfix_offset = Some(idx);
                }
            }

            match ch {
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
            idx += 1;
        }

        let base_slice = if modifier_start <= class_name.len() {
            slice_from(class_name, modifier_start)
        } else {
            ""
        };

        let (base_without_important, has_important) = strip_important(base_slice);

        let (base_trimmed, has_postfix) = match postfix_offset {
            Some(offset)
                if offset > modifier_start
                    && offset <= modifier_start + base_without_important.len() =>
            {
                let relative = offset - modifier_start;
                (slice_range(base_without_important, 0, relative), true)
            }
            _ => (base_without_important, false),
        };

        let is_external =
            modifier_count == 0 && base_trimmed.is_empty() && base_without_important.is_empty();

        Self {
            modifiers,
            modifier_count,
            base: base_trimmed,
            has_important,
            has_postfix,
            is_external,
        }
    }

    pub const fn modifiers(&self) -> &[&'static str] {
        slice_prefix(&self.modifiers, self.modifier_count)
    }

    pub const fn modifiers_mut(&mut self) -> &mut [&'static str] {
        slice_prefix_mut(&mut self.modifiers, self.modifier_count)
    }
}

const fn strip_important(base_with_important: &'static str) -> (&'static str, bool) {
    if base_with_important.is_empty() {
        return ("", false);
    }

    if ends_with_char(base_with_important, '!') {
        let trimmed = slice_to(base_with_important, base_with_important.len() - 1);
        return (trimmed, true);
    }

    if starts_with_char(base_with_important, '!') {
        let trimmed = slice_from(base_with_important, 1);
        return (trimmed, true);
    }

    (base_with_important, false)
}

#[cfg(test)]
mod tests {
    use super::ParsedClass;

    #[test]
    fn parses_modifier_and_base() {
        let parsed = ParsedClass::<4>::parse("hover:focus:bg-red-500");
        assert_eq!(parsed.modifiers(), &["hover", "focus"]);
        assert_eq!(parsed.base, "bg-red-500");
        assert!(!parsed.is_external);
    }

    #[test]
    fn strips_important_suffix() {
        let parsed = ParsedClass::<4>::parse("hover:!bg-red-500");
        assert!(parsed.has_important);
        assert_eq!(parsed.base, "bg-red-500");
    }

    #[test]
    fn detects_postfix() {
        let parsed = ParsedClass::<4>::parse("bg-red-500/50");
        assert!(parsed.has_postfix);
        assert_eq!(parsed.base, "bg-red-500");
    }
}
