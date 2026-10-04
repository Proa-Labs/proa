// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic linking of the RSJS records in one exact root manifest.
//!
//! This module is intentionally downstream of Cargo. It never scans a target
//! directory or resolves a Rust item from source-path text. The caller gives it
//! one validated [`LinkUnitManifest`], and every component/helper identity is
//! selected through the relocation records in the manifest's exact descriptor
//! objects. Stable semantic namespaces come only from the inline identity
//! record joined to that exact definition.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;
use rsjs_ir::abi::{
    CanonicalEncode, CanonicalEncoder, CargoUnitId, LinkUnitId, PlanId, SemanticHash,
    CLIENT_IR_SCHEMA_VERSION,
};
use rsjs_ir::build::{ArtifactRole, BuildIdentityError, LinkPlanMaterial, LinkUnitManifest};
use rsjs_transport::{
    canonical_json_bytes, component_field_record_id, component_record_id,
    shared_state_provider_source_edge_id, ArtifactRecords, ComponentDefinitionAbi,
    ExtractedTransport, ExtractionError, ResolvedComponentDefinition, ResolvedHelperDefinition,
    SemanticIdentity, TransportCallbackShape, TransportComponentFieldRole,
    TransportComponentOriginPermission, TransportComponentRenderKind, TransportRecordId,
    TransportSharedStateProviderSite, UNASSIGNED_CARGO_UNIT_ID,
};
use serde::Deserialize;
use serde_json::Value;
use sha2_accelerated::{Digest, Sha256};
use thiserror::Error;

#[cfg(test)]
use crate::rsjs::build::DESCRIPTOR_OBJECT_HASH_DOMAIN;
use crate::rsjs::build::{DescriptorObjectSnapshot, ValidatedDescriptorUnit};

const TRANSPORT_LINK_SCHEMA_VERSION: u16 = 5;
#[cfg(test)]
const LINK_ARTIFACT_HASH_DOMAIN: &[u8] = b"proa.rsjs.link-artifact.v1";
const COMPONENT_PAYLOAD_FORMAT: &str = "rsjs.component_transport_payload";
const COMPONENT_PAYLOAD_SCHEMA: u16 = 9;
const COMPONENT_DESCRIPTOR_FORMAT: &str = "rsjs.component_descriptor";
const COMPONENT_DESCRIPTOR_SCHEMA_VERSION: u64 = 9;
const HELPER_PAYLOAD_FORMAT: &str = "rsjs.standalone_helper_descriptor";
const COMPONENT_SEMANTIC_DOMAIN: &[u8] = b"proa.rsjs.component-transport-payload.semantic.v8\0";
const HELPER_SEMANTIC_DOMAIN: &[u8] = b"proa.rsjs.standalone-helper.semantic.v1\0";

static PLAN_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

// Parsed records are immutable owned data. Every lookup still follows fresh
// byte validation against both authorities; this never substitutes a prior
// filesystem observation for the current cache lease and content hashes.
type ExtractionKey = (PathBuf, [u8; 32]);
#[derive(Default)]
struct ExtractionCache {
    records: BTreeMap<ExtractionKey, Arc<ArtifactRecords>>,
    insertion_order: std::collections::VecDeque<ExtractionKey>,
}
static EXTRACTION_CACHE: OnceLock<Mutex<ExtractionCache>> = OnceLock::new();
const EXTRACTION_CACHE_LIMIT: usize = 2048;

fn parsed_descriptor_records(
    path: &Path,
    snapshot: &DescriptorObjectSnapshot,
) -> Result<Arc<ArtifactRecords>, ExtractionError> {
    let key = (path.to_path_buf(), snapshot.descriptor_hash);
    let cache = EXTRACTION_CACHE.get_or_init(Default::default);
    if let Some(records) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .records
        .get(&key)
    {
        return Ok(Arc::clone(records));
    }
    let records = Arc::new(ArtifactRecords::extract_exact_bytes(path, &snapshot.bytes)?);
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(existing) = cache.records.get(&key) {
        return Ok(Arc::clone(existing));
    }
    while cache.records.len() >= EXTRACTION_CACHE_LIMIT {
        if let Some(oldest) = cache.insertion_order.pop_front() {
            cache.records.remove(&oldest);
        }
    }
    cache.insertion_order.push_back(key.clone());
    cache.records.insert(key, Arc::clone(&records));
    Ok(records)
}

/// Exact build-local identity of one component or helper definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransportDefinitionId {
    /// Descriptor-pass Cargo unit that emitted the record.
    pub unit: CargoUnitId,
    /// Definition record ID embedded beside its Rust relocation witness.
    pub record: TransportRecordId,
}

impl CanonicalEncode for TransportDefinitionId {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("TransportDefinitionId", 2);
        encoder.field(1, &self.unit);
        encoder.field(2, &self.record);
    }
}

/// One direct component edge after rustc relocation resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedComponentEdge {
    /// Parent-local source edge record.
    pub source_edge: TransportRecordId,
    /// Exact child definition selected by the relocation.
    pub target: TransportDefinitionId,
}

impl CanonicalEncode for LinkedComponentEdge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedComponentEdge", 2);
        encoder.field(1, &self.source_edge);
        encoder.field(2, &self.target);
    }
}

/// One client-helper call after rustc relocation resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedHelperEdge {
    /// Component/helper-local source edge record.
    pub source_edge: TransportRecordId,
    /// Exact standalone helper selected by the relocation.
    pub target: TransportDefinitionId,
}

impl CanonicalEncode for LinkedHelperEdge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedHelperEdge", 2);
        encoder.field(1, &self.source_edge);
        encoder.field(2, &self.target);
    }
}

/// One client-visible component field after its rustc relocation has been
/// joined to the exact defining component.
///
/// The behavior-bearing field/crossing/role/shape claim remains in the
/// producer payload. `field_record` is independently recomputed from that
/// claim before this value is constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedComponentFieldWitness {
    /// Deterministic field record selected from the exact object.
    pub field_record: TransportRecordId,
    /// Exact component definition reached by the record relocation.
    pub target: TransportDefinitionId,
}

impl CanonicalEncode for LinkedComponentFieldWitness {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedComponentFieldWitness", 2);
        encoder.field(1, &self.field_record);
        encoder.field(2, &self.target);
    }
}

/// One descriptor store owned by a relocation-resolved SharedState
/// acquisition.
///
/// `store_id` is local to the defining component descriptor. `source_edge`
/// joins that semantic slot to the exact object record; `target` is the
/// SharedState definition rustc selected through the acquired type's ABI
/// witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedComponentSharedStateAcquisition {
    /// Dense component-local SharedStoreId.
    pub store_id: u32,
    /// Exact component-local acquisition record.
    pub source_edge: TransportRecordId,
    /// Rust-resolved SharedState definition.
    pub target: TransportDefinitionId,
}

impl CanonicalEncode for LinkedComponentSharedStateAcquisition {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedComponentSharedStateAcquisition", 3);
        encoder.field(1, &self.store_id);
        encoder.field(2, &self.source_edge);
        encoder.field(3, &self.target);
    }
}

/// One static SharedState provider declaration after both of its nominal Rust
/// edges have been resolved.
///
/// `source_edge` selects the provider's exact SharedState witness relocation.
/// `child_source_edge` selects the already-linked direct child component edge.
/// The complete server expression and source metadata remain in `payload` for
/// the later final-child rewrite; this canonical record carries only the
/// authenticated topology and identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedComponentSharedStateProviderSite {
    /// Dense component-local provider site ID.
    pub provider_site_id: u32,
    /// Exact component-local provider witness record.
    pub source_edge: TransportRecordId,
    /// Rust-resolved SharedState definition.
    pub target: TransportDefinitionId,
    /// Dense component-local direct child edge ordinal.
    pub child_edge_local_ordinal: u32,
    /// Exact direct child source edge committed by the provider claim.
    pub child_source_edge: TransportRecordId,
    /// Rust-resolved child component definition.
    pub child_target: TransportDefinitionId,
    /// Whole-provider server node retained by the producer descriptor.
    pub site_node: u32,
}

impl CanonicalEncode for LinkedComponentSharedStateProviderSite {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedComponentSharedStateProviderSite", 7);
        encoder.field(1, &self.provider_site_id);
        encoder.field(2, &self.source_edge);
        encoder.field(3, &self.target);
        encoder.field(4, &self.child_edge_local_ordinal);
        encoder.field(5, &self.child_source_edge);
        encoder.field(6, &self.child_target);
        encoder.field(7, &self.site_node);
    }
}

/// Stable namespace emitted from `env!` and `module_path!` beside one exact
/// definition.
///
/// This is deliberately separate from [`TransportDefinitionId`]: rebuilds and
/// relinks change transport identity without changing this declared ABI key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkedSemanticIdentity {
    /// Declared Cargo package namespace.
    pub package: String,
    /// Normalized Rust crate name.
    pub crate_name: String,
    /// Defining module segments, excluding the crate prefix.
    pub module_path: Vec<String>,
    /// Defining component/helper item name.
    pub item: String,
}

impl From<&SemanticIdentity> for LinkedSemanticIdentity {
    fn from(identity: &SemanticIdentity) -> Self {
        Self {
            package: identity.package.clone(),
            crate_name: identity.crate_name.clone(),
            module_path: identity.module_path.clone(),
            item: identity.item.clone(),
        }
    }
}

impl CanonicalEncode for LinkedSemanticIdentity {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSemanticIdentity", 4);
        encoder.field(1, &self.package);
        encoder.field(2, &self.crate_name);
        encoder.field(3, &self.module_path);
        encoder.field(4, &self.item);
    }
}

/// One component descriptor and all of its relocation-resolved outgoing edges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedTransportComponent {
    /// Exact defining record.
    pub definition: TransportDefinitionId,
    /// Stable package/crate/module/item ABI identity emitted beside that exact
    /// record.
    pub identity: LinkedSemanticIdentity,
    /// Producer-validated behavior hash of the complete component payload.
    pub semantic_hash: SemanticHash,
    /// Rust-checked server render surface and client-origin permission carried
    /// by the defining witness record.
    pub component_abi: ComponentDefinitionAbi,
    /// Direct component edges in stable source-edge order.
    pub component_edges: Vec<LinkedComponentEdge>,
    /// Reachable standalone-helper edges in stable source-edge order.
    pub helper_edges: Vec<LinkedHelperEdge>,
    /// Rust-checked client-visible input fields in deterministic record order.
    pub field_witnesses: Vec<LinkedComponentFieldWitness>,
    /// Descriptor-local SharedState stores joined to exact rustc-resolved
    /// acquisition records.
    pub shared_state_acquisitions: Vec<LinkedComponentSharedStateAcquisition>,
    /// Static SharedState provider declarations authenticated against both
    /// their state ABI witness and direct child component edge.
    pub shared_state_provider_sites: Vec<LinkedComponentSharedStateProviderSite>,
    /// Complete embedded payload retained for the semantic ABI adapter and
    /// final-plan diagnostics. Identity comes from `semantic_hash`, not from
    /// host paths or object offsets.
    pub payload: Value,
}

impl CanonicalEncode for LinkedTransportComponent {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedTransportComponent", 9);
        encoder.field(1, &self.definition);
        encoder.field(2, &self.identity);
        encoder.field(3, &self.semantic_hash);
        encoder.field(4, &self.component_abi.flags());
        encoder.field(5, &self.component_edges);
        encoder.field(6, &self.helper_edges);
        encoder.field(7, &self.field_witnesses);
        encoder.field(8, &self.shared_state_acquisitions);
        encoder.field(9, &self.shared_state_provider_sites);
    }
}

/// One standalone helper and its relocation-resolved transitive calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedTransportHelper {
    /// Exact defining record.
    pub definition: TransportDefinitionId,
    /// Stable package/crate/module/item ABI identity emitted beside that exact
    /// record.
    pub identity: LinkedSemanticIdentity,
    /// Producer-validated behavior hash of the complete helper descriptor.
    pub semantic_hash: SemanticHash,
    /// Direct helper calls in stable source-edge order.
    pub helper_edges: Vec<LinkedHelperEdge>,
    /// Complete embedded payload retained for the semantic ABI adapter.
    pub payload: Value,
}

impl CanonicalEncode for LinkedTransportHelper {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedTransportHelper", 4);
        encoder.field(1, &self.definition);
        encoder.field(2, &self.identity);
        encoder.field(3, &self.semantic_hash);
        encoder.field(4, &self.helper_edges);
    }
}

/// One exact SharedState definition after its schema payload has been joined
/// to the rustc-resolved nominal-type witness record.
///
/// This transport-level value intentionally does not decide whether two
/// definitions are schema-compatible or which components acquire them. Those
/// are semantic-linker responsibilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSharedStateDefinition {
    /// Exact defining record selected from one descriptor-pass Cargo unit.
    pub definition: TransportDefinitionId,
    /// Domain-separated hash of the complete SharedState schema payload.
    pub schema_payload_hash: TransportRecordId,
    /// Explicit public protocol key carried by the definition payload.
    pub type_key: String,
    /// Domain-separated hash of the canonical schema ABI bytes.
    pub schema_abi_hash: TransportRecordId,
    /// Complete canonical schema ABI, preserved byte-for-byte.
    pub schema_abi: Vec<u8>,
}

impl CanonicalEncode for LinkedSharedStateDefinition {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSharedStateDefinition", 5);
        encoder.field(1, &self.definition);
        encoder.field(2, &self.schema_payload_hash);
        encoder.field(3, &self.type_key);
        encoder.field(4, &self.schema_abi_hash);
        encoder.field(5, &self.schema_abi);
    }
}

/// One exact component-local `rsjs::shared::<T>()` acquisition after rustc
/// relocation resolution.
///
/// `source_unit` plus `source_edge` is the complete transport identity of the
/// acquisition. The owning component is deliberately unresolved here: the
/// transport must not reconstruct ownership from Rust path text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSharedStateAcquisition {
    /// Descriptor-pass Cargo unit containing the acquisition record.
    pub source_unit: CargoUnitId,
    /// Stable component-local source-edge identifier.
    pub source_edge: TransportRecordId,
    /// Exact SharedState definition selected by the relocation.
    pub target: TransportDefinitionId,
}

impl CanonicalEncode for LinkedSharedStateAcquisition {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSharedStateAcquisition", 3);
        encoder.field(1, &self.source_unit);
        encoder.field(2, &self.source_edge);
        encoder.field(3, &self.target);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedTransportPayload {
    schema_version: u16,
    components: Vec<LinkedTransportComponent>,
    helpers: Vec<LinkedTransportHelper>,
    shared_state_definitions: Vec<LinkedSharedStateDefinition>,
    shared_state_acquisitions: Vec<LinkedSharedStateAcquisition>,
}

impl CanonicalEncode for ResolvedTransportPayload {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ResolvedTransportPayload", 5);
        encoder.field(1, &self.schema_version);
        encoder.field(2, &self.components);
        encoder.field(3, &self.helpers);
        encoder.field(4, &self.shared_state_definitions);
        encoder.field(5, &self.shared_state_acquisitions);
    }
}

/// Root-specific, content-addressed result of exact transport resolution.
///
/// This is the deterministic input boundary for conversion into the canonical
/// component/helper ABI and owner linker. It does not claim that the semantic
/// component graph has already been composed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportLinkPlan {
    /// Exact pre-link root identity.
    pub link_unit: LinkUnitId,
    /// Content identity of this resolved transport graph.
    pub plan_id: PlanId,
    /// Hash material used to derive `plan_id`.
    pub material: LinkPlanMaterial,
    /// Resolved components in canonical definition order.
    pub components: Vec<LinkedTransportComponent>,
    /// Resolved helpers in canonical definition order.
    pub helpers: Vec<LinkedTransportHelper>,
    /// SharedState definitions in canonical exact-record order.
    pub shared_state_definitions: Vec<LinkedSharedStateDefinition>,
    /// SharedState acquisitions in canonical source-record order.
    pub shared_state_acquisitions: Vec<LinkedSharedStateAcquisition>,
}

/// Extract and relocation-link the exact descriptor objects in a root
/// manifest.
pub(crate) fn link_transport_manifest(
    manifest: &LinkUnitManifest,
    validated_units: &[ValidatedDescriptorUnit],
    objects: &BTreeMap<std::path::PathBuf, DescriptorObjectSnapshot>,
) -> Result<TransportLinkPlan, TransportLinkError> {
    manifest
        .validate()
        .map_err(TransportLinkError::InvalidManifest)?;
    let link_unit = manifest
        .link_unit_id()
        .map_err(TransportLinkError::InvalidManifest)?;

    let descriptor_artifacts = descriptor_artifacts(manifest, validated_units)?;
    let extracted = extract_descriptor_artifact_snapshots(&descriptor_artifacts, objects)?;

    let manifest_artifacts = descriptor_artifacts
        .iter()
        .map(|evidence| evidence.artifact)
        .collect::<Vec<_>>();
    validate_artifact_units(&manifest_artifacts, &extracted)?;
    resolve_extracted_transport(link_unit, extracted)
}

#[derive(Clone, Copy)]
struct DescriptorArtifactEvidence<'a> {
    artifact: &'a rsjs_ir::build::LinkArtifact,
    descriptor_hash: [u8; 32],
}

#[cfg(test)]
fn extract_descriptor_artifacts(
    descriptor_artifacts: &[DescriptorArtifactEvidence<'_>],
) -> Result<ExtractedTransport, TransportLinkError> {
    extract_descriptor_artifact_snapshots(descriptor_artifacts, &BTreeMap::new())
}

fn extract_descriptor_artifact_snapshots(
    descriptor_artifacts: &[DescriptorArtifactEvidence<'_>],
    objects: &BTreeMap<std::path::PathBuf, DescriptorObjectSnapshot>,
) -> Result<ExtractedTransport, TransportLinkError> {
    let results = descriptor_artifacts
        .par_iter()
        .map(|evidence| {
            let artifact = evidence.artifact;
            let path = Path::new(&artifact.path);
            let owned;
            let snapshot = match objects.get(path) {
                Some(snapshot) => snapshot,
                None => {
                    owned = DescriptorObjectSnapshot::read(path).map_err(|source| {
                        TransportLinkError::HashDescriptorObject {
                            path: path.to_path_buf(),
                            source,
                        }
                    })?;
                    &owned
                }
            };
            let actual_descriptor = snapshot.descriptor_hash;
            if actual_descriptor != evidence.descriptor_hash {
                return Err(TransportLinkError::DescriptorObjectIdentityHashMismatch {
                    path: path.to_path_buf(),
                    expected: evidence.descriptor_hash,
                    actual: actual_descriptor,
                });
            }
            let actual = SemanticHash::from_bytes(snapshot.link_hash);
            if actual != artifact.content {
                return Err(TransportLinkError::DescriptorObjectHashMismatch {
                    path: path.to_path_buf(),
                    expected: artifact.content,
                    actual,
                });
            }
            parsed_descriptor_records(path, &snapshot)
                .map(|records| records.as_ref().clone())
                .map_err(Into::into)
        })
        .collect::<Vec<Result<_, TransportLinkError>>>();
    // Indexed parallel iteration preserves input order. Consume the results
    // sequentially so multiple failures still report the first manifest entry
    // deterministically.
    let records = results.into_iter().collect::<Result<Vec<_>, _>>()?;
    ExtractedTransport::resolve_exact_artifacts(records).map_err(Into::into)
}

#[cfg(test)]
fn descriptor_object_content_hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DESCRIPTOR_OBJECT_HASH_DOMAIN);
    hasher.update(bytes);
    hasher.finalize().into()
}

#[cfg(test)]
fn link_artifact_content_hash_bytes(bytes: &[u8]) -> SemanticHash {
    let mut hasher = Sha256::new();
    hasher.update(LINK_ARTIFACT_HASH_DOMAIN);
    hasher.update(bytes);
    SemanticHash::from_bytes(hasher.finalize().into())
}

fn descriptor_artifacts<'a>(
    manifest: &'a LinkUnitManifest,
    validated_units: &[ValidatedDescriptorUnit],
) -> Result<Vec<DescriptorArtifactEvidence<'a>>, TransportLinkError> {
    let mut validated_by_unit = BTreeMap::new();
    for unit in validated_units {
        if validated_by_unit
            .insert(unit.unit.as_abi_id(), unit)
            .is_some()
        {
            return Err(TransportLinkError::DuplicateValidatedDescriptorUnit(
                unit.unit.as_abi_id(),
            ));
        }
    }
    let manifest_units = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.unit)
        .collect::<BTreeSet<_>>();
    let mut seen_units = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut descriptors = Vec::new();

    for artifact in manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.role == ArtifactRole::DescriptorObject)
    {
        let path = Path::new(&artifact.path);
        if !path.is_absolute() {
            return Err(TransportLinkError::RelativeDescriptorObject(
                path.to_path_buf(),
            ));
        }
        if !paths.insert(path.to_path_buf()) {
            return Err(TransportLinkError::DuplicateDescriptorPath(
                path.to_path_buf(),
            ));
        }
        if !seen_units.insert(artifact.unit) {
            return Err(TransportLinkError::DuplicateDescriptorUnit(artifact.unit));
        }
        let validated = validated_by_unit.get(&artifact.unit).ok_or(
            TransportLinkError::MissingValidatedDescriptorUnit(artifact.unit),
        )?;
        if validated.descriptor_object != path {
            return Err(TransportLinkError::ValidatedDescriptorPathMismatch {
                unit: artifact.unit,
                manifest: path.to_path_buf(),
                validated: validated.descriptor_object.clone(),
            });
        }
        let descriptor_hash =
            parse_hex_32(&validated.sidecar.descriptor_object_hash).map_err(|_| {
                TransportLinkError::InvalidValidatedDescriptorHash {
                    unit: artifact.unit,
                    value: validated.sidecar.descriptor_object_hash.clone(),
                }
            })?;
        descriptors.push(DescriptorArtifactEvidence {
            artifact,
            descriptor_hash,
        });
    }

    if descriptors.is_empty() {
        return Err(TransportLinkError::NoDescriptorObjects);
    }
    if seen_units != manifest_units {
        return Err(TransportLinkError::DescriptorUnitSetMismatch {
            expected: manifest_units,
            actual: seen_units,
        });
    }
    Ok(descriptors)
}

fn validate_artifact_units(
    descriptor_artifacts: &[&rsjs_ir::build::LinkArtifact],
    extracted: &ExtractedTransport,
) -> Result<(), TransportLinkError> {
    if descriptor_artifacts.len() != extracted.artifacts.len() {
        return Err(TransportLinkError::ExtractionArtifactCount {
            expected: descriptor_artifacts.len(),
            actual: extracted.artifacts.len(),
        });
    }

    for (artifact, records) in descriptor_artifacts.iter().zip(&extracted.artifacts) {
        let expected = *artifact.unit.as_bytes();
        let actual_path = records.artifact_path.as_path();
        if actual_path != Path::new(&artifact.path) {
            return Err(TransportLinkError::ExtractionArtifactOrder {
                expected: PathBuf::from(&artifact.path),
                actual: records.artifact_path.clone(),
            });
        }

        for (kind, unit, location) in
            records
                .definitions
                .iter()
                .map(|record| {
                    (
                        "component definition",
                        record.cargo_unit_id,
                        &record.location,
                    )
                })
                .chain(
                    records
                        .edges
                        .iter()
                        .map(|record| ("component edge", record.cargo_unit_id, &record.location)),
                )
                .chain(
                    records
                        .component_fields
                        .iter()
                        .map(|record| ("component field", record.cargo_unit_id, &record.location)),
                )
                .chain(
                    records.helper_definitions.iter().map(|record| {
                        ("helper definition", record.cargo_unit_id, &record.location)
                    }),
                )
                .chain(
                    records
                        .helper_edges
                        .iter()
                        .map(|record| ("helper edge", record.cargo_unit_id, &record.location)),
                )
                .chain(records.shared_state_definitions.iter().map(|record| {
                    (
                        "SharedState definition",
                        record.cargo_unit_id,
                        &record.location,
                    )
                }))
                .chain(records.shared_state_acquisitions.iter().map(|record| {
                    (
                        "SharedState acquisition",
                        record.cargo_unit_id,
                        &record.location,
                    )
                }))
                .chain(
                    records.component_payloads.iter().map(|record| {
                        ("component payload", record.cargo_unit_id, &record.location)
                    }),
                )
                .chain(
                    records
                        .helper_payloads
                        .iter()
                        .map(|record| ("helper payload", record.cargo_unit_id, &record.location)),
                )
                .chain(records.shared_state_payloads.iter().map(|record| {
                    (
                        "SharedState schema payload",
                        record.cargo_unit_id,
                        &record.location,
                    )
                }))
                .chain(
                    records.semantic_identities.iter().map(|record| {
                        ("semantic identity", record.cargo_unit_id, &record.location)
                    }),
                )
        {
            if unit == UNASSIGNED_CARGO_UNIT_ID {
                return Err(TransportLinkError::UnassignedCargoUnit {
                    kind,
                    location: location.to_string(),
                });
            }
            if unit != expected {
                return Err(TransportLinkError::CargoUnitMismatch {
                    kind,
                    location: location.to_string(),
                    expected: artifact.unit,
                    actual: hex_id(&unit),
                });
            }
        }
    }
    Ok(())
}

fn resolve_extracted_transport(
    link_unit: LinkUnitId,
    extracted: ExtractedTransport,
) -> Result<TransportLinkPlan, TransportLinkError> {
    let (shared_state_definitions, shared_state_acquisitions) =
        resolve_shared_state_transport(&extracted)?;
    let shared_state_acquisitions_by_source =
        unique_shared_state_acquisitions(&shared_state_acquisitions)?;
    let component_edges = unique_component_edges(&extracted)?;
    let component_fields = unique_component_fields(&extracted)?;
    let helper_edges = unique_helper_edges(&extracted)?;
    let mut claimed_components = BTreeSet::new();
    let mut claimed_fields = BTreeSet::new();
    let mut claimed_helpers = BTreeSet::new();
    let mut claimed_shared_state_acquisitions = BTreeSet::new();

    let mut components = extracted
        .resolved_definitions
        .iter()
        .map(|definition| {
            resolve_component(
                definition,
                &component_edges,
                &component_fields,
                &helper_edges,
                &shared_state_acquisitions_by_source,
                &mut claimed_components,
                &mut claimed_fields,
                &mut claimed_helpers,
                &mut claimed_shared_state_acquisitions,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    components.sort_by_key(|component| component.definition);

    let mut helpers = extracted
        .resolved_helper_definitions
        .iter()
        .map(|definition| resolve_helper(definition, &helper_edges, &mut claimed_helpers))
        .collect::<Result<Vec<_>, _>>()?;
    helpers.sort_by_key(|helper| helper.definition);

    let all_component_edges = component_edges.keys().copied().collect::<BTreeSet<_>>();
    if claimed_components != all_component_edges {
        return Err(TransportLinkError::UnclaimedComponentEdges(
            set_difference_hex(&all_component_edges, &claimed_components),
        ));
    }
    let all_component_fields = component_fields.keys().copied().collect::<BTreeSet<_>>();
    if claimed_fields != all_component_fields {
        return Err(TransportLinkError::UnclaimedComponentFields(
            set_difference_hex(&all_component_fields, &claimed_fields),
        ));
    }
    let all_helper_edges = helper_edges.keys().copied().collect::<BTreeSet<_>>();
    if claimed_helpers != all_helper_edges {
        return Err(TransportLinkError::UnclaimedHelperEdges(
            set_difference_hex(&all_helper_edges, &claimed_helpers),
        ));
    }
    let all_shared_state_acquisitions = shared_state_acquisitions_by_source
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    if claimed_shared_state_acquisitions != all_shared_state_acquisitions {
        return Err(TransportLinkError::UnclaimedSharedStateAcquisitions(
            set_difference_hex(
                &all_shared_state_acquisitions,
                &claimed_shared_state_acquisitions,
            ),
        ));
    }

    let payload = ResolvedTransportPayload {
        schema_version: TRANSPORT_LINK_SCHEMA_VERSION,
        components: components.clone(),
        helpers: helpers.clone(),
        shared_state_definitions: shared_state_definitions.clone(),
        shared_state_acquisitions: shared_state_acquisitions.clone(),
    };
    let payload_hash = SemanticHash::derive(&payload);
    let material = LinkPlanMaterial {
        link_unit,
        component_descriptors: components
            .iter()
            .map(|component| component.semantic_hash)
            .collect(),
        helper_descriptors: helpers.iter().map(|helper| helper.semantic_hash).collect(),
        payload: payload_hash,
    };
    let plan_id = material.plan_id();

    Ok(TransportLinkPlan {
        link_unit,
        plan_id,
        material,
        components,
        helpers,
        shared_state_definitions,
        shared_state_acquisitions,
    })
}

type SourceEdgeKey = (TransportRecordId, TransportRecordId);

fn unique_shared_state_acquisitions(
    acquisitions: &[LinkedSharedStateAcquisition],
) -> Result<BTreeMap<SourceEdgeKey, &LinkedSharedStateAcquisition>, TransportLinkError> {
    let mut by_source = BTreeMap::new();
    for acquisition in acquisitions {
        let key = (*acquisition.source_unit.as_bytes(), acquisition.source_edge);
        if by_source.insert(key, acquisition).is_some() {
            return Err(TransportLinkError::DuplicateSharedStateAcquisition {
                unit: hex_id(&key.0),
                source_edge: hex_id(&key.1),
            });
        }
    }
    Ok(by_source)
}

fn resolve_shared_state_transport(
    extracted: &ExtractedTransport,
) -> Result<
    (
        Vec<LinkedSharedStateDefinition>,
        Vec<LinkedSharedStateAcquisition>,
    ),
    TransportLinkError,
> {
    let mut extracted_definition_keys = extracted
        .shared_state_definitions
        .iter()
        .map(|definition| (definition.cargo_unit_id, definition.shared_state_record_id))
        .collect::<Vec<_>>();
    let mut resolved_definition_keys = extracted
        .resolved_shared_state_definitions
        .iter()
        .map(|definition| {
            (
                definition.definition.cargo_unit_id,
                definition.definition.shared_state_record_id,
            )
        })
        .collect::<Vec<_>>();
    extracted_definition_keys.sort_unstable();
    resolved_definition_keys.sort_unstable();
    if extracted_definition_keys != resolved_definition_keys {
        return Err(TransportLinkError::SharedStateResolutionMismatch {
            kind: "definition",
            extracted: source_keys_hex(&extracted_definition_keys),
            resolved: source_keys_hex(&resolved_definition_keys),
        });
    }

    let mut definitions = extracted
        .resolved_shared_state_definitions
        .iter()
        .map(|definition| LinkedSharedStateDefinition {
            definition: TransportDefinitionId {
                unit: CargoUnitId::from_bytes(definition.definition.cargo_unit_id),
                record: definition.definition.shared_state_record_id,
            },
            schema_payload_hash: definition.payload.semantic_payload_hash,
            type_key: definition.payload.type_key.clone(),
            schema_abi_hash: definition.payload.schema_abi_hash,
            schema_abi: definition.payload.schema_abi.clone(),
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|definition| definition.definition);
    let definition_ids = definitions
        .iter()
        .map(|definition| definition.definition)
        .collect::<BTreeSet<_>>();

    let mut extracted_acquisition_keys = extracted
        .shared_state_acquisitions
        .iter()
        .map(|acquisition| (acquisition.cargo_unit_id, acquisition.source_edge_id))
        .collect::<Vec<_>>();
    let mut resolved_acquisition_keys = extracted
        .resolved_shared_state_acquisitions
        .iter()
        .map(|acquisition| {
            (
                acquisition.acquisition.cargo_unit_id,
                acquisition.acquisition.source_edge_id,
            )
        })
        .collect::<Vec<_>>();
    extracted_acquisition_keys.sort_unstable();
    resolved_acquisition_keys.sort_unstable();
    if extracted_acquisition_keys != resolved_acquisition_keys {
        return Err(TransportLinkError::SharedStateResolutionMismatch {
            kind: "acquisition",
            extracted: source_keys_hex(&extracted_acquisition_keys),
            resolved: source_keys_hex(&resolved_acquisition_keys),
        });
    }

    let mut acquisitions = Vec::with_capacity(extracted.resolved_shared_state_acquisitions.len());
    for acquisition in &extracted.resolved_shared_state_acquisitions {
        let target = TransportDefinitionId {
            unit: CargoUnitId::from_bytes(acquisition.definition.cargo_unit_id),
            record: acquisition.definition.shared_state_record_id,
        };
        if !definition_ids.contains(&target) {
            return Err(TransportLinkError::MissingSharedStateAcquisitionTarget {
                source_unit: CargoUnitId::from_bytes(acquisition.acquisition.cargo_unit_id),
                source_edge: hex_id(&acquisition.acquisition.source_edge_id),
                target,
            });
        }
        acquisitions.push(LinkedSharedStateAcquisition {
            source_unit: CargoUnitId::from_bytes(acquisition.acquisition.cargo_unit_id),
            source_edge: acquisition.acquisition.source_edge_id,
            target,
        });
    }
    acquisitions.sort_by_key(|acquisition| (acquisition.source_unit, acquisition.source_edge));

    Ok((definitions, acquisitions))
}

fn unique_component_edges(
    extracted: &ExtractedTransport,
) -> Result<BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedComponentEdge>, TransportLinkError> {
    let mut edges = BTreeMap::new();
    for edge in &extracted.resolved_edges {
        let key = (edge.edge.cargo_unit_id, edge.edge.source_edge_id);
        if edges.insert(key, edge).is_some() {
            return Err(TransportLinkError::DuplicateSourceEdge {
                kind: "component",
                unit: hex_id(&key.0),
                source_edge: hex_id(&key.1),
            });
        }
    }
    Ok(edges)
}

fn unique_helper_edges(
    extracted: &ExtractedTransport,
) -> Result<BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedHelperEdge>, TransportLinkError> {
    let mut edges = BTreeMap::new();
    for edge in &extracted.resolved_helper_edges {
        let key = (edge.edge.cargo_unit_id, edge.edge.source_edge_id);
        if edges.insert(key, edge).is_some() {
            return Err(TransportLinkError::DuplicateSourceEdge {
                kind: "helper",
                unit: hex_id(&key.0),
                source_edge: hex_id(&key.1),
            });
        }
    }
    Ok(edges)
}

fn unique_component_fields(
    extracted: &ExtractedTransport,
) -> Result<BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedComponentField>, TransportLinkError> {
    let mut fields = BTreeMap::new();
    for field in &extracted.resolved_component_fields {
        let key = (field.field.cargo_unit_id, field.field.field_record_id);
        if fields.insert(key, field).is_some() {
            return Err(TransportLinkError::DuplicateComponentField {
                unit: hex_id(&key.0),
                field_record: hex_id(&key.1),
            });
        }
    }
    Ok(fields)
}

fn resolve_component(
    definition: &ResolvedComponentDefinition,
    component_edges: &BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedComponentEdge>,
    component_fields: &BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedComponentField>,
    helper_edges: &BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedHelperEdge>,
    shared_state_acquisitions: &BTreeMap<SourceEdgeKey, &LinkedSharedStateAcquisition>,
    claimed_components: &mut BTreeSet<SourceEdgeKey>,
    claimed_fields: &mut BTreeSet<SourceEdgeKey>,
    claimed_helpers: &mut BTreeSet<SourceEdgeKey>,
    claimed_shared_state_acquisitions: &mut BTreeSet<SourceEdgeKey>,
) -> Result<LinkedTransportComponent, TransportLinkError> {
    let payload_value = parse_payload_json("component", &definition.payload.json)?;
    validate_embedded_semantic_hash("component", &payload_value, COMPONENT_SEMANTIC_DOMAIN)?;
    let payload: ComponentPayload =
        serde_json::from_value(payload_value.clone()).map_err(|source| {
            TransportLinkError::InvalidPayloadSchema {
                kind: "component",
                location: definition.payload.location.to_string(),
                source,
            }
        })?;
    if payload.format != COMPONENT_PAYLOAD_FORMAT
        || payload.schema_version != COMPONENT_PAYLOAD_SCHEMA
    {
        return Err(TransportLinkError::UnsupportedPayload {
            kind: "component",
            format: payload.format,
            schema_version: payload.schema_version,
        });
    }
    validate_component_descriptor(&payload.component_descriptor)?;

    let expected_record = definition.definition.component_record_id;
    let payload_record = parse_record_id(
        "component transport definition_record_id",
        &payload.transport.definition_record_id,
    )?;
    if payload_record != expected_record {
        return Err(TransportLinkError::PayloadDefinitionMismatch {
            kind: "component",
            expected: hex_id(&expected_record),
            actual: hex_id(&payload_record),
        });
    }
    let fingerprint_record = component_record_id(&payload.transport.component_input_fingerprint);
    if fingerprint_record != expected_record {
        return Err(TransportLinkError::PayloadDefinitionMismatch {
            kind: "component fingerprint-derived",
            expected: hex_id(&expected_record),
            actual: hex_id(&fingerprint_record),
        });
    }
    let unit = definition.definition.cargo_unit_id;
    let definition_id = TransportDefinitionId {
        unit: CargoUnitId::from_bytes(unit),
        record: expected_record,
    };
    validate_component_shared_state_claim_shape(
        definition_id,
        &payload.component_descriptor,
        &payload.transport.shared_state_acquisitions,
    )?;

    let mut linked_fields = Vec::new();
    for (ordinal, field) in payload
        .transport
        .component_field_witnesses
        .iter()
        .enumerate()
    {
        let expected_field_id =
            u32::try_from(ordinal).map_err(|_| TransportLinkError::ComponentFieldIdOverflow)?;
        if field.field_id != expected_field_id {
            return Err(TransportLinkError::NonDenseComponentFieldClaims {
                component: definition_id,
                expected: expected_field_id,
                actual: field.field_id,
            });
        }
        let claimed_field_record = validate_component_field_claim_record_id(
            &payload.transport.component_input_fingerprint,
            definition_id,
            field,
        )?;
        linked_fields.push(link_component_field_record(
            unit,
            claimed_field_record,
            definition_id,
            &field.field_name,
            component_fields,
            claimed_fields,
        )?);
    }

    let mut linked_shared_state_acquisitions = Vec::new();
    for (ordinal, acquisition) in payload
        .transport
        .shared_state_acquisitions
        .iter()
        .enumerate()
    {
        let expected_store_id =
            u32::try_from(ordinal).map_err(|_| TransportLinkError::SharedStateStoreIdOverflow)?;
        if acquisition.store_id != expected_store_id {
            return Err(TransportLinkError::NonDenseSharedStateAcquisitionClaims {
                component: definition_id,
                expected: expected_store_id,
                actual: acquisition.store_id,
            });
        }
        let source_edge = validate_shared_state_acquisition_claim_record_id(
            &payload.transport.component_input_fingerprint,
            definition_id,
            acquisition,
        )?;
        linked_shared_state_acquisitions.push(link_shared_state_acquisition_record(
            unit,
            definition_id,
            acquisition.store_id,
            source_edge,
            shared_state_acquisitions,
            claimed_shared_state_acquisitions,
        )?);
    }

    let mut linked_components = Vec::new();
    for edge in &payload.transport.direct_component_edges {
        if edge.identity_authority != "rustc_component_witness_relocation" {
            return Err(TransportLinkError::InvalidIdentityAuthority {
                kind: "component",
                authority: edge.identity_authority.clone(),
            });
        }
        let source_edge = parse_record_id("component source_edge_id", &edge.source_edge_id)?;
        let key = (unit, source_edge);
        if !claimed_components.insert(key) {
            return Err(TransportLinkError::SourceEdgeClaimedTwice {
                kind: "component",
                unit: hex_id(&unit),
                source_edge: hex_id(&source_edge),
            });
        }
        let resolved =
            component_edges
                .get(&key)
                .ok_or_else(|| TransportLinkError::MissingSourceEdge {
                    kind: "component",
                    unit: hex_id(&unit),
                    source_edge: hex_id(&source_edge),
                })?;
        linked_components.push(LinkedComponentEdge {
            source_edge,
            target: TransportDefinitionId {
                unit: CargoUnitId::from_bytes(resolved.definition.cargo_unit_id),
                record: resolved.definition.component_record_id,
            },
        });
    }
    linked_components.sort_by_key(|edge| edge.source_edge);

    let linked_shared_state_provider_sites = validate_and_link_shared_state_provider_sites(
        definition_id,
        unit,
        &payload.transport.component_input_fingerprint,
        &payload.component_descriptor,
        &payload.transport.shared_state_provider_sites,
        &payload.transport.direct_component_edges,
        &linked_components,
        shared_state_acquisitions,
        claimed_shared_state_acquisitions,
    )?;

    let mut linked_helpers = Vec::new();
    for edge in &payload.transport.reachable_helper_edges {
        if edge.identity_authority != "rustc_function_relocation" {
            return Err(TransportLinkError::InvalidIdentityAuthority {
                kind: "helper",
                authority: edge.identity_authority.clone(),
            });
        }
        let source_edge = parse_record_id("component helper source_edge_id", &edge.source_edge_id)?;
        let key = (unit, source_edge);
        if !claimed_helpers.insert(key) {
            return Err(TransportLinkError::SourceEdgeClaimedTwice {
                kind: "helper",
                unit: hex_id(&unit),
                source_edge: hex_id(&source_edge),
            });
        }
        let resolved =
            helper_edges
                .get(&key)
                .ok_or_else(|| TransportLinkError::MissingSourceEdge {
                    kind: "helper",
                    unit: hex_id(&unit),
                    source_edge: hex_id(&source_edge),
                })?;
        linked_helpers.push(LinkedHelperEdge {
            source_edge,
            target: TransportDefinitionId {
                unit: CargoUnitId::from_bytes(resolved.definition.cargo_unit_id),
                record: resolved.definition.helper_record_id,
            },
        });
    }
    linked_helpers.sort_by_key(|edge| edge.source_edge);

    let semantic_hash = parse_semantic_hash(&payload.semantic_hash)?;
    Ok(LinkedTransportComponent {
        definition: definition_id,
        identity: LinkedSemanticIdentity::from(&definition.identity),
        semantic_hash,
        component_abi: definition.definition.component_abi,
        component_edges: linked_components,
        helper_edges: linked_helpers,
        field_witnesses: linked_fields,
        shared_state_acquisitions: linked_shared_state_acquisitions,
        shared_state_provider_sites: linked_shared_state_provider_sites,
        payload: payload_value,
    })
}

fn validate_component_shared_state_claim_shape(
    component: TransportDefinitionId,
    descriptor: &Value,
    claims: &[ComponentSharedStateAcquisitionClaim],
) -> Result<(), TransportLinkError> {
    let stores = descriptor
        .as_object()
        .and_then(|descriptor| descriptor.get("shared_stores"))
        .and_then(Value::as_array)
        .ok_or(TransportLinkError::InvalidSharedStateDescriptorStores {
            component,
            reason: "component_descriptor.shared_stores must be an array".to_owned(),
        })?;
    if stores.len() != claims.len() {
        return Err(
            TransportLinkError::SharedStateAcquisitionClaimCountMismatch {
                component,
                stores: stores.len(),
                claims: claims.len(),
            },
        );
    }
    for (ordinal, store) in stores.iter().enumerate() {
        let actual = store
            .as_object()
            .and_then(|store| store.get("id"))
            .and_then(Value::as_u64)
            .ok_or_else(|| TransportLinkError::InvalidSharedStateDescriptorStores {
                component,
                reason: format!("shared_stores[{ordinal}].id must be an unsigned integer"),
            })?;
        let expected = u64::try_from(ordinal).map_err(|_| {
            TransportLinkError::InvalidSharedStateDescriptorStores {
                component,
                reason: "SharedState descriptor store count exceeds u64".to_owned(),
            }
        })?;
        if actual != expected {
            return Err(TransportLinkError::InvalidSharedStateDescriptorStores {
                component,
                reason: format!(
                    "shared_stores[{ordinal}].id is {actual}, expected dense store id {expected}"
                ),
            });
        }
    }
    Ok(())
}

pub(crate) fn shared_state_acquisition_source_edge_id(
    input_fingerprint: &str,
    store_id: u32,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.shared-state-acquisition.v1\0");
    hasher.update(input_fingerprint.as_bytes());
    hasher.update(store_id.to_le_bytes());
    hasher.finalize().into()
}

fn validate_shared_state_acquisition_claim_record_id(
    input_fingerprint: &str,
    component: TransportDefinitionId,
    acquisition: &ComponentSharedStateAcquisitionClaim,
) -> Result<TransportRecordId, TransportLinkError> {
    let actual = parse_record_id(
        "component SharedState acquisition source_edge_id",
        &acquisition.source_edge_id,
    )?;
    let expected = shared_state_acquisition_source_edge_id(input_fingerprint, acquisition.store_id);
    if actual != expected {
        return Err(TransportLinkError::SharedStateAcquisitionRecordIdMismatch {
            component,
            store_id: acquisition.store_id,
            expected: hex_id(&expected),
            actual: hex_id(&actual),
        });
    }
    Ok(actual)
}

fn link_shared_state_acquisition_record(
    unit: TransportRecordId,
    component: TransportDefinitionId,
    store_id: u32,
    source_edge: TransportRecordId,
    acquisitions: &BTreeMap<SourceEdgeKey, &LinkedSharedStateAcquisition>,
    claimed: &mut BTreeSet<SourceEdgeKey>,
) -> Result<LinkedComponentSharedStateAcquisition, TransportLinkError> {
    let key = (unit, source_edge);
    if !claimed.insert(key) {
        return Err(TransportLinkError::SharedStateAcquisitionClaimedTwice {
            unit: hex_id(&unit),
            source_edge: hex_id(&source_edge),
        });
    }
    let resolved = acquisitions.get(&key).ok_or_else(|| {
        TransportLinkError::MissingSharedStateAcquisition {
            component,
            store_id,
            unit: hex_id(&unit),
            source_edge: hex_id(&source_edge),
        }
    })?;
    Ok(LinkedComponentSharedStateAcquisition {
        store_id,
        source_edge,
        target: resolved.target,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_and_link_shared_state_provider_sites(
    component: TransportDefinitionId,
    unit: TransportRecordId,
    input_fingerprint: &str,
    descriptor: &Value,
    claims: &[ComponentSharedStateProviderSiteClaim],
    direct_edges: &[ComponentSourceEdge],
    linked_component_edges: &[LinkedComponentEdge],
    acquisitions: &BTreeMap<SourceEdgeKey, &LinkedSharedStateAcquisition>,
    claimed_acquisitions: &mut BTreeSet<SourceEdgeKey>,
) -> Result<Vec<LinkedComponentSharedStateProviderSite>, TransportLinkError> {
    let descriptor_sites = descriptor
        .as_object()
        .and_then(|descriptor| descriptor.get("shared_state_provider_sites"))
        .and_then(Value::as_array)
        .ok_or_else(|| TransportLinkError::InvalidSharedStateProviderSites {
            component,
            reason: "component_descriptor.shared_state_provider_sites must be an array".to_owned(),
        })?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            serde_json::from_value::<DescriptorSharedStateProviderSite>(value.clone()).map_err(
                |error| TransportLinkError::InvalidSharedStateProviderSites {
                    component,
                    reason: format!(
                        "component_descriptor.shared_state_provider_sites[{ordinal}] is invalid: \
                         {error}"
                    ),
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    if descriptor_sites.len() != claims.len() {
        return Err(
            TransportLinkError::SharedStateProviderSiteClaimCountMismatch {
                component,
                descriptor_sites: descriptor_sites.len(),
                claims: claims.len(),
            },
        );
    }

    let mut claimed_children = BTreeSet::new();
    let mut linked_sites = Vec::with_capacity(claims.len());
    for (ordinal, (claim, descriptor_site)) in
        claims.iter().zip(descriptor_sites.iter()).enumerate()
    {
        let expected_id = u32::try_from(ordinal)
            .map_err(|_| TransportLinkError::SharedStateProviderSiteIdOverflow)?;
        if claim.provider_site_id != expected_id {
            return Err(TransportLinkError::NonDenseSharedStateProviderSiteClaims {
                component,
                expected: expected_id,
                actual: claim.provider_site_id,
            });
        }
        if descriptor_site.id != expected_id {
            return Err(TransportLinkError::InvalidSharedStateProviderSites {
                component,
                reason: format!(
                    "shared_state_provider_sites[{ordinal}].id is {}, expected dense site id \
                     {expected_id}",
                    descriptor_site.id
                ),
            });
        }
        if claim.identity_authority != "rustc_shared_state_witness_relocation" {
            return Err(TransportLinkError::InvalidIdentityAuthority {
                kind: "SharedState provider",
                authority: claim.identity_authority.clone(),
            });
        }
        if claim.topology != "static_single_wrapperless" {
            return Err(TransportLinkError::InvalidSharedStateProviderSite {
                component,
                provider_site_id: claim.provider_site_id,
                reason: format!("unsupported topology `{}`", claim.topology),
            });
        }
        if !claimed_children.insert(claim.child_edge_local_ordinal) {
            return Err(TransportLinkError::SharedStateProviderChildClaimedTwice {
                component,
                child_edge: claim.child_edge_local_ordinal,
            });
        }

        let child = direct_edges
            .iter()
            .find(|edge| edge.local_ordinal == claim.child_edge_local_ordinal)
            .ok_or_else(|| TransportLinkError::InvalidSharedStateProviderSite {
                component,
                provider_site_id: claim.provider_site_id,
                reason: format!(
                    "child edge {} is absent from direct_component_edges",
                    claim.child_edge_local_ordinal
                ),
            })?;
        if child.boundary != "composed"
            || !matches!(child.lifecycle, ComponentSourceLifecycle::StaticSingle)
            || child.has_rest
            || child.site_node != claim.site_node
            || child.fields.iter().any(|field| {
                !matches!(
                    field.binding.kind.as_str(),
                    "server_only" | "static_callback"
                )
            })
        {
            return Err(TransportLinkError::InvalidSharedStateProviderSite {
                component,
                provider_site_id: claim.provider_site_id,
                reason: "provider child must be one wrapperless static composed edge at the same \
                         server node with only server/static-callback fields"
                    .to_owned(),
            });
        }

        let child_source_edge = parse_record_id(
            "SharedState provider child_source_edge_id",
            &claim.child_source_edge_id,
        )?;
        let direct_child_source_edge =
            parse_record_id("component source_edge_id", &child.source_edge_id)?;
        if child_source_edge != direct_child_source_edge {
            return Err(TransportLinkError::SharedStateProviderChildEdgeMismatch {
                component,
                provider_site_id: claim.provider_site_id,
                expected: hex_id(&direct_child_source_edge),
                actual: hex_id(&child_source_edge),
            });
        }
        let linked_child = linked_component_edges
            .iter()
            .find(|edge| edge.source_edge == child_source_edge)
            .ok_or_else(|| TransportLinkError::InvalidSharedStateProviderSite {
                component,
                provider_site_id: claim.provider_site_id,
                reason: format!(
                    "child source edge {} did not resolve through the component relocation set",
                    hex_id(&child_source_edge)
                ),
            })?;

        if descriptor_site.child_edge != claim.child_edge_local_ordinal
            || descriptor_site.site_node != claim.site_node
            || descriptor_site.state_rust_type != claim.state_rust_type
            || descriptor_site.topology != claim.topology
            || descriptor_site.source_anchor.start_line != claim.source_anchor.line
            || descriptor_site.source_anchor.start_column != claim.source_anchor.column
        {
            return Err(
                TransportLinkError::SharedStateProviderDescriptorClaimMismatch {
                    component,
                    provider_site_id: claim.provider_site_id,
                },
            );
        }

        let expected_source_edge = shared_state_provider_source_edge_id(
            input_fingerprint,
            claim.provider_site_id,
            &TransportSharedStateProviderSite {
                site_node: claim.site_node,
                child_edge_ordinal: claim.child_edge_local_ordinal,
                child_source_edge_id: child_source_edge,
            },
        );
        let source_edge =
            parse_record_id("SharedState provider source_edge_id", &claim.source_edge_id)?;
        if source_edge != expected_source_edge {
            return Err(TransportLinkError::SharedStateProviderRecordIdMismatch {
                component,
                provider_site_id: claim.provider_site_id,
                expected: hex_id(&expected_source_edge),
                actual: hex_id(&source_edge),
            });
        }

        let key = (unit, source_edge);
        if !claimed_acquisitions.insert(key) {
            return Err(TransportLinkError::SharedStateProviderWitnessClaimedTwice {
                unit: hex_id(&unit),
                source_edge: hex_id(&source_edge),
            });
        }
        let resolved = acquisitions.get(&key).ok_or_else(|| {
            TransportLinkError::MissingSharedStateProviderWitness {
                component,
                provider_site_id: claim.provider_site_id,
                unit: hex_id(&unit),
                source_edge: hex_id(&source_edge),
            }
        })?;
        linked_sites.push(LinkedComponentSharedStateProviderSite {
            provider_site_id: claim.provider_site_id,
            source_edge,
            target: resolved.target,
            child_edge_local_ordinal: claim.child_edge_local_ordinal,
            child_source_edge,
            child_target: linked_child.target,
            site_node: claim.site_node,
        });
    }
    Ok(linked_sites)
}

fn validate_component_field_claim_record_id(
    input_fingerprint: &str,
    component: TransportDefinitionId,
    field: &ComponentFieldWitnessClaim,
) -> Result<TransportRecordId, TransportLinkError> {
    let (role, callback_shape) = parse_component_field_role_shape(field)?;
    let expected = component_field_record_id(
        input_fingerprint,
        field.field_id,
        field.crossing_hook_id,
        role,
        callback_shape,
    );
    let actual = parse_record_id("component field field_record_id", &field.field_record_id)?;
    if actual != expected {
        return Err(TransportLinkError::ComponentFieldRecordIdMismatch {
            component,
            field: field.field_name.clone(),
            expected: hex_id(&expected),
            actual: hex_id(&actual),
        });
    }
    Ok(actual)
}

fn link_component_field_record(
    unit: TransportRecordId,
    field_record: TransportRecordId,
    expected_target: TransportDefinitionId,
    field_name: &str,
    component_fields: &BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedComponentField>,
    claimed_fields: &mut BTreeSet<SourceEdgeKey>,
) -> Result<LinkedComponentFieldWitness, TransportLinkError> {
    let key = (unit, field_record);
    if !claimed_fields.insert(key) {
        return Err(TransportLinkError::ComponentFieldClaimedTwice {
            unit: hex_id(&unit),
            field_record: hex_id(&field_record),
        });
    }
    let resolved =
        component_fields
            .get(&key)
            .ok_or_else(|| TransportLinkError::MissingComponentField {
                unit: hex_id(&unit),
                field_record: hex_id(&field_record),
            })?;
    let target = TransportDefinitionId {
        unit: CargoUnitId::from_bytes(resolved.definition.cargo_unit_id),
        record: resolved.definition.component_record_id,
    };
    if target != expected_target {
        return Err(TransportLinkError::ComponentFieldTargetMismatch {
            field: field_name.to_owned(),
            expected: expected_target,
            actual: target,
        });
    }
    Ok(LinkedComponentFieldWitness {
        field_record,
        target,
    })
}

fn parse_component_field_role_shape(
    field: &ComponentFieldWitnessClaim,
) -> Result<(TransportComponentFieldRole, TransportCallbackShape), TransportLinkError> {
    match (field.role.as_str(), field.callback_shape.as_str()) {
        ("value", "not_callback") => Ok((
            TransportComponentFieldRole::Value,
            TransportCallbackShape::NotCallback,
        )),
        ("callback", "required") => Ok((
            TransportComponentFieldRole::Callback,
            TransportCallbackShape::Required,
        )),
        ("callback", "optional") => Ok((
            TransportComponentFieldRole::Callback,
            TransportCallbackShape::Optional,
        )),
        ("node_ref", "not_callback") => Ok((
            TransportComponentFieldRole::NodeRef,
            TransportCallbackShape::NotCallback,
        )),
        ("deferred_projection", "not_callback") => Ok((
            TransportComponentFieldRole::DeferredProjection,
            TransportCallbackShape::NotCallback,
        )),
        _ => Err(TransportLinkError::InvalidComponentFieldRoleShape {
            field: field.field_name.clone(),
            role: field.role.clone(),
            callback_shape: field.callback_shape.clone(),
        }),
    }
}

fn resolve_helper(
    definition: &ResolvedHelperDefinition,
    helper_edges: &BTreeMap<SourceEdgeKey, &rsjs_transport::ResolvedHelperEdge>,
    claimed_helpers: &mut BTreeSet<SourceEdgeKey>,
) -> Result<LinkedTransportHelper, TransportLinkError> {
    let payload_value = parse_payload_json("helper", &definition.payload.json)?;
    validate_embedded_semantic_hash("helper", &payload_value, HELPER_SEMANTIC_DOMAIN)?;
    let payload: HelperPayload =
        serde_json::from_value(payload_value.clone()).map_err(|source| {
            TransportLinkError::InvalidPayloadSchema {
                kind: "helper",
                location: definition.payload.location.to_string(),
                source,
            }
        })?;
    if payload.format != HELPER_PAYLOAD_FORMAT || payload.schema_version != 1 {
        return Err(TransportLinkError::UnsupportedPayload {
            kind: "helper",
            format: payload.format,
            schema_version: payload.schema_version,
        });
    }
    if payload.body_hir_version != 1
        || u32::from(payload.body_ir_version) != CLIENT_IR_SCHEMA_VERSION
        || payload.codec_abi_version != 1
        || payload.effect_class != "origin_free"
    {
        return Err(TransportLinkError::UnsupportedHelperAbi {
            body_hir: payload.body_hir_version,
            body_ir: payload.body_ir_version,
            codec: payload.codec_abi_version,
            effect_class: payload.effect_class,
        });
    }

    let expected_record = definition.definition.helper_record_id;
    let payload_record = parse_record_id("helper defining_record_id", &payload.defining_record_id)?;
    if payload_record != expected_record {
        return Err(TransportLinkError::PayloadDefinitionMismatch {
            kind: "helper",
            expected: hex_id(&expected_record),
            actual: hex_id(&payload_record),
        });
    }
    let unit = definition.definition.cargo_unit_id;
    let mut linked_helpers = Vec::new();
    for edge in payload.direct_calls {
        let source_edge = parse_record_id("helper source_edge_id", &edge.source_edge_id)?;
        let key = (unit, source_edge);
        if !claimed_helpers.insert(key) {
            return Err(TransportLinkError::SourceEdgeClaimedTwice {
                kind: "helper",
                unit: hex_id(&unit),
                source_edge: hex_id(&source_edge),
            });
        }
        let resolved =
            helper_edges
                .get(&key)
                .ok_or_else(|| TransportLinkError::MissingSourceEdge {
                    kind: "helper",
                    unit: hex_id(&unit),
                    source_edge: hex_id(&source_edge),
                })?;
        linked_helpers.push(LinkedHelperEdge {
            source_edge,
            target: TransportDefinitionId {
                unit: CargoUnitId::from_bytes(resolved.definition.cargo_unit_id),
                record: resolved.definition.helper_record_id,
            },
        });
    }
    linked_helpers.sort_by_key(|edge| edge.source_edge);

    Ok(LinkedTransportHelper {
        definition: TransportDefinitionId {
            unit: CargoUnitId::from_bytes(unit),
            record: expected_record,
        },
        identity: LinkedSemanticIdentity::from(&definition.identity),
        semantic_hash: parse_semantic_hash(&payload.semantic_hash)?,
        helper_edges: linked_helpers,
        payload: payload_value,
    })
}

fn parse_payload_json(kind: &'static str, bytes: &[u8]) -> Result<Value, TransportLinkError> {
    serde_json::from_slice(bytes)
        .map_err(|source| TransportLinkError::ParsePayload { kind, source })
}

fn validate_component_descriptor(descriptor: &Value) -> Result<(), TransportLinkError> {
    let format = descriptor
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let schema_version = descriptor
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if format != COMPONENT_DESCRIPTOR_FORMAT
        || schema_version != COMPONENT_DESCRIPTOR_SCHEMA_VERSION
    {
        return Err(TransportLinkError::UnsupportedComponentDescriptor {
            format: format.to_string(),
            schema_version,
        });
    }
    let descriptor_hash = descriptor
        .get("descriptor_hash")
        .and_then(Value::as_str)
        .ok_or(TransportLinkError::MissingComponentDescriptorHash)?;
    parse_semantic_hash(descriptor_hash)?;
    Ok(())
}

fn validate_embedded_semantic_hash(
    kind: &'static str,
    value: &Value,
    domain: &[u8],
) -> Result<(), TransportLinkError> {
    let recorded = value
        .get("semantic_hash")
        .and_then(Value::as_str)
        .ok_or(TransportLinkError::MissingSemanticHash(kind))?;
    let recorded = parse_semantic_hash(recorded)?;
    let mut semantic_value = value.clone();
    semantic_value
        .as_object_mut()
        .ok_or(TransportLinkError::PayloadMustBeObject(kind))?
        .remove("semantic_hash");
    let bytes = canonical_json_bytes(&semantic_value)
        .map_err(|source| TransportLinkError::SerializePayload { kind, source })?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    let expected = SemanticHash::from_bytes(hasher.finalize().into());
    if expected != recorded {
        return Err(TransportLinkError::EmbeddedSemanticHashMismatch {
            kind,
            expected,
            actual: recorded,
        });
    }
    Ok(())
}

fn parse_semantic_hash(value: &str) -> Result<SemanticHash, TransportLinkError> {
    let hex = value
        .strip_prefix("sha256:")
        .ok_or_else(|| TransportLinkError::InvalidHash(value.to_string()))?;
    parse_hex_32(hex)
        .map(SemanticHash::from_bytes)
        .map_err(|_| TransportLinkError::InvalidHash(value.to_string()))
}

fn parse_record_id(
    field: &'static str,
    value: &str,
) -> Result<TransportRecordId, TransportLinkError> {
    parse_hex_32(value).map_err(|_| TransportLinkError::InvalidRecordId {
        field,
        value: value.to_string(),
    })
}

fn parse_hex_32(value: &str) -> Result<[u8; 32], ()> {
    if value.len() != 64
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(());
    }
    let mut out = [0_u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let high = hex_nibble(value.as_bytes()[index * 2])?;
        let low = hex_nibble(value.as_bytes()[index * 2 + 1])?;
        *byte = (high << 4) | low;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}

fn hex_id(id: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in id {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn set_difference_hex(
    all: &BTreeSet<SourceEdgeKey>,
    claimed: &BTreeSet<SourceEdgeKey>,
) -> Vec<String> {
    all.difference(claimed)
        .map(|(unit, edge)| format!("{}:{}", hex_id(unit), hex_id(edge)))
        .collect()
}

fn source_keys_hex(keys: &[SourceEdgeKey]) -> Vec<String> {
    keys.iter()
        .map(|(unit, record)| format!("{}:{}", hex_id(unit), hex_id(record)))
        .collect()
}

/// Atomically persist a resolved transport plan under its content `PlanId`.
///
/// Existing identical content is reused. Existing different content at the
/// same strong ID is treated as a fatal collision.
pub fn write_content_addressed_transport_plan(
    directory: &Path,
    plan: &TransportLinkPlan,
) -> Result<PathBuf, TransportPlanWriteError> {
    if !directory.is_absolute() {
        return Err(TransportPlanWriteError::RelativeDirectory(
            directory.to_path_buf(),
        ));
    }
    fs::create_dir_all(directory).map_err(|source| TransportPlanWriteError::CreateDirectory {
        path: directory.to_path_buf(),
        source,
    })?;

    let value = transport_plan_json(plan);
    let bytes = serde_json::to_vec_pretty(&value).map_err(TransportPlanWriteError::Serialize)?;
    let path = directory.join(format!("{}.transport-plan.json", plan.plan_id));
    if path.exists() {
        let existing = fs::read(&path).map_err(|source| TransportPlanWriteError::ReadExisting {
            path: path.clone(),
            source,
        })?;
        if existing == bytes {
            return Ok(path);
        }
        return Err(TransportPlanWriteError::ContentCollision {
            plan_id: plan.plan_id,
            path,
        });
    }

    let nonce = PLAN_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        ".{}.{}.{}.tmp",
        plan.plan_id,
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| TransportPlanWriteError::CreateTemporary {
            path: temporary.clone(),
            source,
        })?;
    if let Err(source) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(TransportPlanWriteError::WriteTemporary {
            path: temporary,
            source,
        });
    }
    drop(file);
    match fs::rename(&temporary, &path) {
        Ok(()) => Ok(path),
        Err(_source) if path.exists() => {
            let _ = fs::remove_file(&temporary);
            let existing =
                fs::read(&path).map_err(|read_source| TransportPlanWriteError::ReadExisting {
                    path: path.clone(),
                    source: read_source,
                })?;
            if existing == bytes {
                Ok(path)
            } else {
                Err(TransportPlanWriteError::ContentCollision {
                    plan_id: plan.plan_id,
                    path,
                })
            }
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            Err(TransportPlanWriteError::Commit {
                from: temporary,
                to: path,
                source,
            })
        }
    }
}

pub(crate) fn transport_plan_json(plan: &TransportLinkPlan) -> Value {
    let components = plan
        .components
        .iter()
        .map(|component| {
            serde_json::json!({
                "definition": definition_json(component.definition),
                "semantic_identity": semantic_identity_json(&component.identity),
                "semantic_hash": format!("sha256:{}", component.semantic_hash),
                "component_abi": component_definition_abi_json(component.component_abi),
                "component_edges": component.component_edges.iter().map(|edge| {
                    serde_json::json!({
                        "source_edge_id": hex_id(&edge.source_edge),
                        "target": definition_json(edge.target),
                    })
                }).collect::<Vec<_>>(),
                "helper_edges": component.helper_edges.iter().map(|edge| {
                    serde_json::json!({
                        "source_edge_id": hex_id(&edge.source_edge),
                        "target": definition_json(edge.target),
                    })
                }).collect::<Vec<_>>(),
                "field_witnesses": component.field_witnesses.iter().map(|field| {
                    serde_json::json!({
                        "field_record_id": hex_id(&field.field_record),
                        "target": definition_json(field.target),
                    })
                }).collect::<Vec<_>>(),
                "shared_state_acquisitions": component.shared_state_acquisitions.iter().map(|acquisition| {
                    serde_json::json!({
                        "store_id": acquisition.store_id,
                        "source_edge_id": hex_id(&acquisition.source_edge),
                        "target": definition_json(acquisition.target),
                    })
                }).collect::<Vec<_>>(),
                "shared_state_provider_sites": component.shared_state_provider_sites.iter().map(|site| {
                    serde_json::json!({
                        "provider_site_id": site.provider_site_id,
                        "source_edge_id": hex_id(&site.source_edge),
                        "target": definition_json(site.target),
                        "child_edge_local_ordinal": site.child_edge_local_ordinal,
                        "child_source_edge_id": hex_id(&site.child_source_edge),
                        "child_target": definition_json(site.child_target),
                        "site_node": site.site_node,
                    })
                }).collect::<Vec<_>>(),
                "payload": component.payload,
            })
        })
        .collect::<Vec<_>>();
    let helpers = plan
        .helpers
        .iter()
        .map(|helper| {
            serde_json::json!({
                "definition": definition_json(helper.definition),
                "semantic_identity": semantic_identity_json(&helper.identity),
                "semantic_hash": format!("sha256:{}", helper.semantic_hash),
                "helper_edges": helper.helper_edges.iter().map(|edge| {
                    serde_json::json!({
                        "source_edge_id": hex_id(&edge.source_edge),
                        "target": definition_json(edge.target),
                    })
                }).collect::<Vec<_>>(),
                "payload": helper.payload,
            })
        })
        .collect::<Vec<_>>();
    let shared_state_definitions = plan
        .shared_state_definitions
        .iter()
        .map(|definition| {
            serde_json::json!({
                "definition": definition_json(definition.definition),
                "schema_payload_hash": format!("sha256:{}", hex_id(&definition.schema_payload_hash)),
                "type_key": definition.type_key,
                "schema_abi_hash": format!("sha256:{}", hex_id(&definition.schema_abi_hash)),
                "schema_abi_hex": hex_bytes(&definition.schema_abi),
            })
        })
        .collect::<Vec<_>>();
    let shared_state_acquisitions = plan
        .shared_state_acquisitions
        .iter()
        .map(|acquisition| {
            serde_json::json!({
                "source": {
                    "cargo_unit_id": acquisition.source_unit.to_string(),
                    "source_edge_id": hex_id(&acquisition.source_edge),
                },
                "target": definition_json(acquisition.target),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "format": "proa.rsjs.resolved_transport_plan",
        "schema_version": TRANSPORT_LINK_SCHEMA_VERSION,
        "link_unit_id": plan.link_unit.to_string(),
        "plan_id": plan.plan_id.to_string(),
        "payload_hash": format!("sha256:{}", plan.material.payload),
        "component_descriptor_hashes": plan.material.component_descriptors.iter()
            .map(|hash| format!("sha256:{hash}"))
            .collect::<Vec<_>>(),
        "helper_descriptor_hashes": plan.material.helper_descriptors.iter()
            .map(|hash| format!("sha256:{hash}"))
            .collect::<Vec<_>>(),
        "components": components,
        "helpers": helpers,
        "shared_state_definitions": shared_state_definitions,
        "shared_state_acquisitions": shared_state_acquisitions,
    })
}

fn definition_json(definition: TransportDefinitionId) -> Value {
    serde_json::json!({
        "cargo_unit_id": definition.unit.to_string(),
        "record_id": hex_id(&definition.record),
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn semantic_identity_json(identity: &LinkedSemanticIdentity) -> Value {
    serde_json::json!({
        "package": identity.package,
        "crate": identity.crate_name,
        "module_path": identity.module_path,
        "item": identity.item,
    })
}

fn component_definition_abi_json(component_abi: ComponentDefinitionAbi) -> Value {
    let render_kind = match component_abi.render_kind {
        TransportComponentRenderKind::Sync => "sync",
        TransportComponentRenderKind::Async => "async",
    };
    let origin_permission = match component_abi.origin_permission {
        TransportComponentOriginPermission::ParticipantOnly => "participant_only",
        TransportComponentOriginPermission::OriginCapable => "origin_capable",
    };
    serde_json::json!({
        "flags": component_abi.flags(),
        "render_kind": render_kind,
        "origin_permission": origin_permission,
    })
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentPayload {
    component_descriptor: Value,
    format: String,
    schema_version: u16,
    semantic_hash: String,
    transport: ComponentTransport,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentTransport {
    client_value_sources: Value,
    component_input_fingerprint: String,
    component_field_witnesses: Vec<ComponentFieldWitnessClaim>,
    definition_record_id: String,
    direct_component_edges: Vec<ComponentSourceEdge>,
    hydration_constraint: Value,
    reachable_helper_edges: Vec<ComponentHelperSourceEdge>,
    shared_state_acquisitions: Vec<ComponentSharedStateAcquisitionClaim>,
    shared_state_provider_sites: Vec<ComponentSharedStateProviderSiteClaim>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentFieldWitnessClaim {
    callback_shape: String,
    crossing_hook_id: u32,
    field_id: u32,
    field_name: String,
    field_record_id: String,
    role: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentSourceEdge {
    authored_path: String,
    boundary: String,
    fields: Vec<ComponentSourceField>,
    has_rest: bool,
    identity_authority: String,
    lifecycle: ComponentSourceLifecycle,
    local_ordinal: u32,
    site_node: u32,
    source_anchor: SourceAnchor,
    source_edge_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ComponentSourceLifecycle {
    StaticSingle,
    ConditionalSingle {
        control: u32,
        predicate_expression: u32,
        branch: ComponentSourceConditionalBranch,
    },
    Repeated,
    Unknown {
        reason: ComponentSourceUnknownLifecycleReason,
    },
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ComponentSourceConditionalBranch {
    Then,
    Else,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ComponentSourceUnknownLifecycleReason {
    NestedConditionals,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentSourceField {
    binding: ComponentSourceFieldBinding,
    field_ordinal: u32,
    name: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentSourceFieldBinding {
    callback_shape: Option<String>,
    handle_kind: Option<String>,
    kind: String,
    latent_node_ref_id: Option<u32>,
    latent_signal_id: Option<u32>,
    parent_field_id: Option<u32>,
    parent_source: Option<Value>,
    proof: String,
    source_form: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAnchor {
    column: usize,
    line: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentHelperSourceEdge {
    authored_path: String,
    identity_authority: String,
    local_ordinal: u32,
    source_edge_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentSharedStateAcquisitionClaim {
    source_edge_id: String,
    store_id: u32,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentSharedStateProviderSiteClaim {
    child_edge_local_ordinal: u32,
    child_source_edge_id: String,
    identity_authority: String,
    provider_site_id: u32,
    site_node: u32,
    source_anchor: SourceAnchor,
    source_edge_id: String,
    state_rust_type: String,
    topology: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorSharedStateProviderSite {
    child_edge: u32,
    id: u32,
    site_node: u32,
    source_anchor: DescriptorSourceAnchor,
    state_rust_type: String,
    topology: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorSourceAnchor {
    start_column: usize,
    start_line: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperPayload {
    body_hir: Value,
    body_hir_version: u16,
    body_ir: Value,
    body_ir_version: u16,
    codec_abi_version: u16,
    defining_record_id: String,
    direct_calls: Vec<HelperSourceEdge>,
    effect_class: String,
    format: String,
    provisional_identity: Value,
    schema_version: u16,
    semantic_hash: String,
    signature: Value,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperSourceEdge {
    authored_path: String,
    ordinal: usize,
    source_edge_id: String,
}

/// Exact-artifact extraction or transport-link validation failure.
#[derive(Debug, Error)]
pub enum TransportLinkError {
    /// Root manifest failed canonical validation.
    #[error("invalid root link manifest: {0}")]
    InvalidManifest(BuildIdentityError),
    /// No exact descriptor objects were supplied.
    #[error("root link manifest contains no descriptor objects")]
    NoDescriptorObjects,
    /// Descriptor object path must be exact and absolute.
    #[error("descriptor object path must be absolute: `{0}`")]
    RelativeDescriptorObject(PathBuf),
    /// One exact path appeared more than once.
    #[error("descriptor object path appears more than once: `{0}`")]
    DuplicateDescriptorPath(PathBuf),
    /// Every reachable unit must have exactly one descriptor object.
    #[error("Cargo unit {0} has more than one descriptor object")]
    DuplicateDescriptorUnit(CargoUnitId),
    /// Validated descriptor evidence must be unique by Cargo unit.
    #[error("validated descriptor evidence contains Cargo unit {0} more than once")]
    DuplicateValidatedDescriptorUnit(CargoUnitId),
    /// Every manifest descriptor must retain its authoritative wrapper unit.
    #[error("Cargo unit {0} has no validated descriptor evidence")]
    MissingValidatedDescriptorUnit(CargoUnitId),
    /// Manifest and wrapper evidence must name the same exact object path.
    #[error(
        "Cargo unit {unit} uses descriptor object `{manifest}` in the manifest but `{validated}` in validated wrapper evidence"
    )]
    ValidatedDescriptorPathMismatch {
        /// Exact Cargo unit.
        unit: CargoUnitId,
        /// Manifest path.
        manifest: PathBuf,
        /// Validated wrapper/cache path.
        validated: PathBuf,
    },
    /// Wrapper-validated descriptor hashes are exact lowercase SHA-256 values.
    #[error("Cargo unit {unit} has invalid validated descriptor hash `{value}`")]
    InvalidValidatedDescriptorHash {
        /// Exact Cargo unit.
        unit: CargoUnitId,
        /// Rejected hash.
        value: String,
    },
    /// Descriptor objects must cover the exact manifest unit set.
    #[error("descriptor object unit set differs from the root manifest")]
    DescriptorUnitSetMismatch {
        /// All units represented by the manifest.
        expected: BTreeSet<CargoUnitId>,
        /// Units represented by descriptor objects.
        actual: BTreeSet<CargoUnitId>,
    },
    /// Exact artifact could not be hashed.
    #[error("failed to hash descriptor object `{path}`: {source}")]
    HashDescriptorObject {
        /// Exact path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Exact bytes changed after manifest construction.
    #[error(
        "descriptor object `{path}` changed after manifest construction: expected {expected}, actual {actual}"
    )]
    DescriptorObjectHashMismatch {
        /// Exact path.
        path: PathBuf,
        /// Manifest hash.
        expected: SemanticHash,
        /// Fresh hash.
        actual: SemanticHash,
    },
    /// Exact extraction bytes must still match the wrapper-authoritative
    /// descriptor-domain hash, not only the freshly assembled link manifest.
    #[error(
        "descriptor object `{path}` no longer matches validated wrapper evidence: expected {expected:?}, actual {actual:?}"
    )]
    DescriptorObjectIdentityHashMismatch {
        /// Exact path.
        path: PathBuf,
        /// Wrapper/cache descriptor-domain hash.
        expected: [u8; 32],
        /// Hash of the exact bytes passed to the extractor.
        actual: [u8; 32],
    },
    /// Object/rlib transport extraction failed.
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    /// Extractor returned a different number of exact inputs.
    #[error("extractor returned {actual} artifacts for {expected} descriptor objects")]
    ExtractionArtifactCount {
        /// Manifest count.
        expected: usize,
        /// Extracted count.
        actual: usize,
    },
    /// Extractor did not preserve the exact caller order.
    #[error("extractor returned `{actual}` where `{expected}` was requested")]
    ExtractionArtifactOrder {
        /// Exact requested path.
        expected: PathBuf,
        /// Actual extracted path.
        actual: PathBuf,
    },
    /// Placeholder unit IDs are forbidden after descriptor orchestration.
    #[error("unassigned Cargo unit on {kind} record at {location}")]
    UnassignedCargoUnit {
        /// Record kind.
        kind: &'static str,
        /// Exact object location.
        location: String,
    },
    /// A record's embedded unit disagreed with the manifest-listed object.
    #[error(
        "{kind} record at {location} embeds Cargo unit {actual}, expected manifest unit {expected}"
    )]
    CargoUnitMismatch {
        /// Record kind.
        kind: &'static str,
        /// Exact object location.
        location: String,
        /// Manifest unit.
        expected: CargoUnitId,
        /// Embedded lower-case hexadecimal unit.
        actual: String,
    },
    /// Duplicate source-edge ID in one unit.
    #[error("duplicate {kind} source edge {unit}:{source_edge}")]
    DuplicateSourceEdge {
        /// Edge kind.
        kind: &'static str,
        /// Unit ID.
        unit: String,
        /// Source edge ID.
        source_edge: String,
    },
    /// Duplicate component-field record in one unit.
    #[error("duplicate component field record {unit}:{field_record}")]
    DuplicateComponentField {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Field record ID.
        field_record: String,
    },
    /// Two extracted SharedState acquisition records reused one source edge.
    #[error("duplicate SharedState acquisition record {unit}:{source_edge}")]
    DuplicateSharedStateAcquisition {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Component-local source-edge ID.
        source_edge: String,
    },
    /// Embedded descriptor bytes are not JSON.
    #[error("invalid embedded {kind} descriptor JSON: {source}")]
    ParsePayload {
        /// Payload kind.
        kind: &'static str,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Payload wrapper schema does not match its fixed contract.
    #[error("invalid embedded {kind} payload schema at {location}: {source}")]
    InvalidPayloadSchema {
        /// Payload kind.
        kind: &'static str,
        /// Exact object location.
        location: String,
        /// JSON schema error.
        #[source]
        source: serde_json::Error,
    },
    /// Payload format/version unsupported by this linker.
    #[error("unsupported {kind} payload `{format}` schema version {schema_version}")]
    UnsupportedPayload {
        /// Payload kind.
        kind: &'static str,
        /// Format tag.
        format: String,
        /// Schema version.
        schema_version: u16,
    },
    /// Current component descriptor schema is not the expected v9 bridge.
    #[error(
        "unsupported component descriptor `{format}` schema version {schema_version}; expected rsjs.component_descriptor v9"
    )]
    UnsupportedComponentDescriptor {
        /// Descriptor format.
        format: String,
        /// Descriptor schema.
        schema_version: u64,
    },
    /// Component bridge descriptor omitted its stored content hash.
    #[error("component descriptor omitted descriptor_hash")]
    MissingComponentDescriptorHash,
    /// Standalone helper ABI is unsupported.
    #[error(
        "unsupported helper ABI: HIR v{body_hir}, Client IR v{body_ir}, codec v{codec}, effect `{effect_class}`"
    )]
    UnsupportedHelperAbi {
        /// Helper HIR version.
        body_hir: u16,
        /// Client IR version.
        body_ir: u16,
        /// Codec ABI version.
        codec: u16,
        /// Effect class.
        effect_class: String,
    },
    /// Required semantic hash is absent.
    #[error("{0} payload omitted semantic_hash")]
    MissingSemanticHash(&'static str),
    /// Semantic payload must be a JSON object.
    #[error("{0} payload must be a JSON object")]
    PayloadMustBeObject(&'static str),
    /// Hash has the wrong prefix, length, or alphabet.
    #[error("invalid lowercase SHA-256 value `{0}`")]
    InvalidHash(String),
    /// Record ID has the wrong length or alphabet.
    #[error("invalid {field} `{value}`; expected 64 lowercase hexadecimal digits")]
    InvalidRecordId {
        /// Field name.
        field: &'static str,
        /// Rejected value.
        value: String,
    },
    /// Re-encoding a parsed payload failed.
    #[error("failed to canonicalize embedded {kind} payload: {source}")]
    SerializePayload {
        /// Payload kind.
        kind: &'static str,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Stored producer semantic hash did not match the payload.
    #[error("embedded {kind} semantic hash mismatch: expected {expected}, actual {actual}")]
    EmbeddedSemanticHashMismatch {
        /// Payload kind.
        kind: &'static str,
        /// Recomputed hash.
        expected: SemanticHash,
        /// Stored hash.
        actual: SemanticHash,
    },
    /// Payload and defining record disagree.
    #[error("{kind} payload belongs to record {actual}, expected {expected}")]
    PayloadDefinitionMismatch {
        /// Definition kind.
        kind: &'static str,
        /// Extracted definition record.
        expected: String,
        /// Payload metadata.
        actual: String,
    },
    /// Producer claimed token identity rather than Rust relocation identity.
    #[error("invalid {kind} identity authority `{authority}`")]
    InvalidIdentityAuthority {
        /// Edge kind.
        kind: &'static str,
        /// Producer authority tag.
        authority: String,
    },
    /// Payload listed no matching relocation edge.
    #[error("missing relocation-backed {kind} source edge {unit}:{source_edge}")]
    MissingSourceEdge {
        /// Edge kind.
        kind: &'static str,
        /// Source unit.
        unit: String,
        /// Source edge ID.
        source_edge: String,
    },
    /// Two payloads claimed one source edge.
    #[error("{kind} source edge {unit}:{source_edge} was claimed twice")]
    SourceEdgeClaimedTwice {
        /// Edge kind.
        kind: &'static str,
        /// Source unit.
        unit: String,
        /// Source edge ID.
        source_edge: String,
    },
    /// Component field claim count exceeded the canonical u32 ID space.
    #[error("component field claim count exceeds the canonical u32 ID space")]
    ComponentFieldIdOverflow,
    /// Component field claims were not in dense field-ID order.
    #[error(
        "component {component:?} field claims are not dense: expected field {expected}, found {actual}"
    )]
    NonDenseComponentFieldClaims {
        /// Defining component.
        component: TransportDefinitionId,
        /// Expected dense field ID.
        expected: u32,
        /// Claimed field ID.
        actual: u32,
    },
    /// A field role and callback shape formed an impossible contract.
    #[error(
        "component field `{field}` has invalid role/callback shape `{role}`/`{callback_shape}`"
    )]
    InvalidComponentFieldRoleShape {
        /// Source field name.
        field: String,
        /// Claimed role.
        role: String,
        /// Claimed callback shape.
        callback_shape: String,
    },
    /// The payload's field record ID was not the deterministic ID for its
    /// fingerprint and behavior claim.
    #[error("component {component:?} field `{field}` has record ID {actual}, expected {expected}")]
    ComponentFieldRecordIdMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Source field name.
        field: String,
        /// Independently recomputed record ID.
        expected: String,
        /// Payload claim.
        actual: String,
    },
    /// A payload listed no matching exact object record.
    #[error("missing rustc-backed component field record {unit}:{field_record}")]
    MissingComponentField {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Field record ID.
        field_record: String,
    },
    /// Two payload claims selected one component-field object record.
    #[error("component field record {unit}:{field_record} was claimed twice")]
    ComponentFieldClaimedTwice {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Field record ID.
        field_record: String,
    },
    /// A field record relocated to a component other than the payload that
    /// claimed it.
    #[error(
        "component field `{field}` relocated to {actual:?}, expected its defining component {expected:?}"
    )]
    ComponentFieldTargetMismatch {
        /// Source field name.
        field: String,
        /// Payload's defining component.
        expected: TransportDefinitionId,
        /// Relocation-selected component.
        actual: TransportDefinitionId,
    },
    /// SharedState acquisition claim count exceeded the canonical u32 ID
    /// space.
    #[error("SharedState acquisition claim count exceeds the canonical u32 store-ID space")]
    SharedStateStoreIdOverflow,
    /// Component SharedState acquisition claims were not in dense store-ID
    /// order.
    #[error(
        "component {component:?} SharedState acquisition claims are not dense: expected store {expected}, found {actual}"
    )]
    NonDenseSharedStateAcquisitionClaims {
        /// Defining component.
        component: TransportDefinitionId,
        /// Expected dense store ID.
        expected: u32,
        /// Claimed store ID.
        actual: u32,
    },
    /// Descriptor SharedState stores and payload ownership claims were not
    /// one-to-one.
    #[error(
        "component {component:?} has {stores} descriptor SharedState stores but {claims} acquisition ownership claims"
    )]
    SharedStateAcquisitionClaimCountMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Number of descriptor stores.
        stores: usize,
        /// Number of payload ownership claims.
        claims: usize,
    },
    /// The embedded descriptor's SharedState store table was malformed.
    #[error("component {component:?} has invalid SharedState store metadata: {reason}")]
    InvalidSharedStateDescriptorStores {
        /// Defining component.
        component: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// A payload claim did not use the deterministic source-edge ID for its
    /// component fingerprint and store ID.
    #[error(
        "component {component:?} SharedState store {store_id} has acquisition record ID {actual}, expected {expected}"
    )]
    SharedStateAcquisitionRecordIdMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Component-local SharedStoreId.
        store_id: u32,
        /// Independently recomputed source-edge ID.
        expected: String,
        /// Payload claim.
        actual: String,
    },
    /// A component payload listed no matching acquisition object record.
    #[error(
        "component {component:?} store {store_id} is missing relocation-backed SharedState acquisition {unit}:{source_edge}"
    )]
    MissingSharedStateAcquisition {
        /// Defining component.
        component: TransportDefinitionId,
        /// Component-local SharedStoreId.
        store_id: u32,
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Claimed source-edge ID.
        source_edge: String,
    },
    /// Two component payload claims selected one acquisition object record.
    #[error("SharedState acquisition record {unit}:{source_edge} was claimed twice")]
    SharedStateAcquisitionClaimedTwice {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Reused source-edge ID.
        source_edge: String,
    },
    /// SharedState provider claim count exceeded the canonical u32 ID space.
    #[error("SharedState provider-site claim count exceeds the canonical u32 site-ID space")]
    SharedStateProviderSiteIdOverflow,
    /// Component provider-site claims were not in dense site-ID order.
    #[error(
        "component {component:?} SharedState provider claims are not dense: expected site {expected}, found {actual}"
    )]
    NonDenseSharedStateProviderSiteClaims {
        /// Defining component.
        component: TransportDefinitionId,
        /// Expected dense provider site ID.
        expected: u32,
        /// Claimed provider site ID.
        actual: u32,
    },
    /// Debug descriptor provider sites and transport claims were not one-to-one.
    #[error(
        "component {component:?} has {descriptor_sites} descriptor SharedState provider sites but {claims} transport claims"
    )]
    SharedStateProviderSiteClaimCountMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Number of descriptor provider sites.
        descriptor_sites: usize,
        /// Number of transport claims.
        claims: usize,
    },
    /// The embedded descriptor's provider topology was malformed.
    #[error("component {component:?} has invalid SharedState provider metadata: {reason}")]
    InvalidSharedStateProviderSites {
        /// Defining component.
        component: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// One provider claim did not describe the supported static topology.
    #[error(
        "component {component:?} SharedState provider site {provider_site_id} is invalid: {reason}"
    )]
    InvalidSharedStateProviderSite {
        /// Defining component.
        component: TransportDefinitionId,
        /// Dense provider site ID.
        provider_site_id: u32,
        /// Focused validation failure.
        reason: String,
    },
    /// More than one provider attempted to own the same direct child edge.
    #[error(
        "component {component:?} direct child edge {child_edge} is claimed by more than one SharedState provider"
    )]
    SharedStateProviderChildClaimedTwice {
        /// Defining component.
        component: TransportDefinitionId,
        /// Reused child edge ordinal.
        child_edge: u32,
    },
    /// Provider transport and debug-descriptor metadata disagreed.
    #[error(
        "component {component:?} SharedState provider site {provider_site_id} disagrees with its descriptor metadata"
    )]
    SharedStateProviderDescriptorClaimMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Dense provider site ID.
        provider_site_id: u32,
    },
    /// Provider claim selected a different direct component source edge.
    #[error(
        "component {component:?} SharedState provider site {provider_site_id} claims child edge {actual}, expected {expected}"
    )]
    SharedStateProviderChildEdgeMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Dense provider site ID.
        provider_site_id: u32,
        /// Direct component claim.
        expected: String,
        /// Provider claim.
        actual: String,
    },
    /// Provider claim did not use its deterministic topology-bound source ID.
    #[error(
        "component {component:?} SharedState provider site {provider_site_id} has witness record ID {actual}, expected {expected}"
    )]
    SharedStateProviderRecordIdMismatch {
        /// Defining component.
        component: TransportDefinitionId,
        /// Dense provider site ID.
        provider_site_id: u32,
        /// Independently recomputed source-edge ID.
        expected: String,
        /// Payload claim.
        actual: String,
    },
    /// Provider claim listed no exact SharedState witness relocation.
    #[error(
        "component {component:?} SharedState provider site {provider_site_id} is missing relocation-backed witness {unit}:{source_edge}"
    )]
    MissingSharedStateProviderWitness {
        /// Defining component.
        component: TransportDefinitionId,
        /// Dense provider site ID.
        provider_site_id: u32,
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Claimed provider source-edge ID.
        source_edge: String,
    },
    /// More than one payload claim selected the provider's witness record.
    #[error("SharedState provider witness record {unit}:{source_edge} was claimed twice")]
    SharedStateProviderWitnessClaimedTwice {
        /// Descriptor-pass Cargo unit.
        unit: String,
        /// Reused source-edge ID.
        source_edge: String,
    },
    /// Extracted component edges absent from every defining payload.
    #[error("unclaimed component relocation edges: {0:?}")]
    UnclaimedComponentEdges(Vec<String>),
    /// Extracted component-field records absent from every defining payload.
    #[error("unclaimed component field records: {0:?}")]
    UnclaimedComponentFields(Vec<String>),
    /// Extracted helper edges absent from every component/helper payload.
    #[error("unclaimed helper relocation edges: {0:?}")]
    UnclaimedHelperEdges(Vec<String>),
    /// Extracted SharedState acquisitions absent from every defining component
    /// payload.
    #[error("unclaimed SharedState acquisition records: {0:?}")]
    UnclaimedSharedStateAcquisitions(Vec<String>),
    /// Exact SharedState records returned by extraction were not preserved
    /// one-for-one in the corresponding resolved join.
    #[error(
        "SharedState {kind} resolution did not preserve the exact extracted record set: extracted {extracted:?}, resolved {resolved:?}"
    )]
    SharedStateResolutionMismatch {
        /// SharedState record kind.
        kind: &'static str,
        /// Exact extracted `(CargoUnitId, record/source-edge)` keys.
        extracted: Vec<String>,
        /// Exact relocation/payload-resolved keys.
        resolved: Vec<String>,
    },
    /// A resolved acquisition selected a definition absent from the exact
    /// definition/payload join.
    #[error(
        "SharedState acquisition {source_unit}:{source_edge} targets missing definition {target:?}"
    )]
    MissingSharedStateAcquisitionTarget {
        /// Cargo unit containing the acquisition.
        source_unit: CargoUnitId,
        /// Lower-case source-edge record ID.
        source_edge: String,
        /// Relocation-selected definition.
        target: TransportDefinitionId,
    },
}

/// Failure while publishing a content-addressed transport plan.
#[derive(Debug, Error)]
pub enum TransportPlanWriteError {
    /// Output directory must be resolved before writing.
    #[error("transport plan directory must be absolute: `{0}`")]
    RelativeDirectory(PathBuf),
    /// Directory creation failed.
    #[error("failed to create transport plan directory `{path}`: {source}")]
    CreateDirectory {
        /// Output directory.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Plan JSON serialization failed.
    #[error("failed to serialize transport plan: {0}")]
    Serialize(serde_json::Error),
    /// Existing plan could not be read.
    #[error("failed to read existing transport plan `{path}`: {source}")]
    ReadExisting {
        /// Existing path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// A strong ID unexpectedly named different bytes.
    #[error("content collision for PlanId {plan_id} at `{path}`")]
    ContentCollision {
        /// Strong plan identity.
        plan_id: PlanId,
        /// Existing path.
        path: PathBuf,
    },
    /// Atomic temporary creation failed.
    #[error("failed to create temporary transport plan `{path}`: {source}")]
    CreateTemporary {
        /// Temporary path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Atomic temporary write failed.
    #[error("failed to write temporary transport plan `{path}`: {source}")]
    WriteTemporary {
        /// Temporary path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Atomic rename failed.
    #[error("failed to publish transport plan from `{from}` to `{to}`: {source}")]
    Commit {
        /// Temporary path.
        from: PathBuf,
        /// Final path.
        to: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests;
