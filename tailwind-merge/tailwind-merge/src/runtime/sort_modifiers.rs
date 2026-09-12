// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone)]
pub struct ModifierSorter {
    order_sensitive_modifiers: Vec<String>,
}

impl ModifierSorter {
    pub fn new(order_sensitive_modifiers: &[String]) -> Self {
        let mut list = order_sensitive_modifiers.to_vec();
        list.sort();
        list.dedup();
        Self {
            order_sensitive_modifiers: list,
        }
    }

    pub fn sort(&self, modifiers: &[String]) -> Vec<String> {
        if modifiers.len() <= 1 {
            return modifiers.to_vec();
        }

        let mut result: Vec<String> = Vec::with_capacity(modifiers.len());
        let mut pending: Vec<String> = Vec::new();

        for modifier in modifiers {
            let is_position_sensitive =
                modifier.starts_with('[') || self.is_order_sensitive(modifier);

            if is_position_sensitive {
                pending.sort();
                result.append(&mut pending);
                result.push(modifier.clone());
            } else {
                pending.push(modifier.clone());
            }
        }

        pending.sort();
        result.append(&mut pending);
        result
    }

    fn is_order_sensitive(&self, modifier: &str) -> bool {
        self.order_sensitive_modifiers
            .binary_search_by(|probe| probe.as_str().cmp(modifier))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::ModifierSorter;

    #[test]
    fn sorts_modifiers_preserving_order_sensitive_segments() {
        let sorter = ModifierSorter::new(&["hover".into(), "focus".into()]);
        let output = sorter.sort(&["md".into(), "hover".into(), "sm".into()]);
        assert_eq!(output, vec!["md", "hover", "sm"]);
    }
}
