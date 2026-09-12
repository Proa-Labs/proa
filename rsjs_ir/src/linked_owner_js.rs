// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic JavaScript emission for a linked RSJS owner.
//!
//! A linked owner is one browser activation boundary containing one or more
//! reusable component fragments. The semantic linker decides the owner
//! partition and supplies this module with a parent-before-child mount plan.
//! This emitter deliberately does not infer missing graph facts: malformed
//! topology, aliases, callbacks, or browser-facing strings fail before any
//! JavaScript is emitted.

use crate::emit_js::js_string;
use crate::link::OwnerActivationStrategy;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

/// Marker placed on every server-rendered wrapper participating in a linked
/// owner.
pub const LINKED_CANDIDATE_ATTR: &str = "data-rsjs-linked-candidate";

const ISLAND_ROOT_ATTR: &str = "data-rsjs-island-root";
const RUNTIME_MODULE: &str = "/rsjs-runtime.js";

/// One exact-one-element component fragment mounted by a linked owner.
///
/// `parent_candidate_id` is `None` only for the owner root. The mount list in
/// [`LinkedOwnerModulePlan`] must place every parent before its children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedOwnerMount {
    /// Stable candidate identity used by linker edges and the SSR marker.
    pub candidate_id: String,
    /// Direct logical parent, or `None` for the owner root.
    pub parent_candidate_id: Option<String>,
    /// Reactive lifecycle group that owns this mount, when the candidate is
    /// present only while one parent control branch is active.
    ///
    /// The lifecycle root names itself. Every statically present descendant
    /// in that lifecycle subtree names the same root.
    pub lifecycle_root_candidate_id: Option<String>,
    /// Fragment asset identity used in `component_<asset>.js`.
    pub fragment_asset_id: String,
    /// Exact exported mount function in the fragment module.
    pub fragment_export: String,
    /// Key on the wrapper and adjacent signal seed script.
    pub seed_key: String,
}

/// One live-signal alias from a parent fragment into a direct child fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkedOwnerSignalAlias {
    /// Candidate exporting the source signal.
    pub parent_candidate_id: String,
    /// Producer-local signal id in the parent fragment.
    pub parent_signal_id: u32,
    /// Direct child receiving the signal handle.
    pub child_candidate_id: String,
    /// Producer-local signal id overridden in the child fragment.
    pub child_signal_id: u32,
}

/// One child-owned signal initialized from a direct parent's current signal
/// value every time the child occurrence mounts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkedOwnerSignalSeed {
    /// Candidate exporting the source signal.
    pub parent_candidate_id: String,
    /// Producer-local signal id in the parent fragment.
    pub parent_signal_id: u32,
    /// Direct child receiving a freshly sampled value.
    pub child_candidate_id: String,
    /// Producer-local writable signal id initialized in the child fragment.
    pub child_signal_id: u32,
}

/// One exact node-ref object aliased from a parent fragment into a direct
/// child fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkedOwnerNodeRefAlias {
    /// Candidate exporting the source ref object.
    pub parent_candidate_id: String,
    /// Producer-local node-ref id in the parent fragment.
    pub parent_node_ref_id: u32,
    /// Direct child receiving the same ref object.
    pub child_candidate_id: String,
    /// Producer-local node-ref id overridden in the child fragment.
    pub child_node_ref_id: u32,
}

/// One callback connection from a child callback slot to a parent payload
/// handler.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkedOwnerCallbackEdge {
    /// Candidate exporting the payload handler.
    pub parent_candidate_id: String,
    /// Producer-local prelude binding id for the payload handler.
    pub parent_payload_handler_id: u32,
    /// Direct child owning the callback slot.
    pub child_candidate_id: String,
    /// Producer-local callback-slot id in the child fragment.
    pub child_callback_slot_id: u32,
}

/// Branch polarity for one conditional component lifecycle group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkedOwnerLifecycleBranch {
    /// Mount while the parent control predicate is true.
    Then,
    /// Mount while the parent control predicate is false.
    Else,
}

/// One conditional component subtree mounted and disposed as a unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedOwnerLifecycle {
    /// Candidate at the root of the conditionally active subtree.
    pub root_candidate_id: String,
    /// Unconditional direct parent exporting `control_id`.
    pub parent_candidate_id: String,
    /// Producer-local lifecycle-control id in the parent fragment.
    pub control_id: u32,
    /// Predicate polarity that activates this group.
    pub branch: LinkedOwnerLifecycleBranch,
    /// Exact group members in parent-before-child mount order.
    pub candidates: Vec<String>,
}

/// Complete input to the linked-owner JavaScript emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedOwnerModulePlan {
    /// Unique browser asset and hydration registration identity for the owner.
    pub owner_asset_id: String,
    /// Candidate whose wrapper is the owner hydration root.
    pub root_candidate_id: String,
    /// Final linked activation strategy.
    pub strategy: OwnerActivationStrategy,
    /// Whether any mounted fragment has reachable query work.
    ///
    /// This is manifest/runtime planning metadata. Fragment query registration
    /// still resolves response-local metadata from each mount's `seedRoot`.
    pub uses_queries: bool,
    /// Whether any mounted fragment has reachable mutation work.
    ///
    /// This is manifest/runtime planning metadata. Each fragment registers its
    /// exact mutation specs from producer-authored Client IR and resolves
    /// response-local metadata through its own `seedRoot`.
    pub uses_mutations: bool,
    /// Exact-one-element mounts in parent-before-child order.
    pub mounts: Vec<LinkedOwnerMount>,
    /// Live signal aliases. Input order is ignored during emission.
    pub signal_aliases: Vec<LinkedOwnerSignalAlias>,
    /// Child-owned per-mount signal seeds. Input order is ignored during
    /// emission.
    pub signal_seeds: Vec<LinkedOwnerSignalSeed>,
    /// Callback edges. Input order is ignored during emission.
    pub callback_edges: Vec<LinkedOwnerCallbackEdge>,
    /// Conditional lifecycle groups. Input order is ignored during emission;
    /// each group's `candidates` remain ordered.
    pub lifecycles: Vec<LinkedOwnerLifecycle>,
}

/// A fail-closed linked-owner plan validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkedOwnerJsError {
    /// The owner asset is not a safe generated asset id.
    InvalidOwnerAssetId(String),
    /// A candidate id cannot be represented as the generated SSR marker.
    InvalidCandidateId(String),
    /// A fragment asset id could escape or corrupt its generated import path.
    InvalidFragmentAssetId(String),
    /// A fragment export is not a JavaScript identifier.
    InvalidFragmentExport(String),
    /// A seed key cannot be represented as an SSR marker.
    InvalidSeedKey(String),
    /// Interaction event syntax is invalid.
    InvalidInteractionEvent(String),
    /// Media-query syntax is empty, untrimmed, too long, or contains controls.
    InvalidMediaQuery(String),
    /// Two mount records declare the same candidate.
    DuplicateCandidate(String),
    /// Two wrappers in the owner use the same signal seed key.
    DuplicateSeedKey(String),
    /// No mount record declares the configured owner root.
    MissingRootCandidate(String),
    /// A non-root candidate declared itself as another root.
    UnexpectedRootCandidate(String),
    /// The configured root has a parent.
    RootHasParent(String),
    /// The owner root is not the first parent-before-child mount.
    RootIsNotFirst(String),
    /// A mount names a parent absent from the owner plan.
    MissingParent {
        /// Child candidate.
        child: String,
        /// Missing parent candidate.
        parent: String,
    },
    /// The parent graph contains a cycle.
    ParentCycle(String),
    /// A child precedes its parent in the supplied mount order.
    ParentAppearsAfterChild {
        /// Child candidate.
        child: String,
        /// Parent candidate.
        parent: String,
    },
    /// The root seed key must equal the owner hydration identity.
    RootSeedDoesNotMatchOwner {
        /// Root seed key.
        seed_key: String,
        /// Owner asset id.
        owner_asset_id: String,
    },
    /// A signal alias references a candidate absent from the owner.
    UnknownSignalEndpoint(String),
    /// A signal alias does not connect a direct parent and child.
    SignalParentMismatch {
        /// Declared parent endpoint.
        parent: String,
        /// Declared child endpoint.
        child: String,
    },
    /// More than one alias targets the same child-local signal.
    DuplicateSignalTarget {
        /// Child candidate.
        child: String,
        /// Child-local signal id.
        signal_id: u32,
    },
    /// A node-ref alias references a candidate absent from the owner.
    UnknownNodeRefEndpoint(String),
    /// A node-ref alias does not connect a direct parent and child.
    NodeRefParentMismatch {
        /// Declared parent endpoint.
        parent: String,
        /// Declared child endpoint.
        child: String,
    },
    /// More than one alias targets the same child-local node ref.
    DuplicateNodeRefTarget {
        /// Child candidate.
        child: String,
        /// Child-local node-ref id.
        node_ref_id: u32,
    },
    /// Node-ref aliases require one statically present child occurrence.
    NodeRefTargetIsConditional(String),
    /// A per-mount signal seed references a candidate absent from the owner.
    UnknownSignalSeedEndpoint(String),
    /// A per-mount signal seed does not connect a direct parent and child.
    SignalSeedParentMismatch {
        /// Declared parent endpoint.
        parent: String,
        /// Declared child endpoint.
        child: String,
    },
    /// A callback edge references a candidate absent from the owner.
    UnknownCallbackEndpoint(String),
    /// A callback source is not an ancestor of its consuming child.
    CallbackParentMismatch {
        /// Declared parent endpoint.
        parent: String,
        /// Declared child endpoint.
        child: String,
    },
    /// More than one parent handler targets the same child callback slot.
    DuplicateCallbackTarget {
        /// Child candidate.
        child: String,
        /// Child-local callback-slot id.
        callback_slot_id: u32,
    },
    /// Two lifecycle records declare the same root candidate.
    DuplicateLifecycleRoot(String),
    /// A mount references a lifecycle root absent from the plan.
    MissingLifecycle(String),
    /// A lifecycle candidate list references a mount absent from the owner.
    UnknownLifecycleCandidate(String),
    /// The owner root cannot be conditionally mounted.
    OwnerRootHasLifecycle(String),
    /// A lifecycle candidate list is empty.
    EmptyLifecycleCandidates(String),
    /// The lifecycle root is not the first candidate in its group.
    LifecycleRootIsNotFirst(String),
    /// The lifecycle candidate list does not exactly match the mount tags.
    LifecycleCandidatesMismatch(String),
    /// The lifecycle root's declared parent does not match the mount topology.
    LifecycleParentMismatch {
        /// Lifecycle root candidate.
        root: String,
        /// Declared parent candidate.
        parent: String,
    },
    /// Nested/crossing lifecycle groups are not supported.
    LifecycleParentIsConditional {
        /// Lifecycle root candidate.
        root: String,
        /// Conditional parent candidate.
        parent: String,
    },
    /// A non-root lifecycle candidate's direct parent is outside its group.
    LifecycleCandidateParentMismatch {
        /// Lifecycle root candidate.
        root: String,
        /// Candidate whose parent escapes the group.
        candidate: String,
    },
    /// A direct child of a lifecycle member does not carry the same lifecycle
    /// root, so the group would not own its complete mounted subtree.
    LifecycleSubtreeNotClosed {
        /// Lifecycle root candidate.
        root: String,
        /// Lifecycle member with the escaping child.
        parent: String,
        /// Direct child outside the lifecycle group.
        child: String,
    },
}

impl fmt::Display for LinkedOwnerJsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOwnerAssetId(value) => {
                write!(f, "invalid linked-owner asset id `{value}`")
            }
            Self::InvalidCandidateId(value) => {
                write!(f, "invalid linked-owner candidate id `{value}`")
            }
            Self::InvalidFragmentAssetId(value) => {
                write!(f, "invalid linked-owner fragment asset id `{value}`")
            }
            Self::InvalidFragmentExport(value) => {
                write!(f, "invalid linked-owner fragment export `{value}`")
            }
            Self::InvalidSeedKey(value) => {
                write!(f, "invalid linked-owner signal seed key `{value}`")
            }
            Self::InvalidInteractionEvent(value) => {
                write!(f, "invalid linked-owner interaction event `{value}`")
            }
            Self::InvalidMediaQuery(value) => {
                write!(f, "invalid linked-owner media query `{value}`")
            }
            Self::DuplicateCandidate(candidate) => {
                write!(f, "duplicate linked-owner candidate `{candidate}`")
            }
            Self::DuplicateSeedKey(seed_key) => {
                write!(f, "duplicate linked-owner signal seed key `{seed_key}`")
            }
            Self::MissingRootCandidate(candidate) => {
                write!(f, "linked-owner root candidate `{candidate}` is missing")
            }
            Self::UnexpectedRootCandidate(candidate) => {
                write!(f, "non-root candidate `{candidate}` has no parent")
            }
            Self::RootHasParent(candidate) => {
                write!(f, "linked-owner root candidate `{candidate}` has a parent")
            }
            Self::RootIsNotFirst(candidate) => write!(
                f,
                "linked-owner root candidate `{candidate}` is not the first mount"
            ),
            Self::MissingParent { child, parent } => write!(
                f,
                "linked-owner candidate `{child}` references missing parent `{parent}`"
            ),
            Self::ParentCycle(candidate) => write!(
                f,
                "linked-owner parent graph contains a cycle through `{candidate}`"
            ),
            Self::ParentAppearsAfterChild { child, parent } => write!(
                f,
                "linked-owner parent `{parent}` must appear before child `{child}`"
            ),
            Self::RootSeedDoesNotMatchOwner {
                seed_key,
                owner_asset_id,
            } => write!(
                f,
                "linked-owner root seed `{seed_key}` must match owner asset `{owner_asset_id}`"
            ),
            Self::UnknownSignalEndpoint(candidate) => write!(
                f,
                "linked-owner signal alias references unknown candidate `{candidate}`"
            ),
            Self::SignalParentMismatch { parent, child } => write!(
                f,
                "linked-owner signal alias `{parent}` -> `{child}` is not a direct parent edge"
            ),
            Self::DuplicateSignalTarget { child, signal_id } => write!(
                f,
                "linked-owner child `{child}` signal {signal_id} has multiple linked inputs"
            ),
            Self::UnknownNodeRefEndpoint(candidate) => write!(
                f,
                "linked-owner node-ref alias references unknown candidate `{candidate}`"
            ),
            Self::NodeRefParentMismatch { parent, child } => write!(
                f,
                "linked-owner node-ref alias `{parent}` -> `{child}` is not a direct parent edge"
            ),
            Self::DuplicateNodeRefTarget { child, node_ref_id } => write!(
                f,
                "linked-owner child `{child}` node ref {node_ref_id} has multiple linked inputs"
            ),
            Self::NodeRefTargetIsConditional(child) => write!(
                f,
                "linked-owner node-ref target `{child}` is not statically present"
            ),
            Self::UnknownSignalSeedEndpoint(candidate) => write!(
                f,
                "linked-owner signal seed references unknown candidate `{candidate}`"
            ),
            Self::SignalSeedParentMismatch { parent, child } => write!(
                f,
                "linked-owner signal seed `{parent}` -> `{child}` is not a direct parent edge"
            ),
            Self::UnknownCallbackEndpoint(candidate) => write!(
                f,
                "linked-owner callback references unknown candidate `{candidate}`"
            ),
            Self::CallbackParentMismatch { parent, child } => write!(
                f,
                "linked-owner callback source `{parent}` is not an ancestor of `{child}`"
            ),
            Self::DuplicateCallbackTarget {
                child,
                callback_slot_id,
            } => write!(
                f,
                "linked-owner child `{child}` callback slot {callback_slot_id} has multiple sources"
            ),
            Self::DuplicateLifecycleRoot(root) => {
                write!(f, "duplicate linked-owner lifecycle root `{root}`")
            }
            Self::MissingLifecycle(root) => {
                write!(f, "linked-owner mount references missing lifecycle `{root}`")
            }
            Self::UnknownLifecycleCandidate(candidate) => write!(
                f,
                "linked-owner lifecycle references unknown candidate `{candidate}`"
            ),
            Self::OwnerRootHasLifecycle(root) => {
                write!(f, "linked-owner root candidate `{root}` cannot be conditional")
            }
            Self::EmptyLifecycleCandidates(root) => {
                write!(f, "linked-owner lifecycle `{root}` has no candidates")
            }
            Self::LifecycleRootIsNotFirst(root) => write!(
                f,
                "linked-owner lifecycle root `{root}` is not its first candidate"
            ),
            Self::LifecycleCandidatesMismatch(root) => write!(
                f,
                "linked-owner lifecycle `{root}` candidates do not match mount ownership"
            ),
            Self::LifecycleParentMismatch { root, parent } => write!(
                f,
                "linked-owner lifecycle root `{root}` is not a direct child of `{parent}`"
            ),
            Self::LifecycleParentIsConditional { root, parent } => write!(
                f,
                "linked-owner lifecycle `{root}` has conditional parent `{parent}`"
            ),
            Self::LifecycleCandidateParentMismatch { root, candidate } => write!(
                f,
                "linked-owner lifecycle `{root}` candidate `{candidate}` has a parent outside the group"
            ),
            Self::LifecycleSubtreeNotClosed {
                root,
                parent,
                child,
            } => write!(
                f,
                "linked-owner lifecycle `{root}` is not subtree-closed: child `{child}` of `{parent}` does not carry the same lifecycle root"
            ),
        }
    }
}

impl std::error::Error for LinkedOwnerJsError {}

/// Validate and emit one self-registering linked-owner JavaScript module.
///
/// Fragment mounts are expected to accept
/// `{ seedRoot, islandRoot, signals, signalSeeds, nodeRefs, preserveDirtyControls,
/// expose }`. They must populate
/// `expose.signals[producerSignalId]` and
/// `expose.nodeRefs[producerNodeRefId]`,
/// `expose.payloadHandlers[preludeLocalId]` for linked edges that target a
/// later mount, plus `expose.lifecycleControls[controlId]` for parent
/// predicates that govern conditional child groups.
pub fn emit_linked_owner_module(
    plan: &LinkedOwnerModulePlan,
) -> Result<String, LinkedOwnerJsError> {
    let validated = ValidatedPlan::new(plan, &[])?;
    Ok(validated.emit())
}

/// Validate and emit one linked-owner module with exact node-ref aliases.
///
/// This additive entry point keeps the base plan stable while the semantic
/// adapter adopts the handle edge. Imported refs are authenticated as direct,
/// static parent-child edges and are never reconstructed or serialized.
pub fn emit_linked_owner_module_with_node_refs<'a>(
    plan: &'a LinkedOwnerModulePlan,
    node_ref_aliases: &'a [LinkedOwnerNodeRefAlias],
) -> Result<String, LinkedOwnerJsError> {
    let validated = ValidatedPlan::new(plan, node_ref_aliases)?;
    Ok(validated.emit())
}

struct ValidatedPlan<'a> {
    plan: &'a LinkedOwnerModulePlan,
    mount_indices: BTreeMap<&'a str, usize>,
    import_indices: BTreeMap<(&'a str, &'a str), usize>,
    signal_aliases: Vec<&'a LinkedOwnerSignalAlias>,
    signal_seeds: Vec<&'a LinkedOwnerSignalSeed>,
    node_ref_aliases: Vec<&'a LinkedOwnerNodeRefAlias>,
    callback_edges: Vec<&'a LinkedOwnerCallbackEdge>,
    lifecycles: Vec<&'a LinkedOwnerLifecycle>,
}

impl<'a> ValidatedPlan<'a> {
    fn new(
        plan: &'a LinkedOwnerModulePlan,
        node_ref_aliases: &'a [LinkedOwnerNodeRefAlias],
    ) -> Result<Self, LinkedOwnerJsError> {
        validate_asset_id(&plan.owner_asset_id)
            .then_some(())
            .ok_or_else(|| LinkedOwnerJsError::InvalidOwnerAssetId(plan.owner_asset_id.clone()))?;
        validate_marker_value(&plan.root_candidate_id)
            .then_some(())
            .ok_or_else(|| {
                LinkedOwnerJsError::InvalidCandidateId(plan.root_candidate_id.clone())
            })?;
        validate_strategy(&plan.strategy)?;

        let mut mount_indices = BTreeMap::new();
        let mut seed_keys = BTreeSet::new();
        let mut import_pairs = BTreeSet::new();
        for (index, mount) in plan.mounts.iter().enumerate() {
            if !validate_marker_value(&mount.candidate_id) {
                return Err(LinkedOwnerJsError::InvalidCandidateId(
                    mount.candidate_id.clone(),
                ));
            }
            if let Some(parent) = &mount.parent_candidate_id {
                if !validate_marker_value(parent) {
                    return Err(LinkedOwnerJsError::InvalidCandidateId(parent.clone()));
                }
            }
            if let Some(lifecycle_root) = &mount.lifecycle_root_candidate_id {
                if !validate_marker_value(lifecycle_root) {
                    return Err(LinkedOwnerJsError::InvalidCandidateId(
                        lifecycle_root.clone(),
                    ));
                }
            }
            if !validate_asset_id(&mount.fragment_asset_id) {
                return Err(LinkedOwnerJsError::InvalidFragmentAssetId(
                    mount.fragment_asset_id.clone(),
                ));
            }
            if !validate_js_identifier(&mount.fragment_export) {
                return Err(LinkedOwnerJsError::InvalidFragmentExport(
                    mount.fragment_export.clone(),
                ));
            }
            if !validate_marker_value(&mount.seed_key) {
                return Err(LinkedOwnerJsError::InvalidSeedKey(mount.seed_key.clone()));
            }
            if mount_indices
                .insert(mount.candidate_id.as_str(), index)
                .is_some()
            {
                return Err(LinkedOwnerJsError::DuplicateCandidate(
                    mount.candidate_id.clone(),
                ));
            }
            if !seed_keys.insert(mount.seed_key.as_str()) {
                return Err(LinkedOwnerJsError::DuplicateSeedKey(mount.seed_key.clone()));
            }
            import_pairs.insert((
                mount.fragment_asset_id.as_str(),
                mount.fragment_export.as_str(),
            ));
        }

        let Some(root_index) = mount_indices.get(plan.root_candidate_id.as_str()).copied() else {
            return Err(LinkedOwnerJsError::MissingRootCandidate(
                plan.root_candidate_id.clone(),
            ));
        };
        if root_index != 0 {
            return Err(LinkedOwnerJsError::RootIsNotFirst(
                plan.root_candidate_id.clone(),
            ));
        }
        let root = &plan.mounts[root_index];
        if root.parent_candidate_id.is_some() {
            return Err(LinkedOwnerJsError::RootHasParent(root.candidate_id.clone()));
        }
        if root.lifecycle_root_candidate_id.is_some() {
            return Err(LinkedOwnerJsError::OwnerRootHasLifecycle(
                root.candidate_id.clone(),
            ));
        }
        if root.seed_key != plan.owner_asset_id {
            return Err(LinkedOwnerJsError::RootSeedDoesNotMatchOwner {
                seed_key: root.seed_key.clone(),
                owner_asset_id: plan.owner_asset_id.clone(),
            });
        }

        for mount in &plan.mounts {
            if mount.candidate_id != plan.root_candidate_id && mount.parent_candidate_id.is_none() {
                return Err(LinkedOwnerJsError::UnexpectedRootCandidate(
                    mount.candidate_id.clone(),
                ));
            }
            if let Some(parent) = &mount.parent_candidate_id {
                if !mount_indices.contains_key(parent.as_str()) {
                    return Err(LinkedOwnerJsError::MissingParent {
                        child: mount.candidate_id.clone(),
                        parent: parent.clone(),
                    });
                }
            }
        }
        validate_acyclic(plan, &mount_indices)?;
        for (index, mount) in plan.mounts.iter().enumerate() {
            if let Some(parent) = &mount.parent_candidate_id {
                let parent_index = mount_indices[parent.as_str()];
                if parent_index >= index {
                    return Err(LinkedOwnerJsError::ParentAppearsAfterChild {
                        child: mount.candidate_id.clone(),
                        parent: parent.clone(),
                    });
                }
            }
        }
        let lifecycles = validate_lifecycles(plan, &mount_indices)?;

        let mut signal_targets = BTreeSet::new();
        let mut signal_aliases: Vec<_> = plan.signal_aliases.iter().collect();
        signal_aliases.sort();
        for alias in &signal_aliases {
            validate_edge_candidates(
                &mount_indices,
                &alias.parent_candidate_id,
                &alias.child_candidate_id,
                true,
            )?;
            validate_direct_parent(
                plan,
                &mount_indices,
                &alias.parent_candidate_id,
                &alias.child_candidate_id,
                true,
            )?;
            if !signal_targets.insert((alias.child_candidate_id.as_str(), alias.child_signal_id)) {
                return Err(LinkedOwnerJsError::DuplicateSignalTarget {
                    child: alias.child_candidate_id.clone(),
                    signal_id: alias.child_signal_id,
                });
            }
        }

        let mut signal_seeds: Vec<_> = plan.signal_seeds.iter().collect();
        signal_seeds.sort();
        for seed in &signal_seeds {
            for candidate in [
                seed.parent_candidate_id.as_str(),
                seed.child_candidate_id.as_str(),
            ] {
                if !mount_indices.contains_key(candidate) {
                    return Err(LinkedOwnerJsError::UnknownSignalSeedEndpoint(
                        candidate.to_owned(),
                    ));
                }
            }
            if plan.mounts[mount_indices[seed.child_candidate_id.as_str()]]
                .parent_candidate_id
                .as_deref()
                != Some(seed.parent_candidate_id.as_str())
            {
                return Err(LinkedOwnerJsError::SignalSeedParentMismatch {
                    parent: seed.parent_candidate_id.clone(),
                    child: seed.child_candidate_id.clone(),
                });
            }
            if !signal_targets.insert((seed.child_candidate_id.as_str(), seed.child_signal_id)) {
                return Err(LinkedOwnerJsError::DuplicateSignalTarget {
                    child: seed.child_candidate_id.clone(),
                    signal_id: seed.child_signal_id,
                });
            }
        }

        let mut callback_targets = BTreeSet::new();
        let mut callback_edges: Vec<_> = plan.callback_edges.iter().collect();
        callback_edges.sort();
        for edge in &callback_edges {
            validate_edge_candidates(
                &mount_indices,
                &edge.parent_candidate_id,
                &edge.child_candidate_id,
                false,
            )?;
            validate_callback_source_ancestor(
                plan,
                &mount_indices,
                &edge.parent_candidate_id,
                &edge.child_candidate_id,
            )?;
            if !callback_targets.insert((
                edge.child_candidate_id.as_str(),
                edge.child_callback_slot_id,
            )) {
                return Err(LinkedOwnerJsError::DuplicateCallbackTarget {
                    child: edge.child_candidate_id.clone(),
                    callback_slot_id: edge.child_callback_slot_id,
                });
            }
        }

        let mut node_ref_targets = BTreeSet::new();
        let mut node_ref_aliases: Vec<_> = node_ref_aliases.iter().collect();
        node_ref_aliases.sort();
        for alias in &node_ref_aliases {
            for candidate in [
                alias.parent_candidate_id.as_str(),
                alias.child_candidate_id.as_str(),
            ] {
                if !mount_indices.contains_key(candidate) {
                    return Err(LinkedOwnerJsError::UnknownNodeRefEndpoint(
                        candidate.to_owned(),
                    ));
                }
            }
            if plan.mounts[mount_indices[alias.child_candidate_id.as_str()]]
                .parent_candidate_id
                .as_deref()
                != Some(alias.parent_candidate_id.as_str())
            {
                return Err(LinkedOwnerJsError::NodeRefParentMismatch {
                    parent: alias.parent_candidate_id.clone(),
                    child: alias.child_candidate_id.clone(),
                });
            }
            if plan.mounts[mount_indices[alias.child_candidate_id.as_str()]]
                .lifecycle_root_candidate_id
                .is_some()
            {
                return Err(LinkedOwnerJsError::NodeRefTargetIsConditional(
                    alias.child_candidate_id.clone(),
                ));
            }
            if !node_ref_targets
                .insert((alias.child_candidate_id.as_str(), alias.child_node_ref_id))
            {
                return Err(LinkedOwnerJsError::DuplicateNodeRefTarget {
                    child: alias.child_candidate_id.clone(),
                    node_ref_id: alias.child_node_ref_id,
                });
            }
        }

        let import_indices = import_pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| (pair, index))
            .collect();

        Ok(Self {
            plan,
            mount_indices,
            import_indices,
            signal_aliases,
            signal_seeds,
            node_ref_aliases,
            callback_edges,
            lifecycles,
        })
    }

    fn emit(&self) -> String {
        let mut out = String::new();
        self.emit_imports(&mut out);
        out.push('\n');
        out.push_str("const __rsjs_init_linked_owner = (ownerRoot, __rsjs_disposers = []) => {\n");
        out.push_str("    try {\n");

        // Resolve and validate the complete SSR wrapper protocol before
        // installing any fragment resources. Inactive lifecycle groups retain
        // their SSR DOM, so every wrapper is available for this atomic
        // preflight even though its fragment mounts later.
        for (index, mount) in self.plan.mounts.iter().enumerate() {
            self.emit_wrapper_resolution(&mut out, index, mount);
        }
        out.push('\n');
        for index in 0..self.plan.mounts.len() {
            let _ = writeln!(out, "        let __rsjs_expose_{index} = null;");
        }
        for (index, mount) in self.plan.mounts.iter().enumerate() {
            if mount.lifecycle_root_candidate_id.is_some() {
                let _ = writeln!(
                    out,
                    "        let __rsjs_first_lifecycle_mount_{index} = true;"
                );
            }
        }

        // Candidates outside reactive lifecycle groups mount exactly once.
        for (index, mount) in self.plan.mounts.iter().enumerate() {
            if mount.lifecycle_root_candidate_id.is_none() {
                self.emit_mount(&mut out, index, mount, "__rsjs_disposers", "true", 2);
            }
        }
        for lifecycle in &self.lifecycles {
            self.emit_lifecycle(&mut out, lifecycle);
        }

        out.push_str(
            "        return () => dispose_all(__rsjs_disposers, \"linked owner teardown\");\n",
        );
        out.push_str("    } catch (__rsjs_owner_error) {\n");
        out.push_str("        dispose_all(__rsjs_disposers, \"linked owner init rollback\");\n");
        out.push_str("        report_hydration_error(");
        out.push_str(&js_string(&format!(
            "linked owner \"{}\" initialization",
            self.plan.owner_asset_id
        )));
        out.push_str(", __rsjs_owner_error);\n");
        out.push_str("        throw __rsjs_owner_error;\n");
        out.push_str("    }\n");
        out.push_str("};\n\n");
        self.emit_activation(&mut out);
        out
    }

    fn emit_imports(&self, out: &mut String) {
        out.push_str("import { ");
        if !self.lifecycles.is_empty() {
            out.push_str("bind_component_lifecycle, ");
        }
        out.push_str("dispose_all, ");
        match &self.plan.strategy {
            OwnerActivationStrategy::Eager => out.push_str("hydrate"),
            OwnerActivationStrategy::Visible => {
                out.push_str("hydrate_from_module, hydrate_when_visible")
            }
            OwnerActivationStrategy::Idle => out.push_str("hydrate_from_module, hydrate_when_idle"),
            OwnerActivationStrategy::Interaction { .. } => {
                out.push_str("hydrate_from_module, hydrate_on_interaction")
            }
            OwnerActivationStrategy::Media { .. } => {
                out.push_str("hydrate_from_module, hydrate_when_media")
            }
        }
        out.push_str(", report_hydration_error } from ");
        out.push_str(&js_string(RUNTIME_MODULE));
        out.push_str(";\n");
        for ((asset_id, export), index) in &self.import_indices {
            let _ = writeln!(
                out,
                "import {{ {export} as __rsjs_fragment_{index} }} from {};",
                js_string(&format!("./component_{asset_id}.js"))
            );
        }
    }

    fn emit_wrapper_resolution(&self, out: &mut String, index: usize, mount: &LinkedOwnerMount) {
        let wrapper = format!("__rsjs_wrapper_{index}");
        let element = format!("__rsjs_element_{index}");

        out.push('\n');
        if index == 0 {
            let _ = writeln!(out, "        const {wrapper} = ownerRoot;");
            let _ = writeln!(
                out,
                "        if ({wrapper}?.getAttribute({}) !== {}) {{",
                js_string(LINKED_CANDIDATE_ATTR),
                js_string(&mount.candidate_id)
            );
            emit_throw(
                out,
                3,
                &format!(
                    "linked owner root is missing candidate marker `{}`",
                    mount.candidate_id
                ),
            );
            out.push_str("        }\n");
        } else {
            let matches = format!("__rsjs_matches_{index}");
            let selector = format!("[{}=\"{}\"]", LINKED_CANDIDATE_ATTR, mount.candidate_id);
            let _ = writeln!(
                out,
                "        const {matches} = ownerRoot.querySelectorAll({});",
                js_string(&selector)
            );
            let _ = writeln!(out, "        if ({matches}.length !== 1) {{");
            emit_throw(
                out,
                3,
                &format!(
                    "linked owner expected exactly one wrapper for candidate `{}`",
                    mount.candidate_id
                ),
            );
            out.push_str("        }\n");
            let _ = writeln!(out, "        const {wrapper} = {matches}[0];");
            let parent_index = self.mount_indices[mount
                .parent_candidate_id
                .as_deref()
                .expect("validated non-root parent")];
            let _ = writeln!(
                out,
                "        if (!__rsjs_wrapper_{parent_index}.contains({wrapper})) {{"
            );
            emit_throw(
                out,
                3,
                &format!(
                    "linked owner wrapper `{}` is outside parent `{}`",
                    mount.candidate_id,
                    mount
                        .parent_candidate_id
                        .as_deref()
                        .expect("validated non-root parent")
                ),
            );
            out.push_str("        }\n");
        }

        let _ = writeln!(
            out,
            "        if ({wrapper}.getAttribute({}) !== {}) {{",
            js_string(ISLAND_ROOT_ATTR),
            js_string(&mount.seed_key)
        );
        emit_throw(
            out,
            3,
            &format!(
                "linked owner candidate `{}` has the wrong signal seed key",
                mount.candidate_id
            ),
        );
        out.push_str("        }\n");
        let _ = writeln!(out, "        if ({wrapper}.children.length !== 1) {{");
        emit_throw(
            out,
            3,
            &format!(
                "linked owner candidate `{}` must render exactly one element",
                mount.candidate_id
            ),
        );
        out.push_str("        }\n");
        let _ = writeln!(
            out,
            "        const {element} = {wrapper}.firstElementChild;"
        );
    }

    fn emit_mount(
        &self,
        out: &mut String,
        index: usize,
        mount: &LinkedOwnerMount,
        disposers: &str,
        preserve_dirty_controls: &str,
        indent: usize,
    ) {
        let pad = "    ".repeat(indent);
        let inner_pad = "    ".repeat(indent + 1);
        let wrapper = format!("__rsjs_wrapper_{index}");
        let element = format!("__rsjs_element_{index}");
        let expose = format!("__rsjs_expose_{index}");
        let next_expose = format!("__rsjs_next_expose_{index}");
        let clear_expose = format!("__rsjs_clear_expose_{index}");
        let expose_cleared = format!("__rsjs_expose_cleared_{index}");
        let signals = format!("__rsjs_signals_{index}");
        let signal_seeds = format!("__rsjs_signal_seeds_{index}");
        let node_refs = format!("__rsjs_node_refs_{index}");
        let callbacks = format!("__rsjs_callbacks_{index}");
        let exports_node_refs = self
            .node_ref_aliases
            .iter()
            .any(|alias| alias.parent_candidate_id == mount.candidate_id);
        let imports_node_refs = self
            .node_ref_aliases
            .iter()
            .any(|alias| alias.child_candidate_id == mount.candidate_id);

        out.push('\n');
        if exports_node_refs {
            let _ = writeln!(
                out,
                "{pad}const {next_expose} = {{ signals: Object.create(null), nodeRefs: Object.create(null), payloadHandlers: Object.create(null), lifecycleControls: Object.create(null) }};"
            );
        } else {
            let _ = writeln!(
                out,
                "{pad}const {next_expose} = {{ signals: Object.create(null), payloadHandlers: Object.create(null), lifecycleControls: Object.create(null) }};"
            );
        }
        let _ = writeln!(out, "{pad}{expose} = {next_expose};");
        let _ = writeln!(out, "{pad}let {expose_cleared} = false;");
        let _ = writeln!(out, "{pad}const {clear_expose} = () => {{");
        let _ = writeln!(out, "{inner_pad}if ({expose_cleared}) return;");
        let _ = writeln!(out, "{inner_pad}{expose_cleared} = true;");
        let _ = writeln!(
            out,
            "{inner_pad}if ({expose} === {next_expose}) {expose} = null;"
        );
        let _ = writeln!(out, "{pad}}};");
        let _ = writeln!(out, "{pad}{disposers}.push({clear_expose});");
        let _ = writeln!(out, "{pad}const {signals} = Object.create(null);");
        let _ = writeln!(out, "{pad}const {signal_seeds} = Object.create(null);");
        if imports_node_refs {
            let _ = writeln!(out, "{pad}const {node_refs} = Object.create(null);");
        }

        for alias in self
            .signal_aliases
            .iter()
            .filter(|alias| alias.child_candidate_id == mount.candidate_id)
        {
            let parent_index = self.mount_indices[alias.parent_candidate_id.as_str()];
            let value = format!("__rsjs_signal_{}_{}", index, alias.child_signal_id);
            let _ = writeln!(
                out,
                "{pad}const {value} = __rsjs_expose_{parent_index}?.signals?.[{}];",
                alias.parent_signal_id
            );
            let _ = writeln!(
                out,
                "{pad}if ({value} == null || typeof {value}.get !== \"function\") {{"
            );
            emit_throw(
                out,
                indent + 1,
                &format!(
                    "linked owner parent `{}` did not expose signal {}",
                    alias.parent_candidate_id, alias.parent_signal_id
                ),
            );
            let _ = writeln!(out, "{pad}}}");
            let _ = writeln!(out, "{pad}{signals}[{}] = {value};", alias.child_signal_id);
        }

        for seed in self
            .signal_seeds
            .iter()
            .filter(|seed| seed.child_candidate_id == mount.candidate_id)
        {
            let parent_index = self.mount_indices[seed.parent_candidate_id.as_str()];
            let source = format!("__rsjs_seed_source_{}_{}", index, seed.child_signal_id);
            let _ = writeln!(
                out,
                "{pad}const {source} = __rsjs_expose_{parent_index}?.signals?.[{}];",
                seed.parent_signal_id
            );
            let _ = writeln!(
                out,
                "{pad}if ({source} == null || typeof {source}.get !== \"function\") {{"
            );
            emit_throw(
                out,
                indent + 1,
                &format!(
                    "linked owner parent `{}` did not expose seed signal {}",
                    seed.parent_candidate_id, seed.parent_signal_id
                ),
            );
            let _ = writeln!(out, "{pad}}}");
            let _ = writeln!(
                out,
                "{pad}{signal_seeds}[{}] = {source}.get();",
                seed.child_signal_id
            );
        }

        for alias in self
            .node_ref_aliases
            .iter()
            .filter(|alias| alias.child_candidate_id == mount.candidate_id)
        {
            let parent_index = self.mount_indices[alias.parent_candidate_id.as_str()];
            let value = format!("__rsjs_node_ref_{}_{}", index, alias.child_node_ref_id);
            let _ = writeln!(
                out,
                "{pad}const {value} = __rsjs_expose_{parent_index}?.nodeRefs?.[{}];",
                alias.parent_node_ref_id
            );
            let _ = writeln!(
                out,
                "{pad}if ({value} == null || typeof {value} !== \"object\" || !(\"current\" in {value})) {{"
            );
            emit_throw(
                out,
                indent + 1,
                &format!(
                    "linked owner parent `{}` did not expose node ref {}",
                    alias.parent_candidate_id, alias.parent_node_ref_id
                ),
            );
            let _ = writeln!(out, "{pad}}}");
            let _ = writeln!(
                out,
                "{pad}{node_refs}[{}] = {value};",
                alias.child_node_ref_id
            );
        }

        if index != 0 {
            let _ = writeln!(out, "{pad}const {callbacks} = Object.create(null);");
            for edge in self
                .callback_edges
                .iter()
                .filter(|edge| edge.child_candidate_id == mount.candidate_id)
            {
                let parent_index = self.mount_indices[edge.parent_candidate_id.as_str()];
                let handler = format!(
                    "__rsjs_payload_handler_{}_{}",
                    index, edge.child_callback_slot_id
                );
                let _ = writeln!(
                    out,
                    "{pad}const {handler} = __rsjs_expose_{parent_index}?.payloadHandlers?.[{}];",
                    edge.parent_payload_handler_id
                );
                let _ = writeln!(out, "{pad}if (typeof {handler} !== \"function\") {{");
                emit_throw(
                    out,
                    indent + 1,
                    &format!(
                        "linked owner parent `{}` did not expose payload handler {}",
                        edge.parent_candidate_id, edge.parent_payload_handler_id
                    ),
                );
                let _ = writeln!(out, "{pad}}}");
                let _ = writeln!(
                    out,
                    "{pad}{callbacks}[{}] = (__payload) => {handler}(__payload);",
                    js_string(&format!("__callback_{}", edge.child_callback_slot_id))
                );
            }
            self.emit_callback_install(out, index, &element, &callbacks, disposers, indent);
        }

        let import_index = self.import_indices[&(
            mount.fragment_asset_id.as_str(),
            mount.fragment_export.as_str(),
        )];
        let disposer = format!("__rsjs_fragment_disposer_{index}");
        let _ = writeln!(
            out,
            "{pad}const {disposer} = __rsjs_fragment_{import_index}({element}, {{"
        );
        let _ = writeln!(out, "{inner_pad}seedRoot: {wrapper},");
        let _ = writeln!(out, "{inner_pad}islandRoot: ownerRoot,");
        let _ = writeln!(out, "{inner_pad}signals: {signals},");
        let _ = writeln!(out, "{inner_pad}signalSeeds: {signal_seeds},");
        if imports_node_refs {
            let _ = writeln!(out, "{inner_pad}nodeRefs: {node_refs},");
        }
        let _ = writeln!(
            out,
            "{inner_pad}preserveDirtyControls: {preserve_dirty_controls},"
        );
        let _ = writeln!(out, "{inner_pad}expose: {next_expose},");
        let _ = writeln!(out, "{pad}}});");
        let _ = writeln!(
            out,
            "{pad}if (typeof {disposer} === \"function\") {disposers}.push({disposer});"
        );
    }

    fn emit_callback_install(
        &self,
        out: &mut String,
        index: usize,
        element: &str,
        callbacks: &str,
        disposers: &str,
        indent: usize,
    ) {
        let pad = "    ".repeat(indent);
        let inner_pad = "    ".repeat(indent + 1);
        let had = format!("__rsjs_had_callbacks_{index}");
        let previous = format!("__rsjs_previous_callbacks_{index}");
        let restored = format!("__rsjs_callbacks_restored_{index}");
        let restore = format!("__rsjs_restore_callbacks_{index}");
        let _ = writeln!(
            out,
            "{pad}const {had} = Object.prototype.hasOwnProperty.call({element}, \"__rsjs_callbacks\");"
        );
        let _ = writeln!(out, "{pad}const {previous} = {element}.__rsjs_callbacks;");
        let _ = writeln!(out, "{pad}let {restored} = false;");
        let _ = writeln!(out, "{pad}const {restore} = () => {{");
        let _ = writeln!(out, "{inner_pad}if ({restored}) return;");
        let _ = writeln!(out, "{inner_pad}{restored} = true;");
        let _ = writeln!(
            out,
            "{inner_pad}if ({had}) {element}.__rsjs_callbacks = {previous};"
        );
        let _ = writeln!(out, "{inner_pad}else delete {element}.__rsjs_callbacks;");
        let _ = writeln!(out, "{pad}}};");
        let _ = writeln!(out, "{pad}{disposers}.push({restore});");
        let _ = writeln!(out, "{pad}{element}.__rsjs_callbacks = {callbacks};");
    }

    fn emit_lifecycle(&self, out: &mut String, lifecycle: &LinkedOwnerLifecycle) {
        let parent_index = self.mount_indices[lifecycle.parent_candidate_id.as_str()];
        let root_index = self.mount_indices[lifecycle.root_candidate_id.as_str()];
        let control = format!("__rsjs_lifecycle_control_{root_index}");
        let group_disposers = format!("__rsjs_lifecycle_disposers_{root_index}");
        let branch = match lifecycle.branch {
            LinkedOwnerLifecycleBranch::Then => "then",
            LinkedOwnerLifecycleBranch::Else => "else",
        };
        let scope = format!(
            "linked owner \"{}\" lifecycle \"{}\"",
            self.plan.owner_asset_id, lifecycle.root_candidate_id
        );

        out.push('\n');
        let _ = writeln!(
            out,
            "        const {control} = __rsjs_expose_{parent_index}?.lifecycleControls?.[{}];",
            lifecycle.control_id
        );
        out.push_str("        __rsjs_disposers.push(bind_component_lifecycle(\n");
        let _ = writeln!(out, "            {control},");
        let _ = writeln!(out, "            {},", js_string(branch));
        out.push_str("            () => {\n");
        let _ = writeln!(out, "                const {group_disposers} = [];");
        out.push_str("                try {\n");
        for candidate in &lifecycle.candidates {
            let index = self.mount_indices[candidate.as_str()];
            let preserve_dirty_controls = format!("__rsjs_preserve_dirty_controls_{index}");
            let _ = writeln!(
                out,
                "                    const {preserve_dirty_controls} = __rsjs_first_lifecycle_mount_{index};"
            );
            let _ = writeln!(
                out,
                "                    __rsjs_first_lifecycle_mount_{index} = false;"
            );
            self.emit_mount(
                out,
                index,
                &self.plan.mounts[index],
                &group_disposers,
                &preserve_dirty_controls,
                5,
            );
        }
        let _ = writeln!(
            out,
            "                    return () => dispose_all({group_disposers}, {});",
            js_string(&format!("{scope} teardown"))
        );
        out.push_str("                } catch (__rsjs_lifecycle_mount_error) {\n");
        let _ = writeln!(
            out,
            "                    dispose_all({group_disposers}, {});",
            js_string(&format!("{scope} mount rollback"))
        );
        out.push_str("                    throw __rsjs_lifecycle_mount_error;\n");
        out.push_str("                }\n");
        out.push_str("            },\n");
        let _ = writeln!(out, "            {},", js_string(&scope));
        out.push_str("        ));\n");
    }

    fn emit_activation(&self, out: &mut String) {
        let owner = js_string(&self.plan.owner_asset_id);
        match &self.plan.strategy {
            OwnerActivationStrategy::Eager => {
                let _ = writeln!(out, "hydrate({owner}, __rsjs_init_linked_owner);");
            }
            OwnerActivationStrategy::Visible => {
                let _ = writeln!(
                    out,
                    "hydrate_from_module(import.meta.url, hydrate_when_visible)({owner}, __rsjs_init_linked_owner);"
                );
            }
            OwnerActivationStrategy::Idle => {
                let _ = writeln!(
                    out,
                    "hydrate_from_module(import.meta.url, hydrate_when_idle)({owner}, __rsjs_init_linked_owner);"
                );
            }
            OwnerActivationStrategy::Interaction { event } => {
                out.push_str("hydrate_from_module(import.meta.url, hydrate_on_interaction)(");
                out.push_str(&owner);
                out.push_str(", __rsjs_init_linked_owner");
                if let Some(event) = event {
                    out.push_str(", [");
                    out.push_str(&js_string(event));
                    out.push(']');
                }
                out.push_str(");\n");
            }
            OwnerActivationStrategy::Media { query } => {
                let _ = writeln!(
                    out,
                    "hydrate_from_module(import.meta.url, hydrate_when_media)({owner}, __rsjs_init_linked_owner, {});",
                    js_string(query)
                );
            }
        }
    }
}

fn validate_edge_candidates(
    mounts: &BTreeMap<&str, usize>,
    parent: &str,
    child: &str,
    signal: bool,
) -> Result<(), LinkedOwnerJsError> {
    for candidate in [parent, child] {
        if !mounts.contains_key(candidate) {
            return Err(if signal {
                LinkedOwnerJsError::UnknownSignalEndpoint(candidate.to_owned())
            } else {
                LinkedOwnerJsError::UnknownCallbackEndpoint(candidate.to_owned())
            });
        }
    }
    Ok(())
}

fn validate_direct_parent(
    plan: &LinkedOwnerModulePlan,
    mounts: &BTreeMap<&str, usize>,
    parent: &str,
    child: &str,
    signal: bool,
) -> Result<(), LinkedOwnerJsError> {
    if plan.mounts[mounts[child]].parent_candidate_id.as_deref() != Some(parent) {
        return Err(if signal {
            LinkedOwnerJsError::SignalParentMismatch {
                parent: parent.to_owned(),
                child: child.to_owned(),
            }
        } else {
            LinkedOwnerJsError::CallbackParentMismatch {
                parent: parent.to_owned(),
                child: child.to_owned(),
            }
        });
    }
    Ok(())
}

fn validate_callback_source_ancestor(
    plan: &LinkedOwnerModulePlan,
    mounts: &BTreeMap<&str, usize>,
    source: &str,
    child: &str,
) -> Result<(), LinkedOwnerJsError> {
    let mut current = plan.mounts[mounts[child]].parent_candidate_id.as_deref();
    while let Some(candidate) = current {
        if candidate == source {
            return Ok(());
        }
        current = plan.mounts[mounts[candidate]]
            .parent_candidate_id
            .as_deref();
    }
    Err(LinkedOwnerJsError::CallbackParentMismatch {
        parent: source.to_owned(),
        child: child.to_owned(),
    })
}

fn validate_acyclic(
    plan: &LinkedOwnerModulePlan,
    mounts: &BTreeMap<&str, usize>,
) -> Result<(), LinkedOwnerJsError> {
    for start in &plan.mounts {
        let mut path = BTreeSet::new();
        let mut current = Some(start.candidate_id.as_str());
        while let Some(candidate) = current {
            if !path.insert(candidate) {
                return Err(LinkedOwnerJsError::ParentCycle(candidate.to_owned()));
            }
            current = plan.mounts[mounts[candidate]]
                .parent_candidate_id
                .as_deref();
        }
    }
    Ok(())
}

fn validate_lifecycles<'a>(
    plan: &'a LinkedOwnerModulePlan,
    mounts: &BTreeMap<&str, usize>,
) -> Result<Vec<&'a LinkedOwnerLifecycle>, LinkedOwnerJsError> {
    let mut by_root = BTreeMap::new();
    for lifecycle in &plan.lifecycles {
        if lifecycle.root_candidate_id == plan.root_candidate_id {
            return Err(LinkedOwnerJsError::OwnerRootHasLifecycle(
                lifecycle.root_candidate_id.clone(),
            ));
        }
        for candidate in std::iter::once(lifecycle.root_candidate_id.as_str())
            .chain(std::iter::once(lifecycle.parent_candidate_id.as_str()))
            .chain(lifecycle.candidates.iter().map(String::as_str))
        {
            if !validate_marker_value(candidate) {
                return Err(LinkedOwnerJsError::InvalidCandidateId(candidate.to_owned()));
            }
        }
        if !mounts.contains_key(lifecycle.root_candidate_id.as_str()) {
            return Err(LinkedOwnerJsError::UnknownLifecycleCandidate(
                lifecycle.root_candidate_id.clone(),
            ));
        }
        if !mounts.contains_key(lifecycle.parent_candidate_id.as_str()) {
            return Err(LinkedOwnerJsError::MissingParent {
                child: lifecycle.root_candidate_id.clone(),
                parent: lifecycle.parent_candidate_id.clone(),
            });
        }
        for candidate in &lifecycle.candidates {
            if !mounts.contains_key(candidate.as_str()) {
                return Err(LinkedOwnerJsError::UnknownLifecycleCandidate(
                    candidate.clone(),
                ));
            }
        }
        if by_root
            .insert(lifecycle.root_candidate_id.as_str(), lifecycle)
            .is_some()
        {
            return Err(LinkedOwnerJsError::DuplicateLifecycleRoot(
                lifecycle.root_candidate_id.clone(),
            ));
        }
    }

    for mount in &plan.mounts {
        if let Some(root) = mount.lifecycle_root_candidate_id.as_deref() {
            if !by_root.contains_key(root) {
                return Err(LinkedOwnerJsError::MissingLifecycle(root.to_owned()));
            }
        }
    }

    for lifecycle in by_root.values() {
        if lifecycle.candidates.is_empty() {
            return Err(LinkedOwnerJsError::EmptyLifecycleCandidates(
                lifecycle.root_candidate_id.clone(),
            ));
        }
        if lifecycle.candidates.first().map(String::as_str)
            != Some(lifecycle.root_candidate_id.as_str())
        {
            return Err(LinkedOwnerJsError::LifecycleRootIsNotFirst(
                lifecycle.root_candidate_id.clone(),
            ));
        }

        let expected = plan
            .mounts
            .iter()
            .filter(|mount| {
                mount.lifecycle_root_candidate_id.as_deref()
                    == Some(lifecycle.root_candidate_id.as_str())
            })
            .map(|mount| mount.candidate_id.as_str())
            .collect::<Vec<_>>();
        let actual = lifecycle
            .candidates
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(LinkedOwnerJsError::LifecycleCandidatesMismatch(
                lifecycle.root_candidate_id.clone(),
            ));
        }

        let root_mount = &plan.mounts[mounts[lifecycle.root_candidate_id.as_str()]];
        if root_mount.parent_candidate_id.as_deref() != Some(lifecycle.parent_candidate_id.as_str())
        {
            return Err(LinkedOwnerJsError::LifecycleParentMismatch {
                root: lifecycle.root_candidate_id.clone(),
                parent: lifecycle.parent_candidate_id.clone(),
            });
        }
        let parent_mount = &plan.mounts[mounts[lifecycle.parent_candidate_id.as_str()]];
        if parent_mount.lifecycle_root_candidate_id.is_some() {
            return Err(LinkedOwnerJsError::LifecycleParentIsConditional {
                root: lifecycle.root_candidate_id.clone(),
                parent: lifecycle.parent_candidate_id.clone(),
            });
        }

        let members = lifecycle
            .candidates
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for candidate in lifecycle.candidates.iter().skip(1) {
            let mount = &plan.mounts[mounts[candidate.as_str()]];
            let Some(parent) = mount.parent_candidate_id.as_deref() else {
                return Err(LinkedOwnerJsError::LifecycleCandidateParentMismatch {
                    root: lifecycle.root_candidate_id.clone(),
                    candidate: candidate.clone(),
                });
            };
            if !members.contains(parent) {
                return Err(LinkedOwnerJsError::LifecycleCandidateParentMismatch {
                    root: lifecycle.root_candidate_id.clone(),
                    candidate: candidate.clone(),
                });
            }
        }
    }

    // A lifecycle group owns the complete statically mounted subtree below its
    // root. Otherwise an untagged child would mount eagerly while its parent
    // remains inactive, and group teardown could not dispose that child.
    for mount in &plan.mounts {
        let Some(parent_id) = mount.parent_candidate_id.as_deref() else {
            continue;
        };
        let parent = &plan.mounts[mounts[parent_id]];
        let Some(root) = parent.lifecycle_root_candidate_id.as_deref() else {
            continue;
        };
        if mount.lifecycle_root_candidate_id.as_deref() != Some(root) {
            return Err(LinkedOwnerJsError::LifecycleSubtreeNotClosed {
                root: root.to_owned(),
                parent: parent.candidate_id.clone(),
                child: mount.candidate_id.clone(),
            });
        }
    }

    let mut lifecycles = by_root.into_values().collect::<Vec<_>>();
    lifecycles.sort_by_key(|lifecycle| mounts[lifecycle.root_candidate_id.as_str()]);
    Ok(lifecycles)
}

fn validate_strategy(strategy: &OwnerActivationStrategy) -> Result<(), LinkedOwnerJsError> {
    match strategy {
        OwnerActivationStrategy::Interaction { event: Some(event) }
            if !validate_event_name(event) =>
        {
            Err(LinkedOwnerJsError::InvalidInteractionEvent(event.clone()))
        }
        OwnerActivationStrategy::Media { query } if !validate_media_query(query) => {
            Err(LinkedOwnerJsError::InvalidMediaQuery(query.clone()))
        }
        _ => Ok(()),
    }
}

fn validate_asset_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn validate_marker_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn validate_js_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || matches!(first, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn validate_event_name(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':'))
}

fn validate_media_query(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn emit_throw(out: &mut String, indent: usize, message: &str) {
    for _ in 0..indent {
        out.push_str("    ");
    }
    out.push_str("throw new Error(");
    out.push_str(&js_string(message));
    out.push_str(");\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(candidate: &str, parent: Option<&str>, asset: &str, seed: &str) -> LinkedOwnerMount {
        LinkedOwnerMount {
            candidate_id: candidate.to_owned(),
            parent_candidate_id: parent.map(str::to_owned),
            lifecycle_root_candidate_id: None,
            fragment_asset_id: asset.to_owned(),
            fragment_export: format!("mount_{asset}"),
            seed_key: seed.to_owned(),
        }
    }

    fn plan() -> LinkedOwnerModulePlan {
        LinkedOwnerModulePlan {
            owner_asset_id: "owner_7".to_owned(),
            root_candidate_id: "page".to_owned(),
            strategy: OwnerActivationStrategy::Visible,
            uses_queries: false,
            uses_mutations: false,
            mounts: vec![
                mount("page", None, "page_fragment", "owner_7"),
                mount("badge", Some("page"), "badge_fragment", "seed_badge"),
                mount("detail", Some("badge"), "detail_fragment", "seed_detail"),
            ],
            signal_aliases: vec![
                LinkedOwnerSignalAlias {
                    parent_candidate_id: "badge".to_owned(),
                    parent_signal_id: 8,
                    child_candidate_id: "detail".to_owned(),
                    child_signal_id: 2,
                },
                LinkedOwnerSignalAlias {
                    parent_candidate_id: "page".to_owned(),
                    parent_signal_id: 3,
                    child_candidate_id: "badge".to_owned(),
                    child_signal_id: 1,
                },
            ],
            signal_seeds: Vec::new(),
            callback_edges: vec![LinkedOwnerCallbackEdge {
                parent_candidate_id: "page".to_owned(),
                parent_payload_handler_id: 5,
                child_candidate_id: "badge".to_owned(),
                child_callback_slot_id: 4,
            }],
            lifecycles: Vec::new(),
        }
    }

    fn lifecycle_plan() -> LinkedOwnerModulePlan {
        let mut input = plan();
        input.mounts[1].lifecycle_root_candidate_id = Some("badge".to_owned());
        input.mounts[2].lifecycle_root_candidate_id = Some("badge".to_owned());
        input.signal_seeds.push(LinkedOwnerSignalSeed {
            parent_candidate_id: "page".to_owned(),
            parent_signal_id: 7,
            child_candidate_id: "badge".to_owned(),
            child_signal_id: 6,
        });
        input.lifecycles.push(LinkedOwnerLifecycle {
            root_candidate_id: "badge".to_owned(),
            parent_candidate_id: "page".to_owned(),
            control_id: 11,
            branch: LinkedOwnerLifecycleBranch::Then,
            candidates: vec!["badge".to_owned(), "detail".to_owned()],
        });
        input
    }

    #[test]
    fn emits_linked_owner_mounts_activation_aliases_callbacks_and_rollback() {
        let js = emit_linked_owner_module(&plan()).unwrap();

        assert!(js.contains(
            "hydrate_from_module(import.meta.url, hydrate_when_visible)(\"owner_7\", __rsjs_init_linked_owner);"
        ));
        assert!(js.contains(
            "import { mount_badge_fragment as __rsjs_fragment_0 } from \"./component_badge_fragment.js\";"
        ));
        assert!(js.contains(
            "ownerRoot.querySelectorAll(\"[data-rsjs-linked-candidate=\\\"badge\\\"]\")"
        ));
        assert!(js.contains("if (__rsjs_wrapper_1.children.length !== 1)"));
        assert!(js.contains("seedRoot: __rsjs_wrapper_1"));
        assert!(js.contains("islandRoot: ownerRoot"));
        assert!(js.contains("__rsjs_signals_1[1] = __rsjs_signal_1_1;"));
        assert!(js.contains("__rsjs_expose_0?.payloadHandlers?.[5]"));
        assert!(js.contains(
            "__rsjs_callbacks_1[\"__callback_4\"] = (__payload) => __rsjs_payload_handler_1_4(__payload);"
        ));
        assert!(js.contains("__rsjs_disposers.push(__rsjs_restore_callbacks_1);"));
        assert!(js.contains("dispose_all(__rsjs_disposers, \"linked owner init rollback\")"));
        assert!(
            js.contains("report_hydration_error(\"linked owner \\\"owner_7\\\" initialization\"")
        );
    }

    #[test]
    fn callback_sources_may_bypass_participant_only_intermediate_mounts() {
        let mut input = plan();
        input.callback_edges[0].child_candidate_id = "detail".to_owned();
        input.callback_edges[0].child_callback_slot_id = 12;

        let js = emit_linked_owner_module(&input)
            .expect("a root payload handler may feed a descendant callback slot");

        assert!(js.contains("__rsjs_expose_0?.payloadHandlers?.[5]"));
        assert!(js.contains(
            "__rsjs_callbacks_2[\"__callback_12\"] = (__payload) => __rsjs_payload_handler_2_12(__payload);"
        ));
    }

    #[test]
    fn emits_exact_node_ref_aliases_without_reconstructing_ref_objects() {
        let input = plan();
        let aliases = vec![
            LinkedOwnerNodeRefAlias {
                parent_candidate_id: "badge".to_owned(),
                parent_node_ref_id: 2,
                child_candidate_id: "detail".to_owned(),
                child_node_ref_id: 4,
            },
            LinkedOwnerNodeRefAlias {
                parent_candidate_id: "page".to_owned(),
                parent_node_ref_id: 7,
                child_candidate_id: "badge".to_owned(),
                child_node_ref_id: 2,
            },
        ];
        let js = emit_linked_owner_module_with_node_refs(&input, &aliases).unwrap();

        assert!(js.contains("const __rsjs_node_ref_1_2 = __rsjs_expose_0?.nodeRefs?.[7];"));
        assert!(js.contains("__rsjs_node_refs_1[2] = __rsjs_node_ref_1_2;"));
        assert!(js.contains("const __rsjs_node_ref_2_4 = __rsjs_expose_1?.nodeRefs?.[2];"));
        assert!(js.contains("nodeRefs: __rsjs_node_refs_2,"));
        assert!(
            !js.contains("__rsjs_node_ref_1_2 = { current: null }"),
            "the linked owner must forward the exact parent object, never allocate a fallback"
        );

        let mut reversed = aliases;
        reversed.reverse();
        assert_eq!(
            js,
            emit_linked_owner_module_with_node_refs(&input, &reversed).unwrap(),
            "node-ref edge input order must not affect emitted owner JavaScript"
        );
    }

    #[test]
    fn emits_conditional_lifecycle_mounts_per_mount_seeds_and_retry_state() {
        let js = emit_linked_owner_module(&lifecycle_plan()).unwrap();

        assert!(js.contains("import { bind_component_lifecycle, dispose_all, hydrate_from_module"));
        assert!(js.contains(
            "const __rsjs_lifecycle_control_1 = __rsjs_expose_0?.lifecycleControls?.[11];"
        ));
        assert!(js.contains(
            "bind_component_lifecycle(\n            __rsjs_lifecycle_control_1,\n            \"then\","
        ));
        assert!(js.contains("let __rsjs_first_lifecycle_mount_1 = true;"));
        assert!(
            js.contains("const __rsjs_preserve_dirty_controls_1 = __rsjs_first_lifecycle_mount_1;")
        );
        assert!(js.contains("__rsjs_first_lifecycle_mount_1 = false;"));
        assert!(js.contains("__rsjs_signal_seeds_1[6] = __rsjs_seed_source_1_6.get();"));
        assert!(js.contains("preserveDirtyControls: __rsjs_preserve_dirty_controls_1,"));
        assert!(js.contains(
            "return () => dispose_all(__rsjs_lifecycle_disposers_1, \"linked owner \\\"owner_7\\\" lifecycle \\\"badge\\\" teardown\");"
        ));
        assert!(js.contains(
            "dispose_all(__rsjs_lifecycle_disposers_1, \"linked owner \\\"owner_7\\\" lifecycle \\\"badge\\\" mount rollback\");"
        ));

        let root_mount = js
            .find("const __rsjs_fragment_disposer_0")
            .expect("unconditional root mount");
        let lifecycle_binding = js
            .find("bind_component_lifecycle(")
            .expect("lifecycle binding");
        let child_mount = js
            .find("const __rsjs_fragment_disposer_1")
            .expect("conditional child mount");
        let grandchild_mount = js
            .find("const __rsjs_fragment_disposer_2")
            .expect("conditional grandchild mount");
        assert!(root_mount < lifecycle_binding);
        assert!(lifecycle_binding < child_mount);
        assert!(child_mount < grandchild_mount);
    }

    #[test]
    fn edge_input_order_does_not_change_output() {
        let forward = plan();
        let mut reverse = forward.clone();
        reverse.signal_aliases.reverse();
        reverse.callback_edges.reverse();
        assert_eq!(
            emit_linked_owner_module(&forward).unwrap(),
            emit_linked_owner_module(&reverse).unwrap()
        );

        let forward = lifecycle_plan();
        let mut reverse = forward.clone();
        reverse.signal_aliases.reverse();
        reverse.signal_seeds.reverse();
        reverse.callback_edges.reverse();
        reverse.lifecycles.reverse();
        assert_eq!(
            emit_linked_owner_module(&forward).unwrap(),
            emit_linked_owner_module(&reverse).unwrap()
        );
    }

    #[test]
    fn emits_every_activation_strategy() {
        let cases = [
            (
                OwnerActivationStrategy::Eager,
                "hydrate(\"owner_7\", __rsjs_init_linked_owner);",
            ),
            (
                OwnerActivationStrategy::Idle,
                "hydrate_from_module(import.meta.url, hydrate_when_idle)",
            ),
            (
                OwnerActivationStrategy::Interaction {
                    event: Some("click".to_owned()),
                },
                "__rsjs_init_linked_owner, [\"click\"]",
            ),
            (
                OwnerActivationStrategy::Media {
                    query: "(min-width: 48rem)".to_owned(),
                },
                "__rsjs_init_linked_owner, \"(min-width: 48rem)\"",
            ),
        ];
        for (strategy, expected) in cases {
            let mut input = plan();
            input.strategy = strategy;
            assert!(emit_linked_owner_module(&input).unwrap().contains(expected));
        }
    }

    #[test]
    fn rejects_duplicate_candidates_and_seed_keys() {
        let mut input = plan();
        input.mounts[2].candidate_id = "badge".to_owned();
        assert_eq!(
            emit_linked_owner_module(&input),
            Err(LinkedOwnerJsError::DuplicateCandidate("badge".to_owned()))
        );

        let mut input = plan();
        input.mounts[2].seed_key = "seed_badge".to_owned();
        assert_eq!(
            emit_linked_owner_module(&input),
            Err(LinkedOwnerJsError::DuplicateSeedKey(
                "seed_badge".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_missing_or_misordered_parents_and_cycles() {
        let mut missing = plan();
        missing.mounts[1].parent_candidate_id = Some("absent".to_owned());
        assert!(matches!(
            emit_linked_owner_module(&missing),
            Err(LinkedOwnerJsError::MissingParent { .. })
        ));

        let mut order = plan();
        order.mounts.swap(1, 2);
        assert!(matches!(
            emit_linked_owner_module(&order),
            Err(LinkedOwnerJsError::ParentAppearsAfterChild { .. })
        ));

        let mut cycle = plan();
        cycle.mounts[1].parent_candidate_id = Some("detail".to_owned());
        assert!(matches!(
            emit_linked_owner_module(&cycle),
            Err(LinkedOwnerJsError::ParentCycle(_))
        ));
    }

    #[test]
    fn rejects_missing_root_and_additional_roots() {
        let mut missing = plan();
        missing.root_candidate_id = "absent".to_owned();
        assert_eq!(
            emit_linked_owner_module(&missing),
            Err(LinkedOwnerJsError::MissingRootCandidate(
                "absent".to_owned()
            ))
        );

        let mut extra = plan();
        extra.mounts[1].parent_candidate_id = None;
        assert_eq!(
            emit_linked_owner_module(&extra),
            Err(LinkedOwnerJsError::UnexpectedRootCandidate(
                "badge".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_unknown_or_non_parent_signal_edges_and_duplicate_targets() {
        let mut unknown = plan();
        unknown.signal_aliases[0].parent_candidate_id = "absent".to_owned();
        assert_eq!(
            emit_linked_owner_module(&unknown),
            Err(LinkedOwnerJsError::UnknownSignalEndpoint(
                "absent".to_owned()
            ))
        );

        let mut non_parent = plan();
        non_parent.signal_aliases[0].parent_candidate_id = "page".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&non_parent),
            Err(LinkedOwnerJsError::SignalParentMismatch { .. })
        ));

        let mut duplicate = plan();
        duplicate
            .signal_aliases
            .push(duplicate.signal_aliases[1].clone());
        assert!(matches!(
            emit_linked_owner_module(&duplicate),
            Err(LinkedOwnerJsError::DuplicateSignalTarget { .. })
        ));

        let mut unknown_seed = plan();
        unknown_seed.signal_seeds.push(LinkedOwnerSignalSeed {
            parent_candidate_id: "absent".to_owned(),
            parent_signal_id: 1,
            child_candidate_id: "badge".to_owned(),
            child_signal_id: 9,
        });
        assert!(matches!(
            emit_linked_owner_module(&unknown_seed),
            Err(LinkedOwnerJsError::UnknownSignalSeedEndpoint(_))
        ));

        let mut non_parent_seed = plan();
        non_parent_seed.signal_seeds.push(LinkedOwnerSignalSeed {
            parent_candidate_id: "page".to_owned(),
            parent_signal_id: 1,
            child_candidate_id: "detail".to_owned(),
            child_signal_id: 9,
        });
        assert!(matches!(
            emit_linked_owner_module(&non_parent_seed),
            Err(LinkedOwnerJsError::SignalSeedParentMismatch { .. })
        ));

        let mut alias_seed_collision = plan();
        alias_seed_collision
            .signal_seeds
            .push(LinkedOwnerSignalSeed {
                parent_candidate_id: "page".to_owned(),
                parent_signal_id: 9,
                child_candidate_id: "badge".to_owned(),
                child_signal_id: 1,
            });
        assert!(matches!(
            emit_linked_owner_module(&alias_seed_collision),
            Err(LinkedOwnerJsError::DuplicateSignalTarget { .. })
        ));
    }

    #[test]
    fn rejects_invalid_node_ref_alias_topology() {
        let input = plan();
        let unknown = [LinkedOwnerNodeRefAlias {
            parent_candidate_id: "absent".to_owned(),
            parent_node_ref_id: 1,
            child_candidate_id: "badge".to_owned(),
            child_node_ref_id: 2,
        }];
        assert_eq!(
            emit_linked_owner_module_with_node_refs(&input, &unknown),
            Err(LinkedOwnerJsError::UnknownNodeRefEndpoint(
                "absent".to_owned()
            ))
        );

        let non_parent = [LinkedOwnerNodeRefAlias {
            parent_candidate_id: "page".to_owned(),
            parent_node_ref_id: 1,
            child_candidate_id: "detail".to_owned(),
            child_node_ref_id: 2,
        }];
        assert!(matches!(
            emit_linked_owner_module_with_node_refs(&input, &non_parent),
            Err(LinkedOwnerJsError::NodeRefParentMismatch { .. })
        ));

        let duplicate = [
            LinkedOwnerNodeRefAlias {
                parent_candidate_id: "page".to_owned(),
                parent_node_ref_id: 1,
                child_candidate_id: "badge".to_owned(),
                child_node_ref_id: 2,
            },
            LinkedOwnerNodeRefAlias {
                parent_candidate_id: "page".to_owned(),
                parent_node_ref_id: 3,
                child_candidate_id: "badge".to_owned(),
                child_node_ref_id: 2,
            },
        ];
        assert!(matches!(
            emit_linked_owner_module_with_node_refs(&input, &duplicate),
            Err(LinkedOwnerJsError::DuplicateNodeRefTarget { .. })
        ));

        let conditional = lifecycle_plan();
        let alias = [LinkedOwnerNodeRefAlias {
            parent_candidate_id: "page".to_owned(),
            parent_node_ref_id: 1,
            child_candidate_id: "badge".to_owned(),
            child_node_ref_id: 2,
        }];
        assert_eq!(
            emit_linked_owner_module_with_node_refs(&conditional, &alias),
            Err(LinkedOwnerJsError::NodeRefTargetIsConditional(
                "badge".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_incomplete_crossing_or_nested_lifecycle_topology() {
        let mut missing = lifecycle_plan();
        missing.lifecycles.clear();
        assert_eq!(
            emit_linked_owner_module(&missing),
            Err(LinkedOwnerJsError::MissingLifecycle("badge".to_owned()))
        );

        let mut incomplete = lifecycle_plan();
        incomplete.lifecycles[0].candidates.pop();
        assert_eq!(
            emit_linked_owner_module(&incomplete),
            Err(LinkedOwnerJsError::LifecycleCandidatesMismatch(
                "badge".to_owned()
            ))
        );

        let mut wrong_parent = lifecycle_plan();
        wrong_parent.lifecycles[0].parent_candidate_id = "detail".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&wrong_parent),
            Err(LinkedOwnerJsError::LifecycleParentMismatch { .. })
        ));

        let mut nested = lifecycle_plan();
        nested.mounts.push(LinkedOwnerMount {
            candidate_id: "nested".to_owned(),
            parent_candidate_id: Some("badge".to_owned()),
            lifecycle_root_candidate_id: Some("nested".to_owned()),
            fragment_asset_id: "nested_fragment".to_owned(),
            fragment_export: "mount_nested_fragment".to_owned(),
            seed_key: "seed_nested".to_owned(),
        });
        nested.lifecycles.push(LinkedOwnerLifecycle {
            root_candidate_id: "nested".to_owned(),
            parent_candidate_id: "badge".to_owned(),
            control_id: 12,
            branch: LinkedOwnerLifecycleBranch::Else,
            candidates: vec!["nested".to_owned()],
        });
        assert!(matches!(
            emit_linked_owner_module(&nested),
            Err(LinkedOwnerJsError::LifecycleParentIsConditional { .. })
        ));
    }

    #[test]
    fn rejects_lifecycle_subtree_with_unconditional_child() {
        let mut input = lifecycle_plan();
        input.mounts.push(mount(
            "unowned_leaf",
            Some("detail"),
            "unowned_leaf_fragment",
            "seed_unowned_leaf",
        ));

        assert_eq!(
            emit_linked_owner_module(&input),
            Err(LinkedOwnerJsError::LifecycleSubtreeNotClosed {
                root: "badge".to_owned(),
                parent: "detail".to_owned(),
                child: "unowned_leaf".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_unknown_or_non_parent_callback_edges_and_duplicate_slots() {
        let mut unknown = plan();
        unknown.callback_edges[0].child_candidate_id = "absent".to_owned();
        assert_eq!(
            emit_linked_owner_module(&unknown),
            Err(LinkedOwnerJsError::UnknownCallbackEndpoint(
                "absent".to_owned()
            ))
        );

        let mut non_parent = plan();
        non_parent.callback_edges[0].parent_candidate_id = "badge".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&non_parent),
            Err(LinkedOwnerJsError::CallbackParentMismatch { .. })
        ));

        let mut duplicate = plan();
        duplicate
            .callback_edges
            .push(duplicate.callback_edges[0].clone());
        assert!(matches!(
            emit_linked_owner_module(&duplicate),
            Err(LinkedOwnerJsError::DuplicateCallbackTarget { .. })
        ));
    }

    #[test]
    fn rejects_invalid_browser_facing_strings() {
        let mut asset = plan();
        asset.mounts[0].fragment_asset_id = "../escape".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&asset),
            Err(LinkedOwnerJsError::InvalidFragmentAssetId(_))
        ));

        let mut export = plan();
        export.mounts[0].fragment_export = "not-valid".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&export),
            Err(LinkedOwnerJsError::InvalidFragmentExport(_))
        ));

        let mut seed = plan();
        seed.mounts[1].seed_key = "bad seed".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&seed),
            Err(LinkedOwnerJsError::InvalidSeedKey(_))
        ));

        let mut event = plan();
        event.strategy = OwnerActivationStrategy::Interaction {
            event: Some("click\nload".to_owned()),
        };
        assert!(matches!(
            emit_linked_owner_module(&event),
            Err(LinkedOwnerJsError::InvalidInteractionEvent(_))
        ));

        let mut query = plan();
        query.strategy = OwnerActivationStrategy::Media {
            query: " (min-width: 1px)".to_owned(),
        };
        assert!(matches!(
            emit_linked_owner_module(&query),
            Err(LinkedOwnerJsError::InvalidMediaQuery(_))
        ));
    }

    #[test]
    fn root_seed_is_the_hydration_registration_key() {
        let mut input = plan();
        input.mounts[0].seed_key = "different".to_owned();
        assert!(matches!(
            emit_linked_owner_module(&input),
            Err(LinkedOwnerJsError::RootSeedDoesNotMatchOwner { .. })
        ));
    }
}
