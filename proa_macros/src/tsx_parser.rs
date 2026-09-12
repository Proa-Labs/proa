// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hand-rolled TypeScript interface extractor.
//!
//! Parses `interface FooProps { ... }` and `type FooProps = { ... }` blocks
//! from TSX source files, mapping TypeScript types to Rust equivalents.

use std::collections::HashMap;

/// A parsed TypeScript props interface.
#[derive(Debug, Clone)]
pub struct PropsInterface {
    pub fields: Vec<PropsField>,
}

/// A single field in a props interface.
#[derive(Debug, Clone)]
pub struct PropsField {
    /// Original camelCase name from TypeScript
    pub ts_name: String,
    /// snake_case name for Rust
    pub rust_name: String,
    /// The mapped Rust type
    pub rust_type: RustType,
    /// Whether the field was optional in TypeScript (`?:` or `| undefined`)
    pub optional: bool,
}

/// A Rust type mapped from TypeScript.
#[derive(Debug, Clone, PartialEq)]
pub enum RustType {
    F64,
    String,
    Bool,
    Vec(Box<RustType>),
    Option(Box<RustType>),
    HashMap(Box<RustType>, Box<RustType>),
    Children,
    JsonValue,
    Named(String),
}

impl RustType {
    /// Return the token representation for codegen.
    pub fn to_tokens(&self) -> proc_macro2::TokenStream {
        use quote::quote;
        match self {
            RustType::F64 => quote! { f64 },
            RustType::String => quote! { String },
            RustType::Bool => quote! { bool },
            RustType::Vec(inner) => {
                let inner = inner.to_tokens();
                quote! { Vec<#inner> }
            }
            RustType::Option(inner) => {
                let inner = inner.to_tokens();
                quote! { Option<#inner> }
            }
            RustType::HashMap(k, v) => {
                let k = k.to_tokens();
                let v = v.to_tokens();
                quote! { std::collections::HashMap<#k, #v> }
            }
            RustType::Children => quote! { proa_react::Children },
            RustType::JsonValue => quote! { serde_json::Value },
            RustType::Named(name) => {
                let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
                quote! { #ident }
            }
        }
    }
}

/// Extract the props interface for a component from TSX source.
///
/// Looks for `interface {component_name}Props { ... }` or
/// `type {component_name}Props = { ... }` in the source.
///
/// Returns `Ok(None)` if the component exists but has no explicit props interface
/// (will generate an empty struct). Returns `Err` on parse failures.
pub fn extract_props(
    tsx_content: &str,
    component_name: &str,
) -> Result<Option<PropsInterface>, String> {
    let stripped = strip_comments(tsx_content);
    let target_name = format!("{}Props", component_name);

    // Also collect named interfaces referenced by fields (for nested types)
    let all_interfaces = find_all_interfaces(&stripped);

    // Try to find the main props interface
    if let Some(body) = find_interface_body(&stripped, &target_name) {
        let fields = parse_fields(&body, &all_interfaces)?;
        Ok(Some(PropsInterface { fields }))
    } else if let Some(body) = find_type_alias_body(&stripped, &target_name) {
        let fields = parse_fields(&body, &all_interfaces)?;
        Ok(Some(PropsInterface { fields }))
    } else {
        Ok(None)
    }
}

/// Strip `//` and `/* */` comments from source.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment — skip to end of line
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Block comment — skip to */
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            out.push(' '); // replace comment with space to preserve spacing
        } else if bytes[i] == b'\'' || bytes[i] == b'"' {
            // Regular string literal — don't strip inside strings
            let quote = bytes[i];
            out.push(bytes[i] as char);
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < len {
                    out.push(bytes[i] as char);
                    i += 1;
                    out.push(bytes[i] as char);
                    i += 1;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            if i < len {
                out.push(bytes[i] as char);
                i += 1;
            }
        } else if bytes[i] == b'`' {
            // Template literal — handle ${...} interpolations which may contain
            // nested backticks. Only treat a backtick as closing the template
            // when interpolation depth is 0.
            out.push('`');
            i += 1;
            let mut depth: usize = 0; // nesting depth of ${ interpolations
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    // Escaped character — pass through verbatim
                    out.push(bytes[i] as char);
                    i += 1;
                    out.push(bytes[i] as char);
                    i += 1;
                } else if depth == 0 && bytes[i] == b'`' {
                    // Closing backtick of the outermost template literal
                    break;
                } else if i + 1 < len && bytes[i] == b'$' && bytes[i + 1] == b'{' {
                    // Start of interpolation
                    out.push('$');
                    out.push('{');
                    i += 2;
                    depth += 1;
                } else if depth > 0 && bytes[i] == b'}' {
                    // End of interpolation
                    out.push('}');
                    i += 1;
                    depth -= 1;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            if i < len {
                out.push(bytes[i] as char); // closing backtick
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }

    out
}

/// Find `interface {name} { ... }` and return the body between braces.
fn find_interface_body(src: &str, name: &str) -> Option<String> {
    // Pattern: "interface" whitespace name whitespace? (extends ...)? "{"
    let pattern = format!("interface {}", name);
    let mut search_from = 0;

    while let Some(pos) = src[search_from..].find(&pattern) {
        let abs_pos = search_from + pos;
        let after_name = abs_pos + pattern.len();

        // Make sure this is a word boundary (not part of a longer identifier)
        if abs_pos > 0 {
            let prev = src.as_bytes()[abs_pos - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search_from = after_name;
                continue;
            }
        }

        // Check what comes after the name — should be whitespace, {, or extends
        if after_name < src.len() {
            let next = src.as_bytes()[after_name];
            if next.is_ascii_alphanumeric() || next == b'_' {
                search_from = after_name;
                continue;
            }
        }

        // Find the opening brace
        if let Some(brace_start) = src[after_name..].find('{') {
            let brace_abs = after_name + brace_start;
            return extract_brace_body(src, brace_abs);
        }

        search_from = after_name;
    }

    None
}

/// Find `type {name} = { ... }` and return the body between braces.
fn find_type_alias_body(src: &str, name: &str) -> Option<String> {
    let pattern = format!("type {}", name);
    let mut search_from = 0;

    while let Some(pos) = src[search_from..].find(&pattern) {
        let abs_pos = search_from + pos;
        let after_name = abs_pos + pattern.len();

        // Word boundary check
        if abs_pos > 0 {
            let prev = src.as_bytes()[abs_pos - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search_from = after_name;
                continue;
            }
        }
        if after_name < src.len() {
            let next = src.as_bytes()[after_name];
            if next.is_ascii_alphanumeric() || next == b'_' {
                search_from = after_name;
                continue;
            }
        }

        // Find `=` then `{`
        let rest = &src[after_name..];
        if let Some(eq_pos) = rest.find('=') {
            let after_eq = &rest[eq_pos + 1..];
            let trimmed = after_eq.trim_start();
            if trimmed.starts_with('{') {
                let brace_abs = after_name + eq_pos + 1 + (after_eq.len() - trimmed.len());
                return extract_brace_body(src, brace_abs);
            }
        }

        search_from = after_name;
    }

    None
}

/// Extract content between matched braces starting at `start` (which must point to `{`).
fn extract_brace_body(src: &str, start: usize) -> Option<String> {
    let bytes = src.as_bytes();
    if bytes.get(start).copied() != Some(b'{') {
        return None;
    }

    let mut depth = 1;
    let mut i = start + 1;

    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }

    if depth == 0 {
        // Return content between { and }
        Some(src[start + 1..i - 1].to_string())
    } else {
        None
    }
}

/// Find all named interfaces/types in the file.
fn find_all_interfaces(src: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();

    // Find all `interface Foo { ... }`
    let mut search_from = 0;
    while let Some(pos) = src[search_from..].find("interface ") {
        let abs_pos = search_from + pos;
        let after_keyword = abs_pos + "interface ".len();

        // Extract name
        if let Some(name) = extract_identifier(&src[after_keyword..]) {
            if let Some(body) = find_interface_body(src, &name) {
                result.insert(name, body);
            }
        }
        search_from = after_keyword;
    }

    // Find all `type Foo = { ... }`
    search_from = 0;
    while let Some(pos) = src[search_from..].find("type ") {
        let abs_pos = search_from + pos;
        let after_keyword = abs_pos + "type ".len();

        if let Some(name) = extract_identifier(&src[after_keyword..]) {
            if let Some(body) = find_type_alias_body(src, &name) {
                result.insert(name, body);
            }
        }
        search_from = after_keyword;
    }

    result
}

/// Extract a leading identifier from source.
fn extract_identifier(src: &str) -> Option<String> {
    let mut chars = src.chars().peekable();
    let first = chars.peek()?;
    if !first.is_ascii_alphabetic() && *first != '_' {
        return None;
    }

    let mut name = String::new();
    for ch in chars {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            break;
        }
    }

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Parse field declarations from the body of an interface.
fn parse_fields(
    body: &str,
    all_interfaces: &HashMap<String, String>,
) -> Result<Vec<PropsField>, String> {
    let mut fields = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        // Skip lines that don't look like field declarations
        // A field line should contain a colon (name: type or name?: type)
        if !trimmed.contains(':') {
            continue;
        }

        if let Some(field) = parse_field_line(trimmed, all_interfaces)? {
            fields.push(field);
        }
    }

    Ok(fields)
}

/// Parse a single field line like `fieldName?: Type;`
fn parse_field_line(
    line: &str,
    all_interfaces: &HashMap<String, String>,
) -> Result<Option<PropsField>, String> {
    // Remove trailing semicolon/comma
    let line = line.trim_end_matches(|c: char| c == ';' || c == ',' || c.is_whitespace());
    if line.is_empty() {
        return Ok(None);
    }

    // Split on first `:`
    let colon_pos = match line.find(':') {
        Some(p) => p,
        None => return Ok(None),
    };

    let name_part = line[..colon_pos].trim();
    let type_part = line[colon_pos + 1..].trim();

    // Skip function types: `onClick?: () => void`
    if is_function_type(type_part) {
        return Ok(None);
    }

    // Check for optional marker
    let (ts_name, optional_from_question) = if let Some(stripped) = name_part.strip_suffix('?') {
        (stripped.trim(), true)
    } else {
        (name_part, false)
    };

    // Skip readonly modifier
    let ts_name = ts_name.strip_prefix("readonly ").unwrap_or(ts_name).trim();

    // Validate identifier
    if ts_name.is_empty()
        || !ts_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Ok(None);
    }

    let rust_name = camel_to_snake(ts_name);

    // Parse the type
    let (rust_type, optional_from_type) = parse_ts_type(type_part, all_interfaces);

    let optional = optional_from_question || optional_from_type;

    let final_type = if optional {
        match rust_type {
            RustType::Option(_) => rust_type, // already wrapped
            RustType::Children => rust_type,  // Children has its own Default
            _ => RustType::Option(Box::new(rust_type)),
        }
    } else {
        rust_type
    };

    Ok(Some(PropsField {
        ts_name: ts_name.to_string(),
        rust_name,
        rust_type: final_type,
        optional,
    }))
}

/// Parse a TypeScript type string into a RustType.
/// Returns (type, was_optional_from_union).
fn parse_ts_type(ts_type: &str, all_interfaces: &HashMap<String, String>) -> (RustType, bool) {
    let ts_type = ts_type.trim();

    // Handle union with undefined/null → Option<T>
    if let Some(inner) = strip_nullable_union(ts_type) {
        let (inner_type, _) = parse_ts_type(inner, all_interfaces);
        return (inner_type, true);
    }

    // Primitive types
    match ts_type {
        "number" => return (RustType::F64, false),
        "string" => return (RustType::String, false),
        "boolean" | "bool" => return (RustType::Bool, false),
        "React.ReactNode" | "ReactNode" | "JSX.Element" => return (RustType::Children, false),
        "any" | "unknown" => return (RustType::JsonValue, false),
        _ => {}
    }

    // Array<T>
    if let Some(inner) = strip_generic(ts_type, "Array") {
        let (inner_type, _) = parse_ts_type(inner, all_interfaces);
        return (RustType::Vec(Box::new(inner_type)), false);
    }

    // T[]
    if let Some(inner) = ts_type.strip_suffix("[]") {
        let (inner_type, _) = parse_ts_type(inner, all_interfaces);
        return (RustType::Vec(Box::new(inner_type)), false);
    }

    // Record<string, T>
    if let Some(inner) = strip_generic(ts_type, "Record") {
        if let Some(comma) = inner.find(',') {
            let key_type = inner[..comma].trim();
            let val_type = inner[comma + 1..].trim();
            let (k, _) = parse_ts_type(key_type, all_interfaces);
            let (v, _) = parse_ts_type(val_type, all_interfaces);
            return (RustType::HashMap(Box::new(k), Box::new(v)), false);
        }
    }

    // Partial<T> → all fields optional, but for simplicity treat as JsonValue
    if ts_type.starts_with("Partial<") {
        return (RustType::JsonValue, false);
    }

    // Named interface in the same file
    if is_identifier(ts_type) && all_interfaces.contains_key(ts_type) {
        return (RustType::Named(ts_type.to_string()), false);
    }

    // String literal unions like "small" | "medium" | "large" → String
    if ts_type.contains('|') {
        let parts: Vec<&str> = ts_type.split('|').map(|s| s.trim()).collect();
        let all_string_literals = parts.iter().all(|p| {
            (p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\''))
        });
        if all_string_literals {
            return (RustType::String, false);
        }
    }

    // Fallback
    (RustType::JsonValue, false)
}

/// Check if a union type contains only one non-nullable part alongside `null`/`undefined`/`never`.
/// e.g. `T | undefined`, `T | null`, `string | null | undefined`, `string | number | undefined`
/// Returns the single non-nullable part, or `None` if there are 0 or 2+ non-nullable parts.
fn strip_nullable_union(ts_type: &str) -> Option<&str> {
    if !ts_type.contains('|') {
        return None;
    }

    let nullable = ["undefined", "null", "never"];

    let parts: Vec<&str> = ts_type.split('|').map(|s| s.trim()).collect();
    let mut non_nullable: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| !nullable.contains(p))
        .collect();

    if non_nullable.len() == 1 {
        Some(non_nullable.remove(0))
    } else {
        // Zero or multiple non-nullable parts — can't simplify
        None
    }
}

/// Strip `Generic<inner>` and return inner.
fn strip_generic<'a>(ts_type: &'a str, generic_name: &str) -> Option<&'a str> {
    let prefix = format!("{}<", generic_name);
    if ts_type.starts_with(&prefix) && ts_type.ends_with('>') {
        Some(&ts_type[prefix.len()..ts_type.len() - 1])
    } else {
        None
    }
}

/// Check if a type string represents a function type.
/// e.g. `() => void`, `(e: Event) => void`, `(arg: T) => Promise<void>`
fn is_function_type(ts_type: &str) -> bool {
    let trimmed = ts_type.trim();
    // Arrow function types: starts with ( and contains =>
    if trimmed.starts_with('(') && trimmed.contains("=>") {
        return true;
    }
    // Also handle optional function types: `(() => void) | undefined`
    if trimmed.contains("=>") && trimmed.contains('(') {
        return true;
    }
    false
}

/// Check if a string is a simple identifier (PascalCase, camelCase, etc.).
fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Convert camelCase to snake_case.
pub fn camel_to_snake(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    let mut prev_was_upper = false;

    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 && !prev_was_upper {
                result.push('_');
            }
            // Handle sequences of uppercase: "HTMLParser" → "html_parser"
            if prev_was_upper && i + 1 < name.len() {
                let next = name.as_bytes()[i + 1];
                if next.is_ascii_lowercase() {
                    result.push('_');
                }
            }
            result.push(ch.to_ascii_lowercase());
            prev_was_upper = true;
        } else {
            result.push(ch);
            prev_was_upper = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_interface() {
        let tsx = r#"
            interface CounterProps {
                initialCount: number;
                label: string;
            }
            export default function Counter(props: CounterProps) { return <div />; }
        "#;

        let result = extract_props(tsx, "Counter").unwrap().unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[0].ts_name, "initialCount");
        assert_eq!(result.fields[0].rust_name, "initial_count");
        assert_eq!(result.fields[0].rust_type, RustType::F64);
        assert!(!result.fields[0].optional);
        assert_eq!(result.fields[1].ts_name, "label");
        assert_eq!(result.fields[1].rust_type, RustType::String);
    }

    #[test]
    fn test_type_alias() {
        let tsx = r#"
            type ButtonProps = {
                text: string;
                onClick?: () => void;
            };
        "#;

        let result = extract_props(tsx, "Button").unwrap().unwrap();
        assert_eq!(result.fields.len(), 1); // onClick is skipped (function type)
        assert_eq!(result.fields[0].ts_name, "text");
        assert_eq!(result.fields[0].rust_type, RustType::String);
    }

    #[test]
    fn test_optional_fields() {
        let tsx = r#"
            interface WidgetProps {
                name: string;
                count?: number;
                disabled: boolean | undefined;
                label: string | null;
            }
        "#;

        let result = extract_props(tsx, "Widget").unwrap().unwrap();
        assert_eq!(result.fields.len(), 4);

        assert!(!result.fields[0].optional); // name: string
        assert!(result.fields[1].optional); // count?: number
        assert_eq!(
            result.fields[1].rust_type,
            RustType::Option(Box::new(RustType::F64))
        );
        assert!(result.fields[2].optional); // disabled: boolean | undefined
        assert_eq!(
            result.fields[2].rust_type,
            RustType::Option(Box::new(RustType::Bool))
        );
        assert!(result.fields[3].optional); // label: string | null
        assert_eq!(
            result.fields[3].rust_type,
            RustType::Option(Box::new(RustType::String))
        );
    }

    #[test]
    fn test_array_types() {
        let tsx = r#"
            interface ListProps {
                items: string[];
                numbers: Array<number>;
                nested: boolean[][];
            }
        "#;

        let result = extract_props(tsx, "List").unwrap().unwrap();
        assert_eq!(result.fields.len(), 3);
        assert_eq!(
            result.fields[0].rust_type,
            RustType::Vec(Box::new(RustType::String))
        );
        assert_eq!(
            result.fields[1].rust_type,
            RustType::Vec(Box::new(RustType::F64))
        );
        assert_eq!(
            result.fields[2].rust_type,
            RustType::Vec(Box::new(RustType::Vec(Box::new(RustType::Bool))))
        );
    }

    #[test]
    fn test_children_field() {
        let tsx = r#"
            interface AccordionProps {
                title: string;
                children: React.ReactNode;
            }
        "#;

        let result = extract_props(tsx, "Accordion").unwrap().unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[1].ts_name, "children");
        assert_eq!(result.fields[1].rust_type, RustType::Children);
    }

    #[test]
    fn test_complex_type_fallback() {
        let tsx = r#"
            interface ComplexProps {
                data: SomeImportedType;
                config: { nested: boolean };
            }
        "#;

        let result = extract_props(tsx, "Complex").unwrap().unwrap();
        // SomeImportedType is not in the same file → JsonValue
        assert_eq!(result.fields[0].rust_type, RustType::JsonValue);
        // Inline object → doesn't parse as a field (no simple identifier name)
    }

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("initialCount"), "initial_count");
        assert_eq!(camel_to_snake("isEnabled"), "is_enabled");
        assert_eq!(camel_to_snake("HTMLParser"), "html_parser");
        assert_eq!(camel_to_snake("name"), "name");
        assert_eq!(camel_to_snake("onClick"), "on_click");
        assert_eq!(camel_to_snake("defaultOpen"), "default_open");
    }

    #[test]
    fn test_comments_stripped() {
        let tsx = r#"
            interface CounterProps {
                // This is the count
                count: number;
                /* multi
                   line
                   comment */
                label: string;
            }
        "#;

        let result = extract_props(tsx, "Counter").unwrap().unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[0].ts_name, "count");
        assert_eq!(result.fields[1].ts_name, "label");
    }

    #[test]
    fn test_nested_braces_in_interface() {
        let tsx = r#"
            interface OuterProps {
                name: string;
            }

            interface InnerProps {
                value: number;
            }

            function Outer(props: OuterProps) {
                const style = { color: "red" };
                return <div />;
            }
        "#;

        let result = extract_props(tsx, "Outer").unwrap().unwrap();
        assert_eq!(result.fields.len(), 1);
        assert_eq!(result.fields[0].ts_name, "name");
    }

    #[test]
    fn test_no_props_interface() {
        let tsx = r#"
            export default function Logo() {
                return <svg />;
            }
        "#;

        let result = extract_props(tsx, "Logo").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_record_type() {
        let tsx = r#"
            interface ConfigProps {
                settings: Record<string, number>;
            }
        "#;

        let result = extract_props(tsx, "Config").unwrap().unwrap();
        assert_eq!(
            result.fields[0].rust_type,
            RustType::HashMap(Box::new(RustType::String), Box::new(RustType::F64))
        );
    }

    #[test]
    fn test_string_literal_union() {
        let tsx = r#"
            interface ThemeProps {
                variant: "light" | "dark" | "system";
            }
        "#;

        let result = extract_props(tsx, "Theme").unwrap().unwrap();
        assert_eq!(result.fields[0].rust_type, RustType::String);
    }

    #[test]
    fn test_readonly_fields() {
        let tsx = r#"
            interface ReadonlyProps {
                readonly id: string;
                readonly count: number;
            }
        "#;

        let result = extract_props(tsx, "Readonly").unwrap().unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[0].ts_name, "id");
        assert_eq!(result.fields[0].rust_type, RustType::String);
    }

    #[test]
    fn test_named_interface_reference() {
        let tsx = r#"
            interface Address {
                street: string;
                city: string;
            }

            interface UserProps {
                name: string;
                address: Address;
            }
        "#;

        let result = extract_props(tsx, "User").unwrap().unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(
            result.fields[1].rust_type,
            RustType::Named("Address".into())
        );
    }

    #[test]
    fn test_multipart_nullable_union() {
        // string | null | undefined → Option<String>, not JsonValue
        let tsx = r#"
            interface MultiNullableProps {
                label: string | null | undefined;
                count: number | undefined | null;
                flag: boolean | null | undefined;
            }
        "#;

        let result = extract_props(tsx, "MultiNullable").unwrap().unwrap();
        assert_eq!(result.fields.len(), 3);

        assert!(result.fields[0].optional);
        assert_eq!(
            result.fields[0].rust_type,
            RustType::Option(Box::new(RustType::String))
        );
        assert!(result.fields[1].optional);
        assert_eq!(
            result.fields[1].rust_type,
            RustType::Option(Box::new(RustType::F64))
        );
        assert!(result.fields[2].optional);
        assert_eq!(
            result.fields[2].rust_type,
            RustType::Option(Box::new(RustType::Bool))
        );
    }

    #[test]
    fn test_multipart_nonnullable_union_falls_through() {
        // string | number | undefined has two non-nullable parts → still JsonValue
        let tsx = r#"
            interface MixedProps {
                value: string | number | undefined;
            }
        "#;

        let result = extract_props(tsx, "Mixed").unwrap().unwrap();
        assert_eq!(result.fields[0].rust_type, RustType::JsonValue);
    }

    #[test]
    fn test_nested_template_literal_does_not_break_comment_stripping() {
        // Regression test for issue #21: a backtick inside a ${} interpolation
        // must not prematurely close the outer template literal.
        let tsx = r#"
            // This comment should be stripped
            const x = `Hello ${`world`}`;
            interface NestedProps {
                /* also stripped */
                name: string;
            }
        "#;

        let result = extract_props(tsx, "Nested").unwrap().unwrap();
        assert_eq!(result.fields.len(), 1);
        assert_eq!(result.fields[0].ts_name, "name");
        assert_eq!(result.fields[0].rust_type, RustType::String);
    }

    #[test]
    fn test_strip_comments_preserves_template_literal_content() {
        // Ensure comments inside template literals are NOT stripped.
        let src = "const s = `// not a comment`;";
        let stripped = strip_comments(src);
        assert!(stripped.contains("// not a comment"));
    }

    #[test]
    fn test_strip_comments_nested_backtick_interpolation() {
        // The inner backtick pair must not close the outer template literal.
        let src = "const x = `a ${`b`} c`;";
        let stripped = strip_comments(src);
        assert_eq!(stripped, src); // no comments present, output should be identical
    }
}
