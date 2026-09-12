// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Serde round-trip coverage for every `Expr` variant and for `Value`.
//!
//! Runtime-template programs can store expressions as data, so
//! `serialize -> deserialize` must reproduce the original tree exactly for
//! the whole grammar.

use proa_ir::{Expr, Value};

fn lit(v: impl Into<Value>) -> Box<Expr> {
    Box::new(Expr::Lit { value: v.into() })
}

fn var(name: &'static str) -> proa_ir::VarId {
    proa_ir::VarId::new(name)
}

/// One expression per `Expr` variant (every variant in the enum).
fn corpus() -> Vec<Expr> {
    use Expr::*;
    vec![
        // Primitives
        Lit {
            value: Value::Int(42),
        },
        Signal {
            id: proa_ir::SignalId::new("count"),
        },
        Field {
            id: proa_ir::FieldId::new("email"),
        },
        Var { id: var("item") },
        Expr::scope_path(["product", "price"]),
        Response,
        Error,
        Event,
        Target,
        // Property access
        Expr::scope("product").prop("title"),
        Index {
            expr: Box::new(Expr::scope("items")),
            index: lit(0),
        },
        // Arithmetic
        Add {
            left: lit(1),
            right: lit(2),
        },
        Sub {
            left: lit(1),
            right: lit(2),
        },
        Mul {
            left: lit(2.5),
            right: lit(4),
        },
        Div {
            left: lit(1),
            right: lit(2),
        },
        Mod {
            left: lit(5),
            right: lit(3),
        },
        Neg { expr: lit(7) },
        // Comparison
        Eq {
            left: lit(1),
            right: lit(1),
        },
        Ne {
            left: lit(1),
            right: lit(2),
        },
        Lt {
            left: lit(1),
            right: lit(2),
        },
        Le {
            left: lit(1),
            right: lit(2),
        },
        Gt {
            left: lit(2),
            right: lit(1),
        },
        Ge {
            left: lit(2),
            right: lit(1),
        },
        // Logical
        And {
            left: lit(true),
            right: lit(false),
        },
        Or {
            left: lit(true),
            right: lit(false),
        },
        Not { expr: lit(true) },
        // Conditional
        Ternary {
            cond: lit(true),
            then: lit("yes"),
            else_: lit("no"),
        },
        Coalesce {
            left: lit(Value::Null),
            right: lit("fallback"),
        },
        // String operations
        Concat {
            parts: vec![Expr::lit("a"), Expr::lit("b")],
        },
        Len { expr: lit("abc") },
        Trim { expr: lit(" a ") },
        Lower { expr: lit("AB") },
        Upper { expr: lit("ab") },
        Contains {
            haystack: lit("haystack"),
            needle: lit("hay"),
        },
        StartsWith {
            string: lit("abc"),
            prefix: lit("a"),
        },
        EndsWith {
            string: lit("abc"),
            suffix: lit("c"),
        },
        Substring {
            string: lit("abcdef"),
            start: lit(1),
            end: Some(lit(3)),
        },
        Replace {
            string: lit("aaa"),
            from: lit("a"),
            to: lit("b"),
        },
        Split {
            string: lit("a,b"),
            delimiter: lit(","),
        },
        Join {
            array: Box::new(Array {
                items: vec![Expr::lit("a"), Expr::lit("b")],
            }),
            delimiter: lit("-"),
        },
        Matches {
            string: lit("abc"),
            pattern: "^a".into(),
        },
        // Array/Object construction
        Array {
            items: vec![Expr::lit(1), Expr::lit(2)],
        },
        Object {
            entries: vec![("a".into(), Expr::lit(1)), ("b".into(), Expr::lit(2))],
        },
        // Array operations
        Map {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            body: Box::new(Expr::var("x").add(1)),
        },
        Filter {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            predicate: Box::new(Expr::var("x").gt(0)),
        },
        Find {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            predicate: Box::new(Expr::var("x").eq(1)),
        },
        FindIndex {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            predicate: Box::new(Expr::var("x").eq(1)),
        },
        Some {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            predicate: Box::new(Expr::var("x").gt(0)),
        },
        Every {
            array: Box::new(Expr::scope("items")),
            var: var("x"),
            predicate: Box::new(Expr::var("x").gt(0)),
        },
        Reduce {
            array: Box::new(Expr::scope("items")),
            acc_var: var("acc"),
            item_var: var("x"),
            body: Box::new(Expr::var("acc").add(Expr::var("x"))),
            initial: lit(0),
        },
        Includes {
            array: Box::new(Expr::scope("items")),
            value: lit(1),
        },
        First {
            array: Box::new(Expr::scope("items")),
        },
        Last {
            array: Box::new(Expr::scope("items")),
        },
        Slice {
            array: Box::new(Expr::scope("items")),
            start: lit(1),
            end: Some(lit(3)),
        },
        Flat {
            array: Box::new(Expr::scope("items")),
            depth: Some(2),
        },
        Flat {
            array: Box::new(Expr::scope("items")),
            depth: None,
        },
        // Type checking / conversion
        IsNull {
            expr: lit(Value::Null),
        },
        IsNumber { expr: lit(1) },
        IsString { expr: lit("a") },
        IsArray {
            expr: Box::new(Array { items: vec![] }),
        },
        ParseInt { expr: lit("12") },
        ParseFloat { expr: lit("1.5") },
        ToString { expr: lit(1) },
        ToNumber { expr: lit("1") },
        // Math
        Abs { expr: lit(-1) },
        Round { expr: lit(1.5) },
        Floor { expr: lit(1.5) },
        Ceil { expr: lit(1.5) },
        Min {
            values: vec![Expr::lit(1), Expr::lit(2)],
        },
        Max {
            values: vec![Expr::lit(1), Expr::lit(2)],
        },
        Clamp {
            value: lit(5),
            min: lit(0),
            max: lit(10),
        },
        // Number formatting
        FormatNumber {
            value: lit(12.345),
            decimals: 2,
        },
        // Date/time
        Now,
        FormatDate {
            date: lit(0),
            format: "YYYY-MM-DD".into(),
        },
        ParseDate {
            string: lit("2026-01-01"),
            format: "YYYY-MM-DD".into(),
        },
        // Form-specific
        FieldHasError {
            id: proa_ir::FieldId::new("email"),
        },
        FieldError {
            id: proa_ir::FieldId::new("email"),
        },
        FieldTouched {
            id: proa_ir::FieldId::new("email"),
        },
        FieldDirty {
            id: proa_ir::FieldId::new("email"),
        },
        FormValid,
        FormDirty,
        FormData,
        // Validation helpers
        IsEmail {
            expr: lit("a@b.co"),
        },
        IsUrl {
            expr: lit("https://a.co"),
        },
        MatchesPattern {
            expr: lit("abc"),
            pattern: "^a".into(),
        },
        // Payment helpers
        CardType {
            expr: lit("4242424242424242"),
        },
    ]
}

#[test]
fn every_expr_variant_round_trips() {
    for expr in corpus() {
        let json = serde_json::to_string(&expr)
            .unwrap_or_else(|e| panic!("serialize failed for {expr:?}: {e}"));
        let back: Expr = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("deserialize failed for {json}: {e}"));
        assert_eq!(expr, back, "round trip mismatch for {json}");
    }
}

#[test]
fn corpus_covers_every_variant() {
    // Belt and braces: if a new variant is added to Expr, its serialized
    // "type" tag must appear in the corpus. The tag list below is the
    // complete set at the time of writing; extend both when adding variants.
    let tags: std::collections::BTreeSet<String> = corpus()
        .iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        tags.len(),
        85,
        "expected 85 distinct Expr variants, got {tags:?}"
    );
}

#[test]
fn deep_nested_expr_round_trips() {
    let expr = Expr::scope("cart")
        .prop("items")
        .index(0)
        .prop("price")
        .mul(Expr::lit(2))
        .format_number(2);
    let json = serde_json::to_string(&expr).unwrap();
    let back: Expr = serde_json::from_str(&json).unwrap();
    assert_eq!(expr, back);
}

#[test]
fn expr_deserializes_from_hand_written_json() {
    // Runtime authoring path: owned strings from arbitrary JSON.
    let json = r#"{
        "type": "prop",
        "expr": {"type": "scope", "path": ["product"]},
        "prop": "title"
    }"#;
    let expr: Expr = serde_json::from_str(json).unwrap();
    assert_eq!(expr, Expr::scope("product").prop("title"));
}

#[test]
fn value_round_trips() {
    let values = vec![
        Value::Null,
        Value::Bool(true),
        Value::Int(-7),
        Value::Int(i64::MAX),
        Value::Float(2.5),
        Value::Float(-0.125),
        Value::from("hello"),
        Value::Array(vec![Value::Int(1), Value::from("x"), Value::Null]),
        Value::object([
            ("name", Value::from("Widget")),
            ("price_cents", Value::Int(1999)),
            (
                "tags",
                Value::Array(vec![Value::from("new"), Value::from("sale")]),
            ),
            ("meta", Value::object([("weight", Value::Float(0.5))])),
        ]),
    ];
    for value in values {
        let json = serde_json::to_string(&value).unwrap();
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, back, "round trip mismatch for {json}");
    }
}

#[test]
fn value_object_serializes_as_json_map() {
    let value = Value::object([("a", Value::Int(1)), ("b", Value::from("x"))]);
    assert_eq!(serde_json::to_string(&value).unwrap(), r#"{"a":1,"b":"x"}"#);
}

#[test]
fn value_object_preserves_insertion_order() {
    let json = r#"{"z": 1, "a": 2, "m": 3}"#;
    let value: Value = serde_json::from_str(json).unwrap();
    match value {
        Value::Object(entries) => {
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_ref()).collect();
            assert_eq!(keys, vec!["z", "a", "m"]);
        }
        other => panic!("expected object, got {other:?}"),
    }
}
