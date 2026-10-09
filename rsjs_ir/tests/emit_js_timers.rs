// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Timer primitive emission: a hand-built island with one
//! `rsjs::interval(ms, || ...)` declaration and one debounced event
//! handler must emit
//!
//! - `__rsjs_disposers.push(interval(<ms>, () => { ... }));` for the
//!   interval, gated behind an `interval` runtime import, and
//! - `const __handler_<id> = debounce((event) => { ... }, <delay_ms>);` plus
//!   a `__rsjs_disposers.push(__handler_<id>._dispose);` line for the
//!   debounced handler, gated behind a `debounce` runtime import,
//!
//! with the delegation table still referencing the wrapped handler by
//! its usual `__handler_<id>` name.

use rsjs_ir::{
    emit_js, ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, EventName,
    EventValue, IslandBuilder, Value,
};

fn build_ticker_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let tick = b.signal(Value::Int(0));
    let tick_read = b.expr(rsjs_ir::ClientExpr::SignalRead(tick));

    // rsjs::interval(1000, || tick.update(|t| t + 1));
    // The analyser aliases the update-closure parameter to the receiver
    // signal, so the cached body reads `tick` directly.
    let body = ClientBlock::from_stmts(vec![ClientStmt::signal_update(
        tick,
        ClientSimpleClosure::new(
            "t",
            ClientExpr::Binary {
                op: ClientBinaryOp::Add,
                left: Box::new(ClientExpr::SignalRead(tick)),
                right: Box::new(ClientExpr::Literal(Value::Int(1))),
            },
        ),
    )]);
    b.interval(1000, body);

    let root = b.element("div", None);
    let span = b.element("span", Some(root));
    b.bind_text(span, tick_read);
    b.set_root(root);

    b.build()
}

fn build_debounced_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let query = b.signal(Value::String(String::new()));
    let query_read = b.expr(rsjs_ir::ClientExpr::SignalRead(query));

    let root = b.element("div", None);
    let input = b.element("input", Some(root));
    let span = b.element("span", Some(root));
    b.bind_text(span, query_read);

    // on_input={rsjs::debounce(|event| query.set(event.value()), 300)}
    let body = ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        query,
        ClientExpr::EventRead(EventValue::TargetValue),
    )]);
    let handler = b.debounce_handler_from_ir(300, body);
    b.bind_event(input, EventName::Input, handler);
    b.set_root(root);

    b.build()
}

#[test]
fn interval_emits_disposer_pushed_runtime_interval() {
    let island = build_ticker_island();
    let js = emit_js::emit(&island, "Ticker");

    assert!(
        js.contains("__rsjs_disposers.push(interval(1000, () => {"),
        "interval declaration should lower to a disposer-pushed runtime interval, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.update((__c0) => (__sig_0.get() + 1));"),
        "the interval body should run the lowered handler statements, got:\n{js}"
    );
    assert!(
        js.contains("(root, __rsjs_disposers = []) => {"),
        "an island with an interval must receive the runtime-owned disposer list, got:\n{js}"
    );
    assert!(
        js.contains(" interval,") || js.contains(", interval "),
        "runtime import line should include the interval helper, got:\n{js}"
    );
    assert!(
        !js.contains("debounce"),
        "an interval-only island must not import the debounce helper, got:\n{js}"
    );
}

#[test]
fn debounced_handler_emits_wrapped_const_with_disposer() {
    let island = build_debounced_island();
    let js = emit_js::emit(&island, "Search");

    assert!(
        js.contains("const __handler_0 = debounce((event) => {"),
        "debounced handlers should declare the runtime debounce wrapper, got:\n{js}"
    );
    assert!(
        js.contains("    }, 300);"),
        "the delay must follow the handler in the runtime call, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(__handler_0._dispose);"),
        "the debounce wrapper's _dispose must join the island disposer list, got:\n{js}"
    );
    assert!(
        js.contains("__handler_0]"),
        "the delegation table should reference the wrapped handler by name, got:\n{js}"
    );
    assert!(
        js.contains(" debounce,") || js.contains(", debounce "),
        "runtime import line should include the debounce helper, got:\n{js}"
    );
    assert!(
        !js.contains("interval("),
        "a debounce-only island must not import the interval helper, got:\n{js}"
    );
}
