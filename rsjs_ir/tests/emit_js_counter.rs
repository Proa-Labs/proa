// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M2 golden test: feed the M1 Counter fixture through `emit_js::emit`
//! and assert the produced JS string matches a checked-in expected
//! output.
//!
//! The expected string is intentionally readable: 4-space indentation,
//! one declaration per line, semantic identifier names. Any future
//! change to the emitter that produces a different string must produce
//! a small, reviewable diff. Minification is a separate post-emit pass
//! and is not exercised here.

use rsjs_ir::{
    emit_js, CallbackSlotId, ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure,
    ClientStmt, ComponentFragmentCallback, EventName, HydrationStrategy, IslandBuilder,
    JsonPointer, Value,
};

/// Hand-build the Counter fixture (mirrors `tests/counter.rs`).
fn build_counter() -> rsjs_ir::Island {
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

    b.bind_text(span, count_read_expr);
    b.bind_event(decrement_button, EventName::Click, decrement_handler);
    b.bind_event(increment_button, EventName::Click, increment_handler);

    b.set_root(root);

    b.build()
}

#[test]
fn counter_emits_expected_js() {
    let island = build_counter();
    let actual = emit_js::emit(&island, "Counter");
    let expected = include_str!("emit_js_counter.expected.js");
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
fn counter_emit_is_byte_identical_across_repeated_runs() {
    // HashMap iteration order can shift between processes (and, on some
    // platforms, between calls within a single process). Re-emit many
    // times to make sure the sort step in emit_js fully removes that
    // nondeterminism.
    let island = build_counter();
    let first = emit_js::emit(&island, "Counter");
    for _ in 0..32 {
        assert_eq!(emit_js::emit(&island, "Counter"), first);
    }
}

#[test]
fn counter_can_emit_handlers_through_client_ir_v3() {
    let island = build_counter();
    let js = emit_js::emit(&island, "Counter");

    assert!(
        js.contains("__sig_0.update((__c0) => (__sig_0.get() - 1));"),
        "handler emission should emit decrement update, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.update((__c0) => (__sig_0.get() + 1));"),
        "handler emission should emit increment update, got:\n{js}"
    );
    assert!(
        js.contains("bind_delegated_events(root, \"click\""),
        "handler emission should preserve event binding emission, got:\n{js}"
    );
}

#[test]
fn visible_hydration_strategy_uses_visible_runtime_entrypoint() {
    let mut island = build_counter();
    island.hydration_strategy = HydrationStrategy::Visible;
    let js = emit_js::emit(&island, "Counter");

    assert!(
        js.contains("hydrate_when_visible"),
        "visible islands should import and call the visible hydration entrypoint, got:\n{js}"
    );
    assert!(
        js.contains("hydrate_from_module"),
        "visible strategy should route module evaluation through generation fencing, got:\n{js}"
    );
    assert!(
        js.contains(
            "hydrate_from_module(import.meta.url, hydrate_when_visible)(\"Counter\", (root, __rsjs_disposers = []) => {"
        ),
        "visible strategy should register its exact module URL and callback, got:\n{js}"
    );
}

#[test]
fn interaction_hydration_strategy_emits_event_list() {
    let mut island = build_counter();
    island.hydration_strategy = HydrationStrategy::Interaction {
        event: Some("click".to_string()),
    };
    let js = emit_js::emit(&island, "Counter");

    assert!(
        js.contains(
            "hydrate_from_module(import.meta.url, hydrate_on_interaction)(\"Counter\", (root, __rsjs_disposers = []) => {"
        ),
        "interaction strategy should pass its resource stack to the hydration runtime, got:\n{js}"
    );
    assert!(
        !js.contains("const __rsjs_disposers = [];"),
        "hydrated owners should use the runtime-owned rollback stack, got:\n{js}"
    );
    assert!(
        js.ends_with("}, [\"click\"]);\n"),
        "interaction strategy should pass the configured event list, got:\n{js}"
    );
}

#[test]
fn media_hydration_strategy_emits_query() {
    let mut island = build_counter();
    island.hydration_strategy = HydrationStrategy::Media {
        query: "(min-width: 768px)".to_string(),
    };
    let js = emit_js::emit(&island, "Counter");

    assert!(
        js.contains(
            "hydrate_from_module(import.meta.url, hydrate_when_media)(\"Counter\", (root, __rsjs_disposers = []) => {"
        ),
        "media strategy should call the media hydration entrypoint, got:\n{js}"
    );
    assert!(
        js.ends_with("}, \"(min-width: 768px)\");\n"),
        "media strategy should pass the media query, got:\n{js}"
    );
}

#[test]
fn counter_fragment_emits_mount_export_without_self_hydrating() {
    let island = build_counter();
    let js = emit_js::emit_fragment(&island, "counter_1234abcd");
    assert!(
        js.contains("export function mount_counter_1234abcd(root, options = {}) {"),
        "fragment module should export a stable mount function, got:\n{js}"
    );
    assert!(
        !js.contains("hydrate("),
        "fragment modules must not self-hydrate; got:\n{js}"
    );
    assert!(
        js.contains("bind_delegated_events(root, \"click\""),
        "fragment module should still emit the component's binding logic, got:\n{js}"
    );
    assert!(
        js.contains(
            "const __rsjs_signal_initials = read_signal_initials(options.seedRoot ?? root);"
        ),
        "fragment modules must be able to read seeds from an inert linked-member wrapper while \
         binding against its first element child; got:\n{js}"
    );
    assert!(
        js.contains("return () => dispose_all(__rsjs_disposers, \"rsjs bindings\");"),
        "fragment modules should expose a disposer for their bindings, got:\n{js}"
    );
}

#[test]
fn parent_fragment_mounts_are_guarded_and_disposable() {
    let mut b = IslandBuilder::new();
    let root = b.element("div", None);
    let child_root = b.element("section", Some(root));
    b.component_fragment_mount(
        child_root,
        "child_1234abcd".to_string(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![child_root],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    b.set_root(root);
    let island = b.build();

    let js = emit_js::emit(&island, "ParentWithChild");
    assert!(
        js.contains("dispose_all") && js.contains("report_hydration_error"),
        "parent islands with fragment mounts should import lifecycle helpers, got:\n{js}"
    );
    assert!(
        js.contains("(root, __rsjs_disposers = []) => {"),
        "parent island should collect child fragment disposers in the runtime-owned stack, got:\n{js}"
    );
    assert!(
        js.contains("try {")
            && js.contains("const __rsjs_fragment_disposer_1 = __rsjs_mount_child_1234abcd("),
        "fragment mount should be wrapped and captured as a disposer, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(__rsjs_restore_callbacks_1);")
            && js.contains(
                "__rsjs_restore_callbacks_1();\n        report_hydration_error(\"component fragment child_1234abcd\""
            ),
        "callback-table restoration must be registered before the fallible mount and run immediately on failure, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_component_root_1.__rsjs_callbacks = {")
            && !js.contains(".__rsjs_callbacks ?? {}"),
        "even a callback-free rehydrate must install a fresh empty table instead of inheriting stale optional callbacks, got:\n{js}"
    );
    assert!(
        js.contains(
            "report_hydration_error(\"component fragment child_1234abcd\", __rsjs_fragment_error);"
        ),
        "fragment mount errors should be reported without aborting the parent, got:\n{js}"
    );
    assert!(
        js.contains("return () => dispose_all(__rsjs_disposers, \"rsjs bindings\");"),
        "parent island should return a disposer for child fragments, got:\n{js}"
    );
}

#[test]
fn parent_fragment_callback_adapters_import_runtime_helpers() {
    let mut b = IslandBuilder::new();
    let root = b.element("div", None);
    let child_root = b.element("section", Some(root));
    let callback = ComponentFragmentCallback::from_ir(
        CallbackSlotId(0),
        ClientBlock::from_stmts(vec![ClientStmt::expr(ClientExpr::Project {
            target: Box::new(ClientExpr::CallbackPayload),
            path: JsonPointer::new("/value"),
        })]),
    );
    b.component_fragment_mount(
        child_root,
        "child_1234abcd".to_string(),
        vec![callback],
        Vec::new(),
        Vec::new(),
        vec![child_root],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    b.set_root(root);
    let island = b.build();

    let js = emit_js::emit(&island, "ParentWithProjectingCallback");
    assert!(
        js.contains("import { rsjs } from \"/rsjs-runtime-query.js\";"),
        "callback adapter runtime helper use should import rsjs namespace, got:\n{js}"
    );
    assert!(
        js.contains("rsjs.__project(__payload"),
        "callback adapter should emit the projection helper call, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_had_callbacks_1 = Object.prototype.hasOwnProperty.call(")
            && js.contains("__rsjs_previous_callbacks_1")
            && js.contains("__rsjs_callbacks_restored_1")
            && js.contains("__callback_0: (__payload) => {")
            && !js.contains(".__rsjs_callbacks ?? {}"),
        "callback mounts must install an exact table and retain an idempotent restoration snapshot, got:\n{js}"
    );
}
