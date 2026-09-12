// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Type definitions for component metadata

use serde::{Deserialize, Serialize};

/// Component type - determines what HTML element to render
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    #[default]
    Button,
    Svg,
}

/// Represents a complete component definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDef {
    pub name: String,
    pub base_classes: String,
    pub variants: Vec<VariantDef>,
    #[serde(default)]
    pub component_type: ComponentType,
}

#[cfg(test)]
impl ComponentDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_classes: String::new(),
            variants: Vec::new(),
            component_type: ComponentType::default(),
        }
    }

    pub fn with_base_classes(mut self, base_classes: impl Into<String>) -> Self {
        self.base_classes = base_classes.into();
        self
    }

    pub fn with_variant(mut self, variant: VariantDef) -> Self {
        self.variants.push(variant);
        self
    }
}

/// A single variant dimension (e.g., size, color, variant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantDef {
    pub name: String,
    pub enum_name: String,
    pub default_value: String,
    pub options: Vec<VariantOption>,
    #[serde(default)]
    pub optional: bool,
}

/// An option within a variant (e.g., Sm, Md, Lg for size)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantOption {
    pub name: String,
    pub value: String,
    pub classes: String,
}

#[cfg(test)]
impl VariantDef {
    pub fn new(
        name: impl Into<String>,
        enum_name: impl Into<String>,
        default_value: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            enum_name: enum_name.into(),
            default_value: default_value.into(),
            options: Vec::new(),
            optional: false,
        }
    }

    pub fn with_option(mut self, option: VariantOption) -> Self {
        self.options.push(option);
        self
    }
}

#[cfg(test)]
impl VariantOption {
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
        classes: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            classes: classes.into(),
        }
    }
}
