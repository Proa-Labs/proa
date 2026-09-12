// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use rsjs_ir::abi::{CargoUnitId, LinkUnitId, ProducerVersion};
use rsjs_ir::abi::{DescriptorBody, SpanCardinality};
use rsjs_ir::build::LinkPlanMaterial;
use rsjs_ir::ClientStmt;
use serde_json::json;

fn fixture_component_fingerprint(unit: CargoUnitId) -> String {
    format!("sha256:{}", hex_record(unit.as_bytes()))
}

fn definition(byte: u8) -> TransportDefinitionId {
    let unit = CargoUnitId::from_bytes([byte; 32]);
    let fingerprint = fixture_component_fingerprint(unit);
    TransportDefinitionId {
        unit,
        record: component_record_id(&fingerprint),
    }
}

fn semantic(byte: u8) -> SemanticHash {
    SemanticHash::from_bytes([byte; 32])
}

fn hash_text(byte: u8) -> String {
    format!("sha256:{}", hex_record(&[byte; 32]))
}

fn linked_identity(item: &str) -> LinkedSemanticIdentity {
    LinkedSemanticIdentity {
        package: "acme-web".to_string(),
        crate_name: "acme_web".to_string(),
        module_path: vec!["resolved".to_string(), "ui".to_string()],
        item: item.to_string(),
    }
}

fn context(component: Option<TransportDefinitionId>) -> AbiAdapterContext {
    let context = AbiAdapterContext::v1(ProducerVersion::new(0, 1, 0), AbiVersion::new(1, 0));
    match component {
        Some(definition) => context.with_component_identity(
            definition,
            ResolvedSemanticPath::new("acme-web", "acme_web", ["resolved", "ui"], "StaticPanel"),
        ),
        None => context,
    }
}

fn static_descriptor(root_node: Value) -> Value {
    let nodes = if root_node.is_null() {
        Vec::new()
    } else {
        vec![json!({"id": 0, "node": root_node})]
    };
    json!({
        "asset_id": "token_derived_asset",
        "bindings": {
            "attrs": [],
            "conditionals": [],
            "events": [],
            "inputs": [],
            "refs": [],
            "text": []
        },
        "callback_slots": [],
        "client_ir_version": CLIENT_IR_SCHEMA,
        "client_origin_permission": "participant_only",
        "counts": {
            "callback_slots": 0,
            "crossings": 0,
            "exprs": 0,
            "fields": 0,
            "handlers": 0,
            "node_ref_fields": 0,
            "node_refs": 0,
            "nodes": nodes.len(),
            "projection_fields": 0,
            "prop_sites": 0,
            "shared_state_provider_sites": 0,
            "signals": 0,
            "values": 0
        },
        "descriptor_hash": hash_text(0x41),
        "exprs": [],
        "fields": [],
        "format": COMPONENT_DESCRIPTOR_FORMAT,
        "handlers": [],
        "inbound_inputs": {
            "crossings": [],
            "node_ref_fields": [],
            "projection_fields": [],
            "prop_sites": [],
            "values": []
        },
        "is_async": false,
        "js_hash": hash_text(0x42),
        "mutations": [],
        "node_refs": [],
        "queries": [],
        "schema_version": COMPONENT_DESCRIPTOR_SCHEMA,
        "self_ty": "MisleadingAlias",
        "shared_stores": [],
        "shared_state_provider_sites": [],
        "signals": [],
        "struct_name": "MisleadingAlias",
        "view_tree": {
            "root": if nodes.is_empty() { Value::Null } else { Value::from(0) },
            "nodes": nodes
        }
    })
}

fn client_value_descriptor(field_names: &[&str]) -> Value {
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["fields"] = Value::Array(
        field_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                json!({
                    "crossing_id": index,
                    "id": index,
                    "kind": "prop",
                    "name": name,
                    "source": "signal_initializer",
                    "source_anchor": {
                        "start_column": 20 + index,
                        "start_line": 7
                    },
                    "value_id": index
                })
            })
            .collect(),
    );
    descriptor["inbound_inputs"] = json!({
        "crossings": field_names.iter().enumerate().map(|(index, _)| {
            json!({
                "field_id": index,
                "id": index,
                "kind": "value",
                "source_anchor": {
                    "start_column": 20 + index,
                    "start_line": 7
                }
            })
        }).collect::<Vec<_>>(),
        "node_ref_fields": [],
        "projection_fields": [],
        "prop_sites": field_names.iter().enumerate().map(|(index, _)| {
            json!({
                "crossing_id": index,
                "field_id": index,
                "id": index,
                "source_anchor": {
                    "start_column": 20 + index,
                    "start_line": 7
                },
                "target": {
                    "kind": "signal_seed",
                    "signal_id": index
                },
                "temporal": {
                    "kind": "seed_once",
                    "site_id": index
                },
                "value_id": index
            })
        }).collect::<Vec<_>>(),
        "values": field_names.iter().enumerate().map(|(index, _)| {
            json!({
                "field_id": index,
                "id": index,
                "source_anchor": {
                    "start_column": 20 + index,
                    "start_line": 7
                }
            })
        }).collect::<Vec<_>>()
    });
    descriptor["signals"] = Value::Array(
        field_names
            .iter()
            .enumerate()
            .map(|(index, _)| {
                json!({
                    "id": index,
                    "init": {"kind": "int", "value": 0},
                    "ty": {"kind": "i32"}
                })
            })
            .collect(),
    );
    for field in ["crossings", "fields", "prop_sites", "signals", "values"] {
        descriptor["counts"][field] = json!(field_names.len());
    }
    descriptor
}

fn callback_field_descriptor(field_name: &str, callback_shape: &str) -> Value {
    assert!(matches!(callback_shape, "required" | "optional"));
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "button"
    }));
    descriptor["callback_slots"] = json!([{
        "id": 0,
        "name": field_name
    }]);
    descriptor["fields"] = json!([{
        "callback_shape": callback_shape,
        "callback_slot": 0,
        "crossing_id": 0,
        "dispatch": "resolve_current",
        "id": 0,
        "kind": "handler",
        "name": field_name,
        "source": "callback_slot_usage",
        "source_anchor": {"start_column": 17, "start_line": 9},
        "value_id": 0
    }]);
    descriptor["inbound_inputs"] = json!({
        "crossings": [{
            "field_id": 0,
            "id": 0,
            "kind": "callback_resolve_current",
            "source_anchor": {"start_column": 17, "start_line": 9}
        }],
        "node_ref_fields": [],
        "projection_fields": [],
        "prop_sites": [],
        "values": [{
            "field_id": 0,
            "id": 0,
            "source_anchor": {"start_column": 17, "start_line": 9}
        }]
    });
    descriptor["counts"]["callback_slots"] = json!(1);
    descriptor["counts"]["crossings"] = json!(1);
    descriptor["counts"]["fields"] = json!(1);
    descriptor["counts"]["values"] = json!(1);
    descriptor
}

fn attach_callback_field_witness(
    linked: &mut LinkedTransportComponent,
    field_name: &str,
    callback_shape: &str,
) {
    let callback_shape_record = match callback_shape {
        "required" => TransportCallbackShape::Required,
        "optional" => TransportCallbackShape::Optional,
        other => panic!("unsupported callback shape `{other}`"),
    };
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint");
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Callback,
        callback_shape_record,
    );
    linked.payload["transport"]["component_field_witnesses"] = json!([{
        "callback_shape": callback_shape,
        "crossing_hook_id": 0,
        "field_id": 0,
        "field_name": field_name,
        "field_record_id": hex_record(&field_record),
        "role": "callback"
    }]);
    linked
        .field_witnesses
        .push(crate::rsjs::link::LinkedComponentFieldWitness {
            field_record,
            target: linked.definition,
        });
}

fn bound_node_ref_field_descriptor() -> Value {
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "input"
    }));
    descriptor["fields"] = json!([{
        "crossing_id": 0,
        "id": 0,
        "kind": "node_ref",
        "name": "target",
        "source": "node_ref_handle_alias",
        "source_anchor": {
            "start_column": 17,
            "start_line": 9
        },
        "value_id": 0
    }]);
    descriptor["inbound_inputs"] = json!({
        "crossings": [{
            "field_id": 0,
            "id": 0,
            "kind": "node_ref_alias",
            "source_anchor": {
                "start_column": 17,
                "start_line": 9
            }
        }],
        "node_ref_fields": [{
            "bind_target": "input",
            "crossing_id": 0,
            "field_id": 0,
            "id": 0,
            "node_ref_id": 0,
            "source_anchor": {
                "start_column": 17,
                "start_line": 9
            },
            "value_id": 0
        }],
        "projection_fields": [],
        "prop_sites": [],
        "values": [{
            "field_id": 0,
            "id": 0,
            "source_anchor": {
                "start_column": 17,
                "start_line": 9
            }
        }]
    });
    descriptor["node_refs"] = json!([{"id": 0}]);
    descriptor["bindings"]["refs"] = json!([{"node": 0, "ref": 0}]);
    for field in [
        "crossings",
        "fields",
        "node_ref_fields",
        "node_refs",
        "values",
    ] {
        descriptor["counts"][field] = json!(1);
    }
    descriptor
}

fn provider_seed_descriptor(field_name: &str) -> Value {
    let mut descriptor = client_value_descriptor(&[field_name]);
    descriptor["fields"][0]["source"] = json!("shared_state_provider_seed");
    descriptor["signals"] = json!([]);
    descriptor["counts"]["signals"] = json!(0);
    descriptor["inbound_inputs"]["prop_sites"][0]["target"] =
        json!({"kind": "shared_state_provider_seed"});
    descriptor["inbound_inputs"]["prop_sites"][0]["temporal"] =
        json!({"kind": "forbidden_provider_seed"});
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "inferred_opaque_ssr",
                    "tokens": "rsjs::SharedStateProvider { state: self.label, children: WorkbenchChild { label: \"static\" } }"
                }
            }
        ]
    });
    descriptor["counts"]["nodes"] = json!(2);
    descriptor
}

fn attach_value_field_witnesses(
    linked: &mut LinkedTransportComponent,
    field_names: &[&str],
    field_records: &[TransportRecordId],
) {
    assert_eq!(field_names.len(), field_records.len());
    linked.payload["transport"]["component_field_witnesses"] = Value::Array(
        field_names
            .iter()
            .zip(field_records)
            .enumerate()
            .map(|(index, (name, field_record))| {
                json!({
                    "callback_shape": "not_callback",
                    "crossing_hook_id": index,
                    "field_id": index,
                    "field_name": name,
                    "field_record_id": hex_record(field_record),
                    "role": "value"
                })
            })
            .collect(),
    );
    linked.field_witnesses = field_records
        .iter()
        .map(
            |field_record| crate::rsjs::link::LinkedComponentFieldWitness {
                field_record: *field_record,
                target: linked.definition,
            },
        )
        .collect();
}

fn attach_shared_state_acquisitions(
    linked: &mut LinkedTransportComponent,
    targets: &[TransportDefinitionId],
) {
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    linked.payload["transport"]["shared_state_acquisitions"] = Value::Array(
        targets
            .iter()
            .enumerate()
            .map(|(store_id, _)| {
                let store_id = u32::try_from(store_id).expect("fixture store ID");
                json!({
                    "source_edge_id": hex_record(
                        &shared_state_acquisition_source_edge_id(&fingerprint, store_id)
                    ),
                    "store_id": store_id,
                })
            })
            .collect(),
    );
    linked.shared_state_acquisitions = targets
        .iter()
        .enumerate()
        .map(|(store_id, target)| {
            let store_id = u32::try_from(store_id).expect("fixture store ID");
            crate::rsjs::link::LinkedComponentSharedStateAcquisition {
                store_id,
                source_edge: shared_state_acquisition_source_edge_id(&fingerprint, store_id),
                target: *target,
            }
        })
        .collect();
}

fn attach_shared_state_provider(
    linked: &mut LinkedTransportComponent,
    state_target: TransportDefinitionId,
    state_rust_type: &str,
) {
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    let child = linked.payload["transport"]["direct_component_edges"][0]
        .as_object()
        .expect("provider fixture direct child");
    let child_edge_local_ordinal = child["local_ordinal"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .expect("provider fixture child ordinal");
    let child_source_edge = parse_hex_32(
        child["source_edge_id"]
            .as_str()
            .expect("provider fixture child source edge"),
        "component",
        linked.definition,
        "provider fixture child source edge",
    )
    .expect("provider fixture child source edge");
    let site_node = child["site_node"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .expect("provider fixture server node");
    let child_target = linked.component_edges[0].target;
    let source_edge = shared_state_provider_source_edge_id(
        &fingerprint,
        0,
        &TransportSharedStateProviderSite {
            site_node,
            child_edge_ordinal: child_edge_local_ordinal,
            child_source_edge_id: child_source_edge,
        },
    );
    linked.payload["component_descriptor"]["shared_state_provider_sites"] = json!([{
        "child_edge": child_edge_local_ordinal,
        "id": 0,
        "site_node": site_node,
        "source_anchor": {
            "start_column": 17,
            "start_line": 31
        },
        "state_rust_type": state_rust_type,
        "topology": "static_single_wrapperless"
    }]);
    linked.payload["component_descriptor"]["counts"]["shared_state_provider_sites"] = json!(1);
    linked.payload["transport"]["shared_state_provider_sites"] = json!([{
        "child_edge_local_ordinal": child_edge_local_ordinal,
        "child_source_edge_id": hex_record(&child_source_edge),
        "identity_authority": "rustc_shared_state_witness_relocation",
        "provider_site_id": 0,
        "site_node": site_node,
        "source_anchor": {
            "column": 17,
            "line": 31
        },
        "source_edge_id": hex_record(&source_edge),
        "state_rust_type": state_rust_type,
        "topology": "static_single_wrapperless"
    }]);
    linked.shared_state_provider_sites =
        vec![crate::rsjs::link::LinkedComponentSharedStateProviderSite {
            provider_site_id: 0,
            source_edge,
            target: state_target,
            child_edge_local_ordinal,
            child_source_edge,
            child_target,
            site_node,
        }];
}

fn linked_shared_state_definition(
    definition: TransportDefinitionId,
    type_key: &str,
    runtime_abi_version: u32,
    fields: &[(&str, rsjs_transport::SharedStateSchemaCodec)],
) -> LinkedSharedStateDefinition {
    let fields = fields
        .iter()
        .enumerate()
        .map(
            |(id, (name, codec))| rsjs_transport::SharedStateSchemaField {
                id: u32::try_from(id).expect("fixture field ID"),
                name: (*name).to_owned(),
                equality: rsjs_transport::SharedStateSchemaEquality::ObjectIs,
                codec: codec.clone(),
            },
        )
        .collect::<Vec<_>>();
    let schema_abi =
        rsjs_transport::encode_resolved_shared_state_schema_abi(runtime_abi_version, &fields)
            .unwrap();
    linked_shared_state_definition_from_abi(definition, type_key, schema_abi)
}

fn linked_shared_state_definition_from_abi(
    definition: TransportDefinitionId,
    type_key: &str,
    schema_abi: Vec<u8>,
) -> LinkedSharedStateDefinition {
    let schema_payload = rsjs_transport::encode_shared_state_schema_payload(type_key, &schema_abi);
    LinkedSharedStateDefinition {
        definition,
        schema_payload_hash: rsjs_transport::semantic_payload_hash(
            rsjs_transport::PayloadKind::SharedStateDefinition,
            &schema_payload,
        ),
        type_key: type_key.to_owned(),
        schema_abi_hash: rsjs_transport::shared_state_schema_abi_hash(&schema_abi),
        schema_abi,
    }
}

fn context_with_shared_state(
    component: TransportDefinitionId,
    definitions: &[LinkedSharedStateDefinition],
) -> AbiAdapterContext {
    let mut context = context(Some(component));
    for definition in definitions {
        context.insert_shared_state_definition(definition).unwrap();
    }
    context
}

fn active_shared_state_descriptor(field: &str, rust_type: &str) -> Value {
    use rsjs_ir::{ClientBlock, ClientExpr, ClientStmt, SignalId};

    let mut descriptor = static_descriptor(json!({
        "children": [1, 2],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["shared_stores"] = json!([{
        "id": 0,
        "rust_type": rust_type,
        "source_name": "workbench"
    }]);
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "string", "value": "light"},
        "shared": {"field": field, "store": 0},
        "source_name": "mode",
        "ty": {"kind": "string"}
    }]);
    let read = ClientExpr::SignalRead(SignalId(0));
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": serde_json::from_str::<Value>(&client_expr_json(&read)).unwrap()
    }]);
    let write = ClientBlock::from_stmts(vec![ClientStmt::SignalSet {
        id: SignalId(0),
        value: ClientExpr::string("dark"),
    }]);
    descriptor["handlers"] = json!([{
        "body_ir": serde_json::from_str::<Value>(&client_block_json(&write)).unwrap(),
        "body_ir_version": CLIENT_IR_SCHEMA,
        "id": 0
    }]);
    descriptor["bindings"]["text"] = json!([{"expr": 0, "node": 1}]);
    descriptor["bindings"]["events"] = json!([{"event": "click", "handler": 0, "node": 2}]);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1, 2],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {"id": 1, "node": {"kind": "text_binding_slot"}},
            {
                "id": 2,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "button"
                }
            }
        ]
    });
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["handlers"] = json!(1);
    descriptor["counts"]["nodes"] = json!(3);
    descriptor
}

fn component_payload(
    definition: TransportDefinitionId,
    descriptor: Value,
    hash_byte: u8,
) -> LinkedTransportComponent {
    let component_input_fingerprint = fixture_component_fingerprint(definition.unit);
    let component_abi = rsjs_transport::ComponentDefinitionAbi::new(
        if descriptor["is_async"].as_bool().unwrap_or(false) {
            TransportComponentRenderKind::Async
        } else {
            TransportComponentRenderKind::Sync
        },
        match descriptor["client_origin_permission"].as_str() {
            Some("origin_capable") => TransportComponentOriginPermission::OriginCapable,
            _ => TransportComponentOriginPermission::ParticipantOnly,
        },
    );
    LinkedTransportComponent {
        definition,
        identity: linked_identity("StaticPanel"),
        semantic_hash: semantic(hash_byte),
        component_abi,
        component_edges: Vec::new(),
        helper_edges: Vec::new(),
        field_witnesses: Vec::new(),
        shared_state_acquisitions: Vec::new(),
        shared_state_provider_sites: Vec::new(),
        payload: json!({
            "component_descriptor": descriptor,
            "format": COMPONENT_PAYLOAD_FORMAT,
            "schema_version": COMPONENT_PAYLOAD_SCHEMA,
            "semantic_hash": hash_text(hash_byte),
            "transport": {
                "client_value_sources": {
                    "payload_handlers": []
                },
                "component_field_witnesses": [],
                "component_input_fingerprint": component_input_fingerprint,
                "definition_record_id": hex_record(&definition.record),
                "direct_component_edges": [],
                "hydration_constraint": {
                    "kind": "unconstrained"
                },
                "reachable_helper_edges": [],
                "shared_state_acquisitions": [],
                "shared_state_provider_sites": []
            }
        }),
    }
}

fn set_projection_test_field(
    component: &mut LinkedTransportComponent,
    name: &str,
    kind: &str,
    callback_shape: Option<&str>,
) {
    let mut field = json!({
        "id": 0,
        "name": name,
        "kind": kind,
    });
    if let Some(callback_shape) = callback_shape {
        field["callback_shape"] = json!(callback_shape);
    }
    component.payload["component_descriptor"]["fields"] = json!([field]);
}

fn add_projection_test_edge(
    parent: &mut LinkedTransportComponent,
    target: TransportDefinitionId,
    source_byte: u8,
    child_field_name: &str,
) {
    let source_edge = [source_byte; 32];
    parent.component_edges.push(LinkedComponentEdge {
        source_edge,
        target,
    });
    parent.payload["transport"]["direct_component_edges"]
        .as_array_mut()
        .expect("fixture direct edge array")
        .push(json!({
            "source_edge_id": hex_record(&source_edge),
            "fields": [{
                "name": child_field_name,
                "binding": {
                    "kind": "deferred_projection",
                    "parent_field_id": 0,
                },
            }],
        }));
}

fn projection_test_plan(components: Vec<LinkedTransportComponent>) -> TransportLinkPlan {
    let material = LinkPlanMaterial {
        link_unit: LinkUnitId::derive("deferred-projection-role-test"),
        component_descriptors: components
            .iter()
            .map(|component| component.semantic_hash)
            .collect(),
        helper_descriptors: Vec::new(),
        payload: SemanticHash::derive("deferred-projection-role-test-payload"),
    };
    TransportLinkPlan {
        link_unit: material.link_unit,
        plan_id: material.plan_id(),
        material,
        components,
        helpers: Vec::new(),
        shared_state_definitions: Vec::new(),
        shared_state_acquisitions: Vec::new(),
    }
}

fn attach_direct_component_edge(
    linked: &mut LinkedTransportComponent,
    target: TransportDefinitionId,
    boundary: &str,
    lifecycle: &str,
    has_rest: bool,
    binding_kind: &str,
    binding_proof: &str,
) {
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    let descriptor = &mut linked.payload["component_descriptor"];
    let source = if boundary == "composed" {
        "direct_component_site"
    } else {
        "explicit_island_boundary"
    };
    let existing_site = descriptor["view_tree"]["nodes"]
        .as_array()
        .expect("fixture view nodes")
        .iter()
        .find(|record| {
            record["node"]["kind"] == "server_only" && record["node"]["source"] == source
        })
        .and_then(|record| record["id"].as_u64())
        .map(|id| id as usize);
    let site_id = existing_site.unwrap_or_else(|| {
        let root = descriptor["view_tree"]["root"]
            .as_u64()
            .expect("direct-edge fixtures need an element root") as usize;
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        let site_id = nodes.len();
        nodes[root]["node"]["children"]
            .as_array_mut()
            .expect("direct-edge fixture root must be an element")
            .push(json!(site_id));
        nodes.push(json!({
            "id": site_id,
            "node": {
                "kind": "server_only",
                "source": source,
                "tokens": "ImportedBadge { label: \"fixture\" }"
            }
        }));
        descriptor["counts"]["nodes"] = json!(nodes.len());
        site_id
    });
    let committed_lifecycle = match lifecycle {
        "static_single" => TransportDirectComponentLifecycle::StaticSingle,
        "conditional_single" => {
            let predicate_expression = descriptor["bindings"]["conditionals"][0]["condition"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .expect("fixture conditional predicate expression");
            TransportDirectComponentLifecycle::ConditionalSingle {
                control: 0,
                predicate_expression,
                predicate_graph: component_lifecycle_predicate_graph_commitment(
                    descriptor
                        .as_object()
                        .expect("fixture descriptor is an object"),
                    0,
                    predicate_expression,
                ),
                branch: TransportDirectComponentBranch::Then,
            }
        }
        "repeated" => TransportDirectComponentLifecycle::Repeated,
        "unknown" => TransportDirectComponentLifecycle::UnknownNestedConditionals,
        other => panic!("unsupported fixture lifecycle `{other}`"),
    };
    let lifecycle = match lifecycle {
        "static_single" => json!({"kind": "static_single"}),
        "conditional_single" => {
            json!({
                "kind": "conditional_single",
                "control": 0,
                "predicate_expression": descriptor["bindings"]["conditionals"][0]["condition"],
                "branch": "then"
            })
        }
        "repeated" => json!({"kind": "repeated"}),
        "unknown" => json!({"kind": "unknown", "reason": "nested_conditionals"}),
        other => json!({"kind": other}),
    };
    let committed_binding = match binding_kind {
        "server_only" => TransportDirectComponentBinding::ServerOnly,
        "immutable_static_slot_exact_one" => TransportDirectComponentBinding::StaticSlotExactOne,
        "node_ref_handle" => TransportDirectComponentBinding::NodeRefHandle { node_ref: 0 },
        "unsupported" => TransportDirectComponentBinding::Unsupported,
        other => panic!("unsupported fixture binding kind `{other}`"),
    };
    let committed_fields = [TransportDirectComponentField {
        name: "label",
        binding: committed_binding,
    }];
    let source_edge = component_source_edge_id(
        &fingerprint,
        0,
        &TransportDirectComponentEdge {
            boundary: if boundary == "composed" {
                TransportDirectComponentBoundary::Composed
            } else {
                TransportDirectComponentBoundary::ExplicitIsland
            },
            site_node: site_id as u32,
            lifecycle: committed_lifecycle,
            has_rest,
            fields: &committed_fields,
        },
    );
    linked.component_edges.push(LinkedComponentEdge {
        source_edge,
        target,
    });
    let binding = if binding_kind == "node_ref_handle" {
        json!({
            "handle_kind": "node_ref",
            "kind": binding_kind,
            "parent_source": {
                "id": 0,
                "namespace": "legacy_node_ref"
            },
            "proof": binding_proof
        })
    } else {
        json!({
            "kind": binding_kind,
            "proof": binding_proof
        })
    };
    linked.payload["transport"]["direct_component_edges"] = json!([{
        "authored_path": "ImportedBadge",
        "boundary": boundary,
        "fields": [{
            "binding": binding,
            "field_ordinal": 0,
            "name": "label"
        }],
        "has_rest": has_rest,
        "identity_authority": "rustc_component_witness_relocation",
        "lifecycle": lifecycle,
        "local_ordinal": 0,
        "site_node": site_id,
        "source_anchor": {
            "column": 17,
            "line": 31
        },
        "source_edge_id": hex_record(&source_edge)
    }]);
}

fn recommit_direct_component_edge(linked: &mut LinkedTransportComponent, index: usize) {
    let edge = linked.payload["transport"]["direct_component_edges"][index]
        .as_object()
        .expect("fixture direct component edge")
        .clone();
    let local_ordinal = edge["local_ordinal"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .expect("fixture direct component ordinal");
    let boundary = match edge["boundary"].as_str().expect("fixture edge boundary") {
        "composed" => TransportDirectComponentBoundary::Composed,
        "explicit_island" => TransportDirectComponentBoundary::ExplicitIsland,
        other => panic!("unsupported fixture edge boundary `{other}`"),
    };
    let site_node = edge["site_node"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .expect("fixture direct component site");
    let lifecycle = validate_direct_component_lifecycle(
        linked.definition,
        edge["lifecycle"]
            .as_object()
            .expect("fixture direct component lifecycle"),
        linked.payload["component_descriptor"]
            .as_object()
            .expect("fixture component descriptor"),
    )
    .expect("fixture direct component lifecycle must be valid")
    .transport();
    let fields = edge["fields"]
        .as_array()
        .expect("fixture direct component fields")
        .iter()
        .map(|field| {
            let field = field.as_object().expect("fixture direct component field");
            let name = field["name"]
                .as_str()
                .expect("fixture direct component field name");
            let (_, binding) = validate_direct_component_binding_with_projection_roles(
                linked.definition,
                field["binding"]
                    .as_object()
                    .expect("fixture direct component binding"),
                &ResolvedProjectionRoles::default(),
            )
            .expect("fixture direct component binding must be valid");
            (name.to_owned(), binding)
        })
        .collect::<Vec<_>>();
    let committed_fields = fields
        .iter()
        .map(|(name, binding)| TransportDirectComponentField {
            name,
            binding: *binding,
        })
        .collect::<Vec<_>>();
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint");
    let source_edge = component_source_edge_id(
        fingerprint,
        local_ordinal,
        &TransportDirectComponentEdge {
            boundary,
            site_node,
            lifecycle,
            has_rest: edge["has_rest"].as_bool().expect("fixture has_rest"),
            fields: &committed_fields,
        },
    );
    linked.payload["transport"]["direct_component_edges"][index]["source_edge_id"] =
        json!(hex_record(&source_edge));
    linked.component_edges[index].source_edge = source_edge;
}

fn place_direct_site_in_conditional(
    linked: &mut LinkedTransportComponent,
    branch: ComponentLifecycleBranch,
) {
    let site = linked.payload["transport"]["direct_component_edges"][0]["site_node"]
        .as_u64()
        .expect("fixture direct site") as usize;
    let site_wrapper;
    let sibling;
    let node_count;
    {
        let descriptor = &mut linked.payload["component_descriptor"];
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        site_wrapper = nodes.len();
        sibling = site_wrapper + 1;
        nodes.extend([
            json!({
                "id": site_wrapper,
                "node": {
                    "children": [site],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "section"
                }
            }),
            json!({
                "id": sibling,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "aside"
                }
            }),
        ]);
        nodes[0]["node"]["children"] = match branch {
            ComponentLifecycleBranch::Then => json!([site_wrapper, sibling]),
            ComponentLifecycleBranch::Else => json!([sibling, site_wrapper]),
        };
        node_count = nodes.len();
    }
    let (then_children, else_children) = match branch {
        ComponentLifecycleBranch::Then => (json!([site_wrapper]), json!([sibling])),
        ComponentLifecycleBranch::Else => (json!([sibling]), json!([site_wrapper])),
    };
    let descriptor = &mut linked.payload["component_descriptor"];
    linked.component_abi.origin_permission = TransportComponentOriginPermission::OriginCapable;
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "bool", "value": true},
        "ty": {"kind": "bool"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["bindings"]["conditionals"] = json!([{
        "parent": 0,
        "condition": 0,
        "then": then_children,
        "else": else_children,
    }]);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["nodes"] = json!(node_count);
    linked.payload["transport"]["direct_component_edges"][0]["lifecycle"] = json!({
        "kind": "conditional_single",
        "control": 0,
        "predicate_expression": 0,
        "branch": component_lifecycle_branch_name(branch),
    });
    recommit_direct_component_edge(linked, 0);
}

fn place_direct_site_in_loop(linked: &mut LinkedTransportComponent) {
    let site = linked.payload["transport"]["direct_component_edges"][0]["site_node"]
        .as_u64()
        .expect("fixture direct site");
    let descriptor = &mut linked.payload["component_descriptor"];
    descriptor["view_tree"]["nodes"][0]["node"] = json!({
        "attrs": "",
        "body_prefix": "",
        "children": [site],
        "expr": "items",
        "kind": "for_loop",
        "label": "",
        "pat": "item"
    });
    linked.payload["transport"]["direct_component_edges"][0]["lifecycle"] =
        json!({"kind": "repeated"});
    recommit_direct_component_edge(linked, 0);
}

fn place_direct_site_in_nested_conditionals(linked: &mut LinkedTransportComponent) {
    let site = linked.payload["transport"]["direct_component_edges"][0]["site_node"]
        .as_u64()
        .expect("fixture direct site") as usize;
    let inner_parent;
    let outer_else;
    let inner_else;
    let site_wrapper;
    let node_count;
    {
        let descriptor = &mut linked.payload["component_descriptor"];
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        inner_parent = nodes.len();
        outer_else = inner_parent + 1;
        inner_else = inner_parent + 2;
        site_wrapper = inner_parent + 3;
        nodes.extend([
            json!({
                "id": inner_parent,
                "node": {
                    "children": [site_wrapper, inner_else],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "section"
                }
            }),
            json!({
                "id": outer_else,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "aside"
                }
            }),
            json!({
                "id": inner_else,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "p"
                }
            }),
            json!({
                "id": site_wrapper,
                "node": {
                    "children": [site],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "article"
                }
            }),
        ]);
        nodes[0]["node"]["children"] = json!([inner_parent, outer_else]);
        node_count = nodes.len();
    }
    let descriptor = &mut linked.payload["component_descriptor"];
    linked.component_abi.origin_permission = TransportComponentOriginPermission::OriginCapable;
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([
        {
            "id": 0,
            "init": {"kind": "bool", "value": true},
            "ty": {"kind": "bool"}
        },
        {
            "id": 1,
            "init": {"kind": "bool", "value": true},
            "ty": {"kind": "bool"}
        }
    ]);
    descriptor["exprs"] = json!([
        {"id": 0, "expr": {"kind": "signal_read", "signal": 0}},
        {"id": 1, "expr": {"kind": "signal_read", "signal": 1}}
    ]);
    descriptor["bindings"]["conditionals"] = json!([
        {
            "parent": 0,
            "condition": 0,
            "then": [inner_parent],
            "else": [outer_else]
        },
        {
            "parent": inner_parent,
            "condition": 1,
            "then": [site_wrapper],
            "else": [inner_else]
        }
    ]);
    descriptor["counts"]["signals"] = json!(2);
    descriptor["counts"]["exprs"] = json!(2);
    descriptor["counts"]["nodes"] = json!(node_count);
    linked.payload["transport"]["direct_component_edges"][0]["lifecycle"] = json!({
        "kind": "unknown",
        "reason": "nested_conditionals"
    });
    recommit_direct_component_edge(linked, 0);
}

fn attach_component_helper_edges(
    linked: &mut LinkedTransportComponent,
    edges: &[(u32, TransportDefinitionId)],
) {
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    linked.payload["transport"]["reachable_helper_edges"] = Value::Array(
        edges
            .iter()
            .map(|(local_ordinal, _)| {
                let source_edge = component_helper_source_edge_id(&fingerprint, *local_ordinal);
                json!({
                    "authored_path": format!("diagnostic_only_{local_ordinal}"),
                    "identity_authority": "rustc_function_relocation",
                    "local_ordinal": local_ordinal,
                    "source_edge_id": hex_record(&source_edge)
                })
            })
            .collect(),
    );
    linked.helper_edges = edges
        .iter()
        .map(|(local_ordinal, target)| LinkedHelperEdge {
            source_edge: component_helper_source_edge_id(&fingerprint, *local_ordinal),
            target: *target,
        })
        .collect();
    linked.helper_edges.sort_by_key(|edge| edge.source_edge);
}

fn helper_payload(definition: TransportDefinitionId, hash_byte: u8) -> LinkedTransportHelper {
    LinkedTransportHelper {
        definition,
        identity: linked_identity("pluralize"),
        semantic_hash: semantic(hash_byte),
        helper_edges: Vec::new(),
        payload: json!({
            "body_hir": {"kind": "binary"},
            "body_hir_version": 1,
            "body_ir": {
                "kind": "literal",
                "value": {"kind": "int", "value": 1}
            },
            "body_ir_version": CLIENT_IR_SCHEMA,
            "codec_abi_version": 1,
            "defining_record_id": hex_record(&definition.record),
            "direct_calls": [],
            "effect_class": "origin_free",
            "format": HELPER_PAYLOAD_FORMAT,
            "provisional_identity": {
                "crate": "do_not_trust",
                "item": "do_not_trust",
                "key_schema_version": 1,
                "module_identity": "rustc_defining_relocation",
                "package": "do-not-trust"
            },
            "schema_version": 1,
            "semantic_hash": hash_text(hash_byte),
            "signature": {
                "params": [{"name": "count", "rust_type": "u32"}],
                "return_type": "u32"
            }
        }),
    }
}

#[test]
fn static_component_becomes_a_valid_canonical_envelope() {
    let definition = definition(0x11);
    let linked = component_payload(
        definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x21,
    );

    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result.canonical().expect("static subset should convert");
    canonical
        .envelope
        .validate(&AbiSupport::v1(AbiVersion::new(1, 0)))
        .unwrap();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.render_kind, RenderKind::WebRenderSync);
    assert_eq!(body.origin_permission, OriginPermission::ParticipantOnly);
    assert_eq!(body.span, SpanCardinality::Exact(1));
    assert!(body.root_eligible);
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(0)),
        Some(&ResolvedTransportWitness::Definition(definition))
    );
    let codegen = canonical
        .component_codegen()
        .expect("components preserve producer-local codegen coordinates");
    assert_eq!(codegen.asset_id, "token_derived_asset");
    assert_eq!(codegen.fragment_key, "token_derived_asset");
    assert_eq!(
        codegen.fragment_root,
        ComponentFragmentRoot::ExactOneElement
    );
    assert!(codegen.inbound_fields.is_empty());
    assert!(codegen.signals.is_empty());
}

#[test]
fn rustc_component_abi_is_authoritative_over_legacy_descriptor_claims() {
    let definition = definition(0x12);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["is_async"] = json!(true);
    descriptor["client_origin_permission"] = json!("origin_capable");
    let linked = component_payload(definition, descriptor, 0x22);

    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("authenticated async origin-capable component should convert");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.render_kind, RenderKind::WebRender);
    assert_eq!(body.origin_permission, OriginPermission::OriginCapable);

    let mut mismatched_render = linked.clone();
    mismatched_render.component_abi.render_kind = TransportComponentRenderKind::Sync;
    let render_error = adapt_component(&context(Some(definition)), &mismatched_render)
        .expect_err("legacy async claim may not override the defining Rust witness");
    assert!(matches!(
        render_error,
        AbiAdapterError::InvalidPayload {
            kind: "component",
            ..
        }
    ));
    assert!(render_error.to_string().contains("rustc-authenticated"));

    let mut mismatched_origin = linked;
    mismatched_origin.component_abi.origin_permission =
        TransportComponentOriginPermission::ParticipantOnly;
    let origin_error = adapt_component(&context(Some(definition)), &mismatched_origin)
        .expect_err("legacy origin-capable claim may not override the defining Rust witness");
    assert!(matches!(
        origin_error,
        AbiAdapterError::InvalidPayload {
            kind: "component",
            ..
        }
    ));
    assert!(origin_error.to_string().contains("rustc-authenticated"));
}

#[test]
fn direct_callback_and_live_source_metadata_fail_closed() {
    let definition = definition(0x70);

    let static_optional = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    assert_eq!(
        validate_direct_component_binding(
            definition,
            static_optional.as_object().expect("binding object")
        )
        .unwrap(),
        ValidatedDirectComponentBinding::StaticCallback {
            shape: ComponentCallbackShape::Optional,
            form: TransportDirectStaticCallbackForm::None,
        }
    );

    let live_value = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 4,
            "namespace": "legacy_signal"
        },
        "proof": "client_alias_resolves_to_exact_signal_read_v1",
        "source_form": "client_alias"
    });
    assert_eq!(
        validate_direct_component_binding(
            definition,
            live_value.as_object().expect("binding object")
        )
        .unwrap(),
        ValidatedDirectComponentBinding::LiveValue {
            signal: 4,
            source: TransportDirectLiveValueSource::ClientAlias,
        }
    );

    let inbound_live_value = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 9,
            "namespace": "legacy_signal"
        },
        "proof": "inbound_live_field_reuses_exact_live_signal_v1",
        "source_form": "inbound_field"
    });
    assert_eq!(
        validate_direct_component_binding(
            definition,
            inbound_live_value.as_object().expect("binding object")
        )
        .unwrap(),
        ValidatedDirectComponentBinding::LiveValue {
            signal: 9,
            source: TransportDirectLiveValueSource::InboundField,
        }
    );

    let live_callback = json!({
        "callback_shape": "required",
        "kind": "live_callback",
        "parent_source": {
            "id": 7,
            "namespace": "payload_handler_prelude_binding"
        },
        "proof": "payload_handler_prelude_binding_v1"
    });
    assert_eq!(
        validate_direct_component_binding(
            definition,
            live_callback.as_object().expect("binding object")
        )
        .unwrap(),
        ValidatedDirectComponentBinding::LiveCallback {
            handler_prelude_local: 7,
            shape: ComponentCallbackShape::Required,
            form: TransportDirectLiveCallbackForm::Handler,
        }
    );

    let mut mismatched = static_optional;
    mismatched["callback_shape"] = json!("required");
    let error = validate_direct_component_binding(
        definition,
        mismatched.as_object().expect("binding object"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("incompatible with callback shape `required`"),
        "unexpected diagnostic: {error}"
    );

    let mut forged_source = live_value;
    forged_source["parent_source"]["namespace"] = json!("canonical_local_value");
    let error = validate_direct_component_binding(
        definition,
        forged_source.as_object().expect("binding object"),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("expected `legacy_signal`"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn component_value_layout_keeps_inbound_values_and_signals_disjoint() {
    let definition = definition(0x74);
    let layout = ComponentValueLayout::from_counts(definition, 2, 2, 0).unwrap();

    assert_eq!(layout.inbound(definition, 0).unwrap(), LocalValueId::new(0));
    assert_eq!(layout.inbound(definition, 1).unwrap(), LocalValueId::new(1));
    assert_eq!(layout.signal(definition, 0).unwrap(), LocalValueId::new(2));
    assert_eq!(layout.signal(definition, 1).unwrap(), LocalValueId::new(3));

    let mut values = BTreeSet::new();
    layout.populate(&mut values);
    assert_eq!(
        values,
        BTreeSet::from([
            LocalValueId::new(0),
            LocalValueId::new(1),
            LocalValueId::new(2),
            LocalValueId::new(3),
        ])
    );

    let error = ComponentValueLayout::from_counts(definition, u64::from(u32::MAX), 2, 0)
        .expect_err("the signal range must not wrap over the inbound range");
    assert!(
        error.to_string().contains("exhaust"),
        "overflow should produce a focused allocation diagnostic: {error}"
    );
}

#[test]
fn direct_and_aliased_reads_of_one_signal_share_one_canonical_value() {
    let definition = definition(0x75);
    let descriptor = client_value_descriptor(&["first", "second"]);
    let descriptor = descriptor.as_object().unwrap();
    let inbound = validate_inbound_inputs(definition, descriptor).unwrap();
    let layout = ComponentValueLayout::new(definition, &inbound.fields, descriptor, 0).unwrap();

    let direct = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    let alias = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "client_alias_resolves_to_exact_signal_read_v1",
        "source_form": "client_alias"
    });
    let direct_binding =
        validate_direct_component_binding(definition, direct.as_object().unwrap()).unwrap();
    let alias_binding =
        validate_direct_component_binding(definition, alias.as_object().unwrap()).unwrap();
    let fields = [
        ValidatedDirectComponentField {
            name: "direct".to_owned(),
            binding: direct_binding,
            transport_binding: direct_binding.transport(),
        },
        ValidatedDirectComponentField {
            name: "alias".to_owned(),
            binding: alias_binding,
            transport_binding: alias_binding.transport(),
        },
    ];

    let bindings = adapt_direct_component_bindings(
        definition,
        &fields,
        &layout,
        &BTreeMap::new(),
        OriginId::new(0),
    )
    .unwrap();
    let expected = ComponentBindingSource::LiveValue {
        value: LocalValueId::new(2),
    };
    assert_eq!(bindings["direct"].source, expected);
    assert_eq!(bindings["alias"].source, expected);
}

#[test]
fn live_value_edges_use_the_sparse_parent_signal_allocation() {
    let parent = definition(0x79);
    let child = definition(0x7a);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"}
    }]);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["counts"]["signals"] = json!(1);
    let mut linked = component_payload(parent, descriptor, 0x7b);
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    recommit_direct_component_edge(&mut linked, 0);

    let adapted = adapt_component(&context(Some(parent)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("a live parent signal and direct edge should lower canonically");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert!(body.values.contains(&LocalValueId::new(0)));
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
        ComponentBindingSource::LiveValue {
            value: LocalValueId::new(0)
        }
    );
}

#[test]
fn direct_live_value_roots_fail_closed_on_node_payload_derived_signals() {
    let parent = definition(0x7c);
    let child = definition(0x7d);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"},
        "derived": 0
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": serde_json::from_str::<Value>(&client_expr_json(
            &ClientExpr::NodePayload {
                attr: "data-rsjs-payload-p0".to_owned(),
                tokens: "item.label".to_owned(),
            }
        ))
        .unwrap()
    }]);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    let mut linked = component_payload(parent, descriptor, 0x7e);
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    recommit_direct_component_edge(&mut linked, 0);

    let result = adapt_component(&context(Some(parent)), &linked).unwrap();
    let AbiBridgeResult::Unsupported(report) = result else {
        panic!("a node-local payload cannot seed a cross-component signal export");
    };
    assert_eq!(
        report.gaps,
        vec![CompatibilityGap::ComponentClientGraphMetadataMissing {
            features: vec!["unsupported_shapes".to_owned()],
        }]
    );
}

#[test]
fn component_value_layout_remaps_sparse_signal_sources_and_rejects_missing_ones() {
    let definition = definition(0x76);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["signals"] = json!([{"id": 0}, {"id": 2}]);
    let layout = ComponentValueLayout::new(definition, &[], descriptor.as_object().unwrap(), 0)
        .expect("sparse optimized signal IDs must receive dense canonical values");
    assert_eq!(layout.signal(definition, 0).unwrap(), LocalValueId::new(0));
    assert_eq!(layout.signal(definition, 2).unwrap(), LocalValueId::new(1));
    validate_legacy_signal_source(definition, descriptor.as_object().unwrap(), 2)
        .expect("transport lookup must resolve a sparse surviving signal by ID");

    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 0},
        "ty": {"kind": "i32"}
    }]);
    let layout =
        ComponentValueLayout::new(definition, &[], descriptor.as_object().unwrap(), 0).unwrap();
    let error = layout
        .signal(definition, 1)
        .expect_err("an unallocated signal source must fail closed");
    assert!(
        error
            .to_string()
            .contains("not present in the optimized descriptor"),
        "unexpected missing-source diagnostic: {error}"
    );
    let error = validate_legacy_signal_source(definition, descriptor.as_object().unwrap(), 1)
        .expect_err("transport validation must reject the same missing source");
    assert!(
        error.to_string().contains("unknown legacy signal 1"),
        "unexpected transport-source diagnostic: {error}"
    );
}

#[test]
fn component_value_layout_keeps_sparse_expressions_and_payload_callbacks_disjoint() {
    let definition = definition(0x78);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["signals"] = json!([{"id": 3}]);
    descriptor["exprs"] = json!([
        {"id": 2, "expr": {"kind": "signal_read", "signal": 3}},
        {"id": 9, "expr": {"kind": "signal_read", "signal": 3}}
    ]);

    let layout = ComponentValueLayout::new(definition, &[], descriptor.as_object().unwrap(), 1)
        .expect("sparse legacy values should receive dense disjoint allocations");
    assert_eq!(layout.signal(definition, 3).unwrap(), LocalValueId::new(0));
    assert_eq!(layout.expr(definition, 2).unwrap(), LocalValueId::new(1));
    assert_eq!(layout.expr(definition, 9).unwrap(), LocalValueId::new(2));
    assert_eq!(
        layout.payload_handler(definition, 0).unwrap(),
        LocalValueId::new(3)
    );

    let mut values = BTreeSet::new();
    layout.populate(&mut values);
    assert_eq!(
        values,
        BTreeSet::from([
            LocalValueId::new(0),
            LocalValueId::new(1),
            LocalValueId::new(2),
            LocalValueId::new(3),
        ])
    );
}

#[test]
fn inbound_values_crossings_and_temporal_props_form_a_valid_canonical_body() {
    let definition = definition(0x77);
    let mut descriptor = client_value_descriptor(&["value"]);
    descriptor["signals"] = json!([]);
    descriptor["counts"]["signals"] = json!(0);
    descriptor["inbound_inputs"]["prop_sites"][0]["target"] =
        json!({"kind": "explicit_island_boundary"});
    descriptor["inbound_inputs"]["prop_sites"][0]["temporal"] =
        json!({"kind": "forbidden_boundary_crossing"});
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "explicit_island_boundary",
                    "tokens": "rsjs::island(IndependentCounter { initial: self.value })"
                }
            }
        ]
    });
    descriptor["counts"]["nodes"] = json!(2);
    let expected_client_ir_hash = canonical_legacy_json_hash(
        COMPONENT_CLIENT_IR_HASH_DOMAIN,
        &legacy_component_client_program_material(descriptor.as_object().unwrap(), &[]),
    );

    let mut linked = component_payload(definition, descriptor, 0x78);
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Value,
        TransportCallbackShape::NotCallback,
    );
    attach_value_field_witnesses(&mut linked, &["value"], &[field_record]);

    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("forbidden-boundary input metadata is losslessly canonical");
    canonical
        .envelope
        .validate(&AbiSupport::v1(AbiVersion::new(1, 0)))
        .unwrap();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };

    assert_eq!(body.values, BTreeSet::from([LocalValueId::new(0)]));
    assert_eq!(
        body.crossing_hooks,
        BTreeSet::from([CrossingHookId::new(0)])
    );
    assert_eq!(
        body.prop_uses[&PropSiteId::new(0)],
        PropUse {
            field: ComponentFieldId::new(0),
            value: LocalValueId::new(0),
            temporal: TemporalPropMode::ForbiddenBoundaryCrossing,
            origin: OriginId::new(0),
        }
    );
    assert_eq!(
        body.origins,
        BTreeMap::from([(OriginId::new(0), OriginKind::SourceOnly)])
    );
    assert_eq!(
        body.span,
        SpanCardinality::Exact(1),
        "a latent forbidden-boundary guard must not turn static parent markup into a client path"
    );
    assert_eq!(body.client_ir_hash, expected_client_ir_hash);
    assert!(canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::TEMPORAL_PROPS));
    assert!(canonical
        .envelope
        .header
        .runtime
        .required_capabilities
        .contains(CapabilitySet::TEMPORAL_PROPS));
}

#[test]
fn inbound_node_ref_handle_keeps_exact_field_identity_and_terminal_binding() {
    let definition = definition(0x79);
    let mut linked = component_payload(definition, bound_node_ref_field_descriptor(), 0x7a);
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::NodeRef,
        TransportCallbackShape::NotCallback,
    );
    linked.payload["transport"]["component_field_witnesses"] = json!([{
        "callback_shape": "not_callback",
        "crossing_hook_id": 0,
        "field_id": 0,
        "field_name": "target",
        "field_record_id": hex_record(&field_record),
        "role": "node_ref"
    }]);
    linked
        .field_witnesses
        .push(crate::rsjs::link::LinkedComponentFieldWitness {
            field_record,
            target: definition,
        });

    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("an exact inbound NodeRef field is losslessly canonical");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(
        body.fields[&ComponentFieldId::new(0)].role,
        ComponentFieldRole::ClientHandle
    );
    assert_eq!(
        body.fields[&ComponentFieldId::new(0)].client_value,
        Some(LocalValueId::new(0))
    );
    assert_eq!(
        canonical.component_codegen().unwrap().inbound_node_refs,
        vec![ComponentCodegenInboundNodeRef {
            field_id: ComponentFieldId::new(0),
            producer_node_ref_id: 0,
            bind_target: Some(ComponentNodeRefBindTarget::Input),
        }]
    );

    let mut no_terminal = linked.clone();
    no_terminal.payload["component_descriptor"]["bindings"]["refs"] = json!([]);
    let error = adapt_component(&context(Some(definition)), &no_terminal).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must have exactly one terminal local binding or one static direct alias"),
        "unexpected error: {error}"
    );

    let mut forged_identity = linked;
    forged_identity.payload["component_descriptor"]["inbound_inputs"]["node_ref_fields"][0]
        ["node_ref_id"] = json!(9);
    let error = adapt_component(&context(Some(definition)), &forged_identity).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refers to unknown producer NodeRef 9"),
        "unexpected error: {error}"
    );
}

#[test]
fn provider_seed_temporal_record_is_exact_and_latent() {
    let definition = definition(0x7b);
    let descriptor = provider_seed_descriptor("label");
    let inbound = validate_inbound_inputs(definition, descriptor.as_object().unwrap()).unwrap();

    assert_eq!(inbound.prop_uses.len(), 1);
    assert_eq!(
        inbound.prop_uses[0].temporal,
        TemporalPropMode::ForbiddenProviderSeed
    );
    assert_eq!(inbound.prop_uses[0].target_signal, None);
    assert!(
        !inbound_temporal_mode_requires_client_program(inbound.prop_uses[0].temporal),
        "a provider declaration is a latent semantic-link guard, not a client owner input"
    );

    let mut extra_temporal_field = descriptor.clone();
    extra_temporal_field["inbound_inputs"]["prop_sites"][0]["temporal"]["site_id"] = json!(0);
    let error = validate_inbound_inputs(definition, extra_temporal_field.as_object().unwrap())
        .expect_err("provider temporal records must not carry signal-seed fields");
    assert!(
        error
            .to_string()
            .contains("forbidden-provider-seed temporal mode")
            && error.to_string().contains("site_id"),
        "unexpected temporal-shape diagnostic: {error}"
    );

    let mut wrong_target = descriptor.clone();
    wrong_target["inbound_inputs"]["prop_sites"][0]["target"] =
        json!({"kind": "explicit_island_boundary"});
    let error = validate_inbound_inputs(definition, wrong_target.as_object().unwrap())
        .expect_err("provider temporal records require the exact provider-seed target");
    assert!(
        error.to_string().contains("shared_state_provider_seed"),
        "unexpected temporal-target diagnostic: {error}"
    );

    let mut extra_target_field = descriptor;
    extra_target_field["inbound_inputs"]["prop_sites"][0]["target"]["signal_id"] = json!(0);
    let error = validate_inbound_inputs(definition, extra_target_field.as_object().unwrap())
        .expect_err("provider targets must not masquerade as live-signal targets");
    assert!(
        error
            .to_string()
            .contains("SharedStateProvider seed prop target")
            && error.to_string().contains("signal_id"),
        "unexpected provider-target shape diagnostic: {error}"
    );
}

#[test]
fn provider_seed_stays_inert_for_static_edges_and_rejects_live_edges_at_link() {
    let parent_definition = definition(0x7c);
    let child_definition = definition(0x7d);
    let adapter_context =
        AbiAdapterContext::v1(ProducerVersion::new(0, 1, 0), AbiVersion::new(1, 0))
            .with_component_identity(
                parent_definition,
                ResolvedSemanticPath::new(
                    "acme-web",
                    "acme_web",
                    ["resolved", "ui"],
                    "WorkbenchPage",
                ),
            )
            .with_component_identity(
                child_definition,
                ResolvedSemanticPath::new(
                    "acme-web",
                    "acme_web",
                    ["resolved", "ui"],
                    "ProviderShell",
                ),
            );

    let mut child_linked =
        component_payload(child_definition, provider_seed_descriptor("label"), 0x7e);
    let fingerprint = child_linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Value,
        TransportCallbackShape::NotCallback,
    );
    attach_value_field_witnesses(&mut child_linked, &["label"], &[field_record]);
    let child_adapted = adapt_component(&adapter_context, &child_linked).unwrap();
    let child_canonical = child_adapted
        .canonical()
        .expect("a provider guard is losslessly represented in the canonical descriptor");
    let DescriptorBody::Component(child_body) = &child_canonical.envelope.body else {
        panic!("expected child component descriptor");
    };
    assert!(child_body.effects.is_empty());
    assert_eq!(
        child_body.prop_uses[&PropSiteId::new(0)].temporal,
        TemporalPropMode::ForbiddenProviderSeed
    );
    assert_eq!(
        child_body.origins,
        BTreeMap::from([(OriginId::new(0), OriginKind::SourceOnly)])
    );

    let parent_root = || {
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        }))
    };
    let mut static_parent_linked = component_payload(parent_definition, parent_root(), 0x7f);
    attach_direct_component_edge(
        &mut static_parent_linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    let static_parent_adapted = adapt_component(&adapter_context, &static_parent_linked).unwrap();
    let static_parent_canonical = static_parent_adapted
        .canonical()
        .expect("a static provider seed binding must remain canonical");
    let DescriptorBody::Component(static_parent_body) = &static_parent_canonical.envelope.body
    else {
        panic!("expected static parent component descriptor");
    };
    let static_graph = rsjs_ir::link::link_resolved_component_graph(
        &[
            rsjs_ir::link::ResolvedComponentInstance {
                id: rsjs_ir::link::OwnerCandidateId::new(0),
                descriptor: static_parent_body,
                has_browser_obligation: false,
            },
            rsjs_ir::link::ResolvedComponentInstance {
                id: rsjs_ir::link::OwnerCandidateId::new(1),
                descriptor: child_body,
                has_browser_obligation: false,
            },
        ],
        &[rsjs_ir::link::ResolvedComponentEdge {
            parent: rsjs_ir::link::OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: rsjs_ir::link::OwnerCandidateId::new(1),
        }],
    )
    .expect("a static server value may initialize SharedStateProvider");
    assert!(
        static_graph.inputs.live_props.is_empty() && static_graph.inputs.live_callbacks.is_empty(),
        "a static provider declaration must remain inert at semantic link"
    );

    let mut live_parent_descriptor = parent_root();
    live_parent_descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "string", "value": "select"},
        "ty": {"kind": "string"}
    }]);
    live_parent_descriptor["counts"]["signals"] = json!(1);
    live_parent_descriptor["client_origin_permission"] = json!("origin_capable");
    let mut live_parent_linked = component_payload(parent_definition, live_parent_descriptor, 0x80);
    attach_direct_component_edge(
        &mut live_parent_linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    live_parent_linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    recommit_direct_component_edge(&mut live_parent_linked, 0);
    let live_parent_adapted = adapt_component(&adapter_context, &live_parent_linked).unwrap();
    let live_parent_canonical = live_parent_adapted
        .canonical()
        .expect("the live edge remains canonical until temporal semantic linking");
    let DescriptorBody::Component(live_parent_body) = &live_parent_canonical.envelope.body else {
        panic!("expected live parent component descriptor");
    };
    let error = rsjs_ir::link::link_resolved_component_graph(
        &[
            rsjs_ir::link::ResolvedComponentInstance {
                id: rsjs_ir::link::OwnerCandidateId::new(0),
                descriptor: live_parent_body,
                has_browser_obligation: true,
            },
            rsjs_ir::link::ResolvedComponentInstance {
                id: rsjs_ir::link::OwnerCandidateId::new(1),
                descriptor: child_body,
                has_browser_obligation: false,
            },
        ],
        &[rsjs_ir::link::ResolvedComponentEdge {
            parent: rsjs_ir::link::OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: rsjs_ir::link::OwnerCandidateId::new(1),
        }],
    )
    .expect_err("a live value must not silently become a provider initialization snapshot");
    assert!(matches!(
        &error,
        rsjs_ir::link::ComponentGraphLinkError::ForbiddenLiveProviderSeed {
            parent,
            edge,
            field_name,
            site,
        } if *parent == rsjs_ir::link::OwnerCandidateId::new(0)
            && *edge == ComponentEdgeId::new(0)
            && field_name == "label"
            && *site == PropSiteId::new(0)
    ));
    assert!(
        error
            .to_string()
            .contains("SharedStateProvider initialization")
            && error
                .to_string()
                .contains("make SharedState the source of truth"),
        "unexpected provider-seed semantic-link diagnostic: {error}"
    );
}

#[test]
fn static_callback_shape_survives_direct_edge_adaptation() {
    let parent = definition(0x71);
    let child = definition(0x72);
    let mut linked = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x73,
    );
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "some_rsjs_noop_handler_v1",
        "source_form": "some_noop_handler"
    });
    recommit_direct_component_edge(&mut linked, 0);

    let adapted = adapt_component(&context(Some(parent)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("static callback edge should adapt losslessly");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    let source = &body
        .direct_components
        .get(&ComponentEdgeId::new(0))
        .expect("direct component edge")
        .bindings["label"]
        .source;
    assert_eq!(
        source,
        &ComponentBindingSource::StaticCallback {
            shape: ComponentCallbackShape::Optional,
        }
    );
}

#[test]
fn literal_none_uses_the_rustc_resolved_child_field_role() {
    let parent = definition(0x74);
    let child = definition(0x75);
    let mut parent_component = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x76,
    );
    attach_direct_component_edge(
        &mut parent_component,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    parent_component.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    recommit_direct_component_edge(&mut parent_component, 0);

    let mut child_component = component_payload(
        child,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "span"
        })),
        0x77,
    );
    set_projection_test_field(&mut child_component, "label", "server_prop", None);
    child_component.payload["component_descriptor"]["fields"][0]["source"] =
        json!("component_field");
    child_component.payload["component_descriptor"]["counts"]["fields"] = json!(1);

    let plan = projection_test_plan(vec![parent_component, child_component]);
    let adapter_context = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let adapted = adapt_transport_plan(&adapter_context, &plan).unwrap();
    let parent = adapted.components[0]
        .canonical()
        .expect("literal None for a server prop should adapt canonically");
    let DescriptorBody::Component(body) = &parent.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
            body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
            ComponentBindingSource::ServerOnly,
            "the raw None commitment must become a server input only after the exact child field is resolved"
        );
}

#[test]
fn literal_none_is_a_static_snapshot_for_a_rustc_resolved_value_field() {
    let parent = definition(0x80);
    let child = definition(0x81);
    let mut parent_component = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x82,
    );
    attach_direct_component_edge(
        &mut parent_component,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    parent_component.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    recommit_direct_component_edge(&mut parent_component, 0);

    let mut child_descriptor = client_value_descriptor(&["label"]);
    child_descriptor["client_origin_permission"] = json!("origin_capable");
    let mut child_component = component_payload(child, child_descriptor, 0x83);
    let fingerprint = child_component.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Value,
        TransportCallbackShape::NotCallback,
    );
    attach_value_field_witnesses(&mut child_component, &["label"], &[field_record]);

    let plan = projection_test_plan(vec![parent_component, child_component]);
    let adapter_context = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let adapted = adapt_transport_plan(&adapter_context, &plan).unwrap();
    let canonical = adapted.components[0]
        .canonical()
        .expect("literal None should be a static occurrence of a value-capable field");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
        ComponentBindingSource::ServerOnly
    );
}

#[test]
fn literal_none_fails_closed_without_plan_wide_child_role_resolution() {
    let parent = definition(0x84);
    let child = definition(0x85);
    let mut parent_component = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x86,
    );
    attach_direct_component_edge(
        &mut parent_component,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    parent_component.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    recommit_direct_component_edge(&mut parent_component, 0);

    let error = adapt_component(&context(Some(parent)), &parent_component)
        .expect_err("the single-component adapter has no child field-role authority");
    let message = error.to_string();
    assert!(message.contains("exact field role"), "{message}");
    assert!(message.contains("complete transport plan"), "{message}");
    assert!(message.contains(&format!("{child:?}")), "{message}");
}

#[test]
fn literal_none_remains_a_static_callback_for_an_optional_callback_field() {
    let parent = definition(0x78);
    let child = definition(0x79);
    let mut parent_component = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x7a,
    );
    attach_direct_component_edge(
        &mut parent_component,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    parent_component.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    recommit_direct_component_edge(&mut parent_component, 0);

    let mut child_component =
        component_payload(child, callback_field_descriptor("label", "optional"), 0x7b);
    attach_callback_field_witness(&mut child_component, "label", "optional");
    let plan = projection_test_plan(vec![parent_component, child_component]);
    let adapter_context = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let adapted = adapt_transport_plan(&adapter_context, &plan).unwrap();
    let canonical = adapted.components[0]
        .canonical()
        .expect("literal None should satisfy an optional callback field");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
        ComponentBindingSource::StaticCallback {
            shape: ComponentCallbackShape::Optional,
        }
    );
}

#[test]
fn literal_none_rejects_a_rustc_resolved_required_callback_field() {
    let parent = definition(0x7c);
    let child = definition(0x7d);
    let mut parent_component = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x7e,
    );
    attach_direct_component_edge(
        &mut parent_component,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    parent_component.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "optional",
        "kind": "static_callback",
        "proof": "literal_none_callback_candidate_v1",
        "source_form": "none"
    });
    recommit_direct_component_edge(&mut parent_component, 0);

    let mut child_component =
        component_payload(child, callback_field_descriptor("label", "required"), 0x7f);
    attach_callback_field_witness(&mut child_component, "label", "required");
    let plan = projection_test_plan(vec![parent_component, child_component]);
    let adapter_context = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let error = adapt_transport_plan(&adapter_context, &plan)
        .expect_err("literal None cannot satisfy a required callback field");
    let message = error.to_string();
    assert!(message.contains("local ordinal 0"), "{message}");
    assert!(message.contains("field `label`"), "{message}");
    assert!(message.contains("required callback"), "{message}");
    assert!(
        message.contains(&format!("{child:?}")),
        "the diagnostic must identify the rustc-resolved child definition: {message}"
    );
}

#[test]
fn payload_callback_becomes_one_canonical_value_effect_and_live_edge() {
    let parent = definition(0x79);
    let child = definition(0x7a);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    let mut linked = component_payload(parent, descriptor, 0x7b);
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "required",
        "kind": "live_callback",
        "parent_source": {
            "id": 7,
            "namespace": "payload_handler_prelude_binding"
        },
        "proof": "payload_handler_prelude_binding_v1"
    });
    recommit_direct_component_edge(&mut linked, 0);
    let body: Value =
        serde_json::from_str(&client_block_json(&rsjs_ir::ClientBlock::new())).unwrap();
    linked.payload["transport"]["client_value_sources"]["payload_handlers"] = json!([{
        "body_ir": body,
        "body_ir_version": CLIENT_IR_SCHEMA,
        "dependencies": {
            "callback_slots": [],
            "client_module_calls": [],
            "helper_edges": [],
            "mutations": [],
            "node_refs": [],
            "queries": [],
            "signal_reads": [],
            "signal_writes": []
        },
        "kind": "payload_handler",
        "payload_arity": 0,
        "prelude_local_index": 7,
        "source_anchor": {
            "column": 19,
            "line": 12
        }
    }]);

    let adapted = adapt_component(&context(Some(parent)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("closed payload callback should be canonical");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(body.values, BTreeSet::from([LocalValueId::new(0)]));
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::EventHandler)])
    );
    assert_eq!(
        body.origins,
        BTreeMap::from([
            (OriginId::new(0), OriginKind::EventHandler),
            (OriginId::new(1), OriginKind::SourceOnly),
        ])
    );
    assert_eq!(body.dependencies.len(), 4);
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Component
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
            && edge.to == DependencyEndpoint::Component(ComponentEdgeId::new(0))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::OwnerCoupling
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
            && edge.to == DependencyEndpoint::Component(ComponentEdgeId::new(0))
    }));
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
        ComponentBindingSource::LiveCallback {
            value: LocalValueId::new(0),
            shape: ComponentCallbackShape::Required,
        }
    );
    assert_eq!(
        canonical
            .component_codegen()
            .expect("component codegen mapping")
            .payload_handlers,
        vec![ComponentCodegenPayloadHandler {
            prelude_local_index: 7,
            canonical_value: LocalValueId::new(0),
        }]
    );
}

#[test]
fn payload_callback_browser_operations_share_the_canonical_origin_sequence() {
    use rsjs_ir::{ClientBrowserEffect, ClientBrowserRead, LocationField, SignalId, StorageArea};

    let parent = definition(0x7f);
    let child = definition(0x80);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "string", "value": "last-path"},
        "source_name": "storage_key",
        "ty": {"kind": "string"}
    }]);
    let legacy_read = ClientExpr::BrowserRead(ClientBrowserRead::Location(LocationField::Search));
    descriptor["exprs"] = json!([{
        "id": 5,
        "expr": serde_json::from_str::<Value>(&client_expr_json(&legacy_read)).unwrap()
    }]);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);

    let mut linked = component_payload(parent, descriptor, 0x81);
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "required",
        "kind": "live_callback",
        "parent_source": {
            "id": 7,
            "namespace": "payload_handler_prelude_binding"
        },
        "proof": "payload_handler_prelude_binding_v1"
    });
    recommit_direct_component_edge(&mut linked, 0);
    let body = ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
        ClientBrowserEffect::StorageSetItem {
            area: StorageArea::Local,
            key: ClientExpr::SignalRead(SignalId(0)),
            value: ClientExpr::BrowserRead(ClientBrowserRead::Location(LocationField::Pathname)),
        },
    )]);
    linked.payload["transport"]["client_value_sources"]["payload_handlers"] = json!([{
        "body_ir": serde_json::from_str::<Value>(&client_block_json(&body)).unwrap(),
        "body_ir_version": CLIENT_IR_SCHEMA,
        "dependencies": {
            "callback_slots": [],
            "client_module_calls": [],
            "helper_edges": [],
            "mutations": [],
            "node_refs": [],
            "queries": [],
            "signal_reads": [0],
            "signal_writes": []
        },
        "kind": "payload_handler",
        "payload_arity": 0,
        "prelude_local_index": 7,
        "source_anchor": {
            "column": 19,
            "line": 12
        }
    }]);

    let adapted = adapt_component(&context(Some(parent)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("payload browser capabilities have an exact canonical lowering");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };

    assert_eq!(
        body.origins,
        BTreeMap::from([
            (OriginId::new(0), OriginKind::EventHandler),
            (OriginId::new(1), OriginKind::WritableSignal),
            (OriginId::new(2), OriginKind::SourceOnly),
            (OriginId::new(3), OriginKind::BrowserCapabilityWrite),
            (OriginId::new(4), OriginKind::BrowserCapabilityRead),
            (OriginId::new(5), OriginKind::BrowserCapabilityRead),
            (OriginId::new(6), OriginKind::SourceOnly),
        ])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([
            (LocalEffectId::new(0), EffectKind::EventHandler),
            (LocalEffectId::new(1), EffectKind::BrowserWrite),
            (LocalEffectId::new(2), EffectKind::BrowserRead),
            (LocalEffectId::new(3), EffectKind::BrowserRead),
        ])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(1))
            && edge.origin == OriginId::new(3)
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Capture
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(1))
            && edge.origin == OriginId::new(3)
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(2))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(1))
            && edge.origin == OriginId::new(4)
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(3))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(1))
            && edge.origin == OriginId::new(5)
    }));
}

#[test]
fn payload_callback_metadata_is_recomputed_and_fails_closed() {
    let parent = definition(0x7c);
    let child = definition(0x7d);
    let mut linked = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x7e,
    );
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "required",
        "kind": "live_callback",
        "parent_source": {
            "id": 3,
            "namespace": "payload_handler_prelude_binding"
        },
        "proof": "payload_handler_prelude_binding_v1"
    });
    recommit_direct_component_edge(&mut linked, 0);
    let body: Value =
        serde_json::from_str(&client_block_json(&rsjs_ir::ClientBlock::new())).unwrap();
    linked.payload["transport"]["client_value_sources"]["payload_handlers"] = json!([{
        "body_ir": body,
        "body_ir_version": CLIENT_IR_SCHEMA,
        "dependencies": {
            "callback_slots": [],
            "client_module_calls": [],
            "helper_edges": [],
            "mutations": [],
            "node_refs": [],
            "queries": [],
            "signal_reads": [0],
            "signal_writes": []
        },
        "kind": "payload_handler",
        "payload_arity": 0,
        "prelude_local_index": 3,
        "source_anchor": {
            "column": 9,
            "line": 5
        }
    }]);
    let error = adapt_component(&context(Some(parent)), &linked)
        .expect_err("forged dependency claims must fail");
    assert!(
        error
            .to_string()
            .contains("claims signal_reads [0], but its Client IR computes []"),
        "unexpected diagnostic: {error}"
    );

    linked.payload["transport"]["client_value_sources"]["payload_handlers"][0]["dependencies"]
        ["signal_reads"] = json!([]);
    linked.payload["transport"]["client_value_sources"]["payload_handlers"][0]["body_ir"]
        ["unknown_semantic_field"] = json!(true);
    let error = adapt_component(&context(Some(parent)), &linked)
        .expect_err("unknown Client IR fields must fail");
    assert!(
        error
            .to_string()
            .contains("not canonical Client IR v3 JSON"),
        "unexpected diagnostic: {error}"
    );

    let event_body = rsjs_ir::ClientBlock::from_stmts(vec![rsjs_ir::ClientStmt::Expr(
        rsjs_ir::ClientExpr::EventPayload {
            attr: "value".to_owned(),
        },
    )]);
    linked.payload["transport"]["client_value_sources"]["payload_handlers"][0]["body_ir"] =
        serde_json::from_str(&client_block_json(&event_body)).unwrap();
    for payload_arity in [0, 1] {
        linked.payload["transport"]["client_value_sources"]["payload_handlers"][0]
            ["payload_arity"] = json!(payload_arity);
        let error = adapt_component(&context(Some(parent)), &linked)
            .expect_err("component callbacks cannot read DOM event payloads");
        assert!(
            error.to_string().contains(
                "reads DOM EventPayload, but component callbacks receive only CallbackPayload"
            ),
            "unexpected diagnostic for arity {payload_arity}: {error}"
        );
    }
}

#[test]
fn payload_callback_node_refs_remain_fail_closed() {
    let parent = definition(0x8c);
    let child = definition(0x8d);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["node_refs"] = json!([{"id": 0, "source_name": "target"}]);
    descriptor["counts"]["node_refs"] = json!(1);
    let mut linked = component_payload(parent, descriptor, 0x8e);
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "callback_shape": "required",
        "kind": "live_callback",
        "parent_source": {
            "id": 3,
            "namespace": "payload_handler_prelude_binding"
        },
        "proof": "payload_handler_prelude_binding_v1"
    });
    recommit_direct_component_edge(&mut linked, 0);
    let body = ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
        id: rsjs_ir::NodeRefId(0),
        effect: rsjs_ir::ClientNodeRefEffect::Focus,
    }]);
    linked.payload["transport"]["client_value_sources"]["payload_handlers"] = json!([{
        "body_ir": serde_json::from_str::<Value>(&client_block_json(&body)).unwrap(),
        "body_ir_version": CLIENT_IR_SCHEMA,
        "dependencies": {
            "callback_slots": [],
            "client_module_calls": [],
            "helper_edges": [],
            "mutations": [],
            "node_refs": [0],
            "queries": [],
            "signal_reads": [],
            "signal_writes": []
        },
        "kind": "payload_handler",
        "payload_arity": 0,
        "prelude_local_index": 3,
        "source_anchor": {
            "column": 9,
            "line": 5
        }
    }]);

    let result = adapt_component(&context(Some(parent)), &linked).unwrap();
    let AbiBridgeResult::Unsupported(report) = result else {
        panic!("payload handlers cannot export component-local node-ref handles");
    };
    assert_eq!(
        report.gaps,
        vec![CompatibilityGap::ComponentClientGraphMetadataMissing {
            features: vec!["payload_handlers.node_refs".to_owned()],
        }]
    );
}

#[test]
fn deferred_projection_roles_flow_through_participants_and_ignore_static_uses() {
    let root = definition(0x91);
    let middle = definition(0x92);
    let live_leaf = definition(0x93);
    let static_leaf = definition(0x94);

    let mut root_component = component_payload(root, static_descriptor(Value::Null), 0x61);
    let mut middle_component = component_payload(middle, static_descriptor(Value::Null), 0x62);
    let mut live_component = component_payload(live_leaf, static_descriptor(Value::Null), 0x63);
    let mut static_component = component_payload(static_leaf, static_descriptor(Value::Null), 0x64);
    set_projection_test_field(
        &mut root_component,
        "root_value",
        "deferred_projection",
        None,
    );
    set_projection_test_field(
        &mut middle_component,
        "middle_value",
        "deferred_projection",
        None,
    );
    set_projection_test_field(&mut live_component, "value", "prop", None);
    set_projection_test_field(&mut static_component, "static_value", "server_prop", None);
    add_projection_test_edge(&mut root_component, middle, 0xa1, "middle_value");
    add_projection_test_edge(&mut middle_component, live_leaf, 0xa2, "value");
    add_projection_test_edge(&mut middle_component, static_leaf, 0xa3, "static_value");

    let roles = resolve_deferred_projection_roles(&projection_test_plan(vec![
        root_component,
        middle_component,
        live_component,
        static_component,
    ]))
    .expect("a static side use must not conflict with the live projection chain");

    assert_eq!(roles.field(root, 0), Some(ResolvedProjectionRole::Value));
    assert_eq!(roles.field(middle, 0), Some(ResolvedProjectionRole::Value));
}

#[test]
fn deferred_projection_role_conflicts_fail_with_both_concrete_roles() {
    let parent = definition(0x95);
    let value_leaf = definition(0x96);
    let node_ref_leaf = definition(0x97);

    let mut parent_component = component_payload(parent, static_descriptor(Value::Null), 0x65);
    let mut value_component = component_payload(value_leaf, static_descriptor(Value::Null), 0x66);
    let mut node_ref_component =
        component_payload(node_ref_leaf, static_descriptor(Value::Null), 0x67);
    set_projection_test_field(&mut parent_component, "shared", "deferred_projection", None);
    set_projection_test_field(&mut value_component, "value", "prop", None);
    set_projection_test_field(&mut node_ref_component, "target", "node_ref", None);
    add_projection_test_edge(&mut parent_component, value_leaf, 0xb1, "value");
    add_projection_test_edge(&mut parent_component, node_ref_leaf, 0xb2, "target");

    let error = resolve_deferred_projection_roles(&projection_test_plan(vec![
        parent_component,
        value_component,
        node_ref_component,
    ]))
    .expect_err("one deferred field cannot be both a live value and a NodeRef");
    let message = error.to_string();
    assert!(message.contains("self.shared"), "{message}");
    assert!(message.contains("live value"), "{message}");
    assert!(message.contains("NodeRef handle"), "{message}");
}

#[test]
fn deferred_projection_preserves_required_callback_shape_across_hops() {
    let root = definition(0x98);
    let middle = definition(0x99);
    let leaf = definition(0x9a);

    let mut root_component = component_payload(root, static_descriptor(Value::Null), 0x68);
    let mut middle_component = component_payload(middle, static_descriptor(Value::Null), 0x69);
    let mut leaf_component = component_payload(leaf, static_descriptor(Value::Null), 0x6a);
    set_projection_test_field(
        &mut root_component,
        "root_callback",
        "deferred_projection",
        None,
    );
    set_projection_test_field(
        &mut middle_component,
        "middle_callback",
        "deferred_projection",
        None,
    );
    set_projection_test_field(
        &mut leaf_component,
        "on_change",
        "handler",
        Some("required"),
    );
    add_projection_test_edge(&mut root_component, middle, 0xc1, "middle_callback");
    add_projection_test_edge(&mut middle_component, leaf, 0xc2, "on_change");

    let roles = resolve_deferred_projection_roles(&projection_test_plan(vec![
        root_component,
        middle_component,
        leaf_component,
    ]))
    .expect("the terminal callback field fixes every forwarded projection");
    let expected = Some(ResolvedProjectionRole::Callback(
        InboundCallbackShape::Required,
    ));
    assert_eq!(roles.field(root, 0), expected);
    assert_eq!(roles.field(middle, 0), expected);
}

#[test]
fn transport_plan_populates_the_adapter_context_without_path_hints() {
    let component_definition = definition(0x31);
    let helper_definition = definition(0x32);
    let component = component_payload(
        component_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x51,
    );
    let helper = helper_payload(helper_definition, 0x52);
    let material = LinkPlanMaterial {
        link_unit: LinkUnitId::derive("auto-identity-context"),
        component_descriptors: vec![component.semantic_hash],
        helper_descriptors: vec![helper.semantic_hash],
        payload: SemanticHash::derive("auto-identity-payload"),
    };
    let plan = TransportLinkPlan {
        link_unit: material.link_unit,
        plan_id: material.plan_id(),
        material,
        components: vec![component],
        helpers: vec![helper],
        shared_state_definitions: Vec::new(),
        shared_state_acquisitions: Vec::new(),
    };

    let context = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let adapted = adapt_transport_plan(&context, &plan).unwrap();

    assert!(adapted.components[0].canonical().is_some());
    assert!(adapted.helpers[0].canonical().is_some());
}

#[test]
fn authored_type_tokens_do_not_control_semantic_identity() {
    let definition = definition(0x12);
    let first = component_payload(
        definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x22,
    );
    let mut second = first.clone();
    second.payload["component_descriptor"]["struct_name"] = json!("DifferentAlias");
    second.payload["component_descriptor"]["self_ty"] = json!("other::DifferentAlias");
    second.payload["semantic_hash"] = json!(hash_text(0x23));
    second.semantic_hash = semantic(0x23);

    let context = context(Some(definition));
    let first = adapt_component(&context, &first)
        .unwrap()
        .canonical()
        .unwrap()
        .envelope
        .semantic_hash;
    let second = adapt_component(&context, &second)
        .unwrap()
        .canonical()
        .unwrap()
        .envelope
        .semantic_hash;
    assert_eq!(first, second);
}

#[test]
fn missing_relocation_resolved_semantic_identity_is_explicit() {
    let definition = definition(0x13);
    let linked = component_payload(
        definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x24,
    );
    let result = adapt_component(&context(None), &linked).unwrap();
    let AbiBridgeResult::Unsupported(report) = result else {
        panic!("missing identity must fail closed");
    };
    assert_eq!(
        report.gaps,
        vec![CompatibilityGap::MissingResolvedSemanticIdentity]
    );
}

#[test]
fn signal_expression_and_text_binding_lower_to_the_canonical_graph() {
    let definition = definition(0x14);
    let mut descriptor = static_descriptor(json!({"kind": "text_binding_slot"}));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["bindings"]["text"] = json!([{"node": 0, "expr": 0}]);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    let linked = component_payload(definition, descriptor, 0x25);

    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("the first lossless reactive subset should adapt");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.values,
        BTreeSet::from([LocalValueId::new(0), LocalValueId::new(1)])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::DomTextUpdate)])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(1))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(1))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    let codegen = canonical
        .component_codegen()
        .expect("component codegen mapping");
    assert_eq!(
        codegen.signals,
        vec![ComponentCodegenSignal {
            producer_signal_id: 0,
            canonical_value: LocalValueId::new(0),
        }]
    );
    assert_eq!(
        codegen.expressions,
        vec![ComponentCodegenExpression {
            producer_expression_id: 0,
            canonical_value: LocalValueId::new(1),
        }]
    );
    assert_eq!(codegen.fragment_root, ComponentFragmentRoot::Unsupported);
}

#[test]
fn conditional_binding_lowers_to_a_stable_control_and_structural_effect() {
    let definition = definition(0x19);
    let mut descriptor = static_descriptor(json!({
        "children": [1, 2],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "bool", "value": true},
        "ty": {"kind": "bool"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({
                "id": 1,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "p"
                }
            }),
            json!({
                "id": 2,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "aside"
                }
            }),
        ]);
    descriptor["bindings"]["conditionals"] =
        json!([{"parent": 0, "condition": 0, "then": [1], "else": [2]}]);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["nodes"] = json!(3);

    let adapted = adapt_component(
        &context(Some(definition)),
        &component_payload(definition, descriptor, 0x2a),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("ordinary conditionals have a lossless canonical control shape");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.control_regions,
        BTreeSet::from([LocalControlRegionId::new(0)])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::DomStructuralUpdate)])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Control
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(1))
            && edge.to == DependencyEndpoint::Control(LocalControlRegionId::new(0))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Control
            && edge.from == DependencyEndpoint::Control(LocalControlRegionId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    assert!(!body
        .direct_components
        .values()
        .any(|edge| edge.lifecycle_control.is_some()));
}

#[test]
fn keyed_collection_lowers_to_a_site_and_source_driven_structural_effect() {
    let definition = definition(0x1a);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "null", "value": null},
        "ty": {"kind": "vec", "inner": {"kind": "string"}}
    }]);
    descriptor["keyed_collections"] = json!([{
        "container": 1,
        "id": 0,
        "item": {
            "children": [{"kind": "slot", "value": {"kind": "item"}}],
            "dynamic_attrs": [],
            "kind": "element",
            "static_attrs": [],
            "tag": "li"
        },
        "item_root": 3,
        "key": {"kind": "item"},
        "key_tokens": "item",
        "loop_node": 2,
        "source": {"id": 0, "kind": "signal"}
    }]);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "children": [2],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [{"name": "data-rsjs-list", "value": "1"}],
                    "tag": "ul"
                }
            },
            {
                "id": 2,
                "node": {
                    "attrs": "",
                    "body_prefix": "",
                    "children": [3],
                    "expr": "items.get()",
                    "kind": "for_loop",
                    "label": "",
                    "pat": "item"
                }
            },
            {
                "id": 3,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "li"
                }
            }
        ]
    });
    descriptor["counts"]["keyed_collections"] = json!(1);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["nodes"] = json!(4);

    let adapted = adapt_component(
        &context(Some(definition)),
        &component_payload(definition, descriptor, 0x2b),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("a validated keyed collection has a canonical structural graph");
    canonical
        .envelope
        .validate(&AbiSupport::v1(AbiVersion::new(1, 0)))
        .unwrap();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.keyed_collection_sites,
        BTreeSet::from([KeyedCollectionSiteId::new(0)])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::DomStructuralUpdate)])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && matches!(edge.from, DependencyEndpoint::Origin(_))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    assert!(canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::TYPED_DEPENDENCIES));
}

#[test]
fn conditional_direct_component_site_retains_control_and_branch_identity() {
    let parent_definition = definition(0x3a);
    let child_definition = definition(0x3b);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x3c,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    place_direct_site_in_conditional(&mut linked, ComponentLifecycleBranch::Then);

    let adapted = adapt_component(&context(Some(parent_definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("a conditional-single direct edge has a canonical lifecycle");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    let lifecycle = ComponentLifecycleControl {
        control: LocalControlRegionId::new(0),
        predicate: LocalValueId::new(1),
        predicate_origin: OriginId::new(2),
        branch: ComponentLifecycleBranch::Then,
    };
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].lifecycle_control,
        Some(lifecycle)
    );
    assert!(body.dependencies.values().any(|dependency| {
        dependency.kind == DependencyEdgeKind::Control
            && dependency.from == DependencyEndpoint::Control(lifecycle.control)
            && dependency.to == DependencyEndpoint::Component(ComponentEdgeId::new(0))
    }));
    assert!(canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE));

    let mut wrong_branch = linked.clone();
    wrong_branch.payload["transport"]["direct_component_edges"][0]["lifecycle"]["branch"] =
        json!("else");
    recommit_direct_component_edge(&mut wrong_branch, 0);
    let error = adapt_component(&context(Some(parent_definition)), &wrong_branch)
        .expect_err("branch polarity must match the exact schema-v9 site");
    assert!(
        error.to_string().contains("claims control 0 branch `else`"),
        "{error}"
    );

    let mut wrong_predicate = linked.clone();
    wrong_predicate.payload["transport"]["direct_component_edges"][0]["lifecycle"]
        ["predicate_expression"] = json!(1);
    let error = adapt_component(&context(Some(parent_definition)), &wrong_predicate)
        .expect_err("predicate identity must match the exact conditional graph");
    assert!(
        error.to_string().contains("claims predicate expression 1")
            && error
                .to_string()
                .contains("analyzed conditional graph names 0"),
        "{error}"
    );

    let mut wrong_control = linked.clone();
    wrong_control.payload["transport"]["direct_component_edges"][0]["lifecycle"]["control"] =
        json!(1);
    let error = adapt_component(&context(Some(parent_definition)), &wrong_control)
        .expect_err("control identity must match the exact schema-v9 site");
    assert!(
        error.to_string().contains("claims unknown control 1"),
        "{error}"
    );

    let mut unknown_field = linked;
    unknown_field.payload["transport"]["direct_component_edges"][0]["lifecycle"]["extra"] =
        json!(true);
    let error = adapt_component(&context(Some(parent_definition)), &unknown_field)
        .expect_err("lifecycle objects reject unknown wire fields");
    assert!(
        error.to_string().contains("unknown field") || error.to_string().contains("extra"),
        "{error}"
    );
}

#[test]
fn conditional_predicate_graph_cannot_recommit_without_its_rustc_relocation() {
    let parent_definition = definition(0x4a);
    let child_definition = definition(0x4b);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x4c,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    place_direct_site_in_conditional(&mut linked, ComponentLifecycleBranch::Then);
    adapt_component(&context(Some(parent_definition)), &linked)
        .expect("the relocation-authenticated predicate baseline must adapt");

    let rustc_source_edge = linked.component_edges[0].source_edge;
    let descriptor = &mut linked.payload["component_descriptor"];
    descriptor["signals"]
        .as_array_mut()
        .expect("fixture signals")
        .push(json!({
            "id": 1,
            "init": {"kind": "bool", "value": false},
            "ty": {"kind": "bool"}
        }));
    descriptor["exprs"]
        .as_array_mut()
        .expect("fixture expressions")
        .push(json!({
            "id": 1,
            "expr": {"kind": "signal_read", "signal": 1}
        }));
    descriptor["bindings"]["conditionals"][0]["condition"] = json!(1);
    descriptor["counts"]["signals"] = json!(2);
    descriptor["counts"]["exprs"] = json!(2);
    linked.payload["transport"]["direct_component_edges"][0]["lifecycle"]["predicate_expression"] =
        json!(1);

    // Model an attacker who coherently rewrites every JSON claim and outer
    // payload hash. They can derive a new source-edge ID, but cannot change
    // the rustc relocation retained in the object without recompilation.
    recommit_direct_component_edge(&mut linked, 0);
    let forged_payload_edge = linked.component_edges[0].source_edge;
    assert_ne!(
        forged_payload_edge, rustc_source_edge,
        "changing a valid predicate and its graph must change source-edge v3"
    );
    linked.component_edges[0].source_edge = rustc_source_edge;

    assert!(matches!(
        adapt_component(&context(Some(parent_definition)), &linked),
        Err(AbiAdapterError::SourceEdgeSetMismatch {
            kind: "component",
            definition,
            ..
        }) if definition == parent_definition
    ));
}

#[test]
fn counter_with_sparse_handlers_lowers_to_three_browser_effects() {
    let definition = definition(0x15);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 3,
        "init": {"kind": "bigint", "value": "0"},
        "integer_domain": "i64",
        "source_name": "count",
        "ty": {"kind": "i64"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 5,
        "expr": {"kind": "signal_read", "signal": 3}
    }]);
    let decrement = json!({
        "kind": "block",
        "stmts": [{
            "kind": "signal_update",
            "signal": 3,
            "closure": {
                "param": "value",
                "body": {
                    "kind": "binary",
                    "op": "checked_i64_sub",
                    "left": {"kind": "signal_read", "signal": 3},
                    "right": {
                        "kind": "literal",
                        "value": {"kind": "bigint", "value": "1"}
                    }
                }
            }
        }]
    });
    let increment = json!({
        "kind": "block",
        "stmts": [{
            "kind": "signal_update",
            "signal": 3,
            "closure": {
                "param": "value",
                "body": {
                    "kind": "binary",
                    "op": "checked_i64_add",
                    "left": {"kind": "signal_read", "signal": 3},
                    "right": {
                        "kind": "literal",
                        "value": {"kind": "bigint", "value": "1"}
                    }
                }
            }
        }]
    });
    descriptor["handlers"] = json!([
        {"body_ir": decrement, "body_ir_version": CLIENT_IR_SCHEMA, "id": 0},
        {"body_ir": increment, "body_ir_version": CLIENT_IR_SCHEMA, "id": 2},
        {
            "body_ir": {
                "kind": "block",
                "stmts": [{
                    "kind": "signal_set",
                    "signal": 3,
                    "value": {
                        "kind": "literal",
                        "value": {"kind": "bigint", "value": "99"}
                    }
                }]
            },
            "body_ir_version": CLIENT_IR_SCHEMA,
            "id": 5
        }
    ]);
    descriptor["bindings"] = json!({
        "attrs": [],
        "conditionals": [],
        "events": [
            {"event": "click", "handler": 0, "node": 1},
            {"event": "click", "handler": 2, "node": 3}
        ],
        "inputs": [],
        "refs": [],
        "text": [{"expr": 5, "node": 2}]
    });
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1, 2, 3],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "div"
                }
            },
            {
                "id": 1,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "button"
                }
            },
            {"id": 2, "node": {"kind": "text_binding_slot"}},
            {
                "id": 3,
                "node": {
                    "children": [],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "button"
                }
            }
        ]
    });
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["handlers"] = json!(3);
    descriptor["counts"]["nodes"] = json!(4);

    let linked = component_payload(definition, descriptor, 0x26);
    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("Counter is in the first lossless canonical subset");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };

    assert_eq!(
        body.values,
        BTreeSet::from([LocalValueId::new(0), LocalValueId::new(1)])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([
            (LocalEffectId::new(0), EffectKind::EventHandler),
            (LocalEffectId::new(1), EffectKind::EventHandler),
            (LocalEffectId::new(2), EffectKind::DomTextUpdate),
        ])
    );
    for effect in [LocalEffectId::new(0), LocalEffectId::new(1)] {
        assert!(body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Capture
                && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
                && edge.to == DependencyEndpoint::Effect(effect)
        }));
    }
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(1))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(2))
    }));
    assert_eq!(
        body.effects
            .values()
            .filter(|kind| **kind == EffectKind::EventHandler)
            .count(),
        2,
        "an unbound handler declaration must not become a browser obligation"
    );
}

#[test]
fn browser_capabilities_lower_per_occurrence_and_ignore_unbound_handlers() {
    use rsjs_ir::{
        BrowserStorageArea, ClientBlock, ClientBrowserEffect, ClientBrowserRead, ClientExpr,
        ClientStmt, LocationField, SignalId, StorageArea, StorageValueType,
    };

    let definition = definition(0x16);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 3,
        "init": {"kind": "string", "value": "theme"},
        "source_name": "key",
        "ty": {"kind": "string"}
    }]);
    let storage_read = ClientExpr::BrowserRead(ClientBrowserRead::Storage {
        area: BrowserStorageArea::Local,
        value_type: StorageValueType::String,
        key: Box::new(ClientExpr::SignalRead(SignalId(3))),
        fallback: None,
    });
    descriptor["exprs"] = json!([{
        "id": 5,
        "expr": serde_json::from_str::<Value>(&rsjs_ir::client_expr_json(&storage_read))
            .unwrap()
    }]);
    let pathname = ClientExpr::BrowserRead(ClientBrowserRead::Location(LocationField::Pathname));
    let bound_handler = ClientBlock::from_stmts(vec![
        ClientStmt::BrowserEffect(ClientBrowserEffect::StorageSetItem {
            area: StorageArea::Local,
            key: ClientExpr::SignalRead(SignalId(3)),
            value: pathname,
        }),
        ClientStmt::BrowserEffect(ClientBrowserEffect::ClipboardReadTextInto {
            target: SignalId(3),
            fallback: None,
        }),
    ]);
    let unbound_handler = ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
        ClientBrowserEffect::WindowPrint,
    )]);
    descriptor["handlers"] = json!([
        {
            "body_ir": serde_json::from_str::<Value>(&client_block_json(&bound_handler))
                .unwrap(),
            "body_ir_version": CLIENT_IR_SCHEMA,
            "id": 2
        },
        {
            "body_ir": serde_json::from_str::<Value>(&client_block_json(&unbound_handler))
                .unwrap(),
            "body_ir_version": CLIENT_IR_SCHEMA,
            "id": 9
        }
    ]);
    descriptor["bindings"] = json!({
        "attrs": [],
        "conditionals": [],
        "events": [{"event": "click", "handler": 2, "node": 0}],
        "inputs": [],
        "refs": [],
        "text": [{"expr": 5, "node": 1}]
    });
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "button"
                }
            },
            {"id": 1, "node": {"kind": "text_binding_slot"}}
        ]
    });
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["handlers"] = json!(2);
    descriptor["counts"]["nodes"] = json!(2);

    let linked = component_payload(definition, descriptor.clone(), 0x27);
    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("schema-v9 browser operations should lower canonically");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };

    assert_eq!(
        body.effects,
        BTreeMap::from([
            (LocalEffectId::new(0), EffectKind::BrowserRead),
            (LocalEffectId::new(1), EffectKind::EventHandler),
            (LocalEffectId::new(2), EffectKind::BrowserWrite),
            (LocalEffectId::new(3), EffectKind::BrowserRead),
            (LocalEffectId::new(4), EffectKind::BrowserWrite),
            (LocalEffectId::new(5), EffectKind::DomTextUpdate),
        ])
    );
    assert_eq!(
        body.origins,
        BTreeMap::from([
            (OriginId::new(0), OriginKind::WritableSignal),
            (OriginId::new(1), OriginKind::SourceOnly),
            (OriginId::new(2), OriginKind::EventHandler),
            (OriginId::new(3), OriginKind::BrowserCapabilityRead),
            (OriginId::new(4), OriginKind::BrowserCapabilityWrite),
            (OriginId::new(5), OriginKind::BrowserCapabilityRead),
            (OriginId::new(6), OriginKind::BrowserCapabilityWrite),
            (OriginId::new(7), OriginKind::SourceOnly),
        ])
    );
    for effect in [LocalEffectId::new(0), LocalEffectId::new(2)] {
        assert!(body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Capture
                && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
                && edge.to == DependencyEndpoint::Effect(effect)
        }));
    }
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(0))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(1))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(1))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(2))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(3))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(2))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(4))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(0))
    }));
    assert_eq!(
        body.effects
            .values()
            .filter(|kind| **kind == EffectKind::BrowserWrite)
            .count(),
        2,
        "the unbound WindowPrint handler must not become a browser obligation"
    );

    descriptor["client_origin_permission"] = json!("participant_only");
    let linked = component_payload(definition, descriptor, 0x28);
    let error = adapt_component(&context(Some(definition)), &linked)
        .expect_err("browser capability origins require client permission");
    assert!(
        error
            .to_string()
            .contains("requires origin-capable permission"),
        "unexpected origin-permission diagnostic: {error}"
    );
}

#[test]
fn reachable_queries_lower_to_exact_canonical_operations() {
    use rsjs_ir::{ClientBlock, ClientStmt, QueryAccessor, QueryId};

    let definition = definition(0x70);
    let query_read = ClientExpr::QueryRead {
        id: QueryId(0),
        accessor: QueryAccessor::Data,
    };
    let mut descriptor = static_descriptor(json!({
        "children": [1],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "button"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["exprs"] = Value::Array(
        [
            ClientExpr::string("query-key"),
            ClientExpr::string("server-initial"),
            ClientExpr::string("/api/query"),
            query_read,
        ]
        .into_iter()
        .enumerate()
        .map(|(id, expression)| {
            json!({
                "id": id,
                "expr": serde_json::from_str::<Value>(&client_expr_json(&expression))
                    .unwrap()
            })
        })
        .collect(),
    );
    descriptor["queries"] = json!([{
        "client_fetcher": 2,
        "data_ty": {"kind": "string"},
        "id": 0,
        "key_expr": 0,
        "options": {
            "download_progress": false,
            "interval_ms": null,
            "refetch_on_focus": false,
            "retry_count": 0,
            "stale_time_ms": 0,
            "stream_text": false
        },
        "server_initial": 1
    }]);
    let handler = ClientBlock::from_stmts(vec![
        ClientStmt::QueryRefetch { query: QueryId(0) },
        ClientStmt::QueryInvalidate { query: QueryId(0) },
    ]);
    descriptor["handlers"] = json!([{
        "body_ir": serde_json::from_str::<Value>(&client_block_json(&handler)).unwrap(),
        "body_ir_version": CLIENT_IR_SCHEMA,
        "id": 0
    }]);
    descriptor["bindings"] = json!({
        "attrs": [],
        "conditionals": [],
        "events": [{"event": "click", "handler": 0, "node": 0}],
        "inputs": [],
        "refs": [],
        "text": [{"expr": 3, "node": 1}]
    });
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": 1, "node": {"kind": "text_binding_slot"}}));
    descriptor["counts"]["exprs"] = json!(4);
    descriptor["counts"]["handlers"] = json!(1);
    descriptor["counts"]["nodes"] = json!(2);

    let adapted = adapt_component(
        &context(Some(definition)),
        &component_payload(definition, descriptor, 0x71),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("direct reachable query operations should adapt canonically");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.origins.get(&OriginId::new(5)),
        Some(&OriginKind::Query)
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([
            (LocalEffectId::new(0), EffectKind::QueryObservation),
            (LocalEffectId::new(1), EffectKind::EventHandler),
            (LocalEffectId::new(2), EffectKind::QueryRefetch),
            (LocalEffectId::new(3), EffectKind::QueryInvalidation),
            (LocalEffectId::new(4), EffectKind::DomTextUpdate),
        ])
    );
    for effect in [
        LocalEffectId::new(0),
        LocalEffectId::new(2),
        LocalEffectId::new(3),
    ] {
        assert!(body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Effect
                && edge.from == DependencyEndpoint::Origin(OriginId::new(5))
                && edge.to == DependencyEndpoint::Effect(effect)
        }));
    }
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(0))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(3))
    }));
    for effect in [LocalEffectId::new(2), LocalEffectId::new(3)] {
        assert!(body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Effect
                && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(1))
                && edge.to == DependencyEndpoint::Effect(effect)
        }));
    }
    assert!(
        canonical
            .component_codegen()
            .expect("component has codegen")
            .uses_queries
    );
}

#[test]
fn dead_query_and_mutation_declarations_remain_inert() {
    let root = json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    });

    let mut query_descriptor = static_descriptor(root.clone());
    query_descriptor["client_origin_permission"] = json!("origin_capable");
    query_descriptor["exprs"] = Value::Array(
        ["query-key", "server-initial", "/api/query"]
            .into_iter()
            .enumerate()
            .map(|(id, value)| {
                let expression = ClientExpr::string(value);
                json!({
                    "id": id,
                    "expr": serde_json::from_str::<Value>(&client_expr_json(&expression))
                        .unwrap()
                })
            })
            .collect(),
    );
    query_descriptor["queries"] = json!([{
        "client_fetcher": 2,
        "data_ty": {"kind": "string"},
        "id": 0,
        "key_expr": 0,
        "options": {
            "download_progress": false,
            "interval_ms": null,
            "refetch_on_focus": false,
            "retry_count": 0,
            "stale_time_ms": 0,
            "stream_text": false
        },
        "server_initial": 1
    }]);
    query_descriptor["counts"]["exprs"] = json!(3);

    let query_definition = definition(0x80);
    let result = adapt_component(
        &context(Some(query_definition)),
        &component_payload(query_definition, query_descriptor, 0x81),
    )
    .unwrap();
    let AbiBridgeResult::Canonical(query) = result else {
        panic!("a dead query declaration should remain inert");
    };
    let DescriptorBody::Component(body) = &query.envelope.body else {
        unreachable!("query fixture adapts as a component")
    };
    assert!(
        !body.origins.values().any(|kind| *kind == OriginKind::Query),
        "dead query declarations must not create canonical origins"
    );
    assert!(
        !query
            .component_codegen()
            .expect("component has codegen")
            .uses_queries,
        "dead query declarations must not load the query runtime"
    );

    let mut mutation_descriptor = static_descriptor(root.clone());
    mutation_descriptor["client_origin_permission"] = json!("origin_capable");
    mutation_descriptor["exprs"] = Value::Array(
        ["mutation-key", "server-runner", "/api/mutate"]
            .into_iter()
            .enumerate()
            .map(|(id, value)| {
                let expression = ClientExpr::string(value);
                json!({
                    "id": id,
                    "expr": serde_json::from_str::<Value>(&client_expr_json(&expression))
                        .unwrap()
                })
            })
            .collect(),
    );
    mutation_descriptor["mutations"] = json!([{
        "client_runner": 2,
        "id": 0,
        "invalidates": [],
        "key_expr": 0,
        "options": {
            "on_success_invalidate_strategy": "refetch",
            "retry_count": 0
        },
        "server_runner": 1
    }]);
    mutation_descriptor["counts"]["exprs"] = json!(3);

    let mutation_definition = definition(0x82);
    let result = adapt_component(
        &context(Some(mutation_definition)),
        &component_payload(mutation_definition, mutation_descriptor, 0x83),
    )
    .unwrap();
    let AbiBridgeResult::Canonical(mutation) = result else {
        panic!("a dead mutation declaration should remain inert");
    };
    let DescriptorBody::Component(body) = &mutation.envelope.body else {
        unreachable!("mutation fixture adapts as a component")
    };
    assert!(
        !body
            .origins
            .values()
            .any(|kind| *kind == OriginKind::Mutation),
        "dead mutation declarations must not create canonical origins"
    );
    assert!(
        !mutation
            .component_codegen()
            .expect("component has codegen")
            .uses_mutations,
        "dead mutation declarations must not load the mutation runtime"
    );

    let mut shared_descriptor = static_descriptor(root.clone());
    shared_descriptor["client_origin_permission"] = json!("origin_capable");
    shared_descriptor["shared_stores"] = json!([{
        "id": 0,
        "rust_type": "crate :: WorkbenchState",
        "source_name": "workbench"
    }]);

    let shared_definition = definition(0x84);
    let mut shared_component = component_payload(shared_definition, shared_descriptor, 0x85);
    attach_shared_state_acquisitions(&mut shared_component, &[definition(0x86)]);
    let result = adapt_component(&context(Some(shared_definition)), &shared_component).unwrap();
    let AbiBridgeResult::Canonical(shared) = result else {
        panic!("an unused SharedState acquisition must remain inert");
    };
    let DescriptorBody::Component(body) = &shared.envelope.body else {
        unreachable!("SharedState fixture adapts as a component")
    };
    assert!(
        body.shared_state.is_empty()
            && body.origins.is_empty()
            && body.effects.is_empty()
            && body.client_ir_hash
                == rsjs_ir::abi::canonical_semantic_hash("rsjs.empty-component-client-ir.v1"),
        "dead acquisition records must not form an owner, publish client IR, or activate runtime"
    );

    let mut node_ref_descriptor = static_descriptor(root.clone());
    node_ref_descriptor["client_origin_permission"] = json!("origin_capable");
    node_ref_descriptor["node_refs"] = json!([{"id": 0}]);
    node_ref_descriptor["bindings"]["refs"] = json!([{"node": 0, "ref": 0}]);
    node_ref_descriptor["counts"]["node_refs"] = json!(1);

    let node_ref_definition = definition(0x86);
    let adapted = adapt_component(
        &context(Some(node_ref_definition)),
        &component_payload(node_ref_definition, node_ref_descriptor, 0x87),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("a rendered component-local node ref has complete fragment lifecycle codegen");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.origins,
        BTreeMap::from([(OriginId::new(0), OriginKind::NodeRef)])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::NodeReference)])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Origin(OriginId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));

    let mut dead_node_ref_descriptor = static_descriptor(root);
    dead_node_ref_descriptor["client_origin_permission"] = json!("origin_capable");
    dead_node_ref_descriptor["node_refs"] = json!([{"id": 0}]);
    dead_node_ref_descriptor["counts"]["node_refs"] = json!(1);
    let dead_definition = definition(0x88);
    let adapted = adapt_component(
        &context(Some(dead_definition)),
        &component_payload(dead_definition, dead_node_ref_descriptor, 0x89),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("a dead node-ref declaration must remain a static canonical component");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert!(
        body.origins.is_empty() && body.effects.is_empty(),
        "a dead declaration must not create a node-ref origin or owner obligation"
    );
}

#[test]
fn active_shared_state_read_write_lower_with_exact_relocation_schema() {
    let component_definition = definition(0x90);
    let shared_definition = definition(0x91);
    let schema = linked_shared_state_definition(
        shared_definition,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
    );
    let expected_schema = SemanticHash::from_bytes(schema.schema_abi_hash);
    let context = context_with_shared_state(component_definition, &[schema]);
    let mut linked = component_payload(
        component_definition,
        active_shared_state_descriptor("mode", "crate :: state :: WorkbenchAlias"),
        0x92,
    );
    attach_shared_state_acquisitions(&mut linked, &[shared_definition]);

    let adapted = adapt_component(&context, &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("resolved active SharedState reads and writes are canonical");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.shared_state,
        BTreeMap::from([(
            SharedStateRefId::new(0),
            SharedStateReference {
                type_key: "acme.workbench".to_owned(),
                schema_abi: expected_schema,
                origin: OriginId::new(3),
            },
        )])
    );
    assert_eq!(
        body.origins.get(&OriginId::new(3)),
        Some(&OriginKind::SharedState)
    );
    assert!(
        !body
            .origins
            .values()
            .any(|origin| *origin == OriginKind::WritableSignal),
        "a SharedState field cell is not a component-local writable-signal origin"
    );
    assert_eq!(
        body.effects
            .values()
            .filter(|effect| **effect == EffectKind::SharedStateRead)
            .count(),
        1
    );
    assert_eq!(
        body.effects
            .values()
            .filter(|effect| **effect == EffectKind::SharedStateWrite)
            .count(),
        1
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Component
            && edge.from == DependencyEndpoint::Origin(OriginId::new(3))
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(0))
    }));
    let read_effect = body
        .effects
        .iter()
        .find_map(|(effect, kind)| (*kind == EffectKind::SharedStateRead).then_some(*effect))
        .unwrap();
    let write_effect = body
        .effects
        .iter()
        .find_map(|(effect, kind)| (*kind == EffectKind::SharedStateWrite).then_some(*effect))
        .unwrap();
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Origin(OriginId::new(3))
            && edge.to == DependencyEndpoint::Effect(read_effect)
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(read_effect)
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(1))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Effect(write_effect)
            && edge.to == DependencyEndpoint::Value(LocalValueId::new(0))
    }));

    let mut reexported = component_payload(
        component_definition,
        active_shared_state_descriptor("mode", "renamed_dependency :: public_api :: StateReexport"),
        0x93,
    );
    attach_shared_state_acquisitions(&mut reexported, &[shared_definition]);
    let reexported_result = adapt_component(&context, &reexported).unwrap();
    let reexported = reexported_result
        .canonical()
        .expect("a Rust import alias must not affect SharedState identity");
    let DescriptorBody::Component(reexported_body) = &reexported.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        reexported_body.shared_state, body.shared_state,
        "only the relocation-selected definition may supply type_key/schema identity"
    );
}

#[test]
fn shared_state_same_key_conflicts_are_prepared_for_semantic_linking() {
    let first_component = definition(0x94);
    let second_component = definition(0x95);
    let first_definition = definition(0x96);
    let second_definition = definition(0x97);
    let first_schema = linked_shared_state_definition(
        first_definition,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
    );
    let second_schema = linked_shared_state_definition(
        second_definition,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::Bool)],
    );
    let mut context = context_with_shared_state(
        first_component,
        &[first_schema.clone(), second_schema.clone()],
    );
    context.insert_component_identity(
        second_component,
        ResolvedSemanticPath::new("acme-web", "acme_web", ["resolved", "ui"], "SecondPanel"),
    );
    let mut first = component_payload(
        first_component,
        active_shared_state_descriptor("mode", "crate :: FirstState"),
        0x98,
    );
    attach_shared_state_acquisitions(&mut first, &[first_definition]);
    let mut second = component_payload(
        second_component,
        active_shared_state_descriptor("mode", "crate :: SecondState"),
        0x99,
    );
    attach_shared_state_acquisitions(&mut second, &[second_definition]);
    let first = adapt_component(&context, &first)
        .unwrap()
        .canonical()
        .unwrap()
        .clone();
    let second = adapt_component(&context, &second)
        .unwrap()
        .canonical()
        .unwrap()
        .clone();
    let DescriptorBody::Component(first_body) = &first.envelope.body else {
        panic!("expected first component");
    };
    let DescriptorBody::Component(second_body) = &second.envelope.body else {
        panic!("expected second component");
    };
    assert_eq!(
        first_body.shared_state[&SharedStateRefId::new(0)].type_key,
        second_body.shared_state[&SharedStateRefId::new(0)].type_key
    );
    assert_ne!(
        first_body.shared_state[&SharedStateRefId::new(0)].schema_abi,
        second_body.shared_state[&SharedStateRefId::new(0)].schema_abi
    );
    let instances = [
        rsjs_ir::link::ResolvedComponentInstance {
            id: rsjs_ir::link::OwnerCandidateId::new(0),
            descriptor: first_body,
            has_browser_obligation: true,
        },
        rsjs_ir::link::ResolvedComponentInstance {
            id: rsjs_ir::link::OwnerCandidateId::new(1),
            descriptor: second_body,
            has_browser_obligation: true,
        },
    ];
    let conflict = rsjs_ir::link::link_shared_state_schemas(&instances)
        .expect_err("same key with different exact schema hashes must conflict");
    assert_eq!(conflict.type_key, "acme.workbench");
}

#[test]
fn active_shared_state_schema_failures_are_focused_and_dead_unresolved_is_inert() {
    let component_definition = definition(0xa0);
    let target = definition(0xa1);
    let mut linked = component_payload(
        component_definition,
        active_shared_state_descriptor("mode", "crate :: StateAlias"),
        0xa2,
    );
    attach_shared_state_acquisitions(&mut linked, &[target]);

    let missing = adapt_component(&context(Some(component_definition)), &linked)
        .expect_err("an active acquisition requires its exact target definition");
    assert!(
        missing.to_string().contains("missing definition")
            && missing.to_string().contains("active SharedState store 0"),
        "unexpected missing-definition diagnostic: {missing}"
    );

    let wrong_field = linked_shared_state_definition(
        target,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("other", rsjs_transport::SharedStateSchemaCodec::String)],
    );
    let error = adapt_component(
        &context_with_shared_state(component_definition, &[wrong_field]),
        &linked,
    )
    .expect_err("field names must come from the decoded schema");
    assert!(
        error.to_string().contains("does not contain that field")
            && error.to_string().contains("`mode`"),
        "unexpected field diagnostic: {error}"
    );

    let mut mismatched_hash = linked_shared_state_definition(
        target,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
    );
    mismatched_hash.schema_abi_hash = [0x5a; 32];
    let error = adapt_component(
        &context_with_shared_state(component_definition, &[mismatched_hash]),
        &linked,
    )
    .expect_err("the schema hash must match the exact canonical bytes");
    assert!(
        error.to_string().contains("canonical bytes hash"),
        "unexpected hash diagnostic: {error}"
    );

    let malformed =
        linked_shared_state_definition_from_abi(target, "acme.workbench", vec![0xff, 0x00, 0x7f]);
    let error = adapt_component(
        &context_with_shared_state(component_definition, &[malformed]),
        &linked,
    )
    .expect_err("malformed canonical schema bytes must fail during decoding");
    assert!(
        error.to_string().contains("invalid canonical schema ABI"),
        "unexpected schema decoder diagnostic: {error}"
    );

    let bad_runtime = linked_shared_state_definition(
        target,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION + 1,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
    );
    let error = adapt_component(
        &context_with_shared_state(component_definition, &[bad_runtime]),
        &linked,
    )
    .expect_err("unsupported SharedState runtime ABI must fail closed");
    assert!(
        error.to_string().contains("uses runtime ABI") && error.to_string().contains("supports"),
        "unexpected runtime diagnostic: {error}"
    );

    let unresolved_abi = rsjs_transport::encode_unresolved_shared_state_schema_abi(
        SHARED_STATE_RUNTIME_ABI_VERSION,
        SharedStateSchemaUnresolvedReason::NestedClientProps,
    );
    let unresolved =
        linked_shared_state_definition_from_abi(target, "acme.workbench", unresolved_abi);
    let unresolved_context =
        context_with_shared_state(component_definition, std::slice::from_ref(&unresolved));
    let error = adapt_component(&unresolved_context, &linked)
        .expect_err("nested ClientProps schema must remain unresolved");
    assert!(
        error.to_string().contains("nested ClientProps"),
        "unexpected unresolved-schema diagnostic: {error}"
    );

    let mut dead_descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    dead_descriptor["shared_stores"] = json!([{
        "id": 0,
        "rust_type": "crate :: StateAlias",
        "source_name": "unused"
    }]);
    let mut dead = component_payload(component_definition, dead_descriptor, 0xa3);
    attach_shared_state_acquisitions(&mut dead, &[target]);
    let dead_result = adapt_component(&unresolved_context, &dead)
        .expect("unused unresolved definitions must not be decoded");
    let dead = dead_result
        .canonical()
        .expect("unused acquisition remains static");
    let DescriptorBody::Component(dead_body) = &dead.envelope.body else {
        panic!("expected component descriptor");
    };
    assert!(
        dead_body.shared_state.is_empty()
            && dead_body.origins.is_empty()
            && dead_body.effects.is_empty()
    );
}

#[test]
fn bound_node_ref_handler_keeps_exact_canonical_dependency() {
    let definition = definition(0x8a);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "button"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["node_refs"] = json!([{"id": 0, "source_name": "button"}]);
    descriptor["handlers"] = json!([{
        "id": 0,
        "body_ir_version": CLIENT_IR_SCHEMA,
        "body_ir": serde_json::from_str::<Value>(&client_block_json(
            &ClientBlock::from_stmts(vec![
                ClientStmt::Expr(ClientExpr::NodeRefRead {
                    id: rsjs_ir::NodeRefId(0),
                    accessor: rsjs_ir::NodeRefAccessor::Value,
                }),
                ClientStmt::NodeRefEffect {
                    id: rsjs_ir::NodeRefId(0),
                    effect: rsjs_ir::ClientNodeRefEffect::Focus,
                },
            ]),
        ))
        .unwrap()
    }]);
    descriptor["bindings"]["events"] = json!([{"node": 0, "event": "click", "handler": 0}]);
    descriptor["bindings"]["refs"] = json!([{"node": 0, "ref": 0}]);
    descriptor["counts"]["handlers"] = json!(1);
    descriptor["counts"]["node_refs"] = json!(1);

    let adapted = adapt_component(
        &context(Some(definition)),
        &component_payload(definition, descriptor, 0x8b),
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("a bound handler may use its component-local node ref");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };

    assert_eq!(
        body.origins,
        BTreeMap::from([
            (OriginId::new(0), OriginKind::EventHandler),
            (OriginId::new(1), OriginKind::NodeRef),
        ])
    );
    assert_eq!(
        body.effects,
        BTreeMap::from([
            (LocalEffectId::new(0), EffectKind::NodeReference),
            (LocalEffectId::new(1), EffectKind::EventHandler),
        ])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Effect
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(1))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Effect(LocalEffectId::new(0))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(1))
    }));
}

#[test]
fn hydration_constraints_preserve_absent_eager_and_deferred_meaning() {
    let definition = definition(0x7f);
    for (json, expected) in [
        (
            json!({"hydration_constraint": {"kind": "unconstrained"}}),
            HydrationConstraint::Unconstrained,
        ),
        (
            json!({"hydration_constraint": {"kind": "eager"}}),
            HydrationConstraint::Eager,
        ),
        (
            json!({"hydration_constraint": {"kind": "visible"}}),
            HydrationConstraint::Visible,
        ),
        (
            json!({"hydration_constraint": {"kind": "idle"}}),
            HydrationConstraint::Idle,
        ),
        (
            json!({
                "hydration_constraint": {
                    "event": "click",
                    "kind": "interaction"
                }
            }),
            HydrationConstraint::Interaction {
                event: Some("click".to_owned()),
            },
        ),
        (
            json!({
                "hydration_constraint": {
                    "kind": "media",
                    "query": "(min-width: 60rem)"
                }
            }),
            HydrationConstraint::Media {
                query: "(min-width: 60rem)".to_owned(),
            },
        ),
    ] {
        assert_eq!(
            validate_hydration_constraint(definition, json.as_object().unwrap()).unwrap(),
            expected,
        );
    }

    let malformed = json!({
        "hydration_constraint": {
            "kind": "interaction"
        }
    });
    assert!(
        validate_hydration_constraint(definition, malformed.as_object().unwrap())
            .unwrap_err()
            .to_string()
            .contains("missing `event`")
    );
}

#[test]
fn server_fragment_root_uses_an_unanchored_unknown_span() {
    let definition = definition(0x15);
    let linked = component_payload(
        definition,
        static_descriptor(json!({
            "kind": "server_only",
            "source": "explicit_boundary",
            "tokens": "render_markdown(document)"
        })),
        0x26,
    );
    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result.canonical().unwrap();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(
        body.span,
        SpanCardinality::Unknown {
            region: RegionSiteId::new(0),
            anchored: false
        }
    );
    assert!(body.region_sites.contains(&RegionSiteId::new(0)));
}

#[test]
fn static_component_may_keep_an_internal_unknown_dom_span() {
    let definition = definition(0x16);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "inferred_opaque_ssr",
                    "tokens": "render_markdown(document)"
                }
            }
        ]
    });
    descriptor["counts"]["nodes"] = json!(2);
    let linked = component_payload(definition, descriptor, 0x27);

    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("an internal opaque span is harmless without client behavior");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.span, SpanCardinality::Exact(1));
    assert!(body.region_sites.is_empty());
}

#[test]
fn marker_addressed_client_binding_can_follow_an_internal_unknown_dom_span() {
    let definition = definition(0x17);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["bindings"]["text"] = json!([{"node": 2, "expr": 0}]);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1, 2],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "inferred_opaque_ssr",
                    "tokens": "render_markdown(document)"
                }
            },
            {"id": 2, "node": {"kind": "text_binding_slot"}}
        ]
    });
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["nodes"] = json!(3);
    let linked = component_payload(definition, descriptor, 0x28);

    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("owner-scoped binding markers do not depend on preceding sibling cardinality");
    canonical
        .envelope
        .validate(&AbiSupport::v1(AbiVersion::new(1, 0)))
        .unwrap();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.span, SpanCardinality::Exact(1));
    assert!(body.region_sites.is_empty());
    assert_eq!(
        body.effects,
        BTreeMap::from([(LocalEffectId::new(0), EffectKind::DomTextUpdate)])
    );
    assert!(body.dependencies.values().any(|edge| {
        edge.kind == DependencyEdgeKind::Data
            && edge.from == DependencyEndpoint::Value(LocalValueId::new(1))
            && edge.to == DependencyEndpoint::Effect(LocalEffectId::new(0))
    }));
    assert_eq!(
        canonical
            .component_codegen()
            .expect("component codegen mapping")
            .fragment_root,
        ComponentFragmentRoot::ExactOneElement
    );
}

#[test]
fn client_component_static_slot_is_canonical_but_not_standalone_root_eligible() {
    let definition = definition(0x29);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["fields"] = json!([{
        "id": 0,
        "kind": "static_slot",
        "name": "children",
        "permits_nested_owner": false,
        "source": "inferred_single_unconditional_server_slot",
        "span": "exact_one"
    }]);
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["bindings"]["text"] = json!([{"node": 2, "expr": 0}]);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1, 2],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "inferred_simple_ssr",
                    "tokens": "self . children"
                }
            },
            {"id": 2, "node": {"kind": "text_binding_slot"}}
        ]
    });
    descriptor["counts"]["fields"] = json!(1);
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["nodes"] = json!(3);
    let linked = component_payload(definition, descriptor, 0x2a);

    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("the incoming component edge supplies the slot span proof");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert!(
        !body.root_eligible,
        "the definition-level slot contract cannot prove a standalone instantiation"
    );
    let slot = body.fields.get(&ComponentFieldId::new(0)).unwrap();
    assert_eq!(slot.role, ComponentFieldRole::Slot);
    assert_eq!(
        slot.static_slot,
        StaticSlotCapability::Immutable {
            permits_nested_owner: false
        }
    );
}

#[test]
fn client_component_with_a_linked_direct_component_site_converts() {
    let parent_definition = definition(0x18);
    let child_definition = definition(0x19);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "int", "value": 1},
        "ty": {"kind": "i32"}
    }]);
    descriptor["exprs"] = json!([{
        "id": 0,
        "expr": {"kind": "signal_read", "signal": 0}
    }]);
    descriptor["bindings"]["text"] = json!([{"node": 2, "expr": 0}]);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1, 2],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "direct_component_site",
                    "tokens": "ImportedBadge { label: \"ready\" }"
                }
            },
            {"id": 2, "node": {"kind": "text_binding_slot"}}
        ]
    });
    descriptor["counts"]["signals"] = json!(1);
    descriptor["counts"]["exprs"] = json!(1);
    descriptor["counts"]["nodes"] = json!(3);
    let mut linked = component_payload(parent_definition, descriptor, 0x29);
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    let result = adapt_component(&context(Some(parent_definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("a final linked direct-component wrapper has exact-one cardinality");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.span, SpanCardinality::Exact(1));
    assert_eq!(body.direct_components.len(), 1);
}

#[test]
fn root_direct_component_site_is_an_exact_one_element_fragment() {
    let parent_definition = definition(0x1b);
    let child_definition = definition(0x1c);
    let descriptor = static_descriptor(json!({
        "kind": "server_only",
        "source": "direct_component_site",
        "tokens": "ImportedBadge { label: \"ready\" }"
    }));
    let mut linked = component_payload(parent_definition, descriptor, 0x2b);
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );

    let adapted = adapt_component(&context(Some(parent_definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("the final linked boundary supplies one element");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.span, SpanCardinality::Exact(1));
    assert!(body.region_sites.is_empty());
    assert_eq!(
        canonical
            .component_codegen()
            .expect("component codegen metadata")
            .fragment_root,
        ComponentFragmentRoot::ExactOneElement
    );
}

#[test]
fn direct_component_site_provenance_requires_a_composed_transport_edge() {
    let definition = definition(0x1a);
    let mut descriptor = static_descriptor(Value::Null);
    descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "main"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "direct_component_site",
                    "tokens": "render_markdown(document)"
                }
            }
        ]
    });
    descriptor["counts"]["nodes"] = json!(2);
    let linked = component_payload(definition, descriptor, 0x2a);

    let error = adapt_component(&context(Some(definition)), &linked)
        .expect_err("direct-component provenance cannot be forged without a composed edge");
    assert!(
        error.to_string().contains("rustc-resolved composed edge"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn every_composed_transport_edge_requires_one_direct_component_site() {
    let parent_definition = definition(0x1d);
    let child_definition = definition(0x1e);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x2c,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["component_descriptor"]["view_tree"]["nodes"][1]["node"]["source"] =
        json!("inferred_opaque_ssr");

    let error = adapt_component(&context(Some(parent_definition)), &linked)
        .expect_err("a composed transport edge without its exact view site must fail");
    assert!(
        error.to_string().contains("claims site node 1")
            && error.to_string().contains("not a `direct_component_site`"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn server_only_component_fields_need_no_client_type_witness() {
    let definition = definition(0x16);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["fields"] = json!([{
        "id": 0,
        "kind": "server_prop",
        "name": "title",
        "source": "inferred_simple_ssr"
    }]);
    descriptor["counts"]["fields"] = json!(1);
    let linked = component_payload(definition, descriptor, 0x27);
    let result = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("server-only fields have no browser type ABI");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(
        body.fields.get(&ComponentFieldId::new(0)),
        Some(&ComponentFieldSchema {
            source_name: "title".to_owned(),
            role: ComponentFieldRole::ServerOnly,
            type_witness: None,
            crossing_hook: None,
            client_value: None,
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Unsupported,
        })
    );
}

#[test]
fn proven_static_direct_component_edge_becomes_canonical() {
    let parent_definition = definition(0x1a);
    let child_definition = definition(0x1b);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x2a,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    let expected_source_edge = linked.component_edges[0].source_edge;

    let result = adapt_component(&context(Some(parent_definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("a statically present server-only edge is lossless");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    let edge = body
        .direct_components
        .get(&ComponentEdgeId::new(0))
        .expect("canonical direct edge");
    assert_eq!(edge.boundary, ComponentBoundary::Composed);
    assert_eq!(edge.lifecycle_control, None);
    assert_eq!(
        edge.bindings.get("label"),
        Some(&ComponentFieldBinding {
            source: ComponentBindingSource::ServerOnly,
            origin: OriginId::new(0),
        })
    );
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(1)),
        Some(&ResolvedTransportWitness::ComponentEdge {
            source_edge: expected_source_edge,
            target: child_definition,
        })
    );
}

#[test]
fn static_direct_node_ref_handle_alias_becomes_canonical_client_handle() {
    let parent_definition = definition(0x2b);
    let child_definition = definition(0x2c);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["node_refs"] = json!([{"id": 0}]);
    descriptor["counts"]["node_refs"] = json!(1);
    let mut linked = component_payload(parent_definition, descriptor, 0x2d);
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "node_ref_handle",
        "exact_node_ref_handle_alias_v1",
    );

    let result = adapt_component(&context(Some(parent_definition)), &linked).unwrap();
    let canonical = result
        .canonical()
        .expect("a static exact NodeRef alias is lossless");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(body.values, BTreeSet::from([LocalValueId::new(0)]));
    assert_eq!(
        body.origins,
        BTreeMap::from([
            (OriginId::new(0), OriginKind::NodeRef),
            (OriginId::new(1), OriginKind::SourceOnly),
        ])
    );
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].bindings["label"].source,
        ComponentBindingSource::ClientHandle {
            value: LocalValueId::new(0)
        }
    );
    assert_eq!(
        canonical
            .component_codegen()
            .unwrap()
            .direct_node_ref_aliases,
        vec![ComponentCodegenDirectNodeRefAlias {
            edge_id: ComponentEdgeId::new(0),
            field_name: "label".to_owned(),
            producer_node_ref_id: 0,
            canonical_value: LocalValueId::new(0),
        }]
    );

    let mut forged_kind = linked.clone();
    forged_kind.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"]
        ["handle_kind"] = json!("query");
    assert!(matches!(
        adapt_component(&context(Some(parent_definition)), &forged_kind),
        Err(AbiAdapterError::InvalidPayload { reason, .. })
            if reason.contains("unsupported `handle_kind` `query`; expected `node_ref`")
    ));

    let mut missing_kind = linked;
    missing_kind.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"]
        .as_object_mut()
        .unwrap()
        .remove("handle_kind");
    assert!(matches!(
        adapt_component(&context(Some(parent_definition)), &missing_kind),
        Err(AbiAdapterError::InvalidPayload { reason, .. })
            if reason.contains("missing string `handle_kind`")
    ));
}

#[test]
fn static_shared_state_provider_becomes_non_originating_canonical_topology() {
    let parent_definition = definition(0x74);
    let child_definition = definition(0x75);
    let state_definition = definition(0x76);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x77,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    attach_shared_state_provider(
        &mut linked,
        state_definition,
        "crate::state::WorkbenchState",
    );
    let state = linked_shared_state_definition(
        state_definition,
        "acme.workbench",
        SHARED_STATE_RUNTIME_ABI_VERSION,
        &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
    );

    let adapted = adapt_component(
        &context_with_shared_state(parent_definition, &[state]),
        &linked,
    )
    .unwrap();
    let canonical = adapted
        .canonical()
        .expect("a static provider topology is lossless");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(
        body.shared_state_providers.get(&ProviderSiteId::new(0)),
        Some(&SharedStateProviderSite {
            child_edge: ComponentEdgeId::new(0),
            type_key: "acme.workbench".to_owned(),
            schema_abi: SemanticHash::from_bytes(
                linked_shared_state_definition(
                    state_definition,
                    "acme.workbench",
                    SHARED_STATE_RUNTIME_ABI_VERSION,
                    &[("mode", rsjs_transport::SharedStateSchemaCodec::String)],
                )
                .schema_abi_hash
            ),
        })
    );
    assert!(
        body.shared_state.is_empty() && body.effects.is_empty(),
        "declaring a provider must not acquire SharedState or originate browser behavior"
    );
    assert!(canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY));
}

#[test]
fn shared_state_provider_adapter_rejects_tampered_child_and_missing_schema_target() {
    let parent_definition = definition(0x78);
    let child_definition = definition(0x79);
    let state_definition = definition(0x7a);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x7b,
    );
    attach_direct_component_edge(
        &mut linked,
        child_definition,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    attach_shared_state_provider(
        &mut linked,
        state_definition,
        "crate::state::WorkbenchState",
    );

    let mut tampered = linked.clone();
    tampered.payload["transport"]["shared_state_provider_sites"][0]["child_source_edge_id"] =
        json!(hex_record(&[0xdd; 32]));
    let error = adapt_component(&context(Some(parent_definition)), &tampered)
        .expect_err("a provider claim cannot move to another child source edge");
    assert!(error.to_string().contains("claims child source edge"));

    let error = adapt_component(&context(Some(parent_definition)), &linked)
        .expect_err("a provider witness target must have an exact SharedState schema");
    assert!(error.to_string().contains("resolves to missing definition"));
}

#[test]
fn direct_component_source_edges_cannot_be_swapped_across_ordinals() {
    let parent_definition = definition(0x31);
    let first_child = definition(0x32);
    let second_child = definition(0x33);
    let mut linked = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x41,
    );
    attach_direct_component_edge(
        &mut linked,
        first_child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    let second_site = {
        let descriptor = &mut linked.payload["component_descriptor"];
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        let site = nodes.len();
        nodes[0]["node"]["children"]
            .as_array_mut()
            .expect("fixture root children")
            .push(json!(site));
        nodes.push(json!({
            "id": site,
            "node": {
                "kind": "server_only",
                "source": "direct_component_site",
                "tokens": "SecondBadge { label: \"fixture\" }"
            }
        }));
        descriptor["counts"]["nodes"] = json!(nodes.len());
        site
    };
    let second_fields = [TransportDirectComponentField {
        name: "label",
        binding: TransportDirectComponentBinding::ServerOnly,
    }];
    let second_source = component_source_edge_id(
        &fingerprint,
        1,
        &TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::Composed,
            site_node: second_site as u32,
            lifecycle: TransportDirectComponentLifecycle::StaticSingle,
            has_rest: false,
            fields: &second_fields,
        },
    );
    let mut second_claim = linked.payload["transport"]["direct_component_edges"][0].clone();
    second_claim["authored_path"] = json!("SecondBadge");
    second_claim["local_ordinal"] = json!(1);
    second_claim["site_node"] = json!(second_site);
    second_claim["source_edge_id"] = json!(hex_record(&second_source));
    linked.payload["transport"]["direct_component_edges"]
        .as_array_mut()
        .expect("fixture direct edges")
        .push(second_claim);
    linked.component_edges.push(LinkedComponentEdge {
        source_edge: second_source,
        target: second_child,
    });

    let first_claim =
        linked.payload["transport"]["direct_component_edges"][0]["source_edge_id"].clone();
    let second_claim =
        linked.payload["transport"]["direct_component_edges"][1]["source_edge_id"].clone();
    linked.payload["transport"]["direct_component_edges"][0]["source_edge_id"] = second_claim;
    linked.payload["transport"]["direct_component_edges"][1]["source_edge_id"] = first_claim;

    let error = adapt_component(&context(Some(parent_definition)), &linked)
        .expect_err("two valid source-edge IDs may not be swapped across syntax ordinals");
    assert!(
        error
            .to_string()
            .contains("direct component local ordinal 0")
            && error.to_string().contains("expected deterministic edge"),
        "unexpected swapped component-edge diagnostic: {error}"
    );
}

#[test]
fn direct_component_behavior_metadata_cannot_move_between_relocations() {
    let parent_definition = definition(0x34);
    let composed_child = definition(0x35);
    let island_child = definition(0x36);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([{
        "id": 0,
        "init": {"kind": "string", "value": "live"},
        "ty": {"kind": "string"}
    }]);
    descriptor["counts"]["signals"] = json!(1);
    let mut linked = component_payload(parent_definition, descriptor, 0x42);
    attach_direct_component_edge(
        &mut linked,
        composed_child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "kind": "live_value",
        "parent_source": {
            "id": 0,
            "namespace": "legacy_signal"
        },
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    recommit_direct_component_edge(&mut linked, 0);

    let island_site = {
        let descriptor = &mut linked.payload["component_descriptor"];
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        let site = nodes.len();
        nodes[0]["node"]["children"]
            .as_array_mut()
            .expect("fixture root children")
            .push(json!(site));
        nodes.push(json!({
            "id": site,
            "node": {
                "kind": "server_only",
                "source": "explicit_island_boundary",
                "tokens": "IslandBadge { label: \"static\" }"
            }
        }));
        descriptor["counts"]["nodes"] = json!(nodes.len());
        site
    };
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    let island_fields = [TransportDirectComponentField {
        name: "label",
        binding: TransportDirectComponentBinding::ServerOnly,
    }];
    let island_source = component_source_edge_id(
        &fingerprint,
        1,
        &TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::ExplicitIsland,
            site_node: island_site as u32,
            lifecycle: TransportDirectComponentLifecycle::StaticSingle,
            has_rest: false,
            fields: &island_fields,
        },
    );
    let mut island_claim = linked.payload["transport"]["direct_component_edges"][0].clone();
    island_claim["authored_path"] = json!("IslandBadge");
    island_claim["boundary"] = json!("explicit_island");
    island_claim["fields"][0]["binding"] = json!({
        "kind": "server_only",
        "proof": "visible_syntax_has_no_client_dependency_v1"
    });
    island_claim["local_ordinal"] = json!(1);
    island_claim["site_node"] = json!(island_site);
    island_claim["source_edge_id"] = json!(hex_record(&island_source));
    linked.payload["transport"]["direct_component_edges"]
        .as_array_mut()
        .expect("fixture direct edges")
        .push(island_claim);
    linked.component_edges.push(LinkedComponentEdge {
        source_edge: island_source,
        target: island_child,
    });

    adapt_component(&context(Some(parent_definition)), &linked)
        .expect("the authenticated baseline edge tuple must pass transport validation");

    let mut tampered = linked;
    for field in ["boundary", "fields", "has_rest", "lifecycle", "site_node"] {
        let first = tampered.payload["transport"]["direct_component_edges"][0][field].clone();
        let second = tampered.payload["transport"]["direct_component_edges"][1][field].clone();
        tampered.payload["transport"]["direct_component_edges"][0][field] = second;
        tampered.payload["transport"]["direct_component_edges"][1][field] = first;
    }
    let error = adapt_component(&context(Some(parent_definition)), &tampered).expect_err(
        "a live composed tuple and a static island tuple may not move between valid relocations",
    );
    assert!(
        error
            .to_string()
            .contains("direct component local ordinal 0")
            && error.to_string().contains("expected deterministic edge"),
        "unexpected behavior-tuple commitment diagnostic: {error}"
    );
}

#[test]
fn direct_component_live_field_sources_cannot_swap_between_relocations() {
    let parent_definition = definition(0x37);
    let first_child = definition(0x38);
    let second_child = definition(0x39);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["signals"] = json!([
        {
            "id": 0,
            "init": {"kind": "string", "value": "first"},
            "ty": {"kind": "string"}
        },
        {
            "id": 1,
            "init": {"kind": "string", "value": "second"},
            "ty": {"kind": "string"}
        }
    ]);
    descriptor["counts"]["signals"] = json!(2);
    let mut linked = component_payload(parent_definition, descriptor, 0x43);
    attach_direct_component_edge(
        &mut linked,
        first_child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"][0]["binding"] = json!({
        "kind": "live_value",
        "parent_source": {"id": 0, "namespace": "legacy_signal"},
        "proof": "exact_signal_read_v1",
        "source_form": "direct_signal_read"
    });
    recommit_direct_component_edge(&mut linked, 0);

    let second_site = {
        let descriptor = &mut linked.payload["component_descriptor"];
        let nodes = descriptor["view_tree"]["nodes"]
            .as_array_mut()
            .expect("fixture view nodes");
        let site = nodes.len();
        nodes[0]["node"]["children"]
            .as_array_mut()
            .expect("fixture root children")
            .push(json!(site));
        nodes.push(json!({
            "id": site,
            "node": {
                "kind": "server_only",
                "source": "direct_component_site",
                "tokens": "SecondBadge { value: second.get() }"
            }
        }));
        descriptor["counts"]["nodes"] = json!(nodes.len());
        site
    };
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint")
        .to_owned();
    let second_fields = [TransportDirectComponentField {
        name: "label",
        binding: TransportDirectComponentBinding::LiveValue {
            signal: 1,
            source: TransportDirectLiveValueSource::DirectSignalRead,
        },
    }];
    let second_source = component_source_edge_id(
        &fingerprint,
        1,
        &TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::Composed,
            site_node: second_site as u32,
            lifecycle: TransportDirectComponentLifecycle::StaticSingle,
            has_rest: false,
            fields: &second_fields,
        },
    );
    let mut second_claim = linked.payload["transport"]["direct_component_edges"][0].clone();
    second_claim["authored_path"] = json!("SecondBadge");
    second_claim["fields"][0]["binding"]["parent_source"]["id"] = json!(1);
    second_claim["local_ordinal"] = json!(1);
    second_claim["site_node"] = json!(second_site);
    second_claim["source_edge_id"] = json!(hex_record(&second_source));
    linked.payload["transport"]["direct_component_edges"]
        .as_array_mut()
        .expect("fixture direct edges")
        .push(second_claim);
    linked.component_edges.push(LinkedComponentEdge {
        source_edge: second_source,
        target: second_child,
    });

    adapt_component(&context(Some(parent_definition)), &linked)
        .expect("the authenticated distinct live sources must adapt");

    let first_fields = linked.payload["transport"]["direct_component_edges"][0]["fields"].clone();
    let second_fields = linked.payload["transport"]["direct_component_edges"][1]["fields"].clone();
    linked.payload["transport"]["direct_component_edges"][0]["fields"] = second_fields;
    linked.payload["transport"]["direct_component_edges"][1]["fields"] = first_fields;

    let error = adapt_component(&context(Some(parent_definition)), &linked)
        .expect_err("two valid live field tuples may not cross-wire compatible child relocations");
    assert!(
        error
            .to_string()
            .contains("direct component local ordinal 0")
            && error.to_string().contains("expected deterministic edge"),
        "unexpected live-field commitment diagnostic: {error}"
    );
}

#[test]
fn component_fingerprint_and_edge_sets_cannot_cross_swap_parent_definitions() {
    let unit = CargoUnitId::from_bytes([0x45; 32]);
    let fingerprint_a = format!("sha256:{}", "a1".repeat(32));
    let fingerprint_b = format!("sha256:{}", "b2".repeat(32));
    let parent_a = TransportDefinitionId {
        unit,
        record: component_record_id(&fingerprint_a),
    };
    let parent_b = TransportDefinitionId {
        unit,
        record: component_record_id(&fingerprint_b),
    };
    let mut component_a = component_payload(
        parent_a,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x48,
    );
    component_a.payload["transport"]["component_input_fingerprint"] = json!(fingerprint_a);
    attach_direct_component_edge(
        &mut component_a,
        definition(0x46),
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    let mut component_b = component_payload(
        parent_b,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x49,
    );
    component_b.payload["transport"]["component_input_fingerprint"] = json!(fingerprint_b);
    attach_direct_component_edge(
        &mut component_b,
        definition(0x47),
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );

    for field in [
        "component_input_fingerprint",
        "component_field_witnesses",
        "direct_component_edges",
        "reachable_helper_edges",
        "shared_state_acquisitions",
    ] {
        let value_a = component_a.payload["transport"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        let value_b = component_b.payload["transport"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        component_a.payload["transport"]
            .as_object_mut()
            .unwrap()
            .insert(field.to_owned(), value_b);
        component_b.payload["transport"]
            .as_object_mut()
            .unwrap()
            .insert(field.to_owned(), value_a);
    }
    std::mem::swap(
        &mut component_a.component_edges,
        &mut component_b.component_edges,
    );

    let error = adapt_component(&context(Some(parent_a)), &component_a).expect_err(
        "another same-unit component's self-consistent fingerprint and edge set \
             must not authenticate this parent definition",
    );
    assert!(
        error
            .to_string()
            .contains("component_input_fingerprint derives definition record")
            && error
                .to_string()
                .contains("relocation-linked component definition"),
        "unexpected cross-swapped parent diagnostic: {error}"
    );
}

#[test]
fn immutable_static_slot_field_and_independent_edge_become_canonical() {
    let child_definition = definition(0x2c);
    let mut child_descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "section"
    }));
    child_descriptor["fields"] = json!([{
        "id": 0,
        "kind": "static_slot",
        "name": "children",
        "permits_nested_owner": false,
        "source": "inferred_single_unconditional_server_slot",
        "span": "exact_one"
    }]);
    child_descriptor["view_tree"] = json!({
        "root": 0,
        "nodes": [
            {
                "id": 0,
                "node": {
                    "children": [1],
                    "kind": "element",
                    "payload_attrs": [],
                    "server_attrs": [],
                    "static_attrs": [],
                    "tag": "section"
                }
            },
            {
                "id": 1,
                "node": {
                    "kind": "server_only",
                    "source": "inferred_simple_ssr",
                    "tokens": "self . children"
                }
            }
        ]
    });
    child_descriptor["counts"]["fields"] = json!(1);
    child_descriptor["counts"]["nodes"] = json!(2);
    let child = component_payload(child_definition, child_descriptor, 0x2d);
    let child_result = adapt_component(&context(Some(child_definition)), &child).unwrap();
    let child_canonical = child_result
        .canonical()
        .expect("an immutable static-only slot has no browser field witness");
    let DescriptorBody::Component(child_body) = &child_canonical.envelope.body else {
        panic!("expected child component body");
    };
    assert_eq!(
        child_body.fields.get(&ComponentFieldId::new(0)),
        Some(&ComponentFieldSchema {
            source_name: "children".to_owned(),
            role: ComponentFieldRole::Slot,
            type_witness: None,
            crossing_hook: None,
            client_value: None,
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Immutable {
                permits_nested_owner: false,
            },
        })
    );
    assert!(child_canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::STATIC_SLOT_CONTRACTS));

    let parent_definition = definition(0x2e);
    let mut parent = component_payload(
        parent_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x2f,
    );
    attach_direct_component_edge(
        &mut parent,
        child_definition,
        "explicit_island",
        "static_single",
        false,
        "immutable_static_slot_exact_one",
        "direct_fully_static_single_root_html_macro_v1",
    );
    parent.payload["transport"]["direct_component_edges"][0]["fields"][0]["name"] =
        json!("children");
    recommit_direct_component_edge(&mut parent, 0);

    let parent_result = adapt_component(&context(Some(parent_definition)), &parent).unwrap();
    let parent_canonical = parent_result
        .canonical()
        .expect("a proven Exact(1) static slot is a lossless independent edge");
    let DescriptorBody::Component(parent_body) = &parent_canonical.envelope.body else {
        panic!("expected parent component body");
    };
    let edge = parent_body
        .direct_components
        .get(&ComponentEdgeId::new(0))
        .expect("canonical independent edge");
    assert_eq!(edge.boundary, ComponentBoundary::IndependentIsland);
    assert_eq!(
        edge.bindings.get("children"),
        Some(&ComponentFieldBinding {
            source: ComponentBindingSource::StaticSlot {
                span: SpanCardinality::Exact(1),
            },
            origin: OriginId::new(0),
        })
    );
    assert!(parent_canonical
        .envelope
        .header
        .capabilities
        .contains(CapabilitySet::STATIC_SLOT_CONTRACTS));
}

#[test]
fn richer_direct_component_sites_remain_fail_closed() {
    let cases = [
        (
            "rest syntax",
            "composed",
            "static_single",
            true,
            "server_only",
            "visible_syntax_has_no_client_dependency_v1",
        ),
        (
            "unproven binding",
            "composed",
            "static_single",
            false,
            "unsupported",
            "not_proven",
        ),
    ];

    for (label, boundary, lifecycle, has_rest, binding_kind, binding_proof) in cases {
        let parent_definition = definition(0x1c);
        let child_definition = definition(0x1d);
        let mut linked = component_payload(
            parent_definition,
            static_descriptor(json!({
                "children": [],
                "kind": "element",
                "payload_attrs": [],
                "server_attrs": [],
                "static_attrs": [],
                "tag": "main"
            })),
            0x2b,
        );
        attach_direct_component_edge(
            &mut linked,
            child_definition,
            boundary,
            lifecycle,
            has_rest,
            binding_kind,
            binding_proof,
        );
        let AbiBridgeResult::Unsupported(report) =
            adapt_component(&context(Some(parent_definition)), &linked).unwrap()
        else {
            panic!("{label} must not enter canonical linking");
        };
        assert!(
            report
                .gaps
                .contains(&CompatibilityGap::DirectComponentBindingMetadataMissing { edges: 1 }),
            "{label}: {report:?}"
        );
    }
}

#[test]
fn repeated_and_nested_component_sites_have_focused_compatibility_gaps() {
    let parent_definition = definition(0x3d);
    let child_definition = definition(0x3e);
    let fixture = || {
        let mut linked = component_payload(
            parent_definition,
            static_descriptor(json!({
                "children": [],
                "kind": "element",
                "payload_attrs": [],
                "server_attrs": [],
                "static_attrs": [],
                "tag": "main"
            })),
            0x3f,
        );
        attach_direct_component_edge(
            &mut linked,
            child_definition,
            "composed",
            "static_single",
            false,
            "server_only",
            "visible_syntax_has_no_client_dependency_v1",
        );
        linked
    };

    let mut repeated = fixture();
    place_direct_site_in_loop(&mut repeated);
    let AbiBridgeResult::Unsupported(report) =
        adapt_component(&context(Some(parent_definition)), &repeated).unwrap()
    else {
        panic!("repeated component sites need a keyed instance ABI");
    };
    assert_eq!(
        report.gaps,
        vec![CompatibilityGap::RepeatedDirectComponentSites { edges: vec![0] }]
    );

    let mut nested = fixture();
    place_direct_site_in_nested_conditionals(&mut nested);
    let AbiBridgeResult::Unsupported(report) =
        adapt_component(&context(Some(parent_definition)), &nested).unwrap()
    else {
        panic!("nested conditional component sites remain unresolved");
    };
    assert!(
        report
            .gaps
            .contains(&CompatibilityGap::UnknownDirectComponentSites { edges: vec![0] }),
        "{report:?}"
    );
    assert!(
        !report
            .gaps
            .contains(&CompatibilityGap::DirectComponentBindingMetadataMissing { edges: 1 }),
        "{report:?}"
    );
}

#[test]
fn client_visible_component_fields_require_exact_rustc_witnesses_and_live_targets() {
    let definition = definition(0x19);
    let mut descriptor = static_descriptor(json!({
        "children": [],
        "kind": "element",
        "payload_attrs": [],
        "server_attrs": [],
        "static_attrs": [],
        "tag": "main"
    }));
    descriptor["fields"] = json!([{
        "crossing_id": 0,
        "id": 0,
        "kind": "prop",
        "name": "count",
        "source": "signal_initializer",
        "source_anchor": {"start_column": 21, "start_line": 7},
        "value_id": 0
    }]);
    descriptor["inbound_inputs"] = json!({
        "crossings": [{
            "field_id": 0,
            "id": 0,
            "kind": "value",
            "source_anchor": {"start_column": 21, "start_line": 7}
        }],
        "node_ref_fields": [],
        "projection_fields": [],
        "prop_sites": [
            {
                "crossing_id": 0,
                "field_id": 0,
                "id": 0,
                "source_anchor": {"start_column": 21, "start_line": 7},
                "target": {"kind": "signal_seed", "signal_id": 0},
                "temporal": {"kind": "seed_once", "site_id": 0},
                "value_id": 0
            },
            {
                "crossing_id": 0,
                "field_id": 0,
                "id": 1,
                "source_anchor": {"start_column": 27, "start_line": 8},
                "target": {"kind": "live_signal", "signal_id": 1},
                "temporal": {"kind": "live_read"},
                "value_id": 0
            },
            {
                "crossing_id": 0,
                "field_id": 0,
                "id": 2,
                "source_anchor": {"start_column": 27, "start_line": 9},
                "target": {"kind": "live_signal", "signal_id": 2},
                "temporal": {"kind": "query_key_dependency"},
                "value_id": 0
            }
        ],
        "values": [{
            "field_id": 0,
            "id": 0,
            "source_anchor": {"start_column": 21, "start_line": 7}
        }]
    });
    descriptor["signals"] = json!([
        {
            "id": 0,
            "init": {"kind": "int", "value": 0},
            "ty": {"kind": "unknown"}
        },
        {
            "id": 1,
            "init": {"kind": "null", "value": null},
            "ty": {"kind": "unknown"}
        },
        {
            "id": 2,
            "init": {"kind": "null", "value": null},
            "ty": {"kind": "unknown"}
        }
    ]);
    descriptor["exprs"] = Value::Array(
        [
            ClientExpr::QueryKey(vec![ClientExpr::SignalRead(rsjs_ir::SignalId(2))]),
            ClientExpr::string("initial"),
            ClientExpr::string("/api/items"),
            ClientExpr::QueryRead {
                id: rsjs_ir::QueryId(0),
                accessor: rsjs_ir::QueryAccessor::Data,
            },
        ]
        .into_iter()
        .enumerate()
        .map(|(id, expression)| {
            json!({
                "id": id,
                "expr": serde_json::from_str::<Value>(&client_expr_json(&expression))
                    .unwrap()
            })
        })
        .collect(),
    );
    descriptor["queries"] = json!([{
        "client_fetcher": 2,
        "data_ty": {"kind": "string"},
        "id": 0,
        "key_expr": 0,
        "options": {
            "download_progress": false,
            "interval_ms": null,
            "refetch_on_focus": false,
            "retry_count": 0,
            "stale_time_ms": 0,
            "stream_text": false
        },
        "server_initial": 1
    }]);
    descriptor["view_tree"]["nodes"][0]["node"]["children"] = json!([1]);
    descriptor["view_tree"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": 1, "node": {"kind": "text_binding_slot"}}));
    descriptor["bindings"]["text"] = json!([{"expr": 3, "node": 1}]);
    descriptor["client_origin_permission"] = json!("origin_capable");
    descriptor["counts"]["crossings"] = json!(1);
    descriptor["counts"]["fields"] = json!(1);
    descriptor["counts"]["exprs"] = json!(4);
    descriptor["counts"]["nodes"] = json!(2);
    descriptor["counts"]["prop_sites"] = json!(3);
    descriptor["counts"]["signals"] = json!(3);
    descriptor["counts"]["values"] = json!(1);
    let mut linked = component_payload(definition, descriptor, 0x29);
    let missing = adapt_component(&context(Some(definition)), &linked)
        .expect_err("payload v9 must not omit a client-visible field record");
    assert!(
        missing
            .to_string()
            .contains("field witnesses must be one-to-one"),
        "unexpected missing-witness diagnostic: {missing}"
    );

    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = rsjs_transport::component_field_record_id(
        fingerprint,
        0,
        0,
        rsjs_transport::TransportComponentFieldRole::Value,
        rsjs_transport::TransportCallbackShape::NotCallback,
    );
    linked.payload["transport"]["component_field_witnesses"] = json!([{
        "callback_shape": "not_callback",
        "crossing_hook_id": 0,
        "field_id": 0,
        "field_name": "count",
        "field_record_id": hex_record(&field_record),
        "role": "value"
    }]);
    linked
        .field_witnesses
        .push(crate::rsjs::link::LinkedComponentFieldWitness {
            field_record,
            target: definition,
        });
    let descriptor = linked.payload["component_descriptor"].as_object().unwrap();
    let inbound = validate_inbound_inputs(definition, descriptor).unwrap();
    assert_eq!(
        inbound.prop_uses[0].temporal,
        TemporalPropMode::SeedOnce(PropSiteId::new(0))
    );
    assert_eq!(inbound.prop_uses[1].temporal, TemporalPropMode::LiveRead);
    assert_eq!(
        inbound.prop_uses[2].temporal,
        TemporalPropMode::QueryKeyDependency
    );
    let transport = linked.payload["transport"].as_object().unwrap();
    let layout = ComponentWitnessLayout::new(definition, 0, inbound.fields.len(), 0).unwrap();
    let witnesses =
        validate_component_field_witness_set(&linked, transport, &inbound.fields, &layout).unwrap();
    let value_layout =
        ComponentValueLayout::new(definition, &inbound.fields, descriptor, 0).unwrap();
    let fields =
        adapt_component_fields(definition, descriptor, &inbound, &witnesses, &value_layout)
            .unwrap();
    assert_eq!(
        fields.canonical.get(&ComponentFieldId::new(0)),
        Some(&ComponentFieldSchema {
            source_name: "count".to_owned(),
            role: ComponentFieldRole::Value,
            type_witness: Some(WitnessRecordId::new(1)),
            crossing_hook: Some(CrossingHookId::new(0)),
            client_value: Some(LocalValueId::new(0)),
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Unsupported,
        })
    );
    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("the inbound seed and local signal should lower canonically");
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.fields.get(&ComponentFieldId::new(0)),
        fields.canonical.get(&ComponentFieldId::new(0))
    );
    assert!(
        body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Data
                && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
                && edge.to == DependencyEndpoint::Value(LocalValueId::new(1))
        }),
        "SeedOnce must preserve an explicit inbound-value-to-local-signal edge"
    );
    assert!(
        body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Data
                && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
                && edge.to == DependencyEndpoint::Value(LocalValueId::new(2))
        }),
        "LiveRead must preserve an explicit inbound-value-to-live-signal edge"
    );
    assert!(
        body.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Data
                && edge.from == DependencyEndpoint::Value(LocalValueId::new(0))
                && edge.to == DependencyEndpoint::Value(LocalValueId::new(3))
        }),
        "QueryKeyDependency must preserve an explicit inbound-value-to-live-signal edge"
    );
    let live_signal_origin = body
        .dependencies
        .values()
        .find_map(|edge| {
            (edge.from == DependencyEndpoint::Origin(edge.origin)
                && edge.to == DependencyEndpoint::Value(LocalValueId::new(2)))
            .then_some(edge.origin)
        })
        .expect("the live producer signal keeps a provenance origin");
    assert_eq!(
            body.origins.get(&live_signal_origin),
            Some(&OriginKind::SourceOnly),
            "an inbound LiveRead signal participates in a parent graph but is not a standalone client origin"
        );
    let seed_signal_origin = body
        .dependencies
        .values()
        .find_map(|edge| {
            (edge.from == DependencyEndpoint::Origin(edge.origin)
                && edge.to == DependencyEndpoint::Value(LocalValueId::new(1)))
            .then_some(edge.origin)
        })
        .expect("the SeedOnce producer signal keeps a provenance origin");
    assert_eq!(
        body.origins.get(&seed_signal_origin),
        Some(&OriginKind::WritableSignal),
        "a rustc-witnessed SeedOnce signal must still own writable child state"
    );
    let codegen = canonical
        .component_codegen()
        .expect("component codegen mapping");
    assert!(
        codegen.uses_queries,
        "a reachable query-key dependency must publish query runtime usage"
    );
    assert_eq!(
        codegen.inbound_fields,
        vec![ComponentCodegenInboundField {
            field_id: ComponentFieldId::new(0),
            producer_value_id: 0,
            canonical_value: LocalValueId::new(0),
            crossing_hook_id: CrossingHookId::new(0),
            callback_slot_id: None,
        }]
    );
    assert_eq!(
        codegen.inbound_targets,
        vec![
            ComponentCodegenInboundTarget {
                prop_site_id: PropSiteId::new(0),
                field_id: ComponentFieldId::new(0),
                producer_signal_id: 0,
                canonical_signal_value: LocalValueId::new(1),
            },
            ComponentCodegenInboundTarget {
                prop_site_id: PropSiteId::new(1),
                field_id: ComponentFieldId::new(0),
                producer_signal_id: 1,
                canonical_signal_value: LocalValueId::new(2),
            },
            ComponentCodegenInboundTarget {
                prop_site_id: PropSiteId::new(2),
                field_id: ComponentFieldId::new(0),
                producer_signal_id: 2,
                canonical_signal_value: LocalValueId::new(3),
            }
        ]
    );
    assert_eq!(
        codegen.signals,
        vec![
            ComponentCodegenSignal {
                producer_signal_id: 0,
                canonical_value: LocalValueId::new(1),
            },
            ComponentCodegenSignal {
                producer_signal_id: 1,
                canonical_value: LocalValueId::new(2),
            },
            ComponentCodegenSignal {
                producer_signal_id: 2,
                canonical_value: LocalValueId::new(3),
            }
        ]
    );

    let mut malformed = linked;
    malformed.payload["component_descriptor"]["inbound_inputs"]["prop_sites"][0]["temporal"]
        ["site_id"] = json!(9);
    let error = adapt_component(&context(Some(definition)), &malformed)
        .expect_err("SeedOnce must name its exact dense prop site");
    assert!(
        error.to_string().contains("expected 0"),
        "focused temporal validation should explain the mismatched site: {error}"
    );
}

#[test]
fn callback_field_shape_survives_canonical_adaptation() {
    let definition = definition(0x1c);
    let descriptor = |shape: &str| {
        let mut descriptor = static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "button"
        }));
        descriptor["callback_slots"] = json!([{
            "id": 0,
            "name": "on_change"
        }]);
        descriptor["fields"] = json!([{
            "callback_shape": shape,
            "callback_slot": 0,
            "crossing_id": 0,
            "dispatch": "resolve_current",
            "id": 0,
            "kind": "handler",
            "name": "on_change",
            "source": "callback_slot_usage",
            "source_anchor": {"start_column": 17, "start_line": 9},
            "value_id": 0
        }]);
        descriptor["inbound_inputs"] = json!({
            "crossings": [{
                "field_id": 0,
                "id": 0,
                "kind": "callback_resolve_current",
                "source_anchor": {"start_column": 17, "start_line": 9}
            }],
            "node_ref_fields": [],
            "projection_fields": [],
            "prop_sites": [],
            "values": [{
                "field_id": 0,
                "id": 0,
                "source_anchor": {"start_column": 17, "start_line": 9}
            }]
        });
        descriptor["counts"]["callback_slots"] = json!(1);
        descriptor["counts"]["crossings"] = json!(1);
        descriptor["counts"]["fields"] = json!(1);
        descriptor["counts"]["values"] = json!(1);
        descriptor
    };
    let field_witnesses = BTreeMap::from([(
        0,
        ValidatedComponentFieldWitness {
            field_record: [0x55; 32],
            witness: WitnessRecordId::new(1),
        },
    )]);

    let required_descriptor = descriptor("required");
    let required_inbound =
        validate_inbound_inputs(definition, required_descriptor.as_object().unwrap()).unwrap();
    let required_value_layout = ComponentValueLayout::new(
        definition,
        &required_inbound.fields,
        required_descriptor.as_object().unwrap(),
        0,
    )
    .unwrap();
    let required = adapt_component_fields(
        definition,
        required_descriptor.as_object().unwrap(),
        &required_inbound,
        &field_witnesses,
        &required_value_layout,
    )
    .unwrap();
    let optional_descriptor = descriptor("optional");
    let optional_inbound =
        validate_inbound_inputs(definition, optional_descriptor.as_object().unwrap()).unwrap();
    let optional_value_layout = ComponentValueLayout::new(
        definition,
        &optional_inbound.fields,
        optional_descriptor.as_object().unwrap(),
        0,
    )
    .unwrap();
    let optional = adapt_component_fields(
        definition,
        optional_descriptor.as_object().unwrap(),
        &optional_inbound,
        &field_witnesses,
        &optional_value_layout,
    )
    .unwrap();

    assert_eq!(
        required.canonical[&ComponentFieldId::new(0)].callback_shape,
        Some(ComponentCallbackShape::Required)
    );
    assert_eq!(
        optional.canonical[&ComponentFieldId::new(0)].callback_shape,
        Some(ComponentCallbackShape::Optional)
    );
    assert_ne!(
        rsjs_ir::abi::canonical_semantic_hash(&required.canonical),
        rsjs_ir::abi::canonical_semantic_hash(&optional.canonical),
        "callback optionality must remain semantic after transport adaptation"
    );

    let mut linked = component_payload(definition, required_descriptor, 0x2c);
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let field_record = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Callback,
        TransportCallbackShape::Required,
    );
    linked.payload["transport"]["component_field_witnesses"] = json!([{
        "callback_shape": "required",
        "crossing_hook_id": 0,
        "field_id": 0,
        "field_name": "on_change",
        "field_record_id": hex_record(&field_record),
        "role": "callback"
    }]);
    linked
        .field_witnesses
        .push(crate::rsjs::link::LinkedComponentFieldWitness {
            field_record,
            target: definition,
        });
    let adapted = adapt_component(&context(Some(definition)), &linked).unwrap();
    let codegen = adapted
        .canonical()
        .unwrap()
        .component_codegen()
        .expect("callback component codegen mapping");
    assert_eq!(
        codegen.inbound_fields,
        vec![ComponentCodegenInboundField {
            field_id: ComponentFieldId::new(0),
            producer_value_id: 0,
            canonical_value: LocalValueId::new(0),
            crossing_hook_id: CrossingHookId::new(0),
            callback_slot_id: Some(0),
        }]
    );
    assert_eq!(
        codegen.callback_slots,
        vec![ComponentCodegenCallbackSlot {
            producer_callback_slot_id: 0,
            canonical_value: LocalValueId::new(0),
        }]
    );
}

#[test]
fn direct_adapter_recomputes_component_field_record_ids() {
    let definition = definition(0x1a);
    let mut linked = component_payload(definition, client_value_descriptor(&["count"]), 0x2a);
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let expected = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Value,
        TransportCallbackShape::NotCallback,
    );
    let forged = [0xa5; 32];
    assert_ne!(forged, expected);
    attach_value_field_witnesses(&mut linked, &["count"], &[forged]);

    let error = adapt_component(&context(Some(definition)), &linked)
        .expect_err("matching forged payload/link values must not bypass ID derivation");
    assert!(matches!(
        error,
        AbiAdapterError::ComponentFieldRecordIdMismatch {
            definition: actual_definition,
            ref field,
            expected: ref actual_expected,
            actual: ref actual_record,
        } if actual_definition == definition
            && field == "count"
            && actual_expected == &hex_record(&expected)
            && actual_record == &hex_record(&forged)
    ));
}

#[test]
fn direct_adapter_rejects_duplicate_component_field_record_ids() {
    let definition = definition(0x1b);
    let mut linked = component_payload(
        definition,
        client_value_descriptor(&["count", "limit"]),
        0x2b,
    );
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .unwrap();
    let first = component_field_record_id(
        fingerprint,
        0,
        0,
        TransportComponentFieldRole::Value,
        TransportCallbackShape::NotCallback,
    );
    attach_value_field_witnesses(&mut linked, &["count", "limit"], &[first, first]);

    let error = adapt_component(&context(Some(definition)), &linked)
        .expect_err("one object record cannot witness two client-visible fields");
    assert!(matches!(
        error,
        AbiAdapterError::DuplicateComponentFieldRecordId {
            definition: actual_definition,
            ref first_field,
            ref second_field,
            field_record: ref actual_record,
        } if actual_definition == definition
            && first_field == "count"
            && second_field == "limit"
            && actual_record == &hex_record(&first)
    ));
}

#[test]
fn component_witness_layout_separates_every_category_and_checks_overflow() {
    let component_definition = definition(0x1d);
    let layout = ComponentWitnessLayout::from_counts(component_definition, 1, 2, 2).unwrap();
    let witnesses = [
        WitnessRecordId::new(0),
        layout.component(component_definition, 0).unwrap(),
        layout.field(component_definition, 0).unwrap(),
        layout.field(component_definition, 1).unwrap(),
        layout.helper(component_definition, 0).unwrap(),
        layout.helper(component_definition, 1).unwrap(),
    ];
    assert_eq!(
        witnesses,
        [
            WitnessRecordId::new(0),
            WitnessRecordId::new(1),
            WitnessRecordId::new(2),
            WitnessRecordId::new(3),
            WitnessRecordId::new(4),
            WitnessRecordId::new(5),
        ]
    );
    assert_eq!(
        witnesses.iter().copied().collect::<BTreeSet<_>>().len(),
        witnesses.len(),
        "definition, component, field, and helper witnesses must never collide"
    );

    let child = definition(0x2d);
    let first_helper = definition(0x2e);
    let second_helper = definition(0x2f);
    let mut linked = component_payload(component_definition, static_descriptor(Value::Null), 0x3d);
    linked.component_edges.push(LinkedComponentEdge {
        source_edge: [0xd1; 32],
        target: child,
    });
    let table = build_component_witness_table(
        &linked,
        &[ValidatedDirectComponentEdge {
            local_ordinal: 0,
            source_edge: [0xd1; 32],
            composed: true,
            has_rest: false,
            lifecycle: ValidatedDirectComponentLifecycle::StaticSingle,
            site_node: 0,
            fields: Vec::new(),
        }],
        &BTreeMap::from([
            (
                0,
                ValidatedComponentFieldWitness {
                    field_record: [0xd2; 32],
                    witness: layout.field(component_definition, 0).unwrap(),
                },
            ),
            (
                1,
                ValidatedComponentFieldWitness {
                    field_record: [0xd3; 32],
                    witness: layout.field(component_definition, 1).unwrap(),
                },
            ),
        ]),
        &[
            ValidatedComponentHelperEdge {
                local_ordinal: 3,
                source_edge: [0xd4; 32],
                target: first_helper,
            },
            ValidatedComponentHelperEdge {
                local_ordinal: 8,
                source_edge: [0xd5; 32],
                target: second_helper,
            },
        ],
        layout,
    )
    .unwrap();
    assert_eq!(table.len(), 6);
    assert_eq!(
        table[&WitnessRecordId::new(1)],
        ResolvedTransportWitness::ComponentEdge {
            source_edge: [0xd1; 32],
            target: child,
        }
    );
    assert_eq!(
        table[&WitnessRecordId::new(2)],
        ResolvedTransportWitness::ComponentField {
            field_record: [0xd2; 32],
            target: component_definition,
        }
    );
    assert_eq!(
        table[&WitnessRecordId::new(4)],
        ResolvedTransportWitness::HelperEdge {
            source_edge: [0xd4; 32],
            target: first_helper,
        }
    );

    let error =
        ComponentWitnessLayout::from_counts(component_definition, u64::from(u32::MAX), 1, 0)
            .expect_err("the allocator must fail before a category base wraps");
    assert!(
        error.to_string().contains("exhaust"),
        "overflow should produce a focused allocation diagnostic: {error}"
    );
    assert!(layout
        .helper(component_definition, 2)
        .expect_err("out-of-range helper ordinals must fail")
        .to_string()
        .contains("outside its allocated witness range"));
}

#[test]
fn component_helper_source_edges_cannot_be_swapped_across_ordinals() {
    let component_definition = definition(0x34);
    let first_helper = definition(0x35);
    let second_helper = definition(0x36);
    let mut linked = component_payload(
        component_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x44,
    );
    attach_component_helper_edges(&mut linked, &[(0, first_helper), (1, second_helper)]);

    let first_claim =
        linked.payload["transport"]["reachable_helper_edges"][0]["source_edge_id"].clone();
    let second_claim =
        linked.payload["transport"]["reachable_helper_edges"][1]["source_edge_id"].clone();
    linked.payload["transport"]["reachable_helper_edges"][0]["source_edge_id"] = second_claim;
    linked.payload["transport"]["reachable_helper_edges"][1]["source_edge_id"] = first_claim;

    let error = adapt_component(&context(Some(component_definition)), &linked)
        .expect_err("two valid helper-edge IDs may not be swapped across call ordinals");
    assert!(
        error
            .to_string()
            .contains("reachable helper local ordinal 0")
            && error.to_string().contains("expected deterministic edge"),
        "unexpected swapped component-helper diagnostic: {error}"
    );
}

#[test]
fn component_helpers_follow_local_ordinal_after_component_witnesses() {
    let parent = definition(0x1e);
    let child = definition(0x1f);
    let first_helper = definition(0x20);
    let second_helper = definition(0x21);
    let mut linked = component_payload(
        parent,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x2e,
    );
    attach_direct_component_edge(
        &mut linked,
        child,
        "composed",
        "static_single",
        false,
        "server_only",
        "visible_syntax_has_no_client_dependency_v1",
    );
    linked.payload["transport"]["direct_component_edges"][0]["fields"] = json!([]);
    recommit_direct_component_edge(&mut linked, 0);
    attach_component_helper_edges(&mut linked, &[(9, second_helper), (2, first_helper)]);
    let fingerprint = linked.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint");
    let first_source = component_helper_source_edge_id(fingerprint, 2);
    let second_source = component_helper_source_edge_id(fingerprint, 9);

    let canonical = adapt_component(&context(Some(parent)), &linked)
        .unwrap()
        .canonical()
        .expect("relocation-resolved helpers are representable")
        .clone();
    let DescriptorBody::Component(body) = &canonical.envelope.body else {
        panic!("expected component body");
    };
    assert_eq!(
        body.direct_components[&ComponentEdgeId::new(0)].witness,
        WitnessRecordId::new(1)
    );
    assert_eq!(
        body.direct_helpers,
        BTreeMap::from([
            (HelperCallEdgeId::new(0), WitnessRecordId::new(2)),
            (HelperCallEdgeId::new(1), WitnessRecordId::new(3)),
        ])
    );
    assert_eq!(
        canonical.witnesses[&WitnessRecordId::new(2)],
        ResolvedTransportWitness::HelperEdge {
            source_edge: first_source,
            target: first_helper,
        },
        "sparse producer ordinals are normalized in increasing ordinal order"
    );
    assert_eq!(
        canonical.witnesses[&WitnessRecordId::new(3)],
        ResolvedTransportWitness::HelperEdge {
            source_edge: second_source,
            target: second_helper,
        }
    );
}

#[test]
fn component_helper_witnesses_have_complete_semantic_link_coverage() {
    let component_definition = definition(0x22);
    let first_helper_definition = definition(0x23);
    let second_helper_definition = definition(0x24);
    let mut component = component_payload(
        component_definition,
        static_descriptor(json!({
            "children": [],
            "kind": "element",
            "payload_attrs": [],
            "server_attrs": [],
            "static_attrs": [],
            "tag": "main"
        })),
        0x32,
    );
    attach_component_helper_edges(
        &mut component,
        &[(8, second_helper_definition), (3, first_helper_definition)],
    );
    let fingerprint = component.payload["transport"]["component_input_fingerprint"]
        .as_str()
        .expect("fixture component fingerprint");
    let first_source = component_helper_source_edge_id(fingerprint, 3);
    let second_source = component_helper_source_edge_id(fingerprint, 8);
    let first_helper = helper_payload(first_helper_definition, 0x33);
    let second_helper = helper_payload(second_helper_definition, 0x34);
    let context = context(Some(component_definition))
        .with_helper_identity(
            first_helper_definition,
            ResolvedSemanticPath::new("acme-web", "acme_web", ["resolved", "helpers"], "first"),
        )
        .with_helper_identity(
            second_helper_definition,
            ResolvedSemanticPath::new("acme-web", "acme_web", ["resolved", "helpers"], "second"),
        );
    let adapted = AdaptedTransportAbi {
        components: vec![adapt_component(&context, &component).unwrap()],
        helpers: vec![
            adapt_helper(&context, &first_helper).unwrap(),
            adapt_helper(&context, &second_helper).unwrap(),
        ],
    };

    let plan = crate::rsjs::semantic_link::link_semantic_plan(
        LinkUnitId::derive("component-helper-witness-coverage"),
        &adapted,
    )
    .expect("every canonical helper witness should be claimed exactly once");
    assert_eq!(plan.components.len(), 1);
    assert_eq!(
        plan.components[0]
            .helper_edges
            .iter()
            .map(|edge| (edge.call.index(), edge.source_edge, edge.target_definition))
            .collect::<Vec<_>>(),
        vec![
            (0, first_source, first_helper_definition),
            (1, second_source, second_helper_definition),
        ]
    );
    assert_eq!(plan.components[0].helper_closure.len(), 2);
    assert_eq!(plan.reachable_helpers.len(), 2);
}

#[test]
fn standalone_helper_source_edges_cannot_be_swapped_across_ordinals() {
    let helper_definition = definition(0x37);
    let first_target = definition(0x38);
    let second_target = definition(0x39);
    let first_source = helper_direct_call_source_edge_id(helper_definition.record, 0);
    let second_source = helper_direct_call_source_edge_id(helper_definition.record, 1);
    let mut linked = helper_payload(helper_definition, 0x47);
    linked.payload["direct_calls"] = json!([
        {
            "authored_path": "helpers::first",
            "ordinal": 0,
            "source_edge_id": hex_record(&first_source)
        },
        {
            "authored_path": "helpers::second",
            "ordinal": 1,
            "source_edge_id": hex_record(&second_source)
        }
    ]);
    linked.helper_edges = vec![
        LinkedHelperEdge {
            source_edge: first_source,
            target: first_target,
        },
        LinkedHelperEdge {
            source_edge: second_source,
            target: second_target,
        },
    ];

    let first_claim = linked.payload["direct_calls"][0]["source_edge_id"].clone();
    let second_claim = linked.payload["direct_calls"][1]["source_edge_id"].clone();
    linked.payload["direct_calls"][0]["source_edge_id"] = second_claim;
    linked.payload["direct_calls"][1]["source_edge_id"] = first_claim;

    let error = adapt_helper(&context(None), &linked)
        .expect_err("two valid helper-call IDs may not be swapped across call ordinals");
    assert!(
        error.to_string().contains("direct helper call ordinal 0")
            && error.to_string().contains("expected deterministic edge"),
        "unexpected swapped standalone-helper diagnostic: {error}"
    );
}

#[test]
fn helper_carries_its_exact_signature_and_relocation_witnesses() {
    let helper_definition = definition(0x17);
    let target = definition(0x18);
    let source_edge = helper_direct_call_source_edge_id(helper_definition.record, 0);
    let mut linked = helper_payload(helper_definition, 0x28);
    linked.payload["direct_calls"] = json!([{
        "authored_path": "renamed_helper",
        "ordinal": 0,
        "source_edge_id": hex_record(&source_edge)
    }]);
    linked.payload["body_ir"] = json!({
        "kind": "call",
        "function": {
            "kind": "unresolved",
            "path": "renamed_helper"
        },
        "args": []
    });
    linked.helper_edges.push(LinkedHelperEdge {
        source_edge,
        target,
    });
    let context = context(None).with_helper_identity(
        helper_definition,
        ResolvedSemanticPath::new("acme-web", "acme_web", ["labels"], "pluralize"),
    );
    let adapted = adapt_helper(&context, &linked).unwrap();
    let canonical = adapted
        .canonical()
        .expect("complete helper payload should convert");
    let DescriptorBody::Helper(body) = &canonical.envelope.body else {
        panic!("expected helper body");
    };
    assert!(
        canonical.component_codegen().is_none(),
        "helper descriptors must not carry component codegen coordinates"
    );
    let codegen = canonical
        .helper_codegen()
        .expect("helper descriptors carry validated final codegen data");
    assert_eq!(
        codegen.signature(),
        &HelperCodegenSignature {
            params: vec![HelperCodegenParam {
                name: "count".to_owned(),
                rust_type: "u32".to_owned(),
            }],
            return_type: "u32".to_owned(),
        }
    );
    assert_eq!(
        codegen.source_ir_hash(),
        canonical_legacy_json_hash(
            HELPER_CLIENT_IR_JSON_HASH_DOMAIN,
            &linked.payload["body_ir"],
        )
    );
    assert_eq!(codegen.body_ir_hash(), codegen.body().semantic_hash());
    assert_eq!(
        codegen.body().canonical_json(),
        json!({
            "kind": "call",
            "function": {
                "kind": "helper",
                "edge": 0,
                "authored_path": "renamed_helper",
                "name": "renamed_helper"
            },
            "args": []
        }),
        "the codegen bridge must retain the structurally resolved edge, not the unresolved path"
    );
    validate_helper_codegen_bridge(helper_definition, codegen, body).unwrap();

    let tampered = HelperCodegenBridge::try_new(
        helper_definition,
        codegen.signature().clone(),
        codegen.source_ir_hash(),
        HelperCodegenBody::Expr(ClientExpr::Literal(rsjs_ir::Value::Int(7))),
        body,
    )
    .expect_err("a body that omits a committed helper edge is not constructible");
    assert!(tampered.contains("mismatched helper edges"), "{tampered}");

    let stale_source = HelperCodegenBridge::try_new(
        helper_definition,
        codegen.signature().clone(),
        SemanticHash::derive("forged-helper-source-ir"),
        codegen.body().clone(),
        body,
    )
    .expect_err("a source hash not authenticated by the descriptor is not constructible");
    assert!(
        stale_source.contains("source IR hash")
            && stale_source.contains("does not match descriptor hash"),
        "{stale_source}"
    );

    let mut stale_signature = codegen.signature().clone();
    stale_signature.return_type = "String".to_owned();
    let stale_signature = HelperCodegenBridge::try_new(
        helper_definition,
        stale_signature,
        codegen.source_ir_hash(),
        codegen.body().clone(),
        body,
    )
    .expect_err("a signature not authenticated by the descriptor is not constructible");
    assert!(
        stale_signature.contains("signature hash")
            && stale_signature.contains("does not match descriptor hash"),
        "{stale_signature}"
    );
    assert_eq!(
        body.signature_hash,
        canonical_legacy_json_hash(
            HELPER_SIGNATURE_JSON_HASH_DOMAIN,
            &linked.payload["signature"],
        )
    );
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(0)),
        Some(&ResolvedTransportWitness::Definition(helper_definition))
    );
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(1)),
        Some(&ResolvedTransportWitness::HelperEdge {
            source_edge,
            target,
        })
    );
    assert_eq!(
        body.direct_helpers.get(&HelperCallEdgeId::new(0)),
        Some(&WitnessRecordId::new(1))
    );
}

#[test]
fn helper_call_relocations_follow_producer_ordinals_not_linked_record_order() {
    let helper_definition = definition(0x81);
    let first_target = definition(0x82);
    let second_target = definition(0x83);
    let first_source = helper_direct_call_source_edge_id(helper_definition.record, 0);
    let second_source = helper_direct_call_source_edge_id(helper_definition.record, 1);
    let mut linked = helper_payload(helper_definition, 0x45);
    linked.payload["direct_calls"] = json!([
        {
            "authored_path": "helpers::first",
            "ordinal": 0,
            "source_edge_id": hex_record(&first_source)
        },
        {
            "authored_path": "helpers::second",
            "ordinal": 1,
            "source_edge_id": hex_record(&second_source)
        }
    ]);
    linked.payload["body_ir"] = json!({
        "kind": "block",
        "stmts": [
            {
                "kind": "let",
                "local": 1,
                "name": "first_result",
                "ty": {"kind": "unknown"},
                "value": {
                    "kind": "call",
                    "function": {
                        "kind": "unresolved",
                        "path": "helpers::first"
                    },
                    "args": []
                }
            },
            {
                "kind": "return",
                "value": {
                    "kind": "call",
                    "function": {
                        "kind": "unresolved",
                        "path": "helpers::second"
                    },
                    "args": []
                }
            }
        ]
    });
    // Linked records may arrive independently of producer call-site order.
    linked.helper_edges = vec![
        LinkedHelperEdge {
            source_edge: second_source,
            target: second_target,
        },
        LinkedHelperEdge {
            source_edge: first_source,
            target: first_target,
        },
    ];
    let context = context(None).with_helper_identity(
        helper_definition,
        ResolvedSemanticPath::new("acme-web", "acme_web", ["helpers"], "caller"),
    );

    let adapted = adapt_helper(&context, &linked).unwrap();
    let canonical = adapted.canonical().expect("helper should convert");
    let DescriptorBody::Helper(body) = &canonical.envelope.body else {
        panic!("expected helper body");
    };
    assert_eq!(
        body.direct_helpers
            .iter()
            .map(|(call, witness)| (call.index(), witness.index()))
            .collect::<Vec<_>>(),
        vec![(0, 1), (1, 2)]
    );
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(1)),
        Some(&ResolvedTransportWitness::HelperEdge {
            source_edge: first_source,
            target: first_target,
        })
    );
    assert_eq!(
        canonical.witnesses.get(&WitnessRecordId::new(2)),
        Some(&ResolvedTransportWitness::HelperEdge {
            source_edge: second_source,
            target: second_target,
        })
    );
}

#[test]
fn malformed_and_noncanonical_helper_client_ir_fail_closed() {
    let helper_definition = definition(0x84);
    let mut malformed = helper_payload(helper_definition, 0x46);
    malformed.payload["body_ir"] = json!({"kind": "not_client_ir"});
    let error = adapt_helper(&context(None), &malformed).unwrap_err();
    assert!(
        error.to_string().contains("invalid Client IR v3"),
        "{error}"
    );

    let mut noncanonical = helper_payload(helper_definition, 0x46);
    noncanonical.payload["body_ir"]["ignored_semantics"] = json!(true);
    let error = adapt_helper(&context(None), &noncanonical).unwrap_err();
    assert!(
        error.to_string().contains("not canonical Client IR v3"),
        "{error}"
    );
}

#[test]
fn malformed_helper_signatures_fail_closed_before_codegen_bridge_creation() {
    let helper_definition = definition(0x91);
    let mut duplicate = helper_payload(helper_definition, 0x51);
    duplicate.payload["signature"]["params"] = json!([
        {"name": "value", "rust_type": "u32"},
        {"name": "value", "rust_type": "u64"}
    ]);
    let error = adapt_helper(&context(None), &duplicate).unwrap_err();
    assert!(
        error.to_string().contains("appears more than once"),
        "{error}"
    );

    let mut empty = helper_payload(helper_definition, 0x51);
    empty.payload["signature"]["return_type"] = json!("");
    let error = adapt_helper(&context(None), &empty).unwrap_err();
    assert!(error.to_string().contains("must not be empty"), "{error}");

    let mut unsupported = helper_payload(helper_definition, 0x51);
    unsupported.payload["signature"]["params"][0]["rust_type"] = json!("usize");
    let error = adapt_helper(&context(None), &unsupported).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported normalized standalone-helper parameter type `usize`"),
        "{error}"
    );

    let mut borrowed_return = helper_payload(helper_definition, 0x51);
    borrowed_return.payload["signature"]["return_type"] = json!("&[u32]");
    let error = adapt_helper(&context(None), &borrowed_return).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("standalone-helper return type `&[u32]` contains a borrowed slice"),
        "{error}"
    );
}

#[test]
fn effectful_or_arbitrary_runtime_helper_client_ir_fails_origin_free_validation() {
    let helper_definition = definition(0x85);
    let mut effectful = helper_payload(helper_definition, 0x47);
    effectful.payload["body_ir"] = json!({
        "kind": "signal_read",
        "signal": 0
    });
    let error = adapt_helper(&context(None), &effectful).unwrap_err();
    assert!(error.to_string().contains("origin-free"), "{error}");

    let mut runtime_injection = helper_payload(helper_definition, 0x47);
    runtime_injection.payload["body_ir"] = json!({
        "kind": "call",
        "function": {
            "kind": "runtime",
            "name": "(() => globalThis.__forged = true)"
        },
        "args": []
    });
    let error = adapt_helper(&context(None), &runtime_injection).unwrap_err();
    assert!(error.to_string().contains("origin-free"), "{error}");
}

#[test]
fn helper_codegen_local_ids_are_validated_at_the_adapter_boundary() {
    let helper_definition = definition(0x92);
    let mut unknown = helper_payload(helper_definition, 0x52);
    unknown.payload["body_ir"] = json!({
        "kind": "local",
        "local": 1
    });
    let error = adapt_helper(&context(None), &unknown).unwrap_err();
    assert!(
        error.to_string().contains("undeclared or forward local 1"),
        "{error}"
    );

    let mut colliding = helper_payload(helper_definition, 0x52);
    colliding.payload["body_ir"] = json!({
        "kind": "block",
        "stmts": [{
            "kind": "let",
            "local": 0,
            "name": "collides_with_parameter",
            "ty": {"kind": "unknown"},
            "value": {
                "kind": "literal",
                "value": {"kind": "int", "value": 1}
            }
        }]
    });
    let error = adapt_helper(&context(None), &colliding).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("local 0 is declared more than once"),
        "{error}"
    );
}

#[test]
fn helper_len_intrinsic_remains_in_the_origin_free_subset() {
    let helper_definition = definition(0x86);
    let mut linked = helper_payload(helper_definition, 0x48);
    linked.payload["body_ir"] = json!({
        "kind": "call",
        "function": {
            "kind": "runtime",
            "name": rsjs_ir::CLIENT_HELPER_LEN_INTRINSIC_JS
        },
        "args": [{
            "kind": "literal",
            "value": {"kind": "string", "value": "proa"}
        }]
    });
    let context = context(None).with_helper_identity(
        helper_definition,
        ResolvedSemanticPath::new("acme-web", "acme_web", ["helpers"], "length"),
    );
    assert!(adapt_helper(&context, &linked)
        .unwrap()
        .canonical()
        .is_some());
}

#[test]
fn helper_client_ir_call_paths_must_match_dense_relocation_ordinals() {
    let helper_definition = definition(0x87);
    let source_edge = helper_direct_call_source_edge_id(helper_definition.record, 0);
    let mut mismatch = helper_payload(helper_definition, 0x49);
    mismatch.payload["direct_calls"] = json!([{
        "authored_path": "helpers::claimed",
        "ordinal": 0,
        "source_edge_id": hex_record(&source_edge)
    }]);
    mismatch.payload["body_ir"] = json!({
        "kind": "call",
        "function": {
            "kind": "unresolved",
            "path": "helpers::actual"
        },
        "args": []
    });
    mismatch.helper_edges.push(LinkedHelperEdge {
        source_edge,
        target: definition(0x88),
    });
    let error = adapt_helper(&context(None), &mismatch).unwrap_err();
    assert!(
        error.to_string().contains("direct call ordinal 0")
            && error.to_string().contains("helpers::actual")
            && error.to_string().contains("helpers::claimed"),
        "{error}"
    );

    let mut sparse = helper_payload(helper_definition, 0x49);
    let sparse_source_edge = helper_direct_call_source_edge_id(helper_definition.record, 1);
    sparse.payload["direct_calls"] = json!([{
        "authored_path": "helpers::claimed",
        "ordinal": 1,
        "source_edge_id": hex_record(&sparse_source_edge)
    }]);
    sparse.helper_edges.push(LinkedHelperEdge {
        source_edge: sparse_source_edge,
        target: definition(0x88),
    });
    let error = adapt_helper(&context(None), &sparse).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ordinals must be dense from zero"),
        "{error}"
    );
}

#[test]
fn edge_payload_must_match_relocation_resolved_edges() {
    let helper_definition = definition(0x18);
    let source_edge = helper_direct_call_source_edge_id(helper_definition.record, 0);
    let mut linked = helper_payload(helper_definition, 0x29);
    linked.payload["direct_calls"] = json!([{
        "authored_path": "alias::helper",
        "ordinal": 0,
        "source_edge_id": hex_record(&source_edge)
    }]);
    linked.helper_edges.push(LinkedHelperEdge {
        source_edge: [0x92; 32],
        target: definition(0x19),
    });
    assert!(matches!(
        adapt_helper(&context(None), &linked),
        Err(AbiAdapterError::SourceEdgeSetMismatch { .. })
    ));
}

#[test]
fn legacy_json_hash_ignores_object_insertion_order() {
    let first = json!({"a": 1, "b": {"x": true, "y": false}});
    let second = json!({"b": {"y": false, "x": true}, "a": 1});
    assert_eq!(
        canonical_legacy_json_hash("fixture", &first),
        canonical_legacy_json_hash("fixture", &second)
    );
}
