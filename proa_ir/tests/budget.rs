// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Budgeted evaluation: pathological expressions must fail with an error,
//! never panic, overflow the stack, or hang.

use proa_ir::eval::{eval, eval_with_budget, EvalBudget, EvalError, ScopeStack};
use proa_ir::{Expr, Value};

fn empty() -> Value {
    Value::object([] as [(&str, Value); 0])
}

/// `depth`-deep chain of Add nodes.
fn deep_expr(depth: usize) -> Expr {
    let mut expr = Expr::lit(1);
    for _ in 0..depth {
        expr = expr.add(Expr::lit(1));
    }
    expr
}

#[test]
fn depth_limit_rejects_deep_expressions() {
    let root = empty();
    let scope = ScopeStack::new(&root);

    // Default budget: 200 levels must fail, shallow must pass. (Depth
    // stays modest here because recursive drop glue for very deep Expr
    // trees is itself a stack hazard; a host can keep serde's ~128 recursion
    // limit in front of the evaluator.)
    assert!(matches!(
        eval(&deep_expr(200), &scope),
        Err(EvalError::DepthLimit { .. })
    ));
    assert!(eval(&deep_expr(10), &scope).is_ok());

    // Explicit budget boundary.
    let budget = EvalBudget {
        max_depth: 8,
        ..EvalBudget::default()
    };
    assert!(eval_with_budget(&deep_expr(3), &scope, &budget).is_ok());
    assert!(matches!(
        eval_with_budget(&deep_expr(9), &scope, &budget),
        Err(EvalError::DepthLimit { limit: 8 })
    ));
}

#[test]
fn iteration_limit_rejects_large_combinator_work() {
    let root = Value::object([("items", Value::Array((0..100).map(Value::Int).collect()))]);
    let scope = ScopeStack::new(&root);

    let map = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        body: Box::new(Expr::var("x")),
    };

    let tight = EvalBudget {
        max_iterations: 99,
        ..EvalBudget::default()
    };
    assert!(matches!(
        eval_with_budget(&map, &scope, &tight),
        Err(EvalError::IterationLimit { limit: 99 })
    ));

    let enough = EvalBudget {
        max_iterations: 100,
        ..EvalBudget::default()
    };
    assert!(eval_with_budget(&map, &scope, &enough).is_ok());
}

#[test]
fn nested_combinators_share_one_iteration_budget() {
    // 100 x 200 element visits blow the default 10k budget even though
    // each individual loop is small.
    let root = Value::object([("items", Value::Array((0..100).map(Value::Int).collect()))]);
    let scope = ScopeStack::new(&root);

    let inner = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("y"),
        body: Box::new(Expr::var("y")),
    };
    let outer = Expr::Map {
        array: Box::new(Expr::scope("items")),
        var: proa_ir::VarId::new("x"),
        body: Box::new(inner),
    };
    assert!(matches!(
        eval(&outer, &scope),
        Err(EvalError::IterationLimit { .. })
    ));
}

#[test]
fn flat_iterations_are_budgeted() {
    // A deeply-nested array flattened with a large depth still counts
    // every element visit.
    let mut value = Value::Array(vec![Value::Int(1)]);
    for _ in 0..50 {
        value = Value::Array(vec![value, Value::Int(2)]);
    }
    let root = Value::object([("deep", value)]);
    let scope = ScopeStack::new(&root);

    let flat = Expr::Flat {
        array: Box::new(Expr::scope("deep")),
        depth: Some(255),
    };
    let tight = EvalBudget {
        max_iterations: 20,
        ..EvalBudget::default()
    };
    assert!(matches!(
        eval_with_budget(&flat, &scope, &tight),
        Err(EvalError::IterationLimit { .. })
    ));
    assert!(eval(&flat, &scope).is_ok());
}

#[test]
fn deep_deserialized_expression_does_not_overflow_the_stack() {
    // Runtime-template path: hostile JSON -> Expr -> eval must fail cleanly.
    // (serde_json's own recursion limit caps parse depth around 128;
    // stay under it so the failure we exercise is the eval budget.)
    let mut json = String::new();
    for _ in 0..100 {
        json.push_str(r#"{"type":"neg","expr":"#);
    }
    json.push_str(r#"{"type":"lit","value":1}"#);
    for _ in 0..100 {
        json.push('}');
    }
    let expr: Expr = serde_json::from_str(&json).unwrap();

    let root = empty();
    let scope = ScopeStack::new(&root);
    assert!(matches!(
        eval(&expr, &scope),
        Err(EvalError::DepthLimit { .. })
    ));
}
