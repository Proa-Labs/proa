// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;
use std::str::FromStr;
use std::sync::OnceLock;

use syntect::easy::ScopeRegionIterator;
use syntect::highlighting::ScopeSelectors;
use syntect::parsing::{MatchPower, ParseState, Scope, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;

mod markdown_template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HighlightClass {
    Keyword,
    Operator,
    String,
    StringSpecial,
    Comment,
    Function,
    FunctionBuiltin,
    FunctionMacro,
    Type,
    TypeBuiltin,
    Constant,
    ConstantBuiltin,
    Number,
    Attribute,
    VariableBuiltin,
    Constructor,
    Tag,
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    Variable,
    VariableParameter,
    Property,
    Label,
}

impl HighlightClass {
    const fn css_class(self) -> &'static str {
        match self {
            Self::Keyword => "hl-keyword",
            Self::Operator => "hl-operator",
            Self::String => "hl-string",
            Self::StringSpecial => "hl-string-special",
            Self::Comment => "hl-comment",
            Self::Function => "hl-function",
            Self::FunctionBuiltin => "hl-function-builtin",
            Self::FunctionMacro => "hl-function-macro",
            Self::Type => "hl-type",
            Self::TypeBuiltin => "hl-type-builtin",
            Self::Constant => "hl-constant",
            Self::ConstantBuiltin => "hl-constant-builtin",
            Self::Number => "hl-number",
            Self::Attribute => "hl-attribute",
            Self::VariableBuiltin => "hl-variable-builtin",
            Self::Constructor => "hl-constructor",
            Self::Tag => "hl-tag",
            Self::Punctuation => "hl-punctuation",
            Self::PunctuationBracket => "hl-punctuation-bracket",
            Self::PunctuationDelimiter => "hl-punctuation-delimiter",
            Self::Variable => "hl-variable",
            Self::VariableParameter => "hl-variable-parameter",
            Self::Property => "hl-property",
            Self::Label => "hl-label",
        }
    }
}

struct Rule {
    selectors: ScopeSelectors,
    class: HighlightClass,
}

impl Rule {
    fn new(selectors: &str, class: HighlightClass) -> Self {
        Self {
            selectors: ScopeSelectors::from_str(selectors)
                .expect("semantic highlight selectors must be valid"),
            class,
        }
    }
}

struct SemanticHighlighter {
    syntaxes: SyntaxSet,
    rules: Vec<Rule>,
}

impl SemanticHighlighter {
    fn new() -> Self {
        use HighlightClass as Class;

        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            // More-specific selector matches win. The order is the stable
            // tiebreaker, so semantic overrides precede generic categories.
            rules: vec![
                Rule::new(
                    "comment, punctuation.definition.comment",
                    Class::Comment,
                ),
                Rule::new(
                    "constant.character.escape, constant.character.entity, constant.other.placeholder, punctuation.definition.interpolation, punctuation.section.interpolation, meta.interpolation punctuation",
                    Class::StringSpecial,
                ),
                Rule::new(
                    "string, meta.string, punctuation.definition.string, storage.type.string",
                    Class::String,
                ),
                Rule::new(
                    "support.macro, entity.name.macro, entity.name.function.macro, entity.name.function.preprocessor, meta.preprocessor entity.name.function",
                    Class::FunctionMacro,
                ),
                Rule::new("support.function", Class::FunctionBuiltin),
                Rule::new("entity.name.function.constructor", Class::Constructor),
                Rule::new("entity.name.function, variable.function", Class::Function),
                Rule::new("support.type", Class::TypeBuiltin),
                Rule::new(
                    "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.impl",
                    Class::Type,
                ),
                Rule::new("constant.numeric", Class::Number),
                Rule::new(
                    "constant.language, support.constant",
                    Class::ConstantBuiltin,
                ),
                Rule::new("constant", Class::Constant),
                Rule::new(
                    "entity.name.tag, punctuation.definition.tag",
                    Class::Tag,
                ),
                Rule::new(
                    "meta.annotation, variable.annotation, punctuation.definition.annotation, entity.other.attribute-name",
                    Class::Attribute,
                ),
                Rule::new("variable.language, support.variable", Class::VariableBuiltin),
                Rule::new("variable.parameter", Class::VariableParameter),
                Rule::new(
                    "variable.other.member, meta.property-name, support.type.property-name",
                    Class::Property,
                ),
                Rule::new("entity.name.label", Class::Label),
                Rule::new("keyword.operator", Class::Operator),
                Rule::new("keyword, storage.modifier, storage.type", Class::Keyword),
                Rule::new(
                    "punctuation.section, punctuation.definition.parameters.begin, punctuation.definition.parameters.end, punctuation.definition.arguments.begin, punctuation.definition.arguments.end",
                    Class::PunctuationBracket,
                ),
                Rule::new(
                    "punctuation.separator, punctuation.terminator",
                    Class::PunctuationDelimiter,
                ),
                Rule::new("punctuation", Class::Punctuation),
                Rule::new("variable", Class::Variable),
            ],
        }
    }

    fn classify(&self, scopes: &[Scope]) -> Option<HighlightClass> {
        let mut best: Option<(MatchPower, HighlightClass)> = None;
        for rule in &self.rules {
            let Some(power) = rule.selectors.does_match(scopes) else {
                continue;
            };
            if best.as_ref().is_none_or(|(current, _)| power > *current) {
                best = Some((power, rule.class));
            }
        }
        best.map(|(_, class)| class)
    }

    fn syntax_for(&self, language: &str) -> Option<&syntect::parsing::SyntaxReference> {
        let normalized = language.to_ascii_lowercase();
        let token = match normalized.as_str() {
            "" | "text" | "txt" | "plain" | "plaintext" => return None,
            "rs" => "rust",
            "shell" | "shellscript" | "zsh" => "bash",
            // syntect's default syntax set ships no TypeScript grammar, so
            // `typescript` and `tsx` fences rendered as unhighlighted plain
            // text. The JavaScript grammar covers keywords, strings,
            // comments, and numbers correctly; type annotations fall back to
            // plain identifiers, which beats no highlighting at all.
            "js" | "jsx" | "ts" | "tsx" | "typescript" => "javascript",
            "py" => "python",
            "rb" => "ruby",
            "md" => "markdown",
            "yml" => "yaml",
            other => other,
        };
        self.syntaxes.find_syntax_by_token(token)
    }
}

static HIGHLIGHTER: OnceLock<SemanticHighlighter> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct HighlightedLine {
    pub(crate) html: String,
    pub(crate) ending: String,
}

#[derive(Debug, Clone, Copy)]
struct StyledRegion {
    start: usize,
    end: usize,
    class: Option<HighlightClass>,
}

// proa-perf-ok(scope): build-time syntax highlighting emits trusted span chrome and escapes every source fragment before insertion
pub(crate) fn highlight_lines(
    code: &str,
    language: &str,
) -> Result<Option<Vec<HighlightedLine>>, syntect::Error> {
    let highlighter = HIGHLIGHTER.get_or_init(SemanticHighlighter::new);
    let Some(syntax) = highlighter.syntax_for(language) else {
        return Ok(None);
    };

    let mut parser = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut lines = Vec::new();
    let rust_semantic_fallback = is_rust_language(language);
    let markdown_template_rust = if rust_semantic_fallback {
        markdown_template::rust_ranges(code)
    } else {
        Vec::new()
    };
    let mut source_offset = 0;

    for line in LinesWithEndings::from(code) {
        let (body, ending) = crate::code_fence::split_line_ending(line);
        let mut body_bytes_remaining = body.len();
        let mut body_bytes_emitted = 0;
        let mut styled_regions = Vec::new();
        let operations = parser.parse_line(line, &highlighter.syntaxes)?;
        for (region, operation) in ScopeRegionIterator::new(&operations, line) {
            stack.apply(operation)?;
            let visible_bytes = body_bytes_remaining.min(region.len());
            body_bytes_remaining -= visible_bytes;
            if visible_bytes == 0 {
                continue;
            }

            let class = highlighter.classify(stack.as_slice());
            styled_regions.push(StyledRegion {
                start: body_bytes_emitted,
                end: body_bytes_emitted + visible_bytes,
                class,
            });
            body_bytes_emitted += visible_bytes;
        }

        let output = emit_highlighted_line(
            body,
            &styled_regions,
            source_offset,
            &markdown_template_rust,
            highlighter,
            rust_semantic_fallback,
        )?;
        lines.push(HighlightedLine {
            html: output,
            ending: ending.to_string(),
        });
        source_offset += line.len();
    }

    Ok(Some(lines))
}

// proa-perf-ok(scope): build-time syntax framing uses trusted spans and escapes source fragments
fn emit_highlighted_line(
    body: &str,
    styled_regions: &[StyledRegion],
    source_offset: usize,
    markdown_template_rust: &[Range<usize>],
    highlighter: &SemanticHighlighter,
    rust_semantic_fallback: bool,
) -> Result<String, syntect::Error> {
    let mut output = String::with_capacity(body.len());
    let mut open_class = None;
    let line_end = source_offset + body.len();
    let mut cursor = 0;

    for rust_range in markdown_template_rust {
        let start = rust_range.start.max(source_offset);
        let end = rust_range.end.min(line_end);
        if start >= end {
            continue;
        }

        let local_start = start - source_offset;
        let local_end = end - source_offset;
        emit_standard_regions(
            &mut output,
            body,
            styled_regions,
            cursor..local_start,
            rust_semantic_fallback,
            &mut open_class,
        );
        emit_rust_fragment(
            &mut output,
            &body[local_start..local_end],
            highlighter,
            &mut open_class,
        )?;
        cursor = local_end;
    }

    emit_standard_regions(
        &mut output,
        body,
        styled_regions,
        cursor..body.len(),
        rust_semantic_fallback,
        &mut open_class,
    );
    if open_class.is_some() {
        output.push_str("</span>");
    }
    Ok(output)
}

fn emit_standard_regions(
    output: &mut String,
    body: &str,
    styled_regions: &[StyledRegion],
    requested: Range<usize>,
    rust_semantic_fallback: bool,
    open_class: &mut Option<HighlightClass>,
) {
    for region in styled_regions {
        let start = region.start.max(requested.start);
        let end = region.end.min(requested.end);
        if start >= end {
            continue;
        }
        let visible = &body[start..end];
        if rust_semantic_fallback && region.class.is_none() {
            emit_rust_semantic_fallback(output, visible, open_class);
        } else {
            emit_classified_text(output, visible, region.class, open_class);
        }
    }
}

fn emit_rust_fragment(
    output: &mut String,
    fragment: &str,
    highlighter: &SemanticHighlighter,
    open_class: &mut Option<HighlightClass>,
) -> Result<(), syntect::Error> {
    let syntax = highlighter
        .syntax_for("rust")
        .expect("the default syntax set must contain Rust");
    let mut parser = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let operations = parser.parse_line(fragment, &highlighter.syntaxes)?;

    for (region, operation) in ScopeRegionIterator::new(&operations, fragment) {
        stack.apply(operation)?;
        let class = highlighter.classify(stack.as_slice());
        if class.is_none() {
            emit_rust_fragment_fallback(output, region, open_class);
        } else {
            emit_classified_text(output, region, class, open_class);
        }
    }
    Ok(())
}

fn is_rust_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("rust") || language.eq_ignore_ascii_case("rs")
}

fn emit_rust_fragment_fallback(
    output: &mut String,
    text: &str,
    open_class: &mut Option<HighlightClass>,
) {
    let mut plain_start = 0;
    for (index, ch) in text.char_indices() {
        if !matches!(ch, '{' | '}' | '(' | ')' | '[' | ']') {
            continue;
        }
        emit_rust_semantic_fallback(output, &text[plain_start..index], open_class);
        let end = index + ch.len_utf8();
        emit_classified_text(
            output,
            &text[index..end],
            Some(HighlightClass::PunctuationBracket),
            open_class,
        );
        plain_start = end;
    }
    emit_rust_semantic_fallback(output, &text[plain_start..], open_class);
}

/// Fill semantic gaps left by Syntect's TextMate Rust grammar.
///
/// Syntect intentionally has no compiler-level type information. In macro
/// token trees that means a component expression such as
/// `{ProductCard { product }}` arrives as unscoped text. Rust's naming
/// convention gives us a conservative fallback: PascalCase identifiers and
/// one-letter type parameters receive the normal type class, while existing
/// Syntect scopes always win.
fn emit_rust_semantic_fallback(
    output: &mut String,
    text: &str,
    open_class: &mut Option<HighlightClass>,
) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut plain_start = 0;

    while cursor < bytes.len() {
        if !is_ascii_identifier_start(bytes[cursor]) {
            cursor += 1;
            continue;
        }

        let identifier_start = cursor;
        cursor += 1;
        while cursor < bytes.len() && is_ascii_identifier_continue(bytes[cursor]) {
            cursor += 1;
        }

        let identifier = &text[identifier_start..cursor];
        if !looks_like_rust_type(identifier) {
            continue;
        }

        emit_classified_text(
            output,
            &text[plain_start..identifier_start],
            None,
            open_class,
        );
        emit_classified_text(output, identifier, Some(HighlightClass::Type), open_class);
        plain_start = cursor;
    }

    emit_classified_text(output, &text[plain_start..], None, open_class);
}

// proa-perf-ok(scope): this is the low-level build-time span emitter; class names are closed enums and source text is escaped
fn emit_classified_text(
    output: &mut String,
    text: &str,
    class: Option<HighlightClass>,
    open_class: &mut Option<HighlightClass>,
) {
    if text.is_empty() {
        return;
    }

    if class != *open_class {
        if open_class.is_some() {
            output.push_str("</span>");
        }
        if let Some(class) = class {
            output.push_str("<span class=\"");
            output.push_str(class.css_class());
            output.push_str("\">");
        }
        *open_class = class;
    }
    crate::markdown::escape_text(text, output);
}

const fn is_ascii_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_ascii_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn looks_like_rust_type(identifier: &str) -> bool {
    let bytes = identifier.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_uppercase)
        && (bytes.len() == 1 || bytes.iter().any(u8::is_ascii_lowercase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_fallback_highlights_component_structs_in_html_macro_slots() {
        let lines = highlight_lines(
            "html_sync! {\n    <article>\n        {ProductCard { product }}\n        {Badge { label }}\n    </article>\n}\n",
            "rust",
        )
        .unwrap()
        .unwrap();

        assert!(lines[2]
            .html
            .contains("<span class=\"hl-type\">ProductCard</span>"));
        assert!(lines[3]
            .html
            .contains("<span class=\"hl-type\">Badge</span>"));
        assert!(!lines[2]
            .html
            .contains("<span class=\"hl-type\">product</span>"));
        assert!(!lines[3]
            .html
            .contains("<span class=\"hl-type\">label</span>"));
    }

    #[test]
    fn rust_fallback_does_not_reclassify_strings_comments_or_constants() {
        let lines = highlight_lines(
            "let label = \"ProductCard\"; // Badge\nlet count = MAX_ITEMS;\n",
            "rs",
        )
        .unwrap()
        .unwrap();

        assert!(lines[0].html.contains("<span class=\"hl-string\""));
        assert!(lines[0].html.contains("<span class=\"hl-comment\""));
        assert!(!lines[0].html.contains("<span class=\"hl-type\">"));
        assert!(!lines[1]
            .html
            .contains("<span class=\"hl-type\">MAX_ITEMS</span>"));
    }

    #[test]
    fn highlights_rust_control_flow_inside_markdown_templates() {
        let lines = highlight_lines(
            concat!(
                "let summary = md_sync! { r###\"\n",
                "    # {title}\n",
                "\n",
                "    ## Features\n",
                "\n",
                "    {for feature in features.iter() {\n",
                "        md_sync! { \"- \" {feature} \"\\n\" }\n",
                "    }}\n",
                "    {if in_stock { \"In stock\" } else { \"Backordered\" }}\n",
                "    \"###\n",
                "};\n",
            ),
            "rust",
        )
        .unwrap()
        .unwrap();

        assert!(lines[5]
            .html
            .contains("<span class=\"hl-keyword\">for</span>"));
        assert!(lines[5]
            .html
            .contains("<span class=\"hl-operator\">in</span>"));
        assert!(lines[8]
            .html
            .contains("<span class=\"hl-keyword\">if</span>"));
        assert!(lines[8]
            .html
            .contains("<span class=\"hl-keyword\">else</span>"));
        assert!(lines[7]
            .html
            .contains("<span class=\"hl-punctuation-bracket\">}}</span>"));
        assert!(lines[3].html.contains("<span class=\"hl-string\""));
        assert!(lines[8].html.contains("In stock"));
        assert!(lines[8].html.contains("Backordered"));
    }

    #[test]
    fn leaves_markdown_like_braces_in_other_rust_strings_alone() {
        let lines = highlight_lines(
            "let example = \"{for feature in features {not a template}}\";\n",
            "rust",
        )
        .unwrap()
        .unwrap();

        assert!(lines[0].html.contains("<span class=\"hl-string\""));
        assert!(!lines[0]
            .html
            .contains("<span class=\"hl-keyword\">for</span>"));
    }

    #[test]
    fn supports_raw_templates_without_reclassifying_legacy_token_literals() {
        let raw = highlight_lines(
            "let page = md_sync! {r#\"{for item in items { md_sync! { \"- \" {item} } }}\"#};\n",
            "rust",
        )
        .unwrap()
        .unwrap();
        assert!(raw[0]
            .html
            .contains("<span class=\"hl-keyword\">for</span>"));

        let legacy = highlight_lines(
            "let page = md_sync! { \"{for fake in values {text}}\" {actual} };\n",
            "rust",
        )
        .unwrap()
        .unwrap();
        assert!(!legacy[0]
            .html
            .contains("<span class=\"hl-keyword\">for</span>"));
    }
}
