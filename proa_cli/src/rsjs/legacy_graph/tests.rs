// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use rsjs_ir::abi::HelperCallEdgeId;
use rsjs_ir::{
    ClientBrowserEffect, ClientBrowserRead, ClientFunctionRef, ClientLocalId, ClientSimpleClosure,
    ClientStmt, ClientType, EventValue, LocationField, MutationId, NodeRefAccessor, NodeRefId,
    QueryAccessor, QueryId, SignalId, StorageArea,
};
use serde_json::json;

fn expression_json(expression: &ClientExpr) -> Value {
    serde_json::from_str(&client_expr_json(expression)).unwrap()
}

fn block_json(block: &ClientBlock) -> Value {
    serde_json::from_str(&client_block_json(block)).unwrap()
}

fn type_json(client_type: &ClientType) -> Value {
    let mut json = String::new();
    write_client_type_json(&mut json, client_type);
    serde_json::from_str(&json).unwrap()
}

fn base_descriptor() -> Map<String, Value> {
    let expression = ClientExpr::SignalRead(SignalId(2));
    let handler = ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        SignalId(7),
        ClientExpr::SignalRead(SignalId(2)),
    )]);
    json!({
        "schema_version": 9,
        "format": "rsjs.component_descriptor",
        "client_ir_version": CLIENT_IR_SCHEMA,
        "asset_id": "test.component",
        "js_hash": format!("sha256:{}", "0".repeat(64)),
        "struct_name": "Fixture",
        "self_ty": "crate::Fixture",
        "is_async": false,
        "client_origin_permission": "origin_capable",
        "callback_slots": [{"id": 9, "name": "on_change"}],
        "fields": [],
        "inbound_inputs": {
            "values": [],
            "prop_sites": [],
            "crossings": [],
            "node_ref_fields": [],
            "projection_fields": []
        },
        "shared_stores": [],
        "signals": [
            {
                "id": 2,
                "init": {"kind": "int", "value": 1},
                "ty": type_json(&ClientType::I32)
            },
            {
                "id": 7,
                "init": {"kind": "int", "value": 2},
                "ty": type_json(&ClientType::I32),
                "derived": 3
            }
        ],
        "node_refs": [],
        "exprs": [{"id": 3, "expr": expression_json(&expression)}],
        "handlers": [{
            "id": 5,
            "body_ir_version": CLIENT_IR_SCHEMA,
            "body_ir": block_json(&handler)
        }],
        "view_tree": {
            "root": 0,
            "nodes": [
                {
                    "id": 0,
                    "node": {
                        "kind": "element",
                        "tag": "button",
                        "children": [1],
                        "static_attrs": [],
                        "server_attrs": [],
                        "payload_attrs": []
                    }
                },
                {"id": 1, "node": {"kind": "text_binding_slot"}}
            ]
        },
        "bindings": {
            "text": [{"node": 1, "expr": 3}],
            "events": [{"node": 0, "event": "click", "handler": 5}],
            "conditionals": [],
            "inputs": [{"node": 0, "kind": "value", "signal": 7}],
            "attrs": [{"node": 0, "name": "aria-label", "kind": "value", "expr": 3}],
            "refs": []
        },
        "queries": [],
        "mutations": [],
        "counts": {
            "fields": 0,
            "values": 0,
            "prop_sites": 0,
            "crossings": 0,
            "signals": 2,
            "handlers": 1,
            "callback_slots": 1,
            "node_ref_fields": 0,
            "node_refs": 0,
            "projection_fields": 0,
            "exprs": 1,
            "nodes": 2
        },
        "descriptor_hash": format!("sha256:{}", "1".repeat(64))
    })
    .as_object()
    .unwrap()
    .clone()
}

fn add_query(descriptor: &mut Map<String, Value>, id: u32) {
    let expressions = descriptor.get_mut("exprs").unwrap().as_array_mut().unwrap();
    for (expression_id, expression) in [
        (
            4,
            ClientExpr::QueryKey(vec![
                ClientExpr::string("user"),
                ClientExpr::SignalRead(SignalId(2)),
            ]),
        ),
        (5, ClientExpr::string("server-initial")),
        (6, ClientExpr::string("/api/users")),
    ] {
        expressions.push(json!({
            "id": expression_id,
            "expr": expression_json(&expression),
        }));
    }
    descriptor
        .get_mut("queries")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": id,
            "source_name": "user_query",
            "source_anchor": {"start_line": 41, "start_column": 9},
            "key_expr": 4,
            "server_initial": 5,
            "client_fetcher": 6,
            "data_ty": type_json(&ClientType::String),
            "options": {
                "interval_ms": 1500,
                "refetch_on_focus": true,
                "download_progress": true,
                "stream_text": false,
                "retry_count": 3,
                "stale_time_ms": 2500
            }
        }));
    descriptor.insert(
        "query_initializers".to_owned(),
        json!([{
            "id": id,
            "expr": "server_user . clone ()",
            "key_expr": "rsjs :: QueryKey :: new ((\"user\", user_id))"
        }]),
    );
    descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("exprs".to_owned(), json!(4));
}

fn add_shared_field(descriptor: &mut Map<String, Value>, signal_index: usize) {
    descriptor
        .get_mut("shared_stores")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 4,
            "source_name": "workbench",
            "rust_type": "crate :: WorkbenchState",
            "source_anchor": {"start_line": 17, "start_column": 13}
        }));
    descriptor
        .get_mut("signals")
        .unwrap()
        .as_array_mut()
        .unwrap()[signal_index]
        .as_object_mut()
        .unwrap()
        .insert(
            "shared".to_owned(),
            json!({"store": 4, "field": "selected_user"}),
        );
}

fn add_node_ref(
    descriptor: &mut Map<String, Value>,
    id: u32,
    source_name: Option<&str>,
    source_anchor: Option<(u32, u32)>,
) {
    let mut node_ref = serde_json::Map::from_iter([("id".to_owned(), json!(id))]);
    if let Some(source_name) = source_name {
        node_ref.insert("source_name".to_owned(), json!(source_name));
    }
    if let Some((start_line, start_column)) = source_anchor {
        node_ref.insert(
            "source_anchor".to_owned(),
            json!({"start_line": start_line, "start_column": start_column}),
        );
    }
    let declarations = descriptor
        .get_mut("node_refs")
        .unwrap()
        .as_array_mut()
        .unwrap();
    declarations.push(Value::Object(node_ref));
    descriptor["counts"]["node_refs"] = json!(declarations.len());
}

fn make_keyed_collection_descriptor() -> Map<String, Value> {
    let mut descriptor = base_descriptor();
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "kind": "element",
                    "tag": "ul",
                    "children": [1],
                    "static_attrs": [{"name": "data-rsjs-list", "value": "0"}],
                    "server_attrs": [],
                    "payload_attrs": []
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "for_loop",
                    "pat": "item",
                    "expr": "items",
                    "attrs": "",
                    "body_prefix": "",
                    "label": "items",
                    "children": [2]
                }
            },
            {
                "id": 2,
                "node": {
                    "kind": "element",
                    "tag": "li",
                    "children": [],
                    "static_attrs": [],
                    "server_attrs": [],
                    "payload_attrs": []
                }
            }
        ]
    });
    descriptor["bindings"] = json!({
        "text": [],
        "events": [],
        "conditionals": [],
        "inputs": [],
        "attrs": [],
        "refs": []
    });
    descriptor.insert(
        "keyed_collections".to_owned(),
        json!([{
            "id": 0,
            "container": 0,
            "loop_node": 1,
            "item_root": 2,
            "source": {"kind": "expression", "id": 3},
            "key": {"kind": "field", "name": "id"},
            "key_tokens": "item.id",
            "item": {
                "kind": "element",
                "tag": "li",
                "static_attrs": [],
                "dynamic_attrs": [{
                    "name": "data-id",
                    "value": {"kind": "field", "name": "id"}
                }],
                "children": [{
                    "kind": "slot",
                    "value": {"kind": "field", "name": "label"}
                }]
            }
        }]),
    );
    descriptor["counts"]["nodes"] = json!(3);
    descriptor["counts"]
        .as_object_mut()
        .expect("counts object")
        .insert("keyed_collections".to_owned(), json!(1));
    descriptor
}

fn make_payload_descriptor() -> Map<String, Value> {
    let mut descriptor = base_descriptor();
    descriptor["exprs"][0]["expr"] = expression_json(&ClientExpr::NodePayload {
        attr: "data-rsjs-payload-p0".to_owned(),
        tokens: "item.label".to_owned(),
    });
    descriptor["handlers"][0]["body_ir"] =
        block_json(&ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p1".to_owned(),
            },
        )]));
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "kind": "element",
                    "tag": "div",
                    "children": [1],
                    "static_attrs": [],
                    "server_attrs": [],
                    "payload_attrs": []
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "for_loop",
                    "pat": "item",
                    "expr": "items",
                    "attrs": "",
                    "body_prefix": "",
                    "label": "items",
                    "children": [2]
                }
            },
            {
                "id": 2,
                "node": {
                    "kind": "element",
                    "tag": "button",
                    "children": [],
                    "static_attrs": [],
                    "server_attrs": [],
                    "payload_attrs": [
                        {"name": "data-rsjs-payload-p0", "tokens": "item.label"},
                        {"name": "data-rsjs-payload-p1", "tokens": "item.id"}
                    ]
                }
            }
        ]
    });
    descriptor["bindings"] = json!({
        "text": [],
        "events": [{"node": 2, "event": "click", "handler": 5}],
        "conditionals": [],
        "inputs": [],
        "attrs": [{"node": 2, "name": "aria-label", "kind": "value", "expr": 3}],
        "refs": []
    });
    descriptor["counts"]["nodes"] = json!(3);
    descriptor
}

#[test]
fn parses_sparse_ids_and_recomputes_dependencies() {
    let graph = parse_legacy_client_graph(&base_descriptor(), &BTreeSet::new()).unwrap();

    assert_eq!(
        graph
            .signals
            .iter()
            .map(|signal| signal.id)
            .collect::<Vec<_>>(),
        vec![2, 7]
    );
    assert_eq!(graph.expressions[0].id, 3);
    assert_eq!(graph.expressions[0].dependencies.signal_reads[0].0, 2);
    assert_eq!(graph.handlers[0].id, 5);
    assert_eq!(graph.handlers[0].dependencies.signal_reads[0].0, 2);
    assert_eq!(graph.handlers[0].dependencies.signal_writes[0].0, 7);
    assert_eq!(graph.callback_slots[0].id, 9);
    assert_eq!(graph.bindings.events[0].handler, 5);
    assert!(graph.unsupported_features.is_empty());
}

#[test]
fn unknown_local_signal_outside_authenticated_inbound_set_stays_unsupported() {
    let mut descriptor = base_descriptor();
    descriptor["signals"][0]["ty"] = type_json(&ClientType::Unknown);

    let graph = parse_legacy_client_graph_with_inbound_signals(
        &descriptor,
        &BTreeSet::new(),
        &BTreeSet::from([7]),
        &BTreeSet::new(),
    )
    .unwrap();

    assert!(graph
        .unsupported_features
        .iter()
        .any(|feature| feature == "unsupported_shapes"));
}

#[test]
fn keyed_collection_is_canonical_and_keeps_its_source_reachable() {
    let graph =
        parse_legacy_client_graph(&make_keyed_collection_descriptor(), &BTreeSet::new()).unwrap();

    assert_eq!(graph.keyed_collections.len(), 1);
    assert_eq!(
        graph.keyed_collections[0].source,
        LegacyKeyedCollectionSource::Expression(3)
    );
    assert!(graph
        .expressions
        .iter()
        .any(|expression| expression.id == 3));
    assert!(graph.signals.iter().any(|signal| signal.id == 2));
    assert!(!graph
        .unsupported_features
        .iter()
        .any(|feature| feature == "unsupported_shapes"));
}

#[test]
fn joins_repeated_node_and_event_payloads_to_their_declaring_node() {
    let mut descriptor = make_payload_descriptor();
    descriptor["view_tree"]["nodes"][2]["node"]["payload_attrs"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "data-rsjs-payload-p2",
            "tokens": "item.latent"
        }));

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert!(graph.expressions[0].dependencies.uses_node_payload);
    assert!(graph.handlers[0].dependencies.uses_event_payload);
    assert!(graph.unsupported_features.is_empty());
}

#[test]
fn ordinary_event_reads_do_not_require_compiler_payload_attributes() {
    let mut descriptor = make_payload_descriptor();
    descriptor["handlers"][0]["body_ir"] =
        block_json(&ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventRead(EventValue::TargetValue),
        )]));
    descriptor["view_tree"]["nodes"][2]["node"]["payload_attrs"]
        .as_array_mut()
        .unwrap()
        .retain(|attr| attr["name"] != "data-rsjs-payload-p1");

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert!(graph.handlers[0].dependencies.uses_event_payload);
    assert!(graph.unsupported_features.is_empty());
}

#[test]
fn node_payload_cannot_double_as_a_reachable_mutation_invalidation_key() {
    let mut descriptor = make_payload_descriptor();
    for (id, expression) in [
        (4, ClientExpr::string("mutation-key")),
        (5, ClientExpr::string("server-runner")),
        (6, ClientExpr::string("client-runner")),
    ] {
        descriptor["exprs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id, "expr": expression_json(&expression)}));
    }
    descriptor["counts"]["exprs"] = json!(4);
    descriptor["handlers"][0]["body_ir"] =
        block_json(&ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
            mutation: MutationId(11),
            args: Vec::new(),
        }]));
    descriptor["mutations"] = json!([{
        "id": 11,
        "source_name": "save",
        "key_expr": 4,
        "server_runner": 5,
        "client_runner": 6,
        "invalidates": [3],
        "options": {
            "retry_count": 0,
            "on_success_invalidate_strategy": "refetch"
        }
    }]);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert!(graph.reachable_mutations.contains(&11));
    assert!(graph
        .unsupported_features
        .iter()
        .any(|feature| feature == "unsupported_shapes"));
}

#[test]
fn extra_signal_roots_close_payload_and_indirect_resource_dependencies() {
    let mut descriptor = base_descriptor();
    descriptor["bindings"] = json!({
        "text": [],
        "events": [],
        "conditionals": [],
        "inputs": [],
        "attrs": [],
        "refs": []
    });
    add_node_ref(&mut descriptor, 4, Some("exported_target"), None);
    add_query(&mut descriptor, 13);
    descriptor["exprs"][1]["expr"] = expression_json(&ClientExpr::NodeRefRead {
        id: NodeRefId(4),
        accessor: NodeRefAccessor::Value,
    });
    for (id, expression) in [
        (7, ClientExpr::string("mutation-key")),
        (8, ClientExpr::string("server-runner")),
        (9, ClientExpr::string("client-runner")),
    ] {
        descriptor["exprs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id, "expr": expression_json(&expression)}));
    }
    descriptor["exprs"][0]["expr"] = expression_json(&ClientExpr::QueryKey(vec![
        ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p0".to_owned(),
            tokens: "item.label".to_owned(),
        },
        ClientExpr::QueryRead {
            id: QueryId(13),
            accessor: QueryAccessor::Data,
        },
        ClientExpr::MutationRead {
            id: MutationId(11),
            accessor: MutationAccessor::IsPending,
        },
    ]));
    descriptor["counts"]["exprs"] = json!(7);
    descriptor["mutations"] = json!([{
        "id": 11,
        "source_name": "save",
        "key_expr": 7,
        "server_runner": 8,
        "client_runner": 9,
        "invalidates": [],
        "options": {
            "retry_count": 0,
            "on_success_invalidate_strategy": "refetch"
        }
    }]);

    let latent = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert!(latent.reachable_queries.is_empty());
    assert!(latent.reachable_mutations.is_empty());
    assert!(latent.reachable_node_refs.is_empty());
    assert!(!latent
        .unsupported_features
        .iter()
        .any(|feature| feature == "unsupported_shapes"));

    let graph = parse_legacy_client_graph_with_inbound_signals(
        &descriptor,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::from([7]),
    )
    .unwrap();

    assert_eq!(graph.reachable_queries, vec![13]);
    assert_eq!(graph.reachable_mutations, vec![11]);
    assert_eq!(graph.reachable_node_refs, vec![4]);
    assert_eq!(graph.reachable_expressions, vec![3, 4, 5, 6, 7, 8, 9]);
    assert!(graph
        .unsupported_features
        .iter()
        .any(|feature| feature == "unsupported_shapes"));
}

#[test]
fn rejects_noncanonical_or_mismatched_payload_declarations() {
    let mut noncanonical = make_payload_descriptor();
    noncanonical["view_tree"]["nodes"][2]["node"]["payload_attrs"][0]["name"] =
        json!("data-payload");
    let error = parse_legacy_client_graph(&noncanonical, &BTreeSet::new()).unwrap_err();
    assert!(
        error.contains("canonical compiler-owned payload name"),
        "{error}"
    );

    let mut mismatched = make_payload_descriptor();
    mismatched["view_tree"]["nodes"][2]["node"]["payload_attrs"][0]["tokens"] = json!("item.other");
    let error = parse_legacy_client_graph(&mismatched, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("tokens"), "{error}");
    assert!(error.contains("do not match"), "{error}");
}

#[test]
fn rejects_payload_reads_without_the_exact_runtime_node_context() {
    let mut missing_event = make_payload_descriptor();
    missing_event["view_tree"]["nodes"][2]["node"]["payload_attrs"]
        .as_array_mut()
        .unwrap()
        .retain(|attr| attr["name"] != "data-rsjs-payload-p1");
    let error = parse_legacy_client_graph(&missing_event, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("handler 5 reads event payload"), "{error}");

    let mut mixed_text_slot = make_payload_descriptor();
    mixed_text_slot["view_tree"]["nodes"][2]["node"]["children"] = json!([3]);
    mixed_text_slot["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": 3, "node": {"kind": "text_binding_slot"}}));
    mixed_text_slot["bindings"]["attrs"] = json!([]);
    mixed_text_slot["bindings"]["text"] = json!([{"node": 3, "expr": 3}]);
    mixed_text_slot["counts"]["nodes"] = json!(4);
    let error = parse_legacy_client_graph(&mixed_text_slot, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("node-payload expression"), "{error}");
    assert!(error.contains("TextBindingSlot"), "{error}");
}

#[test]
fn keyed_collection_marker_must_name_its_exact_container() {
    let mut descriptor = make_keyed_collection_descriptor();
    descriptor["view_tree"]["nodes"][0]["node"]["static_attrs"][0]["value"] = json!("2");

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        error.contains("data-rsjs-list=\"2\"; expected \"0\""),
        "{error}"
    );
}

#[test]
fn keyed_collection_container_cannot_repeat_inside_an_outer_loop() {
    let mut descriptor = make_keyed_collection_descriptor();
    descriptor["view_tree"]["root"] = json!(3);
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({
                "id": 3,
                "node": {
                    "kind": "element",
                    "tag": "section",
                    "children": [4],
                    "static_attrs": [],
                    "server_attrs": [],
                    "payload_attrs": []
                }
            }),
            json!({
                "id": 4,
                "node": {
                    "kind": "for_loop",
                    "pat": "group",
                    "expr": "groups",
                    "attrs": "",
                    "body_prefix": "",
                    "label": "groups",
                    "children": [0]
                }
            }),
        ]);
    descriptor["counts"]["nodes"] = json!(5);

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        error.contains("keyed collection 0 container node 0 is nested inside another for-loop"),
        "{error}"
    );
}

#[test]
fn accepts_text_bindings_on_element_or_dedicated_slot_nodes() {
    let mut descriptor = base_descriptor();
    descriptor["bindings"]["text"][0]["node"] = json!(0);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert_eq!(graph.bindings.text[0].node, 0);
}

#[test]
fn preserves_node_refs_and_closes_reachability_from_bindings_and_bound_handlers() {
    let mut descriptor = base_descriptor();
    add_node_ref(&mut descriptor, 4, Some("input"), Some((21, 13)));
    add_node_ref(&mut descriptor, 9, Some("dialog"), None);
    add_node_ref(&mut descriptor, 12, Some("dead"), None);
    descriptor["bindings"]["refs"] = json!([{"node": 0, "ref": 4}]);
    descriptor["handlers"][0]["body_ir"] =
        block_json(&ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
            id: NodeRefId(9),
            effect: ClientNodeRefEffect::Focus,
        }]));
    descriptor["handlers"].as_array_mut().unwrap().push(json!({
        "id": 8,
        "body_ir_version": CLIENT_IR_SCHEMA,
        "body_ir": block_json(&ClientBlock::from_stmts(vec![
            ClientStmt::NodeRefEffect {
                id: NodeRefId(12),
                effect: ClientNodeRefEffect::Focus,
            },
        ]))
    }));
    descriptor["counts"]["handlers"] = json!(2);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert_eq!(
        graph
            .node_refs
            .iter()
            .map(|node_ref| (
                node_ref.id,
                node_ref.source_name.as_deref(),
                node_ref.source_anchor,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                4,
                Some("input"),
                Some(SourceAnchor {
                    start_line: 21,
                    start_column: 13,
                }),
            ),
            (9, Some("dialog"), None),
            (12, Some("dead"), None),
        ],
    );
    assert_eq!(
        graph.bindings.node_refs,
        vec![LegacyNodeRefBinding {
            node: 0,
            node_ref: 4,
        }]
    );
    assert_eq!(graph.reachable_node_refs, vec![4, 9]);
    assert!(
        graph.unsupported_features.is_empty(),
        "component-local refs are part of the linked-owner subset"
    );
}

#[test]
fn node_ref_reachability_follows_live_signal_derived_expressions() {
    let mut descriptor = base_descriptor();
    add_node_ref(&mut descriptor, 4, Some("input"), None);
    descriptor["signals"][1]["derived"] = json!(4);
    descriptor["exprs"].as_array_mut().unwrap().push(json!({
        "id": 4,
        "expr": expression_json(&ClientExpr::NodeRefRead {
            id: NodeRefId(4),
            accessor: NodeRefAccessor::Value,
        }),
    }));
    descriptor["counts"]["exprs"] = json!(2);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert_eq!(graph.reachable_node_refs, vec![4]);
}

#[test]
fn node_ref_bindings_reject_dangling_non_element_and_duplicate_targets() {
    let mut dangling = base_descriptor();
    dangling["bindings"]["refs"] = json!([{"node": 0, "ref": 99}]);
    let error = parse_legacy_client_graph(&dangling, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("undeclared ID 99"), "{error}");

    let mut non_element = base_descriptor();
    add_node_ref(&mut non_element, 4, Some("target"), None);
    non_element["bindings"]["refs"] = json!([{"node": 1, "ref": 4}]);
    let error = parse_legacy_client_graph(&non_element, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("expected Element"), "{error}");

    let mut duplicate = base_descriptor();
    add_node_ref(&mut duplicate, 4, Some("target"), None);
    duplicate["view_tree"]["nodes"][1]["node"] = json!({
        "kind": "element",
        "tag": "span",
        "children": [],
        "static_attrs": [],
        "server_attrs": [],
        "payload_attrs": []
    });
    duplicate["bindings"]["refs"] = json!([{"node": 0, "ref": 4}, {"node": 1, "ref": 4}]);
    let error = parse_legacy_client_graph(&duplicate, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("bound to multiple nodes 0 and 1"), "{error}");
}

#[test]
fn cross_kind_nested_node_ref_operations_remain_fail_closed() {
    let mut descriptor = base_descriptor();
    add_node_ref(&mut descriptor, 4, Some("input"), None);
    let expression = ClientExpr::BrowserRead(ClientBrowserRead::Storage {
        area: rsjs_ir::BrowserStorageArea::Local,
        value_type: rsjs_ir::StorageValueType::String,
        key: Box::new(ClientExpr::NodeRefRead {
            id: NodeRefId(4),
            accessor: NodeRefAccessor::Value,
        }),
        fallback: None,
    });
    descriptor["exprs"][0]["expr"] = expression_json(&expression);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();

    assert_eq!(graph.reachable_node_refs, vec![4]);
    assert_eq!(
        graph.unsupported_features,
        vec!["unsupported_shapes"],
        "a nested cross-kind consumer needs unified canonical operation ordering"
    );
}

#[test]
fn browser_operations_preserve_preorder_consumers_and_exact_inputs() {
    let nested_read = ClientExpr::BrowserRead(ClientBrowserRead::Location(LocationField::Pathname));
    let expression = ClientExpr::BrowserRead(ClientBrowserRead::UrlSearchParam {
        key: Box::new(nested_read.clone()),
        fallback: Some(Box::new(ClientExpr::SignalRead(SignalId(2)))),
    });
    let expression_operations = collect_expression_browser_operations(&expression).unwrap();
    assert_eq!(expression_operations.len(), 2);
    assert_eq!(
        expression_operations[0].kind,
        LegacyBrowserOperationKind::Read
    );
    assert_eq!(
        expression_operations[0].consumer,
        LegacyBrowserConsumer::Enclosing
    );
    assert_eq!(
        expression_operations[0].dependencies.signal_reads,
        vec![SignalId(2)]
    );
    assert!(expression_operations[0].dependencies.uses_browser_read);
    assert_eq!(
        expression_operations[1].consumer,
        LegacyBrowserConsumer::Operation(0)
    );
    assert_eq!(
        expression_operations[1].dependencies,
        ClientBlockExternalDependencies::default()
    );

    let handler = ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
        ClientBrowserEffect::StorageSetItem {
            area: StorageArea::Local,
            key: ClientExpr::SignalRead(SignalId(2)),
            value: nested_read,
        },
    )]);
    let handler_operations = collect_handler_browser_operations(&handler).unwrap();
    assert_eq!(handler_operations.len(), 2);
    assert_eq!(
        handler_operations[0].kind,
        LegacyBrowserOperationKind::Write
    );
    assert_eq!(
        handler_operations[0].consumer,
        LegacyBrowserConsumer::Enclosing
    );
    assert_eq!(
        handler_operations[0].dependencies.signal_reads,
        vec![SignalId(2)]
    );
    assert!(handler_operations[0].dependencies.uses_browser_read);
    assert_eq!(handler_operations[1].kind, LegacyBrowserOperationKind::Read);
    assert_eq!(
        handler_operations[1].consumer,
        LegacyBrowserConsumer::Operation(0)
    );
}

#[test]
fn handler_local_current_target_reads_are_dependency_neutral() {
    let target = ClientLocalId(0);
    let value = ClientLocalId(1);
    let handler = ClientBlock::from_stmts(vec![
        ClientStmt::Let {
            local: target,
            name: "input".to_owned(),
            ty: ClientType::DomElement,
            value: Some(ClientExpr::EventCurrentTarget),
        },
        ClientStmt::Let {
            local: value,
            name: "value".to_owned(),
            ty: ClientType::String,
            value: Some(ClientExpr::DomRead {
                target: Box::new(ClientExpr::Local(target)),
                accessor: NodeRefAccessor::Value,
            }),
        },
    ]);

    assert!(collect_handler_browser_operations(&handler)
        .expect("typed current-target reads should survive browser-operation traversal")
        .is_empty());

    let mut client_operations = Vec::new();
    collect_client_block_operations(&handler, &BTreeMap::new(), &mut client_operations)
        .expect("typed current-target reads should survive client-operation traversal");
    assert!(client_operations.is_empty());
}

#[test]
fn normalizes_full_query_and_joins_server_initializer() {
    let mut descriptor = base_descriptor();
    add_query(&mut descriptor, 11);
    descriptor.get_mut("exprs").unwrap().as_array_mut().unwrap()[0]["expr"] =
        expression_json(&ClientExpr::QueryRead {
            id: QueryId(11),
            accessor: QueryAccessor::Data,
        });

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert_eq!(graph.queries.len(), 1);
    let query = &graph.queries[0];
    assert_eq!(query.id, 11);
    assert_eq!(query.source_name.as_deref(), Some("user_query"));
    assert_eq!(
        query.source_anchor,
        Some(SourceAnchor {
            start_line: 41,
            start_column: 9,
        })
    );
    assert_eq!(query.key_expression, 4);
    assert_eq!(query.server_initial_expression, 5);
    assert_eq!(query.client_fetcher_expression, 6);
    assert_eq!(query.data_type, ClientType::String);
    assert_eq!(
        query.options,
        LegacyQueryOptions {
            interval_ms: Some(1500),
            refetch_on_focus: true,
            download_progress: true,
            stream_text: false,
            retry_count: 3,
            stale_time_ms: 2500,
        }
    );
    assert_eq!(
        query.initializer,
        Some(LegacyQueryInitializer {
            expression: "server_user . clone ()".to_owned(),
            key_expression: Some("rsjs :: QueryKey :: new ((\"user\", user_id))".to_owned()),
        })
    );
    assert_eq!(
        graph.expressions[0].client_operations,
        vec![LegacyClientOperation {
            kind: LegacyClientOperationKind::QueryRead {
                query: 11,
                accessor: QueryAccessor::Data,
            },
            consumer: LegacyClientOperationConsumer::Enclosing,
            dependencies: ClientBlockExternalDependencies {
                queries: vec![QueryId(11)],
                ..ClientBlockExternalDependencies::default()
            },
        }]
    );
    assert_eq!(graph.reachable_queries, vec![11]);
    assert_eq!(graph.reachable_expressions, vec![3, 4, 5, 6]);
    assert!(graph.unsupported_features.is_empty());
}

#[test]
fn dead_query_declarations_and_unbound_handler_operations_stay_inert() {
    let mut descriptor = base_descriptor();
    add_query(&mut descriptor, 11);
    descriptor
        .get_mut("handlers")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 8,
            "body_ir_version": CLIENT_IR_SCHEMA,
            "body_ir": block_json(&ClientBlock::from_stmts(vec![
                ClientStmt::QueryRefetch { query: QueryId(11) }
            ]))
        }));
    descriptor["counts"]["handlers"] = json!(2);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert!(graph.reachable_queries.is_empty());
    assert!(
        graph.unsupported_features.is_empty(),
        "dead query metadata is valid producer material, not a compatibility gap"
    );
}

#[test]
fn client_operations_distinguish_query_and_shared_state_effects_in_preorder() {
    let mut descriptor = base_descriptor();
    add_query(&mut descriptor, 11);
    add_shared_field(&mut descriptor, 0);
    let handler = ClientBlock::from_stmts(vec![
        ClientStmt::QueryRefetch { query: QueryId(11) },
        ClientStmt::QueryInvalidate { query: QueryId(11) },
        ClientStmt::SignalSet {
            id: SignalId(2),
            value: ClientExpr::QueryRead {
                id: QueryId(11),
                accessor: QueryAccessor::Status,
            },
        },
        ClientStmt::SignalUpdate {
            id: SignalId(2),
            closure: ClientSimpleClosure::new("current", ClientExpr::SignalRead(SignalId(2))),
        },
    ]);
    descriptor
        .get_mut("handlers")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]["body_ir"] = block_json(&handler);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert_eq!(
        graph.shared_stores,
        vec![LegacySharedStore {
            id: 4,
            source_name: "workbench".to_owned(),
            rust_type: "crate :: WorkbenchState".to_owned(),
            source_anchor: Some(SourceAnchor {
                start_line: 17,
                start_column: 13,
            }),
        }]
    );
    assert!(graph.signals[0].shared);
    assert_eq!(
        graph.signals[0].shared_field,
        Some(LegacySharedFieldRef {
            store: 4,
            field: "selected_user".to_owned(),
        })
    );

    let operations = &graph.handlers[0].client_operations;
    assert_eq!(operations.len(), 6);
    assert_eq!(
        operations
            .iter()
            .map(|operation| operation.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            LegacyClientOperationKind::QueryRefetch { query: 11 },
            LegacyClientOperationKind::QueryInvalidate { query: 11 },
            LegacyClientOperationKind::SharedSet {
                signal: 2,
                field: LegacySharedFieldRef {
                    store: 4,
                    field: "selected_user".to_owned(),
                },
            },
            LegacyClientOperationKind::QueryRead {
                query: 11,
                accessor: QueryAccessor::Status,
            },
            LegacyClientOperationKind::SharedUpdate {
                signal: 2,
                field: LegacySharedFieldRef {
                    store: 4,
                    field: "selected_user".to_owned(),
                },
            },
            LegacyClientOperationKind::SharedRead {
                signal: 2,
                field: LegacySharedFieldRef {
                    store: 4,
                    field: "selected_user".to_owned(),
                },
            },
        ]
    );
    assert_eq!(
        operations[3].consumer,
        LegacyClientOperationConsumer::Operation(2)
    );
    assert_eq!(
        operations[5].consumer,
        LegacyClientOperationConsumer::Operation(4)
    );
    assert_eq!(operations[2].dependencies.signal_writes, vec![SignalId(2)]);
    assert_eq!(operations[2].dependencies.queries, vec![QueryId(11)]);
    assert_eq!(operations[4].dependencies.signal_reads, vec![SignalId(2)]);
    assert_eq!(operations[4].dependencies.signal_writes, vec![SignalId(2)]);
    assert_eq!(
        graph.unsupported_features,
        vec!["shared_backed_signals", "unsupported_shapes"]
    );
}

#[test]
fn rejects_duplicate_or_dangling_query_initializer_ids() {
    let mut descriptor = base_descriptor();
    add_query(&mut descriptor, 11);
    descriptor.insert(
        "query_initializers".to_owned(),
        json!([
            {"id": 11, "expr": "first", "key_expr": null},
            {"id": 11, "expr": "second", "key_expr": null}
        ]),
    );
    let duplicate = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        duplicate.contains("strictly increasing unique IDs"),
        "{duplicate}"
    );

    descriptor.insert(
        "query_initializers".to_owned(),
        json!([{"id": 12, "expr": "missing", "key_expr": null}]),
    );
    let dangling = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(dangling.contains("undeclared ID 12"), "{dangling}");
}

#[test]
fn rejects_dangling_query_expression_and_shared_field_store_ids() {
    let mut query_descriptor = base_descriptor();
    add_query(&mut query_descriptor, 11);
    query_descriptor
        .get_mut("queries")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]["key_expr"] = json!(99);
    let query_error = parse_legacy_client_graph(&query_descriptor, &BTreeSet::new()).unwrap_err();
    assert!(query_error.contains("query 11 key_expr"), "{query_error}");
    assert!(query_error.contains("undeclared ID 99"), "{query_error}");

    let mut shared_descriptor = base_descriptor();
    shared_descriptor
        .get_mut("signals")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]
        .as_object_mut()
        .unwrap()
        .insert(
            "shared".to_owned(),
            json!({"store": 99, "field": "selected_user"}),
        );
    let shared_error = parse_legacy_client_graph(&shared_descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        shared_error.contains("signal 2 shared store"),
        "{shared_error}"
    );
    assert!(shared_error.contains("undeclared ID 99"), "{shared_error}");
}

#[test]
fn rejects_duplicate_shared_store_ids() {
    let mut descriptor = base_descriptor();
    descriptor.insert(
        "shared_stores".to_owned(),
        json!([
            {"id": 4, "source_name": "first", "rust_type": "First"},
            {"id": 4, "source_name": "second", "rust_type": "Second"}
        ]),
    );
    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("shared_stores"), "{error}");
    assert!(error.contains("strictly increasing unique IDs"), "{error}");
}

#[test]
fn validates_query_occurrences_even_in_unbound_handlers() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("handlers")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 8,
            "body_ir_version": CLIENT_IR_SCHEMA,
            "body_ir": block_json(&ClientBlock::from_stmts(vec![
                ClientStmt::QueryRefetch { query: QueryId(99) }
            ]))
        }));
    descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("handlers".to_owned(), json!(2));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("handler 8 query"), "{error}");
    assert!(error.contains("undeclared ID 99"), "{error}");
}

#[test]
fn rejects_noncanonical_client_ir_json() {
    let mut descriptor = base_descriptor();
    descriptor.get_mut("exprs").unwrap().as_array_mut().unwrap()[0]
        .get_mut("expr")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("ignored".to_owned(), Value::Bool(true));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("not canonical Client IR v3 JSON"), "{error}");
}

#[test]
fn validates_helper_edges_against_relocation_authority() {
    let mut descriptor = base_descriptor();
    let helper_call = ClientExpr::call(
        ClientFunctionRef::Helper {
            edge: HelperCallEdgeId::new(4),
            authored_path: "crate::label".to_owned(),
            name: "label".to_owned(),
        },
        Vec::new(),
    );
    descriptor.get_mut("exprs").unwrap().as_array_mut().unwrap()[0]
        .as_object_mut()
        .unwrap()
        .insert("expr".to_owned(), expression_json(&helper_call));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("helper ordinal"), "{error}");
    assert!(error.contains("undeclared ID 4"), "{error}");

    let helpers = BTreeSet::from([4]);
    parse_legacy_client_graph(&descriptor, &helpers).unwrap();
}

#[test]
fn returns_unsupported_features_in_contract_order() {
    let mut descriptor = base_descriptor();
    let import = ClientExpr::call(
        ClientFunctionRef::Import {
            module: "labels".to_owned(),
            name: "label".to_owned(),
        },
        Vec::new(),
    );
    descriptor.get_mut("exprs").unwrap().as_array_mut().unwrap()[0]
        .as_object_mut()
        .unwrap()
        .insert("expr".to_owned(), expression_json(&import));
    descriptor
        .get_mut("bindings")
        .unwrap()
        .get_mut("refs")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({"node": 0, "ref": 12}));
    descriptor
        .get_mut("node_refs")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({"id": 12}));
    descriptor
        .get_mut("shared_stores")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({"id": 4, "source_name": "state", "rust_type": "State"}));
    descriptor
        .get_mut("signals")
        .unwrap()
        .as_array_mut()
        .unwrap()[1]
        .as_object_mut()
        .unwrap()
        .insert("shared".to_owned(), json!({"store": 4, "field": "value"}));
    let counts = descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap();
    counts.insert("node_refs".to_owned(), json!(1));

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert_eq!(
        graph.unsupported_features,
        vec!["shared_backed_signals", "client_imports",]
    );
}

#[test]
fn retains_source_ordered_conditionals_as_dense_controls() {
    let mut descriptor = base_descriptor();
    descriptor["view_tree"]["nodes"][0]["node"]["children"] = json!([1, 2]);
    descriptor["view_tree"]["nodes"][1]["node"] = json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "span"
    });
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 2,
            "node": {
                "children": [],
                "kind": "element",
                "payload_attrs": [],
                "server_attrs": [],
                "static_attrs": [],
                "tag": "em"
            }
        }));
    descriptor["bindings"]["conditionals"] = json!([
        {"parent": 0, "condition": 3, "then": [1], "else": []},
        {"parent": 0, "condition": 3, "then": [], "else": [2]}
    ]);
    descriptor["counts"]["nodes"] = json!(3);

    let graph = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap();
    assert_eq!(
        graph.bindings.conditionals,
        vec![
            LegacyConditionalBinding {
                control: 0,
                parent: 0,
                condition: 3,
                then_children: vec![1],
                else_children: vec![],
            },
            LegacyConditionalBinding {
                control: 1,
                parent: 0,
                condition: 3,
                then_children: vec![],
                else_children: vec![2],
            },
        ]
    );
    assert!(!graph
        .unsupported_features
        .iter()
        .any(|feature| feature == "conditionals"));
}

#[test]
fn rejects_conditional_children_outside_their_direct_parent() {
    let mut descriptor = base_descriptor();
    descriptor["view_tree"]["nodes"][0]["node"]["children"] = json!([1]);
    descriptor["view_tree"]["nodes"][1]["node"] = json!({
        "children": [2],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "span"
    });
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 2,
            "node": {
                "children": [],
                "kind": "element",
                "payload_attrs": [],
                "server_attrs": [],
                "static_attrs": [],
                "tag": "em"
            }
        }));
    descriptor["bindings"]["conditionals"] =
        json!([{"parent": 0, "condition": 3, "then": [2], "else": []}]);
    descriptor["counts"]["nodes"] = json!(3);

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("is not a direct child"), "{error}");
}

#[test]
fn rejects_binding_node_out_of_bounds() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("bindings")
        .unwrap()
        .get_mut("events")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]
        .as_object_mut()
        .unwrap()
        .insert("node".to_owned(), json!(99));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("view_tree has 2 nodes"), "{error}");
}

#[test]
fn rejects_view_tree_self_cycle() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("view_tree")
        .unwrap()
        .get_mut("nodes")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]["node"]["children"] = json!([0]);

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("self-cycle"), "{error}");
}

#[test]
fn rejects_duplicate_view_tree_child_reference() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("view_tree")
        .unwrap()
        .get_mut("nodes")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]["node"]["children"] = json!([1, 1]);

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("child 1 more than once"), "{error}");
}

#[test]
fn rejects_multiple_parent_edge_from_for_loop_children() {
    let mut descriptor = base_descriptor();
    let nodes = descriptor
        .get_mut("view_tree")
        .unwrap()
        .get_mut("nodes")
        .unwrap()
        .as_array_mut()
        .unwrap();
    nodes[0]["node"]["children"] = json!([1, 2]);
    nodes.push(json!({
        "id": 2,
        "node": {
            "attrs": "",
            "body_prefix": "",
            "children": [1],
            "expr": "items",
            "kind": "for_loop",
            "label": "items",
            "pat": "item"
        }
    }));
    descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("nodes".to_owned(), json!(3));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("multiple parents 0 and 2"), "{error}");
}

#[test]
fn rejects_unreachable_view_tree_node() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("view_tree")
        .unwrap()
        .get_mut("nodes")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 2,
            "node": {"kind": "static_text", "text": "detached"}
        }));
    descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("nodes".to_owned(), json!(3));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        error.contains("node 2 is unreachable from view_tree.root 0"),
        "{error}"
    );
}

#[test]
fn rejects_root_that_is_owned_by_another_view_node() {
    let mut descriptor = base_descriptor();
    descriptor
        .get_mut("view_tree")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("root".to_owned(), json!(1));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(
        error.contains("view_tree.root 1 is also a child of view node 0"),
        "{error}"
    );
}

#[test]
fn rejects_disconnected_view_tree_cycle_before_reachability() {
    let mut descriptor = base_descriptor();
    let nodes = descriptor
        .get_mut("view_tree")
        .unwrap()
        .get_mut("nodes")
        .unwrap()
        .as_array_mut()
        .unwrap();
    nodes.push(json!({
        "id": 2,
        "node": {
            "children": [3],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "aside"
        }
    }));
    nodes.push(json!({
        "id": 3,
        "node": {
            "children": [2],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "section"
        }
    }));
    descriptor
        .get_mut("counts")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("nodes".to_owned(), json!(4));

    let error = parse_legacy_client_graph(&descriptor, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("contains a cycle"), "{error}");
}
