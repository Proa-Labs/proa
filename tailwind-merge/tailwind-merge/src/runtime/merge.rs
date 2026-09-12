// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use super::class_map::ClassGroupUtils;
use super::parse::{ClassNameParser, IMPORTANT_MODIFIER};
use super::sort_modifiers::ModifierSorter;

pub fn merge_class_list(
    class_list: &str,
    parser: &ClassNameParser,
    sorter: &ModifierSorter,
    class_utils: &ClassGroupUtils,
) -> String {
    let class_names: Vec<&str> = class_list.split_whitespace().collect();
    if class_names.is_empty() {
        return String::new();
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut kept: Vec<&str> = Vec::with_capacity(class_names.len());

    for class_name in class_names.iter().rev().copied() {
        let parsed = parser.parse(class_name);

        if parsed.is_external {
            kept.push(class_name);
            continue;
        }

        let mut has_postfix_modifier = parsed.maybe_postfix_modifier_position.is_some();
        let mut candidate_base: &str = if let Some(pos) = parsed.maybe_postfix_modifier_position {
            &parsed.base_class_name[..pos]
        } else {
            parsed.base_class_name.as_str()
        };

        let mut class_group_id = class_utils.get_class_group_id(candidate_base);

        if class_group_id.is_none() && has_postfix_modifier {
            has_postfix_modifier = false;
            candidate_base = parsed.base_class_name.as_str();
            class_group_id = class_utils.get_class_group_id(candidate_base);
        }

        let class_group_id = match class_group_id {
            Some(id) => id,
            None => {
                kept.push(class_name);
                continue;
            }
        };

        let sorted_modifiers = sorter.sort(&parsed.modifiers);
        let variant_modifier = if sorted_modifiers.is_empty() {
            String::new()
        } else {
            sorted_modifiers.join(":")
        };
        let mut modifier_id = variant_modifier.clone();
        if parsed.has_important_modifier {
            modifier_id.push_str(IMPORTANT_MODIFIER);
        }

        let class_id = format!("{modifier_id}{class_group_id}");
        if !seen.insert(class_id) {
            continue;
        }

        for conflict in
            class_utils.get_conflicting_class_group_ids(&class_group_id, has_postfix_modifier)
        {
            let conflict_id = format!("{modifier_id}{conflict}");
            seen.insert(conflict_id);
        }

        kept.push(class_name);
    }

    kept.reverse();
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    use super::super::class_map::ClassGroupUtils;
    use super::super::config::Config;
    use super::super::parse::ClassNameParser;
    use super::super::sort_modifiers::ModifierSorter;
    use super::merge_class_list;

    #[test]
    fn merges_conflicting_classes() {
        let config = Config::clone_default();
        let parser = ClassNameParser::new(config.prefix.as_deref());
        let sorter = ModifierSorter::new(&config.order_sensitive_modifiers);
        let utils = ClassGroupUtils::new(&config);

        let merged = merge_class_list("text-sm text-lg font-bold", &parser, &sorter, &utils);
        assert_eq!(merged, "text-lg font-bold");
    }
}
