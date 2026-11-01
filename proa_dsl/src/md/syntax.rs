// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Rust-aware brace matching for template slots.
//!
//! Everything inside a `{ ... }` slot is Rust, so the slot ends at the brace
//! that balances the opening one once strings, characters, raw strings, and
//! comments are skipped as atoms. Markdown never appears inside a slot except
//! through a nested `md_sync!` / `md!` literal, which is an ordinary Rust
//! string token to this scanner.

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

/// Index of the `}` that closes a slot whose contents begin at `start`.
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
    Err("unclosed `{` in Markdown template; write `{{` for a literal brace".into())
}
