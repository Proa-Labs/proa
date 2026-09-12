// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic owner partitioning for the staged RSJS linker.
//!
//! This module begins after component/helper resolution and browser
//! reachability. It deliberately does not discover components or decide which
//! local graph nodes are browser-reachable. Instead, callers provide one owner
//! candidate per linked component instance, the live/lifecycle relations that
//! require candidates to share ownership, and the explicit boundaries that
//! must remain cuts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::abi::{
    CanonicalEncode, CanonicalEncoder, ComponentBindingSource, ComponentBoundary,
    ComponentDescriptor, ComponentEdgeId, ComponentFieldCodec, ComponentFieldId,
    ComponentFieldRole, CrossingHookId, HydrationConstraint, LocalValueId, OriginId, PropSiteId,
    SemanticHash, SharedStateRefId, SpanCardinality, StaticSlotCapability, TemporalPropMode,
};

/// Dense identity of one linked component-instance owner candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerCandidateId(u32);

impl OwnerCandidateId {
    /// Construct a linked candidate identity.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Return the dense linked index.
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl CanonicalEncode for OwnerCandidateId {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerCandidateId", 1);
        encoder.field(1, &self.0);
    }
}

/// Dense identity assigned to one final owner partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinkedOwnerId(u32);

impl LinkedOwnerId {
    /// Construct a final owner identity.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Return the deterministic linked index.
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl CanonicalEncode for LinkedOwnerId {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedOwnerId", 1);
        encoder.field(1, &self.0);
    }
}

/// Why two candidates must activate and dispose as one owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OwnerCouplingKind {
    /// A child consumes a live parent value.
    LiveProp,
    /// A child invokes a compiler-owned live callback slot.
    LiveCallback,
    /// A client-owned handle crosses between the candidates.
    ClientHandle,
    /// Parent-reactive creation, removal, movement, or reordering controls the
    /// child lifecycle.
    ReactiveLifecycle,
}

impl CanonicalEncode for OwnerCouplingKind {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let discriminant = match self {
            Self::LiveProp => 0,
            Self::LiveCallback => 1,
            Self::ClientHandle => 2,
            Self::ReactiveLifecycle => 3,
        };
        encoder.variant("OwnerCouplingKind", discriminant, 0);
    }
}

/// Why a component edge must remain an ownership cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OwnerCutKind {
    /// Explicit `rsjs::island(...)`.
    IndependentIsland,
    /// A separately planned nested owner boundary.
    NestedOwner,
}

impl CanonicalEncode for OwnerCutKind {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let discriminant = match self {
            Self::IndependentIsland => 0,
            Self::NestedOwner => 1,
        };
        encoder.variant("OwnerCutKind", discriminant, 0);
    }
}

/// One relation between linked component owner candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OwnerRelationKind {
    /// The candidates must fuse.
    Coupling(OwnerCouplingKind),
    /// The candidates must remain separate.
    Cut(OwnerCutKind),
    /// Static render nesting carries no ownership implication.
    StaticNesting,
}

impl CanonicalEncode for OwnerRelationKind {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Coupling(kind) => {
                encoder.variant("OwnerRelationKind", 0, 1);
                encoder.field(1, kind);
            }
            Self::Cut(kind) => {
                encoder.variant("OwnerRelationKind", 1, 1);
                encoder.field(1, kind);
            }
            Self::StaticNesting => encoder.variant("OwnerRelationKind", 2, 0),
        }
    }
}

/// A normalized relation in the linked component graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerRelation {
    /// Parent/source candidate.
    pub from: OwnerCandidateId,
    /// Child/target candidate.
    pub to: OwnerCandidateId,
    /// Ownership meaning of this edge.
    pub kind: OwnerRelationKind,
}

impl OwnerRelation {
    /// Construct an ownership relation.
    pub const fn new(
        from: OwnerCandidateId,
        to: OwnerCandidateId,
        kind: OwnerRelationKind,
    ) -> Self {
        Self { from, to, kind }
    }
}

impl CanonicalEncode for OwnerRelation {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerRelation", 3);
        encoder.field(1, &self.from);
        encoder.field(2, &self.to);
        encoder.field(3, &self.kind);
    }
}

/// Post-reachability owner candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerCandidate {
    /// Dense linked identity.
    pub id: OwnerCandidateId,
    /// Whether this component instance contains a reachable browser
    /// obligation before owner fusion.
    ///
    /// Coupling propagates activation across the complete connected component:
    /// a participant child may start `false` and still join an active parent.
    pub has_browser_obligation: bool,
    /// Source-level constraint contributed to any final owner containing this
    /// candidate.
    pub hydration: HydrationConstraint,
}

impl OwnerCandidate {
    /// Construct a candidate.
    pub const fn new(
        id: OwnerCandidateId,
        has_browser_obligation: bool,
        hydration: HydrationConstraint,
    ) -> Self {
        Self {
            id,
            has_browser_obligation,
            hydration,
        }
    }
}

impl CanonicalEncode for OwnerCandidate {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerCandidate", 3);
        encoder.field(1, &self.id);
        encoder.field(2, &self.has_browser_obligation);
        encoder.field(3, &self.hydration);
    }
}

/// One resolved component instance supplied by descriptor extraction and
/// browser reachability.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedComponentInstance<'a> {
    /// Dense component-instance/candidate identity.
    pub id: OwnerCandidateId,
    /// Exact descriptor selected through the Rust relocation witness.
    pub descriptor: &'a ComponentDescriptor,
    /// Whether local reachability retained a browser obligation before
    /// cross-component owner fusion.
    pub has_browser_obligation: bool,
}

/// Exact resolution of one parent direct-component site to one child
/// component instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedComponentEdge {
    /// Parent component instance.
    pub parent: OwnerCandidateId,
    /// Parent-local direct component edge.
    pub edge: ComponentEdgeId,
    /// Resolved child component instance.
    pub child: OwnerCandidateId,
}

/// Canonical owner-partition inputs derived from resolved component
/// descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPartitionInput {
    /// One candidate per resolved component instance.
    pub candidates: Vec<OwnerCandidate>,
    /// Static, coupling, and explicit-cut relations.
    pub relations: Vec<OwnerRelation>,
}

impl CanonicalEncode for OwnerPartitionInput {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerPartitionInput", 2);
        encoder.field(1, &self.candidates);
        encoder.field(2, &self.relations);
    }
}

/// Normative dispatch behavior for a compiler-owned callback edge.
///
/// V1 deliberately has no snapshot/serialized-closure variant. Keeping this
/// choice in the linked plan makes it impossible for later code generation to
/// silently retain the callback that happened to exist during SSR or owner
/// initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LinkedCallbackDispatch {
    /// Resolve the callback currently connected to the edge at invocation
    /// time.
    ResolveCurrent,
}

impl CanonicalEncode for LinkedCallbackDispatch {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::ResolveCurrent => encoder.variant("LinkedCallbackDispatch", 0, 0),
        }
    }
}

/// One concrete use of a live parent value by a linked child instance.
///
/// The record is candidate-specific: repeated uses of the same component
/// definition receive distinct child candidates and therefore distinct
/// `SeedOnce` instance lifetimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkedLivePropUse {
    /// Parent component instance supplying the value.
    pub parent: OwnerCandidateId,
    /// Parent-local direct component edge.
    pub edge: ComponentEdgeId,
    /// Concrete child component instance consuming the value.
    pub child: OwnerCandidateId,
    /// Stable child field selected by the source field name.
    pub field: ComponentFieldId,
    /// Compiler-generated child crossing hook.
    pub crossing_hook: CrossingHookId,
    /// Symbolic parent value connected to the child field.
    pub parent_value: LocalValueId,
    /// Child-local value node receiving the input.
    pub child_value: LocalValueId,
    /// Child-local use site.
    pub site: PropSiteId,
    /// Normative time behavior at this exact use.
    pub temporal: TemporalPropMode,
}

impl CanonicalEncode for LinkedLivePropUse {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedLivePropUse", 9);
        encoder.field(1, &self.parent);
        encoder.field(2, &self.edge);
        encoder.field(3, &self.child);
        encoder.field(4, &self.field);
        encoder.field(5, &self.crossing_hook);
        encoder.field(6, &self.parent_value);
        encoder.field(7, &self.child_value);
        encoder.field(8, &self.site);
        encoder.field(9, &self.temporal);
    }
}

/// One compiler-owned live callback connected to a concrete child instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkedLiveCallback {
    /// Parent component instance supplying the callback.
    pub parent: OwnerCandidateId,
    /// Parent-local direct component edge.
    pub edge: ComponentEdgeId,
    /// Concrete child component instance invoking the callback.
    pub child: OwnerCandidateId,
    /// Stable child callback field.
    pub field: ComponentFieldId,
    /// Compiler-generated child crossing hook.
    pub crossing_hook: CrossingHookId,
    /// Symbolic parent callback value.
    pub parent_value: LocalValueId,
    /// Child-local callback value receiving the live edge.
    pub child_value: LocalValueId,
    /// Required/optional callback container shape validated at the crossing.
    pub shape: crate::abi::ComponentCallbackShape,
    /// Required invocation-time dispatch behavior.
    pub dispatch: LinkedCallbackDispatch,
}

impl CanonicalEncode for LinkedLiveCallback {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedLiveCallback", 9);
        encoder.field(1, &self.parent);
        encoder.field(2, &self.edge);
        encoder.field(3, &self.child);
        encoder.field(4, &self.field);
        encoder.field(5, &self.crossing_hook);
        encoder.field(6, &self.parent_value);
        encoder.field(7, &self.child_value);
        encoder.field(8, &self.shape);
        encoder.field(9, &self.dispatch);
    }
}

/// Candidate-specific live input contracts produced by component linking.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinkedComponentInputs {
    /// Live value uses in deterministic parent/edge/field/site order.
    pub live_props: Vec<LinkedLivePropUse>,
    /// Live callbacks in deterministic parent/edge/field order.
    pub live_callbacks: Vec<LinkedLiveCallback>,
}

impl CanonicalEncode for LinkedComponentInputs {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedComponentInputs", 2);
        encoder.field(1, &self.live_props);
        encoder.field(2, &self.live_callbacks);
    }
}

/// Complete canonical component-link result before owner partitioning.
///
/// Owner relations and temporal input plans are derived in one walk so they
/// cannot disagree about whether a live edge exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentGraph {
    /// Candidate and coupling/cut inputs for owner partitioning.
    pub owner_input: OwnerPartitionInput,
    /// Exact temporal live-prop and current-callback contracts.
    pub inputs: LinkedComponentInputs,
}

impl CanonicalEncode for ResolvedComponentGraph {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ResolvedComponentGraph", 2);
        encoder.field(1, &self.owner_input);
        encoder.field(2, &self.inputs);
    }
}

/// Failure while converting exact component resolutions into owner inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComponentGraphLinkError {
    /// Component instance IDs must be unique and dense.
    NonDenseInstance {
        /// Required identity.
        expected: OwnerCandidateId,
        /// Identity that appeared.
        actual: OwnerCandidateId,
    },
    /// A resolved edge names an absent parent or child instance.
    UnknownInstance {
        /// Missing instance.
        instance: OwnerCandidateId,
    },
    /// A resolved edge names no direct edge in its parent descriptor.
    UnknownDirectEdge {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Missing parent-local edge.
        edge: ComponentEdgeId,
    },
    /// A parent direct edge was resolved more than once.
    DuplicateDirectEdgeResolution {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Duplicate parent-local edge.
        edge: ComponentEdgeId,
    },
    /// A parent direct edge has no relocation-backed child resolution.
    UnresolvedDirectEdge {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Unresolved parent-local edge.
        edge: ComponentEdgeId,
    },
    /// A source field selector is absent from the selected child descriptor.
    UnknownChildField {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
    },
    /// A client-visible selected child field has no crossing hook.
    MissingChildCrossingHook {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child field name.
        field_name: String,
        /// Stable child field ID.
        field: ComponentFieldId,
    },
    /// A selected child value/callback field has no child-local graph value.
    MissingChildClientValue {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child field name.
        field_name: String,
        /// Stable child field ID.
        field: ComponentFieldId,
    },
    /// A selected child callback field has no required/optional shape.
    MissingChildCallbackShape {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child field name.
        field_name: String,
    },
    /// A parent binding kind does not match the selected child field role.
    FieldRoleMismatch {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
        /// Child-declared role.
        role: ComponentFieldRole,
        /// Human-facing supplied binding class.
        binding: &'static str,
    },
    /// A serialized seed does not match the child field codec schema.
    FieldCodecMismatch {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
        /// Child-required codec schema, if any.
        expected: Option<SemanticHash>,
        /// Parent crossing codec schema.
        actual: SemanticHash,
    },
    /// A callback binding's required/optional shape differs from the child field.
    CallbackShapeMismatch {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
        /// Shape required by the child field.
        expected: crate::abi::ComponentCallbackShape,
        /// Shape supplied by the parent binding.
        actual: crate::abi::ComponentCallbackShape,
    },
    /// A static slot is not supported by the selected child field.
    StaticSlotUnsupported {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
    },
    /// A static-only slot was used as a client-reconstructed live slot.
    LiveSlotUnsupported {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child field name.
        field_name: String,
    },
    /// A static slot contains an owner boundary that the child cannot preserve.
    NestedOwnerInStaticSlotUnsupported {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
    },
    /// Descriptor validation should have rejected a live edge through an
    /// explicit island, but the linker checks it again at the trust boundary.
    LiveBindingAcrossIndependentIsland {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Source field name.
        field_name: String,
    },
    /// A live parent value reaches `SharedStateProvider` initialization.
    ForbiddenLiveProviderSeed {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child value field.
        field_name: String,
        /// Child prop-use site carrying the forbidden temporal mode.
        site: PropSiteId,
    },
    /// A live parent value reaches a nested explicit island boundary.
    ForbiddenLiveBoundaryCrossing {
        /// Parent instance.
        parent: OwnerCandidateId,
        /// Parent-local edge.
        edge: ComponentEdgeId,
        /// Child value field.
        field_name: String,
        /// Child prop-use site carrying the forbidden temporal mode.
        site: PropSiteId,
    },
    /// Deterministic owner partitioning failed after descriptor resolution.
    OwnerPartition(OwnerPartitionError),
}

impl fmt::Display for ComponentGraphLinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonDenseInstance { expected, actual } => write!(
                formatter,
                "component instance IDs must be dense; expected {}, found {}",
                expected.index(),
                actual.index()
            ),
            Self::UnknownInstance { instance } => {
                write!(formatter, "resolved component edge refers to unknown instance {}", instance.index())
            }
            Self::UnknownDirectEdge { parent, edge } => write!(
                formatter,
                "component instance {} has no direct edge {}",
                parent.index(),
                edge.index()
            ),
            Self::DuplicateDirectEdgeResolution { parent, edge } => write!(
                formatter,
                "component instance {} direct edge {} was resolved more than once",
                parent.index(),
                edge.index()
            ),
            Self::UnresolvedDirectEdge { parent, edge } => write!(
                formatter,
                "component instance {} direct edge {} has no relocation-backed resolution",
                parent.index(),
                edge.index()
            ),
            Self::UnknownChildField {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} selected a child with no field {field_name:?}",
                parent.index(),
                edge.index()
            ),
            Self::MissingChildCrossingHook {
                parent,
                edge,
                field_name,
                field,
            } => write!(
                formatter,
                "component instance {} edge {} selected client-visible child field {field_name:?} ({}) without a compiler-generated crossing hook",
                parent.index(),
                edge.index(),
                field.index()
            ),
            Self::MissingChildClientValue {
                parent,
                edge,
                field_name,
                field,
            } => write!(
                formatter,
                "component instance {} edge {} selected client value/callback child field {field_name:?} ({}) without a child-local symbolic value",
                parent.index(),
                edge.index(),
                field.index()
            ),
            Self::MissingChildCallbackShape {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} selected callback field {field_name:?} without a required/optional callback shape",
                parent.index(),
                edge.index()
            ),
            Self::FieldRoleMismatch {
                parent,
                edge,
                field_name,
                role,
                binding,
            } => write!(
                formatter,
                "component instance {} edge {} binds child field {field_name:?} ({role:?}) as {binding}",
                parent.index(),
                edge.index()
            ),
            Self::FieldCodecMismatch {
                parent,
                edge,
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "component instance {} edge {} field {field_name:?} requires codec {expected:?}, got {actual}",
                parent.index(),
                edge.index()
            ),
            Self::CallbackShapeMismatch {
                parent,
                edge,
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "component instance {} edge {} field {field_name:?} requires callback shape {expected:?}, got {actual:?}",
                parent.index(),
                edge.index()
            ),
            Self::StaticSlotUnsupported {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} child field {field_name:?} does not support immutable server slots",
                parent.index(),
                edge.index()
            ),
            Self::LiveSlotUnsupported {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} child field {field_name:?} is an immutable SSR-only slot and cannot be reconstructed as a live client slot",
                parent.index(),
                edge.index()
            ),
            Self::NestedOwnerInStaticSlotUnsupported {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} child field {field_name:?} cannot preserve a nested owner in its static slot",
                parent.index(),
                edge.index()
            ),
            Self::LiveBindingAcrossIndependentIsland {
                parent,
                edge,
                field_name,
            } => write!(
                formatter,
                "component instance {} edge {} passes live child field {field_name:?} across an independent island",
                parent.index(),
                edge.index()
            ),
            Self::ForbiddenLiveProviderSeed {
                parent,
                edge,
                field_name,
                site,
            } => write!(
                formatter,
                "component instance {} edge {} passes live field {field_name:?} into SharedStateProvider initialization at prop-use site {}; initialize the provider from static server data or make SharedState the source of truth",
                parent.index(),
                edge.index(),
                site.index()
            ),
            Self::ForbiddenLiveBoundaryCrossing {
                parent,
                edge,
                field_name,
                site,
            } => write!(
                formatter,
                "component instance {} edge {} passes live field {field_name:?} through an independent island at prop-use site {}; live values cannot cross `rsjs::island(...)`",
                parent.index(),
                edge.index(),
                site.index()
            ),
            Self::OwnerPartition(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ComponentGraphLinkError {}

impl From<OwnerPartitionError> for ComponentGraphLinkError {
    fn from(error: OwnerPartitionError) -> Self {
        Self::OwnerPartition(error)
    }
}

/// One component acquisition contributing to a linked SharedState schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SharedStateConsumer {
    /// Linked component instance.
    pub component: OwnerCandidateId,
    /// Component-local acquisition.
    pub reference: SharedStateRefId,
    /// Source provenance in the component descriptor.
    pub origin: OriginId,
}

impl CanonicalEncode for SharedStateConsumer {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SharedStateConsumer", 3);
        encoder.field(1, &self.component);
        encoder.field(2, &self.reference);
        encoder.field(3, &self.origin);
    }
}

/// Deduplicated page-level SharedState schema selected by the linker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedSharedStateSchema {
    /// Public protocol key.
    pub type_key: String,
    /// Exact canonical wire-schema ABI.
    pub schema_abi: SemanticHash,
    /// Sorted consumers using the byte-identical schema.
    pub consumers: Vec<SharedStateConsumer>,
}

impl CanonicalEncode for LinkedSharedStateSchema {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkedSharedStateSchema", 3);
        encoder.field(1, &self.type_key);
        encoder.field(2, &self.schema_abi);
        encoder.field(3, &self.consumers);
    }
}

/// A public SharedState `type_key` selected incompatible schema ABIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStateSchemaConflict {
    /// Conflicting public protocol key.
    pub type_key: String,
    /// First schema selected for the key.
    pub first_schema: SemanticHash,
    /// First linked consumer.
    pub first: SharedStateConsumer,
    /// Incompatible later schema.
    pub second_schema: SemanticHash,
    /// Later linked consumer.
    pub second: SharedStateConsumer,
}

impl fmt::Display for SharedStateSchemaConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "SharedState type_key {:?} has incompatible schema ABIs {} (component {}, reference {}) and {} (component {}, reference {}); one type_key must name exactly one wire schema before any owner activates",
            self.type_key,
            self.first_schema,
            self.first.component.index(),
            self.first.reference.index(),
            self.second_schema,
            self.second.component.index(),
            self.second.reference.index(),
        )
    }
}

impl std::error::Error for SharedStateSchemaConflict {}

/// Group byte-identical SharedState schemas and reject public-key conflicts.
///
/// This runs before owner activation and intentionally does not fuse owners:
/// independent owners reacquire the same response-scoped store by provider
/// instance, while this table verifies only the public schema identity.
pub fn link_shared_state_schemas(
    instances: &[ResolvedComponentInstance<'_>],
) -> Result<Vec<LinkedSharedStateSchema>, SharedStateSchemaConflict> {
    let mut linked = BTreeMap::<String, LinkedSharedStateSchema>::new();
    for instance in instances {
        for (&reference, shared) in &instance.descriptor.shared_state {
            let consumer = SharedStateConsumer {
                component: instance.id,
                reference,
                origin: shared.origin,
            };
            match linked.entry(shared.type_key.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(LinkedSharedStateSchema {
                        type_key: shared.type_key.clone(),
                        schema_abi: shared.schema_abi,
                        consumers: vec![consumer],
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let existing = entry.get_mut();
                    if existing.schema_abi != shared.schema_abi {
                        return Err(SharedStateSchemaConflict {
                            type_key: shared.type_key.clone(),
                            first_schema: existing.schema_abi,
                            first: existing.consumers[0],
                            second_schema: shared.schema_abi,
                            second: consumer,
                        });
                    }
                    existing.consumers.push(consumer);
                }
            }
        }
    }
    for schema in linked.values_mut() {
        schema.consumers.sort_unstable();
        schema.consumers.dedup();
    }
    Ok(linked.into_values().collect())
}

/// Effective activation policy of a final owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OwnerActivationStrategy {
    /// Activate eagerly. This is also the default for an unconstrained owner.
    Eager,
    /// Activate when visible.
    Visible,
    /// Activate during idle time.
    Idle,
    /// Activate on the first qualifying interaction.
    Interaction {
        /// Optional event restriction.
        event: Option<String>,
    },
    /// Activate when a media query matches.
    Media {
        /// Required media query.
        query: String,
    },
}

impl OwnerActivationStrategy {
    fn from_constraint(constraint: &HydrationConstraint) -> Option<Self> {
        match constraint {
            HydrationConstraint::Unconstrained => None,
            HydrationConstraint::Eager => Some(Self::Eager),
            HydrationConstraint::Visible => Some(Self::Visible),
            HydrationConstraint::Idle => Some(Self::Idle),
            HydrationConstraint::Interaction { event } => Some(Self::Interaction {
                event: event.clone(),
            }),
            HydrationConstraint::Media { query } => Some(Self::Media {
                query: query.clone(),
            }),
        }
    }
}

impl CanonicalEncode for OwnerActivationStrategy {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Eager => encoder.variant("OwnerActivationStrategy", 0, 0),
            Self::Visible => encoder.variant("OwnerActivationStrategy", 1, 0),
            Self::Idle => encoder.variant("OwnerActivationStrategy", 2, 0),
            Self::Interaction { event } => {
                encoder.variant("OwnerActivationStrategy", 3, 1);
                encoder.field(1, event);
            }
            Self::Media { query } => {
                encoder.variant("OwnerActivationStrategy", 4, 1);
                encoder.field(1, query);
            }
        }
    }
}

/// One final owner produced by deterministic partitioning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPartition {
    /// Dense owner identity, ordered by the smallest candidate in the
    /// partition.
    pub id: LinkedOwnerId,
    /// Sorted candidate membership.
    pub candidates: Vec<OwnerCandidateId>,
    /// Effective activation strategy.
    pub strategy: OwnerActivationStrategy,
}

impl CanonicalEncode for OwnerPartition {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerPartition", 3);
        encoder.field(1, &self.id);
        encoder.field(2, &self.candidates);
        encoder.field(3, &self.strategy);
    }
}

/// Complete owner-partition result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPartitionPlan {
    /// Active owners in deterministic order.
    pub owners: Vec<OwnerPartition>,
    /// Reverse lookup for every candidate belonging to an active owner.
    ///
    /// Candidates in a coupling component with no browser obligation are
    /// omitted because that component forms no owner.
    pub candidate_to_owner: BTreeMap<OwnerCandidateId, LinkedOwnerId>,
}

impl CanonicalEncode for OwnerPartitionPlan {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("OwnerPartitionPlan", 2);
        encoder.field(1, &self.owners);
        encoder.field(2, &self.candidate_to_owner);
    }
}

/// One coupling step retained in an ownership diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerCouplingStep {
    /// Candidate at the beginning of this step.
    pub from: OwnerCandidateId,
    /// Candidate at the end of this step.
    pub to: OwnerCandidateId,
    /// Coupling reason.
    pub kind: OwnerCouplingKind,
}

/// Deterministic owner-partition failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OwnerPartitionError {
    /// Candidate IDs must be unique and dense from zero.
    NonDenseCandidate {
        /// Required ID at this position.
        expected: OwnerCandidateId,
        /// ID that appeared.
        actual: OwnerCandidateId,
    },
    /// A relation references a candidate absent from the input.
    UnknownCandidate {
        /// Missing candidate.
        candidate: OwnerCandidateId,
    },
    /// A live/lifecycle path attempts to fuse across an explicit owner cut.
    CouplingCrossesCut {
        /// Cut source.
        cut_from: OwnerCandidateId,
        /// Cut target.
        cut_to: OwnerCandidateId,
        /// Kind of explicit cut.
        cut: OwnerCutKind,
        /// Coupling path proving the two sides would otherwise fuse.
        coupling_path: Vec<OwnerCouplingStep>,
    },
    /// One required owner contains incompatible explicit activation
    /// constraints.
    StrategyConflict {
        /// First constrained candidate.
        first: OwnerCandidateId,
        /// First requested strategy.
        first_strategy: OwnerActivationStrategy,
        /// Conflicting constrained candidate.
        second: OwnerCandidateId,
        /// Conflicting requested strategy.
        second_strategy: OwnerActivationStrategy,
        /// Coupling path that forced the candidates into one owner.
        coupling_path: Vec<OwnerCouplingStep>,
    },
}

impl fmt::Display for OwnerPartitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonDenseCandidate { expected, actual } => write!(
                formatter,
                "owner candidate IDs must be dense; expected {}, found {}",
                expected.index(),
                actual.index()
            ),
            Self::UnknownCandidate { candidate } => write!(
                formatter,
                "owner relation references unknown candidate {}",
                candidate.index()
            ),
            Self::CouplingCrossesCut {
                cut_from,
                cut_to,
                cut,
                ..
            } => write!(
                formatter,
                "live owner coupling crosses {cut:?} cut {} -> {}",
                cut_from.index(),
                cut_to.index()
            ),
            Self::StrategyConflict {
                first,
                first_strategy,
                second,
                second_strategy,
                ..
            } => write!(
                formatter,
                "owner candidates {} and {} require incompatible hydration strategies \
                 {first_strategy:?} and {second_strategy:?}",
                first.index(),
                second.index()
            ),
        }
    }
}

impl std::error::Error for OwnerPartitionError {}

/// Validate exact relocation-backed component resolutions and derive the
/// canonical owner-partition input.
pub fn derive_owner_partition_input(
    instances: &[ResolvedComponentInstance<'_>],
    edges: &[ResolvedComponentEdge],
) -> Result<OwnerPartitionInput, ComponentGraphLinkError> {
    Ok(link_resolved_component_graph(instances, edges)?.owner_input)
}

/// Validate exact component resolutions and retain their normative live-input
/// timing contracts alongside the owner-partition input.
pub fn link_resolved_component_graph(
    instances: &[ResolvedComponentInstance<'_>],
    edges: &[ResolvedComponentEdge],
) -> Result<ResolvedComponentGraph, ComponentGraphLinkError> {
    let mut instances = instances.to_vec();
    instances.sort_by_key(|instance| instance.id);
    for (index, instance) in instances.iter().enumerate() {
        let expected = OwnerCandidateId::new(index as u32);
        if instance.id != expected {
            return Err(ComponentGraphLinkError::NonDenseInstance {
                expected,
                actual: instance.id,
            });
        }
    }

    let candidates = instances
        .iter()
        .map(|instance| {
            OwnerCandidate::new(
                instance.id,
                instance.has_browser_obligation,
                instance.descriptor.hydration.clone(),
            )
        })
        .collect::<Vec<_>>();

    let mut relations = BTreeSet::new();
    let mut inputs = LinkedComponentInputs::default();
    let mut seen_edges = BTreeSet::new();
    let mut resolutions = edges.to_vec();
    resolutions.sort_unstable();
    for resolution in resolutions {
        let parent = instances
            .get(resolution.parent.index() as usize)
            .filter(|instance| instance.id == resolution.parent)
            .ok_or(ComponentGraphLinkError::UnknownInstance {
                instance: resolution.parent,
            })?;
        let child = instances
            .get(resolution.child.index() as usize)
            .filter(|instance| instance.id == resolution.child)
            .ok_or(ComponentGraphLinkError::UnknownInstance {
                instance: resolution.child,
            })?;
        let edge = parent
            .descriptor
            .direct_components
            .get(&resolution.edge)
            .ok_or(ComponentGraphLinkError::UnknownDirectEdge {
                parent: resolution.parent,
                edge: resolution.edge,
            })?;

        if !seen_edges.insert((resolution.parent, resolution.edge)) {
            return Err(ComponentGraphLinkError::DuplicateDirectEdgeResolution {
                parent: resolution.parent,
                edge: resolution.edge,
            });
        }
        relations.insert(OwnerRelation::new(
            resolution.parent,
            resolution.child,
            match edge.boundary {
                ComponentBoundary::Composed => OwnerRelationKind::StaticNesting,
                ComponentBoundary::IndependentIsland => {
                    OwnerRelationKind::Cut(OwnerCutKind::IndependentIsland)
                }
            },
        ));

        for (field_name, binding) in &edge.bindings {
            let (field_id, field) = child
                .descriptor
                .fields
                .iter()
                .find(|(_, field)| field.source_name == *field_name)
                .ok_or_else(|| ComponentGraphLinkError::UnknownChildField {
                    parent: resolution.parent,
                    edge: resolution.edge,
                    field_name: field_name.clone(),
                })?;

            let mut coupling = validate_resolved_field_binding(
                resolution.parent,
                resolution.edge,
                field_name,
                field,
                &binding.source,
            )?;
            match &binding.source {
                ComponentBindingSource::LiveValue { value } => {
                    let crossing_hook = required_child_crossing_hook(
                        resolution.parent,
                        resolution.edge,
                        field_name,
                        *field_id,
                        field,
                    )?;
                    let linked_uses = link_live_prop_temporal_uses(
                        resolution.parent,
                        resolution.edge,
                        resolution.child,
                        field_name,
                        child.descriptor,
                        *field_id,
                        crossing_hook,
                        *value,
                    )?;
                    if linked_uses.is_empty() {
                        // A field that has no client-reachable use remains an
                        // ordinary SSR input. Merely receiving a live parent
                        // expression must not fuse otherwise disconnected
                        // owners.
                        coupling = None;
                    } else {
                        inputs.live_props.extend(linked_uses);
                    }
                }
                ComponentBindingSource::LiveCallback { value, shape } => {
                    let crossing_hook = required_child_crossing_hook(
                        resolution.parent,
                        resolution.edge,
                        field_name,
                        *field_id,
                        field,
                    )?;
                    let child_value = required_child_client_value(
                        resolution.parent,
                        resolution.edge,
                        field_name,
                        *field_id,
                        field,
                    )?;
                    inputs.live_callbacks.push(LinkedLiveCallback {
                        parent: resolution.parent,
                        edge: resolution.edge,
                        child: resolution.child,
                        field: *field_id,
                        crossing_hook,
                        parent_value: *value,
                        child_value,
                        shape: *shape,
                        dispatch: LinkedCallbackDispatch::ResolveCurrent,
                    });
                }
                _ => {}
            }
            if edge.boundary == ComponentBoundary::IndependentIsland && coupling.is_some() {
                return Err(
                    ComponentGraphLinkError::LiveBindingAcrossIndependentIsland {
                        parent: resolution.parent,
                        edge: resolution.edge,
                        field_name: field_name.clone(),
                    },
                );
            }
            if let Some(kind) = coupling {
                relations.insert(OwnerRelation::new(
                    resolution.parent,
                    resolution.child,
                    OwnerRelationKind::Coupling(kind),
                ));
            }
        }

        if edge.lifecycle_control.is_some() {
            relations.insert(OwnerRelation::new(
                resolution.parent,
                resolution.child,
                OwnerRelationKind::Coupling(OwnerCouplingKind::ReactiveLifecycle),
            ));
        }
    }

    for instance in &instances {
        for edge in instance.descriptor.direct_components.keys() {
            if !seen_edges.contains(&(instance.id, *edge)) {
                return Err(ComponentGraphLinkError::UnresolvedDirectEdge {
                    parent: instance.id,
                    edge: *edge,
                });
            }
        }
    }

    Ok(ResolvedComponentGraph {
        owner_input: OwnerPartitionInput {
            candidates,
            relations: relations.into_iter().collect(),
        },
        inputs,
    })
}

/// Resolve component field contracts and immediately partition final owners.
pub fn partition_resolved_components(
    instances: &[ResolvedComponentInstance<'_>],
    edges: &[ResolvedComponentEdge],
) -> Result<OwnerPartitionPlan, ComponentGraphLinkError> {
    let graph = link_resolved_component_graph(instances, edges)?;
    partition_owners(&graph.owner_input.candidates, &graph.owner_input.relations)
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn link_live_prop_temporal_uses(
    parent: OwnerCandidateId,
    edge: ComponentEdgeId,
    child_instance: OwnerCandidateId,
    field_name: &str,
    child: &ComponentDescriptor,
    field: ComponentFieldId,
    crossing_hook: CrossingHookId,
    parent_value: LocalValueId,
) -> Result<Vec<LinkedLivePropUse>, ComponentGraphLinkError> {
    let mut linked = Vec::new();
    for (&site, prop_use) in &child.prop_uses {
        if prop_use.field != field {
            continue;
        }
        match prop_use.temporal {
            TemporalPropMode::LiveRead
            | TemporalPropMode::SeedOnce(_)
            | TemporalPropMode::EffectDependency
            | TemporalPropMode::QueryKeyDependency => {
                linked.push(LinkedLivePropUse {
                    parent,
                    edge,
                    child: child_instance,
                    field,
                    crossing_hook,
                    parent_value,
                    child_value: prop_use.value,
                    site,
                    temporal: prop_use.temporal,
                });
            }
            TemporalPropMode::ForbiddenProviderSeed => {
                return Err(ComponentGraphLinkError::ForbiddenLiveProviderSeed {
                    parent,
                    edge,
                    field_name: field_name.to_string(),
                    site,
                });
            }
            TemporalPropMode::ForbiddenBoundaryCrossing => {
                return Err(ComponentGraphLinkError::ForbiddenLiveBoundaryCrossing {
                    parent,
                    edge,
                    field_name: field_name.to_string(),
                    site,
                });
            }
        }
    }
    Ok(linked)
}

fn required_child_crossing_hook(
    parent: OwnerCandidateId,
    edge: ComponentEdgeId,
    field_name: &str,
    field_id: ComponentFieldId,
    field: &crate::abi::ComponentFieldSchema,
) -> Result<CrossingHookId, ComponentGraphLinkError> {
    field
        .crossing_hook
        .ok_or_else(|| ComponentGraphLinkError::MissingChildCrossingHook {
            parent,
            edge,
            field_name: field_name.to_string(),
            field: field_id,
        })
}

fn required_child_client_value(
    parent: OwnerCandidateId,
    edge: ComponentEdgeId,
    field_name: &str,
    field_id: ComponentFieldId,
    field: &crate::abi::ComponentFieldSchema,
) -> Result<LocalValueId, ComponentGraphLinkError> {
    field
        .client_value
        .ok_or_else(|| ComponentGraphLinkError::MissingChildClientValue {
            parent,
            edge,
            field_name: field_name.to_string(),
            field: field_id,
        })
}

fn validate_callback_shape(
    parent: OwnerCandidateId,
    edge: ComponentEdgeId,
    field_name: &str,
    field: &crate::abi::ComponentFieldSchema,
    actual: crate::abi::ComponentCallbackShape,
) -> Result<(), ComponentGraphLinkError> {
    if field.role != ComponentFieldRole::Callback {
        return Err(ComponentGraphLinkError::FieldRoleMismatch {
            parent,
            edge,
            field_name: field_name.to_string(),
            role: field.role,
            binding: "callback",
        });
    }
    let Some(expected) = field.callback_shape else {
        return Err(ComponentGraphLinkError::MissingChildCallbackShape {
            parent,
            edge,
            field_name: field_name.to_string(),
        });
    };
    if expected == actual {
        Ok(())
    } else {
        Err(ComponentGraphLinkError::CallbackShapeMismatch {
            parent,
            edge,
            field_name: field_name.to_string(),
            expected,
            actual,
        })
    }
}

fn validate_resolved_field_binding(
    parent: OwnerCandidateId,
    edge: ComponentEdgeId,
    field_name: &str,
    field: &crate::abi::ComponentFieldSchema,
    source: &ComponentBindingSource,
) -> Result<Option<OwnerCouplingKind>, ComponentGraphLinkError> {
    let mismatch = |binding| ComponentGraphLinkError::FieldRoleMismatch {
        parent,
        edge,
        field_name: field_name.to_string(),
        role: field.role,
        binding,
    };

    match source {
        ComponentBindingSource::ServerOnly => {
            // The field role describes what the child is capable of accepting,
            // while this binding describes one concrete parent site. A value
            // field may therefore receive an ordinary SSR value at one site
            // and a live value at another. Only the latter forms an owner
            // coupling and contributes temporal live-prop uses.
            if matches!(
                field.role,
                ComponentFieldRole::ServerOnly | ComponentFieldRole::Value
            ) {
                Ok(None)
            } else {
                Err(mismatch("server-only input"))
            }
        }
        ComponentBindingSource::StaticCallback { shape } => {
            validate_callback_shape(parent, edge, field_name, field, *shape)?;
            Ok(None)
        }
        ComponentBindingSource::SerializedSeed { codec, .. } => {
            if field.role != ComponentFieldRole::Value {
                return Err(mismatch("serialized seed"));
            }
            let expected = match field.codec {
                ComponentFieldCodec::None => None,
                ComponentFieldCodec::ClientEncode { schema } => Some(schema),
            };
            if expected == Some(*codec) {
                Ok(None)
            } else {
                Err(ComponentGraphLinkError::FieldCodecMismatch {
                    parent,
                    edge,
                    field_name: field_name.to_string(),
                    expected,
                    actual: *codec,
                })
            }
        }
        ComponentBindingSource::LiveValue { .. } => {
            if field.role == ComponentFieldRole::Value {
                Ok(Some(OwnerCouplingKind::LiveProp))
            } else {
                Err(mismatch("live value"))
            }
        }
        ComponentBindingSource::LiveCallback { shape, .. } => {
            validate_callback_shape(parent, edge, field_name, field, *shape)?;
            Ok(Some(OwnerCouplingKind::LiveCallback))
        }
        ComponentBindingSource::ClientHandle { .. } => {
            if field.role == ComponentFieldRole::ClientHandle {
                Ok(Some(OwnerCouplingKind::ClientHandle))
            } else {
                Err(mismatch("client handle"))
            }
        }
        ComponentBindingSource::StaticSlot { span } => {
            if field.role != ComponentFieldRole::Slot {
                return Err(mismatch("static server slot"));
            }
            match field.static_slot {
                StaticSlotCapability::Unsupported => {
                    Err(ComponentGraphLinkError::StaticSlotUnsupported {
                        parent,
                        edge,
                        field_name: field_name.to_string(),
                    })
                }
                StaticSlotCapability::Immutable {
                    permits_nested_owner,
                } if !permits_nested_owner && span_contains_owner_boundary(span) => Err(
                    ComponentGraphLinkError::NestedOwnerInStaticSlotUnsupported {
                        parent,
                        edge,
                        field_name: field_name.to_string(),
                    },
                ),
                StaticSlotCapability::Immutable { .. } => Ok(None),
            }
        }
        ComponentBindingSource::LiveSlot { .. } => {
            if field.role != ComponentFieldRole::Slot {
                return Err(mismatch("live client slot"));
            }
            if field.type_witness.is_none() || field.crossing_hook.is_none() {
                return Err(ComponentGraphLinkError::LiveSlotUnsupported {
                    parent,
                    edge,
                    field_name: field_name.to_string(),
                });
            }
            Ok(Some(OwnerCouplingKind::ReactiveLifecycle))
        }
    }
}

fn span_contains_owner_boundary(span: &SpanCardinality) -> bool {
    match span {
        SpanCardinality::OwnerBoundary { .. } => true,
        SpanCardinality::Sequence(parts) => parts.iter().any(span_contains_owner_boundary),
        SpanCardinality::Exact(_) | SpanCardinality::Unknown { .. } => false,
    }
}

/// Partition a post-reachability component graph into deterministic owners.
///
/// Coupling is undirected for ownership: either endpoint cannot activate or
/// dispose independently. Static nesting is ignored. Explicit cuts are checked
/// after the complete coupling closure is known, which catches indirect paths
/// instead of only contradictory parallel edges.
pub fn partition_owners(
    candidates: &[OwnerCandidate],
    relations: &[OwnerRelation],
) -> Result<OwnerPartitionPlan, OwnerPartitionError> {
    validate_candidates(candidates)?;
    let count = candidates.len();
    let mut union = UnionFind::new(count);
    let mut coupling_adjacency = vec![Vec::<(OwnerCandidateId, OwnerCouplingKind)>::new(); count];
    let mut cuts = Vec::new();

    let normalized_relations = relations.iter().copied().collect::<BTreeSet<_>>();
    for relation in normalized_relations {
        validate_relation_endpoint(relation.from, count)?;
        validate_relation_endpoint(relation.to, count)?;
        match relation.kind {
            OwnerRelationKind::Coupling(kind) => {
                union.union(relation.from.index() as usize, relation.to.index() as usize);
                coupling_adjacency[relation.from.index() as usize].push((relation.to, kind));
                coupling_adjacency[relation.to.index() as usize].push((relation.from, kind));
            }
            OwnerRelationKind::Cut(kind) => cuts.push((relation.from, relation.to, kind)),
            OwnerRelationKind::StaticNesting => {}
        }
    }

    for neighbors in &mut coupling_adjacency {
        neighbors.sort_unstable();
        neighbors.dedup();
    }

    for (from, to, cut) in cuts {
        if union.find(from.index() as usize) == union.find(to.index() as usize) {
            return Err(OwnerPartitionError::CouplingCrossesCut {
                cut_from: from,
                cut_to: to,
                cut,
                coupling_path: coupling_path(from, to, &coupling_adjacency)
                    .expect("unioned candidates must have a coupling path"),
            });
        }
    }

    let mut groups = BTreeMap::<usize, Vec<&OwnerCandidate>>::new();
    for candidate in candidates {
        let root = union.find(candidate.id.index() as usize);
        groups.entry(root).or_default().push(candidate);
    }

    let mut active_groups = groups
        .into_values()
        .filter(|group| {
            group
                .iter()
                .any(|candidate| candidate.has_browser_obligation)
        })
        .collect::<Vec<_>>();
    for group in &mut active_groups {
        group.sort_by_key(|candidate| candidate.id);
    }
    active_groups.sort_by_key(|group| group.first().expect("owner group cannot be empty").id);

    let mut owners = Vec::with_capacity(active_groups.len());
    let mut candidate_to_owner = BTreeMap::new();
    for (owner_index, group) in active_groups.into_iter().enumerate() {
        let strategy = resolve_strategy(&group, &coupling_adjacency)?;
        let owner_id = LinkedOwnerId::new(owner_index as u32);
        let candidate_ids = group
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        for candidate in &candidate_ids {
            candidate_to_owner.insert(*candidate, owner_id);
        }
        owners.push(OwnerPartition {
            id: owner_id,
            candidates: candidate_ids,
            strategy,
        });
    }

    Ok(OwnerPartitionPlan {
        owners,
        candidate_to_owner,
    })
}

fn validate_candidates(candidates: &[OwnerCandidate]) -> Result<(), OwnerPartitionError> {
    let mut ids = candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    for (index, actual) in ids.into_iter().enumerate() {
        let expected = OwnerCandidateId::new(index as u32);
        if actual != expected {
            return Err(OwnerPartitionError::NonDenseCandidate { expected, actual });
        }
    }
    Ok(())
}

fn validate_relation_endpoint(
    candidate: OwnerCandidateId,
    candidate_count: usize,
) -> Result<(), OwnerPartitionError> {
    if candidate.index() as usize >= candidate_count {
        Err(OwnerPartitionError::UnknownCandidate { candidate })
    } else {
        Ok(())
    }
}

fn resolve_strategy(
    candidates: &[&OwnerCandidate],
    adjacency: &[Vec<(OwnerCandidateId, OwnerCouplingKind)>],
) -> Result<OwnerActivationStrategy, OwnerPartitionError> {
    let mut selected: Option<(OwnerCandidateId, OwnerActivationStrategy)> = None;
    for candidate in candidates {
        let Some(strategy) = OwnerActivationStrategy::from_constraint(&candidate.hydration) else {
            continue;
        };
        match &selected {
            None => selected = Some((candidate.id, strategy)),
            Some((_, selected_strategy)) if selected_strategy == &strategy => {}
            Some((first, first_strategy)) => {
                return Err(OwnerPartitionError::StrategyConflict {
                    first: *first,
                    first_strategy: first_strategy.clone(),
                    second: candidate.id,
                    second_strategy: strategy,
                    coupling_path: coupling_path(*first, candidate.id, adjacency)
                        .expect("candidates in one owner group must have a coupling path"),
                });
            }
        }
    }
    Ok(selected
        .map(|(_, strategy)| strategy)
        .unwrap_or(OwnerActivationStrategy::Eager))
}

fn coupling_path(
    start: OwnerCandidateId,
    goal: OwnerCandidateId,
    adjacency: &[Vec<(OwnerCandidateId, OwnerCouplingKind)>],
) -> Option<Vec<OwnerCouplingStep>> {
    if start == goal {
        return Some(Vec::new());
    }

    let mut queue = VecDeque::from([start]);
    let mut visited = BTreeSet::from([start]);
    let mut previous = BTreeMap::<OwnerCandidateId, (OwnerCandidateId, OwnerCouplingKind)>::new();

    while let Some(current) = queue.pop_front() {
        for &(next, kind) in &adjacency[current.index() as usize] {
            if !visited.insert(next) {
                continue;
            }
            previous.insert(next, (current, kind));
            if next == goal {
                let mut reversed = Vec::new();
                let mut cursor = goal;
                while cursor != start {
                    let &(from, step_kind) = previous
                        .get(&cursor)
                        .expect("visited non-root candidate has a predecessor");
                    reversed.push(OwnerCouplingStep {
                        from,
                        to: cursor,
                        kind: step_kind,
                    });
                    cursor = from;
                }
                reversed.reverse();
                return Some(reversed);
            }
            queue.push_back(next);
        }
    }
    None
}

#[derive(Debug)]
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            rank: vec![0; len],
        }
    }

    fn find(&mut self, node: usize) -> usize {
        let parent = self.parent[node];
        if parent != node {
            self.parent[node] = self.find(parent);
        }
        self.parent[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left == right {
            return;
        }
        match self.rank[left].cmp(&self.rank[right]) {
            std::cmp::Ordering::Less => self.parent[left] = right,
            std::cmp::Ordering::Greater => self.parent[right] = left,
            std::cmp::Ordering::Equal => {
                self.parent[right] = left;
                self.rank[left] += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{
        canonical_semantic_hash, ComponentCallbackShape, ComponentFieldBinding, ComponentFieldId,
        ComponentFieldSchema, ComponentSemanticKey, CrossingHookId, DirectComponentEdge, OriginId,
        OriginKind, OriginPermission, OwnerBoundaryKind, OwnerSiteId, PropUse, RenderKind,
        SharedStateReference, StaticSlotCapability, TemporalPropMode, WitnessRecordId,
    };

    fn candidate(id: u32, active: bool, hydration: HydrationConstraint) -> OwnerCandidate {
        OwnerCandidate::new(OwnerCandidateId::new(id), active, hydration)
    }

    fn relation(from: u32, to: u32, kind: OwnerRelationKind) -> OwnerRelation {
        OwnerRelation::new(OwnerCandidateId::new(from), OwnerCandidateId::new(to), kind)
    }

    fn descriptor(name: &str, witness: u32, hydration: HydrationConstraint) -> ComponentDescriptor {
        let mut descriptor = ComponentDescriptor::v1(
            ComponentSemanticKey::v1("acme", "acme", ["ui"], name).unwrap(),
            WitnessRecordId::new(witness),
            RenderKind::WebRenderSync,
            OriginPermission::ParticipantOnly,
            SpanCardinality::Exact(1),
        );
        descriptor.hydration = hydration;
        descriptor
            .origins
            .insert(OriginId::new(0), OriginKind::SourceOnly);
        descriptor
    }

    fn value_child(name: &str, witness: u32) -> ComponentDescriptor {
        let mut child = descriptor(name, witness, HydrationConstraint::Unconstrained);
        child.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "value".to_string(),
                role: ComponentFieldRole::Value,
                type_witness: Some(WitnessRecordId::new(witness + 100)),
                crossing_hook: Some(CrossingHookId::new(0)),
                client_value: Some(crate::abi::LocalValueId::new(0)),
                callback_shape: None,
                codec: ComponentFieldCodec::ClientEncode {
                    schema: canonical_semantic_hash("u32"),
                },
                static_slot: StaticSlotCapability::Unsupported,
            },
        );
        child.crossing_hooks.insert(CrossingHookId::new(0));
        child.values.insert(crate::abi::LocalValueId::new(0));
        child.prop_uses.insert(
            PropSiteId::new(0),
            PropUse {
                field: ComponentFieldId::new(0),
                value: crate::abi::LocalValueId::new(0),
                temporal: TemporalPropMode::LiveRead,
                origin: OriginId::new(0),
            },
        );
        child
    }

    fn callback_child(name: &str, witness: u32) -> ComponentDescriptor {
        let mut child = descriptor(name, witness, HydrationConstraint::Unconstrained);
        child.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "on_change".to_string(),
                role: ComponentFieldRole::Callback,
                type_witness: Some(WitnessRecordId::new(witness + 100)),
                crossing_hook: Some(CrossingHookId::new(0)),
                client_value: Some(crate::abi::LocalValueId::new(0)),
                callback_shape: Some(ComponentCallbackShape::Required),
                codec: ComponentFieldCodec::None,
                static_slot: StaticSlotCapability::Unsupported,
            },
        );
        child.crossing_hooks.insert(CrossingHookId::new(0));
        child.values.insert(crate::abi::LocalValueId::new(0));
        child
    }

    fn attach_edge(
        parent: &mut ComponentDescriptor,
        child: &ComponentDescriptor,
        boundary: ComponentBoundary,
        field_name: &str,
        source: ComponentBindingSource,
    ) {
        parent.values.insert(crate::abi::LocalValueId::new(0));
        let mut bindings = BTreeMap::new();
        bindings.insert(
            field_name.to_string(),
            ComponentFieldBinding {
                source,
                origin: OriginId::new(0),
            },
        );
        parent.direct_components.insert(
            ComponentEdgeId::new(0),
            DirectComponentEdge {
                witness: child.defining_witness,
                boundary,
                bindings,
                lifecycle_control: None,
                origin: OriginId::new(0),
            },
        );
    }

    fn attach_shared_state(descriptor: &mut ComponentDescriptor, type_key: &str, schema: &str) {
        descriptor.shared_state.insert(
            SharedStateRefId::new(0),
            SharedStateReference {
                type_key: type_key.to_string(),
                schema_abi: canonical_semantic_hash(schema),
                origin: OriginId::new(0),
            },
        );
    }

    #[test]
    fn resolved_live_field_binding_fuses_exact_child_witness() {
        let child = value_child("Badge", 11);
        let mut parent = descriptor("Page", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );

        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: false,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        let input = derive_owner_partition_input(&instances, &edges).unwrap();
        assert!(input.relations.contains(&relation(
            0,
            1,
            OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp)
        )));

        let plan = partition_resolved_components(&instances, &edges).unwrap();
        assert_eq!(plan.owners.len(), 1);
        assert_eq!(
            plan.owners[0].candidates,
            [OwnerCandidateId::new(0), OwnerCandidateId::new(1)]
        );
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Visible);
    }

    #[test]
    fn server_only_site_may_bind_a_value_field_without_becoming_live() {
        let child = value_child("Badge", 11);
        let mut parent = descriptor("Page", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::ServerOnly,
        );

        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: false,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        let graph = link_resolved_component_graph(&instances, &edges).unwrap();
        assert!(graph.inputs.live_props.is_empty());
        assert_eq!(
            graph.owner_input.relations,
            [relation(0, 1, OwnerRelationKind::StaticNesting)]
        );

        let plan = partition_resolved_components(&instances, &edges).unwrap();
        assert_eq!(plan.owners.len(), 1);
        assert_eq!(plan.owners[0].candidates, [OwnerCandidateId::new(0)]);
        assert!(!plan
            .candidate_to_owner
            .contains_key(&OwnerCandidateId::new(1)));
    }

    #[test]
    fn binding_roles_still_fail_closed_for_incompatible_sources() {
        let mut server_child = descriptor("ServerBadge", 11, HydrationConstraint::Unconstrained);
        server_child.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "value".to_string(),
                role: ComponentFieldRole::ServerOnly,
                type_witness: None,
                crossing_hook: None,
                client_value: None,
                callback_shape: None,
                codec: ComponentFieldCodec::None,
                static_slot: StaticSlotCapability::Unsupported,
            },
        );
        let mut live_parent = descriptor("LivePage", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut live_parent,
            &server_child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let live_instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &live_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &server_child,
                has_browser_obligation: false,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];
        assert!(matches!(
            link_resolved_component_graph(&live_instances, &edges),
            Err(ComponentGraphLinkError::FieldRoleMismatch {
                role: ComponentFieldRole::ServerOnly,
                binding: "live value",
                ..
            })
        ));

        let callback_child = callback_child("CallbackChild", 12);
        let mut static_parent = descriptor("StaticPage", 8, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut static_parent,
            &callback_child,
            ComponentBoundary::Composed,
            "on_change",
            ComponentBindingSource::ServerOnly,
        );
        let callback_instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &static_parent,
                has_browser_obligation: false,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &callback_child,
                has_browser_obligation: false,
            },
        ];
        assert!(matches!(
            link_resolved_component_graph(&callback_instances, &edges),
            Err(ComponentGraphLinkError::FieldRoleMismatch {
                role: ComponentFieldRole::Callback,
                binding: "server-only input",
                ..
            })
        ));
    }

    #[test]
    fn unused_live_value_does_not_fuse_disconnected_owners() {
        let mut child = value_child("Badge", 11);
        child.values.clear();
        child.prop_uses.clear();
        let mut parent = descriptor("Page", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );

        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        let input = derive_owner_partition_input(&instances, &edges).unwrap();
        assert_eq!(
            input.relations,
            vec![relation(0, 1, OwnerRelationKind::StaticNesting)]
        );
        let plan = partition_resolved_components(&instances, &edges).unwrap();
        assert_eq!(plan.owners.len(), 2);
    }

    #[test]
    fn forbidden_live_prop_temporal_uses_fail_at_link_time() {
        let mut provider_child = value_child("ProviderChild", 11);
        provider_child
            .prop_uses
            .get_mut(&PropSiteId::new(0))
            .unwrap()
            .temporal = TemporalPropMode::ForbiddenProviderSeed;
        let mut provider_parent = descriptor("ProviderPage", 7, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut provider_parent,
            &provider_child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let provider_instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &provider_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &provider_child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];
        assert!(matches!(
            derive_owner_partition_input(&provider_instances, &edges),
            Err(ComponentGraphLinkError::ForbiddenLiveProviderSeed {
                site,
                ..
            }) if site == PropSiteId::new(0)
        ));

        let mut island_child = value_child("IslandChild", 12);
        island_child
            .prop_uses
            .get_mut(&PropSiteId::new(0))
            .unwrap()
            .temporal = TemporalPropMode::ForbiddenBoundaryCrossing;
        let mut island_parent = descriptor("IslandPage", 8, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut island_parent,
            &island_child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let island_instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &island_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &island_child,
                has_browser_obligation: true,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&island_instances, &edges),
            Err(ComponentGraphLinkError::ForbiddenLiveBoundaryCrossing {
                site,
                ..
            }) if site == PropSiteId::new(0)
        ));
    }

    #[test]
    fn every_supported_live_prop_temporal_mode_requires_one_owner() {
        for temporal in [
            TemporalPropMode::LiveRead,
            TemporalPropMode::SeedOnce(PropSiteId::new(0)),
            TemporalPropMode::EffectDependency,
            TemporalPropMode::QueryKeyDependency,
        ] {
            let mut child = value_child("TemporalChild", 11);
            child
                .prop_uses
                .get_mut(&PropSiteId::new(0))
                .unwrap()
                .temporal = temporal;
            let mut parent = descriptor("TemporalPage", 7, HydrationConstraint::Visible);
            attach_edge(
                &mut parent,
                &child,
                ComponentBoundary::Composed,
                "value",
                ComponentBindingSource::LiveValue {
                    value: crate::abi::LocalValueId::new(0),
                },
            );
            let instances = [
                ResolvedComponentInstance {
                    id: OwnerCandidateId::new(0),
                    descriptor: &parent,
                    has_browser_obligation: true,
                },
                ResolvedComponentInstance {
                    id: OwnerCandidateId::new(1),
                    descriptor: &child,
                    has_browser_obligation: true,
                },
            ];
            let edges = [ResolvedComponentEdge {
                parent: OwnerCandidateId::new(0),
                edge: ComponentEdgeId::new(0),
                child: OwnerCandidateId::new(1),
            }];
            let plan = partition_resolved_components(&instances, &edges).unwrap();
            assert_eq!(
                plan.owners.len(),
                1,
                "temporal mode {temporal:?} must preserve its live owner edge"
            );
        }
    }

    #[test]
    fn linked_live_prop_plan_preserves_every_temporal_use() {
        let mut child = value_child("TemporalChild", 11);
        for (site, temporal) in [
            (1, TemporalPropMode::SeedOnce(PropSiteId::new(1))),
            (2, TemporalPropMode::EffectDependency),
            (3, TemporalPropMode::QueryKeyDependency),
        ] {
            child.values.insert(crate::abi::LocalValueId::new(site));
            child.prop_uses.insert(
                PropSiteId::new(site),
                PropUse {
                    field: ComponentFieldId::new(0),
                    value: crate::abi::LocalValueId::new(site),
                    temporal,
                    origin: OriginId::new(0),
                },
            );
        }

        let mut parent = descriptor("TemporalPage", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        let graph = link_resolved_component_graph(&instances, &edges).unwrap();
        assert_eq!(
            graph.inputs.live_props,
            [
                LinkedLivePropUse {
                    parent: OwnerCandidateId::new(0),
                    edge: ComponentEdgeId::new(0),
                    child: OwnerCandidateId::new(1),
                    field: ComponentFieldId::new(0),
                    crossing_hook: CrossingHookId::new(0),
                    parent_value: crate::abi::LocalValueId::new(0),
                    child_value: crate::abi::LocalValueId::new(0),
                    site: PropSiteId::new(0),
                    temporal: TemporalPropMode::LiveRead,
                },
                LinkedLivePropUse {
                    parent: OwnerCandidateId::new(0),
                    edge: ComponentEdgeId::new(0),
                    child: OwnerCandidateId::new(1),
                    field: ComponentFieldId::new(0),
                    crossing_hook: CrossingHookId::new(0),
                    parent_value: crate::abi::LocalValueId::new(0),
                    child_value: crate::abi::LocalValueId::new(1),
                    site: PropSiteId::new(1),
                    temporal: TemporalPropMode::SeedOnce(PropSiteId::new(1)),
                },
                LinkedLivePropUse {
                    parent: OwnerCandidateId::new(0),
                    edge: ComponentEdgeId::new(0),
                    child: OwnerCandidateId::new(1),
                    field: ComponentFieldId::new(0),
                    crossing_hook: CrossingHookId::new(0),
                    parent_value: crate::abi::LocalValueId::new(0),
                    child_value: crate::abi::LocalValueId::new(2),
                    site: PropSiteId::new(2),
                    temporal: TemporalPropMode::EffectDependency,
                },
                LinkedLivePropUse {
                    parent: OwnerCandidateId::new(0),
                    edge: ComponentEdgeId::new(0),
                    child: OwnerCandidateId::new(1),
                    field: ComponentFieldId::new(0),
                    crossing_hook: CrossingHookId::new(0),
                    parent_value: crate::abi::LocalValueId::new(0),
                    child_value: crate::abi::LocalValueId::new(3),
                    site: PropSiteId::new(3),
                    temporal: TemporalPropMode::QueryKeyDependency,
                },
            ]
        );
        assert!(graph.owner_input.relations.contains(&relation(
            0,
            1,
            OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp)
        )));
    }

    #[test]
    fn linked_callback_plan_requires_invocation_time_resolution() {
        let child = callback_child("CallbackChild", 11);
        let mut parent = descriptor("CallbackPage", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "on_change",
            ComponentBindingSource::LiveCallback {
                value: crate::abi::LocalValueId::new(0),
                shape: ComponentCallbackShape::Required,
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        let graph = link_resolved_component_graph(&instances, &edges).unwrap();
        assert_eq!(
            graph.inputs.live_callbacks,
            [LinkedLiveCallback {
                parent: OwnerCandidateId::new(0),
                edge: ComponentEdgeId::new(0),
                child: OwnerCandidateId::new(1),
                field: ComponentFieldId::new(0),
                crossing_hook: CrossingHookId::new(0),
                parent_value: crate::abi::LocalValueId::new(0),
                child_value: crate::abi::LocalValueId::new(0),
                shape: ComponentCallbackShape::Required,
                dispatch: LinkedCallbackDispatch::ResolveCurrent,
            }]
        );
        assert!(graph.owner_input.relations.contains(&relation(
            0,
            1,
            OwnerRelationKind::Coupling(OwnerCouplingKind::LiveCallback)
        )));
    }

    #[test]
    fn live_input_without_a_crossing_hook_fails_closed() {
        let mut child = value_child("BrokenChild", 11);
        child
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap()
            .crossing_hook = None;
        let mut parent = descriptor("BrokenPage", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];

        assert!(matches!(
            link_resolved_component_graph(&instances, &edges),
            Err(ComponentGraphLinkError::MissingChildCrossingHook {
                field,
                ..
            }) if field == ComponentFieldId::new(0)
        ));
    }

    #[test]
    fn resolved_edges_fail_on_field_role_and_codec_mismatches() {
        let child = value_child("Badge", 11);
        let mut parent = descriptor("Page", 7, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "missing",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: false,
            },
        ];
        let edge = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];
        assert!(matches!(
            derive_owner_partition_input(&instances, &edge),
            Err(ComponentGraphLinkError::UnknownChildField { .. })
        ));

        let mut codec_parent = descriptor("Page", 7, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut codec_parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::SerializedSeed {
                value: crate::abi::LocalValueId::new(0),
                codec: canonical_semantic_hash("String"),
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &codec_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: false,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&instances, &edge),
            Err(ComponentGraphLinkError::FieldCodecMismatch { .. })
        ));

        let callback_child = callback_child("CallbackChild", 12);
        let mut callback_parent = descriptor("CallbackPage", 8, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut callback_parent,
            &callback_child,
            ComponentBoundary::Composed,
            "on_change",
            ComponentBindingSource::LiveCallback {
                value: crate::abi::LocalValueId::new(0),
                shape: ComponentCallbackShape::Optional,
            },
        );
        let callback_instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &callback_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &callback_child,
                has_browser_obligation: true,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&callback_instances, &edge),
            Err(ComponentGraphLinkError::CallbackShapeMismatch {
                expected: ComponentCallbackShape::Required,
                actual: ComponentCallbackShape::Optional,
                ..
            })
        ));
    }

    #[test]
    fn immutable_static_slot_crosses_island_without_owner_fusion() {
        let mut child = descriptor("Disclosure", 11, HydrationConstraint::Idle);
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

        let mut parent = descriptor("Page", 7, HydrationConstraint::Visible);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::IndependentIsland,
            "children",
            ComponentBindingSource::StaticSlot {
                span: SpanCardinality::Unknown {
                    region: crate::abi::RegionSiteId::new(0),
                    anchored: true,
                },
            },
        );
        parent.region_sites.insert(crate::abi::RegionSiteId::new(0));

        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        let edges = [ResolvedComponentEdge {
            parent: OwnerCandidateId::new(0),
            edge: ComponentEdgeId::new(0),
            child: OwnerCandidateId::new(1),
        }];
        let plan = partition_resolved_components(&instances, &edges).unwrap();
        assert_eq!(plan.owners.len(), 2);
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Visible);
        assert_eq!(plan.owners[1].strategy, OwnerActivationStrategy::Idle);

        let mut arbitrary_parent = parent.clone();
        arbitrary_parent
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .bindings
            .get_mut("children")
            .unwrap()
            .source = ComponentBindingSource::ServerOnly;
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &arbitrary_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&instances, &edges),
            Err(ComponentGraphLinkError::FieldRoleMismatch {
                field_name,
                role: ComponentFieldRole::Slot,
                binding: "server-only input",
                ..
            }) if field_name == "children"
        ));

        let mut live_parent = parent.clone();
        live_parent
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .bindings
            .get_mut("children")
            .unwrap()
            .source = ComponentBindingSource::LiveSlot {
            control: crate::abi::LocalControlRegionId::new(0),
        };
        live_parent
            .control_regions
            .insert(crate::abi::LocalControlRegionId::new(0));
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &live_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&instances, &edges),
            Err(ComponentGraphLinkError::LiveSlotUnsupported { .. })
        ));

        let mut nested_parent = parent;
        nested_parent
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .bindings
            .get_mut("children")
            .unwrap()
            .source = ComponentBindingSource::StaticSlot {
            span: SpanCardinality::OwnerBoundary {
                owner: OwnerSiteId::new(0),
                kind: OwnerBoundaryKind::NestedOwner,
            },
        };
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &nested_parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: true,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&instances, &edges),
            Err(ComponentGraphLinkError::NestedOwnerInStaticSlotUnsupported { .. })
        ));
    }

    #[test]
    fn every_direct_component_site_requires_one_exact_resolution() {
        let child = value_child("Badge", 11);
        let mut parent = descriptor("Page", 7, HydrationConstraint::Unconstrained);
        attach_edge(
            &mut parent,
            &child,
            ComponentBoundary::Composed,
            "value",
            ComponentBindingSource::LiveValue {
                value: crate::abi::LocalValueId::new(0),
            },
        );
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &parent,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &child,
                has_browser_obligation: false,
            },
        ];
        assert!(matches!(
            derive_owner_partition_input(&instances, &[]),
            Err(ComponentGraphLinkError::UnresolvedDirectEdge { .. })
        ));
    }

    #[test]
    fn shared_state_schemas_deduplicate_or_fail_before_activation() {
        let mut toolbar = descriptor("Toolbar", 1, HydrationConstraint::Visible);
        attach_shared_state(&mut toolbar, "acme.workbench", "schema-v1");
        let mut inspector = descriptor("Inspector", 2, HydrationConstraint::Idle);
        attach_shared_state(&mut inspector, "acme.workbench", "schema-v1");
        let instances = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &toolbar,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &inspector,
                has_browser_obligation: true,
            },
        ];

        let linked = link_shared_state_schemas(&instances).unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].type_key, "acme.workbench");
        assert_eq!(
            linked[0].consumers,
            [
                SharedStateConsumer {
                    component: OwnerCandidateId::new(0),
                    reference: SharedStateRefId::new(0),
                    origin: OriginId::new(0),
                },
                SharedStateConsumer {
                    component: OwnerCandidateId::new(1),
                    reference: SharedStateRefId::new(0),
                    origin: OriginId::new(0),
                },
            ]
        );

        inspector
            .shared_state
            .get_mut(&SharedStateRefId::new(0))
            .unwrap()
            .schema_abi = canonical_semantic_hash("schema-v2");
        let conflicting = [
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(0),
                descriptor: &toolbar,
                has_browser_obligation: true,
            },
            ResolvedComponentInstance {
                id: OwnerCandidateId::new(1),
                descriptor: &inspector,
                has_browser_obligation: true,
            },
        ];
        let error = link_shared_state_schemas(&conflicting).unwrap_err();
        assert_eq!(error.type_key, "acme.workbench");
        assert_ne!(error.first_schema, error.second_schema);
        assert!(error.to_string().contains("before any owner activates"));
    }

    #[test]
    fn disconnected_active_children_remain_separate_owners() {
        let candidates = [
            candidate(0, false, HydrationConstraint::Unconstrained),
            candidate(1, true, HydrationConstraint::Visible),
            candidate(
                2,
                true,
                HydrationConstraint::Interaction {
                    event: Some("click".to_string()),
                },
            ),
        ];
        let relations = [
            relation(0, 1, OwnerRelationKind::StaticNesting),
            relation(0, 2, OwnerRelationKind::StaticNesting),
        ];

        let plan = partition_owners(&candidates, &relations).unwrap();
        assert_eq!(plan.owners.len(), 2);
        assert_eq!(plan.owners[0].candidates, [OwnerCandidateId::new(1)]);
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Visible);
        assert_eq!(plan.owners[1].candidates, [OwnerCandidateId::new(2)]);
        assert_eq!(
            plan.owners[1].strategy,
            OwnerActivationStrategy::Interaction {
                event: Some("click".to_string())
            }
        );
        assert!(!plan
            .candidate_to_owner
            .contains_key(&OwnerCandidateId::new(0)));
    }

    #[test]
    fn live_prop_fuses_participant_and_propagates_owner_activation() {
        let candidates = [
            candidate(0, true, HydrationConstraint::Visible),
            candidate(1, false, HydrationConstraint::Unconstrained),
            candidate(2, false, HydrationConstraint::Unconstrained),
        ];
        let relations = [
            relation(
                0,
                1,
                OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp),
            ),
            relation(
                1,
                2,
                OwnerRelationKind::Coupling(OwnerCouplingKind::LiveCallback),
            ),
        ];

        let plan = partition_owners(&candidates, &relations).unwrap();
        assert_eq!(plan.owners.len(), 1);
        assert_eq!(
            plan.owners[0].candidates,
            [
                OwnerCandidateId::new(0),
                OwnerCandidateId::new(1),
                OwnerCandidateId::new(2)
            ]
        );
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Visible);
    }

    #[test]
    fn coupling_across_an_indirect_island_cut_fails_with_the_path() {
        let candidates = [
            candidate(0, true, HydrationConstraint::Unconstrained),
            candidate(1, false, HydrationConstraint::Unconstrained),
            candidate(2, true, HydrationConstraint::Unconstrained),
        ];
        let relations = [
            relation(
                0,
                1,
                OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp),
            ),
            relation(
                1,
                2,
                OwnerRelationKind::Coupling(OwnerCouplingKind::ReactiveLifecycle),
            ),
            relation(
                0,
                2,
                OwnerRelationKind::Cut(OwnerCutKind::IndependentIsland),
            ),
        ];

        let error = partition_owners(&candidates, &relations).unwrap_err();
        let OwnerPartitionError::CouplingCrossesCut {
            cut_from,
            cut_to,
            coupling_path,
            ..
        } = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(cut_from, OwnerCandidateId::new(0));
        assert_eq!(cut_to, OwnerCandidateId::new(2));
        assert_eq!(coupling_path.len(), 2);
        assert_eq!(coupling_path[0].kind, OwnerCouplingKind::LiveProp);
        assert_eq!(coupling_path[1].kind, OwnerCouplingKind::ReactiveLifecycle);
    }

    #[test]
    fn incompatible_constraints_fail_with_the_fusion_path() {
        let candidates = [
            candidate(0, true, HydrationConstraint::Visible),
            candidate(1, false, HydrationConstraint::Unconstrained),
            candidate(2, true, HydrationConstraint::Idle),
        ];
        let relations = [
            relation(
                0,
                1,
                OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp),
            ),
            relation(
                1,
                2,
                OwnerRelationKind::Coupling(OwnerCouplingKind::LiveCallback),
            ),
        ];

        let error = partition_owners(&candidates, &relations).unwrap_err();
        let OwnerPartitionError::StrategyConflict {
            first,
            second,
            first_strategy,
            second_strategy,
            coupling_path,
        } = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(first, OwnerCandidateId::new(0));
        assert_eq!(second, OwnerCandidateId::new(2));
        assert_eq!(first_strategy, OwnerActivationStrategy::Visible);
        assert_eq!(second_strategy, OwnerActivationStrategy::Idle);
        assert_eq!(coupling_path.len(), 2);
    }

    #[test]
    fn different_interaction_events_conflict_inside_one_owner() {
        let candidates = [
            candidate(
                0,
                true,
                HydrationConstraint::Interaction {
                    event: Some("pointerdown".to_string()),
                },
            ),
            candidate(
                1,
                true,
                HydrationConstraint::Interaction {
                    event: Some("click".to_string()),
                },
            ),
        ];
        let relations = [relation(
            0,
            1,
            OwnerRelationKind::Coupling(OwnerCouplingKind::LiveCallback),
        )];

        assert!(matches!(
            partition_owners(&candidates, &relations),
            Err(OwnerPartitionError::StrategyConflict {
                first_strategy: OwnerActivationStrategy::Interaction { .. },
                second_strategy: OwnerActivationStrategy::Interaction { .. },
                ..
            })
        ));
    }

    #[test]
    fn explicit_island_cut_preserves_separate_active_owners() {
        let candidates = [
            candidate(0, true, HydrationConstraint::Visible),
            candidate(1, true, HydrationConstraint::Idle),
        ];
        let relations = [relation(
            0,
            1,
            OwnerRelationKind::Cut(OwnerCutKind::IndependentIsland),
        )];

        let plan = partition_owners(&candidates, &relations).unwrap();
        assert_eq!(plan.owners.len(), 2);
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Visible);
        assert_eq!(plan.owners[1].strategy, OwnerActivationStrategy::Idle);
    }

    #[test]
    fn equal_constraints_and_duplicate_relations_are_deterministic() {
        let candidates = [
            candidate(0, true, HydrationConstraint::Visible),
            candidate(1, true, HydrationConstraint::Visible),
        ];
        let edge = relation(
            1,
            0,
            OwnerRelationKind::Coupling(OwnerCouplingKind::ClientHandle),
        );

        let forward = partition_owners(&candidates, &[edge, edge]).unwrap();
        let reversed_candidates = [candidates[1].clone(), candidates[0].clone()];
        let reversed = partition_owners(&reversed_candidates, &[edge]).unwrap();
        assert_eq!(forward, reversed);
        assert_eq!(forward.owners[0].strategy, OwnerActivationStrategy::Visible);
        assert_eq!(
            crate::abi::canonical_semantic_hash(&forward),
            crate::abi::canonical_semantic_hash(&reversed)
        );
    }

    #[test]
    fn all_inactive_coupling_component_forms_no_owner() {
        let candidates = [
            candidate(0, false, HydrationConstraint::Visible),
            candidate(1, false, HydrationConstraint::Visible),
        ];
        let relations = [relation(
            0,
            1,
            OwnerRelationKind::Coupling(OwnerCouplingKind::LiveProp),
        )];

        let plan = partition_owners(&candidates, &relations).unwrap();
        assert!(plan.owners.is_empty());
        assert!(plan.candidate_to_owner.is_empty());
    }

    #[test]
    fn unconstrained_active_owner_defaults_to_eager() {
        let candidates = [candidate(0, true, HydrationConstraint::Unconstrained)];
        let plan = partition_owners(&candidates, &[]).unwrap();
        assert_eq!(plan.owners[0].strategy, OwnerActivationStrategy::Eager);
    }

    #[test]
    fn rejects_sparse_or_unknown_candidate_ids() {
        let sparse = [candidate(1, true, HydrationConstraint::Unconstrained)];
        assert!(matches!(
            partition_owners(&sparse, &[]),
            Err(OwnerPartitionError::NonDenseCandidate { .. })
        ));

        let candidates = [candidate(0, true, HydrationConstraint::Unconstrained)];
        let relations = [relation(0, 1, OwnerRelationKind::StaticNesting)];
        assert_eq!(
            partition_owners(&candidates, &relations),
            Err(OwnerPartitionError::UnknownCandidate {
                candidate: OwnerCandidateId::new(1)
            })
        );
    }
}
