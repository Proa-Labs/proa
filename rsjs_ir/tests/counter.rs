// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M1 fixture: hand-construct the Counter island.
//!
//! Preserves the historical M1 Counter fixture. The quoted block is milestone
//! notation, not current authoring syntax.
//!
//! ```text
//! pub struct Counter { pub initial: i32 }
//!
//! #[rsjs] impl ... for Counter {
//!     fn render(self, cx: ...) -> ... {
//!         let count = signal::<i32>(self.initial);
//!         html_sync! {
//!             <div>
//!                 <button on_click={decrement(count)}>"-"</button>
//!                 <span>{count.get()}</span>
//!                 <button on_click={increment(count)}>"+"</button>
//!             </div>
//!         }
//!     }
//! }
//! ```
//!
//! The fixture is the M1 exit criterion: it pressure-tests the IR
//! shape end-to-end (signals + handlers + bindings + view tree)
//! before any emitter or pass exists to consume it.

use rsjs_ir::{
    Bindings, ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, EventName,
    ExprArena, ExprId, IslandBuilder, NodeId, Value, ViewNode,
};

/// Build the Counter fixture island.
///
/// Returns the finalised island plus the ids the assertions need to
/// reference (the `count` signal id, the two button node ids, the
/// span node id, the two handler ids, and the `count.get()` expr
/// id). Returning the ids out-of-band keeps the fixture's structural
/// assertions decoupled from the order the builder hands them out.
struct CounterFixture {
    island: rsjs_ir::Island,
    count_signal: rsjs_ir::SignalId,
    decrement_handler: rsjs_ir::EventHandlerId,
    increment_handler: rsjs_ir::EventHandlerId,
    decrement_button: NodeId,
    increment_button: NodeId,
    span: NodeId,
    count_read_expr: ExprId,
}

fn build_counter() -> CounterFixture {
    let mut b = IslandBuilder::new();

    // 1. Signal: `let count = signal::<i32>(0);`
    let count_signal = b.signal(Value::Int(0));

    // 2. Reactive expression for the span: `count.get()`. This is
    //    the expression the span's text binding points at.
    let count_read_expr = b.expr(ClientExpr::SignalRead(count_signal));

    // 3. Decrement handler: `count.update(|c| c - 1)`. We model the
    //    handler body as a single `SignalUpdate` statement whose
    //    closure body is `c - 1`.
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

    // 4. Increment handler: `count.update(|c| c + 1)`.
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

    // 5. View tree: <div> with three children: <button>, <span>, <button>.
    let root = b.element("div", None);
    let decrement_button = b.element("button", Some(root));
    let span = b.element("span", Some(root));
    let increment_button = b.element("button", Some(root));

    // 6. Span text content is driven by `count.get()`.
    //    text_node would also work, but the span will (in real
    //    output) carry static "+/-" siblings or similar, so we
    //    bind the span itself rather than allocating an interior
    //    slot. M1's placeholder ViewNode treats the span as an
    //    Element with no children but a text binding wired
    //    directly to its NodeId.
    b.bind_text(span, count_read_expr);

    // 7. Click bindings on the two buttons.
    b.bind_event(decrement_button, EventName::Click, decrement_handler);
    b.bind_event(increment_button, EventName::Click, increment_handler);

    b.set_root(root);

    CounterFixture {
        island: b.build(),
        count_signal,
        decrement_handler,
        increment_handler,
        decrement_button,
        increment_button,
        span,
        count_read_expr,
    }
}

#[test]
fn counter_has_one_signal_with_int_zero_initial() {
    let f = build_counter();
    assert_eq!(f.island.signals.len(), 1, "expected exactly one signal");
    let signal = &f.island.signals[0];
    assert_eq!(signal.id, f.count_signal);
    assert_eq!(signal.init, Value::Int(0));
}

#[test]
fn counter_has_two_handlers_each_mutating_the_count_signal() {
    let f = build_counter();
    assert_eq!(f.island.handlers.len(), 2, "expected exactly two handlers");

    // Both handlers should mutate `count` via Client IR v3 SignalUpdate.
    for handler in &f.island.handlers {
        assert_eq!(
            handler.body_ir().stmts.len(),
            1,
            "expected each handler to be a single statement"
        );
        match &handler.body_ir().stmts[0] {
            ClientStmt::SignalUpdate { id, closure } => {
                assert_eq!(*id, f.count_signal, "handler must mutate the count signal");
                assert_eq!(closure.param, "c");
                // Body is c +/- 1; assert the shape but don't pin
                // the operator (decrement vs increment).
                match &*closure.body {
                    ClientExpr::Binary { op, left, right } => {
                        assert!(matches!(op, ClientBinaryOp::Add | ClientBinaryOp::Sub));
                        assert_eq!(**left, ClientExpr::SignalRead(f.count_signal));
                        assert_eq!(**right, ClientExpr::Literal(Value::Int(1)));
                    }
                    other => panic!("expected BinaryOp body, got {other:?}"),
                }
            }
            other => panic!("expected SignalUpdate statement, got {other:?}"),
        }
    }

    // The two handlers must use opposite operators.
    let ops: Vec<ClientBinaryOp> = f
        .island
        .handlers
        .iter()
        .map(|h| match &h.body_ir().stmts[0] {
            ClientStmt::SignalUpdate { closure, .. } => match &*closure.body {
                ClientExpr::Binary { op, .. } => *op,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        })
        .collect();
    assert!(
        ops.contains(&ClientBinaryOp::Add) && ops.contains(&ClientBinaryOp::Sub),
        "expected one increment and one decrement, got {ops:?}"
    );
}

#[test]
fn counter_view_tree_root_has_three_children_in_source_order() {
    let f = build_counter();
    let root = f
        .island
        .view_tree
        .root()
        .expect("view tree must have a root");
    let root_node = f.island.view_tree.get(root);
    match root_node {
        ViewNode::Element { tag, children, .. } => {
            assert_eq!(tag, "div");
            assert_eq!(
                children.as_slice(),
                &[f.decrement_button, f.span, f.increment_button],
                "children must be in source order: decrement, span, increment"
            );
        }
        other => panic!("expected root to be an element, got {other:?}"),
    }
}

#[test]
fn span_text_binding_points_at_signal_read_of_count() {
    let f = build_counter();
    let bound_expr_id = f
        .island
        .bindings
        .text_bindings
        .get(&f.span)
        .copied()
        .expect("span must have a text binding");
    assert_eq!(bound_expr_id, f.count_read_expr);
    let bound_expr = f.island.exprs.client_expr(bound_expr_id);
    assert_eq!(bound_expr, &ClientExpr::SignalRead(f.count_signal));
}

#[test]
fn each_button_binds_click_to_its_handler() {
    let f = build_counter();
    let dec = f
        .island
        .bindings
        .event_bindings
        .get(&(f.decrement_button, EventName::Click))
        .copied()
        .expect("decrement button must have an on_click binding");
    assert_eq!(dec, f.decrement_handler);

    let inc = f
        .island
        .bindings
        .event_bindings
        .get(&(f.increment_button, EventName::Click))
        .copied()
        .expect("increment button must have an on_click binding");
    assert_eq!(inc, f.increment_handler);

    // No stray event bindings beyond the two clicks.
    assert_eq!(f.island.bindings.event_bindings.len(), 2);
}

#[test]
fn arena_and_signals_are_dense_and_monotonic() {
    let f = build_counter();
    // Signals are zero-indexed in declaration order.
    for (i, decl) in f.island.signals.iter().enumerate() {
        assert_eq!(decl.id, rsjs_ir::SignalId(i as u32));
    }
    // Handlers are zero-indexed in declaration order.
    for (i, h) in f.island.handlers.iter().enumerate() {
        assert_eq!(h.id, rsjs_ir::EventHandlerId(i as u32));
    }
    // The arena holds at least the count.get() expression we wired
    // into the span; it may hold more (the closures inside
    // handlers do not currently round-trip through the arena, but
    // text bindings do). Check the one we put there directly.
    let expr_arena_check: &ExprArena = &f.island.exprs;
    assert!(
        !expr_arena_check.is_empty(),
        "expected at least one expression in the arena"
    );
    assert_eq!(
        expr_arena_check.client_expr(f.count_read_expr),
        &ClientExpr::SignalRead(f.count_signal)
    );
}

#[test]
fn bindings_default_is_empty() {
    let bindings = Bindings::new();
    assert!(bindings.text_bindings.is_empty());
    assert!(bindings.event_bindings.is_empty());
}

#[test]
fn event_name_dom_and_attr_round_trip() {
    let events = [
        (EventName::Click, "click", "on_click"),
        (EventName::Input, "input", "on_input"),
        (EventName::Change, "change", "on_change"),
        (EventName::Paste, "paste", "on_paste"),
        (EventName::Focus, "focus", "on_focus"),
        (EventName::Blur, "blur", "on_blur"),
        (EventName::KeyDown, "keydown", "on_keydown"),
        (EventName::KeyUp, "keyup", "on_keyup"),
        (EventName::MouseDown, "mousedown", "on_mousedown"),
        (EventName::MouseUp, "mouseup", "on_mouseup"),
        (EventName::MouseMove, "mousemove", "on_mousemove"),
        (EventName::MouseOver, "mouseover", "on_mouseover"),
        (EventName::MouseOut, "mouseout", "on_mouseout"),
        (EventName::Scroll, "scroll", "on_scroll"),
        (EventName::Wheel, "wheel", "on_wheel"),
        (EventName::BeforeInput, "beforeinput", "on_beforeinput"),
        (
            EventName::CompositionStart,
            "compositionstart",
            "on_compositionstart",
        ),
        (
            EventName::CompositionUpdate,
            "compositionupdate",
            "on_compositionupdate",
        ),
        (
            EventName::CompositionEnd,
            "compositionend",
            "on_compositionend",
        ),
        (EventName::DragStart, "dragstart", "on_dragstart"),
        (EventName::DragEnter, "dragenter", "on_dragenter"),
        (EventName::DragOver, "dragover", "on_dragover"),
        (EventName::DragLeave, "dragleave", "on_dragleave"),
        (EventName::Drop, "drop", "on_drop"),
        (EventName::DragEnd, "dragend", "on_dragend"),
        (EventName::Toggle, "toggle", "on_toggle"),
        (EventName::BeforeToggle, "beforetoggle", "on_beforetoggle"),
        (EventName::PointerDown, "pointerdown", "on_pointerdown"),
        (EventName::PointerUp, "pointerup", "on_pointerup"),
        (EventName::PointerMove, "pointermove", "on_pointermove"),
        (EventName::PointerOver, "pointerover", "on_pointerover"),
        (EventName::PointerOut, "pointerout", "on_pointerout"),
        (EventName::PointerEnter, "pointerenter", "on_pointerenter"),
        (EventName::PointerLeave, "pointerleave", "on_pointerleave"),
        (
            EventName::GotPointerCapture,
            "gotpointercapture",
            "on_gotpointercapture",
        ),
        (
            EventName::LostPointerCapture,
            "lostpointercapture",
            "on_lostpointercapture",
        ),
        (EventName::TouchStart, "touchstart", "on_touchstart"),
        (EventName::TouchMove, "touchmove", "on_touchmove"),
        (EventName::TouchEnd, "touchend", "on_touchend"),
        (EventName::TouchCancel, "touchcancel", "on_touchcancel"),
        (EventName::DocumentClick, "click", "on_document_click"),
        (
            EventName::DocumentPointerDown,
            "pointerdown",
            "on_document_pointerdown",
        ),
        (EventName::DocumentKeyDown, "keydown", "on_document_keydown"),
        (EventName::WindowPopState, "popstate", "on_window_popstate"),
        (
            EventName::WindowHashChange,
            "hashchange",
            "on_window_hashchange",
        ),
        (EventName::WindowStorage, "storage", "on_window_storage"),
        (EventName::WindowResize, "resize", "on_window_resize"),
        (EventName::WindowScroll, "scroll", "on_window_scroll"),
        (EventName::Submit, "submit", "on_submit"),
    ];
    for (event, dom_name, attr_name) in events {
        assert_eq!(event.as_dom_name(), dom_name);
        assert_eq!(event.as_attr_name(), attr_name);
    }
}
