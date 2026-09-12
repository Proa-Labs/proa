// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::path::Path;

fn roots(source: &str) -> HtmlRoot {
    let inv = find_macros(source).into_iter().next().expect("macro");
    parse_invocation(source, &inv).expect("parse")
}

#[test]
fn layout_changes_are_render_preserving() {
    let a = roots("fn f() { html_sync! { <div class=\"x\">\n<span>\"hi\"</span></div> } }");
    let b = roots("fn f() { html_sync! {\n    <div class=\"x\">\n        <span>\"hi\"</span>\n    </div>\n} }");
    assert!(nodes_equal(&a.nodes, &b.nodes, false).is_ok());
}

#[test]
fn text_changes_fail_verification() {
    let a = roots("fn f() { html_sync! { <span>\"hi\"</span> } }");
    let b = roots("fn f() { html_sync! { <span>\"hi \"</span> } }");
    assert!(nodes_equal(&a.nodes, &b.nodes, false).is_err());
}

#[test]
fn verify_passes_on_formatter_output() {
    let source =
        "fn f() { html_sync! { <div class=\"x\" data-n={n}><span>\"hi\"</span>{body}</div> } }";
    let formatted = format_file(Path::new("sample.rs"), source).expect("format");
    let diags = verify_render_preserving(Path::new("sample.rs"), source, &formatted.output);
    assert!(diags.is_empty(), "verify failed: {diags:?}");
}

#[test]
fn verify_allows_nested_macro_name_canonicalization() {
    let source = "fn f() { html_sync! { <div>{if show { html_sync! { <button data_item on_click={handler}></button> }; }}</div> } }";
    let formatted = format_file(Path::new("sample.rs"), source).expect("format");
    let diags = verify_render_preserving(Path::new("sample.rs"), source, &formatted.output);
    assert!(diags.is_empty(), "verify failed: {diags:?}");
}

#[test]
fn verify_allows_rsjs_legacy_name_formatting() {
    let source = "#[rsjs::rsjs(component, client)]\nfn f() { html_sync! { <button data - item on - click={handler}></button> } }";
    let formatted = format_file(Path::new("sample.rs"), source).expect("format");
    let diags = verify_render_preserving(Path::new("sample.rs"), source, &formatted.output);
    assert!(diags.is_empty(), "verify failed: {diags:?}");
}

// ── Markdown templates ───────────────────────────────────────────────

/// The text a Markdown template lowers to, for asserting that formatting
/// moved the template without changing what it renders.
fn md_lowered(source: &str) -> Vec<String> {
    find_macros(source)
        .iter()
        .filter(|inv| inv.dialect == MacroDialect::Md)
        .filter_map(|inv| md_render_text(source, inv))
        .collect()
}

#[test]
fn anchors_md_template_to_the_macro_indentation() {
    let source = "impl A {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        md_sync! {\"
## {self.title}

{self.description}
\"}
        .render_md(cx)
    }
}";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
        formatted.contains("            ## {self.title}\n"),
        "template not re-anchored:\n{formatted}"
    );
    assert!(
        formatted.contains("\n            \"\n        }\n"),
        "closing quote or macro brace not canonically aligned:\n{formatted}"
    );
    assert_eq!(md_lowered(source), md_lowered(&formatted));
}

#[test]
fn md_formatting_is_idempotent() {
    let source = "fn f() {
    let s = md_sync! {\"
        # Title

        Body.
    \"};
}";
    let once = format_file(Path::new("sample.rs"), source).unwrap().output;
    let twice = format_file(Path::new("sample.rs"), &once).unwrap().output;
    assert_eq!(once, twice);
    assert_eq!(md_lowered(source), md_lowered(&once));
}

#[test]
fn md_formatting_preserves_relative_indentation() {
    let source = "fn f() {
    let s = md_sync! {\"
# List

- top
  - nested

    code_line();
\"};
}";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    // Macro indent 4 + one level = 8, and the relative offsets survive.
    assert!(
        formatted.contains("        - top\n          - nested\n"),
        "{formatted}"
    );
    assert!(
        formatted.contains("\n            code_line();\n"),
        "{formatted}"
    );
    assert_eq!(md_lowered(source), md_lowered(&formatted));
}

#[test]
fn md_formatting_handles_raw_strings() {
    let source = "fn f() {
    let s = md_sync! {r###\"
# {title}

A \"quoted\" word.
\"###};
}";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(
        formatted.contains("r###\"\n        # {title}\n"),
        "{formatted}"
    );
    assert!(formatted.contains("\n        \"###\n    }"), "{formatted}");
    assert_eq!(md_lowered(source), md_lowered(&formatted));
}

#[test]
fn md_single_line_template_is_left_alone() {
    // Leading whitespace in a single-line template is content.
    let source = "fn f() { let s = md_sync!(\"    indented\"); }";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
}

#[test]
fn md_token_mode_body_is_left_alone() {
    let source = "fn f() { let s = md_sync! { \"# \" {title} \"\\n\" }; }";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
}

#[test]
fn md_body_with_a_comment_is_left_alone() {
    // Rebuilding the body from the literal token would drop the comment.
    let source = "fn f() {
    let s = md_sync! {
        // the product heading
        \"
# Title
\"
    };
}";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
}

#[test]
fn md_escaped_newlines_are_left_alone() {
    // `\n` escapes decouple source lines from rendered lines, so the
    // layout indentation is not safe to rewrite.
    let source = "fn f() {
    let s = md_sync! {\"
# Title\\n\\nBody.
\"};
}";
    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
}

#[test]
fn verify_accepts_md_reindentation() {
    let source = "impl A {
    fn f(&self) {
        md_sync! {\"
# {self.title}

Body.
\"}
        .render_md(cx)
    }
}";
    let formatted = format_file(Path::new("sample.rs"), source).expect("format");
    let diags = verify_render_preserving(Path::new("sample.rs"), source, &formatted.output);
    assert!(diags.is_empty(), "verify failed: {diags:?}");
}

#[test]
fn verify_rejects_md_bodies_that_render_differently() {
    let source = "fn f() {
    let s = md_sync! {\"
# Title

Body.
\"};
}";
    // Simulate a formatter bug: indent one line of a template whose other
    // lines stay flush left. The common prefix stays empty, so those four
    // spaces survive lowering and turn the paragraph into a code block.
    let corrupted = source.replace("Body.", "    Body.");
    let diags = verify_render_preserving(Path::new("sample.rs"), source, &corrupted);
    assert!(
        diags.iter().any(|d| d.code == "format/verify"),
        "expected a verify failure: {diags:?}"
    );
}
