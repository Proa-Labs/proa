// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M4 fixture: hand-construct the ConditionalCounter island.
//!
//! Preserves the historical M4 conditional fixture. The quoted block is
//! milestone notation, not current authoring syntax.
//!
//! ```text
//! pub struct ConditionalCounter { pub initial: i32 }
//!
//! #[rsjs] impl ... for ConditionalCounter {
//!     fn render(self, cx: ...) -> ... {
//!         let count = signal::<i32>(self.initial);
//!         html_sync! {
//!             <div>
//!                 <button on_click={decrement(count)}>"-"</button>
//!                 <span>{count.get()}</span>
//!                 <button on_click={increment(count)}>"+"</button>
//!                 {if count.get() > 5 {
//!                     <p class="hint">"That's a lot!"</p>
//!                 }}
//!             </div>
//!         }
//!     }
//! }
//! ```
//!
//! The fixture is the M4 exit criterion for the IR layer: it pressures
//! the side-table surface with a second concrete shape (a
//! `ConditionalSection` keyed off the parent `<div>`).

use rsjs_ir::{
    ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, EventName, ExprId,
    IslandBuilder, NodeId, Value, ViewNode,
};

/// Build the ConditionalCounter fixture island.
struct ConditionalCounterFixture {
    island: rsjs_ir::Island,
    count_signal: rsjs_ir::SignalId,
    root: NodeId,
    decrement_button: NodeId,
    increment_button: NodeId,
    span: NodeId,
    hint_paragraph: NodeId,
    condition_expr: ExprId,
}

fn build_conditional_counter() -> ConditionalCounterFixture {
    let mut b = IslandBuilder::new();

    // 1. Signal: `let count = signal::<i32>(0);`
    let count_signal = b.signal(Value::Int(0));

    // 2. Reactive expression for the span: `count.get()`.
    let count_read_expr = b.expr(ClientExpr::SignalRead(count_signal));

    // 3. Decrement / increment handlers, identical to the M1 Counter.
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

    // 4. View tree.
    let root = b.element("div", None);
    let decrement_button = b.element("button", Some(root));
    let span = b.element("span", Some(root));
    let increment_button = b.element("button", Some(root));
    let hint_paragraph = b.element("p", Some(root));

    b.bind_text(span, count_read_expr);
    b.bind_event(decrement_button, EventName::Click, decrement_handler);
    b.bind_event(increment_button, EventName::Click, increment_handler);

    // 5. Conditional: show the hint paragraph when count > 5.
    let condition_expr = b.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Gt,
        left: Box::new(ClientExpr::SignalRead(count_signal)),
        right: Box::new(ClientExpr::Literal(Value::Int(5))),
    });
    b.conditional_section(root, condition_expr, vec![hint_paragraph], Vec::new());

    b.set_root(root);

    ConditionalCounterFixture {
        island: b.build(),
        count_signal,
        root,
        decrement_button,
        increment_button,
        span,
        hint_paragraph,
        condition_expr,
    }
}

#[test]
fn fixture_root_owns_all_four_children_in_source_order() {
    let f = build_conditional_counter();
    let root_node = f.island.view_tree.get(f.root);
    match root_node {
        ViewNode::Element { tag, children, .. } => {
            assert_eq!(tag, "div");
            assert_eq!(
                children.as_slice(),
                &[
                    f.decrement_button,
                    f.span,
                    f.increment_button,
                    f.hint_paragraph,
                ],
                "the conditional branch's nodes live as ordinary children of the parent"
            );
        }
        other => panic!("expected root to be an element, got {other:?}"),
    }
}

#[test]
fn conditional_section_is_keyed_by_parent_node_id() {
    let f = build_conditional_counter();
    let sections = f
        .island
        .bindings
        .conditional_sections
        .get(&f.root)
        .expect("conditional sections must be keyed by the parent NodeId");
    assert_eq!(sections.len(), 1);
    let section = &sections[0];
    assert_eq!(section.condition, f.condition_expr);
    assert_eq!(section.then_children, vec![f.hint_paragraph]);
    assert!(
        section.else_children.is_empty(),
        "if-only shape has no else children"
    );
}

#[test]
fn condition_expr_is_count_gt_five() {
    let f = build_conditional_counter();
    match f.island.exprs.client_expr(f.condition_expr) {
        ClientExpr::Binary { op, left, right } => {
            assert_eq!(*op, ClientBinaryOp::Gt);
            assert_eq!(**left, ClientExpr::SignalRead(f.count_signal));
            assert_eq!(**right, ClientExpr::Literal(Value::Int(5)));
        }
        other => panic!("expected Binary, got {other:?}"),
    }
}

#[test]
fn building_two_conditionals_on_the_same_parent_preserves_order() {
    let mut b = IslandBuilder::new();
    let first_signal = b.signal(Value::Bool(true));
    let second_signal = b.signal(Value::Bool(false));
    let first_cond = b.expr(ClientExpr::SignalRead(first_signal));
    let second_cond = b.expr(ClientExpr::SignalRead(second_signal));
    let parent = b.element("div", None);
    let first_child = b.element("span", Some(parent));
    let second_child = b.element("em", Some(parent));
    b.conditional_section(parent, first_cond, vec![first_child], Vec::new());
    b.conditional_section(parent, second_cond, vec![second_child], Vec::new());
    let island = b.build();

    let sections = island
        .bindings
        .conditional_sections
        .get(&parent)
        .expect("conditional sections must be grouped by parent");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].condition, first_cond);
    assert_eq!(sections[0].then_children, vec![first_child]);
    assert_eq!(sections[1].condition, second_cond);
    assert_eq!(sections[1].then_children, vec![second_child]);
}

#[test]
fn reserving_an_outer_conditional_keeps_its_ordinal_during_recursive_lowering() {
    let mut b = IslandBuilder::new();
    let outer_signal = b.signal(Value::Bool(true));
    let nested_signal = b.signal(Value::Bool(false));
    let outer_cond = b.expr(ClientExpr::SignalRead(outer_signal));
    let nested_cond = b.expr(ClientExpr::SignalRead(nested_signal));
    let parent = b.element("div", None);
    let outer_child = b.element("section", Some(parent));
    let nested_child = b.element("aside", Some(parent));

    let outer = b.reserve_conditional_section(parent, outer_cond);
    let nested = b.reserve_conditional_section(parent, nested_cond);
    b.complete_conditional_section(parent, nested, vec![nested_child], Vec::new());
    b.complete_conditional_section(parent, outer, vec![outer_child], Vec::new());
    let island = b.build();

    let sections = &island.bindings.conditional_sections[&parent];
    assert_eq!(sections[0].condition, outer_cond);
    assert_eq!(sections[0].then_children, vec![outer_child]);
    assert_eq!(sections[1].condition, nested_cond);
    assert_eq!(sections[1].then_children, vec![nested_child]);
}
