// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Standard-library-only generator for the bootstrap `rsjs_web` bindings.

use std::collections::HashSet;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SCHEMA: &str = include_str!("schema/bindings.txt");
const CHECKED_IN: &str = include_str!("src/generated.rs");
const SUPPORTED_SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Any,
    Element,
    HtmlBase,
    Html,
}

struct Element<'a> {
    name: &'a str,
    interface: &'a str,
    kind: ElementKind,
    aliases: Vec<&'a str>,
    capabilities: Vec<&'a str>,
}

struct Target<'a> {
    name: &'a str,
    interface: &'a str,
}

struct Capability<'a> {
    name: &'a str,
    doc: &'a str,
}

struct Tag<'a> {
    name: &'a str,
    element: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperationKind {
    Read,
    Effect,
}

struct Operation<'a> {
    variant: &'a str,
    method: &'a str,
    kind: OperationKind,
    capability: &'a str,
    args: Vec<OperationArgument<'a>>,
    fallback: &'a str,
    origins: Vec<&'a str>,
    aliases: Vec<&'a str>,
}

struct OperationArgument<'a> {
    kind: &'a str,
    name: &'a str,
}

struct Event<'a> {
    name: &'a str,
    doc: &'a str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rsjs_web code generation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (schema_version, capabilities, targets, elements, tags, events, operations) =
        parse_schema()?;
    let generated = format_source(&generate_source(
        schema_version,
        &capabilities,
        &targets,
        &elements,
        &tags,
        &events,
        &operations,
    ))?;
    let command = env::args().nth(1).unwrap_or_else(|| "--check".to_owned());

    match command.as_str() {
        "--check" => {
            if generated != CHECKED_IN {
                return Err(format!(
                    "src/generated.rs is stale; run `{}` --write",
                    env::args()
                        .next()
                        .unwrap_or_else(|| "rsjs-web-codegen".to_owned())
                ));
            }
        }
        "--write" => fs::write(generated_path(), generated)
            .map_err(|error| format!("could not write src/generated.rs: {error}"))?,
        "--stdout" => print!("{generated}"),
        other => {
            return Err(format!(
                "unknown argument `{other}`; expected --check, --write, or --stdout"
            ));
        }
    }

    Ok(())
}

fn format_source(source: &str) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start rustfmt for generated bindings: {error}"))?;

    child
        .stdin
        .take()
        .expect("piped rustfmt stdin must be available")
        .write_all(source.as_bytes())
        .map_err(|error| format!("could not send generated bindings to rustfmt: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not wait for rustfmt: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt rejected generated bindings: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("rustfmt returned non-UTF-8 generated bindings: {error}"))
}

fn generated_path() -> PathBuf {
    Path::new(file!())
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("src/generated.rs")
}

type ParsedSchema = (
    u32,
    Vec<Capability<'static>>,
    Vec<Target<'static>>,
    Vec<Element<'static>>,
    Vec<Tag<'static>>,
    Vec<Event<'static>>,
    Vec<Operation<'static>>,
);

fn parse_schema() -> Result<ParsedSchema, String> {
    let mut schema_version = None;
    let mut capabilities = Vec::new();
    let mut targets = Vec::new();
    let mut elements = Vec::new();
    let mut tags = Vec::new();
    let mut events = Vec::new();
    let mut operations = Vec::new();
    let mut rust_names = HashSet::new();
    let mut operation_variants = HashSet::new();
    let mut operation_methods = HashSet::new();
    let mut tag_names = HashSet::new();

    for (index, raw_line) in SCHEMA.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields = line.split('|').map(str::trim).collect::<Vec<_>>();
        match fields.first().copied() {
            Some("schema") if fields.len() == 2 => {
                if schema_version.is_some() {
                    return Err(format!("line {line_number}: duplicate schema version row"));
                }
                let parsed = fields[1].parse::<u32>().map_err(|error| {
                    format!("line {line_number}: invalid schema version: {error}")
                })?;
                if parsed != SUPPORTED_SCHEMA_VERSION {
                    return Err(format!(
                        "line {line_number}: unsupported schema version {parsed}; expected {SUPPORTED_SCHEMA_VERSION}"
                    ));
                }
                schema_version = Some(parsed);
            }
            Some("capability") if fields.len() == 3 => {
                let name = fields[1];
                validate_ident(name, line_number)?;
                insert_unique(&mut rust_names, name, line_number)?;
                if fields[2].is_empty() {
                    return Err(format!(
                        "line {line_number}: capability documentation cannot be empty"
                    ));
                }
                capabilities.push(Capability {
                    name,
                    doc: fields[2],
                });
            }
            Some("target") if fields.len() == 3 => {
                let name = fields[1];
                validate_ident(name, line_number)?;
                insert_unique(&mut rust_names, name, line_number)?;
                if fields[2].is_empty() {
                    return Err(format!(
                        "line {line_number}: target browser interface cannot be empty"
                    ));
                }
                targets.push(Target {
                    name,
                    interface: fields[2],
                });
            }
            Some("element") if fields.len() == 6 => {
                let name = fields[1];
                validate_ident(name, line_number)?;
                insert_unique(&mut rust_names, name, line_number)?;

                let kind = match fields[3] {
                    "any" => ElementKind::Any,
                    "element" => ElementKind::Element,
                    "html-base" => ElementKind::HtmlBase,
                    "html" => ElementKind::Html,
                    other => {
                        return Err(format!(
                            "line {line_number}: unsupported element kind `{other}`"
                        ));
                    }
                };

                let aliases = comma_list(fields[4]);
                for alias in &aliases {
                    validate_ident(alias, line_number)?;
                    insert_unique(&mut rust_names, alias, line_number)?;
                }

                let element_capabilities = comma_list(fields[5]);

                elements.push(Element {
                    name,
                    interface: fields[2],
                    kind,
                    aliases,
                    capabilities: element_capabilities,
                });
            }
            Some("tag") if fields.len() == 3 => {
                let name = fields[1];
                validate_tag_name(name, line_number)?;
                if !tag_names.insert(name) {
                    return Err(format!("line {line_number}: duplicate HTML tag `{name}`"));
                }
                let element = fields[2];
                validate_ident(element, line_number)?;
                tags.push(Tag { name, element });
            }
            Some("operation") if fields.len() == 9 => {
                let variant = fields[1];
                let method = fields[2];
                validate_ident(variant, line_number)?;
                validate_ident(method, line_number)?;
                insert_unique(&mut operation_variants, variant, line_number)?;
                insert_unique(&mut operation_methods, method, line_number)?;

                let kind = match fields[3] {
                    "read" => OperationKind::Read,
                    "effect" => OperationKind::Effect,
                    other => {
                        return Err(format!(
                            "line {line_number}: unsupported operation kind `{other}`"
                        ));
                    }
                };
                let mut argument_names = HashSet::new();
                let args = comma_list(fields[5])
                    .into_iter()
                    .map(|declaration| {
                        let (kind, name) = declaration.split_once(':').ok_or_else(|| {
                            format!(
                                "line {line_number}: operation argument `{declaration}` must be declared as `kind:name`"
                            )
                        })?;
                        validate_ident(name, line_number)?;
                        if !argument_names.insert(name) {
                            return Err(format!(
                                "line {line_number}: duplicate operation argument name `{name}`"
                            ));
                        }
                        Ok(OperationArgument { kind, name })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                for arg in &args {
                    if !matches!(
                        arg.kind,
                        "reactive" | "static_attribute" | "static_style_property"
                    ) {
                        return Err(format!(
                            "line {line_number}: unsupported operation argument kind `{}`",
                            arg.kind
                        ));
                    }
                }
                if !matches!(
                    fields[6],
                    "unit"
                        | "string_empty"
                        | "bool_false"
                        | "bool_true"
                        | "i32_zero"
                        | "u32_zero"
                        | "f64_zero"
                ) {
                    return Err(format!(
                        "line {line_number}: unsupported operation fallback `{}`",
                        fields[6]
                    ));
                }
                if kind == OperationKind::Effect && fields[6] != "unit" {
                    return Err(format!(
                        "line {line_number}: effect operations must use the `unit` fallback"
                    ));
                }
                if kind == OperationKind::Read && fields[6] == "unit" {
                    return Err(format!(
                        "line {line_number}: read operations must return a scalar fallback"
                    ));
                }

                let origins = comma_list(fields[7]);
                if origins.is_empty() {
                    return Err(format!(
                        "line {line_number}: operation must support at least one handle origin"
                    ));
                }
                for origin in &origins {
                    if !matches!(*origin, "node_ref" | "current_target" | "element_ref") {
                        return Err(format!(
                            "line {line_number}: unsupported handle origin `{origin}`"
                        ));
                    }
                }
                if kind == OperationKind::Effect && origins.contains(&"current_target") {
                    return Err(format!(
                        "line {line_number}: current_target effects are not supported in schema v5"
                    ));
                }

                let aliases = comma_list(fields[8]);
                for alias in &aliases {
                    validate_ident(alias, line_number)?;
                    insert_unique(&mut operation_methods, alias, line_number)?;
                }
                operations.push(Operation {
                    variant,
                    method,
                    kind,
                    capability: fields[4],
                    args,
                    fallback: fields[6],
                    origins,
                    aliases,
                });
            }
            Some("event") if fields.len() == 3 => {
                let name = fields[1];
                validate_ident(name, line_number)?;
                insert_unique(&mut rust_names, name, line_number)?;
                events.push(Event {
                    name,
                    doc: fields[2],
                });
            }
            Some(kind) => {
                return Err(format!(
                    "line {line_number}: malformed `{kind}` row with {} fields",
                    fields.len()
                ));
            }
            None => unreachable!("an empty line was filtered above"),
        }
    }

    if capabilities.is_empty()
        || targets.is_empty()
        || elements.is_empty()
        || tags.is_empty()
        || events.is_empty()
        || operations.is_empty()
    {
        return Err(
            "schema must contain at least one capability, target, element, tag, event, and operation"
                .to_owned(),
        );
    }
    if events.first().map(|event| event.name) != Some("Event") {
        return Err("the first event row must be the universal `Event` handle".to_owned());
    }

    let schema_version =
        schema_version.ok_or_else(|| "schema must declare `schema|<version>`".to_owned())?;
    let capability_names = capabilities
        .iter()
        .map(|capability| capability.name)
        .collect::<HashSet<_>>();
    let element_names = elements
        .iter()
        .map(|element| element.name)
        .collect::<HashSet<_>>();
    for element in &elements {
        for capability in &element.capabilities {
            if !capability_names.contains(capability) {
                return Err(format!(
                    "element `{}` references unknown capability `{capability}`",
                    element.name
                ));
            }
        }
    }
    for operation in &operations {
        if !capability_names.contains(operation.capability) {
            return Err(format!(
                "operation `{}` references unknown capability `{}`",
                operation.method, operation.capability
            ));
        }
    }
    for tag in &tags {
        if !element_names.contains(tag.element) {
            return Err(format!(
                "HTML tag `{}` references unknown element marker `{}`",
                tag.name, tag.element
            ));
        }
    }
    if !capability_names.contains("HasElementApi") {
        return Err("schema must declare the compatibility capability `HasElementApi`".to_owned());
    }
    Ok((
        schema_version,
        capabilities,
        targets,
        elements,
        tags,
        events,
        operations,
    ))
}

fn comma_list(value: &'static str) -> Vec<&'static str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn validate_ident(value: &str, line_number: usize) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(format!("line {line_number}: Rust name cannot be empty"));
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || chars.any(|character| !(character == '_' || character.is_ascii_alphanumeric()))
    {
        return Err(format!(
            "line {line_number}: `{value}` is not a simple Rust identifier"
        ));
    }
    Ok(())
}

fn validate_tag_name(value: &str, line_number: usize) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(format!("line {line_number}: HTML tag cannot be empty"));
    };
    if !first.is_ascii_alphabetic()
        || chars.any(|character| {
            !(character.is_ascii_alphanumeric() || character == '-' || character == '_')
        })
    {
        return Err(format!(
            "line {line_number}: `{value}` is not a simple markup tag"
        ));
    }
    Ok(())
}

fn insert_unique(
    names: &mut HashSet<&'static str>,
    value: &'static str,
    line_number: usize,
) -> Result<(), String> {
    if names.insert(value) {
        Ok(())
    } else {
        Err(format!(
            "line {line_number}: duplicate generated Rust name `{value}`"
        ))
    }
}

fn generate_source(
    schema_version: u32,
    capabilities: &[Capability<'_>],
    targets: &[Target<'_>],
    elements: &[Element<'_>],
    tags: &[Tag<'_>],
    events: &[Event<'_>],
    operations: &[Operation<'_>],
) -> String {
    let mut output = String::new();
    output.push_str("// Copyright Proa Labs, Inc.\n");
    output.push_str("// SPDX-License-Identifier: Apache-2.0\n\n");
    output.push_str("// @generated by ../codegen.rs from ../schema/bindings.txt; do not edit.\n\n");
    writeln!(
        output,
        "/// Version of the normalized RSJS browser binding schema.\npub const BINDING_SCHEMA_VERSION: u32 = {schema_version};\n"
    )
    .unwrap();
    render_dom(&mut output, capabilities, targets, elements);
    render_tags(&mut output, elements, tags);
    render_operations(&mut output, capabilities, operations);
    render_events(&mut output, targets, elements, events);
    output
}

fn render_dom(
    output: &mut String,
    capabilities: &[Capability<'_>],
    targets: &[Target<'_>],
    elements: &[Element<'_>],
) {
    output.push_str("/// Closed DOM marker hierarchy and element capabilities.\n");
    output.push_str("pub mod dom {\n");
    output.push_str("    mod private {\n");
    output.push_str("        pub trait Sealed {}\n");
    output.push_str("    }\n\n");
    output.push_str("    /// Marker implemented by compiler-known browser event targets.\n");
    output.push_str("    pub trait DomTargetMarker: private::Sealed + Copy + 'static {\n");
    output.push_str("        /// Browser interface spelling consumed by binding generation.\n");
    output.push_str("        const INTERFACE: &'static str;\n");
    output.push_str("    }\n\n");
    output.push_str("    /// Marker implemented by compiler-known DOM nodes.\n");
    output.push_str("    pub trait NodeMarker: DomTargetMarker {}\n\n");
    output.push_str("    /// Marker implemented by compiler-known DOM elements.\n");
    output.push_str("    pub trait ElementMarker: NodeMarker {}\n\n");
    output.push_str("    /// Marker implemented by HTML element interfaces.\n");
    output.push_str("    pub trait HtmlElementMarker: ElementMarker {}\n\n");
    for capability in capabilities {
        writeln!(output, "    #[doc = {:?}]", capability.doc).unwrap();
        writeln!(
            output,
            "    pub trait {}: ElementMarker {{}}\n",
            capability.name
        )
        .unwrap();
    }
    for target in targets {
        writeln!(
            output,
            "    #[doc = {:?}]",
            format!(
                "Marker for the browser `{}` event target.",
                target.interface
            )
        )
        .unwrap();
        output.push_str("    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]\n");
        writeln!(output, "    pub struct {};", target.name).unwrap();
        writeln!(output, "    impl private::Sealed for {} {{}}", target.name).unwrap();
        writeln!(output, "    impl DomTargetMarker for {} {{", target.name).unwrap();
        writeln!(
            output,
            "        const INTERFACE: &'static str = {:?};",
            target.interface
        )
        .unwrap();
        output.push_str("    }\n\n");
    }
    for element in elements {
        let marker_doc = if element.kind == ElementKind::Any {
            "Broad element marker used when an exact element interface is intentionally unspecified. It cannot represent Document or Window event targets."
                .to_owned()
        } else {
            format!("Marker for the browser `{}` interface.", element.interface)
        };
        writeln!(output, "    #[doc = {marker_doc:?}]").unwrap();
        output.push_str("    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]\n");
        writeln!(output, "    pub struct {};", element.name).unwrap();
        writeln!(output, "    impl private::Sealed for {} {{}}", element.name).unwrap();
        writeln!(output, "    impl DomTargetMarker for {} {{", element.name).unwrap();
        writeln!(
            output,
            "        const INTERFACE: &'static str = {:?};",
            element.interface
        )
        .unwrap();
        output.push_str("    }\n");
        writeln!(output, "    impl NodeMarker for {} {{}}", element.name).unwrap();
        writeln!(output, "    impl ElementMarker for {} {{}}", element.name).unwrap();
        if matches!(element.kind, ElementKind::HtmlBase | ElementKind::Html) {
            writeln!(
                output,
                "    impl HtmlElementMarker for {} {{}}",
                element.name
            )
            .unwrap();
        }
        for capability in &element.capabilities {
            writeln!(output, "    impl {capability} for {} {{}}", element.name).unwrap();
        }
        output.push('\n');
    }

    for element in elements {
        for alias in &element.aliases {
            writeln!(
                output,
                "    /// Compatibility alias for [`{}`].",
                element.name
            )
            .unwrap();
            writeln!(output, "    pub type {alias} = {};", element.name).unwrap();
        }
    }
    output.push_str("}\n\n");
}

// proa-perf-ok(scope): this generates Rust source text, not rendered HTML
fn render_tags(output: &mut String, elements: &[Element<'_>], tags: &[Tag<'_>]) {
    output.push_str("/// Generated identity of one canonical DOM element marker.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("#[non_exhaustive]\n");
    output.push_str("pub enum DomElementKind {\n");
    for element in elements {
        writeln!(output, "    {},", element.name).unwrap();
    }
    output.push_str("}\n\n");
    output.push_str("impl DomElementKind {\n");
    output.push_str("    /// Rust marker name generated in [`dom`].\n");
    output.push_str("    pub const fn as_rust_marker_name(self) -> &'static str {\n");
    output.push_str("        match self {\n");
    for element in elements {
        writeln!(output, "            Self::{0} => {0:?},", element.name).unwrap();
    }
    output.push_str("        }\n");
    output.push_str("    }\n\n");
    output.push_str("    /// Browser interface spelling for this marker.\n");
    output.push_str("    pub const fn interface(self) -> &'static str {\n");
    output.push_str("        match self {\n");
    for element in elements {
        writeln!(
            output,
            "            Self::{} => {:?},",
            element.name, element.interface
        )
        .unwrap();
    }
    output.push_str("        }\n");
    output.push_str("    }\n");
    output.push_str("}\n\n");

    output.push_str("/// Exact HTML-tag mapping shared by compiler binding witnesses.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("pub struct DomTagSpec {\n");
    output.push_str("    pub tag: &'static str,\n");
    output.push_str("    pub element: DomElementKind,\n");
    output.push_str("}\n\n");
    output.push_str("/// Generated exact HTML-tag registry.\n");
    output.push_str("pub static DOM_TAGS: &[DomTagSpec] = &[\n");
    for tag in tags {
        writeln!(
            output,
            "    DomTagSpec {{ tag: {:?}, element: DomElementKind::{} }},",
            tag.name, tag.element
        )
        .unwrap();
    }
    output.push_str("];\n\n");
    output.push_str("/// Find an exact generated element marker for an HTML tag.\n");
    output.push_str("pub fn find_dom_tag(tag: &str) -> Option<&'static DomTagSpec> {\n");
    output.push_str("    DOM_TAGS.iter().find(|spec| spec.tag.eq_ignore_ascii_case(tag))\n");
    output.push_str("}\n\n");
}

// proa-perf-ok(scope): this generates Rust source text, not rendered HTML
fn render_operations(
    output: &mut String,
    capabilities: &[Capability<'_>],
    operations: &[Operation<'_>],
) {
    output.push_str("/// Capability family required by a generated DOM operation.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("#[non_exhaustive]\n");
    output.push_str("pub enum DomCapability {\n");
    for capability in capabilities {
        writeln!(output, "    {},", capability.name).unwrap();
    }
    output.push_str("}\n\n");

    output.push_str("/// Stable identity of a generated DOM operation.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("#[non_exhaustive]\n");
    output.push_str("pub enum DomOperation {\n");
    for operation in operations {
        writeln!(output, "    {},", operation.variant).unwrap();
    }
    output.push_str("}\n\n");

    output.push_str("/// Whether a generated DOM operation produces a value or an effect.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("pub enum DomOperationKind { Read, Effect }\n\n");
    output.push_str("/// How one generated DOM operation argument is checked and lowered.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str(
        "pub enum DomArgumentKind { Reactive, StaticAttribute, StaticStyleProperty }\n\n",
    );
    output.push_str("/// One named argument in a generated DOM operation signature.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("pub struct DomArgumentSpec {\n");
    output.push_str("    pub name: &'static str,\n");
    output.push_str("    pub kind: DomArgumentKind,\n");
    output.push_str("}\n\n");
    output.push_str("/// Typed fallback returned when a compiler handle executes on the server.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("pub enum DomFallback { Unit, StringEmpty, BoolFalse, BoolTrue, I32Zero, U32Zero, F64Zero }\n\n");
    output.push_str("/// Compiler-handle origin on which a DOM operation is exposed.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("pub enum DomHandleOrigin { NodeRef, CurrentTarget, ElementRef }\n\n");

    output.push_str("/// One generated, reviewable DOM operation declaration.\n");
    output.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
    output.push_str("pub struct DomOperationSpec {\n");
    output.push_str("    pub operation: DomOperation,\n");
    output.push_str("    pub method: &'static str,\n");
    output.push_str("    pub aliases: &'static [&'static str],\n");
    output.push_str("    pub kind: DomOperationKind,\n");
    output.push_str("    pub capability: DomCapability,\n");
    output.push_str("    pub args: &'static [DomArgumentSpec],\n");
    output.push_str("    pub fallback: DomFallback,\n");
    output.push_str("    pub origins: &'static [DomHandleOrigin],\n");
    output.push_str("}\n\n");
    output.push_str("impl DomOperationSpec {\n");
    output.push_str("    /// Whether this operation is exposed on the requested handle origin.\n");
    output.push_str("    pub fn supports(&self, origin: DomHandleOrigin) -> bool {\n");
    output.push_str("        self.origins.contains(&origin)\n");
    output.push_str("    }\n\n");
    output.push_str("    /// Whether `method` is the canonical name or a compatibility alias.\n");
    output.push_str("    pub fn matches_method(&self, method: &str) -> bool {\n");
    output.push_str("        self.method == method || self.aliases.contains(&method)\n");
    output.push_str("    }\n");
    output.push_str("}\n\n");

    output.push_str("/// Generated DOM operation registry.\n");
    output.push_str("pub static DOM_OPERATIONS: &[DomOperationSpec] = &[\n");
    for operation in operations {
        writeln!(output, "    DomOperationSpec {{").unwrap();
        writeln!(
            output,
            "        operation: DomOperation::{},",
            operation.variant
        )
        .unwrap();
        writeln!(output, "        method: {:?},", operation.method).unwrap();
        output.push_str("        aliases: &[");
        for (index, alias) in operation.aliases.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            write!(output, "{alias:?}").unwrap();
        }
        output.push_str("],\n");
        writeln!(
            output,
            "        kind: DomOperationKind::{},",
            operation_kind_variant(operation.kind)
        )
        .unwrap();
        writeln!(
            output,
            "        capability: DomCapability::{},",
            operation.capability
        )
        .unwrap();
        output.push_str("        args: &[");
        for (index, arg) in operation.args.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            write!(
                output,
                "DomArgumentSpec {{ name: {:?}, kind: DomArgumentKind::{} }}",
                arg.name,
                argument_kind_variant(arg.kind)
            )
            .unwrap();
        }
        output.push_str("],\n");
        writeln!(
            output,
            "        fallback: DomFallback::{},",
            fallback_variant(operation.fallback)
        )
        .unwrap();
        output.push_str("        origins: &[");
        for (index, origin) in operation.origins.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            write!(output, "DomHandleOrigin::{}", origin_variant(origin)).unwrap();
        }
        output.push_str("],\n");
        output.push_str("    },\n");
    }
    output.push_str("];\n\n");
    output.push_str("/// Find a generated DOM operation by its Rust method name or alias.\n");
    output.push_str(
        "pub fn find_dom_operation(method: &str) -> Option<&'static DomOperationSpec> {\n",
    );
    output.push_str("    DOM_OPERATIONS.iter().find(|spec| spec.matches_method(method))\n");
    output.push_str("}\n\n");
    output.push_str("impl DomOperation {\n");
    output.push_str("    /// Return the generated declaration for this stable operation.\n");
    output.push_str("    pub fn spec(self) -> &'static DomOperationSpec {\n");
    output.push_str("        DOM_OPERATIONS.iter().find(|spec| spec.operation == self).expect(\"every generated DOM operation has one registry entry\")\n");
    output.push_str("    }\n");
    output.push_str("}\n\n");

    render_handle_macro(
        output,
        "__impl_dom_node_ref_methods",
        "node_ref",
        capabilities,
        operations,
    );
    render_handle_macro(
        output,
        "__impl_dom_current_target_methods",
        "current_target",
        capabilities,
        operations,
    );
    render_handle_macro(
        output,
        "__impl_dom_element_ref_methods",
        "element_ref",
        capabilities,
        operations,
    );
}

// proa-perf-ok(scope): this generates Rust source text, not rendered HTML
fn render_handle_macro(
    output: &mut String,
    macro_name: &str,
    origin: &str,
    capabilities: &[Capability<'_>],
    operations: &[Operation<'_>],
) {
    output.push_str("/// Implement the generated DOM operation surface for a compiler handle.\n");
    output.push_str("#[doc(hidden)]\n");
    output.push_str("#[macro_export]\n");
    writeln!(output, "macro_rules! {macro_name} {{").unwrap();
    output.push_str("    ($handle:ident) => {\n");
    for capability in capabilities {
        let matching = operations
            .iter()
            .filter(|operation| {
                operation.capability == capability.name && operation.origins.contains(&origin)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        writeln!(
            output,
            "        impl<E: $crate::dom::{}> $handle<E> {{",
            capability.name
        )
        .unwrap();
        for operation in matching {
            render_stub_method(output, operation, operation.method);
            for alias in &operation.aliases {
                render_stub_method(output, operation, alias);
            }
        }
        output.push_str("        }\n");
    }
    output.push_str("    };\n");
    output.push_str("}\n\n");
}

fn render_stub_method(output: &mut String, operation: &Operation<'_>, method: &str) {
    writeln!(
        output,
        "            #[doc = {:?}]",
        format!("Compiler-lowered DOM operation `{method}`.")
    )
    .unwrap();
    output.push_str("            #[inline(always)]\n");
    write!(output, "            pub fn {method}").unwrap();

    let reactive_generics = operation
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| (arg.kind == "reactive").then_some(format!("A{index}")))
        .collect::<Vec<_>>();
    if !reactive_generics.is_empty() {
        write!(output, "<{}>", reactive_generics.join(", ")).unwrap();
    }
    output.push_str("(&self");
    for (index, arg) in operation.args.iter().enumerate() {
        let ty = match arg.kind {
            "reactive" => format!("A{index}"),
            "static_attribute" | "static_style_property" => "&'static str".to_owned(),
            _ => unreachable!("operation argument was validated while parsing"),
        };
        write!(output, ", _arg{index}: {ty}").unwrap();
    }
    output.push(')');
    let (return_type, body) = fallback_stub(operation.fallback);
    if let Some(return_type) = return_type {
        write!(output, " -> {return_type}").unwrap();
    }
    writeln!(output, " {{ {body} }}").unwrap();
}

fn operation_kind_variant(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Read => "Read",
        OperationKind::Effect => "Effect",
    }
}

fn argument_kind_variant(kind: &str) -> &'static str {
    match kind {
        "reactive" => "Reactive",
        "static_attribute" => "StaticAttribute",
        "static_style_property" => "StaticStyleProperty",
        _ => unreachable!("operation argument was validated while parsing"),
    }
}

fn fallback_variant(fallback: &str) -> &'static str {
    match fallback {
        "unit" => "Unit",
        "string_empty" => "StringEmpty",
        "bool_false" => "BoolFalse",
        "bool_true" => "BoolTrue",
        "i32_zero" => "I32Zero",
        "u32_zero" => "U32Zero",
        "f64_zero" => "F64Zero",
        _ => unreachable!("operation fallback was validated while parsing"),
    }
}

fn origin_variant(origin: &str) -> &'static str {
    match origin {
        "node_ref" => "NodeRef",
        "current_target" => "CurrentTarget",
        "element_ref" => "ElementRef",
        _ => unreachable!("operation origin was validated while parsing"),
    }
}

fn fallback_stub(fallback: &str) -> (Option<&'static str>, &'static str) {
    match fallback {
        "unit" => (None, ""),
        "string_empty" => (Some("String"), "String::new()"),
        "bool_false" => (Some("bool"), "false"),
        "bool_true" => (Some("bool"), "true"),
        "i32_zero" => (Some("i32"), "0"),
        "u32_zero" => (Some("u32"), "0"),
        "f64_zero" => (Some("f64"), "0.0"),
        _ => unreachable!("operation fallback was validated while parsing"),
    }
}

// proa-perf-ok(scope): this generates Rust source text, not rendered HTML
fn render_events(
    output: &mut String,
    targets: &[Target<'_>],
    elements: &[Element<'_>],
    events: &[Event<'_>],
) {
    output.push_str("/// Closed event-interface families used by compiler binding witnesses.\n");
    output.push_str("pub mod event_interface {\n");
    output.push_str("    mod private {\n");
    output.push_str("        pub trait Sealed {}\n");
    output.push_str("    }\n\n");
    output.push_str("    /// Marker implemented by compiler-known event interface families.\n");
    output.push_str("    pub trait Interface: private::Sealed + Copy + 'static {\n");
    output.push_str("        /// Stable family name shared with the RSJS compiler registry.\n");
    output.push_str("        const NAME: &'static str;\n");
    output.push_str("    }\n\n");

    for event in events {
        writeln!(output, "    #[doc = {:?}]", event.doc).unwrap();
        output.push_str("    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]\n");
        writeln!(output, "    pub struct {};", event.name).unwrap();
        writeln!(output, "    impl private::Sealed for {} {{}}", event.name).unwrap();
        writeln!(output, "    impl Interface for {} {{", event.name).unwrap();
        writeln!(
            output,
            "        const NAME: &'static str = {:?};",
            event.name
        )
        .unwrap();
        output.push_str("    }\n\n");
    }
    output.pop();
    output.push_str("}\n\n");

    output.push_str("/// Event-interface family selected by the RSJS event registry.\n");
    output.push_str("///\n");
    output
        .push_str("/// This generated enum lets compiler crates consume the same vocabulary as\n");
    output.push_str("/// the Rust witness marker module above.\n");
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    output.push_str("#[non_exhaustive]\n");
    output.push_str("pub enum EventInterface {\n");
    for event in events {
        writeln!(output, "    {},", event.name).unwrap();
    }
    output.push_str("}\n\n");
    output.push_str("impl EventInterface {\n");
    output.push_str("    /// Rust marker name in [`event_interface`].\n");
    output.push_str("    pub const fn as_rust_marker_name(self) -> &'static str {\n");
    output.push_str("        match self {\n");
    for event in events {
        writeln!(output, "            Self::{0} => {0:?},", event.name).unwrap();
    }
    output.push_str("        }\n");
    output.push_str("    }\n");
    output.push_str("}\n\n");

    output.push_str("/// Bridge implemented by the canonical event handles owned by `rsjs`.\n");
    output.push_str("///\n");
    output
        .push_str("/// `rsjs_web` owns interface vocabulary but deliberately does not define a\n");
    output.push_str("/// second public event-handle family.\n");
    output.push_str("pub trait EventParam {\n");
    output.push_str(
        "    /// DOM target represented by this event's compiler-selected current target.\n",
    );
    output.push_str("    type CurrentTarget: dom::DomTargetMarker;\n");
    output.push_str("}\n\n");

    output.push_str(
        "/// Bridge implemented when an `rsjs` event handle matches an interface family.\n",
    );
    output.push_str("pub trait EventFor<Interface>: EventParam\n");
    output.push_str("where\n");
    output.push_str("    Interface: event_interface::Interface,\n");
    output.push_str("{\n");
    output.push_str("}\n\n");

    output.push_str(
        "/// Implemented when an authored element marker can represent a concrete element.\n",
    );
    output.push_str("#[diagnostic::on_unimplemented(\n");
    output.push_str(
        "    message = \"element reference marker `{Self}` does not match binding target `{Target}`\",\n",
    );
    output
        .push_str("    label = \"this node-ref marker is incompatible with the `ref` element\",\n");
    output.push_str(
        "    note = \"parameterize `node_ref` with the bound element marker, or use `dom::Element` or `dom::HtmlElement` for an intentionally broader element contract\"\n",
    );
    output.push_str(")]\n");
    output.push_str("pub trait ElementTargetFor<Target>: dom::ElementMarker\n");
    output.push_str("where\n");
    output.push_str("    Target: dom::ElementMarker,\n");
    output.push_str("{\n");
    output.push_str("}\n\n");
    output.push_str("impl<Target: dom::ElementMarker> ElementTargetFor<Target> for dom::Any {}\n");
    output.push_str(
        "impl<Target: dom::ElementMarker> ElementTargetFor<Target> for dom::Element {}\n",
    );
    output.push_str(
        "impl<Target: dom::HtmlElementMarker> ElementTargetFor<Target> for dom::HtmlElement {}\n",
    );

    for element in elements
        .iter()
        .filter(|element| element.kind == ElementKind::Html)
    {
        writeln!(
            output,
            "impl ElementTargetFor<dom::{0}> for dom::{0} {{}}",
            element.name
        )
        .unwrap();
    }
    output.push_str(
        "\n/// Implemented when an authored current-target marker can represent a concrete DOM target.\n",
    );
    output.push_str("#[diagnostic::on_unimplemented(\n");
    output.push_str(
        "    message = \"event current-target marker `{Self}` does not match binding target `{Target}`\",\n",
    );
    output.push_str(
        "    label = \"this event target marker is incompatible with the `on_*` binding\",\n",
    );
    output.push_str(
        "    note = \"parameterize the event type with `rsjs::web::dom::Document` for `on_document_*`, `rsjs::web::dom::Window` for `on_window_*`, or the bound element marker for a local handler; `dom::Any` is element-local only\"\n",
    );
    output.push_str(")]\n");
    output.push_str("pub trait EventCurrentTargetFor<Target>: dom::DomTargetMarker\n");
    output.push_str("where\n");
    output.push_str("    Target: dom::DomTargetMarker,\n");
    output.push_str("{\n");
    output.push_str("}\n\n");
    // The DOM marker hierarchy is sealed, so emit the broad assignability
    // relations as an explicit matrix. Besides making the generated policy
    // auditable, this lets rustc report the `EventCurrentTargetFor` diagnostic
    // directly instead of selecting a failed inner `ElementMarker` obligation.
    for target in elements {
        writeln!(
            output,
            "impl EventCurrentTargetFor<dom::{0}> for dom::Any {{}}",
            target.name
        )
        .unwrap();
    }
    for target in elements {
        writeln!(
            output,
            "impl EventCurrentTargetFor<dom::{0}> for dom::Element {{}}",
            target.name
        )
        .unwrap();
    }
    for target in elements
        .iter()
        .filter(|element| matches!(element.kind, ElementKind::HtmlBase | ElementKind::Html))
    {
        writeln!(
            output,
            "impl EventCurrentTargetFor<dom::{0}> for dom::HtmlElement {{}}",
            target.name
        )
        .unwrap();
    }
    for element in elements
        .iter()
        .filter(|element| element.kind == ElementKind::Html)
    {
        writeln!(
            output,
            "impl EventCurrentTargetFor<dom::{0}> for dom::{0} {{}}",
            element.name
        )
        .unwrap();
    }
    for target in targets {
        writeln!(
            output,
            "impl EventCurrentTargetFor<dom::{0}> for dom::{0} {{}}",
            target.name
        )
        .unwrap();
    }
    output.push('\n');

    output.push_str("/// Rustc witness for a DOM event's family and concrete binding target.\n");
    output.push_str("#[doc(hidden)]\n");
    output.push_str("pub fn assert_event_type<Target, Interface, E>()\n");
    output.push_str("where\n");
    output.push_str("    Target: dom::DomTargetMarker,\n");
    output.push_str("    Interface: event_interface::Interface,\n");
    output.push_str("    E: EventFor<Interface>,\n");
    output.push_str("    E::CurrentTarget: EventCurrentTargetFor<Target>,\n");
    output.push_str("{\n");
    output.push_str("}\n");
}
