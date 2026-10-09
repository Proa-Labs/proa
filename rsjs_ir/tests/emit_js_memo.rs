// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Memo (derived) cell emission: a hand-built island with two
//! leaf signals and one derived cell bound to a `<span>` must emit a
//! runtime `memo(...)` declaration with an explicit dep array,
//! push its `_dispose` into the island disposer list, and keep the
//! derived cell out of the seed-blob read path.

use rsjs_ir::{emit_js, ClientBinaryOp, ClientExpr, IslandBuilder, Value};

fn build_total_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let price = b.signal(Value::Int(10));
    let qty = b.signal(Value::Int(2));
    let body = b.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Mul,
        left: Box::new(ClientExpr::SignalRead(price)),
        right: Box::new(ClientExpr::SignalRead(qty)),
    });
    let total = b.memo(body);
    let total_read = b.expr(ClientExpr::SignalRead(total));

    let root = b.element("div", None);
    let span = b.element("span", Some(root));
    b.bind_text(span, total_read);
    b.set_root(root);

    b.build()
}

#[test]
fn memo_cell_emits_runtime_memo_declaration() {
    let island = build_total_island();
    let js = emit_js::emit(&island, "Total");

    assert!(
        js.contains(
            "const __sig_2 = memo(() => (__sig_0.get() * __sig_1.get()), [__sig_0, __sig_1]);"
        ),
        "derived cell should declare a memo with its dep array, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(__sig_2._dispose);"),
        "derived cell teardown must join the island disposer list, got:\n{js}"
    );
    assert!(
        js.contains(", memo,") || js.contains(" memo,") || js.contains(", memo "),
        "runtime import line should include the memo helper, got:\n{js}"
    );
    assert!(
        !js.contains("signal_initial(__rsjs_signal_initials, 2,"),
        "derived cells must never read the seed blob, got:\n{js}"
    );
    assert!(
        js.contains("bind_text(__node_1, () => __sig_2.get(), [__sig_2])"),
        "bindings reading the derived cell should subscribe to it directly, got:\n{js}"
    );
}

#[test]
fn memo_cell_keeps_leaf_signal_seed_reads() {
    let island = build_total_island();
    let js = emit_js::emit(&island, "Total");

    assert!(
        js.contains("const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, 10));"),
        "leaf signals should still seed from the dehydrated blob, got:\n{js}"
    );
    assert!(
        js.contains("const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, 2));"),
        "leaf signals should still seed from the dehydrated blob, got:\n{js}"
    );
}
