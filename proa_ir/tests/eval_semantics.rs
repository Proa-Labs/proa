// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server evaluator semantics, asserted against hand-computed JavaScript
//! behavior. Every expectation here was checked against the ECMAScript
//! semantics the JS emitters rely on; see proa_ir/SEMANTICS.md.

use proa_ir::eval::{eval, EvalError, ScopeStack};
use proa_ir::{Expr, Value};

fn lit(v: impl Into<Value>) -> Expr {
    Expr::Lit { value: v.into() }
}

fn boxed(v: impl Into<Value>) -> Box<Expr> {
    Box::new(lit(v))
}

fn empty_scope() -> Value {
    Value::object([] as [(&str, Value); 0])
}

fn run(expr: &Expr) -> Result<Value, EvalError> {
    let root = empty_scope();
    let scope = ScopeStack::new(&root);
    eval(expr, &scope)
}

fn ok(expr: &Expr) -> Value {
    run(expr).expect("expected successful evaluation")
}

/// Evaluate and render like a template slot would (JS `String(x)`).
fn ok_string(expr: &Expr) -> String {
    let stringified = Expr::ToString {
        expr: Box::new(expr.clone()),
    };
    match ok(&stringified) {
        Value::String(s) => s.to_string(),
        other => panic!("ToString produced non-string {other:?}"),
    }
}

fn num(expr: &Expr) -> f64 {
    match ok(expr) {
        Value::Int(i) => i as f64,
        Value::Float(f) => f,
        other => panic!("expected number, got {other:?}"),
    }
}

fn b(expr: &Expr) -> bool {
    match ok(expr) {
        Value::Bool(v) => v,
        other => panic!("expected bool, got {other:?}"),
    }
}

// ================================================================
// Number model: JS f64 arithmetic
// ================================================================

#[test]
fn int_division_is_float() {
    // JS: 1 / 2 === 0.5
    let e = lit(1).div(lit(2));
    assert_eq!(num(&e), 0.5);
}

#[test]
fn arithmetic_loses_precision_beyond_2_53_like_js() {
    // JS: 9007199254740992 + 1 === 9007199254740992
    let e = lit(9_007_199_254_740_992i64).add(lit(1));
    assert_eq!(num(&e), 9_007_199_254_740_992.0);
    assert_eq!(ok_string(&e), "9007199254740992");
}

#[test]
fn division_by_zero_is_infinity() {
    // JS: 1/0 === Infinity, -1/0 === -Infinity, 0/0 is NaN
    assert_eq!(num(&lit(1).div(lit(0))), f64::INFINITY);
    assert_eq!(ok_string(&lit(1).div(lit(0))), "Infinity");
    assert_eq!(ok_string(&lit(-1).div(lit(0))), "-Infinity");
    assert_eq!(ok_string(&lit(0).div(lit(0))), "NaN");
}

#[test]
fn modulo_is_js_remainder() {
    // JS: -5 % 3 === -2 (sign of dividend), 5.5 % 2 === 1.5
    assert_eq!(
        num(&Expr::Mod {
            left: boxed(-5),
            right: boxed(3),
        }),
        -2.0
    );
    assert_eq!(
        num(&Expr::Mod {
            left: boxed(5.5),
            right: boxed(2),
        }),
        1.5
    );
}

#[test]
fn string_operands_coerce_in_arithmetic() {
    // JS: "5" * "2" === 10, "5" - 2 === 3, [5] - 1 === 4, null + 1 === 1
    assert_eq!(num(&lit("5").mul(lit("2"))), 10.0);
    assert_eq!(num(&lit("5").sub(lit(2))), 3.0);
    let arr = Expr::Array {
        items: vec![lit(5)],
    };
    assert_eq!(
        num(&Expr::Sub {
            left: Box::new(arr),
            right: boxed(1),
        }),
        4.0
    );
    assert_eq!(num(&lit(Value::Null).add(lit(1))), 1.0);
}

#[test]
fn bool_operands_coerce_in_arithmetic() {
    // JS: true + true === 2
    assert_eq!(num(&lit(true).add(lit(true))), 2.0);
}

#[test]
fn number_to_string_matches_js() {
    // JS: String(3.0) === "3", String(0.1) === "0.1",
    //     String(1e21) === "1e+21", String(1e-7) === "1e-7"
    assert_eq!(ok_string(&lit(3.0)), "3");
    assert_eq!(ok_string(&lit(0.1)), "0.1");
    assert_eq!(ok_string(&lit(1e21)), "1e+21");
    assert_eq!(ok_string(&lit(1e-7)), "1e-7");
    assert_eq!(ok_string(&lit(0.1).add(lit(0.2))), "0.30000000000000004");
    assert_eq!(ok_string(&lit(-0.0)), "0");
}

// ================================================================
// Concat is the JS `+` chain, not string joining
// ================================================================

#[test]
fn concat_of_numbers_adds_like_the_emitted_plus_chain() {
    // The emitter compiles Concat to (a + b + c); JS: (1 + 2 + "x") === "3x"
    let e = Expr::Concat {
        parts: vec![lit(1), lit(2), lit("x")],
    };
    assert_eq!(ok(&e), Value::from("3x".to_string()));

    // JS: ("x" + 1 + 2) === "x12"
    let e = Expr::Concat {
        parts: vec![lit("x"), lit(1), lit(2)],
    };
    assert_eq!(ok(&e), Value::from("x12".to_string()));
}

#[test]
fn concat_edge_arities() {
    // Zero parts -> "" and a single part passes through unchanged,
    // matching FormCodegen's emission.
    assert_eq!(ok(&Expr::Concat { parts: vec![] }), Value::from(""));
    assert_eq!(
        ok(&Expr::Concat {
            parts: vec![lit(5)]
        }),
        Value::Int(5)
    );
}

#[test]
fn concat_stringifies_null_like_js_plus() {
    // JS: "a" + null === "anull"
    let e = Expr::Concat {
        parts: vec![lit("a"), lit(Value::Null)],
    };
    assert_eq!(ok(&e), Value::from("anull".to_string()));
}

// ================================================================
// Null propagation
// ================================================================

#[test]
fn missing_scope_and_props_null_propagate() {
    let root = Value::object([("product", Value::object([("title", Value::from("Widget"))]))]);
    let scope = ScopeStack::new(&root);

    let hit = Expr::scope("product").prop("title");
    assert_eq!(eval(&hit, &scope).unwrap(), Value::from("Widget"));

    let miss = Expr::scope("nope").prop("deep").prop("deeper");
    assert_eq!(eval(&miss, &scope).unwrap(), Value::Null);

    let scope_path_miss = Expr::scope_path(["product", "missing", "leaf"]);
    assert_eq!(eval(&scope_path_miss, &scope).unwrap(), Value::Null);
}

#[test]
fn index_misses_are_null() {
    let arr = || Expr::Array {
        items: vec![lit(10), lit(20)],
    };
    // JS: arr[-1] and arr[1.5] are undefined; arr[1.0] and arr["1"] hit.
    assert_eq!(ok(&arr().index(lit(-1))), Value::Null);
    assert_eq!(ok(&arr().index(lit(1.5))), Value::Null);
    assert_eq!(ok(&arr().index(lit(1.0))), Value::Int(20));
    assert_eq!(ok(&arr().index(lit("1"))), Value::Int(20));
    assert_eq!(ok(&arr().index(lit("01"))), Value::Null);
    assert_eq!(ok(&arr().index(lit(2))), Value::Null);
}

#[test]
fn coalesce_only_fires_on_null() {
    // JS ??: null ?? "x" === "x"; false ?? "x" === false; 0 ?? "x" === 0
    assert_eq!(
        ok(&Expr::Coalesce {
            left: boxed(Value::Null),
            right: boxed("x"),
        }),
        Value::from("x")
    );
    assert_eq!(
        ok(&Expr::Coalesce {
            left: boxed(false),
            right: boxed("x"),
        }),
        Value::Bool(false)
    );
    assert_eq!(
        ok(&Expr::Coalesce {
            left: boxed(0),
            right: boxed("x"),
        }),
        Value::Int(0)
    );
}

// ================================================================
// Comparison: strict equality, JS relational coercion
// ================================================================

#[test]
fn eq_is_strict_across_int_float_but_not_types() {
    // JS ===: 1 === 1.0 is true (one number type); 1 === "1" is false
    assert!(b(&lit(1).eq(lit(1.0))));
    assert!(!b(&lit(1).eq(lit("1"))));
    assert!(b(&lit(1).ne(lit("1"))));
    assert!(!b(&lit(Value::Null).eq(lit(0))));
    assert!(b(&lit(Value::Null).eq(lit(Value::Null))));
}

#[test]
fn nan_is_not_equal_to_itself_but_includes_finds_it() {
    // JS: NaN === NaN is false; [NaN].includes(NaN) is true (SameValueZero)
    let nan = || lit(f64::NAN);
    assert!(!b(&nan().eq(nan())));
    let includes = Expr::Includes {
        array: Box::new(Expr::Array { items: vec![nan()] }),
        value: Box::new(nan()),
    };
    assert!(b(&includes));
}

#[test]
fn composite_equality_is_an_error_not_a_guess() {
    // JS compares arrays/objects by reference; server values have no
    // identity, so Eq errors instead of silently diverging.
    let e = Expr::Array { items: vec![] }.eq(Expr::Array { items: vec![] });
    assert!(matches!(
        run(&e),
        Err(EvalError::CompositeComparison { op: "Eq" })
    ));
}

#[test]
fn string_number_relational_comparisons_coerce_like_js() {
    // JS: "5" < 10 (numeric), "10" < "9" (string compare), "a" < "b"
    assert!(b(&lit("5").lt(lit(10))));
    assert!(b(&lit("10").lt(lit("9"))));
    assert!(b(&lit("a").lt(lit("b"))));
    // JS: "abc" < 5 -> NaN comparison -> false; likewise >= is false
    assert!(!b(&lit("abc").lt(lit(5))));
    assert!(!b(&lit("abc").ge(lit(5))));
}

#[test]
fn relational_nan_always_false() {
    let nan = || lit(f64::NAN);
    assert!(!b(&nan().lt(lit(1))));
    assert!(!b(&nan().le(lit(1))));
    assert!(!b(&nan().gt(lit(1))));
    assert!(!b(&nan().ge(lit(1))));
}

// ================================================================
// Strict-boolean logic (RFC decision; see SEMANTICS.md)
// ================================================================

#[test]
fn logic_requires_booleans() {
    assert!(matches!(
        run(&lit(1).and(lit(true))),
        Err(EvalError::TypeMismatch { op: "And", .. })
    ));
    assert!(matches!(
        run(&Expr::Ternary {
            cond: boxed("truthy"),
            then: boxed(1),
            else_: boxed(2),
        }),
        Err(EvalError::TypeMismatch { op: "Ternary", .. })
    ));
}

#[test]
fn logic_short_circuits_like_js() {
    // The right side must not evaluate when the left decides: a
    // client-only node on the right would otherwise error.
    let client_only = Expr::signal("x");
    assert!(!b(&lit(false).and(client_only.clone())));
    assert!(b(&lit(true).or(client_only)));
}

#[test]
fn ternary_branches() {
    let e = Expr::Ternary {
        cond: Box::new(lit(3).gt(lit(2))),
        then: boxed("yes"),
        else_: boxed("no"),
    };
    assert_eq!(ok(&e), Value::from("yes"));
}

// ================================================================
// String ops
// ================================================================

#[test]
fn length_is_utf16_code_units() {
    // JS: "héllo".length === 5, "👍".length === 2
    assert_eq!(ok(&lit("héllo").len()), Value::Int(5));
    assert_eq!(ok(&lit("👍").len()), Value::Int(2));
    assert_eq!(
        ok(&Expr::Len {
            expr: Box::new(Expr::Array {
                items: vec![lit(1), lit(2)]
            })
        }),
        Value::Int(2)
    );
    // JS: (5).length is undefined
    assert_eq!(ok(&lit(5).len()), Value::Null);
    // JS: null.length throws
    assert!(matches!(
        run(&lit(Value::Null).len()),
        Err(EvalError::TypeMismatch { op: "Len", .. })
    ));
}

#[test]
fn substring_swaps_and_clamps_like_js() {
    // JS: "hello".substring(3, 1) === "el", (-2, 2) === "he", (1) === "ello"
    let sub = |start: i32, end: Option<i32>| Expr::Substring {
        string: boxed("hello"),
        start: boxed(start),
        end: end.map(boxed),
    };
    assert_eq!(ok(&sub(3, Some(1))), Value::from("el"));
    assert_eq!(ok(&sub(-2, Some(2))), Value::from("he"));
    assert_eq!(ok(&sub(1, None)), Value::from("ello"));
    assert_eq!(ok(&sub(0, Some(99))), Value::from("hello"));
}

#[test]
fn slice_supports_negative_indices() {
    // JS: [1,2,3,4].slice(-2) === [3,4]; "hello".slice(1, -1) === "ell"
    let e = Expr::Slice {
        array: Box::new(Expr::Array {
            items: vec![lit(1), lit(2), lit(3), lit(4)],
        }),
        start: boxed(-2),
        end: None,
    };
    assert_eq!(ok(&e), Value::Array(vec![Value::Int(3), Value::Int(4)]));

    let e = Expr::Slice {
        array: boxed("hello"),
        start: boxed(1),
        end: Some(boxed(-1)),
    };
    assert_eq!(ok(&e), Value::from("ell"));
}

#[test]
fn replace_replaces_first_occurrence_only() {
    // Pinned to FormCodegen's `.replace(from, to)`: JS replaces the FIRST
    // occurrence for string patterns.
    let e = Expr::Replace {
        string: boxed("aaa"),
        from: boxed("a"),
        to: boxed("b"),
    };
    assert_eq!(ok(&e), Value::from("baa"));
}

#[test]
fn split_and_join_match_js() {
    // JS: "a,b".split(",") === ["a","b"]; "".split(",") === [""];
    //     "".split("") === []; "ab".split("") === ["a","b"]
    let split = |s: &'static str, d: &'static str| Expr::Split {
        string: boxed(s),
        delimiter: boxed(d),
    };
    assert_eq!(
        ok(&split("a,b", ",")),
        Value::Array(vec![Value::from("a"), Value::from("b")])
    );
    assert_eq!(ok(&split("", ",")), Value::Array(vec![Value::from("")]));
    assert_eq!(ok(&split("", "")), Value::Array(vec![]));
    assert_eq!(
        ok(&split("ab", "")),
        Value::Array(vec![Value::from("a"), Value::from("b")])
    );

    // JS: [1,null,"x"].join("-") === "1--x" (null becomes empty)
    let e = Expr::Join {
        array: Box::new(Expr::Array {
            items: vec![lit(1), lit(Value::Null), lit("x")],
        }),
        delimiter: boxed("-"),
    };
    assert_eq!(ok(&e), Value::from("1--x"));
}

#[test]
fn trim_upper_lower_contains() {
    assert_eq!(ok(&lit("  a ").trim()), Value::from("a"));
    assert_eq!(ok(&lit("aB").upper()), Value::from("AB"));
    assert_eq!(ok(&lit("aB").lower()), Value::from("ab"));
    assert!(b(&lit("haystack").contains(lit("stack"))));
    // JS: "a1b".includes(1) coerces the needle
    assert!(b(&lit("a1b").contains(lit(1))));
    // JS: (5).trim() throws
    assert!(matches!(
        run(&lit(5).trim()),
        Err(EvalError::TypeMismatch { op: "Trim", .. })
    ));
}

#[test]
fn matches_evaluates_regex() {
    let e = |s: &'static str| Expr::Matches {
        string: boxed(s),
        pattern: "^[a-z]+\\d$".into(),
    };
    assert!(b(&e("abc1")));
    assert!(!b(&e("1abc")));

    let bad = Expr::Matches {
        string: boxed("x"),
        pattern: "(".into(),
    };
    assert!(matches!(run(&bad), Err(EvalError::InvalidRegex { .. })));
}

// ================================================================
// Array combinators
// ================================================================

fn items_scope() -> Value {
    Value::object([(
        "items",
        Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    )])
}

#[test]
fn map_filter_reduce_work_against_scope() {
    let root = items_scope();
    let scope = ScopeStack::new(&root);

    let doubled = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        body: Box::new(Expr::var("x").mul(lit(2))),
    };
    assert_eq!(
        eval(&doubled, &scope).unwrap(),
        Value::Array(vec![
            Value::Float(2.0),
            Value::Float(4.0),
            Value::Float(6.0)
        ])
    );

    let evens = Expr::Filter {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        predicate: Box::new(
            Expr::Mod {
                left: Box::new(Expr::var("x")),
                right: boxed(2),
            }
            .eq(lit(0)),
        ),
    };
    assert_eq!(
        eval(&evens, &scope).unwrap(),
        Value::Array(vec![Value::Int(2)])
    );

    let sum = Expr::Reduce {
        array: Box::new(Expr::scope("items")),
        acc_var: proa_ir::VarId::new("acc"),
        item_var: proa_ir::VarId::new("x"),
        body: Box::new(Expr::var("acc").add(Expr::var("x"))),
        initial: boxed(0),
    };
    assert_eq!(eval(&sum, &scope).unwrap(), Value::Float(6.0));
}

#[test]
fn empty_array_combinators_match_js() {
    let empty = || Box::new(Expr::Array { items: vec![] });
    let var = proa_ir::VarId::new("x");
    let never = || Box::new(lit(true));

    // JS: [].every(p) === true, [].some(p) === false
    assert!(b(&Expr::Every {
        array: empty(),
        var: var.clone(),
        predicate: never(),
    }));
    assert!(!b(&Expr::Some {
        array: empty(),
        var: var.clone(),
        predicate: never(),
    }));
    // JS: [].find(p) === undefined -> Null; [].findIndex(p) === -1
    assert_eq!(
        ok(&Expr::Find {
            array: empty(),
            var: var.clone(),
            predicate: never(),
        }),
        Value::Null
    );
    assert_eq!(
        ok(&Expr::FindIndex {
            array: empty(),
            var: var.clone(),
            predicate: never(),
        }),
        Value::Int(-1)
    );
    // JS: [].reduce(f, init) === init
    assert_eq!(
        ok(&Expr::Reduce {
            array: empty(),
            acc_var: proa_ir::VarId::new("acc"),
            item_var: var,
            body: boxed(0),
            initial: boxed(42),
        }),
        Value::Int(42)
    );
    // JS: [][0] and [].at(-1) are undefined
    assert_eq!(ok(&Expr::First { array: empty() }), Value::Null);
    assert_eq!(ok(&Expr::Last { array: empty() }), Value::Null);
    // JS: [].join("-") === ""
    assert_eq!(
        ok(&Expr::Join {
            array: empty(),
            delimiter: boxed("-"),
        }),
        Value::from("")
    );
}

#[test]
fn flat_defaults_to_depth_one() {
    let nested = || {
        Box::new(Expr::Array {
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
        })
    };
    // JS: [1,[2,[3]]].flat() === [1,2,[3]]
    assert_eq!(
        ok(&Expr::Flat {
            array: nested(),
            depth: None,
        }),
        Value::Array(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Array(vec![Value::Int(3)])
        ])
    );
    // JS: .flat(2) === [1,2,3]
    assert_eq!(
        ok(&Expr::Flat {
            array: nested(),
            depth: Some(2),
        }),
        Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
}

#[test]
fn combinator_vars_shadow_outer_bindings() {
    let root = items_scope();
    let mut scope = ScopeStack::new(&root);
    scope.push_var("x", Value::Int(100));

    // Inner Map rebinds x per element; outer x visible outside the body.
    let e = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        body: Box::new(Expr::var("x")),
    };
    assert_eq!(
        eval(&e, &scope).unwrap(),
        Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
    assert_eq!(eval(&Expr::var("x"), &scope).unwrap(), Value::Int(100));

    let unknown = Expr::var("missing");
    assert!(matches!(
        eval(&unknown, &scope),
        Err(EvalError::UnknownVar { .. })
    ));
}

// ================================================================
// Type/convert and math ops
// ================================================================

#[test]
fn to_number_matches_js_number() {
    let to_num = |v: Value| Expr::ToNumber {
        expr: Box::new(Expr::Lit { value: v }),
    };
    // JS Number(): "" -> 0, " 12 " -> 12, "0x10" -> 16, "abc" -> NaN,
    // null -> 0, true -> 1, [5] -> 5, [1,2] -> NaN, {} -> NaN
    assert_eq!(num(&to_num(Value::from(""))), 0.0);
    assert_eq!(num(&to_num(Value::from(" 12 "))), 12.0);
    assert_eq!(num(&to_num(Value::from("0x10"))), 16.0);
    assert!(num(&to_num(Value::from("abc"))).is_nan());
    assert!(num(&to_num(Value::from("12abc"))).is_nan());
    assert_eq!(num(&to_num(Value::Null)), 0.0);
    assert_eq!(num(&to_num(Value::Bool(true))), 1.0);
    assert_eq!(num(&to_num(Value::Array(vec![Value::Int(5)]))), 5.0);
    assert!(num(&to_num(Value::Array(vec![Value::Int(1), Value::Int(2)]))).is_nan());
    assert!(num(&to_num(Value::object([("a", Value::Int(1))]))).is_nan());
    assert_eq!(num(&to_num(Value::from("1e3"))), 1000.0);
    assert_eq!(num(&to_num(Value::from("-Infinity"))), f64::NEG_INFINITY);
}

#[test]
fn parse_int_and_parse_float_take_prefixes() {
    let pi = |s: &'static str| Expr::ParseInt { expr: boxed(s) };
    let pf = |s: &'static str| Expr::ParseFloat { expr: boxed(s) };
    // JS parseInt: "12abc" -> 12, "abc" -> NaN, " -42 " -> -42, "1.9" -> 1
    assert_eq!(num(&pi("12abc")), 12.0);
    assert!(num(&pi("abc")).is_nan());
    assert_eq!(num(&pi(" -42 ")), -42.0);
    assert_eq!(num(&pi("1.9")), 1.0);
    // JS parseFloat: "6.25xyz" -> 6.25, ".5" -> 0.5, "1e2m" -> 100
    assert_eq!(num(&pf("6.25xyz")), 6.25);
    assert_eq!(num(&pf(".5")), 0.5);
    assert_eq!(num(&pf("1e2m")), 100.0);
    assert!(num(&pf("e5")).is_nan());
}

#[test]
fn math_round_is_half_up_toward_positive_infinity() {
    let round = |v: f64| Expr::Round { expr: boxed(v) };
    // JS Math.round: 2.5 -> 3, -2.5 -> -2, 0.49999999999999994 -> 0
    assert_eq!(num(&round(2.5)), 3.0);
    assert_eq!(num(&round(-2.5)), -2.0);
    assert_eq!(num(&round(0.499_999_999_999_999_94)), 0.0);
    assert_eq!(num(&round(-3.7)), -4.0);
}

#[test]
fn min_max_match_js_math() {
    // JS: Math.min() === Infinity, Math.max() === -Infinity, NaN wins
    assert_eq!(num(&Expr::Min { values: vec![] }), f64::INFINITY);
    assert_eq!(num(&Expr::Max { values: vec![] }), f64::NEG_INFINITY);
    assert!(num(&Expr::Min {
        values: vec![lit(1), lit(f64::NAN)],
    })
    .is_nan());
    assert_eq!(
        num(&Expr::Min {
            values: vec![lit(3), lit("2")],
        }),
        2.0
    );
}

#[test]
fn clamp_matches_emitted_composition() {
    let clamp = |v: i32, min: i32, max: i32| Expr::Clamp {
        value: boxed(v),
        min: boxed(min),
        max: boxed(max),
    };
    assert_eq!(num(&clamp(10, 2, 5)), 5.0);
    assert_eq!(num(&clamp(0, 2, 5)), 2.0);
    assert_eq!(num(&clamp(3, 2, 5)), 3.0);
}

// ================================================================
// Objects, dates, misc
// ================================================================

#[test]
fn object_duplicate_keys_last_wins() {
    // JS object literal: {a: 1, a: 2}.a === 2
    let e = Expr::Prop {
        expr: Box::new(Expr::Object {
            entries: vec![("a".into(), lit(1)), ("a".into(), lit(2))],
        }),
        prop: "a".into(),
    };
    assert_eq!(ok(&e), Value::Int(2));
}

#[test]
fn format_date_formats_utc() {
    let e = Expr::FormatDate {
        date: boxed(1_783_082_096_789i64),
        format: "YYYY-MM-DD HH:mm".into(),
    };
    assert_eq!(ok(&e), Value::from("2026-07-03 12:34"));

    let invalid = Expr::FormatDate {
        date: boxed("not a date"),
        format: "YYYY".into(),
    };
    assert_eq!(ok(&invalid), Value::from("Invalid Date"));
}

#[test]
fn client_only_and_unsupported_nodes_error() {
    assert!(matches!(
        run(&Expr::signal("count")),
        Err(EvalError::ClientOnly { variant: "Signal" })
    ));
    assert!(matches!(
        run(&Expr::field("email")),
        Err(EvalError::ClientOnly { variant: "Field" })
    ));
    assert!(matches!(
        run(&Expr::Now),
        Err(EvalError::Unsupported { variant: "Now" })
    ));
    assert!(matches!(
        run(&Expr::ParseDate {
            string: boxed("2026-01-01"),
            format: "YYYY-MM-DD".into(),
        }),
        Err(EvalError::Unsupported {
            variant: "ParseDate"
        })
    ));
}

#[test]
fn scope_composes_with_index_and_prop() {
    let root = Value::object([(
        "cart",
        Value::object([(
            "items",
            Value::Array(vec![
                Value::object([("name", Value::from("Widget")), ("price", Value::Int(1999))]),
                Value::object([("name", Value::from("Gadget")), ("price", Value::Int(2999))]),
            ]),
        )]),
    )]);
    let scope = ScopeStack::new(&root);

    let e = Expr::scope("cart").prop("items").index(1).prop("name");
    assert_eq!(eval(&e, &scope).unwrap(), Value::from("Gadget"));

    let len = Expr::scope("cart").prop("items").len();
    assert_eq!(eval(&len, &scope).unwrap(), Value::Int(2));
}

#[test]
fn string_indexing_matches_js() {
    // JS: "abc"[1] === "b"; "abc"[9] === undefined
    assert_eq!(ok(&lit("abc").index(lit(1))), Value::from("b"));
    assert_eq!(ok(&lit("abc").index(lit(9))), Value::Null);
    // First/Last on strings
    assert_eq!(
        ok(&Expr::First {
            array: boxed("abc")
        }),
        Value::from("a")
    );
    assert_eq!(
        ok(&Expr::Last {
            array: boxed("abc")
        }),
        Value::from("c")
    );
}
