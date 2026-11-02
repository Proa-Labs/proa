// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_dsl::md::{parse_tpl, TplNode};
use proptest::prelude::*;

fn text_only(source: &str) -> String {
    parse_tpl(source)
        .unwrap()
        .into_iter()
        .map(|node| match node {
            TplNode::Text(text) => text,
            TplNode::Expr(_) => panic!("escaped text became a Rust slot: {source:?}"),
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    // Braces, quotes, slashes, whitespace and Unicode in prose survive exactly
    // one escape/decode pass. Backticks/fences deliberately have other rules.
    #[test]
    fn doubled_braces_round_trip_prose(
        chars in prop::collection::vec(prop::sample::select(vec![
            '{', '}', '\\', '"', '\'', '\n', '\r', '\t', ':', ' ', 'a', 'é', '🦀',
        ]), 0..128)
    ) {
        let original: String = chars.into_iter().collect();
        let escaped = original.replace('{', "{{").replace('}', "}}");
        prop_assert_eq!(text_only(&escaped), original);
    }

    #[test]
    fn escaped_braces_can_surround_one_active_slot(left in 0..32usize, right in 0..32usize) {
        let source = format!("{}{{value}}{}", "{{".repeat(left), "}}".repeat(right));
        let mut slots = 0;
        let output: String = parse_tpl(&source).unwrap().into_iter().map(|node| match node {
            TplNode::Text(text) => text,
            TplNode::Expr(expr) => {
                assert!(matches!(&expr.expr, syn::Expr::Path(path) if path.path.is_ident("value")));
                slots += 1;
                "Ada".into()
            }
        }).collect();
        prop_assert_eq!(slots, 1);
        prop_assert_eq!(output, format!("{}Ada{}", "{".repeat(left), "}".repeat(right)));
    }

    #[test]
    fn quoted_fences_preserve_arbitrary_braces(
        chars in prop::collection::vec(prop::sample::select(vec![
            '{', '}', '\\', '"', '\'', '\n', '\t', ':', ' ', 'a', 'é', '🦀',
        ]), 0..128),
        depth in 1..5usize,
        spaces in 0..4usize,
        ticks in any::<bool>(),
        length in 3..8usize,
    ) {
        let prefix = format!("{}> ", " ".repeat(spaces)).repeat(depth);
        let marker = if ticks { "`" } else { "~" }.repeat(length);
        let original: String = chars.into_iter().collect();
        let body = original.replace('\n', &format!("\n{prefix}"));
        let code = format!("{prefix}{marker}text\n{prefix}{body}\n{prefix}{marker}\n");
        let nodes = parse_tpl(&format!("{code}{{value}}")).unwrap();
        prop_assert_eq!(nodes.len(), 2);
        let TplNode::Text(text) = &nodes[0] else { panic!("code became a slot") };
        prop_assert_eq!(text, &code);
        prop_assert!(matches!(&nodes[1], TplNode::Expr(_)));
    }
}

#[test]
fn unmatched_brace_runs_are_rejected() {
    for count in 0..128 {
        for brace in ['{', '}'] {
            let source = brace.to_string().repeat(count);
            if count % 2 == 0 {
                assert_eq!(text_only(&source), brace.to_string().repeat(count / 2));
            } else {
                assert!(parse_tpl(&source).is_err(), "{source}");
            }
        }
    }
}

#[test]
fn strings_and_comments_inside_slots_are_not_template_escapes() {
    for source in [
        r#"{"{{name}}"}"#,
        r#"{r"{{name}}"}"#,
        "{ /* }}} {{{ */ name }",
        "{ // }}} {{{\n name }",
        r#"{format!("{{{name}}}")}"#,
        r##"{md_sync! { r#"{{{name}}}"# }}"##,
    ] {
        assert!(
            matches!(parse_tpl(source).unwrap().as_slice(), [TplNode::Expr(_)]),
            "{source}"
        );
    }
}

#[test]
fn quoted_fences_keep_braces_verbatim_and_stop_at_the_quote_boundary() {
    for source in [
        "> ~~~text\n> {{name}} {not_rust\n> ~~~\n{name}",
        "> ```text\n> {{name}} {not_rust\n> ```\n{name}",
        "> > ~~~text\n> > {{name}} {not_rust\n> > ~~~\n{name}",
        "> ~~~text\n> {{name}} {not_rust\n{name}",
        "> > ~~~text\n> > {{name}} {not_rust\n> {name}",
        "> ~~~~text\n> ~~~\n> {{name}} {not_rust\n> ~~~~\n{name}",
        "> ~~~text\n> ~~~\u{a0}\n> {{name}} {not_rust\n> ~~~\n{name}",
    ] {
        for newline in ["\n", "\r\n"] {
            let source = source.replace('\n', newline);
            let prefix = source.strip_suffix("{name}").unwrap();
            let nodes = parse_tpl(&source).unwrap();
            assert!(
                matches!(nodes.as_slice(), [TplNode::Text(text), TplNode::Expr(_)] if text == prefix),
                "{source}: {nodes:?}"
            );
        }
    }
}
