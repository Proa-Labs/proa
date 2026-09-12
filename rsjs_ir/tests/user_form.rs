// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M5 fixture: hand-construct the UserForm island.
//!
//! Preserves the historical M5 form fixture: a username/email pair of text
//! inputs, a `submitting` flag, per-field
//! validation as derived expressions, conditional error messages, and
//! a submit handler that flips `submitting`.
//! The quoted block below is historical milestone notation, not current
//! authoring syntax.
//!
//! ```text
//! pub struct UserForm;
//!
//! #[rsjs] impl ... for UserForm {
//!     fn render(self, cx: ...) -> ... {
//!         let username  = signal::<String>("");
//!         let email     = signal::<String>("");
//!         let submitting = signal::<bool>(false);
//!         html_sync! {
//!             <form on_submit={...}>
//!                 <input bind_value={username}/>
//!                 {if !username.is_non_empty() {
//!                     <p class="error">"Username required"</p>
//!                 }}
//!                 <input bind_value={email}/>
//!                 {if email.eq("") {
//!                     <p class="error">"Email required"</p>
//!                 }}
//!                 <button type="submit">"Submit"</button>
//!             </form>
//!         }
//!     }
//! }
//! ```
//!
//! The fixture exercises:
//!
//! - [`InputBinding`] (the new M5 side-table) on the two text inputs.
//! - `EventName::Submit` on the `<form>`, with a handler that sets the
//!   `submitting` signal to `true`.
//! - Two validation predicate shapes: `ClientExpr::IsNonEmpty` (for
//!   username) and `ClientExpr::Binary { op: ClientBinaryOp::Eq, ... }` against
//!   `Value::String("")` (for email). Both shapes cover the same validation
//!   ground.
//! - Two `ConditionalSection`s for the per-field error messages.

use rsjs_ir::{
    ClientBinaryOp, ClientBlock, ClientExpr, ClientStmt, EventName, ExprId, InputBinding,
    InputKind, IslandBuilder, NodeId, SignalId, Value, ViewNode,
};

struct UserFormFixture {
    island: rsjs_ir::Island,
    username_signal: SignalId,
    email_signal: SignalId,
    submitting_signal: SignalId,
    form: NodeId,
    username_input: NodeId,
    email_input: NodeId,
    username_error: NodeId,
    email_error: NodeId,
    submit_button: NodeId,
    username_invalid_expr: ExprId,
    email_invalid_expr: ExprId,
}

fn build_user_form() -> UserFormFixture {
    let mut b = IslandBuilder::new();

    // 1. Signals.
    let username_signal = b.signal(Value::String(String::new()));
    let email_signal = b.signal(Value::String(String::new()));
    let submitting_signal = b.signal(Value::Bool(false));

    // 2. Validation predicates as derived expressions.
    //    username_invalid = !username.is_non_empty()
    //    email_invalid    = email == ""
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

    // 3. Submit handler: submitting.set(true).
    let submit_handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        submitting_signal,
        ClientExpr::bool(true),
    )]));

    // 4. View tree. <form> wraps the inputs, error paragraphs, and the
    //    submit button. The error paragraphs are siblings of the
    //    inputs, gated by per-field ConditionalSections (one per
    //    field, hung off the form NodeId via separate conditional
    //    parents). v0.1's `conditional_section(parent, ...)` permits
    //    only one section per parent NodeId, so we host each error's
    //    conditional on its own `<div>` wrapper.
    let form = b.element("form", None);
    let username_input = b.element("input", Some(form));
    let username_error_wrapper = b.element("div", Some(form));
    let username_error = b.element("p", Some(username_error_wrapper));
    let email_input = b.element("input", Some(form));
    let email_error_wrapper = b.element("div", Some(form));
    let email_error = b.element("p", Some(email_error_wrapper));
    let submit_button = b.element("button", Some(form));

    // 5. Bindings.
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

    UserFormFixture {
        island: b.build(),
        username_signal,
        email_signal,
        submitting_signal,
        form,
        username_input,
        email_input,
        username_error,
        email_error,
        submit_button,
        username_invalid_expr,
        email_invalid_expr,
    }
}

#[test]
fn fixture_signals_are_three_typed_initial_values() {
    let f = build_user_form();
    assert_eq!(f.island.signals.len(), 3);
    assert_eq!(f.island.signals[0].id, f.username_signal);
    assert_eq!(f.island.signals[0].init, Value::String(String::new()));
    assert_eq!(f.island.signals[1].id, f.email_signal);
    assert_eq!(f.island.signals[1].init, Value::String(String::new()));
    assert_eq!(f.island.signals[2].id, f.submitting_signal);
    assert_eq!(f.island.signals[2].init, Value::Bool(false));
}

#[test]
fn input_bindings_keyed_by_input_node_id() {
    let f = build_user_form();
    let username_binding = f
        .island
        .bindings
        .input_bindings
        .get(&f.username_input)
        .expect("username input binding");
    assert_eq!(
        *username_binding,
        InputBinding {
            node: f.username_input,
            kind: InputKind::Value,
            signal: f.username_signal,
        }
    );
    let email_binding = f
        .island
        .bindings
        .input_bindings
        .get(&f.email_input)
        .expect("email input binding");
    assert_eq!(
        *email_binding,
        InputBinding {
            node: f.email_input,
            kind: InputKind::Value,
            signal: f.email_signal,
        }
    );
}

#[test]
fn submit_handler_is_event_bound_to_form() {
    let f = build_user_form();
    let handler = f
        .island
        .bindings
        .event_bindings
        .get(&(f.form, EventName::Submit))
        .expect("submit binding on form");
    let handler_body = f.island.handlers[handler.0 as usize].body_ir();
    assert_eq!(handler_body.stmts.len(), 1);
    match &handler_body.stmts[0] {
        ClientStmt::SignalSet { id, value } => {
            assert_eq!(*id, f.submitting_signal);
            assert_eq!(*value, ClientExpr::Literal(Value::Bool(true)));
        }
        other => panic!("expected SignalSet, got {other:?}"),
    }
}

#[test]
fn username_invalid_predicate_uses_is_non_empty() {
    let f = build_user_form();
    match f.island.exprs.client_expr(f.username_invalid_expr) {
        ClientExpr::Binary { op, left, right } => {
            assert_eq!(*op, ClientBinaryOp::Eq);
            match left.as_ref() {
                ClientExpr::IsNonEmpty { target } => {
                    assert_eq!(**target, ClientExpr::SignalRead(f.username_signal));
                }
                other => panic!("expected IsNonEmpty, got {other:?}"),
            }
            assert_eq!(**right, ClientExpr::Literal(Value::Bool(false)));
        }
        other => panic!("expected Binary, got {other:?}"),
    }
}

#[test]
fn email_invalid_predicate_uses_eq_against_empty_string() {
    let f = build_user_form();
    match f.island.exprs.client_expr(f.email_invalid_expr) {
        ClientExpr::Binary { op, left, right } => {
            assert_eq!(*op, ClientBinaryOp::Eq);
            assert_eq!(**left, ClientExpr::SignalRead(f.email_signal));
            assert_eq!(**right, ClientExpr::Literal(Value::String(String::new())));
        }
        other => panic!("expected Binary, got {other:?}"),
    }
}

#[test]
fn dep_collector_walks_through_is_non_empty() {
    let f = build_user_form();
    let deps = rsjs_ir::analysis::collect_deps(&f.island.exprs, f.username_invalid_expr);
    assert_eq!(
        deps,
        vec![f.username_signal],
        "the IsNonEmpty target must be walked so its signal contributes a dep"
    );
}

#[test]
fn structurally_eq_handles_string_transforms_and_predicates() {
    use rsjs_ir::{StringPredicateOp, StringTransformOp};

    let upper = ClientExpr::StringTransform {
        op: StringTransformOp::ToUppercase,
        target: Box::new(ClientExpr::SignalRead(rsjs_ir::SignalId(0))),
    };
    assert!(upper == upper.clone());
    let trim = ClientExpr::StringTransform {
        op: StringTransformOp::Trim,
        target: Box::new(ClientExpr::SignalRead(rsjs_ir::SignalId(0))),
    };
    assert!(
        upper != trim,
        "transform op must participate in structural equality"
    );

    let starts = ClientExpr::StringPredicate {
        op: StringPredicateOp::StartsWith,
        haystack: Box::new(ClientExpr::SignalRead(rsjs_ir::SignalId(0))),
        needle: Box::new(ClientExpr::Literal(Value::String("a".to_string()))),
    };
    assert!(starts == starts.clone());
    let ends = ClientExpr::StringPredicate {
        op: StringPredicateOp::EndsWith,
        haystack: Box::new(ClientExpr::SignalRead(rsjs_ir::SignalId(0))),
        needle: Box::new(ClientExpr::Literal(Value::String("a".to_string()))),
    };
    assert!(
        starts != ends,
        "predicate op must participate in structural equality"
    );
}

#[test]
fn input_kind_dom_property_and_attr_round_trip() {
    assert_eq!(InputKind::Value.as_dom_property(), "value");
    assert_eq!(InputKind::Checked.as_dom_property(), "checked");
    assert_eq!(InputKind::Selected.as_dom_property(), "value");
    assert_eq!(InputKind::Value.as_attr_name(), "bind_value");
    assert_eq!(InputKind::Checked.as_attr_name(), "bind_checked");
    assert_eq!(InputKind::Selected.as_attr_name(), "bind_selected");
}

#[test]
fn building_two_input_bindings_on_the_same_node_panics() {
    use std::panic::catch_unwind;
    let result = catch_unwind(|| {
        let mut b = IslandBuilder::new();
        let s = b.signal(Value::String(String::new()));
        let n = b.element("input", None);
        b.bind_input(n, InputKind::Value, s);
        b.bind_input(n, InputKind::Value, s);
    });
    assert!(result.is_err());
}

#[test]
fn fixture_root_is_the_form_node() {
    let f = build_user_form();
    let root = f.island.view_tree.root().expect("root set");
    assert_eq!(root, f.form);
    match f.island.view_tree.get(root) {
        ViewNode::Element { tag, children, .. } => {
            assert_eq!(tag, "form");
            assert_eq!(
                children.len(),
                5,
                "form owns 2 inputs, 2 error wrappers, 1 button"
            );
            assert_eq!(children[0], f.username_input);
            assert_eq!(children[2], f.email_input);
            assert_eq!(children[4], f.submit_button);
        }
        other => panic!("expected element, got {other:?}"),
    }
    // Silence unused-field warnings in the fixture struct.
    let _ = (f.username_error, f.email_error);
}

#[test]
fn island_default_is_sync() {
    let f = build_user_form();
    assert!(!f.island.is_async, "form fixture is a sync island");
}
