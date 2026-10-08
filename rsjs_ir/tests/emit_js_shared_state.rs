// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use rsjs_ir::{
    emit_js, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt, ClientType, EventName,
    IslandBuilder,
};

#[test]
fn shared_fields_reuse_signal_dependencies_without_local_seed_cells() {
    let mut builder = IslandBuilder::new();
    let store = builder.shared_store("workbench".into(), "crate::WorkbenchState".into(), None);
    let button_bg = builder.shared_field(store, "button_bg".into(), ClientType::String);
    let radius = builder.shared_field(store, "button_radius".into(), ClientType::I32);
    let label_body = builder.expr(ClientExpr::SignalRead(button_bg));
    let label = builder.memo(label_body);
    let label_read = builder.expr(ClientExpr::SignalRead(label));
    let increment =
        builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            radius,
            ClientSimpleClosure::new(
                "radius",
                ClientExpr::binary(
                    rsjs_ir::ClientBinaryOp::Add,
                    ClientExpr::signal_read(radius),
                    ClientExpr::int(1),
                ),
            ),
        )]));
    let root = builder.element("div", None);
    let label_node = builder.element("span", Some(root));
    let button = builder.element("button", Some(root));
    builder.bind_text(label_node, label_read);
    builder.bind_event(button, EventName::Click, increment);
    builder.set_root(root);

    let js = emit_js::emit(&builder.build(), "SharedWorkbench");
    assert!(js.contains("acquire_shared_state(root, 0)"), "{js}");
    assert!(
        js.contains("const __sig_0 = __rsjs_shared_0.field(\"button_bg\");"),
        "{js}"
    );
    assert!(
        js.contains("const __sig_1 = __rsjs_shared_0.field(\"button_radius\");"),
        "{js}"
    );
    assert!(
        js.contains("const __sig_2 = memo(() => __sig_0.get(), [__sig_0]);"),
        "memo must subscribe only to the shared field it reads:\n{js}"
    );
    assert!(
        !js.contains("read_signal_initials(root)"),
        "page-owned fields must not create island-local seed cells:\n{js}"
    );
    assert!(
        !js.contains("options.seedRoot") && !js.contains("options.islandRoot"),
        "standalone islands must acquire SharedState from their own root:\n{js}"
    );
}

#[test]
fn fragment_shared_state_metadata_uses_seed_root_while_dom_bindings_use_fragment_root() {
    let mut builder = IslandBuilder::new();
    let store = builder.shared_store("workbench".into(), "crate::WorkbenchState".into(), None);
    let button_bg = builder.shared_field(store, "button_bg".into(), ClientType::String);
    let radius = builder.shared_field(store, "button_radius".into(), ClientType::I32);
    let label_read = builder.expr(ClientExpr::SignalRead(button_bg));
    let increment =
        builder.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            radius,
            ClientSimpleClosure::new(
                "radius",
                ClientExpr::binary(
                    rsjs_ir::ClientBinaryOp::Add,
                    ClientExpr::signal_read(radius),
                    ClientExpr::int(1),
                ),
            ),
        )]));
    let root = builder.element("div", None);
    let label_node = builder.element("span", Some(root));
    let button = builder.element("button", Some(root));
    builder.bind_text(label_node, label_read);
    builder.bind_event(button, EventName::Click, increment);
    builder.set_root(root);

    let js = emit_js::emit_fragment(&builder.build(), "shared_workbench_fragment");

    assert!(
        js.contains("acquire_shared_state(options.seedRoot ?? root, 0)"),
        "a linked fragment must resolve adjacent SharedState metadata from its candidate wrapper:\n{js}"
    );
    assert!(
        js.contains("walk_path(root, [0], \"data-rsjs-bind\", \"1\")"),
        "the SharedState-backed DOM binding must stay scoped to the fragment's inner root:\n{js}"
    );
    assert!(
        !js.contains("walk_path(options.seedRoot ?? root")
            && !js.contains("acquire_shared_state(options.islandRoot"),
        "neither the outer seed wrapper nor the fused owner root may replace the fragment's DOM authority or metadata scope:\n{js}"
    );
}
