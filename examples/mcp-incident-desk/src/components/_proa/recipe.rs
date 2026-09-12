// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Legacy dynamic recipe builder.
//!
//! New components use a typed, component-local `*Recipe` plus
//! [`crate::components::_proa::variant_spec::ComponentSpec`]. That path is const-constructible,
//! streams classes directly, exports anatomy/variant metadata, and preserves
//! compound rules across generated render targets.
//!
//! These types remain available for source compatibility, but should not be
//! used for new component work.

#![allow(deprecated)]

use heapless::FnvIndexMap;

/// Recipe variant enums must implement a `classes()` method.
///
/// All recipe variant enums (size, color, variant, radius, etc.) should implement
/// an inherent `classes()` method to provide their corresponding Tailwind class strings.
///
/// **Note**: We use inherent methods instead of a trait because trait methods cannot be
/// `const fn` in stable Rust, and we need const evaluation for compile-time class generation.
///
/// # Required Method Signature
///
/// ```rust,ignore
/// impl YourEnum {
///     pub const fn classes(self) -> &'static str {
///         match self {
///             YourEnum::VariantA => CONST_A,
///             YourEnum::VariantB => CONST_B,
///             // ...
///         }
///     }
/// }
/// ```
///
/// # Requirements
///
/// - Must be `pub const fn` for compile-time evaluation
/// - Must return `&'static str` containing space-separated Tailwind classes
/// - Should be exhaustive (match all enum variants)
/// - Can return an empty string `""` if no classes apply
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// pub enum ButtonSize {
///     Xs,
///     Sm,
///     #[default]
///     Md,
///     Lg,
///     Xl,
/// }
///
/// impl ButtonSize {
///     pub const fn classes(self) -> &'static str {
///         match self {
///             ButtonSize::Xs => BUTTON_SIZE_XS,
///             ButtonSize::Sm => BUTTON_SIZE_SM,
///             ButtonSize::Md => BUTTON_SIZE_MD,
///             ButtonSize::Lg => BUTTON_SIZE_LG,
///             ButtonSize::Xl => BUTTON_SIZE_XL,
///         }
///     }
/// }
/// ```
/// Maximum number of variants in a recipe (e.g., size, color, variant)
const MAX_VARIANTS: usize = 8;
/// Maximum number of values per variant (e.g., sm, md, lg, xl)
const MAX_VARIANT_VALUES: usize = 8;
/// Maximum number of compound variants
const MAX_COMPOUND_VARIANTS: usize = 8;
/// Maximum number of conditions in a compound variant
const MAX_COMPOUND_CONDITIONS: usize = 4;

/// A recipe defines variants that output Tailwind classes (stack-allocated, O(1) lookups)
#[derive(Debug, Clone)]
#[deprecated(note = "use a typed component-local recipe implementing variant_spec::ComponentSpec")]
pub struct Recipe {
    /// Base Tailwind classes applied to all variants
    pub base: &'static [&'static str],
    /// Variant definitions (stack-allocated hash map for O(1) lookups)
    pub variants: FnvIndexMap<&'static str, VariantMap, MAX_VARIANTS>,
    /// Compound variants (multiple conditions -> tailwind classes)
    pub compound_variants: heapless::Vec<CompoundVariant, MAX_COMPOUND_VARIANTS>,
    /// Default variant values (stack-allocated hash map)
    pub defaults: FnvIndexMap<&'static str, &'static str, MAX_VARIANTS>,
}

/// Stack-allocated map of variant values
pub type VariantMap = FnvIndexMap<&'static str, &'static [&'static str], MAX_VARIANT_VALUES>;

#[derive(Debug, Clone)]
#[deprecated(note = "declare compound rules through ComponentSpec::compound_variant_metadata")]
pub struct CompoundVariant {
    pub conditions: FnvIndexMap<&'static str, &'static str, MAX_COMPOUND_CONDITIONS>,
    pub classes: &'static [&'static str],
}

impl Recipe {
    /// Build classes for the given variant props (returns fixed array + count)
    /// **O(1) lookups** - uses stack-allocated hash maps
    pub fn classes<const N: usize>(&self, props: &[(&str, &str)]) -> ([&'static str; N], usize) {
        let mut classes = [""; N];
        let mut idx = 0;

        // Add base classes
        for &class in self.base {
            if idx < N {
                classes[idx] = class;
                idx += 1;
            }
        }

        // Build a map of selected variants (stack-allocated)
        let mut selected: FnvIndexMap<&str, &str, MAX_VARIANTS> = FnvIndexMap::new();
        for (key, value) in props {
            let _ = selected.insert(*key, *value);
        }

        // Apply defaults for unspecified variants
        for (key, default) in &self.defaults {
            if !selected.contains_key(key) {
                let _ = selected.insert(key, default);
            }
        }

        // Apply variant classes (O(1) lookup per variant)
        for (variant_name, variant_value) in &selected {
            if let Some(variant_map) = self.variants.get(variant_name) {
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

        // Apply compound variants (O(1) condition checks)
        for compound in &self.compound_variants {
            let mut matches = true;
            for (key, value) in &compound.conditions {
                if selected.get(key) != Some(value) {
                    matches = false;
                    break;
                }
            }
            if matches {
                for &class in compound.classes {
                    if idx < N {
                        classes[idx] = class;
                        idx += 1;
                    }
                }
            }
        }

        (classes, idx)
    }
}

/// Builder for creating recipes
#[deprecated(note = "use a typed component-local recipe implementing variant_spec::ComponentSpec")]
pub struct RecipeBuilder {
    base: &'static [&'static str],
    variants: FnvIndexMap<&'static str, VariantMap, MAX_VARIANTS>,
    compound_variants: heapless::Vec<CompoundVariant, MAX_COMPOUND_VARIANTS>,
    defaults: FnvIndexMap<&'static str, &'static str, MAX_VARIANTS>,
}

impl RecipeBuilder {
    pub fn new() -> Self {
        Self {
            base: &[],
            variants: FnvIndexMap::new(),
            compound_variants: heapless::Vec::new(),
            defaults: FnvIndexMap::new(),
        }
    }

    /// Set base Tailwind classes
    pub fn base(mut self, classes: &'static [&'static str]) -> Self {
        self.base = classes;
        self
    }

    /// Add a variant with its Tailwind class mappings
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

    /// Add a compound variant (when multiple conditions match)
    pub fn compound_variant(
        mut self,
        conditions: &[(&'static str, &'static str)],
        classes: &'static [&'static str],
    ) -> Self {
        let mut condition_map: FnvIndexMap<&'static str, &'static str, MAX_COMPOUND_CONDITIONS> =
            FnvIndexMap::new();
        for (key, value) in conditions {
            let _ = condition_map.insert(*key, *value);
        }
        let _ = self.compound_variants.push(CompoundVariant {
            conditions: condition_map,
            classes,
        });
        self
    }

    /// Set default value for a variant
    pub fn default(mut self, variant: &'static str, value: &'static str) -> Self {
        let _ = self.defaults.insert(variant, value);
        self
    }

    pub fn build(self) -> Recipe {
        Recipe {
            base: self.base,
            variants: self.variants,
            compound_variants: self.compound_variants,
            defaults: self.defaults,
        }
    }
}

impl Default for RecipeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
