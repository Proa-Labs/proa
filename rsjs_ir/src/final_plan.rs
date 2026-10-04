// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Exact final-plan byte validation and owner/helper selection shared by the
//! CLI and proc macros. This module has no filesystem or environment inputs.

use crate::abi::{HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, HELPER_SIGNATURE_JSON_HASH_DOMAIN};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PLAN_FORMAT: &str = "proa.rsjs.semantic_link_plan";
const PLAN_SCHEMA_VERSION: u64 = 15;
mod unit_projection;
pub use unit_projection::{
    unit_projection_bytes, unit_projection_bytes_for_units, UNIT_PROJECTION_FILE_SUFFIX,
};

const OWNER_HASH_PLACEHOLDER: &str = "rsjs_owner_content_placeholder";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    Component,
    Helper,
}

impl DefinitionKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Helper => "client helper",
        }
    }
}

/// Exact final record selected for one macro expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalPlanSelection {
    pub plan_id: [u8; 32],
    pub dependency_id: [u8; 32],
}

/// Ownership role selected for one concrete component occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FinalOwnerRole {
    /// This occurrence has no reachable browser obligation.
    ServerOnly,
    /// This occurrence emits the final owner boundary.
    Root { owner_id: u32 },
    /// This occurrence is fused into an owner rooted at another candidate.
    Member {
        owner_id: u32,
        root_candidate_id: u32,
    },
}

/// Effective activation strategy selected for one final owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum FinalOwnerActivationStrategy {
    Eager,
    Visible,
    Idle,
    Interaction { event: Option<String> },
    Media { query: String },
}

/// Exact conditional control governing one concrete component occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FinalComponentLifecycle {
    pub control: u32,
    pub predicate_value: u32,
    pub predicate_origin: u32,
    pub branch: FinalComponentLifecycleBranch,
}

/// Branch polarity selected by one conditional component occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FinalComponentLifecycleBranch {
    Then,
    Else,
}

/// One concrete occurrence of the selected component definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FinalComponentSite {
    pub candidate_id: u32,
    pub parent_candidate_id: Option<u32>,
    pub incoming_edge_id: Option<u32>,
    pub structural_path: Vec<u32>,
    /// Exact conditional control on the incoming edge. Static-single sites
    /// and standalone roots carry `None`.
    pub incoming_lifecycle: Option<FinalComponentLifecycle>,
    /// Conditional root whose browser mount/dispose lifetime governs this
    /// site. A conditional root names itself; static descendants inherit it.
    pub reactive_lifecycle_root_candidate_id: Option<u32>,
    pub owner_role: FinalOwnerRole,
    /// `None` only for server-only occurrences. Every member carries the
    /// effective strategy of the owner it is fused into.
    pub owner_strategy: Option<FinalOwnerActivationStrategy>,
    /// Plan-specific owner module asset. Members name their enclosing owner
    /// but do not publish this asset themselves.
    pub owner_asset_id: Option<String>,
    /// Key used by the adjacent response seed blob. Owner roots reuse the
    /// owner asset ID; fused members get a distinct inert key.
    pub seed_key: Option<String>,
    /// Reusable definition-level fragment mounted for this occurrence.
    pub fragment_asset_id: String,
}

/// Final, candidate-specific time semantics for one inbound live value use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum FinalTemporalPropMode {
    LiveRead,
    SeedOnce { seed_site_id: u32 },
    EffectDependency,
    QueryKeyDependency,
}

/// One exact live value use selected from the linked semantic plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FinalLivePropUse {
    pub parent_candidate_id: u32,
    pub edge_id: u32,
    pub child_candidate_id: u32,
    pub field_id: u32,
    pub crossing_hook_id: u32,
    pub parent_value_id: u32,
    pub child_value_id: u32,
    pub prop_site_id: u32,
    pub temporal: FinalTemporalPropMode,
}

/// Exact Rust callback container shape validated by semantic linking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum FinalCallbackShape {
    Required,
    Optional,
}

/// One exact compiler-owned callback edge selected from the linked plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FinalLiveCallback {
    pub parent_candidate_id: u32,
    pub edge_id: u32,
    pub child_candidate_id: u32,
    pub field_id: u32,
    pub crossing_hook_id: u32,
    pub parent_value_id: u32,
    pub child_value_id: u32,
    pub shape: FinalCallbackShape,
}

/// One exact lexical SharedState provider occurrence selected by the final
/// semantic plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FinalSharedStateProviderOccurrence {
    pub host_candidate_id: u32,
    pub provider_site_id: u32,
    pub child_candidate_id: u32,
    pub type_key: String,
    pub schema_abi: [u8; 32],
}

/// One exact component-local SharedState acquisition and its lexical provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FinalSharedStateProviderRequirement {
    pub consumer_candidate_id: u32,
    pub reference_id: u32,
    pub provider_host_candidate_id: u32,
    pub provider_site_id: u32,
    pub type_key: String,
    pub schema_abi: [u8; 32],
}

/// Terminal DOM family authenticated for an inbound `NodeRef<T>` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum FinalNodeRefBindTarget {
    Element,
    HtmlElement,
    Input,
    Textarea,
    Select,
    Form,
    Dialog,
}

/// One occurrence-specific exact `NodeRef<T>` object alias.
///
/// Both node-ref IDs are producer-local fragment coordinates. The canonical
/// value, field, and crossing IDs are retained so final compilation can
/// independently reject a stale or internally inconsistent link plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FinalNodeRefHandleAlias {
    pub parent_candidate_id: u32,
    pub edge_id: u32,
    pub parent_node_ref_id: u32,
    pub parent_value_id: u32,
    pub child_candidate_id: u32,
    pub field_id: u32,
    pub crossing_hook_id: u32,
    pub child_node_ref_id: u32,
    pub child_value_id: u32,
    pub bind_target: Option<FinalNodeRefBindTarget>,
}

/// Producer-proven root shape for one reusable component fragment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalComponentFragmentRoot {
    ExactOneElement,
    Unsupported,
}

/// Final-codegen coordinates for one client-visible component field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalCodegenInboundField {
    pub field_id: u32,
    pub producer_value_id: u32,
    pub canonical_value_id: u32,
    pub crossing_hook_id: u32,
    pub callback_slot_id: Option<u32>,
}

/// Final-codegen coordinates for one concrete local signal target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalCodegenInboundTarget {
    pub prop_site_id: u32,
    pub field_id: u32,
    pub producer_signal_id: u32,
    pub canonical_signal_value_id: u32,
}

/// One producer-local ID joined to its canonical client value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalCodegenValue {
    pub producer_id: u32,
    pub canonical_value_id: u32,
}

/// Producer-local `NodeRef` represented by one canonical inbound field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalCodegenInboundNodeRef {
    pub field_id: u32,
    pub producer_node_ref_id: u32,
    pub bind_target: Option<FinalNodeRefBindTarget>,
}

/// Producer-local source of one static direct `NodeRef` handle edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalCodegenDirectNodeRefAlias {
    pub edge_id: u32,
    pub field_name: String,
    pub producer_node_ref_id: u32,
    pub canonical_value_id: u32,
}

/// Linker-validated coordinates needed to mount one reusable component
/// fragment without reconstructing producer-local IDs from path spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalComponentCodegen {
    pub asset_id: String,
    pub fragment_key: String,
    pub fragment_root: FinalComponentFragmentRoot,
    /// Whether this reusable fragment contains reachable query work.
    pub uses_queries: bool,
    /// Whether this reusable fragment contains reachable mutation work.
    pub uses_mutations: bool,
    pub inbound_fields: Vec<FinalCodegenInboundField>,
    pub inbound_targets: Vec<FinalCodegenInboundTarget>,
    pub signals: Vec<FinalCodegenValue>,
    pub expressions: Vec<FinalCodegenValue>,
    pub payload_handlers: Vec<FinalCodegenValue>,
    pub callback_slots: Vec<FinalCodegenValue>,
    pub inbound_node_refs: Vec<FinalCodegenInboundNodeRef>,
    pub direct_node_ref_aliases: Vec<FinalCodegenDirectNodeRefAlias>,
}

/// Runtime identity of a logical render occurrence, independent of global
/// planner ordinals, descriptor generations, and unrelated plan contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalRuntimeSiteIdentity {
    /// Logical standalone root containing this occurrence.
    pub scope_id: [u8; 32],
    /// Full identity of the definition at its structural edge path.
    pub site_id: [u8; 32],
    /// Compact server/DOM coordinate. Collisions are rejected across the
    /// complete plan before any component selection is admitted.
    pub candidate_id: u64,
}

/// Exact final component record and all of its plan-specific occurrences.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalComponentSelection {
    pub plan_id: [u8; 32],
    /// Cargo witness identity; the loader replaces the global fallback with
    /// the freshly verified complete unit projection hash.
    pub dependency_id: [u8; 32],
    /// Generation identity of the selected definition's reusable fragment.
    /// It commits the descriptor/compiler generation, export union, and
    /// complete potential helper closure, independently of global ordinals.
    pub fragment_asset_id: String,
    /// Stable runtime identities indexed by internal dense planner candidate.
    pub runtime_sites: BTreeMap<u32, FinalRuntimeSiteIdentity>,
    /// Unique standalone occurrence of this component definition, when its
    /// descriptor permits ordinary root rendering. A component with a
    /// required immutable static slot is rendered only through an exact
    /// incoming component site and therefore has no standalone occurrence.
    pub standalone_candidate_id: Option<u32>,
    /// Every standalone or linked occurrence of this definition.
    pub sites: Vec<FinalComponentSite>,
    /// Exact child occurrence for each selected `(parent candidate, edge)`.
    pub child_sites: BTreeMap<(u32, u32), u32>,
    /// Inbound live value uses grouped by their concrete child occurrence.
    pub live_props_by_child: BTreeMap<u32, Vec<FinalLivePropUse>>,
    /// Inbound live callbacks grouped by their concrete child occurrence.
    pub live_callbacks_by_child: BTreeMap<u32, Vec<FinalLiveCallback>>,
    /// Provider declarations grouped by exact host occurrence of this
    /// component definition.
    pub shared_state_providers_by_host: BTreeMap<u32, Vec<FinalSharedStateProviderOccurrence>>,
    /// SharedState acquisitions grouped by exact consumer occurrence of this
    /// component definition.
    pub shared_state_requirements_by_consumer:
        BTreeMap<u32, Vec<FinalSharedStateProviderRequirement>>,
    /// NodeRef aliases whose parent or child is an occurrence of this
    /// component definition.
    pub node_ref_aliases_by_parent: BTreeMap<u32, Vec<FinalNodeRefHandleAlias>>,
    pub node_ref_aliases_by_child: BTreeMap<u32, Vec<FinalNodeRefHandleAlias>>,
    /// Producer-local codegen coordinates for every candidate in the closed
    /// linked component graph. Owner-root emission needs the complete
    /// partition, not only candidates whose definition is the current macro.
    pub codegen_by_candidate: BTreeMap<u32, FinalComponentCodegen>,
    /// Every component occurrence in the closed linked graph, including
    /// definitions compiled by other macro expansions.
    pub all_sites: Vec<FinalComponentSite>,
    /// Exact child occurrence for every `(parent candidate, edge)` in the
    /// closed graph.
    pub all_child_sites: BTreeMap<(u32, u32), u32>,
    /// All final live value uses, retained so an owner-root expansion can emit
    /// the complete fused owner rather than only its own definition.
    pub all_live_props_by_child: BTreeMap<u32, Vec<FinalLivePropUse>>,
    /// All final callback edges in the closed graph.
    pub all_live_callbacks_by_child: BTreeMap<u32, Vec<FinalLiveCallback>>,
    /// All occurrence-specific NodeRef aliases in the closed graph.
    pub all_node_ref_aliases: Vec<FinalNodeRefHandleAlias>,
    /// Relocation-authoritative helper codegen retained until the final macro
    /// can join producer-local call IDs back through their source-edge IDs.
    pub helper_linkage: FinalHelperLinkage,
}

impl FinalComponentSelection {
    pub fn runtime_site(&self, candidate: u32) -> Result<FinalRuntimeSiteIdentity, String> {
        self.runtime_sites.get(&candidate).copied().ok_or_else(|| {
            format!("linked component candidate {candidate} has no stable runtime identity")
        })
    }
}

/// Final helper material selected for one component definition.
///
/// The semantic plan deliberately uses canonical dense call IDs, while a
/// fresh final macro analysis still carries producer-local IDs. Source-edge
/// identities are the exact relocation-backed bridge between those two ID
/// spaces; authored Rust paths are never consulted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FinalHelperLinkage {
    roots_by_source: BTreeMap<[u8; 32], FinalRootHelperBinding>,
    definitions: Vec<crate::LinkedHelperDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FinalRootHelperBinding {
    target: crate::LinkedHelperKey,
}

/// Join the final macro's producer-local helper IDs to the canonical helper
/// graph selected from the content-addressed semantic plan.
pub fn linked_helper_program(
    selection: &FinalComponentSelection,
    local_edges: impl IntoIterator<Item = (crate::abi::HelperCallEdgeId, [u8; 32])>,
) -> Result<crate::LinkedHelperProgram, String> {
    let mut seen_local = BTreeSet::new();
    let mut seen_sources = BTreeSet::new();
    let mut root_calls = Vec::new();
    let mut reachable = BTreeSet::new();

    for (local_edge, source_edge) in local_edges {
        if !seen_local.insert(local_edge) {
            return Err(format!(
                "final helper linkage contains producer-local edge {} more than once",
                local_edge.index()
            ));
        }
        if !seen_sources.insert(source_edge) {
            return Err(format!(
                "final helper linkage maps source edge `{}` more than once",
                hex_id(&source_edge)
            ));
        }
        let Some(binding) = selection.helper_linkage.roots_by_source.get(&source_edge) else {
            return Err(format!(
                "producer-local helper edge {} has source edge `{}`, but the final semantic plan has no exact root binding for it",
                local_edge.index(),
                hex_id(&source_edge)
            ));
        };
        root_calls.push(crate::LinkedHelperCallBinding::new(
            local_edge,
            binding.target.clone(),
        ));
        reachable.insert(binding.target.clone());
    }

    // Descriptor linkage is intentionally a conservative relocation superset:
    // a final owner can make a derived signal or payload adapter live after
    // the descriptor pass. Final emission supplies the exact post-DSE roots,
    // so select only their transitive helper closure. Requiring every plan
    // root here would force dead potential calls into deployable JavaScript.
    let definitions_by_key = selection
        .helper_linkage
        .definitions
        .iter()
        .map(|definition| (definition.key.clone(), definition))
        .collect::<BTreeMap<_, _>>();
    let mut pending = reachable.clone();
    while let Some(key) = pending.iter().next().cloned() {
        pending.remove(&key);
        let definition = definitions_by_key.get(&key).ok_or_else(|| {
            "final semantic plan helper root has no codegen definition".to_string()
        })?;
        for call in &definition.calls {
            if reachable.insert(call.target.clone()) {
                pending.insert(call.target.clone());
            }
        }
    }
    let definitions = reachable
        .iter()
        .map(|key| {
            (*definitions_by_key
                .get(key)
                .expect("reachable helper definition was checked above"))
            .clone()
        })
        .collect();

    crate::LinkedHelperProgram::new(root_calls, definitions)
        .map_err(|error| format!("invalid final linked-helper program: {error}"))
}

pub trait PlanSelection {
    fn dependency_id(&self) -> [u8; 32];
}

impl PlanSelection for FinalPlanSelection {
    fn dependency_id(&self) -> [u8; 32] {
        self.dependency_id
    }
}

impl PlanSelection for FinalComponentSelection {
    fn dependency_id(&self) -> [u8; 32] {
        self.dependency_id
    }
}

/// Build every linked-owner module rooted by the current component
/// definition.
///
/// Final-plan validation already established the owner partition. This
/// projection only joins canonical graph values back to the producer-local
/// signal/handler coordinates required by reusable fragment modules. Missing
/// or ambiguous joins fail closed; the emitter never guesses from source
/// spelling.
pub fn linked_owner_module_plans(
    selection: &FinalComponentSelection,
) -> Result<Vec<crate::LinkedOwnerModulePlan>, String> {
    let mut root_sites = selection
        .sites
        .iter()
        .filter(|site| matches!(site.owner_role, FinalOwnerRole::Root { .. }))
        .collect::<Vec<_>>();
    root_sites.sort_by_key(|site| site.candidate_id);

    root_sites
        .into_iter()
        .map(|root| linked_owner_module_plan(selection, root))
        .collect()
}

/// Producer-local signals this component definition must expose to a linked
/// child at one or more final occurrences.
pub fn linked_fragment_signal_exports(
    selection: &FinalComponentSelection,
) -> Result<Vec<crate::SignalId>, String> {
    let current_candidates = selection
        .sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    fragment_signal_exports_for(selection, &current_candidates)
}

fn fragment_signal_exports_for(
    selection: &FinalComponentSelection,
    current_candidates: &BTreeSet<u32>,
) -> Result<Vec<crate::SignalId>, String> {
    let mut producer_ids = BTreeSet::new();
    for records in selection.all_live_props_by_child.values() {
        for record in records {
            if !current_candidates.contains(&record.parent_candidate_id) {
                continue;
            }
            let codegen = codegen_for(selection, record.parent_candidate_id)?;
            producer_ids.insert(producer_value_id(
                &codegen.signals,
                record.parent_value_id,
                "parent signal",
                record.parent_candidate_id,
            )?);
        }
    }
    Ok(producer_ids.into_iter().map(crate::SignalId).collect())
}

/// Predicate expressions this component definition must expose to its linked
/// owner so conditional component subtrees can mount and dispose without
/// reconstructing producer-local expression IDs.
pub fn linked_fragment_lifecycle_exports(
    selection: &FinalComponentSelection,
) -> Result<Vec<crate::emit_js::FragmentLifecycleControlExport>, String> {
    let current_candidates = selection
        .sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    fragment_lifecycle_exports_for(selection, &current_candidates)
}

fn fragment_lifecycle_exports_for(
    selection: &FinalComponentSelection,
    current_candidates: &BTreeSet<u32>,
) -> Result<Vec<crate::emit_js::FragmentLifecycleControlExport>, String> {
    let mut controls = BTreeMap::<u32, u32>::new();

    for site in &selection.all_sites {
        let Some(parent_candidate_id) = site.parent_candidate_id else {
            continue;
        };
        if !current_candidates.contains(&parent_candidate_id) {
            continue;
        }
        let Some(lifecycle) = site.incoming_lifecycle else {
            continue;
        };
        let codegen = codegen_for(selection, parent_candidate_id)?;
        let producer_expression_id = producer_value_id(
            &codegen.expressions,
            lifecycle.predicate_value,
            "lifecycle predicate expression",
            parent_candidate_id,
        )?;
        if let Some(previous) = controls.insert(lifecycle.control, producer_expression_id) {
            if previous != producer_expression_id {
                return Err(format!(
                    "linked-owner lifecycle control {} resolves to conflicting predicate \
                     expressions on component candidate {parent_candidate_id}",
                    lifecycle.control
                ));
            }
        }
    }

    Ok(controls
        .into_iter()
        .map(|(control_id, producer_expression_id)| {
            crate::emit_js::FragmentLifecycleControlExport {
                control_id,
                producer_expression_id: crate::ExprId(producer_expression_id),
            }
        })
        .collect())
}

/// Producer-local NodeRef declarations this component fragment must expose to
/// a linked direct child.
pub fn linked_fragment_node_ref_exports(
    selection: &FinalComponentSelection,
) -> Vec<crate::NodeRefId> {
    selection
        .node_ref_aliases_by_parent
        .values()
        .flatten()
        .map(|alias| alias.parent_node_ref_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(crate::NodeRefId)
        .collect()
}

/// Producer-local NodeRef declarations this component fragment must receive
/// from its exact linked parent.
pub fn linked_fragment_node_ref_imports(
    selection: &FinalComponentSelection,
) -> Vec<crate::NodeRefId> {
    selection
        .node_ref_aliases_by_child
        .values()
        .flatten()
        .map(|alias| alias.child_node_ref_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(crate::NodeRefId)
        .collect()
}

/// Exact NodeRef object aliases belonging to one already-validated owner
/// module plan.
pub fn linked_owner_node_ref_aliases(
    selection: &FinalComponentSelection,
    plan: &crate::LinkedOwnerModulePlan,
) -> Result<Vec<crate::LinkedOwnerNodeRefAlias>, String> {
    let candidates = plan
        .mounts
        .iter()
        .map(|mount| {
            mount.candidate_id.parse::<u64>().map_err(|_| {
                format!(
                    "linked-owner mount candidate `{}` is not a canonical u64 runtime ID",
                    mount.candidate_id
                )
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut aliases = Vec::new();
    for alias in &selection.all_node_ref_aliases {
        let parent = selection
            .runtime_site(alias.parent_candidate_id)?
            .candidate_id;
        let child = selection
            .runtime_site(alias.child_candidate_id)?
            .candidate_id;
        let parent_in_owner = candidates.contains(&parent);
        let child_in_owner = candidates.contains(&child);
        match (parent_in_owner, child_in_owner) {
            (false, false) => continue,
            (true, true) => aliases.push(crate::LinkedOwnerNodeRefAlias {
                parent_candidate_id: parent.to_string(),
                parent_node_ref_id: alias.parent_node_ref_id,
                child_candidate_id: child.to_string(),
                child_node_ref_id: alias.child_node_ref_id,
            }),
            _ => {
                return Err(format!(
                    "linked NodeRef alias {}:{} -> {} crosses the boundary of owner `{}`",
                    alias.parent_candidate_id,
                    alias.edge_id,
                    alias.child_candidate_id,
                    plan.owner_asset_id,
                ));
            }
        }
    }
    Ok(aliases)
}

fn linked_owner_module_plan(
    selection: &FinalComponentSelection,
    root: &FinalComponentSite,
) -> Result<crate::LinkedOwnerModulePlan, String> {
    let FinalOwnerRole::Root { owner_id } = root.owner_role else {
        return Err(format!(
            "candidate {} is not a linked-owner root",
            root.candidate_id
        ));
    };
    let owner_asset_id = root.owner_asset_id.clone().ok_or_else(|| {
        format!(
            "linked-owner root candidate {} is missing its owner asset",
            root.candidate_id
        )
    })?;
    let strategy = final_owner_strategy(root.owner_strategy.as_ref().ok_or_else(|| {
        format!(
            "linked-owner root candidate {} is missing its activation strategy",
            root.candidate_id
        )
    })?);

    let mut owner_sites = selection
        .all_sites
        .iter()
        .filter(|site| match site.owner_role {
            FinalOwnerRole::Root {
                owner_id: candidate_owner,
            }
            | FinalOwnerRole::Member {
                owner_id: candidate_owner,
                ..
            } => candidate_owner == owner_id,
            FinalOwnerRole::ServerOnly => false,
        })
        .collect::<Vec<_>>();
    owner_sites.sort_by_key(|site| site.candidate_id);
    if owner_sites.first().map(|site| site.candidate_id) != Some(root.candidate_id) {
        return Err(format!(
            "linked owner {owner_id} does not begin with root candidate {}",
            root.candidate_id
        ));
    }
    let owner_candidates = owner_sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    let uses_queries = owner_sites.iter().try_fold(false, |uses_queries, site| {
        let codegen = selection
            .codegen_by_candidate
            .get(&site.candidate_id)
            .ok_or_else(|| {
                format!(
                    "linked-owner candidate {} has no final component codegen record",
                    site.candidate_id
                )
            })?;
        Ok::<_, String>(uses_queries || codegen.uses_queries)
    })?;
    let uses_mutations = owner_sites.iter().try_fold(false, |uses_mutations, site| {
        let codegen = selection
            .codegen_by_candidate
            .get(&site.candidate_id)
            .ok_or_else(|| {
                format!(
                    "linked-owner candidate {} has no final component codegen record",
                    site.candidate_id
                )
            })?;
        Ok::<_, String>(uses_mutations || codegen.uses_mutations)
    })?;

    let mut mounts = Vec::with_capacity(owner_sites.len());
    for site in &owner_sites {
        let codegen = selection
            .codegen_by_candidate
            .get(&site.candidate_id)
            .ok_or_else(|| {
                format!(
                    "linked-owner candidate {} has no final component codegen record",
                    site.candidate_id
                )
            })?;
        if codegen.fragment_root != FinalComponentFragmentRoot::ExactOneElement {
            return Err(format!(
                "rsjs/linked-owner-fragment-root: candidate {} (`{}`) participates in browser owner {owner_id}, but its reusable fragment does not have exactly one element root",
                site.candidate_id, codegen.asset_id
            ));
        }
        let parent_candidate_id = if site.candidate_id == root.candidate_id {
            None
        } else {
            let parent = site.parent_candidate_id.ok_or_else(|| {
                format!(
                    "linked-owner member candidate {} has no structural parent",
                    site.candidate_id
                )
            })?;
            if !owner_candidates.contains(&parent) {
                return Err(format!(
                    "linked-owner member candidate {} has parent {parent} outside owner {owner_id}",
                    site.candidate_id
                ));
            }
            Some(parent.to_string())
        };
        mounts.push(crate::LinkedOwnerMount {
            candidate_id: site.candidate_id.to_string(),
            parent_candidate_id,
            lifecycle_root_candidate_id: site
                .reactive_lifecycle_root_candidate_id
                .map(|candidate| candidate.to_string()),
            fragment_asset_id: codegen.fragment_key.clone(),
            fragment_export: crate::emit_js::component_fragment_export_name(&codegen.fragment_key),
            seed_key: site.seed_key.clone().ok_or_else(|| {
                format!(
                    "linked-owner candidate {} is missing its response seed key",
                    site.candidate_id
                )
            })?,
        });
    }

    let mut signal_aliases = BTreeSet::new();
    let mut signal_seeds = BTreeSet::new();
    for (&child_candidate_id, records) in &selection.all_live_props_by_child {
        if !owner_candidates.contains(&child_candidate_id) {
            continue;
        }
        for record in records {
            ensure_direct_owner_edge(
                selection,
                &owner_candidates,
                record.parent_candidate_id,
                record.edge_id,
                child_candidate_id,
            )?;
            let parent_codegen = codegen_for(selection, record.parent_candidate_id)?;
            let child_codegen = codegen_for(selection, child_candidate_id)?;
            let parent_signal_id = producer_value_id(
                &parent_codegen.signals,
                record.parent_value_id,
                "parent signal",
                record.parent_candidate_id,
            )?;
            let child_signal_id = inbound_signal_target(child_codegen, record, child_candidate_id)?;
            match record.temporal {
                FinalTemporalPropMode::SeedOnce { .. } => {
                    signal_seeds.insert((
                        record.parent_candidate_id.to_string(),
                        parent_signal_id,
                        child_candidate_id.to_string(),
                        child_signal_id,
                    ));
                }
                FinalTemporalPropMode::LiveRead
                | FinalTemporalPropMode::EffectDependency
                | FinalTemporalPropMode::QueryKeyDependency => {
                    signal_aliases.insert(crate::LinkedOwnerSignalAlias {
                        parent_candidate_id: record.parent_candidate_id.to_string(),
                        parent_signal_id,
                        child_candidate_id: child_candidate_id.to_string(),
                        child_signal_id,
                    });
                }
            }
        }
    }

    let mut callback_edges = Vec::new();
    for (&child_candidate_id, records) in &selection.all_live_callbacks_by_child {
        if !owner_candidates.contains(&child_candidate_id) {
            continue;
        }
        for record in records {
            ensure_direct_owner_edge(
                selection,
                &owner_candidates,
                record.parent_candidate_id,
                record.edge_id,
                child_candidate_id,
            )?;
            let child_codegen = codegen_for(selection, child_candidate_id)?;
            let child_slots = child_codegen
                .callback_slots
                .iter()
                .filter(|slot| slot.canonical_value_id == record.child_value_id)
                .collect::<Vec<_>>();
            let child_callback_slot_id = match child_slots.as_slice() {
                [] => {
                    // A pure projection forwards the canonical callback value
                    // but never invokes it locally, so it owns no callback
                    // slot and needs no runtime installation.
                    continue;
                }
                [slot] => slot.producer_id,
                _ => {
                    return Err(format!(
                        "linked-owner child callback value {} on candidate {child_candidate_id} resolves to more than one producer slot",
                        record.child_value_id
                    ));
                }
            };
            let (handler_candidate_id, parent_payload_handler_id) = resolve_payload_handler_source(
                selection,
                record.parent_candidate_id,
                record.parent_value_id,
                &mut BTreeSet::new(),
            )?;
            if !owner_candidates.contains(&handler_candidate_id) {
                return Err(format!(
                    "linked-owner callback source candidate {handler_candidate_id} is outside owner {owner_id}"
                ));
            }
            callback_edges.push(crate::LinkedOwnerCallbackEdge {
                parent_candidate_id: handler_candidate_id.to_string(),
                parent_payload_handler_id,
                child_candidate_id: child_candidate_id.to_string(),
                child_callback_slot_id,
            });
        }
    }

    let mut lifecycles = Vec::new();
    for site in &owner_sites {
        let Some(lifecycle) = site.incoming_lifecycle else {
            continue;
        };
        if site.reactive_lifecycle_root_candidate_id != Some(site.candidate_id) {
            return Err(format!(
                "linked-owner conditional candidate {} does not name itself as lifecycle root",
                site.candidate_id
            ));
        }
        let parent_candidate_id = site.parent_candidate_id.ok_or_else(|| {
            format!(
                "linked-owner conditional candidate {} has no structural parent",
                site.candidate_id
            )
        })?;
        let candidates = owner_sites
            .iter()
            .filter(|candidate| {
                candidate.reactive_lifecycle_root_candidate_id == Some(site.candidate_id)
            })
            .map(|candidate| candidate.candidate_id.to_string())
            .collect::<Vec<_>>();
        let root_candidate_id = site.candidate_id.to_string();
        if candidates.first() != Some(&root_candidate_id) {
            return Err(format!(
                "linked-owner conditional candidate {} is not first in its lifecycle subtree",
                site.candidate_id
            ));
        }
        lifecycles.push(crate::LinkedOwnerLifecycle {
            root_candidate_id: site.candidate_id.to_string(),
            parent_candidate_id: parent_candidate_id.to_string(),
            control_id: lifecycle.control,
            branch: match lifecycle.branch {
                FinalComponentLifecycleBranch::Then => crate::LinkedOwnerLifecycleBranch::Then,
                FinalComponentLifecycleBranch::Else => crate::LinkedOwnerLifecycleBranch::Else,
            },
            candidates,
        });
    }

    let mut plan = crate::LinkedOwnerModulePlan {
        owner_asset_id,
        root_candidate_id: root.candidate_id.to_string(),
        strategy,
        uses_queries,
        uses_mutations,
        mounts,
        signal_aliases: signal_aliases.into_iter().collect(),
        signal_seeds: signal_seeds
            .into_iter()
            .map(
                |(parent_candidate_id, parent_signal_id, child_candidate_id, child_signal_id)| {
                    crate::LinkedOwnerSignalSeed {
                        parent_candidate_id,
                        parent_signal_id,
                        child_candidate_id,
                        child_signal_id,
                    }
                },
            )
            .collect(),
        callback_edges,
        lifecycles,
    };
    normalize_owner_runtime_candidates(selection, &mut plan)?;
    Ok(plan)
}

fn normalize_owner_runtime_candidates(
    selection: &FinalComponentSelection,
    plan: &mut crate::LinkedOwnerModulePlan,
) -> Result<(), String> {
    let normalize = |candidate: &mut String| -> Result<(), String> {
        let dense = candidate.parse::<u32>().map_err(|_| {
            format!("linked-owner candidate `{candidate}` is not a dense planner coordinate")
        })?;
        *candidate = selection.runtime_site(dense)?.candidate_id.to_string();
        Ok(())
    };
    normalize(&mut plan.root_candidate_id)?;
    for mount in &mut plan.mounts {
        normalize(&mut mount.candidate_id)?;
        if let Some(parent) = &mut mount.parent_candidate_id {
            normalize(parent)?;
        }
        if let Some(root) = &mut mount.lifecycle_root_candidate_id {
            normalize(root)?;
        }
    }
    for edge in &mut plan.signal_aliases {
        normalize(&mut edge.parent_candidate_id)?;
        normalize(&mut edge.child_candidate_id)?;
    }
    for edge in &mut plan.signal_seeds {
        normalize(&mut edge.parent_candidate_id)?;
        normalize(&mut edge.child_candidate_id)?;
    }
    for edge in &mut plan.callback_edges {
        normalize(&mut edge.parent_candidate_id)?;
        normalize(&mut edge.child_candidate_id)?;
    }
    for lifecycle in &mut plan.lifecycles {
        normalize(&mut lifecycle.root_candidate_id)?;
        normalize(&mut lifecycle.parent_candidate_id)?;
        for candidate in &mut lifecycle.candidates {
            normalize(candidate)?;
        }
    }
    Ok(())
}

fn resolve_payload_handler_source(
    selection: &FinalComponentSelection,
    candidate_id: u32,
    canonical_value_id: u32,
    visiting: &mut BTreeSet<(u32, u32)>,
) -> Result<(u32, u32), String> {
    if !visiting.insert((candidate_id, canonical_value_id)) {
        return Err(format!(
            "linked-owner callback forwarding cycle reaches candidate {candidate_id} value {canonical_value_id}"
        ));
    }
    let codegen = codegen_for(selection, candidate_id)?;
    let local = codegen
        .payload_handlers
        .iter()
        .filter(|handler| handler.canonical_value_id == canonical_value_id)
        .collect::<Vec<_>>();
    match local.as_slice() {
        [handler] => return Ok((candidate_id, handler.producer_id)),
        [] => {}
        _ => {
            return Err(format!(
                "linked-owner callback value {canonical_value_id} on candidate {candidate_id} resolves to more than one local payload handler"
            ));
        }
    }

    let incoming = selection
        .all_live_callbacks_by_child
        .get(&candidate_id)
        .into_iter()
        .flatten()
        .filter(|record| record.child_value_id == canonical_value_id)
        .collect::<Vec<_>>();
    let [incoming] = incoming.as_slice() else {
        return Err(format!(
            "linked-owner callback value {canonical_value_id} on candidate {candidate_id} resolves to neither one local payload handler nor one inbound callback edge"
        ));
    };
    resolve_payload_handler_source(
        selection,
        incoming.parent_candidate_id,
        incoming.parent_value_id,
        visiting,
    )
}

fn codegen_for(
    selection: &FinalComponentSelection,
    candidate_id: u32,
) -> Result<&FinalComponentCodegen, String> {
    selection
        .codegen_by_candidate
        .get(&candidate_id)
        .ok_or_else(|| {
            format!("linked-owner candidate {candidate_id} has no component codegen record")
        })
}

fn producer_value_id(
    values: &[FinalCodegenValue],
    canonical_value_id: u32,
    label: &str,
    candidate_id: u32,
) -> Result<u32, String> {
    let matches = values
        .iter()
        .filter(|value| value.canonical_value_id == canonical_value_id)
        .collect::<Vec<_>>();
    let [value] = matches.as_slice() else {
        return Err(format!(
            "linked-owner {label} value {canonical_value_id} on candidate {candidate_id} does not resolve to exactly one producer-local ID"
        ));
    };
    Ok(value.producer_id)
}

fn inbound_signal_target(
    codegen: &FinalComponentCodegen,
    record: &FinalLivePropUse,
    child_candidate_id: u32,
) -> Result<u32, String> {
    let inbound_fields = codegen
        .inbound_fields
        .iter()
        .filter(|field| {
            field.field_id == record.field_id
                && field.canonical_value_id == record.child_value_id
                && field.crossing_hook_id == record.crossing_hook_id
                && field.callback_slot_id.is_none()
        })
        .collect::<Vec<_>>();
    let [_inbound_field] = inbound_fields.as_slice() else {
        return Err(format!(
            "linked live prop for child candidate {child_candidate_id}, field {}, crossing {}, value {} does not resolve to exactly one inbound value field",
            record.field_id, record.crossing_hook_id, record.child_value_id
        ));
    };

    // The inbound field value and its synthetic current-value signal are
    // distinct canonical graph nodes. The semantic adapter has already
    // validated the exact value-to-signal data dependency; final codegen joins
    // the producer-local target by the dense prop-site and field coordinates
    // carried by that validated record.
    let targets = codegen
        .inbound_targets
        .iter()
        .filter(|target| {
            target.prop_site_id == record.prop_site_id && target.field_id == record.field_id
        })
        .collect::<Vec<_>>();
    let [target] = targets.as_slice() else {
        return Err(format!(
            "linked live prop for child candidate {child_candidate_id}, field {}, prop site {} does not resolve to exactly one producer-local signal target",
            record.field_id, record.prop_site_id
        ));
    };

    let signals = codegen
        .signals
        .iter()
        .filter(|signal| {
            signal.producer_id == target.producer_signal_id
                && signal.canonical_value_id == target.canonical_signal_value_id
        })
        .collect::<Vec<_>>();
    let [_signal] = signals.as_slice() else {
        return Err(format!(
            "linked live prop for child candidate {child_candidate_id}, field {}, prop site {} targets producer signal {}, but that signal does not resolve to its canonical value {}",
            record.field_id,
            record.prop_site_id,
            target.producer_signal_id,
            target.canonical_signal_value_id
        ));
    };

    Ok(target.producer_signal_id)
}

fn ensure_direct_owner_edge(
    selection: &FinalComponentSelection,
    owner_candidates: &BTreeSet<u32>,
    parent_candidate_id: u32,
    edge_id: u32,
    child_candidate_id: u32,
) -> Result<(), String> {
    if !owner_candidates.contains(&parent_candidate_id) {
        return Err(format!(
            "linked input {parent_candidate_id} -> {child_candidate_id} crosses owner boundaries"
        ));
    }
    if selection
        .all_child_sites
        .get(&(parent_candidate_id, edge_id))
        != Some(&child_candidate_id)
    {
        return Err(format!(
            "linked input {parent_candidate_id} --edge {edge_id}--> {child_candidate_id} is not the exact direct component edge"
        ));
    }
    Ok(())
}

fn final_owner_strategy(
    strategy: &FinalOwnerActivationStrategy,
) -> crate::link::OwnerActivationStrategy {
    match strategy {
        FinalOwnerActivationStrategy::Eager => crate::link::OwnerActivationStrategy::Eager,
        FinalOwnerActivationStrategy::Visible => crate::link::OwnerActivationStrategy::Visible,
        FinalOwnerActivationStrategy::Idle => crate::link::OwnerActivationStrategy::Idle,
        FinalOwnerActivationStrategy::Interaction { event } => {
            crate::link::OwnerActivationStrategy::Interaction {
                event: event.clone(),
            }
        }
        FinalOwnerActivationStrategy::Media { query } => {
            crate::link::OwnerActivationStrategy::Media {
                query: query.clone(),
            }
        }
    }
}

pub fn validate_plan_header(bytes: &[u8], expected_plan_id: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("final RSJS plan is not valid JSON: {error}"))?;
    let root = object(&value, "final RSJS plan")?;
    exact_str(root, "format", PLAN_FORMAT)?;
    exact_u64(root, "schema_version", PLAN_SCHEMA_VERSION)?;
    let actual_plan_id = string(root, "plan_id")?;
    decode_lower_hex_id("plan.plan_id", actual_plan_id)?;
    if actual_plan_id != expected_plan_id {
        return Err(format!(
            "final RSJS plan ID mismatch: build expects `{expected_plan_id}`, file contains `{actual_plan_id}`"
        ));
    }
    Ok(value)
}

pub fn validate_helper_plan_bytes(
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
    plan_id: [u8; 32],
    bytes: &[u8],
) -> Result<FinalPlanSelection, String> {
    let expected_plan_id = hex_id(&plan_id);
    let value = validate_plan_header(bytes, &expected_plan_id)?;
    let root = object(&value, "final RSJS plan")?;
    let helpers = parse_helpers(root)?;
    parse_helper_codegen(root, &helpers)?;
    let expected_definition = DefinitionId {
        cargo_unit_id,
        record_id,
    };
    if helpers.contains_key(&expected_definition) {
        Ok(FinalPlanSelection {
            plan_id,
            dependency_id: plan_id,
        })
    } else {
        Err(format!(
            "final RSJS plan `{expected_plan_id}` has no {} definition for descriptor unit `{}` and record `{}`; the plan or final-unit mapping is stale",
            DefinitionKind::Helper.label(),
            hex_id(&cargo_unit_id),
            hex_id(&record_id)
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DefinitionId {
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
}

#[derive(Clone, Debug)]
struct ComponentEdge {
    target_definition: DefinitionId,
    target_semantic_key: String,
    lifecycle: ComponentEdgeLifecycle,
}

#[derive(Clone, Debug)]
struct HelperEdgeRecord {
    source_edge: [u8; 32],
    target_definition: DefinitionId,
    target: crate::LinkedHelperKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComponentEdgeLifecycleBranch {
    Then,
    Else,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComponentEdgeLifecycle {
    StaticSingle,
    ConditionalSingle {
        control: u32,
        predicate_value: u32,
        predicate_origin: u32,
        branch: ComponentEdgeLifecycleBranch,
    },
}

#[derive(Clone, Debug)]
struct ComponentRecord {
    semantic_key: String,
    root_eligible: bool,
    edges: BTreeMap<u32, ComponentEdge>,
    helper_edges: BTreeMap<u32, HelperEdgeRecord>,
    helper_closure: Vec<crate::LinkedHelperKey>,
}

#[derive(Clone, Debug)]
struct HelperRecord {
    semantic_key: crate::LinkedHelperKey,
    signature_hash: [u8; 32],
    client_ir_hash: [u8; 32],
    helper_edges: BTreeMap<u32, HelperEdgeRecord>,
    transitive_closure: Vec<crate::LinkedHelperKey>,
}

#[derive(Clone, Debug)]
struct ComponentSiteRecord {
    candidate_id: u32,
    root_semantic_key: String,
    component_semantic_key: String,
    definition: DefinitionId,
    parent_candidate_id: Option<u32>,
    incoming_edge_id: Option<u32>,
    structural_path: Vec<u32>,
    incoming_lifecycle: Option<ComponentEdgeLifecycle>,
    reactive_lifecycle_root_candidate_id: Option<u32>,
}

fn runtime_site_identities(
    sites: &[ComponentSiteRecord],
) -> Result<BTreeMap<u32, FinalRuntimeSiteIdentity>, String> {
    let mut identities = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    for site in sites {
        let root = decode_lower_hex_bytes("runtime root semantic key", &site.root_semantic_key)?;
        let component = decode_lower_hex_bytes(
            "runtime component semantic key",
            &site.component_semantic_key,
        )?;
        let mut scope = Sha256::new();
        scope.update(b"proa.rsjs.runtime-root.v1\0");
        scope.update((root.len() as u64).to_le_bytes());
        scope.update(&root);
        let scope_id: [u8; 32] = scope.finalize().into();
        let mut occurrence = Sha256::new();
        occurrence.update(b"proa.rsjs.runtime-site.v1\0");
        occurrence.update(scope_id);
        occurrence.update((component.len() as u64).to_le_bytes());
        occurrence.update(&component);
        occurrence.update((site.structural_path.len() as u64).to_le_bytes());
        for edge in &site.structural_path {
            occurrence.update(edge.to_le_bytes());
        }
        let site_id: [u8; 32] = occurrence.finalize().into();
        let identity = FinalRuntimeSiteIdentity {
            scope_id,
            site_id,
            candidate_id: u64::from_le_bytes(site_id[..8].try_into().expect("eight hash bytes")),
        };
        admit_runtime_candidate(&mut candidates, identity)?;
        if identities.insert(site.candidate_id, identity).is_some() {
            return Err(format!(
                "duplicate planner candidate {} in runtime identity table",
                site.candidate_id
            ));
        }
    }
    Ok(identities)
}

fn admit_runtime_candidate(
    candidates: &mut BTreeMap<u64, [u8; 32]>,
    identity: FinalRuntimeSiteIdentity,
) -> Result<(), String> {
    if let Some(previous) = candidates.insert(identity.candidate_id, identity.site_id) {
        return Err(format!(
            "runtime component-site identity collision at candidate {} between {} and {}; \
             refusing to publish an ambiguous render plan",
            identity.candidate_id,
            hex_id(&previous),
            hex_id(&identity.site_id),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StandaloneRootDispositionRecord {
    StructuralIncomingOnly,
    Admitted { candidate_id: u32 },
    ContextRequired,
}

pub fn validate_component_plan_bytes(
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
    plan_id: [u8; 32],
    bytes: &[u8],
) -> Result<FinalComponentSelection, String> {
    let expected_plan_id = hex_id(&plan_id);
    let value = validate_plan_header(bytes, &expected_plan_id)?;
    let root = object(&value, "final RSJS plan")?;
    let components = parse_components(root)?;
    let helpers = parse_helpers(root)?;
    let helper_codegen = parse_helper_codegen(root, &helpers)?;
    validate_component_helper_graph(&components, &helpers)?;
    let component_codegen = parse_component_codegen(root, &components)?;
    let sites = parse_component_sites(root, &components)?;
    let child_sites = validate_site_structure(&components, &sites)?;
    let runtime_sites = runtime_site_identities(&sites)?;
    let standalone_roots = parse_standalone_roots(root, &components, &sites)?;
    let (candidate_to_owner, owner_roots, owner_strategies) = validate_owner_mapping(root, &sites)?;
    validate_lifecycle_owner_groups(&sites, &candidate_to_owner)?;
    let (live_props_by_child, live_callbacks_by_child) =
        parse_component_inputs(root, &sites, &child_sites)?;
    let (shared_state_providers_by_host, shared_state_requirements_by_consumer) =
        parse_shared_state_provider_topology(root, &sites)?;
    let node_ref_aliases = parse_node_ref_aliases(
        root,
        &sites,
        &child_sites,
        &candidate_to_owner,
        &component_codegen,
    )?;

    let selected_definition = DefinitionId {
        cargo_unit_id,
        record_id,
    };
    if !components.contains_key(&selected_definition) {
        return Err(format!(
            "final RSJS plan `{expected_plan_id}` has no component definition for descriptor unit `{}` and record `{}`; the plan or final-unit mapping is stale",
            hex_id(&cargo_unit_id),
            hex_id(&record_id)
        ));
    }
    let helper_linkage = select_component_helper_linkage(
        selected_definition,
        &components,
        &helpers,
        &helper_codegen,
    )?;

    let mut standalone = None;
    let mut all_sites = Vec::with_capacity(sites.len());
    let mut selected_sites = Vec::new();
    for site in &sites {
        let is_selected = site.definition == selected_definition;
        if is_selected
            && site.parent_candidate_id.is_none()
            && standalone.replace(site.candidate_id).is_some()
        {
            return Err(format!(
                "component definition `{}`/`{}` has more than one standalone component site",
                hex_id(&cargo_unit_id),
                hex_id(&record_id)
            ));
        }
        let site_codegen = component_codegen.get(&site.definition).ok_or_else(|| {
            format!(
                "component candidate {} has no final codegen record for definition `{}`/`{}`",
                site.candidate_id,
                hex_id(&site.definition.cargo_unit_id),
                hex_id(&site.definition.record_id),
            )
        })?;
        let (owner_role, owner_strategy) = match candidate_to_owner.get(&site.candidate_id).copied()
        {
            None => (FinalOwnerRole::ServerOnly, None),
            Some(owner_id) => {
                let root_candidate_id = owner_roots[&owner_id];
                let role = if root_candidate_id == site.candidate_id {
                    FinalOwnerRole::Root { owner_id }
                } else {
                    FinalOwnerRole::Member {
                        owner_id,
                        root_candidate_id,
                    }
                };
                (role, Some(owner_strategies[&owner_id].clone()))
            }
        };
        let (owner_asset_id, seed_key) = match owner_role {
            FinalOwnerRole::ServerOnly => (None, None),
            FinalOwnerRole::Root { .. } => {
                let owner_asset = OWNER_HASH_PLACEHOLDER.to_owned();
                (Some(owner_asset.clone()), Some(owner_asset))
            }
            FinalOwnerRole::Member { .. } => (
                Some(OWNER_HASH_PLACEHOLDER.to_owned()),
                Some(member_seed_key(runtime_sites[&site.candidate_id].site_id)),
            ),
        };
        let final_site = FinalComponentSite {
            candidate_id: site.candidate_id,
            parent_candidate_id: site.parent_candidate_id,
            incoming_edge_id: site.incoming_edge_id,
            structural_path: site.structural_path.clone(),
            incoming_lifecycle: site.incoming_lifecycle.and_then(final_component_lifecycle),
            reactive_lifecycle_root_candidate_id: site.reactive_lifecycle_root_candidate_id,
            owner_role,
            owner_strategy,
            owner_asset_id,
            seed_key,
            fragment_asset_id: site_codegen.fragment_key.clone(),
        };
        if is_selected {
            selected_sites.push(final_site.clone());
        }
        all_sites.push(final_site);
    }
    let standalone_candidate_id = match standalone_roots[&selected_definition] {
        StandaloneRootDispositionRecord::Admitted { candidate_id } => {
            if standalone != Some(candidate_id) {
                return Err(format!(
                    "standalone-root decision for component definition `{}`/`{}` selects candidate {candidate_id}, but its concrete root site is {standalone:?}",
                    hex_id(&cargo_unit_id),
                    hex_id(&record_id)
                ));
            }
            Some(candidate_id)
        }
        StandaloneRootDispositionRecord::StructuralIncomingOnly
        | StandaloneRootDispositionRecord::ContextRequired => {
            if standalone.is_some() {
                return Err(format!(
                    "non-standalone component definition `{}`/`{}` unexpectedly has root candidate {standalone:?}",
                    hex_id(&cargo_unit_id),
                    hex_id(&record_id)
                ));
            }
            None
        }
    };
    let selected_candidates = selected_sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    let all_child_sites = child_sites.clone();
    let all_live_props_by_child = live_props_by_child.clone();
    let all_live_callbacks_by_child = live_callbacks_by_child.clone();
    let all_node_ref_aliases = node_ref_aliases.clone();
    let child_sites = child_sites
        .into_iter()
        .filter(|((parent, _), _)| selected_candidates.contains(parent))
        .collect();
    let live_props_by_child = live_props_by_child
        .into_iter()
        .filter(|(child, _)| selected_candidates.contains(child))
        .collect();
    let live_callbacks_by_child = live_callbacks_by_child
        .into_iter()
        .filter(|(child, _)| selected_candidates.contains(child))
        .collect();
    let shared_state_providers_by_host = shared_state_providers_by_host
        .into_iter()
        .filter(|(host, _)| selected_candidates.contains(host))
        .collect();
    let shared_state_requirements_by_consumer = shared_state_requirements_by_consumer
        .into_iter()
        .filter(|(consumer, _)| selected_candidates.contains(consumer))
        .collect();
    let mut node_ref_aliases_by_parent = BTreeMap::<u32, Vec<FinalNodeRefHandleAlias>>::new();
    let mut node_ref_aliases_by_child = BTreeMap::<u32, Vec<FinalNodeRefHandleAlias>>::new();
    for alias in node_ref_aliases {
        if selected_candidates.contains(&alias.parent_candidate_id) {
            node_ref_aliases_by_parent
                .entry(alias.parent_candidate_id)
                .or_default()
                .push(alias);
        }
        if selected_candidates.contains(&alias.child_candidate_id) {
            node_ref_aliases_by_child
                .entry(alias.child_candidate_id)
                .or_default()
                .push(alias);
        }
    }
    let codegen_by_candidate = sites
        .iter()
        .map(|site| {
            let codegen = component_codegen.get(&site.definition).ok_or_else(|| {
                format!(
                    "component candidate {} has no final codegen record for definition `{}`/`{}`",
                    site.candidate_id,
                    hex_id(&site.definition.cargo_unit_id),
                    hex_id(&site.definition.record_id),
                )
            })?;
            Ok((site.candidate_id, codegen.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;

    let mut selection = FinalComponentSelection {
        plan_id,
        dependency_id: plan_id,
        fragment_asset_id: String::new(),
        runtime_sites,
        standalone_candidate_id,
        sites: selected_sites,
        child_sites,
        live_props_by_child,
        live_callbacks_by_child,
        shared_state_providers_by_host,
        shared_state_requirements_by_consumer,
        node_ref_aliases_by_parent,
        node_ref_aliases_by_child,
        codegen_by_candidate,
        all_sites,
        all_child_sites,
        all_live_props_by_child,
        all_live_callbacks_by_child,
        all_node_ref_aliases,
        helper_linkage,
    };
    finalize_asset_identities(
        &mut selection,
        selected_definition,
        &components,
        &sites,
        root,
    )?;
    Ok(selection)
}

fn parse_components(
    root: &Map<String, Value>,
) -> Result<BTreeMap<DefinitionId, ComponentRecord>, String> {
    let mut components = BTreeMap::new();
    for (index, value) in array(root, "components")?.iter().enumerate() {
        let label = format!("components[{index}]");
        let entry = object(value, &label)?;
        let definition = parse_definition_field(entry, "definition", &label)?;
        let semantic_key = canonical_hex_string(entry, "semantic_key_canonical", &label)?;
        let root_eligible = bool_field(entry, "root_eligible", &label)?;
        let mut edges = BTreeMap::new();
        for (edge_index, value) in array(entry, "component_edges")?.iter().enumerate() {
            let edge_label = format!("{label}.component_edges[{edge_index}]");
            let edge = object(value, &edge_label)?;
            let edge_id = u32_field(edge, "edge_id", &edge_label)?;
            let target_definition = parse_definition_field(edge, "target_definition", &edge_label)?;
            let target_semantic_key =
                canonical_hex_string(edge, "target_semantic_key_canonical", &edge_label)?;
            let lifecycle = parse_component_edge_lifecycle(
                edge.get("lifecycle")
                    .ok_or_else(|| format!("{edge_label} is missing `lifecycle`"))?,
                &edge_label,
            )?;
            if edges
                .insert(
                    edge_id,
                    ComponentEdge {
                        target_definition,
                        target_semantic_key,
                        lifecycle,
                    },
                )
                .is_some()
            {
                return Err(format!(
                    "{label} contains duplicate component edge {edge_id}"
                ));
            }
        }
        let helper_edges = parse_helper_edges(entry, &label)?;
        let helper_closure = parse_helper_key_array(entry, "helper_closure", &label)?;
        if components
            .insert(
                definition,
                ComponentRecord {
                    semantic_key,
                    root_eligible,
                    edges,
                    helper_edges,
                    helper_closure,
                },
            )
            .is_some()
        {
            return Err(format!(
                "final RSJS plan contains duplicate component definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }

    for (definition, component) in &components {
        for (edge_id, edge) in &component.edges {
            let Some(target) = components.get(&edge.target_definition) else {
                return Err(format!(
                    "component `{}`/`{}` edge {edge_id} targets a definition absent from `components`",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            };
            if target.semantic_key != edge.target_semantic_key {
                return Err(format!(
                    "component `{}`/`{}` edge {edge_id} target semantic key disagrees with its exact target definition",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            }
        }
    }
    Ok(components)
}

fn parse_helper_edges(
    entry: &Map<String, Value>,
    label: &str,
) -> Result<BTreeMap<u32, HelperEdgeRecord>, String> {
    let mut edges = BTreeMap::new();
    let mut source_edges = BTreeSet::new();
    for (index, value) in array(entry, "helper_edges")?.iter().enumerate() {
        let edge_label = format!("{label}.helper_edges[{index}]");
        let edge = object(value, &edge_label)?;
        let call_id = u32_field(edge, "call_id", &edge_label)?;
        let expected = u32::try_from(index)
            .map_err(|_| format!("{label}.helper_edges exceeds the u32 ID space"))?;
        if call_id != expected {
            return Err(format!(
                "{edge_label}.call_id must be dense at {expected}; found {call_id}"
            ));
        }
        let source_edge = decode_lower_hex_id(
            &format!("{edge_label}.source_edge_id"),
            string(edge, "source_edge_id")?,
        )?;
        if !source_edges.insert(source_edge) {
            return Err(format!(
                "{label}.helper_edges contains duplicate source edge `{}`",
                hex_id(&source_edge)
            ));
        }
        let target_definition = parse_definition_field(edge, "target_definition", &edge_label)?;
        let target = linked_helper_key(edge, "target_semantic_key_canonical", &edge_label)?;
        if edges
            .insert(
                call_id,
                HelperEdgeRecord {
                    source_edge,
                    target_definition,
                    target,
                },
            )
            .is_some()
        {
            return Err(format!(
                "{label}.helper_edges contains duplicate call ID {call_id}"
            ));
        }
    }
    Ok(edges)
}

fn parse_helper_key_array(
    entry: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Vec<crate::LinkedHelperKey>, String> {
    let values = array(entry, field)?;
    let mut keys = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let encoded = value.as_str().ok_or_else(|| {
            format!("{label}.{field}[{index}] must be lowercase canonical hexadecimal")
        })?;
        let key = linked_helper_key_text(&format!("{label}.{field}[{index}]"), encoded)?;
        if !seen.insert(key.clone()) {
            return Err(format!(
                "{label}.{field} contains duplicate canonical helper identity at index {index}"
            ));
        }
        keys.push(key);
    }
    Ok(keys)
}

fn parse_helpers(
    root: &Map<String, Value>,
) -> Result<BTreeMap<DefinitionId, HelperRecord>, String> {
    let mut helpers = BTreeMap::new();
    let mut definitions_by_key = BTreeMap::new();
    for (index, value) in array(root, "helpers")?.iter().enumerate() {
        let label = format!("helpers[{index}]");
        let entry = object(value, &label)?;
        let definition = parse_definition_field(entry, "definition", &label)?;
        let semantic_key = linked_helper_key(entry, "semantic_key_canonical", &label)?;
        let signature_hash = sha256_field(entry, "signature_hash", &label)?;
        let client_ir_hash = sha256_field(entry, "client_ir_hash", &label)?;
        let helper_edges = parse_helper_edges(entry, &label)?;
        let transitive_closure = parse_helper_key_array(entry, "transitive_closure", &label)?;

        if definitions_by_key
            .insert(semantic_key.clone(), definition)
            .is_some()
        {
            return Err(format!(
                "final RSJS plan contains duplicate canonical helper identity in {label}"
            ));
        }
        if helpers
            .insert(
                definition,
                HelperRecord {
                    semantic_key,
                    signature_hash,
                    client_ir_hash,
                    helper_edges,
                    transitive_closure,
                },
            )
            .is_some()
        {
            return Err(format!(
                "final RSJS plan contains duplicate helper definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }

    for (definition, helper) in &helpers {
        for (call_id, edge) in &helper.helper_edges {
            let Some(target) = helpers.get(&edge.target_definition) else {
                return Err(format!(
                    "helper `{}`/`{}` call {call_id} targets a definition absent from `helpers`",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            };
            if target.semantic_key != edge.target {
                return Err(format!(
                    "helper `{}`/`{}` call {call_id} target semantic identity disagrees with its exact target definition",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            }
        }
        let actual = helper_reachable_keys(&helper.semantic_key, &helpers, &definitions_by_key);
        let stored = helper
            .transitive_closure
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual != stored {
            return Err(format!(
                "helper `{}`/`{}` transitive_closure is not the exact reachable helper set",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }
    Ok(helpers)
}

fn helper_reachable_keys(
    root: &crate::LinkedHelperKey,
    helpers: &BTreeMap<DefinitionId, HelperRecord>,
    definitions_by_key: &BTreeMap<crate::LinkedHelperKey, DefinitionId>,
) -> BTreeSet<crate::LinkedHelperKey> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root.clone()];
    while let Some(key) = pending.pop() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(definition) = definitions_by_key.get(&key) else {
            continue;
        };
        if let Some(helper) = helpers.get(definition) {
            pending.extend(helper.helper_edges.values().map(|edge| edge.target.clone()));
        }
    }
    reachable
}

fn parse_helper_codegen(
    root: &Map<String, Value>,
    helpers: &BTreeMap<DefinitionId, HelperRecord>,
) -> Result<BTreeMap<DefinitionId, crate::LinkedHelperDefinition>, String> {
    let mut records = BTreeMap::new();
    for (index, value) in array(root, "helper_codegen")?.iter().enumerate() {
        let label = format!("helper_codegen[{index}]");
        let entry = object(value, &label)?;
        require_exact_object_fields(
            entry,
            &[
                "body_ir",
                "body_ir_hash",
                "definition",
                "semantic_key_canonical",
                "signature",
                "source_ir_hash",
            ],
            &label,
        )?;
        let definition = parse_definition_field(entry, "definition", &label)?;
        let helper = helpers.get(&definition).ok_or_else(|| {
            format!(
                "{label} names a definition absent from `helpers`: `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            )
        })?;
        let semantic_key = linked_helper_key(entry, "semantic_key_canonical", &label)?;
        if semantic_key != helper.semantic_key {
            return Err(format!(
                "{label}.semantic_key_canonical disagrees with its exact helper definition"
            ));
        }
        let source_ir_hash = sha256_field(entry, "source_ir_hash", &label)?;
        if source_ir_hash != helper.client_ir_hash {
            return Err(format!(
                "{label}.source_ir_hash does not match helpers[{index}].client_ir_hash"
            ));
        }

        let signature_value = entry
            .get("signature")
            .ok_or_else(|| format!("{label} is missing `signature`"))?;
        let signature = object(signature_value, &format!("{label}.signature"))?;
        require_exact_object_fields(
            signature,
            &["params", "return_type"],
            &format!("{label}.signature"),
        )?;
        let signature_hash = legacy_json_hash(HELPER_SIGNATURE_JSON_HASH_DOMAIN, signature_value)?;
        if signature_hash != helper.signature_hash {
            return Err(format!(
                "{label}.signature does not match the helper descriptor signature hash"
            ));
        }
        let params_value = array(signature, "params")?;
        let mut params = Vec::with_capacity(params_value.len());
        let mut source_names = BTreeSet::new();
        for (param_index, value) in params_value.iter().enumerate() {
            let param_label = format!("{label}.signature.params[{param_index}]");
            let param = object(value, &param_label)?;
            require_exact_object_fields(param, &["name", "rust_type"], &param_label)?;
            let name = string(param, "name")?;
            let rust_type = string(param, "rust_type")?;
            if name.is_empty() || rust_type.is_empty() {
                return Err(format!(
                    "{param_label} must contain a non-empty name and Rust type"
                ));
            }
            if !source_names.insert(name) {
                return Err(format!(
                    "{label}.signature contains duplicate parameter name `{name}`"
                ));
            }
            let ty = final_helper_type(
                rust_type,
                crate::ClientHelperTypePosition::Parameter,
                &format!("{param_label}.rust_type"),
            )?;
            // Helper functions are private and positional. Stable generated
            // names avoid JS reserved-word/raw-identifier hazards without
            // changing the canonical Rust signature committed above.
            params.push(crate::ClientParam::new(
                format!("__rsjs_arg_{param_index}"),
                ty,
            ));
        }
        let return_type_name = string(signature, "return_type")?;
        let return_ty = final_helper_type(
            return_type_name,
            crate::ClientHelperTypePosition::Return,
            &format!("{label}.signature.return_type"),
        )?;

        let body_value = entry
            .get("body_ir")
            .ok_or_else(|| format!("{label} is missing `body_ir`"))?;
        let body_hash = legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, body_value)?;
        let stored_body_hash = sha256_field(entry, "body_ir_hash", &label)?;
        if body_hash != stored_body_hash {
            return Err(format!(
                "{label}.body_ir does not match its stored relocation-resolved body hash"
            ));
        }
        let body = parse_canonical_helper_body(body_value, &label)?;
        let calls = helper
            .helper_edges
            .iter()
            .map(|(&call_id, edge)| {
                Ok(crate::LinkedHelperCallBinding::new(
                    crate::abi::HelperCallEdgeId::new(call_id),
                    edge.target.clone(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let definition_value =
            crate::LinkedHelperDefinition::new(semantic_key, params, return_ty, body, calls);
        if records.insert(definition, definition_value).is_some() {
            return Err(format!(
                "final RSJS plan contains duplicate helper_codegen definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }

    for definition in helpers.keys() {
        if !records.contains_key(definition) {
            return Err(format!(
                "final RSJS plan has no helper_codegen record for definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }

    // Validate every helper body and edge table once, including helpers that
    // are not reachable from this particular component. Synthetic roots make
    // the total plan graph closed without changing deployable call IDs.
    let synthetic_roots = records
        .values()
        .enumerate()
        .map(|(index, helper)| {
            let edge = u32::try_from(index)
                .map_err(|_| "final helper graph exceeds the u32 ID space".to_owned())?;
            Ok(crate::LinkedHelperCallBinding::new(
                crate::abi::HelperCallEdgeId::new(edge),
                helper.key.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    crate::LinkedHelperProgram::new_for_internal_validation(
        synthetic_roots,
        records.values().cloned().collect(),
    )
    .map_err(|error| format!("invalid helper_codegen graph: {error}"))?;

    Ok(records)
}

fn final_helper_type(
    name: &str,
    position: crate::ClientHelperTypePosition,
    label: &str,
) -> Result<crate::ClientType, String> {
    crate::parse_client_helper_type(name, position)
        .map_err(|error| format!("{label} is invalid: {error}"))
}

fn parse_canonical_helper_body(value: &Value, label: &str) -> Result<crate::ClientBlock, String> {
    if value.get("kind").and_then(Value::as_str) == Some("block") {
        let body = crate::parse_client_block_json(value)
            .map_err(|error| format!("{label}.body_ir is invalid Client IR v3: {error}"))?;
        let encoded = crate::client_block_json(&body);
        let normalized: Value = serde_json::from_str(&encoded)
            .map_err(|error| format!("failed to normalize {label}.body_ir: {error}"))?;
        if normalized != *value {
            return Err(format!(
                "{label}.body_ir is not canonical Client IR v3 or contains unknown fields"
            ));
        }
        Ok(body)
    } else {
        let expression = crate::parse_client_expr_json(value)
            .map_err(|error| format!("{label}.body_ir is invalid Client IR v3: {error}"))?;
        let encoded = crate::client_expr_json(&expression);
        let normalized: Value = serde_json::from_str(&encoded)
            .map_err(|error| format!("failed to normalize {label}.body_ir: {error}"))?;
        if normalized != *value {
            return Err(format!(
                "{label}.body_ir is not canonical Client IR v3 or contains unknown fields"
            ));
        }
        Ok(crate::ClientBlock::from_stmts(vec![
            crate::ClientStmt::Return(Some(expression)),
        ]))
    }
}

fn validate_component_helper_graph(
    components: &BTreeMap<DefinitionId, ComponentRecord>,
    helpers: &BTreeMap<DefinitionId, HelperRecord>,
) -> Result<(), String> {
    let definitions_by_key = helpers
        .iter()
        .map(|(&definition, helper)| (helper.semantic_key.clone(), definition))
        .collect::<BTreeMap<_, _>>();
    for (definition, component) in components {
        for (call_id, edge) in &component.helper_edges {
            let Some(target) = helpers.get(&edge.target_definition) else {
                return Err(format!(
                    "component `{}`/`{}` helper call {call_id} targets a definition absent from `helpers`",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            };
            if target.semantic_key != edge.target {
                return Err(format!(
                    "component `{}`/`{}` helper call {call_id} target semantic identity disagrees with its exact target definition",
                    hex_id(&definition.cargo_unit_id),
                    hex_id(&definition.record_id)
                ));
            }
        }

        let mut actual = BTreeSet::new();
        for edge in component.helper_edges.values() {
            actual.extend(helper_reachable_keys(
                &edge.target,
                helpers,
                &definitions_by_key,
            ));
        }
        let stored = component
            .helper_closure
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual != stored {
            return Err(format!(
                "component `{}`/`{}` helper_closure is not the exact reachable helper set",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }
    Ok(())
}

fn select_component_helper_linkage(
    selected_definition: DefinitionId,
    components: &BTreeMap<DefinitionId, ComponentRecord>,
    helpers: &BTreeMap<DefinitionId, HelperRecord>,
    helper_codegen: &BTreeMap<DefinitionId, crate::LinkedHelperDefinition>,
) -> Result<FinalHelperLinkage, String> {
    let component = components
        .get(&selected_definition)
        .expect("selected component definition was validated above");
    let definitions_by_key = helpers
        .iter()
        .map(|(&definition, helper)| (helper.semantic_key.clone(), definition))
        .collect::<BTreeMap<_, _>>();

    let mut roots_by_source = BTreeMap::new();
    for edge in component.helper_edges.values() {
        if roots_by_source
            .insert(
                edge.source_edge,
                FinalRootHelperBinding {
                    target: edge.target.clone(),
                },
            )
            .is_some()
        {
            return Err(format!(
                "component `{}`/`{}` maps one helper source edge more than once",
                hex_id(&selected_definition.cargo_unit_id),
                hex_id(&selected_definition.record_id)
            ));
        }
    }

    let mut definitions = Vec::with_capacity(component.helper_closure.len());
    for key in &component.helper_closure {
        let definition = definitions_by_key.get(key).ok_or_else(|| {
            format!(
                "component `{}`/`{}` helper_closure contains an unknown canonical helper identity",
                hex_id(&selected_definition.cargo_unit_id),
                hex_id(&selected_definition.record_id)
            )
        })?;
        let codegen = helper_codegen.get(definition).ok_or_else(|| {
            format!(
                "component helper closure definition `{}`/`{}` has no helper_codegen record",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            )
        })?;
        definitions.push(codegen.clone());
    }

    Ok(FinalHelperLinkage {
        roots_by_source,
        definitions,
    })
}

fn parse_component_edge_lifecycle(
    value: &Value,
    edge_label: &str,
) -> Result<ComponentEdgeLifecycle, String> {
    let label = format!("{edge_label}.lifecycle");
    parse_component_lifecycle(value, &label)
}

fn parse_component_lifecycle(value: &Value, label: &str) -> Result<ComponentEdgeLifecycle, String> {
    let lifecycle = object(value, label)?;
    let kind = string(lifecycle, "kind")?;
    match kind {
        "static_single" => {
            require_exact_object_fields(lifecycle, &["kind"], label)?;
            Ok(ComponentEdgeLifecycle::StaticSingle)
        }
        "conditional_single" => {
            require_exact_object_fields(
                lifecycle,
                &[
                    "branch",
                    "control",
                    "kind",
                    "predicate_origin",
                    "predicate_value",
                ],
                label,
            )?;
            let control = u32_field(lifecycle, "control", label)?;
            let predicate_value = u32_field(lifecycle, "predicate_value", label)?;
            let predicate_origin = u32_field(lifecycle, "predicate_origin", label)?;
            let branch = match string(lifecycle, "branch")? {
                "then" => ComponentEdgeLifecycleBranch::Then,
                "else" => ComponentEdgeLifecycleBranch::Else,
                other => {
                    return Err(format!(
                        "{label}.branch must be `then` or `else`; found `{other}`"
                    ));
                }
            };
            Ok(ComponentEdgeLifecycle::ConditionalSingle {
                control,
                predicate_value,
                predicate_origin,
                branch,
            })
        }
        other => Err(format!(
            "{label}.kind must be `static_single` or `conditional_single`; found `{other}`"
        )),
    }
}

const fn component_lifecycle_branch_name(branch: ComponentEdgeLifecycleBranch) -> &'static str {
    match branch {
        ComponentEdgeLifecycleBranch::Then => "then",
        ComponentEdgeLifecycleBranch::Else => "else",
    }
}

const fn final_component_lifecycle(
    lifecycle: ComponentEdgeLifecycle,
) -> Option<FinalComponentLifecycle> {
    match lifecycle {
        ComponentEdgeLifecycle::StaticSingle => None,
        ComponentEdgeLifecycle::ConditionalSingle {
            control,
            predicate_value,
            predicate_origin,
            branch,
        } => Some(FinalComponentLifecycle {
            control,
            predicate_value,
            predicate_origin,
            branch: match branch {
                ComponentEdgeLifecycleBranch::Then => FinalComponentLifecycleBranch::Then,
                ComponentEdgeLifecycleBranch::Else => FinalComponentLifecycleBranch::Else,
            },
        }),
    }
}

fn parse_component_codegen(
    root: &Map<String, Value>,
    components: &BTreeMap<DefinitionId, ComponentRecord>,
) -> Result<BTreeMap<DefinitionId, FinalComponentCodegen>, String> {
    let mut records = BTreeMap::new();
    for (index, value) in array(root, "component_codegen")?.iter().enumerate() {
        let label = format!("component_codegen[{index}]");
        let entry = object(value, &label)?;
        let definition = parse_definition_field(entry, "definition", &label)?;
        if !components.contains_key(&definition) {
            return Err(format!(
                "{label} names a definition absent from `components`: `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id),
            ));
        }

        let asset_id = string(entry, "asset_id")?.to_owned();
        let fragment_key = string(entry, "fragment_key")?.to_owned();
        if !valid_asset_key(&asset_id) {
            return Err(format!(
                "{label}.asset_id must be a non-empty ASCII asset key"
            ));
        }
        if fragment_key != asset_id {
            return Err(format!(
                "{label}.fragment_key `{fragment_key}` does not match asset_id `{asset_id}`"
            ));
        }
        let fragment_root = entry
            .get("fragment_root")
            .ok_or_else(|| format!("{label} is missing `fragment_root`"))?;
        let fragment_root = object(fragment_root, &format!("{label}.fragment_root"))?;
        let fragment_root = match string(fragment_root, "kind")? {
            "exact_one_element" => FinalComponentFragmentRoot::ExactOneElement,
            "unsupported" => FinalComponentFragmentRoot::Unsupported,
            other => {
                return Err(format!(
                    "{label}.fragment_root.kind has unsupported value `{other}`"
                ))
            }
        };

        let inbound_fields = array(entry, "inbound_fields")?
            .iter()
            .enumerate()
            .map(|(field_index, value)| {
                let field_label = format!("{label}.inbound_fields[{field_index}]");
                let field = object(value, &field_label)?;
                let field_id = u32_field(field, "field_id", &field_label)?;
                let producer_value_id =
                    u32_field(field, "producer_value_id", &field_label)?;
                let expected = u32::try_from(field_index)
                    .map_err(|_| format!("{label}.inbound_fields exceeds the u32 ID space"))?;
                if field_id != expected || producer_value_id != expected {
                    return Err(format!(
                        "{field_label} must be dense at field/value ID {expected}; found field {field_id}, value {producer_value_id}"
                    ));
                }
                Ok(FinalCodegenInboundField {
                    field_id,
                    producer_value_id,
                    canonical_value_id: u32_field(
                        field,
                        "canonical_value_id",
                        &field_label,
                    )?,
                    crossing_hook_id: u32_field(
                        field,
                        "crossing_hook_id",
                        &field_label,
                    )?,
                    callback_slot_id: optional_u32_field(
                        field,
                        "callback_slot_id",
                        &field_label,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let inbound_targets = array(entry, "inbound_targets")?
            .iter()
            .enumerate()
            .map(|(target_index, value)| {
                let target_label = format!("{label}.inbound_targets[{target_index}]");
                let target = object(value, &target_label)?;
                Ok(FinalCodegenInboundTarget {
                    prop_site_id: u32_field(target, "prop_site_id", &target_label)?,
                    field_id: u32_field(target, "field_id", &target_label)?,
                    producer_signal_id: u32_field(target, "producer_signal_id", &target_label)?,
                    canonical_signal_value_id: u32_field(
                        target,
                        "canonical_signal_value_id",
                        &target_label,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        require_strict_order(
            &format!("{label}.inbound_targets"),
            inbound_targets.iter().map(|target| target.prop_site_id),
        )?;

        let signals = parse_codegen_values(
            entry,
            "signals",
            "producer_signal_id",
            "canonical_value_id",
            &label,
        )?;
        let expressions = parse_codegen_values(
            entry,
            "expressions",
            "producer_expression_id",
            "canonical_value_id",
            &label,
        )?;
        let payload_handlers = parse_codegen_values(
            entry,
            "payload_handlers",
            "prelude_local_index",
            "canonical_value_id",
            &label,
        )?;
        let callback_slots = parse_codegen_values(
            entry,
            "callback_slots",
            "producer_callback_slot_id",
            "canonical_value_id",
            &label,
        )?;
        let inbound_node_refs = array(entry, "inbound_node_refs")?
            .iter()
            .enumerate()
            .map(|(node_ref_index, value)| {
                let node_ref_label = format!("{label}.inbound_node_refs[{node_ref_index}]");
                let node_ref = object(value, &node_ref_label)?;
                Ok(FinalCodegenInboundNodeRef {
                    field_id: u32_field(node_ref, "field_id", &node_ref_label)?,
                    producer_node_ref_id: u32_field(
                        node_ref,
                        "producer_node_ref_id",
                        &node_ref_label,
                    )?,
                    bind_target: optional_node_ref_bind_target(
                        node_ref,
                        "bind_target",
                        &node_ref_label,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        require_strict_order(
            &format!("{label}.inbound_node_refs"),
            inbound_node_refs.iter().map(|node_ref| node_ref.field_id),
        )?;
        let direct_node_ref_aliases = array(entry, "direct_node_ref_aliases")?
            .iter()
            .enumerate()
            .map(|(alias_index, value)| {
                let alias_label = format!("{label}.direct_node_ref_aliases[{alias_index}]");
                let alias = object(value, &alias_label)?;
                let field_name = string(alias, "field_name")?.to_owned();
                if field_name.is_empty() {
                    return Err(format!("{alias_label}.field_name must not be empty"));
                }
                Ok(FinalCodegenDirectNodeRefAlias {
                    edge_id: u32_field(alias, "edge_id", &alias_label)?,
                    field_name,
                    producer_node_ref_id: u32_field(alias, "producer_node_ref_id", &alias_label)?,
                    canonical_value_id: u32_field(alias, "canonical_value_id", &alias_label)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if direct_node_ref_aliases.windows(2).any(|pair| {
            (pair[0].edge_id, pair[0].field_name.as_str())
                >= (pair[1].edge_id, pair[1].field_name.as_str())
        }) {
            return Err(format!(
                "{label}.direct_node_ref_aliases must be unique and strictly ordered by edge/field"
            ));
        }

        let record = FinalComponentCodegen {
            asset_id,
            fragment_key,
            fragment_root,
            uses_queries: bool_field(entry, "uses_queries", &label)?,
            uses_mutations: bool_field(entry, "uses_mutations", &label)?,
            inbound_fields,
            inbound_targets,
            signals,
            expressions,
            payload_handlers,
            callback_slots,
            inbound_node_refs,
            direct_node_ref_aliases,
        };
        if records.insert(definition, record).is_some() {
            return Err(format!(
                "final RSJS plan contains duplicate component_codegen definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id),
            ));
        }
    }

    if records.len() != components.len() {
        let missing = components
            .keys()
            .find(|definition| !records.contains_key(definition))
            .expect("unequal maps with no extras must contain a missing component");
        return Err(format!(
            "final RSJS plan has no component_codegen record for definition `{}`/`{}`",
            hex_id(&missing.cargo_unit_id),
            hex_id(&missing.record_id),
        ));
    }
    Ok(records)
}

fn parse_codegen_values(
    parent: &Map<String, Value>,
    collection: &str,
    producer_field: &str,
    canonical_field: &str,
    parent_label: &str,
) -> Result<Vec<FinalCodegenValue>, String> {
    let values = array(parent, collection)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let label = format!("{parent_label}.{collection}[{index}]");
            let value = object(value, &label)?;
            Ok(FinalCodegenValue {
                producer_id: u32_field(value, producer_field, &label)?,
                canonical_value_id: u32_field(value, canonical_field, &label)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    require_strict_order(
        &format!("{parent_label}.{collection}"),
        values.iter().map(|value| value.producer_id),
    )?;
    Ok(values)
}

fn require_strict_order(label: &str, values: impl IntoIterator<Item = u32>) -> Result<(), String> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|previous| previous >= value) {
            return Err(format!(
                "{label} must be strictly ordered by producer-local ID"
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

fn valid_asset_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn parse_component_sites(
    root: &Map<String, Value>,
    components: &BTreeMap<DefinitionId, ComponentRecord>,
) -> Result<Vec<ComponentSiteRecord>, String> {
    let values = array(root, "component_sites")?;
    let mut sites = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let label = format!("component_sites[{index}]");
        let entry = object(value, &label)?;
        let candidate_id = u32_field(entry, "candidate_id", &label)?;
        if candidate_id != index as u32 {
            return Err(format!(
                "component site candidates must be unique and dense; expected candidate {index}, found {candidate_id}"
            ));
        }
        let definition = parse_definition_field(entry, "definition", &label)?;
        let Some(component) = components.get(&definition) else {
            return Err(format!(
                "{label} names component definition `{}`/`{}` absent from `components`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        };
        let component_semantic_key =
            canonical_hex_string(entry, "component_semantic_key_canonical", &label)?;
        if component_semantic_key != component.semantic_key {
            return Err(format!(
                "{label} semantic key disagrees with its exact component definition"
            ));
        }
        let root_semantic_key = canonical_hex_string(entry, "root_semantic_key_canonical", &label)?;
        let parent_candidate_id = optional_u32_field(entry, "parent_candidate_id", &label)?;
        let incoming_edge_id = optional_u32_field(entry, "incoming_edge_id", &label)?;
        let path = array(entry, "structural_path")?;
        let mut structural_path = Vec::with_capacity(path.len());
        for (path_index, value) in path.iter().enumerate() {
            structural_path.push(value_u32(
                value,
                &format!("{label}.structural_path[{path_index}]"),
            )?);
        }
        let incoming_lifecycle_value = entry
            .get("incoming_lifecycle")
            .ok_or_else(|| format!("{label} is missing `incoming_lifecycle`"))?;
        let incoming_lifecycle = if incoming_lifecycle_value.is_null() {
            None
        } else {
            Some(parse_component_lifecycle(
                incoming_lifecycle_value,
                &format!("{label}.incoming_lifecycle"),
            )?)
        };
        sites.push(ComponentSiteRecord {
            candidate_id,
            root_semantic_key,
            component_semantic_key,
            definition,
            parent_candidate_id,
            incoming_edge_id,
            structural_path,
            incoming_lifecycle,
            reactive_lifecycle_root_candidate_id: optional_u32_field(
                entry,
                "reactive_lifecycle_root_candidate_id",
                &label,
            )?,
        });
    }
    Ok(sites)
}

fn parse_standalone_roots(
    root: &Map<String, Value>,
    components: &BTreeMap<DefinitionId, ComponentRecord>,
    sites: &[ComponentSiteRecord],
) -> Result<BTreeMap<DefinitionId, StandaloneRootDispositionRecord>, String> {
    let values = array(root, "standalone_roots")?;
    if values.len() != components.len() {
        return Err(format!(
            "`standalone_roots` must contain exactly one decision per component definition; expected {}, found {}",
            components.len(),
            values.len()
        ));
    }
    let semantic_keys = components
        .values()
        .map(|component| component.semantic_key.as_str())
        .collect::<BTreeSet<_>>();
    let root_sites = sites
        .iter()
        .filter(|site| site.parent_candidate_id.is_none())
        .map(|site| (site.definition, site.candidate_id))
        .collect::<BTreeMap<_, _>>();
    let mut decisions = BTreeMap::new();
    for (index, value) in values.iter().enumerate() {
        let label = format!("standalone_roots[{index}]");
        let entry = object(value, &label)?;
        let definition = parse_definition_field(entry, "definition", &label)?;
        let Some(component) = components.get(&definition) else {
            return Err(format!(
                "{label} names component definition `{}`/`{}` absent from `components`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        };
        let semantic_key = canonical_hex_string(entry, "component_semantic_key_canonical", &label)?;
        if semantic_key != component.semantic_key {
            return Err(format!(
                "{label} semantic key disagrees with its exact component definition"
            ));
        }
        let disposition_label = format!("{label}.disposition");
        let disposition = object(
            entry
                .get("disposition")
                .ok_or_else(|| format!("{label} is missing `disposition`"))?,
            &disposition_label,
        )?;
        let parsed = match string(disposition, "kind")? {
            "structural_incoming_only" => {
                if component.root_eligible || root_sites.contains_key(&definition) {
                    return Err(format!(
                        "{disposition_label} marks a root-eligible definition or concrete root site as structural-incoming-only"
                    ));
                }
                StandaloneRootDispositionRecord::StructuralIncomingOnly
            }
            "admitted" => {
                let candidate_id = u32_field(disposition, "candidate_id", &disposition_label)?;
                if !component.root_eligible
                    || root_sites.get(&definition).copied() != Some(candidate_id)
                {
                    return Err(format!(
                        "{disposition_label} admitted candidate {candidate_id} does not match the definition's exact standalone root site"
                    ));
                }
                StandaloneRootDispositionRecord::Admitted { candidate_id }
            }
            "context_required" => {
                if !component.root_eligible || root_sites.contains_key(&definition) {
                    return Err(format!(
                        "{disposition_label} may only classify a structurally root-eligible definition with no concrete root site"
                    ));
                }
                let missing = array(disposition, "missing")?;
                if missing.is_empty() {
                    return Err(format!(
                        "{disposition_label}.missing must name at least one lexical context requirement"
                    ));
                }
                for (missing_index, value) in missing.iter().enumerate() {
                    let missing_label = format!("{disposition_label}.missing[{missing_index}]");
                    let missing = object(value, &missing_label)?;
                    let consumer = canonical_hex_string(
                        missing,
                        "consumer_semantic_key_canonical",
                        &missing_label,
                    )?;
                    if !semantic_keys.contains(consumer.as_str()) {
                        return Err(format!(
                            "{missing_label} names a consumer semantic key absent from `components`"
                        ));
                    }
                    let path = array(missing, "structural_path")?;
                    for (path_index, edge) in path.iter().enumerate() {
                        value_u32(
                            edge,
                            &format!("{missing_label}.structural_path[{path_index}]"),
                        )?;
                    }
                    match string(missing, "kind")? {
                        "shared_state" => {
                            require_exact_object_fields(
                                missing,
                                &[
                                    "consumer_semantic_key_canonical",
                                    "kind",
                                    "reference_id",
                                    "schema_abi",
                                    "structural_path",
                                    "type_key",
                                ],
                                &missing_label,
                            )?;
                            u32_field(missing, "reference_id", &missing_label)?;
                            shared_state_type_key(missing, "type_key", &missing_label)?;
                            sha256_field(missing, "schema_abi", &missing_label)?;
                        }
                        "node_ref" => {
                            require_exact_object_fields(
                                missing,
                                &[
                                    "bind_target",
                                    "consumer_semantic_key_canonical",
                                    "crossing_hook_id",
                                    "field_id",
                                    "kind",
                                    "node_ref_id",
                                    "structural_path",
                                ],
                                &missing_label,
                            )?;
                            u32_field(missing, "field_id", &missing_label)?;
                            u32_field(missing, "crossing_hook_id", &missing_label)?;
                            u32_field(missing, "node_ref_id", &missing_label)?;
                            optional_node_ref_bind_target(missing, "bind_target", &missing_label)?;
                        }
                        "callback" => {
                            require_exact_object_fields(
                                missing,
                                &[
                                    "callback_shape",
                                    "consumer_semantic_key_canonical",
                                    "crossing_hook_id",
                                    "field_id",
                                    "kind",
                                    "structural_path",
                                ],
                                &missing_label,
                            )?;
                            u32_field(missing, "field_id", &missing_label)?;
                            u32_field(missing, "crossing_hook_id", &missing_label)?;
                            match string(missing, "callback_shape")? {
                                "required" | "optional" => {}
                                other => {
                                    return Err(format!(
                                        "{missing_label}.callback_shape has unsupported value `{other}`"
                                    ));
                                }
                            }
                        }
                        other => {
                            return Err(format!(
                                "{missing_label}.kind has unsupported context requirement `{other}`"
                            ));
                        }
                    }
                }
                StandaloneRootDispositionRecord::ContextRequired
            }
            other => {
                return Err(format!(
                    "{disposition_label}.kind has unsupported value `{other}`"
                ));
            }
        };
        if decisions.insert(definition, parsed).is_some() {
            return Err(format!(
                "`standalone_roots` contains more than one decision for component definition `{}`/`{}`",
                hex_id(&definition.cargo_unit_id),
                hex_id(&definition.record_id)
            ));
        }
    }
    if decisions.len() != components.len() {
        return Err("`standalone_roots` does not cover every component definition".into());
    }
    Ok(decisions)
}

fn validate_site_structure(
    components: &BTreeMap<DefinitionId, ComponentRecord>,
    sites: &[ComponentSiteRecord],
) -> Result<BTreeMap<(u32, u32), u32>, String> {
    let mut roots_by_definition = BTreeMap::new();
    let mut roots_by_semantic_key = BTreeMap::new();
    let mut child_sites = BTreeMap::new();

    for site in sites {
        match (site.parent_candidate_id, site.incoming_edge_id) {
            (None, None) => {
                let component = &components[&site.definition];
                if !component.root_eligible {
                    return Err(format!(
                        "root-ineligible component definition `{}`/`{}` has a standalone root site",
                        hex_id(&site.definition.cargo_unit_id),
                        hex_id(&site.definition.record_id)
                    ));
                }
                if !site.structural_path.is_empty()
                    || site.root_semantic_key != site.component_semantic_key
                    || site.incoming_lifecycle.is_some()
                    || site.reactive_lifecycle_root_candidate_id.is_some()
                {
                    return Err(format!(
                        "root component site {} has invalid structural or lifecycle metadata",
                        site.candidate_id
                    ));
                }
                if roots_by_definition
                    .insert(site.definition, site.candidate_id)
                    .is_some()
                {
                    return Err(format!(
                        "component definition `{}`/`{}` has multiple standalone roots",
                        hex_id(&site.definition.cargo_unit_id),
                        hex_id(&site.definition.record_id)
                    ));
                }
                if roots_by_semantic_key
                    .insert(site.root_semantic_key.clone(), site.candidate_id)
                    .is_some()
                {
                    return Err(format!(
                        "semantic component root `{}` appears more than once",
                        site.root_semantic_key
                    ));
                }
            }
            (Some(parent_id), Some(edge_id)) => {
                let Some(parent) = sites.get(parent_id as usize) else {
                    return Err(format!(
                        "component site {} names absent parent candidate {parent_id}",
                        site.candidate_id
                    ));
                };
                if parent_id >= site.candidate_id {
                    return Err(format!(
                        "component site {} parent {parent_id} must precede its child",
                        site.candidate_id
                    ));
                }
                let parent_component = &components[&parent.definition];
                let Some(edge) = parent_component.edges.get(&edge_id) else {
                    return Err(format!(
                        "component site {parent_id} has no local component edge {edge_id}"
                    ));
                };
                let mut expected_path = parent.structural_path.clone();
                expected_path.push(edge_id);
                if parent.root_semantic_key != site.root_semantic_key
                    || expected_path != site.structural_path
                    || edge.target_definition != site.definition
                    || edge.target_semantic_key != site.component_semantic_key
                {
                    return Err(format!(
                        "component site {} disagrees with parent {parent_id} edge {edge_id}",
                        site.candidate_id
                    ));
                }
                if site.incoming_lifecycle != Some(edge.lifecycle) {
                    return Err(format!(
                        "component site {} incoming lifecycle disagrees with parent {parent_id} edge {edge_id}",
                        site.candidate_id
                    ));
                }
                let expected_lifecycle_root = match edge.lifecycle {
                    ComponentEdgeLifecycle::StaticSingle => {
                        parent.reactive_lifecycle_root_candidate_id
                    }
                    ComponentEdgeLifecycle::ConditionalSingle {
                        control, branch, ..
                    } => {
                        if let Some(reactive_lifecycle_root) =
                            parent.reactive_lifecycle_root_candidate_id
                        {
                            return Err(format!(
                                "component site {} introduces nested conditional lifecycle \
                                 control {control} branch `{}` beneath lifecycle root \
                                 {reactive_lifecycle_root}",
                                site.candidate_id,
                                component_lifecycle_branch_name(branch),
                            ));
                        }
                        Some(site.candidate_id)
                    }
                };
                if site.reactive_lifecycle_root_candidate_id != expected_lifecycle_root {
                    return Err(format!(
                        "component site {} has lifecycle root {:?}, but parent {parent_id} edge \
                         {edge_id} requires {:?}",
                        site.candidate_id,
                        site.reactive_lifecycle_root_candidate_id,
                        expected_lifecycle_root,
                    ));
                }
                if child_sites
                    .insert((parent_id, edge_id), site.candidate_id)
                    .is_some()
                {
                    return Err(format!(
                        "parent candidate {parent_id} edge {edge_id} maps to more than one child site"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "component site {} must provide both parent and incoming edge, or neither",
                    site.candidate_id
                ));
            }
        }
    }

    for site in sites {
        let component = &components[&site.definition];
        for edge_id in component.edges.keys() {
            if !child_sites.contains_key(&(site.candidate_id, *edge_id)) {
                return Err(format!(
                    "component site {} is missing child mapping for local edge {edge_id}",
                    site.candidate_id
                ));
            }
        }
    }
    Ok(child_sites)
}

fn validate_lifecycle_owner_groups(
    sites: &[ComponentSiteRecord],
    candidate_to_owner: &BTreeMap<u32, u32>,
) -> Result<(), String> {
    for site in sites {
        let Some(lifecycle_root) = site.reactive_lifecycle_root_candidate_id else {
            continue;
        };
        let root = sites.get(lifecycle_root as usize).ok_or_else(|| {
            format!(
                "component site {} names absent lifecycle root candidate {lifecycle_root}",
                site.candidate_id
            )
        })?;
        if root.candidate_id != lifecycle_root
            || root.reactive_lifecycle_root_candidate_id != Some(lifecycle_root)
            || !matches!(
                root.incoming_lifecycle,
                Some(ComponentEdgeLifecycle::ConditionalSingle { .. })
            )
        {
            return Err(format!(
                "component site {} names candidate {lifecycle_root}, but that candidate is not \
                 a conditional lifecycle root",
                site.candidate_id
            ));
        }
        let root_parent = root.parent_candidate_id.ok_or_else(|| {
            format!(
                "conditional lifecycle root candidate {lifecycle_root} has no structural parent"
            )
        })?;
        if sites[root_parent as usize]
            .reactive_lifecycle_root_candidate_id
            .is_some()
        {
            return Err(format!(
                "conditional lifecycle root candidate {lifecycle_root} has parent {root_parent} \
                 inside another lifecycle group"
            ));
        }

        let site_owner = candidate_to_owner
            .get(&site.candidate_id)
            .copied()
            .ok_or_else(|| {
                format!(
                    "component site {} belongs to lifecycle root {lifecycle_root}, but has no \
                     browser owner",
                    site.candidate_id
                )
            })?;
        let root_owner = candidate_to_owner
            .get(&lifecycle_root)
            .copied()
            .ok_or_else(|| {
                format!(
                    "conditional lifecycle root candidate {lifecycle_root} has no browser owner"
                )
            })?;
        let parent_owner = candidate_to_owner
            .get(&root_parent)
            .copied()
            .ok_or_else(|| {
                format!(
                    "conditional lifecycle root candidate {lifecycle_root} has parent \
                     {root_parent} without a browser owner"
                )
            })?;
        if site_owner != root_owner || parent_owner != root_owner {
            return Err(format!(
                "conditional lifecycle group rooted at candidate {lifecycle_root} crosses owner \
                 boundaries: parent owner {parent_owner}, root owner {root_owner}, candidate {} \
                 owner {site_owner}",
                site.candidate_id
            ));
        }
    }
    Ok(())
}

fn validate_owner_mapping(
    root: &Map<String, Value>,
    sites: &[ComponentSiteRecord],
) -> Result<
    (
        BTreeMap<u32, u32>,
        BTreeMap<u32, u32>,
        BTreeMap<u32, FinalOwnerActivationStrategy>,
    ),
    String,
> {
    let mut membership = BTreeMap::new();
    let mut owner_roots = BTreeMap::new();
    let mut owner_strategies = BTreeMap::new();
    for (index, value) in array(root, "owners")?.iter().enumerate() {
        let label = format!("owners[{index}]");
        let owner = object(value, &label)?;
        let owner_id = u32_field(owner, "owner_id", &label)?;
        if owner_id != index as u32 {
            return Err(format!(
                "owner IDs must be unique and dense; expected owner {index}, found {owner_id}"
            ));
        }
        let candidates = array(owner, "candidates")?;
        if candidates.is_empty() {
            return Err(format!("{label} must contain at least one candidate"));
        }
        let mut previous = None;
        let mut root_semantic_key = None;
        for (candidate_index, value) in candidates.iter().enumerate() {
            let candidate = value_u32(value, &format!("{label}.candidates[{candidate_index}]"))?;
            let Some(site) = sites.get(candidate as usize) else {
                return Err(format!(
                    "{label} names absent component candidate {candidate}"
                ));
            };
            if previous.is_some_and(|previous| previous >= candidate) {
                return Err(format!(
                    "{label}.candidates must be unique and strictly increasing"
                ));
            }
            previous = Some(candidate);
            match &root_semantic_key {
                None => root_semantic_key = Some(&site.root_semantic_key),
                Some(expected) if *expected != &site.root_semantic_key => {
                    return Err(format!(
                        "{label} cannot contain candidates from different standalone roots"
                    ));
                }
                Some(_) => {}
            }
            if membership.insert(candidate, owner_id).is_some() {
                return Err(format!(
                    "component candidate {candidate} belongs to more than one owner"
                ));
            }
        }
        owner_roots.insert(
            owner_id,
            value_u32(&candidates[0], &format!("{label}.candidates[0]"))?,
        );
        let strategy = owner
            .get("strategy")
            .ok_or_else(|| format!("{label} is missing `strategy`"))?;
        owner_strategies.insert(owner_id, parse_owner_strategy(strategy, &label)?);
    }

    let mut reverse = BTreeMap::new();
    for (index, value) in array(root, "candidate_to_owner")?.iter().enumerate() {
        let label = format!("candidate_to_owner[{index}]");
        let entry = object(value, &label)?;
        let candidate = u32_field(entry, "candidate_id", &label)?;
        let owner = u32_field(entry, "owner_id", &label)?;
        if candidate as usize >= sites.len() || !owner_roots.contains_key(&owner) {
            return Err(format!("{label} names an absent candidate or owner"));
        }
        if reverse.insert(candidate, owner).is_some() {
            return Err(format!(
                "`candidate_to_owner` contains duplicate candidate {candidate}"
            ));
        }
    }
    if reverse != membership {
        return Err(
            "`candidate_to_owner` does not exactly match the owner candidate membership".into(),
        );
    }
    Ok((reverse, owner_roots, owner_strategies))
}

fn parse_owner_strategy(
    value: &Value,
    owner_label: &str,
) -> Result<FinalOwnerActivationStrategy, String> {
    let label = format!("{owner_label}.strategy");
    let strategy = object(value, &label)?;
    let kind = string(strategy, "kind")?;
    match kind {
        "eager" => Ok(FinalOwnerActivationStrategy::Eager),
        "visible" => Ok(FinalOwnerActivationStrategy::Visible),
        "idle" => Ok(FinalOwnerActivationStrategy::Idle),
        "interaction" => {
            let event = strategy
                .get("event")
                .ok_or_else(|| format!("{label} is missing `event`"))?;
            let event = if event.is_null() {
                None
            } else {
                let event = event
                    .as_str()
                    .ok_or_else(|| format!("{label}.event must be a string or null"))?;
                if event.trim().is_empty() {
                    return Err(format!("{label}.event cannot be empty"));
                }
                Some(event.to_owned())
            };
            Ok(FinalOwnerActivationStrategy::Interaction { event })
        }
        "media" => {
            let query = string(strategy, "query")?;
            if query.trim().is_empty() {
                return Err(format!("{label}.query cannot be empty"));
            }
            Ok(FinalOwnerActivationStrategy::Media {
                query: query.to_owned(),
            })
        }
        other => Err(format!(
            "{label}.kind uses unsupported owner activation strategy `{other}`"
        )),
    }
}

fn parse_component_inputs(
    root: &Map<String, Value>,
    sites: &[ComponentSiteRecord],
    child_sites: &BTreeMap<(u32, u32), u32>,
) -> Result<
    (
        BTreeMap<u32, Vec<FinalLivePropUse>>,
        BTreeMap<u32, Vec<FinalLiveCallback>>,
    ),
    String,
> {
    let inputs = root
        .get("component_inputs")
        .ok_or("final RSJS plan is missing `component_inputs`")?;
    let inputs = object(inputs, "component_inputs")?;
    let mut live_props_by_child = BTreeMap::<u32, Vec<FinalLivePropUse>>::new();
    let mut seen_props = BTreeSet::new();
    for (index, value) in array(inputs, "live_props")?.iter().enumerate() {
        let label = format!("component_inputs.live_props[{index}]");
        let input = object(value, &label)?;
        let parent_candidate_id = u32_field(input, "parent_candidate_id", &label)?;
        let edge_id = u32_field(input, "edge_id", &label)?;
        let child_candidate_id = u32_field(input, "child_candidate_id", &label)?;
        validate_component_input_edge(
            &label,
            sites,
            child_sites,
            parent_candidate_id,
            edge_id,
            child_candidate_id,
        )?;
        let prop_site_id = u32_field(input, "prop_site_id", &label)?;
        let temporal = input
            .get("temporal")
            .ok_or_else(|| format!("{label} is missing `temporal`"))?;
        let temporal = object(temporal, &format!("{label}.temporal"))?;
        let temporal = match string(temporal, "kind")? {
            "live_read" => FinalTemporalPropMode::LiveRead,
            "seed_once" => {
                let seed_site_id = u32_field(temporal, "seed_site_id", &label)?;
                if seed_site_id != prop_site_id {
                    return Err(format!(
                        "{label} SeedOnce names prop site {seed_site_id}, but the use is site {prop_site_id}"
                    ));
                }
                FinalTemporalPropMode::SeedOnce { seed_site_id }
            }
            "effect_dependency" => FinalTemporalPropMode::EffectDependency,
            "query_key_dependency" => FinalTemporalPropMode::QueryKeyDependency,
            other => {
                return Err(format!(
                    "{label} has unsupported temporal mode `{other}`; forbidden modes must fail during semantic linking"
                ))
            }
        };
        let record = FinalLivePropUse {
            parent_candidate_id,
            edge_id,
            child_candidate_id,
            field_id: u32_field(input, "field_id", &label)?,
            crossing_hook_id: u32_field(input, "crossing_hook_id", &label)?,
            parent_value_id: u32_field(input, "parent_value_id", &label)?,
            child_value_id: u32_field(input, "child_value_id", &label)?,
            prop_site_id,
            temporal,
        };
        if !seen_props.insert(record) {
            return Err("`component_inputs.live_props` contains a duplicate record".into());
        }
        live_props_by_child
            .entry(child_candidate_id)
            .or_default()
            .push(record);
    }

    let mut live_callbacks_by_child = BTreeMap::<u32, Vec<FinalLiveCallback>>::new();
    let mut seen_callbacks = BTreeSet::new();
    for (index, value) in array(inputs, "live_callbacks")?.iter().enumerate() {
        let label = format!("component_inputs.live_callbacks[{index}]");
        let input = object(value, &label)?;
        let parent_candidate_id = u32_field(input, "parent_candidate_id", &label)?;
        let edge_id = u32_field(input, "edge_id", &label)?;
        let child_candidate_id = u32_field(input, "child_candidate_id", &label)?;
        validate_component_input_edge(
            &label,
            sites,
            child_sites,
            parent_candidate_id,
            edge_id,
            child_candidate_id,
        )?;
        let dispatch = string(input, "dispatch")?;
        if dispatch != "resolve_current" {
            return Err(format!(
                "{label} has unsupported callback dispatch `{dispatch}`; compiler-owned callbacks must resolve the current edge at invocation time"
            ));
        }
        let shape = match string(input, "callback_shape")? {
            "required" => FinalCallbackShape::Required,
            "optional" => FinalCallbackShape::Optional,
            other => {
                return Err(format!(
                    "{label} has unsupported callback shape `{other}`; expected `required` or `optional`"
                ))
            }
        };
        let record = FinalLiveCallback {
            parent_candidate_id,
            edge_id,
            child_candidate_id,
            field_id: u32_field(input, "field_id", &label)?,
            crossing_hook_id: u32_field(input, "crossing_hook_id", &label)?,
            parent_value_id: u32_field(input, "parent_value_id", &label)?,
            child_value_id: u32_field(input, "child_value_id", &label)?,
            shape,
        };
        if !seen_callbacks.insert(record) {
            return Err("`component_inputs.live_callbacks` contains a duplicate record".into());
        }
        live_callbacks_by_child
            .entry(child_candidate_id)
            .or_default()
            .push(record);
    }

    Ok((live_props_by_child, live_callbacks_by_child))
}

fn parse_shared_state_provider_topology(
    root: &Map<String, Value>,
    sites: &[ComponentSiteRecord],
) -> Result<
    (
        BTreeMap<u32, Vec<FinalSharedStateProviderOccurrence>>,
        BTreeMap<u32, Vec<FinalSharedStateProviderRequirement>>,
    ),
    String,
> {
    let mut provider_index = BTreeMap::<(u32, u32), FinalSharedStateProviderOccurrence>::new();
    let mut previous_provider = None;
    for (index, value) in array(root, "shared_state_providers")?.iter().enumerate() {
        let label = format!("shared_state_providers[{index}]");
        let entry = object(value, &label)?;
        let host_candidate_id = u32_field(entry, "host_candidate_id", &label)?;
        let provider_site_id = u32_field(entry, "provider_site_id", &label)?;
        let child_candidate_id = u32_field(entry, "child_candidate_id", &label)?;
        let key = (host_candidate_id, provider_site_id);
        if previous_provider.is_some_and(|previous| previous >= key) {
            return Err(
                "`shared_state_providers` must be unique and strictly ordered by host/provider site"
                    .into(),
            );
        }
        previous_provider = Some(key);
        let Some(host) = sites.get(host_candidate_id as usize) else {
            return Err(format!(
                "{label} names absent host candidate {host_candidate_id}"
            ));
        };
        let Some(child) = sites.get(child_candidate_id as usize) else {
            return Err(format!(
                "{label} names absent provider child candidate {child_candidate_id}"
            ));
        };
        if child.parent_candidate_id != Some(host_candidate_id)
            || !matches!(
                child.incoming_lifecycle,
                Some(ComponentEdgeLifecycle::StaticSingle)
            )
            || host.reactive_lifecycle_root_candidate_id.is_some()
        {
            return Err(format!(
                "{label} must bind one static direct child of a host outside reactive lifecycle"
            ));
        }
        let type_key = shared_state_type_key(entry, "type_key", &label)?;
        let schema_abi = sha256_field(entry, "schema_abi", &label)?;
        let provider = FinalSharedStateProviderOccurrence {
            host_candidate_id,
            provider_site_id,
            child_candidate_id,
            type_key,
            schema_abi,
        };
        if provider_index.insert(key, provider).is_some() {
            return Err(format!(
                "{label} duplicates provider occurrence {host_candidate_id}:{provider_site_id}"
            ));
        }
    }
    let provider_children = provider_index
        .values()
        .map(|provider| provider.child_candidate_id)
        .collect::<BTreeSet<_>>();
    if provider_children.len() != provider_index.len() {
        return Err(
            "`shared_state_providers` assigns one child occurrence to multiple providers".into(),
        );
    }

    let mut requirements_by_consumer =
        BTreeMap::<u32, Vec<FinalSharedStateProviderRequirement>>::new();
    let mut seen_requirements = BTreeSet::new();
    let mut previous_requirement = None;
    for (index, value) in array(root, "shared_state_requirements")?.iter().enumerate() {
        let label = format!("shared_state_requirements[{index}]");
        let entry = object(value, &label)?;
        let consumer_candidate_id = u32_field(entry, "consumer_candidate_id", &label)?;
        let reference_id = u32_field(entry, "reference_id", &label)?;
        let provider_host_candidate_id = u32_field(entry, "provider_host_candidate_id", &label)?;
        let provider_site_id = u32_field(entry, "provider_site_id", &label)?;
        let order_key = (consumer_candidate_id, reference_id);
        if previous_requirement.is_some_and(|previous| previous >= order_key) {
            return Err(
                "`shared_state_requirements` must be unique and strictly ordered by consumer/reference"
                    .into(),
            );
        }
        previous_requirement = Some(order_key);
        if sites.get(consumer_candidate_id as usize).is_none() {
            return Err(format!(
                "{label} names absent consumer candidate {consumer_candidate_id}"
            ));
        }
        let provider_key = (provider_host_candidate_id, provider_site_id);
        let Some(provider) = provider_index.get(&provider_key) else {
            return Err(format!(
                "{label} names absent provider occurrence {provider_host_candidate_id}:{provider_site_id}"
            ));
        };
        if !component_site_is_descendant_of(
            sites,
            consumer_candidate_id,
            provider.child_candidate_id,
        ) {
            return Err(format!(
                "{label} consumer {consumer_candidate_id} is outside provider occurrence {provider_host_candidate_id}:{provider_site_id}"
            ));
        }
        let type_key = shared_state_type_key(entry, "type_key", &label)?;
        let schema_abi = sha256_field(entry, "schema_abi", &label)?;
        if type_key != provider.type_key || schema_abi != provider.schema_abi {
            return Err(format!(
                "{label} schema identity disagrees with provider occurrence {provider_host_candidate_id}:{provider_site_id}"
            ));
        }
        if !seen_requirements.insert(order_key) {
            return Err(format!(
                "{label} duplicates consumer {consumer_candidate_id} reference {reference_id}"
            ));
        }
        requirements_by_consumer
            .entry(consumer_candidate_id)
            .or_default()
            .push(FinalSharedStateProviderRequirement {
                consumer_candidate_id,
                reference_id,
                provider_host_candidate_id,
                provider_site_id,
                type_key,
                schema_abi,
            });
    }

    let mut providers_by_host = BTreeMap::<u32, Vec<FinalSharedStateProviderOccurrence>>::new();
    for provider in provider_index.into_values() {
        providers_by_host
            .entry(provider.host_candidate_id)
            .or_default()
            .push(provider);
    }
    Ok((providers_by_host, requirements_by_consumer))
}

fn parse_node_ref_aliases(
    root: &Map<String, Value>,
    sites: &[ComponentSiteRecord],
    child_sites: &BTreeMap<(u32, u32), u32>,
    candidate_to_owner: &BTreeMap<u32, u32>,
    component_codegen: &BTreeMap<DefinitionId, FinalComponentCodegen>,
) -> Result<Vec<FinalNodeRefHandleAlias>, String> {
    let mut aliases = Vec::new();
    let mut previous_key = None;
    for (index, value) in array(root, "node_ref_aliases")?.iter().enumerate() {
        let label = format!("node_ref_aliases[{index}]");
        let entry = object(value, &label)?;
        let alias = FinalNodeRefHandleAlias {
            parent_candidate_id: u32_field(entry, "parent_candidate_id", &label)?,
            edge_id: u32_field(entry, "edge_id", &label)?,
            parent_node_ref_id: u32_field(entry, "parent_node_ref_id", &label)?,
            parent_value_id: u32_field(entry, "parent_value_id", &label)?,
            child_candidate_id: u32_field(entry, "child_candidate_id", &label)?,
            field_id: u32_field(entry, "field_id", &label)?,
            crossing_hook_id: u32_field(entry, "crossing_hook_id", &label)?,
            child_node_ref_id: u32_field(entry, "child_node_ref_id", &label)?,
            child_value_id: u32_field(entry, "child_value_id", &label)?,
            bind_target: optional_node_ref_bind_target(entry, "bind_target", &label)?,
        };
        let key = (alias.parent_candidate_id, alias.edge_id, alias.field_id);
        if previous_key.is_some_and(|previous| previous >= key) {
            return Err(
                "`node_ref_aliases` must be unique and strictly ordered by parent/edge/field"
                    .into(),
            );
        }
        previous_key = Some(key);

        validate_component_input_edge(
            &label,
            sites,
            child_sites,
            alias.parent_candidate_id,
            alias.edge_id,
            alias.child_candidate_id,
        )?;
        let parent = &sites[alias.parent_candidate_id as usize];
        let child = &sites[alias.child_candidate_id as usize];
        if child.incoming_lifecycle != Some(ComponentEdgeLifecycle::StaticSingle) {
            return Err(format!(
                "{label} must bind a static direct child occurrence"
            ));
        }
        let parent_owner = candidate_to_owner.get(&alias.parent_candidate_id);
        let child_owner = candidate_to_owner.get(&alias.child_candidate_id);
        if parent_owner.is_none() || parent_owner != child_owner {
            return Err(format!(
                "{label} must remain inside one final browser owner"
            ));
        }

        let parent_codegen = component_codegen.get(&parent.definition).ok_or_else(|| {
            format!(
                "{label} parent candidate {} has no component_codegen record",
                alias.parent_candidate_id
            )
        })?;
        let parent_matches = parent_codegen
            .direct_node_ref_aliases
            .iter()
            .filter(|mapping| {
                mapping.edge_id == alias.edge_id
                    && mapping.producer_node_ref_id == alias.parent_node_ref_id
                    && mapping.canonical_value_id == alias.parent_value_id
            })
            .count();
        if parent_matches != 1 {
            return Err(format!(
                "{label} parent NodeRef does not match exactly one authenticated direct alias codegen record"
            ));
        }

        let child_codegen = component_codegen.get(&child.definition).ok_or_else(|| {
            format!(
                "{label} child candidate {} has no component_codegen record",
                alias.child_candidate_id
            )
        })?;
        let child_node_ref_matches = child_codegen
            .inbound_node_refs
            .iter()
            .filter(|mapping| {
                mapping.field_id == alias.field_id
                    && mapping.producer_node_ref_id == alias.child_node_ref_id
                    && mapping.bind_target == alias.bind_target
            })
            .count();
        if child_node_ref_matches != 1 {
            return Err(format!(
                "{label} child NodeRef does not match exactly one authenticated inbound NodeRef codegen record"
            ));
        }
        let child_field_matches = child_codegen
            .inbound_fields
            .iter()
            .filter(|field| {
                field.field_id == alias.field_id
                    && field.canonical_value_id == alias.child_value_id
                    && field.crossing_hook_id == alias.crossing_hook_id
                    && field.callback_slot_id.is_none()
            })
            .count();
        if child_field_matches != 1 {
            return Err(format!(
                "{label} child field/value/crossing evidence does not match exactly one inbound field"
            ));
        }
        aliases.push(alias);
    }
    Ok(aliases)
}

fn component_site_is_descendant_of(
    sites: &[ComponentSiteRecord],
    mut candidate: u32,
    ancestor: u32,
) -> bool {
    loop {
        if candidate == ancestor {
            return true;
        }
        let Some(site) = sites.get(candidate as usize) else {
            return false;
        };
        let Some(parent) = site.parent_candidate_id else {
            return false;
        };
        candidate = parent;
    }
}

fn validate_component_input_edge(
    label: &str,
    sites: &[ComponentSiteRecord],
    child_sites: &BTreeMap<(u32, u32), u32>,
    parent_candidate_id: u32,
    edge_id: u32,
    child_candidate_id: u32,
) -> Result<(), String> {
    let Some(parent) = sites.get(parent_candidate_id as usize) else {
        return Err(format!(
            "{label} names absent parent candidate {parent_candidate_id}"
        ));
    };
    let Some(child) = sites.get(child_candidate_id as usize) else {
        return Err(format!(
            "{label} names absent child candidate {child_candidate_id}"
        ));
    };
    if child_sites.get(&(parent_candidate_id, edge_id)) != Some(&child_candidate_id)
        || child.parent_candidate_id != Some(parent.candidate_id)
        || child.incoming_edge_id != Some(edge_id)
    {
        return Err(format!(
            "{label} does not match the exact component site edge {parent_candidate_id}:{edge_id} -> {child_candidate_id}"
        ));
    }
    Ok(())
}

fn parse_definition_field(
    parent: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<DefinitionId, String> {
    let definition = parent
        .get(field)
        .ok_or_else(|| format!("{label} is missing `{field}`"))?;
    let definition = object(definition, &format!("{label}.{field}"))?;
    let unit = string(definition, "cargo_unit_id")?;
    let record = string(definition, "record_id")?;
    Ok(DefinitionId {
        cargo_unit_id: decode_lower_hex_id(&format!("{label}.{field}.cargo_unit_id"), unit)?,
        record_id: decode_lower_hex_id(&format!("{label}.{field}.record_id"), record)?,
    })
}

fn canonical_hex_string(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<String, String> {
    let value = string(object, field)?;
    if value.len() % 2 != 0
        || value
            .as_bytes()
            .iter()
            .any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "{label}.{field} must contain an even number of lowercase hexadecimal characters"
        ));
    }
    Ok(value.to_owned())
}

fn linked_helper_key(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<crate::LinkedHelperKey, String> {
    let encoded = canonical_hex_string(object, field, label)?;
    linked_helper_key_text(&format!("{label}.{field}"), &encoded)
}

fn linked_helper_key_text(label: &str, encoded: &str) -> Result<crate::LinkedHelperKey, String> {
    let bytes = decode_lower_hex_bytes(label, encoded)?;
    crate::LinkedHelperKey::from_canonical_bytes(bytes)
        .map_err(|error| format!("{label} is not a valid canonical helper identity: {error}"))
}

fn legacy_json_hash(domain: &str, value: &Value) -> Result<[u8; 32], String> {
    // Match transport's feature-independent JSON: preserve array order and
    // recursively sort every object, including with serde preserve_order.
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| format!("failed to canonicalize final helper JSON: {error}"))?;
    let hash = crate::abi::canonical_legacy_json_bytes_hash(domain, &bytes);
    Ok(*hash.as_bytes())
}

fn sha256_field(object: &Map<String, Value>, field: &str, label: &str) -> Result<[u8; 32], String> {
    let value = string(object, field)?;
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!(
            "{label}.{field} must be `sha256:` followed by 64 lowercase hexadecimal characters"
        ));
    };
    decode_lower_hex_id(&format!("{label}.{field}"), hex)
}

fn shared_state_type_key(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<String, String> {
    let value = string(object, field)?;
    if value.contains('.')
        && value.trim() == value
        && !value.chars().any(char::is_whitespace)
        && !value.contains('/')
        && !value.contains('\\')
    {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "{label}.{field} is not a namespaced SharedState protocol key"
        ))
    }
}

fn optional_u32_field(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<u32>, String> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("{label} is missing `{field}`"))?;
    if value.is_null() {
        Ok(None)
    } else {
        value_u32(value, &format!("{label}.{field}")).map(Some)
    }
}

fn optional_node_ref_bind_target(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<FinalNodeRefBindTarget>, String> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("{label} is missing `{field}`"))?;
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| format!("{label}.{field} must be null or a DOM-family string"))?;
    let target = match value {
        "element" => FinalNodeRefBindTarget::Element,
        "html_element" => FinalNodeRefBindTarget::HtmlElement,
        "input" => FinalNodeRefBindTarget::Input,
        "textarea" => FinalNodeRefBindTarget::Textarea,
        "select" => FinalNodeRefBindTarget::Select,
        "form" => FinalNodeRefBindTarget::Form,
        "dialog" => FinalNodeRefBindTarget::Dialog,
        other => {
            return Err(format!(
                "{label}.{field} has unsupported NodeRef bind target `{other}`"
            ));
        }
    };
    Ok(Some(target))
}

fn u32_field(object: &Map<String, Value>, field: &str, label: &str) -> Result<u32, String> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("{label} is missing `{field}`"))?;
    value_u32(value, &format!("{label}.{field}"))
}

fn bool_field(object: &Map<String, Value>, field: &str, label: &str) -> Result<bool, String> {
    object
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{label}.{field} must be a boolean"))
}

fn value_u32(value: &Value, label: &str) -> Result<u32, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("{label} must be an unsigned integer"))?;
    u32::try_from(value).map_err(|_| format!("{label} exceeds the supported u32 range"))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be a JSON object"))
}

fn require_exact_object_fields(
    object: &Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        return Ok(());
    }

    let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
    let unknown = actual.difference(&expected).copied().collect::<Vec<_>>();
    Err(format!(
        "{label} has invalid fields; missing {missing:?}, unknown {unknown:?}"
    ))
}

fn array<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a [Value], String> {
    object
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("final RSJS plan field `{field}` must be an array"))
}

fn string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("final RSJS plan field `{field}` must be a string"))
}

fn exact_str(object: &Map<String, Value>, field: &str, expected: &str) -> Result<(), String> {
    let actual = string(object, field)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "unsupported final RSJS plan `{field}` value `{actual}`; expected `{expected}`"
        ))
    }
}

fn exact_u64(object: &Map<String, Value>, field: &str, expected: u64) -> Result<(), String> {
    let actual = object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("final RSJS plan field `{field}` must be an unsigned integer"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "unsupported final RSJS plan `{field}` value `{actual}`; expected `{expected}`"
        ))
    }
}

pub fn decode_lower_hex_id(label: &str, encoded: &str) -> Result<[u8; 32], String> {
    if encoded.len() != 64 {
        return Err(format!(
            "`{label}` must contain exactly 64 lowercase hexadecimal characters; received {}",
            encoded.len()
        ));
    }
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let high_index = index * 2;
        let low_index = high_index + 1;
        let high = lower_hex(encoded.as_bytes()[high_index]).ok_or_else(|| {
            format!("`{label}` contains a non-lowercase-hex byte at offset {high_index}")
        })?;
        let low = lower_hex(encoded.as_bytes()[low_index]).ok_or_else(|| {
            format!("`{label}` contains a non-lowercase-hex byte at offset {low_index}")
        })?;
        *output = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_lower_hex_bytes(label: &str, encoded: &str) -> Result<Vec<u8>, String> {
    if !encoded.len().is_multiple_of(2) {
        return Err(format!(
            "`{label}` must contain an even number of lowercase hexadecimal characters"
        ));
    }
    let mut decoded = Vec::with_capacity(encoded.len() / 2);
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let offset = index * 2;
        let high = lower_hex(pair[0]).ok_or_else(|| {
            format!("`{label}` contains a non-lowercase-hex byte at offset {offset}")
        })?;
        let low = lower_hex(pair[1]).ok_or_else(|| {
            format!(
                "`{label}` contains a non-lowercase-hex byte at offset {}",
                offset + 1
            )
        })?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

const fn lower_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub fn hex_id(id: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in id {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn canonical_plan_json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    serde_json::to_vec(&canonical)
        .map_err(|error| format!("failed to canonicalize final-plan content: {error}"))
}

fn definition_json_records<'a>(
    root: &'a Map<String, Value>,
    table: &str,
) -> Result<BTreeMap<DefinitionId, &'a Value>, String> {
    array(root, table)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let label = format!("{table}[{index}]");
            let entry = object(value, &label)?;
            Ok((parse_definition_field(entry, "definition", &label)?, value))
        })
        .collect()
}

fn finalize_asset_identities(
    selection: &mut FinalComponentSelection,
    selected_definition: DefinitionId,
    components: &BTreeMap<DefinitionId, ComponentRecord>,
    sites: &[ComponentSiteRecord],
    root: &Map<String, Value>,
) -> Result<(), String> {
    let codegen_records = definition_json_records(root, "component_codegen")?;
    let helper_records = definition_json_records(root, "helpers")?;
    let helper_codegen = definition_json_records(root, "helper_codegen")?;
    let helper_definitions_by_key = helper_records
        .iter()
        .map(|(&definition, value)| {
            let entry = object(value, "validated helper")?;
            Ok((
                linked_helper_key(entry, "semantic_key_canonical", "validated helper")?,
                definition,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut fragments = BTreeMap::new();
    for (&definition, component) in components {
        let candidates = sites
            .iter()
            .filter(|site| site.definition == definition)
            .map(|site| site.candidate_id)
            .collect::<BTreeSet<_>>();
        let signals = fragment_signal_exports_for(selection, &candidates)?;
        let lifecycles = fragment_lifecycle_exports_for(selection, &candidates)?;
        let node_exports = selection
            .all_node_ref_aliases
            .iter()
            .filter(|alias| candidates.contains(&alias.parent_candidate_id))
            .map(|alias| alias.parent_node_ref_id)
            .collect::<BTreeSet<_>>();
        let node_imports = selection
            .all_node_ref_aliases
            .iter()
            .filter(|alias| candidates.contains(&alias.child_candidate_id))
            .map(|alias| alias.child_node_ref_id)
            .collect::<BTreeSet<_>>();
        let helpers = component
            .helper_closure
            .iter()
            .map(|key| {
                let helper_definition = helper_definitions_by_key.get(key).ok_or_else(|| {
                    "validated component closure has no exact helper definition".to_owned()
                })?;
                Ok(serde_json::json!({
                    "descriptor": helper_records[helper_definition],
                    "codegen": helper_codegen[helper_definition],
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let material = serde_json::json!({
            "descriptor_unit": hex_id(&definition.cargo_unit_id),
            "descriptor_record": hex_id(&definition.record_id),
            "semantic_key": component.semantic_key,
            "codegen": codegen_records[&definition],
            "active": selection.all_sites.iter().any(|site|
                candidates.contains(&site.candidate_id)
                    && !matches!(site.owner_role, FinalOwnerRole::ServerOnly)),
            "signal_exports": signals.iter().map(|signal| signal.0).collect::<Vec<_>>(),
            "lifecycle_exports": lifecycles.iter().map(|export| serde_json::json!({
                "control": export.control_id,
                "expression": export.producer_expression_id.0,
            })).collect::<Vec<_>>(),
            "node_ref_exports": node_exports.into_iter().collect::<Vec<_>>(),
            "node_ref_imports": node_imports.into_iter().collect::<Vec<_>>(),
            "helper_bindings": component.helper_edges.iter().map(|(&call, edge)| serde_json::json!({
                "call": call,
                "source": hex_id(&edge.source_edge),
                "target": edge.target.as_bytes(),
            })).collect::<Vec<_>>(),
            "helpers": helpers,
        });
        let mut hash = Sha256::new();
        hash.update(b"proa.rsjs.final-component-fragment.v1\0");
        hash.update(canonical_plan_json_bytes(&material)?);
        let digest: [u8; 32] = hash.finalize().into();
        fragments.insert(definition, format!("rsjs_fragment_{}", hex_id(&digest)));
    }
    selection.fragment_asset_id = fragments[&selected_definition].clone();
    for site in sites {
        let fragment = &fragments[&site.definition];
        selection
            .codegen_by_candidate
            .get_mut(&site.candidate_id)
            .ok_or_else(|| "validated candidate has no codegen generation".to_owned())?
            .fragment_key = fragment.clone();
    }
    for site in &mut selection.all_sites {
        site.fragment_asset_id = selection.codegen_by_candidate[&site.candidate_id]
            .fragment_key
            .clone();
    }
    let mut owners = BTreeMap::new();
    for site in &selection.all_sites {
        if let FinalOwnerRole::Root { owner_id } = site.owner_role {
            owners.insert(owner_id, owner_content_asset_id(selection, site)?);
        }
    }
    for site in &mut selection.all_sites {
        match site.owner_role {
            FinalOwnerRole::ServerOnly => {}
            FinalOwnerRole::Root { owner_id } => {
                site.owner_asset_id = Some(owners[&owner_id].clone());
                site.seed_key = site.owner_asset_id.clone();
            }
            FinalOwnerRole::Member { owner_id, .. } => {
                site.owner_asset_id = Some(owners[&owner_id].clone());
            }
        }
    }
    let selected_candidates = selection
        .sites
        .iter()
        .map(|site| site.candidate_id)
        .collect::<BTreeSet<_>>();
    selection.sites = selection
        .all_sites
        .iter()
        .filter(|site| selected_candidates.contains(&site.candidate_id))
        .cloned()
        .collect();
    Ok(())
}

fn owner_content_asset_id(
    selection: &FinalComponentSelection,
    root: &FinalComponentSite,
) -> Result<String, String> {
    let mut plan = linked_owner_module_plan(selection, root)?;
    // The owner's name appears in its own registration and root seed check.
    // Hash the exact emitter with only those self-reference slots normalized,
    // then emit normally using the resulting content identity.
    plan.owner_asset_id = OWNER_HASH_PLACEHOLDER.to_owned();
    plan.mounts[0].seed_key = OWNER_HASH_PLACEHOLDER.to_owned();
    let aliases = linked_owner_node_ref_aliases(selection, &plan)?;
    let js = crate::emit_linked_owner_module_with_node_refs(&plan, &aliases)
        .map_err(|error| format!("failed to hash linked-owner payload: {error}"))?;
    let identity = selection.runtime_site(root.candidate_id)?;
    let mut hash = Sha256::new();
    hash.update(b"proa.rsjs.final-owner-module.v1\0");
    hash.update(identity.site_id);
    hash.update([u8::from(plan.uses_queries), u8::from(plan.uses_mutations)]);
    hash.update((js.len() as u64).to_le_bytes());
    hash.update(js.as_bytes());
    let digest: [u8; 32] = hash.finalize().into();
    Ok(format!("rsjs_owner_{}", hex_id(&digest)))
}

fn member_seed_key(site_id: [u8; 32]) -> String {
    format!("rsjs_member_{}", hex_id(&site_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PLAN_ID: [u8; 32] = [0x22; 32];
    const PARENT_UNIT: [u8; 32] = [1; 32];
    const PARENT_RECORD: [u8; 32] = [2; 32];
    const CHILD_UNIT: [u8; 32] = [3; 32];
    const CHILD_RECORD: [u8; 32] = [4; 32];
    const GRANDCHILD_UNIT: [u8; 32] = [5; 32];
    const GRANDCHILD_RECORD: [u8; 32] = [6; 32];
    const HELPER_UNIT: [u8; 32] = [8; 32];
    const HELPER_RECORD: [u8; 32] = [9; 32];

    #[test]
    fn helper_hash_keeps_transport_canonical_bytes_across_json_feature_graphs() {
        let unordered: Value = serde_json::from_str(
            r#"{"z":0,"a":[{"z":null,"a":"newline\nquote\""},true,false,1.5]}"#,
        )
        .unwrap();
        let canonical_bytes = br#"{"a":[{"a":"newline\nquote\"","z":null},true,false,1.5],"z":0}"#;
        let reordered: Value = serde_json::from_slice(canonical_bytes).unwrap();
        let expected = crate::abi::canonical_legacy_json_bytes_hash(
            HELPER_CODEGEN_IR_JSON_HASH_DOMAIN,
            canonical_bytes,
        );
        for value in [&unordered, &reordered] {
            assert_eq!(
                legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, value).unwrap(),
                *expected.as_bytes(),
            );
        }

        let mut different_array_order = unordered;
        different_array_order["a"]
            .as_array_mut()
            .unwrap()
            .swap(1, 2);
        assert_ne!(
            legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &different_array_order).unwrap(),
            *expected.as_bytes(),
            "canonicalization must preserve semantically significant array order",
        );
    }

    fn definition(unit: [u8; 32], record: [u8; 32]) -> Value {
        json!({
            "cargo_unit_id": hex_id(&unit),
            "record_id": hex_id(&record),
        })
    }

    fn component_codegen(unit: [u8; 32], record: [u8; 32], asset_id: &str) -> Value {
        json!({
            "definition": definition(unit, record),
            "asset_id": asset_id,
            "fragment_key": asset_id,
            "fragment_root": { "kind": "exact_one_element" },
            "uses_queries": false,
            "uses_mutations": false,
            "inbound_fields": [],
            "inbound_targets": [],
            "signals": [],
            "expressions": [],
            "payload_handlers": [],
            "callback_slots": [],
            "inbound_node_refs": [],
            "direct_node_ref_aliases": [],
        })
    }

    fn linked_plan() -> Value {
        json!({
            "format": PLAN_FORMAT,
            "schema_version": PLAN_SCHEMA_VERSION,
            "link_unit_id": "11".repeat(32),
            "plan_id": "22".repeat(32),
            "payload_hash": format!("sha256:{}", "33".repeat(32)),
            "component_descriptor_hashes": [],
            "helper_descriptor_hashes": [],
            "components": [
                {
                    "definition": definition(PARENT_UNIT, PARENT_RECORD),
                    "semantic_key_canonical": "aa",
                    "root_eligible": true,
                    "descriptor_hash": format!("sha256:{}", "44".repeat(32)),
                    "descriptor_semantic_abi": "00",
                    "component_edges": [{
                        "edge_id": 7,
                        "witness_id": 0,
                        "source_edge_id": "00",
                        "target_definition": definition(CHILD_UNIT, CHILD_RECORD),
                        "target_semantic_key_canonical": "bb",
                        "lifecycle": { "kind": "static_single" },
                    }],
                    "helper_edges": [],
                    "helper_closure": [],
                },
                {
                    "definition": definition(CHILD_UNIT, CHILD_RECORD),
                    "semantic_key_canonical": "bb",
                    "root_eligible": true,
                    "descriptor_hash": format!("sha256:{}", "55".repeat(32)),
                    "descriptor_semantic_abi": "00",
                    "component_edges": [],
                    "helper_edges": [],
                    "helper_closure": [],
                },
            ],
            "component_codegen": [
                component_codegen(PARENT_UNIT, PARENT_RECORD, "parent_1234"),
                component_codegen(CHILD_UNIT, CHILD_RECORD, "child_1234"),
            ],
            "helper_codegen": [],
            "component_sites": [
                {
                    "candidate_id": 0,
                    "root_semantic_key_canonical": "aa",
                    "component_semantic_key_canonical": "aa",
                    "definition": definition(PARENT_UNIT, PARENT_RECORD),
                    "parent_candidate_id": null,
                    "incoming_edge_id": null,
                    "structural_path": [],
                    "incoming_lifecycle": null,
                    "reactive_lifecycle_root_candidate_id": null,
                },
                {
                    "candidate_id": 1,
                    "root_semantic_key_canonical": "aa",
                    "component_semantic_key_canonical": "bb",
                    "definition": definition(CHILD_UNIT, CHILD_RECORD),
                    "parent_candidate_id": 0,
                    "incoming_edge_id": 7,
                    "structural_path": [7],
                    "incoming_lifecycle": { "kind": "static_single" },
                    "reactive_lifecycle_root_candidate_id": null,
                },
                {
                    "candidate_id": 2,
                    "root_semantic_key_canonical": "bb",
                    "component_semantic_key_canonical": "bb",
                    "definition": definition(CHILD_UNIT, CHILD_RECORD),
                    "parent_candidate_id": null,
                    "incoming_edge_id": null,
                    "structural_path": [],
                    "incoming_lifecycle": null,
                    "reactive_lifecycle_root_candidate_id": null,
                },
            ],
            "standalone_roots": [
                {
                    "definition": definition(PARENT_UNIT, PARENT_RECORD),
                    "component_semantic_key_canonical": "aa",
                    "disposition": {
                        "kind": "admitted",
                        "candidate_id": 0,
                    },
                },
                {
                    "definition": definition(CHILD_UNIT, CHILD_RECORD),
                    "component_semantic_key_canonical": "bb",
                    "disposition": {
                        "kind": "admitted",
                        "candidate_id": 2,
                    },
                },
            ],
            "helpers": [],
            "reachable_helpers": [],
            "owners": [
                {
                    "owner_id": 0,
                    "candidates": [0, 1],
                    "strategy": { "kind": "eager" },
                },
                {
                    "owner_id": 1,
                    "candidates": [2],
                    "strategy": { "kind": "visible" },
                },
            ],
            "candidate_to_owner": [
                { "candidate_id": 0, "owner_id": 0 },
                { "candidate_id": 1, "owner_id": 0 },
                { "candidate_id": 2, "owner_id": 1 },
            ],
            "component_inputs": {
                "live_props": [],
                "live_callbacks": [],
            },
            "shared_state": [],
            "shared_state_providers": [],
            "shared_state_requirements": [],
            "node_ref_aliases": [],
        })
    }

    fn linked_helper_plan() -> Value {
        let mut plan = linked_plan();
        let signature = json!({ "params": [], "return_type": "u32" });
        let body: Value =
            serde_json::from_str(&crate::client_expr_json(&crate::ClientExpr::int(1))).unwrap();
        let signature_hash =
            legacy_json_hash(HELPER_SIGNATURE_JSON_HASH_DOMAIN, &signature).unwrap();
        let body_hash = legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &body).unwrap();
        let source_ir_hash = [0x66; 32];
        plan["components"][0]["helper_edges"] = json!([{
            "call_id": 0,
            "witness_id": 0,
            "source_edge_id": "77".repeat(32),
            "target_definition": definition(HELPER_UNIT, HELPER_RECORD),
            "target_semantic_key_canonical": "dd",
        }]);
        plan["components"][0]["helper_closure"] = json!(["dd"]);
        plan["helpers"] = json!([{
            "definition": definition(HELPER_UNIT, HELPER_RECORD),
            "semantic_key_canonical": "dd",
            "descriptor_hash": format!("sha256:{}", "88".repeat(32)),
            "descriptor_semantic_abi": "00",
            "signature_hash": format!("sha256:{}", hex_id(&signature_hash)),
            "client_ir_hash": format!("sha256:{}", hex_id(&source_ir_hash)),
            "helper_edges": [],
            "transitive_closure": ["dd"],
        }]);
        plan["helper_codegen"] = json!([{
            "definition": definition(HELPER_UNIT, HELPER_RECORD),
            "semantic_key_canonical": "dd",
            "signature": signature,
            "source_ir_hash": format!("sha256:{}", hex_id(&source_ir_hash)),
            "body_ir_hash": format!("sha256:{}", hex_id(&body_hash)),
            "body_ir": body,
        }]);
        plan["reachable_helpers"] = json!(["dd"]);
        plan
    }

    fn conditional_plan(branch: &str) -> Value {
        let mut plan = linked_plan();
        let lifecycle = json!({
            "kind": "conditional_single",
            "control": 3,
            "predicate_value": 5,
            "predicate_origin": 7,
            "branch": branch,
        });
        plan["components"][0]["component_edges"][0]["lifecycle"] = lifecycle.clone();
        plan["component_sites"][1]["incoming_lifecycle"] = lifecycle;
        plan["component_sites"][1]["reactive_lifecycle_root_candidate_id"] = json!(1);
        plan["component_codegen"][0]["expressions"] = json!([{
            "producer_expression_id": 13,
            "canonical_value_id": 5,
        }]);
        plan
    }

    fn configure_live_codegen(plan: &mut Value) {
        plan["component_codegen"][0]["signals"] = json!([{
            "producer_signal_id": 17, "canonical_value_id": 5,
        }]);
        plan["component_codegen"][0]["payload_handlers"] = json!([{
            "prelude_local_index": 19, "canonical_value_id": 11,
        }]);
        plan["component_codegen"][1]["inbound_fields"] = json!([
            {
                "field_id": 0, "producer_value_id": 0,
                "canonical_value_id": 6, "crossing_hook_id": 4, "callback_slot_id": null,
            },
            {
                "field_id": 1, "producer_value_id": 1,
                "canonical_value_id": 12, "crossing_hook_id": 10, "callback_slot_id": 29,
            },
        ]);
        plan["component_codegen"][1]["inbound_targets"] = json!([{
            "prop_site_id": 8, "field_id": 0,
            "producer_signal_id": 23, "canonical_signal_value_id": 7,
        }]);
        plan["component_codegen"][1]["signals"] = json!([{
            "producer_signal_id": 23, "canonical_value_id": 7,
        }]);
        plan["component_codegen"][1]["callback_slots"] = json!([{
            "producer_callback_slot_id": 29, "canonical_value_id": 12,
        }]);
    }

    fn node_ref_plan() -> Value {
        let mut plan = linked_plan();
        plan["component_sites"]
            .as_array_mut()
            .unwrap()
            .retain(|site| site["candidate_id"] != json!(2));
        plan["owners"]
            .as_array_mut()
            .unwrap()
            .retain(|owner| owner["owner_id"] != json!(1));
        plan["candidate_to_owner"]
            .as_array_mut()
            .unwrap()
            .retain(|mapping| mapping["candidate_id"] != json!(2));
        plan["standalone_roots"][1]["disposition"] = json!({
            "kind": "context_required",
            "missing": [{
                "kind": "node_ref",
                "consumer_semantic_key_canonical": "bb",
                "structural_path": [],
                "field_id": 0,
                "crossing_hook_id": 31,
                "node_ref_id": 8,
                "bind_target": "input",
            }],
        });
        plan["component_codegen"][0]["direct_node_ref_aliases"] = json!([{
            "edge_id": 7,
            "field_name": "focus_ref",
            "producer_node_ref_id": 4,
            "canonical_value_id": 20,
        }]);
        plan["component_codegen"][1]["inbound_fields"] = json!([{
            "field_id": 0,
            "producer_value_id": 0,
            "canonical_value_id": 30,
            "crossing_hook_id": 31,
            "callback_slot_id": null,
        }]);
        plan["component_codegen"][1]["inbound_node_refs"] = json!([{
            "field_id": 0,
            "producer_node_ref_id": 8,
            "bind_target": "input",
        }]);
        plan["node_ref_aliases"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "parent_node_ref_id": 4,
            "parent_value_id": 20,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 31,
            "child_node_ref_id": 8,
            "child_value_id": 30,
            "bind_target": "input",
        }]);
        plan
    }

    fn conditional_plan_with_static_descendant() -> Value {
        let mut plan = conditional_plan("then");
        plan["components"][1]["component_edges"] = json!([{
            "edge_id": 9,
            "witness_id": 0,
            "source_edge_id": "01",
            "target_definition": definition(GRANDCHILD_UNIT, GRANDCHILD_RECORD),
            "target_semantic_key_canonical": "cc",
            "lifecycle": { "kind": "static_single" },
        }]);
        plan["components"].as_array_mut().unwrap().push(json!({
            "definition": definition(GRANDCHILD_UNIT, GRANDCHILD_RECORD),
            "semantic_key_canonical": "cc",
            "root_eligible": false,
            "descriptor_hash": format!("sha256:{}", "66".repeat(32)),
            "descriptor_semantic_abi": "00",
            "component_edges": [],
            "helper_edges": [],
            "helper_closure": [],
        }));
        plan["component_codegen"]
            .as_array_mut()
            .unwrap()
            .push(component_codegen(
                GRANDCHILD_UNIT,
                GRANDCHILD_RECORD,
                "grandchild_1234",
            ));
        plan["component_sites"].as_array_mut().unwrap().insert(
            2,
            json!({
                "candidate_id": 2,
                "root_semantic_key_canonical": "aa",
                "component_semantic_key_canonical": "cc",
                "definition": definition(GRANDCHILD_UNIT, GRANDCHILD_RECORD),
                "parent_candidate_id": 1,
                "incoming_edge_id": 9,
                "structural_path": [7, 9],
                "incoming_lifecycle": { "kind": "static_single" },
                "reactive_lifecycle_root_candidate_id": 1,
            }),
        );
        // This fixture exercises the child only beneath the conditional
        // parent. Once the child definition gains a grandchild edge, keeping
        // its unrelated standalone root would also require a second expanded
        // grandchild occurrence and distract from the lifecycle assertion.
        plan["components"][1]["root_eligible"] = json!(false);
        plan["component_sites"].as_array_mut().unwrap().remove(3);
        plan["standalone_roots"][1]["disposition"] = json!({ "kind": "structural_incoming_only" });
        plan["standalone_roots"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "definition": definition(GRANDCHILD_UNIT, GRANDCHILD_RECORD),
                "component_semantic_key_canonical": "cc",
                "disposition": { "kind": "structural_incoming_only" },
            }));
        plan["owners"][0]["candidates"] = json!([0, 1, 2]);
        plan["owners"].as_array_mut().unwrap().truncate(1);
        plan["candidate_to_owner"] = json!([
            { "candidate_id": 0, "owner_id": 0 },
            { "candidate_id": 1, "owner_id": 0 },
            { "candidate_id": 2, "owner_id": 0 },
        ]);
        plan
    }

    fn runtime_candidate(selection: &FinalComponentSelection, dense: u32) -> String {
        selection
            .runtime_site(dense)
            .unwrap()
            .candidate_id
            .to_string()
    }

    fn encode(plan: &Value) -> Vec<u8> {
        serde_json::to_vec(plan).unwrap()
    }

    fn insert_unrelated_first_root(plan: &mut Value) {
        fn shift(value: &mut Value) {
            match value {
                Value::Object(object) => {
                    for (key, value) in object {
                        if matches!(
                            key.as_str(),
                            "candidate_id"
                                | "parent_candidate_id"
                                | "child_candidate_id"
                                | "root_candidate_id"
                                | "reactive_lifecycle_root_candidate_id"
                                | "provider_host_candidate_id"
                                | "host_candidate_id"
                                | "owner_id"
                        ) {
                            if let Some(number) = value.as_u64() {
                                *value = json!(number + 1);
                            }
                        } else if key == "candidates" {
                            for candidate in value.as_array_mut().unwrap() {
                                *candidate = json!(candidate.as_u64().unwrap() + 1);
                            }
                        } else {
                            shift(value);
                        }
                    }
                }
                Value::Array(values) => values.iter_mut().for_each(shift),
                _ => {}
            }
        }
        shift(plan);
        let unit = [10; 32];
        let record = [11; 32];
        plan["components"].as_array_mut().unwrap().insert(
            0,
            json!({
                "definition": definition(unit, record),
                "semantic_key_canonical": "01",
                "root_eligible": true,
                "component_edges": [],
                "helper_edges": [],
                "helper_closure": [],
            }),
        );
        plan["component_codegen"]
            .as_array_mut()
            .unwrap()
            .insert(0, component_codegen(unit, record, "unrelated_1234"));
        plan["component_sites"].as_array_mut().unwrap().insert(
            0,
            json!({
                "candidate_id": 0,
                "root_semantic_key_canonical": "01",
                "component_semantic_key_canonical": "01",
                "definition": definition(unit, record),
                "parent_candidate_id": null,
                "incoming_edge_id": null,
                "structural_path": [],
                "incoming_lifecycle": null,
                "reactive_lifecycle_root_candidate_id": null,
            }),
        );
        plan["standalone_roots"].as_array_mut().unwrap().insert(
            0,
            json!({
                "definition": definition(unit, record),
                "component_semantic_key_canonical": "01",
                "disposition": { "kind": "admitted", "candidate_id": 0 },
            }),
        );
        plan["owners"].as_array_mut().unwrap().insert(
            0,
            json!({
                "owner_id": 0, "candidates": [0], "strategy": { "kind": "eager" },
            }),
        );
        plan["candidate_to_owner"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({ "candidate_id": 0, "owner_id": 0 }));
    }

    #[test]
    fn unrelated_root_insertion_preserves_runtime_sites_and_browser_coordinates() {
        let mut plan = node_ref_plan();
        let before =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        insert_unrelated_first_root(&mut plan);
        let new_plan_id = [0x23; 32];
        plan["plan_id"] = json!(hex_id(&new_plan_id));
        let after =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, new_plan_id, &encode(&plan))
                .unwrap();
        assert_ne!(before.plan_id, after.plan_id);
        for (&dense, &identity) in &before.runtime_sites {
            assert_eq!(identity, after.runtime_site(dense + 1).unwrap());
        }
        assert_eq!(before.all_sites[1].seed_key, after.all_sites[2].seed_key);
        let old_owner = linked_owner_module_plans(&before).unwrap().remove(0);
        let new_owner = linked_owner_module_plans(&after).unwrap().remove(0);
        assert_eq!(old_owner.root_candidate_id, new_owner.root_candidate_id);
        assert_eq!(
            linked_owner_node_ref_aliases(&before, &old_owner).unwrap(),
            linked_owner_node_ref_aliases(&after, &new_owner).unwrap(),
        );
        assert_eq!(old_owner.mounts[1], new_owner.mounts[1]);
        assert_eq!(old_owner, new_owner);
        assert_eq!(
            crate::emit_linked_owner_module_with_node_refs(
                &old_owner,
                &linked_owner_node_ref_aliases(&before, &old_owner).unwrap()
            )
            .unwrap(),
            crate::emit_linked_owner_module_with_node_refs(
                &new_owner,
                &linked_owner_node_ref_aliases(&after, &new_owner).unwrap()
            )
            .unwrap(),
        );
    }

    #[test]
    fn descriptor_generation_changes_do_not_change_logical_occurrence_identity() {
        let mut plan = linked_plan();
        let before =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        fn replace_unit(value: &mut Value) {
            match value {
                Value::Object(object) => {
                    for (key, value) in object {
                        if key == "cargo_unit_id" && *value == json!(hex_id(&PARENT_UNIT)) {
                            *value = json!(hex_id(&[12; 32]));
                        } else {
                            replace_unit(value);
                        }
                    }
                }
                Value::Array(values) => values.iter_mut().for_each(replace_unit),
                _ => {}
            }
        }
        replace_unit(&mut plan);
        let after = validate_component_plan_bytes([12; 32], PARENT_RECORD, PLAN_ID, &encode(&plan))
            .unwrap();
        assert_eq!(before.runtime_sites, after.runtime_sites);
        assert_ne!(before.fragment_asset_id, after.fragment_asset_id);
        assert_ne!(
            before.all_sites[0].owner_asset_id,
            after.all_sites[0].owner_asset_id
        );
        assert_eq!(
            before.all_sites[2].owner_asset_id, after.all_sites[2].owner_asset_id,
            "a different definition's standalone owner has no changed import"
        );
        assert_ne!(
            before.runtime_site(1).unwrap(),
            before.runtime_site(2).unwrap(),
            "a child under its parent and the same definition's standalone root are distinct"
        );
    }

    #[test]
    fn child_fragment_generation_refreshes_every_importing_owner() {
        let mut plan = linked_plan();
        insert_unrelated_first_root(&mut plan);
        let before =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        plan["component_codegen"][2]["asset_id"] = json!("child_changed");
        plan["component_codegen"][2]["fragment_key"] = json!("child_changed");
        let after =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(before.fragment_asset_id, after.fragment_asset_id);
        assert_ne!(
            before.all_sites[2].fragment_asset_id,
            after.all_sites[2].fragment_asset_id
        );
        for root in [1, 3] {
            assert_ne!(
                before.all_sites[root].owner_asset_id,
                after.all_sites[root].owner_asset_id
            );
        }
        assert_eq!(
            before.all_sites[0].owner_asset_id,
            after.all_sites[0].owner_asset_id
        );
    }

    #[test]
    fn helper_body_generation_refreshes_shared_consumers_and_leaves_unrelated_owners_stable() {
        let mut plan = linked_helper_plan();
        plan["components"][1]["helper_edges"] = plan["components"][0]["helper_edges"].clone();
        plan["components"][1]["helper_edges"][0]["source_edge_id"] = json!("79".repeat(32));
        for component in plan["components"].as_array_mut().unwrap() {
            component["helper_closure"] = json!(["dd", "ee"]);
        }
        let mut leaf_descriptor = plan["helpers"][0].clone();
        leaf_descriptor["definition"] = definition([15; 32], [16; 32]);
        leaf_descriptor["semantic_key_canonical"] = json!("ee");
        leaf_descriptor["transitive_closure"] = json!(["ee"]);
        let mut leaf_codegen = plan["helper_codegen"][0].clone();
        leaf_codegen["definition"] = definition([15; 32], [16; 32]);
        leaf_codegen["semantic_key_canonical"] = json!("ee");
        plan["helpers"][0]["helper_edges"] = json!([{
            "call_id": 0, "witness_id": 0, "source_edge_id": "78".repeat(32),
            "target_definition": definition([15; 32], [16; 32]),
            "target_semantic_key_canonical": "ee",
        }]);
        plan["helpers"][0]["transitive_closure"] = json!(["dd", "ee"]);
        let forwarding_body = crate::ClientExpr::call(
            crate::ClientFunctionRef::Helper {
                edge: crate::abi::HelperCallEdgeId::new(0),
                authored_path: "ignored::leaf".to_owned(),
                name: "leaf".to_owned(),
            },
            vec![],
        );
        let forwarding_body: Value =
            serde_json::from_str(&crate::client_expr_json(&forwarding_body)).unwrap();
        let forwarding_hash =
            legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &forwarding_body).unwrap();
        plan["helper_codegen"][0]["body_ir"] = forwarding_body;
        plan["helper_codegen"][0]["body_ir_hash"] =
            json!(format!("sha256:{}", hex_id(&forwarding_hash)));
        plan["helpers"]
            .as_array_mut()
            .unwrap()
            .push(leaf_descriptor);
        plan["helper_codegen"]
            .as_array_mut()
            .unwrap()
            .push(leaf_codegen);
        plan["reachable_helpers"] = json!(["ee", "dd"]);
        insert_unrelated_first_root(&mut plan);
        let before =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        let body: Value =
            serde_json::from_str(&crate::client_expr_json(&crate::ClientExpr::int(2))).unwrap();
        let hash = legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &body).unwrap();
        // Only the transitive leaf changes; both components still call the
        // unchanged forwarding helper through the same source-edge bindings.
        plan["helper_codegen"][1]["body_ir"] = body;
        plan["helper_codegen"][1]["body_ir_hash"] = json!(format!("sha256:{}", hex_id(&hash)));
        let after =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_ne!(before.fragment_asset_id, after.fragment_asset_id);
        for root in [1, 3] {
            assert_ne!(
                before.all_sites[root].owner_asset_id,
                after.all_sites[root].owner_asset_id
            );
        }
        assert_eq!(
            before.all_sites[0].owner_asset_id,
            after.all_sites[0].owner_asset_id
        );
    }

    #[test]
    fn required_export_union_changes_fragment_generation_without_source_id_changes() {
        let mut plan = linked_plan();
        configure_live_codegen(&mut plan);
        let before =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert!(linked_fragment_signal_exports(&before).unwrap().is_empty());
        plan["component_inputs"]["live_props"] = json!([{
            "parent_candidate_id": 0, "edge_id": 7, "child_candidate_id": 1,
            "field_id": 0, "crossing_hook_id": 4,
            "parent_value_id": 5, "child_value_id": 6, "prop_site_id": 8,
            "temporal": { "kind": "live_read" },
        }]);
        let after =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            linked_fragment_signal_exports(&after).unwrap(),
            vec![crate::SignalId(17)]
        );
        assert_ne!(before.fragment_asset_id, after.fragment_asset_id);
        assert_ne!(
            before.all_sites[0].owner_asset_id,
            after.all_sites[0].owner_asset_id
        );
        assert_eq!(
            before.all_sites[2].owner_asset_id,
            after.all_sites[2].owner_asset_id
        );
    }

    #[test]
    fn unit_projections_exclude_global_plan_ids_and_unrelated_ordinals() {
        let mut plan = node_ref_plan();
        let parent = unit_projection_bytes(PARENT_UNIT, PLAN_ID, &encode(&plan)).unwrap();
        let child = unit_projection_bytes(CHILD_UNIT, PLAN_ID, &encode(&plan)).unwrap();
        insert_unrelated_first_root(&mut plan);
        let changed_plan_id = [0x31; 32];
        plan["plan_id"] = json!(hex_id(&changed_plan_id));
        assert_eq!(
            parent,
            unit_projection_bytes(PARENT_UNIT, changed_plan_id, &encode(&plan)).unwrap()
        );
        assert_eq!(
            child,
            unit_projection_bytes(CHILD_UNIT, changed_plan_id, &encode(&plan)).unwrap()
        );
        let parsed: Value = serde_json::from_slice(&parent).unwrap();
        assert!(parsed.get("plan_id").is_none());
        assert!(
            parsed["components"][0]["selection"]["sites"][0]["candidate_id"]["scope"].is_string()
        );
    }

    #[test]
    fn unit_projection_tracks_all_occurrences_and_changed_child_consumers() {
        let mut plan = linked_plan();
        insert_unrelated_first_root(&mut plan);
        let unrelated_unit = [10; 32];
        let unrelated = unit_projection_bytes(unrelated_unit, PLAN_ID, &encode(&plan)).unwrap();
        let parent = unit_projection_bytes(PARENT_UNIT, PLAN_ID, &encode(&plan)).unwrap();
        let child = unit_projection_bytes(CHILD_UNIT, PLAN_ID, &encode(&plan)).unwrap();
        let projected: Value = serde_json::from_slice(&child).unwrap();
        assert_eq!(
            projected["components"][0]["selection"]["sites"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        plan["component_codegen"][2]["asset_id"] = json!("child_changed");
        plan["component_codegen"][2]["fragment_key"] = json!("child_changed");
        assert_ne!(
            parent,
            unit_projection_bytes(PARENT_UNIT, PLAN_ID, &encode(&plan)).unwrap()
        );
        assert_ne!(
            child,
            unit_projection_bytes(CHILD_UNIT, PLAN_ID, &encode(&plan)).unwrap()
        );
        assert_eq!(
            unrelated,
            unit_projection_bytes(unrelated_unit, PLAN_ID, &encode(&plan)).unwrap()
        );
    }

    #[test]
    fn helper_only_unit_projection_contains_validated_codegen_and_no_global_identity() {
        let mut plan = linked_helper_plan();
        let before = unit_projection_bytes(HELPER_UNIT, PLAN_ID, &encode(&plan)).unwrap();
        let projected: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(projected["components"], json!([]));
        assert_eq!(projected["helpers"].as_array().unwrap().len(), 1);
        let changed_plan_id = [0x31; 32];
        plan["plan_id"] = json!(hex_id(&changed_plan_id));
        assert_eq!(
            before,
            unit_projection_bytes(HELPER_UNIT, changed_plan_id, &encode(&plan)).unwrap()
        );
        plan["helper_codegen"][0]["body_ir_hash"] = json!(format!("sha256:{}", "00".repeat(32)));
        assert!(
            unit_projection_bytes(HELPER_UNIT, changed_plan_id, &encode(&plan))
                .unwrap_err()
                .contains("body_ir")
        );
    }

    #[test]
    fn runtime_identity_distinguishes_structural_paths_and_rejects_compact_collisions() {
        let plan = linked_plan();
        let root = plan.as_object().unwrap();
        let components = parse_components(root).unwrap();
        let mut sites = parse_component_sites(root, &components).unwrap();
        let mut repeated_child = sites[1].clone();
        repeated_child.candidate_id = 3;
        repeated_child.structural_path = vec![8];
        repeated_child.incoming_edge_id = Some(8);
        sites.push(repeated_child);
        let identities = runtime_site_identities(&sites).unwrap();
        assert_eq!(identities[&1].scope_id, identities[&3].scope_id);
        assert_ne!(identities[&1].site_id, identities[&3].site_id);
        assert_ne!(identities[&1].candidate_id, identities[&3].candidate_id);

        let mut candidates = BTreeMap::new();
        admit_runtime_candidate(&mut candidates, identities[&1]).unwrap();
        let collision = FinalRuntimeSiteIdentity {
            candidate_id: identities[&1].candidate_id,
            ..identities[&3]
        };
        assert!(admit_runtime_candidate(&mut candidates, collision)
            .unwrap_err()
            .contains("refusing to publish an ambiguous render plan"));
        assert!(
            admit_runtime_candidate(
                &mut BTreeMap::from([(identities[&1].candidate_id, identities[&1].site_id),]),
                identities[&1]
            )
            .is_err(),
            "duplicate full coordinates are also ambiguous"
        );
    }

    #[test]
    fn exact_definition_selects_standalone_and_every_linked_occurrence() {
        let selection = validate_component_plan_bytes(
            CHILD_UNIT,
            CHILD_RECORD,
            PLAN_ID,
            &encode(&linked_plan()),
        )
        .unwrap();
        assert_eq!(selection.plan_id, PLAN_ID);
        assert_eq!(selection.standalone_candidate_id, Some(2));
        assert_eq!(
            selection
                .sites
                .iter()
                .map(|site| (site.candidate_id, site.owner_role))
                .collect::<Vec<_>>(),
            vec![
                (
                    1,
                    FinalOwnerRole::Member {
                        owner_id: 0,
                        root_candidate_id: 0,
                    },
                ),
                (2, FinalOwnerRole::Root { owner_id: 1 }),
            ]
        );
        assert_eq!(
            selection
                .sites
                .iter()
                .map(|site| site.owner_strategy.clone())
                .collect::<Vec<_>>(),
            vec![
                Some(FinalOwnerActivationStrategy::Eager),
                Some(FinalOwnerActivationStrategy::Visible),
            ]
        );
        let expected_owner_0 = selection.all_sites[0].owner_asset_id.clone().unwrap();
        let expected_owner_1 = selection.all_sites[2].owner_asset_id.clone().unwrap();
        assert_eq!(
            selection.sites[0].owner_asset_id.as_deref(),
            Some(expected_owner_0.as_str())
        );
        assert_eq!(
            selection.sites[0].seed_key.as_deref(),
            Some(member_seed_key(selection.runtime_site(1).unwrap().site_id).as_str())
        );
        assert_eq!(
            selection.sites[1].owner_asset_id.as_deref(),
            Some(expected_owner_1.as_str())
        );
        assert_eq!(
            selection.sites[1].seed_key.as_deref(),
            Some(expected_owner_1.as_str())
        );
        assert_eq!(
            selection.sites[0].fragment_asset_id,
            selection.fragment_asset_id
        );
        assert!(selection.child_sites.is_empty());

        let parent = validate_component_plan_bytes(
            PARENT_UNIT,
            PARENT_RECORD,
            PLAN_ID,
            &encode(&linked_plan()),
        )
        .unwrap();
        assert_eq!(parent.child_sites.get(&(0, 7)), Some(&1));
        assert!(parent.live_props_by_child.is_empty());
        assert!(parent.live_callbacks_by_child.is_empty());
        assert_eq!(parent.codegen_by_candidate.len(), 3);
        assert_eq!(parent.codegen_by_candidate[&1].asset_id, "child_1234");
        assert_eq!(
            parent.codegen_by_candidate[&0].fragment_root,
            FinalComponentFragmentRoot::ExactOneElement
        );
    }

    #[test]
    fn root_ineligible_component_selects_only_its_exact_incoming_sites() {
        let mut plan = linked_plan();
        plan["components"][1]["root_eligible"] = json!(false);
        plan["component_sites"]
            .as_array_mut()
            .unwrap()
            .retain(|site| site["candidate_id"] != json!(2));
        plan["owners"]
            .as_array_mut()
            .unwrap()
            .retain(|owner| owner["owner_id"] != json!(1));
        plan["candidate_to_owner"]
            .as_array_mut()
            .unwrap()
            .retain(|mapping| mapping["candidate_id"] != json!(2));
        plan["standalone_roots"][1]["disposition"] = json!({ "kind": "structural_incoming_only" });

        let selection =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();

        assert_eq!(selection.standalone_candidate_id, None);
        assert_eq!(
            selection
                .sites
                .iter()
                .map(|site| site.candidate_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn context_required_component_selects_nested_sites_but_no_standalone_root() {
        let mut plan = linked_plan();
        plan["component_sites"]
            .as_array_mut()
            .unwrap()
            .retain(|site| site["candidate_id"] != json!(2));
        plan["owners"]
            .as_array_mut()
            .unwrap()
            .retain(|owner| owner["owner_id"] != json!(1));
        plan["candidate_to_owner"]
            .as_array_mut()
            .unwrap()
            .retain(|mapping| mapping["candidate_id"] != json!(2));
        plan["standalone_roots"][1]["disposition"] = json!({
            "kind": "context_required",
            "missing": [{
                "kind": "shared_state",
                "consumer_semantic_key_canonical": "bb",
                "structural_path": [],
                "reference_id": 0,
                "type_key": "acme.workbench",
                "schema_abi": format!("sha256:{}", "77".repeat(32)),
            }],
        });

        let selection =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .expect("provider-dependent component may retain exact nested occurrences");
        assert_eq!(selection.standalone_candidate_id, None);
        assert_eq!(
            selection
                .sites
                .iter()
                .map(|site| site.candidate_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn exact_shared_state_provider_topology_is_selected_per_host_and_consumer() {
        let mut plan = linked_plan();
        let schema = format!("sha256:{}", "77".repeat(32));
        plan["shared_state_providers"] = json!([{
            "host_candidate_id": 0,
            "provider_site_id": 3,
            "child_candidate_id": 1,
            "type_key": "acme.workbench",
            "schema_abi": schema,
        }]);
        plan["shared_state_requirements"] = json!([{
            "consumer_candidate_id": 1,
            "reference_id": 0,
            "provider_host_candidate_id": 0,
            "provider_site_id": 3,
            "type_key": "acme.workbench",
            "schema_abi": schema,
        }]);

        let parent =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .expect("provider host selection");
        let provider = &parent.shared_state_providers_by_host[&0][0];
        assert_eq!(provider.provider_site_id, 3);
        assert_eq!(provider.child_candidate_id, 1);
        assert!(parent.shared_state_requirements_by_consumer.is_empty());

        let child =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .expect("provider consumer selection");
        let requirement = &child.shared_state_requirements_by_consumer[&1][0];
        assert_eq!(requirement.provider_host_candidate_id, 0);
        assert_eq!(requirement.provider_site_id, 3);
        assert!(child.shared_state_providers_by_host.is_empty());

        plan["shared_state_requirements"][0]["provider_site_id"] = json!(4);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .expect_err("an exact acquisition cannot fall back to a nearby provider");
        assert!(error.contains("names absent provider occurrence 0:4"));
    }

    #[test]
    fn exact_node_ref_alias_is_selected_for_fragments_and_owner_codegen() {
        let plan = node_ref_plan();
        let parent =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .expect("NodeRef producer selection");
        assert_eq!(
            linked_fragment_node_ref_exports(&parent),
            vec![crate::NodeRefId(4)]
        );
        assert!(linked_fragment_node_ref_imports(&parent).is_empty());
        let owner_plan = linked_owner_module_plans(&parent)
            .expect("owner plan")
            .into_iter()
            .next()
            .expect("root owner");
        assert_eq!(
            linked_owner_node_ref_aliases(&parent, &owner_plan).expect("owner aliases"),
            vec![crate::LinkedOwnerNodeRefAlias {
                parent_candidate_id: runtime_candidate(&parent, 0),
                parent_node_ref_id: 4,
                child_candidate_id: runtime_candidate(&parent, 1),
                child_node_ref_id: 8,
            }]
        );

        let child =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .expect("NodeRef consumer selection");
        assert!(linked_fragment_node_ref_exports(&child).is_empty());
        assert_eq!(
            linked_fragment_node_ref_imports(&child),
            vec![crate::NodeRefId(8)]
        );
        assert_eq!(child.standalone_candidate_id, None);
    }

    #[test]
    fn node_ref_alias_evidence_and_owner_boundary_fail_closed() {
        let mut plan = node_ref_plan();
        plan["node_ref_aliases"][0]["child_node_ref_id"] = json!(9);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .expect_err("stale child producer coordinate must fail");
        assert!(error.contains("authenticated inbound NodeRef"));

        let mut plan = node_ref_plan();
        plan["owners"][0]["candidates"] = json!([0]);
        plan["owners"].as_array_mut().unwrap().push(json!({
            "owner_id": 1,
            "candidates": [1],
            "strategy": { "kind": "eager" },
        }));
        plan["candidate_to_owner"][1]["owner_id"] = json!(1);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .expect_err("owner mapping tamper must fail");
        assert!(error.contains("one final browser owner"), "{error}");
    }

    #[test]
    fn root_ineligible_component_rejects_a_standalone_site() {
        let mut plan = linked_plan();
        plan["components"][1]["root_eligible"] = json!(false);

        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();

        assert!(error.contains("root-ineligible component definition"));
        assert!(error.contains("has a standalone root site"));
    }

    #[test]
    fn owner_root_projects_the_complete_cross_definition_mount_plan() {
        let parent = validate_component_plan_bytes(
            PARENT_UNIT,
            PARENT_RECORD,
            PLAN_ID,
            &encode(&linked_plan()),
        )
        .unwrap();
        let plans = linked_owner_module_plans(&parent).unwrap();
        assert_eq!(plans.len(), 1);
        let owner = &plans[0];
        assert_eq!(owner.root_candidate_id, runtime_candidate(&parent, 0));
        assert_eq!(owner.mounts.len(), 2);
        assert_eq!(owner.mounts[0].fragment_asset_id, parent.fragment_asset_id);
        assert_eq!(
            owner.mounts[1].fragment_asset_id,
            parent.codegen_by_candidate[&1].fragment_key
        );
        assert_eq!(
            owner.mounts[1].parent_candidate_id.as_deref(),
            Some(runtime_candidate(&parent, 0).as_str())
        );
        assert!(crate::emit_linked_owner_module(owner)
            .unwrap()
            .contains("data-rsjs-linked-candidate"));

        let child = validate_component_plan_bytes(
            CHILD_UNIT,
            CHILD_RECORD,
            PLAN_ID,
            &encode(&linked_plan()),
        )
        .unwrap();
        let child_plans = linked_owner_module_plans(&child).unwrap();
        assert_eq!(child_plans.len(), 1);
        assert_eq!(
            child_plans[0].root_candidate_id,
            runtime_candidate(&child, 2)
        );
        assert_eq!(child_plans[0].mounts.len(), 1);
    }

    #[test]
    fn linked_owner_unions_reachable_query_and_mutation_features_from_every_fragment() {
        let mut plan = linked_plan();
        plan["component_codegen"][1]["uses_queries"] = json!(true);
        plan["component_codegen"][1]["uses_mutations"] = json!(true);

        let parent =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        let parent_owner = linked_owner_module_plans(&parent).unwrap().remove(0);
        assert!(
            parent_owner.uses_queries,
            "a query in a fused child must enable the enclosing owner query runtime"
        );
        assert!(
            parent_owner.uses_mutations,
            "a mutation in a fused child must enable the enclosing owner mutation runtime"
        );

        let child =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        let child_owner = linked_owner_module_plans(&child).unwrap().remove(0);
        assert!(
            child_owner.uses_queries,
            "the same fragment's standalone owner must retain its query feature"
        );
        assert!(
            child_owner.uses_mutations,
            "the same fragment's standalone owner must retain its mutation feature"
        );
    }

    #[test]
    fn owner_root_projects_live_signals_and_callbacks_to_producer_local_ids() {
        let mut plan = linked_plan();
        plan["component_codegen"][0]["signals"] = json!([{
            "producer_signal_id": 17,
            "canonical_value_id": 5,
        }]);
        plan["component_codegen"][0]["payload_handlers"] = json!([{
            "prelude_local_index": 19,
            "canonical_value_id": 11,
        }]);
        plan["component_codegen"][1]["inbound_fields"] = json!([{
            "field_id": 0,
            "producer_value_id": 0,
            "canonical_value_id": 6,
            "crossing_hook_id": 4,
            "callback_slot_id": null,
        }]);
        plan["component_codegen"][1]["inbound_targets"] = json!([
            {
                "prop_site_id": 8,
                "field_id": 0,
                "producer_signal_id": 23,
                "canonical_signal_value_id": 7,
            },
            {
                "prop_site_id": 13,
                "field_id": 0,
                "producer_signal_id": 23,
                "canonical_signal_value_id": 7,
            },
        ]);
        plan["component_codegen"][1]["signals"] = json!([{
            "producer_signal_id": 23,
            "canonical_value_id": 7,
        }]);
        plan["component_codegen"][1]["callback_slots"] = json!([{
            "producer_callback_slot_id": 29,
            "canonical_value_id": 12,
        }]);
        plan["component_inputs"] = json!({
            "live_props": [
                {
                    "parent_candidate_id": 0,
                    "edge_id": 7,
                    "child_candidate_id": 1,
                    "field_id": 0,
                    "crossing_hook_id": 4,
                    "parent_value_id": 5,
                    "child_value_id": 6,
                    "prop_site_id": 8,
                    "temporal": { "kind": "live_read" },
                },
                {
                    "parent_candidate_id": 0,
                    "edge_id": 7,
                    "child_candidate_id": 1,
                    "field_id": 0,
                    "crossing_hook_id": 4,
                    "parent_value_id": 5,
                    "child_value_id": 6,
                    "prop_site_id": 13,
                    "temporal": { "kind": "live_read" },
                },
            ],
            "live_callbacks": [{
                "parent_candidate_id": 0,
                "edge_id": 7,
                "child_candidate_id": 1,
                "field_id": 9,
                "crossing_hook_id": 10,
                "parent_value_id": 11,
                "child_value_id": 12,
                "callback_shape": "required",
                "dispatch": "resolve_current",
            }],
        });

        let parent =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        let owner = linked_owner_module_plans(&parent).unwrap().remove(0);
        assert_eq!(
            owner.signal_aliases,
            vec![crate::LinkedOwnerSignalAlias {
                parent_candidate_id: runtime_candidate(&parent, 0),
                parent_signal_id: 17,
                child_candidate_id: runtime_candidate(&parent, 1),
                child_signal_id: 23,
            }]
        );
        assert_eq!(
            owner.callback_edges,
            vec![crate::LinkedOwnerCallbackEdge {
                parent_candidate_id: runtime_candidate(&parent, 0),
                parent_payload_handler_id: 19,
                child_candidate_id: runtime_candidate(&parent, 1),
                child_callback_slot_id: 29,
            }]
        );
        assert_eq!(
            linked_fragment_signal_exports(&parent).unwrap(),
            vec![crate::SignalId(17)]
        );

        let emitted = crate::emit_linked_owner_module(&owner).unwrap();
        assert!(emitted.contains("const __rsjs_signal_1_23 = __rsjs_expose_0?.signals?.[17];"));
        assert!(emitted.contains("__rsjs_signals_1[23] = __rsjs_signal_1_23;"));
        assert!(emitted.contains(
            "__rsjs_callbacks_1[\"__callback_29\"] = (__payload) => __rsjs_payload_handler_1_29(__payload);"
        ));

        plan["component_codegen"][1]["signals"][0]["canonical_value_id"] = json!(99);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("does not resolve to its canonical value 7"),
            "{error}"
        );
    }

    #[test]
    fn owner_root_projects_seed_once_as_a_fresh_signal_seed() {
        let mut plan = linked_plan();
        plan["component_codegen"][0]["signals"] = json!([{
            "producer_signal_id": 17,
            "canonical_value_id": 5,
        }]);
        plan["component_codegen"][1]["inbound_fields"] = json!([{
            "field_id": 0,
            "producer_value_id": 0,
            "canonical_value_id": 6,
            "crossing_hook_id": 4,
            "callback_slot_id": null,
        }]);
        plan["component_codegen"][1]["inbound_targets"] = json!([{
            "prop_site_id": 8,
            "field_id": 0,
            "producer_signal_id": 23,
            "canonical_signal_value_id": 7,
        }]);
        plan["component_codegen"][1]["signals"] = json!([{
            "producer_signal_id": 23,
            "canonical_value_id": 7,
        }]);
        plan["component_inputs"]["live_props"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 4,
            "parent_value_id": 5,
            "child_value_id": 6,
            "prop_site_id": 8,
            "temporal": {
                "kind": "seed_once",
                "seed_site_id": 8,
            },
        }]);

        let selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        let owner = linked_owner_module_plans(&selection).unwrap().remove(0);
        assert!(owner.signal_aliases.is_empty());
        assert_eq!(
            owner.signal_seeds,
            vec![crate::LinkedOwnerSignalSeed {
                parent_candidate_id: runtime_candidate(&selection, 0),
                parent_signal_id: 17,
                child_candidate_id: runtime_candidate(&selection, 1),
                child_signal_id: 23,
            }]
        );
        assert_eq!(
            linked_fragment_signal_exports(&selection).unwrap(),
            vec![crate::SignalId(17)]
        );
    }

    #[test]
    fn component_codegen_is_total_exact_and_fail_closed() {
        let mut plan = linked_plan();
        plan["component_codegen"].as_array_mut().unwrap().pop();
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("has no component_codegen record"));

        let mut plan = linked_plan();
        plan["component_codegen"][0]["fragment_key"] = json!("foreign_fragment");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("does not match asset_id"));

        let mut plan = linked_plan();
        plan["component_codegen"][0]["signals"] = json!([
            { "producer_signal_id": 4, "canonical_value_id": 8 },
            { "producer_signal_id": 4, "canonical_value_id": 9 },
        ]);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("must be strictly ordered"));

        let mut plan = linked_plan();
        plan["component_codegen"][0]["fragment_root"]["kind"] = json!("guess");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("unsupported value `guess`"));

        let mut plan = linked_plan();
        plan["component_codegen"][0]
            .as_object_mut()
            .unwrap()
            .remove("uses_queries");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("component_codegen[0].uses_queries must be a boolean"),
            "{error}"
        );

        let mut plan = linked_plan();
        plan["component_codegen"][0]
            .as_object_mut()
            .unwrap()
            .remove("uses_mutations");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("component_codegen[0].uses_mutations must be a boolean"),
            "{error}"
        );
    }

    #[test]
    fn owner_activation_strategy_is_validated_fail_closed() {
        let mut plan = linked_plan();
        plan["owners"][0]["strategy"] = json!({
            "kind": "interaction",
            "event": "pointerdown",
        });
        let selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            selection.sites[0].owner_strategy,
            Some(FinalOwnerActivationStrategy::Interaction {
                event: Some("pointerdown".to_owned()),
            })
        );

        plan["owners"][0]["strategy"] = json!({
            "kind": "media",
            "query": "",
        });
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("strategy.query cannot be empty"));

        plan["owners"][0]["strategy"] = json!({
            "kind": "future-strategy",
        });
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("unsupported owner activation strategy"));
    }

    #[test]
    fn live_input_records_are_selected_by_exact_child_occurrence() {
        let mut plan = linked_plan();
        configure_live_codegen(&mut plan);
        plan["component_inputs"] = json!({
            "live_props": [{
                "parent_candidate_id": 0,
                "edge_id": 7,
                "child_candidate_id": 1,
                "field_id": 0,
                "crossing_hook_id": 4,
                "parent_value_id": 5,
                "child_value_id": 6,
                "prop_site_id": 8,
                "temporal": {
                    "kind": "seed_once",
                    "seed_site_id": 8,
                },
            }],
            "live_callbacks": [{
                "parent_candidate_id": 0,
                "edge_id": 7,
                "child_candidate_id": 1,
                "field_id": 1,
                "crossing_hook_id": 10,
                "parent_value_id": 11,
                "child_value_id": 12,
                "callback_shape": "required",
                "dispatch": "resolve_current",
            }],
        });

        let child =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            child.live_props_by_child.get(&1),
            Some(&vec![FinalLivePropUse {
                parent_candidate_id: 0,
                edge_id: 7,
                child_candidate_id: 1,
                field_id: 0,
                crossing_hook_id: 4,
                parent_value_id: 5,
                child_value_id: 6,
                prop_site_id: 8,
                temporal: FinalTemporalPropMode::SeedOnce { seed_site_id: 8 },
            }])
        );
        assert_eq!(
            child.live_callbacks_by_child.get(&1),
            Some(&vec![FinalLiveCallback {
                parent_candidate_id: 0,
                edge_id: 7,
                child_candidate_id: 1,
                field_id: 1,
                crossing_hook_id: 10,
                parent_value_id: 11,
                child_value_id: 12,
                shape: FinalCallbackShape::Required,
            }])
        );
        assert!(!child.live_props_by_child.contains_key(&2));

        plan["component_inputs"]["live_callbacks"][0]["callback_shape"] = json!("optional");
        let optional_child =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            optional_child.live_callbacks_by_child[&1][0].shape,
            FinalCallbackShape::Optional
        );
    }

    #[test]
    fn live_input_records_fail_closed_on_stale_edges_or_temporal_semantics() {
        let mut plan = linked_plan();
        plan["component_inputs"]["live_props"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 2,
            "field_id": 0,
            "crossing_hook_id": 0,
            "parent_value_id": 0,
            "child_value_id": 0,
            "prop_site_id": 0,
            "temporal": { "kind": "live_read" },
        }]);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("does not match the exact component site edge"));

        let mut plan = linked_plan();
        plan["component_inputs"]["live_props"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 0,
            "parent_value_id": 0,
            "child_value_id": 0,
            "prop_site_id": 1,
            "temporal": { "kind": "seed_once", "seed_site_id": 2 },
        }]);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("SeedOnce names prop site 2"));

        let mut plan = linked_plan();
        plan["component_inputs"]["live_callbacks"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 0,
            "parent_value_id": 0,
            "dispatch": "snapshot",
        }]);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("must resolve the current edge"));

        let mut plan = linked_plan();
        plan["component_inputs"]["live_callbacks"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 0,
            "parent_value_id": 0,
            "child_value_id": 0,
            "dispatch": "resolve_current",
        }]);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("field `callback_shape` must be a string"));

        let mut plan = linked_plan();
        plan["component_inputs"]["live_callbacks"] = json!([{
            "parent_candidate_id": 0,
            "edge_id": 7,
            "child_candidate_id": 1,
            "field_id": 0,
            "crossing_hook_id": 0,
            "parent_value_id": 0,
            "child_value_id": 0,
            "callback_shape": "maybe",
            "dispatch": "resolve_current",
        }]);
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("unsupported callback shape `maybe`"));
    }

    #[test]
    fn another_unit_or_record_cannot_consume_the_plan_entry() {
        let error =
            validate_component_plan_bytes(PARENT_UNIT, [3; 32], PLAN_ID, &encode(&linked_plan()))
                .unwrap_err();
        assert!(error.contains("has no component definition"));
    }

    #[test]
    fn stale_schema_fails_closed_before_feature_parsing() {
        let mut plan = linked_plan();
        plan["schema_version"] = json!(10);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error
                .contains("unsupported final RSJS plan `schema_version` value `10`; expected `15`"),
            "{error}"
        );
    }

    #[test]
    fn conditional_component_lifecycle_reaches_final_owner_and_fragment_exports() {
        let plan = conditional_plan("else");
        let selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            selection.all_sites[1].incoming_lifecycle,
            Some(FinalComponentLifecycle {
                control: 3,
                predicate_value: 5,
                predicate_origin: 7,
                branch: FinalComponentLifecycleBranch::Else,
            })
        );
        assert_eq!(
            selection.all_sites[1].reactive_lifecycle_root_candidate_id,
            Some(1)
        );

        let owner = linked_owner_module_plans(&selection).unwrap().remove(0);
        assert_eq!(owner.mounts[0].lifecycle_root_candidate_id, None);
        assert_eq!(
            owner.mounts[1].lifecycle_root_candidate_id.as_deref(),
            Some(runtime_candidate(&selection, 1).as_str())
        );
        assert_eq!(
            owner.lifecycles,
            vec![crate::LinkedOwnerLifecycle {
                root_candidate_id: runtime_candidate(&selection, 1),
                parent_candidate_id: runtime_candidate(&selection, 0),
                control_id: 3,
                branch: crate::LinkedOwnerLifecycleBranch::Else,
                candidates: vec![runtime_candidate(&selection, 1)],
            }]
        );
        assert_eq!(
            linked_fragment_lifecycle_exports(&selection).unwrap(),
            vec![crate::emit_js::FragmentLifecycleControlExport {
                control_id: 3,
                producer_expression_id: crate::ExprId(13),
            }]
        );

        let mut plan = linked_plan();
        plan["components"][0]["component_edges"][0]["lifecycle"]["extra"] = json!(true);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("invalid fields") && error.contains("extra"),
            "{error}"
        );
    }

    #[test]
    fn static_descendants_inherit_the_conditional_lifecycle_group() {
        let plan = conditional_plan_with_static_descendant();
        let selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();
        assert_eq!(
            selection.all_sites[2].reactive_lifecycle_root_candidate_id,
            Some(1)
        );
        assert_eq!(selection.all_sites[2].incoming_lifecycle, None);

        let owner = linked_owner_module_plans(&selection).unwrap().remove(0);
        assert_eq!(
            owner.mounts[2].lifecycle_root_candidate_id.as_deref(),
            Some(runtime_candidate(&selection, 1).as_str())
        );
        assert_eq!(
            owner.lifecycles[0].candidates,
            vec![
                runtime_candidate(&selection, 1),
                runtime_candidate(&selection, 2)
            ]
        );
    }

    #[test]
    fn conditional_lifecycle_site_metadata_fails_closed_on_mismatch() {
        let mut plan = conditional_plan("then");
        plan["component_sites"][1]["incoming_lifecycle"]["branch"] = json!("else");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("incoming lifecycle disagrees"), "{error}");

        let mut plan = conditional_plan("then");
        plan["component_sites"][1]["reactive_lifecycle_root_candidate_id"] = json!(0);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("lifecycle root Some(0)") && error.contains("requires Some(1)"),
            "{error}"
        );
    }

    #[test]
    fn conditional_lifecycle_groups_cannot_cross_owners_or_nest() {
        let mut plan = conditional_plan("then");
        plan["owners"] = json!([
            {
                "owner_id": 0,
                "candidates": [0],
                "strategy": { "kind": "eager" },
            },
            {
                "owner_id": 1,
                "candidates": [1],
                "strategy": { "kind": "eager" },
            },
            {
                "owner_id": 2,
                "candidates": [2],
                "strategy": { "kind": "visible" },
            },
        ]);
        plan["candidate_to_owner"] = json!([
            { "candidate_id": 0, "owner_id": 0 },
            { "candidate_id": 1, "owner_id": 1 },
            { "candidate_id": 2, "owner_id": 2 },
        ]);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("crosses owner boundaries"), "{error}");

        let mut plan = conditional_plan_with_static_descendant();
        let nested = json!({
            "kind": "conditional_single",
            "control": 4,
            "predicate_value": 8,
            "predicate_origin": 9,
            "branch": "else",
        });
        plan["components"][1]["component_edges"][0]["lifecycle"] = nested.clone();
        plan["component_sites"][2]["incoming_lifecycle"] = nested;
        plan["component_sites"][2]["reactive_lifecycle_root_candidate_id"] = json!(2);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("nested conditional lifecycle")
                && error.contains("beneath lifecycle root 1"),
            "{error}"
        );
    }

    #[test]
    fn lifecycle_predicate_export_requires_an_exact_producer_expression() {
        let mut plan = conditional_plan("then");
        plan["component_codegen"][0]["expressions"] = json!([]);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(
            error.contains("lifecycle predicate expression value 5")
                && error.contains("does not resolve to exactly one producer-local ID"),
            "{error}"
        );
    }

    #[test]
    fn repeated_parent_occurrences_deduplicate_remapped_lifecycle_coordinates() {
        let plan = conditional_plan("then");
        let mut selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();

        let mut repeated_parent = selection.sites[0].clone();
        repeated_parent.candidate_id = 10;
        selection.sites.push(repeated_parent);

        let mut repeated_codegen = selection.codegen_by_candidate[&0].clone();
        repeated_codegen.expressions[0].canonical_value_id = 105;
        selection.codegen_by_candidate.insert(10, repeated_codegen);

        let mut repeated_child = selection.all_sites[1].clone();
        repeated_child.candidate_id = 11;
        repeated_child.parent_candidate_id = Some(10);
        repeated_child.incoming_lifecycle = Some(FinalComponentLifecycle {
            control: 3,
            predicate_value: 105,
            predicate_origin: 107,
            branch: FinalComponentLifecycleBranch::Then,
        });
        repeated_child.reactive_lifecycle_root_candidate_id = Some(11);
        selection.all_sites.push(repeated_child);

        assert_eq!(
            linked_fragment_lifecycle_exports(&selection).unwrap(),
            vec![crate::emit_js::FragmentLifecycleControlExport {
                control_id: 3,
                producer_expression_id: crate::ExprId(13),
            }]
        );
    }

    #[test]
    fn component_sites_must_be_dense_and_exactly_match_parent_edges() {
        let mut plan = linked_plan();
        plan["component_sites"][1]["candidate_id"] = json!(4);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("unique and dense"));

        let mut plan = linked_plan();
        plan["component_sites"][1]["definition"] = definition(PARENT_UNIT, PARENT_RECORD);
        plan["component_sites"][1]["component_semantic_key_canonical"] = json!("aa");
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("disagrees with parent"));
    }

    #[test]
    fn duplicate_or_missing_child_site_mappings_fail_closed() {
        let mut plan = linked_plan();
        plan["component_sites"].as_array_mut().unwrap().insert(
            2,
            json!({
                "candidate_id": 2,
                "root_semantic_key_canonical": "aa",
                "component_semantic_key_canonical": "bb",
                "definition": definition(CHILD_UNIT, CHILD_RECORD),
                "parent_candidate_id": 0,
                "incoming_edge_id": 7,
                "structural_path": [7],
                "incoming_lifecycle": { "kind": "static_single" },
                "reactive_lifecycle_root_candidate_id": null,
            }),
        );
        plan["component_sites"][3]["candidate_id"] = json!(3);
        for entry in plan["candidate_to_owner"].as_array_mut().unwrap() {
            let candidate = entry["candidate_id"].as_u64().unwrap();
            if candidate >= 2 {
                entry["candidate_id"] = json!(candidate + 1);
            }
        }
        plan["owners"][1]["candidates"] = json!([3]);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("maps to more than one child site"));

        let mut plan = linked_plan();
        plan["component_sites"].as_array_mut().unwrap().remove(1);
        plan["component_sites"][1]["candidate_id"] = json!(1);
        plan["owners"] = json!([]);
        plan["candidate_to_owner"] = json!([]);
        let error =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("missing child mapping"));
    }

    #[test]
    fn owner_reverse_map_must_exactly_match_owner_membership() {
        let mut plan = linked_plan();
        plan["candidate_to_owner"].as_array_mut().unwrap().pop();
        let error =
            validate_component_plan_bytes(CHILD_UNIT, CHILD_RECORD, PLAN_ID, &encode(&plan))
                .unwrap_err();
        assert!(error.contains("does not exactly match"));
    }

    #[test]
    fn helper_validation_has_no_component_site_dependency() {
        let signature = json!({ "params": [], "return_type": "u32" });
        let body: Value =
            serde_json::from_str(&crate::client_expr_json(&crate::ClientExpr::int(1))).unwrap();
        let signature_hash =
            legacy_json_hash(HELPER_SIGNATURE_JSON_HASH_DOMAIN, &signature).unwrap();
        let body_hash = legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &body).unwrap();
        let source_ir_hash = [0x77; 32];
        let plan = json!({
            "format": PLAN_FORMAT,
            "schema_version": PLAN_SCHEMA_VERSION,
            "plan_id": "22".repeat(32),
            "helpers": [{
                "definition": definition([8; 32], [9; 32]),
                "semantic_key_canonical": "aa",
                "signature_hash": format!("sha256:{}", hex_id(&signature_hash)),
                "client_ir_hash": format!("sha256:{}", hex_id(&source_ir_hash)),
                "helper_edges": [],
                "transitive_closure": ["aa"],
            }],
            "helper_codegen": [{
                "definition": definition([8; 32], [9; 32]),
                "semantic_key_canonical": "aa",
                "signature": signature,
                "source_ir_hash": format!("sha256:{}", hex_id(&source_ir_hash)),
                "body_ir_hash": format!("sha256:{}", hex_id(&body_hash)),
                "body_ir": body,
            }],
            "component_sites": "intentionally absent from helper semantics",
        });
        let selection =
            validate_helper_plan_bytes([8; 32], [9; 32], PLAN_ID, &encode(&plan)).unwrap();
        assert_eq!(selection.plan_id, PLAN_ID);
    }

    #[test]
    fn helper_codegen_source_edge_join_selects_the_exact_final_subset_and_fails_closed() {
        let plan = linked_helper_plan();
        let mut selection =
            validate_component_plan_bytes(PARENT_UNIT, PARENT_RECORD, PLAN_ID, &encode(&plan))
                .unwrap();

        // Descriptor linkage is a potential-root superset. Add a second valid
        // relocation and definition, then prove exact final roots neither
        // require nor emit it.
        let unused_key = crate::LinkedHelperKey::from_canonical_bytes(vec![0xee]).unwrap();
        let mut unused_definition = selection.helper_linkage.definitions[0].clone();
        unused_definition.key = unused_key.clone();
        selection.helper_linkage.definitions.push(unused_definition);
        selection
            .helper_linkage
            .roots_by_source
            .insert([0x78; 32], FinalRootHelperBinding { target: unused_key });

        let program = linked_helper_program(
            &selection,
            [(crate::abi::HelperCallEdgeId::new(42), [0x77; 32])],
        )
        .unwrap();
        assert_eq!(program.len(), 1);
        let empty_program = linked_helper_program(&selection, []).unwrap();
        assert!(empty_program.is_empty());

        let error = linked_helper_program(
            &selection,
            [(crate::abi::HelperCallEdgeId::new(42), [0x76; 32])],
        )
        .unwrap_err();
        assert!(error.contains("no exact root binding"), "{error}");

        let mut missing_codegen = linked_helper_plan();
        missing_codegen["helper_codegen"] = json!([]);
        let error = validate_component_plan_bytes(
            PARENT_UNIT,
            PARENT_RECORD,
            PLAN_ID,
            &encode(&missing_codegen),
        )
        .unwrap_err();
        assert!(error.contains("has no helper_codegen record"), "{error}");

        let mut stale_body = linked_helper_plan();
        stale_body["helper_codegen"][0]["body_ir"] = json!({ "kind": "unit", "extra": true });
        let error = validate_component_plan_bytes(
            PARENT_UNIT,
            PARENT_RECORD,
            PLAN_ID,
            &encode(&stale_body),
        )
        .unwrap_err();
        assert!(error.contains("body hash"), "{error}");

        let mut stale_closure = linked_helper_plan();
        stale_closure["components"][0]["helper_closure"] = json!([]);
        let error = validate_component_plan_bytes(
            PARENT_UNIT,
            PARENT_RECORD,
            PLAN_ID,
            &encode(&stale_closure),
        )
        .unwrap_err();
        assert!(error.contains("helper_closure"), "{error}");
    }
}
