// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic semantic linking for canonical RSJS descriptors.
//!
//! This stage begins after object extraction, rustc relocation resolution, and
//! legacy-to-canonical ABI adaptation. It deliberately never reconstructs
//! identity from an authored path, item spelling, source location, or target
//! directory. Every component/helper edge is selected through a
//! [`ResolvedTransportWitness`].
//!
//! Build-local definition coordinates are retained in [`SemanticLinkPlan`] for
//! final-compilation lookup, but are generally omitted from the reusable
//! semantic graph. Helper relocations are the deliberate exception: each
//! `(call, source edge, target definition, target semantic key)` tuple controls
//! final JavaScript routing and is therefore authenticated by [`PlanId`].
//! [`LinkPlanMaterial`] also includes the exact [`LinkUnitId`], so the final ID
//! binds both the realized build and the complete behavior plan.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rsjs_ir::abi::{
    canonical_bytes, AbiValidationError, CanonicalEncode, CanonicalEncoder, ComponentBindingSource,
    ComponentCallbackShape, ComponentDescriptor, ComponentEdgeId, ComponentFieldId,
    ComponentFieldRole, ComponentLifecycleBranch, ComponentLifecycleControl, ComponentSemanticKey,
    CrossingHookId, DescriptorBody, DescriptorEnvelope, DescriptorKind, HelperCallEdgeId,
    HelperDescriptor, HelperEffectClass, HelperSemanticKey, LinkUnitId, LocalValueId, OriginKind,
    PlanId, ProviderSiteId, SemanticHash, SharedStateRefId, TemporalPropMode, WitnessRecordId,
};
use rsjs_ir::build::LinkPlanMaterial;
use rsjs_ir::link::{
    link_resolved_component_graph, link_shared_state_schemas, partition_owners,
    ComponentGraphLinkError, LinkedCallbackDispatch, LinkedComponentInputs,
    LinkedSharedStateSchema, OwnerActivationStrategy, OwnerCandidateId, OwnerCouplingKind,
    OwnerPartitionPlan, OwnerRelation, OwnerRelationKind, ResolvedComponentEdge,
    ResolvedComponentGraph, ResolvedComponentInstance, SharedStateSchemaConflict,
};
use rsjs_transport::TransportRecordId;
use serde_json::Value;
use thiserror::Error;

use crate::rsjs::abi_adapter::{
    validate_component_codegen_bridge, validate_helper_codegen_bridge, AbiBridgeResult,
    AdaptedTransportAbi, CanonicalDescriptorBridge, ComponentCodegenBridge, ComponentFragmentRoot,
    ComponentNodeRefBindTarget, HelperCodegenBody, HelperCodegenBridge, ResolvedTransportWitness,
    UnsupportedDescriptor,
};
use crate::rsjs::link::TransportDefinitionId;

/// External semantic-plan schema generation.
pub const SEMANTIC_LINK_SCHEMA_VERSION: u16 = 15;
/// Fixed external semantic-plan format discriminator.
pub const SEMANTIC_PLAN_FORMAT: &str = "proa.rsjs.semantic_link_plan";
/// Content-addressed filename suffix used by the final compiler.
pub const SEMANTIC_PLAN_FILE_SUFFIX: &str = "semantic-plan.json";

static SEMANTIC_PLAN_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

/// One direct component edge resolved through an exact rustc witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticComponentEdge {
    /// Parent-local canonical edge ID.
    pub edge: ComponentEdgeId,
    /// Parent-local canonical witness record.
    pub witness: WitnessRecordId,
    /// Exact source-edge record reached in the descriptor object.
    pub source_edge: TransportRecordId,
    /// Exact build-local target definition.
    pub target_definition: TransportDefinitionId,
    /// Stable semantic identity of the selected target.
    pub target: ComponentSemanticKey,
    /// Conditional-single occurrence metadata, or `None` for static-single.
    pub lifecycle_control: Option<ComponentLifecycleControl>,
}

/// One helper call resolved through an exact rustc witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticHelperEdge {
    /// Caller-local canonical call edge ID.
    pub call: HelperCallEdgeId,
    /// Caller-local canonical witness record.
    pub witness: WitnessRecordId,
    /// Exact source-edge record reached in the descriptor object.
    pub source_edge: TransportRecordId,
    /// Exact build-local target definition.
    pub target_definition: TransportDefinitionId,
    /// Stable semantic identity of the selected target.
    pub target: HelperSemanticKey,
}

/// One canonical component and its exact resolved outgoing graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSemanticComponent {
    /// Exact build-local defining record.
    pub definition: TransportDefinitionId,
    /// Stable component ABI key.
    pub semantic_key: ComponentSemanticKey,
    /// Complete canonical descriptor.
    pub descriptor: DescriptorEnvelope,
    /// Validated producer-local relocation data for final browser codegen.
    pub codegen: ComponentCodegenBridge,
    /// Direct component edges in canonical local-ID order.
    pub component_edges: Vec<SemanticComponentEdge>,
    /// Direct helper edges in canonical local-ID order.
    pub helper_edges: Vec<SemanticHelperEdge>,
    /// Dependency-first reachable origin-free helper closure.
    pub helper_closure: Vec<HelperSemanticKey>,
}

/// One concrete component site in a standalone-root expansion.
///
/// Component definitions remain deduplicated in [`LinkedSemanticComponent`].
/// A fresh site is allocated for every standalone root and every direct
/// static-single or conditional-single child edge reached beneath that root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSemanticComponentSite {
    /// Dense component-instance/candidate identity.
    pub candidate: OwnerCandidateId,
    /// Stable semantic identity of the standalone root being expanded.
    pub root: ComponentSemanticKey,
    /// Stable semantic identity of the component rendered at this site.
    pub component: ComponentSemanticKey,
    /// Exact build-local definition rendered at this site.
    pub definition: TransportDefinitionId,
    /// Parent site, or `None` for the standalone root itself.
    pub parent: Option<OwnerCandidateId>,
    /// Parent-local direct edge that created this site.
    pub incoming_edge: Option<ComponentEdgeId>,
    /// Deterministic root-relative sequence of direct component edge IDs.
    pub structural_path: Vec<ComponentEdgeId>,
    /// Exact conditional control and branch on the incoming edge.
    ///
    /// This is `None` for standalone roots and static-single child sites.
    pub incoming_lifecycle: Option<ComponentLifecycleControl>,
    /// Conditional site whose mount/dispose lifetime governs this site.
    ///
    /// A conditional site names itself. Its static descendants inherit the
    /// same root so owner partitioning cannot leave browser-active descendants
    /// mounted after their governing branch disappears.
    pub reactive_lifecycle_root: Option<OwnerCandidateId>,
}

/// One exact lexical SharedState provider occurrence in an admitted root.
///
/// The provider itself renders no DOM. `host` plus `provider_site` is the
/// stable final-plan identity used to tag the response-local provider scope;
/// `child` is the direct component subtree rendered while that scope is
/// active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSharedStateProviderOccurrence {
    /// Component occurrence containing the provider declaration.
    pub host: OwnerCandidateId,
    /// Host-local canonical provider site.
    pub provider_site: ProviderSiteId,
    /// Exact direct child occurrence inside the provider.
    pub child: OwnerCandidateId,
    /// Public SharedState protocol key.
    pub type_key: String,
    /// Complete canonical wire-schema ABI.
    pub schema_abi: SemanticHash,
}

/// One component acquisition resolved to an exact lexical provider
/// occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSharedStateProviderRequirement {
    /// Consuming component occurrence.
    pub consumer: OwnerCandidateId,
    /// Consumer-local acquisition record.
    pub reference: SharedStateRefId,
    /// Exact provider host occurrence.
    pub provider_host: OwnerCandidateId,
    /// Exact provider site within `provider_host`.
    pub provider_site: ProviderSiteId,
    /// Public SharedState protocol key.
    pub type_key: String,
    /// Complete canonical wire-schema ABI.
    pub schema_abi: SemanticHash,
}

/// Missing lexical SharedState context that prevents one structurally valid
/// component definition from becoming a standalone render root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneSharedStateRequirement {
    /// Consumer reached beneath the attempted root.
    pub consumer: ComponentSemanticKey,
    /// Root-relative direct-component edge path to that consumer.
    pub structural_path: Vec<ComponentEdgeId>,
    /// Consumer-local acquisition record.
    pub reference: SharedStateRefId,
    /// Required public protocol key.
    pub type_key: String,
    /// Required canonical wire-schema ABI.
    pub schema_abi: SemanticHash,
}

/// Missing incoming `NodeRef<T>` context that prevents one structurally valid
/// component definition from becoming a standalone render root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneNodeRefRequirement {
    /// Consumer reached beneath the attempted root.
    pub consumer: ComponentSemanticKey,
    /// Root-relative direct-component edge path to that consumer.
    pub structural_path: Vec<ComponentEdgeId>,
    /// Consumer-local NodeRef field.
    pub field: ComponentFieldId,
    /// Exact child crossing hook for the handle.
    pub crossing_hook: CrossingHookId,
    /// Producer-local NodeRef declaration represented by the field.
    pub node_ref: u32,
    /// Terminal DOM family when the consumer binds the handle locally.
    ///
    /// `None` means the consumer forwards the exact handle through another
    /// static direct component edge.
    pub bind_target: Option<ComponentNodeRefBindTarget>,
}

/// One compiler-owned callback that must be supplied by an analyzed parent
/// edge before a component can participate in a client owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneCallbackRequirement {
    /// Consumer that declares the inbound callback.
    pub consumer: ComponentSemanticKey,
    /// Root-relative path to the consumer. Standalone callback requirements
    /// currently arise only at the attempted root, so this is empty.
    pub structural_path: Vec<ComponentEdgeId>,
    /// Consumer-local callback field.
    pub field: ComponentFieldId,
    /// Exact crossing hook for the callback.
    pub crossing_hook: CrossingHookId,
    /// Required/optional callback shape authenticated by the field witness.
    pub shape: ComponentCallbackShape,
}

/// One ancestor-supplied context requirement for a standalone root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandaloneContextRequirement {
    /// Lexically scoped SharedState provider context.
    SharedState(StandaloneSharedStateRequirement),
    /// Exact incoming `NodeRef<T>` handle.
    NodeRef(StandaloneNodeRefRequirement),
    /// Current callback supplied by an analyzed parent edge.
    Callback(StandaloneCallbackRequirement),
}

/// One occurrence-specific exact `NodeRef<T>` object alias.
///
/// Producer-local IDs are intentionally retained here because final linked
/// owner codegen mounts the two concrete producer fragments and must pass the
/// same runtime object from parent to child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedNodeRefHandleAlias {
    /// Parent occurrence exporting the source object.
    pub parent: OwnerCandidateId,
    /// Parent-local direct component edge.
    pub edge: ComponentEdgeId,
    /// Producer-local NodeRef declaration in the parent fragment.
    pub parent_node_ref: u32,
    /// Canonical parent handle value authenticated by the direct binding.
    pub parent_value: LocalValueId,
    /// Direct child occurrence receiving the same object.
    pub child: OwnerCandidateId,
    /// Canonical child field receiving the handle.
    pub field: ComponentFieldId,
    /// Exact child crossing hook.
    pub crossing_hook: CrossingHookId,
    /// Producer-local NodeRef declaration overridden in the child fragment.
    pub child_node_ref: u32,
    /// Canonical child handle value authenticated by the field schema.
    pub child_value: LocalValueId,
    /// Terminal DOM family when the child binds the handle locally.
    pub bind_target: Option<ComponentNodeRefBindTarget>,
}

/// Why a component definition does or does not receive an ordinary standalone
/// render occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandaloneRootDisposition {
    /// The definition requires an immutable incoming static slot and can only
    /// occur beneath a concrete parent edge.
    StructuralIncomingOnly,
    /// The definition is a valid standalone root.
    Admitted {
        /// Dense root candidate allocated in the final component-site table.
        candidate: OwnerCandidateId,
    },
    /// The definition is structurally root-capable but one or more reachable
    /// acquisitions require lexical context supplied by an ancestor.
    ContextRequired {
        /// Complete, deterministic set of missing lexical requirements.
        missing: Vec<StandaloneContextRequirement>,
    },
}

/// Standalone-root decision for one canonical component definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneRootDecision {
    /// Exact build-local definition.
    pub definition: TransportDefinitionId,
    /// Stable component identity.
    pub component: ComponentSemanticKey,
    /// Final root disposition.
    pub disposition: StandaloneRootDisposition,
}

/// One canonical helper and its exact resolved outgoing graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSemanticHelper {
    /// Exact build-local defining record.
    pub definition: TransportDefinitionId,
    /// Stable helper ABI key.
    pub semantic_key: HelperSemanticKey,
    /// Complete canonical descriptor.
    pub descriptor: DescriptorEnvelope,
    /// Validated concrete signature and relocation-resolved Client IR.
    pub codegen: HelperCodegenBridge,
    /// Direct helper calls in canonical local-ID order.
    pub helper_edges: Vec<SemanticHelperEdge>,
    /// Dependency-first transitive closure, including this helper.
    pub transitive_closure: Vec<HelperSemanticKey>,
}

/// Complete deterministic semantic plan for one exact root link unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticLinkPlan {
    /// Exact pre-link root identity.
    pub link_unit: LinkUnitId,
    /// Content identity of the canonical semantic plan.
    pub plan_id: PlanId,
    /// Descriptor and payload hashes used to derive `plan_id`.
    pub material: LinkPlanMaterial,
    /// Components sorted by stable semantic key.
    pub components: Vec<LinkedSemanticComponent>,
    /// Concrete standalone-root, static-single, and conditional-single sites.
    pub sites: Vec<LinkedSemanticComponentSite>,
    /// Explicit standalone-root decision for every component definition.
    pub root_decisions: Vec<StandaloneRootDecision>,
    /// Exact lexical provider occurrences in admitted root expansions.
    pub shared_state_providers: Vec<LinkedSharedStateProviderOccurrence>,
    /// Exact consumer-to-provider resolutions in admitted root expansions.
    pub shared_state_requirements: Vec<LinkedSharedStateProviderRequirement>,
    /// Occurrence-specific exact NodeRef object aliases.
    pub node_ref_aliases: Vec<LinkedNodeRefHandleAlias>,
    /// Helpers sorted by stable semantic key.
    pub helpers: Vec<LinkedSemanticHelper>,
    /// Dependency-first union of helpers reachable from any component.
    pub reachable_helpers: Vec<HelperSemanticKey>,
    /// Final deterministic owner partitions.
    pub owners: OwnerPartitionPlan,
    /// Candidate-specific live-prop timing and current-callback contracts.
    pub component_inputs: LinkedComponentInputs,
    /// Deduplicated, conflict-checked SharedState schemas.
    pub shared_state: Vec<LinkedSharedStateSchema>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticComponentEdgeMaterial {
    edge: ComponentEdgeId,
    target: ComponentSemanticKey,
    lifecycle_control: Option<ComponentLifecycleControl>,
}

impl CanonicalEncode for SemanticComponentEdgeMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticComponentEdgeMaterial", 3);
        encoder.field(1, &self.edge);
        encoder.field(2, &self.target);
        encoder.field(3, &self.lifecycle_control);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticHelperEdgeMaterial {
    call: HelperCallEdgeId,
    target: HelperSemanticKey,
    source_edge: TransportRecordId,
    target_definition: TransportDefinitionId,
}

impl From<&SemanticHelperEdge> for SemanticHelperEdgeMaterial {
    fn from(edge: &SemanticHelperEdge) -> Self {
        Self {
            call: edge.call,
            target: edge.target.clone(),
            source_edge: edge.source_edge,
            target_definition: edge.target_definition,
        }
    }
}

impl CanonicalEncode for SemanticHelperEdgeMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticHelperEdgeMaterial", 4);
        encoder.field(1, &self.call);
        // Field 2 retains the established target identity. The authenticated
        // relocation coordinates extend the record without reusing an
        // established canonical field number.
        encoder.field(2, &self.target);
        encoder.field(3, &self.source_edge);
        encoder.field(4, &self.target_definition);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticComponentMaterial {
    semantic_key: ComponentSemanticKey,
    descriptor_hash: SemanticHash,
    codegen: ComponentCodegenBridge,
    component_edges: Vec<SemanticComponentEdgeMaterial>,
    helper_edges: Vec<SemanticHelperEdgeMaterial>,
    helper_closure: Vec<HelperSemanticKey>,
}

impl CanonicalEncode for SemanticComponentMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticComponentMaterial", 6);
        encoder.field(1, &self.semantic_key);
        encoder.field(2, &self.descriptor_hash);
        encoder.field(3, &self.codegen);
        encoder.field(4, &self.component_edges);
        encoder.field(5, &self.helper_edges);
        encoder.field(6, &self.helper_closure);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticComponentSiteMaterial {
    candidate: OwnerCandidateId,
    root: ComponentSemanticKey,
    component: ComponentSemanticKey,
    parent: Option<OwnerCandidateId>,
    incoming_edge: Option<ComponentEdgeId>,
    structural_path: Vec<ComponentEdgeId>,
    incoming_lifecycle: Option<ComponentLifecycleControl>,
    reactive_lifecycle_root: Option<OwnerCandidateId>,
}

impl CanonicalEncode for SemanticComponentSiteMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticComponentSiteMaterial", 8);
        encoder.field(1, &self.candidate);
        encoder.field(2, &self.root);
        encoder.field(3, &self.component);
        encoder.field(4, &self.parent);
        encoder.field(5, &self.incoming_edge);
        encoder.field(6, &self.structural_path);
        encoder.field(7, &self.incoming_lifecycle);
        encoder.field(8, &self.reactive_lifecycle_root);
    }
}

impl CanonicalEncode for LinkedSharedStateProviderOccurrence {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSharedStateProviderOccurrence", 5);
        encoder.field(1, &self.host);
        encoder.field(2, &self.provider_site);
        encoder.field(3, &self.child);
        encoder.field(4, &self.type_key);
        encoder.field(5, &self.schema_abi);
    }
}

impl CanonicalEncode for LinkedSharedStateProviderRequirement {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSharedStateProviderRequirement", 6);
        encoder.field(1, &self.consumer);
        encoder.field(2, &self.reference);
        encoder.field(3, &self.provider_host);
        encoder.field(4, &self.provider_site);
        encoder.field(5, &self.type_key);
        encoder.field(6, &self.schema_abi);
    }
}

impl CanonicalEncode for StandaloneSharedStateRequirement {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("StandaloneSharedStateRequirement", 5);
        encoder.field(1, &self.consumer);
        encoder.field(2, &self.structural_path);
        encoder.field(3, &self.reference);
        encoder.field(4, &self.type_key);
        encoder.field(5, &self.schema_abi);
    }
}

impl CanonicalEncode for StandaloneNodeRefRequirement {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("StandaloneNodeRefRequirement", 6);
        encoder.field(1, &self.consumer);
        encoder.field(2, &self.structural_path);
        encoder.field(3, &self.field);
        encoder.field(4, &self.crossing_hook);
        encoder.field(5, &self.node_ref);
        encoder.field(6, &self.bind_target);
    }
}

impl CanonicalEncode for StandaloneCallbackRequirement {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("StandaloneCallbackRequirement", 5);
        encoder.field(1, &self.consumer);
        encoder.field(2, &self.structural_path);
        encoder.field(3, &self.field);
        encoder.field(4, &self.crossing_hook);
        encoder.field(5, &self.shape);
    }
}

impl CanonicalEncode for StandaloneContextRequirement {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::SharedState(requirement) => {
                encoder.variant("StandaloneContextRequirement", 0, 1);
                encoder.field(1, requirement);
            }
            Self::NodeRef(requirement) => {
                encoder.variant("StandaloneContextRequirement", 1, 1);
                encoder.field(1, requirement);
            }
            Self::Callback(requirement) => {
                encoder.variant("StandaloneContextRequirement", 2, 1);
                encoder.field(1, requirement);
            }
        }
    }
}

impl CanonicalEncode for LinkedNodeRefHandleAlias {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedNodeRefHandleAlias", 10);
        encoder.field(1, &self.parent);
        encoder.field(2, &self.edge);
        encoder.field(3, &self.parent_node_ref);
        encoder.field(4, &self.parent_value);
        encoder.field(5, &self.child);
        encoder.field(6, &self.field);
        encoder.field(7, &self.crossing_hook);
        encoder.field(8, &self.child_node_ref);
        encoder.field(9, &self.child_value);
        encoder.field(10, &self.bind_target);
    }
}

impl CanonicalEncode for StandaloneRootDisposition {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::StructuralIncomingOnly => {
                encoder.variant("StandaloneRootDisposition", 0, 0);
            }
            Self::Admitted { candidate } => {
                encoder.variant("StandaloneRootDisposition", 1, 1);
                encoder.field(1, candidate);
            }
            Self::ContextRequired { missing } => {
                encoder.variant("StandaloneRootDisposition", 2, 1);
                encoder.field(1, missing);
            }
        }
    }
}

impl CanonicalEncode for StandaloneRootDecision {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        // Build-local definition coordinates are intentionally excluded from
        // semantic identity. `component` is the stable canonical identity;
        // `definition` exists only for final-compilation record selection.
        encoder.record("StandaloneRootDecision", 2);
        encoder.field(1, &self.component);
        encoder.field(2, &self.disposition);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticHelperMaterial {
    semantic_key: HelperSemanticKey,
    descriptor_hash: SemanticHash,
    signature_hash: SemanticHash,
    codegen: HelperCodegenBridge,
    helper_edges: Vec<SemanticHelperEdgeMaterial>,
    transitive_closure: Vec<HelperSemanticKey>,
}

impl CanonicalEncode for SemanticHelperMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticHelperMaterial", 6);
        encoder.field(1, &self.semantic_key);
        encoder.field(2, &self.descriptor_hash);
        encoder.field(3, &self.signature_hash);
        // Preserve the established schema-13 field meanings. Concrete
        // codegen was added later and therefore extends the canonical record
        // instead of renumbering helper graph material already committed by
        // earlier plan identities.
        encoder.field(4, &self.helper_edges);
        encoder.field(5, &self.transitive_closure);
        encoder.field(6, &self.codegen);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticPlanPayload {
    schema_version: u16,
    components: Vec<SemanticComponentMaterial>,
    sites: Vec<SemanticComponentSiteMaterial>,
    helpers: Vec<SemanticHelperMaterial>,
    reachable_helpers: Vec<HelperSemanticKey>,
    owners: OwnerPartitionPlan,
    component_inputs: LinkedComponentInputs,
    shared_state: Vec<LinkedSharedStateSchema>,
    root_decisions: Vec<StandaloneRootDecision>,
    shared_state_providers: Vec<LinkedSharedStateProviderOccurrence>,
    shared_state_requirements: Vec<LinkedSharedStateProviderRequirement>,
    node_ref_aliases: Vec<LinkedNodeRefHandleAlias>,
}

impl CanonicalEncode for SemanticPlanPayload {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticPlanPayload", 12);
        encoder.field(1, &self.schema_version);
        encoder.field(2, &self.components);
        encoder.field(3, &self.sites);
        encoder.field(4, &self.helpers);
        encoder.field(5, &self.reachable_helpers);
        encoder.field(6, &self.owners);
        encoder.field(7, &self.component_inputs);
        encoder.field(8, &self.shared_state);
        encoder.field(9, &self.root_decisions);
        encoder.field(10, &self.shared_state_providers);
        encoder.field(11, &self.shared_state_requirements);
        encoder.field(12, &self.node_ref_aliases);
    }
}

#[derive(Clone)]
struct PendingComponent {
    definition: TransportDefinitionId,
    bridge: CanonicalDescriptorBridge,
}

impl PendingComponent {
    fn body(&self) -> &ComponentDescriptor {
        match &self.bridge.envelope.body {
            DescriptorBody::Component(component) => component,
            _ => unreachable!("component collection was classified"),
        }
    }
}

#[derive(Clone)]
struct PendingHelper {
    definition: TransportDefinitionId,
    bridge: CanonicalDescriptorBridge,
}

impl PendingHelper {
    fn body(&self) -> &HelperDescriptor {
        match &self.bridge.envelope.body {
            DescriptorBody::Helper(helper) => helper,
            _ => unreachable!("helper collection was classified"),
        }
    }
}

/// Expected semantic meaning of one bridge witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticWitnessKind {
    /// Defining component/helper item.
    Definition,
    /// Direct component syntax edge.
    ComponentEdge,
    /// Client-visible component input field.
    ComponentField,
    /// Direct helper call.
    HelperEdge,
}

impl fmt::Display for SemanticWitnessKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Definition => formatter.write_str("definition"),
            Self::ComponentEdge => formatter.write_str("component edge"),
            Self::ComponentField => formatter.write_str("component field"),
            Self::HelperEdge => formatter.write_str("helper edge"),
        }
    }
}

fn witness_kind(witness: &ResolvedTransportWitness) -> SemanticWitnessKind {
    match witness {
        ResolvedTransportWitness::Definition(_) => SemanticWitnessKind::Definition,
        ResolvedTransportWitness::ComponentEdge { .. } => SemanticWitnessKind::ComponentEdge,
        ResolvedTransportWitness::ComponentField { .. } => SemanticWitnessKind::ComponentField,
        ResolvedTransportWitness::HelperEdge { .. } => SemanticWitnessKind::HelperEdge,
    }
}

/// Fail-closed semantic-link failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SemanticLinkError {
    /// The adapter could not losslessly construct one canonical descriptor.
    #[error("unsupported descriptor: {0}")]
    Unsupported(UnsupportedDescriptor),
    /// The canonical graph names a runtime family the current linked-owner
    /// emitter cannot publish faithfully.
    #[error(
        "component {definition:?} uses runtime features unsupported by linked-owner codegen: {features:?}"
    )]
    UnsupportedLinkedOwnerRuntimeFeatures {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Stable runtime-family labels in contract order.
        features: Vec<&'static str>,
    },
    /// A component/helper result appeared in the wrong adapter collection.
    #[error(
        "{collection} adapter collection contains a canonical {actual:?} descriptor; expected {expected:?}"
    )]
    UnexpectedDescriptorKind {
        /// Adapter collection label.
        collection: &'static str,
        /// Required body kind.
        expected: DescriptorKind,
        /// Actual body kind.
        actual: DescriptorKind,
    },
    /// The descriptor body and its construction-time codegen tag disagree.
    #[error(
        "canonical {descriptor_kind} descriptor {definition:?} carries {codegen_kind} codegen"
    )]
    DescriptorCodegenKindMismatch {
        /// Descriptor body kind.
        descriptor_kind: &'static str,
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Codegen payload kind.
        codegen_kind: &'static str,
    },
    /// Producer-local component codegen coordinates disagree with the
    /// canonical descriptor they accompany.
    #[error("component {definition:?} has invalid codegen bridge: {reason}")]
    InvalidComponentCodegen {
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// Producer-local helper codegen data disagrees with the canonical helper
    /// descriptor it accompanies.
    #[error("helper {definition:?} has invalid codegen bridge: {reason}")]
    InvalidHelperCodegen {
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// The complete helper codegen graph fails shared final-link validation.
    #[error("linked helper codegen graph is invalid: {reason}")]
    InvalidHelperProgram {
        /// Focused shared-validator diagnostic.
        reason: String,
    },
    /// A supposedly canonical descriptor no longer satisfies canonical shape
    /// validation.
    #[error("canonical {kind} descriptor {definition:?} is invalid: {source}")]
    InvalidCanonicalDescriptor {
        /// Component/helper label.
        kind: &'static str,
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Canonical descriptor validation failure.
        #[source]
        source: AbiValidationError,
    },
    /// Public fields were changed without refreshing the canonical hash.
    #[error(
        "canonical {kind} descriptor {definition:?} has a stale semantic hash: stored {stored}, recomputed {recomputed}"
    )]
    DescriptorHashMismatch {
        /// Component/helper label.
        kind: &'static str,
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Stored descriptor hash.
        stored: SemanticHash,
        /// Recomputed descriptor hash.
        recomputed: SemanticHash,
    },
    /// A required canonical witness has no exact transport resolution.
    #[error("{kind} descriptor {definition:?} is missing {expected} witness {witness:?}")]
    MissingWitness {
        /// Component/helper label.
        kind: &'static str,
        /// Exact build-local definition when already known.
        definition: TransportDefinitionId,
        /// Canonical witness ID.
        witness: WitnessRecordId,
        /// Required witness meaning.
        expected: SemanticWitnessKind,
    },
    /// The exact witness table gives a different edge class.
    #[error(
        "{kind} descriptor {definition:?} witness {witness:?} is {actual}, expected {expected}"
    )]
    WitnessKindMismatch {
        /// Component/helper label.
        kind: &'static str,
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Canonical witness ID.
        witness: WitnessRecordId,
        /// Required witness meaning.
        expected: SemanticWitnessKind,
        /// Actual witness meaning.
        actual: SemanticWitnessKind,
    },
    /// An adapter result has no usable definition identity yet.
    #[error(
        "canonical {kind} descriptor has no exact defining witness {witness:?}; descriptor adaptation must preserve its TransportDefinitionId"
    )]
    MissingDefiningWitness {
        /// Component/helper label.
        kind: &'static str,
        /// Canonical defining witness.
        witness: WitnessRecordId,
    },
    /// The canonical defining witness resolved as an edge rather than an item.
    #[error(
        "canonical {kind} defining witness {witness:?} is {actual}, expected a definition; no descriptor identity can be inferred from an edge target"
    )]
    DefiningWitnessKindMismatch {
        /// Component/helper label.
        kind: &'static str,
        /// Canonical defining witness.
        witness: WitnessRecordId,
        /// Actual witness meaning.
        actual: SemanticWitnessKind,
    },
    /// Two canonical records claim the same exact definition.
    #[error(
        "exact transport definition {definition:?} is claimed by both {first_kind} and {second_kind} descriptors"
    )]
    DuplicateDefinition {
        /// Reused exact definition.
        definition: TransportDefinitionId,
        /// First descriptor kind.
        first_kind: &'static str,
        /// Second descriptor kind.
        second_kind: &'static str,
    },
    /// Stable component identity is ambiguous.
    #[error(
        "duplicate component semantic key {key:?} for exact definitions {first:?} and {second:?}"
    )]
    DuplicateComponentKey {
        /// Ambiguous stable key.
        key: ComponentSemanticKey,
        /// First exact definition.
        first: TransportDefinitionId,
        /// Second exact definition.
        second: TransportDefinitionId,
    },
    /// Stable helper identity is ambiguous.
    #[error(
        "duplicate helper semantic key {key:?} for exact definitions {first:?} and {second:?}"
    )]
    DuplicateHelperKey {
        /// Ambiguous stable key.
        key: HelperSemanticKey,
        /// First exact definition.
        first: TransportDefinitionId,
        /// Second exact definition.
        second: TransportDefinitionId,
    },
    /// A component witness resolved to no canonical component definition.
    #[error(
        "component {parent:?} edge {edge:?} resolved to unknown component definition {target:?}"
    )]
    UnknownComponentTarget {
        /// Parent exact definition.
        parent: TransportDefinitionId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Relocation-selected target.
        target: TransportDefinitionId,
    },
    /// A component field witness relocated to another component definition.
    #[error(
        "component {component:?} field {field:?} resolved to foreign component definition {target:?}"
    )]
    ForeignComponentFieldTarget {
        /// Component claiming the field.
        component: TransportDefinitionId,
        /// Canonical component field.
        field: rsjs_ir::abi::ComponentFieldId,
        /// Relocation-selected target.
        target: TransportDefinitionId,
    },
    /// Two canonical component fields selected one exact object record.
    #[error("component {component:?} has duplicate rustc-backed field record {field_record:?}")]
    DuplicateComponentFieldRecord {
        /// Defining component.
        component: TransportDefinitionId,
        /// Reused object record.
        field_record: TransportRecordId,
    },
    /// A helper witness resolved to no canonical helper definition.
    #[error(
        "{caller_kind} {caller:?} helper edge {call:?} resolved to unknown helper definition {target:?}"
    )]
    UnknownHelperTarget {
        /// Component/helper caller label.
        caller_kind: &'static str,
        /// Caller exact definition.
        caller: TransportDefinitionId,
        /// Caller-local helper edge.
        call: HelperCallEdgeId,
        /// Relocation-selected target.
        target: TransportDefinitionId,
    },
    /// A component definition recursively reaches itself through static sites.
    #[error(
        "recursive static component graph under root {root:?}: component {parent:?} edge {edge:?} reaches ancestor {target:?} at structural path {path:?}; recursive component sites cannot form a finite owner plan"
    )]
    RecursiveStaticComponentGraph {
        /// Standalone root whose expansion found the cycle.
        root: ComponentSemanticKey,
        /// Component containing the back edge.
        parent: ComponentSemanticKey,
        /// Parent-local edge that closes the cycle.
        edge: ComponentEdgeId,
        /// Ancestor definition reached again.
        target: ComponentSemanticKey,
        /// Root-relative path including the closing edge.
        path: Vec<ComponentEdgeId>,
    },
    /// A conditional site cannot be nested under another conditional
    /// component lifetime until nested mount generations are represented.
    #[error(
        "nested conditional component site under root {root:?}: component {parent:?} edge {edge:?} is conditional-single on control {control:?} branch {branch:?} at structural path {path:?}, inside lifecycle root candidate {reactive_lifecycle_root:?}; nested component mount/dispose identity is not implemented"
    )]
    UnsupportedNestedConditionalComponentSite {
        /// Standalone root whose expansion reached the nested site.
        root: ComponentSemanticKey,
        /// Component containing the nested dynamic edge.
        parent: ComponentSemanticKey,
        /// Parent-local dynamic edge.
        edge: ComponentEdgeId,
        /// Parent-local conditional control.
        control: rsjs_ir::abi::LocalControlRegionId,
        /// Branch containing the component occurrence.
        branch: ComponentLifecycleBranch,
        /// Root-relative path including the nested edge.
        path: Vec<ComponentEdgeId>,
        /// Existing conditional site governing the parent.
        reactive_lifecycle_root: OwnerCandidateId,
    },
    /// A provider declaration inherited a client-controlled component
    /// lifecycle. V1 provider scopes must be established by the static shell.
    #[error(
        "dynamic SharedState provider under root {root:?}: component {host:?} provider site {provider_site:?} at structural path {path:?} inherits lifecycle root {reactive_lifecycle_root:?}; move the provider into the static shell above the conditional component edge"
    )]
    DynamicSharedStateProviderHostUnsupported {
        /// Standalone root whose expansion reached the provider.
        root: ComponentSemanticKey,
        /// Component containing the provider declaration.
        host: ComponentSemanticKey,
        /// Host-local provider site.
        provider_site: ProviderSiteId,
        /// Root-relative path to the provider host.
        path: Vec<ComponentEdgeId>,
        /// Conditional component site governing the host.
        reactive_lifecycle_root: OwnerCandidateId,
    },
    /// One public SharedState key names incompatible schemas anywhere in the
    /// closed definition set.
    #[error(
        "SharedState type_key {type_key:?} has incompatible schema ABIs {first_schema} in component {first_component:?} and {second_schema} in component {second_component:?}; one type_key must name exactly one wire schema before any owner activates"
    )]
    SharedStateDeclarationConflict {
        /// Conflicting public protocol key.
        type_key: String,
        /// First schema encountered in canonical component order.
        first_schema: SemanticHash,
        /// First declaring component.
        first_component: ComponentSemanticKey,
        /// Incompatible schema.
        second_schema: SemanticHash,
        /// Later declaring component.
        second_component: ComponentSemanticKey,
    },
    /// A static direct component edge does not provide the exact NodeRef
    /// handle required by its child definition.
    #[error(
        "invalid NodeRef handle alias from component {parent:?} edge {edge:?} to {child:?}: {reason}"
    )]
    InvalidNodeRefHandleAlias {
        /// Parent component.
        parent: ComponentSemanticKey,
        /// Parent-local static direct edge.
        edge: ComponentEdgeId,
        /// Resolved child component.
        child: ComponentSemanticKey,
        /// Focused contract mismatch.
        reason: String,
    },
    /// A NodeRef handle edge did not force parent and child into one owner.
    #[error(
        "NodeRef handle alias on edge {edge:?} crosses final owners between candidates {parent:?} and {child:?}"
    )]
    NodeRefAliasOwnerMismatch {
        /// Parent occurrence.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child occurrence.
        child: OwnerCandidateId,
    },
    /// One canonical witness was reused for distinct semantic edges.
    #[error(
        "{kind} descriptor {definition:?} reuses witness {witness:?} for multiple semantic records"
    )]
    WitnessClaimedMultipleTimes {
        /// Component/helper label.
        kind: &'static str,
        /// Exact definition.
        definition: TransportDefinitionId,
        /// Reused witness.
        witness: WitnessRecordId,
    },
    /// Two witness records claim the same exact source edge.
    #[error(
        "{kind} descriptor {definition:?} has duplicate {edge_kind} source-edge record {source_edge:?}"
    )]
    DuplicateSourceEdge {
        /// Component/helper label.
        kind: &'static str,
        /// Exact definition.
        definition: TransportDefinitionId,
        /// Component/helper edge label.
        edge_kind: &'static str,
        /// Reused source edge.
        source_edge: TransportRecordId,
    },
    /// The bridge contains a relocation witness no canonical record consumed.
    #[error("{kind} descriptor {definition:?} contains unclaimed {actual} witness {witness:?}")]
    UnclaimedWitness {
        /// Component/helper label.
        kind: &'static str,
        /// Exact definition.
        definition: TransportDefinitionId,
        /// Canonical witness ID.
        witness: WitnessRecordId,
        /// Witness table meaning.
        actual: SemanticWitnessKind,
    },
    /// A reachable helper violates the v1 origin-free contract.
    #[error("reachable helper {helper:?} is not OriginFree")]
    ReachableOriginatingHelper {
        /// Stable helper identity.
        helper: HelperSemanticKey,
    },
    /// Canonical component resolution or owner partitioning failed.
    #[error("component graph linking failed: {0}")]
    ComponentGraph(#[from] ComponentGraphLinkError),
    /// SharedState public key selected incompatible wire schemas.
    #[error("SharedState schema linking failed: {0}")]
    SharedState(#[from] SharedStateSchemaConflict),
}

/// Internal-consistency failure in an owned semantic plan.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SemanticPlanValidationError {
    /// The material belongs to a different root.
    #[error("semantic plan material has LinkUnitId {actual}, but the plan declares {expected}")]
    LinkUnitMismatch {
        /// Plan-level root.
        expected: LinkUnitId,
        /// Material root.
        actual: LinkUnitId,
    },
    /// A linked record's envelope has the wrong body kind.
    #[error(
        "semantic plan definition {definition:?} contains a {actual:?} descriptor; expected {expected:?}"
    )]
    UnexpectedDescriptorKind {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Required body kind.
        expected: DescriptorKind,
        /// Actual body kind.
        actual: DescriptorKind,
    },
    /// A component's producer-local relocation table no longer matches its
    /// canonical descriptor.
    #[error("semantic plan component {definition:?} has invalid codegen bridge: {reason}")]
    InvalidComponentCodegen {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// A helper's concrete final-codegen data no longer matches its canonical
    /// descriptor.
    #[error("semantic plan helper {definition:?} has invalid codegen bridge: {reason}")]
    InvalidHelperCodegen {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Focused validation failure.
        reason: String,
    },
    /// The complete persisted helper graph fails shared final-link validation.
    #[error("semantic plan helper codegen graph is invalid: {reason}")]
    InvalidHelperProgram {
        /// Focused shared-validator diagnostic.
        reason: String,
    },
    /// A persisted descriptor no longer satisfies canonical shape validation.
    #[error("semantic plan descriptor {definition:?} is invalid: {source}")]
    InvalidCanonicalDescriptor {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Canonical descriptor validation failure.
        #[source]
        source: AbiValidationError,
    },
    /// A nested conditional site was inserted into an otherwise coherent
    /// semantic plan.
    #[error(
        "semantic plan component site {candidate:?} introduces a nested conditional lifetime beneath lifecycle root {reactive_lifecycle_root:?}"
    )]
    NestedConditionalComponentSite {
        /// Nested site candidate.
        candidate: OwnerCandidateId,
        /// Existing conditional site governing its parent.
        reactive_lifecycle_root: OwnerCandidateId,
    },
    /// A component graph names a runtime family the final linked-owner
    /// emitter cannot publish faithfully.
    #[error(
        "semantic plan component {definition:?} uses runtime features unsupported by linked-owner codegen: {features:?}"
    )]
    UnsupportedLinkedOwnerRuntimeFeatures {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Stable runtime-family labels in contract order.
        features: Vec<&'static str>,
    },
    /// A copied stable key disagrees with its canonical descriptor.
    #[error(
        "semantic plan component {definition:?} has a copied semantic key that disagrees with its descriptor"
    )]
    ComponentKeyMismatch {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
    },
    /// A copied stable key disagrees with its canonical descriptor.
    #[error(
        "semantic plan helper {definition:?} has a copied semantic key that disagrees with its descriptor"
    )]
    HelperKeyMismatch {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
    },
    /// A descriptor body changed without refreshing its hash.
    #[error(
        "semantic plan descriptor {definition:?} has stale hash {stored}; recomputed {recomputed}"
    )]
    DescriptorHashMismatch {
        /// Exact lookup coordinate.
        definition: TransportDefinitionId,
        /// Stored descriptor hash.
        stored: SemanticHash,
        /// Recomputed descriptor hash.
        recomputed: SemanticHash,
    },
    /// Component site IDs must be unique and dense.
    #[error(
        "semantic plan component-site IDs must be dense; expected {expected:?}, found {actual:?}"
    )]
    NonDenseComponentSite {
        /// Required candidate ID.
        expected: OwnerCandidateId,
        /// Candidate ID that appeared.
        actual: OwnerCandidateId,
    },
    /// A component site names no matching deduplicated definition.
    #[error(
        "semantic plan component site {candidate:?} names definition {definition:?} as {component:?}, but no matching component definition exists"
    )]
    UnknownComponentSiteDefinition {
        /// Site candidate.
        candidate: OwnerCandidateId,
        /// Exact build-local definition.
        definition: TransportDefinitionId,
        /// Copied stable key.
        component: ComponentSemanticKey,
    },
    /// Root-site metadata is internally inconsistent.
    #[error("semantic plan root component site {candidate:?} has inconsistent root metadata")]
    InvalidRootComponentSite {
        /// Root site candidate.
        candidate: OwnerCandidateId,
    },
    /// Child-site metadata does not match its parent edge.
    #[error(
        "semantic plan component site {candidate:?} does not match its parent, incoming edge, structural path, or resolved target"
    )]
    InvalidChildComponentSite {
        /// Child site candidate.
        candidate: OwnerCandidateId,
    },
    /// Candidate-specific component inputs could not be re-linked.
    #[error("semantic plan component input graph is invalid: {0}")]
    InvalidComponentInputGraph(#[source] ComponentGraphLinkError),
    /// Stored timing/callback records differ from descriptor-derived records.
    #[error(
        "semantic plan component timing/callback records do not match its descriptors and concrete sites"
    )]
    ComponentInputsMismatch,
    /// Stored owners differ from the descriptor-derived coupling graph.
    #[error("semantic plan owners do not match its descriptor-derived component coupling graph")]
    OwnerPlanMismatch,
    /// Stored SharedState schemas/consumers differ from descriptor-derived
    /// records.
    #[error(
        "semantic plan SharedState schemas or consumers do not match its descriptors and concrete sites"
    )]
    SharedStatePlanMismatch,
    /// Stored standalone-root decisions or exact provider resolutions differ
    /// from a fresh lexical-topology expansion.
    #[error(
        "semantic plan standalone-root or SharedState provider topology does not match its canonical component descriptors"
    )]
    SharedStateProviderTopologyMismatch,
    /// Stored occurrence-specific NodeRef aliases differ from a fresh static
    /// component expansion.
    #[error(
        "semantic plan NodeRef aliases do not match its canonical component descriptors and concrete sites"
    )]
    NodeRefAliasTopologyMismatch,
    /// A stored NodeRef alias crosses final owner partitions.
    #[error(
        "semantic plan NodeRef alias on edge {edge:?} crosses final owners between candidates {parent:?} and {child:?}"
    )]
    NodeRefAliasOwnerMismatch {
        /// Parent occurrence.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child occurrence.
        child: OwnerCandidateId,
    },
    /// Canonical provider topology could not be reconstructed from a mutated
    /// owned plan.
    #[error("semantic plan SharedState provider topology is invalid: {reason}")]
    InvalidSharedStateProviderTopology {
        /// Focused semantic-link diagnostic.
        reason: String,
    },
    /// Component descriptor input hashes are incomplete or reordered.
    #[error("semantic plan component descriptor hash list does not match its canonical records")]
    ComponentDescriptorHashesMismatch,
    /// Helper descriptor input hashes are incomplete or reordered.
    #[error("semantic plan helper descriptor hash list does not match its canonical records")]
    HelperDescriptorHashesMismatch,
    /// The behavior payload no longer matches the material.
    #[error("semantic plan payload hash mismatch: material {stored}, recomputed {recomputed}")]
    PayloadHashMismatch {
        /// Hash in `LinkPlanMaterial`.
        stored: SemanticHash,
        /// Hash over current canonical plan fields.
        recomputed: SemanticHash,
    },
    /// The public plan ID no longer matches its material.
    #[error("semantic plan ID mismatch: stored {stored}, recomputed {recomputed}")]
    PlanIdMismatch {
        /// Plan-level ID.
        stored: PlanId,
        /// ID derived from current material.
        recomputed: PlanId,
    },
}

/// Deterministic JSON serialization failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SemanticPlanSerializationError {
    /// The owned plan is internally inconsistent.
    #[error(transparent)]
    InvalidPlan(#[from] SemanticPlanValidationError),
    /// A future strategy cannot be serialized under the v1 file schema.
    #[error("owner {owner} uses an unsupported activation strategy")]
    UnsupportedOwnerStrategy {
        /// Dense final owner ID.
        owner: u32,
    },
    /// A future temporal mode cannot be serialized under this file schema.
    #[error(
        "component input from candidate {parent} edge {edge} at prop-use site {site} uses an unsupported temporal mode"
    )]
    UnsupportedTemporalMode {
        /// Parent candidate.
        parent: u32,
        /// Parent-local edge.
        edge: u32,
        /// Child-local prop-use site.
        site: u32,
    },
    /// A future callback dispatch mode cannot be serialized under this schema.
    #[error(
        "component callback from candidate {parent} edge {edge} uses an unsupported dispatch mode"
    )]
    UnsupportedCallbackDispatch {
        /// Parent candidate.
        parent: u32,
        /// Parent-local edge.
        edge: u32,
    },
}

/// Atomic semantic-plan persistence failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SemanticPlanWriteError {
    /// Content-addressed plan directories must be explicit absolute paths.
    #[error("semantic plan directory must be absolute: {0}")]
    RelativeDirectory(PathBuf),
    /// The plan could not be converted to its deterministic external form.
    #[error(transparent)]
    SerializePlan(#[from] SemanticPlanSerializationError),
    /// JSON encoding failed.
    #[error("failed to encode semantic plan JSON: {0}")]
    EncodeJson(#[from] serde_json::Error),
    /// Output directory creation failed.
    #[error("failed to create semantic plan directory {path}: {source}")]
    CreateDirectory {
        /// Requested directory.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Existing content could not be read.
    #[error("failed to read existing semantic plan {path}: {source}")]
    ReadExisting {
        /// Existing content-addressed path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A new temporary file could not be created.
    #[error("failed to create temporary semantic plan {path}: {source}")]
    CreateTemporary {
        /// Temporary path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Temporary content could not be durably written.
    #[error("failed to write temporary semantic plan {path}: {source}")]
    WriteTemporary {
        /// Temporary path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Atomic commit failed.
    #[error("failed to atomically commit semantic plan {from} to {to}: {source}")]
    Commit {
        /// Temporary source path.
        from: PathBuf,
        /// Final content-addressed path.
        to: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Strong-ID collision or inconsistent attempted rewrite.
    #[error("semantic plan ID {plan_id} already exists with different content at {path}")]
    ContentCollision {
        /// Colliding strong ID.
        plan_id: PlanId,
        /// Existing path.
        path: PathBuf,
    },
}

/// Link canonical descriptors using only relocation-resolved witness records.
///
/// Input vector order and witness-map insertion order do not affect the output.
/// Components and helpers are renumbered by their stable semantic keys before
/// owner partitioning and plan hashing.
pub fn link_semantic_plan(
    link_unit: LinkUnitId,
    adapted: &AdaptedTransportAbi,
) -> Result<SemanticLinkPlan, SemanticLinkError> {
    let mut components = collect_components(&adapted.components)?;
    let mut helpers = collect_helpers(&adapted.helpers)?;

    components.sort_by(|left, right| left.body().semantic_key.cmp(&right.body().semantic_key));
    helpers.sort_by(|left, right| left.body().semantic_key.cmp(&right.body().semantic_key));

    let component_by_definition = index_component_definitions(&components)?;
    let helper_by_definition = index_helper_definitions(&helpers)?;
    validate_definition_kinds(&component_by_definition, &helper_by_definition)?;
    validate_component_keys(&components)?;
    validate_helper_keys(&helpers)?;

    let helper_keys = helpers
        .iter()
        .map(|helper| helper.body().semantic_key.clone())
        .collect::<Vec<_>>();
    let mut helper_adjacency = vec![Vec::<usize>::new(); helpers.len()];
    let mut linked_helpers = Vec::with_capacity(helpers.len());

    for (index, helper) in helpers.iter().enumerate() {
        let body = helper.body();
        let mut claimed = BTreeSet::from([body.defining_witness]);
        let mut source_edges = BTreeSet::new();
        let mut resolved = Vec::with_capacity(body.direct_helpers.len());
        for (&call, &witness) in &body.direct_helpers {
            claim_witness("helper", helper.definition, witness, &mut claimed)?;
            let (source_edge, target_definition) =
                expect_helper_edge("helper", helper.definition, witness, &helper.bridge)?;
            if !source_edges.insert(source_edge) {
                return Err(SemanticLinkError::DuplicateSourceEdge {
                    kind: "helper",
                    definition: helper.definition,
                    edge_kind: "helper",
                    source_edge,
                });
            }
            let Some(&target_index) = helper_by_definition.get(&target_definition) else {
                return Err(SemanticLinkError::UnknownHelperTarget {
                    caller_kind: "helper",
                    caller: helper.definition,
                    call,
                    target: target_definition,
                });
            };
            helper_adjacency[index].push(target_index);
            resolved.push(SemanticHelperEdge {
                call,
                witness,
                source_edge,
                target_definition,
                target: helper_keys[target_index].clone(),
            });
        }
        helper_adjacency[index].sort_by(|&left, &right| helper_keys[left].cmp(&helper_keys[right]));
        helper_adjacency[index].dedup();
        ensure_complete_witness_coverage("helper", helper, &claimed)?;
        linked_helpers.push(LinkedSemanticHelper {
            definition: helper.definition,
            semantic_key: body.semantic_key.clone(),
            descriptor: helper.bridge.envelope.clone(),
            codegen: helper
                .bridge
                .helper_codegen()
                .cloned()
                .expect("helper collection validates the codegen kind"),
            helper_edges: resolved,
            transitive_closure: Vec::new(),
        });
    }

    validate_linked_helper_codegen_graph(&linked_helpers)
        .map_err(|reason| SemanticLinkError::InvalidHelperProgram { reason })?;

    for (index, helper) in linked_helpers.iter_mut().enumerate() {
        let closure = helper_closure_order(&[index], &helper_adjacency, &helpers, &helper_keys)?;
        helper.transitive_closure = closure
            .into_iter()
            .map(|helper| helper_keys[helper].clone())
            .collect();
    }

    let mut linked_components = Vec::with_capacity(components.len());
    let partition_descriptors = components
        .iter()
        .map(|component| component.body().clone())
        .collect::<Vec<_>>();
    let mut global_helper_roots = BTreeSet::new();

    for component in &components {
        let body = component.body();
        let mut claimed = BTreeSet::from([body.defining_witness]);
        let mut component_sources = BTreeSet::new();
        let mut component_field_records = BTreeSet::new();
        let mut helper_sources = BTreeSet::new();
        let mut direct_components = Vec::with_capacity(body.direct_components.len());
        let mut direct_helpers = Vec::with_capacity(body.direct_helpers.len());
        let mut helper_roots = BTreeSet::new();

        for (&edge, direct) in &body.direct_components {
            claim_witness(
                "component",
                component.definition,
                direct.witness,
                &mut claimed,
            )?;
            let (source_edge, target_definition) =
                expect_component_edge(component.definition, direct.witness, &component.bridge)?;
            if !component_sources.insert(source_edge) {
                return Err(SemanticLinkError::DuplicateSourceEdge {
                    kind: "component",
                    definition: component.definition,
                    edge_kind: "component",
                    source_edge,
                });
            }
            let Some(&target_index) = component_by_definition.get(&target_definition) else {
                return Err(SemanticLinkError::UnknownComponentTarget {
                    parent: component.definition,
                    edge,
                    target: target_definition,
                });
            };

            direct_components.push(SemanticComponentEdge {
                edge,
                witness: direct.witness,
                source_edge,
                target_definition,
                target: components[target_index].body().semantic_key.clone(),
                lifecycle_control: direct.lifecycle_control,
            });
        }

        for (&field, schema) in &body.fields {
            let Some(witness) = schema.type_witness else {
                continue;
            };
            claim_witness("component", component.definition, witness, &mut claimed)?;
            let (field_record, target) =
                expect_component_field(component.definition, witness, &component.bridge)?;
            if target != component.definition {
                return Err(SemanticLinkError::ForeignComponentFieldTarget {
                    component: component.definition,
                    field,
                    target,
                });
            }
            if !component_field_records.insert(field_record) {
                return Err(SemanticLinkError::DuplicateComponentFieldRecord {
                    component: component.definition,
                    field_record,
                });
            }
        }

        for (&call, &witness) in &body.direct_helpers {
            claim_witness("component", component.definition, witness, &mut claimed)?;
            let (source_edge, target_definition) = expect_helper_edge(
                "component",
                component.definition,
                witness,
                &component.bridge,
            )?;
            if !helper_sources.insert(source_edge) {
                return Err(SemanticLinkError::DuplicateSourceEdge {
                    kind: "component",
                    definition: component.definition,
                    edge_kind: "helper",
                    source_edge,
                });
            }
            let Some(&target_index) = helper_by_definition.get(&target_definition) else {
                return Err(SemanticLinkError::UnknownHelperTarget {
                    caller_kind: "component",
                    caller: component.definition,
                    call,
                    target: target_definition,
                });
            };
            helper_roots.insert(target_index);
            global_helper_roots.insert(target_index);
            direct_helpers.push(SemanticHelperEdge {
                call,
                witness,
                source_edge,
                target_definition,
                target: helper_keys[target_index].clone(),
            });
        }
        ensure_complete_witness_coverage("component", component, &claimed)?;

        let helper_closure = helper_closure_order(
            &helper_roots.into_iter().collect::<Vec<_>>(),
            &helper_adjacency,
            &helpers,
            &helper_keys,
        )?
        .into_iter()
        .map(|helper| helper_keys[helper].clone())
        .collect();
        linked_components.push(LinkedSemanticComponent {
            definition: component.definition,
            semantic_key: body.semantic_key.clone(),
            descriptor: component.bridge.envelope.clone(),
            codegen: component
                .bridge
                .component_codegen()
                .cloned()
                .expect("component collection validates the codegen kind"),
            component_edges: direct_components,
            helper_edges: direct_helpers,
            helper_closure,
        });
    }

    let site_plan = instantiate_component_sites(&linked_components, &component_by_definition)?;
    let sites = site_plan.sites;
    let resolved_component_edges = site_plan.resolved_edges;
    let instances = sites
        .iter()
        .map(|site| {
            let definition_index = component_by_definition[&site.definition];
            let descriptor = &partition_descriptors[definition_index];
            ResolvedComponentInstance {
                id: site.candidate,
                descriptor,
                // Canonical descriptors contain the post-reachability browser
                // effect set. Source-only participant effects stay latent until
                // a live edge couples them to an active upstream component.
                has_browser_obligation: has_local_browser_obligation(descriptor),
            }
        })
        .collect::<Vec<_>>();
    let component_graph =
        link_component_graph_with_lifecycle(&instances, &resolved_component_edges, &sites)?;
    let owners = partition_owners(
        &component_graph.owner_input.candidates,
        &component_graph.owner_input.relations,
    )
    .map_err(ComponentGraphLinkError::from)?;
    validate_node_ref_alias_owners(&site_plan.node_ref_aliases, &owners).map_err(
        |(parent, edge, child)| SemanticLinkError::NodeRefAliasOwnerMismatch {
            parent,
            edge,
            child,
        },
    )?;
    let shared_state = link_shared_state_schemas(&instances)?;

    let reachable_helpers = helper_closure_order(
        &global_helper_roots.into_iter().collect::<Vec<_>>(),
        &helper_adjacency,
        &helpers,
        &helper_keys,
    )?
    .into_iter()
    .map(|helper| helper_keys[helper].clone())
    .collect::<Vec<_>>();

    let payload = semantic_plan_payload(
        &linked_components,
        &sites,
        &linked_helpers,
        &reachable_helpers,
        &owners,
        &component_graph.inputs,
        &shared_state,
        &site_plan.root_decisions,
        &site_plan.shared_state_providers,
        &site_plan.shared_state_requirements,
        &site_plan.node_ref_aliases,
    )
    .expect("semantic linker constructs helper records from helper descriptors");
    let material = LinkPlanMaterial {
        link_unit,
        component_descriptors: linked_components
            .iter()
            .map(|component| component.descriptor.semantic_hash)
            .collect(),
        helper_descriptors: linked_helpers
            .iter()
            .map(|helper| helper.descriptor.semantic_hash)
            .collect(),
        payload: SemanticHash::derive(&payload),
    };
    let plan_id = material.plan_id();

    Ok(SemanticLinkPlan {
        link_unit,
        plan_id,
        material,
        components: linked_components,
        sites,
        root_decisions: site_plan.root_decisions,
        shared_state_providers: site_plan.shared_state_providers,
        shared_state_requirements: site_plan.shared_state_requirements,
        node_ref_aliases: site_plan.node_ref_aliases,
        helpers: linked_helpers,
        reachable_helpers,
        owners,
        component_inputs: component_graph.inputs,
        shared_state,
    })
}

struct ComponentSiteInstantiation {
    sites: Vec<LinkedSemanticComponentSite>,
    resolved_edges: Vec<ResolvedComponentEdge>,
    root_decisions: Vec<StandaloneRootDecision>,
    shared_state_providers: Vec<LinkedSharedStateProviderOccurrence>,
    shared_state_requirements: Vec<LinkedSharedStateProviderRequirement>,
    node_ref_aliases: Vec<LinkedNodeRefHandleAlias>,
}

#[derive(Clone)]
struct ActiveSharedStateProvider {
    host: OwnerCandidateId,
    provider_site: ProviderSiteId,
    type_key: String,
    schema_abi: SemanticHash,
}

fn instantiate_component_sites(
    components: &[LinkedSemanticComponent],
    component_by_definition: &BTreeMap<TransportDefinitionId, usize>,
) -> Result<ComponentSiteInstantiation, SemanticLinkError> {
    struct ExpansionFrame {
        component_index: usize,
        site: OwnerCandidateId,
        next_edge: usize,
        provider_depth_before_entry: usize,
    }

    validate_global_shared_state_schemas(components)?;

    // Every structurally root-eligible descriptor is attempted independently.
    // A failed lexical-provider lookup discards the whole provisional
    // expansion, including its candidate IDs, so admitted sites remain dense
    // and no invalid standalone render implementation can be selected later.
    let mut sites = Vec::new();
    let mut resolved_edges = Vec::new();
    let mut root_decisions = Vec::with_capacity(components.len());
    let mut shared_state_providers = Vec::new();
    let mut shared_state_requirements = Vec::new();
    let mut node_ref_aliases = Vec::new();
    for (root_index, root_component) in components.iter().enumerate() {
        let DescriptorBody::Component(root_body) = &root_component.descriptor.body else {
            unreachable!("linked component collection contains component descriptors")
        };
        if !root_body.root_eligible {
            root_decisions.push(StandaloneRootDecision {
                definition: root_component.definition,
                component: root_component.semantic_key.clone(),
                disposition: StandaloneRootDisposition::StructuralIncomingOnly,
            });
            continue;
        }

        let candidate_base = sites.len() as u32;
        let root_candidate = OwnerCandidateId::new(candidate_base);
        let mut provisional_sites = vec![LinkedSemanticComponentSite {
            candidate: root_candidate,
            root: root_component.semantic_key.clone(),
            component: root_component.semantic_key.clone(),
            definition: root_component.definition,
            parent: None,
            incoming_edge: None,
            structural_path: Vec::new(),
            incoming_lifecycle: None,
            reactive_lifecycle_root: None,
        }];
        let mut provisional_edges = Vec::new();
        let mut provisional_providers = Vec::new();
        let mut provisional_requirements = Vec::new();
        let mut provisional_node_ref_aliases = Vec::new();
        let mut missing = Vec::new();
        let mut active_providers = Vec::<ActiveSharedStateProvider>::new();
        record_shared_state_requirements(
            root_component,
            root_candidate,
            &[],
            &active_providers,
            &mut provisional_requirements,
            &mut missing,
        );
        record_standalone_callback_requirements(root_component, &[], &mut missing);
        record_standalone_node_ref_requirements(root_component, &[], &mut missing);

        let mut stack = vec![ExpansionFrame {
            component_index: root_index,
            site: root_candidate,
            next_edge: 0,
            provider_depth_before_entry: 0,
        }];
        while !stack.is_empty() {
            let frame_index = stack.len() - 1;
            let component_index = stack[frame_index].component_index;
            let next_edge = stack[frame_index].next_edge;
            let component = &components[component_index];
            let Some(edge) = component.component_edges.get(next_edge).cloned() else {
                let frame = stack.pop().expect("stack was checked non-empty");
                active_providers.truncate(frame.provider_depth_before_entry);
                continue;
            };
            stack[frame_index].next_edge += 1;
            let parent_site = stack[frame_index].site;
            let parent_index = usize::try_from(parent_site.index() - candidate_base)
                .expect("provisional candidate offset fits usize");

            let DescriptorBody::Component(body) = &component.descriptor.body else {
                unreachable!("linked component collection contains component descriptors")
            };
            let direct = body
                .direct_components
                .get(&edge.edge)
                .expect("resolved component edge remains present in its descriptor");
            let mut structural_path = provisional_sites[parent_index].structural_path.clone();
            structural_path.push(edge.edge);
            let parent_lifecycle_root = provisional_sites[parent_index].reactive_lifecycle_root;
            if let (Some(lifecycle), Some(reactive_lifecycle_root)) =
                (direct.lifecycle_control, parent_lifecycle_root)
            {
                return Err(
                    SemanticLinkError::UnsupportedNestedConditionalComponentSite {
                        root: root_component.semantic_key.clone(),
                        parent: component.semantic_key.clone(),
                        edge: edge.edge,
                        control: lifecycle.control,
                        branch: lifecycle.branch,
                        path: structural_path,
                        reactive_lifecycle_root,
                    },
                );
            }

            let target_index = component_by_definition[&edge.target_definition];
            if stack
                .iter()
                .any(|ancestor| ancestor.component_index == target_index)
            {
                return Err(SemanticLinkError::RecursiveStaticComponentGraph {
                    root: root_component.semantic_key.clone(),
                    parent: component.semantic_key.clone(),
                    edge: edge.edge,
                    target: components[target_index].semantic_key.clone(),
                    path: structural_path,
                });
            }

            let provider = body
                .shared_state_providers
                .iter()
                .find(|(_, provider)| provider.child_edge == edge.edge);
            if let (Some((&provider_site, _)), Some(reactive_lifecycle_root)) =
                (provider, parent_lifecycle_root)
            {
                return Err(
                    SemanticLinkError::DynamicSharedStateProviderHostUnsupported {
                        root: root_component.semantic_key.clone(),
                        host: component.semantic_key.clone(),
                        provider_site,
                        path: provisional_sites[parent_index].structural_path.clone(),
                        reactive_lifecycle_root,
                    },
                );
            }

            let child_candidate =
                OwnerCandidateId::new(candidate_base + provisional_sites.len() as u32);
            let reactive_lifecycle_root = if direct.lifecycle_control.is_some() {
                Some(child_candidate)
            } else {
                parent_lifecycle_root
            };
            provisional_sites.push(LinkedSemanticComponentSite {
                candidate: child_candidate,
                root: root_component.semantic_key.clone(),
                component: components[target_index].semantic_key.clone(),
                definition: edge.target_definition,
                parent: Some(parent_site),
                incoming_edge: Some(edge.edge),
                structural_path: structural_path.clone(),
                incoming_lifecycle: direct.lifecycle_control,
                reactive_lifecycle_root,
            });
            provisional_edges.push(ResolvedComponentEdge {
                parent: parent_site,
                edge: edge.edge,
                child: child_candidate,
            });
            provisional_node_ref_aliases.extend(resolve_node_ref_aliases(
                component,
                parent_site,
                edge.edge,
                &components[target_index],
                child_candidate,
            )?);

            let provider_depth_before_entry = active_providers.len();
            if let Some((&provider_site, provider)) = provider {
                provisional_providers.push(LinkedSharedStateProviderOccurrence {
                    host: parent_site,
                    provider_site,
                    child: child_candidate,
                    type_key: provider.type_key.clone(),
                    schema_abi: provider.schema_abi,
                });
                active_providers.push(ActiveSharedStateProvider {
                    host: parent_site,
                    provider_site,
                    type_key: provider.type_key.clone(),
                    schema_abi: provider.schema_abi,
                });
            }
            record_shared_state_requirements(
                &components[target_index],
                child_candidate,
                &structural_path,
                &active_providers,
                &mut provisional_requirements,
                &mut missing,
            );
            stack.push(ExpansionFrame {
                component_index: target_index,
                site: child_candidate,
                next_edge: 0,
                provider_depth_before_entry,
            });
        }

        if missing.is_empty() {
            root_decisions.push(StandaloneRootDecision {
                definition: root_component.definition,
                component: root_component.semantic_key.clone(),
                disposition: StandaloneRootDisposition::Admitted {
                    candidate: root_candidate,
                },
            });
            sites.extend(provisional_sites);
            resolved_edges.extend(provisional_edges);
            shared_state_providers.extend(provisional_providers);
            shared_state_requirements.extend(provisional_requirements);
            node_ref_aliases.extend(provisional_node_ref_aliases);
        } else {
            root_decisions.push(StandaloneRootDecision {
                definition: root_component.definition,
                component: root_component.semantic_key.clone(),
                disposition: StandaloneRootDisposition::ContextRequired { missing },
            });
        }
    }

    Ok(ComponentSiteInstantiation {
        sites,
        resolved_edges,
        root_decisions,
        shared_state_providers,
        shared_state_requirements,
        node_ref_aliases,
    })
}

fn record_shared_state_requirements(
    component: &LinkedSemanticComponent,
    consumer: OwnerCandidateId,
    structural_path: &[ComponentEdgeId],
    active_providers: &[ActiveSharedStateProvider],
    requirements: &mut Vec<LinkedSharedStateProviderRequirement>,
    missing: &mut Vec<StandaloneContextRequirement>,
) {
    let DescriptorBody::Component(body) = &component.descriptor.body else {
        unreachable!("linked component collection contains component descriptors")
    };
    for (&reference, shared) in &body.shared_state {
        let provider = active_providers
            .iter()
            .rev()
            .find(|provider| provider.type_key == shared.type_key);
        match provider {
            Some(provider) if provider.schema_abi == shared.schema_abi => {
                requirements.push(LinkedSharedStateProviderRequirement {
                    consumer,
                    reference,
                    provider_host: provider.host,
                    provider_site: provider.provider_site,
                    type_key: shared.type_key.clone(),
                    schema_abi: shared.schema_abi,
                });
            }
            // Global schema validation runs before expansion, so a matching
            // public key can never carry a different schema here.
            Some(_) => unreachable!("global SharedState schema validation precedes expansion"),
            None => missing.push(StandaloneContextRequirement::SharedState(
                StandaloneSharedStateRequirement {
                    consumer: component.semantic_key.clone(),
                    structural_path: structural_path.to_vec(),
                    reference,
                    type_key: shared.type_key.clone(),
                    schema_abi: shared.schema_abi,
                },
            )),
        }
    }
}

fn record_standalone_node_ref_requirements(
    component: &LinkedSemanticComponent,
    structural_path: &[ComponentEdgeId],
    missing: &mut Vec<StandaloneContextRequirement>,
) {
    let DescriptorBody::Component(body) = &component.descriptor.body else {
        unreachable!("linked component collection contains component descriptors")
    };
    for node_ref in &component.codegen.inbound_node_refs {
        let field = &body.fields[&node_ref.field_id];
        missing.push(StandaloneContextRequirement::NodeRef(
            StandaloneNodeRefRequirement {
                consumer: component.semantic_key.clone(),
                structural_path: structural_path.to_vec(),
                field: node_ref.field_id,
                crossing_hook: field
                    .crossing_hook
                    .expect("validated inbound NodeRef has a crossing hook"),
                node_ref: node_ref.producer_node_ref_id,
                bind_target: node_ref.bind_target,
            },
        ));
    }
}

fn record_standalone_callback_requirements(
    component: &LinkedSemanticComponent,
    structural_path: &[ComponentEdgeId],
    missing: &mut Vec<StandaloneContextRequirement>,
) {
    let DescriptorBody::Component(body) = &component.descriptor.body else {
        unreachable!("linked component collection contains component descriptors")
    };
    for (&field_id, field) in &body.fields {
        if field.role != ComponentFieldRole::Callback {
            continue;
        }
        let shape = field
            .callback_shape
            .expect("validated inbound callback has a callback shape");
        if matches!(shape, ComponentCallbackShape::Optional) {
            // An absent optional callback is its standalone value. Generated
            // client dispatch already treats that value as a no-op, so this
            // field does not require an enclosing component occurrence.
            continue;
        }
        missing.push(StandaloneContextRequirement::Callback(
            StandaloneCallbackRequirement {
                consumer: component.semantic_key.clone(),
                structural_path: structural_path.to_vec(),
                field: field_id,
                crossing_hook: field
                    .crossing_hook
                    .expect("validated inbound callback has a crossing hook"),
                shape,
            },
        ));
    }
}

fn resolve_node_ref_aliases(
    parent: &LinkedSemanticComponent,
    parent_candidate: OwnerCandidateId,
    edge_id: ComponentEdgeId,
    child: &LinkedSemanticComponent,
    child_candidate: OwnerCandidateId,
) -> Result<Vec<LinkedNodeRefHandleAlias>, SemanticLinkError> {
    let DescriptorBody::Component(parent_body) = &parent.descriptor.body else {
        unreachable!("linked component collection contains component descriptors")
    };
    let DescriptorBody::Component(child_body) = &child.descriptor.body else {
        unreachable!("linked component collection contains component descriptors")
    };
    let direct = &parent_body.direct_components[&edge_id];
    let parent_aliases = parent
        .codegen
        .direct_node_ref_aliases
        .iter()
        .filter(|alias| alias.edge_id == edge_id)
        .collect::<Vec<_>>();

    let mut resolved_by_field = BTreeMap::<ComponentFieldId, LinkedNodeRefHandleAlias>::new();
    for alias in parent_aliases {
        let Some(binding) = direct.bindings.get(&alias.field_name) else {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "parent codegen names absent child field `{}`",
                    alias.field_name
                ),
            ));
        };
        if binding.source
            != (ComponentBindingSource::ClientHandle {
                value: alias.canonical_value,
            })
        {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "field `{}` is not the authenticated ClientHandle value {}",
                    alias.field_name,
                    alias.canonical_value.index()
                ),
            ));
        }

        let Some((&field_id, field)) = child_body
            .fields
            .iter()
            .find(|(_, field)| field.source_name == alias.field_name)
        else {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "resolved child has no NodeRef field named `{}`",
                    alias.field_name
                ),
            ));
        };
        if field.role != ComponentFieldRole::ClientHandle {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "resolved child field `{}` is {:?}, expected ClientHandle",
                    alias.field_name, field.role
                ),
            ));
        }
        let Some(child_codegen) = child
            .codegen
            .inbound_node_refs
            .iter()
            .find(|mapping| mapping.field_id == field_id)
        else {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "resolved child field `{}` has no authenticated inbound NodeRef mapping",
                    alias.field_name
                ),
            ));
        };
        let crossing_hook = field
            .crossing_hook
            .expect("validated ClientHandle field has a crossing hook");
        let child_value = field
            .client_value
            .expect("validated ClientHandle field has a client value");
        let linked = LinkedNodeRefHandleAlias {
            parent: parent_candidate,
            edge: edge_id,
            parent_node_ref: alias.producer_node_ref_id,
            parent_value: alias.canonical_value,
            child: child_candidate,
            field: field_id,
            crossing_hook,
            child_node_ref: child_codegen.producer_node_ref_id,
            child_value,
            bind_target: child_codegen.bind_target,
        };
        if resolved_by_field.insert(field_id, linked).is_some() {
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "child NodeRef field `{}` is supplied more than once",
                    alias.field_name
                ),
            ));
        }
    }

    for required in &child.codegen.inbound_node_refs {
        if !resolved_by_field.contains_key(&required.field_id) {
            let field = &child_body.fields[&required.field_id];
            return Err(invalid_node_ref_alias(
                parent,
                edge_id,
                child,
                format!(
                    "required child NodeRef field `{}` has no exact parent handle binding",
                    field.source_name
                ),
            ));
        }
    }

    Ok(resolved_by_field.into_values().collect())
}

fn invalid_node_ref_alias(
    parent: &LinkedSemanticComponent,
    edge: ComponentEdgeId,
    child: &LinkedSemanticComponent,
    reason: String,
) -> SemanticLinkError {
    SemanticLinkError::InvalidNodeRefHandleAlias {
        parent: parent.semantic_key.clone(),
        edge,
        child: child.semantic_key.clone(),
        reason,
    }
}

fn validate_global_shared_state_schemas(
    components: &[LinkedSemanticComponent],
) -> Result<(), SemanticLinkError> {
    let mut schemas = BTreeMap::<String, (SemanticHash, ComponentSemanticKey)>::new();
    for component in components {
        let DescriptorBody::Component(body) = &component.descriptor.body else {
            unreachable!("linked component collection contains component descriptors")
        };
        let declarations = body
            .shared_state
            .values()
            .map(|shared| (&shared.type_key, shared.schema_abi))
            .chain(
                body.shared_state_providers
                    .values()
                    .map(|provider| (&provider.type_key, provider.schema_abi)),
            );
        for (type_key, schema_abi) in declarations {
            if let Some((first_schema, first_component)) = schemas.get(type_key) {
                if *first_schema != schema_abi {
                    return Err(SemanticLinkError::SharedStateDeclarationConflict {
                        type_key: type_key.clone(),
                        first_schema: *first_schema,
                        first_component: first_component.clone(),
                        second_schema: schema_abi,
                        second_component: component.semantic_key.clone(),
                    });
                }
            } else {
                schemas.insert(
                    type_key.clone(),
                    (schema_abi, component.semantic_key.clone()),
                );
            }
        }
    }
    Ok(())
}

fn link_component_graph_with_lifecycle<'a>(
    instances: &[ResolvedComponentInstance<'a>],
    edges: &[ResolvedComponentEdge],
    sites: &[LinkedSemanticComponentSite],
) -> Result<ResolvedComponentGraph, ComponentGraphLinkError> {
    let mut graph = link_resolved_component_graph(instances, edges)?;
    let mut relations = graph
        .owner_input
        .relations
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for site in sites {
        if site.reactive_lifecycle_root.is_none() {
            continue;
        }
        let Some(parent) = site.parent else {
            continue;
        };
        relations.insert(OwnerRelation::new(
            parent,
            site.candidate,
            OwnerRelationKind::Coupling(OwnerCouplingKind::ReactiveLifecycle),
        ));
    }
    graph.owner_input.relations = relations.into_iter().collect();
    Ok(graph)
}

fn validate_node_ref_alias_owners(
    aliases: &[LinkedNodeRefHandleAlias],
    owners: &OwnerPartitionPlan,
) -> Result<(), (OwnerCandidateId, ComponentEdgeId, OwnerCandidateId)> {
    for alias in aliases {
        let parent_owner = owners.candidate_to_owner.get(&alias.parent);
        let child_owner = owners.candidate_to_owner.get(&alias.child);
        if parent_owner.is_none() || parent_owner != child_owner {
            return Err((alias.parent, alias.edge, alias.child));
        }
    }
    Ok(())
}

fn semantic_plan_payload(
    components: &[LinkedSemanticComponent],
    sites: &[LinkedSemanticComponentSite],
    helpers: &[LinkedSemanticHelper],
    reachable_helpers: &[HelperSemanticKey],
    owners: &OwnerPartitionPlan,
    component_inputs: &LinkedComponentInputs,
    shared_state: &[LinkedSharedStateSchema],
    root_decisions: &[StandaloneRootDecision],
    shared_state_providers: &[LinkedSharedStateProviderOccurrence],
    shared_state_requirements: &[LinkedSharedStateProviderRequirement],
    node_ref_aliases: &[LinkedNodeRefHandleAlias],
) -> Result<SemanticPlanPayload, SemanticPlanValidationError> {
    let components = components
        .iter()
        .map(|component| SemanticComponentMaterial {
            semantic_key: component.semantic_key.clone(),
            descriptor_hash: component.descriptor.semantic_hash,
            codegen: component.codegen.clone(),
            component_edges: component
                .component_edges
                .iter()
                .map(|edge| SemanticComponentEdgeMaterial {
                    edge: edge.edge,
                    target: edge.target.clone(),
                    lifecycle_control: edge.lifecycle_control,
                })
                .collect(),
            helper_edges: component
                .helper_edges
                .iter()
                .map(SemanticHelperEdgeMaterial::from)
                .collect(),
            helper_closure: component.helper_closure.clone(),
        })
        .collect();
    let sites = sites
        .iter()
        .map(|site| SemanticComponentSiteMaterial {
            candidate: site.candidate,
            root: site.root.clone(),
            component: site.component.clone(),
            parent: site.parent,
            incoming_edge: site.incoming_edge,
            structural_path: site.structural_path.clone(),
            incoming_lifecycle: site.incoming_lifecycle,
            reactive_lifecycle_root: site.reactive_lifecycle_root,
        })
        .collect();
    let helpers = helpers
        .iter()
        .map(|helper| {
            let DescriptorBody::Helper(body) = &helper.descriptor.body else {
                return Err(SemanticPlanValidationError::UnexpectedDescriptorKind {
                    definition: helper.definition,
                    expected: DescriptorKind::Helper,
                    actual: helper.descriptor.body.kind(),
                });
            };
            Ok(SemanticHelperMaterial {
                semantic_key: helper.semantic_key.clone(),
                descriptor_hash: helper.descriptor.semantic_hash,
                signature_hash: body.signature_hash,
                codegen: helper.codegen.clone(),
                helper_edges: helper
                    .helper_edges
                    .iter()
                    .map(SemanticHelperEdgeMaterial::from)
                    .collect(),
                transitive_closure: helper.transitive_closure.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SemanticPlanPayload {
        schema_version: SEMANTIC_LINK_SCHEMA_VERSION,
        components,
        sites,
        helpers,
        reachable_helpers: reachable_helpers.to_vec(),
        owners: owners.clone(),
        component_inputs: component_inputs.clone(),
        shared_state: shared_state.to_vec(),
        root_decisions: root_decisions.to_vec(),
        shared_state_providers: shared_state_providers.to_vec(),
        shared_state_requirements: shared_state_requirements.to_vec(),
        node_ref_aliases: node_ref_aliases.to_vec(),
    })
}

fn linked_helper_key(key: &HelperSemanticKey) -> Result<rsjs_ir::LinkedHelperKey, String> {
    rsjs_ir::LinkedHelperKey::from_canonical_bytes(canonical_bytes(key))
        .map_err(|error| format!("invalid canonical helper identity: {error}"))
}

fn validate_linked_helper_codegen_graph(helpers: &[LinkedSemanticHelper]) -> Result<(), String> {
    let mut roots = Vec::with_capacity(helpers.len());
    let mut definitions = Vec::with_capacity(helpers.len());

    for helper in helpers {
        let key = linked_helper_key(&helper.semantic_key)?;

        let mut params = Vec::with_capacity(helper.codegen.signature().params.len());
        for (param_index, param) in helper.codegen.signature().params.iter().enumerate() {
            let ty = rsjs_ir::parse_client_helper_type(
                &param.rust_type,
                rsjs_ir::ClientHelperTypePosition::Parameter,
            )
            .map_err(|error| {
                format!(
                    "helper {:?} parameter {param_index} has invalid type: {error}",
                    helper.semantic_key,
                )
            })?;
            params.push(rsjs_ir::ClientParam::new(
                format!("__rsjs_arg_{param_index}"),
                ty,
            ));
        }
        let return_ty = rsjs_ir::parse_client_helper_type(
            &helper.codegen.signature().return_type,
            rsjs_ir::ClientHelperTypePosition::Return,
        )
        .map_err(|error| {
            format!(
                "helper {:?} has invalid return type: {error}",
                helper.semantic_key,
            )
        })?;
        let deployable_signature = params
            .iter()
            .all(|param| !client_type_contains_value_object(&param.ty))
            && !client_type_contains_value_object(&return_ty);
        if deployable_signature {
            let root_edge = u32::try_from(roots.len())
                .map_err(|_| "linked helper graph exceeds the canonical u32 ID space".to_owned())?;
            roots.push(rsjs_ir::LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(root_edge),
                key.clone(),
            ));
        }
        let body = match helper.codegen.body() {
            HelperCodegenBody::Expr(expression) => {
                rsjs_ir::ClientBlock::from_stmts(vec![rsjs_ir::ClientStmt::Return(Some(
                    expression.clone(),
                ))])
            }
            HelperCodegenBody::Block(block) => block.clone(),
        };
        let calls = helper
            .helper_edges
            .iter()
            .map(|edge| {
                linked_helper_key(&edge.target)
                    .map(|target| rsjs_ir::LinkedHelperCallBinding::new(edge.call, target))
            })
            .collect::<Result<Vec<_>, String>>()?;
        definitions.push(rsjs_ir::LinkedHelperDefinition::new(
            key, params, return_ty, body, calls,
        ));
    }

    if !helpers.is_empty() && roots.is_empty() {
        return Err(
            "linked helper graph has no transport-safe deployable root; helper-internal value objects must be reachable from a scalar public helper"
                .to_owned(),
        );
    }

    // Descriptor extraction sees every private generated field/method helper,
    // including helpers no transport-safe root calls. Validate only the
    // closure reachable from deployable signatures; unreachable private
    // object definitions are deliberately absent from final codegen.
    let mut reachable = roots
        .iter()
        .map(|root| root.target.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
    while let Some(key) = pending.pop() {
        let Some(definition) = definitions.iter().find(|definition| definition.key == key) else {
            continue;
        };
        for call in &definition.calls {
            if reachable.insert(call.target.clone()) {
                pending.push(call.target.clone());
            }
        }
    }
    definitions.retain(|definition| reachable.contains(&definition.key));

    rsjs_ir::LinkedHelperProgram::new(roots, definitions)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn client_type_contains_value_object(ty: &rsjs_ir::ClientType) -> bool {
    match ty {
        rsjs_ir::ClientType::ValueObject { .. } => true,
        rsjs_ir::ClientType::Option(inner) | rsjs_ir::ClientType::Vec(inner) => {
            client_type_contains_value_object(inner)
        }
        _ => false,
    }
}

/// Recompute every strong identity carried by an owned semantic plan.
pub fn validate_semantic_plan(plan: &SemanticLinkPlan) -> Result<(), SemanticPlanValidationError> {
    if plan.material.link_unit != plan.link_unit {
        return Err(SemanticPlanValidationError::LinkUnitMismatch {
            expected: plan.link_unit,
            actual: plan.material.link_unit,
        });
    }

    for component in &plan.components {
        let DescriptorBody::Component(body) = &component.descriptor.body else {
            return Err(SemanticPlanValidationError::UnexpectedDescriptorKind {
                definition: component.definition,
                expected: DescriptorKind::Component,
                actual: component.descriptor.body.kind(),
            });
        };
        if body.semantic_key != component.semantic_key {
            return Err(SemanticPlanValidationError::ComponentKeyMismatch {
                definition: component.definition,
            });
        }
        validate_component_codegen_bridge(&component.codegen, body).map_err(|reason| {
            SemanticPlanValidationError::InvalidComponentCodegen {
                definition: component.definition,
                reason,
            }
        })?;
        validate_plan_descriptor_hash(component.definition, &component.descriptor)?;
    }
    for helper in &plan.helpers {
        let DescriptorBody::Helper(body) = &helper.descriptor.body else {
            return Err(SemanticPlanValidationError::UnexpectedDescriptorKind {
                definition: helper.definition,
                expected: DescriptorKind::Helper,
                actual: helper.descriptor.body.kind(),
            });
        };
        if body.semantic_key != helper.semantic_key {
            return Err(SemanticPlanValidationError::HelperKeyMismatch {
                definition: helper.definition,
            });
        }
        validate_helper_codegen_bridge(helper.definition, &helper.codegen, body).map_err(
            |reason| SemanticPlanValidationError::InvalidHelperCodegen {
                definition: helper.definition,
                reason,
            },
        )?;
        validate_plan_descriptor_hash(helper.definition, &helper.descriptor)?;
    }
    validate_linked_helper_codegen_graph(&plan.helpers)
        .map_err(|reason| SemanticPlanValidationError::InvalidHelperProgram { reason })?;
    let component_by_definition = plan
        .components
        .iter()
        .enumerate()
        .map(|(index, component)| (component.definition, index))
        .collect::<BTreeMap<_, _>>();
    // Validate the stored site graph before comparing it with a fresh
    // provider-aware expansion. This preserves focused diagnostics for
    // malformed parent/edge/lifecycle metadata; the broader topology mismatch
    // below is reserved for coherent sites paired with stale root/provider
    // decisions.
    validate_component_sites(&plan.components, &plan.sites)?;
    let rebuilt_sites = instantiate_component_sites(&plan.components, &component_by_definition)
        .map_err(
            |error| SemanticPlanValidationError::InvalidSharedStateProviderTopology {
                reason: error.to_string(),
            },
        )?;
    if rebuilt_sites.sites != plan.sites
        || rebuilt_sites.root_decisions != plan.root_decisions
        || rebuilt_sites.shared_state_providers != plan.shared_state_providers
        || rebuilt_sites.shared_state_requirements != plan.shared_state_requirements
    {
        return Err(SemanticPlanValidationError::SharedStateProviderTopologyMismatch);
    }
    if rebuilt_sites.node_ref_aliases != plan.node_ref_aliases {
        return Err(SemanticPlanValidationError::NodeRefAliasTopologyMismatch);
    }
    let (component_inputs, owners, shared_state) =
        relink_plan_graphs(&plan.components, &plan.sites)?;
    if component_inputs != plan.component_inputs {
        return Err(SemanticPlanValidationError::ComponentInputsMismatch);
    }
    if owners != plan.owners {
        return Err(SemanticPlanValidationError::OwnerPlanMismatch);
    }
    validate_node_ref_alias_owners(&plan.node_ref_aliases, &plan.owners).map_err(
        |(parent, edge, child)| SemanticPlanValidationError::NodeRefAliasOwnerMismatch {
            parent,
            edge,
            child,
        },
    )?;
    if shared_state != plan.shared_state {
        return Err(SemanticPlanValidationError::SharedStatePlanMismatch);
    }

    let component_hashes = plan
        .components
        .iter()
        .map(|component| component.descriptor.semantic_hash)
        .collect::<Vec<_>>();
    if component_hashes != plan.material.component_descriptors {
        return Err(SemanticPlanValidationError::ComponentDescriptorHashesMismatch);
    }
    let helper_hashes = plan
        .helpers
        .iter()
        .map(|helper| helper.descriptor.semantic_hash)
        .collect::<Vec<_>>();
    if helper_hashes != plan.material.helper_descriptors {
        return Err(SemanticPlanValidationError::HelperDescriptorHashesMismatch);
    }

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
    )?;
    let payload_hash = SemanticHash::derive(&payload);
    if payload_hash != plan.material.payload {
        return Err(SemanticPlanValidationError::PayloadHashMismatch {
            stored: plan.material.payload,
            recomputed: payload_hash,
        });
    }
    let plan_id = plan.material.plan_id();
    if plan_id != plan.plan_id {
        return Err(SemanticPlanValidationError::PlanIdMismatch {
            stored: plan.plan_id,
            recomputed: plan_id,
        });
    }
    Ok(())
}

fn has_local_browser_obligation(descriptor: &ComponentDescriptor) -> bool {
    !descriptor.effects.is_empty()
        && descriptor
            .origins
            .values()
            .copied()
            .any(OriginKind::requires_client_permission)
}

fn relink_plan_graphs(
    components: &[LinkedSemanticComponent],
    sites: &[LinkedSemanticComponentSite],
) -> Result<
    (
        LinkedComponentInputs,
        OwnerPartitionPlan,
        Vec<LinkedSharedStateSchema>,
    ),
    SemanticPlanValidationError,
> {
    let components_by_definition = components
        .iter()
        .map(|component| (component.definition, component))
        .collect::<BTreeMap<_, _>>();
    let instances = sites
        .iter()
        .map(|site| {
            let component = components_by_definition
                .get(&site.definition)
                .expect("component sites are validated before graph relinking");
            let DescriptorBody::Component(descriptor) = &component.descriptor.body else {
                unreachable!("component collection contains component descriptors")
            };
            ResolvedComponentInstance {
                id: site.candidate,
                descriptor,
                has_browser_obligation: has_local_browser_obligation(descriptor),
            }
        })
        .collect::<Vec<_>>();
    let edges = sites
        .iter()
        .filter_map(|site| match (site.parent, site.incoming_edge) {
            (Some(parent), Some(edge)) => Some(ResolvedComponentEdge {
                parent,
                edge,
                child: site.candidate,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let graph = link_component_graph_with_lifecycle(&instances, &edges, sites)
        .map_err(SemanticPlanValidationError::InvalidComponentInputGraph)?;
    let owners = partition_owners(&graph.owner_input.candidates, &graph.owner_input.relations)
        .map_err(ComponentGraphLinkError::from)
        .map_err(SemanticPlanValidationError::InvalidComponentInputGraph)?;
    let shared_state = link_shared_state_schemas(&instances)
        .map_err(|_| SemanticPlanValidationError::SharedStatePlanMismatch)?;
    Ok((graph.inputs, owners, shared_state))
}

fn validate_component_sites(
    components: &[LinkedSemanticComponent],
    sites: &[LinkedSemanticComponentSite],
) -> Result<(), SemanticPlanValidationError> {
    let components_by_definition = components
        .iter()
        .map(|component| (component.definition, component))
        .collect::<BTreeMap<_, _>>();

    for (index, site) in sites.iter().enumerate() {
        let expected = OwnerCandidateId::new(index as u32);
        if site.candidate != expected {
            return Err(SemanticPlanValidationError::NonDenseComponentSite {
                expected,
                actual: site.candidate,
            });
        }
        let Some(component) = components_by_definition.get(&site.definition) else {
            return Err(
                SemanticPlanValidationError::UnknownComponentSiteDefinition {
                    candidate: site.candidate,
                    definition: site.definition,
                    component: site.component.clone(),
                },
            );
        };
        if component.semantic_key != site.component {
            return Err(
                SemanticPlanValidationError::UnknownComponentSiteDefinition {
                    candidate: site.candidate,
                    definition: site.definition,
                    component: site.component.clone(),
                },
            );
        }

        match (site.parent, site.incoming_edge) {
            (None, None) => {
                let DescriptorBody::Component(body) = &component.descriptor.body else {
                    unreachable!("linked component collection contains component descriptors")
                };
                if !body.root_eligible
                    || site.root != site.component
                    || !site.structural_path.is_empty()
                    || site.incoming_lifecycle.is_some()
                    || site.reactive_lifecycle_root.is_some()
                {
                    return Err(SemanticPlanValidationError::InvalidRootComponentSite {
                        candidate: site.candidate,
                    });
                }
            }
            (Some(parent), Some(incoming_edge)) => {
                let Some(parent_site) = sites.get(parent.index() as usize) else {
                    return Err(SemanticPlanValidationError::InvalidChildComponentSite {
                        candidate: site.candidate,
                    });
                };
                let Some(parent_component) = components_by_definition.get(&parent_site.definition)
                else {
                    return Err(SemanticPlanValidationError::InvalidChildComponentSite {
                        candidate: site.candidate,
                    });
                };
                let expected_path = parent_site
                    .structural_path
                    .iter()
                    .copied()
                    .chain(std::iter::once(incoming_edge))
                    .collect::<Vec<_>>();
                let resolved_edge = parent_component
                    .component_edges
                    .iter()
                    .find(|edge| edge.edge == incoming_edge);
                let resolved_target = resolved_edge.map(|edge| edge.target_definition);
                let incoming_lifecycle = resolved_edge.and_then(|edge| edge.lifecycle_control);
                let DescriptorBody::Component(parent_body) = &parent_component.descriptor.body
                else {
                    unreachable!("linked component collection contains component descriptors")
                };
                let descriptor_lifecycle = parent_body
                    .direct_components
                    .get(&incoming_edge)
                    .and_then(|edge| edge.lifecycle_control);
                if let (Some(_), Some(reactive_lifecycle_root)) =
                    (incoming_lifecycle, parent_site.reactive_lifecycle_root)
                {
                    return Err(
                        SemanticPlanValidationError::NestedConditionalComponentSite {
                            candidate: site.candidate,
                            reactive_lifecycle_root,
                        },
                    );
                }
                let expected_lifecycle_root = if incoming_lifecycle.is_some() {
                    Some(site.candidate)
                } else {
                    parent_site.reactive_lifecycle_root
                };
                if parent >= site.candidate
                    || parent_site.root != site.root
                    || expected_path != site.structural_path
                    || resolved_target != Some(site.definition)
                    || incoming_lifecycle != descriptor_lifecycle
                    || site.incoming_lifecycle != incoming_lifecycle
                    || site.reactive_lifecycle_root != expected_lifecycle_root
                {
                    return Err(SemanticPlanValidationError::InvalidChildComponentSite {
                        candidate: site.candidate,
                    });
                }
            }
            _ => {
                return Err(SemanticPlanValidationError::InvalidChildComponentSite {
                    candidate: site.candidate,
                });
            }
        }
    }
    Ok(())
}

fn validate_plan_descriptor_hash(
    definition: TransportDefinitionId,
    descriptor: &DescriptorEnvelope,
) -> Result<(), SemanticPlanValidationError> {
    let refreshed = DescriptorEnvelope::new(
        descriptor.header.clone(),
        descriptor.body.clone(),
        descriptor.debug.clone(),
    )
    .map_err(
        |source| SemanticPlanValidationError::InvalidCanonicalDescriptor { definition, source },
    )?;
    if refreshed.semantic_hash != descriptor.semantic_hash {
        return Err(SemanticPlanValidationError::DescriptorHashMismatch {
            definition,
            stored: descriptor.semantic_hash,
            recomputed: refreshed.semantic_hash,
        });
    }
    Ok(())
}

/// Serialize a validated semantic plan into deterministic external JSON.
///
/// Semantic keys and descriptor ABI records are written as canonical hex
/// rather than reconstructed source paths. Exact definition coordinates remain
/// explicit so the final rustc wrapper can select the correct plan record.
pub fn semantic_plan_json(
    plan: &SemanticLinkPlan,
) -> Result<Value, SemanticPlanSerializationError> {
    validate_semantic_plan(plan)?;

    let components = plan
        .components
        .iter()
        .map(|component| {
            let DescriptorBody::Component(body) = &component.descriptor.body else {
                unreachable!("linked component collection contains component descriptors")
            };
            serde_json::json!({
                "definition": definition_json(component.definition),
                "semantic_key_canonical": canonical_hex(&component.semantic_key),
                "root_eligible": body.root_eligible,
                "descriptor_hash": hash_json(component.descriptor.semantic_hash),
                "descriptor_semantic_abi": hex_bytes(&component.descriptor.semantic_bytes()),
                "component_edges": component.component_edges.iter().map(|edge| {
                    serde_json::json!({
                        "edge_id": edge.edge.index(),
                        "witness_id": edge.witness.index(),
                        "source_edge_id": hex_bytes(&edge.source_edge),
                        "target_definition": definition_json(edge.target_definition),
                        "target_semantic_key_canonical": canonical_hex(&edge.target),
                        "lifecycle": component_lifecycle_json(edge.lifecycle_control),
                    })
                }).collect::<Vec<_>>(),
                "helper_edges": component.helper_edges.iter().map(helper_edge_json)
                    .collect::<Vec<_>>(),
                "helper_closure": component.helper_closure.iter()
                    .map(canonical_hex)
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let component_codegen = plan
        .components
        .iter()
        .map(|component| component_codegen_json(component.definition, &component.codegen))
        .collect::<Vec<_>>();
    let helper_codegen = plan
        .helpers
        .iter()
        .map(|helper| helper_codegen_json(helper.definition, &helper.semantic_key, &helper.codegen))
        .collect::<Vec<_>>();
    let sites = plan
        .sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "candidate_id": site.candidate.index(),
                "root_semantic_key_canonical": canonical_hex(&site.root),
                "component_semantic_key_canonical": canonical_hex(&site.component),
                "definition": definition_json(site.definition),
                "parent_candidate_id": site.parent.map(OwnerCandidateId::index),
                "incoming_edge_id": site.incoming_edge.map(ComponentEdgeId::index),
                "structural_path": site.structural_path.iter()
                    .map(|edge| edge.index())
                    .collect::<Vec<_>>(),
                "incoming_lifecycle": site.parent.map(|_| {
                    component_lifecycle_json(site.incoming_lifecycle)
                }),
                "reactive_lifecycle_root_candidate_id": site.reactive_lifecycle_root
                    .map(OwnerCandidateId::index),
            })
        })
        .collect::<Vec<_>>();
    let helpers = plan
        .helpers
        .iter()
        .map(|helper| {
            let DescriptorBody::Helper(body) = &helper.descriptor.body else {
                unreachable!("validated helper descriptor kind")
            };
            serde_json::json!({
                "definition": definition_json(helper.definition),
                "semantic_key_canonical": canonical_hex(&helper.semantic_key),
                "descriptor_hash": hash_json(helper.descriptor.semantic_hash),
                "descriptor_semantic_abi": hex_bytes(&helper.descriptor.semantic_bytes()),
                "signature_hash": hash_json(body.signature_hash),
                "client_ir_hash": hash_json(body.client_ir_hash),
                "helper_edges": helper.helper_edges.iter().map(helper_edge_json)
                    .collect::<Vec<_>>(),
                "transitive_closure": helper.transitive_closure.iter()
                    .map(canonical_hex)
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let owners = plan
        .owners
        .owners
        .iter()
        .map(|owner| {
            Ok(serde_json::json!({
                "owner_id": owner.id.index(),
                "candidates": owner.candidates.iter()
                    .map(|candidate| candidate.index())
                    .collect::<Vec<_>>(),
                "strategy": owner_strategy_json(owner.id.index(), &owner.strategy)?,
            }))
        })
        .collect::<Result<Vec<_>, SemanticPlanSerializationError>>()?;
    let candidate_to_owner = plan
        .owners
        .candidate_to_owner
        .iter()
        .map(|(candidate, owner)| {
            serde_json::json!({
                "candidate_id": candidate.index(),
                "owner_id": owner.index(),
            })
        })
        .collect::<Vec<_>>();
    let component_inputs = component_inputs_json(&plan.component_inputs)?;
    let shared_state = plan
        .shared_state
        .iter()
        .map(|schema| {
            serde_json::json!({
                "type_key": schema.type_key,
                "schema_abi": hash_json(schema.schema_abi),
                "consumers": schema.consumers.iter().map(|consumer| {
                    serde_json::json!({
                        "candidate_id": consumer.component.index(),
                        "reference_id": consumer.reference.index(),
                        "origin_id": consumer.origin.index(),
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let root_decisions = plan
        .root_decisions
        .iter()
        .map(|decision| {
            let disposition = match &decision.disposition {
                StandaloneRootDisposition::StructuralIncomingOnly => {
                    serde_json::json!({ "kind": "structural_incoming_only" })
                }
                StandaloneRootDisposition::Admitted { candidate } => serde_json::json!({
                    "kind": "admitted",
                    "candidate_id": candidate.index(),
                }),
                StandaloneRootDisposition::ContextRequired { missing } => serde_json::json!({
                    "kind": "context_required",
                    "missing": missing.iter().map(|requirement| {
                        standalone_context_requirement_json(requirement)
                    }).collect::<Vec<_>>(),
                }),
            };
            serde_json::json!({
                "definition": definition_json(decision.definition),
                "component_semantic_key_canonical": canonical_hex(&decision.component),
                "disposition": disposition,
            })
        })
        .collect::<Vec<_>>();
    let shared_state_providers = plan
        .shared_state_providers
        .iter()
        .map(|provider| {
            serde_json::json!({
                "host_candidate_id": provider.host.index(),
                "provider_site_id": provider.provider_site.index(),
                "child_candidate_id": provider.child.index(),
                "type_key": provider.type_key,
                "schema_abi": hash_json(provider.schema_abi),
            })
        })
        .collect::<Vec<_>>();
    let shared_state_requirements = plan
        .shared_state_requirements
        .iter()
        .map(|requirement| {
            serde_json::json!({
                "consumer_candidate_id": requirement.consumer.index(),
                "reference_id": requirement.reference.index(),
                "provider_host_candidate_id": requirement.provider_host.index(),
                "provider_site_id": requirement.provider_site.index(),
                "type_key": requirement.type_key,
                "schema_abi": hash_json(requirement.schema_abi),
            })
        })
        .collect::<Vec<_>>();
    let node_ref_aliases = plan
        .node_ref_aliases
        .iter()
        .map(|alias| {
            serde_json::json!({
                "parent_candidate_id": alias.parent.index(),
                "edge_id": alias.edge.index(),
                "parent_node_ref_id": alias.parent_node_ref,
                "parent_value_id": alias.parent_value.index(),
                "child_candidate_id": alias.child.index(),
                "field_id": alias.field.index(),
                "crossing_hook_id": alias.crossing_hook.index(),
                "child_node_ref_id": alias.child_node_ref,
                "child_value_id": alias.child_value.index(),
                "bind_target": node_ref_bind_target_json(alias.bind_target),
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "format": SEMANTIC_PLAN_FORMAT,
        "schema_version": SEMANTIC_LINK_SCHEMA_VERSION,
        "link_unit_id": plan.link_unit.to_string(),
        "plan_id": plan.plan_id.to_string(),
        "payload_hash": hash_json(plan.material.payload),
        "component_descriptor_hashes": plan.material.component_descriptors.iter()
            .copied()
            .map(hash_json)
            .collect::<Vec<_>>(),
        "helper_descriptor_hashes": plan.material.helper_descriptors.iter()
            .copied()
            .map(hash_json)
            .collect::<Vec<_>>(),
        "components": components,
        "component_codegen": component_codegen,
        "component_sites": sites,
        "helpers": helpers,
        "helper_codegen": helper_codegen,
        "reachable_helpers": plan.reachable_helpers.iter()
            .map(canonical_hex)
            .collect::<Vec<_>>(),
        "owners": owners,
        "candidate_to_owner": candidate_to_owner,
        "component_inputs": component_inputs,
        "shared_state": shared_state,
        "standalone_roots": root_decisions,
        "shared_state_providers": shared_state_providers,
        "shared_state_requirements": shared_state_requirements,
        "node_ref_aliases": node_ref_aliases,
    }))
}

fn standalone_context_requirement_json(requirement: &StandaloneContextRequirement) -> Value {
    match requirement {
        StandaloneContextRequirement::SharedState(requirement) => serde_json::json!({
            "kind": "shared_state",
            "consumer_semantic_key_canonical": canonical_hex(&requirement.consumer),
            "structural_path": requirement.structural_path.iter()
                .map(|edge| edge.index())
                .collect::<Vec<_>>(),
            "reference_id": requirement.reference.index(),
            "type_key": requirement.type_key,
            "schema_abi": hash_json(requirement.schema_abi),
        }),
        StandaloneContextRequirement::NodeRef(requirement) => serde_json::json!({
            "kind": "node_ref",
            "consumer_semantic_key_canonical": canonical_hex(&requirement.consumer),
            "structural_path": requirement.structural_path.iter()
                .map(|edge| edge.index())
                .collect::<Vec<_>>(),
            "field_id": requirement.field.index(),
            "crossing_hook_id": requirement.crossing_hook.index(),
            "node_ref_id": requirement.node_ref,
            "bind_target": node_ref_bind_target_json(requirement.bind_target),
        }),
        StandaloneContextRequirement::Callback(requirement) => serde_json::json!({
            "kind": "callback",
            "consumer_semantic_key_canonical": canonical_hex(&requirement.consumer),
            "structural_path": requirement.structural_path.iter()
                .map(|edge| edge.index())
                .collect::<Vec<_>>(),
            "field_id": requirement.field.index(),
            "crossing_hook_id": requirement.crossing_hook.index(),
            "callback_shape": match requirement.shape {
                ComponentCallbackShape::Required => "required",
                ComponentCallbackShape::Optional => "optional",
            },
        }),
    }
}

fn node_ref_bind_target_json(target: Option<ComponentNodeRefBindTarget>) -> Value {
    match target {
        None => Value::Null,
        Some(ComponentNodeRefBindTarget::Element) => Value::String("element".to_owned()),
        Some(ComponentNodeRefBindTarget::HtmlElement) => Value::String("html_element".to_owned()),
        Some(ComponentNodeRefBindTarget::Input) => Value::String("input".to_owned()),
        Some(ComponentNodeRefBindTarget::TextArea) => Value::String("textarea".to_owned()),
        Some(ComponentNodeRefBindTarget::Select) => Value::String("select".to_owned()),
        Some(ComponentNodeRefBindTarget::Form) => Value::String("form".to_owned()),
        Some(ComponentNodeRefBindTarget::Dialog) => Value::String("dialog".to_owned()),
    }
}

fn component_lifecycle_json(lifecycle: Option<ComponentLifecycleControl>) -> Value {
    match lifecycle {
        None => serde_json::json!({
            "kind": "static_single",
        }),
        Some(lifecycle) => serde_json::json!({
            "kind": "conditional_single",
            "control": lifecycle.control.index(),
            "predicate_value": lifecycle.predicate.index(),
            "predicate_origin": lifecycle.predicate_origin.index(),
            "branch": match lifecycle.branch {
                ComponentLifecycleBranch::Then => "then",
                ComponentLifecycleBranch::Else => "else",
            },
        }),
    }
}

fn component_codegen_json(
    definition: TransportDefinitionId,
    codegen: &ComponentCodegenBridge,
) -> Value {
    let fragment_root = match codegen.fragment_root {
        ComponentFragmentRoot::ExactOneElement => {
            serde_json::json!({ "kind": "exact_one_element" })
        }
        ComponentFragmentRoot::Unsupported => {
            serde_json::json!({ "kind": "unsupported" })
        }
    };
    serde_json::json!({
        "definition": definition_json(definition),
        "asset_id": codegen.asset_id,
        "fragment_key": codegen.fragment_key,
        "fragment_root": fragment_root,
        "inbound_fields": codegen.inbound_fields.iter().map(|mapping| {
            serde_json::json!({
                "field_id": mapping.field_id.index(),
                "producer_value_id": mapping.producer_value_id,
                "canonical_value_id": mapping.canonical_value.index(),
                "crossing_hook_id": mapping.crossing_hook_id.index(),
                "callback_slot_id": mapping.callback_slot_id,
            })
        }).collect::<Vec<_>>(),
        "inbound_targets": codegen.inbound_targets.iter().map(|mapping| {
            serde_json::json!({
                "prop_site_id": mapping.prop_site_id.index(),
                "field_id": mapping.field_id.index(),
                "producer_signal_id": mapping.producer_signal_id,
                "canonical_signal_value_id": mapping.canonical_signal_value.index(),
            })
        }).collect::<Vec<_>>(),
        "signals": codegen.signals.iter().map(|mapping| {
            serde_json::json!({
                "producer_signal_id": mapping.producer_signal_id,
                "canonical_value_id": mapping.canonical_value.index(),
            })
        }).collect::<Vec<_>>(),
        "expressions": codegen.expressions.iter().map(|mapping| {
            serde_json::json!({
                "producer_expression_id": mapping.producer_expression_id,
                "canonical_value_id": mapping.canonical_value.index(),
            })
        }).collect::<Vec<_>>(),
        "payload_handlers": codegen.payload_handlers.iter().map(|mapping| {
            serde_json::json!({
                "prelude_local_index": mapping.prelude_local_index,
                "canonical_value_id": mapping.canonical_value.index(),
            })
        }).collect::<Vec<_>>(),
        "callback_slots": codegen.callback_slots.iter().map(|mapping| {
            serde_json::json!({
                "producer_callback_slot_id": mapping.producer_callback_slot_id,
                "canonical_value_id": mapping.canonical_value.index(),
            })
        }).collect::<Vec<_>>(),
        "inbound_node_refs": codegen.inbound_node_refs.iter().map(|mapping| {
            serde_json::json!({
                "field_id": mapping.field_id.index(),
                "producer_node_ref_id": mapping.producer_node_ref_id,
                "bind_target": node_ref_bind_target_json(mapping.bind_target),
            })
        }).collect::<Vec<_>>(),
        "direct_node_ref_aliases": codegen.direct_node_ref_aliases.iter().map(|mapping| {
            serde_json::json!({
                "edge_id": mapping.edge_id.index(),
                "field_name": mapping.field_name,
                "producer_node_ref_id": mapping.producer_node_ref_id,
                "canonical_value_id": mapping.canonical_value.index(),
            })
        }).collect::<Vec<_>>(),
        "uses_queries": codegen.uses_queries,
        "uses_mutations": codegen.uses_mutations,
    })
}

fn helper_codegen_json(
    definition: TransportDefinitionId,
    semantic_key: &HelperSemanticKey,
    codegen: &HelperCodegenBridge,
) -> Value {
    serde_json::json!({
        "definition": definition_json(definition),
        "semantic_key_canonical": canonical_hex(semantic_key),
        "signature": codegen.signature().canonical_json(),
        "source_ir_hash": hash_json(codegen.source_ir_hash()),
        "body_ir_hash": hash_json(codegen.body_ir_hash()),
        "body_ir": codegen.body().canonical_json(),
    })
}

fn component_inputs_json(
    inputs: &LinkedComponentInputs,
) -> Result<Value, SemanticPlanSerializationError> {
    let live_props = inputs
        .live_props
        .iter()
        .map(|input| {
            let temporal = match input.temporal {
                TemporalPropMode::LiveRead => serde_json::json!({
                    "kind": "live_read",
                }),
                TemporalPropMode::SeedOnce(seed_site) => serde_json::json!({
                    "kind": "seed_once",
                    "seed_site_id": seed_site.index(),
                }),
                TemporalPropMode::EffectDependency => serde_json::json!({
                    "kind": "effect_dependency",
                }),
                TemporalPropMode::QueryKeyDependency => serde_json::json!({
                    "kind": "query_key_dependency",
                }),
                TemporalPropMode::ForbiddenProviderSeed
                | TemporalPropMode::ForbiddenBoundaryCrossing => {
                    return Err(SemanticPlanSerializationError::UnsupportedTemporalMode {
                        parent: input.parent.index(),
                        edge: input.edge.index(),
                        site: input.site.index(),
                    });
                }
                _ => {
                    return Err(SemanticPlanSerializationError::UnsupportedTemporalMode {
                        parent: input.parent.index(),
                        edge: input.edge.index(),
                        site: input.site.index(),
                    });
                }
            };
            Ok(serde_json::json!({
                "parent_candidate_id": input.parent.index(),
                "edge_id": input.edge.index(),
                "child_candidate_id": input.child.index(),
                "field_id": input.field.index(),
                "crossing_hook_id": input.crossing_hook.index(),
                "parent_value_id": input.parent_value.index(),
                "child_value_id": input.child_value.index(),
                "prop_site_id": input.site.index(),
                "temporal": temporal,
            }))
        })
        .collect::<Result<Vec<_>, SemanticPlanSerializationError>>()?;
    let live_callbacks = inputs
        .live_callbacks
        .iter()
        .map(|input| {
            let callback_shape = match input.shape {
                ComponentCallbackShape::Required => "required",
                ComponentCallbackShape::Optional => "optional",
            };
            let dispatch = match input.dispatch {
                LinkedCallbackDispatch::ResolveCurrent => "resolve_current",
                _ => {
                    return Err(
                        SemanticPlanSerializationError::UnsupportedCallbackDispatch {
                            parent: input.parent.index(),
                            edge: input.edge.index(),
                        },
                    );
                }
            };
            Ok(serde_json::json!({
                "parent_candidate_id": input.parent.index(),
                "edge_id": input.edge.index(),
                "child_candidate_id": input.child.index(),
                "field_id": input.field.index(),
                "crossing_hook_id": input.crossing_hook.index(),
                "parent_value_id": input.parent_value.index(),
                "child_value_id": input.child_value.index(),
                "callback_shape": callback_shape,
                "dispatch": dispatch,
            }))
        })
        .collect::<Result<Vec<_>, SemanticPlanSerializationError>>()?;

    Ok(serde_json::json!({
        "live_props": live_props,
        "live_callbacks": live_callbacks,
    }))
}

fn helper_edge_json(edge: &SemanticHelperEdge) -> Value {
    serde_json::json!({
        "call_id": edge.call.index(),
        "witness_id": edge.witness.index(),
        "source_edge_id": hex_bytes(&edge.source_edge),
        "target_definition": definition_json(edge.target_definition),
        "target_semantic_key_canonical": canonical_hex(&edge.target),
    })
}

fn owner_strategy_json(
    owner: u32,
    strategy: &OwnerActivationStrategy,
) -> Result<Value, SemanticPlanSerializationError> {
    match strategy {
        OwnerActivationStrategy::Eager => Ok(serde_json::json!({ "kind": "eager" })),
        OwnerActivationStrategy::Visible => Ok(serde_json::json!({ "kind": "visible" })),
        OwnerActivationStrategy::Idle => Ok(serde_json::json!({ "kind": "idle" })),
        OwnerActivationStrategy::Interaction { event } => Ok(serde_json::json!({
            "kind": "interaction",
            "event": event,
        })),
        OwnerActivationStrategy::Media { query } => Ok(serde_json::json!({
            "kind": "media",
            "query": query,
        })),
        _ => Err(SemanticPlanSerializationError::UnsupportedOwnerStrategy { owner }),
    }
}

fn definition_json(definition: TransportDefinitionId) -> Value {
    serde_json::json!({
        "cargo_unit_id": definition.unit.to_string(),
        "record_id": hex_bytes(&definition.record),
    })
}

fn hash_json(hash: SemanticHash) -> String {
    format!("sha256:{hash}")
}

fn canonical_hex<T: CanonicalEncode + ?Sized>(value: &T) -> String {
    hex_bytes(&canonical_bytes(value))
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Atomically persist a semantic plan under its strong [`PlanId`].
///
/// Existing byte-identical content is reused. Different content at the same
/// strong ID is a hard collision rather than an overwrite.
pub fn write_content_addressed_semantic_plan(
    directory: &Path,
    plan: &SemanticLinkPlan,
) -> Result<PathBuf, SemanticPlanWriteError> {
    if !directory.is_absolute() {
        return Err(SemanticPlanWriteError::RelativeDirectory(
            directory.to_path_buf(),
        ));
    }
    fs::create_dir_all(directory).map_err(|source| SemanticPlanWriteError::CreateDirectory {
        path: directory.to_path_buf(),
        source,
    })?;

    let value = semantic_plan_json(plan)?;
    let bytes = serde_json::to_vec_pretty(&value)?;
    let path = directory.join(format!("{}.{}", plan.plan_id, SEMANTIC_PLAN_FILE_SUFFIX));
    if path.exists() {
        let existing = fs::read(&path).map_err(|source| SemanticPlanWriteError::ReadExisting {
            path: path.clone(),
            source,
        })?;
        if existing == bytes {
            return Ok(path);
        }
        return Err(SemanticPlanWriteError::ContentCollision {
            plan_id: plan.plan_id,
            path,
        });
    }

    let nonce = SEMANTIC_PLAN_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
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
        .map_err(|source| SemanticPlanWriteError::CreateTemporary {
            path: temporary.clone(),
            source,
        })?;
    if let Err(source) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(SemanticPlanWriteError::WriteTemporary {
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
                fs::read(&path).map_err(|source| SemanticPlanWriteError::ReadExisting {
                    path: path.clone(),
                    source,
                })?;
            if existing == bytes {
                Ok(path)
            } else {
                Err(SemanticPlanWriteError::ContentCollision {
                    plan_id: plan.plan_id,
                    path,
                })
            }
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            Err(SemanticPlanWriteError::Commit {
                from: temporary,
                to: path,
                source,
            })
        }
    }
}

fn collect_components(
    results: &[AbiBridgeResult],
) -> Result<Vec<PendingComponent>, SemanticLinkError> {
    results
        .iter()
        .map(|result| {
            let bridge = require_canonical(result)?;
            let DescriptorBody::Component(component) = &bridge.envelope.body else {
                return Err(SemanticLinkError::UnexpectedDescriptorKind {
                    collection: "component",
                    expected: DescriptorKind::Component,
                    actual: bridge.envelope.body.kind(),
                });
            };
            let definition =
                defining_transport_id("component", component.defining_witness, bridge)?;
            validate_descriptor_hash("component", definition, &bridge.envelope)?;
            let Some(codegen) = bridge.component_codegen() else {
                return Err(SemanticLinkError::DescriptorCodegenKindMismatch {
                    descriptor_kind: "component",
                    definition,
                    codegen_kind: "helper",
                });
            };
            validate_component_codegen_bridge(codegen, component).map_err(|reason| {
                SemanticLinkError::InvalidComponentCodegen { definition, reason }
            })?;
            Ok(PendingComponent {
                definition,
                bridge: bridge.clone(),
            })
        })
        .collect()
}

fn collect_helpers(results: &[AbiBridgeResult]) -> Result<Vec<PendingHelper>, SemanticLinkError> {
    results
        .iter()
        .map(|result| {
            let bridge = require_canonical(result)?;
            let DescriptorBody::Helper(helper) = &bridge.envelope.body else {
                return Err(SemanticLinkError::UnexpectedDescriptorKind {
                    collection: "helper",
                    expected: DescriptorKind::Helper,
                    actual: bridge.envelope.body.kind(),
                });
            };
            let definition = defining_transport_id("helper", helper.defining_witness, bridge)?;
            validate_descriptor_hash("helper", definition, &bridge.envelope)?;
            let Some(codegen) = bridge.helper_codegen() else {
                return Err(SemanticLinkError::DescriptorCodegenKindMismatch {
                    descriptor_kind: "helper",
                    definition,
                    codegen_kind: "component",
                });
            };
            validate_helper_codegen_bridge(definition, codegen, helper)
                .map_err(|reason| SemanticLinkError::InvalidHelperCodegen { definition, reason })?;
            Ok(PendingHelper {
                definition,
                bridge: bridge.clone(),
            })
        })
        .collect()
}

fn require_canonical(
    result: &AbiBridgeResult,
) -> Result<&CanonicalDescriptorBridge, SemanticLinkError> {
    match result {
        AbiBridgeResult::Canonical(bridge) => Ok(bridge),
        AbiBridgeResult::Unsupported(descriptor) => {
            Err(SemanticLinkError::Unsupported(descriptor.clone()))
        }
    }
}

fn defining_transport_id(
    kind: &'static str,
    witness: WitnessRecordId,
    bridge: &CanonicalDescriptorBridge,
) -> Result<TransportDefinitionId, SemanticLinkError> {
    match bridge.witnesses.get(&witness) {
        Some(ResolvedTransportWitness::Definition(definition)) => Ok(*definition),
        Some(actual) => Err(SemanticLinkError::DefiningWitnessKindMismatch {
            kind,
            witness,
            actual: witness_kind(actual),
        }),
        None => Err(SemanticLinkError::MissingDefiningWitness { kind, witness }),
    }
}

fn validate_descriptor_hash(
    kind: &'static str,
    definition: TransportDefinitionId,
    envelope: &DescriptorEnvelope,
) -> Result<(), SemanticLinkError> {
    let refreshed = DescriptorEnvelope::new(
        envelope.header.clone(),
        envelope.body.clone(),
        envelope.debug.clone(),
    )
    .map_err(|source| SemanticLinkError::InvalidCanonicalDescriptor {
        kind,
        definition,
        source,
    })?;
    if refreshed.semantic_hash != envelope.semantic_hash {
        return Err(SemanticLinkError::DescriptorHashMismatch {
            kind,
            definition,
            stored: envelope.semantic_hash,
            recomputed: refreshed.semantic_hash,
        });
    }
    Ok(())
}

fn index_component_definitions(
    components: &[PendingComponent],
) -> Result<BTreeMap<TransportDefinitionId, usize>, SemanticLinkError> {
    let mut definitions = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if definitions.insert(component.definition, index).is_some() {
            return Err(SemanticLinkError::DuplicateDefinition {
                definition: component.definition,
                first_kind: "component",
                second_kind: "component",
            });
        }
    }
    Ok(definitions)
}

fn index_helper_definitions(
    helpers: &[PendingHelper],
) -> Result<BTreeMap<TransportDefinitionId, usize>, SemanticLinkError> {
    let mut definitions = BTreeMap::new();
    for (index, helper) in helpers.iter().enumerate() {
        if definitions.insert(helper.definition, index).is_some() {
            return Err(SemanticLinkError::DuplicateDefinition {
                definition: helper.definition,
                first_kind: "helper",
                second_kind: "helper",
            });
        }
    }
    Ok(definitions)
}

fn validate_definition_kinds(
    components: &BTreeMap<TransportDefinitionId, usize>,
    helpers: &BTreeMap<TransportDefinitionId, usize>,
) -> Result<(), SemanticLinkError> {
    if let Some(definition) = components
        .keys()
        .find(|definition| helpers.contains_key(definition))
    {
        return Err(SemanticLinkError::DuplicateDefinition {
            definition: *definition,
            first_kind: "component",
            second_kind: "helper",
        });
    }
    Ok(())
}

fn validate_component_keys(components: &[PendingComponent]) -> Result<(), SemanticLinkError> {
    for pair in components.windows(2) {
        if pair[0].body().semantic_key == pair[1].body().semantic_key {
            return Err(SemanticLinkError::DuplicateComponentKey {
                key: pair[0].body().semantic_key.clone(),
                first: pair[0].definition,
                second: pair[1].definition,
            });
        }
    }
    Ok(())
}

fn validate_helper_keys(helpers: &[PendingHelper]) -> Result<(), SemanticLinkError> {
    for pair in helpers.windows(2) {
        if pair[0].body().semantic_key == pair[1].body().semantic_key {
            return Err(SemanticLinkError::DuplicateHelperKey {
                key: pair[0].body().semantic_key.clone(),
                first: pair[0].definition,
                second: pair[1].definition,
            });
        }
    }
    Ok(())
}

fn claim_witness(
    kind: &'static str,
    definition: TransportDefinitionId,
    witness: WitnessRecordId,
    claimed: &mut BTreeSet<WitnessRecordId>,
) -> Result<(), SemanticLinkError> {
    if !claimed.insert(witness) {
        return Err(SemanticLinkError::WitnessClaimedMultipleTimes {
            kind,
            definition,
            witness,
        });
    }
    Ok(())
}

fn expect_component_edge(
    definition: TransportDefinitionId,
    witness: WitnessRecordId,
    bridge: &CanonicalDescriptorBridge,
) -> Result<(TransportRecordId, TransportDefinitionId), SemanticLinkError> {
    match bridge.witnesses.get(&witness) {
        Some(ResolvedTransportWitness::ComponentEdge {
            source_edge,
            target,
        }) => Ok((*source_edge, *target)),
        Some(actual) => Err(SemanticLinkError::WitnessKindMismatch {
            kind: "component",
            definition,
            witness,
            expected: SemanticWitnessKind::ComponentEdge,
            actual: witness_kind(actual),
        }),
        None => Err(SemanticLinkError::MissingWitness {
            kind: "component",
            definition,
            witness,
            expected: SemanticWitnessKind::ComponentEdge,
        }),
    }
}

fn expect_component_field(
    definition: TransportDefinitionId,
    witness: WitnessRecordId,
    bridge: &CanonicalDescriptorBridge,
) -> Result<(TransportRecordId, TransportDefinitionId), SemanticLinkError> {
    match bridge.witnesses.get(&witness) {
        Some(ResolvedTransportWitness::ComponentField {
            field_record,
            target,
        }) => Ok((*field_record, *target)),
        Some(actual) => Err(SemanticLinkError::WitnessKindMismatch {
            kind: "component",
            definition,
            witness,
            expected: SemanticWitnessKind::ComponentField,
            actual: witness_kind(actual),
        }),
        None => Err(SemanticLinkError::MissingWitness {
            kind: "component",
            definition,
            witness,
            expected: SemanticWitnessKind::ComponentField,
        }),
    }
}

fn expect_helper_edge(
    kind: &'static str,
    definition: TransportDefinitionId,
    witness: WitnessRecordId,
    bridge: &CanonicalDescriptorBridge,
) -> Result<(TransportRecordId, TransportDefinitionId), SemanticLinkError> {
    match bridge.witnesses.get(&witness) {
        Some(ResolvedTransportWitness::HelperEdge {
            source_edge,
            target,
        }) => Ok((*source_edge, *target)),
        Some(actual) => Err(SemanticLinkError::WitnessKindMismatch {
            kind,
            definition,
            witness,
            expected: SemanticWitnessKind::HelperEdge,
            actual: witness_kind(actual),
        }),
        None => Err(SemanticLinkError::MissingWitness {
            kind,
            definition,
            witness,
            expected: SemanticWitnessKind::HelperEdge,
        }),
    }
}

fn ensure_complete_witness_coverage<T>(
    kind: &'static str,
    descriptor: &T,
    claimed: &BTreeSet<WitnessRecordId>,
) -> Result<(), SemanticLinkError>
where
    T: SemanticBridgeRecord,
{
    for (&witness, resolved) in &descriptor.bridge().witnesses {
        if !claimed.contains(&witness) {
            return Err(SemanticLinkError::UnclaimedWitness {
                kind,
                definition: descriptor.definition(),
                witness,
                actual: witness_kind(resolved),
            });
        }
    }
    Ok(())
}

trait SemanticBridgeRecord {
    fn definition(&self) -> TransportDefinitionId;
    fn bridge(&self) -> &CanonicalDescriptorBridge;
}

impl SemanticBridgeRecord for PendingComponent {
    fn definition(&self) -> TransportDefinitionId {
        self.definition
    }

    fn bridge(&self) -> &CanonicalDescriptorBridge {
        &self.bridge
    }
}

impl SemanticBridgeRecord for PendingHelper {
    fn definition(&self) -> TransportDefinitionId {
        self.definition
    }

    fn bridge(&self) -> &CanonicalDescriptorBridge {
        &self.bridge
    }
}

fn helper_closure_order(
    roots: &[usize],
    adjacency: &[Vec<usize>],
    helpers: &[PendingHelper],
    keys: &[HelperSemanticKey],
) -> Result<Vec<usize>, SemanticLinkError> {
    let mut roots = roots.to_vec();
    roots.sort_by(|&left, &right| keys[left].cmp(&keys[right]));
    roots.dedup();

    // 0 = unseen, 1 = visiting, 2 = emitted. A back edge to state 1 is a
    // supported helper cycle; its SCC members are emitted once while the
    // explicit stack unwinds. No recursion means graph depth cannot overflow
    // the process stack.
    let mut state = vec![0_u8; helpers.len()];
    let mut order = Vec::new();
    for root in roots {
        if state[root] == 2 {
            continue;
        }
        validate_origin_free(root, helpers)?;
        state[root] = 1;
        let mut stack = vec![(root, 0_usize)];
        while let Some((node, next_dependency)) = stack.last_mut() {
            if *next_dependency < adjacency[*node].len() {
                let dependency = adjacency[*node][*next_dependency];
                *next_dependency += 1;
                match state[dependency] {
                    0 => {
                        validate_origin_free(dependency, helpers)?;
                        state[dependency] = 1;
                        stack.push((dependency, 0));
                    }
                    1 | 2 => {}
                    _ => unreachable!("helper traversal state is three-valued"),
                }
            } else {
                let (finished, _) = stack.pop().expect("stack contains current helper");
                if state[finished] != 2 {
                    state[finished] = 2;
                    order.push(finished);
                }
            }
        }
    }
    Ok(order)
}

fn validate_origin_free(helper: usize, helpers: &[PendingHelper]) -> Result<(), SemanticLinkError> {
    if helpers[helper].body().effect_class != HelperEffectClass::OriginFree {
        return Err(SemanticLinkError::ReachableOriginatingHelper {
            helper: helpers[helper].body().semantic_key.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
