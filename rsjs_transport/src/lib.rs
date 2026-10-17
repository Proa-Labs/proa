// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Fixed object-section records and exact-artifact extraction for RSJS.
//!
//! This crate is deliberately narrower than the future `cargo proa` linker. It
//! defines the descriptor-pass transport envelope, reads only artifact paths
//! supplied by its caller, and resolves component edges, field witnesses,
//! helper calls, and SharedState acquisitions by following object relocations.
//! Component/helper definition records also carry a dedicated inline
//! package/crate/module/item namespace generated at the defining expansion.
//! SharedState uses its explicit public `type_key` plus complete canonical
//! schema ABI bytes instead; its Rust nominal identity remains relocation-only.
//! The extractor never decodes target-native Rust metadata pointers. It does
//! not scan target directories, infer Rust identity from call-site token paths
//! or demangled symbols, or build a root-specific link plan.

mod canonical_json;
#[cfg(feature = "extract")]
mod extract;
mod wire;

pub use canonical_json::canonical_json_bytes;

#[cfg(feature = "extract")]
pub use extract::{
    extract_exact_artifact, extract_exact_artifacts, ArtifactLocation, ArtifactRecords,
    ComponentDefinition, ComponentDescriptorPayload, ComponentEdge, ComponentField,
    ExtractedTransport, ExtractionError, HelperDefinition, HelperEdge, RelocationIdentity,
    ResolvedComponentDefinition, ResolvedComponentEdge, ResolvedComponentField,
    ResolvedHelperDefinition, ResolvedHelperEdge, ResolvedSharedStateAcquisition,
    ResolvedSharedStateDefinition, SemanticIdentity, SharedStateAcquisition, SharedStateDefinition,
    SharedStateDefinitionPayload, StandaloneHelperPayload,
};
pub use wire::{
    component_field_record_id, component_helper_source_edge_id,
    component_lifecycle_predicate_graph_commitment, component_record_id, component_source_edge_id,
    decode_shared_state_schema_abi, encode_payload_record, encode_resolved_shared_state_schema_abi,
    encode_semantic_identity, encode_semantic_identity_record,
    encode_shared_state_definition_payload_record, encode_shared_state_schema_payload,
    encode_unresolved_shared_state_schema_abi, helper_direct_call_source_edge_id,
    semantic_payload_hash, shared_state_provider_source_edge_id, shared_state_schema_abi_hash,
    ComponentDefinitionAbi, ComponentDefinitionRecord, ComponentEdgeRecord, ComponentFieldRecord,
    FunctionRelocationSlot, HelperDefinitionRecord, HelperEdgeRecord, PayloadKind, PayloadRecord,
    RecordHeader, RecordKind, RelocationSlot, SemanticIdentityHeader, SemanticIdentityKind,
    SemanticIdentityRecord, SharedStateAcquisitionRecord, SharedStateDefinitionRecord,
    SharedStateSchemaAbi, SharedStateSchemaAbiError, SharedStateSchemaAbiResolution,
    SharedStateSchemaCodec, SharedStateSchemaEquality, SharedStateSchemaField,
    SharedStateSchemaUnresolvedReason, TransportCallbackShape, TransportComponentFieldRole,
    TransportComponentOriginPermission, TransportComponentRenderKind,
    TransportDirectComponentBinding, TransportDirectComponentBoundary,
    TransportDirectComponentBranch, TransportDirectComponentEdge, TransportDirectComponentField,
    TransportDirectComponentLifecycle, TransportDirectLiveCallbackForm,
    TransportDirectLiveValueSource, TransportDirectStaticCallbackForm, TransportRecordId,
    TransportSharedStateProviderSite, COFF_COMPONENT_FIELD_SECTION, COFF_COMPONENT_PAYLOAD_SECTION,
    COFF_COMPONENT_SECTION, COFF_EDGE_SECTION, COFF_HELPER_EDGE_SECTION,
    COFF_HELPER_PAYLOAD_SECTION, COFF_HELPER_SECTION, COFF_SEMANTIC_IDENTITY_SECTION,
    COFF_SHARED_STATE_ACQUISITION_SECTION, COFF_SHARED_STATE_PAYLOAD_SECTION,
    COFF_SHARED_STATE_SECTION, COMPONENT_DEFINITION_FLAG_ASYNC,
    COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE, COMPONENT_DEFINITION_KNOWN_FLAGS,
    ELF_COMPONENT_FIELD_SECTION, ELF_COMPONENT_PAYLOAD_SECTION, ELF_COMPONENT_SECTION,
    ELF_EDGE_SECTION, ELF_HELPER_EDGE_SECTION, ELF_HELPER_PAYLOAD_SECTION, ELF_HELPER_SECTION,
    ELF_SEMANTIC_IDENTITY_SECTION, ELF_SHARED_STATE_ACQUISITION_SECTION,
    ELF_SHARED_STATE_PAYLOAD_SECTION, ELF_SHARED_STATE_SECTION, MACHO_COMPONENT_FIELD_SECTION,
    MACHO_COMPONENT_PAYLOAD_SECTION, MACHO_COMPONENT_SECTION, MACHO_DATA_SEGMENT,
    MACHO_EDGE_SECTION, MACHO_HELPER_EDGE_SECTION, MACHO_HELPER_PAYLOAD_SECTION,
    MACHO_HELPER_SECTION, MACHO_SEMANTIC_IDENTITY_SECTION, MACHO_SHARED_STATE_ACQUISITION_SECTION,
    MACHO_SHARED_STATE_PAYLOAD_SECTION, MACHO_SHARED_STATE_SECTION, PAYLOAD_FORMAT_MAGIC,
    PAYLOAD_FORMAT_VERSION, PAYLOAD_HEADER_SIZE, SEMANTIC_IDENTITY_FORMAT_MAGIC,
    SEMANTIC_IDENTITY_FORMAT_VERSION, SEMANTIC_IDENTITY_HEADER_SIZE,
    SHARED_STATE_CANONICAL_ABI_HEADER_SIZE, SHARED_STATE_CANONICAL_ABI_MAGIC,
    SHARED_STATE_CANONICAL_ABI_VERSION, SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE,
    SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE, SHARED_STATE_RUNTIME_ABI_VERSION,
    SHARED_STATE_SCHEMA_FORMAT_MAGIC, SHARED_STATE_SCHEMA_FORMAT_VERSION,
    SHARED_STATE_SCHEMA_HEADER_SIZE, TRANSPORT_FORMAT_MAGIC, TRANSPORT_FORMAT_VERSION,
    TRANSPORT_RECORD_SIZE, UNASSIGNED_CARGO_UNIT_ID,
};
