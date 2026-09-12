// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! FormatNumber (JS `toFixed`) parity: server evaluator results asserted
//! against hand-checked V8 outputs, plus the JS-emitter side of the pair.

use proa_ir::eval::{eval, EvalError, ScopeStack};
use proa_ir::{Expr, Value};

fn fmt(value: impl Into<Value>, decimals: u8) -> Result<Value, EvalError> {
    let expr = Expr::FormatNumber {
        value: Box::new(Expr::Lit {
            value: value.into(),
        }),
        decimals,
    };
    let root = Value::object([] as [(&str, Value); 0]);
    let scope = ScopeStack::new(&root);
    eval(&expr, &scope)
}

fn fmt_str(value: impl Into<Value>, decimals: u8) -> String {
    match fmt(value, decimals).unwrap() {
        Value::String(s) => s.to_string(),
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn matches_js_to_fixed_on_ties_and_fp_traps() {
    // All expectations are V8 outputs.
    assert_eq!(fmt_str(0.125, 2), "0.13"); // exact tie rounds up (Rust {:.2} would give "0.12")
    assert_eq!(fmt_str(-0.125, 2), "-0.13"); // ties round away from zero on the magnitude
    assert_eq!(fmt_str(1.005, 2), "1.00"); // 1.005 is really 1.00499...
    assert_eq!(fmt_str(2.675, 2), "2.67"); // 2.675 is really 2.67499...
    assert_eq!(fmt_str(0.1 + 0.2, 2), "0.30");
    assert_eq!(fmt_str(1234.5678, 2), "1234.57");
    assert_eq!(fmt_str(0.995, 2), "0.99"); // 0.995 is really 0.99499...
    assert_eq!(fmt_str(9.995, 2), "9.99"); // 9.995 is really 9.99499... (no carry)
    assert_eq!(fmt_str(0.999, 2), "1.00"); // carry across the decimal point
    assert_eq!(fmt_str(99.999, 2), "100.00"); // carry grows the integer part
}

#[test]
fn integer_and_zero_decimal_cases() {
    assert_eq!(fmt_str(5, 0), "5");
    assert_eq!(fmt_str(5, 2), "5.00");
    assert_eq!(fmt_str(2.5, 0), "3"); // toFixed is half-up on magnitude
    assert_eq!(fmt_str(-2.5, 0), "-3"); // unlike Math.round(-2.5) === -2
    assert_eq!(fmt_str(0, 2), "0.00");
    assert_eq!(fmt_str(-0.0, 2), "0.00"); // -0 gets no sign
    assert_eq!(fmt_str(-0.0004, 2), "-0.00"); // but negative-rounding-to-zero does
}

#[test]
fn out_of_fixed_range_falls_back_to_to_string() {
    // JS: (1e21).toFixed(2) === "1e+21"
    assert_eq!(fmt_str(1e21, 2), "1e+21");
    assert_eq!(fmt_str(f64::NAN, 2), "NaN");
    assert_eq!(fmt_str(f64::INFINITY, 2), "Infinity");
}

#[test]
fn coerces_value_via_to_number_like_js() {
    // JS: ("12.345").toFixed would throw, but the IR node evaluates its
    // operand first; the emitted `(expr).toFixed(d)` sees a number
    // whenever the expression is numeric. Strings coerce via ToNumber.
    assert_eq!(fmt_str("12.345", 2), "12.35"); // 12.345 is really 12.34500...02
    assert_eq!(fmt_str(Value::Null, 2), "0.00");
}

#[test]
fn decimals_beyond_100_error() {
    assert!(matches!(
        fmt(1.0, 101),
        Err(EvalError::InvalidDecimals { decimals: 101 })
    ));
    // 100 is the JS maximum and must work.
    assert_eq!(fmt_str(1.5, 100).len(), 102);
}

#[test]
fn currency_concat_example() {
    // The pervasive money-formatting shape:
    // "$" + (total_cents / 100).toFixed(2)
    let root = Value::object([(
        "cart",
        Value::object([("total_cents", Value::Int(123_456))]),
    )]);
    let scope = ScopeStack::new(&root);
    let price = Expr::Concat {
        parts: vec![
            Expr::lit("$"),
            Expr::scope("cart")
                .prop("total_cents")
                .div(Expr::lit(100))
                .format_number(2),
        ],
    };
    assert_eq!(eval(&price, &scope).unwrap(), Value::from("$1234.56"));

    // Emitter side of the same expression (parity is exercised end-to-end
    // in tests/js_conformance.rs when node is available).
    let js = proa_ir::codegen::expr_to_js(&price);
    assert_eq!(
        js,
        "(\"$\" + ((($scope[\"cart\"]).total_cents / 100)).toFixed(2))"
    );
}
