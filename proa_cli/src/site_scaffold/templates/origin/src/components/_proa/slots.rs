// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Legacy dynamic slot-recipe builder.
//!
//! New multipart components declare a [`crate::components::_proa::Anatomy`] constant and expose
//! named `&'static str` slots from a typed component-local recipe. That keeps
//! DOM parts, Rust styling, metadata, and generated targets on one contract.
//!
//! These types remain available for source compatibility, but should not be
//! used for new component work.

#![allow(deprecated)]

use heapless::FnvIndexMap;

/// Maximum number of slots in a recipe (e.g., root, header, body, footer)
const MAX_SLOTS: usize = 8;
/// Maximum number of variants per slot
const MAX_SLOT_VARIANTS: usize = 8;
/// Maximum number of values per variant
const MAX_VARIANT_VALUES: usize = 8;

/// A slot recipe defines styling for multiple parts of a component (stack-allocated, O(1) lookups)
#[derive(Debug, Clone)]
#[deprecated(note = "use a typed component-local recipe plus crate::components::_proa::Anatomy")]
pub struct SlotRecipe {
    /// Each slot has its own base classes and variants (stack-allocated hash map)
    pub slots: FnvIndexMap<&'static str, SlotDefinition, MAX_SLOTS>,
}

#[derive(Debug, Clone)]
#[deprecated(note = "use named fields on a typed component-local recipe")]
pub struct SlotDefinition {
    /// Base Tailwind classes for this slot
    pub base: &'static [&'static str],
    /// Variant classes for this slot (stack-allocated hash map for O(1) lookups)
    pub variants: FnvIndexMap<&'static str, VariantMap, MAX_SLOT_VARIANTS>,
}

/// Stack-allocated map of variant values
pub type VariantMap = FnvIndexMap<&'static str, &'static [&'static str], MAX_VARIANT_VALUES>;

impl SlotRecipe {
    /// Get classes for a specific slot (O(1) lookups)
    pub fn slot_classes<const N: usize>(
        &self,
        slot_name: &str,
        props: &[(&str, &str)],
    ) -> ([&'static str; N], usize) {
        let mut classes = [""; N];
        let mut idx = 0;

        // O(1) lookup for the slot
        if let Some(slot_def) = self.slots.get(slot_name) {
            // Add base classes
            for &class in slot_def.base {
                if idx < N {
                    classes[idx] = class;
                    idx += 1;
                }
            }

            // Apply variant classes (O(1) lookups)
            for (variant_name, variant_value) in props {
                if let Some(variant_map) = slot_def.variants.get(variant_name) {
                    if let Some(variant_classes) = variant_map.get(variant_value) {
                        for &class in *variant_classes {
                            if idx < N {
                                classes[idx] = class;
                                idx += 1;
                            }
                        }
                    }
                }
            }
        }

        (classes, idx)
    }
}

/// Builder for slot recipes
#[deprecated(note = "use a typed component-local recipe plus crate::components::_proa::Anatomy")]
pub struct SlotRecipeBuilder {
    slots: FnvIndexMap<&'static str, SlotDefinition, MAX_SLOTS>,
}

impl SlotRecipeBuilder {
    pub fn new() -> Self {
        Self {
            slots: FnvIndexMap::new(),
        }
    }

    /// Define a slot with its base classes
    pub fn slot(self, name: &'static str, base: &'static [&'static str]) -> SlotBuilder {
        SlotBuilder {
            recipe_builder: self,
            slot_name: name,
            base,
            variants: FnvIndexMap::new(),
        }
    }

    pub fn build(self) -> SlotRecipe {
        SlotRecipe { slots: self.slots }
    }
}

impl Default for SlotRecipeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for individual slots
#[deprecated(note = "use named fields on a typed component-local recipe")]
pub struct SlotBuilder {
    recipe_builder: SlotRecipeBuilder,
    slot_name: &'static str,
    base: &'static [&'static str],
    variants: FnvIndexMap<&'static str, VariantMap, MAX_SLOT_VARIANTS>,
}

impl SlotBuilder {
    /// Add a variant for this slot
    pub fn variant(
        mut self,
        name: &'static str,
        values: &[(&'static str, &'static [&'static str])],
    ) -> Self {
        let mut variant_map: VariantMap = FnvIndexMap::new();
        for (key, classes) in values {
            let _ = variant_map.insert(*key, *classes);
        }
        let _ = self.variants.insert(name, variant_map);
        self
    }

    /// Finish this slot and start another
    pub fn slot(self, name: &'static str, base: &'static [&'static str]) -> SlotBuilder {
        // Save current slot
        let slot_def = SlotDefinition {
            base: self.base,
            variants: self.variants,
        };
        let mut recipe_builder = self.recipe_builder;
        let _ = recipe_builder.slots.insert(self.slot_name, slot_def);

        // Start new slot
        SlotBuilder {
            recipe_builder,
            slot_name: name,
            base,
            variants: FnvIndexMap::new(),
        }
    }

    /// Finish building
    pub fn build(self) -> SlotRecipe {
        // Save final slot
        let slot_def = SlotDefinition {
            base: self.base,
            variants: self.variants,
        };
        let mut recipe_builder = self.recipe_builder;
        let _ = recipe_builder.slots.insert(self.slot_name, slot_def);

        recipe_builder.build()
    }
}
