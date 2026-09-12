// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server-dialect validator: client-only, impure, and unsupported nodes
//! are rejected with the variant name and a path into the tree.

use proa_ir::dialect::{
    validate_server_dialect, validate_server_dialect_bounded, DialectError, DialectErrorReason,
};
use proa_ir::{Expr, Value};

fn lit(v: impl Into<Value>) -> Expr {
    Expr::Lit { value: v.into() }
}

#[test]
fn server_dialect_expression_passes() {
    // A realistic storefront slot: conditional money formatting over the
    // page scope with combinators.
    let subtotal = Expr::Reduce {
        array: Box::new(Expr::scope("cart").prop("items")),
        acc_var: proa_ir::VarId::new("acc"),
        item_var: proa_ir::VarId::new("item"),
        body: Box::new(
            Expr::var("acc").add(
                Expr::var("item")
                    .prop("price_cents")
                    .mul(Expr::var("item").prop("qty")),
            ),
        ),
        initial: Box::new(lit(0)),
    };
    let expr = Expr::Ternary {
        cond: Box::new(Expr::scope("cart").prop("items").len().gt(lit(0))),
        then: Box::new(Expr::Concat {
            parts: vec![lit("$"), subtotal.div(lit(100)).format_number(2)],
        }),
        else_: Box::new(lit("Cart is empty")),
    };
    assert_eq!(validate_server_dialect(&expr), Ok(()));
}

#[test]
fn client_only_root_is_rejected_with_variant_and_empty_path() {
    let err = validate_server_dialect(&Expr::signal("count")).unwrap_err();
    assert_eq!(
        err,
        vec![DialectError {
            variant: "Signal",
            path: String::new(),
            reason: DialectErrorReason::ClientOnly,
        }]
    );
}

#[test]
fn nested_client_only_node_reports_its_path() {
    // Ternary { cond: Gt { left: Signal, .. }, then: lit, else_: Field }
    let expr = Expr::Ternary {
        cond: Box::new(Expr::signal("count").gt(lit(0))),
        then: Box::new(lit("some")),
        else_: Box::new(Expr::field("fallback")),
    };
    let errors = validate_server_dialect(&expr).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].variant, "Signal");
    assert_eq!(errors[0].path, "cond.left");
    assert_eq!(errors[0].reason, DialectErrorReason::ClientOnly);
    assert_eq!(errors[1].variant, "Field");
    assert_eq!(errors[1].path, "else");
}

#[test]
fn indexed_paths_point_at_the_offending_element() {
    let expr = Expr::Concat {
        parts: vec![lit("a"), lit("b"), Expr::Now],
    };
    let errors = validate_server_dialect(&expr).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].variant, "Now");
    assert_eq!(errors[0].path, "parts[2]");
    assert_eq!(errors[0].reason, DialectErrorReason::Impure);
}

#[test]
fn combinator_bodies_are_walked() {
    let expr = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        body: Box::new(Expr::var("x").add(Expr::Response)),
    };
    let errors = validate_server_dialect(&expr).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].variant, "Response");
    assert_eq!(errors[0].path, "body.right");
}

#[test]
fn all_client_only_variants_are_rejected() {
    let cases: Vec<(Expr, &str, DialectErrorReason)> = vec![
        (Expr::signal("s"), "Signal", DialectErrorReason::ClientOnly),
        (Expr::field("f"), "Field", DialectErrorReason::ClientOnly),
        (Expr::Response, "Response", DialectErrorReason::ClientOnly),
        (Expr::Error, "Error", DialectErrorReason::ClientOnly),
        (Expr::Event, "Event", DialectErrorReason::ClientOnly),
        (Expr::Target, "Target", DialectErrorReason::ClientOnly),
        (
            Expr::FieldHasError {
                id: proa_ir::FieldId::new("f"),
            },
            "FieldHasError",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::FieldError {
                id: proa_ir::FieldId::new("f"),
            },
            "FieldError",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::FieldTouched {
                id: proa_ir::FieldId::new("f"),
            },
            "FieldTouched",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::FieldDirty {
                id: proa_ir::FieldId::new("f"),
            },
            "FieldDirty",
            DialectErrorReason::ClientOnly,
        ),
        (Expr::FormValid, "FormValid", DialectErrorReason::ClientOnly),
        (Expr::FormDirty, "FormDirty", DialectErrorReason::ClientOnly),
        (Expr::FormData, "FormData", DialectErrorReason::ClientOnly),
        (
            Expr::IsEmail {
                expr: Box::new(lit("a@b.co")),
            },
            "IsEmail",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::IsUrl {
                expr: Box::new(lit("https://a.co")),
            },
            "IsUrl",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::MatchesPattern {
                expr: Box::new(lit("x")),
                pattern: "^x$".into(),
            },
            "MatchesPattern",
            DialectErrorReason::ClientOnly,
        ),
        (
            Expr::card_type(lit("4242")),
            "CardType",
            DialectErrorReason::ClientOnly,
        ),
        (Expr::Now, "Now", DialectErrorReason::Impure),
        (
            Expr::ParseDate {
                string: Box::new(lit("2026-01-01")),
                format: "YYYY-MM-DD".into(),
            },
            "ParseDate",
            DialectErrorReason::Unsupported,
        ),
    ];
    for (expr, variant, reason) in cases {
        let errors =
            validate_server_dialect(&expr).expect_err(&format!("{variant} should be rejected"));
        assert_eq!(errors.len(), 1, "{variant}");
        assert_eq!(errors[0].variant, variant);
        assert_eq!(errors[0].reason, reason);
    }
}

#[test]
fn multiple_errors_are_all_collected() {
    let expr = Expr::Array {
        items: vec![Expr::signal("a"), lit(1), Expr::Now, Expr::field("b")],
    };
    let errors = validate_server_dialect(&expr).unwrap_err();
    let summary: Vec<(&str, &str)> = errors
        .iter()
        .map(|e| (e.variant, e.path.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Signal", "items[0]"),
            ("Now", "items[2]"),
            ("Field", "items[3]"),
        ]
    );
}

#[test]
fn bounded_validation_stops_after_the_requested_error_count() {
    let expr = Expr::Array {
        items: (0..1_024).map(|_| Expr::Now).collect(),
    };

    let errors = validate_server_dialect_bounded(&expr, 2).unwrap_err();

    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].path, "items[0]");
    assert_eq!(errors[1].path, "items[1]");
}

#[test]
fn display_is_actionable() {
    let expr = Expr::Ternary {
        cond: Box::new(Expr::signal("open").eq(lit(true))),
        then: Box::new(lit(1)),
        else_: Box::new(lit(2)),
    };
    let errors = validate_server_dialect(&expr).unwrap_err();
    assert_eq!(
        errors[0].to_string(),
        "client-only expression `Signal` at `cond.left`"
    );
}

#[test]
fn validator_accepted_expressions_are_evaluable() {
    // The validator and the evaluator must agree on the dialect boundary:
    // anything accepted here must not hit ClientOnly/Unsupported at eval.
    let root = Value::object([(
        "cart",
        Value::object([(
            "items",
            Value::Array(vec![Value::object([
                ("price_cents", Value::Int(1999)),
                ("qty", Value::Int(2)),
            ])]),
        )]),
    )]);
    let scope = proa_ir::eval::ScopeStack::new(&root);

    let expr = Expr::Concat {
        parts: vec![
            lit("$"),
            Expr::Reduce {
                array: Box::new(Expr::scope("cart").prop("items")),
                acc_var: proa_ir::VarId::new("acc"),
                item_var: proa_ir::VarId::new("item"),
                body: Box::new(
                    Expr::var("acc").add(
                        Expr::var("item")
                            .prop("price_cents")
                            .mul(Expr::var("item").prop("qty")),
                    ),
                ),
                initial: Box::new(lit(0)),
            }
            .div(lit(100))
            .format_number(2),
        ],
    };
    assert_eq!(validate_server_dialect(&expr), Ok(()));
    assert_eq!(
        proa_ir::eval::eval(&expr, &scope).unwrap(),
        Value::from("$39.98")
    );
}
