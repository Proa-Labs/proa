// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Single source of truth for component variants
//!
//! This module defines traits that allow variant enums to describe themselves,
//! eliminating duplication between Rust code and TypeScript metadata.

use crate::components::_proa::Anatomy;

/// Metadata for a single variant option (e.g., "Sm", "Md", "Lg")
#[derive(Debug, Clone, Copy)]
pub struct VariantOption<T> {
    /// The enum variant value
    pub variant: T,
    /// Display label (e.g., "Sm")
    pub label: &'static str,
    /// TypeScript enum value (e.g., "sm")
    pub value: &'static str,
    /// Tailwind CSS classes
    pub classes: &'static str,
}

/// Trait for variant enums that can describe themselves
///
/// This trait is automatically implemented by `#[derive(ProaVariant)]`
pub trait VariantSpec: Copy + Default + 'static {
    /// Name of the variant dimension (e.g., "size", "color")
    const NAME: &'static str;

    /// TypeScript enum type name (e.g., "ButtonSize")
    const TS_TYPE: &'static str;

    /// All available options for this variant
    const OPTIONS: &'static [VariantOption<Self>];

    /// Get the Tailwind classes for this variant value
    fn classes(self) -> &'static str;

    /// Get the metadata option for this variant value
    fn metadata_option(self) -> &'static VariantOption<Self> {
        Self::OPTIONS
            .iter()
            .find(|opt| {
                // Compare by classes since we can't derive PartialEq on the trait
                opt.classes == self.classes()
            })
            .expect("variant not found in OPTIONS")
    }
}

/// Metadata about a variant dimension
#[derive(Debug, Clone)]

pub struct VariantMetadata {
    pub name: &'static str,

    pub ts_type: &'static str,

    pub default_label: &'static str,
    pub default_value: &'static str,
    pub options: Vec<VariantOptionMetadata>,
    /// Whether this variant is optional (recipe default is None)
    pub optional: bool,
}

/// Metadata about a single option within a variant
#[derive(Debug, Clone)]

pub struct VariantOptionMetadata {
    pub label: &'static str,
    pub value: &'static str,
    pub classes: &'static str,
}

/// One axis/value condition in a compound variant.
///
/// Conditions are represented as an ordered list instead of a map so metadata
/// output is deterministic and downstream generators do not need to sort keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

pub struct CompoundVariantConditionMetadata {
    /// Variant axis name (for example, `"color"`).
    pub name: &'static str,
    /// Selected option value (for example, `"primary"`).
    pub value: &'static str,
}

impl CompoundVariantConditionMetadata {
    pub const fn new(name: &'static str, value: &'static str) -> Self {
        Self { name, value }
    }
}

/// Classes selected only when every condition matches.
///
/// This models recipes whose appearance cannot be represented as the sum of
/// independent axis classes, such as Button's `color x variant` matrix.
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct CompoundVariantMetadata {
    pub conditions: Vec<CompoundVariantConditionMetadata>,
    pub classes: &'static str,
}

/// Stable DOM scope and parts for a component.
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct AnatomyMetadata {
    pub scope: &'static str,
    pub parts: Vec<&'static str>,
}

impl From<Anatomy> for AnatomyMetadata {
    fn from(anatomy: Anatomy) -> Self {
        Self {
            scope: anatomy.scope(),
            parts: anatomy.parts().to_vec(),
        }
    }
}

impl CompoundVariantMetadata {
    pub fn new(conditions: Vec<CompoundVariantConditionMetadata>, classes: &'static str) -> Self {
        Self {
            conditions,
            classes,
        }
    }
}

impl<T: VariantSpec> From<&VariantOption<T>> for VariantOptionMetadata {
    fn from(opt: &VariantOption<T>) -> Self {
        Self {
            label: opt.label,
            value: opt.value,
            classes: opt.classes,
        }
    }
}

/// Extract metadata from a VariantSpec implementor
pub fn variant_metadata<T: VariantSpec>() -> VariantMetadata {
    let default = T::default();
    let default_opt = default.metadata_option();

    VariantMetadata {
        name: T::NAME,
        ts_type: T::TS_TYPE,
        default_label: default_opt.label,
        default_value: default_opt.value,
        options: T::OPTIONS.iter().map(VariantOptionMetadata::from).collect(),
        optional: false, // Default to false, will be overridden if recipe default is None
    }
}

/// Extract option/default metadata for an axis whose classes are compound-only.
///
/// Values and TypeScript types remain available to generators, but each
/// independent option has an empty class string. The actual appearance is
/// supplied by [`ComponentSpec::compound_variant_metadata`].
pub fn compound_axis_metadata<T: VariantSpec>() -> VariantMetadata {
    let mut metadata = variant_metadata::<T>();
    for option in &mut metadata.options {
        option.classes = "";
    }
    metadata
}

/// Trait for component recipes that can describe themselves
pub trait ComponentSpec {
    /// Component name (e.g., "Button")
    const NAME: &'static str;

    /// Get the base Tailwind classes for this recipe
    fn base_classes(&self) -> &'static str;

    /// Get metadata for all variants
    fn variant_metadata(&self) -> Vec<VariantMetadata>;

    /// Get metadata for class rules that depend on multiple variant axes.
    ///
    /// Most recipes are additive and need no compound rules, so the default
    /// keeps existing implementations source-compatible.
    fn compound_variant_metadata(&self) -> Vec<CompoundVariantMetadata> {
        Vec::new()
    }

    /// Stable DOM anatomy shared by every render target.
    fn anatomy(&self) -> Option<Anatomy> {
        None
    }
}

/// Component type - determines what HTML element to render
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]

pub enum ComponentType {
    #[default]
    Button,
    Svg,
}

/// Component metadata for TypeScript generation
#[derive(Debug, Clone)]

pub struct ComponentMetadata {
    pub name: &'static str,
    pub base_classes: &'static str,
    pub variants: Vec<VariantMetadata>,
    /// Stable scope/part contract for generated render targets.
    pub anatomy: Option<AnatomyMetadata>,
    /// Class rules selected by combinations of two or more variant axes.
    pub compound_variants: Vec<CompoundVariantMetadata>,

    pub component_type: ComponentType,
    /// Component props (non-variant fields like disabled, id, etc.)
    pub props: Vec<PropMetadata>,
    /// HTML element to render (e.g., "button", "div", "input")
    pub element: Option<&'static str>,
    /// SVG inner content for icon components (the path/circle elements)
    pub svg_content: Option<&'static str>,
    /// Whether this component renders its children
    pub has_children: bool,
    /// Other components this component renders internally
    pub dependencies: Vec<ComponentDependency>,
}

/// A component dependency - another component rendered inside this one
#[derive(Debug, Clone)]

pub struct ComponentDependency {
    /// Name of the component (e.g., "Spinner")
    pub component: &'static str,
    /// Condition prop that controls rendering (e.g., "loading")
    pub condition: &'static str,
    /// Props to pass to the component as key-value pairs
    pub props: Vec<(&'static str, &'static str)>,
    /// Where to render relative to children
    pub position: RenderPosition,
}

/// Where to render a dependency relative to children
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]

pub enum RenderPosition {
    #[default]
    BeforeChildren,
    AfterChildren,
}

/// Metadata for a component prop
#[derive(Debug, Clone)]

pub struct PropMetadata {
    /// Prop name (e.g., "disabled", "id")
    pub name: &'static str,
    /// TypeScript type (e.g., "boolean", "string", "ButtonType")
    pub ts_type: &'static str,
    /// Whether the prop is optional
    pub optional: bool,
    /// Default value as TypeScript expression (e.g., "false", "'button'")
    pub default: Option<&'static str>,
    /// HTML attribute name if different from prop name (e.g., "class" -> "className" in React)
    pub html_attr: Option<&'static str>,
    /// Whether this is the children prop
    pub is_children: bool,
    /// Whether this prop maps directly to an HTML attribute on the element
    pub is_html_attr: bool,
    /// Expression to compute this attribute (e.g., "disabled || loading" for computed disabled)
    pub computed: Option<&'static str>,
}

impl PropMetadata {
    pub const fn new(name: &'static str, ts_type: &'static str) -> Self {
        Self {
            name,
            ts_type,
            optional: false,
            default: None,
            html_attr: None,
            is_children: false,
            is_html_attr: false,
            computed: None,
        }
    }

    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    pub const fn with_default(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }

    pub const fn children(mut self) -> Self {
        self.is_children = true;
        self
    }

    /// Mark this prop as an HTML attribute that should be passed to the element
    pub const fn html_attr(mut self) -> Self {
        self.is_html_attr = true;
        self
    }

    /// Set a computed expression for this prop (e.g., "disabled || loading")
    pub const fn computed(mut self, expr: &'static str) -> Self {
        self.computed = Some(expr);
        self.is_html_attr = true;
        self
    }
}

/// Generate component metadata from a ComponentSpec instance
pub fn component_metadata<T: ComponentSpec>(spec: &T) -> ComponentMetadata {
    ComponentMetadata {
        name: T::NAME,
        base_classes: spec.base_classes(),
        variants: spec.variant_metadata(),
        anatomy: spec.anatomy().map(AnatomyMetadata::from),
        compound_variants: spec.compound_variant_metadata(),
        component_type: ComponentType::default(),
        props: Vec::new(),
        element: None,
        svg_content: None,
        has_children: true,
        dependencies: Vec::new(),
    }
}

/// Generate component metadata with a specific component type
pub fn component_metadata_with_type<T: ComponentSpec>(
    spec: &T,
    component_type: ComponentType,
) -> ComponentMetadata {
    ComponentMetadata {
        name: T::NAME,
        base_classes: spec.base_classes(),
        variants: spec.variant_metadata(),
        anatomy: spec.anatomy().map(AnatomyMetadata::from),
        compound_variants: spec.compound_variant_metadata(),
        component_type,
        props: Vec::new(),
        element: None,
        svg_content: None,
        has_children: true,
        dependencies: Vec::new(),
    }
}

/// Generate component metadata with props
pub fn component_metadata_with_props<T: ComponentSpec>(
    spec: &T,
    component_type: ComponentType,
    element: &'static str,
    props: Vec<PropMetadata>,
) -> ComponentMetadata {
    ComponentMetadata {
        name: T::NAME,
        base_classes: spec.base_classes(),
        variants: spec.variant_metadata(),
        anatomy: spec.anatomy().map(AnatomyMetadata::from),
        compound_variants: spec.compound_variant_metadata(),
        component_type,
        props,
        element: Some(element),
        svg_content: None,
        has_children: true,
        dependencies: Vec::new(),
    }
}

impl ComponentMetadata {
    /// Validate the cross-render contract before exporting or generating code.
    ///
    /// This catches metadata drift at the source: duplicate anatomy parts,
    /// invalid defaults, and compound conditions that refer to missing axes or
    /// values. Validation is allocation-free and deterministic.
    pub fn validate(&self) -> Result<(), MetadataValidationError> {
        if self.name.is_empty() {
            return Err(MetadataValidationError::EmptyComponentName);
        }

        if let Some(anatomy) = &self.anatomy {
            if anatomy.scope.is_empty() {
                return Err(MetadataValidationError::EmptyAnatomyScope);
            }
            if !anatomy.parts.contains(&"root") {
                return Err(MetadataValidationError::AnatomyMissingRoot);
            }
            for (index, part) in anatomy.parts.iter().enumerate() {
                if part.is_empty() {
                    return Err(MetadataValidationError::EmptyAnatomyPart);
                }
                if anatomy.parts[..index].contains(part) {
                    return Err(MetadataValidationError::DuplicateAnatomyPart(part));
                }
            }
        }

        for (variant_index, variant) in self.variants.iter().enumerate() {
            if variant.name.is_empty() {
                return Err(MetadataValidationError::EmptyVariantName);
            }
            if self.variants[..variant_index]
                .iter()
                .any(|candidate| candidate.name == variant.name)
            {
                return Err(MetadataValidationError::DuplicateVariantName(variant.name));
            }
            if variant.options.is_empty() {
                return Err(MetadataValidationError::VariantWithoutOptions(variant.name));
            }
            if !variant
                .options
                .iter()
                .any(|option| option.value == variant.default_value)
            {
                return Err(MetadataValidationError::MissingVariantDefault {
                    axis: variant.name,
                    value: variant.default_value,
                });
            }

            for (option_index, option) in variant.options.iter().enumerate() {
                if option.value.is_empty() {
                    return Err(MetadataValidationError::EmptyVariantValue(variant.name));
                }
                if variant.options[..option_index]
                    .iter()
                    .any(|candidate| candidate.value == option.value)
                {
                    return Err(MetadataValidationError::DuplicateVariantValue {
                        axis: variant.name,
                        value: option.value,
                    });
                }
            }
        }

        for (compound_index, compound) in self.compound_variants.iter().enumerate() {
            if compound.conditions.len() < 2 {
                return Err(MetadataValidationError::CompoundNeedsMultipleConditions(
                    compound_index,
                ));
            }
            if compound.classes.is_empty() {
                return Err(MetadataValidationError::EmptyCompoundClasses(
                    compound_index,
                ));
            }

            for (condition_index, condition) in compound.conditions.iter().enumerate() {
                if compound.conditions[..condition_index]
                    .iter()
                    .any(|candidate| candidate.name == condition.name)
                {
                    return Err(MetadataValidationError::DuplicateCompoundAxis {
                        compound: compound_index,
                        axis: condition.name,
                    });
                }

                let Some(axis) = self
                    .variants
                    .iter()
                    .find(|variant| variant.name == condition.name)
                else {
                    return Err(MetadataValidationError::UnknownCompoundAxis {
                        compound: compound_index,
                        axis: condition.name,
                    });
                };
                if !axis
                    .options
                    .iter()
                    .any(|option| option.value == condition.value)
                {
                    return Err(MetadataValidationError::UnknownCompoundValue {
                        compound: compound_index,
                        axis: condition.name,
                        value: condition.value,
                    });
                }
            }

            for previous in 0..compound_index {
                if same_compound_conditions(
                    &self.compound_variants[previous].conditions,
                    &compound.conditions,
                ) {
                    return Err(MetadataValidationError::DuplicateCompoundRule {
                        first: previous,
                        second: compound_index,
                    });
                }
            }
        }

        Ok(())
    }

    /// Set SVG content for icon components
    pub fn with_svg_content(mut self, content: &'static str) -> Self {
        self.svg_content = Some(content);
        self.has_children = false;
        self
    }

    /// Set whether this component has children
    pub fn with_children(mut self, has_children: bool) -> Self {
        self.has_children = has_children;
        self
    }

    /// Add a component dependency (e.g., Button depends on Spinner when loading)
    pub fn with_dependency(
        mut self,
        component: &'static str,
        condition: &'static str,
        props: Vec<(&'static str, &'static str)>,
        position: RenderPosition,
    ) -> Self {
        self.dependencies.push(ComponentDependency {
            component,
            condition,
            props,
            position,
        });
        self
    }
}

fn same_compound_conditions(
    left: &[CompoundVariantConditionMetadata],
    right: &[CompoundVariantConditionMetadata],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|condition| right.iter().any(|candidate| candidate == condition))
}

/// Structural error in recipe/anatomy metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataValidationError {
    EmptyComponentName,
    EmptyAnatomyScope,
    AnatomyMissingRoot,
    EmptyAnatomyPart,
    DuplicateAnatomyPart(&'static str),
    EmptyVariantName,
    DuplicateVariantName(&'static str),
    VariantWithoutOptions(&'static str),
    MissingVariantDefault {
        axis: &'static str,
        value: &'static str,
    },
    EmptyVariantValue(&'static str),
    DuplicateVariantValue {
        axis: &'static str,
        value: &'static str,
    },
    CompoundNeedsMultipleConditions(usize),
    EmptyCompoundClasses(usize),
    DuplicateCompoundAxis {
        compound: usize,
        axis: &'static str,
    },
    UnknownCompoundAxis {
        compound: usize,
        axis: &'static str,
    },
    UnknownCompoundValue {
        compound: usize,
        axis: &'static str,
        value: &'static str,
    },
    DuplicateCompoundRule {
        first: usize,
        second: usize,
    },
}

impl core::fmt::Display for MetadataValidationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
