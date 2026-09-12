// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M5 golden test: feed the UserForm fixture through `emit_js::emit`
//! and assert the produced JS matches the checked-in expected output.
//!
//! The UserForm fixture exercises three new emit_js shapes:
//!
//! - `bind_input(node, "value", signal)` for each of the two text
//!   inputs (the M5 InputBinding side-table).
//! - `bind_submit(node, handler)` for the form's submit event (the
//!   submit-specific runtime helper that calls `preventDefault`).
//! - `ClientExpr::IsNonEmpty` lowered as an inline JS predicate inside the
//!   conditional getter for the username error.
//!
//! Plus the import line gains `bind_input` and `bind_submit`, since
//! the island actually uses them. The Counter / ConditionalCounter
//! goldens stay byte-stable because the import line is conditional.

use rsjs_ir::{
    emit_js, ClientBinaryOp, ClientBlock, ClientExpr, ClientStmt, EventName, InputKind,
    IslandBuilder, Value,
};

fn build_user_form() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let username_signal = b.signal(Value::String(String::new()));
    let email_signal = b.signal(Value::String(String::new()));
    let submitting_signal = b.signal(Value::Bool(false));

    let username_invalid_expr = b.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Eq,
        left: Box::new(ClientExpr::IsNonEmpty {
            target: Box::new(ClientExpr::SignalRead(username_signal)),
        }),
        right: Box::new(ClientExpr::Literal(Value::Bool(false))),
    });
    let email_invalid_expr = b.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Eq,
        left: Box::new(ClientExpr::SignalRead(email_signal)),
        right: Box::new(ClientExpr::Literal(Value::String(String::new()))),
    });

    let submit_handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        submitting_signal,
        ClientExpr::bool(true),
    )]));

    let form = b.element("form", None);
    let username_input = b.element("input", Some(form));
    let username_error_wrapper = b.element("div", Some(form));
    let username_error = b.element("p", Some(username_error_wrapper));
    let email_input = b.element("input", Some(form));
    let email_error_wrapper = b.element("div", Some(form));
    let email_error = b.element("p", Some(email_error_wrapper));
    let _submit_button = b.element("button", Some(form));

    b.bind_input(username_input, InputKind::Value, username_signal);
    b.bind_input(email_input, InputKind::Value, email_signal);
    b.bind_event(form, EventName::Submit, submit_handler);
    b.conditional_section(
        username_error_wrapper,
        username_invalid_expr,
        vec![username_error],
        Vec::new(),
    );
    b.conditional_section(
        email_error_wrapper,
        email_invalid_expr,
        vec![email_error],
        Vec::new(),
    );

    b.set_root(form);
    b.build()
}

#[test]
fn user_form_emits_expected_js() {
    let island = build_user_form();
    let actual = emit_js::emit(&island, "UserForm");
    let expected = include_str!("emit_js_user_form.expected.js");
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
fn user_form_emit_is_byte_identical_across_repeated_runs() {
    let island = build_user_form();
    let first = emit_js::emit(&island, "UserForm");
    for _ in 0..32 {
        assert_eq!(emit_js::emit(&island, "UserForm"), first);
    }
}

#[test]
fn user_form_can_emit_handlers_through_client_ir_v3() {
    let island = build_user_form();
    let js = emit_js::emit(&island, "UserForm");

    assert!(
        js.contains("__sig_2.set(true);"),
        "handler emission should emit submit signal set, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(bind_submit(__node_0, __handler_0));"),
        "handler emission should preserve submit binding emission, got:\n{js}"
    );
}
