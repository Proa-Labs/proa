// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M4 golden test: feed the ConditionalCounter fixture through
//! `emit_js::emit` and assert the produced JS matches a checked-in
//! expected output.
//!
//! The ConditionalCounter fixture extends the M1 Counter with a
//! `<p class="hint">"That's a lot!"</p>` paragraph that is shown only
//! when `count.get() > 5`. The new emit_js shape under test is a
//! single `bind_conditional(...)` call carrying the predicate getter,
//! the dep list, and the resolved DOM nodes for each branch.

use rsjs_ir::{
    emit_js, ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, EventName,
    IslandBuilder, Value,
};

/// Hand-build the ConditionalCounter fixture (mirrors `tests/conditional.rs`).
fn build_conditional_counter() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let count_signal = b.signal(Value::Int(0));
    let count_read_expr = b.expr(ClientExpr::SignalRead(count_signal));

    let decrement_handler =
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            count_signal,
            ClientSimpleClosure::new(
                "c",
                ClientExpr::binary(
                    ClientBinaryOp::Sub,
                    ClientExpr::signal_read(count_signal),
                    ClientExpr::int(1),
                ),
            ),
        )]));
    let increment_handler =
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            count_signal,
            ClientSimpleClosure::new(
                "c",
                ClientExpr::binary(
                    ClientBinaryOp::Add,
                    ClientExpr::signal_read(count_signal),
                    ClientExpr::int(1),
                ),
            ),
        )]));

    let root = b.element("div", None);
    let decrement_button = b.element("button", Some(root));
    let span = b.element("span", Some(root));
    let increment_button = b.element("button", Some(root));
    let hint_paragraph = b.element("p", Some(root));

    b.bind_text(span, count_read_expr);
    b.bind_event(decrement_button, EventName::Click, decrement_handler);
    b.bind_event(increment_button, EventName::Click, increment_handler);

    let condition_expr = b.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Gt,
        left: Box::new(ClientExpr::SignalRead(count_signal)),
        right: Box::new(ClientExpr::Literal(Value::Int(5))),
    });
    b.conditional_section(root, condition_expr, vec![hint_paragraph], Vec::new());

    b.set_root(root);
    b.build()
}

#[test]
fn conditional_counter_emits_expected_js() {
    let island = build_conditional_counter();
    let actual = emit_js::emit(&island, "ConditionalCounter");
    let expected = include_str!("emit_js_conditional.expected.js");
    assert_eq!(
        actual, expected,
        "emitted JS did not match the golden file. \
         Diff:\n\
         --- expected\n\
         +++ actual\n\
         {actual}"
    );
}

#[test]
fn conditional_counter_emit_is_byte_identical_across_repeated_runs() {
    let island = build_conditional_counter();
    let first = emit_js::emit(&island, "ConditionalCounter");
    for _ in 0..32 {
        assert_eq!(emit_js::emit(&island, "ConditionalCounter"), first);
    }
}
