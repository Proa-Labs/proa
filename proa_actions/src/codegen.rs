// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::dynamic::{ActionHandlerMetadata, ActionMetadata, ActionVariant};
use std::fmt::Write;

struct HandlerDetails {
    ts_event_type: String,
    prevent_default: bool,
}

/// Generate TypeScript contract file (types only, always overwritten)
pub fn generate_contract(actions: &[ActionMetadata]) -> String {
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED - DO NOT EDIT\n");
    output.push_str("// This file is regenerated on every build\n\n");

    for action in actions {
        // Generate namespace with constants
        output.push_str(&format!("export namespace {} {{\n", action.name));
        for variant in &action.variants {
            output.push_str(&format!(
                "    export const {} = \"{}.{}.{}\" as const;\n",
                variant.constant, action.scope, action.name, variant.name
            ));
        }
        output.push_str("}\n\n");

        // Generate parameter types
        for variant in &action.variants {
            if !variant.params.is_empty() {
                output.push_str(&format!("export interface {}Params {{\n", variant.name));
                for param in &variant.params {
                    let ts_type = rust_type_to_typescript(&param.type_name);
                    output.push_str(&format!("    {}: {};\n", param.name, ts_type));
                }
                output.push_str("}\n\n");
            }
        }
    }

    // Generate handler type aliases and handler interfaces
    for action in actions {
        for variant in &action.variants {
            let alias = handler_alias_name(action, variant);
            let signature = handler_signature(variant);
            output.push_str(&format!("export type {} = {};\n\n", alias, signature));
        }
    }

    for action in actions {
        output.push_str(&format!("export interface {}Handlers {{\n", action.name));
        for variant in &action.variants {
            let alias = handler_alias_name(action, variant);
            output.push_str(&format!(
                "    [{} .{}]: {};\n",
                action.name, variant.constant, alias
            ));
        }
        output.push_str("}\n\n");
    }

    output
}

/// Generate TypeScript runtime file (wiring only, always overwritten)
pub fn generate_runtime(actions: &[ActionMetadata], _page_name: &str) -> String {
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED - DO NOT EDIT\n");
    output.push_str("// This file is regenerated on every build\n\n");

    if !actions.is_empty() {
        let namespaces = actions
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let handler_types = actions
            .iter()
            .map(|a| format!("{}Handlers", a.name))
            .collect::<Vec<_>>()
            .join(", ");

        let _ = writeln!(output, "import {{ {namespaces} }} from './contract';");
        let _ = writeln!(
            output,
            "import type {{ {handler_types} }} from './contract';"
        );
        output.push('\n');
    }

    // Generate registry for each action type
    for action in actions {
        let handlers_name = to_camel_case(&action.name);
        let _ = writeln!(
            output,
            "let {handlers_name}Handlers: {name}Handlers | null = null;",
            name = action.name
        );
        output.push('\n');

        let _ = writeln!(
            output,
            "export function register{name}Handlers(handlers: {name}Handlers) {{",
            name = action.name
        );
        let _ = writeln!(output, "    {handlers_name}Handlers = handlers;");
        output.push_str("}\n\n");
    }

    // Generate event delegation for multiple event types
    output.push_str("// Event delegation for actions\n");
    output.push_str("const EVENT_TYPES = ['click', 'blur', 'focus', 'mouseover', 'mouseout', 'change', 'input'] as const;\n\n");
    output.push_str("for (const eventType of EVENT_TYPES) {\n");
    output.push_str("    document.addEventListener(eventType, async (event) => {\n");
    output.push_str("        const target = event.target as HTMLElement;\n");
    output.push_str("        const attrName = `data-on${eventType}`;\n");
    output.push_str("        const actionEl = target.closest(`[${attrName}]`);\n");
    output.push_str("        if (!actionEl) return;\n\n");
    output.push_str("        const actionName = actionEl.getAttribute(attrName);\n");
    output.push_str("        if (!actionName) return;\n\n");
    output.push_str("        // Parse parameters from data attributes\n");
    output.push_str("        const params: Record<string, string> = {};\n");
    output.push_str("        for (const attr of Array.from(actionEl.attributes)) {\n");
    output.push_str("            if (attr.name.startsWith('data-param-')) {\n");
    output.push_str("                const paramName = attr.name.replace('data-param-', '');\n");
    output.push_str("                params[paramName] = attr.value;\n");
    output.push_str("            }\n");
    output.push_str("        }\n\n");

    for action in actions {
        let handlers_name = to_camel_case(&action.name);
        let _ = writeln!(
            output,
            "        if (actionName.startsWith('{scope}.{name}.')) {{",
            scope = action.scope,
            name = action.name
        );
        let _ = writeln!(
            output,
            "            const handlers = {handlers_name}Handlers;"
        );
        output.push_str("            if (!handlers) {\n");
        let _ = writeln!(
            output,
            "                console.error('No handlers registered for {name}');",
            name = action.name
        );
        output.push_str("                return;\n");
        output.push_str("            }\n\n");
        output.push_str("            switch (actionName) {\n");

        for variant in &action.variants {
            let constant = format!("{}.{}", action.name, variant.constant);
            let HandlerDetails {
                ts_event_type,
                prevent_default,
            } = handler_details(variant);
            let params_type = params_type_name(variant);

            let _ = writeln!(output, "                case {constant}: {{");
            let _ = writeln!(
                output,
                "                    const handler = handlers[{constant}];"
            );
            output.push_str("                    if (!handler) {\n");
            output.push_str("                        return;\n");
            output.push_str("                    }\n");
            let _ = writeln!(
                output,
                "                    const typedEvent = event as unknown as {ts_event_type};"
            );
            if prevent_default {
                output.push_str("                    typedEvent.preventDefault();\n");
            }
            if let Some(param_type) = params_type {
                let _ = writeln!(
                    output,
                    "                    const typedParams = params as unknown as {param_type};"
                );
                output.push_str("                    await handler(typedParams, typedEvent);\n");
            } else {
                output.push_str("                    await handler(typedEvent);\n");
            }
            output.push_str("                    break;\n");
            output.push_str("                }\n");
        }

        output.push_str("            }\n");
        output.push_str("            return;\n");
        output.push_str("        }\n\n");
    }

    output.push_str("    });\n");
    output.push_str("}\n");

    output
}

/// Generate TypeScript handler stub (only if it doesn't exist)
pub fn generate_handler_stub(actions: &[ActionMetadata], page_name: &str) -> String {
    let mut output = String::new();

    output.push_str("// USER-WRITTEN HANDLERS\n");
    output.push_str("// This file is only created once and is never overwritten\n\n");

    let _ = writeln!(
        output,
        "import {{ {names} }} from '../generated/{page_name}.contract';",
        names = actions
            .iter()
            .map(|a| format!("{}, {}Handlers", a.name, a.name))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let _ = writeln!(
        output,
        "import {{ {names} }} from '../generated/{page_name}.runtime';",
        names = actions
            .iter()
            .map(|a| format!("register{}Handlers", a.name))
            .collect::<Vec<_>>()
            .join(", ")
    );
    output.push('\n');

    for action in actions {
        output.push_str(&format!(
            "const {}Handlers: {}Handlers = {{\n",
            to_camel_case(&action.name),
            action.name
        ));
        for variant in &action.variants {
            let details = handler_details(variant);
            let params_type = params_type_name(variant);

            if params_type.is_some() {
                output.push_str(&format!(
                    "    [{} .{}]: async (params, event) => {{\n",
                    action.name, variant.constant
                ));
            } else {
                output.push_str(&format!(
                    "    [{} .{}]: async (event) => {{\n",
                    action.name, variant.constant
                ));
            }
            if details.prevent_default {
                output.push_str("        event.preventDefault();\n");
            }
            output.push_str("        // TODO: Implement handler\n");
            if params_type.is_some() {
                output.push_str(&format!(
                    "        console.log('Handler called:', {}.{}, params, event);\n",
                    action.name, variant.constant
                ));
            } else {
                output.push_str(&format!(
                    "        console.log('Handler called:', {}.{}, event);\n",
                    action.name, variant.constant
                ));
            }
            output.push_str("    },\n");
        }
        output.push_str("};\n\n");
        let _ = writeln!(
            output,
            "register{name}Handlers({camel}Handlers);",
            name = action.name,
            camel = to_camel_case(&action.name)
        );
        output.push('\n');
    }

    output
}

fn handler_alias_name(action: &ActionMetadata, variant: &ActionVariant) -> String {
    format!("{}{}Handler", action.name, variant.name)
}

fn handler_signature(variant: &ActionVariant) -> String {
    let HandlerDetails { ts_event_type, .. } = handler_details(variant);
    if let Some(params_type) = params_type_name(variant) {
        format!(
            "(params: {}, event: {}) => void | Promise<void>",
            params_type, ts_event_type
        )
    } else {
        format!("(event: {}) => void | Promise<void>", ts_event_type)
    }
}

fn params_type_name(variant: &ActionVariant) -> Option<String> {
    if variant.params.is_empty() {
        None
    } else {
        Some(format!("{}Params", variant.name))
    }
}

fn handler_details(variant: &ActionVariant) -> HandlerDetails {
    event_type_string(variant.handler.as_ref())
}

fn event_type_string(handler: Option<&ActionHandlerMetadata>) -> HandlerDetails {
    match handler {
        Some(metadata) => {
            if let Some(explicit) = &metadata.ts_event_type {
                return HandlerDetails {
                    ts_event_type: explicit.clone(),
                    prevent_default: metadata.prevent_default,
                };
            }

            let base = metadata
                .event
                .as_deref()
                .map(default_event_type)
                .unwrap_or_else(|| "Event".to_string());

            let ts_event_type = metadata
                .element
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|element| {
                    format!(
                        "{} & {{ currentTarget: {}; target: {} }}",
                        base, element, element
                    )
                })
                .unwrap_or(base);

            HandlerDetails {
                ts_event_type,
                prevent_default: metadata.prevent_default,
            }
        }
        None => HandlerDetails {
            ts_event_type: "Event".to_string(),
            prevent_default: false,
        },
    }
}

fn default_event_type(event: &str) -> String {
    match event {
        "click" | "dblclick" | "mousedown" | "mouseup" | "mouseover" | "mouseout"
        | "mouseenter" | "mouseleave" | "contextmenu" => "MouseEvent".to_string(),
        "focus" | "blur" | "focusin" | "focusout" => "FocusEvent".to_string(),
        "keyup" | "keydown" | "keypress" => "KeyboardEvent".to_string(),
        "submit" => "SubmitEvent".to_string(),
        "input" => "InputEvent".to_string(),
        _ => "Event".to_string(),
    }
}

/// Convert Rust type names to TypeScript type names
fn rust_type_to_typescript(rust_type: &str) -> String {
    match rust_type {
        "String" | "str" | "&str" => "string".to_string(),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "f32" | "f64" => "number".to_string(),
        "bool" => "boolean".to_string(),
        _ if rust_type.starts_with("Option<") => {
            let inner = &rust_type[7..rust_type.len() - 1];
            format!("{} | undefined", rust_type_to_typescript(inner))
        }
        _ if rust_type.starts_with("Vec<") => {
            let inner = &rust_type[4..rust_type.len() - 1];
            format!("{}[]", rust_type_to_typescript(inner))
        }
        _ => "any".to_string(),
    }
}

/// Convert PascalCase to camelCase
fn to_camel_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_type_to_typescript() {
        assert_eq!(rust_type_to_typescript("String"), "string");
        assert_eq!(rust_type_to_typescript("i32"), "number");
        assert_eq!(rust_type_to_typescript("bool"), "boolean");
        assert_eq!(
            rust_type_to_typescript("Option<String>"),
            "string | undefined"
        );
        assert_eq!(rust_type_to_typescript("Vec<i32>"), "number[]");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("UserAction"), "userAction");
        assert_eq!(to_camel_case("HTTP"), "hTTP");
    }
}
