// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Layout rules for multi-line `md!` / `md_sync!` templates.
//!
//! A Markdown template is one string literal, so the whitespace at the start
//! of each line is ambiguous: it is either Markdown (an indented code block,
//! a nested list item) or the indentation that keeps the template aligned with
//! the Rust code around it. Proa resolves this by treating the *common* prefix
//! of every line as layout. Inside a multiline control body, one additional
//! indentation level is layout too; deeper whitespace remains Markdown content.
//!
//! This module defines whole-template layout; `md::control` defines control-body
//! layout. The macros and formatter share both rules.

/// Whether a template literal carries layout indentation.
///
/// Only a template that opens with a newline is laid out across lines. A
/// single-line template such as `md_sync!("  indented")` is all content, so
/// its leading whitespace is left alone.
pub fn is_multiline_template(value: &str) -> bool {
    value.starts_with('\n')
}

/// Strip the leading newline and the layout indentation from a template.
///
/// Returns the text the template lowers to. Only the whitespace prefix shared
/// by every non-blank line is removed, so relative indentation (nested list
/// items, indented code blocks) survives. The final line holds the closing
/// quote: it is whitespace-only, so it never constrains the common prefix and
/// collapses to nothing.
///
/// Whitespace is compared by character rather than display width, so a
/// template mixing tabs and spaces finds no common prefix and is left alone.
pub fn dedent_template(value: &str) -> String {
    let Some(body) = value.strip_prefix('\n') else {
        return value.to_owned();
    };

    let common = layout_indent(body);
    if common.is_empty() {
        return body.to_owned();
    }

    let mut out = String::with_capacity(body.len());
    let mut lines = body.split('\n').peekable();
    while let Some(line) = lines.next() {
        // A blank line may be shorter than the common prefix (an editor
        // trimmed it), so strip only the part that actually matches.
        out.push_str(&line[common_prefix(common, line).len()..]);
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// The whitespace prefix shared by every non-blank line of an already
/// newline-stripped template body.
pub fn layout_indent(body: &str) -> &str {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(leading_whitespace)
        .reduce(common_prefix)
        .unwrap_or("")
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Longest common prefix of two strings, truncated to a `char` boundary.
fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let len = a
        .char_indices()
        .zip(b.chars())
        .take_while(|((_, x), y)| x == y)
        .map(|((idx, x), _)| idx + x.len_utf8())
        .last()
        .unwrap_or(0);
    &a[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_template_is_untouched() {
        assert_eq!(dedent_template("  indented"), "  indented");
        assert!(!is_multiline_template("  indented"));
    }

    #[test]
    fn flush_left_template_is_unchanged_apart_from_the_opening_newline() {
        assert_eq!(
            dedent_template("\n# Title\n\nBody.\n"),
            "# Title\n\nBody.\n"
        );
    }

    #[test]
    fn common_indentation_is_removed() {
        assert_eq!(
            dedent_template("\n        # Title\n\n        Body.\n    "),
            "# Title\n\nBody.\n"
        );
    }

    #[test]
    fn arbitrary_alignment_is_removed() {
        // The prefix need not be a multiple of the indent width, and the
        // closing-quote line need not line up with it.
        assert_eq!(
            dedent_template("\n             ## Title\n\n             Body.\n        "),
            "## Title\n\nBody.\n"
        );
    }

    #[test]
    fn relative_indentation_survives() {
        assert_eq!(
            dedent_template("\n    - top\n      - nested\n"),
            "- top\n  - nested\n"
        );
    }

    #[test]
    fn blank_lines_shorter_than_the_prefix_are_tolerated() {
        assert_eq!(
            dedent_template("\n        # Title\n  \n        Body.\n"),
            "# Title\n\nBody.\n"
        );
    }

    #[test]
    fn mixed_tabs_and_spaces_find_no_common_prefix() {
        assert_eq!(
            dedent_template("\n\t# Tab\n    # Space\n"),
            "\t# Tab\n    # Space\n"
        );
    }

    #[test]
    fn multibyte_content_is_not_split() {
        assert_eq!(
            dedent_template("\n    # Malibu — Beach\n"),
            "# Malibu — Beach\n"
        );
    }
}
