// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! TypeScript code generation using class-variance-authority (cva)

use crate::config::CodegenConfig;
use crate::types::{ComponentDef, VariantDef};
use std::fmt::Write;

pub struct TsGenerator;

impl TsGenerator {
    /// Get the component name in kebab-case for data-slot
    fn kebab_case(name: &str) -> String {
        let mut result = String::new();
        for (i, ch) in name.chars().enumerate() {
            if ch.is_uppercase() {
                if i > 0 {
                    result.push('-');
                }
                result.push(ch.to_lowercase().next().unwrap());
            } else {
                result.push(ch);
            }
        }
        result
    }

    fn upper_snake_case(name: &str) -> String {
        Self::kebab_case(name).replace('-', "_").to_uppercase()
    }

    fn options_const_name(enum_name: &str) -> String {
        format!("{}_OPTIONS", Self::upper_snake_case(enum_name))
    }

    fn default_const_name(enum_name: &str) -> String {
        format!("{}_DEFAULT", Self::upper_snake_case(enum_name))
    }
}

impl TsGenerator {
    /// Generate TypeScript enum for a variant
    pub fn generate_enum(variant: &VariantDef) -> String {
        let mut output = String::new();

        writeln!(&mut output, "export enum {} {{", variant.enum_name).unwrap();

        for option in &variant.options {
            writeln!(&mut output, "  {} = \"{}\",", option.name, option.value).unwrap();
        }

        writeln!(&mut output, "}}").unwrap();
        writeln!(&mut output).unwrap();

        let options_const = Self::options_const_name(&variant.enum_name);
        writeln!(&mut output, "export const {} = [", options_const).unwrap();
        for option in &variant.options {
            writeln!(
                &mut output,
                "  {{ value: \"{}\", label: \"{}\", classes: \"{}\" }},",
                option.value, option.name, option.classes
            )
            .unwrap();
        }
        writeln!(&mut output, "] as const;").unwrap();
        writeln!(
            &mut output,
            "export type {}Option = typeof {}[number];",
            variant.enum_name, options_const
        )
        .unwrap();
        writeln!(
            &mut output,
            "export type {}Value = {}Option['value'];",
            variant.enum_name, variant.enum_name
        )
        .unwrap();
        let default_const = Self::default_const_name(&variant.enum_name);
        let default_value = variant.default_value.to_lowercase();
        writeln!(
            &mut output,
            "export const {} = \"{}\";",
            default_const, default_value
        )
        .unwrap();

        output
    }

    /// Generate TypeScript type for component props using cva's VariantProps
    pub fn generate_props_type(component: &ComponentDef) -> String {
        use crate::types::ComponentType;

        let mut output = String::new();

        // Collect variant names that might conflict with HTML props
        let variant_names: Vec<&str> = component.variants.iter().map(|v| v.name.as_str()).collect();

        // Determine the element type based on component_type
        let (element_type, _ref_type) = match component.component_type {
            ComponentType::Svg => ("'svg'", "SVGSVGElement"),
            ComponentType::Button => ("'button'", "HTMLButtonElement"),
        };

        // Check if any variants conflict with native element props (like 'color')
        let has_conflicts = variant_names.contains(&"color");

        if has_conflicts {
            // Use Omit to exclude conflicting props
            let omit_props: Vec<&str> = variant_names
                .iter()
                .filter(|&&name| name == "color")
                .copied()
                .collect();
            let omit_str = omit_props.join(" | ");

            writeln!(
                &mut output,
                "export interface {}Props extends Omit<React.ComponentPropsWithoutRef<{}>, '{}'>, VariantProps<typeof {}Variants> {{",
                component.name,
                element_type,
                omit_str,
                component.name.to_lowercase()
            ).unwrap();
        } else {
            writeln!(
                &mut output,
                "export interface {}Props extends React.ComponentPropsWithoutRef<{}>, VariantProps<typeof {}Variants> {{",
                component.name,
                element_type,
                component.name.to_lowercase()
            ).unwrap();
        }

        writeln!(&mut output, "  children?: React.ReactNode;").unwrap();

        writeln!(&mut output, "}}").unwrap();

        // Store ref_type for later use (we'll need it in component generation)
        output
    }

    /// Generate cva recipe with variants
    pub fn generate_cva_recipe(component: &ComponentDef) -> String {
        let mut output = String::new();

        writeln!(
            &mut output,
            "export const {}Variants = cva(",
            component.name.to_lowercase()
        )
        .unwrap();

        // Base classes
        writeln!(&mut output, "  \"{}\",", component.base_classes).unwrap();

        // Variants object
        writeln!(&mut output, "  {{").unwrap();
        writeln!(&mut output, "    variants: {{").unwrap();

        // Generate each variant dimension
        for variant in &component.variants {
            writeln!(&mut output, "      {}: {{", variant.name).unwrap();

            for option in &variant.options {
                writeln!(
                    &mut output,
                    "        {}: \"{}\",",
                    option.value, option.classes
                )
                .unwrap();
            }

            writeln!(&mut output, "      }},").unwrap();
        }

        writeln!(&mut output, "    }},").unwrap();

        // Default variants
        writeln!(&mut output, "    defaultVariants: {{").unwrap();
        for variant in &component.variants {
            // If variant is optional (recipe default is None), use undefined
            if variant.optional {
                writeln!(&mut output, "      {}: undefined,", variant.name).unwrap();
            } else {
                writeln!(
                    &mut output,
                    "      {}: \"{}\",",
                    variant.name,
                    variant.default_value.to_lowercase()
                )
                .unwrap();
            }
        }
        writeln!(&mut output, "    }},").unwrap();

        writeln!(&mut output, "  }}").unwrap();
        writeln!(&mut output, ");").unwrap();
        writeln!(&mut output).unwrap();

        let defaults_const = format!("{}Defaults", component.name.to_lowercase());
        writeln!(&mut output, "export const {} = {{", defaults_const).unwrap();
        for variant in &component.variants {
            writeln!(
                &mut output,
                "  {}: \"{}\",",
                variant.name,
                variant.default_value.to_lowercase()
            )
            .unwrap();
        }
        writeln!(&mut output, "}} as const;").unwrap();
        writeln!(&mut output).unwrap();

        let options_const = format!("{}VariantOptions", component.name.to_lowercase());
        writeln!(&mut output, "export const {} = {{", options_const).unwrap();
        for variant in &component.variants {
            let variant_const = Self::options_const_name(&variant.enum_name);
            writeln!(&mut output, "  {}: {},", variant.name, variant_const).unwrap();
        }
        writeln!(&mut output, "}} as const;").unwrap();
        writeln!(&mut output).unwrap();

        output
    }

    /// Generate React component using cva
    pub fn generate_react_component(component: &ComponentDef, config: &CodegenConfig) -> String {
        let mut output = String::new();

        let variant_names: Vec<String> =
            component.variants.iter().map(|v| v.name.clone()).collect();
        let variant_str = variant_names.join(", ");
        let variant_bindings = if variant_str.is_empty() {
            String::new()
        } else {
            format!("{variant_str}, ")
        };
        let variant_arguments = if variant_str.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {variant_str} }}")
        };
        let data_slot = Self::kebab_case(&component.name);

        if config.use_forward_ref {
            // React 18 style with forwardRef
            writeln!(&mut output, "import * as React from 'react';").unwrap();
            writeln!(&mut output, "import {{ forwardRef }} from 'react';").unwrap();
            writeln!(
                &mut output,
                "import {{ {}Variants, {}Props }} from './recipe';",
                component.name.to_lowercase(),
                component.name
            )
            .unwrap();
            writeln!(&mut output, "import {{ cn }} from '../../lib/utils';").unwrap();
            writeln!(&mut output).unwrap();

            // Determine ref type based on component type
            use crate::types::ComponentType;
            let ref_type = match component.component_type {
                ComponentType::Svg => "SVGSVGElement",
                ComponentType::Button => "HTMLButtonElement",
            };

            if config.use_named_functions {
                // Named function style
                writeln!(
                    &mut output,
                    "export function {}({{ {}children, className, ...props }}: {}Props & {{ ref?: React.Ref<{}> }}) {{",
                    component.name, variant_bindings, component.name, ref_type
                ).unwrap();
            } else {
                // Const with forwardRef
                writeln!(
                    &mut output,
                    "export const {} = forwardRef<{}, {}Props>(",
                    component.name, ref_type, component.name
                )
                .unwrap();
                writeln!(
                    &mut output,
                    "  ({{ {}children, className, ...props }}, ref) => {{",
                    variant_bindings
                )
                .unwrap();
            }

            // Special handling for Button: solid variant defaults to primary color if not specified
            if component.name == "Button"
                && variant_names.contains(&"color".to_string())
                && variant_names.contains(&"variant".to_string())
            {
                writeln!(
                    &mut output,
                    "    // Solid variant gets primary color by default; outline/ghost ignore color prop\n    const finalColor = variant === 'solid' ? (color || 'primary') : undefined;"
                )
                .unwrap();
                let variant_str_modified = variant_str.replace("color", "color: finalColor");
                writeln!(
                    &mut output,
                    "    const classes = cn({}Variants({{ {} }}), className);",
                    component.name.to_lowercase(),
                    variant_str_modified
                )
                .unwrap();
            } else {
                writeln!(
                    &mut output,
                    "    const classes = cn({}Variants({}), className);",
                    component.name.to_lowercase(),
                    variant_arguments
                )
                .unwrap();
            }
            writeln!(&mut output).unwrap();

            writeln!(&mut output, "    return (").unwrap();

            // Generate element based on component type
            match component.component_type {
                ComponentType::Svg if component.name == "Spinner" => {
                    writeln!(&mut output, "      <svg").unwrap();
                    writeln!(&mut output, "        ref={{ref}}").unwrap();
                    if config.add_data_slot {
                        writeln!(&mut output, "        data-slot=\"{}\"", data_slot).unwrap();
                    }
                    writeln!(&mut output, "        className={{classes}}").unwrap();
                    writeln!(&mut output, "        xmlns=\"http://www.w3.org/2000/svg\"").unwrap();
                    writeln!(&mut output, "        width=\"24\"").unwrap();
                    writeln!(&mut output, "        height=\"24\"").unwrap();
                    writeln!(&mut output, "        viewBox=\"0 0 24 24\"").unwrap();
                    writeln!(&mut output, "        fill=\"none\"").unwrap();
                    writeln!(&mut output, "        stroke=\"currentColor\"").unwrap();
                    writeln!(&mut output, "        strokeWidth=\"2\"").unwrap();
                    writeln!(&mut output, "        strokeLinecap=\"round\"").unwrap();
                    writeln!(&mut output, "        strokeLinejoin=\"round\"").unwrap();
                    writeln!(&mut output, "        role=\"status\"").unwrap();
                    writeln!(&mut output, "        aria-label=\"Loading\"").unwrap();
                    writeln!(&mut output, "        {{...props}}").unwrap();
                    writeln!(&mut output, "      >").unwrap();
                    writeln!(
                        &mut output,
                        "        <path d=\"M21 12a9 9 0 1 1-6.219-8.56\" />"
                    )
                    .unwrap();
                    writeln!(&mut output, "      </svg>").unwrap();
                }
                _ => {
                    if config.add_data_slot {
                        writeln!(&mut output, "      <button ref={{ref}} data-slot=\"{}\" className={{classes}} {{...props}}>",
                            data_slot).unwrap();
                    } else {
                        writeln!(
                            &mut output,
                            "      <button ref={{ref}} className={{classes}} {{...props}}>"
                        )
                        .unwrap();
                    }
                    writeln!(&mut output, "        {{children}}").unwrap();
                    writeln!(&mut output, "      </button>").unwrap();
                }
            }

            writeln!(&mut output, "    );").unwrap();

            if config.use_named_functions {
                writeln!(&mut output, "}}").unwrap();
            } else {
                writeln!(&mut output, "  }}").unwrap();
                writeln!(&mut output, ");").unwrap();
                writeln!(&mut output).unwrap();
                writeln!(
                    &mut output,
                    "{}.displayName = '{}';",
                    component.name, component.name
                )
                .unwrap();
            }
        } else {
            // React 19 style - ref as prop, no forwardRef
            writeln!(&mut output, "import * as React from 'react';").unwrap();
            writeln!(
                &mut output,
                "import {{ {}Variants, {}Props }} from './recipe';",
                component.name.to_lowercase(),
                component.name
            )
            .unwrap();
            writeln!(&mut output, "import {{ cn }} from '../../lib/utils';").unwrap();
            writeln!(&mut output).unwrap();

            // Determine ref type based on component type
            use crate::types::ComponentType;
            let ref_type = match component.component_type {
                ComponentType::Svg => "SVGSVGElement",
                ComponentType::Button => "HTMLButtonElement",
            };

            writeln!(
                &mut output,
                "export function {}({{ {}children, className, ref, ...props }}: {}Props & {{ ref?: React.Ref<{}> }}) {{",
                component.name, variant_bindings, component.name, ref_type
            ).unwrap();

            // Special handling for Button: solid variant defaults to primary color if not specified
            if component.name == "Button"
                && variant_names.contains(&"color".to_string())
                && variant_names.contains(&"variant".to_string())
            {
                writeln!(
                    &mut output,
                    "  // Solid variant gets primary color by default; outline/ghost ignore color prop\n  const finalColor = variant === 'solid' ? (color || 'primary') : undefined;"
                )
                .unwrap();
                let variant_str_modified = variant_str.replace("color", "color: finalColor");
                writeln!(
                    &mut output,
                    "  const classes = cn({}Variants({{ {} }}), className);",
                    component.name.to_lowercase(),
                    variant_str_modified
                )
                .unwrap();
            } else {
                writeln!(
                    &mut output,
                    "  const classes = cn({}Variants({}), className);",
                    component.name.to_lowercase(),
                    variant_arguments
                )
                .unwrap();
            }
            writeln!(&mut output).unwrap();

            writeln!(&mut output, "  return (").unwrap();

            // Generate element based on component type
            match component.component_type {
                ComponentType::Svg if component.name == "Spinner" => {
                    writeln!(&mut output, "    <svg").unwrap();
                    writeln!(&mut output, "      ref={{ref}}").unwrap();
                    if config.add_data_slot {
                        writeln!(&mut output, "      data-slot=\"{}\"", data_slot).unwrap();
                    }
                    writeln!(&mut output, "      className={{classes}}").unwrap();
                    writeln!(&mut output, "      xmlns=\"http://www.w3.org/2000/svg\"").unwrap();
                    writeln!(&mut output, "      width=\"24\"").unwrap();
                    writeln!(&mut output, "      height=\"24\"").unwrap();
                    writeln!(&mut output, "      viewBox=\"0 0 24 24\"").unwrap();
                    writeln!(&mut output, "      fill=\"none\"").unwrap();
                    writeln!(&mut output, "      stroke=\"currentColor\"").unwrap();
                    writeln!(&mut output, "      strokeWidth=\"2\"").unwrap();
                    writeln!(&mut output, "      strokeLinecap=\"round\"").unwrap();
                    writeln!(&mut output, "      strokeLinejoin=\"round\"").unwrap();
                    writeln!(&mut output, "      role=\"status\"").unwrap();
                    writeln!(&mut output, "      aria-label=\"Loading\"").unwrap();
                    writeln!(&mut output, "      {{...props}}").unwrap();
                    writeln!(&mut output, "    >").unwrap();
                    writeln!(
                        &mut output,
                        "      <path d=\"M21 12a9 9 0 1 1-6.219-8.56\" />"
                    )
                    .unwrap();
                    writeln!(&mut output, "    </svg>").unwrap();
                }
                _ => {
                    if config.add_data_slot {
                        writeln!(
                            &mut output,
                            "    <button ref={{ref}} data-slot=\"{}\" className={{classes}} {{...props}}>",
                            data_slot
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            &mut output,
                            "    <button ref={{ref}} className={{classes}} {{...props}}>"
                        )
                        .unwrap();
                    }

                    writeln!(&mut output, "      {{children}}").unwrap();
                    writeln!(&mut output, "    </button>").unwrap();
                }
            }

            writeln!(&mut output, "  );").unwrap();
            writeln!(&mut output, "}}").unwrap();
        }

        output
    }

    /// Generate complete TypeScript file with cva recipe
    pub fn generate_recipe_file(component: &ComponentDef) -> String {
        let mut output = String::new();

        writeln!(&mut output, "// Generated by proa-codegen - DO NOT EDIT").unwrap();
        writeln!(
            &mut output,
            "// Source: component metadata for {}",
            component.name
        )
        .unwrap();
        writeln!(&mut output).unwrap();

        writeln!(&mut output, "import * as React from 'react';").unwrap();
        writeln!(
            &mut output,
            "import {{ cva, type VariantProps }} from 'class-variance-authority';"
        )
        .unwrap();
        writeln!(&mut output).unwrap();

        // Generate enums
        for variant in &component.variants {
            output.push_str(&Self::generate_enum(variant));
            writeln!(&mut output).unwrap();
        }

        // Generate cva recipe
        output.push_str(&Self::generate_cva_recipe(component));
        writeln!(&mut output).unwrap();

        // Generate props type
        output.push_str(&Self::generate_props_type(component));

        output
    }

    /// Generate complete React component file
    pub fn generate_component_file(component: &ComponentDef, config: &CodegenConfig) -> String {
        let mut output = String::new();

        writeln!(&mut output, "// Generated by proa-codegen - DO NOT EDIT").unwrap();
        writeln!(
            &mut output,
            "// Source: component metadata for {}",
            component.name
        )
        .unwrap();
        writeln!(&mut output).unwrap();

        output.push_str(&Self::generate_react_component(component, config));

        output
    }

    /// Generate index.ts that exports everything
    pub fn generate_index_file(component: &ComponentDef) -> String {
        let mut output = String::new();

        writeln!(&mut output, "// Generated by proa-codegen - DO NOT EDIT").unwrap();
        writeln!(&mut output).unwrap();

        writeln!(&mut output, "export * from './recipe';").unwrap();
        writeln!(&mut output, "export * from './{}';", component.name).unwrap();

        output
    }

    /// Generate cn utility function
    pub fn generate_cn_util() -> String {
        let mut output = String::new();

        writeln!(&mut output, "// Generated by proa-codegen - DO NOT EDIT").unwrap();
        writeln!(&mut output).unwrap();
        writeln!(
            &mut output,
            "import {{ clsx, type ClassValue }} from 'clsx';"
        )
        .unwrap();
        writeln!(&mut output).unwrap();
        writeln!(
            &mut output,
            "export function cn(...inputs: ClassValue[]) {{"
        )
        .unwrap();
        writeln!(&mut output, "  return clsx(inputs);").unwrap();
        writeln!(&mut output, "}}").unwrap();

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_generate_enum() {
        let variant = VariantDef::new("size", "ButtonSize", "Default")
            .with_option(VariantOption::new("Sm", "sm", "h-8 px-3"))
            .with_option(VariantOption::new("Default", "default", "h-9 px-4"))
            .with_option(VariantOption::new("Lg", "lg", "h-10 px-6"));

        let ts = TsGenerator::generate_enum(&variant);

        assert!(ts.contains("export enum ButtonSize"));
        assert!(ts.contains("Sm = \"sm\""));
        assert!(ts.contains("Default = \"default\""));
        assert!(ts.contains("Lg = \"lg\""));
        assert!(ts.contains("export const BUTTON_SIZE_OPTIONS = ["));
        assert!(ts.contains("{ value: \"sm\", label: \"Sm\""));
        assert!(ts.contains("export type ButtonSizeOption = typeof BUTTON_SIZE_OPTIONS[number];"));
        assert!(ts.contains("export type ButtonSizeValue = ButtonSizeOption['value'];"));
        assert!(ts.contains("export const BUTTON_SIZE_DEFAULT = \"default\";"));
    }

    #[test]
    fn test_generate_cva_recipe() {
        let component = ComponentDef::new("Button")
            .with_base_classes("inline-flex items-center")
            .with_variant(
                VariantDef::new("size", "ButtonSize", "Default")
                    .with_option(VariantOption::new("Sm", "sm", "h-8 px-3"))
                    .with_option(VariantOption::new("Default", "default", "h-9 px-4")),
            );

        let ts = TsGenerator::generate_cva_recipe(&component);

        assert!(ts.contains("export const buttonVariants = cva("));
        assert!(ts.contains("\"inline-flex items-center\""));
        assert!(ts.contains("variants:"));
        assert!(ts.contains("defaultVariants:"));
        assert!(ts.contains("export const buttonDefaults = {"));
        assert!(ts.contains("size: \"default\""));
        assert!(ts.contains("export const buttonVariantOptions = {"));
        assert!(ts.contains("size: BUTTON_SIZE_OPTIONS"));
    }

    #[test]
    fn component_without_variants_has_valid_destructuring() {
        let component = ComponentDef::new("Badge").with_base_classes("inline-flex");

        for config in [CodegenConfig::react_18(), CodegenConfig::react_19()] {
            let ts = TsGenerator::generate_react_component(&component, &config);
            assert!(!ts.contains("{ ,"));
            assert!(ts.contains("{ children, className"));
            assert!(ts.contains("badgeVariants({})"));
        }
    }
}
