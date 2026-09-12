// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cross-checks the server evaluator against the ACTUAL JavaScript the
//! emitter produces, executed by node. For each corpus expression the
//! test compares JS `String(<emitted expr>)` with the server's
//! `ToString(eval(expr))`; string rendering is exactly what a template
//! slot observes, so this pins both the value semantics and number
//! formatting in one shot.
//!
//! Skips silently when `node` is not on PATH (CI without a JS runtime);
//! SEMANTICS.md remains the contract either way.

use proa_ir::codegen::expr_to_js;
use proa_ir::eval::{eval, ScopeStack};
use proa_ir::{Expr, Value};
use std::process::Command;

fn lit(v: impl Into<Value>) -> Expr {
    Expr::Lit { value: v.into() }
}

fn scope_value() -> Value {
    Value::object([
        ("count", Value::Int(7)),
        ("price_cents", Value::Int(123_456)),
        ("title", Value::from("Deluxe Widget")),
        (
            "items",
            Value::Array(vec![Value::Int(3), Value::Int(1), Value::Int(2)]),
        ),
        (
            "product",
            Value::object([("name", Value::from("Gadget")), ("stock", Value::Int(0))]),
        ),
    ])
}

/// Server-dialect expressions whose rendered output must be identical in
/// both worlds. Only JS-throwing shapes (null property access) and the
/// deliberately documented divergences are excluded.
fn corpus() -> Vec<Expr> {
    vec![
        // Arithmetic and number formatting
        lit(1).div(lit(2)),
        lit(1).div(lit(0)),
        lit(0).div(lit(0)),
        lit(-1).div(lit(0)),
        Expr::Mod {
            left: Box::new(lit(-5)),
            right: Box::new(lit(3)),
        },
        lit(0.1).add(lit(0.2)),
        lit(9_007_199_254_740_992i64).add(lit(1)),
        lit(true).add(lit(true)),
        lit("5").mul(lit("2")),
        lit(1e21),
        lit(1e-7),
        lit(-0.0),
        // Concat is the + chain
        Expr::Concat {
            parts: vec![lit(1), lit(2), lit("x")],
        },
        Expr::Concat {
            parts: vec![lit("total: "), Expr::scope("count")],
        },
        // FormatNumber == toFixed
        lit(0.125).format_number(2),
        lit(-0.125).format_number(2),
        lit(1.005).format_number(2),
        lit(2.675).format_number(2),
        lit(0.995).format_number(2),
        lit(9.995).format_number(2),
        lit(0.999).format_number(2),
        lit(1234.5678).format_number(2),
        lit(5).format_number(0),
        lit(2.5).format_number(0),
        lit(-2.5).format_number(0),
        lit(-0.0004).format_number(2),
        lit(f64::NAN).format_number(2),
        Expr::scope("price_cents").div(lit(100)).format_number(2),
        Expr::Concat {
            parts: vec![
                lit("$"),
                Expr::scope("price_cents").div(lit(100)).format_number(2),
            ],
        },
        // Comparison (rendered as booleans)
        lit(1).eq(lit(1.0)),
        lit(1).eq(lit("1")),
        lit("5").lt(lit(10)),
        lit("10").lt(lit("9")),
        lit("abc").ge(lit(5)),
        lit(f64::NAN).eq(lit(f64::NAN)),
        // Logic on booleans (the shared dialect)
        lit(true).and(lit(false)),
        lit(false).or(lit(true)),
        lit(true).not(),
        Expr::Ternary {
            cond: Box::new(Expr::scope("product").prop("stock").gt(lit(0))),
            then: Box::new(lit("in stock")),
            else_: Box::new(lit("sold out")),
        },
        Expr::Coalesce {
            left: Box::new(lit(Value::Null)),
            right: Box::new(lit("fallback")),
        },
        Expr::Coalesce {
            left: Box::new(lit(0)),
            right: Box::new(lit("fallback")),
        },
        // Strings
        lit("héllo👍").len(),
        lit("  padded  ").trim(),
        lit("MiXeD").lower(),
        lit("MiXeD").upper(),
        lit("haystack").contains(lit("stack")),
        lit("a1b").contains(lit(1)),
        Expr::StartsWith {
            string: Box::new(Expr::scope("title")),
            prefix: Box::new(lit("Deluxe")),
        },
        Expr::EndsWith {
            string: Box::new(Expr::scope("title")),
            suffix: Box::new(lit("Widget")),
        },
        Expr::Substring {
            string: Box::new(lit("hello")),
            start: Box::new(lit(3)),
            end: Some(Box::new(lit(1))),
        },
        Expr::Substring {
            string: Box::new(lit("hello")),
            start: Box::new(lit(-2)),
            end: Some(Box::new(lit(2))),
        },
        Expr::Replace {
            string: Box::new(lit("aaa")),
            from: Box::new(lit("a")),
            to: Box::new(lit("b")),
        },
        Expr::Split {
            string: Box::new(lit("a,b,c")),
            delimiter: Box::new(lit(",")),
        },
        Expr::Split {
            string: Box::new(lit("")),
            delimiter: Box::new(lit(",")),
        },
        Expr::Join {
            array: Box::new(Expr::Array {
                items: vec![lit(1), lit(Value::Null), lit("x")],
            }),
            delimiter: Box::new(lit("-")),
        },
        // Indexing / access
        Expr::scope("items").index(lit(1)),
        Expr::scope("items").index(lit("1")),
        Expr::scope("items").len(),
        Expr::scope("product").prop("name"),
        lit("abc").index(lit(1)),
        Expr::First {
            array: Box::new(Expr::scope("items")),
        },
        Expr::Last {
            array: Box::new(Expr::scope("items")),
        },
        Expr::Slice {
            array: Box::new(Expr::scope("items")),
            start: Box::new(lit(-2)),
            end: None,
        },
        Expr::Flat {
            array: Box::new(Expr::Array {
                items: vec![
                    lit(1),
                    Expr::Array {
                        items: vec![
                            lit(2),
                            Expr::Array {
                                items: vec![lit(3)],
                            },
                        ],
                    },
                ],
            }),
            depth: None,
        },
        // Combinators
        Expr::Map {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            body: Box::new(Expr::var("x").mul(lit(10))),
        },
        Expr::Filter {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            predicate: Box::new(Expr::var("x").gt(lit(1))),
        },
        Expr::Find {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            predicate: Box::new(Expr::var("x").eq(lit(2))),
        },
        Expr::FindIndex {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            predicate: Box::new(Expr::var("x").eq(lit(99))),
        },
        Expr::Some {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            predicate: Box::new(Expr::var("x").gt(lit(2))),
        },
        Expr::Every {
            array: Box::new(Expr::scope("items")),
            var: proa_ir::VarId::new("x"),
            predicate: Box::new(Expr::var("x").gt(lit(0))),
        },
        Expr::Reduce {
            array: Box::new(Expr::scope("items")),
            acc_var: proa_ir::VarId::new("acc"),
            item_var: proa_ir::VarId::new("x"),
            body: Box::new(Expr::var("acc").add(Expr::var("x"))),
            initial: Box::new(lit(0)),
        },
        Expr::Includes {
            array: Box::new(Expr::scope("items")),
            value: Box::new(lit(2)),
        },
        // Type/convert
        Expr::ToNumber {
            expr: Box::new(lit(" 12 ")),
        },
        Expr::ToNumber {
            expr: Box::new(lit("0x10")),
        },
        Expr::ToNumber {
            expr: Box::new(lit("abc")),
        },
        Expr::ParseInt {
            expr: Box::new(lit("12abc")),
        },
        Expr::ParseFloat {
            expr: Box::new(lit("3.14xyz")),
        },
        Expr::IsNumber {
            expr: Box::new(lit(1)),
        },
        Expr::IsString {
            expr: Box::new(lit(1)),
        },
        Expr::IsArray {
            expr: Box::new(Expr::scope("items")),
        },
        Expr::IsNull {
            expr: Box::new(lit(Value::Null)),
        },
        // Math
        Expr::Round {
            expr: Box::new(lit(2.5)),
        },
        Expr::Round {
            expr: Box::new(lit(-2.5)),
        },
        Expr::Round {
            expr: Box::new(lit(0.499_999_999_999_999_94)),
        },
        Expr::Abs {
            expr: Box::new(lit(-3.5)),
        },
        Expr::Floor {
            expr: Box::new(lit(-1.5)),
        },
        Expr::Ceil {
            expr: Box::new(lit(-1.5)),
        },
        Expr::Min {
            values: vec![lit(3), lit("2")],
        },
        Expr::Min { values: vec![] },
        Expr::Max { values: vec![] },
        Expr::Clamp {
            value: Box::new(lit(10)),
            min: Box::new(lit(2)),
            max: Box::new(lit(5)),
        },
        // Matches
        Expr::Matches {
            string: Box::new(lit("abc1")),
            pattern: "^[a-z]+\\d$".into(),
        },
    ]
}

#[test]
fn evaluator_matches_node_output() {
    let Some(node) = find_node() else {
        eprintln!("skipping js_conformance: node not found on PATH");
        return;
    };

    let root = scope_value();
    let scope = ScopeStack::new(&root);
    let corpus = corpus();

    // One node invocation for the whole corpus.
    let mut script = String::new();
    script.push_str("const $scope = ");
    script.push_str(&serde_json::to_string(&root).unwrap());
    script.push_str(";\nconst results = [];\n");
    for expr in &corpus {
        script.push_str("results.push(String(");
        script.push_str(&expr_to_js(expr));
        script.push_str("));\n");
    }
    script.push_str("console.log(JSON.stringify(results));\n");

    let output = Command::new(node)
        .arg("-e")
        .arg(&script)
        .output()
        .expect("failed to run node");
    assert!(
        output.status.success(),
        "node failed:\n{}\nscript:\n{script}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let js_results: Vec<String> = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(js_results.len(), corpus.len());

    for (expr, js_result) in corpus.iter().zip(&js_results) {
        let server_value = eval(expr, &scope)
            .unwrap_or_else(|e| panic!("server eval failed for {}: {e}", expr_to_js(expr)));
        let rendered = Expr::ToString {
            expr: Box::new(expr.clone()),
        };
        let server_result = match eval(&rendered, &scope).unwrap() {
            Value::String(s) => s.to_string(),
            other => panic!("ToString produced {other:?}"),
        };
        assert_eq!(
            &server_result,
            js_result,
            "server/client divergence for {} (server value {server_value:?})",
            expr_to_js(expr)
        );
    }
}

fn find_node() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|candidate| candidate.is_file())
}
