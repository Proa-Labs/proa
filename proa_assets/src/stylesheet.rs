// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! References from a stylesheet to other public files.
//!
//! A hashed file has one URL, so `url(images/logo.svg)` cannot reach its
//! target by logical name. Publication finds each reference here and writes
//! the target's published path in its place, before the stylesheet itself is
//! hashed. Only `url(...)`, `@import`, and `image-set(...)` carry URLs; the
//! rest of the file is copied byte for byte.

use std::ops::Range;

use cssparser::{ParseError, Parser, ParserInput, Token};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

const PATH_ENCODING: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// One URL as written in a stylesheet.
pub(crate) struct Reference {
    span: Range<usize>,
    pub(crate) url: String,
    /// Whether the source wrote `url(...)` or a bare string.
    function: bool,
}

/// What a written URL points at.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// Another origin, a `data:` URL, or a fragment: nothing to resolve.
    External,
    /// A path from the origin root, which only the running server can place.
    RootRelative,
    /// A public file by logical path, plus any `?query` or `#fragment`.
    Public { logical: String, suffix: String },
    /// A relative path that leaves the public directory.
    Outside,
}

pub(crate) fn references(source: &str) -> Result<Vec<Reference>, ()> {
    fn visit<'i>(
        parser: &mut Parser<'i, '_>,
        found: &mut Vec<Reference>,
        depth: usize,
        image_set: bool,
    ) -> Result<(), ParseError<'i, ()>> {
        if depth > 128 {
            return Err(parser.new_custom_error(()));
        }
        let mut import = false;
        loop {
            let start = parser.position().byte_index();
            let token = match parser.next_including_whitespace_and_comments() {
                Ok(token) => token.clone(),
                Err(_) => break,
            };
            let mut reference = None;
            match token {
                Token::WhiteSpace(_) | Token::Comment(_) => continue,
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                    import = true;
                    continue;
                }
                Token::UnquotedUrl(url) => reference = Some((url.to_string(), true)),
                Token::QuotedString(url) if import || image_set => {
                    reference = Some((url.to_string(), false));
                }
                Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                    // Only the plain `url("...")` form is rewritten. One with
                    // modifiers, or anything else in the parentheses, is left
                    // exactly as written, and the rest of the file still is
                    // processed.
                    let url = parser.parse_nested_block(|nested| {
                        let url = nested
                            .try_parse(|nested| nested.expect_string_cloned())
                            .ok()
                            .filter(|_| nested.is_exhausted());
                        while nested.next_including_whitespace_and_comments().is_ok() {}
                        Ok::<_, ParseError<'i, ()>>(url)
                    })?;
                    reference = url.map(|url| (url.to_string(), true));
                }
                Token::Function(name)
                    if name.eq_ignore_ascii_case("image-set")
                        || name.eq_ignore_ascii_case("-webkit-image-set") =>
                {
                    parser.parse_nested_block(|nested| visit(nested, found, depth + 1, true))?;
                }
                Token::Function(_)
                | Token::CurlyBracketBlock
                | Token::SquareBracketBlock
                | Token::ParenthesisBlock => {
                    parser.parse_nested_block(|nested| visit(nested, found, depth + 1, false))?;
                }
                _ => {}
            }
            if let Some((url, function)) = reference {
                found.push(Reference {
                    span: start..parser.position().byte_index(),
                    url,
                    function,
                });
            }
            import = false;
        }
        Ok(())
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut found = Vec::new();
    visit(&mut parser, &mut found, 0, false).map_err(|_| ())?;
    Ok(found)
}

/// Resolve `url` as written in the stylesheet at logical path `from`.
pub(crate) fn target(from: &str, url: &str) -> Target {
    let url = url.trim();
    let scheme = url
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.is_empty() && !scheme.contains(['/', '?', '#']));
    if url.is_empty() || url.starts_with('#') || url.starts_with("//") || scheme {
        return Target::External;
    }
    if url.starts_with('/') {
        return Target::RootRelative;
    }
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let (path, suffix) = url.split_at(end);
    let Ok(path) = percent_decode_str(path).decode_utf8() else {
        return Target::Outside;
    };
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Target::Outside;
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Target::Outside;
    }
    Target::Public {
        logical: parts.join("/"),
        suffix: suffix.into(),
    }
}

/// The relative URL from the file at `from` to the file at `to`. Both are
/// paths below the public route, so the result holds on any origin.
pub(crate) fn relative(from: &str, to: &str) -> String {
    let mut from: Vec<&str> = from.split('/').collect();
    from.pop();
    let to: Vec<&str> = to.split('/').collect();
    let shared = from
        .iter()
        .zip(&to[..to.len() - 1])
        .take_while(|(a, b)| a == b)
        .count();
    let mut path = "../".repeat(from.len() - shared);
    path.push_str(&to[shared..].join("/"));
    utf8_percent_encode(&path, PATH_ENCODING).to_string()
}

/// Copy `source`, writing `replace(reference)` over each reference it
/// returns a URL for.
pub(crate) fn rewrite(
    source: &str,
    references: &[Reference],
    mut replace: impl FnMut(&Reference) -> Option<String>,
) -> String {
    let mut out = String::with_capacity(source.len());
    let mut written = 0;
    for reference in references {
        let Some(url) = replace(reference) else {
            continue;
        };
        out.push_str(&source[written..reference.span.start]);
        if reference.function {
            out.push_str("url(");
        }
        cssparser::serialize_string(&url, &mut out).expect("write CSS string");
        if reference.function {
            out.push(')');
        }
        written = reference.span.end;
    }
    out.push_str(&source[written..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_url_form_and_nothing_else() {
        let source = r#"
            @import "base.css";
            @import url(theme.css) screen;
            a { background: url( 'a.png' ) }
            b { background: image-set("b.png" 1x, url(b2.png) 2x) }
            c { content: "not-a-url.png" }
            /* url(comment.png) */
        "#;
        let urls: Vec<_> = references(source)
            .unwrap()
            .into_iter()
            .map(|reference| reference.url)
            .collect();
        assert_eq!(urls, ["base.css", "theme.css", "a.png", "b.png", "b2.png"]);
    }

    #[test]
    fn a_url_this_does_not_understand_is_left_alone_and_the_rest_is_still_found() {
        let source = r#"
            a { background: url("a.png" cross-origin(anonymous)) }
            b { background: url(var(--image)) }
            c { background: url("c.png") }
        "#;
        let found = references(source).unwrap();
        let urls: Vec<_> = found
            .iter()
            .map(|reference| reference.url.as_str())
            .collect();
        assert_eq!(urls, ["c.png"]);
        let out = rewrite(source, &found, |_| Some("x.png".into()));
        assert!(
            out.contains(r#"url("a.png" cross-origin(anonymous))"#),
            "{out}"
        );
        assert!(out.contains("url(var(--image))"));
        assert!(out.contains(r#"c { background: url("x.png") }"#));
    }

    #[test]
    fn resolves_relative_paths_and_leaves_other_urls_alone() {
        let public = |logical: &str, suffix: &str| Target::Public {
            logical: logical.into(),
            suffix: suffix.into(),
        };
        assert_eq!(
            target("styles/app.css", "../images/a.png"),
            public("images/a.png", "")
        );
        assert_eq!(
            target("app.css", "./fonts/a.woff2?v=1#x"),
            public("fonts/a.woff2", "?v=1#x")
        );
        assert_eq!(
            target("app.css", "my%20file.png"),
            public("my file.png", "")
        );
        assert_eq!(target("app.css", "../secret"), Target::Outside);
        assert_eq!(target("app.css", "/api/image"), Target::RootRelative);
        for external in [
            "https://cdn.example/a.png",
            "//cdn.example/a.png",
            "data:image/png;base64,AA",
            "#mask",
            "",
        ] {
            assert_eq!(target("app.css", external), Target::External, "{external}");
        }
    }

    #[test]
    fn relative_urls_cross_directories_and_escape_names() {
        assert_eq!(
            relative(
                "immutable/styles/app.0123.css",
                "immutable/images/a.4567.png"
            ),
            "../images/a.4567.png"
        );
        assert_eq!(
            relative("immutable/app.css", "mutable/data.json"),
            "../mutable/data.json"
        );
        assert_eq!(
            relative("immutable/app.css", "immutable/my file.png"),
            "my%20file.png"
        );
    }

    #[test]
    fn rewriting_keeps_every_other_byte() {
        let source = "a{background:url(a.png)} b{background:url('b.png')} @import \"c.css\";";
        let found = references(source).unwrap();
        let out = rewrite(source, &found, |reference| {
            (reference.url != "b.png").then(|| format!("x/{}", reference.url))
        });
        assert_eq!(
            out,
            "a{background:url(\"x/a.png\")} b{background:url('b.png')} @import \"x/c.css\";"
        );
    }
}
