// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_dsl::md::{ExprLocation, MdTemplateInput, TplNode};
use quote::ToTokens;

#[test]
fn commonmark_code_contexts_match_the_shared_editor_corpus() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown_contexts.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let input = format!("r#####\"{}\"#####", case["markdown"].as_str().unwrap());
        let template = syn::parse_str::<MdTemplateInput>(&input).unwrap();
        let source = template.lit.to_token_stream().to_string();
        let nodes = template
            .parse_nodes()
            .unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
        let actual: Vec<_> = nodes
            .into_iter()
            .filter_map(|node| {
                let TplNode::Expr(expr) = node else {
                    return None;
                };
                let ExprLocation::Literal { range, .. } = expr.location else {
                    panic!(
                        "missing source map in {}: {:?} / {:?}",
                        case["name"],
                        source,
                        template.lit.value()
                    )
                };
                Some(
                    source[range]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .collect();
        let expected: Vec<_> = case["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slot| {
                slot.as_str()
                    .unwrap()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        assert_eq!(actual, expected, "{}", case["name"]);
    }
}
