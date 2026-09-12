// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use super::config::{ClassDefinition, Config};
use super::validators::ValidatorFn;

#[derive(Clone)]
pub struct ClassGroupUtils {
    class_map: ClassPartNode,
    conflicting_class_groups: HashMap<String, Vec<String>>,
    conflicting_class_group_modifiers: HashMap<String, Vec<String>>,
}

#[derive(Default, Clone)]
struct ClassPartNode {
    next_part: HashMap<String, ClassPartNode>,
    validators: Vec<ValidatorEntry>,
    class_group_id: Option<String>,
}

#[derive(Clone)]
struct ValidatorEntry {
    class_group_id: String,
    validator: ValidatorFn,
}

impl ClassGroupUtils {
    pub fn new(config: &Config) -> Self {
        let mut class_map = ClassPartNode::default();
        for (class_group_id, class_group) in &config.class_groups {
            process_classes_recursively(class_group, &mut class_map, class_group_id, &config.theme);
        }

        Self {
            class_map,
            conflicting_class_groups: config.conflicting_class_groups.clone(),
            conflicting_class_group_modifiers: config.conflicting_class_group_modifiers.clone(),
        }
    }

    pub fn get_class_group_id(&self, class_name: &str) -> Option<String> {
        let mut parts: Vec<&str> = class_name.split('-').collect();
        if matches!(parts.first(), Some(&"")) && parts.len() > 1 {
            parts.remove(0);
        }

        get_group_recursive(&parts, &self.class_map)
            .map(|id| id.to_string())
            .or_else(|| get_group_id_for_arbitrary_property(class_name))
    }

    pub fn get_conflicting_class_group_ids(
        &self,
        class_group_id: &str,
        has_postfix_modifier: bool,
    ) -> Vec<String> {
        let mut conflicts = self
            .conflicting_class_groups
            .get(class_group_id)
            .cloned()
            .unwrap_or_default();

        if has_postfix_modifier {
            if let Some(extra) = self
                .conflicting_class_group_modifiers
                .get(class_group_id)
                .cloned()
            {
                conflicts.extend(extra);
            }
        }

        conflicts
    }
}

fn get_group_recursive<'a>(parts: &[&str], node: &'a ClassPartNode) -> Option<&'a String> {
    if parts.is_empty() {
        return node.class_group_id.as_ref();
    }

    if let Some(next) = node.next_part.get(parts[0]) {
        if let Some(group_id) = get_group_recursive(&parts[1..], next) {
            return Some(group_id);
        }
    }

    if node.validators.is_empty() {
        return None;
    }

    let remainder = parts.join("-");
    node.validators
        .iter()
        .find(|entry| (entry.validator)(&remainder))
        .map(|entry| &entry.class_group_id)
}

fn get_group_id_for_arbitrary_property(class_name: &str) -> Option<String> {
    if !class_name.starts_with('[') || !class_name.ends_with(']') {
        return None;
    }

    let inner = &class_name[1..class_name.len() - 1];
    let property_end = inner.find(':')?;
    if property_end == 0 {
        return None;
    }
    let property = &inner[..property_end];
    Some(format!("arbitrary..{property}"))
}

fn process_classes_recursively(
    class_group: &[ClassDefinition],
    node: &mut ClassPartNode,
    class_group_id: &str,
    theme: &HashMap<String, Vec<ClassDefinition>>,
) {
    for definition in class_group {
        match definition {
            ClassDefinition::Literal(literal) => {
                if literal.is_empty() {
                    node.class_group_id = Some(class_group_id.to_string());
                } else {
                    let target = get_part(node, literal);
                    target.class_group_id = Some(class_group_id.to_string());
                }
            }
            ClassDefinition::Validator(validator) => {
                node.validators.push(ValidatorEntry {
                    class_group_id: class_group_id.to_string(),
                    validator: *validator,
                });
            }
            ClassDefinition::Theme(key) => {
                if let Some(theme_group) = theme.get(key) {
                    process_classes_recursively(theme_group, node, class_group_id, theme);
                }
            }
            ClassDefinition::Object(entries) => {
                for entry in entries {
                    let target = get_part(node, &entry.key);
                    process_classes_recursively(&entry.definitions, target, class_group_id, theme);
                }
            }
        }
    }
}

fn get_part<'a>(node: &'a mut ClassPartNode, path: &str) -> &'a mut ClassPartNode {
    let mut current = node;
    for part in path.split('-') {
        current = current.next_part.entry(part.to_string()).or_default();
    }
    current
}

#[cfg(test)]
mod tests {
    use super::Config;
    use super::*;

    #[test]
    fn finds_group_for_simple_class() {
        let config = Config::clone_default();
        let utils = ClassGroupUtils::new(&config);
        assert_eq!(
            utils.get_class_group_id("bg-red-500").as_deref(),
            Some("bg-color")
        );
    }
}
