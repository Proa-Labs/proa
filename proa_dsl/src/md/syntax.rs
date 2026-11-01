// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Separate Rust header/expression lexing from literal Markdown body scanning.

use super::{fenced_code_end, inline_code_end};

pub(super) fn keyword<'a>(input: &'a str, word: &str) -> Option<&'a str> {
    input.strip_prefix(word).filter(|tail| {
        tail.starts_with(char::is_whitespace) || (word == "if" && tail.starts_with('('))
    })
}

pub(super) fn is_control(input: &str) -> bool {
    keyword(input, "if").is_some() || keyword(input, "for").is_some()
}

/// End of a Rust token whose interior cannot contain template delimiters.
fn atom_end(input: &str, start: usize) -> Result<Option<usize>, String> {
    let tail = &input[start..];
    if tail.starts_with("//") {
        return Ok(Some(start + tail.find('\n').unwrap_or(tail.len())));
    }
    if tail.starts_with("/*") {
        let mut depth = 1;
        let mut pos = start + 2;
        while pos < input.len() {
            if input[pos..].starts_with("/*") {
                depth += 1;
                pos += 2;
            } else if input[pos..].starts_with("*/") {
                depth -= 1;
                pos += 2;
                if depth == 0 {
                    return Ok(Some(pos));
                }
            } else {
                pos += input[pos..].chars().next().unwrap().len_utf8();
            }
        }
        return Err("unclosed Rust block comment in Markdown expression".into());
    }
    for prefix in ["br", "cr", "r"] {
        if let Some(rest) = tail.strip_prefix(prefix) {
            let hashes = rest.bytes().take_while(|b| *b == b'#').count();
            if rest.as_bytes().get(hashes) == Some(&b'"') {
                let body = start + prefix.len() + hashes + 1;
                let close = format!("\"{}", "#".repeat(hashes));
                let end = input[body..]
                    .find(&close)
                    .ok_or("unclosed Rust raw string in Markdown expression")?;
                return Ok(Some(body + end + close.len()));
            }
        }
    }
    let quote = if tail.starts_with('"') {
        Some(start)
    } else if tail.starts_with("b\"") || tail.starts_with("c\"") {
        Some(start + 1)
    } else {
        None
    };
    if let Some(quote) = quote {
        let mut pos = quote + 1;
        while pos < input.len() {
            match input.as_bytes()[pos] {
                b'"' => return Ok(Some(pos + 1)),
                b'\\' => {
                    pos += 1;
                    if pos < input.len() {
                        pos += input[pos..].chars().next().unwrap().len_utf8();
                    }
                }
                _ => pos += input[pos..].chars().next().unwrap().len_utf8(),
            }
        }
        return Err("unclosed Rust string in Markdown expression".into());
    }
    let apostrophe = if tail.starts_with('\'') {
        Some(start)
    } else if tail.starts_with("b'") {
        Some(start + 1)
    } else {
        None
    };
    if let Some(quote) = apostrophe {
        let body = quote + 1;
        if input[body..].starts_with('\\') {
            let escape = body + 1;
            let end = if input[escape..].starts_with("u{") {
                input[escape + 2..]
                    .find('}')
                    .map(|end| escape + 2 + end + 1)
            } else if input[escape..].starts_with('x') {
                Some(escape + 3)
            } else {
                input[escape..]
                    .chars()
                    .next()
                    .map(|ch| escape + ch.len_utf8())
            };
            if let Some(end) = end.filter(|end| input.as_bytes().get(*end) == Some(&b'\'')) {
                return Ok(Some(end + 1));
            }
        } else if let Some(ch) = input[body..].chars().next() {
            let end = body + ch.len_utf8();
            if input.as_bytes().get(end) == Some(&b'\'') {
                return Ok(Some(end + 1));
            }
        }
        // Otherwise this is a lifetime, not a character literal.
    }
    Ok(None)
}

pub(super) fn rust_brace_end(input: &str, start: usize) -> Result<usize, String> {
    let mut depth = 1;
    let mut pos = start;
    while pos < input.len() {
        if let Some(end) = atom_end(input, pos)? {
            pos = end;
            continue;
        }
        match input.as_bytes()[pos] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(pos);
                }
            }
            _ => {}
        }
        pos += input[pos..].chars().next().unwrap().len_utf8();
    }
    Err("unclosed `{` in Markdown expression".into())
}

/// Locate the body only after a complete Rust control header. This permits
/// block conditions, destructuring patterns and const-generic expressions.
pub(super) fn control_open(input: &str) -> Result<usize, String> {
    let mut pos = 0;
    let mut delimiters = 0usize;
    while pos < input.len() {
        if let Some(end) = atom_end(input, pos)? {
            pos = end;
            continue;
        }
        match input.as_bytes()[pos] {
            b'(' | b'[' => delimiters += 1,
            b')' | b']' => delimiters = delimiters.saturating_sub(1),
            b'{' => {
                if delimiters == 0 {
                    let candidate = format!("{} {{}}", &input[..pos]);
                    if matches!(
                        syn::parse_str::<syn::Expr>(&candidate),
                        Ok(syn::Expr::If(_) | syn::Expr::ForLoop(_))
                    ) {
                        return Ok(pos);
                    }
                }
                pos = rust_brace_end(input, pos + 1)? + 1;
                continue;
            }
            _ => {}
        }
        pos += input[pos..].chars().next().unwrap().len_utf8();
    }
    Err("expected a Rust `if` or `for` header followed by a Markdown body".into())
}

pub(super) fn for_in(input: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < input.len() {
        if let Some(end) = atom_end(input, pos).ok()? {
            pos = end;
            continue;
        }
        let tail = &input[pos..];
        if tail.starts_with("in")
            && input[..pos]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
            && tail[2..].starts_with(char::is_whitespace)
        {
            return Some(pos);
        }
        pos += tail.chars().next()?.len_utf8();
    }
    None
}

fn control_end(input: &str, closers: usize) -> Result<usize, String> {
    let open = control_open(input)?;
    let mut end = markdown_body_end(input, open + 1, closers)? + 1;
    if keyword(input, "if").is_some() {
        let tail = input[end..].trim_start();
        if let Some(rest) = tail
            .strip_prefix("else")
            .filter(|r| r.starts_with(char::is_whitespace) || r.starts_with('{'))
        {
            let rest = rest.trim_start();
            let offset = input.len() - rest.len();
            end = if keyword(rest, "if").is_some() {
                offset + control_end(rest, closers)?
            } else if rest.starts_with('{') {
                markdown_body_end(input, offset + 1, closers)? + 1
            } else {
                return Err("expected `{` or `if` after `else`".into());
            };
        }
    }
    Ok(end)
}

/// Look beyond an escaped closing brace, skipping literal code and Rust slots.
fn later_close(input: &str, mut pos: usize, closers: usize) -> bool {
    let mut escaped_opens = 0usize;
    while pos < input.len() {
        if pos == 0 || input.as_bytes()[pos - 1] == b'\n' {
            let indent = input[pos..].len() - input[pos..].trim_start_matches([' ', '\t']).len();
            if let Some(end) = fenced_code_end(input, pos + indent, &input[pos..pos + indent]) {
                pos = end;
                continue;
            }
        }
        if input.as_bytes()[pos] == b'`' {
            if let Some(end) = inline_code_end(input, pos) {
                pos = end;
                continue;
            }
        }
        if input[pos..].starts_with("{{") {
            escaped_opens += 1;
            pos += 2;
            continue;
        }
        if input[pos..].starts_with("}}") && escaped_opens > 0 {
            escaped_opens -= 1;
            pos += 2;
            continue;
        }
        if input.as_bytes()[pos] == b'{' {
            let content = input[pos + 1..].trim_start();
            let end = if is_control(content) {
                control_end(content, closers + 2).and_then(|end| {
                    let end = input.len() - content.len() + end;
                    let rest = input[end..].trim_start();
                    if rest.starts_with('}') {
                        Ok(input.len() - rest.len())
                    } else {
                        Err("unclosed nested Markdown control".into())
                    }
                })
            } else {
                rust_brace_end(input, pos + 1)
            };
            match end {
                Ok(end) => {
                    pos = end + 1;
                    continue;
                }
                Err(_) => return false,
            }
        }
        if input.as_bytes()[pos] == b'}' {
            return true;
        }
        pos += input[pos..].chars().next().unwrap().len_utf8();
    }
    false
}

/// Markdown quotes are prose. Only `{expression}` enters Rust lexical mode.
/// Standalone closing markers terminate multiline bodies; doubled braces in
/// content stay literal. At an inline boundary, reserve the closing run needed
/// by the enclosing controls and treat preceding pairs as escaped content.
pub(super) fn markdown_body_end(
    input: &str,
    start: usize,
    closers: usize,
) -> Result<usize, String> {
    let mut pos = start;
    while pos < input.len() {
        let line_start = pos == start || input.as_bytes()[pos - 1] == b'\n';
        if line_start {
            let indent = input[pos..].len() - input[pos..].trim_start_matches([' ', '\t']).len();
            if let Some(end) = fenced_code_end(input, pos + indent, &input[pos..pos + indent]) {
                pos = end;
                continue;
            }
            let content = pos + indent;
            if is_control(&input[content..]) && control_open(&input[content..]).is_ok() {
                pos = content + control_end(&input[content..], closers + 1)?;
                continue;
            }
        }
        match input.as_bytes()[pos] {
            b'`' => {
                if let Some(end) = inline_code_end(input, pos) {
                    pos = end;
                    continue;
                }
            }
            b'{' => {
                if input[pos..].starts_with("{{") {
                    pos += 2;
                    continue;
                }
                let content = input[pos + 1..].trim_start();
                if is_control(content) {
                    let offset = input.len() - content.len();
                    pos = offset + control_end(content, closers + 2)?;
                    pos += input[pos..].len() - input[pos..].trim_start().len();
                    if input.as_bytes().get(pos) != Some(&b'}') {
                        return Err("unexpected tokens after Markdown control block".into());
                    }
                    pos += 1;
                } else {
                    pos = rust_brace_end(input, pos + 1)? + 1;
                }
                continue;
            }
            b'}' => {
                let run = input[pos..].bytes().take_while(|b| *b == b'}').count();
                let tail = input[pos + run..].split('\n').next().unwrap_or("").trim();
                let standalone = input[..pos]
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                    && (tail.is_empty()
                        || tail.starts_with('{')
                        || tail.strip_prefix("else").is_some_and(|rest| {
                            rest.starts_with('{') || rest.starts_with(char::is_whitespace)
                        }));
                if run >= 2 && !standalone {
                    if run > closers && (run - closers).is_multiple_of(2) {
                        pos += run - closers;
                        return Ok(pos);
                    }
                    if later_close(input, pos + run, closers) {
                        pos += 2;
                        continue;
                    }
                }
                return Ok(pos);
            }
            _ => {}
        }
        pos += input[pos..].chars().next().unwrap().len_utf8();
    }
    Err("unclosed Markdown control body".into())
}
