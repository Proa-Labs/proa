// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub const IMPORTANT_MODIFIER: &str = "!";

#[derive(Debug, Clone)]
pub struct ParsedClassName {
    pub is_external: bool,
    pub modifiers: Vec<String>,
    pub has_important_modifier: bool,
    pub base_class_name: String,
    pub maybe_postfix_modifier_position: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct ClassNameParser {
    prefix_with_separator: Option<String>,
}

impl ClassNameParser {
    pub fn new(prefix: Option<&str>) -> Self {
        Self {
            prefix_with_separator: prefix.map(|value| format!("{value}:")),
        }
    }

    pub fn parse(&self, class_name: &str) -> ParsedClassName {
        if let Some(prefix) = &self.prefix_with_separator {
            if let Some(inner) = class_name.strip_prefix(prefix) {
                return self.parse_inner(inner);
            }
            return ParsedClassName {
                is_external: true,
                modifiers: Vec::new(),
                has_important_modifier: false,
                base_class_name: class_name.to_string(),
                maybe_postfix_modifier_position: None,
            };
        }

        self.parse_inner(class_name)
    }

    fn parse_inner(&self, class_name: &str) -> ParsedClassName {
        let mut modifiers = Vec::new();
        let mut bracket_depth = 0;
        let mut paren_depth = 0;
        let mut modifier_start = 0usize;
        let mut postfix_modifier_position: Option<usize> = None;

        for (idx, ch) in class_name.char_indices() {
            if bracket_depth == 0 && paren_depth == 0 {
                if ch == ':' {
                    modifiers.push(class_name[modifier_start..idx].to_string());
                    modifier_start = idx + 1;
                    continue;
                }
                if ch == '/' {
                    postfix_modifier_position = Some(idx);
                    continue;
                }
            }

            match ch {
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
        }

        let base_with_important = if modifiers.is_empty() {
            class_name.to_string()
        } else {
            class_name[modifier_start..].to_string()
        };

        let base_class_name = strip_important_modifier(&base_with_important);
        let has_important_modifier = base_class_name != base_with_important;

        let maybe_postfix_modifier_position = postfix_modifier_position
            .filter(|&pos| pos > modifier_start)
            .map(|pos| pos - modifier_start);

        ParsedClassName {
            is_external: false,
            modifiers,
            has_important_modifier,
            base_class_name,
            maybe_postfix_modifier_position,
        }
    }
}

fn strip_important_modifier(base_class_name: &str) -> String {
    if let Some(stripped) = base_class_name.strip_suffix(IMPORTANT_MODIFIER) {
        stripped.to_string()
    } else if let Some(stripped) = base_class_name.strip_prefix(IMPORTANT_MODIFIER) {
        stripped.to_string()
    } else {
        base_class_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_and_base() {
        let parser = ClassNameParser::default();
        let parsed = parser.parse("hover:focus:bg-red-500");
        assert_eq!(parsed.modifiers, vec!["hover", "focus"]);
        assert_eq!(parsed.base_class_name, "bg-red-500");
        assert!(!parsed.is_external);
    }

    #[test]
    fn strips_important_suffix() {
        let parser = ClassNameParser::default();
        let parsed = parser.parse("hover:!bg-red-500");
        assert!(parsed.has_important_modifier);
        assert_eq!(parsed.base_class_name, "bg-red-500");
    }
}
