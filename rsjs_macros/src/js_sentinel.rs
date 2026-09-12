// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Lexical validation for compiler-reserved identifiers in emitted JavaScript.
//!
//! This is deliberately a lexer, not a substring search. Compiler diagnostics
//! and authored static text may contain the reserved spelling in string and
//! template literals or comments without creating a runtime reference. Template
//! interpolations, however, return to the JavaScript lexical grammar and must be
//! inspected recursively.

use std::fmt;

pub(super) use rsjs_ir::UNLINKED_HELPER_IDENTIFIER_PREFIX;

/// Whether emitted JavaScript contains an identifier token in the compiler's
/// reserved unresolved-helper namespace.
///
/// The emitter only produces ordinary (unescaped) ASCII identifiers. The
/// scanner nevertheless consumes non-ASCII and escaped identifier code points
/// as part of their surrounding token so the reserved ASCII spelling cannot be
/// mistaken for a token start in the middle of another identifier.
pub(super) fn contains_unlinked_helper_identifier(
    javascript: &str,
) -> Result<bool, JavascriptLexError> {
    Scanner::new(javascript).scan_program()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delimiter {
    Program,
    TemplateInterpolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParenKind {
    Expression,
    ControlHeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BraceKind {
    StatementBlock,
    ExpressionBody,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastToken {
    None,
    Expression,
    ControlHeaderClose,
    Arrow,
    MemberAccess,
    BlockIntroducer,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CodeContext {
    /// ECMAScript's lexical goal: `true` means a slash may begin a regexp
    /// literal; `false` means it is division or division-assignment.
    regexp_allowed: bool,
    pending_control_paren: bool,
    last_token: LastToken,
}

impl Default for CodeContext {
    fn default() -> Self {
        Self {
            regexp_allowed: true,
            pending_control_paren: false,
            last_token: LastToken::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JavascriptLexError {
    offset: usize,
    construct: &'static str,
}

impl JavascriptLexError {
    fn unterminated(offset: usize, construct: &'static str) -> Self {
        Self { offset, construct }
    }
}

impl fmt::Display for JavascriptLexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unterminated {} beginning at byte {}",
            self.construct, self.offset
        )
    }
}

struct Scanner<'source> {
    source: &'source str,
    bytes: &'source [u8],
    offset: usize,
}

impl<'source> Scanner<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            offset: 0,
        }
    }

    fn scan_program(mut self) -> Result<bool, JavascriptLexError> {
        self.scan_code(Delimiter::Program)
    }

    fn scan_code(&mut self, delimiter: Delimiter) -> Result<bool, JavascriptLexError> {
        let interpolation_start = self.offset.saturating_sub(2);
        let mut context = CodeContext::default();
        let mut parens = Vec::new();
        let mut braces = Vec::new();

        while let Some(byte) = self.peek() {
            match byte {
                byte if byte.is_ascii_whitespace() => self.offset += 1,
                b'\'' | b'"' => {
                    self.scan_quoted_string(byte)?;
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                b'`' => {
                    if self.scan_template_literal()? {
                        return Ok(true);
                    }
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                b'/' if self.peek_at(1) == Some(b'/') => self.scan_line_comment(),
                b'/' if self.peek_at(1) == Some(b'*') => self.scan_block_comment()?,
                b'/' if context.regexp_allowed => {
                    self.scan_regexp_literal()?;
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                b'/' => {
                    self.offset += 1;
                    if self.peek() == Some(b'=') {
                        self.offset += 1;
                    }
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Other;
                }
                b'}' if delimiter == Delimiter::TemplateInterpolation && braces.is_empty() => {
                    self.offset += 1;
                    return Ok(false);
                }
                b'{' => {
                    let kind = classify_open_brace(context);
                    braces.push(kind);
                    self.offset += 1;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::None;
                }
                b'}' => {
                    let kind = braces.pop().unwrap_or(BraceKind::StatementBlock);
                    self.offset += 1;
                    context.regexp_allowed = kind == BraceKind::StatementBlock;
                    context.pending_control_paren = false;
                    context.last_token = if kind == BraceKind::StatementBlock {
                        LastToken::BlockIntroducer
                    } else {
                        LastToken::Expression
                    };
                }
                b'(' => {
                    let kind = if context.pending_control_paren {
                        ParenKind::ControlHeader
                    } else {
                        ParenKind::Expression
                    };
                    parens.push(kind);
                    self.offset += 1;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Other;
                }
                b')' => {
                    let kind = parens.pop().unwrap_or(ParenKind::Expression);
                    self.offset += 1;
                    context.regexp_allowed = kind == ParenKind::ControlHeader;
                    context.pending_control_paren = false;
                    context.last_token = if kind == ParenKind::ControlHeader {
                        LastToken::ControlHeaderClose
                    } else {
                        LastToken::Expression
                    };
                }
                b'[' => {
                    self.offset += 1;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Other;
                }
                b']' => {
                    self.offset += 1;
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                b'.' if self.peek_at(1).is_some_and(|next| next.is_ascii_digit()) => {
                    self.scan_number();
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                byte if byte.is_ascii_digit() => {
                    self.scan_number();
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Expression;
                }
                byte if is_identifier_start(byte)
                    || (byte == b'\\' && self.peek_at(1) == Some(b'u')) =>
                {
                    let start = self.offset;
                    self.scan_identifier()?;
                    let identifier = &self.source[start..self.offset];
                    if identifier_starts_with_reserved_prefix(identifier) {
                        return Ok(true);
                    }
                    if context.last_token == LastToken::MemberAccess {
                        // IdentifierName permits keywords after `.` and `?.`.
                        // They remain the tail of an expression, so a following
                        // slash is division rather than a regexp literal.
                        context.regexp_allowed = false;
                        context.pending_control_paren = false;
                        context.last_token = LastToken::Expression;
                    } else {
                        classify_identifier(identifier, &mut context);
                    }
                }
                b'+' | b'-' if self.peek_at(1) == Some(byte) => {
                    self.offset += 2;
                    // Prefix ++/-- still expects an expression; postfix ++/--
                    // still ends one. The preceding lexical goal distinguishes
                    // those two generated-code forms.
                    if !context.regexp_allowed {
                        context.last_token = LastToken::Expression;
                    }
                    context.pending_control_paren = false;
                }
                b'=' if self.peek_at(1) == Some(b'>') => {
                    self.offset += 2;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Arrow;
                }
                b'.' => {
                    self.offset += 1;
                    context.regexp_allowed = false;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::MemberAccess;
                }
                b';' => {
                    self.offset += 1;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::None;
                }
                // Generated operators and separators permit an expression on
                // their right-hand side (and therefore a regexp literal).
                b',' | b':' | b'?' | b'=' | b'!' | b'~' | b'+' | b'-' | b'*' | b'%' | b'&'
                | b'|' | b'^' | b'<' | b'>' => {
                    self.scan_operator();
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Other;
                }
                _ => {
                    // Generated rsjs output does not use JSX or other lexical
                    // extensions. Consuming an unknown punctuator as a token
                    // boundary is conservative: it cannot hide a later
                    // reserved identifier.
                    self.offset += 1;
                    context.regexp_allowed = true;
                    context.pending_control_paren = false;
                    context.last_token = LastToken::Other;
                }
            }
        }

        if delimiter == Delimiter::TemplateInterpolation {
            Err(JavascriptLexError::unterminated(
                interpolation_start,
                "template interpolation",
            ))
        } else {
            Ok(false)
        }
    }

    fn scan_quoted_string(&mut self, quote: u8) -> Result<(), JavascriptLexError> {
        let start = self.offset;
        self.offset += 1;
        while let Some(byte) = self.peek() {
            match byte {
                byte if byte == quote => {
                    self.offset += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.offset += 1;
                    if self.peek().is_some() {
                        self.offset += 1;
                    } else {
                        break;
                    }
                }
                b'\n' | b'\r' => break,
                _ => self.offset += 1,
            }
        }
        Err(JavascriptLexError::unterminated(start, "string literal"))
    }

    fn scan_line_comment(&mut self) {
        self.offset += 2;
        while let Some(byte) = self.peek() {
            self.offset += 1;
            if matches!(byte, b'\n' | b'\r') {
                break;
            }
        }
    }

    fn scan_block_comment(&mut self) -> Result<(), JavascriptLexError> {
        let start = self.offset;
        self.offset += 2;
        while self.peek().is_some() {
            if self.peek() == Some(b'*') && self.peek_at(1) == Some(b'/') {
                self.offset += 2;
                return Ok(());
            }
            self.offset += 1;
        }
        Err(JavascriptLexError::unterminated(start, "block comment"))
    }

    fn scan_template_literal(&mut self) -> Result<bool, JavascriptLexError> {
        let start = self.offset;
        self.offset += 1;
        while let Some(byte) = self.peek() {
            match byte {
                b'`' => {
                    self.offset += 1;
                    return Ok(false);
                }
                b'\\' => {
                    self.offset += 1;
                    if self.peek().is_some() {
                        self.offset += 1;
                    } else {
                        break;
                    }
                }
                b'$' if self.peek_at(1) == Some(b'{') => {
                    self.offset += 2;
                    if self.scan_code(Delimiter::TemplateInterpolation)? {
                        return Ok(true);
                    }
                }
                _ => self.offset += 1,
            }
        }
        Err(JavascriptLexError::unterminated(start, "template literal"))
    }

    fn scan_regexp_literal(&mut self) -> Result<(), JavascriptLexError> {
        let start = self.offset;
        self.offset += 1;
        let mut in_character_class = false;
        while let Some(byte) = self.peek() {
            match byte {
                b'\\' => {
                    self.offset += 1;
                    if self.peek().is_some() {
                        self.offset += 1;
                    } else {
                        break;
                    }
                }
                b'[' if !in_character_class => {
                    in_character_class = true;
                    self.offset += 1;
                }
                b']' if in_character_class => {
                    in_character_class = false;
                    self.offset += 1;
                }
                b'/' if !in_character_class => {
                    self.offset += 1;
                    self.scan_identifier_continuation()?;
                    return Ok(());
                }
                b'\n' | b'\r' => break,
                _ => self.offset += 1,
            }
        }
        Err(JavascriptLexError::unterminated(start, "regexp literal"))
    }

    fn scan_identifier(&mut self) -> Result<(), JavascriptLexError> {
        if self.peek() == Some(b'\\') {
            self.scan_identifier_unicode_escape()?;
        } else {
            self.offset += 1;
        }
        self.scan_identifier_continuation()
    }

    fn scan_identifier_continuation(&mut self) -> Result<(), JavascriptLexError> {
        while let Some(byte) = self.peek() {
            if is_identifier_continue(byte) {
                self.offset += 1;
            } else if byte == b'\\' && self.peek_at(1) == Some(b'u') {
                // Identifier Unicode escapes remain part of this token. Full
                // escape decoding happens only when checking the reserved
                // prefix below; tokenization merely validates and consumes it.
                self.scan_identifier_unicode_escape()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn scan_identifier_unicode_escape(&mut self) -> Result<(), JavascriptLexError> {
        let start = self.offset;
        debug_assert_eq!(self.peek(), Some(b'\\'));
        debug_assert_eq!(self.peek_at(1), Some(b'u'));
        self.offset += 2;

        if self.peek() == Some(b'{') {
            self.offset += 1;
            let digits_start = self.offset;
            while self.peek().is_some_and(|next| next.is_ascii_hexdigit()) {
                self.offset += 1;
            }
            if self.offset == digits_start || self.peek() != Some(b'}') {
                return Err(JavascriptLexError::unterminated(
                    start,
                    "identifier Unicode escape",
                ));
            }
            self.offset += 1;
        } else {
            for _ in 0..4 {
                if !self.peek().is_some_and(|next| next.is_ascii_hexdigit()) {
                    return Err(JavascriptLexError::unterminated(
                        start,
                        "identifier Unicode escape",
                    ));
                }
                self.offset += 1;
            }
        }
        Ok(())
    }

    fn scan_number(&mut self) {
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_') {
                self.offset += 1;
            } else {
                break;
            }
        }
    }

    fn scan_operator(&mut self) {
        self.offset += 1;
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b'=' | b'&' | b'|' | b'?' | b'<' | b'>' | b'*'))
        {
            self.offset += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn peek_at(&self, relative: usize) -> Option<u8> {
        self.bytes.get(self.offset + relative).copied()
    }
}

fn classify_identifier(identifier: &str, context: &mut CodeContext) {
    context.pending_control_paren = matches!(
        identifier,
        "if" | "while" | "for" | "with" | "switch" | "catch"
    );

    if context.pending_control_paren {
        context.regexp_allowed = true;
        context.last_token = LastToken::BlockIntroducer;
    } else if matches!(
        identifier,
        "return"
            | "throw"
            | "case"
            | "delete"
            | "void"
            | "typeof"
            | "new"
            | "in"
            | "instanceof"
            | "await"
            | "yield"
    ) {
        context.regexp_allowed = true;
        context.last_token = LastToken::Other;
    } else if matches!(identifier, "else" | "do" | "try" | "finally") {
        context.regexp_allowed = true;
        context.last_token = LastToken::BlockIntroducer;
    } else {
        context.regexp_allowed = false;
        context.last_token = LastToken::Expression;
    }
}

fn classify_open_brace(context: CodeContext) -> BraceKind {
    if context.last_token == LastToken::Arrow {
        BraceKind::ExpressionBody
    } else if matches!(
        context.last_token,
        LastToken::None | LastToken::ControlHeaderClose | LastToken::BlockIntroducer
    ) {
        BraceKind::StatementBlock
    } else if context.regexp_allowed {
        BraceKind::Object
    } else {
        // Function and class expressions end with their body. Treat the close
        // as expression-ending so a following slash is never allowed to hide
        // a division operand. Declarations are conservatively treated the same
        // way; generated declarations put a statement boundary after `}`.
        BraceKind::ExpressionBody
    }
}

const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

const fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn identifier_starts_with_reserved_prefix(identifier: &str) -> bool {
    let mut code_points = IdentifierCodePoints::new(identifier);
    UNLINKED_HELPER_IDENTIFIER_PREFIX
        .chars()
        .all(|expected| code_points.next() == Some(expected))
}

/// Decodes only the `UnicodeEscapeSequence` forms permitted inside an
/// IdentifierName. Tokenization has already validated their shape, so this
/// iterator can stay deliberately small and allocation-free.
struct IdentifierCodePoints<'source> {
    source: &'source str,
    offset: usize,
}

impl<'source> IdentifierCodePoints<'source> {
    fn new(source: &'source str) -> Self {
        Self { source, offset: 0 }
    }
}

impl Iterator for IdentifierCodePoints<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.source.get(self.offset..)?;
        if !rest.starts_with("\\u") {
            let next = rest.chars().next()?;
            self.offset += next.len_utf8();
            return Some(next);
        }

        let escape = rest.as_bytes();
        let (digits, consumed) = if escape.get(2) == Some(&b'{') {
            let close = rest.find('}')?;
            (&rest[3..close], close + 1)
        } else {
            (rest.get(2..6)?, 6)
        };
        let value = u32::from_str_radix(digits, 16).ok()?;
        self.offset += consumed;
        char::from_u32(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(source: &str) -> bool {
        contains_unlinked_helper_identifier(source).expect("test source should lex")
    }

    #[test]
    fn finds_reserved_identifiers_in_executable_code() {
        for source in [
            "__rsjs_unlinked_helper_edge_0();",
            r#"\u005f\u005frsjs_unlinked_helper_edge_0();"#,
            r#"\u{5f}\u{5f}rsjs_unlinked_helper_edge_0();"#,
            "const value = __rsjs_unlinked_helper_edge_12(arg);",
            "runtime.__rsjs_unlinked_helper_edge_2(value);",
            "left / __rsjs_unlinked_helper_edge_3 / right;",
            "const f = () => {} / __rsjs_unlinked_helper_edge_4 / right;",
            "const f = function () {} / __rsjs_unlinked_helper_edge_5 / right;",
            "object.if / __rsjs_unlinked_helper_edge_6 / right;",
            "object?.return / __rsjs_unlinked_helper_edge_7 / right;",
            "const f = () => __rsjs_unlinked_helper_edge_8();",
            "const f = value => __rsjs_unlinked_helper_edge_9(value);",
        ] {
            assert!(contains(source), "failed to reject: {source}");
        }
    }

    #[test]
    fn ignores_reserved_spelling_in_strings_and_comments() {
        for source in [
            r#"const text = "__rsjs_unlinked_helper_edge_0";"#,
            r#"const text = '__rsjs_unlinked_helper_edge_0';"#,
            "// __rsjs_unlinked_helper_edge_0()\nconst ready = true;",
            "/* __rsjs_unlinked_helper_edge_0() */ const ready = true;",
            "const prefix__rsjs_unlinked_helper_edge_0 = true;",
            r#"const prefix\u005f__rsjs_unlinked_helper_edge_0 = true;"#,
            r#"const \u{73}afe__rsjs_unlinked_helper_edge_0 = true;"#,
        ] {
            assert!(!contains(source), "false positive for: {source}");
        }
    }

    #[test]
    fn ignores_template_raw_text_but_scans_interpolations() {
        assert!(!contains(
            "const text = `raw __rsjs_unlinked_helper_edge_0 text`;"
        ));
        assert!(!contains(
            "const text = `outer ${`raw __rsjs_unlinked_helper_edge_0`} tail`;"
        ));
        assert!(!contains(
            "const text = `${\"__rsjs_unlinked_helper_edge_0\"} ${/* __rsjs_unlinked_helper_edge_1 */ safe}`;"
        ));
        assert!(!contains(
            "const text = `${/__rsjs_unlinked_helper_edge_0/.source}`;"
        ));
        assert!(contains(
            "const text = `value: ${__rsjs_unlinked_helper_edge_0()}`;"
        ));
        assert!(contains(
            "const text = `outer ${`inner ${__rsjs_unlinked_helper_edge_0()}`}`;"
        ));
    }

    #[test]
    fn regexp_literals_are_not_code_identifiers() {
        for source in [
            "const matcher = /__rsjs_unlinked_helper_edge_0/;",
            "return /__rsjs_unlinked_helper_edge_0/.test(value);",
            "if (ready) /__rsjs_unlinked_helper_edge_0/.test(value);",
            "const matcher = /[a-z/__rsjs_unlinked_helper_edge_0]+/g;",
        ] {
            assert!(!contains(source), "false positive for: {source}");
        }
    }

    #[test]
    fn malformed_lexical_constructs_fail_closed() {
        for source in [
            "const text = '__rsjs_unlinked_",
            "/* __rsjs_unlinked_",
            "const text = `__rsjs_unlinked_",
            "const text = `${safe",
            "return /__rsjs_unlinked_",
            r#"const value = \u{};"#,
            r#"const value = \u12x4;"#,
        ] {
            assert!(
                contains_unlinked_helper_identifier(source).is_err(),
                "malformed source unexpectedly passed: {source}"
            );
        }
    }
}
