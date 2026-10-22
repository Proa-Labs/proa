// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::abi_adapter::{
    CompatibilityGap, HelperCodegenBody, HelperCodegenParam, HelperCodegenSignature,
};
use rsjs_ir::abi::{
    AbiValidationError, AbiVersion, CapabilitySet, CargoUnitId, ComponentBindingSource,
    ComponentBoundary, ComponentCallbackShape, ComponentFieldBinding, ComponentFieldCodec,
    ComponentFieldId, ComponentFieldRole, ComponentFieldSchema, ComponentSemanticKey,
    CrossingHookId, DependencyEdge, DependencyEdgeId, DependencyEdgeKind, DependencyEndpoint,
    DescriptorHeader, DirectComponentEdge, EffectKind, HelperSemanticKey, LocalControlRegionId,
    LocalEffectId, LocalValueId, OriginId, OriginKind, OriginPermission, ProducerVersion,
    PropSiteId, PropUse, RenderKind, RuntimeCompatibility, SharedStateProviderSite,
    SharedStateRefId, SharedStateReference, SpanCardinality, StaticSlotCapability,
    TemporalPropMode, VersionRange,
};

fn definition(label: &str, record: u8) -> TransportDefinitionId {
    TransportDefinitionId {
        unit: CargoUnitId::derive(label),
        record: [record; 32],
    }
}

fn header(kind: DescriptorKind) -> DescriptorHeader {
    let runtime_capabilities = CapabilitySet::TEMPORAL_PROPS
        .union(CapabilitySet::OWNER_PARTITION_INPUTS)
        .union(CapabilitySet::DOM_SPAN_CARDINALITY);
    DescriptorHeader::v1(
        kind,
        ProducerVersion::new(0, 1, 0),
        RuntimeCompatibility::new(
            VersionRange::exact(AbiVersion::new(1, 0)),
            runtime_capabilities,
        ),
    )
}

fn component_key(item: &str) -> ComponentSemanticKey {
    ComponentSemanticKey::v1("acme", "app", ["ui"], item).unwrap()
}

fn helper_key(item: &str) -> HelperSemanticKey {
    HelperSemanticKey::v1("acme", "app", ["helpers"], item).unwrap()
}

/// Independent append-only encoding oracle for the helper material
/// record. Keeping this outside the production implementation makes a
/// future accidental reuse of established field numbers observable.
struct AppendOnlySemanticHelperMaterial<'a>(&'a SemanticHelperMaterial);

impl CanonicalEncode for AppendOnlySemanticHelperMaterial<'_> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let material = self.0;
        encoder.record("SemanticHelperMaterial", 6);
        encoder.field(1, &material.semantic_key);
        encoder.field(2, &material.descriptor_hash);
        encoder.field(3, &material.signature_hash);
        encoder.field(4, &material.helper_edges);
        encoder.field(5, &material.transitive_closure);
        encoder.field(6, &material.codegen);
    }
}

fn fixture_component_codegen(item: &str, body: &ComponentDescriptor) -> ComponentCodegenBridge {
    let asset_id = format!("fixture_{}", item.to_ascii_lowercase());
    let mut callback_slot = 0_u32;
    let inbound_fields = body
        .fields
        .iter()
        .filter_map(|(&field_id, field)| {
            let canonical_value = field.client_value?;
            let callback_slot_id = (field.role == ComponentFieldRole::Callback).then(|| {
                let slot = callback_slot;
                callback_slot += 1;
                slot
            });
            Some(crate::abi_adapter::ComponentCodegenInboundField {
                field_id,
                producer_value_id: field_id.index(),
                canonical_value,
                crossing_hook_id: field
                    .crossing_hook
                    .expect("client-visible fixture field has a crossing hook"),
                callback_slot_id,
            })
        })
        .collect::<Vec<_>>();
    let inbound_values = inbound_fields
        .iter()
        .map(|field| field.canonical_value)
        .collect::<BTreeSet<_>>();
    let inbound_targets = body
        .prop_uses
        .iter()
        .filter_map(|(&site, prop_use)| {
            match prop_use.temporal {
                TemporalPropMode::LiveRead | TemporalPropMode::QueryKeyDependency => {}
                TemporalPropMode::SeedOnce(seed_site) => assert_eq!(seed_site, site),
                _ => return None,
            }
            let target = body
                .dependencies
                .values()
                .find_map(|dependency| {
                    (dependency.kind == rsjs_ir::abi::DependencyEdgeKind::Data
                        && dependency.from
                            == rsjs_ir::abi::DependencyEndpoint::Value(prop_use.value))
                    .then_some(match dependency.to {
                        rsjs_ir::abi::DependencyEndpoint::Value(value) => Some(value),
                        _ => None,
                    })
                    .flatten()
                })
                .expect("SeedOnce fixture has a canonical target-signal dependency");
            Some(crate::abi_adapter::ComponentCodegenInboundTarget {
                prop_site_id: site,
                field_id: prop_use.field,
                producer_signal_id: target.index(),
                canonical_signal_value: target,
            })
        })
        .collect::<Vec<_>>();
    let signals = body
        .values
        .iter()
        .copied()
        .filter(|value| !inbound_values.contains(value))
        .map(
            |canonical_value| crate::abi_adapter::ComponentCodegenSignal {
                producer_signal_id: canonical_value.index(),
                canonical_value,
            },
        )
        .collect();
    let callback_slots = inbound_fields
        .iter()
        .filter_map(|field| {
            field.callback_slot_id.map(|producer_callback_slot_id| {
                crate::abi_adapter::ComponentCodegenCallbackSlot {
                    producer_callback_slot_id,
                    canonical_value: field.canonical_value,
                }
            })
        })
        .collect();
    ComponentCodegenBridge {
        asset_id: asset_id.clone(),
        fragment_key: asset_id,
        fragment_root: if body.span == SpanCardinality::Exact(1) {
            ComponentFragmentRoot::ExactOneElement
        } else {
            ComponentFragmentRoot::Unsupported
        },
        inbound_fields,
        inbound_targets,
        signals,
        expressions: Vec::new(),
        payload_handlers: Vec::new(),
        callback_slots,
        inbound_node_refs: Vec::new(),
        direct_node_ref_aliases: Vec::new(),
        uses_queries: body
            .origins
            .values()
            .any(|kind| matches!(kind, OriginKind::Query))
            || body.effects.values().any(|kind| {
                matches!(
                    kind,
                    EffectKind::QueryObservation
                        | EffectKind::QueryRefetch
                        | EffectKind::QueryInvalidation
                )
            }),
        uses_mutations: body
            .origins
            .values()
            .any(|kind| matches!(kind, OriginKind::Mutation))
            || body
                .effects
                .values()
                .any(|kind| matches!(kind, EffectKind::Mutation)),
    }
}

fn canonical_component_bridge(
    envelope: DescriptorEnvelope,
    witnesses: BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
    codegen: ComponentCodegenBridge,
) -> CanonicalDescriptorBridge {
    CanonicalDescriptorBridge::try_component(envelope, witnesses, codegen)
        .expect("fixture component bridge is internally consistent")
}

fn canonical_helper_bridge(
    definition: TransportDefinitionId,
    envelope: DescriptorEnvelope,
    witnesses: BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
    codegen: HelperCodegenBridge,
) -> CanonicalDescriptorBridge {
    CanonicalDescriptorBridge::try_helper(definition, envelope, witnesses, codegen)
        .expect("fixture helper bridge is internally consistent")
}

fn component_bridge(
    item: &str,
    definition: TransportDefinitionId,
    helper_targets: &[(WitnessRecordId, TransportDefinitionId, u8)],
) -> AbiBridgeResult {
    let mut body = ComponentDescriptor::v1(
        component_key(item),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::ParticipantOnly,
        SpanCardinality::Exact(1),
    );
    let mut witnesses = BTreeMap::from([(
        WitnessRecordId::new(0),
        ResolvedTransportWitness::Definition(definition),
    )]);
    for (index, &(witness, target, source)) in helper_targets.iter().enumerate() {
        body.direct_helpers
            .insert(HelperCallEdgeId::new(index as u32), witness);
        witnesses.insert(
            witness,
            ResolvedTransportWitness::HelperEdge {
                source_edge: [source; 32],
                target,
            },
        );
    }
    let envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(body.clone()),
        None,
    )
    .unwrap();
    AbiBridgeResult::Canonical(canonical_component_bridge(
        envelope,
        witnesses,
        fixture_component_codegen(item, &body),
    ))
}

fn shared_state_component_bridge(
    item: &str,
    definition: TransportDefinitionId,
    type_key: &str,
    schema_abi: SemanticHash,
) -> AbiBridgeResult {
    let mut result = component_bridge(item, definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut result else {
        unreachable!("fixture component is canonical")
    };
    let codegen = {
        let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
            unreachable!("fixture component contains a component descriptor")
        };
        let origin = OriginId::new(0);
        let value = LocalValueId::new(0);
        let read = LocalEffectId::new(0);
        let write = LocalEffectId::new(1);
        body.origin_permission = OriginPermission::OriginCapable;
        body.client_ir_hash = SemanticHash::derive(&format!("{item}-shared-state-client-ir"));
        body.values.insert(value);
        body.effects.insert(read, EffectKind::SharedStateRead);
        body.effects.insert(write, EffectKind::SharedStateWrite);
        body.origins.insert(origin, OriginKind::SharedState);
        body.shared_state.insert(
            SharedStateRefId::new(0),
            SharedStateReference {
                type_key: type_key.to_owned(),
                schema_abi,
                origin,
            },
        );
        body.dependencies.extend([
            (
                DependencyEdgeId::new(0),
                DependencyEdge::new(
                    DependencyEdgeKind::Component,
                    DependencyEndpoint::Origin(origin),
                    DependencyEndpoint::Value(value),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(1),
                DependencyEdge::new(
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Origin(origin),
                    DependencyEndpoint::Effect(read),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(2),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Effect(read),
                    DependencyEndpoint::Value(value),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(3),
                DependencyEdge::new(
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Origin(origin),
                    DependencyEndpoint::Effect(write),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(4),
                DependencyEdge::new(
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Effect(write),
                    DependencyEndpoint::Value(value),
                    origin,
                ),
            ),
        ]);
        fixture_component_codegen(item, body)
    };
    bridge.replace_component_codegen_unchecked(codegen);
    bridge.envelope.refresh_hashes();
    result
}

fn helper_bridge(
    item: &str,
    definition: TransportDefinitionId,
    signature: &str,
    helper_targets: &[(WitnessRecordId, TransportDefinitionId, u8, usize)],
) -> AbiBridgeResult {
    let (params, suffix) = signature
        .strip_prefix("fn(")
        .and_then(|rest| rest.split_once(')'))
        .expect("fixture helper signatures use fn(...) syntax");
    let codegen_signature = HelperCodegenSignature {
        params: if params.is_empty() {
            Vec::new()
        } else {
            params
                .split(',')
                .enumerate()
                .map(|(index, rust_type)| HelperCodegenParam {
                    name: format!("arg_{index}"),
                    rust_type: rust_type.trim().to_owned(),
                })
                .collect()
        },
        return_type: suffix
            .trim()
            .strip_prefix("->")
            .map(str::trim)
            .unwrap_or("()")
            .to_owned(),
    };
    let parameter_count =
        u32::try_from(codegen_signature.params.len()).expect("fixture parameter count fits u32");
    let mut codegen_stmts = helper_targets
        .iter()
        .enumerate()
        .map(|(index, (_, _, _, target_arity))| {
            let edge = u32::try_from(index).expect("fixture helper edge fits u32");
            let local = parameter_count
                .checked_add(edge)
                .expect("fixture helper local fits u32");
            rsjs_ir::ClientStmt::let_value(
                rsjs_ir::ClientLocalId(local),
                format!("target_{edge}"),
                rsjs_ir::ClientType::Unknown,
                rsjs_ir::ClientExpr::Call {
                    function: rsjs_ir::ClientFunctionRef::Helper {
                        edge: HelperCallEdgeId::new(edge),
                        authored_path: format!("fixture::target_{edge}"),
                        name: format!("target_{edge}"),
                    },
                    args: (0..*target_arity)
                        .map(|_| rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::Int(0)))
                        .collect(),
                },
            )
        })
        .collect::<Vec<_>>();
    let return_value = match codegen_signature.return_type.as_str() {
        "()" => None,
        "bool" => Some(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::Bool(false))),
        "String" | "&str" => Some(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::String(
            String::new(),
        ))),
        "i64" | "u64" => Some(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::BigInt(0))),
        "f32" | "f64" => Some(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::Float(0.0))),
        _ => Some(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::Int(0))),
    };
    codegen_stmts.push(rsjs_ir::ClientStmt::Return(return_value));
    let codegen_body = HelperCodegenBody::Block(rsjs_ir::ClientBlock {
        stmts: codegen_stmts,
    });
    let source_ir_hash = SemanticHash::derive(&format!("{item}-ir"));
    let mut body = HelperDescriptor::v1(
        helper_key(item),
        WitnessRecordId::new(0),
        codegen_signature.semantic_hash(),
        source_ir_hash,
    );
    let mut witnesses = BTreeMap::from([(
        WitnessRecordId::new(0),
        ResolvedTransportWitness::Definition(definition),
    )]);
    for (index, &(witness, target, source, _)) in helper_targets.iter().enumerate() {
        body.direct_helpers
            .insert(HelperCallEdgeId::new(index as u32), witness);
        witnesses.insert(
            witness,
            ResolvedTransportWitness::HelperEdge {
                source_edge: [source; 32],
                target,
            },
        );
    }
    let codegen = HelperCodegenBridge::try_new_for_test(
        definition,
        codegen_signature,
        source_ir_hash,
        codegen_body,
        &body,
    )
    .expect("fixture helper codegen is valid for its descriptor");
    let envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Helper),
        DescriptorBody::Helper(body),
        None,
    )
    .unwrap();
    AbiBridgeResult::Canonical(canonical_helper_bridge(
        definition, envelope, witnesses, codegen,
    ))
}

fn parent_component_bridge(
    item: &str,
    definition: TransportDefinitionId,
    child: TransportDefinitionId,
) -> AbiBridgeResult {
    component_with_children_bridge(item, definition, &[(child, 77)])
}

fn component_with_children_bridge(
    item: &str,
    definition: TransportDefinitionId,
    children: &[(TransportDefinitionId, u8)],
) -> AbiBridgeResult {
    let mut body = ComponentDescriptor::v1(
        component_key(item),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::ParticipantOnly,
        SpanCardinality::Exact(1),
    );
    body.origins
        .insert(OriginId::new(0), OriginKind::SourceOnly);
    let mut witnesses = BTreeMap::from([(
        WitnessRecordId::new(0),
        ResolvedTransportWitness::Definition(definition),
    )]);
    for (index, &(child, source)) in children.iter().enumerate() {
        let edge = ComponentEdgeId::new(index as u32);
        let witness = WitnessRecordId::new(index as u32 + 1);
        body.direct_components.insert(
            edge,
            DirectComponentEdge {
                witness,
                boundary: ComponentBoundary::Composed,
                bindings: BTreeMap::new(),
                lifecycle_control: None,
                origin: OriginId::new(0),
            },
        );
        witnesses.insert(
            witness,
            ResolvedTransportWitness::ComponentEdge {
                source_edge: [source; 32],
                target: child,
            },
        );
    }
    let envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(body.clone()),
        None,
    )
    .unwrap();
    AbiBridgeResult::Canonical(canonical_component_bridge(
        envelope,
        witnesses,
        fixture_component_codegen(item, &body),
    ))
}

fn node_ref_handle_alias_bridges(
    parent_definition: TransportDefinitionId,
    child_definition: TransportDefinitionId,
) -> (AbiBridgeResult, AbiBridgeResult) {
    let child_field = ComponentFieldId::new(0);
    let child_crossing = CrossingHookId::new(0);
    let child_value = LocalValueId::new(0);
    let child_origin = OriginId::new(0);
    let child_effect = LocalEffectId::new(0);
    let child_field_witness = WitnessRecordId::new(1);
    let mut child = ComponentDescriptor::v1(
        component_key("NodeRefChild"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::ParticipantOnly,
        SpanCardinality::Exact(1),
    );
    child.values.insert(child_value);
    child.crossing_hooks.insert(child_crossing);
    child.origins.insert(child_origin, OriginKind::SourceOnly);
    child
        .effects
        .insert(child_effect, EffectKind::NodeReference);
    child.dependencies.insert(
        DependencyEdgeId::new(0),
        DependencyEdge::new(
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(child_origin),
            DependencyEndpoint::Effect(child_effect),
            child_origin,
        ),
    );
    child.fields.insert(
        child_field,
        ComponentFieldSchema {
            source_name: "target".to_owned(),
            role: ComponentFieldRole::ClientHandle,
            type_witness: Some(child_field_witness),
            crossing_hook: Some(child_crossing),
            client_value: Some(child_value),
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Unsupported,
        },
    );
    let mut child_codegen = fixture_component_codegen("NodeRefChild", &child);
    child_codegen.inbound_node_refs = vec![crate::abi_adapter::ComponentCodegenInboundNodeRef {
        field_id: child_field,
        producer_node_ref_id: 11,
        bind_target: Some(ComponentNodeRefBindTarget::Input),
    }];
    let child = AbiBridgeResult::Canonical(canonical_component_bridge(
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(child),
            None,
        )
        .unwrap(),
        BTreeMap::from([
            (
                WitnessRecordId::new(0),
                ResolvedTransportWitness::Definition(child_definition),
            ),
            (
                child_field_witness,
                ResolvedTransportWitness::ComponentField {
                    field_record: [91; 32],
                    target: child_definition,
                },
            ),
        ]),
        child_codegen,
    ));

    let edge = ComponentEdgeId::new(0);
    let parent_value = LocalValueId::new(0);
    let parent_origin = OriginId::new(0);
    let parent_effect = LocalEffectId::new(0);
    let edge_witness = WitnessRecordId::new(1);
    let mut parent = ComponentDescriptor::v1(
        component_key("NodeRefParent"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::OriginCapable,
        SpanCardinality::Exact(1),
    );
    parent.values.insert(parent_value);
    parent.origins.insert(parent_origin, OriginKind::NodeRef);
    parent
        .effects
        .insert(parent_effect, EffectKind::NodeReference);
    parent.dependencies.insert(
        DependencyEdgeId::new(0),
        DependencyEdge::new(
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(parent_origin),
            DependencyEndpoint::Effect(parent_effect),
            parent_origin,
        ),
    );
    parent.direct_components.insert(
        edge,
        DirectComponentEdge {
            witness: edge_witness,
            boundary: ComponentBoundary::Composed,
            bindings: BTreeMap::from([(
                "target".to_owned(),
                ComponentFieldBinding {
                    source: ComponentBindingSource::ClientHandle {
                        value: parent_value,
                    },
                    origin: parent_origin,
                },
            )]),
            lifecycle_control: None,
            origin: parent_origin,
        },
    );
    let mut parent_codegen = fixture_component_codegen("NodeRefParent", &parent);
    parent_codegen.signals.clear();
    parent_codegen.direct_node_ref_aliases =
        vec![crate::abi_adapter::ComponentCodegenDirectNodeRefAlias {
            edge_id: edge,
            field_name: "target".to_owned(),
            producer_node_ref_id: 7,
            canonical_value: parent_value,
        }];
    let parent = AbiBridgeResult::Canonical(canonical_component_bridge(
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(parent),
            None,
        )
        .unwrap(),
        BTreeMap::from([
            (
                WitnessRecordId::new(0),
                ResolvedTransportWitness::Definition(parent_definition),
            ),
            (
                edge_witness,
                ResolvedTransportWitness::ComponentEdge {
                    source_edge: [92; 32],
                    target: child_definition,
                },
            ),
        ]),
        parent_codegen,
    ));

    (parent, child)
}

fn shared_state_provider_component_bridge(
    item: &str,
    definition: TransportDefinitionId,
    children: &[(TransportDefinitionId, u8)],
    providers: &[(u32, u32, &str, SemanticHash)],
) -> AbiBridgeResult {
    let mut result = component_with_children_bridge(item, definition, children);
    let AbiBridgeResult::Canonical(bridge) = &mut result else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    for &(site, child_edge, type_key, schema_abi) in providers {
        body.shared_state_providers.insert(
            ProviderSiteId::new(site),
            SharedStateProviderSite {
                child_edge: ComponentEdgeId::new(child_edge),
                type_key: type_key.to_owned(),
                schema_abi,
            },
        );
    }
    bridge.envelope.refresh_hashes();
    result
}

fn immutable_static_slot_bridges(
    parent_definition: TransportDefinitionId,
    child_definition: TransportDefinitionId,
    live_slot: bool,
) -> Result<(AbiBridgeResult, AbiBridgeResult), AbiValidationError> {
    let mut child = ComponentDescriptor::v1(
        component_key("Disclosure"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::OriginCapable,
        SpanCardinality::Exact(1),
    );
    child.root_eligible = false;
    child.fields.insert(
        ComponentFieldId::new(0),
        ComponentFieldSchema {
            source_name: "children".to_string(),
            role: ComponentFieldRole::Slot,
            type_witness: None,
            crossing_hook: None,
            client_value: None,
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Immutable {
                permits_nested_owner: false,
            },
        },
    );
    // Model independent browser work without coupling it to the slot.
    child.origins.insert(OriginId::new(0), OriginKind::Effect);
    child
        .effects
        .insert(LocalEffectId::new(0), EffectKind::DomTextUpdate);
    let child_envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(child.clone()),
        None,
    )?;
    let child_bridge = AbiBridgeResult::Canonical(canonical_component_bridge(
        child_envelope,
        BTreeMap::from([(
            WitnessRecordId::new(0),
            ResolvedTransportWitness::Definition(child_definition),
        )]),
        fixture_component_codegen("Disclosure", &child),
    ));

    let mut parent = ComponentDescriptor::v1(
        component_key("Page"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::ParticipantOnly,
        SpanCardinality::Exact(1),
    );
    parent
        .origins
        .insert(OriginId::new(0), OriginKind::SourceOnly);
    let source = if live_slot {
        parent.control_regions.insert(LocalControlRegionId::new(0));
        ComponentBindingSource::LiveSlot {
            control: LocalControlRegionId::new(0),
        }
    } else {
        ComponentBindingSource::StaticSlot {
            span: SpanCardinality::Exact(1),
        }
    };
    parent.direct_components.insert(
        ComponentEdgeId::new(0),
        DirectComponentEdge {
            witness: WitnessRecordId::new(1),
            boundary: ComponentBoundary::IndependentIsland,
            bindings: BTreeMap::from([(
                "children".to_string(),
                ComponentFieldBinding {
                    source,
                    origin: OriginId::new(0),
                },
            )]),
            lifecycle_control: None,
            origin: OriginId::new(0),
        },
    );
    let parent_envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(parent.clone()),
        None,
    )?;
    let parent_bridge = AbiBridgeResult::Canonical(canonical_component_bridge(
        parent_envelope,
        BTreeMap::from([
            (
                WitnessRecordId::new(0),
                ResolvedTransportWitness::Definition(parent_definition),
            ),
            (
                WitnessRecordId::new(1),
                ResolvedTransportWitness::ComponentEdge {
                    source_edge: [0x73; 32],
                    target: child_definition,
                },
            ),
        ]),
        fixture_component_codegen("Page", &parent),
    ));

    Ok((parent_bridge, child_bridge))
}

fn live_value_and_callback_bridges(
    parent_definition: TransportDefinitionId,
    child_definition: TransportDefinitionId,
    temporal: TemporalPropMode,
) -> (AbiBridgeResult, AbiBridgeResult) {
    let mut child = ComponentDescriptor::v1(
        component_key("LiveChild"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::ParticipantOnly,
        SpanCardinality::Exact(1),
    );
    child
        .origins
        .insert(OriginId::new(0), OriginKind::SourceOnly);
    child.fields.insert(
        ComponentFieldId::new(0),
        ComponentFieldSchema {
            source_name: "value".to_string(),
            role: ComponentFieldRole::Value,
            type_witness: Some(WitnessRecordId::new(40)),
            crossing_hook: Some(CrossingHookId::new(0)),
            client_value: Some(LocalValueId::new(0)),
            callback_shape: None,
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Unsupported,
        },
    );
    child.fields.insert(
        ComponentFieldId::new(1),
        ComponentFieldSchema {
            source_name: "on_change".to_string(),
            role: ComponentFieldRole::Callback,
            type_witness: Some(WitnessRecordId::new(41)),
            crossing_hook: Some(CrossingHookId::new(1)),
            client_value: Some(LocalValueId::new(1)),
            callback_shape: Some(ComponentCallbackShape::Required),
            codec: ComponentFieldCodec::None,
            static_slot: StaticSlotCapability::Unsupported,
        },
    );
    child
        .crossing_hooks
        .extend([CrossingHookId::new(0), CrossingHookId::new(1)]);
    child
        .values
        .extend([LocalValueId::new(0), LocalValueId::new(1)]);
    child.prop_uses.insert(
        PropSiteId::new(0),
        PropUse {
            field: ComponentFieldId::new(0),
            value: LocalValueId::new(0),
            temporal,
            origin: OriginId::new(0),
        },
    );
    if matches!(
        temporal,
        TemporalPropMode::LiveRead | TemporalPropMode::SeedOnce(_)
    ) {
        child.values.insert(LocalValueId::new(2));
        child.dependencies.insert(
            rsjs_ir::abi::DependencyEdgeId::new(0),
            rsjs_ir::abi::DependencyEdge::new(
                rsjs_ir::abi::DependencyEdgeKind::Data,
                rsjs_ir::abi::DependencyEndpoint::Value(LocalValueId::new(0)),
                rsjs_ir::abi::DependencyEndpoint::Value(LocalValueId::new(2)),
                OriginId::new(0),
            ),
        );
    }
    child
        .effects
        .insert(LocalEffectId::new(0), EffectKind::DomTextUpdate);
    let child_envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(child.clone()),
        None,
    )
    .unwrap();
    let child_bridge = AbiBridgeResult::Canonical(canonical_component_bridge(
        child_envelope,
        BTreeMap::from([
            (
                WitnessRecordId::new(0),
                ResolvedTransportWitness::Definition(child_definition),
            ),
            (
                WitnessRecordId::new(40),
                ResolvedTransportWitness::ComponentField {
                    field_record: [0x40; 32],
                    target: child_definition,
                },
            ),
            (
                WitnessRecordId::new(41),
                ResolvedTransportWitness::ComponentField {
                    field_record: [0x41; 32],
                    target: child_definition,
                },
            ),
        ]),
        fixture_component_codegen("LiveChild", &child),
    ));

    let mut parent = ComponentDescriptor::v1(
        component_key("LiveParent"),
        WitnessRecordId::new(0),
        RenderKind::WebRenderSync,
        OriginPermission::OriginCapable,
        SpanCardinality::Exact(1),
    );
    parent
        .origins
        .insert(OriginId::new(0), OriginKind::WritableSignal);
    parent
        .values
        .extend([LocalValueId::new(0), LocalValueId::new(1)]);
    parent
        .effects
        .insert(LocalEffectId::new(0), EffectKind::DomTextUpdate);
    parent.direct_components.insert(
        ComponentEdgeId::new(0),
        DirectComponentEdge {
            witness: WitnessRecordId::new(1),
            boundary: ComponentBoundary::Composed,
            bindings: BTreeMap::from([
                (
                    "on_change".to_string(),
                    ComponentFieldBinding {
                        source: ComponentBindingSource::LiveCallback {
                            value: LocalValueId::new(1),
                            shape: ComponentCallbackShape::Required,
                        },
                        origin: OriginId::new(0),
                    },
                ),
                (
                    "value".to_string(),
                    ComponentFieldBinding {
                        source: ComponentBindingSource::LiveValue {
                            value: LocalValueId::new(0),
                        },
                        origin: OriginId::new(0),
                    },
                ),
            ]),
            lifecycle_control: None,
            origin: OriginId::new(0),
        },
    );
    let parent_envelope = DescriptorEnvelope::new(
        header(DescriptorKind::Component),
        DescriptorBody::Component(parent.clone()),
        None,
    )
    .unwrap();
    let parent_bridge = AbiBridgeResult::Canonical(canonical_component_bridge(
        parent_envelope,
        BTreeMap::from([
            (
                WitnessRecordId::new(0),
                ResolvedTransportWitness::Definition(parent_definition),
            ),
            (
                WitnessRecordId::new(1),
                ResolvedTransportWitness::ComponentEdge {
                    source_edge: [77; 32],
                    target: child_definition,
                },
            ),
        ]),
        fixture_component_codegen("LiveParent", &parent),
    ));

    (parent_bridge, child_bridge)
}

fn with_browser_effect(mut result: AbiBridgeResult) -> AbiBridgeResult {
    let AbiBridgeResult::Canonical(bridge) = &mut result else {
        unreachable!("test component bridges are canonical")
    };
    let DescriptorBody::Component(component) = &mut bridge.envelope.body else {
        unreachable!("test component bridge contains a component")
    };
    component.origin_permission = OriginPermission::OriginCapable;
    component
        .origins
        .insert(OriginId::new(0), OriginKind::Effect);
    component
        .effects
        .insert(LocalEffectId::new(0), EffectKind::DomTextUpdate);
    bridge.envelope.refresh_hashes();
    result
}

fn plan_with_signature(signature: &str) -> SemanticLinkPlan {
    let component = definition("component-unit", 1);
    let helper = definition("helper-unit", 2);
    let adapted = AdaptedTransportAbi {
        components: vec![component_bridge(
            "Page",
            component,
            &[(WitnessRecordId::new(1), helper, 21)],
        )],
        helpers: vec![helper_bridge("format", helper, signature, &[])],
    };
    link_semantic_plan(LinkUnitId::derive("root"), &adapted).unwrap()
}

#[test]
fn exact_helper_signature_changes_plan_identity() {
    let u32_plan = plan_with_signature("fn(u32) -> String");
    let u64_plan = plan_with_signature("fn(u64) -> String");

    assert_ne!(
        u32_plan.helpers[0].descriptor.semantic_hash,
        u64_plan.helpers[0].descriptor.semantic_hash
    );
    assert_ne!(u32_plan.material.payload, u64_plan.material.payload);
    assert_ne!(u32_plan.plan_id, u64_plan.plan_id);
}

#[test]
fn canonical_bridge_construction_rejects_descriptor_codegen_kind_confusion() {
    let component_definition = definition("kind-safe-component", 1);
    let helper_definition = definition("kind-safe-helper", 2);
    let component = component_bridge("Page", component_definition, &[]);
    let helper = helper_bridge("format", helper_definition, "fn()", &[]);
    let AbiBridgeResult::Canonical(component) = component else {
        unreachable!("fixture component is canonical")
    };
    let AbiBridgeResult::Canonical(helper) = helper else {
        unreachable!("fixture helper is canonical")
    };

    let component_error = CanonicalDescriptorBridge::try_component(
        helper.envelope.clone(),
        helper.witnesses.clone(),
        component.component_codegen().cloned().unwrap(),
    )
    .expect_err("component codegen cannot accompany a helper descriptor");
    assert_eq!(
        component_error,
        "component codegen requires a component descriptor"
    );

    let helper_error = CanonicalDescriptorBridge::try_helper(
        helper_definition,
        component.envelope.clone(),
        component.witnesses.clone(),
        helper.helper_codegen().cloned().unwrap(),
    )
    .expect_err("helper codegen cannot accompany a component descriptor");
    assert_eq!(helper_error, "helper codegen requires a helper descriptor");
}

#[test]
fn helper_codegen_body_is_plan_identity_and_fails_closed_when_stale_or_wrong_kind() {
    let component = definition("helper-codegen-component", 1);
    let helper = definition("helper-codegen-helper", 2);
    let component_result =
        component_bridge("Page", component, &[(WitnessRecordId::new(1), helper, 21)]);
    let AbiBridgeResult::Canonical(component_bridge) = &component_result else {
        unreachable!("fixture component is canonical")
    };
    let component_codegen = component_bridge
        .component_codegen()
        .cloned()
        .expect("fixture component codegen");
    let original = helper_bridge("format", helper, "fn(u32) -> i32", &[]);
    let mut changed = original.clone();
    let AbiBridgeResult::Canonical(changed_bridge) = &mut changed else {
        unreachable!("fixture helper is canonical")
    };
    let existing_codegen = changed_bridge
        .helper_codegen()
        .expect("fixture helper codegen");
    let changed_signature = existing_codegen.signature().clone();
    let changed_source_ir_hash = existing_codegen.source_ir_hash();
    let DescriptorBody::Helper(changed_descriptor) = &changed_bridge.envelope.body else {
        unreachable!("fixture helper descriptor")
    };
    changed_bridge.replace_helper_codegen_unchecked(
        HelperCodegenBridge::try_new_for_test(
            helper,
            changed_signature,
            changed_source_ir_hash,
            HelperCodegenBody::Expr(rsjs_ir::ClientExpr::Literal(rsjs_ir::Value::Int(41))),
            changed_descriptor,
        )
        .expect("replacement body preserves the helper descriptor contract"),
    );

    let original_plan = link_semantic_plan(
        LinkUnitId::derive("helper-codegen-root"),
        &AdaptedTransportAbi {
            components: vec![component_result.clone()],
            helpers: vec![original.clone()],
        },
    )
    .unwrap();
    let changed_plan = link_semantic_plan(
        LinkUnitId::derive("helper-codegen-root"),
        &AdaptedTransportAbi {
            components: vec![component_result.clone()],
            helpers: vec![changed.clone()],
        },
    )
    .unwrap();
    assert_eq!(
        original_plan.helpers[0].descriptor.semantic_hash,
        changed_plan.helpers[0].descriptor.semantic_hash,
        "concrete codegen remains separate from the canonical helper descriptor"
    );
    assert_ne!(
        original_plan.material.payload,
        changed_plan.material.payload
    );
    assert_ne!(original_plan.plan_id, changed_plan.plan_id);

    let mut stale = original.clone();
    let AbiBridgeResult::Canonical(stale_bridge) = &mut stale else {
        unreachable!()
    };
    let DescriptorBody::Helper(stale_descriptor) = &mut stale_bridge.envelope.body else {
        unreachable!("fixture helper descriptor")
    };
    stale_descriptor.client_ir_hash = SemanticHash::derive("stale-helper-source-ir");
    stale_bridge.envelope.refresh_hashes();
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("helper-codegen-root"),
            &AdaptedTransportAbi {
                components: vec![component_result.clone()],
                helpers: vec![stale],
            },
        ),
        Err(SemanticLinkError::InvalidHelperCodegen { .. })
    ));

    let AbiBridgeResult::Canonical(runtime_bridge) = &original else {
        unreachable!()
    };
    let runtime_codegen = runtime_bridge.helper_codegen().unwrap();
    let DescriptorBody::Helper(runtime_descriptor) = &runtime_bridge.envelope.body else {
        unreachable!("fixture helper descriptor")
    };
    let error = HelperCodegenBridge::try_new_for_test(
        helper,
        runtime_codegen.signature().clone(),
        runtime_codegen.source_ir_hash(),
        HelperCodegenBody::Expr(rsjs_ir::ClientExpr::Call {
            function: rsjs_ir::ClientFunctionRef::Runtime(
                "(() => globalThis.__forged = true)".to_owned(),
            ),
            args: Vec::new(),
        }),
        runtime_descriptor,
    )
    .expect_err("an origin-bearing runtime call is not constructible as helper codegen");
    assert!(error.contains("origin-free"), "{error}");

    let mut wrong_kind = original;
    let AbiBridgeResult::Canonical(wrong_kind_bridge) = &mut wrong_kind else {
        unreachable!()
    };
    wrong_kind_bridge.replace_component_codegen_unchecked(component_codegen);
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("helper-codegen-root"),
            &AdaptedTransportAbi {
                components: vec![component_result],
                helpers: vec![wrong_kind],
            },
        ),
        Err(SemanticLinkError::DescriptorCodegenKindMismatch { .. })
    ));
}

#[test]
fn helper_program_validation_covers_link_and_persisted_plan_boundaries() {
    let component = definition("helper-program-component", 1);
    let caller = definition("helper-program-caller", 2);
    let target = definition("helper-program-target", 3);
    let component_result =
        component_bridge("Page", component, &[(WitnessRecordId::new(1), caller, 31)]);
    let wrong_arity = AdaptedTransportAbi {
        components: vec![component_result.clone()],
        helpers: vec![
            helper_bridge(
                "caller",
                caller,
                "fn()",
                &[(WitnessRecordId::new(1), target, 32, 0)],
            ),
            helper_bridge("target", target, "fn(u32)", &[]),
        ],
    };
    let error = link_semantic_plan(LinkUnitId::derive("wrong-helper-arity"), &wrong_arity)
        .expect_err("helper calls must match their relocation-resolved target signature");
    let SemanticLinkError::InvalidHelperProgram { reason } = error else {
        panic!("unexpected helper arity diagnostic: {error}");
    };
    assert!(
        reason.contains("with 0 arguments") && reason.contains("expected 1"),
        "{reason}"
    );

    let mut fallthrough = helper_bridge("fallthrough", caller, "fn() -> i32", &[]);
    let AbiBridgeResult::Canonical(fallthrough_bridge) = &mut fallthrough else {
        unreachable!("fixture helper is canonical")
    };
    let codegen = fallthrough_bridge.helper_codegen().unwrap();
    let signature = codegen.signature().clone();
    let source_ir_hash = codegen.source_ir_hash();
    let DescriptorBody::Helper(descriptor) = &fallthrough_bridge.envelope.body else {
        unreachable!("fixture helper descriptor")
    };
    fallthrough_bridge.replace_helper_codegen_unchecked(
        HelperCodegenBridge::try_new_for_test(
            caller,
            signature,
            source_ir_hash,
            HelperCodegenBody::Block(rsjs_ir::ClientBlock::default()),
            descriptor,
        )
        .expect("structural helper validation leaves return typing to the linked program"),
    );
    let error = link_semantic_plan(
        LinkUnitId::derive("helper-fallthrough"),
        &AdaptedTransportAbi {
            components: vec![component_result.clone()],
            helpers: vec![fallthrough],
        },
    )
    .expect_err("a non-unit helper may not fall through without a value");
    let SemanticLinkError::InvalidHelperProgram { reason } = error else {
        panic!("unexpected helper return-shape diagnostic: {error}");
    };
    assert!(reason.contains("falls through"), "{reason}");

    let valid = AdaptedTransportAbi {
        components: vec![component_result],
        helpers: vec![
            helper_bridge(
                "caller",
                caller,
                "fn()",
                &[(WitnessRecordId::new(1), target, 32, 1)],
            ),
            helper_bridge("target", target, "fn(u32)", &[]),
        ],
    };
    let mut plan =
        link_semantic_plan(LinkUnitId::derive("persisted-helper-program"), &valid).unwrap();
    let caller = plan
        .helpers
        .iter_mut()
        .find(|helper| helper.semantic_key == helper_key("caller"))
        .expect("linked caller helper");
    let HelperCodegenBody::Block(mut body) = caller.codegen.body().clone() else {
        unreachable!("fixture caller uses a block body")
    };
    let rsjs_ir::ClientStmt::Let {
        value: Some(rsjs_ir::ClientExpr::Call { args, .. }),
        ..
    } = &mut body.stmts[0]
    else {
        unreachable!("fixture caller starts with its helper call")
    };
    args.clear();
    let DescriptorBody::Helper(descriptor) = &caller.descriptor.body else {
        unreachable!("linked helper descriptor")
    };
    caller.codegen = HelperCodegenBridge::try_new_for_test(
        caller.definition,
        caller.codegen.signature().clone(),
        caller.codegen.source_ir_hash(),
        HelperCodegenBody::Block(body),
        descriptor,
    )
    .expect("the tampered caller remains structurally valid in isolation");
    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::InvalidHelperProgram { .. })
    ));
}

#[test]
fn component_codegen_coordinates_change_plan_identity() {
    let component = definition("component-codegen", 1);
    let original = component_bridge("Page", component, &[]);
    let mut changed = original.clone();
    let AbiBridgeResult::Canonical(changed_bridge) = &mut changed else {
        unreachable!("fixture component is canonical")
    };
    let codegen = changed_bridge
        .component_codegen_mut()
        .expect("fixture component codegen");
    codegen.asset_id = "fixture_page_relinked".to_owned();
    codegen.fragment_key = "fixture_page_relinked".to_owned();

    let original = link_semantic_plan(
        LinkUnitId::derive("component-codegen-root"),
        &AdaptedTransportAbi {
            components: vec![original],
            helpers: vec![],
        },
    )
    .unwrap();
    let changed = link_semantic_plan(
        LinkUnitId::derive("component-codegen-root"),
        &AdaptedTransportAbi {
            components: vec![changed],
            helpers: vec![],
        },
    )
    .unwrap();

    assert_eq!(
        original.components[0].descriptor.semantic_hash,
        changed.components[0].descriptor.semantic_hash,
        "artifact coordinates are codegen data rather than component identity"
    );
    assert_ne!(original.material.payload, changed.material.payload);
    assert_ne!(original.plan_id, changed.plan_id);
}

#[test]
fn structural_controls_link_without_enabling_dynamic_component_sites() {
    let definition = definition("conditional-component", 1);
    let mut component = component_bridge("Disclosure", definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut component else {
        unreachable!("fixture component is canonical")
    };
    let codegen = {
        let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
            unreachable!("fixture component contains a component descriptor")
        };
        let origin = OriginId::new(0);
        let value = LocalValueId::new(0);
        let control = LocalControlRegionId::new(0);
        let effect = LocalEffectId::new(0);
        body.origin_permission = OriginPermission::OriginCapable;
        body.client_ir_hash = SemanticHash::derive("conditional-component-client-ir");
        body.values.insert(value);
        body.control_regions.insert(control);
        body.effects.insert(effect, EffectKind::DomStructuralUpdate);
        body.origins.insert(origin, OriginKind::WritableSignal);
        body.dependencies.extend([
            (
                DependencyEdgeId::new(0),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Origin(origin),
                    DependencyEndpoint::Value(value),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(1),
                DependencyEdge::new(
                    DependencyEdgeKind::Control,
                    DependencyEndpoint::Value(value),
                    DependencyEndpoint::Control(control),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(2),
                DependencyEdge::new(
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Origin(origin),
                    DependencyEndpoint::Effect(effect),
                    origin,
                ),
            ),
            (
                DependencyEdgeId::new(3),
                DependencyEdge::new(
                    DependencyEdgeKind::Control,
                    DependencyEndpoint::Control(control),
                    DependencyEndpoint::Effect(effect),
                    origin,
                ),
            ),
        ]);
        fixture_component_codegen("Disclosure", body)
    };
    bridge.replace_component_codegen_unchecked(codegen);
    bridge.envelope.refresh_hashes();

    let plan = link_semantic_plan(
        LinkUnitId::derive("conditional-component-root"),
        &AdaptedTransportAbi {
            components: vec![component],
            helpers: vec![],
        },
    )
    .expect("ordinary structural controls should link");
    let DescriptorBody::Component(body) = &plan.components[0].descriptor.body else {
        panic!("expected component descriptor");
    };
    assert_eq!(
        body.control_regions,
        BTreeSet::from([LocalControlRegionId::new(0)])
    );
    assert_eq!(
        body.effects.get(&LocalEffectId::new(0)),
        Some(&EffectKind::DomStructuralUpdate)
    );
    assert_eq!(plan.sites.len(), 1);
    assert_eq!(plan.owners.owners.len(), 1);
    validate_semantic_plan(&plan).expect("the control graph should relink exactly");
}

#[test]
fn malformed_component_codegen_mapping_fails_closed() {
    let parent = definition("malformed-parent", 1);
    let child = definition("malformed-child", 2);
    let (parent_bridge, mut child_result) = live_value_and_callback_bridges(
        parent,
        child,
        TemporalPropMode::SeedOnce(PropSiteId::new(0)),
    );
    let AbiBridgeResult::Canonical(child_bridge) = &mut child_result else {
        unreachable!("fixture child is canonical")
    };
    child_bridge
        .component_codegen_mut()
        .unwrap()
        .inbound_targets[0]
        .canonical_signal_value = LocalValueId::new(1);

    let error = link_semantic_plan(
        LinkUnitId::derive("malformed-codegen-root"),
        &AdaptedTransportAbi {
            components: vec![parent_bridge, child_result],
            helpers: vec![],
        },
    )
    .expect_err("a target signal mapping may not disagree with the signal table");
    assert!(matches!(
        error,
        SemanticLinkError::InvalidComponentCodegen {
            definition: actual,
            ..
        } if actual == child
    ));
    assert!(
        error.to_string().contains("signal table maps it"),
        "unexpected focused codegen diagnostic: {error}"
    );
}

#[test]
fn direct_component_identity_comes_from_exact_witness() {
    let parent = definition("parent", 1);
    let child = definition("child", 2);
    let adapted = AdaptedTransportAbi {
        // Deliberately reverse semantic-key and dependency order.
        components: vec![
            parent_component_bridge("Parent", parent, child),
            with_browser_effect(component_bridge("Child", child, &[])),
        ],
        helpers: vec![],
    };

    let plan = link_semantic_plan(LinkUnitId::derive("root"), &adapted).unwrap();
    let parent = plan
        .components
        .iter()
        .find(|component| component.semantic_key == component_key("Parent"))
        .unwrap();
    assert_eq!(parent.component_edges.len(), 1);
    assert_eq!(parent.component_edges[0].target_definition, child);
    assert_eq!(parent.component_edges[0].target, component_key("Child"));
    assert_eq!(parent.component_edges[0].source_edge, [77; 32]);

    let parent_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("Parent"))
        .unwrap();
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.parent == Some(parent_root.candidate))
        .unwrap();
    assert_eq!(child_site.definition, child);
    assert_eq!(child_site.component, component_key("Child"));
    assert_eq!(child_site.structural_path, [ComponentEdgeId::new(0)]);

    let json = semantic_plan_json(&plan).expect("static component lifecycle should serialize");
    let parent_json = json["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| {
            component["semantic_key_canonical"] == canonical_hex(&component_key("Parent"))
        })
        .unwrap();
    assert_eq!(
        parent_json["component_edges"][0]["lifecycle"],
        serde_json::json!({"kind": "static_single"})
    );
}

#[test]
fn semantic_plan_carries_prop_timing_and_current_callback_dispatch() {
    let parent = definition("live-parent", 1);
    let child = definition("live-child", 2);
    let (parent_bridge, child_bridge) = live_value_and_callback_bridges(
        parent,
        child,
        TemporalPropMode::SeedOnce(PropSiteId::new(0)),
    );
    let adapted = AdaptedTransportAbi {
        components: vec![parent_bridge, child_bridge],
        helpers: vec![],
    };

    let plan = link_semantic_plan(LinkUnitId::derive("live-root"), &adapted).unwrap();
    assert_eq!(plan.component_inputs.live_props.len(), 1);
    assert_eq!(plan.component_inputs.live_callbacks.len(), 1);
    let live_prop = plan.component_inputs.live_props[0];
    let live_callback = plan.component_inputs.live_callbacks[0];
    assert_eq!(
        live_prop.temporal,
        TemporalPropMode::SeedOnce(PropSiteId::new(0))
    );
    assert_eq!(
        live_callback.dispatch,
        LinkedCallbackDispatch::ResolveCurrent
    );
    assert_eq!(live_callback.shape, ComponentCallbackShape::Required);
    assert_eq!(live_callback.child_value, LocalValueId::new(1));
    assert_eq!(live_prop.parent, live_callback.parent);
    assert_eq!(live_prop.child, live_callback.child);
    assert_eq!(
        plan.owners.candidate_to_owner[&live_prop.parent],
        plan.owners.candidate_to_owner[&live_prop.child],
        "live value and callback records must drive the same owner coupling"
    );

    let json = semantic_plan_json(&plan).unwrap();
    assert_eq!(
        json["component_inputs"]["live_props"][0]["temporal"]["kind"],
        "seed_once"
    );
    assert_eq!(
        json["component_inputs"]["live_props"][0]["temporal"]["seed_site_id"],
        0
    );
    assert_eq!(
        json["component_inputs"]["live_callbacks"][0]["dispatch"],
        "resolve_current"
    );
    assert_eq!(
        json["component_inputs"]["live_callbacks"][0]["callback_shape"],
        "required"
    );
    assert_eq!(
        json["component_inputs"]["live_callbacks"][0]["child_value_id"],
        1
    );
    let child_codegen = json["component_codegen"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["asset_id"] == "fixture_livechild")
        .expect("child component codegen record");
    assert_eq!(
        child_codegen["fragment_root"],
        serde_json::json!({"kind": "exact_one_element"})
    );
    assert_eq!(
        child_codegen["inbound_fields"],
        serde_json::json!([
            {
                "field_id": 0,
                "producer_value_id": 0,
                "canonical_value_id": 0,
                "crossing_hook_id": 0,
                "callback_slot_id": null
            },
            {
                "field_id": 1,
                "producer_value_id": 1,
                "canonical_value_id": 1,
                "crossing_hook_id": 1,
                "callback_slot_id": 0
            }
        ])
    );
    assert_eq!(
        child_codegen["inbound_targets"],
        serde_json::json!([{
            "prop_site_id": 0,
            "field_id": 0,
            "producer_signal_id": 2,
            "canonical_signal_value_id": 2
        }])
    );
    assert_eq!(
        child_codegen["signals"],
        serde_json::json!([{
            "producer_signal_id": 2,
            "canonical_value_id": 2
        }])
    );
    assert_eq!(
        child_codegen["callback_slots"],
        serde_json::json!([{
            "producer_callback_slot_id": 0,
            "canonical_value_id": 1
        }])
    );

    let mut missing_callback = plan.clone();
    missing_callback.component_inputs.live_callbacks.clear();
    assert!(matches!(
        validate_semantic_plan(&missing_callback),
        Err(SemanticPlanValidationError::ComponentInputsMismatch)
    ));
    let mut split_owner = plan.clone();
    split_owner.owners.candidate_to_owner.clear();
    split_owner.owners.owners.clear();
    assert!(matches!(
        validate_semantic_plan(&split_owner),
        Err(SemanticPlanValidationError::OwnerPlanMismatch)
    ));

    let (live_parent, live_child) =
        live_value_and_callback_bridges(parent, child, TemporalPropMode::LiveRead);
    let live_plan = link_semantic_plan(
        LinkUnitId::derive("live-root"),
        &AdaptedTransportAbi {
            components: vec![live_parent, live_child],
            helpers: vec![],
        },
    )
    .unwrap();
    assert_ne!(
        plan.plan_id, live_plan.plan_id,
        "changing SeedOnce to LiveRead must change the exact semantic plan"
    );
}

#[test]
fn standalone_live_callback_participant_requires_context_and_has_no_owner() {
    let parent = definition("latent-live-parent", 1);
    let child = definition("latent-live-child", 2);
    let (_, child_bridge) =
        live_value_and_callback_bridges(parent, child, TemporalPropMode::LiveRead);

    let plan = link_semantic_plan(
        LinkUnitId::derive("latent-live-child-root"),
        &AdaptedTransportAbi {
            components: vec![child_bridge],
            helpers: vec![],
        },
    )
    .unwrap();

    assert!(
        plan.sites.is_empty(),
        "a current callback has no standalone browser source"
    );
    assert!(matches!(
        plan.root_decisions.as_slice(),
        [StandaloneRootDecision {
            disposition: StandaloneRootDisposition::ContextRequired { missing },
            ..
        }] if matches!(
            missing.as_slice(),
            [StandaloneContextRequirement::Callback(requirement)]
                if requirement.consumer == component_key("LiveChild")
                    && requirement.structural_path.is_empty()
                    && requirement.field == ComponentFieldId::new(1)
                    && requirement.crossing_hook == CrossingHookId::new(1)
                    && requirement.shape == ComponentCallbackShape::Required
        )
    ));
    assert!(
        plan.owners.owners.is_empty(),
        "a context-required callback participant cannot form a standalone owner"
    );
    assert!(plan.owners.candidate_to_owner.is_empty());
    validate_semantic_plan(&plan).expect("the latent owner plan must relink identically");
}

#[test]
fn standalone_optional_callback_participant_is_admitted() {
    let parent = definition("optional-live-parent", 1);
    let child = definition("optional-live-child", 2);
    let (_, mut child_bridge) =
        live_value_and_callback_bridges(parent, child, TemporalPropMode::LiveRead);
    let AbiBridgeResult::Canonical(bridge) = &mut child_bridge else {
        unreachable!("fixture child is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture child contains a component descriptor")
    };
    body.fields
        .get_mut(&ComponentFieldId::new(1))
        .expect("fixture callback field")
        .callback_shape = Some(ComponentCallbackShape::Optional);
    bridge.envelope.refresh_hashes();

    let plan = link_semantic_plan(
        LinkUnitId::derive("optional-live-child-root"),
        &AdaptedTransportAbi {
            components: vec![child_bridge],
            helpers: vec![],
        },
    )
    .expect("an absent optional callback is a valid standalone value");

    assert_eq!(plan.sites.len(), 1);
    assert!(matches!(
        plan.root_decisions.as_slice(),
        [StandaloneRootDecision {
            disposition: StandaloneRootDisposition::Admitted { candidate },
            ..
        }] if *candidate == plan.sites[0].candidate
    ));
    validate_semantic_plan(&plan).expect("the optional callback plan must relink exactly");
}

#[test]
fn writable_parent_fuses_latent_live_read_child_into_one_owner() {
    let parent = definition("writable-live-parent", 1);
    let child = definition("writable-live-child", 2);
    let (parent_bridge, child_bridge) =
        live_value_and_callback_bridges(parent, child, TemporalPropMode::LiveRead);

    let plan = link_semantic_plan(
        LinkUnitId::derive("writable-live-root"),
        &AdaptedTransportAbi {
            components: vec![parent_bridge, child_bridge],
            helpers: vec![],
        },
    )
    .unwrap();

    let parent_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("LiveParent"))
        .expect("standalone parent site");
    let nested_child = plan
        .sites
        .iter()
        .find(|site| site.parent == Some(parent_root.candidate))
        .expect("child site nested under the writable parent");
    assert_eq!(plan.owners.owners.len(), 1);
    assert_eq!(
        plan.owners.owners[0].candidates,
        vec![parent_root.candidate, nested_child.candidate],
        "the live prop edge must fuse the latent participant into the writable parent owner"
    );
    assert!(plan.root_decisions.iter().any(|decision| {
        decision.component == component_key("LiveChild")
            && matches!(
                &decision.disposition,
                StandaloneRootDisposition::ContextRequired { missing }
                    if matches!(
                        missing.as_slice(),
                        [StandaloneContextRequirement::Callback(requirement)]
                            if requirement.field == ComponentFieldId::new(1)
                                && requirement.shape == ComponentCallbackShape::Required
                    )
            )
    }));
    validate_semantic_plan(&plan).expect("the fused owner plan must relink identically");
}

#[test]
fn repeated_static_child_definition_gets_one_fresh_site_per_edge() {
    let parent = definition("parent", 1);
    let child = definition("child", 2);
    let adapted = AdaptedTransportAbi {
        components: vec![
            component_with_children_bridge("Parent", parent, &[(child, 70), (child, 71)]),
            with_browser_effect(component_bridge("Child", child, &[])),
        ],
        helpers: vec![],
    };

    let plan = link_semantic_plan(LinkUnitId::derive("root"), &adapted).unwrap();
    assert_eq!(plan.components.len(), 2, "definitions stay deduplicated");
    let parent_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("Parent"))
        .unwrap();
    let child_sites = plan
        .sites
        .iter()
        .filter(|site| site.parent == Some(parent_root.candidate))
        .collect::<Vec<_>>();
    assert_eq!(child_sites.len(), 2);
    assert_ne!(child_sites[0].candidate, child_sites[1].candidate);
    assert_eq!(
        child_sites
            .iter()
            .map(|site| site.structural_path.clone())
            .collect::<Vec<_>>(),
        [vec![ComponentEdgeId::new(0)], vec![ComponentEdgeId::new(1)],]
    );
    assert_eq!(
        plan.sites
            .iter()
            .filter(|site| site.component == component_key("Child"))
            .count(),
        3,
        "two child uses plus the child's standalone root"
    );
    let child_candidates = plan
        .sites
        .iter()
        .filter(|site| site.component == component_key("Child"))
        .map(|site| site.candidate)
        .collect::<BTreeSet<_>>();
    assert_eq!(child_candidates.len(), 3);
    assert_eq!(plan.owners.owners.len(), 3);
    assert_eq!(
        plan.owners
            .candidate_to_owner
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        child_candidates,
        "static nesting keeps all three component uses as separate owners"
    );
}

#[test]
fn separate_standalone_roots_never_share_a_child_site() {
    let first = definition("first-parent", 1);
    let second = definition("second-parent", 2);
    let child = definition("child", 3);
    let adapted = AdaptedTransportAbi {
        components: vec![
            component_with_children_bridge("First", first, &[(child, 61)]),
            component_with_children_bridge("Second", second, &[(child, 62)]),
            component_bridge("Child", child, &[]),
        ],
        helpers: vec![],
    };

    let plan = link_semantic_plan(LinkUnitId::derive("root"), &adapted).unwrap();
    let first_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("First"))
        .unwrap();
    let second_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("Second"))
        .unwrap();
    let first_child = plan
        .sites
        .iter()
        .find(|site| site.parent == Some(first_root.candidate))
        .unwrap();
    let second_child = plan
        .sites
        .iter()
        .find(|site| site.parent == Some(second_root.candidate))
        .unwrap();
    assert_ne!(first_child.candidate, second_child.candidate);
    assert_eq!(first_child.root, component_key("First"));
    assert_eq!(second_child.root, component_key("Second"));
}

#[test]
fn immutable_exact_one_slot_keeps_the_explicit_island_as_an_independent_owner() {
    let parent = definition("static-slot-parent", 81);
    let child = definition("static-slot-child", 82);
    let (parent_bridge, child_bridge) =
        immutable_static_slot_bridges(parent, child, false).unwrap();
    let adapted = AdaptedTransportAbi {
        components: vec![parent_bridge, child_bridge],
        helpers: vec![],
    };

    let plan = link_semantic_plan(LinkUnitId::derive("static-slot"), &adapted).unwrap();
    let parent_root = plan
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("Page"))
        .unwrap();
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.parent == Some(parent_root.candidate))
        .unwrap();
    assert_eq!(child_site.component, component_key("Disclosure"));
    assert!(
            plan.sites.iter().all(|site| {
                site.parent.is_some() || site.component != component_key("Disclosure")
            }),
            "a component whose immutable slot lacks a standalone Exact(1) proof must not form a root site"
        );
    assert!(
        !plan
            .owners
            .candidate_to_owner
            .contains_key(&parent_root.candidate),
        "the static parent has no browser obligation"
    );
    assert!(
        plan.owners
            .candidate_to_owner
            .contains_key(&child_site.candidate),
        "the disclosure owns its browser behavior independently"
    );
    assert!(plan.component_inputs.live_props.is_empty());
    assert!(plan.component_inputs.live_callbacks.is_empty());
    validate_semantic_plan(&plan).expect("root gating must relink identically");
}

#[test]
fn static_only_island_slot_rejects_client_reconstruction() {
    let parent = definition("live-slot-parent", 83);
    let child = definition("live-slot-child", 84);
    assert!(matches!(
        immutable_static_slot_bridges(parent, child, true),
        Err(AbiValidationError::LiveBindingAcrossIndependentIsland {
            edge,
            field_name,
        }) if edge == ComponentEdgeId::new(0) && field_name == "children"
    ));
}

#[test]
fn nested_site_paths_and_plan_ids_are_order_invariant() {
    let parent = definition("parent", 1);
    let middle = definition("middle", 2);
    let leaf = definition("leaf", 3);
    let parent_result = component_with_children_bridge("Parent", parent, &[(middle, 51)]);
    let middle_result = component_with_children_bridge("Middle", middle, &[(leaf, 52)]);
    let leaf_result = component_bridge("Leaf", leaf, &[]);
    let forward = AdaptedTransportAbi {
        components: vec![
            parent_result.clone(),
            middle_result.clone(),
            leaf_result.clone(),
        ],
        helpers: vec![],
    };
    let reversed = AdaptedTransportAbi {
        components: vec![leaf_result, middle_result, parent_result],
        helpers: vec![],
    };

    let forward = link_semantic_plan(LinkUnitId::derive("root"), &forward).unwrap();
    let reversed = link_semantic_plan(LinkUnitId::derive("root"), &reversed).unwrap();
    assert_eq!(forward.plan_id, reversed.plan_id);
    assert_eq!(forward.sites, reversed.sites);

    let parent_root = forward
        .sites
        .iter()
        .find(|site| site.parent.is_none() && site.component == component_key("Parent"))
        .unwrap();
    let middle_site = forward
        .sites
        .iter()
        .find(|site| site.parent == Some(parent_root.candidate))
        .unwrap();
    let leaf_site = forward
        .sites
        .iter()
        .find(|site| site.parent == Some(middle_site.candidate))
        .unwrap();
    assert_eq!(
        leaf_site.structural_path,
        [ComponentEdgeId::new(0), ComponentEdgeId::new(0)]
    );
}

#[test]
fn recursive_static_component_graph_fails_with_the_closing_path() {
    let alpha = definition("alpha", 1);
    let beta = definition("beta", 2);
    let adapted = AdaptedTransportAbi {
        components: vec![
            component_with_children_bridge("Alpha", alpha, &[(beta, 41)]),
            component_with_children_bridge("Beta", beta, &[(alpha, 42)]),
        ],
        helpers: vec![],
    };

    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &adapted),
        Err(SemanticLinkError::RecursiveStaticComponentGraph {
            root,
            parent,
            edge,
            target,
            path,
        }) if root == component_key("Alpha")
            && parent == component_key("Beta")
            && edge == ComponentEdgeId::new(0)
            && target == component_key("Alpha")
            && path == [
                ComponentEdgeId::new(0),
                ComponentEdgeId::new(0),
            ]
    ));
}

fn mark_first_edge_conditional(result: &mut AbiBridgeResult, branch: ComponentLifecycleBranch) {
    let AbiBridgeResult::Canonical(bridge) = result else {
        unreachable!()
    };
    let DescriptorBody::Component(component) = &mut bridge.envelope.body else {
        unreachable!()
    };
    let origin = OriginId::new(0);
    let value = LocalValueId::new(0);
    let control = LocalControlRegionId::new(0);
    let effect = LocalEffectId::new(0);
    let edge = ComponentEdgeId::new(0);
    component.origin_permission = OriginPermission::OriginCapable;
    component.origins.insert(origin, OriginKind::WritableSignal);
    component.values.insert(value);
    component.control_regions.insert(control);
    component
        .effects
        .insert(effect, EffectKind::DomStructuralUpdate);
    component.dependencies.extend([
        (
            DependencyEdgeId::new(0),
            DependencyEdge::new(
                DependencyEdgeKind::Data,
                DependencyEndpoint::Origin(origin),
                DependencyEndpoint::Value(value),
                origin,
            ),
        ),
        (
            DependencyEdgeId::new(1),
            DependencyEdge::new(
                DependencyEdgeKind::Control,
                DependencyEndpoint::Value(value),
                DependencyEndpoint::Control(control),
                origin,
            ),
        ),
        (
            DependencyEdgeId::new(2),
            DependencyEdge::new(
                DependencyEdgeKind::Effect,
                DependencyEndpoint::Origin(origin),
                DependencyEndpoint::Effect(effect),
                origin,
            ),
        ),
        (
            DependencyEdgeId::new(3),
            DependencyEdge::new(
                DependencyEdgeKind::Control,
                DependencyEndpoint::Control(control),
                DependencyEndpoint::Effect(effect),
                origin,
            ),
        ),
        (
            DependencyEdgeId::new(4),
            DependencyEdge::new(
                DependencyEdgeKind::Control,
                DependencyEndpoint::Control(control),
                DependencyEndpoint::Component(edge),
                origin,
            ),
        ),
    ]);
    component
        .direct_components
        .get_mut(&edge)
        .unwrap()
        .lifecycle_control = Some(ComponentLifecycleControl {
        control,
        predicate: value,
        predicate_origin: origin,
        branch,
    });
    bridge.envelope.header.capabilities = bridge
        .envelope
        .header
        .capabilities
        .union(CapabilitySet::TYPED_DEPENDENCIES)
        .union(CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE);
    bridge
        .component_codegen_mut()
        .expect("component fixture has codegen")
        .signals
        .push(crate::abi_adapter::ComponentCodegenSignal {
            producer_signal_id: value.index(),
            canonical_value: value,
        });
    bridge.envelope.refresh_hashes();
}

#[test]
fn conditional_component_sites_preserve_then_else_and_control_provenance() {
    let then_parent = definition("then-parent", 1);
    let then_child = definition("then-child", 2);
    let else_parent = definition("else-parent", 3);
    let else_child = definition("else-child", 4);
    let mut then_result =
        component_with_children_bridge("ThenParent", then_parent, &[(then_child, 31)]);
    let mut else_result =
        component_with_children_bridge("ElseParent", else_parent, &[(else_child, 32)]);
    mark_first_edge_conditional(&mut then_result, ComponentLifecycleBranch::Then);
    mark_first_edge_conditional(&mut else_result, ComponentLifecycleBranch::Else);
    let plan = link_semantic_plan(
        LinkUnitId::derive("conditional-site-provenance"),
        &AdaptedTransportAbi {
            components: vec![
                then_result,
                component_bridge("ThenChild", then_child, &[]),
                else_result,
                component_bridge("ElseChild", else_child, &[]),
            ],
            helpers: vec![],
        },
    )
    .expect("conditional-single sites should reach semantic linking");

    for (root, branch) in [
        (component_key("ThenParent"), ComponentLifecycleBranch::Then),
        (component_key("ElseParent"), ComponentLifecycleBranch::Else),
    ] {
        let site = plan
            .sites
            .iter()
            .find(|site| {
                site.root == root && site.structural_path.as_slice() == [ComponentEdgeId::new(0)]
            })
            .expect("conditional child site");
        assert_eq!(
            site.incoming_lifecycle,
            Some(ComponentLifecycleControl {
                control: LocalControlRegionId::new(0),
                predicate: LocalValueId::new(0),
                predicate_origin: OriginId::new(0),
                branch,
            })
        );
        assert_eq!(site.reactive_lifecycle_root, Some(site.candidate));

        let json = semantic_plan_json(&plan).expect("conditional plan should serialize");
        let site_json = json["component_sites"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["candidate_id"] == site.candidate.index())
            .expect("serialized conditional site");
        assert_eq!(
            site_json["incoming_lifecycle"],
            serde_json::json!({
                "kind": "conditional_single",
                "control": 0,
                "predicate_value": 0,
                "predicate_origin": 0,
                "branch": match branch {
                    ComponentLifecycleBranch::Then => "then",
                    ComponentLifecycleBranch::Else => "else",
                },
            })
        );
        assert_eq!(
            site_json["reactive_lifecycle_root_candidate_id"],
            site.candidate.index()
        );
    }
    validate_semantic_plan(&plan).expect("conditional provenance should relink exactly");
}

#[test]
fn conditional_lifecycle_fuses_every_static_descendant_into_one_owner() {
    let parent = definition("lifecycle-parent", 1);
    let child = definition("lifecycle-child", 2);
    let grandchild = definition("lifecycle-grandchild", 3);
    let mut parent_result =
        component_with_children_bridge("LifecycleParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let plan = link_semantic_plan(
        LinkUnitId::derive("conditional-descendant-owner"),
        &AdaptedTransportAbi {
            components: vec![
                parent_result,
                component_with_children_bridge("LifecycleChild", child, &[(grandchild, 32)]),
                component_bridge("LifecycleGrandchild", grandchild, &[]),
            ],
            helpers: vec![],
        },
    )
    .expect("conditional subtree should fuse");

    let subtree = plan
        .sites
        .iter()
        .filter(|site| site.root == component_key("LifecycleParent"))
        .collect::<Vec<_>>();
    assert_eq!(subtree.len(), 3);
    let lifecycle_root = subtree
        .iter()
        .find(|site| site.structural_path.len() == 1)
        .expect("conditional child");
    let grandchild_site = subtree
        .iter()
        .find(|site| site.structural_path.len() == 2)
        .expect("static descendant");
    assert_eq!(
        grandchild_site.reactive_lifecycle_root,
        Some(lifecycle_root.candidate)
    );

    let owner = plan
        .owners
        .candidate_to_owner
        .get(&subtree[0].candidate)
        .copied()
        .expect("browser-active parent owner");
    assert!(subtree
        .iter()
        .all(|site| { plan.owners.candidate_to_owner.get(&site.candidate) == Some(&owner) }));
    let owner_candidates = &plan.owners.owners[owner.index() as usize].candidates;
    assert_eq!(
        owner_candidates.iter().copied().collect::<BTreeSet<_>>(),
        subtree
            .iter()
            .map(|site| site.candidate)
            .collect::<BTreeSet<_>>()
    );
    validate_semantic_plan(&plan).expect("transitive lifecycle fusion should relink");
}

#[test]
fn conditional_lifecycle_rejects_an_island_cut_in_its_static_subtree() {
    let parent = definition("cut-parent", 1);
    let child = definition("cut-child", 2);
    let grandchild = definition("cut-grandchild", 3);
    let mut parent_result = component_with_children_bridge("CutParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let mut child_result = component_with_children_bridge("CutChild", child, &[(grandchild, 32)]);
    let AbiBridgeResult::Canonical(child_bridge) = &mut child_result else {
        unreachable!()
    };
    let DescriptorBody::Component(child_body) = &mut child_bridge.envelope.body else {
        unreachable!()
    };
    child_body
        .direct_components
        .get_mut(&ComponentEdgeId::new(0))
        .unwrap()
        .boundary = ComponentBoundary::IndependentIsland;
    child_bridge.envelope.refresh_hashes();

    let error = link_semantic_plan(
        LinkUnitId::derive("conditional-descendant-cut"),
        &AdaptedTransportAbi {
            components: vec![
                parent_result,
                child_result,
                component_bridge("CutGrandchild", grandchild, &[]),
            ],
            helpers: vec![],
        },
    )
    .expect_err("a lifecycle-controlled subtree cannot cross an island cut");
    assert!(matches!(
        error,
        SemanticLinkError::ComponentGraph(ComponentGraphLinkError::OwnerPartition(
            rsjs_ir::link::OwnerPartitionError::CouplingCrossesCut {
                cut: rsjs_ir::link::OwnerCutKind::IndependentIsland,
                coupling_path,
                ..
            }
        )) if coupling_path
            .iter()
            .all(|step| step.kind == OwnerCouplingKind::ReactiveLifecycle)
    ));
}

#[test]
fn conditional_lifecycle_reports_descendant_strategy_conflicts() {
    let parent = definition("strategy-parent", 1);
    let child = definition("strategy-child", 2);
    let grandchild = definition("strategy-grandchild", 3);
    let mut parent_result =
        component_with_children_bridge("StrategyParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let AbiBridgeResult::Canonical(parent_bridge) = &mut parent_result else {
        unreachable!()
    };
    let DescriptorBody::Component(parent_body) = &mut parent_bridge.envelope.body else {
        unreachable!()
    };
    parent_body.hydration = rsjs_ir::abi::HydrationConstraint::Visible;
    parent_bridge.envelope.refresh_hashes();

    let child_result = component_with_children_bridge("StrategyChild", child, &[(grandchild, 32)]);
    let mut grandchild_result = component_bridge("StrategyGrandchild", grandchild, &[]);
    let AbiBridgeResult::Canonical(grandchild_bridge) = &mut grandchild_result else {
        unreachable!()
    };
    let DescriptorBody::Component(grandchild_body) = &mut grandchild_bridge.envelope.body else {
        unreachable!()
    };
    grandchild_body.hydration = rsjs_ir::abi::HydrationConstraint::Idle;
    grandchild_bridge.envelope.refresh_hashes();

    let error = link_semantic_plan(
        LinkUnitId::derive("conditional-descendant-strategy"),
        &AdaptedTransportAbi {
            components: vec![parent_result, child_result, grandchild_result],
            helpers: vec![],
        },
    )
    .expect_err("a lifecycle owner cannot have incompatible activation strategies");
    assert!(matches!(
        error,
        SemanticLinkError::ComponentGraph(ComponentGraphLinkError::OwnerPartition(
            rsjs_ir::link::OwnerPartitionError::StrategyConflict {
                first_strategy: OwnerActivationStrategy::Visible,
                second_strategy: OwnerActivationStrategy::Idle,
                coupling_path,
                ..
            }
        )) if coupling_path.len() == 2
            && coupling_path
                .iter()
                .all(|step| step.kind == OwnerCouplingKind::ReactiveLifecycle)
    ));
}

#[test]
fn nested_conditional_component_sites_remain_rejected() {
    let parent = definition("nested-parent", 1);
    let child = definition("nested-child", 2);
    let grandchild = definition("nested-grandchild", 3);
    let mut parent_result = component_with_children_bridge("NestedParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let mut child_result =
        component_with_children_bridge("NestedChild", child, &[(grandchild, 32)]);
    mark_first_edge_conditional(&mut child_result, ComponentLifecycleBranch::Else);

    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("nested-conditional-site"),
            &AdaptedTransportAbi {
                components: vec![
                    parent_result,
                    child_result,
                    component_bridge("NestedGrandchild", grandchild, &[]),
                ],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::UnsupportedNestedConditionalComponentSite {
            root,
            parent,
            edge,
            control,
            branch,
            path,
            ..
        }) if root == component_key("NestedParent")
            && parent == component_key("NestedChild")
            && edge == ComponentEdgeId::new(0)
            && control == LocalControlRegionId::new(0)
            && branch == ComponentLifecycleBranch::Else
            && path == [ComponentEdgeId::new(0), ComponentEdgeId::new(0)]
    ));
}

#[test]
fn semantic_plan_validation_rejects_rehashed_site_lifecycle_tampering() {
    let parent = definition("tamper-parent", 1);
    let child = definition("tamper-child", 2);
    let mut parent_result = component_with_children_bridge("TamperParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let mut plan = link_semantic_plan(
        LinkUnitId::derive("conditional-plan-tamper"),
        &AdaptedTransportAbi {
            components: vec![parent_result, component_bridge("TamperChild", child, &[])],
            helpers: vec![],
        },
    )
    .unwrap();
    let site = plan
        .sites
        .iter_mut()
        .find(|site| site.root == component_key("TamperParent") && site.structural_path.len() == 1)
        .expect("conditional child site");
    site.incoming_lifecycle
        .as_mut()
        .expect("conditional lifecycle")
        .branch = ComponentLifecycleBranch::Else;

    let payload = semantic_plan_payload(
        &plan.components,
        &plan.sites,
        &plan.helpers,
        &plan.reachable_helpers,
        &plan.owners,
        &plan.component_inputs,
        &plan.shared_state,
        &plan.root_decisions,
        &plan.shared_state_providers,
        &plan.shared_state_requirements,
        &plan.node_ref_aliases,
    )
    .expect("coherently mutated conditional plan payload");
    plan.material.payload = SemanticHash::derive(&payload);
    plan.plan_id = plan.material.plan_id();

    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::InvalidChildComponentSite { .. })
    ));
}

#[test]
fn semantic_plan_validation_rejects_rehashed_predicate_dependency_tampering() {
    let parent = definition("predicate-tamper-parent", 1);
    let child = definition("predicate-tamper-child", 2);
    let mut parent_result =
        component_with_children_bridge("PredicateTamperParent", parent, &[(child, 31)]);
    mark_first_edge_conditional(&mut parent_result, ComponentLifecycleBranch::Then);
    let mut plan = link_semantic_plan(
        LinkUnitId::derive("conditional-predicate-plan-tamper"),
        &AdaptedTransportAbi {
            components: vec![
                parent_result,
                component_bridge("PredicateTamperChild", child, &[]),
            ],
            helpers: vec![],
        },
    )
    .unwrap();
    let parent = plan
        .components
        .iter_mut()
        .find(|component| component.definition == parent)
        .expect("conditional parent component");
    let DescriptorBody::Component(body) = &mut parent.descriptor.body else {
        panic!("expected component descriptor");
    };
    body.dependencies.insert(
        DependencyEdgeId::new(5),
        DependencyEdge::new(
            DependencyEdgeKind::Control,
            DependencyEndpoint::Value(LocalValueId::new(0)),
            DependencyEndpoint::Control(LocalControlRegionId::new(0)),
            OriginId::new(0),
        ),
    );
    parent.descriptor.refresh_hashes();

    plan.material.component_descriptors = plan
        .components
        .iter()
        .map(|component| component.descriptor.semantic_hash)
        .collect();
    let payload = semantic_plan_payload(
        &plan.components,
        &plan.sites,
        &plan.helpers,
        &plan.reachable_helpers,
        &plan.owners,
        &plan.component_inputs,
        &plan.shared_state,
        &plan.root_decisions,
        &plan.shared_state_providers,
        &plan.shared_state_requirements,
        &plan.node_ref_aliases,
    )
    .expect("coherently rehashed predicate-tampered plan payload");
    plan.material.payload = SemanticHash::derive(&payload);
    plan.plan_id = plan.material.plan_id();

    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::InvalidCanonicalDescriptor {
            source:
                AbiValidationError::LifecyclePredicateDependencyMismatch {
                    actual,
                    ..
                },
            ..
        }) if actual.len() == 2
    ));
}

#[test]
fn duplicate_component_semantic_keys_fail_closed() {
    let first = definition("first", 1);
    let second = definition("second", 2);
    let adapted = AdaptedTransportAbi {
        components: vec![
            component_bridge("Badge", first, &[]),
            component_bridge("Badge", second, &[]),
        ],
        helpers: vec![],
    };

    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &adapted),
        Err(SemanticLinkError::DuplicateComponentKey { first: a, second: b, .. })
            if (a == first && b == second) || (a == second && b == first)
    ));
}

#[test]
fn duplicate_helper_semantic_keys_fail_closed() {
    let first = definition("first-helper", 1);
    let second = definition("second-helper", 2);
    let adapted = AdaptedTransportAbi {
        components: vec![],
        helpers: vec![
            helper_bridge("format", first, "fn(u32)", &[]),
            helper_bridge("format", second, "fn(u32)", &[]),
        ],
    };

    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &adapted),
        Err(SemanticLinkError::DuplicateHelperKey {
            first: a,
            second: b,
            ..
        }) if (a == first && b == second) || (a == second && b == first)
    ));
}

#[test]
fn unsupported_adapter_result_stops_the_semantic_link() {
    let definition = definition("unsupported", 1);
    let adapted = AdaptedTransportAbi {
        components: vec![AbiBridgeResult::Unsupported(UnsupportedDescriptor {
            kind: DescriptorKind::Component,
            definition,
            semantic_path: None,
            gaps: vec![CompatibilityGap::MissingResolvedSemanticIdentity],
        })],
        helpers: vec![],
    };

    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &adapted),
        Err(SemanticLinkError::Unsupported(descriptor))
            if descriptor.definition == definition
    ));
}

#[test]
fn canonical_mutation_runtime_enters_linked_owner_plans() {
    let mutation_definition = definition("mutation", 1);
    let mut component = component_bridge("RuntimeFeature", mutation_definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut component else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    body.origin_permission = OriginPermission::OriginCapable;
    body.origins.insert(OriginId::new(0), OriginKind::Mutation);
    body.effects
        .insert(LocalEffectId::new(0), EffectKind::Mutation);
    bridge
        .component_codegen_mut()
        .expect("component fixture has codegen")
        .uses_mutations = true;
    bridge.envelope.refresh_hashes();

    let mutation_plan = link_semantic_plan(
        LinkUnitId::derive("mutation"),
        &AdaptedTransportAbi {
            components: vec![component],
            helpers: vec![],
        },
    )
    .expect("canonical mutation graphs carry their fragment runtime feature");
    assert!(
        mutation_plan.components[0].codegen.uses_mutations,
        "the exact reachable mutation bit must survive semantic linking"
    );

    let definition = definition("browser-capability", 2);
    let mut browser = component_bridge("BrowserCapability", definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut browser else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    body.origin_permission = OriginPermission::OriginCapable;
    body.origins
        .insert(OriginId::new(0), OriginKind::BrowserCapabilityRead);
    body.effects
        .insert(LocalEffectId::new(0), EffectKind::BrowserRead);
    bridge.envelope.refresh_hashes();
    link_semantic_plan(
        LinkUnitId::derive("browser-capability"),
        &AdaptedTransportAbi {
            components: vec![browser],
            helpers: vec![],
        },
    )
    .expect("browser capabilities are emitted by component fragments");
}

#[test]
fn static_direct_node_ref_handle_alias_is_occurrence_specific_and_blocks_child_root() {
    let parent_definition = definition("node-ref-parent", 1);
    let child_definition = definition("node-ref-child", 2);
    let (parent, child) = node_ref_handle_alias_bridges(parent_definition, child_definition);
    let mut plan = link_semantic_plan(
        LinkUnitId::derive("node-ref-handle-alias"),
        &AdaptedTransportAbi {
            components: vec![child, parent],
            helpers: vec![],
        },
    )
    .expect("a rustc-resolved static direct NodeRef handle aliases exactly");

    assert_eq!(
        plan.node_ref_aliases,
        vec![LinkedNodeRefHandleAlias {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            parent_node_ref: 7,
            parent_value: LocalValueId::new(0),
            child: OwnerCandidateId::new(1),
            field: ComponentFieldId::new(0),
            crossing_hook: CrossingHookId::new(0),
            child_node_ref: 11,
            child_value: LocalValueId::new(0),
            bind_target: Some(ComponentNodeRefBindTarget::Input),
        }]
    );
    assert_eq!(
        plan.owners.candidate_to_owner[&OwnerCandidateId::new(0)],
        plan.owners.candidate_to_owner[&OwnerCandidateId::new(1)]
    );

    let child_root = plan
        .root_decisions
        .iter()
        .find(|decision| decision.definition == child_definition)
        .expect("child root decision");
    assert_eq!(
        child_root.disposition,
        StandaloneRootDisposition::ContextRequired {
            missing: vec![StandaloneContextRequirement::NodeRef(
                StandaloneNodeRefRequirement {
                    consumer: component_key("NodeRefChild"),
                    structural_path: vec![],
                    field: ComponentFieldId::new(0),
                    crossing_hook: CrossingHookId::new(0),
                    node_ref: 11,
                    bind_target: Some(ComponentNodeRefBindTarget::Input),
                }
            )],
        }
    );

    let json = semantic_plan_json(&plan).expect("NodeRef plan serializes");
    assert_eq!(json["schema_version"], SEMANTIC_LINK_SCHEMA_VERSION);
    assert_eq!(json["node_ref_aliases"][0]["parent_node_ref_id"], 7);
    assert_eq!(json["node_ref_aliases"][0]["child_node_ref_id"], 11);
    assert_eq!(json["node_ref_aliases"][0]["bind_target"], "input");
    let child_json = json["standalone_roots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|root| root["definition"]["record_id"] == hex_bytes(&[2; 32]))
        .expect("child standalone-root JSON");
    assert_eq!(child_json["disposition"]["missing"][0]["kind"], "node_ref");
    validate_semantic_plan(&plan).expect("NodeRef plan should relink exactly");

    plan.node_ref_aliases[0].child_node_ref = 12;
    let payload = semantic_plan_payload(
        &plan.components,
        &plan.sites,
        &plan.helpers,
        &plan.reachable_helpers,
        &plan.owners,
        &plan.component_inputs,
        &plan.shared_state,
        &plan.root_decisions,
        &plan.shared_state_providers,
        &plan.shared_state_requirements,
        &plan.node_ref_aliases,
    )
    .expect("the tampered NodeRef plan still has serializable records");
    plan.material.payload = SemanticHash::derive(&payload);
    plan.plan_id = plan.material.plan_id();
    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::NodeRefAliasTopologyMismatch)
    ));
}

#[test]
fn standalone_shared_state_consumer_is_context_required_and_has_no_root_site() {
    let consumer_definition = definition("shared-state-consumer", 1);
    let schema = SemanticHash::derive("acme.workbench.schema.v1");
    let plan = link_semantic_plan(
        LinkUnitId::derive("shared-state-context-required"),
        &AdaptedTransportAbi {
            components: vec![shared_state_component_bridge(
                "SharedPanel",
                consumer_definition,
                "acme.workbench",
                schema,
            )],
            helpers: vec![],
        },
    )
    .expect("provider-dependent definitions remain linkable as nested components");

    assert!(plan.sites.is_empty());
    assert!(plan.owners.owners.is_empty());
    assert!(plan.shared_state.is_empty());
    assert_eq!(
        plan.root_decisions,
        vec![StandaloneRootDecision {
            definition: consumer_definition,
            component: component_key("SharedPanel"),
            disposition: StandaloneRootDisposition::ContextRequired {
                missing: vec![StandaloneContextRequirement::SharedState(
                    StandaloneSharedStateRequirement {
                        consumer: component_key("SharedPanel"),
                        structural_path: vec![],
                        reference: SharedStateRefId::new(0),
                        type_key: "acme.workbench".to_owned(),
                        schema_abi: schema,
                    },
                )],
            },
        }]
    );
    let json = semantic_plan_json(&plan).expect("context-required plan should serialize");
    assert_eq!(
        json["standalone_roots"][0]["disposition"]["kind"],
        "context_required"
    );
    validate_semantic_plan(&plan).expect("context-required root decision should relink");
}

#[test]
fn provider_root_resolves_consumer_to_exact_lexical_occurrence() {
    let provider_definition = definition("provider-page", 1);
    let consumer_definition = definition("provider-consumer", 2);
    let schema = SemanticHash::derive("acme.workbench.schema.v1");
    let mut plan = link_semantic_plan(
        LinkUnitId::derive("shared-state-provider-root"),
        &AdaptedTransportAbi {
            components: vec![
                shared_state_provider_component_bridge(
                    "ProviderPage",
                    provider_definition,
                    &[(consumer_definition, 41)],
                    &[(0, 0, "acme.workbench", schema)],
                ),
                shared_state_component_bridge(
                    "SharedPanel",
                    consumer_definition,
                    "acme.workbench",
                    schema,
                ),
            ],
            helpers: vec![],
        },
    )
    .expect("the provider page should admit its exact consumer subtree");

    assert_eq!(plan.sites.len(), 2);
    let provider = plan
        .shared_state_providers
        .first()
        .expect("one exact provider occurrence");
    let requirement = plan
        .shared_state_requirements
        .first()
        .expect("one exact provider requirement");
    assert_eq!(provider.host, OwnerCandidateId::new(0));
    assert_eq!(provider.provider_site, ProviderSiteId::new(0));
    assert_eq!(provider.child, OwnerCandidateId::new(1));
    assert_eq!(requirement.consumer, provider.child);
    assert_eq!(requirement.provider_host, provider.host);
    assert_eq!(requirement.provider_site, provider.provider_site);
    assert_eq!(
        plan.shared_state,
        vec![LinkedSharedStateSchema {
            type_key: "acme.workbench".to_owned(),
            schema_abi: schema,
            consumers: vec![rsjs_ir::link::SharedStateConsumer {
                component: OwnerCandidateId::new(1),
                reference: SharedStateRefId::new(0),
                origin: OriginId::new(0),
            }],
        }]
    );
    let consumer_decision = plan
        .root_decisions
        .iter()
        .find(|decision| decision.definition == consumer_definition)
        .expect("consumer root decision");
    assert!(matches!(
        consumer_decision.disposition,
        StandaloneRootDisposition::ContextRequired { .. }
    ));
    let provider_decision = plan
        .root_decisions
        .iter()
        .find(|decision| decision.definition == provider_definition)
        .expect("provider root decision");
    assert_eq!(
        provider_decision.disposition,
        StandaloneRootDisposition::Admitted {
            candidate: OwnerCandidateId::new(0),
        }
    );

    let json = semantic_plan_json(&plan).expect("provider plan should serialize");
    assert_eq!(json["shared_state_providers"][0]["host_candidate_id"], 0);
    assert_eq!(
        json["shared_state_requirements"][0]["consumer_candidate_id"],
        1
    );
    validate_semantic_plan(&plan).expect("the exact SharedState plan should relink");

    plan.shared_state_requirements[0].provider_site = ProviderSiteId::new(1);
    let payload = semantic_plan_payload(
        &plan.components,
        &plan.sites,
        &plan.helpers,
        &plan.reachable_helpers,
        &plan.owners,
        &plan.component_inputs,
        &plan.shared_state,
        &plan.root_decisions,
        &plan.shared_state_providers,
        &plan.shared_state_requirements,
        &plan.node_ref_aliases,
    )
    .expect("the tampered plan still has serializable records");
    plan.material.payload = SemanticHash::derive(&payload);
    plan.plan_id = plan.material.plan_id();
    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::SharedStateProviderTopologyMismatch)
    ));
}

#[test]
fn sibling_provider_sites_remain_distinct_and_nested_provider_shadows_outer() {
    let sibling_page = definition("sibling-provider-page", 1);
    let consumer = definition("sibling-provider-consumer", 2);
    let schema = SemanticHash::derive("acme.workbench.schema.v1");
    let sibling_plan = link_semantic_plan(
        LinkUnitId::derive("shared-state-sibling-providers"),
        &AdaptedTransportAbi {
            components: vec![
                shared_state_provider_component_bridge(
                    "SiblingProviderPage",
                    sibling_page,
                    &[(consumer, 51), (consumer, 52)],
                    &[
                        (0, 0, "acme.workbench", schema),
                        (1, 1, "acme.workbench", schema),
                    ],
                ),
                shared_state_component_bridge(
                    "SiblingConsumer",
                    consumer,
                    "acme.workbench",
                    schema,
                ),
            ],
            helpers: vec![],
        },
    )
    .expect("sibling provider instances with one schema are independent");
    assert_eq!(sibling_plan.shared_state_providers.len(), 2);
    assert_eq!(sibling_plan.shared_state_requirements.len(), 2);
    assert_eq!(
        sibling_plan
            .shared_state_requirements
            .iter()
            .map(|requirement| requirement.provider_site)
            .collect::<Vec<_>>(),
        vec![ProviderSiteId::new(0), ProviderSiteId::new(1)]
    );

    let outer = definition("outer-provider", 3);
    let inner = definition("inner-provider", 4);
    let nested_consumer = definition("nested-provider-consumer", 5);
    let nested_plan = link_semantic_plan(
        LinkUnitId::derive("shared-state-provider-shadow"),
        &AdaptedTransportAbi {
            components: vec![
                shared_state_provider_component_bridge(
                    "OuterProvider",
                    outer,
                    &[(inner, 61)],
                    &[(0, 0, "acme.workbench", schema)],
                ),
                shared_state_provider_component_bridge(
                    "InnerProvider",
                    inner,
                    &[(nested_consumer, 62)],
                    &[(0, 0, "acme.workbench", schema)],
                ),
                shared_state_component_bridge(
                    "NestedConsumer",
                    nested_consumer,
                    "acme.workbench",
                    schema,
                ),
            ],
            helpers: vec![],
        },
    )
    .expect("nearest lexical provider should shadow the outer provider");
    let outer_root = nested_plan
        .root_decisions
        .iter()
        .find_map(|decision| {
            (decision.definition == outer)
                .then_some(&decision.disposition)
                .and_then(|disposition| match disposition {
                    StandaloneRootDisposition::Admitted { candidate } => Some(*candidate),
                    _ => None,
                })
        })
        .expect("outer provider is an admitted root");
    let inner_occurrence = nested_plan
        .shared_state_providers
        .iter()
        .find(|provider| provider.host != outer_root)
        .expect("inner provider occurrence");
    let nested_requirement = nested_plan
        .shared_state_requirements
        .iter()
        .find(|requirement| requirement.consumer == inner_occurrence.child)
        .expect("nested consumer requirement");
    assert_eq!(nested_requirement.provider_host, inner_occurrence.host);
    assert_eq!(
        nested_requirement.provider_site,
        inner_occurrence.provider_site
    );
}

#[test]
fn provider_schema_conflicts_and_dynamic_provider_hosts_fail_closed() {
    let provider = definition("conflicting-provider", 1);
    let consumer = definition("conflicting-consumer", 2);
    let first_schema = SemanticHash::derive("acme.workbench.schema.v1");
    let second_schema = SemanticHash::derive("acme.workbench.schema.v2");
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("shared-state-schema-conflict"),
            &AdaptedTransportAbi {
                components: vec![
                    shared_state_provider_component_bridge(
                        "ConflictingProvider",
                        provider,
                        &[(consumer, 71)],
                        &[(0, 0, "acme.workbench", first_schema)],
                    ),
                    shared_state_component_bridge(
                        "ConflictingConsumer",
                        consumer,
                        "acme.workbench",
                        second_schema,
                    ),
                ],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::SharedStateDeclarationConflict {
            type_key,
            first_schema: a,
            second_schema: b,
            ..
        }) if type_key == "acme.workbench"
            && ((a == first_schema && b == second_schema)
                || (a == second_schema && b == first_schema))
    ));

    let conditional_parent = definition("conditional-provider-parent", 3);
    let provider_host = definition("conditional-provider-host", 4);
    let conditional_consumer = definition("conditional-provider-consumer", 5);
    let mut conditional = component_with_children_bridge(
        "ConditionalProviderParent",
        conditional_parent,
        &[(provider_host, 72)],
    );
    mark_first_edge_conditional(&mut conditional, ComponentLifecycleBranch::Then);
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("dynamic-shared-state-provider"),
            &AdaptedTransportAbi {
                components: vec![
                    conditional,
                    shared_state_provider_component_bridge(
                        "ConditionalProviderHost",
                        provider_host,
                        &[(conditional_consumer, 73)],
                        &[(0, 0, "acme.workbench", first_schema)],
                    ),
                    shared_state_component_bridge(
                        "ConditionalProviderConsumer",
                        conditional_consumer,
                        "acme.workbench",
                        first_schema,
                    ),
                ],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::DynamicSharedStateProviderHostUnsupported {
            provider_site,
            ..
        }) if provider_site == ProviderSiteId::new(0)
    ));
}

#[test]
fn canonical_query_work_forms_an_owner_and_publishes_codegen_usage() {
    let definition = definition("query", 1);
    let mut component = component_bridge("QueryPanel", definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut component else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    body.origin_permission = OriginPermission::OriginCapable;
    body.origins.insert(OriginId::new(0), OriginKind::Query);
    body.effects
        .insert(LocalEffectId::new(0), EffectKind::QueryObservation);
    bridge
        .component_codegen_mut()
        .expect("component fixture has codegen")
        .uses_queries = true;
    bridge.envelope.refresh_hashes();

    let plan = link_semantic_plan(
        LinkUnitId::derive("query"),
        &AdaptedTransportAbi {
            components: vec![component],
            helpers: vec![],
        },
    )
    .expect("canonical reachable query work is supported by linked-owner codegen");
    assert_eq!(plan.owners.owners.len(), 1);
    assert_eq!(
        plan.owners.owners[0].candidates,
        vec![OwnerCandidateId::new(0)]
    );
    let json = semantic_plan_json(&plan).expect("query plan should serialize");
    assert_eq!(json["component_codegen"][0]["uses_queries"], true);
    validate_semantic_plan(&plan).expect("query owner plan should validate");
}

#[test]
fn reachable_component_local_node_ref_forms_one_owner_but_dead_declaration_does_not() {
    let node_ref_definition = definition("node-ref", 3);
    let mut node_ref = component_bridge("NodeRefPanel", node_ref_definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut node_ref else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    body.origin_permission = OriginPermission::OriginCapable;
    body.origins.insert(OriginId::new(0), OriginKind::NodeRef);
    body.effects
        .insert(LocalEffectId::new(0), EffectKind::NodeReference);
    body.dependencies.insert(
        rsjs_ir::abi::DependencyEdgeId::new(0),
        rsjs_ir::abi::DependencyEdge::new(
            rsjs_ir::abi::DependencyEdgeKind::Effect,
            rsjs_ir::abi::DependencyEndpoint::Origin(OriginId::new(0)),
            rsjs_ir::abi::DependencyEndpoint::Effect(LocalEffectId::new(0)),
            OriginId::new(0),
        ),
    );
    bridge.envelope.refresh_hashes();

    let plan = link_semantic_plan(
        LinkUnitId::derive("node-ref-root"),
        &AdaptedTransportAbi {
            components: vec![node_ref],
            helpers: vec![],
        },
    )
    .expect("component-local node refs have complete fragment lifecycle codegen");
    assert_eq!(plan.owners.owners.len(), 1);
    assert_eq!(
        plan.owners.owners[0].candidates,
        vec![OwnerCandidateId::new(0)]
    );
    validate_semantic_plan(&plan).expect("node-ref owner plan should validate");

    let dead_definition = definition("dead-node-ref", 4);
    let mut dead = component_bridge("DeadNodeRefPanel", dead_definition, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut dead else {
        unreachable!("fixture component is canonical")
    };
    let DescriptorBody::Component(body) = &mut bridge.envelope.body else {
        unreachable!("fixture component contains a component descriptor")
    };
    body.origin_permission = OriginPermission::OriginCapable;
    bridge.envelope.refresh_hashes();
    let plan = link_semantic_plan(
        LinkUnitId::derive("dead-node-ref-root"),
        &AdaptedTransportAbi {
            components: vec![dead],
            helpers: vec![],
        },
    )
    .expect("dead node-ref declarations leave no canonical origin or effect");
    assert!(
        plan.owners.owners.is_empty(),
        "a dead declaration must not activate an owner"
    );
    validate_semantic_plan(&plan).expect("dead-node-ref plan should validate");
}

#[test]
fn missing_and_mismatched_helper_witnesses_are_rejected() {
    let component = definition("component", 1);
    let helper = definition("helper", 2);
    let mut missing = component_bridge("Page", component, &[(WitnessRecordId::new(1), helper, 3)]);
    let AbiBridgeResult::Canonical(missing_bridge) = &mut missing else {
        unreachable!()
    };
    missing_bridge.witnesses.remove(&WitnessRecordId::new(1));
    let helper_result = helper_bridge("format", helper, "fn()", &[]);
    let missing_input = AdaptedTransportAbi {
        components: vec![missing],
        helpers: vec![helper_result.clone()],
    };
    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &missing_input),
        Err(SemanticLinkError::MissingWitness {
            expected: SemanticWitnessKind::HelperEdge,
            ..
        })
    ));

    let mut mismatched =
        component_bridge("Page", component, &[(WitnessRecordId::new(1), helper, 3)]);
    let AbiBridgeResult::Canonical(mismatched_bridge) = &mut mismatched else {
        unreachable!()
    };
    mismatched_bridge.witnesses.insert(
        WitnessRecordId::new(1),
        ResolvedTransportWitness::ComponentEdge {
            source_edge: [3; 32],
            target: helper,
        },
    );
    let mismatched_input = AdaptedTransportAbi {
        components: vec![mismatched],
        helpers: vec![helper_result],
    };
    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &mismatched_input),
        Err(SemanticLinkError::WitnessKindMismatch {
            expected: SemanticWitnessKind::HelperEdge,
            actual: SemanticWitnessKind::ComponentEdge,
            ..
        })
    ));
}

#[test]
fn component_field_witnesses_are_exact_and_fully_claimed() {
    let parent = definition("field-parent", 1);
    let child_definition = definition("field-child", 2);
    let (_, child) =
        live_value_and_callback_bridges(parent, child_definition, TemporalPropMode::LiveRead);

    let mut missing = child.clone();
    let AbiBridgeResult::Canonical(bridge) = &mut missing else {
        unreachable!()
    };
    bridge.witnesses.remove(&WitnessRecordId::new(40));
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("root"),
            &AdaptedTransportAbi {
                components: vec![missing],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::MissingWitness {
            expected: SemanticWitnessKind::ComponentField,
            ..
        })
    ));

    let mut wrong_kind = child.clone();
    let AbiBridgeResult::Canonical(bridge) = &mut wrong_kind else {
        unreachable!()
    };
    bridge.witnesses.insert(
        WitnessRecordId::new(40),
        ResolvedTransportWitness::HelperEdge {
            source_edge: [0x40; 32],
            target: child_definition,
        },
    );
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("root"),
            &AdaptedTransportAbi {
                components: vec![wrong_kind],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::WitnessKindMismatch {
            expected: SemanticWitnessKind::ComponentField,
            actual: SemanticWitnessKind::HelperEdge,
            ..
        })
    ));

    let mut foreign = child.clone();
    let AbiBridgeResult::Canonical(bridge) = &mut foreign else {
        unreachable!()
    };
    bridge.witnesses.insert(
        WitnessRecordId::new(40),
        ResolvedTransportWitness::ComponentField {
            field_record: [0x40; 32],
            target: parent,
        },
    );
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("root"),
            &AdaptedTransportAbi {
                components: vec![foreign],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::ForeignComponentFieldTarget {
            component,
            field,
            target,
        }) if component == child_definition
            && field == ComponentFieldId::new(0)
            && target == parent
    ));

    let mut duplicate = child.clone();
    let AbiBridgeResult::Canonical(bridge) = &mut duplicate else {
        unreachable!()
    };
    bridge.witnesses.insert(
        WitnessRecordId::new(41),
        ResolvedTransportWitness::ComponentField {
            field_record: [0x40; 32],
            target: child_definition,
        },
    );
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("root"),
            &AdaptedTransportAbi {
                components: vec![duplicate],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::DuplicateComponentFieldRecord {
            component,
            field_record,
        }) if component == child_definition && field_record == [0x40; 32]
    ));

    let mut unclaimed = child;
    let AbiBridgeResult::Canonical(bridge) = &mut unclaimed else {
        unreachable!()
    };
    bridge.witnesses.insert(
        WitnessRecordId::new(99),
        ResolvedTransportWitness::ComponentField {
            field_record: [0x99; 32],
            target: child_definition,
        },
    );
    assert!(matches!(
        link_semantic_plan(
            LinkUnitId::derive("root"),
            &AdaptedTransportAbi {
                components: vec![unclaimed],
                helpers: vec![],
            },
        ),
        Err(SemanticLinkError::UnclaimedWitness {
            actual: SemanticWitnessKind::ComponentField,
            ..
        })
    ));
}

#[test]
fn helper_material_extends_established_canonical_fields_append_only() {
    let component = definition("helper-material-component", 1);
    let helper_definition = definition("helper-material-target", 2);
    let plan = link_semantic_plan(
        LinkUnitId::derive("helper-material-layout"),
        &AdaptedTransportAbi {
            components: vec![component_bridge(
                "HelperMaterialPage",
                component,
                &[(WitnessRecordId::new(1), helper_definition, 0xa1)],
            )],
            helpers: vec![helper_bridge(
                "helper_material_target",
                helper_definition,
                "fn() -> i32",
                &[],
            )],
        },
    )
    .unwrap();
    let helper = &plan.helpers[0];
    let DescriptorBody::Helper(descriptor) = &helper.descriptor.body else {
        unreachable!("fixture helper has a helper descriptor")
    };
    let material = SemanticHelperMaterial {
        semantic_key: helper.semantic_key.clone(),
        descriptor_hash: helper.descriptor.semantic_hash,
        signature_hash: descriptor.signature_hash,
        codegen: helper.codegen.clone(),
        helper_edges: helper
            .helper_edges
            .iter()
            .map(SemanticHelperEdgeMaterial::from)
            .collect(),
        transitive_closure: helper.transitive_closure.clone(),
    };

    assert_eq!(
        canonical_bytes(&material),
        canonical_bytes(&AppendOnlySemanticHelperMaterial(&material)),
        "helper codegen must append at field 6 without renumbering the established graph fields",
    );
}

#[test]
fn component_helper_relocation_tuple_changes_plan_identity() {
    let component = definition("relocation-component", 1);
    let alpha = definition("relocation-alpha", 2);
    let beta = definition("relocation-beta", 3);
    let helpers = vec![
        helper_bridge("relocation_alpha", alpha, "fn() -> i32", &[]),
        helper_bridge("relocation_beta", beta, "fn() -> i32", &[]),
    ];
    let link_unit = LinkUnitId::derive("helper-relocation-identity");

    let original = link_semantic_plan(
        link_unit,
        &AdaptedTransportAbi {
            components: vec![component_bridge(
                "RelocationPage",
                component,
                &[
                    (WitnessRecordId::new(1), alpha, 0xa1),
                    (WitnessRecordId::new(2), beta, 0xb2),
                ],
            )],
            helpers: helpers.clone(),
        },
    )
    .unwrap();
    let swapped = link_semantic_plan(
        link_unit,
        &AdaptedTransportAbi {
            components: vec![component_bridge(
                "RelocationPage",
                component,
                &[
                    (WitnessRecordId::new(1), alpha, 0xb2),
                    (WitnessRecordId::new(2), beta, 0xa1),
                ],
            )],
            helpers,
        },
    )
    .unwrap();

    assert_eq!(original.link_unit, swapped.link_unit);
    assert_eq!(
        original.material.component_descriptors, swapped.material.component_descriptors,
        "relocation records are not descriptor semantic identity"
    );
    assert_eq!(
        original.components[0]
            .helper_edges
            .iter()
            .map(|edge| (edge.call, edge.target_definition, edge.target.clone()))
            .collect::<Vec<_>>(),
        swapped.components[0]
            .helper_edges
            .iter()
            .map(|edge| (edge.call, edge.target_definition, edge.target.clone()))
            .collect::<Vec<_>>()
    );
    assert_ne!(
        original.material.payload, swapped.material.payload,
        "source-edge routing must be authenticated behavior material"
    );
    assert_ne!(original.plan_id, swapped.plan_id);
    validate_semantic_plan(&original).unwrap();
    validate_semantic_plan(&swapped).unwrap();
}

#[test]
fn helper_source_edge_swap_is_rejected_under_the_original_plan_id() {
    let component = definition("tamper-component", 1);
    let caller = definition("tamper-caller", 2);
    let alpha = definition("tamper-alpha", 3);
    let beta = definition("tamper-beta", 4);
    let mut plan = link_semantic_plan(
        LinkUnitId::derive("helper-relocation-tamper"),
        &AdaptedTransportAbi {
            components: vec![component_bridge(
                "TamperPage",
                component,
                &[(WitnessRecordId::new(1), caller, 0xc0)],
            )],
            helpers: vec![
                helper_bridge(
                    "tamper_caller",
                    caller,
                    "fn() -> i32",
                    &[
                        (WitnessRecordId::new(1), alpha, 0xc1, 0),
                        (WitnessRecordId::new(2), beta, 0xc2, 0),
                    ],
                ),
                helper_bridge("tamper_alpha", alpha, "fn() -> i32", &[]),
                helper_bridge("tamper_beta", beta, "fn() -> i32", &[]),
            ],
        },
    )
    .unwrap();
    let original_plan_id = plan.plan_id;
    let caller = plan
        .helpers
        .iter_mut()
        .find(|helper| helper.definition == caller)
        .expect("linked caller helper");
    let (first, second) = caller.helper_edges.split_at_mut(1);
    std::mem::swap(&mut first[0].source_edge, &mut second[0].source_edge);

    assert_eq!(plan.plan_id, original_plan_id);
    assert!(matches!(
        validate_semantic_plan(&plan),
        Err(SemanticPlanValidationError::PayloadHashMismatch { .. })
    ));
}

#[test]
fn helper_cycles_have_one_deterministic_dependency_order() {
    let component = definition("component", 1);
    let alpha = definition("alpha", 2);
    let beta = definition("beta", 3);
    let component_result =
        component_bridge("Page", component, &[(WitnessRecordId::new(1), alpha, 10)]);
    let alpha_result = helper_bridge(
        "alpha",
        alpha,
        "fn(i32) -> i32",
        &[(WitnessRecordId::new(1), beta, 11, 1)],
    );
    let beta_result = helper_bridge(
        "beta",
        beta,
        "fn(i32) -> i32",
        &[(WitnessRecordId::new(1), alpha, 12, 1)],
    );

    let forward = AdaptedTransportAbi {
        components: vec![component_result.clone()],
        helpers: vec![alpha_result.clone(), beta_result.clone()],
    };
    let reversed = AdaptedTransportAbi {
        components: vec![component_result],
        helpers: vec![beta_result, alpha_result],
    };
    let forward = link_semantic_plan(LinkUnitId::derive("root"), &forward).unwrap();
    let reversed = link_semantic_plan(LinkUnitId::derive("root"), &reversed).unwrap();

    assert_eq!(forward.plan_id, reversed.plan_id);
    assert_eq!(forward.material, reversed.material);
    assert_eq!(
        semantic_plan_json(&forward).unwrap(),
        semantic_plan_json(&reversed).unwrap(),
        "helper codegen serialization must be independent of input collection order"
    );
    assert_eq!(forward.reachable_helpers, reversed.reachable_helpers);
    assert_eq!(forward.reachable_helpers.len(), 2);
    assert_eq!(
        forward
            .helpers
            .iter()
            .map(|helper| helper.semantic_key.clone())
            .collect::<Vec<_>>(),
        [helper_key("alpha"), helper_key("beta")]
    );
    let json = semantic_plan_json(&forward).unwrap();
    assert_eq!(json["helper_codegen"].as_array().unwrap().len(), 2);
    assert_eq!(
        json["helper_codegen"][0]["body_ir"]["stmts"][0]["value"]["function"]["edge"], 0,
        "transitive helper calls retain dense caller-local edge IDs"
    );
}

#[test]
fn semantic_plan_writer_is_content_addressed_and_preserves_exact_coordinates() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let plan = plan_with_signature("fn(u32) -> String");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "proa-rsjs-semantic-plan-{}-{nonce}",
        std::process::id()
    ));

    let first = write_content_addressed_semantic_plan(&directory, &plan).unwrap();
    let second = write_content_addressed_semantic_plan(&directory, &plan).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.file_name().unwrap().to_string_lossy(),
        format!("{}.semantic-plan.json", plan.plan_id)
    );

    let value: Value = serde_json::from_slice(&fs::read(&first).unwrap()).unwrap();
    assert_eq!(value["format"], SEMANTIC_PLAN_FORMAT);
    assert_eq!(
        value["schema_version"],
        serde_json::json!(SEMANTIC_LINK_SCHEMA_VERSION)
    );
    assert_eq!(value["plan_id"], plan.plan_id.to_string());
    assert_eq!(value["link_unit_id"], plan.link_unit.to_string());
    assert_eq!(
        value["components"][0]["definition"]["cargo_unit_id"],
        plan.components[0].definition.unit.to_string()
    );
    assert_eq!(
        value["components"][0]["definition"]["record_id"],
        hex_bytes(&plan.components[0].definition.record)
    );
    assert_eq!(
        value["component_codegen"][0]["definition"], value["components"][0]["definition"],
        "the codegen record must retain the exact component definition coordinate"
    );
    assert_eq!(
        value["component_codegen"][0]["asset_id"],
        plan.components[0].codegen.asset_id
    );
    assert_eq!(
        value["component_codegen"][0]["fragment_key"],
        plan.components[0].codegen.fragment_key
    );
    assert_eq!(
        value["helper_codegen"][0]["definition"], value["helpers"][0]["definition"],
        "helper codegen must retain the exact defining coordinate"
    );
    assert_eq!(
        value["helper_codegen"][0]["semantic_key_canonical"],
        value["helpers"][0]["semantic_key_canonical"]
    );
    assert_eq!(
        value["helper_codegen"][0]["source_ir_hash"],
        value["helpers"][0]["client_ir_hash"]
    );
    assert_eq!(
        value["helper_codegen"][0]["body_ir_hash"],
        hash_json(plan.helpers[0].codegen.body_ir_hash())
    );
    assert_eq!(value["helper_codegen"][0]["body_ir"]["kind"], "block");
    assert_eq!(
        value["helper_codegen"][0]["signature"],
        plan.helpers[0].codegen.signature().canonical_json()
    );
    assert!(value["components"][0]
        .get("descriptor_semantic_abi")
        .is_some());
    let DescriptorBody::Component(component_body) = &plan.components[0].descriptor.body else {
        unreachable!("test plan component must contain a component descriptor")
    };
    assert_eq!(
        value["components"][0]["root_eligible"],
        serde_json::json!(component_body.root_eligible)
    );
    assert_eq!(
        value["component_sites"][0]["candidate_id"],
        serde_json::json!(0)
    );
    assert!(value["components"][0].get("candidate_id").is_none());

    fs::remove_file(&first).unwrap();
    fs::remove_dir(&directory).unwrap();
}

#[test]
fn extra_exact_edge_witness_is_not_silently_ignored() {
    let component = definition("component", 1);
    let helper = definition("helper", 2);
    let mut component_result = component_bridge("Page", component, &[]);
    let AbiBridgeResult::Canonical(bridge) = &mut component_result else {
        unreachable!()
    };
    bridge.witnesses.insert(
        WitnessRecordId::new(9),
        ResolvedTransportWitness::HelperEdge {
            source_edge: [9; 32],
            target: helper,
        },
    );
    let adapted = AdaptedTransportAbi {
        components: vec![component_result],
        helpers: vec![helper_bridge("format", helper, "fn()", &[])],
    };

    assert!(matches!(
        link_semantic_plan(LinkUnitId::derive("root"), &adapted),
        Err(SemanticLinkError::UnclaimedWitness {
            witness,
            actual: SemanticWitnessKind::HelperEdge,
            ..
        }) if witness == WitnessRecordId::new(9)
    ));
}
