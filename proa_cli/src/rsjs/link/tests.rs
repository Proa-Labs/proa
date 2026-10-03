// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn extraction_consumes_the_validated_snapshot_without_rereading_its_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("descriptor.o");
    // A metadata-only archive is a valid transport carrier with zero records.
    fs::write(
        &path,
        b"!<arch>\nmetadata/       0           0     0     100644  0         `\n",
    )
    .unwrap();
    let snapshot = DescriptorObjectSnapshot::read(&path).unwrap();
    let artifact = rsjs_ir::build::LinkArtifact {
        unit: CargoUnitId::from_bytes([1; 32]),
        role: ArtifactRole::DescriptorObject,
        content: SemanticHash::from_bytes(snapshot.link_hash),
        path: path.to_string_lossy().into_owned(),
    };
    let evidence = [DescriptorArtifactEvidence {
        artifact: &artifact,
        descriptor_hash: snapshot.descriptor_hash,
    }];
    let objects = BTreeMap::from([(path.clone(), snapshot)]);
    fs::remove_file(&path).unwrap();
    let extracted = extract_descriptor_artifact_snapshots(&evidence, &objects).unwrap();
    assert!(extracted.definitions.is_empty());
    assert!(extracted.helper_definitions.is_empty());

    let wrong_authority = [DescriptorArtifactEvidence {
        artifact: &artifact,
        descriptor_hash: [0; 32],
    }];
    assert!(matches!(
        extract_descriptor_artifact_snapshots(&wrong_authority, &objects),
        Err(TransportLinkError::DescriptorObjectIdentityHashMismatch { .. })
    ));
}

#[test]
fn parsed_record_reuse_requires_the_exact_path_and_byte_generation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("descriptor.o");
    let mut bytes =
        b"!<arch>\nmetadata/       0           0     0     100644  0         `\n".to_vec();
    fs::write(&path, &bytes).unwrap();
    let first = DescriptorObjectSnapshot::read(&path).unwrap();
    let parsed = parsed_descriptor_records(&path, &first).unwrap();
    assert!(Arc::ptr_eq(
        &parsed,
        &parsed_descriptor_records(&path, &first).unwrap()
    ));
    bytes[24] = b'1'; // A different valid archive generation, even with identical records.
    fs::write(&path, &bytes).unwrap();
    let next = DescriptorObjectSnapshot::read(&path).unwrap();
    assert!(!Arc::ptr_eq(
        &parsed,
        &parsed_descriptor_records(&path, &next).unwrap()
    ));
    let other_path = directory.path().join("another.o");
    assert!(!Arc::ptr_eq(
        &parsed,
        &parsed_descriptor_records(&other_path, &first).unwrap()
    ));
}

#[test]
fn exact_byte_hash_matches_the_manifest_hash_contract() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("descriptor.o");
    let bytes = b"one exact descriptor-object snapshot";
    fs::write(&path, bytes).unwrap();

    assert_eq!(
        link_artifact_content_hash_bytes(bytes),
        crate::rsjs::build::link_artifact_content_hash(&path).unwrap()
    );
}

#[test]
fn parallel_descriptor_reads_report_the_first_manifest_error() {
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first-missing.o");
    let second_path = directory.path().join("second-missing.o");
    let artifact = |byte, path: &Path| rsjs_ir::build::LinkArtifact {
        unit: CargoUnitId::from_bytes([byte; 32]),
        role: ArtifactRole::DescriptorObject,
        content: SemanticHash::from_bytes([byte; 32]),
        path: path.to_string_lossy().into_owned(),
    };
    let first = artifact(1, &first_path);
    let second = artifact(2, &second_path);

    let evidence = [
        DescriptorArtifactEvidence {
            artifact: &first,
            descriptor_hash: [1; 32],
        },
        DescriptorArtifactEvidence {
            artifact: &second,
            descriptor_hash: [2; 32],
        },
    ];
    let error = extract_descriptor_artifacts(&evidence).unwrap_err();
    assert!(matches!(
        error,
        TransportLinkError::HashDescriptorObject { path, .. } if path == first_path
    ));
}

#[test]
fn descriptor_hash_is_checked_before_untrusted_bytes_are_parsed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("descriptor.o");
    let bytes = b"not an object";
    fs::write(&path, bytes).unwrap();
    let artifact = rsjs_ir::build::LinkArtifact {
        unit: CargoUnitId::from_bytes([1; 32]),
        role: ArtifactRole::DescriptorObject,
        content: SemanticHash::from_bytes([0; 32]),
        path: path.to_string_lossy().into_owned(),
    };

    assert!(matches!(
        extract_descriptor_artifacts(&[DescriptorArtifactEvidence {
            artifact: &artifact,
            descriptor_hash: descriptor_object_content_hash_bytes(bytes),
        }]),
        Err(TransportLinkError::DescriptorObjectHashMismatch { path: actual, .. })
            if actual == path
    ));
}

#[test]
fn freshly_blessed_link_hash_cannot_replace_wrapper_descriptor_authority() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replaced-descriptor.o");
    let replacement = b"replacement bytes blessed by a later manifest read";
    fs::write(&path, replacement).unwrap();
    let artifact = rsjs_ir::build::LinkArtifact {
        unit: CargoUnitId::from_bytes([7; 32]),
        role: ArtifactRole::DescriptorObject,
        content: link_artifact_content_hash_bytes(replacement),
        path: path.to_string_lossy().into_owned(),
    };

    assert!(matches!(
        extract_descriptor_artifacts(&[DescriptorArtifactEvidence {
            artifact: &artifact,
            descriptor_hash: [0; 32],
        }]),
        Err(TransportLinkError::DescriptorObjectIdentityHashMismatch {
            path: actual,
            ..
        }) if actual == path
    ));
}

#[test]
fn lowercase_record_ids_are_strict() {
    assert_eq!(parse_hex_32(&"ab".repeat(32)), Ok([0xab; 32]));
    assert!(parse_hex_32(&"AB".repeat(32)).is_err());
    assert!(parse_hex_32(&"a".repeat(63)).is_err());
    assert!(parse_hex_32(&"gg".repeat(32)).is_err());
}

#[test]
fn transport_payload_hash_excludes_only_its_stored_hash() {
    let mut value = serde_json::json!({
        "format": COMPONENT_PAYLOAD_FORMAT,
        "schema_version": COMPONENT_PAYLOAD_SCHEMA,
        "payload": {"answer": 42},
    });
    let bytes = canonical_json_bytes(&value).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(COMPONENT_SEMANTIC_DOMAIN);
    hasher.update(bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    value.as_object_mut().unwrap().insert(
        "semantic_hash".into(),
        Value::String(format!("sha256:{}", hex_id(&digest))),
    );

    // Model the producer/linker boundary: the producer seals and emits
    // bytes, then the linker parses a fresh Value before validating it.
    let encoded = serde_json::to_vec(&value).unwrap();
    let mut decoded: Value = serde_json::from_slice(&encoded).unwrap();
    validate_embedded_semantic_hash("component", &decoded, COMPONENT_SEMANTIC_DOMAIN).unwrap();
    decoded["payload"]["answer"] = Value::from(43);
    assert!(matches!(
        validate_embedded_semantic_hash("component", &decoded, COMPONENT_SEMANTIC_DOMAIN),
        Err(TransportLinkError::EmbeddedSemanticHashMismatch { .. })
    ));
}

#[test]
fn component_lifecycle_transport_is_strictly_tagged() {
    let conditional = serde_json::from_value::<ComponentSourceLifecycle>(serde_json::json!({
        "kind": "conditional_single",
        "control": 4,
        "predicate_expression": 9,
        "branch": "else",
    }))
    .expect("the schema-v7 conditional lifecycle shape should parse");
    assert!(matches!(
        conditional,
        ComponentSourceLifecycle::ConditionalSingle {
            control: 4,
            predicate_expression: 9,
            branch: ComponentSourceConditionalBranch::Else,
        }
    ));

    assert!(
        serde_json::from_value::<ComponentSourceLifecycle>(serde_json::json!({
            "kind": "conditional_single",
            "control": 4,
            "predicate_expression": 9,
            "branch": "else",
            "extra": true,
        }))
        .is_err(),
        "unknown lifecycle fields must fail closed"
    );
    assert!(
        serde_json::from_value::<ComponentSourceLifecycle>(serde_json::json!("non_static"))
            .is_err(),
        "the lossy schema-v5 string lifecycle must not parse as schema v7"
    );
    assert!(
        serde_json::from_value::<ComponentSourceLifecycle>(serde_json::json!({
            "kind": "unknown",
            "reason": "arbitrary",
        }))
        .is_err(),
        "unknown reasons are a closed v6 vocabulary"
    );
}

#[test]
fn node_ref_handle_binding_preserves_the_exact_handle_discriminator() {
    let binding = serde_json::from_value::<ComponentSourceFieldBinding>(serde_json::json!({
        "handle_kind": "node_ref",
        "kind": "node_ref_handle",
        "parent_source": {
            "id": 7,
            "namespace": "legacy_node_ref",
        },
        "proof": "exact_node_ref_handle_alias_v1",
    }))
    .expect("the exact NodeRef handle binding must cross the transport parser");
    assert_eq!(binding.handle_kind.as_deref(), Some("node_ref"));
    assert_eq!(binding.kind, "node_ref_handle");

    assert!(
        serde_json::from_value::<ComponentSourceFieldBinding>(serde_json::json!({
            "handle_kind": "node_ref",
            "kind": "node_ref_handle",
            "parent_source": {
                "id": 7,
                "namespace": "legacy_node_ref",
            },
            "proof": "exact_node_ref_handle_alias_v1",
            "uncommitted_semantics": true,
        }))
        .is_err(),
        "unknown binding fields must remain fail-closed"
    );
}

#[test]
fn deferred_projection_binding_preserves_both_latent_identities() {
    let binding = serde_json::from_value::<ComponentSourceFieldBinding>(serde_json::json!({
        "kind": "deferred_projection",
        "latent_node_ref_id": 5,
        "latent_signal_id": 3,
        "parent_field_id": 1,
        "proof": "rustc_linked_child_field_resolves_exact_projection_role_v1",
    }))
    .expect("deferred projection identities must cross the transport parser");
    assert_eq!(binding.kind, "deferred_projection");
    assert_eq!(binding.parent_field_id, Some(1));
    assert_eq!(binding.latent_signal_id, Some(3));
    assert_eq!(binding.latent_node_ref_id, Some(5));
}

#[allow(clippy::type_complexity)]
fn shared_state_provider_link_fixture() -> (
    TransportDefinitionId,
    TransportRecordId,
    String,
    Value,
    Vec<ComponentSharedStateProviderSiteClaim>,
    Vec<ComponentSourceEdge>,
    Vec<LinkedComponentEdge>,
    LinkedSharedStateAcquisition,
) {
    let unit = [0x31; 32];
    let component = TransportDefinitionId {
        unit: CargoUnitId::from_bytes(unit),
        record: [0x32; 32],
    };
    let child_target = TransportDefinitionId {
        unit: CargoUnitId::from_bytes([0x41; 32]),
        record: [0x42; 32],
    };
    let state_target = TransportDefinitionId {
        unit: CargoUnitId::from_bytes([0x51; 32]),
        record: [0x52; 32],
    };
    let fingerprint = "sha256:provider-link-fixture".to_owned();
    let child_source_edge = [0x61; 32];
    let provider_source_edge = shared_state_provider_source_edge_id(
        &fingerprint,
        0,
        &TransportSharedStateProviderSite {
            site_node: 7,
            child_edge_ordinal: 0,
            child_source_edge_id: child_source_edge,
        },
    );
    let descriptor = serde_json::json!({
        "shared_state_provider_sites": [{
            "child_edge": 0,
            "id": 0,
            "site_node": 7,
            "source_anchor": {
                "start_column": 9,
                "start_line": 14,
            },
            "state_rust_type": "crate::WorkbenchState",
            "topology": "static_single_wrapperless",
        }],
    });
    let claims = vec![ComponentSharedStateProviderSiteClaim {
        child_edge_local_ordinal: 0,
        child_source_edge_id: hex_id(&child_source_edge),
        identity_authority: "rustc_shared_state_witness_relocation".to_owned(),
        provider_site_id: 0,
        site_node: 7,
        source_anchor: SourceAnchor {
            column: 9,
            line: 14,
        },
        source_edge_id: hex_id(&provider_source_edge),
        state_rust_type: "crate::WorkbenchState".to_owned(),
        topology: "static_single_wrapperless".to_owned(),
    }];
    let direct_edges = vec![ComponentSourceEdge {
        authored_path: "crate::WorkbenchChildren".to_owned(),
        boundary: "composed".to_owned(),
        fields: Vec::new(),
        has_rest: false,
        identity_authority: "rustc_component_witness_relocation".to_owned(),
        lifecycle: ComponentSourceLifecycle::StaticSingle,
        local_ordinal: 0,
        site_node: 7,
        source_anchor: SourceAnchor {
            column: 9,
            line: 14,
        },
        source_edge_id: hex_id(&child_source_edge),
    }];
    let linked_children = vec![LinkedComponentEdge {
        source_edge: child_source_edge,
        target: child_target,
    }];
    let acquisition = LinkedSharedStateAcquisition {
        source_unit: CargoUnitId::from_bytes(unit),
        source_edge: provider_source_edge,
        target: state_target,
    };
    (
        component,
        unit,
        fingerprint,
        descriptor,
        claims,
        direct_edges,
        linked_children,
        acquisition,
    )
}

#[test]
fn shared_state_provider_link_authenticates_both_nominal_edges() {
    let (
        component,
        unit,
        fingerprint,
        descriptor,
        claims,
        direct_edges,
        linked_children,
        acquisition,
    ) = shared_state_provider_link_fixture();
    let records = BTreeMap::from([((unit, acquisition.source_edge), &acquisition)]);
    let mut claimed = BTreeSet::new();
    let linked = validate_and_link_shared_state_provider_sites(
        component,
        unit,
        &fingerprint,
        &descriptor,
        &claims,
        &direct_edges,
        &linked_children,
        &records,
        &mut claimed,
    )
    .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].target, acquisition.target);
    assert_eq!(linked[0].child_target, linked_children[0].target);
    assert_eq!(linked[0].site_node, 7);
}

#[test]
fn shared_state_provider_link_rejects_non_dense_swapped_and_missing_claims() {
    let (
        component,
        unit,
        fingerprint,
        descriptor,
        mut claims,
        direct_edges,
        linked_children,
        acquisition,
    ) = shared_state_provider_link_fixture();
    let records = BTreeMap::from([((unit, acquisition.source_edge), &acquisition)]);

    claims[0].provider_site_id = 1;
    assert!(matches!(
        validate_and_link_shared_state_provider_sites(
            component,
            unit,
            &fingerprint,
            &descriptor,
            &claims,
            &direct_edges,
            &linked_children,
            &records,
            &mut BTreeSet::new(),
        ),
        Err(TransportLinkError::NonDenseSharedStateProviderSiteClaims { .. })
    ));

    let (_, _, _, descriptor, mut claims, direct_edges, linked_children, _acquisition) =
        shared_state_provider_link_fixture();
    claims[0].child_source_edge_id = hex_id(&[0xa1; 32]);
    assert!(matches!(
        validate_and_link_shared_state_provider_sites(
            component,
            unit,
            &fingerprint,
            &descriptor,
            &claims,
            &direct_edges,
            &linked_children,
            &records,
            &mut BTreeSet::new(),
        ),
        Err(TransportLinkError::SharedStateProviderChildEdgeMismatch { .. })
    ));

    let (_, _, _, descriptor, claims, direct_edges, linked_children, _) =
        shared_state_provider_link_fixture();
    assert!(matches!(
        validate_and_link_shared_state_provider_sites(
            component,
            unit,
            &fingerprint,
            &descriptor,
            &claims,
            &direct_edges,
            &linked_children,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
        ),
        Err(TransportLinkError::MissingSharedStateProviderWitness { .. })
    ));
}

#[test]
fn shared_state_provider_link_rejects_reused_children_and_tampered_claims() {
    let (
        component,
        unit,
        fingerprint,
        mut descriptor,
        mut claims,
        direct_edges,
        linked_children,
        acquisition,
    ) = shared_state_provider_link_fixture();
    claims[0].state_rust_type = "crate::OtherState".to_owned();
    let records = BTreeMap::from([((unit, acquisition.source_edge), &acquisition)]);
    assert!(matches!(
        validate_and_link_shared_state_provider_sites(
            component,
            unit,
            &fingerprint,
            &descriptor,
            &claims,
            &direct_edges,
            &linked_children,
            &records,
            &mut BTreeSet::new(),
        ),
        Err(TransportLinkError::SharedStateProviderDescriptorClaimMismatch { .. })
    ));

    let (_, _, _, original_descriptor, mut claims, direct_edges, linked_children, acquisition) =
        shared_state_provider_link_fixture();
    let mut second = claims[0].clone();
    second.provider_site_id = 1;
    second.source_edge_id = hex_id(&shared_state_provider_source_edge_id(
        &fingerprint,
        1,
        &TransportSharedStateProviderSite {
            site_node: second.site_node,
            child_edge_ordinal: second.child_edge_local_ordinal,
            child_source_edge_id: parse_hex_32(&second.child_source_edge_id).unwrap(),
        },
    ));
    claims.push(second);
    descriptor["shared_state_provider_sites"] = serde_json::json!([
        original_descriptor["shared_state_provider_sites"][0].clone(),
        {
            "child_edge": 0,
            "id": 1,
            "site_node": 7,
            "source_anchor": {
                "start_column": 9,
                "start_line": 14,
            },
            "state_rust_type": "crate::WorkbenchState",
            "topology": "static_single_wrapperless",
        }
    ]);
    assert!(matches!(
        validate_and_link_shared_state_provider_sites(
            component,
            unit,
            &fingerprint,
            &descriptor,
            &claims,
            &direct_edges,
            &linked_children,
            &BTreeMap::from([((unit, acquisition.source_edge), &acquisition)]),
            &mut BTreeSet::new(),
        ),
        Err(TransportLinkError::SharedStateProviderChildClaimedTwice { .. })
    ));
}

#[test]
fn normalized_namespace_is_semantic_plan_material() {
    let first = LinkedSemanticIdentity {
        package: "acme-web".to_string(),
        crate_name: "acme_web".to_string(),
        module_path: vec!["ui".to_string()],
        item: "StatusBadge".to_string(),
    };
    let mut moved = first.clone();
    moved.module_path = vec!["components".to_string()];
    let mut renamed = first.clone();
    renamed.item = "StateBadge".to_string();

    assert_ne!(SemanticHash::derive(&first), SemanticHash::derive(&moved));
    assert_ne!(SemanticHash::derive(&first), SemanticHash::derive(&renamed));
}

fn empty_extracted_transport() -> ExtractedTransport {
    ExtractedTransport {
        artifacts: Vec::new(),
        definitions: Vec::new(),
        edges: Vec::new(),
        resolved_edges: Vec::new(),
        component_fields: Vec::new(),
        resolved_component_fields: Vec::new(),
        helper_definitions: Vec::new(),
        helper_edges: Vec::new(),
        resolved_helper_edges: Vec::new(),
        shared_state_definitions: Vec::new(),
        shared_state_acquisitions: Vec::new(),
        resolved_shared_state_acquisitions: Vec::new(),
        resolved_definitions: Vec::new(),
        resolved_helper_definitions: Vec::new(),
        resolved_shared_state_definitions: Vec::new(),
        semantic_identities: Vec::new(),
    }
}

fn shared_state_definition(
    unit_byte: u8,
    record_byte: u8,
    type_key: &str,
    schema_abi: &[u8],
) -> (
    rsjs_transport::SharedStateDefinition,
    rsjs_transport::ResolvedSharedStateDefinition,
) {
    let unit = [unit_byte; 32];
    let record = [record_byte; 32];
    let location = rsjs_transport::ArtifactLocation {
        artifact_path: PathBuf::from(format!("/tmp/shared-state-{unit_byte:02x}.o")),
        archive_member: None,
        archive_member_index: None,
        section: ".rsjs.shared_state".to_owned(),
        record_offset: 0,
    };
    let definition = rsjs_transport::SharedStateDefinition {
        location: location.clone(),
        cargo_unit_id: unit,
        shared_state_record_id: record,
        witness: rsjs_transport::RelocationIdentity::Symbol(format!(
            "shared_state_{record_byte:02x}"
        )),
    };
    let schema_payload = rsjs_transport::encode_shared_state_schema_payload(type_key, schema_abi);
    let payload = rsjs_transport::SharedStateDefinitionPayload {
        location: rsjs_transport::ArtifactLocation {
            section: ".rsjs.shared_state_payload".to_owned(),
            ..location
        },
        cargo_unit_id: unit,
        shared_state_record_id: record,
        semantic_payload_hash: rsjs_transport::semantic_payload_hash(
            rsjs_transport::PayloadKind::SharedStateDefinition,
            &schema_payload,
        ),
        type_key: type_key.to_owned(),
        schema_abi_hash: rsjs_transport::shared_state_schema_abi_hash(schema_abi),
        schema_abi: schema_abi.to_vec(),
    };
    (
        definition.clone(),
        rsjs_transport::ResolvedSharedStateDefinition {
            definition,
            payload,
        },
    )
}

fn shared_state_acquisition(
    unit_byte: u8,
    source_edge_id: TransportRecordId,
    target: &rsjs_transport::SharedStateDefinition,
) -> (
    rsjs_transport::SharedStateAcquisition,
    rsjs_transport::ResolvedSharedStateAcquisition,
) {
    let acquisition = rsjs_transport::SharedStateAcquisition {
        location: rsjs_transport::ArtifactLocation {
            artifact_path: PathBuf::from(format!("/tmp/shared-acquire-{unit_byte:02x}.o")),
            archive_member: None,
            archive_member_index: None,
            section: ".rsjs.shared_state_acquisition".to_owned(),
            record_offset: 0,
        },
        cargo_unit_id: [unit_byte; 32],
        source_edge_id,
        witness: target.witness.clone(),
    };
    (
        acquisition.clone(),
        rsjs_transport::ResolvedSharedStateAcquisition {
            acquisition,
            definition: target.clone(),
        },
    )
}

fn component_with_shared_state_acquisition(
    unit_byte: u8,
    record_byte: u8,
    fingerprint: &str,
    source_edge: TransportRecordId,
) -> ResolvedComponentDefinition {
    let unit = [unit_byte; 32];
    let record = component_record_id(fingerprint);
    let location = rsjs_transport::ArtifactLocation {
        artifact_path: PathBuf::from(format!("/tmp/shared-owner-{unit_byte:02x}.o")),
        archive_member: None,
        archive_member_index: None,
        section: ".rsjs.component".to_owned(),
        record_offset: 0,
    };
    let witness =
        rsjs_transport::RelocationIdentity::Symbol(format!("shared_owner_{record_byte:02x}"));
    let definition = rsjs_transport::ComponentDefinition {
        location: location.clone(),
        cargo_unit_id: unit,
        component_record_id: record,
        component_abi: ComponentDefinitionAbi::new(
            TransportComponentRenderKind::Sync,
            TransportComponentOriginPermission::OriginCapable,
        ),
        witness,
    };
    let mut payload = serde_json::json!({
        "component_descriptor": {
            "descriptor_hash": format!("sha256:{}", "00".repeat(32)),
            "format": COMPONENT_DESCRIPTOR_FORMAT,
            "schema_version": COMPONENT_DESCRIPTOR_SCHEMA_VERSION,
            "shared_stores": [{"id": 0}],
            "shared_state_provider_sites": [],
        },
        "format": COMPONENT_PAYLOAD_FORMAT,
        "schema_version": COMPONENT_PAYLOAD_SCHEMA,
        "transport": {
            "client_value_sources": {},
            "component_field_witnesses": [],
            "component_input_fingerprint": fingerprint,
            "definition_record_id": hex_id(&record),
            "direct_component_edges": [],
            "hydration_constraint": {},
            "reachable_helper_edges": [],
            "shared_state_acquisitions": [{
                "source_edge_id": hex_id(&source_edge),
                "store_id": 0,
            }],
            "shared_state_provider_sites": [],
        },
    });
    let semantic_bytes = canonical_json_bytes(&payload).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(COMPONENT_SEMANTIC_DOMAIN);
    hasher.update(semantic_bytes);
    let semantic_hash: [u8; 32] = hasher.finalize().into();
    payload.as_object_mut().unwrap().insert(
        "semantic_hash".to_owned(),
        Value::String(format!("sha256:{}", hex_id(&semantic_hash))),
    );
    let json = serde_json::to_vec(&payload).unwrap();
    ResolvedComponentDefinition {
        definition,
        payload: rsjs_transport::ComponentDescriptorPayload {
            location: rsjs_transport::ArtifactLocation {
                section: ".rsjs.component_payload".to_owned(),
                ..location.clone()
            },
            cargo_unit_id: unit,
            component_record_id: record,
            semantic_payload_hash: rsjs_transport::semantic_payload_hash(
                rsjs_transport::PayloadKind::ComponentDescriptor,
                &json,
            ),
            json,
        },
        identity: SemanticIdentity {
            location,
            kind: rsjs_transport::SemanticIdentityKind::Component,
            cargo_unit_id: unit,
            definition_record_id: record,
            package: "shared-owner-fixture".to_owned(),
            crate_name: "shared_owner_fixture".to_owned(),
            module_path: vec!["owners".to_owned()],
            item: format!("Owner{record_byte:02x}"),
        },
    }
}

fn reseal_component_fixture_payload(component: &mut ResolvedComponentDefinition, value: Value) {
    let mut semantic_value = value;
    semantic_value
        .as_object_mut()
        .expect("fixture component payload")
        .remove("semantic_hash");
    let mut hasher = Sha256::new();
    hasher.update(COMPONENT_SEMANTIC_DOMAIN);
    hasher.update(canonical_json_bytes(&semantic_value).unwrap());
    let semantic_hash: [u8; 32] = hasher.finalize().into();
    semantic_value.as_object_mut().unwrap().insert(
        "semantic_hash".to_owned(),
        Value::String(format!("sha256:{}", hex_id(&semantic_hash))),
    );
    let json = serde_json::to_vec(&semantic_value).unwrap();
    component.payload.semantic_payload_hash = rsjs_transport::semantic_payload_hash(
        rsjs_transport::PayloadKind::ComponentDescriptor,
        &json,
    );
    component.payload.json = json;
}

#[test]
fn component_fingerprints_and_edge_claim_sets_cannot_cross_swap_definitions() {
    let fingerprint_a = "sha256:component-parent-a";
    let fingerprint_b = "sha256:component-parent-b";
    let edge_a = shared_state_acquisition_source_edge_id(fingerprint_a, 0);
    let edge_b = shared_state_acquisition_source_edge_id(fingerprint_b, 0);
    let mut component_a =
        component_with_shared_state_acquisition(0x41, 0x71, fingerprint_a, edge_a);
    let mut component_b =
        component_with_shared_state_acquisition(0x41, 0x72, fingerprint_b, edge_b);
    let mut payload_a: Value = serde_json::from_slice(&component_a.payload.json).unwrap();
    let mut payload_b: Value = serde_json::from_slice(&component_b.payload.json).unwrap();

    for field in [
        "component_input_fingerprint",
        "component_field_witnesses",
        "direct_component_edges",
        "reachable_helper_edges",
        "shared_state_acquisitions",
        "shared_state_provider_sites",
    ] {
        let value_a = payload_a["transport"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        let value_b = payload_b["transport"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        payload_a["transport"]
            .as_object_mut()
            .unwrap()
            .insert(field.to_owned(), value_b);
        payload_b["transport"]
            .as_object_mut()
            .unwrap()
            .insert(field.to_owned(), value_a);
    }
    reseal_component_fixture_payload(&mut component_a, payload_a);
    reseal_component_fixture_payload(&mut component_b, payload_b);

    let mut extracted = empty_extracted_transport();
    extracted.resolved_definitions = vec![component_a, component_b];
    let error = resolve_extracted_transport(
        LinkUnitId::derive("cross-swapped-component-fingerprints"),
        extracted,
    )
    .expect_err(
        "self-consistent edge sets from another component must not authenticate the parent",
    );
    assert!(matches!(
        error,
        TransportLinkError::PayloadDefinitionMismatch {
            kind: "component fingerprint-derived",
            ..
        }
    ));
}

#[test]
fn shared_state_records_are_deterministic_plan_material_and_json() {
    let (definition_a, resolved_definition_a) =
        shared_state_definition(0x11, 0x21, "acme.filters", &[0x00, 0xff, 0x10]);
    let (definition_b, resolved_definition_b) =
        shared_state_definition(0x12, 0x22, "acme.workbench", &[0x01, 0x02]);
    let fingerprint_a = "sha256:shared-owner-a";
    let fingerprint_b = "sha256:shared-owner-b";
    let source_edge_a = shared_state_acquisition_source_edge_id(fingerprint_a, 0);
    let source_edge_b = shared_state_acquisition_source_edge_id(fingerprint_b, 0);
    let (acquisition_a, resolved_acquisition_a) =
        shared_state_acquisition(0x31, source_edge_a, &definition_a);
    let (acquisition_b, resolved_acquisition_b) =
        shared_state_acquisition(0x30, source_edge_b, &definition_b);
    let component_a =
        component_with_shared_state_acquisition(0x31, 0x71, fingerprint_a, source_edge_a);
    let component_b =
        component_with_shared_state_acquisition(0x30, 0x72, fingerprint_b, source_edge_b);

    let mut first_input = empty_extracted_transport();
    first_input.resolved_definitions = vec![component_a.clone(), component_b.clone()];
    first_input.shared_state_definitions = vec![definition_b.clone(), definition_a.clone()];
    first_input.resolved_shared_state_definitions =
        vec![resolved_definition_b.clone(), resolved_definition_a.clone()];
    first_input.shared_state_acquisitions = vec![acquisition_a.clone(), acquisition_b.clone()];
    first_input.resolved_shared_state_acquisitions = vec![
        resolved_acquisition_a.clone(),
        resolved_acquisition_b.clone(),
    ];

    let mut reversed_input = empty_extracted_transport();
    reversed_input.resolved_definitions = vec![component_b.clone(), component_a.clone()];
    reversed_input.shared_state_definitions = vec![definition_a, definition_b];
    reversed_input.resolved_shared_state_definitions =
        vec![resolved_definition_a, resolved_definition_b];
    reversed_input.shared_state_acquisitions = vec![acquisition_b, acquisition_a];
    reversed_input.resolved_shared_state_acquisitions =
        vec![resolved_acquisition_b, resolved_acquisition_a];

    let link_unit = LinkUnitId::derive("shared-state-root");
    let first = resolve_extracted_transport(link_unit, first_input).unwrap();
    let reversed = resolve_extracted_transport(link_unit, reversed_input).unwrap();
    assert_eq!(first.material.payload, reversed.material.payload);
    assert_eq!(first.plan_id, reversed.plan_id);
    assert_eq!(first.shared_state_definitions.len(), 2);
    assert_eq!(first.shared_state_acquisitions.len(), 2);
    assert_eq!(
        first.shared_state_definitions[0].definition.record,
        [0x21; 32]
    );
    assert_eq!(
        first.shared_state_acquisitions[0].source_unit,
        CargoUnitId::from_bytes([0x30; 32])
    );
    assert_eq!(first.shared_state_acquisitions[0].target.record, [0x22; 32]);

    let json = transport_plan_json(&first);
    assert_eq!(
        json["schema_version"],
        serde_json::json!(TRANSPORT_LINK_SCHEMA_VERSION)
    );
    assert_eq!(
        json["shared_state_definitions"][0]["type_key"],
        serde_json::json!("acme.filters")
    );
    assert_eq!(
        json["shared_state_definitions"][0]["schema_abi_hex"],
        serde_json::json!("00ff10")
    );
    assert_eq!(
        json["shared_state_definitions"][0]["schema_abi_hash"],
        serde_json::json!(format!(
            "sha256:{}",
            hex_id(&rsjs_transport::shared_state_schema_abi_hash(&[
                0x00, 0xff, 0x10
            ]))
        ))
    );
    assert_eq!(
        json["shared_state_acquisitions"][0]["source"]["source_edge_id"],
        serde_json::json!(hex_id(&source_edge_b))
    );
    assert_eq!(
        json["shared_state_acquisitions"][0]["target"]["record_id"],
        serde_json::json!(hex_id(&[0x22; 32]))
    );
    assert_eq!(
        json["components"][0]["shared_state_acquisitions"][0]["store_id"],
        serde_json::json!(0)
    );
    assert_eq!(
        json["components"][0]["shared_state_acquisitions"][0]["source_edge_id"],
        serde_json::json!(hex_id(&source_edge_b))
    );

    let (changed_definition, changed_resolved_definition) =
        shared_state_definition(0x11, 0x21, "acme.filters", &[0x00, 0xff, 0x11]);
    let (unchanged_definition, unchanged_resolved_definition) =
        shared_state_definition(0x12, 0x22, "acme.workbench", &[0x01, 0x02]);
    let (changed_acquisition, changed_resolved_acquisition) =
        shared_state_acquisition(0x31, source_edge_a, &changed_definition);
    let (unchanged_acquisition, unchanged_resolved_acquisition) =
        shared_state_acquisition(0x30, source_edge_b, &unchanged_definition);
    let mut changed_input = empty_extracted_transport();
    changed_input.resolved_definitions = vec![component_a, component_b];
    changed_input.shared_state_definitions = vec![changed_definition, unchanged_definition];
    changed_input.resolved_shared_state_definitions =
        vec![changed_resolved_definition, unchanged_resolved_definition];
    changed_input.shared_state_acquisitions = vec![changed_acquisition, unchanged_acquisition];
    changed_input.resolved_shared_state_acquisitions =
        vec![changed_resolved_acquisition, unchanged_resolved_acquisition];
    let changed = resolve_extracted_transport(link_unit, changed_input).unwrap();
    assert_ne!(first.material.payload, changed.material.payload);
    assert_ne!(first.plan_id, changed.plan_id);
}

#[test]
fn shared_state_transport_fails_closed_when_resolution_drops_records() {
    let (definition, resolved_definition) =
        shared_state_definition(0x51, 0x61, "acme.preferences", &[0x01]);
    let mut extracted = empty_extracted_transport();
    extracted.shared_state_definitions.push(definition.clone());

    assert!(matches!(
        resolve_extracted_transport(LinkUnitId::derive("missing-shared-state"), extracted),
        Err(TransportLinkError::SharedStateResolutionMismatch {
            kind: "definition",
            ..
        })
    ));

    let (acquisition, resolved_acquisition) =
        shared_state_acquisition(0x52, [0x62; 32], &definition);
    let mut extracted = empty_extracted_transport();
    extracted.shared_state_definitions.push(definition.clone());
    extracted
        .resolved_shared_state_definitions
        .push(resolved_definition.clone());
    extracted
        .shared_state_acquisitions
        .push(acquisition.clone());

    assert!(matches!(
        resolve_extracted_transport(
            LinkUnitId::derive("missing-shared-state-acquisition"),
            extracted
        ),
        Err(TransportLinkError::SharedStateResolutionMismatch {
            kind: "acquisition",
            ..
        })
    ));

    let mut extracted = empty_extracted_transport();
    extracted.shared_state_definitions.push(definition);
    extracted
        .resolved_shared_state_definitions
        .push(resolved_definition);
    extracted.shared_state_acquisitions.push(acquisition);
    extracted
        .resolved_shared_state_acquisitions
        .push(resolved_acquisition);
    assert!(matches!(
        resolve_extracted_transport(
            LinkUnitId::derive("unclaimed-shared-state-acquisition"),
            extracted
        ),
        Err(TransportLinkError::UnclaimedSharedStateAcquisitions(_))
    ));
}

#[test]
fn shared_state_claim_ids_reject_swapped_or_forged_store_ownership() {
    let fingerprint = "sha256:shared-claim-integrity";
    let component = TransportDefinitionId {
        unit: CargoUnitId::from_bytes([0x71; 32]),
        record: [0x72; 32],
    };
    let store_zero = shared_state_acquisition_source_edge_id(fingerprint, 0);
    let store_one = shared_state_acquisition_source_edge_id(fingerprint, 1);
    assert_ne!(store_zero, store_one);

    let valid = ComponentSharedStateAcquisitionClaim {
        source_edge_id: hex_id(&store_zero),
        store_id: 0,
    };
    assert_eq!(
        validate_shared_state_acquisition_claim_record_id(fingerprint, component, &valid).unwrap(),
        store_zero
    );

    let swapped = ComponentSharedStateAcquisitionClaim {
        source_edge_id: hex_id(&store_one),
        store_id: 0,
    };
    assert!(matches!(
        validate_shared_state_acquisition_claim_record_id(fingerprint, component, &swapped),
        Err(TransportLinkError::SharedStateAcquisitionRecordIdMismatch { store_id: 0, .. })
    ));
}

#[test]
fn shared_state_acquisition_records_must_exist_and_be_claimed_once() {
    let unit = [0x81; 32];
    let source_edge = [0x82; 32];
    let component = TransportDefinitionId {
        unit: CargoUnitId::from_bytes(unit),
        record: [0x83; 32],
    };
    let missing = link_shared_state_acquisition_record(
        unit,
        component,
        0,
        source_edge,
        &BTreeMap::new(),
        &mut BTreeSet::new(),
    );
    assert!(matches!(
        missing,
        Err(TransportLinkError::MissingSharedStateAcquisition { store_id: 0, .. })
    ));

    let target = TransportDefinitionId {
        unit: CargoUnitId::from_bytes([0x84; 32]),
        record: [0x85; 32],
    };
    let linked = LinkedSharedStateAcquisition {
        source_unit: CargoUnitId::from_bytes(unit),
        source_edge,
        target,
    };
    let records = BTreeMap::from([((unit, source_edge), &linked)]);
    let mut claimed = BTreeSet::new();
    let first = link_shared_state_acquisition_record(
        unit,
        component,
        0,
        source_edge,
        &records,
        &mut claimed,
    )
    .unwrap();
    assert_eq!(first.target, target);
    assert!(matches!(
        link_shared_state_acquisition_record(
            unit,
            component,
            1,
            source_edge,
            &records,
            &mut claimed,
        ),
        Err(TransportLinkError::SharedStateAcquisitionClaimedTwice { .. })
    ));

    assert!(matches!(
        unique_shared_state_acquisitions(&[linked.clone(), linked]),
        Err(TransportLinkError::DuplicateSharedStateAcquisition { .. })
    ));
}

#[test]
fn rustc_component_abi_is_transport_plan_material_and_serialized() {
    fn plan(component_abi: ComponentDefinitionAbi) -> TransportLinkPlan {
        let link_unit = LinkUnitId::derive("component-abi-root");
        let component = LinkedTransportComponent {
            definition: TransportDefinitionId {
                unit: CargoUnitId::from_bytes([0x51; 32]),
                record: [0x52; 32],
            },
            identity: LinkedSemanticIdentity {
                package: "acme-web".to_owned(),
                crate_name: "acme_web".to_owned(),
                module_path: vec!["ui".to_owned()],
                item: "StatusBadge".to_owned(),
            },
            semantic_hash: SemanticHash::derive("status-badge-descriptor"),
            component_abi,
            component_edges: Vec::new(),
            helper_edges: Vec::new(),
            field_witnesses: Vec::new(),
            shared_state_acquisitions: Vec::new(),
            shared_state_provider_sites: Vec::new(),
            payload: Value::Null,
        };
        let resolved = ResolvedTransportPayload {
            schema_version: TRANSPORT_LINK_SCHEMA_VERSION,
            components: vec![component.clone()],
            helpers: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
        };
        let material = LinkPlanMaterial {
            link_unit,
            component_descriptors: vec![component.semantic_hash],
            helper_descriptors: Vec::new(),
            payload: SemanticHash::derive(&resolved),
        };
        TransportLinkPlan {
            link_unit,
            plan_id: material.plan_id(),
            material,
            components: vec![component],
            helpers: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
        }
    }

    let sync = plan(ComponentDefinitionAbi::SYNC_PARTICIPANT);
    let async_origin = plan(ComponentDefinitionAbi::new(
        TransportComponentRenderKind::Async,
        TransportComponentOriginPermission::OriginCapable,
    ));
    assert_ne!(sync.material.payload, async_origin.material.payload);
    assert_ne!(sync.plan_id, async_origin.plan_id);

    let json = transport_plan_json(&async_origin);
    assert_eq!(
        json["schema_version"],
        serde_json::json!(TRANSPORT_LINK_SCHEMA_VERSION)
    );
    assert_eq!(
        json["components"][0]["component_abi"],
        serde_json::json!({
            "flags": 3,
            "render_kind": "async",
            "origin_permission": "origin_capable",
        })
    );
}

#[test]
fn component_field_claim_ids_cover_crossing_role_and_shape() {
    let fingerprint = "sha256:component-field-test";
    let component = TransportDefinitionId {
        unit: CargoUnitId::from_bytes([0x11; 32]),
        record: [0x22; 32],
    };
    let record = component_field_record_id(
        fingerprint,
        0,
        7,
        TransportComponentFieldRole::Callback,
        TransportCallbackShape::Optional,
    );
    let mut claim = ComponentFieldWitnessClaim {
        callback_shape: "optional".to_owned(),
        crossing_hook_id: 7,
        field_id: 0,
        field_name: "on_change".to_owned(),
        field_record_id: hex_id(&record),
        role: "callback".to_owned(),
    };
    assert_eq!(
        validate_component_field_claim_record_id(fingerprint, component, &claim).unwrap(),
        record
    );

    claim.crossing_hook_id = 8;
    assert!(matches!(
        validate_component_field_claim_record_id(fingerprint, component, &claim),
        Err(TransportLinkError::ComponentFieldRecordIdMismatch { .. })
    ));
    claim.crossing_hook_id = 7;
    claim.role = "value".to_owned();
    assert!(matches!(
        validate_component_field_claim_record_id(fingerprint, component, &claim),
        Err(TransportLinkError::InvalidComponentFieldRoleShape { .. })
    ));

    let node_ref_record = component_field_record_id(
        fingerprint,
        1,
        9,
        TransportComponentFieldRole::NodeRef,
        TransportCallbackShape::NotCallback,
    );
    let node_ref_claim = ComponentFieldWitnessClaim {
        callback_shape: "not_callback".to_owned(),
        crossing_hook_id: 9,
        field_id: 1,
        field_name: "target".to_owned(),
        field_record_id: hex_id(&node_ref_record),
        role: "node_ref".to_owned(),
    };
    assert_eq!(
        validate_component_field_claim_record_id(fingerprint, component, &node_ref_claim,).unwrap(),
        node_ref_record
    );
}

#[test]
fn component_field_records_must_exist_once_and_target_the_claiming_component() {
    let unit = [0x31; 32];
    let field_record = [0x32; 32];
    let expected = TransportDefinitionId {
        unit: CargoUnitId::from_bytes(unit),
        record: [0x33; 32],
    };
    let key = (unit, field_record);

    let missing = link_component_field_record(
        unit,
        field_record,
        expected,
        "count",
        &BTreeMap::new(),
        &mut BTreeSet::new(),
    );
    assert!(matches!(
        missing,
        Err(TransportLinkError::MissingComponentField { .. })
    ));

    let mut already_claimed = BTreeSet::from([key]);
    let duplicate = link_component_field_record(
        unit,
        field_record,
        expected,
        "count",
        &BTreeMap::new(),
        &mut already_claimed,
    );
    assert!(matches!(
        duplicate,
        Err(TransportLinkError::ComponentFieldClaimedTwice { .. })
    ));

    let location = rsjs_transport::ArtifactLocation {
        artifact_path: PathBuf::from("/tmp/component-field.o"),
        archive_member: None,
        archive_member_index: None,
        section: ".rsjs.component_field".to_owned(),
        record_offset: 0,
    };
    let witness = rsjs_transport::RelocationIdentity::Symbol("foreign_component".to_owned());
    let resolved = rsjs_transport::ResolvedComponentField {
        field: rsjs_transport::ComponentField {
            location: location.clone(),
            cargo_unit_id: unit,
            field_record_id: field_record,
            witness: witness.clone(),
        },
        definition: rsjs_transport::ComponentDefinition {
            location,
            cargo_unit_id: unit,
            component_record_id: [0x44; 32],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness,
        },
    };
    let fields = BTreeMap::from([(key, &resolved)]);
    let foreign = link_component_field_record(
        unit,
        field_record,
        expected,
        "count",
        &fields,
        &mut BTreeSet::new(),
    );
    assert!(matches!(
        foreign,
        Err(TransportLinkError::ComponentFieldTargetMismatch {
            expected: target,
            actual,
            ..
        }) if target == expected && actual.record == [0x44; 32]
    ));
}
