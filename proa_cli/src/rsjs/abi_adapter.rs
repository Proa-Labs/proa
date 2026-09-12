// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Fail-closed bridge from the legacy embedded RSJS descriptors to the
//! canonical compiler ABI.
//!
//! The object transport/linker has already done the important identity work by
//! the time values reach this module: every definition and outgoing edge is
//! selected through a rustc relocation. This bridge therefore never treats
//! `struct_name`, `self_ty`, `authored_path`, `asset_id`, or a last path segment
//! as identity authority.
//!
//! Component schema v9 carries dense latent inbound input metadata. Transport
//! payload v9 joins each client-visible value/callback field to a rustc-checked
//! field projection and exact defining-component relocation. A direct
//! component relocation ID also commits its complete behavior-bearing edge
//! tuple: boundary, site, lifecycle, struct-rest shape, and every ordered field
//! binding. The bridge still rejects graph behavior it cannot represent
//! losslessly; it never invents a nominal type identity or serialization codec
//! from source tokens.
//!
//! Standalone helper payload v1 contains the concrete parameter/result
//! signature. The adapter hashes that exact contract into the canonical helper
//! descriptor, then preserves every direct helper call through its
//! relocation-resolved witness.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rsjs_ir::abi::{
    AbiSupport, AbiValidationError, AbiVersion, CapabilitySet, ComponentBindingSource,
    ComponentBoundary, ComponentCallbackShape, ComponentDescriptor, ComponentEdgeId,
    ComponentFieldBinding, ComponentFieldCodec, ComponentFieldId, ComponentFieldRole,
    ComponentFieldSchema, ComponentLifecycleBranch, ComponentLifecycleControl,
    ComponentSemanticKey, CrossingHookId, DependencyEdge, DependencyEdgeId, DependencyEdgeKind,
    DependencyEndpoint, DescriptorBody, DescriptorEnvelope, DescriptorHeader, DescriptorKind,
    DirectComponentEdge, EffectKind, HelperCallEdgeId, HelperDescriptor, HelperSemanticKey,
    HydrationConstraint, KeyedCollectionSiteId, LocalControlRegionId, LocalEffectId, LocalValueId,
    OriginId, OriginKind, OriginPermission, ProducerVersion, PropSiteId, PropUse, ProviderSiteId,
    RegionSiteId, RenderKind, RuntimeCompatibility, SemanticHash, SharedStateProviderSite,
    SharedStateRefId, SharedStateReference, SpanCardinality, StaticSlotCapability,
    TemporalPropMode, VersionRange, WitnessRecordId, CLIENT_IR_SCHEMA_VERSION,
    HELPER_CLIENT_IR_JSON_HASH_DOMAIN, HELPER_CODEGEN_IR_JSON_HASH_DOMAIN,
    HELPER_SIGNATURE_JSON_HASH_DOMAIN,
};
use rsjs_ir::abi::{CanonicalEncode, CanonicalEncoder};
use rsjs_ir::{
    client_block_json, client_expr_json, collect_client_block_external_dependencies,
    parse_client_block_json, parse_client_block_json_with_options,
    parse_client_expr_json_with_options, parse_client_helper_type, validate_client_helper_block,
    validate_client_helper_expr, walk_client_block, walk_client_expr, ClientBlock,
    ClientBlockExternalDependencies, ClientExpr, ClientFunctionRef, ClientHelperTypePosition,
    ClientJsonParseOptions, ClientModuleCall,
};
use rsjs_transport::{
    canonical_json_bytes, component_field_record_id, component_helper_source_edge_id,
    component_lifecycle_predicate_graph_commitment, component_record_id, component_source_edge_id,
    decode_shared_state_schema_abi, helper_direct_call_source_edge_id,
    shared_state_provider_source_edge_id, shared_state_schema_abi_hash,
    SharedStateSchemaAbiResolution, SharedStateSchemaUnresolvedReason, TransportCallbackShape,
    TransportComponentFieldRole, TransportComponentOriginPermission, TransportComponentRenderKind,
    TransportDirectComponentBinding, TransportDirectComponentBoundary,
    TransportDirectComponentBranch, TransportDirectComponentEdge, TransportDirectComponentField,
    TransportDirectComponentLifecycle, TransportDirectLiveCallbackForm,
    TransportDirectLiveValueSource, TransportDirectStaticCallbackForm, TransportRecordId,
    TransportSharedStateProviderSite, SHARED_STATE_RUNTIME_ABI_VERSION,
};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::rsjs::legacy_graph::{
    collect_handler_browser_operations, parse_legacy_client_graph_with_inbound_signals,
    LegacyAttributeBinding, LegacyBrowserConsumer, LegacyBrowserOperation,
    LegacyBrowserOperationKind, LegacyClientGraph, LegacyClientOperation,
    LegacyClientOperationConsumer, LegacyClientOperationKind, LegacyConditionalBinding,
    LegacyInputBinding, LegacyKeyedCollection, LegacyKeyedCollectionSource, LegacyMutation,
    LegacyQuery, LegacySignal, LegacyTextBinding,
};
use crate::rsjs::link::{
    shared_state_acquisition_source_edge_id, LinkedComponentEdge, LinkedHelperEdge,
    LinkedSemanticIdentity, LinkedSharedStateDefinition, LinkedTransportComponent,
    LinkedTransportHelper, TransportDefinitionId, TransportLinkPlan,
};

const COMPONENT_PAYLOAD_FORMAT: &str = "rsjs.component_transport_payload";
const COMPONENT_PAYLOAD_SCHEMA: u64 = 9;
const COMPONENT_DESCRIPTOR_FORMAT: &str = "rsjs.component_descriptor";
const COMPONENT_DESCRIPTOR_SCHEMA: u64 = 9;
const CLIENT_IR_SCHEMA: u64 = CLIENT_IR_SCHEMA_VERSION as u64;
const HELPER_PAYLOAD_FORMAT: &str = "rsjs.standalone_helper_descriptor";
const HELPER_PAYLOAD_SCHEMA: u64 = 1;
const HELPER_HIR_SCHEMA: u64 = 1;
const HELPER_IR_SCHEMA: u64 = CLIENT_IR_SCHEMA_VERSION as u64;
const HELPER_CODEC_ABI: u64 = 1;
const COMPONENT_CLIENT_IR_HASH_DOMAIN: &str = "rsjs.legacy-component-client-program.v3";
const PAYLOAD_HANDLER_CLIENT_IR_HASH_DOMAIN: &str = "rsjs.payload-handler-client-block.v3";

/// Normalized semantic identity obtained from an authoritative defining-item
/// resolver.
///
/// Callers must populate this from rustc/object metadata reached through the
/// defining relocation. Legacy token paths are diagnostic data and are never a
/// valid source for this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSemanticPath {
    /// Declared Cargo package namespace.
    pub package: String,
    /// Normalized Rust crate name.
    pub crate_name: String,
    /// Rust-resolved module segments containing the item.
    pub module_path: Vec<String>,
    /// Rust-resolved defining item name.
    pub item: String,
}

impl ResolvedSemanticPath {
    /// Construct a normalized path. Canonical namespace validation happens
    /// when the path is converted into a component/helper key.
    pub fn new<I, S>(
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            package: package.into(),
            crate_name: crate_name.into(),
            module_path: module_path.into_iter().map(Into::into).collect(),
            item: item.into(),
        }
    }

    fn component_key(&self) -> Result<ComponentSemanticKey, AbiValidationError> {
        ComponentSemanticKey::v1(
            self.package.clone(),
            self.crate_name.clone(),
            self.module_path.clone(),
            self.item.clone(),
        )
    }

    fn helper_key(&self) -> Result<HelperSemanticKey, AbiValidationError> {
        HelperSemanticKey::v1(
            self.package.clone(),
            self.crate_name.clone(),
            self.module_path.clone(),
            self.item.clone(),
        )
    }
}

impl From<&LinkedSemanticIdentity> for ResolvedSemanticPath {
    fn from(identity: &LinkedSemanticIdentity) -> Self {
        Self {
            package: identity.package.clone(),
            crate_name: identity.crate_name.clone(),
            module_path: identity.module_path.clone(),
            item: identity.item.clone(),
        }
    }
}

/// Explicit migration policy and relocation-authoritative semantic identities.
#[derive(Clone, Debug)]
pub struct AbiAdapterContext {
    producer: ProducerVersion,
    runtime_protocol: AbiVersion,
    support: AbiSupport,
    component_identities: BTreeMap<TransportDefinitionId, ResolvedSemanticPath>,
    helper_identities: BTreeMap<TransportDefinitionId, ResolvedSemanticPath>,
    shared_state_definitions: BTreeMap<TransportDefinitionId, ResolvedSharedStateDefinition>,
}

#[derive(Clone, Debug)]
struct ResolvedSharedStateDefinition {
    type_key: String,
    schema_abi_hash: TransportRecordId,
    schema_abi: Vec<u8>,
}

impl AbiAdapterContext {
    /// Construct a bridge targeting the complete canonical ABI v1 support
    /// matrix.
    ///
    /// Legacy descriptors did not carry a runtime range. The bridge uses the
    /// caller-selected protocol as an exact range and declares no runtime
    /// capabilities for the accepted static-component subset.
    pub fn v1(producer: ProducerVersion, runtime_protocol: AbiVersion) -> Self {
        Self {
            producer,
            runtime_protocol,
            support: AbiSupport::v1(runtime_protocol),
            component_identities: BTreeMap::new(),
            helper_identities: BTreeMap::new(),
            shared_state_definitions: BTreeMap::new(),
        }
    }

    /// Construct a v1 adapter context from the identities carried by one exact
    /// relocation-linked transport plan.
    ///
    /// No authored type/helper path is consulted. Each entry was joined to its
    /// definition by `(CargoUnitId, definition_record_id)` during exact
    /// object/rlib extraction.
    pub fn from_transport_plan(
        producer: ProducerVersion,
        runtime_protocol: AbiVersion,
        plan: &TransportLinkPlan,
    ) -> Result<Self, AbiAdapterError> {
        let mut context = Self::v1(producer, runtime_protocol);
        for component in &plan.components {
            context.insert_component_identity(
                component.definition,
                ResolvedSemanticPath::from(&component.identity),
            );
        }
        for helper in &plan.helpers {
            context.insert_helper_identity(
                helper.definition,
                ResolvedSemanticPath::from(&helper.identity),
            );
        }
        for definition in &plan.shared_state_definitions {
            context.insert_shared_state_definition(definition)?;
        }
        Ok(context)
    }

    fn insert_shared_state_definition(
        &mut self,
        definition: &LinkedSharedStateDefinition,
    ) -> Result<(), AbiAdapterError> {
        let resolved = ResolvedSharedStateDefinition {
            type_key: definition.type_key.clone(),
            schema_abi_hash: definition.schema_abi_hash,
            schema_abi: definition.schema_abi.clone(),
        };
        if self
            .shared_state_definitions
            .insert(definition.definition, resolved)
            .is_some()
        {
            return Err(invalid(
                "SharedState definition",
                definition.definition,
                "the exact definition record appears more than once in the transport plan",
            ));
        }
        Ok(())
    }

    /// Register the semantic path selected for an exact component definition.
    pub fn insert_component_identity(
        &mut self,
        definition: TransportDefinitionId,
        identity: ResolvedSemanticPath,
    ) -> Option<ResolvedSemanticPath> {
        self.component_identities.insert(definition, identity)
    }

    /// Register the semantic path selected for an exact helper definition.
    pub fn insert_helper_identity(
        &mut self,
        definition: TransportDefinitionId,
        identity: ResolvedSemanticPath,
    ) -> Option<ResolvedSemanticPath> {
        self.helper_identities.insert(definition, identity)
    }

    /// Builder-style component identity registration.
    pub fn with_component_identity(
        mut self,
        definition: TransportDefinitionId,
        identity: ResolvedSemanticPath,
    ) -> Self {
        self.insert_component_identity(definition, identity);
        self
    }

    /// Builder-style helper identity registration.
    pub fn with_helper_identity(
        mut self,
        definition: TransportDefinitionId,
        identity: ResolvedSemanticPath,
    ) -> Self {
        self.insert_helper_identity(definition, identity);
        self
    }
}

/// Exact build-local relocation selected for one canonical witness ID.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolvedTransportWitness {
    /// The descriptor's own defining relocation record.
    Definition(TransportDefinitionId),
    /// A direct component source edge and the definition rustc selected.
    ComponentEdge {
        /// Exact parent-local source edge record.
        source_edge: TransportRecordId,
        /// Exact child definition reached by the relocation.
        target: TransportDefinitionId,
    },
    /// A client-visible input field record and its defining component.
    ComponentField {
        /// Deterministic component-field object record.
        field_record: TransportRecordId,
        /// Exact defining component reached by the relocation.
        target: TransportDefinitionId,
    },
    /// A helper call source edge and the definition rustc selected.
    HelperEdge {
        /// Exact caller-local source edge record.
        source_edge: TransportRecordId,
        /// Exact helper definition reached by the relocation.
        target: TransportDefinitionId,
    },
}

/// Producer view-root contract available to final component-fragment codegen.
///
/// The legacy descriptor distinguishes an element root from text and opaque
/// one-node roots, while the canonical span ABI intentionally records only DOM
/// cardinality. Preserve that stronger producer proof instead of asking the
/// final macro to infer mountability from an asset key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentFragmentRoot {
    /// The fragment has exactly one element root and can receive a stable mount
    /// marker directly on that element.
    ExactOneElement,
    /// The producer did not prove the first owner-module fragment contract.
    Unsupported,
}

impl CanonicalEncode for ComponentFragmentRoot {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.variant(
            "ComponentFragmentRoot",
            match self {
                Self::ExactOneElement => 1,
                Self::Unsupported => 2,
            },
            0,
        );
    }
}

/// Producer-local identity for one canonical inbound component value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenInboundField {
    /// Dense producer component-field ID.
    pub field_id: ComponentFieldId,
    /// Producer descriptor value ID before canonical allocation.
    pub producer_value_id: u32,
    /// Canonical value allocated to this inbound field.
    pub canonical_value: LocalValueId,
    /// Producer/canonical crossing hook selected for the field.
    pub crossing_hook_id: CrossingHookId,
    /// Producer callback slot for callback fields.
    pub callback_slot_id: Option<u32>,
}

impl CanonicalEncode for ComponentCodegenInboundField {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenInboundField", 5);
        encoder.field(1, &self.field_id);
        encoder.field(2, &self.producer_value_id);
        encoder.field(3, &self.canonical_value);
        encoder.field(4, &self.crossing_hook_id);
        encoder.field(5, &self.callback_slot_id);
    }
}

/// Producer signal targeted by one canonical signal-backed inbound use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenInboundTarget {
    /// Canonical prop-use site.
    pub prop_site_id: PropSiteId,
    /// Component field supplying the seed.
    pub field_id: ComponentFieldId,
    /// Producer-local signal ID named by the descriptor target.
    pub producer_signal_id: u32,
    /// Canonical value allocated to that producer signal.
    pub canonical_signal_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenInboundTarget {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenInboundTarget", 4);
        encoder.field(1, &self.prop_site_id);
        encoder.field(2, &self.field_id);
        encoder.field(3, &self.producer_signal_id);
        encoder.field(4, &self.canonical_signal_value);
    }
}

/// Producer signal to canonical value relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenSignal {
    /// Producer-local legacy signal ID.
    pub producer_signal_id: u32,
    /// Canonical value allocated to that signal.
    pub canonical_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenSignal {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenSignal", 2);
        encoder.field(1, &self.producer_signal_id);
        encoder.field(2, &self.canonical_value);
    }
}

/// Producer arena expression to canonical value relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenExpression {
    /// Producer-local legacy expression ID.
    pub producer_expression_id: u32,
    /// Canonical value allocated to that expression.
    pub canonical_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenExpression {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenExpression", 2);
        encoder.field(1, &self.producer_expression_id);
        encoder.field(2, &self.canonical_value);
    }
}

/// Transported parent callback body to canonical value relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenPayloadHandler {
    /// Producer render-prelude local index used by direct component bindings.
    pub prelude_local_index: u32,
    /// Canonical value allocated to the transported callback body.
    pub canonical_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenPayloadHandler {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenPayloadHandler", 2);
        encoder.field(1, &self.prelude_local_index);
        encoder.field(2, &self.canonical_value);
    }
}

/// Producer callback slot joined to its canonical inbound callback value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenCallbackSlot {
    /// Producer-local callback slot ID used by emitted fragment Client IR.
    pub producer_callback_slot_id: u32,
    /// Canonical inbound callback value resolved at invocation time.
    pub canonical_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenCallbackSlot {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenCallbackSlot", 2);
        encoder.field(1, &self.producer_callback_slot_id);
        encoder.field(2, &self.canonical_value);
    }
}

/// Rust-checked DOM family for the unique terminal binding of an inbound
/// `NodeRef<T>` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentNodeRefBindTarget {
    /// `NodeRef<Element>`.
    Element,
    /// `NodeRef<HtmlElement>`.
    HtmlElement,
    /// `NodeRef<HtmlInputElement>`.
    Input,
    /// `NodeRef<HtmlTextAreaElement>`.
    TextArea,
    /// `NodeRef<HtmlSelectElement>`.
    Select,
    /// `NodeRef<HtmlFormElement>`.
    Form,
    /// `NodeRef<HtmlDialogElement>`.
    Dialog,
}

impl CanonicalEncode for ComponentNodeRefBindTarget {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let discriminant = match self {
            Self::Element => 0,
            Self::HtmlElement => 1,
            Self::Input => 2,
            Self::TextArea => 3,
            Self::Select => 4,
            Self::Form => 5,
            Self::Dialog => 6,
        };
        encoder.variant("ComponentNodeRefBindTarget", discriminant, 0);
    }
}

/// Producer identity for one required inbound `NodeRef<T>` field.
///
/// The field witness proves the actual Rust `NodeRef<T>` projection. This
/// record keeps the producer-local NodeRef ID and its optional, rustc-checked
/// terminal element family available to occurrence-specific final linking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenInboundNodeRef {
    /// Canonical component field.
    pub field_id: ComponentFieldId,
    /// Producer-local NodeRef declaration represented by the field.
    pub producer_node_ref_id: u32,
    /// Unique terminal DOM family when this definition binds the handle.
    ///
    /// `None` means the definition forwards the exact handle through one
    /// static direct component edge.
    pub bind_target: Option<ComponentNodeRefBindTarget>,
}

impl CanonicalEncode for ComponentCodegenInboundNodeRef {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenInboundNodeRef", 3);
        encoder.field(1, &self.field_id);
        encoder.field(2, &self.producer_node_ref_id);
        encoder.field(3, &self.bind_target);
    }
}

/// Producer identity for one direct `NodeRef<T>` handle alias edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenDirectNodeRefAlias {
    /// Canonical parent-local component edge.
    pub edge_id: ComponentEdgeId,
    /// Exact child field name carried by the direct struct literal.
    pub field_name: String,
    /// Producer-local parent NodeRef declaration.
    pub producer_node_ref_id: u32,
    /// Canonical parent handle value used by `ComponentBindingSource`.
    pub canonical_value: LocalValueId,
}

impl CanonicalEncode for ComponentCodegenDirectNodeRefAlias {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenDirectNodeRefAlias", 4);
        encoder.field(1, &self.edge_id);
        encoder.field(2, &self.field_name);
        encoder.field(3, &self.producer_node_ref_id);
        encoder.field(4, &self.canonical_value);
    }
}

/// Validated producer-local relocation data needed by final browser codegen.
///
/// This record is deliberately separate from [`ComponentDescriptor`].
/// Producer IDs and legacy asset keys are build/codegen coordinates, not
/// semantic component identity. The semantic link plan nevertheless hashes
/// the complete record because changing any relocation changes emitted code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentCodegenBridge {
    /// Opaque producer component asset ID.
    pub asset_id: String,
    /// Opaque producer component-fragment key.
    ///
    /// The current producer uses the same key as `asset_id`; keeping a
    /// dedicated field makes that codegen ABI explicit and versionable.
    pub fragment_key: String,
    /// Producer-proven fragment root contract.
    pub fragment_root: ComponentFragmentRoot,
    /// Inbound fields in canonical component-field order.
    pub inbound_fields: Vec<ComponentCodegenInboundField>,
    /// Actual `SeedOnce` signal targets in canonical prop-site order.
    pub inbound_targets: Vec<ComponentCodegenInboundTarget>,
    /// Surviving legacy signals in producer-ID order.
    pub signals: Vec<ComponentCodegenSignal>,
    /// Surviving legacy expressions in producer-ID order.
    pub expressions: Vec<ComponentCodegenExpression>,
    /// Transported payload handlers in prelude-local order.
    pub payload_handlers: Vec<ComponentCodegenPayloadHandler>,
    /// Callback slots in producer-ID order.
    pub callback_slots: Vec<ComponentCodegenCallbackSlot>,
    /// Required inbound NodeRef fields in canonical field order.
    pub inbound_node_refs: Vec<ComponentCodegenInboundNodeRef>,
    /// Static direct NodeRef aliases in edge/field order.
    pub direct_node_ref_aliases: Vec<ComponentCodegenDirectNodeRefAlias>,
    /// Whether this producer fragment contains reachable query observation,
    /// refetch, or invalidation work.
    ///
    /// Declaration presence is intentionally insufficient: dead queries must
    /// not make the linked owner load the query runtime.
    pub uses_queries: bool,
    /// Whether this producer fragment contains reachable mutation observation
    /// or execution work.
    ///
    /// As with queries, declaration presence is insufficient: dead mutations
    /// remain producer metadata and do not load the mutation runtime.
    pub uses_mutations: bool,
}

impl CanonicalEncode for ComponentCodegenBridge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentCodegenBridge", 13);
        encoder.field(1, &self.asset_id);
        encoder.field(2, &self.fragment_key);
        encoder.field(3, &self.fragment_root);
        encoder.field(4, &self.inbound_fields);
        encoder.field(5, &self.inbound_targets);
        encoder.field(6, &self.signals);
        encoder.field(7, &self.expressions);
        encoder.field(8, &self.payload_handlers);
        encoder.field(9, &self.callback_slots);
        encoder.field(10, &self.inbound_node_refs);
        encoder.field(11, &self.direct_node_ref_aliases);
        encoder.field(12, &self.uses_queries);
        encoder.field(13, &self.uses_mutations);
    }
}

/// One exact parameter in a standalone helper's transported signature.
///
/// The producer already normalizes the accepted Rust spelling. Keeping that
/// spelling here, rather than collapsing it to a JavaScript representation,
/// preserves distinctions such as `u32`/`i32` and `Vec<T>`/`&[T]` that remain
/// part of the helper ABI even when their browser representations coincide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperCodegenParam {
    /// Source parameter name in declaration order. Final JS emission uses a
    /// stable positional identifier rather than treating this as JS source.
    pub name: String,
    /// Producer-normalized Rust type spelling.
    pub rust_type: String,
}

impl CanonicalEncode for HelperCodegenParam {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("HelperCodegenParam", 2);
        encoder.field(1, &self.name);
        encoder.field(2, &self.rust_type);
    }
}

/// Complete transported signature needed to validate one final browser helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperCodegenSignature {
    /// Parameters in declaration order.
    pub params: Vec<HelperCodegenParam>,
    /// Producer-normalized Rust return type spelling.
    pub return_type: String,
}

impl HelperCodegenSignature {
    /// Reconstruct the exact canonical legacy signature value whose hash is
    /// committed by [`HelperDescriptor::signature_hash`].
    pub fn canonical_json(&self) -> Value {
        serde_json::json!({
            "params": self.params.iter().map(|param| serde_json::json!({
                "name": param.name,
                "rust_type": param.rust_type,
            })).collect::<Vec<_>>(),
            "return_type": self.return_type,
        })
    }

    /// Strong hash of the exact parameter/result contract.
    pub fn semantic_hash(&self) -> SemanticHash {
        canonical_legacy_json_hash(HELPER_SIGNATURE_JSON_HASH_DOMAIN, &self.canonical_json())
    }
}

impl CanonicalEncode for HelperCodegenSignature {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("HelperCodegenSignature", 2);
        encoder.field(1, &self.params);
        encoder.field(2, &self.return_type);
    }
}

/// Validated relocation-resolved Client IR for a standalone helper.
///
/// Calls to other standalone helpers are represented structurally as
/// [`ClientFunctionRef::Helper`] values with dense caller-local
/// [`HelperCallEdgeId`]s. Authored paths remain diagnostic metadata only; no
/// final consumer needs to resolve a Rust spelling again.
#[derive(Clone, Debug)]
pub enum HelperCodegenBody {
    /// Expression-bodied helper.
    Expr(ClientExpr),
    /// Block-bodied helper.
    Block(ClientBlock),
}

impl PartialEq for HelperCodegenBody {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_json() == other.canonical_json()
    }
}

// Client IR contains `f64` literals and therefore cannot derive `Eq` directly.
// Its canonical JSON codec gives every value, including non-finite floats, a
// stable representation; use that representation as this bridge's equality
// contract instead of inheriting IEEE `NaN != NaN` behavior.
impl Eq for HelperCodegenBody {}

impl HelperCodegenBody {
    /// Serialize through the canonical Client IR v3 writer.
    pub fn canonical_json(&self) -> Value {
        let encoded = match self {
            Self::Expr(expr) => client_expr_json(expr),
            Self::Block(block) => client_block_json(block),
        };
        serde_json::from_str(&encoded).expect("typed Client IR always serializes as canonical JSON")
    }

    /// Hash of the relocation-resolved codegen body.
    pub fn semantic_hash(&self) -> SemanticHash {
        canonical_legacy_json_hash(HELPER_CODEGEN_IR_JSON_HASH_DOMAIN, &self.canonical_json())
    }

    fn validate(
        &self,
        parameter_count: usize,
    ) -> Result<rsjs_ir::ClientHelperValidationReport, rsjs_ir::ClientHelperValidationError> {
        match self {
            Self::Expr(expr) => validate_client_helper_expr(expr, parameter_count),
            Self::Block(block) => validate_client_helper_block(block, parameter_count),
        }
    }
}

impl CanonicalEncode for HelperCodegenBody {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        // Client IR has its own canonical JSON codec. Commit those exact bytes
        // with an explicit body-kind variant instead of maintaining a second,
        // subtly divergent canonical encoder for the full IR vocabulary.
        let bytes = canonical_json_bytes(&self.canonical_json())
            .expect("typed Client IR always serializes canonically");
        match self {
            Self::Expr(_) => {
                encoder.variant("HelperCodegenBody", 0, 1);
                encoder.field(1, &bytes);
            }
            Self::Block(_) => {
                encoder.variant("HelperCodegenBody", 1, 1);
                encoder.field(1, &bytes);
            }
        }
    }
}

/// Validated standalone-helper data needed by final browser codegen.
///
/// This is deliberately separate from [`HelperDescriptor`]. The descriptor
/// commits semantic identity and graph contracts; this bridge retains the
/// concrete signature and relocation-resolved body needed to emit code. The
/// source hash is retained for descriptor authentication; the resolved-body
/// hash is derived from the owned body so it can never become stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperCodegenBridge {
    /// Exact parameter/result contract.
    signature: HelperCodegenSignature,
    /// Hash of the producer's canonical pre-link Client IR.
    source_ir_hash: SemanticHash,
    /// Relocation-resolved Client IR body.
    body: HelperCodegenBody,
}

impl HelperCodegenBridge {
    /// Construct a helper-codegen bridge that is valid for `descriptor`.
    ///
    /// The resolved-body hash is deliberately not accepted as input or stored:
    /// it is always derived from the owned body. This makes a stale
    /// `(body, body_ir_hash)` pair unrepresentable through the safe API. The
    /// remaining cross-object commitments are checked here before a bridge can
    /// enter the semantic linker.
    fn try_new(
        definition: TransportDefinitionId,
        signature: HelperCodegenSignature,
        source_ir_hash: SemanticHash,
        body: HelperCodegenBody,
        descriptor: &HelperDescriptor,
    ) -> Result<Self, String> {
        let bridge = Self {
            signature,
            source_ir_hash,
            body,
        };
        bridge.validate_against(definition, descriptor)?;
        Ok(bridge)
    }

    /// Test-only construction gate for semantic-link adversarial fixtures.
    ///
    /// Production construction remains private to this trusted adapter module,
    /// which alone witnesses that `body` was derived from the source bytes
    /// committed by `source_ir_hash`.
    #[cfg(test)]
    pub(crate) fn try_new_for_test(
        definition: TransportDefinitionId,
        signature: HelperCodegenSignature,
        source_ir_hash: SemanticHash,
        body: HelperCodegenBody,
        descriptor: &HelperDescriptor,
    ) -> Result<Self, String> {
        Self::try_new(definition, signature, source_ir_hash, body, descriptor)
    }

    /// Exact parameter/result contract authenticated at construction.
    pub fn signature(&self) -> &HelperCodegenSignature {
        &self.signature
    }

    /// Hash of the producer's canonical pre-link Client IR.
    pub fn source_ir_hash(&self) -> SemanticHash {
        self.source_ir_hash
    }

    /// Relocation-resolved, origin-free Client IR body.
    pub fn body(&self) -> &HelperCodegenBody {
        &self.body
    }

    /// Hash of the relocation-resolved body.
    ///
    /// This is computed from the body on every observation so callers cannot
    /// accidentally persist a hash that no longer describes the emitted IR.
    pub fn body_ir_hash(&self) -> SemanticHash {
        self.body.semantic_hash()
    }

    fn validate_against(
        &self,
        definition: TransportDefinitionId,
        descriptor: &HelperDescriptor,
    ) -> Result<(), String> {
        validate_helper_codegen_bridge_parts(definition, self, descriptor)
    }
}

impl CanonicalEncode for HelperCodegenBridge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("HelperCodegenBridge", 4);
        encoder.field(1, &self.signature);
        encoder.field(2, &self.source_ir_hash);
        encoder.field(3, &self.body);
        encoder.field(4, &self.body_ir_hash());
    }
}

fn validate_helper_codegen_body(
    definition: TransportDefinitionId,
    body: &HelperCodegenBody,
    parameter_count: usize,
    expected_edges: &[HelperCallEdgeId],
) -> Result<(), String> {
    let report = body.validate(parameter_count).map_err(|error| {
        format!("origin-free helper codegen body for {definition:?} is invalid: {error}")
    })?;
    let actual_edges = report
        .calls
        .iter()
        .map(|call| call.edge)
        .collect::<Vec<_>>();
    if actual_edges != expected_edges {
        return Err(format!(
            "helper codegen body has mismatched helper edges: computed {actual_edges:?}, expected \
             {expected_edges:?}",
        ));
    }
    Ok(())
}

/// Validate that final helper codegen data is the exact concrete realization
/// of its canonical semantic descriptor.
pub fn validate_helper_codegen_bridge(
    definition: TransportDefinitionId,
    codegen: &HelperCodegenBridge,
    descriptor: &HelperDescriptor,
) -> Result<(), String> {
    codegen.validate_against(definition, descriptor)
}

fn validate_helper_codegen_bridge_parts(
    definition: TransportDefinitionId,
    codegen: &HelperCodegenBridge,
    descriptor: &HelperDescriptor,
) -> Result<(), String> {
    if codegen
        .signature
        .params
        .iter()
        .any(|param| param.name.is_empty() || param.rust_type.is_empty())
        || codegen.signature.return_type.is_empty()
    {
        return Err("helper codegen signature contains an empty name or type".to_owned());
    }
    let mut names = BTreeSet::new();
    if codegen
        .signature
        .params
        .iter()
        .any(|param| !names.insert(param.name.as_str()))
    {
        return Err("helper codegen signature contains duplicate parameter names".to_owned());
    }
    for param in &codegen.signature.params {
        parse_client_helper_type(&param.rust_type, ClientHelperTypePosition::Parameter).map_err(
            |error| {
                format!(
                    "helper codegen parameter `{}` has an invalid type: {error}",
                    param.name,
                )
            },
        )?;
    }
    if let Err(error) = parse_client_helper_type(
        &codegen.signature.return_type,
        ClientHelperTypePosition::Return,
    ) {
        return Err(format!(
            "helper codegen has an invalid return type: {error}"
        ));
    }
    let signature_hash = codegen.signature.semantic_hash();
    if signature_hash != descriptor.signature_hash {
        return Err(format!(
            "helper codegen signature hash {signature_hash} does not match descriptor hash {}",
            descriptor.signature_hash,
        ));
    }
    if codegen.source_ir_hash != descriptor.client_ir_hash {
        return Err(format!(
            "helper codegen source IR hash {} does not match descriptor hash {}",
            codegen.source_ir_hash, descriptor.client_ir_hash,
        ));
    }
    let expected_edges = descriptor
        .direct_helpers
        .keys()
        .copied()
        .collect::<Vec<_>>();
    validate_helper_codegen_body(
        definition,
        &codegen.body,
        codegen.signature.params.len(),
        &expected_edges,
    )
}

/// A validated canonical envelope plus its build-local relocation table.
///
/// Transport identities are intentionally outside the envelope's semantic
/// hash. They are exact build-local witnesses, not stable component/helper
/// identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDescriptorBridge {
    /// Canonical, hash-refreshed, ABI-v1-validated descriptor.
    pub(crate) envelope: DescriptorEnvelope,
    /// Canonical witness ID to exact relocation-resolved transport identity.
    pub(crate) witnesses: BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
    codegen: CanonicalDescriptorCodegen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CanonicalDescriptorCodegen {
    Component(ComponentCodegenBridge),
    Helper(HelperCodegenBridge),
}

impl CanonicalDescriptorBridge {
    /// Borrow the validated canonical descriptor.
    pub fn envelope(&self) -> &DescriptorEnvelope {
        &self.envelope
    }

    /// Borrow the relocation-resolved witness table.
    pub fn witnesses(&self) -> &BTreeMap<WitnessRecordId, ResolvedTransportWitness> {
        &self.witnesses
    }

    /// Borrow component codegen when this is a component bridge.
    pub fn component_codegen(&self) -> Option<&ComponentCodegenBridge> {
        match &self.codegen {
            CanonicalDescriptorCodegen::Component(codegen) => Some(codegen),
            CanonicalDescriptorCodegen::Helper(_) => None,
        }
    }

    /// Borrow helper codegen when this is a standalone-helper bridge.
    pub fn helper_codegen(&self) -> Option<&HelperCodegenBridge> {
        match &self.codegen {
            CanonicalDescriptorCodegen::Component(_) => None,
            CanonicalDescriptorCodegen::Helper(codegen) => Some(codegen),
        }
    }

    pub(crate) fn try_component(
        envelope: DescriptorEnvelope,
        witnesses: BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
        codegen: ComponentCodegenBridge,
    ) -> Result<Self, String> {
        let DescriptorBody::Component(descriptor) = &envelope.body else {
            return Err("component codegen requires a component descriptor".to_owned());
        };
        validate_component_codegen_bridge(&codegen, descriptor)?;
        Ok(Self {
            envelope,
            witnesses,
            codegen: CanonicalDescriptorCodegen::Component(codegen),
        })
    }

    pub(crate) fn try_helper(
        definition: TransportDefinitionId,
        envelope: DescriptorEnvelope,
        witnesses: BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
        codegen: HelperCodegenBridge,
    ) -> Result<Self, String> {
        let DescriptorBody::Helper(descriptor) = &envelope.body else {
            return Err("helper codegen requires a helper descriptor".to_owned());
        };
        validate_helper_codegen_bridge(definition, &codegen, descriptor)?;
        Ok(Self {
            envelope,
            witnesses,
            codegen: CanonicalDescriptorCodegen::Helper(codegen),
        })
    }

    #[cfg(test)]
    pub(crate) fn component_codegen_mut(&mut self) -> Option<&mut ComponentCodegenBridge> {
        match &mut self.codegen {
            CanonicalDescriptorCodegen::Component(codegen) => Some(codegen),
            CanonicalDescriptorCodegen::Helper(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_component_codegen_unchecked(&mut self, codegen: ComponentCodegenBridge) {
        self.codegen = CanonicalDescriptorCodegen::Component(codegen);
    }

    #[cfg(test)]
    pub(crate) fn replace_helper_codegen_unchecked(&mut self, codegen: HelperCodegenBridge) {
        self.codegen = CanonicalDescriptorCodegen::Helper(codegen);
    }
}

/// One bridge outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AbiBridgeResult {
    /// The source payload was represented without guessing.
    Canonical(CanonicalDescriptorBridge),
    /// The payload is valid legacy input, but canonical conversion would lose
    /// or invent behavior-affecting information.
    Unsupported(UnsupportedDescriptor),
}

impl AbiBridgeResult {
    /// Borrow the canonical result, if this descriptor is fully compatible.
    pub fn canonical(&self) -> Option<&CanonicalDescriptorBridge> {
        match self {
            Self::Canonical(descriptor) => Some(descriptor),
            Self::Unsupported(_) => None,
        }
    }
}

/// Fail-closed compatibility report for one valid legacy descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedDescriptor {
    /// Component or helper body kind.
    pub kind: DescriptorKind,
    /// Exact relocation-resolved defining record.
    pub definition: TransportDefinitionId,
    /// Every independently detected compatibility gap, in deterministic order.
    pub gaps: Vec<CompatibilityGap>,
}

impl fmt::Display for UnsupportedDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} descriptor {:?} cannot be converted to canonical ABI v1",
            self.kind, self.definition
        )?;
        for gap in &self.gaps {
            write!(formatter, "; {gap}")?;
        }
        Ok(())
    }
}

/// Behavior-affecting information absent from the legacy/canonical boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompatibilityGap {
    /// The transport has not yet recovered the normalized defining namespace.
    MissingResolvedSemanticIdentity,
    /// Client-visible schema-v9 fields have no concrete field-type/signature
    /// witness records.
    ComponentFieldWitnessesMissing {
        /// Inferred legacy field names requiring witnesses.
        fields: Vec<String>,
    },
    /// The legacy client graph has not been lowered into the canonical typed
    /// origin/effect/dependency model.
    ComponentClientGraphMetadataMissing {
        /// Non-empty legacy sections that require canonical lowering.
        features: Vec<String>,
    },
    /// Direct edges lack complete binding or boundary metadata.
    DirectComponentBindingMetadataMissing {
        /// Number of direct edges.
        edges: usize,
    },
    /// Repeated direct component occurrences need a keyed instance ABI.
    RepeatedDirectComponentSites {
        /// Producer-local direct edge ordinals.
        edges: Vec<u32>,
    },
    /// Direct component occurrences have an unresolved structural lifecycle.
    UnknownDirectComponentSites {
        /// Producer-local direct edge ordinals.
        edges: Vec<u32>,
    },
    /// SharedState v9 records carry a Rust type token but not `type_key` plus
    /// the complete canonical wire-schema ABI hash.
    SharedStateSchemaAbiMissing {
        /// Number of acquired stores.
        stores: usize,
    },
    /// The v9 view serializer collapsed a node variant to `"unknown"`.
    UnknownViewNodeKind,
}

impl fmt::Display for CompatibilityGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingResolvedSemanticIdentity => formatter.write_str(
                "missing normalized package/crate/module/item identity from the defining relocation",
            ),
            Self::ComponentFieldWitnessesMissing { fields } => write!(
                formatter,
                "legacy component fields lack concrete type/crossing witnesses: {}",
                fields.join(", ")
            ),
            Self::ComponentClientGraphMetadataMissing { features } => write!(
                formatter,
                "legacy client graph lacks canonical typed origin/effect/dependency metadata for: {}",
                features.join(", ")
            ),
            Self::DirectComponentBindingMetadataMissing { edges } => write!(
                formatter,
                "{edges} direct component edge(s) cannot be losslessly adapted as static \
                 server-only composition"
            ),
            Self::RepeatedDirectComponentSites { edges } => write!(
                formatter,
                "repeated direct component edge(s) {edges:?} require a keyed instance lifecycle ABI"
            ),
            Self::UnknownDirectComponentSites { edges } => write!(
                formatter,
                "direct component edge(s) {edges:?} have nested/unknown structural lifecycle semantics"
            ),
            Self::SharedStateSchemaAbiMissing { stores } => write!(
                formatter,
                "{stores} SharedState acquisition(s) lack type_key/schema ABI metadata"
            ),
            Self::UnknownViewNodeKind => formatter.write_str(
                "legacy view-tree serialization contains an unknown node kind",
            ),
        }
    }
}

/// Results for every descriptor in one already linked transport plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptedTransportAbi {
    /// Component results in the linked plan's canonical definition order.
    pub components: Vec<AbiBridgeResult>,
    /// Helper results in the linked plan's canonical definition order.
    pub helpers: Vec<AbiBridgeResult>,
}

/// Malformed data or a violated exact-link invariant.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AbiAdapterError {
    /// A linked payload is not the exact legacy schema this adapter knows.
    #[error("invalid linked {kind} payload for {definition:?}: {reason}")]
    InvalidPayload {
        /// Component/helper label.
        kind: &'static str,
        /// Exact defining record.
        definition: TransportDefinitionId,
        /// Focused schema/invariant detail.
        reason: String,
    },
    /// The embedded semantic hash differs from the linker's validated value.
    #[error(
        "linked {kind} payload hash disagrees with its transport link result for {definition:?}: payload {payload}, linked {linked}"
    )]
    SemanticHashMismatch {
        /// Component/helper label.
        kind: &'static str,
        /// Exact defining record.
        definition: TransportDefinitionId,
        /// Hash embedded in the JSON payload.
        payload: SemanticHash,
        /// Hash carried by the linked transport value.
        linked: SemanticHash,
    },
    /// A payload source-edge set differs from relocation-linked edges.
    #[error(
        "linked {kind} source-edge set disagrees with payload for {definition:?}: payload {payload:?}, linked {linked:?}"
    )]
    SourceEdgeSetMismatch {
        /// Component/helper edge label.
        kind: &'static str,
        /// Exact defining record.
        definition: TransportDefinitionId,
        /// Source edges named by the payload.
        payload: Vec<String>,
        /// Source edges actually resolved by relocations.
        linked: Vec<String>,
    },
    /// A direct adapter caller supplied a field record that does not match the
    /// shared producer/linker identity derivation.
    #[error(
        "component field witness `{field}` for {definition:?} has record ID {actual}, expected {expected}"
    )]
    ComponentFieldRecordIdMismatch {
        /// Exact defining component.
        definition: TransportDefinitionId,
        /// Rust source field name.
        field: String,
        /// Independently recomputed record ID.
        expected: String,
        /// Payload and linked-plan record ID.
        actual: String,
    },
    /// Two client-visible fields reused one exact object-record identity.
    #[error(
        "component field witnesses `{first_field}` and `{second_field}` for {definition:?} reuse record ID {field_record}"
    )]
    DuplicateComponentFieldRecordId {
        /// Exact defining component.
        definition: TransportDefinitionId,
        /// First field that claimed the record.
        first_field: String,
        /// Later field that reused the record.
        second_field: String,
        /// Reused record ID.
        field_record: String,
    },
    /// An authoritative semantic namespace is invalid.
    #[error("invalid resolved semantic identity for {kind} {definition:?}: {source}")]
    InvalidSemanticIdentity {
        /// Component/helper label.
        kind: &'static str,
        /// Exact defining record.
        definition: TransportDefinitionId,
        /// Canonical namespace validation failure.
        #[source]
        source: AbiValidationError,
    },
    /// A constructed descriptor did not pass canonical ABI validation.
    #[error("canonical descriptor validation failed for {kind} {definition:?}: {source}")]
    CanonicalValidation {
        /// Component/helper label.
        kind: &'static str,
        /// Exact defining record.
        definition: TransportDefinitionId,
        /// Canonical validation failure.
        #[source]
        source: AbiValidationError,
    },
}

/// Convert all descriptors in one exact transport link plan.
pub fn adapt_transport_plan(
    context: &AbiAdapterContext,
    plan: &TransportLinkPlan,
) -> Result<AdaptedTransportAbi, AbiAdapterError> {
    let projection_roles = resolve_deferred_projection_roles(plan)?;
    let components = plan
        .components
        .iter()
        .map(|component| {
            adapt_component_with_projection_roles(context, component, &projection_roles)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let helpers = plan
        .helpers
        .iter()
        .map(|helper| adapt_helper(context, helper))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AdaptedTransportAbi {
        components,
        helpers,
    })
}

/// Convert one relocation-linked component payload.
///
/// This deliberately fails closed for syntax whose meaning depends on an
/// exact child field, including a bare `None` callback candidate. Call
/// [`adapt_transport_plan`] when child-definition roles are required.
pub fn adapt_component(
    context: &AbiAdapterContext,
    linked: &LinkedTransportComponent,
) -> Result<AbiBridgeResult, AbiAdapterError> {
    adapt_component_with_projection_roles(context, linked, &ResolvedProjectionRoles::default())
}

fn adapt_component_with_projection_roles(
    context: &AbiAdapterContext,
    linked: &LinkedTransportComponent,
    projection_roles: &ResolvedProjectionRoles,
) -> Result<AbiBridgeResult, AbiAdapterError> {
    let root = payload_object("component", linked.definition, &linked.payload)?;
    require_known_fields(
        "component transport payload",
        root,
        &[
            "component_descriptor",
            "format",
            "schema_version",
            "semantic_hash",
            "transport",
        ],
        linked.definition,
    )?;
    require_exact_str(
        root,
        "format",
        COMPONENT_PAYLOAD_FORMAT,
        "component",
        linked.definition,
    )?;
    require_exact_u64(
        root,
        "schema_version",
        COMPONENT_PAYLOAD_SCHEMA,
        "component",
        linked.definition,
    )?;
    validate_linked_semantic_hash("component", linked.definition, root, linked.semantic_hash)?;

    let transport = required_object(root, "transport", "component", linked.definition)?;
    require_known_fields(
        "component transport metadata",
        transport,
        &[
            "client_value_sources",
            "component_field_witnesses",
            "component_input_fingerprint",
            "definition_record_id",
            "direct_component_edges",
            "hydration_constraint",
            "reachable_helper_edges",
            "shared_state_acquisitions",
            "shared_state_provider_sites",
        ],
        linked.definition,
    )?;
    validate_definition_record(
        "component",
        linked.definition,
        transport,
        "definition_record_id",
    )?;
    let input_fingerprint = validated_component_input_fingerprint(linked, transport)?;

    let descriptor = required_object(root, "component_descriptor", "component", linked.definition)?;
    validate_component_descriptor_header(linked.definition, descriptor)?;
    let (render_kind, origin_permission) = validate_component_definition_abi(linked, descriptor)?;
    let direct_component_edges = validate_component_edge_set(
        linked,
        transport,
        descriptor,
        input_fingerprint,
        projection_roles,
    )?;
    let direct_helper_edges =
        validate_component_helper_edge_set(linked, transport, input_fingerprint)?;
    let shared_state_acquisitions =
        validate_component_shared_state_acquisition_set(linked, transport, descriptor)?;
    let shared_state_provider_sites = validate_component_shared_state_provider_site_set(
        context,
        linked,
        transport,
        descriptor,
        input_fingerprint,
        &direct_component_edges,
    )?;
    let hydration_constraint = validate_hydration_constraint(linked.definition, transport)?;
    let inbound_inputs = validate_inbound_inputs_with_projection_roles(
        linked.definition,
        descriptor,
        projection_roles,
    )?;
    let payload_handlers = validate_payload_handler_values(
        linked,
        transport,
        descriptor,
        &inbound_inputs,
        &direct_component_edges,
        &direct_helper_edges,
    )?;
    let value_layout = ComponentValueLayout::new_with_handles(
        linked.definition,
        &inbound_inputs,
        descriptor,
        payload_handlers.len(),
        &direct_component_edges,
    )?;
    let reachable_helper_ordinals = direct_helper_edges
        .iter()
        .map(|edge| edge.local_ordinal)
        .collect::<BTreeSet<_>>();
    let authenticated_unknown_signal_types = authenticated_unknown_inbound_signals(&inbound_inputs);
    let extra_signal_roots = direct_component_edges
        .iter()
        .flat_map(|edge| &edge.fields)
        .filter_map(|field| match field.binding {
            ValidatedDirectComponentBinding::LiveValue { signal, .. } => Some(signal),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let legacy_graph = parse_legacy_client_graph_with_inbound_signals(
        descriptor,
        &reachable_helper_ordinals,
        &authenticated_unknown_signal_types,
        &extra_signal_roots,
    )
    .map_err(|reason| {
        invalid(
            "component",
            linked.definition,
            format!("invalid schema-v9 client graph: {reason}"),
        )
    })?;
    let direct_node_ref_sources = direct_component_edges
        .iter()
        .flat_map(|edge| edge.fields.iter())
        .filter_map(|field| match field.binding {
            ValidatedDirectComponentBinding::NodeRefHandle { node_ref } => Some(node_ref),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let reachable_node_refs = legacy_graph
        .reachable_node_refs
        .iter()
        .copied()
        .chain(direct_node_ref_sources.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    validate_node_ref_handle_topology(
        linked.definition,
        &inbound_inputs,
        &legacy_graph,
        &direct_component_edges,
    )?;
    let active_shared_state = resolve_active_shared_state_plan(
        context,
        linked.definition,
        &legacy_graph,
        &shared_state_acquisitions,
    )?;
    validate_query_key_prop_targets(linked.definition, &inbound_inputs, &legacy_graph)?;
    let bound_handler_ids = legacy_graph
        .bindings
        .events
        .iter()
        .map(|binding| binding.handler)
        .collect::<BTreeSet<_>>();
    let bound_handler_count = legacy_graph
        .handlers
        .iter()
        .filter(|handler| bound_handler_ids.contains(&handler.id))
        .count();
    let legacy_expression_browser_operation_count = legacy_graph
        .expressions
        .iter()
        .map(|expression| expression.browser_operations.len())
        .try_fold(0_usize, |count, operations| count.checked_add(operations))
        .ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "legacy expression browser-operation count overflows usize",
            )
        })?;
    let legacy_handler_browser_operation_count = legacy_graph
        .handlers
        .iter()
        .filter(|handler| bound_handler_ids.contains(&handler.id))
        .map(|handler| handler.browser_operations.len())
        .try_fold(0_usize, |count, operations| count.checked_add(operations))
        .ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "legacy handler browser-operation count overflows usize",
            )
        })?;
    let legacy_browser_operation_count = legacy_expression_browser_operation_count
        .checked_add(legacy_handler_browser_operation_count)
        .ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "legacy browser-operation count overflows usize",
            )
        })?;
    let payload_browser_operation_count = payload_handlers
        .iter()
        .map(|handler| handler.browser_operations.len())
        .try_fold(0_usize, |count, operations| count.checked_add(operations))
        .ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "payload browser-operation count overflows usize",
            )
        })?;
    let browser_operation_count = payload_browser_operation_count
        .checked_add(legacy_browser_operation_count)
        .ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "payload and legacy browser-operation counts overflow usize",
            )
        })?;
    let dom_binding_count = legacy_graph.bindings.text.len()
        + legacy_graph.bindings.attributes.len()
        + legacy_graph.bindings.inputs.len()
        + legacy_graph.bindings.conditionals.len()
        + legacy_graph.keyed_collections.len();
    let callback_values =
        payload_handler_value_map(linked.definition, &payload_handlers, &value_layout)?;
    let origin_layout = ComponentOriginLayout::new(
        linked.definition,
        inbound_inputs.prop_uses.len(),
        payload_handlers.len(),
        legacy_graph.signals.len(),
        legacy_graph.expressions.len(),
        bound_handler_count,
        browser_operation_count,
        reachable_node_refs.len(),
        legacy_graph.reachable_queries.len(),
        legacy_graph.reachable_mutations.len(),
        active_shared_state.stores.len(),
        dom_binding_count,
        direct_component_edges.len(),
    )?;
    let witness_layout = ComponentWitnessLayout::new(
        linked.definition,
        direct_component_edges.len(),
        inbound_inputs.fields.len(),
        direct_helper_edges.len(),
    )?;
    let field_witnesses = validate_component_field_witness_set(
        linked,
        transport,
        &inbound_inputs.fields,
        &witness_layout,
    )?;
    let adapted_fields = adapt_component_fields(
        linked.definition,
        descriptor,
        &inbound_inputs,
        &field_witnesses,
        &value_layout,
    )?;
    let immutable_static_slot_names = adapted_fields
        .canonical
        .values()
        .filter(|field| {
            field.role == ComponentFieldRole::Slot
                && matches!(field.static_slot, StaticSlotCapability::Immutable { .. })
        })
        .map(|field| field.source_name.clone())
        .collect::<BTreeSet<_>>();
    let view = validate_view_tree(
        linked.definition,
        descriptor,
        &direct_component_edges,
        &legacy_graph.bindings.conditionals,
        &immutable_static_slot_names,
    )?;

    let mut gaps = Vec::new();
    let identity = context.component_identities.get(&linked.definition);
    if identity.is_none() {
        gaps.push(CompatibilityGap::MissingResolvedSemanticIdentity);
    }

    let mut features = legacy_graph.unsupported_features.clone();
    if active_shared_state.operation_shapes_supported {
        features.retain(|feature| feature != "shared_backed_signals");
    }
    if payload_handlers
        .iter()
        .any(|handler| !handler.dependencies.client_module_calls.is_empty())
    {
        features.push("payload_handlers.client_module_calls".to_owned());
        features.sort();
        features.dedup();
    }
    for (used, feature) in [
        (
            payload_handlers
                .iter()
                .any(|handler| handler.dependencies.uses_node_payload),
            "payload_handlers.node_payload",
        ),
        (
            payload_handlers
                .iter()
                .any(|handler| !handler.dependencies.node_refs.is_empty()),
            "payload_handlers.node_refs",
        ),
        (
            payload_handlers
                .iter()
                .any(|handler| !handler.dependencies.queries.is_empty()),
            "payload_handlers.queries",
        ),
        (
            payload_handlers
                .iter()
                .any(|handler| !handler.dependencies.mutations.is_empty()),
            "payload_handlers.mutations",
        ),
    ] {
        if used {
            features.push(feature.to_owned());
        }
    }
    features.sort();
    features.dedup();
    let has_live_direct_component_bindings = direct_component_edges.iter().any(|edge| {
        edge.fields.iter().any(|field| {
            matches!(
                field.binding,
                ValidatedDirectComponentBinding::LiveValue { .. }
                    | ValidatedDirectComponentBinding::LiveCallback { .. }
                    | ValidatedDirectComponentBinding::ForwardedCallback { .. }
                    | ValidatedDirectComponentBinding::NodeRefHandle { .. }
            )
        })
    });
    let static_repetition_is_the_only_unsupported_shape = features.as_slice()
        == ["unsupported_shapes"]
        && direct_component_edges
            .iter()
            .any(|edge| matches!(edge.lifecycle, ValidatedDirectComponentLifecycle::Repeated))
        && legacy_graph.signals.is_empty()
        && legacy_graph.expressions.is_empty()
        && legacy_graph.handlers.is_empty()
        && legacy_graph.callback_slots.is_empty()
        && legacy_graph.bindings.text.is_empty()
        && legacy_graph.bindings.attributes.is_empty()
        && legacy_graph.bindings.inputs.is_empty()
        && legacy_graph.bindings.events.is_empty()
        && legacy_graph.node_refs.is_empty()
        && legacy_graph.queries.is_empty()
        && legacy_graph.mutations.is_empty()
        && inbound_inputs.fields.is_empty()
        && payload_handlers.is_empty()
        && direct_helper_edges.is_empty()
        && !active_shared_state.is_active()
        && !has_live_direct_component_bindings;
    if static_repetition_is_the_only_unsupported_shape {
        // The repeated lifecycle gap below is the exact missing capability.
        // A server-only loop does not independently require client-graph
        // lowering merely because its preserved site is repeated.
        features.clear();
    }
    let has_legacy_client_state = !features.is_empty()
        || !legacy_graph.signals.is_empty()
        || !legacy_graph.expressions.is_empty()
        || !legacy_graph.handlers.is_empty()
        || !legacy_graph.callback_slots.is_empty()
        || !legacy_graph.bindings.text.is_empty()
        || !legacy_graph.bindings.attributes.is_empty()
        || !legacy_graph.bindings.inputs.is_empty()
        || !legacy_graph.bindings.events.is_empty()
        || !legacy_graph.keyed_collections.is_empty()
        || !legacy_graph.node_refs.is_empty()
        || !legacy_graph.queries.is_empty()
        || !legacy_graph.mutations.is_empty()
        || !inbound_inputs.fields.is_empty()
        || !payload_handlers.is_empty()
        || !direct_helper_edges.is_empty()
        || active_shared_state.is_active()
        || has_live_direct_component_bindings;
    if !features.is_empty() {
        gaps.push(CompatibilityGap::ComponentClientGraphMetadataMissing { features });
    }
    if direct_component_edges
        .iter()
        .any(|edge| !edge.has_supported_binding_shape())
    {
        gaps.push(CompatibilityGap::DirectComponentBindingMetadataMissing {
            edges: linked.component_edges.len(),
        });
    }
    let repeated_component_edges = direct_component_edges
        .iter()
        .filter_map(|edge| {
            matches!(edge.lifecycle, ValidatedDirectComponentLifecycle::Repeated)
                .then_some(edge.local_ordinal)
        })
        .collect::<Vec<_>>();
    if !repeated_component_edges.is_empty() {
        gaps.push(CompatibilityGap::RepeatedDirectComponentSites {
            edges: repeated_component_edges,
        });
    }
    let unknown_component_edges = direct_component_edges
        .iter()
        .filter_map(|edge| {
            matches!(
                edge.lifecycle,
                ValidatedDirectComponentLifecycle::UnknownNestedConditionals
            )
            .then_some(edge.local_ordinal)
        })
        .collect::<Vec<_>>();
    if !unknown_component_edges.is_empty() {
        gaps.push(CompatibilityGap::UnknownDirectComponentSites {
            edges: unknown_component_edges,
        });
    }
    if view.has_unknown_kind {
        gaps.push(CompatibilityGap::UnknownViewNodeKind);
    }

    if !gaps.is_empty() {
        return Ok(AbiBridgeResult::Unsupported(UnsupportedDescriptor {
            kind: DescriptorKind::Component,
            definition: linked.definition,
            gaps,
        }));
    }

    let identity = identity.expect("missing identity returned as an unsupported gap");
    let semantic_key =
        identity
            .component_key()
            .map_err(|source| AbiAdapterError::InvalidSemanticIdentity {
                kind: "component",
                definition: linked.definition,
                source,
            })?;
    let mut descriptor_body = ComponentDescriptor::v1(
        semantic_key,
        WitnessRecordId::new(0),
        render_kind,
        origin_permission,
        view.root_span.clone(),
    );
    // A definition-level immutable slot proves the child preserves that
    // range, not that every possible standalone instantiation supplies an
    // Exact(1) value. Such components become owner candidates only through a
    // linked incoming edge whose StaticSlot binding carries the occurrence-
    // level span proof.
    descriptor_body.root_eligible = immutable_static_slot_names.is_empty();
    descriptor_body.hydration = hydration_constraint;
    if has_legacy_client_state {
        descriptor_body.client_ir_hash = canonical_legacy_json_hash(
            COMPONENT_CLIENT_IR_HASH_DOMAIN,
            &legacy_component_client_program_material(descriptor, &payload_handlers),
        );
    }
    descriptor_body.fields.extend(adapted_fields.canonical);
    populate_component_input_graph(
        linked.definition,
        &mut descriptor_body,
        &inbound_inputs,
        &value_layout,
        origin_layout,
    )?;
    let shared_state_origins = populate_shared_state_references(
        linked.definition,
        &mut descriptor_body,
        &legacy_graph,
        &active_shared_state,
        &value_layout,
        origin_layout,
    )?;
    let mut browser_operation_ordinal = 0_u32;
    populate_payload_handler_graph(
        linked.definition,
        &mut descriptor_body,
        &payload_handlers,
        &inbound_inputs,
        &value_layout,
        origin_layout,
        &direct_helper_edges,
        &shared_state_origins,
        &mut browser_operation_ordinal,
    )?;
    populate_legacy_client_graph(
        linked.definition,
        &mut descriptor_body,
        &legacy_graph,
        &inbound_inputs,
        &value_layout,
        origin_layout,
        &direct_helper_edges,
        &shared_state_origins,
        &reachable_node_refs,
        &mut browser_operation_ordinal,
    )?;
    if browser_operation_ordinal != origin_layout.browser_operation_count {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "canonical browser-operation lowering consumed {browser_operation_ordinal} origins, but the validated payload and legacy graphs reserved {}",
                origin_layout.browser_operation_count,
            ),
        ));
    }
    for edge in &direct_component_edges {
        let edge_id = ComponentEdgeId::new(edge.local_ordinal);
        let origin = origin_layout.component(linked.definition, edge.local_ordinal)?;
        let witness = witness_layout.component(linked.definition, edge.local_ordinal)?;
        descriptor_body
            .origins
            .insert(origin, OriginKind::SourceOnly);
        let bindings = adapt_direct_component_bindings(
            linked.definition,
            &edge.fields,
            &value_layout,
            &callback_values,
            origin,
        )?;
        let lifecycle_control = match edge.lifecycle {
            ValidatedDirectComponentLifecycle::StaticSingle => None,
            ValidatedDirectComponentLifecycle::ConditionalSingle {
                control,
                predicate_expression,
                branch,
                ..
            } => {
                let expected_predicate =
                    value_layout.expr(linked.definition, predicate_expression)?;
                Some(canonical_component_lifecycle_control(
                    linked.definition,
                    &descriptor_body,
                    control,
                    expected_predicate,
                    branch,
                )?)
            }
            ValidatedDirectComponentLifecycle::Repeated
            | ValidatedDirectComponentLifecycle::UnknownNestedConditionals => {
                unreachable!("unsupported component lifecycle returned before canonical lowering")
            }
        };
        descriptor_body.direct_components.insert(
            edge_id,
            DirectComponentEdge {
                witness,
                boundary: if edge.composed {
                    ComponentBoundary::Composed
                } else {
                    ComponentBoundary::IndependentIsland
                },
                bindings: bindings.clone(),
                lifecycle_control,
                origin,
            },
        );
        if let Some(lifecycle) = lifecycle_control {
            push_dependency(
                linked.definition,
                &mut descriptor_body,
                DependencyEdgeKind::Control,
                DependencyEndpoint::Control(lifecycle.control),
                DependencyEndpoint::Component(edge_id),
                origin,
            )?;
        }
        for binding in bindings.values() {
            let value = match binding.source {
                ComponentBindingSource::LiveValue { value }
                | ComponentBindingSource::LiveCallback { value, .. }
                | ComponentBindingSource::ClientHandle { value } => Some(value),
                _ => None,
            };
            let Some(value) = value else {
                continue;
            };
            for kind in [
                DependencyEdgeKind::Component,
                DependencyEdgeKind::OwnerCoupling,
            ] {
                push_dependency(
                    linked.definition,
                    &mut descriptor_body,
                    kind,
                    DependencyEndpoint::Value(value),
                    DependencyEndpoint::Component(edge_id),
                    origin,
                )?;
            }
        }
    }
    for site in &shared_state_provider_sites {
        descriptor_body.shared_state_providers.insert(
            ProviderSiteId::new(site.provider_site_id),
            SharedStateProviderSite {
                child_edge: ComponentEdgeId::new(site.child_edge_local_ordinal),
                type_key: site.type_key.clone(),
                schema_abi: site.schema_abi,
            },
        );
    }
    for (index, edge) in direct_helper_edges.iter().enumerate() {
        debug_assert!(
            index == 0 || direct_helper_edges[index - 1].local_ordinal < edge.local_ordinal,
            "validated component helper edges are ordered by producer-local ordinal"
        );
        let call = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                "reachable helper edge count exceeds the canonical u32 ID space",
            )
        })?;
        descriptor_body.direct_helpers.insert(
            HelperCallEdgeId::new(call),
            witness_layout.helper(linked.definition, call)?,
        );
    }
    if matches!(view.root_span, SpanCardinality::Unknown { .. }) {
        descriptor_body.region_sites.insert(RegionSiteId::new(0));
    }

    let mut header = DescriptorHeader::v1(
        DescriptorKind::Component,
        context.producer,
        RuntimeCompatibility::new(
            VersionRange::exact(context.runtime_protocol),
            CapabilitySet::NONE,
        ),
    );
    header.capabilities =
        CapabilitySet::ORIGIN_EFFECT_TAXONOMY.union(CapabilitySet::DOM_SPAN_CARDINALITY);
    if !descriptor_body.dependencies.is_empty() {
        header.capabilities = header.capabilities.union(CapabilitySet::TYPED_DEPENDENCIES);
    }
    if !descriptor_body.fields.is_empty() {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::COMPONENT_FIELD_SCHEMA);
    }
    if !descriptor_body.prop_uses.is_empty() {
        header.capabilities = header.capabilities.union(CapabilitySet::TEMPORAL_PROPS);
        header.runtime.required_capabilities = header
            .runtime
            .required_capabilities
            .union(CapabilitySet::TEMPORAL_PROPS);
    }
    if descriptor_body
        .fields
        .values()
        .any(|field| matches!(field.static_slot, StaticSlotCapability::Immutable { .. }))
        || descriptor_body.direct_components.values().any(|edge| {
            edge.bindings
                .values()
                .any(|binding| matches!(binding.source, ComponentBindingSource::StaticSlot { .. }))
        })
    {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::STATIC_SLOT_CONTRACTS);
    }
    if !descriptor_body.direct_components.is_empty() {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::DIRECT_COMPONENT_BINDINGS)
            .union(CapabilitySet::OWNER_PARTITION_INPUTS);
        header.runtime.required_capabilities = header
            .runtime
            .required_capabilities
            .union(CapabilitySet::OWNER_PARTITION_INPUTS);
    }
    if !descriptor_body.shared_state_providers.is_empty() {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY);
    }
    if descriptor_body
        .direct_components
        .values()
        .any(|edge| edge.lifecycle_control.is_some())
    {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE);
    }
    if !descriptor_body.direct_helpers.is_empty() {
        header.capabilities = header
            .capabilities
            .union(CapabilitySet::ORIGIN_FREE_HELPERS);
    }
    if !descriptor_body.region_sites.is_empty() {
        header.runtime.required_capabilities = header
            .runtime
            .required_capabilities
            .union(CapabilitySet::DOM_SPAN_CARDINALITY);
    }

    let component_codegen = build_component_codegen_bridge(
        linked.definition,
        descriptor,
        &inbound_inputs,
        &legacy_graph,
        &payload_handlers,
        &value_layout,
        &view,
        &direct_component_edges,
    )?;
    validate_component_codegen_bridge(&component_codegen, &descriptor_body).map_err(|reason| {
        invalid(
            "component",
            linked.definition,
            format!("invalid component codegen bridge: {reason}"),
        )
    })?;

    let envelope =
        DescriptorEnvelope::new(header, DescriptorBody::Component(descriptor_body), None).map_err(
            |source| AbiAdapterError::CanonicalValidation {
                kind: "component",
                definition: linked.definition,
                source,
            },
        )?;
    envelope
        .validate(&context.support)
        .map_err(|source| AbiAdapterError::CanonicalValidation {
            kind: "component",
            definition: linked.definition,
            source,
        })?;

    let witnesses = build_component_witness_table(
        linked,
        &direct_component_edges,
        &field_witnesses,
        &direct_helper_edges,
        witness_layout,
    )?;
    let bridge = CanonicalDescriptorBridge::try_component(envelope, witnesses, component_codegen)
        .map_err(|reason| {
        invalid(
            "component",
            linked.definition,
            format!("invalid canonical bridge: {reason}"),
        )
    })?;
    Ok(AbiBridgeResult::Canonical(bridge))
}

/// Convert one relocation-linked standalone helper payload.
pub fn adapt_helper(
    context: &AbiAdapterContext,
    linked: &LinkedTransportHelper,
) -> Result<AbiBridgeResult, AbiAdapterError> {
    let root = payload_object("helper", linked.definition, &linked.payload)?;
    require_known_fields(
        "standalone helper payload",
        root,
        &[
            "body_hir",
            "body_hir_version",
            "body_ir",
            "body_ir_version",
            "codec_abi_version",
            "defining_record_id",
            "direct_calls",
            "effect_class",
            "format",
            "provisional_identity",
            "schema_version",
            "semantic_hash",
            "signature",
        ],
        linked.definition,
    )?;
    require_exact_str(
        root,
        "format",
        HELPER_PAYLOAD_FORMAT,
        "helper",
        linked.definition,
    )?;
    require_exact_u64(
        root,
        "schema_version",
        HELPER_PAYLOAD_SCHEMA,
        "helper",
        linked.definition,
    )?;
    require_exact_u64(
        root,
        "body_hir_version",
        HELPER_HIR_SCHEMA,
        "helper",
        linked.definition,
    )?;
    require_exact_u64(
        root,
        "body_ir_version",
        HELPER_IR_SCHEMA,
        "helper",
        linked.definition,
    )?;
    require_exact_u64(
        root,
        "codec_abi_version",
        HELPER_CODEC_ABI,
        "helper",
        linked.definition,
    )?;
    require_exact_str(
        root,
        "effect_class",
        "origin_free",
        "helper",
        linked.definition,
    )?;
    validate_linked_semantic_hash("helper", linked.definition, root, linked.semantic_hash)?;
    validate_definition_record("helper", linked.definition, root, "defining_record_id")?;
    let direct_helper_edges = validate_helper_edge_set(linked, root)?;

    let signature = required_object(root, "signature", "helper", linked.definition)?;
    require_known_fields(
        "helper signature",
        signature,
        &["params", "return_type"],
        linked.definition,
    )?;
    let params = required_array(signature, "params", "helper", linked.definition)?;
    let mut codegen_params = Vec::with_capacity(params.len());
    let mut parameter_names = BTreeSet::new();
    for (index, param) in params.iter().enumerate() {
        let param = param.as_object().ok_or_else(|| {
            invalid(
                "helper",
                linked.definition,
                format!("signature.params[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "helper signature parameter",
            param,
            &["name", "rust_type"],
            linked.definition,
        )?;
        let name = required_str(param, "name", "helper", linked.definition)?;
        let rust_type = required_str(param, "rust_type", "helper", linked.definition)?;
        if name.is_empty() || rust_type.is_empty() {
            return Err(invalid(
                "helper",
                linked.definition,
                format!(
                    "signature.params[{index}] must carry a non-empty parameter name and Rust type"
                ),
            ));
        }
        parse_client_helper_type(rust_type, ClientHelperTypePosition::Parameter).map_err(
            |error| {
                invalid(
                    "helper",
                    linked.definition,
                    format!(
                        "signature.params[{index}] has an invalid normalized Rust type: {error}"
                    ),
                )
            },
        )?;
        if !parameter_names.insert(name) {
            return Err(invalid(
                "helper",
                linked.definition,
                format!("signature parameter name `{name}` appears more than once"),
            ));
        }
        codegen_params.push(HelperCodegenParam {
            name: name.to_owned(),
            rust_type: rust_type.to_owned(),
        });
    }
    let return_type = required_str(signature, "return_type", "helper", linked.definition)?;
    if return_type.is_empty() {
        return Err(invalid(
            "helper",
            linked.definition,
            "signature.return_type must not be empty",
        ));
    }
    if let Err(error) = parse_client_helper_type(return_type, ClientHelperTypePosition::Return) {
        return Err(invalid(
            "helper",
            linked.definition,
            format!("signature.return_type has an invalid normalized Rust type: {error}"),
        ));
    }
    let codegen_signature = HelperCodegenSignature {
        params: codegen_params,
        return_type: return_type.to_owned(),
    };
    let body_ir = root.get("body_ir").ok_or_else(|| {
        invalid(
            "helper",
            linked.definition,
            "standalone helper payload is missing `body_ir`",
        )
    })?;
    let (normalized_body_ir, resolved_body_ir) =
        validate_standalone_helper_client_ir(linked.definition, body_ir, &direct_helper_edges)?;
    // Validate the origin-free local body before semantic-identity admission.
    // An otherwise unsupported helper must not turn malformed Client IR into
    // a compatibility gap and thereby hide a producer defect. Construction
    // below repeats this check against the completed descriptor so every
    // stored bridge still enters the linker fully authenticated.
    let expected_helper_edges = direct_helper_edges
        .iter()
        .map(|edge| HelperCallEdgeId::new(edge.ordinal))
        .collect::<Vec<_>>();
    validate_helper_codegen_body(
        linked.definition,
        &resolved_body_ir,
        codegen_signature.params.len(),
        &expected_helper_edges,
    )
    .map_err(|reason| {
        invalid(
            "helper",
            linked.definition,
            format!("invalid helper codegen body: {reason}"),
        )
    })?;
    let Some(identity) = context.helper_identities.get(&linked.definition) else {
        return Ok(AbiBridgeResult::Unsupported(UnsupportedDescriptor {
            kind: DescriptorKind::Helper,
            definition: linked.definition,
            gaps: vec![CompatibilityGap::MissingResolvedSemanticIdentity],
        }));
    };
    let semantic_key =
        identity
            .helper_key()
            .map_err(|source| AbiAdapterError::InvalidSemanticIdentity {
                kind: "helper",
                definition: linked.definition,
                source,
            })?;
    let signature_hash = canonical_legacy_json_hash(
        HELPER_SIGNATURE_JSON_HASH_DOMAIN,
        &Value::Object(signature.clone()),
    );
    let client_ir_hash =
        canonical_legacy_json_hash(HELPER_CLIENT_IR_JSON_HASH_DOMAIN, &normalized_body_ir);
    let mut descriptor_body = HelperDescriptor::v1(
        semantic_key,
        WitnessRecordId::new(0),
        signature_hash,
        client_ir_hash,
    );

    let mut witnesses = BTreeMap::new();
    witnesses.insert(
        WitnessRecordId::new(0),
        ResolvedTransportWitness::Definition(linked.definition),
    );
    for edge in &direct_helper_edges {
        let call = HelperCallEdgeId::new(edge.ordinal);
        let witness_id = edge.ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "helper",
                linked.definition,
                "direct helper call count exceeds the canonical witness ID space",
            )
        })?;
        let witness = WitnessRecordId::new(witness_id);
        descriptor_body.direct_helpers.insert(call, witness);
        witnesses.insert(
            witness,
            ResolvedTransportWitness::HelperEdge {
                source_edge: edge.source_edge,
                target: edge.target,
            },
        );
    }

    let mut header = DescriptorHeader::v1(
        DescriptorKind::Helper,
        context.producer,
        RuntimeCompatibility::new(
            VersionRange::exact(context.runtime_protocol),
            CapabilitySet::NONE,
        ),
    );
    header.capabilities =
        CapabilitySet::ORIGIN_EFFECT_TAXONOMY.union(CapabilitySet::ORIGIN_FREE_HELPERS);
    let envelope = DescriptorEnvelope::new(header, DescriptorBody::Helper(descriptor_body), None)
        .map_err(|source| AbiAdapterError::CanonicalValidation {
        kind: "helper",
        definition: linked.definition,
        source,
    })?;
    envelope
        .validate(&context.support)
        .map_err(|source| AbiAdapterError::CanonicalValidation {
            kind: "helper",
            definition: linked.definition,
            source,
        })?;

    let DescriptorBody::Helper(helper_descriptor) = &envelope.body else {
        unreachable!("standalone helper adapter constructed a helper descriptor")
    };
    let helper_codegen = HelperCodegenBridge::try_new(
        linked.definition,
        codegen_signature,
        client_ir_hash,
        resolved_body_ir,
        helper_descriptor,
    )
    .map_err(|reason| {
        invalid(
            "helper",
            linked.definition,
            format!("invalid helper codegen bridge: {reason}"),
        )
    })?;

    let bridge = CanonicalDescriptorBridge::try_helper(
        linked.definition,
        envelope,
        witnesses,
        helper_codegen,
    )
    .map_err(|reason| {
        invalid(
            "helper",
            linked.definition,
            format!("invalid canonical bridge: {reason}"),
        )
    })?;
    Ok(AbiBridgeResult::Canonical(bridge))
}

#[derive(Debug)]
struct AdaptedComponentFields {
    canonical: BTreeMap<ComponentFieldId, ComponentFieldSchema>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboundFieldKind {
    Value,
    Callback,
    NodeRef,
    ServerOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboundCallbackShape {
    Required,
    Optional,
}

#[derive(Clone, Debug)]
struct ValidatedInboundField {
    name: String,
    kind: InboundFieldKind,
    value: u64,
    crossing: u64,
    callback_slot: Option<u32>,
    callback_shape: Option<InboundCallbackShape>,
    /// Raw field witnesses for deferred projections commit this role even
    /// though canonical adaptation uses the rustc-linked concrete `kind`.
    record_role: TransportComponentFieldRole,
}

#[derive(Clone, Debug)]
struct ValidatedInboundPropUse {
    site: u32,
    field: u32,
    value: u32,
    temporal: TemporalPropMode,
    target_signal: Option<u32>,
}

#[derive(Clone, Debug)]
struct ValidatedInboundNodeRef {
    field: u32,
    value: u32,
    node_ref: u32,
    bind_target: Option<ComponentNodeRefBindTarget>,
}

#[derive(Clone, Debug)]
struct ValidatedInboundInputs {
    fields: Vec<ValidatedInboundField>,
    prop_uses: Vec<ValidatedInboundPropUse>,
    node_refs: Vec<ValidatedInboundNodeRef>,
    /// Latent producer signals carried by raw deferred projections. Keep them
    /// source-only even when linking resolves the field as a callback, NodeRef,
    /// or server-only value.
    projection_signals: BTreeSet<u32>,
}

/// Producer signals whose value is supplied by an enclosing component graph.
///
/// These remain source-only during canonical origin classification. A
/// `SeedOnce` target is deliberately absent: it samples an inbound field into
/// a fresh child-owned writable cell.
fn inbound_source_signals(inbound: &ValidatedInboundInputs) -> BTreeSet<u32> {
    inbound
        .prop_uses
        .iter()
        .filter_map(|prop_use| {
            matches!(
                prop_use.temporal,
                TemporalPropMode::LiveRead | TemporalPropMode::QueryKeyDependency
            )
            .then_some(prop_use.target_signal)
            .flatten()
        })
        .chain(inbound.projection_signals.iter().copied())
        .collect()
}

/// Inbound-backed producer signals allowed to retain an unknown Client IR type.
///
/// Descriptor compilation cannot resolve the Rust type of `self.field` while
/// analyzing an inbound read or seed expression. The exact component-field
/// relocation and generated Rust crossing/`ClientEncode` checks authenticate
/// the value path; operations that require a concrete numeric or nominal
/// client type still reject `ClientType::Unknown` in the producer analyzer.
///
/// This set is intentionally broader than [`inbound_source_signals`]: a
/// `SeedOnce` signal may remain dynamically typed while still owning fresh
/// writable child state.
fn authenticated_unknown_inbound_signals(inbound: &ValidatedInboundInputs) -> BTreeSet<u32> {
    inbound
        .prop_uses
        .iter()
        .filter_map(|prop_use| {
            matches!(
                prop_use.temporal,
                TemporalPropMode::LiveRead
                    | TemporalPropMode::QueryKeyDependency
                    | TemporalPropMode::SeedOnce(_)
            )
            .then_some(prop_use.target_signal)
            .flatten()
        })
        .chain(inbound.projection_signals.iter().copied())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedProjectionRole {
    ServerOnly,
    Value,
    Callback(InboundCallbackShape),
    NodeRef,
}

impl ResolvedProjectionRole {
    fn label(self) -> &'static str {
        match self {
            Self::ServerOnly => "server-only",
            Self::Value => "live value",
            Self::Callback(InboundCallbackShape::Required) => "required callback",
            Self::Callback(InboundCallbackShape::Optional) => "optional callback",
            Self::NodeRef => "NodeRef handle",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ResolvedProjectionRoles {
    fields: BTreeMap<(TransportDefinitionId, u32), ResolvedProjectionRole>,
    fields_by_name: BTreeMap<(TransportDefinitionId, String), ResolvedProjectionRole>,
}

impl ResolvedProjectionRoles {
    fn field(
        &self,
        definition: TransportDefinitionId,
        field: u32,
    ) -> Option<ResolvedProjectionRole> {
        self.fields.get(&(definition, field)).copied()
    }

    fn field_by_name(
        &self,
        definition: TransportDefinitionId,
        field: &str,
    ) -> Option<ResolvedProjectionRole> {
        self.fields_by_name
            .get(&(definition, field.to_owned()))
            .copied()
    }
}

#[derive(Clone, Debug)]
struct ProjectionConstraint {
    parent: (TransportDefinitionId, u32),
    child: Option<(TransportDefinitionId, u32)>,
    child_server_only: bool,
    parent_name: String,
    child_name: String,
}

/// Resolve exact `self.<field>` roles over the complete relocation-linked
/// graph before adapting any one component in isolation.
///
/// Token paths never participate. A source edge identifies the rustc-resolved
/// child definition, the child descriptor identifies its exact field, and the
/// deferred binding commits the parent field ID plus both latent identities.
fn resolve_deferred_projection_roles(
    plan: &TransportLinkPlan,
) -> Result<ResolvedProjectionRoles, AbiAdapterError> {
    let components = plan
        .components
        .iter()
        .map(|component| (component.definition, component))
        .collect::<BTreeMap<_, _>>();
    let mut roles = BTreeMap::<(TransportDefinitionId, u32), Option<ResolvedProjectionRole>>::new();
    let mut names = BTreeMap::<(TransportDefinitionId, u32), String>::new();
    let mut fields_by_name =
        BTreeMap::<(TransportDefinitionId, String), (u32, Option<ResolvedProjectionRole>)>::new();

    for component in &plan.components {
        let root = payload_object("component", component.definition, &component.payload)?;
        let descriptor = required_object(
            root,
            "component_descriptor",
            "component",
            component.definition,
        )?;
        for (index, field) in
            required_array(descriptor, "fields", "component", component.definition)?
                .iter()
                .enumerate()
        {
            let field = field.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    component.definition,
                    format!("fields[{index}] must be an object"),
                )
            })?;
            let field_id = checked_u32(
                required_u64(field, "id", "component", component.definition)?,
                "component field ID",
                component.definition,
            )?;
            let name = required_str(field, "name", "component", component.definition)?.to_owned();
            let role = match required_str(field, "kind", "component", component.definition)? {
                "deferred_projection" => None,
                "prop" => Some(ResolvedProjectionRole::Value),
                "handler" => {
                    let shape = match required_str(
                        field,
                        "callback_shape",
                        "component",
                        component.definition,
                    )? {
                        "required" => InboundCallbackShape::Required,
                        "optional" => InboundCallbackShape::Optional,
                        other => {
                            return Err(invalid(
                                "component",
                                component.definition,
                                format!(
                                    "callback field `{name}` has unsupported callback shape `{other}`"
                                ),
                            ));
                        }
                    };
                    Some(ResolvedProjectionRole::Callback(shape))
                }
                "node_ref" => Some(ResolvedProjectionRole::NodeRef),
                "server_prop" | "static_slot" => Some(ResolvedProjectionRole::ServerOnly),
                other => {
                    return Err(invalid(
                        "component",
                        component.definition,
                        format!(
                            "unsupported component field kind `{other}` while resolving deferred projections"
                        ),
                    ));
                }
            };
            let key = (component.definition, field_id);
            if roles.insert(key, role).is_some()
                || names.insert(key, name.clone()).is_some()
                || fields_by_name
                    .insert((component.definition, name.clone()), (field_id, role))
                    .is_some()
            {
                return Err(invalid(
                    "component",
                    component.definition,
                    format!("component field `{name}` is not uniquely identified"),
                ));
            }
        }
    }

    let mut constraints = Vec::new();
    for component in &plan.components {
        let root = payload_object("component", component.definition, &component.payload)?;
        let transport = required_object(root, "transport", "component", component.definition)?;
        for (edge_index, edge) in required_array(
            transport,
            "direct_component_edges",
            "component",
            component.definition,
        )?
        .iter()
        .enumerate()
        {
            let edge = edge.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    component.definition,
                    format!("direct_component_edges[{edge_index}] must be an object"),
                )
            })?;
            let source_edge =
                parse_record_field(edge, "source_edge_id", "component", component.definition)?;
            let target = component
                .component_edges
                .iter()
                .find(|candidate| candidate.source_edge == source_edge)
                .map(|candidate| candidate.target)
                .ok_or_else(|| {
                    invalid(
                        "component",
                        component.definition,
                        format!(
                            "deferred projection edge {} has no rustc-resolved child",
                            hex_record(&source_edge)
                        ),
                    )
                })?;
            if !components.contains_key(&target) {
                return Err(invalid(
                    "component",
                    component.definition,
                    "deferred projection edge resolves to an unknown component definition",
                ));
            }
            for (field_index, field) in
                required_array(edge, "fields", "component", component.definition)?
                    .iter()
                    .enumerate()
            {
                let field = field.as_object().ok_or_else(|| {
                    invalid(
                        "component",
                        component.definition,
                        format!(
                            "direct_component_edges[{edge_index}].fields[{field_index}] must be an object"
                        ),
                    )
                })?;
                let binding = required_object(field, "binding", "component", component.definition)?;
                if required_str(binding, "kind", "component", component.definition)?
                    != "deferred_projection"
                {
                    continue;
                }
                let parent_field = checked_u32(
                    required_u64(
                        binding,
                        "parent_field_id",
                        "component",
                        component.definition,
                    )?,
                    "deferred parent field ID",
                    component.definition,
                )?;
                let parent = (component.definition, parent_field);
                let parent_name = names.get(&parent).cloned().ok_or_else(|| {
                    invalid(
                        "component",
                        component.definition,
                        format!(
                            "deferred projection references unknown parent field {parent_field}"
                        ),
                    )
                })?;
                let child_name =
                    required_str(field, "name", "component", component.definition)?.to_owned();
                let child_schema = fields_by_name.get(&(target, child_name.clone()));
                let child_server_only = matches!(
                    child_schema,
                    None | Some((_, Some(ResolvedProjectionRole::ServerOnly)))
                );
                let child = (!child_server_only)
                    .then(|| child_schema.map(|(field, _)| (target, *field)))
                    .flatten();
                constraints.push(ProjectionConstraint {
                    parent,
                    child,
                    child_server_only,
                    parent_name,
                    child_name,
                });
            }
        }
    }

    let merge = |left: ResolvedProjectionRole,
                 right: ResolvedProjectionRole,
                 constraint: &ProjectionConstraint|
     -> Result<ResolvedProjectionRole, AbiAdapterError> {
        if left == right {
            Ok(left)
        } else {
            Err(invalid(
                "component",
                constraint.parent.0,
                format!(
                    "deferred projection `self.{}` feeds child field `{}` with conflicting roles: {} versus {}\nhelp: use separate component fields for values, callbacks, and NodeRef handles",
                    constraint.parent_name,
                    constraint.child_name,
                    left.label(),
                    right.label(),
                ),
            ))
        }
    };

    let mut changed = true;
    while changed {
        changed = false;
        for constraint in &constraints {
            // A static/unused child assignment places no browser-role demand on
            // the parent projection. Another outgoing edge may still prove
            // that same parent field is a live value, callback, or NodeRef.
            if constraint.child_server_only {
                continue;
            }
            let parent_role = roles.get(&constraint.parent).copied().flatten();
            let child_role = constraint
                .child
                .and_then(|child| roles.get(&child).copied().flatten());
            match (parent_role, child_role) {
                (Some(parent), Some(child)) => {
                    merge(parent, child, constraint)?;
                }
                (None, Some(child)) => {
                    roles.insert(constraint.parent, Some(child));
                    changed = true;
                }
                (Some(parent), None) => {
                    let child = constraint
                        .child
                        .expect("a non-server unresolved constraint has a child field");
                    roles.insert(child, Some(parent));
                    changed = true;
                }
                (None, None) => {}
            }
        }
    }

    // A closed projection cycle with no browser-consuming endpoint is
    // semantically server-only. This is the only default; any concrete role
    // elsewhere in the equality component has already propagated above.
    let fields_by_name = fields_by_name
        .into_iter()
        .map(|((definition, name), (field, _))| {
            let role = roles
                .get(&(definition, field))
                .copied()
                .flatten()
                .unwrap_or(ResolvedProjectionRole::ServerOnly);
            ((definition, name), role)
        })
        .collect();
    let fields = roles
        .into_iter()
        .map(|(key, role)| (key, role.unwrap_or(ResolvedProjectionRole::ServerOnly)))
        .collect();
    Ok(ResolvedProjectionRoles {
        fields,
        fields_by_name,
    })
}

#[cfg(test)]
fn inbound_temporal_mode_requires_client_program(mode: TemporalPropMode) -> bool {
    match mode {
        TemporalPropMode::LiveRead
        | TemporalPropMode::SeedOnce(_)
        | TemporalPropMode::EffectDependency
        | TemporalPropMode::QueryKeyDependency => true,
        TemporalPropMode::ForbiddenProviderSeed | TemporalPropMode::ForbiddenBoundaryCrossing => {
            false
        }
        // A future temporal mode must remain conservative until this bridge
        // understands whether it creates browser work.
        _ => true,
    }
}

#[derive(Clone, Debug)]
struct ValidatedComponentFieldWitness {
    field_record: TransportRecordId,
    witness: WitnessRecordId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidatedPayloadHandlerValue {
    prelude_local_index: u32,
    payload_arity: u8,
    body_ir: Value,
    body_hash: SemanticHash,
    dependencies: ClientBlockExternalDependencies,
    /// Browser capability operations in stable Client IR preorder.
    ///
    /// This is recomputed from `body_ir`; transport dependency claims are not
    /// trusted as operation provenance.
    browser_operations: Vec<LegacyBrowserOperation>,
}

/// Collision-free canonical value ranges for one schema-v9 component.
///
/// Latent inbound field values retain their already-dense producer IDs.
/// Surviving legacy signals and expressions occupy the following ranges in
/// source ID order, and transported payload callbacks occupy the final range,
/// so independently authored value categories can never alias.
///
/// Legacy descriptor IDs are not necessarily dense: optimization may remove a
/// dead signal while preserving the IDs referenced by the remaining graph.
/// Keep an explicit legacy-ID-to-dense-ordinal map instead of treating a
/// `SignalId` as a vector index.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ComponentValueLayout {
    inbound_count: u32,
    signal_start: u64,
    signal_count: u32,
    signal_ordinals: Option<BTreeMap<u32, u32>>,
    expr_start: u64,
    expr_count: u32,
    expr_ordinals: Option<BTreeMap<u32, u32>>,
    payload_handler_start: u64,
    payload_handler_count: u32,
    node_ref_handle_start: u64,
    node_ref_handle_count: u32,
    node_ref_values: BTreeMap<u32, LocalValueId>,
}

impl ComponentValueLayout {
    #[cfg(test)]
    fn new(
        definition: TransportDefinitionId,
        inbound_fields: &[ValidatedInboundField],
        descriptor: &Map<String, Value>,
        payload_handler_count: usize,
    ) -> Result<Self, AbiAdapterError> {
        Self::new_with_handles(
            definition,
            &ValidatedInboundInputs {
                fields: inbound_fields.to_vec(),
                prop_uses: Vec::new(),
                node_refs: Vec::new(),
                projection_signals: BTreeSet::new(),
            },
            descriptor,
            payload_handler_count,
            &[],
        )
    }

    fn new_with_handles(
        definition: TransportDefinitionId,
        inbound: &ValidatedInboundInputs,
        descriptor: &Map<String, Value>,
        payload_handler_count: usize,
        direct_component_edges: &[ValidatedDirectComponentEdge],
    ) -> Result<Self, AbiAdapterError> {
        for (index, field) in inbound.fields.iter().enumerate() {
            if field.value != index as u64 {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "inbound field `{}` has value ID {}, expected dense ID {index}",
                        field.name, field.value
                    ),
                ));
            }
        }

        let signals = required_array(descriptor, "signals", "component", definition)?;
        let mut signal_ordinals = BTreeMap::new();
        let mut previous_signal = None;
        for (index, signal) in signals.iter().enumerate() {
            let signal = signal.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("signals[{index}] must be an object"),
                )
            })?;
            let actual = checked_u32(
                required_u64(signal, "id", "component", definition)?,
                "legacy signal ID",
                definition,
            )?;
            if previous_signal.is_some_and(|previous| actual <= previous) {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "legacy signal IDs must be strictly increasing after optimization; \
                         signals[{index}] has ID {actual} after {}",
                        previous_signal.expect("checked above")
                    ),
                ));
            }
            let ordinal = u32::try_from(index).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "legacy signal count exceeds the canonical u32 value space",
                )
            })?;
            signal_ordinals.insert(actual, ordinal);
            previous_signal = Some(actual);
        }

        let exprs = required_array(descriptor, "exprs", "component", definition)?;
        let mut expr_ordinals = BTreeMap::new();
        let mut previous_expr = None;
        for (index, expr) in exprs.iter().enumerate() {
            let expr = expr.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("exprs[{index}] must be an object"),
                )
            })?;
            let actual = checked_u32(
                required_u64(expr, "id", "component", definition)?,
                "legacy expression ID",
                definition,
            )?;
            if previous_expr.is_some_and(|previous| actual <= previous) {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "legacy expression IDs must be strictly increasing after optimization; \
                         exprs[{index}] has ID {actual} after {}",
                        previous_expr.expect("checked above")
                    ),
                ));
            }
            let ordinal = u32::try_from(index).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "legacy expression count exceeds the canonical u32 value space",
                )
            })?;
            expr_ordinals.insert(actual, ordinal);
            previous_expr = Some(actual);
        }

        let declared_node_refs = required_array(descriptor, "node_refs", "component", definition)?
            .iter()
            .enumerate()
            .map(|(index, node_ref)| {
                let node_ref = node_ref.as_object().ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("node_refs[{index}] must be an object"),
                    )
                })?;
                checked_u32(
                    required_u64(node_ref, "id", "component", definition)?,
                    "legacy node-ref ID",
                    definition,
                )
            })
            .collect::<Result<BTreeSet<_>, AbiAdapterError>>()?;
        let mut inbound_node_ref_values = BTreeMap::new();
        for node_ref in &inbound.node_refs {
            if inbound_node_ref_values
                .insert(node_ref.node_ref, LocalValueId::new(node_ref.value))
                .is_some()
            {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "producer NodeRef {} is represented by more than one inbound field",
                        node_ref.node_ref
                    ),
                ));
            }
        }
        let handle_sources = direct_component_edges
            .iter()
            .flat_map(|edge| edge.fields.iter())
            .filter_map(|field| match field.binding {
                ValidatedDirectComponentBinding::NodeRefHandle { node_ref } => Some(node_ref),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if let Some(unknown) = handle_sources
            .iter()
            .find(|node_ref| !declared_node_refs.contains(node_ref))
        {
            return Err(invalid(
                "component",
                definition,
                format!("NodeRef handle refers to unknown producer NodeRef {unknown}"),
            ));
        }
        let allocated_node_refs = handle_sources
            .iter()
            .copied()
            .filter(|node_ref| !inbound_node_ref_values.contains_key(node_ref))
            .collect::<Vec<_>>();

        let mut layout = Self::from_allocation(
            definition,
            u64::try_from(inbound.fields.len()).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "inbound value count exceeds the canonical u64 count space",
                )
            })?,
            u64::try_from(signals.len()).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "legacy signal count exceeds the canonical u64 count space",
                )
            })?,
            u64::try_from(exprs.len()).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "legacy expression count exceeds the canonical u64 count space",
                )
            })?,
            u64::try_from(payload_handler_count).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "payload-handler count exceeds the canonical u64 count space",
                )
            })?,
            u64::try_from(allocated_node_refs.len()).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "NodeRef handle count exceeds the canonical u64 count space",
                )
            })?,
            Some(signal_ordinals),
            Some(expr_ordinals),
        )?;
        layout.node_ref_values = inbound_node_ref_values;
        for (ordinal, node_ref) in allocated_node_refs.into_iter().enumerate() {
            let ordinal = u64::try_from(ordinal).expect("usize fits u64");
            let value = layout
                .node_ref_handle_start
                .checked_add(ordinal)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        "NodeRef handle value ID exceeds the canonical u32 value space",
                    )
                })?;
            layout
                .node_ref_values
                .insert(node_ref, LocalValueId::new(value));
        }
        Ok(layout)
    }

    #[cfg(test)]
    fn from_counts(
        definition: TransportDefinitionId,
        inbound_count: u64,
        signal_count: u64,
        payload_handler_count: u64,
    ) -> Result<Self, AbiAdapterError> {
        Self::from_allocation(
            definition,
            inbound_count,
            signal_count,
            0,
            payload_handler_count,
            0,
            None,
            None,
        )
    }

    fn from_allocation(
        definition: TransportDefinitionId,
        inbound_count: u64,
        signal_count: u64,
        expr_count: u64,
        payload_handler_count: u64,
        node_ref_handle_count: u64,
        signal_ordinals: Option<BTreeMap<u32, u32>>,
        expr_ordinals: Option<BTreeMap<u32, u32>>,
    ) -> Result<Self, AbiAdapterError> {
        let inbound_count = u32::try_from(inbound_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "inbound value count exceeds the canonical u32 value space",
            )
        })?;
        let signal_count = u32::try_from(signal_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy signal count exceeds the canonical u32 value space",
            )
        })?;
        let expr_count = u32::try_from(expr_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy expression count exceeds the canonical u32 value space",
            )
        })?;
        let payload_handler_count = u32::try_from(payload_handler_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "payload-handler count exceeds the canonical u32 value space",
            )
        })?;
        let node_ref_handle_count = u32::try_from(node_ref_handle_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "NodeRef handle count exceeds the canonical u32 value space",
            )
        })?;
        let signal_start = u64::from(inbound_count);
        let expr_start = signal_start
            .checked_add(u64::from(signal_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy signals exhaust the canonical u64 value allocation space",
                )
            })?;
        let payload_handler_start =
            expr_start
                .checked_add(u64::from(expr_count))
                .ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        "legacy expressions exhaust the canonical u64 value allocation space",
                    )
                })?;
        let node_ref_handle_start = payload_handler_start
            .checked_add(u64::from(payload_handler_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "payload handlers exhaust the canonical u64 value allocation space",
                )
            })?;
        let allocation_end = node_ref_handle_start
            .checked_add(u64::from(node_ref_handle_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "NodeRef handles exhaust the canonical u64 value allocation space",
                )
            })?;
        if allocation_end > u64::from(u32::MAX) + 1 {
            return Err(invalid(
                "component",
                definition,
                "inbound values and legacy signals exhaust the canonical u32 value space",
            ));
        }

        Ok(Self {
            inbound_count,
            signal_start,
            signal_count,
            signal_ordinals,
            expr_start,
            expr_count,
            expr_ordinals,
            payload_handler_start,
            payload_handler_count,
            node_ref_handle_start,
            node_ref_handle_count,
            node_ref_values: BTreeMap::new(),
        })
    }

    fn inbound(
        &self,
        definition: TransportDefinitionId,
        value: u64,
    ) -> Result<LocalValueId, AbiAdapterError> {
        let value = u32::try_from(value).map_err(|_| {
            invalid(
                "component",
                definition,
                format!("inbound value ID {value} exceeds the canonical u32 value space"),
            )
        })?;
        if value >= self.inbound_count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound value ID {value} is outside its allocated range of {}",
                    self.inbound_count
                ),
            ));
        }
        Ok(LocalValueId::new(value))
    }

    fn signal(
        &self,
        definition: TransportDefinitionId,
        signal: u32,
    ) -> Result<LocalValueId, AbiAdapterError> {
        let ordinal = match &self.signal_ordinals {
            Some(ordinals) => ordinals.get(&signal).copied().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!(
                        "legacy signal source {signal} is not present in the optimized descriptor"
                    ),
                )
            })?,
            None if signal < self.signal_count => signal,
            None => {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "legacy signal source {signal} is outside its allocated range of {}",
                        self.signal_count
                    ),
                ));
            }
        };
        let value = self
            .signal_start
            .checked_add(u64::from(ordinal))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy signal value ID exceeds the canonical u32 value space",
                )
            })?;
        Ok(LocalValueId::new(value))
    }

    fn payload_handler(
        &self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<LocalValueId, AbiAdapterError> {
        if ordinal >= self.payload_handler_count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "payload-handler ordinal {ordinal} is outside its allocated range of {}",
                    self.payload_handler_count
                ),
            ));
        }
        let value = self
            .payload_handler_start
            .checked_add(u64::from(ordinal))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "payload-handler value ID exceeds the canonical u32 value space",
                )
            })?;
        Ok(LocalValueId::new(value))
    }

    fn expr(
        &self,
        definition: TransportDefinitionId,
        expr: u32,
    ) -> Result<LocalValueId, AbiAdapterError> {
        let ordinal = match &self.expr_ordinals {
            Some(ordinals) => ordinals.get(&expr).copied().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!(
                        "legacy expression source {expr} is not present in the optimized descriptor"
                    ),
                )
            })?,
            None if expr < self.expr_count => expr,
            None => {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "legacy expression source {expr} is outside its allocated range of {}",
                        self.expr_count
                    ),
                ));
            }
        };
        let value = self
            .expr_start
            .checked_add(u64::from(ordinal))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy expression value ID exceeds the canonical u32 value space",
                )
            })?;
        Ok(LocalValueId::new(value))
    }

    fn node_ref(
        &self,
        definition: TransportDefinitionId,
        node_ref: u32,
    ) -> Result<LocalValueId, AbiAdapterError> {
        self.node_ref_values.get(&node_ref).copied().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!(
                    "producer NodeRef {node_ref} has no canonical handle value; only an inbound or direct-aliased NodeRef may cross a component edge"
                ),
            )
        })
    }

    fn populate(&self, values: &mut BTreeSet<LocalValueId>) {
        values.extend((0..self.inbound_count).map(LocalValueId::new));
        values.extend((0..self.signal_count).map(|signal| {
            LocalValueId::new(
                u32::try_from(self.signal_start + u64::from(signal))
                    .expect("validated component value allocation fits u32"),
            )
        }));
        values.extend((0..self.expr_count).map(|expr| {
            LocalValueId::new(
                u32::try_from(self.expr_start + u64::from(expr))
                    .expect("validated component value allocation fits u32"),
            )
        }));
        values.extend((0..self.payload_handler_count).map(|handler| {
            LocalValueId::new(
                u32::try_from(self.payload_handler_start + u64::from(handler))
                    .expect("validated component value allocation fits u32"),
            )
        }));
        values.extend((0..self.node_ref_handle_count).map(|node_ref| {
            LocalValueId::new(
                u32::try_from(self.node_ref_handle_start + u64::from(node_ref))
                    .expect("validated component value allocation fits u32"),
            )
        }));
    }
}

/// Dense source-provenance ranges for inbound prop sites, transported
/// callbacks, legacy graph declarations/effects, then direct components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ComponentOriginLayout {
    prop_use_count: u32,
    payload_handler_start: u64,
    payload_handler_count: u32,
    signal_start: u64,
    signal_count: u32,
    expr_start: u64,
    expr_count: u32,
    handler_start: u64,
    handler_count: u32,
    browser_operation_start: u64,
    browser_operation_count: u32,
    node_ref_start: u64,
    node_ref_count: u32,
    query_start: u64,
    query_count: u32,
    mutation_start: u64,
    mutation_count: u32,
    shared_state_start: u64,
    shared_state_count: u32,
    binding_start: u64,
    binding_count: u32,
    component_start: u64,
    component_count: u32,
}

impl ComponentOriginLayout {
    fn new(
        definition: TransportDefinitionId,
        prop_use_count: usize,
        payload_handler_count: usize,
        signal_count: usize,
        expr_count: usize,
        handler_count: usize,
        browser_operation_count: usize,
        node_ref_count: usize,
        query_count: usize,
        mutation_count: usize,
        shared_state_count: usize,
        binding_count: usize,
        component_count: usize,
    ) -> Result<Self, AbiAdapterError> {
        let prop_use_count = u64::try_from(prop_use_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "prop-use count exceeds the canonical u64 origin space",
            )
        })?;
        let component_count = u64::try_from(component_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component edge count exceeds the canonical u64 origin space",
            )
        })?;
        let payload_handler_count = u64::try_from(payload_handler_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "payload-handler count exceeds the canonical u64 origin space",
            )
        })?;
        let signal_count = u64::try_from(signal_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy signal count exceeds the canonical u64 origin space",
            )
        })?;
        let expr_count = u64::try_from(expr_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy expression count exceeds the canonical u64 origin space",
            )
        })?;
        let handler_count = u64::try_from(handler_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "bound legacy handler count exceeds the canonical u64 origin space",
            )
        })?;
        let browser_operation_count = u64::try_from(browser_operation_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "browser-operation count exceeds the canonical u64 origin space",
            )
        })?;
        let node_ref_count = u64::try_from(node_ref_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable node-reference count exceeds the canonical u64 origin space",
            )
        })?;
        let query_count = u64::try_from(query_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable query count exceeds the canonical u64 origin space",
            )
        })?;
        let mutation_count = u64::try_from(mutation_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable mutation count exceeds the canonical u64 origin space",
            )
        })?;
        let shared_state_count = u64::try_from(shared_state_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "active SharedState count exceeds the canonical u64 origin space",
            )
        })?;
        let binding_count = u64::try_from(binding_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding count exceeds the canonical u64 origin space",
            )
        })?;
        let prop_use_count = u32::try_from(prop_use_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "prop-use count exceeds the canonical u32 origin space",
            )
        })?;
        let component_count = u32::try_from(component_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component edge count exceeds the canonical u32 origin space",
            )
        })?;
        let payload_handler_count = u32::try_from(payload_handler_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "payload-handler count exceeds the canonical u32 origin space",
            )
        })?;
        let signal_count = u32::try_from(signal_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy signal count exceeds the canonical u32 origin space",
            )
        })?;
        let expr_count = u32::try_from(expr_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy expression count exceeds the canonical u32 origin space",
            )
        })?;
        let handler_count = u32::try_from(handler_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "bound legacy handler count exceeds the canonical u32 origin space",
            )
        })?;
        let browser_operation_count = u32::try_from(browser_operation_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "browser-operation count exceeds the canonical u32 origin space",
            )
        })?;
        let node_ref_count = u32::try_from(node_ref_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable node-reference count exceeds the canonical u32 origin space",
            )
        })?;
        let query_count = u32::try_from(query_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable query count exceeds the canonical u32 origin space",
            )
        })?;
        let mutation_count = u32::try_from(mutation_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable mutation count exceeds the canonical u32 origin space",
            )
        })?;
        let shared_state_count = u32::try_from(shared_state_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "active SharedState count exceeds the canonical u32 origin space",
            )
        })?;
        let binding_count = u32::try_from(binding_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding count exceeds the canonical u32 origin space",
            )
        })?;
        let payload_handler_start = u64::from(prop_use_count);
        let signal_start = payload_handler_start
            .checked_add(u64::from(payload_handler_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "payload handlers exhaust the canonical u64 origin allocation space",
                )
            })?;
        let expr_start = signal_start
            .checked_add(u64::from(signal_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy signals exhaust the canonical u64 origin allocation space",
                )
            })?;
        let handler_start = expr_start
            .checked_add(u64::from(expr_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy expressions exhaust the canonical u64 origin allocation space",
                )
            })?;
        let browser_operation_start = handler_start
            .checked_add(u64::from(handler_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "bound legacy handlers exhaust the canonical u64 origin allocation space",
                )
            })?;
        let node_ref_start = browser_operation_start
            .checked_add(u64::from(browser_operation_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "browser operations exhaust the canonical u64 origin allocation space",
                )
            })?;
        let query_start = node_ref_start
            .checked_add(u64::from(node_ref_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "reachable node references exhaust the canonical u64 origin allocation space",
                )
            })?;
        let mutation_start = query_start
            .checked_add(u64::from(query_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "reachable queries exhaust the canonical u64 origin allocation space",
                )
            })?;
        let shared_state_start = mutation_start
            .checked_add(u64::from(mutation_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "reachable mutations exhaust the canonical u64 origin allocation space",
                )
            })?;
        let binding_start = shared_state_start
            .checked_add(u64::from(shared_state_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "active SharedState acquisitions exhaust the canonical u64 origin allocation space",
                )
            })?;
        let component_start = binding_start
            .checked_add(u64::from(binding_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "legacy DOM bindings exhaust the canonical u64 origin allocation space",
                )
            })?;
        let allocation_end = component_start
            .checked_add(u64::from(component_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "component edges exhaust the canonical u64 origin allocation space",
                )
            })?;
        if allocation_end > u64::from(u32::MAX) + 1 {
            return Err(invalid(
                "component",
                definition,
                "component graph origins exhaust the canonical u32 origin space",
            ));
        }
        Ok(Self {
            prop_use_count,
            payload_handler_start,
            payload_handler_count,
            signal_start,
            signal_count,
            expr_start,
            expr_count,
            handler_start,
            handler_count,
            browser_operation_start,
            browser_operation_count,
            node_ref_start,
            node_ref_count,
            query_start,
            query_count,
            mutation_start,
            mutation_count,
            shared_state_start,
            shared_state_count,
            binding_start,
            binding_count,
            component_start,
            component_count,
        })
    }

    fn payload_handler(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        if ordinal >= self.payload_handler_count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "payload-handler ordinal {ordinal} is outside its allocated origin range of {}",
                    self.payload_handler_count
                ),
            ));
        }
        let origin = self
            .payload_handler_start
            .checked_add(u64::from(ordinal))
            .and_then(|origin| u32::try_from(origin).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "payload-handler origin ID exceeds the canonical u32 origin space",
                )
            })?;
        Ok(OriginId::new(origin))
    }

    fn prop_use(
        self,
        definition: TransportDefinitionId,
        site: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        if site >= self.prop_use_count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "prop-use site {site} is outside its allocated origin range of {}",
                    self.prop_use_count
                ),
            ));
        }
        Ok(OriginId::new(site))
    }

    fn signal(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "legacy signal",
            self.signal_start,
            self.signal_count,
            ordinal,
        )
    }

    fn expr(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "legacy expression",
            self.expr_start,
            self.expr_count,
            ordinal,
        )
    }

    fn handler(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "bound legacy handler",
            self.handler_start,
            self.handler_count,
            ordinal,
        )
    }

    fn binding(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "legacy DOM binding",
            self.binding_start,
            self.binding_count,
            ordinal,
        )
    }

    fn browser_operation(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "browser operation",
            self.browser_operation_start,
            self.browser_operation_count,
            ordinal,
        )
    }

    fn node_ref(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "reachable node reference",
            self.node_ref_start,
            self.node_ref_count,
            ordinal,
        )
    }

    fn query(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "reachable query",
            self.query_start,
            self.query_count,
            ordinal,
        )
    }

    fn mutation(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "reachable mutation",
            self.mutation_start,
            self.mutation_count,
            ordinal,
        )
    }

    fn shared_state(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        self.range_origin(
            definition,
            "active SharedState acquisition",
            self.shared_state_start,
            self.shared_state_count,
            ordinal,
        )
    }

    fn range_origin(
        self,
        definition: TransportDefinitionId,
        label: &'static str,
        start: u64,
        count: u32,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        if ordinal >= count {
            return Err(invalid(
                "component",
                definition,
                format!("{label} ordinal {ordinal} is outside its origin range of {count}"),
            ));
        }
        let origin = start
            .checked_add(u64::from(ordinal))
            .and_then(|origin| u32::try_from(origin).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("{label} origin ID exceeds the canonical u32 origin space"),
                )
            })?;
        Ok(OriginId::new(origin))
    }

    fn component(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<OriginId, AbiAdapterError> {
        if ordinal >= self.component_count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "component edge ordinal {ordinal} is outside its allocated origin range of {}",
                    self.component_count
                ),
            ));
        }
        let origin = self
            .component_start
            .checked_add(u64::from(ordinal))
            .and_then(|origin| u32::try_from(origin).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "component edge origin ID exceeds the canonical u32 origin space",
                )
            })?;
        Ok(OriginId::new(origin))
    }
}

fn populate_component_input_graph(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    inbound: &ValidatedInboundInputs,
    value_layout: &ComponentValueLayout,
    origin_layout: ComponentOriginLayout,
) -> Result<(), AbiAdapterError> {
    value_layout.populate(&mut descriptor.values);

    for field in &inbound.fields {
        let crossing = u32::try_from(field.crossing).map_err(|_| {
            invalid(
                "component",
                definition,
                format!(
                    "component field `{}` crossing hook exceeds the canonical u32 ID space",
                    field.name
                ),
            )
        })?;
        descriptor
            .crossing_hooks
            .insert(CrossingHookId::new(crossing));
    }

    for prop_use in &inbound.prop_uses {
        let site = PropSiteId::new(prop_use.site);
        let origin = origin_layout.prop_use(definition, prop_use.site)?;
        let value = value_layout.inbound(definition, u64::from(prop_use.value))?;
        descriptor.origins.insert(origin, OriginKind::SourceOnly);
        descriptor.prop_uses.insert(
            site,
            PropUse {
                field: ComponentFieldId::new(prop_use.field),
                value,
                temporal: prop_use.temporal,
                origin,
            },
        );
        if let Some(target_signal) = prop_use.target_signal {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Data,
                DependencyEndpoint::Value(value),
                DependencyEndpoint::Value(value_layout.signal(definition, target_signal)?),
                origin,
            )?;
        }
    }

    Ok(())
}

fn payload_handler_value_map(
    definition: TransportDefinitionId,
    handlers: &[ValidatedPayloadHandlerValue],
    value_layout: &ComponentValueLayout,
) -> Result<BTreeMap<u32, LocalValueId>, AbiAdapterError> {
    handlers
        .iter()
        .enumerate()
        .map(|(ordinal, handler)| {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "payload-handler count exceeds the canonical u32 value space",
                )
            })?;
            Ok((
                handler.prelude_local_index,
                value_layout.payload_handler(definition, ordinal)?,
            ))
        })
        .collect()
}

fn legacy_component_client_program_material(
    descriptor: &Map<String, Value>,
    handlers: &[ValidatedPayloadHandlerValue],
) -> Value {
    Value::Object(Map::from_iter([
        (
            "component_descriptor".to_owned(),
            Value::Object(descriptor.clone()),
        ),
        (
            "payload_handlers".to_owned(),
            Value::Array(
                handlers
                    .iter()
                    .map(|handler| {
                        serde_json::json!({
                            "body_hash": format!("sha256:{}", handler.body_hash),
                            "body_ir": handler.body_ir,
                            "payload_arity": handler.payload_arity,
                            "prelude_local_index": handler.prelude_local_index,
                        })
                    })
                    .collect(),
            ),
        ),
    ]))
}

fn populate_shared_state_references(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    graph: &LegacyClientGraph,
    shared_state: &ActiveSharedStatePlan,
    value_layout: &ComponentValueLayout,
    origin_layout: ComponentOriginLayout,
) -> Result<BTreeMap<u32, OriginId>, AbiAdapterError> {
    let mut store_origins = BTreeMap::new();
    for (ordinal, store) in shared_state.stores.iter().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "active SharedState acquisition")?;
        let origin = origin_layout.shared_state(definition, ordinal)?;
        descriptor.origins.insert(origin, OriginKind::SharedState);
        descriptor.shared_state.insert(
            SharedStateRefId::new(ordinal),
            SharedStateReference {
                type_key: store.type_key.clone(),
                schema_abi: store.schema_abi,
                origin,
            },
        );
        store_origins.insert(store.store_id, origin);
    }
    let mut signal_origins = BTreeMap::new();
    for signal in &graph.signals {
        let Some(field) = &signal.shared_field else {
            continue;
        };
        let origin = store_origins.get(&field.store).copied().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!(
                    "active SharedState signal {} lost store {} during canonical allocation",
                    signal.id, field.store,
                ),
            )
        })?;
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Component,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Value(value_layout.signal(definition, signal.id)?),
            origin,
        )?;
        signal_origins.insert(signal.id, origin);
    }
    Ok(signal_origins)
}

fn populate_payload_handler_graph(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    handlers: &[ValidatedPayloadHandlerValue],
    inbound: &ValidatedInboundInputs,
    value_layout: &ComponentValueLayout,
    origin_layout: ComponentOriginLayout,
    direct_helper_edges: &[ValidatedComponentHelperEdge],
    shared_state_origins: &BTreeMap<u32, OriginId>,
    browser_operation_ordinal: &mut u32,
) -> Result<(), AbiAdapterError> {
    let helper_ids = direct_helper_edges
        .iter()
        .enumerate()
        .map(|(dense, helper)| {
            let dense = u32::try_from(dense).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "reachable helper count exceeds the canonical u32 ID space",
                )
            })?;
            Ok((helper.local_ordinal, HelperCallEdgeId::new(dense)))
        })
        .collect::<Result<BTreeMap<_, _>, AbiAdapterError>>()?;
    let callback_values = inbound_callback_values(definition, inbound, value_layout)?;

    for (ordinal, handler) in handlers.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            invalid(
                "component",
                definition,
                "payload-handler count exceeds the canonical u32 graph space",
            )
        })?;
        let value = value_layout.payload_handler(definition, ordinal)?;
        let effect = LocalEffectId::new(ordinal);
        let origin = origin_layout.payload_handler(definition, ordinal)?;
        descriptor.origins.insert(origin, OriginKind::EventHandler);
        descriptor.effects.insert(effect, EffectKind::EventHandler);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Data,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Value(value),
            origin,
        )?;
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Value(value),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;

        let mut captured_signals = BTreeSet::new();
        for signal in handler
            .dependencies
            .signal_reads
            .iter()
            .chain(&handler.dependencies.signal_writes)
        {
            if captured_signals.insert(signal.0) {
                push_dependency(
                    definition,
                    descriptor,
                    DependencyEdgeKind::Capture,
                    DependencyEndpoint::Value(value_layout.signal(definition, signal.0)?),
                    DependencyEndpoint::Effect(effect),
                    origin,
                )?;
            }
        }
        for callback in &handler.dependencies.callback_slots {
            let Some(&callback_value) = callback_values.get(&callback.0) else {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "payload handler {} lost inbound callback slot {} during canonical allocation",
                        handler.prelude_local_index, callback.0,
                    ),
                ));
            };
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Capture,
                DependencyEndpoint::Value(callback_value),
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
        }
        for helper in &handler.dependencies.helper_edges {
            let Some(&helper) = helper_ids.get(&helper.index()) else {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "payload handler {} lost relocation-resolved helper edge {} during canonical allocation",
                        handler.prelude_local_index,
                        helper.index(),
                    ),
                ));
            };
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::HelperCall,
                DependencyEndpoint::Helper(helper),
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
        }
        populate_browser_operations(
            definition,
            descriptor,
            &handler.browser_operations,
            DependencyEndpoint::Effect(effect),
            value_layout,
            &callback_values,
            &helper_ids,
            origin_layout,
            browser_operation_ordinal,
        )?;
    }
    for (ordinal, handler) in handlers.iter().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "payload handler")?;
        let enclosing = DependencyEndpoint::Effect(LocalEffectId::new(ordinal));
        // Transported payload callbacks carry only their aggregate dependency
        // summary; explicit legacy expression/handler bodies instead carry
        // `LegacyClientOperation` records and are lowered below. These two
        // collections are disjoint, so synthesizing SharedState effects here
        // cannot duplicate operation-backed read/write effects.
        populate_payload_shared_state_effects(
            definition,
            descriptor,
            handler,
            enclosing,
            value_layout,
            shared_state_origins,
        )?;
    }
    Ok(())
}

fn populate_payload_shared_state_effects(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    handler: &ValidatedPayloadHandlerValue,
    enclosing: DependencyEndpoint,
    value_layout: &ComponentValueLayout,
    shared_state_origins: &BTreeMap<u32, OriginId>,
) -> Result<(), AbiAdapterError> {
    let mut seen = BTreeSet::new();
    for (read, signal) in handler
        .dependencies
        .signal_reads
        .iter()
        .map(|signal| (true, signal.0))
        .chain(
            handler
                .dependencies
                .signal_writes
                .iter()
                .map(|signal| (false, signal.0)),
        )
    {
        let Some(&origin) = shared_state_origins.get(&signal) else {
            continue;
        };
        if !seen.insert((read, signal)) {
            continue;
        }
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.effects.insert(
            effect,
            if read {
                EffectKind::SharedStateRead
            } else {
                EffectKind::SharedStateWrite
            },
        );
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
        if read {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                enclosing,
                origin,
            )?;
        } else {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Effect,
                enclosing,
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Effect,
                DependencyEndpoint::Effect(effect),
                DependencyEndpoint::Value(value_layout.signal(definition, signal)?),
                origin,
            )?;
        }
    }
    Ok(())
}

fn inbound_callback_values(
    definition: TransportDefinitionId,
    inbound: &ValidatedInboundInputs,
    value_layout: &ComponentValueLayout,
) -> Result<BTreeMap<u32, LocalValueId>, AbiAdapterError> {
    let mut callbacks = BTreeMap::new();
    for field in &inbound.fields {
        if field.kind != InboundFieldKind::Callback {
            continue;
        }
        let Some(slot) = field.callback_slot else {
            // A pure callback projection is forwarded through canonical
            // values and flattened to its original payload handler. Only a
            // component that actually invokes the callback owns a local slot.
            continue;
        };
        let value = value_layout.inbound(definition, field.value)?;
        if callbacks.insert(slot, value).is_some() {
            return Err(invalid(
                "component",
                definition,
                format!("multiple inbound callback fields claim callback slot {slot}"),
            ));
        }
    }
    Ok(callbacks)
}

fn validate_query_key_prop_targets(
    definition: TransportDefinitionId,
    inbound: &ValidatedInboundInputs,
    graph: &LegacyClientGraph,
) -> Result<(), AbiAdapterError> {
    let expressions = graph
        .expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let reachable_queries = graph
        .reachable_queries
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for prop_use in &inbound.prop_uses {
        if prop_use.temporal != TemporalPropMode::QueryKeyDependency {
            continue;
        }
        let Some(target_signal) = prop_use.target_signal else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "query-key prop site {} has no live-signal target",
                    prop_use.site
                ),
            ));
        };
        let consumed_by_reachable_key = graph.queries.iter().any(|query| {
            reachable_queries.contains(&query.id)
                && expressions
                    .get(&query.key_expression)
                    .is_some_and(|expression| {
                        expression
                            .dependencies
                            .signal_reads
                            .iter()
                            .any(|signal| signal.0 == target_signal)
                    })
        });
        if !consumed_by_reachable_key {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "query-key prop site {} targets signal {}, but no reachable query key reads it",
                    prop_use.site, target_signal
                ),
            ));
        }
    }
    Ok(())
}

fn validate_node_ref_handle_topology(
    definition: TransportDefinitionId,
    inbound: &ValidatedInboundInputs,
    graph: &LegacyClientGraph,
    direct_edges: &[ValidatedDirectComponentEdge],
) -> Result<(), AbiAdapterError> {
    let mut local_bindings = BTreeMap::<u32, usize>::new();
    for binding in &graph.bindings.node_refs {
        *local_bindings.entry(binding.node_ref).or_default() += 1;
    }
    let mut outgoing_aliases = BTreeMap::<u32, Vec<(u32, String)>>::new();
    for edge in direct_edges {
        for field in &edge.fields {
            let ValidatedDirectComponentBinding::NodeRefHandle { node_ref } = field.binding else {
                continue;
            };
            outgoing_aliases
                .entry(node_ref)
                .or_default()
                .push((edge.local_ordinal, field.name.clone()));
        }
    }

    let inbound_by_node_ref = inbound
        .node_refs
        .iter()
        .map(|node_ref| (node_ref.node_ref, node_ref))
        .collect::<BTreeMap<_, _>>();
    let involved = inbound_by_node_ref
        .keys()
        .copied()
        .chain(outgoing_aliases.keys().copied())
        .collect::<BTreeSet<_>>();
    for node_ref in involved {
        let local_count = local_bindings.get(&node_ref).copied().unwrap_or(0);
        let aliases = outgoing_aliases
            .get(&node_ref)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let sink_count = local_count
            .checked_add(aliases.len())
            .expect("two in-memory collection lengths fit usize");
        if sink_count != 1 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "NodeRef handle {node_ref} must have exactly one terminal local binding or one static direct alias, found {local_count} local binding(s) and {} alias edge(s)",
                    aliases.len()
                ),
            ));
        }
        if let Some(inbound) = inbound_by_node_ref.get(&node_ref) {
            match (inbound.bind_target, local_count) {
                (Some(_), 1) | (None, 0) => {}
                (Some(_), _) => {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "inbound NodeRef handle {node_ref} claims a rustc-checked bind target but has {local_count} local bindings"
                        ),
                    ));
                }
                (None, _) => {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "inbound NodeRef handle {node_ref} has a local binding without a rustc-checked bind target"
                        ),
                    ));
                }
            }
        } else if local_count != 0 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "locally created NodeRef handle {node_ref} cannot be both locally bound and passed through a component alias"
                ),
            ));
        }
    }
    Ok(())
}

fn build_component_codegen_bridge(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    inbound: &ValidatedInboundInputs,
    graph: &LegacyClientGraph,
    payload_handlers: &[ValidatedPayloadHandlerValue],
    value_layout: &ComponentValueLayout,
    view: &ValidatedView,
    direct_component_edges: &[ValidatedDirectComponentEdge],
) -> Result<ComponentCodegenBridge, AbiAdapterError> {
    let asset_id = required_str(descriptor, "asset_id", "component", definition)?.to_owned();
    if !valid_component_artifact_key(&asset_id) {
        return Err(invalid(
            "component",
            definition,
            format!("component asset_id `{asset_id}` is not a non-empty ASCII asset key"),
        ));
    }

    let inbound_fields = inbound
        .fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.kind != InboundFieldKind::ServerOnly)
        .map(|(field_index, field)| {
            let field_id = u32::try_from(field_index).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "inbound component field count exceeds the canonical u32 ID space",
                )
            })?;
            Ok(ComponentCodegenInboundField {
                field_id: ComponentFieldId::new(field_id),
                producer_value_id: checked_u32(
                    field.value,
                    "inbound producer value ID",
                    definition,
                )?,
                canonical_value: value_layout.inbound(definition, field.value)?,
                crossing_hook_id: CrossingHookId::new(checked_u32(
                    field.crossing,
                    "inbound crossing hook ID",
                    definition,
                )?),
                callback_slot_id: field.callback_slot,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;

    let inbound_targets = inbound
        .prop_uses
        .iter()
        .filter_map(|prop_use| {
            prop_use.target_signal.map(|producer_signal_id| {
                Ok(ComponentCodegenInboundTarget {
                    prop_site_id: PropSiteId::new(prop_use.site),
                    field_id: ComponentFieldId::new(prop_use.field),
                    producer_signal_id,
                    canonical_signal_value: value_layout.signal(definition, producer_signal_id)?,
                })
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;

    let signals = graph
        .signals
        .iter()
        .map(|signal| {
            Ok(ComponentCodegenSignal {
                producer_signal_id: signal.id,
                canonical_value: value_layout.signal(definition, signal.id)?,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;
    let expressions = graph
        .expressions
        .iter()
        .map(|expression| {
            Ok(ComponentCodegenExpression {
                producer_expression_id: expression.id,
                canonical_value: value_layout.expr(definition, expression.id)?,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;
    let payload_handlers = payload_handlers
        .iter()
        .enumerate()
        .map(|(ordinal, handler)| {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "payload-handler count exceeds the canonical u32 value space",
                )
            })?;
            Ok(ComponentCodegenPayloadHandler {
                prelude_local_index: handler.prelude_local_index,
                canonical_value: value_layout.payload_handler(definition, ordinal)?,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;

    let callback_values = inbound_callback_values(definition, inbound, value_layout)?;
    if callback_values.len() != graph.callback_slots.len() {
        return Err(invalid(
            "component",
            definition,
            format!(
                "producer callback slots must join one-to-one with inbound callback fields; \
                 found {} callback slots and {} inbound callback fields",
                graph.callback_slots.len(),
                callback_values.len(),
            ),
        ));
    }
    let callback_slots = graph
        .callback_slots
        .iter()
        .map(|slot| {
            let Some(&canonical_value) = callback_values.get(&slot.id) else {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "producer callback slot {} (`{}`) has no matching inbound callback field",
                        slot.id, slot.name,
                    ),
                ));
            };
            Ok(ComponentCodegenCallbackSlot {
                producer_callback_slot_id: slot.id,
                canonical_value,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;

    let inbound_node_refs = inbound
        .node_refs
        .iter()
        .map(|node_ref| ComponentCodegenInboundNodeRef {
            field_id: ComponentFieldId::new(node_ref.field),
            producer_node_ref_id: node_ref.node_ref,
            bind_target: node_ref.bind_target,
        })
        .collect::<Vec<_>>();
    let mut direct_node_ref_aliases = direct_component_edges
        .iter()
        .flat_map(|edge| {
            edge.fields.iter().filter_map(move |field| {
                let ValidatedDirectComponentBinding::NodeRefHandle { node_ref } = field.binding
                else {
                    return None;
                };
                Some((edge.local_ordinal, field.name.clone(), node_ref))
            })
        })
        .map(|(edge, field_name, producer_node_ref_id)| {
            Ok(ComponentCodegenDirectNodeRefAlias {
                edge_id: ComponentEdgeId::new(edge),
                field_name,
                producer_node_ref_id,
                canonical_value: value_layout.node_ref(definition, producer_node_ref_id)?,
            })
        })
        .collect::<Result<Vec<_>, AbiAdapterError>>()?;
    direct_node_ref_aliases.sort_by(|left, right| {
        (left.edge_id, left.field_name.as_str()).cmp(&(right.edge_id, right.field_name.as_str()))
    });

    Ok(ComponentCodegenBridge {
        asset_id: asset_id.clone(),
        fragment_key: asset_id,
        fragment_root: view.fragment_root,
        inbound_fields,
        inbound_targets,
        signals,
        expressions,
        payload_handlers,
        callback_slots,
        inbound_node_refs,
        direct_node_ref_aliases,
        uses_queries: !graph.reachable_queries.is_empty(),
        uses_mutations: !graph.reachable_mutations.is_empty(),
    })
}

fn valid_component_artifact_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Validate producer-local codegen coordinates against their canonical
/// component graph.
///
/// The canonical descriptor deliberately does not make producer IDs semantic
/// component identity. This check instead proves that the relocation table is
/// total, non-overlapping, deterministically ordered, and attached to the
/// canonical values/fields it claims before final codegen can observe it.
pub fn validate_component_codegen_bridge(
    codegen: &ComponentCodegenBridge,
    descriptor: &ComponentDescriptor,
) -> Result<(), String> {
    if !valid_component_artifact_key(&codegen.asset_id) {
        return Err(format!(
            "asset_id `{}` is not a non-empty ASCII asset key",
            codegen.asset_id
        ));
    }
    if codegen.fragment_key != codegen.asset_id {
        return Err(format!(
            "fragment key `{}` does not match the legacy producer asset key `{}`",
            codegen.fragment_key, codegen.asset_id
        ));
    }
    if matches!(
        codegen.fragment_root,
        ComponentFragmentRoot::ExactOneElement
    ) && descriptor.span != SpanCardinality::Exact(1)
    {
        return Err(
            "exact-one-element fragment root disagrees with the canonical DOM span".to_owned(),
        );
    }
    let canonical_uses_queries = descriptor
        .origins
        .values()
        .any(|kind| matches!(kind, OriginKind::Query))
        || descriptor.effects.values().any(|kind| {
            matches!(
                kind,
                EffectKind::QueryObservation
                    | EffectKind::QueryRefetch
                    | EffectKind::QueryInvalidation
            )
        });
    if codegen.uses_queries != canonical_uses_queries {
        return Err(format!(
            "component codegen uses_queries={} disagrees with canonical reachable query work={canonical_uses_queries}",
            codegen.uses_queries
        ));
    }
    let canonical_uses_mutations = descriptor
        .origins
        .values()
        .any(|kind| matches!(kind, OriginKind::Mutation))
        || descriptor
            .effects
            .values()
            .any(|kind| matches!(kind, EffectKind::Mutation));
    if codegen.uses_mutations != canonical_uses_mutations {
        return Err(format!(
            "component codegen uses_mutations={} disagrees with canonical reachable mutation work={canonical_uses_mutations}",
            codegen.uses_mutations
        ));
    }

    let expected_fields = descriptor
        .fields
        .iter()
        .filter(|(_, field)| field.client_value.is_some())
        .collect::<Vec<_>>();
    if expected_fields.len() != codegen.inbound_fields.len() {
        return Err(format!(
            "component codegen has {} inbound fields, but the canonical descriptor has {} client-visible fields",
            codegen.inbound_fields.len(),
            expected_fields.len(),
        ));
    }

    let mut expected_callbacks = Vec::new();
    let mut allocated_values = BTreeSet::new();
    for ((expected_field_id, _), mapping) in expected_fields.iter().zip(&codegen.inbound_fields) {
        if mapping.field_id != **expected_field_id
            || mapping.producer_value_id != expected_field_id.index()
        {
            return Err(format!(
                "inbound fields must preserve canonical source order; expected field/value {}, found field {} value {}",
                expected_field_id.index(),
                mapping.field_id.index(),
                mapping.producer_value_id,
            ));
        }
        let Some(field) = descriptor.fields.get(&mapping.field_id) else {
            return Err(format!(
                "inbound codegen field {} does not exist in the canonical descriptor",
                mapping.field_id.index(),
            ));
        };
        if field.client_value != Some(mapping.canonical_value)
            || field.crossing_hook != Some(mapping.crossing_hook_id)
        {
            return Err(format!(
                "inbound codegen field {} disagrees with its canonical value or crossing hook",
                mapping.field_id.index(),
            ));
        }
        match field.role {
            ComponentFieldRole::Value => {
                if mapping.callback_slot_id.is_some() {
                    return Err(format!(
                        "value field {} unexpectedly names a callback slot",
                        mapping.field_id.index(),
                    ));
                }
            }
            ComponentFieldRole::Callback => {
                if let Some(callback_slot_id) = mapping.callback_slot_id {
                    expected_callbacks.push((callback_slot_id, mapping.canonical_value));
                }
            }
            ComponentFieldRole::ClientHandle => {
                if mapping.callback_slot_id.is_some() {
                    return Err(format!(
                        "client-handle field {} unexpectedly names a callback slot",
                        mapping.field_id.index(),
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "client-visible field {} has unsupported codegen role {:?}",
                    mapping.field_id.index(),
                    field.role,
                ));
            }
        }
        if !allocated_values.insert(mapping.canonical_value) {
            return Err(format!(
                "canonical value {} is claimed by more than one producer value category",
                mapping.canonical_value.index(),
            ));
        }
    }
    if expected_fields
        .iter()
        .zip(&codegen.inbound_fields)
        .any(|((field_id, _), mapping)| **field_id != mapping.field_id)
    {
        return Err(
            "component codegen inbound fields do not exactly match canonical fields".into(),
        );
    }

    ensure_strict_component_codegen_order(
        "signal",
        codegen
            .signals
            .iter()
            .map(|mapping| mapping.producer_signal_id),
    )?;
    ensure_strict_component_codegen_order(
        "expression",
        codegen
            .expressions
            .iter()
            .map(|mapping| mapping.producer_expression_id),
    )?;
    ensure_strict_component_codegen_order(
        "payload handler",
        codegen
            .payload_handlers
            .iter()
            .map(|mapping| mapping.prelude_local_index),
    )?;
    ensure_strict_component_codegen_order(
        "callback slot",
        codegen
            .callback_slots
            .iter()
            .map(|mapping| mapping.producer_callback_slot_id),
    )?;
    ensure_strict_component_codegen_order(
        "inbound target",
        codegen
            .inbound_targets
            .iter()
            .map(|mapping| mapping.prop_site_id.index()),
    )?;
    ensure_strict_component_codegen_order(
        "inbound NodeRef",
        codegen
            .inbound_node_refs
            .iter()
            .map(|mapping| mapping.field_id.index()),
    )?;
    if codegen.direct_node_ref_aliases.windows(2).any(|pair| {
        (pair[0].edge_id, pair[0].field_name.as_str())
            >= (pair[1].edge_id, pair[1].field_name.as_str())
    }) {
        return Err(
            "direct NodeRef aliases must be unique and strictly ordered by edge/field".to_owned(),
        );
    }

    let expected_node_ref_fields = descriptor
        .fields
        .iter()
        .filter(|(_, field)| field.role == ComponentFieldRole::ClientHandle)
        .map(|(&field, _)| field)
        .collect::<Vec<_>>();
    let actual_node_ref_fields = codegen
        .inbound_node_refs
        .iter()
        .map(|mapping| mapping.field_id)
        .collect::<Vec<_>>();
    if actual_node_ref_fields != expected_node_ref_fields {
        return Err(format!(
            "inbound NodeRef mappings do not exactly match canonical client-handle fields; expected {expected_node_ref_fields:?}, found {actual_node_ref_fields:?}"
        ));
    }
    let mut producer_node_refs = BTreeSet::new();
    for mapping in &codegen.inbound_node_refs {
        if !producer_node_refs.insert(mapping.producer_node_ref_id) {
            return Err(format!(
                "producer NodeRef {} is represented by more than one inbound field",
                mapping.producer_node_ref_id
            ));
        }
        let field = &descriptor.fields[&mapping.field_id];
        if field.crossing_hook.is_none() || field.client_value.is_none() {
            return Err(format!(
                "inbound NodeRef field {} lacks its canonical value/crossing contract",
                mapping.field_id.index()
            ));
        }
    }
    let mut alias_values = BTreeMap::<LocalValueId, u32>::new();
    for alias in &codegen.direct_node_ref_aliases {
        let edge = descriptor
            .direct_components
            .get(&alias.edge_id)
            .ok_or_else(|| {
                format!(
                    "direct NodeRef alias refers to absent edge {}",
                    alias.edge_id.index()
                )
            })?;
        if edge.boundary != ComponentBoundary::Composed || edge.lifecycle_control.is_some() {
            return Err(format!(
                "direct NodeRef alias edge {} must be static composed",
                alias.edge_id.index()
            ));
        }
        let binding = edge.bindings.get(&alias.field_name).ok_or_else(|| {
            format!(
                "direct NodeRef alias edge {} field `{}` has no canonical binding",
                alias.edge_id.index(),
                alias.field_name
            )
        })?;
        if binding.source
            != (ComponentBindingSource::ClientHandle {
                value: alias.canonical_value,
            })
        {
            return Err(format!(
                "direct NodeRef alias edge {} field `{}` disagrees with its canonical ClientHandle value",
                alias.edge_id.index(),
                alias.field_name
            ));
        }
        if let Some(previous) =
            alias_values.insert(alias.canonical_value, alias.producer_node_ref_id)
        {
            return Err(format!(
                "canonical NodeRef handle value {} is aliased more than once (producer refs {previous} and {})",
                alias.canonical_value.index(),
                alias.producer_node_ref_id
            ));
        }
    }

    for canonical_value in codegen
        .signals
        .iter()
        .map(|mapping| mapping.canonical_value)
        .chain(
            codegen
                .expressions
                .iter()
                .map(|mapping| mapping.canonical_value),
        )
        .chain(
            codegen
                .payload_handlers
                .iter()
                .map(|mapping| mapping.canonical_value),
        )
    {
        if !allocated_values.insert(canonical_value) {
            return Err(format!(
                "canonical value {} is claimed by more than one producer value category",
                canonical_value.index(),
            ));
        }
    }
    for alias in &codegen.direct_node_ref_aliases {
        if allocated_values.contains(&alias.canonical_value) {
            let inbound_matches = codegen.inbound_node_refs.iter().any(|inbound| {
                descriptor.fields[&inbound.field_id].client_value == Some(alias.canonical_value)
                    && inbound.producer_node_ref_id == alias.producer_node_ref_id
            });
            if !inbound_matches {
                return Err(format!(
                    "canonical NodeRef handle value {} collides with another producer value category",
                    alias.canonical_value.index()
                ));
            }
        } else {
            allocated_values.insert(alias.canonical_value);
        }
    }
    if allocated_values != descriptor.values {
        return Err(format!(
            "component codegen value mappings do not exactly partition canonical values; mapped {:?}, descriptor {:?}",
            allocated_values
                .iter()
                .map(|value| value.index())
                .collect::<Vec<_>>(),
            descriptor
                .values
                .iter()
                .map(|value| value.index())
                .collect::<Vec<_>>(),
        ));
    }

    expected_callbacks.sort_unstable_by_key(|(slot, _)| *slot);
    let actual_callbacks = codegen
        .callback_slots
        .iter()
        .map(|mapping| (mapping.producer_callback_slot_id, mapping.canonical_value))
        .collect::<Vec<_>>();
    if actual_callbacks != expected_callbacks {
        return Err(format!(
            "callback-slot mappings do not exactly join inbound callback fields; expected {expected_callbacks:?}, found {actual_callbacks:?}"
        ));
    }

    let expected_target_sites = descriptor
        .prop_uses
        .iter()
        .filter_map(|(&site, prop_use)| match prop_use.temporal {
            TemporalPropMode::LiveRead | TemporalPropMode::QueryKeyDependency => Some(site),
            TemporalPropMode::SeedOnce(seed_site) if seed_site == site => Some(site),
            _ => None,
        })
        .collect::<Vec<_>>();
    if expected_target_sites.len() != codegen.inbound_targets.len() {
        return Err(format!(
            "component codegen has {} inbound signal targets, but the canonical descriptor has {} signal-backed prop-use sites",
            codegen.inbound_targets.len(),
            expected_target_sites.len(),
        ));
    }
    for (&expected_site, target) in expected_target_sites.iter().zip(&codegen.inbound_targets) {
        if target.prop_site_id != expected_site {
            return Err(format!(
                "inbound target order mismatch: expected prop site {}, found {}",
                expected_site.index(),
                target.prop_site_id.index(),
            ));
        }
        let prop_use = &descriptor.prop_uses[&expected_site];
        if prop_use.field != target.field_id {
            return Err(format!(
                "inbound target at prop site {} names field {}, expected {}",
                expected_site.index(),
                target.field_id.index(),
                prop_use.field.index(),
            ));
        }
        let Some(inbound) = codegen
            .inbound_fields
            .iter()
            .find(|mapping| mapping.field_id == target.field_id)
        else {
            return Err(format!(
                "inbound target at prop site {} names absent field {}",
                expected_site.index(),
                target.field_id.index(),
            ));
        };
        if inbound.canonical_value != prop_use.value {
            return Err(format!(
                "inbound target at prop site {} is attached to the wrong canonical field value",
                expected_site.index(),
            ));
        }
        let Some(signal) = codegen
            .signals
            .iter()
            .find(|mapping| mapping.producer_signal_id == target.producer_signal_id)
        else {
            return Err(format!(
                "inbound target at prop site {} names absent producer signal {}",
                expected_site.index(),
                target.producer_signal_id,
            ));
        };
        if signal.canonical_value != target.canonical_signal_value {
            return Err(format!(
                "inbound target at prop site {} maps producer signal {} to canonical value {}, but the signal table maps it to {}",
                expected_site.index(),
                target.producer_signal_id,
                target.canonical_signal_value.index(),
                signal.canonical_value.index(),
            ));
        }
        if !descriptor.dependencies.values().any(|edge| {
            edge.kind == DependencyEdgeKind::Data
                && edge.from == DependencyEndpoint::Value(prop_use.value)
                && edge.to == DependencyEndpoint::Value(target.canonical_signal_value)
        }) {
            return Err(format!(
                "inbound target at prop site {} has no canonical value-to-signal dependency",
                expected_site.index(),
            ));
        }
    }

    Ok(())
}

fn ensure_strict_component_codegen_order(
    label: &str,
    ids: impl IntoIterator<Item = u32>,
) -> Result<(), String> {
    let mut previous = None;
    for id in ids {
        if previous.is_some_and(|previous| previous >= id) {
            return Err(format!(
                "component codegen {label} IDs must be unique and strictly increasing"
            ));
        }
        previous = Some(id);
    }
    Ok(())
}

fn populate_legacy_client_graph(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    graph: &LegacyClientGraph,
    inbound: &ValidatedInboundInputs,
    value_layout: &ComponentValueLayout,
    origin_layout: ComponentOriginLayout,
    direct_helper_edges: &[ValidatedComponentHelperEdge],
    shared_state_origins: &BTreeMap<u32, OriginId>,
    reachable_node_refs: &[u32],
    browser_operation_ordinal: &mut u32,
) -> Result<(), AbiAdapterError> {
    let callback_values = inbound_callback_values(definition, inbound, value_layout)?;
    let live_input_signals = inbound_source_signals(inbound);
    let helper_ids = direct_helper_edges
        .iter()
        .enumerate()
        .map(|(dense, helper)| {
            let dense = u32::try_from(dense).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "reachable helper count exceeds the canonical u32 ID space",
                )
            })?;
            Ok((helper.local_ordinal, HelperCallEdgeId::new(dense)))
        })
        .collect::<Result<BTreeMap<_, _>, AbiAdapterError>>()?;
    let mut node_ref_effects = BTreeMap::new();
    let inbound_node_refs = inbound
        .node_refs
        .iter()
        .map(|node_ref| node_ref.node_ref)
        .collect::<BTreeSet<_>>();
    for (ordinal, node_ref) in reachable_node_refs.iter().copied().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "reachable node reference")?;
        let origin = origin_layout.node_ref(definition, ordinal)?;
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.origins.insert(
            origin,
            if inbound_node_refs.contains(&node_ref) {
                OriginKind::SourceOnly
            } else {
                OriginKind::NodeRef
            },
        );
        descriptor.effects.insert(effect, EffectKind::NodeReference);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
        node_ref_effects.insert(node_ref, effect);
    }
    let queries_by_id = graph
        .queries
        .iter()
        .map(|query| (query.id, query))
        .collect::<BTreeMap<_, _>>();
    let mut query_origins = BTreeMap::new();
    for (ordinal, query) in graph.reachable_queries.iter().copied().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "reachable query")?;
        let origin = origin_layout.query(definition, ordinal)?;
        descriptor.origins.insert(origin, OriginKind::Query);
        query_origins.insert(query, origin);
    }
    let mutations_by_id = graph
        .mutations
        .iter()
        .map(|mutation| (mutation.id, mutation))
        .collect::<BTreeMap<_, _>>();
    let mut mutation_origins = BTreeMap::new();
    for (ordinal, mutation) in graph.reachable_mutations.iter().copied().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "reachable mutation")?;
        let origin = origin_layout.mutation(definition, ordinal)?;
        descriptor.origins.insert(origin, OriginKind::Mutation);
        mutation_origins.insert(mutation, origin);
    }
    let reachable_expressions = graph
        .reachable_expressions
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    for (ordinal, signal) in graph.signals.iter().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "legacy signal")?;
        let origin = origin_layout.signal(definition, ordinal)?;
        let value = value_layout.signal(definition, signal.id)?;
        descriptor.origins.insert(
            origin,
            if signal.shared_field.is_some()
                || signal.derived_expression.is_some()
                || live_input_signals.contains(&signal.id)
            {
                OriginKind::SourceOnly
            } else {
                OriginKind::WritableSignal
            },
        );
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Data,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Value(value),
            origin,
        )?;
        if let Some(expression) = signal.derived_expression {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Data,
                DependencyEndpoint::Value(value_layout.expr(definition, expression)?),
                DependencyEndpoint::Value(value),
                origin,
            )?;
        }
    }

    for (ordinal, expression) in graph.expressions.iter().enumerate() {
        let ordinal = checked_ordinal(definition, ordinal, "legacy expression")?;
        let origin = origin_layout.expr(definition, ordinal)?;
        let value = value_layout.expr(definition, expression.id)?;
        descriptor.origins.insert(origin, OriginKind::SourceOnly);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Data,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Value(value),
            origin,
        )?;
        push_legacy_external_dependencies(
            definition,
            descriptor,
            &expression.dependencies,
            value_layout,
            &callback_values,
            &helper_ids,
            DependencyEndpoint::Value(value),
            DependencyEdgeKind::Data,
            origin,
        )?;
        if reachable_expressions.contains(&expression.id) {
            populate_query_operations(
                definition,
                descriptor,
                &expression.client_operations,
                DependencyEndpoint::Value(value),
                &query_origins,
                &queries_by_id,
                value_layout,
            )?;
            populate_mutation_operations(
                definition,
                descriptor,
                &expression.client_operations,
                DependencyEndpoint::Value(value),
                &mutation_origins,
                &mutations_by_id,
                value_layout,
            )?;
        }
        populate_shared_state_operations(
            definition,
            descriptor,
            &expression.client_operations,
            DependencyEndpoint::Value(value),
            value_layout,
            &callback_values,
            &helper_ids,
            shared_state_origins,
        )?;
        populate_node_ref_operations(
            definition,
            descriptor,
            &expression.client_operations,
            DependencyEndpoint::Value(value),
            &node_ref_effects,
            origin,
        )?;
        populate_browser_operations(
            definition,
            descriptor,
            &expression.browser_operations,
            DependencyEndpoint::Value(value),
            value_layout,
            &callback_values,
            &helper_ids,
            origin_layout,
            browser_operation_ordinal,
        )?;
    }

    let bound_handler_ids = graph
        .bindings
        .events
        .iter()
        .map(|binding| binding.handler)
        .collect::<BTreeSet<_>>();
    let mut bound_ordinal = 0_u32;
    for handler in &graph.handlers {
        if !bound_handler_ids.contains(&handler.id) {
            continue;
        }
        let origin = origin_layout.handler(definition, bound_ordinal)?;
        bound_ordinal = bound_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "bound legacy handler origin allocation overflowed u32",
            )
        })?;
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.origins.insert(origin, OriginKind::EventHandler);
        descriptor.effects.insert(effect, EffectKind::EventHandler);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
        push_legacy_external_dependencies(
            definition,
            descriptor,
            &handler.dependencies,
            value_layout,
            &callback_values,
            &helper_ids,
            DependencyEndpoint::Effect(effect),
            DependencyEdgeKind::Capture,
            origin,
        )?;
        populate_query_operations(
            definition,
            descriptor,
            &handler.client_operations,
            DependencyEndpoint::Effect(effect),
            &query_origins,
            &queries_by_id,
            value_layout,
        )?;
        populate_mutation_operations(
            definition,
            descriptor,
            &handler.client_operations,
            DependencyEndpoint::Effect(effect),
            &mutation_origins,
            &mutations_by_id,
            value_layout,
        )?;
        populate_shared_state_operations(
            definition,
            descriptor,
            &handler.client_operations,
            DependencyEndpoint::Effect(effect),
            value_layout,
            &callback_values,
            &helper_ids,
            shared_state_origins,
        )?;
        populate_node_ref_operations(
            definition,
            descriptor,
            &handler.client_operations,
            DependencyEndpoint::Effect(effect),
            &node_ref_effects,
            origin,
        )?;
        populate_browser_operations(
            definition,
            descriptor,
            &handler.browser_operations,
            DependencyEndpoint::Effect(effect),
            value_layout,
            &callback_values,
            &helper_ids,
            origin_layout,
            browser_operation_ordinal,
        )?;
    }

    let mut binding_ordinal = 0_u32;
    for binding in &graph.bindings.text {
        populate_text_binding(
            definition,
            descriptor,
            binding,
            value_layout,
            origin_layout.binding(definition, binding_ordinal)?,
        )?;
        binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding origin allocation overflowed u32",
            )
        })?;
    }
    for binding in &graph.bindings.attributes {
        populate_attribute_binding(
            definition,
            descriptor,
            binding,
            value_layout,
            origin_layout.binding(definition, binding_ordinal)?,
        )?;
        binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding origin allocation overflowed u32",
            )
        })?;
    }
    for binding in &graph.bindings.inputs {
        populate_input_binding(
            definition,
            descriptor,
            binding,
            value_layout,
            origin_layout.binding(definition, binding_ordinal)?,
        )?;
        binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding origin allocation overflowed u32",
            )
        })?;
    }
    for binding in &graph.bindings.conditionals {
        populate_conditional_binding(
            definition,
            descriptor,
            binding,
            value_layout,
            origin_layout.binding(definition, binding_ordinal)?,
        )?;
        binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "legacy DOM-binding origin allocation overflowed u32",
            )
        })?;
    }
    for collection in &graph.keyed_collections {
        populate_keyed_collection(
            definition,
            descriptor,
            collection,
            value_layout,
            origin_layout.binding(definition, binding_ordinal)?,
        )?;
        binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "keyed-collection binding origin allocation overflowed u32",
            )
        })?;
    }
    debug_assert_eq!(binding_ordinal, origin_layout.binding_count);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn populate_shared_state_operations(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    operations: &[LegacyClientOperation],
    enclosing: DependencyEndpoint,
    value_layout: &ComponentValueLayout,
    callback_values: &BTreeMap<u32, LocalValueId>,
    helper_ids: &BTreeMap<u32, HelperCallEdgeId>,
    shared_state_origins: &BTreeMap<u32, OriginId>,
) -> Result<(), AbiAdapterError> {
    let mut operation_effects = BTreeMap::new();
    for (index, operation) in operations.iter().enumerate() {
        let (signal, effect_kind, read) = match &operation.kind {
            LegacyClientOperationKind::SharedRead { signal, .. } => {
                (*signal, EffectKind::SharedStateRead, true)
            }
            LegacyClientOperationKind::SharedSet { signal, .. }
            | LegacyClientOperationKind::SharedUpdate { signal, .. } => {
                (*signal, EffectKind::SharedStateWrite, false)
            }
            _ => continue,
        };
        let Some(&origin) = shared_state_origins.get(&signal) else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "SharedState operation for signal {signal} has no active relocation-resolved acquisition"
                ),
            ));
        };
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.effects.insert(effect, effect_kind);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;

        for dependency in &operation.dependencies.signal_reads {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Capture,
                DependencyEndpoint::Value(value_layout.signal(definition, dependency.0)?),
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
        }
        for dependency in &operation.dependencies.signal_writes {
            if dependency.0 != signal {
                push_dependency(
                    definition,
                    descriptor,
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Effect(effect),
                    DependencyEndpoint::Value(value_layout.signal(definition, dependency.0)?),
                    origin,
                )?;
            }
        }
        for callback in &operation.dependencies.callback_slots {
            let Some(&value) = callback_values.get(&callback.0) else {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "SharedState operation lost inbound callback slot {} during canonical allocation",
                        callback.0,
                    ),
                ));
            };
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Capture,
                DependencyEndpoint::Value(value),
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
        }
        for helper in &operation.dependencies.helper_edges {
            let Some(&helper_id) = helper_ids.get(&helper.index()) else {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "SharedState operation lost relocation-resolved helper edge {} during canonical allocation",
                        helper.index(),
                    ),
                ));
            };
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::HelperCall,
                DependencyEndpoint::Helper(helper_id),
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
        }

        let consumer = match operation.consumer {
            LegacyClientOperationConsumer::Enclosing => enclosing,
            LegacyClientOperationConsumer::Operation(parent) => {
                let parent = usize::try_from(parent).map_err(|_| {
                    invalid(
                        "component",
                        definition,
                        "SharedState operation parent exceeds the host index space",
                    )
                })?;
                operation_effects.get(&parent).copied().map_or_else(
                    || {
                        Err(invalid(
                            "component",
                            definition,
                            format!(
                                "SharedState operation {index} depends on operation {parent}, which has no canonical SharedState effect"
                            ),
                        ))
                    },
                    |effect| Ok(DependencyEndpoint::Effect(effect)),
                )?
            }
        };
        if read {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                consumer,
                origin,
            )?;
        } else {
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Effect,
                consumer,
                DependencyEndpoint::Effect(effect),
                origin,
            )?;
            push_dependency(
                definition,
                descriptor,
                DependencyEdgeKind::Effect,
                DependencyEndpoint::Effect(effect),
                DependencyEndpoint::Value(value_layout.signal(definition, signal)?),
                origin,
            )?;
        }
        operation_effects.insert(index, effect);
    }
    Ok(())
}

fn populate_query_operations(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    operations: &[crate::rsjs::legacy_graph::LegacyClientOperation],
    enclosing: DependencyEndpoint,
    query_origins: &BTreeMap<u32, OriginId>,
    queries: &BTreeMap<u32, &LegacyQuery>,
    value_layout: &ComponentValueLayout,
) -> Result<(), AbiAdapterError> {
    for operation in operations {
        let (query, effect_kind, read) = match operation.kind {
            LegacyClientOperationKind::QueryRead { query, .. } => {
                (query, EffectKind::QueryObservation, true)
            }
            LegacyClientOperationKind::QueryRefetch { query } => {
                (query, EffectKind::QueryRefetch, false)
            }
            LegacyClientOperationKind::QueryInvalidate { query } => {
                (query, EffectKind::QueryInvalidation, false)
            }
            _ => continue,
        };
        if !matches!(operation.consumer, LegacyClientOperationConsumer::Enclosing) {
            return Err(invalid(
                "component",
                definition,
                "nested query operation escaped the fail-closed legacy graph gate",
            ));
        }
        let Some(&origin) = query_origins.get(&query) else {
            // The enclosing expression or handler is not rooted in rendered
            // client work. Its declaration remains hashed producer material,
            // but it must not acquire a query origin or activate an owner.
            continue;
        };
        let Some(query_declaration) = queries.get(&query).copied() else {
            return Err(invalid(
                "component",
                definition,
                format!("reachable query operation refers to missing query {query}"),
            ));
        };
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.effects.insert(effect, effect_kind);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;

        let input_expressions: &[u32] = match effect_kind {
            EffectKind::QueryObservation => &[
                query_declaration.key_expression,
                query_declaration.server_initial_expression,
                query_declaration.client_fetcher_expression,
            ],
            EffectKind::QueryRefetch => &[
                query_declaration.key_expression,
                query_declaration.client_fetcher_expression,
            ],
            EffectKind::QueryInvalidation => &[query_declaration.key_expression],
            _ => unreachable!("query operation selected a non-query effect kind"),
        };
        let mut captured = BTreeSet::new();
        for expression in input_expressions {
            if captured.insert(*expression) {
                push_dependency(
                    definition,
                    descriptor,
                    DependencyEdgeKind::Capture,
                    DependencyEndpoint::Value(value_layout.expr(definition, *expression)?),
                    DependencyEndpoint::Effect(effect),
                    origin,
                )?;
            }
        }

        let (kind, from, to) = if read {
            (
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                enclosing,
            )
        } else {
            (
                DependencyEdgeKind::Effect,
                enclosing,
                DependencyEndpoint::Effect(effect),
            )
        };
        push_dependency(definition, descriptor, kind, from, to, origin)?;
    }
    Ok(())
}

fn populate_mutation_operations(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    operations: &[LegacyClientOperation],
    enclosing: DependencyEndpoint,
    mutation_origins: &BTreeMap<u32, OriginId>,
    mutations: &BTreeMap<u32, &LegacyMutation>,
    value_layout: &ComponentValueLayout,
) -> Result<(), AbiAdapterError> {
    for operation in operations {
        let (mutation, read) = match operation.kind {
            LegacyClientOperationKind::MutationRead { mutation, .. } => (mutation, true),
            LegacyClientOperationKind::MutationRun { mutation } => (mutation, false),
            _ => continue,
        };
        if !matches!(operation.consumer, LegacyClientOperationConsumer::Enclosing) {
            return Err(invalid(
                "component",
                definition,
                "nested mutation operation escaped the fail-closed legacy graph gate",
            ));
        }
        let Some(&origin) = mutation_origins.get(&mutation) else {
            // The enclosing expression or handler is not rooted in rendered
            // client work. Its declaration remains hashed producer material,
            // but it must not acquire a mutation origin or activate an owner.
            continue;
        };
        let Some(mutation_declaration) = mutations.get(&mutation).copied() else {
            return Err(invalid(
                "component",
                definition,
                format!("reachable mutation operation refers to missing mutation {mutation}"),
            ));
        };
        let effect = next_effect_id(definition, descriptor)?;
        descriptor.effects.insert(effect, EffectKind::Mutation);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;

        // These are precisely the browser registration inputs. The server
        // runner never crosses into the fragment; server-evaluated metadata
        // remains in the producer's adjacent SSR blob.
        let mut captured = BTreeSet::new();
        for expression in [
            mutation_declaration.key_expression,
            mutation_declaration.client_runner_expression,
        ]
        .into_iter()
        .chain(mutation_declaration.invalidates.iter().copied())
        {
            if captured.insert(expression) {
                push_dependency(
                    definition,
                    descriptor,
                    DependencyEdgeKind::Capture,
                    DependencyEndpoint::Value(value_layout.expr(definition, expression)?),
                    DependencyEndpoint::Effect(effect),
                    origin,
                )?;
            }
        }

        let (kind, from, to) = if read {
            (
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                enclosing,
            )
        } else {
            (
                DependencyEdgeKind::Effect,
                enclosing,
                DependencyEndpoint::Effect(effect),
            )
        };
        push_dependency(definition, descriptor, kind, from, to, origin)?;
    }
    Ok(())
}

fn populate_node_ref_operations(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    operations: &[crate::rsjs::legacy_graph::LegacyClientOperation],
    enclosing: DependencyEndpoint,
    node_ref_effects: &BTreeMap<u32, LocalEffectId>,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    for operation in operations {
        let (node_ref, read) = match &operation.kind {
            LegacyClientOperationKind::NodeRefRead { node_ref } => (*node_ref, true),
            LegacyClientOperationKind::NodeRefWrite { node_ref } => (*node_ref, false),
            _ => continue,
        };
        if !matches!(operation.consumer, LegacyClientOperationConsumer::Enclosing) {
            return Err(invalid(
                "component",
                definition,
                "nested node-reference operation escaped the fail-closed legacy graph gate",
            ));
        }
        let Some(&effect) = node_ref_effects.get(&node_ref) else {
            // The enclosing expression/handler is not rooted in rendered
            // client work. Its declaration remains in the hashed codegen
            // material, but it must not acquire a canonical browser origin.
            continue;
        };
        let (kind, from, to) = if read {
            (
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                enclosing,
            )
        } else {
            (
                DependencyEdgeKind::Effect,
                enclosing,
                DependencyEndpoint::Effect(effect),
            )
        };
        push_dependency(definition, descriptor, kind, from, to, origin)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn populate_browser_operations(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    operations: &[LegacyBrowserOperation],
    enclosing: DependencyEndpoint,
    value_layout: &ComponentValueLayout,
    callback_values: &BTreeMap<u32, LocalValueId>,
    helper_ids: &BTreeMap<u32, HelperCallEdgeId>,
    origin_layout: ComponentOriginLayout,
    global_ordinal: &mut u32,
) -> Result<(), AbiAdapterError> {
    let mut operation_effects = Vec::with_capacity(operations.len());
    for (local_ordinal, operation) in operations.iter().enumerate() {
        let local_ordinal = checked_ordinal(definition, local_ordinal, "browser operation")?;
        let origin = origin_layout.browser_operation(definition, *global_ordinal)?;
        *global_ordinal = global_ordinal.checked_add(1).ok_or_else(|| {
            invalid(
                "component",
                definition,
                "browser-operation origin allocation overflowed u32",
            )
        })?;
        let effect = next_effect_id(definition, descriptor)?;
        let (origin_kind, effect_kind) = match operation.kind {
            LegacyBrowserOperationKind::Read => {
                (OriginKind::BrowserCapabilityRead, EffectKind::BrowserRead)
            }
            LegacyBrowserOperationKind::Write => {
                (OriginKind::BrowserCapabilityWrite, EffectKind::BrowserWrite)
            }
        };
        descriptor.origins.insert(origin, origin_kind);
        descriptor.effects.insert(effect, effect_kind);
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Origin(origin),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
        push_browser_operation_dependencies(
            definition,
            descriptor,
            &operation.dependencies,
            effect,
            value_layout,
            callback_values,
            helper_ids,
            origin,
        )?;

        let consumer = match operation.consumer {
            LegacyBrowserConsumer::Enclosing => enclosing,
            LegacyBrowserConsumer::Operation(parent) => {
                if parent >= local_ordinal {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "browser operation {local_ordinal} references non-enclosing operation {parent}"
                        ),
                    ));
                }
                let parent = usize::try_from(parent).map_err(|_| {
                    invalid(
                        "component",
                        definition,
                        format!("browser operation parent {parent} exceeds usize"),
                    )
                })?;
                DependencyEndpoint::Effect(operation_effects[parent])
            }
        };
        let (kind, from, to) = match operation.kind {
            LegacyBrowserOperationKind::Read => (
                DependencyEdgeKind::Data,
                DependencyEndpoint::Effect(effect),
                consumer,
            ),
            LegacyBrowserOperationKind::Write => (
                DependencyEdgeKind::Effect,
                consumer,
                DependencyEndpoint::Effect(effect),
            ),
        };
        push_dependency(definition, descriptor, kind, from, to, origin)?;
        operation_effects.push(effect);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_browser_operation_dependencies(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    dependencies: &ClientBlockExternalDependencies,
    effect: LocalEffectId,
    value_layout: &ComponentValueLayout,
    callback_values: &BTreeMap<u32, LocalValueId>,
    helper_ids: &BTreeMap<u32, HelperCallEdgeId>,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    for signal in &dependencies.signal_reads {
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Capture,
            DependencyEndpoint::Value(value_layout.signal(definition, signal.0)?),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
    }
    for signal in &dependencies.signal_writes {
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Effect,
            DependencyEndpoint::Effect(effect),
            DependencyEndpoint::Value(value_layout.signal(definition, signal.0)?),
            origin,
        )?;
    }
    for callback in &dependencies.callback_slots {
        let Some(&value) = callback_values.get(&callback.0) else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "browser operation references callback slot {}, but no inbound callback value was allocated",
                    callback.0
                ),
            ));
        };
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::Capture,
            DependencyEndpoint::Value(value),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
    }
    for helper in &dependencies.helper_edges {
        let Some(&helper_id) = helper_ids.get(&helper.index()) else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "browser operation lost relocation-resolved helper edge {} during canonical allocation",
                    helper.index()
                ),
            ));
        };
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::HelperCall,
            DependencyEndpoint::Helper(helper_id),
            DependencyEndpoint::Effect(effect),
            origin,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_legacy_external_dependencies(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    dependencies: &ClientBlockExternalDependencies,
    value_layout: &ComponentValueLayout,
    callback_values: &BTreeMap<u32, LocalValueId>,
    helper_ids: &BTreeMap<u32, HelperCallEdgeId>,
    target: DependencyEndpoint,
    value_edge_kind: DependencyEdgeKind,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    let mut signals = BTreeSet::new();
    for signal in dependencies
        .signal_reads
        .iter()
        .chain(&dependencies.signal_writes)
    {
        if signals.insert(signal.0) {
            push_dependency(
                definition,
                descriptor,
                value_edge_kind,
                DependencyEndpoint::Value(value_layout.signal(definition, signal.0)?),
                target,
                origin,
            )?;
        }
    }
    for callback in &dependencies.callback_slots {
        let Some(&value) = callback_values.get(&callback.0) else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "legacy Client IR references callback slot {}, but no inbound callback value was allocated",
                    callback.0
                ),
            ));
        };
        push_dependency(
            definition,
            descriptor,
            value_edge_kind,
            DependencyEndpoint::Value(value),
            target,
            origin,
        )?;
    }
    for helper in &dependencies.helper_edges {
        let Some(&helper_id) = helper_ids.get(&helper.index()) else {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "legacy Client IR lost relocation-resolved helper edge {} during canonical allocation",
                    helper.index()
                ),
            ));
        };
        push_dependency(
            definition,
            descriptor,
            DependencyEdgeKind::HelperCall,
            DependencyEndpoint::Helper(helper_id),
            target,
            origin,
        )?;
    }
    Ok(())
}

fn populate_text_binding(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    binding: &LegacyTextBinding,
    value_layout: &ComponentValueLayout,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    populate_dom_binding_effect(
        definition,
        descriptor,
        EffectKind::DomTextUpdate,
        value_layout.expr(definition, binding.expression)?,
        origin,
    )
}

fn populate_attribute_binding(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    binding: &LegacyAttributeBinding,
    value_layout: &ComponentValueLayout,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    populate_dom_binding_effect(
        definition,
        descriptor,
        EffectKind::DomAttributeUpdate,
        value_layout.expr(definition, binding.expression)?,
        origin,
    )
}

fn populate_input_binding(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    binding: &LegacyInputBinding,
    value_layout: &ComponentValueLayout,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    populate_dom_binding_effect(
        definition,
        descriptor,
        EffectKind::DomPropertyUpdate,
        value_layout.signal(definition, binding.signal)?,
        origin,
    )
}

fn populate_keyed_collection(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    collection: &LegacyKeyedCollection,
    value_layout: &ComponentValueLayout,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    let site = KeyedCollectionSiteId::new(collection.id);
    if !descriptor.keyed_collection_sites.insert(site) {
        return Err(invalid(
            "component",
            definition,
            format!(
                "keyed collection site {} is declared more than once",
                collection.id
            ),
        ));
    }
    let source = match collection.source {
        LegacyKeyedCollectionSource::Signal(signal) => value_layout.signal(definition, signal)?,
        LegacyKeyedCollectionSource::Expression(expression) => {
            value_layout.expr(definition, expression)?
        }
    };
    populate_dom_binding_effect(
        definition,
        descriptor,
        EffectKind::DomStructuralUpdate,
        source,
        origin,
    )
}

fn populate_conditional_binding(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    binding: &LegacyConditionalBinding,
    value_layout: &ComponentValueLayout,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    let control = LocalControlRegionId::new(binding.control);
    if !descriptor.control_regions.insert(control) {
        return Err(invalid(
            "component",
            definition,
            format!(
                "conditional control {} is declared more than once",
                binding.control
            ),
        ));
    }
    let effect = next_effect_id(definition, descriptor)?;
    descriptor.origins.insert(origin, OriginKind::SourceOnly);
    descriptor
        .effects
        .insert(effect, EffectKind::DomStructuralUpdate);
    push_dependency(
        definition,
        descriptor,
        DependencyEdgeKind::Effect,
        DependencyEndpoint::Origin(origin),
        DependencyEndpoint::Effect(effect),
        origin,
    )?;
    push_dependency(
        definition,
        descriptor,
        DependencyEdgeKind::Control,
        DependencyEndpoint::Value(value_layout.expr(definition, binding.condition)?),
        DependencyEndpoint::Control(control),
        origin,
    )?;
    push_dependency(
        definition,
        descriptor,
        DependencyEdgeKind::Control,
        DependencyEndpoint::Control(control),
        DependencyEndpoint::Effect(effect),
        origin,
    )
}

fn populate_dom_binding_effect(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    kind: EffectKind,
    value: LocalValueId,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    let effect = next_effect_id(definition, descriptor)?;
    descriptor.origins.insert(origin, OriginKind::SourceOnly);
    descriptor.effects.insert(effect, kind);
    push_dependency(
        definition,
        descriptor,
        DependencyEdgeKind::Effect,
        DependencyEndpoint::Origin(origin),
        DependencyEndpoint::Effect(effect),
        origin,
    )?;
    push_dependency(
        definition,
        descriptor,
        DependencyEdgeKind::Data,
        DependencyEndpoint::Value(value),
        DependencyEndpoint::Effect(effect),
        origin,
    )
}

fn checked_ordinal(
    definition: TransportDefinitionId,
    ordinal: usize,
    label: &'static str,
) -> Result<u32, AbiAdapterError> {
    u32::try_from(ordinal).map_err(|_| {
        invalid(
            "component",
            definition,
            format!("{label} count exceeds the canonical u32 ID space"),
        )
    })
}

fn next_effect_id(
    definition: TransportDefinitionId,
    descriptor: &ComponentDescriptor,
) -> Result<LocalEffectId, AbiAdapterError> {
    u32::try_from(descriptor.effects.len())
        .map(LocalEffectId::new)
        .map_err(|_| {
            invalid(
                "component",
                definition,
                "canonical effect count exceeds the u32 ID space",
            )
        })
}

fn push_dependency(
    definition: TransportDefinitionId,
    descriptor: &mut ComponentDescriptor,
    kind: DependencyEdgeKind,
    from: DependencyEndpoint,
    to: DependencyEndpoint,
    origin: OriginId,
) -> Result<(), AbiAdapterError> {
    let id = u32::try_from(descriptor.dependencies.len()).map_err(|_| {
        invalid(
            "component",
            definition,
            "canonical dependency count exceeds the u32 ID space",
        )
    })?;
    descriptor.dependencies.insert(
        DependencyEdgeId::new(id),
        DependencyEdge::new(kind, from, to, origin),
    );
    Ok(())
}

fn canonical_component_lifecycle_control(
    definition: TransportDefinitionId,
    descriptor: &ComponentDescriptor,
    control: u32,
    expected_predicate: LocalValueId,
    branch: ComponentLifecycleBranch,
) -> Result<ComponentLifecycleControl, AbiAdapterError> {
    // Source-edge v3 authenticates the control/branch/site, producer predicate
    // expression, and the complete non-cyclic descriptor graph under the
    // parent component input fingerprint. Derive the canonical value and its
    // provenance from that already-validated graph, then require the value to
    // match the relocation-authenticated producer expression mapping.
    let control = LocalControlRegionId::new(control);
    let predicate_dependencies = descriptor
        .dependencies
        .values()
        .filter(|dependency| dependency.to == DependencyEndpoint::Control(control))
        .collect::<Vec<_>>();
    let [dependency] = predicate_dependencies.as_slice() else {
        return Err(invalid(
            "component",
            definition,
            format!(
                "conditional component lifecycle control {} must have exactly one canonical \
                 predicate dependency, found {}",
                control.index(),
                predicate_dependencies.len(),
            ),
        ));
    };
    let DependencyEdge {
        kind: DependencyEdgeKind::Control,
        from: DependencyEndpoint::Value(predicate),
        origin: predicate_origin,
        ..
    } = **dependency
    else {
        return Err(invalid(
            "component",
            definition,
            format!(
                "conditional component lifecycle control {} is not driven by a canonical \
                 Value -> Control dependency",
                control.index(),
            ),
        ));
    };
    if predicate != expected_predicate {
        return Err(invalid(
            "component",
            definition,
            format!(
                "conditional component lifecycle control {} resolves predicate value {}, but \
                 its relocation-authenticated expression maps to value {}",
                control.index(),
                predicate.index(),
                expected_predicate.index(),
            ),
        ));
    }
    Ok(ComponentLifecycleControl {
        control,
        predicate,
        predicate_origin,
        branch,
    })
}

fn adapt_direct_component_bindings(
    definition: TransportDefinitionId,
    fields: &[ValidatedDirectComponentField],
    value_layout: &ComponentValueLayout,
    callback_values: &BTreeMap<u32, LocalValueId>,
    origin: OriginId,
) -> Result<BTreeMap<String, ComponentFieldBinding>, AbiAdapterError> {
    let mut bindings = BTreeMap::new();
    for field in fields {
        let source = match field.binding {
            ValidatedDirectComponentBinding::ServerOnly => ComponentBindingSource::ServerOnly,
            ValidatedDirectComponentBinding::StaticSlotExactOne => {
                ComponentBindingSource::StaticSlot {
                    span: SpanCardinality::Exact(1),
                }
            }
            ValidatedDirectComponentBinding::StaticCallback { shape, .. } => {
                ComponentBindingSource::StaticCallback { shape }
            }
            ValidatedDirectComponentBinding::LiveValue { signal, .. } => {
                ComponentBindingSource::LiveValue {
                    value: value_layout.signal(definition, signal)?,
                }
            }
            ValidatedDirectComponentBinding::LiveCallback {
                handler_prelude_local,
                shape,
                ..
            } => {
                let Some(&value) = callback_values.get(&handler_prelude_local) else {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "live callback field `{}` references missing payload-handler prelude binding {handler_prelude_local}",
                            field.name
                        ),
                    ));
                };
                ComponentBindingSource::LiveCallback { value, shape }
            }
            ValidatedDirectComponentBinding::ForwardedCallback {
                parent_field,
                shape,
            } => ComponentBindingSource::LiveCallback {
                value: value_layout.inbound(definition, u64::from(parent_field))?,
                shape,
            },
            ValidatedDirectComponentBinding::NodeRefHandle { node_ref } => {
                ComponentBindingSource::ClientHandle {
                    value: value_layout.node_ref(definition, node_ref)?,
                }
            }
            ValidatedDirectComponentBinding::Unsupported => {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "unproven direct component field `{}` reached canonical adaptation",
                        field.name
                    ),
                ));
            }
        };
        if bindings
            .insert(field.name.clone(), ComponentFieldBinding { source, origin })
            .is_some()
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "direct component bindings contain duplicate field `{}`",
                    field.name
                ),
            ));
        }
    }
    Ok(bindings)
}

/// Collision-free canonical witness ranges for one component descriptor.
///
/// Witness zero is always the defining item. The remaining categories are
/// contiguous and ordered independently of object extraction order:
/// component edges, client-visible fields, then reachable helper calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ComponentWitnessLayout {
    component_start: u64,
    component_count: u32,
    field_start: u64,
    field_count: u32,
    helper_start: u64,
    helper_count: u32,
}

impl ComponentWitnessLayout {
    fn new(
        definition: TransportDefinitionId,
        component_count: usize,
        field_count: usize,
        helper_count: usize,
    ) -> Result<Self, AbiAdapterError> {
        let component_count = u64::try_from(component_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component edge count exceeds the canonical u64 count space",
            )
        })?;
        let field_count = u64::try_from(field_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component field count exceeds the canonical u64 count space",
            )
        })?;
        let helper_count = u64::try_from(helper_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable helper edge count exceeds the canonical u64 count space",
            )
        })?;
        Self::from_counts(definition, component_count, field_count, helper_count)
    }

    fn from_counts(
        definition: TransportDefinitionId,
        component_count: u64,
        field_count: u64,
        helper_count: u64,
    ) -> Result<Self, AbiAdapterError> {
        let component_count = u32::try_from(component_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component edge count exceeds the canonical u32 witness space",
            )
        })?;
        let field_count = u32::try_from(field_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "component field count exceeds the canonical u32 witness space",
            )
        })?;
        let helper_count = u32::try_from(helper_count).map_err(|_| {
            invalid(
                "component",
                definition,
                "reachable helper edge count exceeds the canonical u32 witness space",
            )
        })?;

        let component_start = 1_u64;
        let field_start = component_start
            .checked_add(u64::from(component_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "component edge witnesses exhaust the canonical u64 allocation space",
                )
            })?;
        let helper_start = field_start
            .checked_add(u64::from(field_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "component field witnesses exhaust the canonical u64 allocation space",
                )
            })?;
        let allocation_end = helper_start
            .checked_add(u64::from(helper_count))
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    "reachable helper witnesses exhaust the canonical u64 allocation space",
                )
            })?;
        if allocation_end > u64::from(u32::MAX) + 1 {
            return Err(invalid(
                "component",
                definition,
                "component witnesses exhaust the canonical u32 witness space",
            ));
        }

        Ok(Self {
            component_start,
            component_count,
            field_start,
            field_count,
            helper_start,
            helper_count,
        })
    }

    fn component(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<WitnessRecordId, AbiAdapterError> {
        self.witness(
            definition,
            "component edge",
            self.component_start,
            self.component_count,
            ordinal,
        )
    }

    fn field(
        self,
        definition: TransportDefinitionId,
        field: u32,
    ) -> Result<WitnessRecordId, AbiAdapterError> {
        self.witness(
            definition,
            "component field",
            self.field_start,
            self.field_count,
            field,
        )
    }

    fn helper(
        self,
        definition: TransportDefinitionId,
        ordinal: u32,
    ) -> Result<WitnessRecordId, AbiAdapterError> {
        self.witness(
            definition,
            "reachable helper edge",
            self.helper_start,
            self.helper_count,
            ordinal,
        )
    }

    fn witness(
        self,
        definition: TransportDefinitionId,
        category: &'static str,
        start: u64,
        count: u32,
        ordinal: u32,
    ) -> Result<WitnessRecordId, AbiAdapterError> {
        if ordinal >= count {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "{category} ordinal {ordinal} is outside its allocated witness range of {count}"
                ),
            ));
        }
        let witness = start
            .checked_add(u64::from(ordinal))
            .and_then(|witness| u32::try_from(witness).ok())
            .ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("{category} witness ID exceeds the canonical u32 witness space"),
                )
            })?;
        Ok(WitnessRecordId::new(witness))
    }
}

fn build_component_witness_table(
    linked: &LinkedTransportComponent,
    component_edges: &[ValidatedDirectComponentEdge],
    field_witnesses: &BTreeMap<u32, ValidatedComponentFieldWitness>,
    helper_edges: &[ValidatedComponentHelperEdge],
    layout: ComponentWitnessLayout,
) -> Result<BTreeMap<WitnessRecordId, ResolvedTransportWitness>, AbiAdapterError> {
    let mut witnesses = BTreeMap::new();
    insert_component_witness(
        linked.definition,
        &mut witnesses,
        WitnessRecordId::new(0),
        ResolvedTransportWitness::Definition(linked.definition),
    )?;

    for edge in component_edges {
        let linked_edge = linked
            .component_edges
            .iter()
            .find(|linked_edge| linked_edge.source_edge == edge.source_edge)
            .ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "direct component local ordinal {} has no relocation-resolved source edge",
                        edge.local_ordinal
                    ),
                )
            })?;
        insert_component_witness(
            linked.definition,
            &mut witnesses,
            layout.component(linked.definition, edge.local_ordinal)?,
            ResolvedTransportWitness::ComponentEdge {
                source_edge: edge.source_edge,
                target: linked_edge.target,
            },
        )?;
    }

    for (&field, witness) in field_witnesses {
        let expected = layout.field(linked.definition, field)?;
        if witness.witness != expected {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "component field {field} carries witness {:?}, expected {:?}",
                    witness.witness, expected
                ),
            ));
        }
        insert_component_witness(
            linked.definition,
            &mut witnesses,
            expected,
            ResolvedTransportWitness::ComponentField {
                field_record: witness.field_record,
                target: linked.definition,
            },
        )?;
    }

    if helper_edges
        .windows(2)
        .any(|pair| pair[0].local_ordinal >= pair[1].local_ordinal)
    {
        return Err(invalid(
            "component",
            linked.definition,
            "reachable helper witnesses must be strictly ordered by producer-local ordinal",
        ));
    }
    for (index, edge) in helper_edges.iter().enumerate() {
        let call = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                "reachable helper edge count exceeds the canonical u32 ID space",
            )
        })?;
        insert_component_witness(
            linked.definition,
            &mut witnesses,
            layout.helper(linked.definition, call)?,
            ResolvedTransportWitness::HelperEdge {
                source_edge: edge.source_edge,
                target: edge.target,
            },
        )?;
    }

    Ok(witnesses)
}

fn insert_component_witness(
    definition: TransportDefinitionId,
    witnesses: &mut BTreeMap<WitnessRecordId, ResolvedTransportWitness>,
    witness: WitnessRecordId,
    resolved: ResolvedTransportWitness,
) -> Result<(), AbiAdapterError> {
    if witnesses.insert(witness, resolved).is_some() {
        return Err(invalid(
            "component",
            definition,
            format!("canonical witness ID {witness:?} was allocated more than once"),
        ));
    }
    Ok(())
}

/// Validate schema-v9 latent input metadata without pretending it contains
/// rustc's missing field type/signature authority.
///
/// A structurally valid descriptor proceeds to compatibility-gap reporting;
/// malformed IDs, temporal modes, targets, or callback dispatch fail before
/// semantic linking can observe them.
fn validate_inbound_inputs_with_projection_roles(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    projection_roles: &ResolvedProjectionRoles,
) -> Result<ValidatedInboundInputs, AbiAdapterError> {
    let fields_json = required_array(descriptor, "fields", "component", definition)?;
    let mut fields = Vec::<ValidatedInboundField>::new();
    for (index, value) in fields_json.iter().enumerate() {
        let field = value.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("fields[{index}] must be an object"),
            )
        })?;
        require_exact_u64(field, "id", index as u64, "component", definition)?;
        let raw_kind = required_str(field, "kind", "component", definition)?;
        let (kind, record_role) = match raw_kind {
            "prop" => (InboundFieldKind::Value, TransportComponentFieldRole::Value),
            "handler" => (
                InboundFieldKind::Callback,
                TransportComponentFieldRole::Callback,
            ),
            "node_ref" => (
                InboundFieldKind::NodeRef,
                TransportComponentFieldRole::NodeRef,
            ),
            "deferred_projection" => {
                let field_id = checked_u32(
                    required_u64(field, "id", "component", definition)?,
                    "deferred projection field ID",
                    definition,
                )?;
                let resolved = projection_roles.field(definition, field_id).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!(
                            "deferred projection field {field_id} requires complete transport-plan adaptation"
                        ),
                    )
                })?;
                (
                    match resolved {
                        ResolvedProjectionRole::ServerOnly => InboundFieldKind::ServerOnly,
                        ResolvedProjectionRole::Value => InboundFieldKind::Value,
                        ResolvedProjectionRole::Callback(_) => InboundFieldKind::Callback,
                        ResolvedProjectionRole::NodeRef => InboundFieldKind::NodeRef,
                    },
                    TransportComponentFieldRole::DeferredProjection,
                )
            }
            "server_prop" | "static_slot" => continue,
            other => {
                return Err(invalid(
                    "component",
                    definition,
                    format!("unsupported schema-v9 component field kind `{other}`"),
                ));
            }
        };
        if index != fields.len() {
            return Err(invalid(
                "component",
                definition,
                "inbound fields must be a dense prefix of the component field ID space",
            ));
        }
        let name = required_str(field, "name", "component", definition)?.to_owned();
        let crossing = required_u64(field, "crossing_id", "component", definition)?;
        let (value, callback_slot, callback_shape) = match kind {
            InboundFieldKind::Value => (
                required_u64(field, "value_id", "component", definition)?,
                None,
                None,
            ),
            InboundFieldKind::Callback => {
                let (slot, callback_shape) = if record_role
                    == TransportComponentFieldRole::DeferredProjection
                {
                    let shape = match projection_roles
                        .field(definition, index as u32)
                        .expect("deferred field resolution was checked above")
                    {
                        ResolvedProjectionRole::Callback(shape) => shape,
                        _ => unreachable!("resolved callback kind must carry a callback shape"),
                    };
                    (None, shape)
                } else {
                    require_exact_str(
                        field,
                        "dispatch",
                        "resolve_current",
                        "component",
                        definition,
                    )?;
                    let slot = required_u64(field, "callback_slot", "component", definition)?;
                    let callback_slots =
                        required_array(descriptor, "callback_slots", "component", definition)?;
                    let valid_slot = callback_slots.iter().any(|candidate| {
                        candidate.as_object().is_some_and(|candidate| {
                            required_u64(candidate, "id", "component", definition).ok()
                                == Some(slot)
                                && required_str(candidate, "name", "component", definition).ok()
                                    == Some(name.as_str())
                        })
                    });
                    if !valid_slot {
                        return Err(invalid(
                            "component",
                            definition,
                            format!(
                                "callback field `{name}` refers to unknown or differently named callback slot {slot}"
                            ),
                        ));
                    }
                    let callback_shape = match required_str(
                        field,
                        "callback_shape",
                        "component",
                        definition,
                    )? {
                        "required" => InboundCallbackShape::Required,
                        "optional" => InboundCallbackShape::Optional,
                        other => {
                            return Err(invalid(
                                    "component",
                                    definition,
                                    format!(
                                        "callback field `{name}` has unsupported callback_shape `{other}`"
                                    ),
                                ));
                        }
                    };
                    (
                        Some(checked_u32(slot, "callback slot ID", definition)?),
                        callback_shape,
                    )
                };
                (
                    required_u64(field, "value_id", "component", definition)?,
                    slot,
                    Some(callback_shape),
                )
            }
            InboundFieldKind::NodeRef => (
                required_u64(field, "value_id", "component", definition)?,
                None,
                None,
            ),
            InboundFieldKind::ServerOnly => (
                required_u64(field, "value_id", "component", definition)?,
                None,
                None,
            ),
        };
        validate_source_anchor(field, "source_anchor", definition)?;
        fields.push(ValidatedInboundField {
            name,
            kind,
            value,
            crossing,
            callback_slot,
            callback_shape,
            record_role,
        });
    }

    let inbound = required_object(descriptor, "inbound_inputs", "component", definition)?;
    require_known_fields(
        "component inbound input metadata",
        inbound,
        &[
            "crossings",
            "node_ref_fields",
            "projection_fields",
            "prop_sites",
            "values",
        ],
        definition,
    )?;

    let values = required_array(inbound, "values", "component", definition)?;
    if values.len() != fields.len() {
        return Err(invalid(
            "component",
            definition,
            format!(
                "schema-v9 inbound values must be one-to-one with inbound fields; found {} values for {} fields",
                values.len(),
                fields.len()
            ),
        ));
    }
    for (index, value) in values.iter().enumerate() {
        let value = value.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("inbound_inputs.values[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "inbound component value",
            value,
            &["field_id", "id", "source_anchor"],
            definition,
        )?;
        require_exact_u64(value, "id", index as u64, "component", definition)?;
        let field = required_u64(value, "field_id", "component", definition)?;
        let field_index = checked_descriptor_index(field, "inbound field", definition)?;
        let Some(field_schema) = fields.get(field_index) else {
            return Err(invalid(
                "component",
                definition,
                format!("inbound value {index} refers to unknown field {field}"),
            ));
        };
        if field_schema.value != index as u64 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound value {index} conflicts with field `{}` value metadata",
                    field_schema.name
                ),
            ));
        }
        validate_source_anchor(value, "source_anchor", definition)?;
    }

    let crossings = required_array(inbound, "crossings", "component", definition)?;
    if crossings.len() != fields.len() {
        return Err(invalid(
            "component",
            definition,
            format!(
                "inbound crossing hooks must be one-to-one with inbound fields; found {} crossings for {} fields",
                crossings.len(),
                fields.len()
            ),
        ));
    }
    for (index, crossing) in crossings.iter().enumerate() {
        let crossing = crossing.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("inbound_inputs.crossings[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "inbound component crossing",
            crossing,
            &["field_id", "id", "kind", "source_anchor"],
            definition,
        )?;
        require_exact_u64(crossing, "id", index as u64, "component", definition)?;
        let field = required_u64(crossing, "field_id", "component", definition)?;
        let field_index = checked_descriptor_index(field, "inbound field", definition)?;
        let Some(field_schema) = fields.get(field_index) else {
            return Err(invalid(
                "component",
                definition,
                format!("inbound crossing {index} refers to unknown field {field}"),
            ));
        };
        if field_schema.crossing != index as u64 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound crossing {index} conflicts with field `{}` crossing_id {}",
                    field_schema.name, field_schema.crossing
                ),
            ));
        }
        let expected = match field_schema.record_role {
            TransportComponentFieldRole::Value => "value",
            TransportComponentFieldRole::Callback => "callback_resolve_current",
            TransportComponentFieldRole::NodeRef => "node_ref_alias",
            TransportComponentFieldRole::DeferredProjection => "deferred_projection",
        };
        require_exact_str(crossing, "kind", expected, "component", definition)?;
        validate_source_anchor(crossing, "source_anchor", definition)?;
    }

    let prop_sites = required_array(inbound, "prop_sites", "component", definition)?;
    let signals = required_array(descriptor, "signals", "component", definition)?;
    let mut validated_prop_uses = Vec::with_capacity(prop_sites.len());
    for (index, site) in prop_sites.iter().enumerate() {
        let site = site.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("inbound_inputs.prop_sites[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "inbound component prop site",
            site,
            &[
                "crossing_id",
                "field_id",
                "id",
                "source_anchor",
                "target",
                "temporal",
                "value_id",
            ],
            definition,
        )?;
        require_exact_u64(site, "id", index as u64, "component", definition)?;
        let field = required_u64(site, "field_id", "component", definition)?;
        let value = required_u64(site, "value_id", "component", definition)?;
        let crossing = required_u64(site, "crossing_id", "component", definition)?;
        let field_index = checked_descriptor_index(field, "inbound field", definition)?;
        let Some(field_schema) = fields.get(field_index) else {
            return Err(invalid(
                "component",
                definition,
                format!("inbound prop site {index} refers to unknown field {field}"),
            ));
        };
        if field_schema.kind != InboundFieldKind::Value
            || field_schema.value != value
            || field_schema.crossing != crossing
            || values
                .get(checked_descriptor_index(
                    value,
                    "inbound value",
                    definition,
                )?)
                .is_none()
            || crossings
                .get(checked_descriptor_index(
                    crossing,
                    "inbound crossing",
                    definition,
                )?)
                .is_none()
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound prop site {index} conflicts with field `{}` value/crossing metadata",
                    field_schema.name
                ),
            ));
        }

        let temporal = required_object(site, "temporal", "component", definition)?;
        let target = required_object(site, "target", "component", definition)?;
        let (temporal, target_signal) =
            match required_str(temporal, "kind", "component", definition)? {
                "live_read" => {
                    require_known_fields(
                        "LiveRead temporal mode",
                        temporal,
                        &["kind"],
                        definition,
                    )?;
                    require_known_fields(
                        "live-signal prop target",
                        target,
                        &["kind", "signal_id"],
                        definition,
                    )?;
                    require_exact_str(target, "kind", "live_signal", "component", definition)?;
                    let signal = required_u64(target, "signal_id", "component", definition)?;
                    if !signals.iter().any(|candidate| {
                        candidate.as_object().is_some_and(|candidate| {
                            required_u64(candidate, "id", "component", definition).ok()
                                == Some(signal)
                        })
                    }) {
                        return Err(invalid(
                            "component",
                            definition,
                            format!("inbound prop site {index} targets unknown signal {signal}"),
                        ));
                    }
                    (
                        TemporalPropMode::LiveRead,
                        Some(checked_u32(signal, "live-signal target", definition)?),
                    )
                }
                "query_key_dependency" => {
                    require_known_fields(
                        "QueryKeyDependency temporal mode",
                        temporal,
                        &["kind"],
                        definition,
                    )?;
                    require_known_fields(
                        "query-key live-signal prop target",
                        target,
                        &["kind", "signal_id"],
                        definition,
                    )?;
                    require_exact_str(target, "kind", "live_signal", "component", definition)?;
                    let signal = required_u64(target, "signal_id", "component", definition)?;
                    if !signals.iter().any(|candidate| {
                        candidate.as_object().is_some_and(|candidate| {
                            required_u64(candidate, "id", "component", definition).ok()
                                == Some(signal)
                        })
                    }) {
                        return Err(invalid(
                            "component",
                            definition,
                            format!(
                            "inbound query-key prop site {index} targets unknown signal {signal}"
                        ),
                        ));
                    }
                    (
                        TemporalPropMode::QueryKeyDependency,
                        Some(checked_u32(
                            signal,
                            "query-key live-signal target",
                            definition,
                        )?),
                    )
                }
                "seed_once" => {
                    require_known_fields(
                        "SeedOnce temporal mode",
                        temporal,
                        &["kind", "site_id"],
                        definition,
                    )?;
                    require_exact_u64(temporal, "site_id", index as u64, "component", definition)?;
                    require_known_fields(
                        "signal-seed prop target",
                        target,
                        &["kind", "signal_id"],
                        definition,
                    )?;
                    require_exact_str(target, "kind", "signal_seed", "component", definition)?;
                    let signal = required_u64(target, "signal_id", "component", definition)?;
                    if !signals.iter().any(|candidate| {
                        candidate.as_object().is_some_and(|candidate| {
                            required_u64(candidate, "id", "component", definition).ok()
                                == Some(signal)
                        })
                    }) {
                        return Err(invalid(
                            "component",
                            definition,
                            format!("inbound prop site {index} targets unknown signal {signal}"),
                        ));
                    }
                    let site = u32::try_from(index).map_err(|_| {
                        invalid(
                            "component",
                            definition,
                            "inbound prop-use site count exceeds the canonical u32 ID space",
                        )
                    })?;
                    (
                        TemporalPropMode::SeedOnce(PropSiteId::new(site)),
                        Some(checked_u32(signal, "signal-seed target", definition)?),
                    )
                }
                "forbidden_boundary_crossing" => {
                    require_known_fields(
                        "forbidden-boundary temporal mode",
                        temporal,
                        &["kind"],
                        definition,
                    )?;
                    require_known_fields(
                        "explicit-island prop target",
                        target,
                        &["kind"],
                        definition,
                    )?;
                    require_exact_str(
                        target,
                        "kind",
                        "explicit_island_boundary",
                        "component",
                        definition,
                    )?;
                    (TemporalPropMode::ForbiddenBoundaryCrossing, None)
                }
                "forbidden_provider_seed" => {
                    require_known_fields(
                        "forbidden-provider-seed temporal mode",
                        temporal,
                        &["kind"],
                        definition,
                    )?;
                    require_known_fields(
                        "SharedStateProvider seed prop target",
                        target,
                        &["kind"],
                        definition,
                    )?;
                    require_exact_str(
                        target,
                        "kind",
                        "shared_state_provider_seed",
                        "component",
                        definition,
                    )?;
                    (TemporalPropMode::ForbiddenProviderSeed, None)
                }
                other => {
                    return Err(invalid(
                        "component",
                        definition,
                        format!("unsupported inbound temporal mode `{other}`"),
                    ));
                }
            };
        validated_prop_uses.push(ValidatedInboundPropUse {
            site: u32::try_from(index).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "inbound prop-use site count exceeds the canonical u32 ID space",
                )
            })?,
            field: u32::try_from(field).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    format!("inbound prop site {index} field ID exceeds u32"),
                )
            })?,
            value: u32::try_from(value).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    format!("inbound prop site {index} value ID exceeds u32"),
                )
            })?,
            temporal,
            target_signal,
        });
        validate_source_anchor(site, "source_anchor", definition)?;
    }

    let declared_node_refs = required_array(descriptor, "node_refs", "component", definition)?
        .iter()
        .enumerate()
        .map(|(index, node_ref)| {
            let node_ref = node_ref.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("node_refs[{index}] must be an object"),
                )
            })?;
            checked_u32(
                required_u64(node_ref, "id", "component", definition)?,
                "legacy node-ref ID",
                definition,
            )
        })
        .collect::<Result<BTreeSet<_>, AbiAdapterError>>()?;
    let node_ref_fields = required_array(inbound, "node_ref_fields", "component", definition)?;
    let expected_node_ref_fields = fields
        .iter()
        .filter(|field| field.record_role == TransportComponentFieldRole::NodeRef)
        .count();
    if node_ref_fields.len() != expected_node_ref_fields {
        return Err(invalid(
            "component",
            definition,
            format!(
                "inbound node-ref records must be one-to-one with NodeRef fields; found {} records for {expected_node_ref_fields} fields",
                node_ref_fields.len()
            ),
        ));
    }
    let mut validated_node_refs = Vec::with_capacity(node_ref_fields.len());
    let mut claimed_node_ref_fields = BTreeSet::new();
    let mut claimed_node_refs = BTreeSet::new();
    for (index, node_ref) in node_ref_fields.iter().enumerate() {
        let node_ref = node_ref.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("inbound_inputs.node_ref_fields[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "inbound NodeRef field",
            node_ref,
            &[
                "bind_target",
                "crossing_id",
                "field_id",
                "id",
                "node_ref_id",
                "source_anchor",
                "value_id",
            ],
            definition,
        )?;
        require_exact_u64(node_ref, "id", index as u64, "component", definition)?;
        let field = checked_u32(
            required_u64(node_ref, "field_id", "component", definition)?,
            "inbound NodeRef field ID",
            definition,
        )?;
        let value = checked_u32(
            required_u64(node_ref, "value_id", "component", definition)?,
            "inbound NodeRef value ID",
            definition,
        )?;
        let crossing = checked_u32(
            required_u64(node_ref, "crossing_id", "component", definition)?,
            "inbound NodeRef crossing ID",
            definition,
        )?;
        let producer_node_ref = checked_u32(
            required_u64(node_ref, "node_ref_id", "component", definition)?,
            "inbound producer NodeRef ID",
            definition,
        )?;
        let Some(field_schema) = fields.get(field as usize) else {
            return Err(invalid(
                "component",
                definition,
                format!("inbound NodeRef record {index} refers to unknown field {field}"),
            ));
        };
        if field_schema.kind != InboundFieldKind::NodeRef
            || field_schema.value != u64::from(value)
            || field_schema.crossing != u64::from(crossing)
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound NodeRef record {index} disagrees with field `{}` value/crossing identity",
                    field_schema.name
                ),
            ));
        }
        if !declared_node_refs.contains(&producer_node_ref) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "inbound NodeRef record {index} refers to unknown producer NodeRef {producer_node_ref}"
                ),
            ));
        }
        if !claimed_node_ref_fields.insert(field) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "component field `{}` has more than one inbound NodeRef identity",
                    field_schema.name
                ),
            ));
        }
        if !claimed_node_refs.insert(producer_node_ref) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "producer NodeRef {producer_node_ref} is represented by more than one inbound field"
                ),
            ));
        }
        let bind_target = node_ref
            .get("bind_target")
            .map(|target| {
                let target = target.as_str().ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("inbound NodeRef record {index} bind_target must be a string"),
                    )
                })?;
                match target {
                    "element" => Ok(ComponentNodeRefBindTarget::Element),
                    "html_element" => Ok(ComponentNodeRefBindTarget::HtmlElement),
                    "input" => Ok(ComponentNodeRefBindTarget::Input),
                    "textarea" => Ok(ComponentNodeRefBindTarget::TextArea),
                    "select" => Ok(ComponentNodeRefBindTarget::Select),
                    "form" => Ok(ComponentNodeRefBindTarget::Form),
                    "dialog" => Ok(ComponentNodeRefBindTarget::Dialog),
                    other => Err(invalid(
                        "component",
                        definition,
                        format!(
                            "inbound NodeRef record {index} has unsupported bind_target `{other}`"
                        ),
                    )),
                }
            })
            .transpose()?;
        validate_source_anchor(node_ref, "source_anchor", definition)?;
        validated_node_refs.push(ValidatedInboundNodeRef {
            field,
            value,
            node_ref: producer_node_ref,
            bind_target,
        });
    }

    let declared_signals = required_array(descriptor, "signals", "component", definition)?
        .iter()
        .enumerate()
        .map(|(index, signal)| {
            let signal = signal.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("signals[{index}] must be an object"),
                )
            })?;
            checked_u32(
                required_u64(signal, "id", "component", definition)?,
                "legacy signal ID",
                definition,
            )
        })
        .collect::<Result<BTreeSet<_>, AbiAdapterError>>()?;
    let projections = required_array(inbound, "projection_fields", "component", definition)?;
    let mut claimed_projection_fields = BTreeSet::new();
    let mut projection_signals = BTreeSet::new();
    for (index, projection) in projections.iter().enumerate() {
        let projection = projection.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("inbound_inputs.projection_fields[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "inbound deferred projection",
            projection,
            &[
                "crossing_id",
                "field_id",
                "id",
                "node_ref_id",
                "signal_id",
                "source_anchor",
                "value_id",
            ],
            definition,
        )?;
        require_exact_u64(projection, "id", index as u64, "component", definition)?;
        let field = checked_u32(
            required_u64(projection, "field_id", "component", definition)?,
            "deferred projection field ID",
            definition,
        )?;
        let value = checked_u32(
            required_u64(projection, "value_id", "component", definition)?,
            "deferred projection value ID",
            definition,
        )?;
        let crossing = checked_u32(
            required_u64(projection, "crossing_id", "component", definition)?,
            "deferred projection crossing ID",
            definition,
        )?;
        let signal = checked_u32(
            required_u64(projection, "signal_id", "component", definition)?,
            "deferred projection signal ID",
            definition,
        )?;
        let node_ref = checked_u32(
            required_u64(projection, "node_ref_id", "component", definition)?,
            "deferred projection NodeRef ID",
            definition,
        )?;
        let Some(field_schema) = fields.get(field as usize) else {
            return Err(invalid(
                "component",
                definition,
                format!("deferred projection {index} refers to unknown field {field}"),
            ));
        };
        if field_schema.value != u64::from(value) || field_schema.crossing != u64::from(crossing) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "deferred projection {index} disagrees with field `{}` value/crossing identity",
                    field_schema.name
                ),
            ));
        }
        if !claimed_projection_fields.insert(field) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "component field `{}` has more than one deferred projection record",
                    field_schema.name
                ),
            ));
        }
        if !declared_signals.contains(&signal) {
            return Err(invalid(
                "component",
                definition,
                format!("deferred projection {index} refers to unknown producer signal {signal}"),
            ));
        }
        projection_signals.insert(signal);
        if !declared_node_refs.contains(&node_ref) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "deferred projection {index} refers to unknown producer NodeRef {node_ref}"
                ),
            ));
        }
        validate_source_anchor(projection, "source_anchor", definition)?;

        match field_schema.kind {
            InboundFieldKind::Value => {
                if !validated_prop_uses
                    .iter()
                    .any(|prop_use| prop_use.field == field)
                {
                    let site = u32::try_from(validated_prop_uses.len()).map_err(|_| {
                        invalid(
                            "component",
                            definition,
                            "inbound prop-use site count exceeds the canonical u32 ID space",
                        )
                    })?;
                    validated_prop_uses.push(ValidatedInboundPropUse {
                        site,
                        field,
                        value,
                        temporal: TemporalPropMode::LiveRead,
                        target_signal: Some(signal),
                    });
                }
            }
            InboundFieldKind::NodeRef => {
                if !validated_node_refs
                    .iter()
                    .any(|inbound| inbound.field == field)
                {
                    if !claimed_node_refs.insert(node_ref) {
                        return Err(invalid(
                            "component",
                            definition,
                            format!(
                                "producer NodeRef {node_ref} is represented by more than one inbound field"
                            ),
                        ));
                    }
                    validated_node_refs.push(ValidatedInboundNodeRef {
                        field,
                        value,
                        node_ref,
                        bind_target: None,
                    });
                }
            }
            InboundFieldKind::Callback | InboundFieldKind::ServerOnly => {}
        }
    }
    for (field_id, field) in fields.iter().enumerate() {
        let field_id = u32::try_from(field_id).map_err(|_| {
            invalid(
                "component",
                definition,
                "component field count exceeds the canonical u32 ID space",
            )
        })?;
        if field.record_role == TransportComponentFieldRole::DeferredProjection
            && !claimed_projection_fields.contains(&field_id)
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "deferred projection field `{}` has no latent projection record",
                    field.name
                ),
            ));
        }
    }

    for (field_id, field) in fields.iter().enumerate() {
        let field_id = u32::try_from(field_id).map_err(|_| {
            invalid(
                "component",
                definition,
                "component field count exceeds the canonical u32 ID space",
            )
        })?;
        if field.kind == InboundFieldKind::Value
            && !validated_prop_uses
                .iter()
                .any(|prop_use| prop_use.field == field_id)
        {
            return Err(invalid(
                "component",
                definition,
                format!("inbound value field `{}` has no prop-use site", field.name),
            ));
        }
    }

    Ok(ValidatedInboundInputs {
        fields,
        prop_uses: validated_prop_uses,
        node_refs: validated_node_refs,
        projection_signals,
    })
}

#[cfg(test)]
fn validate_inbound_inputs(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
) -> Result<ValidatedInboundInputs, AbiAdapterError> {
    validate_inbound_inputs_with_projection_roles(
        definition,
        descriptor,
        &ResolvedProjectionRoles::default(),
    )
}

fn validated_component_input_fingerprint<'a>(
    linked: &LinkedTransportComponent,
    transport: &'a Map<String, Value>,
) -> Result<&'a str, AbiAdapterError> {
    let fingerprint = required_str(
        transport,
        "component_input_fingerprint",
        "component",
        linked.definition,
    )?;
    let fingerprint_hex = fingerprint.strip_prefix("sha256:").ok_or_else(|| {
        invalid(
            "component",
            linked.definition,
            "`component_input_fingerprint` must use the `sha256:` prefix",
        )
    })?;
    parse_hex_32(
        fingerprint_hex,
        "component",
        linked.definition,
        "component_input_fingerprint",
    )?;
    let fingerprint_record = component_record_id(fingerprint);
    if fingerprint_record != linked.definition.record {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "component_input_fingerprint derives definition record {}, but the \
                 relocation-linked component definition is {}",
                hex_record(&fingerprint_record),
                hex_record(&linked.definition.record),
            ),
        ));
    }
    Ok(fingerprint)
}

fn validate_component_field_witness_set(
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    inbound_fields: &[ValidatedInboundField],
    witness_layout: &ComponentWitnessLayout,
) -> Result<BTreeMap<u32, ValidatedComponentFieldWitness>, AbiAdapterError> {
    let fingerprint = required_str(
        transport,
        "component_input_fingerprint",
        "component",
        linked.definition,
    )?;
    let fingerprint_hex = fingerprint.strip_prefix("sha256:").ok_or_else(|| {
        invalid(
            "component",
            linked.definition,
            "`component_input_fingerprint` must use the `sha256:` prefix",
        )
    })?;
    parse_hex_32(
        fingerprint_hex,
        "component",
        linked.definition,
        "component_input_fingerprint",
    )?;

    let claims = required_array(
        transport,
        "component_field_witnesses",
        "component",
        linked.definition,
    )?;
    if claims.len() != inbound_fields.len() || claims.len() != linked.field_witnesses.len() {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "component field witnesses must be one-to-one with inbound fields; \
                 found {} payload claims, {} linked records, and {} inbound fields",
                claims.len(),
                linked.field_witnesses.len(),
                inbound_fields.len()
            ),
        ));
    }

    let mut validated = BTreeMap::new();
    let mut seen_records = BTreeMap::<TransportRecordId, String>::new();
    for (index, (claim, linked_field)) in claims.iter().zip(&linked.field_witnesses).enumerate() {
        let claim = claim.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("component_field_witnesses[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "component field witness",
            claim,
            &[
                "callback_shape",
                "crossing_hook_id",
                "field_id",
                "field_name",
                "field_record_id",
                "role",
            ],
            linked.definition,
        )?;
        require_exact_u64(
            claim,
            "field_id",
            index as u64,
            "component",
            linked.definition,
        )?;
        let inbound = &inbound_fields[index];
        let field_name = required_str(claim, "field_name", "component", linked.definition)?;
        if field_name != inbound.name {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "component field witness {index} names `{field_name}`, \
                     but descriptor field {index} is `{}`",
                    inbound.name
                ),
            ));
        }
        require_exact_u64(
            claim,
            "crossing_hook_id",
            inbound.crossing,
            "component",
            linked.definition,
        )?;
        let (role, callback_shape) = match inbound.record_role {
            TransportComponentFieldRole::Value if inbound.callback_shape.is_none() => {
                require_exact_str(claim, "role", "value", "component", linked.definition)?;
                require_exact_str(
                    claim,
                    "callback_shape",
                    "not_callback",
                    "component",
                    linked.definition,
                )?;
                (
                    TransportComponentFieldRole::Value,
                    TransportCallbackShape::NotCallback,
                )
            }
            TransportComponentFieldRole::Callback
                if inbound.callback_shape == Some(InboundCallbackShape::Required) =>
            {
                require_exact_str(claim, "role", "callback", "component", linked.definition)?;
                require_exact_str(
                    claim,
                    "callback_shape",
                    "required",
                    "component",
                    linked.definition,
                )?;
                (
                    TransportComponentFieldRole::Callback,
                    TransportCallbackShape::Required,
                )
            }
            TransportComponentFieldRole::Callback
                if inbound.callback_shape == Some(InboundCallbackShape::Optional) =>
            {
                require_exact_str(claim, "role", "callback", "component", linked.definition)?;
                require_exact_str(
                    claim,
                    "callback_shape",
                    "optional",
                    "component",
                    linked.definition,
                )?;
                (
                    TransportComponentFieldRole::Callback,
                    TransportCallbackShape::Optional,
                )
            }
            TransportComponentFieldRole::NodeRef if inbound.callback_shape.is_none() => {
                require_exact_str(claim, "role", "node_ref", "component", linked.definition)?;
                require_exact_str(
                    claim,
                    "callback_shape",
                    "not_callback",
                    "component",
                    linked.definition,
                )?;
                (
                    TransportComponentFieldRole::NodeRef,
                    TransportCallbackShape::NotCallback,
                )
            }
            TransportComponentFieldRole::DeferredProjection => {
                require_exact_str(
                    claim,
                    "role",
                    "deferred_projection",
                    "component",
                    linked.definition,
                )?;
                require_exact_str(
                    claim,
                    "callback_shape",
                    "not_callback",
                    "component",
                    linked.definition,
                )?;
                (
                    TransportComponentFieldRole::DeferredProjection,
                    TransportCallbackShape::NotCallback,
                )
            }
            _ => {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!(
                        "descriptor field `{}` has an inconsistent role/callback shape",
                        inbound.name
                    ),
                ));
            }
        };

        let field_record =
            parse_record_field(claim, "field_record_id", "component", linked.definition)?;
        if let Some(first_field) = seen_records.insert(field_record, inbound.name.clone()) {
            return Err(AbiAdapterError::DuplicateComponentFieldRecordId {
                definition: linked.definition,
                first_field,
                second_field: inbound.name.clone(),
                field_record: hex_record(&field_record),
            });
        }
        let index = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                "component field count exceeds the canonical u32 ID space",
            )
        })?;
        let crossing = u32::try_from(inbound.crossing).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                format!(
                    "component field witness `{}` crossing hook exceeds the canonical u32 ID space",
                    inbound.name
                ),
            )
        })?;
        let expected_record =
            component_field_record_id(fingerprint, index, crossing, role, callback_shape);
        if field_record != expected_record {
            return Err(AbiAdapterError::ComponentFieldRecordIdMismatch {
                definition: linked.definition,
                field: inbound.name.clone(),
                expected: hex_record(&expected_record),
                actual: hex_record(&field_record),
            });
        }
        if linked_field.field_record != field_record || linked_field.target != linked.definition {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "component field witness `{}` does not match its rustc-resolved record",
                    inbound.name
                ),
            ));
        }
        validated.insert(
            index,
            ValidatedComponentFieldWitness {
                field_record,
                witness: witness_layout.field(linked.definition, index)?,
            },
        );
    }
    Ok(validated)
}

fn checked_descriptor_index(
    id: u64,
    label: &str,
    definition: TransportDefinitionId,
) -> Result<usize, AbiAdapterError> {
    usize::try_from(id).map_err(|_| {
        invalid(
            "component",
            definition,
            format!("{label} id {id} exceeds this target's index space"),
        )
    })
}

fn validate_source_anchor(
    object: &Map<String, Value>,
    field: &str,
    definition: TransportDefinitionId,
) -> Result<(), AbiAdapterError> {
    let anchor = required_object(object, field, "component", definition)?;
    require_known_fields(
        "source anchor",
        anchor,
        &["start_column", "start_line"],
        definition,
    )?;
    required_u64(anchor, "start_line", "component", definition)?;
    required_u64(anchor, "start_column", "component", definition)?;
    Ok(())
}

/// Convert schema-v9 field metadata after exact field records have been
/// relocation-linked and checked against their defining component.
///
/// A server-only field has no browser ABI: both Rust compilations type-check
/// the authored struct literal and render method, while the semantic linker
/// only needs the normalized source name. Requiring a synthetic type witness
/// for that field would invent client-facing authority where none exists.
///
/// Value and callback witnesses prove the defining macro projected the actual
/// Rust field and, for callbacks, checked the exact `Handler<T>` or
/// `Option<Handler<T>>` shape. They deliberately do not invent a nominal type
/// hash or a serialization codec. The v9 immutable static-only slot has no
/// browser representation, while normal Rust type checking validates its field
/// assignment and render bounds.
fn adapt_component_fields(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    inbound: &ValidatedInboundInputs,
    field_witnesses: &BTreeMap<u32, ValidatedComponentFieldWitness>,
    value_layout: &ComponentValueLayout,
) -> Result<AdaptedComponentFields, AbiAdapterError> {
    let fields = required_array(descriptor, "fields", "component", definition)?;
    let mut canonical = BTreeMap::new();

    for (index, value) in fields.iter().enumerate() {
        let field = value.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("fields[{index}] must be an object"),
            )
        })?;
        let kind = required_str(field, "kind", "component", definition)?;
        let name = required_str(field, "name", "component", definition)?.to_owned();
        let field_index = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                definition,
                "component field count exceeds the canonical u32 ID space",
            )
        })?;
        let field_id = ComponentFieldId::new(field_index);
        require_exact_u64(
            field,
            "id",
            u64::try_from(index).expect("usize always fits u64 on supported targets"),
            "component",
            definition,
        )?;

        match kind {
            "server_prop" => {
                require_known_fields(
                    "server-only component field",
                    field,
                    &["id", "kind", "name", "source"],
                    definition,
                )?;
                required_str(field, "source", "component", definition)?;
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
                        role: ComponentFieldRole::ServerOnly,
                        type_witness: None,
                        crossing_hook: None,
                        client_value: None,
                        callback_shape: None,
                        codec: ComponentFieldCodec::None,
                        static_slot: StaticSlotCapability::Unsupported,
                    },
                );
            }
            "static_slot" => {
                require_known_fields(
                    "immutable static-only component slot",
                    field,
                    &[
                        "id",
                        "kind",
                        "name",
                        "permits_nested_owner",
                        "source",
                        "span",
                    ],
                    definition,
                )?;
                require_exact_str(
                    field,
                    "source",
                    "inferred_single_unconditional_server_slot",
                    "component",
                    definition,
                )?;
                require_exact_str(field, "span", "exact_one", "component", definition)?;
                if required_bool(field, "permits_nested_owner", "component", definition)? {
                    return Err(invalid(
                        "component",
                        definition,
                        "v9 inferred static slots cannot preserve nested owners",
                    ));
                }
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
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
            }
            "prop" => {
                require_known_fields(
                    "client value component field",
                    field,
                    &[
                        "crossing_id",
                        "id",
                        "kind",
                        "name",
                        "source",
                        "source_anchor",
                        "value_id",
                    ],
                    definition,
                )?;
                let client_value = value_layout.inbound(
                    definition,
                    required_u64(field, "value_id", "component", definition)?,
                )?;
                let crossing_id =
                    u32::try_from(required_u64(field, "crossing_id", "component", definition)?)
                        .map_err(|_| {
                            invalid(
                                "component",
                                definition,
                                format!("component field `{name}` crossing_id exceeds u32"),
                            )
                        })?;
                required_str(field, "source", "component", definition)?;
                validate_source_anchor(field, "source_anchor", definition)?;
                let witness = field_witnesses.get(&field_index).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("component value field `{name}` has no rustc field witness"),
                    )
                })?;
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
                        role: ComponentFieldRole::Value,
                        type_witness: Some(witness.witness),
                        crossing_hook: Some(CrossingHookId::new(crossing_id)),
                        client_value: Some(client_value),
                        callback_shape: None,
                        codec: ComponentFieldCodec::None,
                        static_slot: StaticSlotCapability::Unsupported,
                    },
                );
            }
            "node_ref" => {
                require_known_fields(
                    "client NodeRef handle component field",
                    field,
                    &[
                        "crossing_id",
                        "id",
                        "kind",
                        "name",
                        "source",
                        "source_anchor",
                        "value_id",
                    ],
                    definition,
                )?;
                require_exact_str(
                    field,
                    "source",
                    "node_ref_handle_alias",
                    "component",
                    definition,
                )?;
                let client_value = value_layout.inbound(
                    definition,
                    required_u64(field, "value_id", "component", definition)?,
                )?;
                let crossing_id = checked_u32(
                    required_u64(field, "crossing_id", "component", definition)?,
                    "NodeRef field crossing ID",
                    definition,
                )?;
                validate_source_anchor(field, "source_anchor", definition)?;
                let witness = field_witnesses.get(&field_index).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("component NodeRef field `{name}` has no rustc field witness"),
                    )
                })?;
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
                        role: ComponentFieldRole::ClientHandle,
                        type_witness: Some(witness.witness),
                        crossing_hook: Some(CrossingHookId::new(crossing_id)),
                        client_value: Some(client_value),
                        callback_shape: None,
                        codec: ComponentFieldCodec::None,
                        static_slot: StaticSlotCapability::Unsupported,
                    },
                );
            }
            "handler" => {
                require_known_fields(
                    "client callback component field",
                    field,
                    &[
                        "callback_slot",
                        "callback_shape",
                        "crossing_id",
                        "dispatch",
                        "id",
                        "kind",
                        "name",
                        "source",
                        "source_anchor",
                        "value_id",
                    ],
                    definition,
                )?;
                required_u64(field, "callback_slot", "component", definition)?;
                let client_value = value_layout.inbound(
                    definition,
                    required_u64(field, "value_id", "component", definition)?,
                )?;
                let callback_shape =
                    match required_str(field, "callback_shape", "component", definition)? {
                        "required" => ComponentCallbackShape::Required,
                        "optional" => ComponentCallbackShape::Optional,
                        other => {
                            return Err(invalid(
                                "component",
                                definition,
                                format!(
                                "callback field `{name}` has unsupported callback_shape `{other}`"
                            ),
                            ));
                        }
                    };
                let crossing_id =
                    u32::try_from(required_u64(field, "crossing_id", "component", definition)?)
                        .map_err(|_| {
                            invalid(
                                "component",
                                definition,
                                format!("component field `{name}` crossing_id exceeds u32"),
                            )
                        })?;
                require_exact_str(
                    field,
                    "dispatch",
                    "resolve_current",
                    "component",
                    definition,
                )?;
                required_str(field, "source", "component", definition)?;
                validate_source_anchor(field, "source_anchor", definition)?;
                let witness = field_witnesses.get(&field_index).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("component callback field `{name}` has no rustc field witness"),
                    )
                })?;
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
                        role: ComponentFieldRole::Callback,
                        type_witness: Some(witness.witness),
                        crossing_hook: Some(CrossingHookId::new(crossing_id)),
                        client_value: Some(client_value),
                        callback_shape: Some(callback_shape),
                        codec: ComponentFieldCodec::None,
                        static_slot: StaticSlotCapability::Unsupported,
                    },
                );
            }
            "deferred_projection" => {
                require_known_fields(
                    "deferred component field projection",
                    field,
                    &[
                        "crossing_id",
                        "id",
                        "kind",
                        "name",
                        "source",
                        "source_anchor",
                        "value_id",
                    ],
                    definition,
                )?;
                require_exact_str(
                    field,
                    "source",
                    "rustc_linked_child_field",
                    "component",
                    definition,
                )?;
                validate_source_anchor(field, "source_anchor", definition)?;
                let resolved = inbound.fields.get(index).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("deferred field `{name}` has no resolved inbound metadata"),
                    )
                })?;
                let crossing_id = checked_u32(
                    required_u64(field, "crossing_id", "component", definition)?,
                    "deferred field crossing ID",
                    definition,
                )?;
                let client_value = value_layout.inbound(
                    definition,
                    required_u64(field, "value_id", "component", definition)?,
                )?;
                let witness = field_witnesses.get(&field_index).ok_or_else(|| {
                    invalid(
                        "component",
                        definition,
                        format!("deferred field `{name}` has no rustc field witness"),
                    )
                })?;
                let (role, type_witness, crossing_hook, client_value, callback_shape) = match (
                    resolved.kind,
                    resolved.callback_shape,
                ) {
                    (InboundFieldKind::ServerOnly, None) => {
                        (ComponentFieldRole::ServerOnly, None, None, None, None)
                    }
                    (InboundFieldKind::Value, None) => (
                        ComponentFieldRole::Value,
                        Some(witness.witness),
                        Some(CrossingHookId::new(crossing_id)),
                        Some(client_value),
                        None,
                    ),
                    (InboundFieldKind::NodeRef, None) => (
                        ComponentFieldRole::ClientHandle,
                        Some(witness.witness),
                        Some(CrossingHookId::new(crossing_id)),
                        Some(client_value),
                        None,
                    ),
                    (InboundFieldKind::Callback, Some(InboundCallbackShape::Required)) => (
                        ComponentFieldRole::Callback,
                        Some(witness.witness),
                        Some(CrossingHookId::new(crossing_id)),
                        Some(client_value),
                        Some(ComponentCallbackShape::Required),
                    ),
                    (InboundFieldKind::Callback, Some(InboundCallbackShape::Optional)) => (
                        ComponentFieldRole::Callback,
                        Some(witness.witness),
                        Some(CrossingHookId::new(crossing_id)),
                        Some(client_value),
                        Some(ComponentCallbackShape::Optional),
                    ),
                    _ => {
                        return Err(invalid(
                                "component",
                                definition,
                                format!(
                                    "deferred field `{name}` resolved to an inconsistent role/callback shape"
                                ),
                            ));
                    }
                };
                canonical.insert(
                    field_id,
                    ComponentFieldSchema {
                        source_name: name,
                        role,
                        type_witness,
                        crossing_hook,
                        client_value,
                        callback_shape,
                        codec: ComponentFieldCodec::None,
                        static_slot: StaticSlotCapability::Unsupported,
                    },
                );
            }
            other => {
                return Err(invalid(
                    "component",
                    definition,
                    format!("unsupported schema-v9 component field kind `{other}`"),
                ));
            }
        }
    }

    Ok(AdaptedComponentFields { canonical })
}

#[derive(Debug)]
struct ValidatedView {
    root_span: SpanCardinality,
    fragment_root: ComponentFragmentRoot,
    has_unknown_kind: bool,
}

fn validate_component_descriptor_header(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
) -> Result<(), AbiAdapterError> {
    require_known_fields(
        "component descriptor v9",
        descriptor,
        &[
            "asset_id",
            "bindings",
            "callback_slots",
            "client_ir_version",
            "client_origin_permission",
            "counts",
            "descriptor_hash",
            "exprs",
            "fields",
            "format",
            "handlers",
            "inbound_inputs",
            "intervals",
            "is_async",
            "js_hash",
            "keyed_collections",
            "mutation_initializers",
            "mutations",
            "node_refs",
            "queries",
            "query_initializers",
            "schema_version",
            "self_ty",
            "shared_stores",
            "shared_state_provider_sites",
            "signals",
            "struct_name",
            "view_tree",
        ],
        definition,
    )?;
    require_exact_str(
        descriptor,
        "format",
        COMPONENT_DESCRIPTOR_FORMAT,
        "component",
        definition,
    )?;
    require_exact_u64(
        descriptor,
        "schema_version",
        COMPONENT_DESCRIPTOR_SCHEMA,
        "component",
        definition,
    )?;
    require_exact_u64(
        descriptor,
        "client_ir_version",
        CLIENT_IR_SCHEMA,
        "component",
        definition,
    )?;
    for field in [
        "asset_id",
        "descriptor_hash",
        "js_hash",
        "self_ty",
        "struct_name",
        "client_origin_permission",
    ] {
        required_str(descriptor, field, "component", definition)?;
    }
    required_bool(descriptor, "is_async", "component", definition)?;

    for field in [
        "callback_slots",
        "exprs",
        "fields",
        "handlers",
        "mutations",
        "node_refs",
        "queries",
        "shared_stores",
        "shared_state_provider_sites",
        "signals",
    ] {
        required_array(descriptor, field, "component", definition)?;
    }
    for field in ["intervals", "mutation_initializers", "query_initializers"] {
        optional_array(descriptor, field, "component", definition)?;
    }
    optional_array(descriptor, "keyed_collections", "component", definition)?;
    required_object(descriptor, "bindings", "component", definition)?;
    required_object(descriptor, "counts", "component", definition)?;
    required_object(descriptor, "inbound_inputs", "component", definition)?;
    required_object(descriptor, "view_tree", "component", definition)?;

    validate_sha256_field(descriptor, "descriptor_hash", "component", definition)?;
    validate_sha256_field(descriptor, "js_hash", "component", definition)?;
    validate_counts(definition, descriptor)
}

fn validate_component_definition_abi(
    linked: &LinkedTransportComponent,
    descriptor: &Map<String, Value>,
) -> Result<(RenderKind, OriginPermission), AbiAdapterError> {
    let claimed_async = required_bool(descriptor, "is_async", "component", linked.definition)?;
    let witnessed_async = matches!(
        linked.component_abi.render_kind,
        TransportComponentRenderKind::Async
    );
    if claimed_async != witnessed_async {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "legacy `is_async` claim {claimed_async} disagrees with the rustc-authenticated component ABI ({})",
                if witnessed_async { "async" } else { "sync" },
            ),
        ));
    }

    let claimed_origin = match required_str(
        descriptor,
        "client_origin_permission",
        "component",
        linked.definition,
    )? {
        "participant_only" => TransportComponentOriginPermission::ParticipantOnly,
        "origin_capable" => TransportComponentOriginPermission::OriginCapable,
        other => {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "unsupported `client_origin_permission` `{other}`; expected `participant_only` or `origin_capable`"
                ),
            ));
        }
    };
    if claimed_origin != linked.component_abi.origin_permission {
        let witnessed = match linked.component_abi.origin_permission {
            TransportComponentOriginPermission::ParticipantOnly => "participant_only",
            TransportComponentOriginPermission::OriginCapable => "origin_capable",
        };
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "legacy `client_origin_permission` claim `{}` disagrees with the rustc-authenticated component ABI (`{witnessed}`)",
                match claimed_origin {
                    TransportComponentOriginPermission::ParticipantOnly => "participant_only",
                    TransportComponentOriginPermission::OriginCapable => "origin_capable",
                },
            ),
        ));
    }

    let render_kind = match linked.component_abi.render_kind {
        TransportComponentRenderKind::Sync => RenderKind::WebRenderSync,
        TransportComponentRenderKind::Async => RenderKind::WebRender,
    };
    let origin_permission = match linked.component_abi.origin_permission {
        TransportComponentOriginPermission::ParticipantOnly => OriginPermission::ParticipantOnly,
        TransportComponentOriginPermission::OriginCapable => OriginPermission::OriginCapable,
    };
    Ok((render_kind, origin_permission))
}

fn validate_counts(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
) -> Result<(), AbiAdapterError> {
    let counts = required_object(descriptor, "counts", "component", definition)?;
    require_known_fields(
        "component descriptor counts",
        counts,
        &[
            "callback_slots",
            "crossings",
            "exprs",
            "fields",
            "handlers",
            "keyed_collections",
            "node_ref_fields",
            "node_refs",
            "nodes",
            "projection_fields",
            "prop_sites",
            "signals",
            "shared_state_provider_sites",
            "values",
        ],
        definition,
    )?;
    let expected = [
        ("fields", "fields"),
        ("signals", "signals"),
        ("handlers", "handlers"),
        ("callback_slots", "callback_slots"),
        ("node_refs", "node_refs"),
        ("exprs", "exprs"),
        ("shared_state_provider_sites", "shared_state_provider_sites"),
    ];
    for (count_name, array_name) in expected {
        let actual = required_u64(counts, count_name, "component", definition)?;
        let array_len = required_array(descriptor, array_name, "component", definition)?.len();
        if actual != array_len as u64 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "counts.{count_name} is {actual}, but `{array_name}` has {array_len} entries"
                ),
            ));
        }
    }
    if let Some(collections) =
        optional_array(descriptor, "keyed_collections", "component", definition)?
    {
        require_exact_u64(
            counts,
            "keyed_collections",
            collections.len() as u64,
            "component",
            definition,
        )?;
    } else if counts.contains_key("keyed_collections") {
        require_exact_u64(counts, "keyed_collections", 0, "component", definition)?;
    }
    let inbound = required_object(descriptor, "inbound_inputs", "component", definition)?;
    for (count_name, array_name) in [
        ("values", "values"),
        ("prop_sites", "prop_sites"),
        ("crossings", "crossings"),
        ("node_ref_fields", "node_ref_fields"),
        ("projection_fields", "projection_fields"),
    ] {
        let actual = required_u64(counts, count_name, "component", definition)?;
        let array_len = required_array(inbound, array_name, "component", definition)?.len();
        if actual != array_len as u64 {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "counts.{count_name} is {actual}, but `inbound_inputs.{array_name}` has {array_len} entries"
                ),
            ));
        }
    }
    let view = required_object(descriptor, "view_tree", "component", definition)?;
    let nodes = required_array(view, "nodes", "component", definition)?.len();
    let count = required_u64(counts, "nodes", "component", definition)?;
    if count != nodes as u64 {
        return Err(invalid(
            "component",
            definition,
            format!("counts.nodes is {count}, but `view_tree.nodes` has {nodes} entries"),
        ));
    }
    Ok(())
}

fn validate_view_tree(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    direct_component_edges: &[ValidatedDirectComponentEdge],
    conditional_bindings: &[LegacyConditionalBinding],
    immutable_static_slot_names: &BTreeSet<String>,
) -> Result<ValidatedView, AbiAdapterError> {
    let tree = required_object(descriptor, "view_tree", "component", definition)?;
    require_known_fields("component view tree", tree, &["nodes", "root"], definition)?;
    let nodes = required_array(tree, "nodes", "component", definition)?;
    let mut kinds = Vec::with_capacity(nodes.len());
    let mut direct_component_sites = BTreeSet::new();
    let mut immutable_static_slot_counts = BTreeMap::<String, usize>::new();
    let mut children = Vec::<Vec<u32>>::with_capacity(nodes.len());
    let mut server_sources = Vec::<Option<String>>::with_capacity(nodes.len());
    let mut for_loop_nodes = BTreeSet::new();
    let mut has_unknown_kind = false;

    for (expected_id, record) in nodes.iter().enumerate() {
        let mut node_children = Vec::new();
        let mut server_source = None;
        let record = record.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("view_tree.nodes[{expected_id}] must be an object"),
            )
        })?;
        require_known_fields(
            "component view-tree node record",
            record,
            &["id", "node"],
            definition,
        )?;
        let id = required_u64(record, "id", "component", definition)?;
        if id != expected_id as u64 {
            return Err(invalid(
                "component",
                definition,
                format!("view_tree node IDs must be dense; expected {expected_id}, found {id}"),
            ));
        }
        let node = required_object(record, "node", "component", definition)?;
        let kind = required_str(node, "kind", "component", definition)?;
        match kind {
            "element" => {
                require_known_fields(
                    "element view node",
                    node,
                    &[
                        "children",
                        "kind",
                        "payload_attrs",
                        "server_attrs",
                        "static_attrs",
                        "tag",
                    ],
                    definition,
                )?;
                required_str(node, "tag", "component", definition)?;
                node_children = required_u32_array(node, "children", definition)?;
                required_array(node, "static_attrs", "component", definition)?;
                required_array(node, "server_attrs", "component", definition)?;
                required_array(node, "payload_attrs", "component", definition)?;
            }
            "text_binding_slot" => {
                require_known_fields("text binding view node", node, &["kind"], definition)?;
            }
            "static_text" => {
                require_known_fields("static text view node", node, &["kind", "text"], definition)?;
                required_str(node, "text", "component", definition)?;
            }
            "server_only" => {
                require_known_fields(
                    "server-only view node",
                    node,
                    &["kind", "source", "tokens"],
                    definition,
                )?;
                let source = required_str(node, "source", "component", definition)?;
                server_source = Some(source.to_owned());
                if source == "direct_component_site" {
                    direct_component_sites.insert(expected_id);
                }
                let tokens = required_str(node, "tokens", "component", definition)?;
                if source == "inferred_simple_ssr" {
                    if let Some(field_name) = direct_self_field_name_from_tokens(tokens) {
                        if immutable_static_slot_names.contains(&field_name) {
                            *immutable_static_slot_counts.entry(field_name).or_default() += 1;
                        }
                    }
                }
            }
            "for_loop" => {
                require_known_fields(
                    "server loop view node",
                    node,
                    &[
                        "attrs",
                        "body_prefix",
                        "children",
                        "expr",
                        "kind",
                        "label",
                        "pat",
                    ],
                    definition,
                )?;
                for field in ["attrs", "body_prefix", "expr", "label", "pat"] {
                    required_str(node, field, "component", definition)?;
                }
                node_children = required_u32_array(node, "children", definition)?;
                for_loop_nodes.insert(expected_id as u32);
            }
            "unknown" => {
                require_known_fields("unknown view node", node, &["kind"], definition)?;
                has_unknown_kind = true;
            }
            other => {
                return Err(invalid(
                    "component",
                    definition,
                    format!("unsupported view node kind `{other}` in schema v9"),
                ));
            }
        }
        kinds.push(kind.to_owned());
        children.push(node_children);
        server_sources.push(server_source);
    }

    let mut claimed_component_sites = BTreeSet::new();
    for edge in direct_component_edges {
        let site = usize::try_from(edge.site_node).map_err(|_| {
            invalid(
                "component",
                definition,
                format!(
                    "direct component edge {} site node {} exceeds host usize",
                    edge.local_ordinal, edge.site_node
                ),
            )
        })?;
        let expected_source = if edge.composed {
            "direct_component_site"
        } else {
            "explicit_island_boundary"
        };
        if kinds.get(site).map(String::as_str) != Some("server_only")
            || server_sources.get(site).and_then(Option::as_deref) != Some(expected_source)
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "direct component edge {} claims site node {}, but that node is not a `{expected_source}` server site",
                    edge.local_ordinal, edge.site_node
                ),
            ));
        }
        if !claimed_component_sites.insert(edge.site_node) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "more than one direct component edge claims schema-v9 site node {}",
                    edge.site_node
                ),
            ));
        }

        let containing_controls =
            containing_conditional_controls(edge.site_node, conditional_bindings, &children);
        let inside_loop = node_has_ancestor_in(edge.site_node, &for_loop_nodes, &children);
        match edge.lifecycle {
            ValidatedDirectComponentLifecycle::StaticSingle => {
                if inside_loop || !containing_controls.is_empty() {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "direct component edge {} claims `static_single`, but site node {} is structurally controlled",
                            edge.local_ordinal, edge.site_node
                        ),
                    ));
                }
            }
            ValidatedDirectComponentLifecycle::ConditionalSingle {
                control, branch, ..
            } => {
                if inside_loop {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "direct component edge {} claims `conditional_single`, but site node {} is repeated by a loop",
                            edge.local_ordinal, edge.site_node
                        ),
                    ));
                }
                let expected = vec![(control, branch)];
                if containing_controls != expected {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "direct component edge {} conditional lifecycle claims control {control} branch `{}`, but site node {} is contained by {containing_controls:?}",
                            edge.local_ordinal,
                            component_lifecycle_branch_name(branch),
                            edge.site_node,
                        ),
                    ));
                }
            }
            ValidatedDirectComponentLifecycle::Repeated => {
                if !inside_loop {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "direct component edge {} claims `repeated`, but site node {} has no loop ancestor",
                            edge.local_ordinal, edge.site_node
                        ),
                    ));
                }
            }
            ValidatedDirectComponentLifecycle::UnknownNestedConditionals => {
                if inside_loop || containing_controls.len() < 2 {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "direct component edge {} claims unknown nested conditionals, but site node {} is contained by {} conditional control(s){}",
                            edge.local_ordinal,
                            edge.site_node,
                            containing_controls.len(),
                            if inside_loop { " and a loop" } else { "" },
                        ),
                    ));
                }
            }
        }
    }

    let composed_edge_count = direct_component_edges
        .iter()
        .filter(|edge| edge.composed)
        .count();
    if direct_component_sites.len() != composed_edge_count {
        return Err(invalid(
            "component",
            definition,
            format!(
                "view tree carries {} direct component site(s), but transport carries \
                 {composed_edge_count} rustc-resolved composed edge(s); every preserved \
                 composed edge must own exactly one site",
                direct_component_sites.len()
            ),
        ));
    }
    for field_name in immutable_static_slot_names {
        if immutable_static_slot_counts.get(field_name) != Some(&1) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "immutable static-slot field `{field_name}` must name exactly one \
                     unconditional `self.{field_name}` server node"
                ),
            ));
        }
    }

    let root = tree
        .get("root")
        .ok_or_else(|| invalid("component", definition, "missing `view_tree.root`"))?;
    let (root_span, fragment_root) = if root.is_null() {
        if !nodes.is_empty() {
            return Err(invalid(
                "component",
                definition,
                "view_tree.root is null but nodes are present",
            ));
        }
        (
            SpanCardinality::Exact(0),
            ComponentFragmentRoot::Unsupported,
        )
    } else {
        let root = root.as_u64().ok_or_else(|| {
            invalid(
                "component",
                definition,
                "`view_tree.root` must be null or an unsigned node ID",
            )
        })?;
        let root = usize::try_from(root).map_err(|_| {
            invalid(
                "component",
                definition,
                "`view_tree.root` exceeds host usize",
            )
        })?;
        let kind = kinds.get(root).ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("view_tree.root {root} does not name a node"),
            )
        })?;
        let (span, fragment_root) = match kind.as_str() {
            "element" => (
                SpanCardinality::Exact(1),
                ComponentFragmentRoot::ExactOneElement,
            ),
            "static_text" | "text_binding_slot" => (
                SpanCardinality::Exact(1),
                ComponentFragmentRoot::Unsupported,
            ),
            "server_only" if direct_component_sites.contains(&root) => (
                // Final semantic linking replaces the preserved Rust SSR
                // expression with `LinkedRenderBoundary`, whose wrapper is
                // exactly one element and is therefore a valid fragment root.
                SpanCardinality::Exact(1),
                ComponentFragmentRoot::ExactOneElement,
            ),
            "server_only" | "for_loop" | "unknown" => (
                SpanCardinality::Unknown {
                    region: RegionSiteId::new(0),
                    anchored: false,
                },
                ComponentFragmentRoot::Unsupported,
            ),
            _ => unreachable!("all accepted node kinds handled above"),
        };
        (span, fragment_root)
    };
    Ok(ValidatedView {
        root_span,
        fragment_root,
        has_unknown_kind,
    })
}

fn containing_conditional_controls(
    site: u32,
    conditionals: &[LegacyConditionalBinding],
    children: &[Vec<u32>],
) -> Vec<(u32, ComponentLifecycleBranch)> {
    let mut controls = Vec::new();
    for conditional in conditionals {
        if conditional
            .then_children
            .iter()
            .copied()
            .any(|root| subtree_contains_node(root, site, children))
        {
            controls.push((conditional.control, ComponentLifecycleBranch::Then));
        }
        if conditional
            .else_children
            .iter()
            .copied()
            .any(|root| subtree_contains_node(root, site, children))
        {
            controls.push((conditional.control, ComponentLifecycleBranch::Else));
        }
    }
    controls
}

fn subtree_contains_node(root: u32, target: u32, children: &[Vec<u32>]) -> bool {
    let mut stack = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == target {
            return true;
        }
        if !visited.insert(node) {
            continue;
        }
        if let Some(node_children) = children.get(node as usize) {
            stack.extend(node_children.iter().copied());
        }
    }
    false
}

fn node_has_ancestor_in(node: u32, ancestors: &BTreeSet<u32>, children: &[Vec<u32>]) -> bool {
    ancestors
        .iter()
        .copied()
        .any(|ancestor| ancestor != node && subtree_contains_node(ancestor, node, children))
}

const fn component_lifecycle_branch_name(branch: ComponentLifecycleBranch) -> &'static str {
    match branch {
        ComponentLifecycleBranch::Then => "then",
        ComponentLifecycleBranch::Else => "else",
    }
}

fn direct_self_field_name_from_tokens(tokens: &str) -> Option<String> {
    fn inner(expression: &syn::Expr) -> Option<String> {
        let expression = match expression {
            syn::Expr::Paren(paren) => return inner(&paren.expr),
            syn::Expr::Group(group) => return inner(&group.expr),
            syn::Expr::Reference(reference) => return inner(&reference.expr),
            expression => expression,
        };
        let syn::Expr::Field(field) = expression else {
            return None;
        };
        let syn::Expr::Path(base) = field.base.as_ref() else {
            return None;
        };
        if !base.path.is_ident("self") {
            return None;
        }
        let syn::Member::Named(name) = &field.member else {
            return None;
        };
        Some(name.to_string())
    }

    syn::parse_str::<syn::Expr>(tokens)
        .ok()
        .as_ref()
        .and_then(inner)
}

fn validate_hydration_constraint(
    definition: TransportDefinitionId,
    transport: &Map<String, Value>,
) -> Result<HydrationConstraint, AbiAdapterError> {
    let constraint = required_object(transport, "hydration_constraint", "component", definition)?;
    let kind = required_str(constraint, "kind", "component", definition)?;
    match kind {
        "unconstrained" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["kind"],
                definition,
            )?;
            Ok(HydrationConstraint::Unconstrained)
        }
        "eager" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["kind"],
                definition,
            )?;
            Ok(HydrationConstraint::Eager)
        }
        "visible" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["kind"],
                definition,
            )?;
            Ok(HydrationConstraint::Visible)
        }
        "idle" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["kind"],
                definition,
            )?;
            Ok(HydrationConstraint::Idle)
        }
        "interaction" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["event", "kind"],
                definition,
            )?;
            let event = match constraint.get("event") {
                Some(Value::Null) => None,
                Some(Value::String(event)) if !event.trim().is_empty() => Some(event.clone()),
                Some(Value::String(_)) => {
                    return Err(invalid(
                        "component",
                        definition,
                        "interaction hydration event cannot be empty",
                    ));
                }
                Some(_) => {
                    return Err(invalid(
                        "component",
                        definition,
                        "interaction hydration `event` must be a string or null",
                    ));
                }
                None => {
                    return Err(invalid(
                        "component",
                        definition,
                        "interaction hydration constraint is missing `event`",
                    ));
                }
            };
            Ok(HydrationConstraint::Interaction { event })
        }
        "media" => {
            require_known_fields(
                "component hydration constraint",
                constraint,
                &["kind", "query"],
                definition,
            )?;
            let query = required_str(constraint, "query", "component", definition)?;
            if query.trim().is_empty() {
                return Err(invalid(
                    "component",
                    definition,
                    "media hydration query cannot be empty",
                ));
            }
            Ok(HydrationConstraint::Media {
                query: query.to_owned(),
            })
        }
        other => Err(invalid(
            "component",
            definition,
            format!("unsupported hydration constraint kind `{other}`"),
        )),
    }
}

fn validate_payload_handler_values(
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    descriptor: &Map<String, Value>,
    inbound: &ValidatedInboundInputs,
    direct_component_edges: &[ValidatedDirectComponentEdge],
    direct_helper_edges: &[ValidatedComponentHelperEdge],
) -> Result<Vec<ValidatedPayloadHandlerValue>, AbiAdapterError> {
    let sources = required_object(
        transport,
        "client_value_sources",
        "component",
        linked.definition,
    )?;
    require_known_fields(
        "component client value sources",
        sources,
        &["payload_handlers"],
        linked.definition,
    )?;
    let handlers = required_array(sources, "payload_handlers", "component", linked.definition)?;

    let referenced = direct_component_edges
        .iter()
        .flat_map(|edge| &edge.fields)
        .filter_map(|field| match field.binding {
            ValidatedDirectComponentBinding::LiveCallback {
                handler_prelude_local,
                ..
            } => Some(handler_prelude_local),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if handlers.len() != referenced.len() {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "payload-handler values must be one-to-one with referenced live callbacks; found {} records for {} referenced prelude bindings",
                handlers.len(),
                referenced.len(),
            ),
        ));
    }

    let mut validated = Vec::with_capacity(handlers.len());
    let mut actual_bindings = Vec::with_capacity(handlers.len());
    for (index, handler) in handlers.iter().enumerate() {
        let handler = handler.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("client_value_sources.payload_handlers[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "payload-handler client value",
            handler,
            &[
                "body_ir",
                "body_ir_version",
                "dependencies",
                "kind",
                "payload_arity",
                "prelude_local_index",
                "source_anchor",
            ],
            linked.definition,
        )?;
        require_exact_str(
            handler,
            "kind",
            "payload_handler",
            "component",
            linked.definition,
        )?;
        require_exact_u64(
            handler,
            "body_ir_version",
            CLIENT_IR_SCHEMA,
            "component",
            linked.definition,
        )?;
        let prelude_local_index = checked_u32(
            required_u64(
                handler,
                "prelude_local_index",
                "component",
                linked.definition,
            )?,
            "payload-handler prelude_local_index",
            linked.definition,
        )?;
        if actual_bindings
            .last()
            .is_some_and(|previous| *previous >= prelude_local_index)
        {
            return Err(invalid(
                "component",
                linked.definition,
                "payload-handler values must be strictly ordered by prelude_local_index",
            ));
        }
        actual_bindings.push(prelude_local_index);

        let payload_arity = required_u64(handler, "payload_arity", "component", linked.definition)?;
        let payload_arity = match payload_arity {
            0 | 1 => payload_arity as u8,
            other => {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!(
                        "payload handler at prelude binding {prelude_local_index} has unsupported payload_arity {other}; expected 0 or 1"
                    ),
                ));
            }
        };

        let source_anchor =
            required_object(handler, "source_anchor", "component", linked.definition)?;
        require_known_fields(
            "payload-handler source anchor",
            source_anchor,
            &["column", "line"],
            linked.definition,
        )?;
        required_u64(source_anchor, "line", "component", linked.definition)?;
        required_u64(source_anchor, "column", "component", linked.definition)?;

        let body_json = handler.get("body_ir").ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                "payload-handler client value is missing `body_ir`",
            )
        })?;
        let body = parse_client_block_json(body_json).map_err(|error| {
            invalid(
                "component",
                linked.definition,
                format!(
                    "payload handler at prelude binding {prelude_local_index} has invalid Client IR v3: {error}"
                ),
            )
        })?;
        let normalized_body: Value = serde_json::from_str(&client_block_json(&body)).map_err(
            |error| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "failed to normalize payload handler at prelude binding {prelude_local_index}: {error}"
                    ),
                )
            },
        )?;
        if normalized_body != *body_json {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "payload handler at prelude binding {prelude_local_index} is not canonical Client IR v3 JSON or contains unknown semantic fields"
                ),
            ));
        }

        let dependencies =
            collect_client_block_external_dependencies(&body).map_err(|error| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "payload handler at prelude binding {prelude_local_index} is not a closed transportable Client IR block: {error:?}"
                    ),
                )
            })?;
        let browser_operations = collect_handler_browser_operations(&body).map_err(|error| {
            invalid(
                "component",
                linked.definition,
                format!(
                    "payload handler at prelude binding {prelude_local_index} has invalid browser-operation structure: {error}"
                ),
            )
        })?;
        if payload_arity == 0 && dependencies.uses_callback_payload {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "zero-argument payload handler at prelude binding {prelude_local_index} reads CallbackPayload"
                ),
            ));
        }
        if dependencies.uses_event_payload {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "payload handler at prelude binding {prelude_local_index} reads DOM EventPayload, but component callbacks receive only CallbackPayload"
                ),
            ));
        }
        validate_payload_handler_dependency_claims(
            linked.definition,
            prelude_local_index,
            handler,
            &dependencies,
        )?;
        validate_payload_handler_dependency_ids(
            linked.definition,
            descriptor,
            inbound,
            direct_helper_edges,
            &dependencies,
        )?;

        validated.push(ValidatedPayloadHandlerValue {
            prelude_local_index,
            payload_arity,
            body_hash: canonical_legacy_json_hash(
                PAYLOAD_HANDLER_CLIENT_IR_HASH_DOMAIN,
                &normalized_body,
            ),
            body_ir: normalized_body,
            dependencies,
            browser_operations,
        });
    }

    let expected_bindings = referenced.into_iter().collect::<Vec<_>>();
    if actual_bindings != expected_bindings {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "payload-handler values do not exactly match referenced live-callback prelude bindings; expected {expected_bindings:?}, found {actual_bindings:?}"
            ),
        ));
    }
    Ok(validated)
}

fn validate_payload_handler_dependency_claims(
    definition: TransportDefinitionId,
    prelude_local_index: u32,
    handler: &Map<String, Value>,
    computed: &ClientBlockExternalDependencies,
) -> Result<(), AbiAdapterError> {
    let claims = required_object(handler, "dependencies", "component", definition)?;
    require_known_fields(
        "payload-handler dependency claims",
        claims,
        &[
            "callback_slots",
            "client_module_calls",
            "helper_edges",
            "mutations",
            "node_refs",
            "queries",
            "signal_reads",
            "signal_writes",
        ],
        definition,
    )?;

    for (field, expected) in [
        (
            "signal_reads",
            computed
                .signal_reads
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
        ),
        (
            "signal_writes",
            computed
                .signal_writes
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
        ),
        (
            "queries",
            computed.queries.iter().map(|id| id.0).collect::<Vec<_>>(),
        ),
        (
            "mutations",
            computed.mutations.iter().map(|id| id.0).collect::<Vec<_>>(),
        ),
        (
            "node_refs",
            computed.node_refs.iter().map(|id| id.0).collect::<Vec<_>>(),
        ),
        (
            "callback_slots",
            computed
                .callback_slots
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
        ),
        (
            "helper_edges",
            computed
                .helper_edges
                .iter()
                .map(|id| id.index())
                .collect::<Vec<_>>(),
        ),
    ] {
        let actual = required_u32_array(claims, field, definition)?;
        if actual != expected {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "payload handler at prelude binding {prelude_local_index} claims {field} {actual:?}, but its Client IR computes {expected:?}"
                ),
            ));
        }
    }

    let calls = required_array(claims, "client_module_calls", "component", definition)?;
    let mut actual_calls = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        let call = call.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("client_module_calls[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "payload-handler client-module call",
            call,
            &["module", "name"],
            definition,
        )?;
        actual_calls.push(ClientModuleCall {
            module: required_str(call, "module", "component", definition)?.to_owned(),
            name: required_str(call, "name", "component", definition)?.to_owned(),
        });
    }
    if actual_calls != computed.client_module_calls {
        return Err(invalid(
            "component",
            definition,
            format!(
                "payload handler at prelude binding {prelude_local_index} claims client-module calls {actual_calls:?}, but its Client IR computes {:?}",
                computed.client_module_calls,
            ),
        ));
    }
    Ok(())
}

fn validate_payload_handler_dependency_ids(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    inbound: &ValidatedInboundInputs,
    direct_helper_edges: &[ValidatedComponentHelperEdge],
    dependencies: &ClientBlockExternalDependencies,
) -> Result<(), AbiAdapterError> {
    validate_descriptor_ids(
        definition,
        descriptor,
        "signals",
        dependencies
            .signal_reads
            .iter()
            .chain(&dependencies.signal_writes)
            .map(|id| id.0),
    )?;
    validate_descriptor_ids(
        definition,
        descriptor,
        "queries",
        dependencies.queries.iter().map(|id| id.0),
    )?;
    validate_descriptor_ids(
        definition,
        descriptor,
        "mutations",
        dependencies.mutations.iter().map(|id| id.0),
    )?;
    validate_descriptor_ids(
        definition,
        descriptor,
        "node_refs",
        dependencies.node_refs.iter().map(|id| id.0),
    )?;
    validate_descriptor_ids(
        definition,
        descriptor,
        "callback_slots",
        dependencies.callback_slots.iter().map(|id| id.0),
    )?;

    let callback_slots = required_array(descriptor, "callback_slots", "component", definition)?;
    for callback in &dependencies.callback_slots {
        let record = callback_slots
            .iter()
            .filter_map(Value::as_object)
            .find(|record| {
                record
                    .get("id")
                    .and_then(Value::as_u64)
                    .is_some_and(|id| id == u64::from(callback.0))
            })
            .expect("validated descriptor callback ID is present");
        let name = required_str(record, "name", "component", definition)?;
        if !inbound
            .fields
            .iter()
            .any(|field| field.kind == InboundFieldKind::Callback && field.name == name)
        {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "payload handler invokes callback slot {} (`{name}`), but it has no matching inbound callback field",
                    callback.0,
                ),
            ));
        }
    }

    let helper_ordinals = direct_helper_edges
        .iter()
        .map(|edge| edge.local_ordinal)
        .collect::<BTreeSet<_>>();
    for helper in &dependencies.helper_edges {
        if !helper_ordinals.contains(&helper.index()) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "payload handler references helper edge {}, but no relocation-resolved reachable helper has that producer-local ordinal",
                    helper.index(),
                ),
            ));
        }
    }
    Ok(())
}

fn validate_descriptor_ids(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    collection: &'static str,
    referenced: impl IntoIterator<Item = u32>,
) -> Result<(), AbiAdapterError> {
    let records = required_array(descriptor, collection, "component", definition)?;
    let mut present = BTreeSet::new();
    let mut previous = None;
    for (index, record) in records.iter().enumerate() {
        let record = record.as_object().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("{collection}[{index}] must be an object"),
            )
        })?;
        let id = checked_u32(
            required_u64(record, "id", "component", definition)?,
            &format!("{collection} ID"),
            definition,
        )?;
        if previous.is_some_and(|previous| id <= previous) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "{collection} IDs must be strictly increasing after optimization; \
                     {collection}[{index}] has ID {id} after {}",
                    previous.expect("checked above")
                ),
            ));
        }
        present.insert(id);
        previous = Some(id);
    }
    for id in referenced {
        if !present.contains(&id) {
            return Err(invalid(
                "component",
                definition,
                format!("payload-handler dependency references unknown {collection} ID {id}"),
            ));
        }
    }
    Ok(())
}

fn required_u32_array(
    object: &Map<String, Value>,
    field: &str,
    definition: TransportDefinitionId,
) -> Result<Vec<u32>, AbiAdapterError> {
    required_array(object, field, "component", definition)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = value.as_u64().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("`{field}[{index}]` must be an unsigned integer"),
                )
            })?;
            checked_u32(value, field, definition)
        })
        .collect()
}

fn checked_u32(
    value: u64,
    label: &str,
    definition: TransportDefinitionId,
) -> Result<u32, AbiAdapterError> {
    u32::try_from(value).map_err(|_| {
        invalid(
            "component",
            definition,
            format!("{label} {value} exceeds the canonical u32 ID space"),
        )
    })
}

#[derive(Debug)]
struct ValidatedDirectComponentEdge {
    local_ordinal: u32,
    source_edge: TransportRecordId,
    composed: bool,
    has_rest: bool,
    lifecycle: ValidatedDirectComponentLifecycle,
    site_node: u32,
    fields: Vec<ValidatedDirectComponentField>,
}

impl ValidatedDirectComponentEdge {
    fn has_supported_binding_shape(&self) -> bool {
        !self.has_rest
            && self.fields.iter().all(|field| {
                matches!(
                    (self.composed, field.binding),
                    (true, ValidatedDirectComponentBinding::ServerOnly)
                        | (false, ValidatedDirectComponentBinding::ServerOnly)
                        | (true, ValidatedDirectComponentBinding::StaticCallback { .. })
                        | (
                            false,
                            ValidatedDirectComponentBinding::StaticCallback { .. }
                        )
                        | (true, ValidatedDirectComponentBinding::LiveValue { .. })
                        | (true, ValidatedDirectComponentBinding::LiveCallback { .. })
                        | (
                            true,
                            ValidatedDirectComponentBinding::ForwardedCallback { .. }
                        )
                        | (true, ValidatedDirectComponentBinding::NodeRefHandle { .. })
                        | (false, ValidatedDirectComponentBinding::StaticSlotExactOne)
                )
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidatedDirectComponentLifecycle {
    StaticSingle,
    ConditionalSingle {
        control: u32,
        predicate_expression: u32,
        predicate_graph: TransportRecordId,
        branch: ComponentLifecycleBranch,
    },
    Repeated,
    UnknownNestedConditionals,
}

impl ValidatedDirectComponentLifecycle {
    fn transport(self) -> TransportDirectComponentLifecycle {
        match self {
            Self::StaticSingle => TransportDirectComponentLifecycle::StaticSingle,
            Self::ConditionalSingle {
                control,
                predicate_expression,
                predicate_graph,
                branch,
            } => TransportDirectComponentLifecycle::ConditionalSingle {
                control,
                predicate_expression,
                predicate_graph,
                branch: match branch {
                    ComponentLifecycleBranch::Then => TransportDirectComponentBranch::Then,
                    ComponentLifecycleBranch::Else => TransportDirectComponentBranch::Else,
                },
            },
            Self::Repeated => TransportDirectComponentLifecycle::Repeated,
            Self::UnknownNestedConditionals => {
                TransportDirectComponentLifecycle::UnknownNestedConditionals
            }
        }
    }
}

#[derive(Debug)]
struct ValidatedDirectComponentField {
    name: String,
    binding: ValidatedDirectComponentBinding,
    transport_binding: TransportDirectComponentBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidatedDirectComponentBinding {
    ServerOnly,
    StaticSlotExactOne,
    StaticCallback {
        shape: ComponentCallbackShape,
        form: TransportDirectStaticCallbackForm,
    },
    LiveValue {
        signal: u32,
        source: TransportDirectLiveValueSource,
    },
    LiveCallback {
        handler_prelude_local: u32,
        shape: ComponentCallbackShape,
        form: TransportDirectLiveCallbackForm,
    },
    ForwardedCallback {
        parent_field: u32,
        shape: ComponentCallbackShape,
    },
    NodeRefHandle {
        node_ref: u32,
    },
    Unsupported,
}

impl ValidatedDirectComponentBinding {
    fn transport(self) -> TransportDirectComponentBinding {
        match self {
            Self::ServerOnly => TransportDirectComponentBinding::ServerOnly,
            Self::StaticSlotExactOne => TransportDirectComponentBinding::StaticSlotExactOne,
            Self::StaticCallback { form, .. } => {
                TransportDirectComponentBinding::StaticCallback { form }
            }
            Self::LiveValue { signal, source } => {
                TransportDirectComponentBinding::LiveValue { signal, source }
            }
            Self::LiveCallback {
                handler_prelude_local,
                form,
                ..
            } => TransportDirectComponentBinding::LiveCallback {
                handler_prelude_local,
                form,
            },
            Self::ForwardedCallback { .. } => {
                unreachable!("a resolved forwarded callback retains a separate raw commitment")
            }
            Self::NodeRefHandle { node_ref } => {
                TransportDirectComponentBinding::NodeRefHandle { node_ref }
            }
            Self::Unsupported => TransportDirectComponentBinding::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidatedLiveValueSourceForm {
    DirectSignalRead,
    ClientAlias,
    InboundField,
}

fn validate_component_edge_set(
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    descriptor: &Map<String, Value>,
    input_fingerprint: &str,
    projection_roles: &ResolvedProjectionRoles,
) -> Result<Vec<ValidatedDirectComponentEdge>, AbiAdapterError> {
    let payload_edges = required_array(
        transport,
        "direct_component_edges",
        "component",
        linked.definition,
    )?;
    // Relocations are the authority for the exact rustc-resolved edge set.
    // Compare that envelope before using a claimed edge ID to look up its
    // target; otherwise a coherently recommitted JSON edge is reported as a
    // generic missing child instead of the relocation mismatch it actually
    // represents.
    let payload_ids = payload_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let edge = edge.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!("direct_component_edges[{index}] must be an object"),
                )
            })?;
            parse_record_field(edge, "source_edge_id", "component", linked.definition)
        })
        .collect::<Result<Vec<_>, _>>()?;
    compare_component_edges(
        linked.definition,
        "component",
        &payload_ids,
        &linked.component_edges,
    )?;
    let mut validated = Vec::with_capacity(payload_edges.len());
    for (index, edge) in payload_edges.iter().enumerate() {
        let edge = edge.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("direct_component_edges[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "component source edge",
            edge,
            &[
                "authored_path",
                "boundary",
                "fields",
                "has_rest",
                "identity_authority",
                "lifecycle",
                "local_ordinal",
                "site_node",
                "source_anchor",
                "source_edge_id",
            ],
            linked.definition,
        )?;
        require_exact_str(
            edge,
            "identity_authority",
            "rustc_component_witness_relocation",
            "component",
            linked.definition,
        )?;
        let composed = match required_str(edge, "boundary", "component", linked.definition)? {
            "composed" => true,
            "explicit_island" => false,
            other => {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!("unsupported direct component boundary `{other}`"),
                ));
            }
        };
        required_str(edge, "authored_path", "component", linked.definition)?;
        let has_rest = required_bool(edge, "has_rest", "component", linked.definition)?;
        let local_ordinal = u32::try_from(required_u64(
            edge,
            "local_ordinal",
            "component",
            linked.definition,
        )?)
        .map_err(|_| {
            invalid(
                "component",
                linked.definition,
                format!("direct_component_edges[{index}].local_ordinal exceeds u32"),
            )
        })?;
        if local_ordinal as usize != index {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "direct component local ordinals must be dense and source ordered; \
                     expected {index}, found {local_ordinal}"
                ),
            ));
        }
        let source_edge = payload_ids[index];
        let target = linked
            .component_edges
            .iter()
            .find(|candidate| candidate.source_edge == source_edge)
            .map(|candidate| candidate.target)
            .ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "direct component local ordinal {local_ordinal} source edge {} has no rustc-resolved child definition",
                        hex_record(&source_edge),
                    ),
                )
            })?;
        let lifecycle = validate_direct_component_lifecycle(
            linked.definition,
            required_object(edge, "lifecycle", "component", linked.definition)?,
            descriptor,
        )?;
        let site_node = checked_u32(
            required_u64(edge, "site_node", "component", linked.definition)?,
            "direct component site node",
            linked.definition,
        )?;
        let mut fields = Vec::new();
        let mut field_names = BTreeSet::new();
        for (field_index, field) in required_array(edge, "fields", "component", linked.definition)?
            .iter()
            .enumerate()
        {
            let field = field.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "direct_component_edges[{index}].fields[{field_index}] must be an object"
                    ),
                )
            })?;
            require_known_fields(
                "direct component field binding",
                field,
                &["binding", "field_ordinal", "name"],
                linked.definition,
            )?;
            let field_ordinal =
                required_u64(field, "field_ordinal", "component", linked.definition)?;
            if field_ordinal != field_index as u64 {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!(
                        "direct component field ordinals must be dense and source ordered; \
                         expected {field_index}, found {field_ordinal}"
                    ),
                ));
            }
            let name = required_str(field, "name", "component", linked.definition)?.to_owned();
            if !field_names.insert(name.clone()) {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!("direct component edge contains duplicate field `{name}`"),
                ));
            }
            let binding = required_object(field, "binding", "component", linked.definition)?;
            let (mut binding, transport_binding) =
                validate_direct_component_binding_with_projection_roles(
                    linked.definition,
                    binding,
                    projection_roles,
                )?;
            // `None` has no type at descriptor-expansion time. The producer
            // therefore commits the exact ambiguous syntax as a static
            // callback candidate, while this final adapter uses the
            // relocation-resolved child definition to select its semantic
            // meaning. Keep `transport_binding` unchanged so the source-edge
            // commitment continues to authenticate the producer's bytes.
            if matches!(
                binding,
                ValidatedDirectComponentBinding::StaticCallback {
                    shape: ComponentCallbackShape::Optional,
                    form: TransportDirectStaticCallbackForm::None,
                }
            ) {
                match projection_roles.field_by_name(target, &name) {
                    Some(ResolvedProjectionRole::Callback(InboundCallbackShape::Optional)) => {}
                    Some(ResolvedProjectionRole::Callback(InboundCallbackShape::Required)) => {
                        return Err(invalid(
                            "component",
                            linked.definition,
                            format!(
                                "direct component local ordinal {local_ordinal} field `{name}` uses literal `None`, but rustc-resolved child definition {target:?} declares a required callback",
                            ),
                        ));
                    }
                    Some(
                        ResolvedProjectionRole::ServerOnly
                        | ResolvedProjectionRole::Value
                        | ResolvedProjectionRole::NodeRef,
                    ) => {
                        binding = ValidatedDirectComponentBinding::ServerOnly;
                    }
                    None => {
                        return Err(invalid(
                            "component",
                            linked.definition,
                            format!(
                                "direct component local ordinal {local_ordinal} field `{name}` uses literal `None`, but the exact field role for rustc-resolved child definition {target:?} is unavailable\nhelp: adapt the complete transport plan so the child field can be resolved before assigning semantic callback behavior",
                            ),
                        ));
                    }
                }
            }
            if let ValidatedDirectComponentBinding::LiveValue { signal, .. } = binding {
                validate_legacy_signal_source(linked.definition, descriptor, signal)?;
            }
            if let ValidatedDirectComponentBinding::NodeRefHandle { node_ref } = binding {
                validate_legacy_node_ref_source(linked.definition, descriptor, node_ref)?;
            }
            if let TransportDirectComponentBinding::DeferredProjection {
                field: parent_field,
                signal,
                node_ref,
            } = transport_binding
            {
                validate_legacy_signal_source(linked.definition, descriptor, signal)?;
                validate_legacy_node_ref_source(linked.definition, descriptor, node_ref)?;
                let inbound =
                    required_object(descriptor, "inbound_inputs", "component", linked.definition)?;
                let exact_projection =
                    required_array(inbound, "projection_fields", "component", linked.definition)?
                        .iter()
                        .filter_map(Value::as_object)
                        .filter(|projection| {
                            required_u64(projection, "field_id", "component", linked.definition)
                                .ok()
                                == Some(u64::from(parent_field))
                                && required_u64(
                                    projection,
                                    "signal_id",
                                    "component",
                                    linked.definition,
                                )
                                .ok()
                                    == Some(u64::from(signal))
                                && required_u64(
                                    projection,
                                    "node_ref_id",
                                    "component",
                                    linked.definition,
                                )
                                .ok()
                                    == Some(u64::from(node_ref))
                        })
                        .count();
                if exact_projection != 1 {
                    return Err(invalid(
                        "component",
                        linked.definition,
                        format!(
                            "deferred direct field `{name}` does not resolve to exactly one authenticated parent projection"
                        ),
                    ));
                }
            }
            if !composed
                && matches!(
                    binding,
                    ValidatedDirectComponentBinding::LiveValue { .. }
                        | ValidatedDirectComponentBinding::LiveCallback { .. }
                        | ValidatedDirectComponentBinding::ForwardedCallback { .. }
                        | ValidatedDirectComponentBinding::NodeRefHandle { .. }
                )
            {
                return Err(invalid(
                    "component",
                    linked.definition,
                    format!(
                        "direct component field `{name}` carries a live binding across an explicit island boundary"
                    ),
                ));
            }
            fields.push(ValidatedDirectComponentField {
                name,
                binding,
                transport_binding,
            });
        }
        if fields.iter().any(|field| {
            matches!(
                field.binding,
                ValidatedDirectComponentBinding::NodeRefHandle { .. }
            )
        }) && (!composed
            || has_rest
            || !matches!(lifecycle, ValidatedDirectComponentLifecycle::StaticSingle))
        {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "direct component edge {local_ordinal} carries a NodeRef handle, but NodeRef handles require one non-rest static composed edge"
                ),
            ));
        }
        let source_anchor = required_object(edge, "source_anchor", "component", linked.definition)?;
        require_known_fields(
            "direct component source anchor",
            source_anchor,
            &["column", "line"],
            linked.definition,
        )?;
        required_u64(source_anchor, "line", "component", linked.definition)?;
        required_u64(source_anchor, "column", "component", linked.definition)?;
        let field_commitments = fields
            .iter()
            .map(|field| TransportDirectComponentField {
                name: &field.name,
                binding: field.transport_binding,
            })
            .collect::<Vec<_>>();
        let expected_source_edge = component_source_edge_id(
            input_fingerprint,
            local_ordinal,
            &TransportDirectComponentEdge {
                boundary: if composed {
                    TransportDirectComponentBoundary::Composed
                } else {
                    TransportDirectComponentBoundary::ExplicitIsland
                },
                site_node,
                lifecycle: lifecycle.transport(),
                has_rest,
                fields: &field_commitments,
            },
        );
        if source_edge != expected_source_edge {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "direct component local ordinal {local_ordinal} has source edge {}, \
                     expected deterministic edge {}",
                    hex_record(&source_edge),
                    hex_record(&expected_source_edge),
                ),
            ));
        }
        validated.push(ValidatedDirectComponentEdge {
            local_ordinal,
            source_edge,
            composed,
            has_rest,
            lifecycle,
            site_node,
            fields,
        });
    }
    Ok(validated)
}

fn validate_direct_component_lifecycle(
    definition: TransportDefinitionId,
    lifecycle: &Map<String, Value>,
    descriptor: &Map<String, Value>,
) -> Result<ValidatedDirectComponentLifecycle, AbiAdapterError> {
    let kind = required_str(lifecycle, "kind", "component", definition)?;
    match kind {
        "static_single" => {
            require_known_fields(
                "static-single direct component lifecycle",
                lifecycle,
                &["kind"],
                definition,
            )?;
            Ok(ValidatedDirectComponentLifecycle::StaticSingle)
        }
        "conditional_single" => {
            require_known_fields(
                "conditional-single direct component lifecycle",
                lifecycle,
                &["branch", "control", "kind", "predicate_expression"],
                definition,
            )?;
            let control = checked_u32(
                required_u64(lifecycle, "control", "component", definition)?,
                "conditional component lifecycle control",
                definition,
            )?;
            let branch = match required_str(lifecycle, "branch", "component", definition)? {
                "then" => ComponentLifecycleBranch::Then,
                "else" => ComponentLifecycleBranch::Else,
                other => {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "conditional component lifecycle branch must be `then` or `else`, found `{other}`"
                        ),
                    ));
                }
            };
            let predicate_expression = checked_u32(
                required_u64(lifecycle, "predicate_expression", "component", definition)?,
                "conditional component lifecycle predicate expression",
                definition,
            )?;
            let conditionals = required_array(
                required_object(descriptor, "bindings", "component", definition)?,
                "conditionals",
                "component",
                definition,
            )?;
            let conditional = conditionals.get(control as usize).ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("conditional component lifecycle claims unknown control {control}"),
                )
            })?;
            let conditional = conditional.as_object().ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!("conditional binding {control} must be an object"),
                )
            })?;
            let graph_predicate = checked_u32(
                required_u64(conditional, "condition", "component", definition)?,
                "conditional binding predicate expression",
                definition,
            )?;
            if predicate_expression != graph_predicate {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "conditional component lifecycle control {control} claims predicate expression \
                         {predicate_expression}, but the analyzed conditional graph names {graph_predicate}"
                    ),
                ));
            }
            // Serialization and hashing live in rsjs_transport, so this is
            // byte-identical to the macro-side commitment. `descriptor` is
            // outside the transport/source-edge envelope, keeping it acyclic.
            let predicate_graph = component_lifecycle_predicate_graph_commitment(
                descriptor,
                control,
                predicate_expression,
            );
            Ok(ValidatedDirectComponentLifecycle::ConditionalSingle {
                control,
                predicate_expression,
                predicate_graph,
                branch,
            })
        }
        "repeated" => {
            require_known_fields(
                "repeated direct component lifecycle",
                lifecycle,
                &["kind"],
                definition,
            )?;
            Ok(ValidatedDirectComponentLifecycle::Repeated)
        }
        "unknown" => {
            require_known_fields(
                "unknown direct component lifecycle",
                lifecycle,
                &["kind", "reason"],
                definition,
            )?;
            require_exact_str(
                lifecycle,
                "reason",
                "nested_conditionals",
                "component",
                definition,
            )?;
            Ok(ValidatedDirectComponentLifecycle::UnknownNestedConditionals)
        }
        other => Err(invalid(
            "component",
            definition,
            format!("unsupported direct component lifecycle kind `{other}`"),
        )),
    }
}

fn validate_legacy_signal_source(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    signal: u32,
) -> Result<(), AbiAdapterError> {
    let signals = required_array(descriptor, "signals", "component", definition)?;
    let record = signals
        .iter()
        .filter_map(Value::as_object)
        .find(|record| {
            record
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|id| id == u64::from(signal))
        })
        .ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("live component value refers to unknown legacy signal {signal}"),
            )
        })?;
    require_exact_u64(record, "id", signal as u64, "component", definition)
}

fn validate_legacy_node_ref_source(
    definition: TransportDefinitionId,
    descriptor: &Map<String, Value>,
    node_ref: u32,
) -> Result<(), AbiAdapterError> {
    let node_refs = required_array(descriptor, "node_refs", "component", definition)?;
    let record = node_refs
        .iter()
        .filter_map(Value::as_object)
        .find(|record| {
            record
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|id| id == u64::from(node_ref))
        })
        .ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("NodeRef handle refers to unknown legacy node ref {node_ref}"),
            )
        })?;
    require_exact_u64(record, "id", node_ref as u64, "component", definition)
}

fn validate_direct_component_binding_with_projection_roles(
    definition: TransportDefinitionId,
    binding: &Map<String, Value>,
    projection_roles: &ResolvedProjectionRoles,
) -> Result<
    (
        ValidatedDirectComponentBinding,
        TransportDirectComponentBinding,
    ),
    AbiAdapterError,
> {
    let kind = required_str(binding, "kind", "component", definition)?;
    let proof = required_str(binding, "proof", "component", definition)?;
    let validated = match kind {
        "server_only" => {
            require_known_fields(
                "direct server-only component field binding",
                binding,
                &["kind", "proof"],
                definition,
            )?;
            require_exact_binding_proof(
                definition,
                kind,
                proof,
                "visible_syntax_has_no_client_dependency_v1",
            )?;
            Ok(ValidatedDirectComponentBinding::ServerOnly)
        }
        "immutable_static_slot_exact_one" => {
            require_known_fields(
                "direct immutable static-slot component field binding",
                binding,
                &["kind", "proof"],
                definition,
            )?;
            require_exact_binding_proof(
                definition,
                kind,
                proof,
                "direct_fully_static_single_root_html_macro_v1",
            )?;
            Ok(ValidatedDirectComponentBinding::StaticSlotExactOne)
        }
        "static_callback" => {
            require_known_fields(
                "direct static callback component field binding",
                binding,
                &["callback_shape", "kind", "proof", "source_form"],
                definition,
            )?;
            let shape = parse_direct_callback_shape(definition, binding)?;
            let source_form = required_str(binding, "source_form", "component", definition)?;
            let (expected, form) = match (source_form, shape) {
                ("none", ComponentCallbackShape::Optional) => (
                    "literal_none_callback_candidate_v1",
                    TransportDirectStaticCallbackForm::None,
                ),
                ("noop_handler", ComponentCallbackShape::Required) => (
                    "rsjs_noop_handler_v1",
                    TransportDirectStaticCallbackForm::NoopHandler,
                ),
                ("some_noop_handler", ComponentCallbackShape::Optional) => (
                    "some_rsjs_noop_handler_v1",
                    TransportDirectStaticCallbackForm::SomeNoopHandler,
                ),
                _ => {
                    return Err(invalid(
                        "component",
                        definition,
                        format!(
                            "static callback source form `{source_form}` is incompatible with callback shape `{}`",
                            direct_callback_shape_name(shape)
                        ),
                    ));
                }
            };
            require_exact_binding_proof(definition, kind, proof, expected)?;
            Ok(ValidatedDirectComponentBinding::StaticCallback { shape, form })
        }
        "live_value" => {
            require_known_fields(
                "direct live-value component field binding",
                binding,
                &["kind", "parent_source", "proof", "source_form"],
                definition,
            )?;
            let (source_form, source) =
                match required_str(binding, "source_form", "component", definition)? {
                    "direct_signal_read" => (
                        ValidatedLiveValueSourceForm::DirectSignalRead,
                        TransportDirectLiveValueSource::DirectSignalRead,
                    ),
                    "client_alias" => (
                        ValidatedLiveValueSourceForm::ClientAlias,
                        TransportDirectLiveValueSource::ClientAlias,
                    ),
                    "inbound_field" => (
                        ValidatedLiveValueSourceForm::InboundField,
                        TransportDirectLiveValueSource::InboundField,
                    ),
                    other => {
                        return Err(invalid(
                            "component",
                            definition,
                            format!("unsupported live-value source_form `{other}`"),
                        ));
                    }
                };
            let expected = match source_form {
                ValidatedLiveValueSourceForm::DirectSignalRead => "exact_signal_read_v1",
                ValidatedLiveValueSourceForm::ClientAlias => {
                    "client_alias_resolves_to_exact_signal_read_v1"
                }
                ValidatedLiveValueSourceForm::InboundField => {
                    "inbound_live_field_reuses_exact_live_signal_v1"
                }
            };
            require_exact_binding_proof(definition, kind, proof, expected)?;
            let signal =
                parse_direct_parent_source(definition, binding, "legacy_signal", "live-value")?;
            Ok(ValidatedDirectComponentBinding::LiveValue { signal, source })
        }
        "live_callback" => {
            require_known_fields(
                "direct live callback component field binding",
                binding,
                &["callback_shape", "kind", "parent_source", "proof"],
                definition,
            )?;
            let shape = parse_direct_callback_shape(definition, binding)?;
            let expected = match shape {
                ComponentCallbackShape::Required => "payload_handler_prelude_binding_v1",
                ComponentCallbackShape::Optional => "some_payload_handler_prelude_binding_v1",
            };
            require_exact_binding_proof(definition, kind, proof, expected)?;
            let handler_prelude_local = parse_direct_parent_source(
                definition,
                binding,
                "payload_handler_prelude_binding",
                "live-callback",
            )?;
            Ok(ValidatedDirectComponentBinding::LiveCallback {
                handler_prelude_local,
                shape,
                form: match shape {
                    ComponentCallbackShape::Required => TransportDirectLiveCallbackForm::Handler,
                    ComponentCallbackShape::Optional => {
                        TransportDirectLiveCallbackForm::SomeHandler
                    }
                },
            })
        }
        "node_ref_handle" => {
            require_known_fields(
                "direct NodeRef handle component field binding",
                binding,
                &["handle_kind", "kind", "parent_source", "proof"],
                definition,
            )?;
            require_exact_str(binding, "handle_kind", "node_ref", "component", definition)?;
            require_exact_binding_proof(definition, kind, proof, "exact_node_ref_handle_alias_v1")?;
            let node_ref = parse_direct_parent_source(
                definition,
                binding,
                "legacy_node_ref",
                "NodeRef-handle",
            )?;
            Ok(ValidatedDirectComponentBinding::NodeRefHandle { node_ref })
        }
        "deferred_projection" => {
            require_known_fields(
                "deferred direct component field projection",
                binding,
                &[
                    "kind",
                    "latent_node_ref_id",
                    "latent_signal_id",
                    "parent_field_id",
                    "proof",
                ],
                definition,
            )?;
            require_exact_binding_proof(
                definition,
                kind,
                proof,
                "rustc_linked_child_field_resolves_exact_projection_role_v1",
            )?;
            let parent_field = checked_u32(
                required_u64(binding, "parent_field_id", "component", definition)?,
                "deferred parent field ID",
                definition,
            )?;
            let signal = checked_u32(
                required_u64(binding, "latent_signal_id", "component", definition)?,
                "deferred latent signal ID",
                definition,
            )?;
            let node_ref = checked_u32(
                required_u64(binding, "latent_node_ref_id", "component", definition)?,
                "deferred latent NodeRef ID",
                definition,
            )?;
            let resolved = projection_roles.field(definition, parent_field).ok_or_else(|| {
                invalid(
                    "component",
                    definition,
                    format!(
                        "deferred projection field {parent_field} requires complete transport-plan adaptation"
                    ),
                )
            })?;
            let concrete = match resolved {
                ResolvedProjectionRole::ServerOnly => ValidatedDirectComponentBinding::ServerOnly,
                ResolvedProjectionRole::Value => ValidatedDirectComponentBinding::LiveValue {
                    signal,
                    source: TransportDirectLiveValueSource::InboundField,
                },
                ResolvedProjectionRole::Callback(shape) => {
                    ValidatedDirectComponentBinding::ForwardedCallback {
                        parent_field,
                        shape: match shape {
                            InboundCallbackShape::Required => ComponentCallbackShape::Required,
                            InboundCallbackShape::Optional => ComponentCallbackShape::Optional,
                        },
                    }
                }
                ResolvedProjectionRole::NodeRef => {
                    ValidatedDirectComponentBinding::NodeRefHandle { node_ref }
                }
            };
            return Ok((
                concrete,
                TransportDirectComponentBinding::DeferredProjection {
                    field: parent_field,
                    signal,
                    node_ref,
                },
            ));
        }
        "unsupported" => {
            require_known_fields(
                "unsupported direct component field binding",
                binding,
                &["kind", "proof"],
                definition,
            )?;
            require_exact_binding_proof(definition, kind, proof, "not_proven")?;
            Ok(ValidatedDirectComponentBinding::Unsupported)
        }
        _ => Err(invalid(
            "component",
            definition,
            format!("unsupported direct component field binding kind `{kind}`"),
        )),
    }?;
    let transport = validated.transport();
    Ok((validated, transport))
}

#[cfg(test)]
fn validate_direct_component_binding(
    definition: TransportDefinitionId,
    binding: &Map<String, Value>,
) -> Result<ValidatedDirectComponentBinding, AbiAdapterError> {
    validate_direct_component_binding_with_projection_roles(
        definition,
        binding,
        &ResolvedProjectionRoles::default(),
    )
    .map(|(binding, _)| binding)
}

fn require_exact_binding_proof(
    definition: TransportDefinitionId,
    kind: &str,
    actual: &str,
    expected: &str,
) -> Result<(), AbiAdapterError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid(
            "component",
            definition,
            format!(
                "direct component field binding `{kind}` requires proof `{expected}`, found `{actual}`"
            ),
        ))
    }
}

fn parse_direct_callback_shape(
    definition: TransportDefinitionId,
    binding: &Map<String, Value>,
) -> Result<ComponentCallbackShape, AbiAdapterError> {
    match required_str(binding, "callback_shape", "component", definition)? {
        "required" => Ok(ComponentCallbackShape::Required),
        "optional" => Ok(ComponentCallbackShape::Optional),
        other => Err(invalid(
            "component",
            definition,
            format!("unsupported direct callback shape `{other}`"),
        )),
    }
}

fn direct_callback_shape_name(shape: ComponentCallbackShape) -> &'static str {
    match shape {
        ComponentCallbackShape::Required => "required",
        ComponentCallbackShape::Optional => "optional",
    }
}

fn parse_direct_parent_source(
    definition: TransportDefinitionId,
    binding: &Map<String, Value>,
    expected_namespace: &str,
    binding_kind: &str,
) -> Result<u32, AbiAdapterError> {
    let source = required_object(binding, "parent_source", "component", definition)?;
    require_known_fields(
        "direct component parent source",
        source,
        &["id", "namespace"],
        definition,
    )?;
    require_exact_str(
        source,
        "namespace",
        expected_namespace,
        "component",
        definition,
    )?;
    u32::try_from(required_u64(source, "id", "component", definition)?).map_err(|_| {
        invalid(
            "component",
            definition,
            format!("{binding_kind} parent source ID exceeds u32"),
        )
    })
}

#[derive(Clone, Debug)]
struct ActiveSharedStateStore {
    store_id: u32,
    type_key: String,
    schema_abi: SemanticHash,
}

#[derive(Clone, Debug, Default)]
struct ActiveSharedStatePlan {
    stores: Vec<ActiveSharedStateStore>,
    operation_shapes_supported: bool,
}

#[derive(Clone, Debug)]
struct DecodedSharedStateDefinition {
    type_key: String,
    schema_abi: SemanticHash,
    fields: BTreeSet<String>,
}

fn resolve_shared_state_definition(
    context: &AbiAdapterContext,
    component: TransportDefinitionId,
    target: TransportDefinitionId,
    usage: &str,
) -> Result<DecodedSharedStateDefinition, AbiAdapterError> {
    let raw = context
        .shared_state_definitions
        .get(&target)
        .ok_or_else(|| {
            invalid(
                "component",
                component,
                format!("{usage} resolves to missing definition {target:?}"),
            )
        })?;
    let expected_schema_hash = shared_state_schema_abi_hash(&raw.schema_abi);
    if raw.schema_abi_hash != expected_schema_hash {
        return Err(invalid(
            "component",
            component,
            format!(
                "SharedState definition {target:?} has schema ABI hash {}, but its canonical bytes hash to {}",
                hex_record(&raw.schema_abi_hash),
                hex_record(&expected_schema_hash),
            ),
        ));
    }
    let schema = decode_shared_state_schema_abi(&raw.schema_abi).map_err(|error| {
        invalid(
            "component",
            component,
            format!(
                "SharedState definition {target:?} has an invalid canonical schema ABI: {error}"
            ),
        )
    })?;
    if schema.runtime_abi_version != SHARED_STATE_RUNTIME_ABI_VERSION {
        return Err(invalid(
            "component",
            component,
            format!(
                "SharedState definition {target:?} uses runtime ABI {}, but this compiler supports {}",
                schema.runtime_abi_version, SHARED_STATE_RUNTIME_ABI_VERSION,
            ),
        ));
    }
    match schema.resolution {
        SharedStateSchemaAbiResolution::Resolved => {}
        SharedStateSchemaAbiResolution::Unresolved(
            SharedStateSchemaUnresolvedReason::NestedClientProps,
        ) => {
            return Err(invalid(
                "component",
                component,
                format!(
                    "SharedState definition {target:?} is unresolved because a field contains nested ClientProps"
                ),
            ));
        }
    }
    Ok(DecodedSharedStateDefinition {
        type_key: raw.type_key.clone(),
        schema_abi: SemanticHash::from_bytes(raw.schema_abi_hash),
        fields: schema.fields.into_iter().map(|field| field.name).collect(),
    })
}

impl ActiveSharedStatePlan {
    fn is_active(&self) -> bool {
        !self.stores.is_empty()
    }
}

fn resolve_active_shared_state_plan(
    context: &AbiAdapterContext,
    definition: TransportDefinitionId,
    graph: &LegacyClientGraph,
    acquisitions: &BTreeMap<u32, TransportDefinitionId>,
) -> Result<ActiveSharedStatePlan, AbiAdapterError> {
    let mut decoded_definitions = BTreeMap::new();
    let mut active_stores = BTreeMap::new();
    let signals = graph
        .signals
        .iter()
        .map(|signal| (signal.id, signal))
        .collect::<BTreeMap<_, _>>();

    for signal in &graph.signals {
        let Some(field) = &signal.shared_field else {
            continue;
        };
        let target = acquisitions.get(&field.store).copied().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!(
                    "active SharedState signal {} refers to store {}, but no exact acquisition target exists",
                    signal.id, field.store,
                ),
            )
        })?;
        let decoded = if let Some(decoded) = decoded_definitions.get(&target) {
            decoded
        } else {
            let decoded = resolve_shared_state_definition(
                context,
                definition,
                target,
                &format!("active SharedState store {}", field.store),
            )?;
            decoded_definitions.insert(target, decoded);
            decoded_definitions
                .get(&target)
                .expect("the decoded SharedState definition was just inserted")
        };
        if !decoded.fields.contains(&field.field) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "SharedState signal {} refers to field `{}` on store {}, but definition {target:?} does not contain that field",
                    signal.id, field.field, field.store,
                ),
            ));
        }
        active_stores
            .entry(field.store)
            .or_insert_with(|| ActiveSharedStateStore {
                store_id: field.store,
                type_key: decoded.type_key.clone(),
                schema_abi: decoded.schema_abi,
            });
    }

    let mut operation_shapes_supported = true;
    for expression in &graph.expressions {
        operation_shapes_supported &= validate_shared_state_operation_shapes(
            definition,
            &expression.client_operations,
            &signals,
        )?;
    }
    let bound_handlers = graph
        .bindings
        .events
        .iter()
        .map(|binding| binding.handler)
        .collect::<BTreeSet<_>>();
    for handler in &graph.handlers {
        if bound_handlers.contains(&handler.id) {
            operation_shapes_supported &= validate_shared_state_operation_shapes(
                definition,
                &handler.client_operations,
                &signals,
            )?;
        }
    }

    Ok(ActiveSharedStatePlan {
        stores: active_stores.into_values().collect(),
        operation_shapes_supported,
    })
}

fn validate_shared_state_operation_shapes(
    definition: TransportDefinitionId,
    operations: &[LegacyClientOperation],
    signals: &BTreeMap<u32, &LegacySignal>,
) -> Result<bool, AbiAdapterError> {
    let mut supported = true;
    for (index, operation) in operations.iter().enumerate() {
        let (signal_id, field) = match &operation.kind {
            LegacyClientOperationKind::SharedRead { signal, field }
            | LegacyClientOperationKind::SharedSet { signal, field }
            | LegacyClientOperationKind::SharedUpdate { signal, field } => (*signal, field),
            _ => continue,
        };
        let signal = signals.get(&signal_id).copied().ok_or_else(|| {
            invalid(
                "component",
                definition,
                format!("SharedState operation refers to missing signal {signal_id}"),
            )
        })?;
        if signal.shared_field.as_ref() != Some(field) {
            return Err(invalid(
                "component",
                definition,
                format!(
                    "SharedState operation for signal {signal_id} claims store {} field `{}`, but the signal has {:?}",
                    field.store, field.field, signal.shared_field,
                ),
            ));
        }
        if let LegacyClientOperationConsumer::Operation(parent) = operation.consumer {
            let parent = usize::try_from(parent).map_err(|_| {
                invalid(
                    "component",
                    definition,
                    "SharedState operation parent exceeds the host index space",
                )
            })?;
            if parent >= index {
                return Err(invalid(
                    "component",
                    definition,
                    format!(
                        "SharedState operation {index} refers to non-enclosing operation {parent}"
                    ),
                ));
            }
            supported &= matches!(
                operations[parent].kind,
                LegacyClientOperationKind::SharedRead { .. }
                    | LegacyClientOperationKind::SharedSet { .. }
                    | LegacyClientOperationKind::SharedUpdate { .. }
            );
        }
    }
    Ok(supported)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValidatedComponentHelperEdge {
    local_ordinal: u32,
    source_edge: TransportRecordId,
    target: TransportDefinitionId,
}

fn validate_component_shared_state_acquisition_set(
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    descriptor: &Map<String, Value>,
) -> Result<BTreeMap<u32, TransportDefinitionId>, AbiAdapterError> {
    let input_fingerprint = required_str(
        transport,
        "component_input_fingerprint",
        "component",
        linked.definition,
    )?;
    let claims = required_array(
        transport,
        "shared_state_acquisitions",
        "component",
        linked.definition,
    )?;
    let stores = required_array(descriptor, "shared_stores", "component", linked.definition)?;
    if claims.len() != stores.len() || claims.len() != linked.shared_state_acquisitions.len() {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "SharedState acquisition ownership must be one-to-one with descriptor stores; \
                 found {} payload claims, {} linked records, and {} descriptor stores",
                claims.len(),
                linked.shared_state_acquisitions.len(),
                stores.len()
            ),
        ));
    }

    let mut seen_edges = BTreeSet::new();
    let mut acquisitions = BTreeMap::new();
    for (index, ((claim, store), linked_acquisition)) in claims
        .iter()
        .zip(stores)
        .zip(&linked.shared_state_acquisitions)
        .enumerate()
    {
        let claim = claim.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("shared_state_acquisitions[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "component SharedState acquisition",
            claim,
            &["source_edge_id", "store_id"],
            linked.definition,
        )?;
        require_exact_u64(
            claim,
            "store_id",
            index as u64,
            "component",
            linked.definition,
        )?;
        let store = store.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("shared_stores[{index}] must be an object"),
            )
        })?;
        require_exact_u64(store, "id", index as u64, "component", linked.definition)?;
        let source_edge =
            parse_record_field(claim, "source_edge_id", "component", linked.definition)?;
        let store_id = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                "SharedState acquisition count exceeds the canonical u32 store-ID space",
            )
        })?;
        let expected_source_edge =
            shared_state_acquisition_source_edge_id(input_fingerprint, store_id);
        if source_edge != expected_source_edge {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState store {store_id} has source edge {}, expected deterministic edge {}",
                    hex_record(&source_edge),
                    hex_record(&expected_source_edge),
                ),
            ));
        }
        if !seen_edges.insert(source_edge) {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState acquisition source edge {} appears more than once",
                    hex_record(&source_edge)
                ),
            ));
        }
        if linked_acquisition.store_id != store_id || linked_acquisition.source_edge != source_edge
        {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState store {index} ownership disagrees with the relocation-linked \
                     acquisition"
                ),
            ));
        }
        acquisitions.insert(store_id, linked_acquisition.target);
    }

    Ok(acquisitions)
}

#[derive(Clone, Debug)]
struct ValidatedSharedStateProviderSite {
    provider_site_id: u32,
    child_edge_local_ordinal: u32,
    type_key: String,
    schema_abi: SemanticHash,
}

fn validate_component_shared_state_provider_site_set(
    context: &AbiAdapterContext,
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    descriptor: &Map<String, Value>,
    input_fingerprint: &str,
    direct_edges: &[ValidatedDirectComponentEdge],
) -> Result<Vec<ValidatedSharedStateProviderSite>, AbiAdapterError> {
    let claims = required_array(
        transport,
        "shared_state_provider_sites",
        "component",
        linked.definition,
    )?;
    let descriptor_sites = required_array(
        descriptor,
        "shared_state_provider_sites",
        "component",
        linked.definition,
    )?;
    if claims.len() != descriptor_sites.len()
        || claims.len() != linked.shared_state_provider_sites.len()
    {
        return Err(invalid(
            "component",
            linked.definition,
            format!(
                "SharedState provider topology must be one-to-one; found {} transport claims, {} \
                 linked records, and {} descriptor sites",
                claims.len(),
                linked.shared_state_provider_sites.len(),
                descriptor_sites.len(),
            ),
        ));
    }

    let mut claimed_children = BTreeSet::new();
    let mut validated = Vec::with_capacity(claims.len());
    for (index, ((claim, descriptor_site), linked_site)) in claims
        .iter()
        .zip(descriptor_sites)
        .zip(&linked.shared_state_provider_sites)
        .enumerate()
    {
        let provider_site_id = u32::try_from(index).map_err(|_| {
            invalid(
                "component",
                linked.definition,
                "SharedState provider-site count exceeds the canonical u32 site-ID space",
            )
        })?;
        let claim = claim.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("shared_state_provider_sites[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "component SharedState provider site",
            claim,
            &[
                "child_edge_local_ordinal",
                "child_source_edge_id",
                "identity_authority",
                "provider_site_id",
                "site_node",
                "source_anchor",
                "source_edge_id",
                "state_rust_type",
                "topology",
            ],
            linked.definition,
        )?;
        require_exact_u64(
            claim,
            "provider_site_id",
            u64::from(provider_site_id),
            "component",
            linked.definition,
        )?;
        require_exact_str(
            claim,
            "identity_authority",
            "rustc_shared_state_witness_relocation",
            "component",
            linked.definition,
        )?;
        require_exact_str(
            claim,
            "topology",
            "static_single_wrapperless",
            "component",
            linked.definition,
        )?;
        let child_edge_local_ordinal = checked_u32(
            required_u64(
                claim,
                "child_edge_local_ordinal",
                "component",
                linked.definition,
            )?,
            "SharedState provider child edge",
            linked.definition,
        )?;
        if !claimed_children.insert(child_edge_local_ordinal) {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "direct child edge {child_edge_local_ordinal} is claimed by more than one \
                     SharedState provider"
                ),
            ));
        }
        let site_node = checked_u32(
            required_u64(claim, "site_node", "component", linked.definition)?,
            "SharedState provider server node",
            linked.definition,
        )?;
        let state_rust_type =
            required_str(claim, "state_rust_type", "component", linked.definition)?;

        let source_anchor =
            required_object(claim, "source_anchor", "component", linked.definition)?;
        require_known_fields(
            "SharedState provider transport source anchor",
            source_anchor,
            &["column", "line"],
            linked.definition,
        )?;
        let source_line = required_u64(source_anchor, "line", "component", linked.definition)?;
        let source_column = required_u64(source_anchor, "column", "component", linked.definition)?;

        let descriptor_site = descriptor_site.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("descriptor shared_state_provider_sites[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "descriptor SharedState provider site",
            descriptor_site,
            &[
                "child_edge",
                "id",
                "site_node",
                "source_anchor",
                "state_rust_type",
                "topology",
            ],
            linked.definition,
        )?;
        require_exact_u64(
            descriptor_site,
            "id",
            u64::from(provider_site_id),
            "component",
            linked.definition,
        )?;
        require_exact_u64(
            descriptor_site,
            "child_edge",
            u64::from(child_edge_local_ordinal),
            "component",
            linked.definition,
        )?;
        require_exact_u64(
            descriptor_site,
            "site_node",
            u64::from(site_node),
            "component",
            linked.definition,
        )?;
        require_exact_str(
            descriptor_site,
            "state_rust_type",
            state_rust_type,
            "component",
            linked.definition,
        )?;
        require_exact_str(
            descriptor_site,
            "topology",
            "static_single_wrapperless",
            "component",
            linked.definition,
        )?;
        let descriptor_anchor = required_object(
            descriptor_site,
            "source_anchor",
            "component",
            linked.definition,
        )?;
        require_known_fields(
            "SharedState provider descriptor source anchor",
            descriptor_anchor,
            &["start_column", "start_line"],
            linked.definition,
        )?;
        require_exact_u64(
            descriptor_anchor,
            "start_line",
            source_line,
            "component",
            linked.definition,
        )?;
        require_exact_u64(
            descriptor_anchor,
            "start_column",
            source_column,
            "component",
            linked.definition,
        )?;

        let child = direct_edges
            .get(child_edge_local_ordinal as usize)
            .filter(|edge| edge.local_ordinal == child_edge_local_ordinal)
            .ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "SharedState provider site {provider_site_id} refers to missing direct \
                         child edge {child_edge_local_ordinal}"
                    ),
                )
            })?;
        if !child.composed
            || !matches!(
                child.lifecycle,
                ValidatedDirectComponentLifecycle::StaticSingle
            )
            || child.has_rest
            || child.site_node != site_node
            || child.fields.iter().any(|field| {
                !matches!(
                    field.binding,
                    ValidatedDirectComponentBinding::ServerOnly
                        | ValidatedDirectComponentBinding::StaticCallback { .. }
                )
            })
        {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState provider site {provider_site_id} must own one wrapperless static \
                     composed child edge at server node {site_node} with only \
                     server/static-callback fields"
                ),
            ));
        }

        let child_source_edge = parse_record_field(
            claim,
            "child_source_edge_id",
            "component",
            linked.definition,
        )?;
        if child_source_edge != child.source_edge {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState provider site {provider_site_id} claims child source edge {}, \
                     but direct child {child_edge_local_ordinal} has {}",
                    hex_record(&child_source_edge),
                    hex_record(&child.source_edge),
                ),
            ));
        }
        let child_target = linked
            .component_edges
            .iter()
            .find(|edge| edge.source_edge == child_source_edge)
            .map(|edge| edge.target)
            .ok_or_else(|| {
                invalid(
                    "component",
                    linked.definition,
                    format!(
                        "SharedState provider site {provider_site_id} child edge {} is absent from \
                         the relocation-linked component set",
                        hex_record(&child_source_edge),
                    ),
                )
            })?;
        let source_edge =
            parse_record_field(claim, "source_edge_id", "component", linked.definition)?;
        let expected_source_edge = shared_state_provider_source_edge_id(
            input_fingerprint,
            provider_site_id,
            &TransportSharedStateProviderSite {
                site_node,
                child_edge_ordinal: child_edge_local_ordinal,
                child_source_edge_id: child_source_edge,
            },
        );
        if source_edge != expected_source_edge {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState provider site {provider_site_id} has source edge {}, expected \
                     deterministic edge {}",
                    hex_record(&source_edge),
                    hex_record(&expected_source_edge),
                ),
            ));
        }
        if linked_site.provider_site_id != provider_site_id
            || linked_site.source_edge != source_edge
            || linked_site.child_edge_local_ordinal != child_edge_local_ordinal
            || linked_site.child_source_edge != child_source_edge
            || linked_site.child_target != child_target
            || linked_site.site_node != site_node
        {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "SharedState provider site {provider_site_id} disagrees with its \
                     relocation-authenticated linked topology"
                ),
            ));
        }

        let state = resolve_shared_state_definition(
            context,
            linked.definition,
            linked_site.target,
            &format!("SharedState provider site {provider_site_id}"),
        )?;
        validated.push(ValidatedSharedStateProviderSite {
            provider_site_id,
            child_edge_local_ordinal,
            type_key: state.type_key,
            schema_abi: state.schema_abi,
        });
    }
    Ok(validated)
}

fn validate_component_helper_edge_set(
    linked: &LinkedTransportComponent,
    transport: &Map<String, Value>,
    input_fingerprint: &str,
) -> Result<Vec<ValidatedComponentHelperEdge>, AbiAdapterError> {
    let payload_edges = required_array(
        transport,
        "reachable_helper_edges",
        "component",
        linked.definition,
    )?;
    let mut payload_ids = Vec::with_capacity(payload_edges.len());
    let mut edges_by_ordinal = BTreeMap::new();
    for (index, edge) in payload_edges.iter().enumerate() {
        let edge = edge.as_object().ok_or_else(|| {
            invalid(
                "component",
                linked.definition,
                format!("reachable_helper_edges[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "component helper source edge",
            edge,
            &[
                "authored_path",
                "identity_authority",
                "local_ordinal",
                "source_edge_id",
            ],
            linked.definition,
        )?;
        require_exact_str(
            edge,
            "identity_authority",
            "rustc_function_relocation",
            "component",
            linked.definition,
        )?;
        required_str(edge, "authored_path", "component", linked.definition)?;
        let local_ordinal = u32::try_from(required_u64(
            edge,
            "local_ordinal",
            "component",
            linked.definition,
        )?)
        .map_err(|_| {
            invalid(
                "component",
                linked.definition,
                format!("reachable_helper_edges[{index}].local_ordinal exceeds u32"),
            )
        })?;
        let source_edge =
            parse_record_field(edge, "source_edge_id", "component", linked.definition)?;
        let expected_source_edge =
            component_helper_source_edge_id(input_fingerprint, local_ordinal);
        if source_edge != expected_source_edge {
            return Err(invalid(
                "component",
                linked.definition,
                format!(
                    "reachable helper local ordinal {local_ordinal} has source edge {}, \
                     expected deterministic edge {}",
                    hex_record(&source_edge),
                    hex_record(&expected_source_edge),
                ),
            ));
        }
        payload_ids.push(source_edge);
        if edges_by_ordinal
            .insert(local_ordinal, source_edge)
            .is_some()
        {
            return Err(invalid(
                "component",
                linked.definition,
                format!("reachable helper local ordinal {local_ordinal} appears more than once"),
            ));
        }
    }
    compare_helper_edges(
        linked.definition,
        "component helper",
        &payload_ids,
        &linked.helper_edges,
    )?;

    edges_by_ordinal
        .into_iter()
        .map(|(local_ordinal, source_edge)| {
            let linked_edge = linked
                .helper_edges
                .iter()
                .find(|edge| edge.source_edge == source_edge)
                .ok_or_else(|| {
                    invalid(
                        "component",
                        linked.definition,
                        format!(
                            "reachable helper local ordinal {local_ordinal} has no \
                             relocation-resolved source edge"
                        ),
                    )
                })?;
            Ok(ValidatedComponentHelperEdge {
                local_ordinal,
                source_edge,
                target: linked_edge.target,
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidatedStandaloneHelperEdge {
    ordinal: u32,
    authored_path: String,
    source_edge: TransportRecordId,
    target: TransportDefinitionId,
}

fn validate_helper_edge_set(
    linked: &LinkedTransportHelper,
    root: &Map<String, Value>,
) -> Result<Vec<ValidatedStandaloneHelperEdge>, AbiAdapterError> {
    let payload_edges = required_array(root, "direct_calls", "helper", linked.definition)?;
    let mut payload_ids = Vec::with_capacity(payload_edges.len());
    let mut edges_by_ordinal = BTreeMap::new();
    for (index, edge) in payload_edges.iter().enumerate() {
        let edge = edge.as_object().ok_or_else(|| {
            invalid(
                "helper",
                linked.definition,
                format!("direct_calls[{index}] must be an object"),
            )
        })?;
        require_known_fields(
            "helper direct call",
            edge,
            &["authored_path", "ordinal", "source_edge_id"],
            linked.definition,
        )?;
        let authored_path =
            required_str(edge, "authored_path", "helper", linked.definition)?.to_owned();
        if authored_path.is_empty() {
            return Err(invalid(
                "helper",
                linked.definition,
                format!("direct_calls[{index}].authored_path cannot be empty"),
            ));
        }
        let ordinal = u32::try_from(required_u64(edge, "ordinal", "helper", linked.definition)?)
            .map_err(|_| {
                invalid(
                    "helper",
                    linked.definition,
                    format!("direct_calls[{index}].ordinal exceeds u32"),
                )
            })?;
        let source_edge = parse_record_field(edge, "source_edge_id", "helper", linked.definition)?;
        let expected_source_edge =
            helper_direct_call_source_edge_id(linked.definition.record, ordinal);
        if source_edge != expected_source_edge {
            return Err(invalid(
                "helper",
                linked.definition,
                format!(
                    "direct helper call ordinal {ordinal} has source edge {}, \
                     expected deterministic edge {}",
                    hex_record(&source_edge),
                    hex_record(&expected_source_edge),
                ),
            ));
        }
        payload_ids.push(source_edge);
        if edges_by_ordinal
            .insert(ordinal, (authored_path, source_edge))
            .is_some()
        {
            return Err(invalid(
                "helper",
                linked.definition,
                format!("direct helper call ordinal {ordinal} appears more than once"),
            ));
        }
    }
    compare_helper_edges(
        linked.definition,
        "helper",
        &payload_ids,
        &linked.helper_edges,
    )?;

    edges_by_ordinal
        .into_iter()
        .enumerate()
        .map(|(expected, (ordinal, (authored_path, source_edge)))| {
            let expected = u32::try_from(expected).map_err(|_| {
                invalid(
                    "helper",
                    linked.definition,
                    "direct helper call count exceeds the canonical u32 ID space",
                )
            })?;
            if ordinal != expected {
                return Err(invalid(
                    "helper",
                    linked.definition,
                    format!(
                        "direct helper call ordinals must be dense from zero; \
                         expected ordinal {expected}, found {ordinal}"
                    ),
                ));
            }
            let linked_edge = linked
                .helper_edges
                .iter()
                .find(|edge| edge.source_edge == source_edge)
                .ok_or_else(|| {
                    invalid(
                        "helper",
                        linked.definition,
                        format!(
                            "direct helper call ordinal {ordinal} has no \
                             relocation-resolved source edge"
                        ),
                    )
                })?;
            Ok(ValidatedStandaloneHelperEdge {
                ordinal,
                authored_path,
                source_edge,
                target: linked_edge.target,
            })
        })
        .collect()
}

fn parse_standalone_helper_body(
    definition: TransportDefinitionId,
    body_ir: &Value,
    options: &ClientJsonParseOptions<'_>,
) -> Result<HelperCodegenBody, AbiAdapterError> {
    let parsed = if body_ir.get("kind").and_then(Value::as_str) == Some("block") {
        HelperCodegenBody::Block(
            parse_client_block_json_with_options(body_ir, options).map_err(|error| {
                invalid(
                    "helper",
                    definition,
                    format!("standalone helper has invalid Client IR v3 block: {error}"),
                )
            })?,
        )
    } else {
        HelperCodegenBody::Expr(
            parse_client_expr_json_with_options(body_ir, options).map_err(|error| {
                invalid(
                    "helper",
                    definition,
                    format!("standalone helper has invalid Client IR v3 expression: {error}"),
                )
            })?,
        )
    };
    Ok(parsed)
}

fn transported_helper_paths(body: &HelperCodegenBody) -> Vec<String> {
    let mut paths = Vec::new();
    let mut visit = |expression: &ClientExpr| {
        if let ClientExpr::Call {
            function: ClientFunctionRef::Unresolved(path),
            ..
        } = expression
        {
            paths.push(path.clone());
        }
    };
    match body {
        HelperCodegenBody::Expr(expression) => walk_client_expr(expression, &mut visit),
        HelperCodegenBody::Block(block) => walk_client_block(block, &mut visit),
    }
    paths
}

fn validate_standalone_helper_client_ir(
    definition: TransportDefinitionId,
    body_ir: &Value,
    direct_edges: &[ValidatedStandaloneHelperEdge],
) -> Result<(Value, HelperCodegenBody), AbiAdapterError> {
    let transported =
        parse_standalone_helper_body(definition, body_ir, &ClientJsonParseOptions::new())?;
    let normalized = transported.canonical_json();
    if normalized != *body_ir {
        return Err(invalid(
            "helper",
            definition,
            "standalone helper body is not canonical Client IR v3 JSON or contains unknown \
             semantic fields",
        ));
    }

    // Only relocation joining is transport-specific. The shared Client IR
    // validator rechecks the complete pure subset and lexical-local contract
    // after every unresolved call has been structurally resolved below.
    let transported_paths = transported_helper_paths(&transported);
    if transported_paths.len() != direct_edges.len() {
        return Err(invalid(
            "helper",
            definition,
            format!(
                "standalone helper Client IR contains {} direct helper call(s), but the \
                 relocation payload contains {}",
                transported_paths.len(),
                direct_edges.len(),
            ),
        ));
    }
    for (index, (actual, edge)) in transported_paths.iter().zip(direct_edges).enumerate() {
        if actual != &edge.authored_path {
            return Err(invalid(
                "helper",
                definition,
                format!(
                    "standalone helper direct call ordinal {index} is `{actual}` in Client IR, \
                     but the relocation payload names `{}`",
                    edge.authored_path,
                ),
            ));
        }
    }

    let next_ordinal = std::cell::Cell::new(0_usize);
    let resolution_error = std::cell::RefCell::new(None::<String>);
    let resolve = |path: &str| {
        let ordinal = next_ordinal.get();
        let Some(next) = ordinal.checked_add(1) else {
            *resolution_error.borrow_mut() =
                Some("direct helper call count exceeds usize".to_owned());
            return None;
        };
        next_ordinal.set(next);
        let Some(edge) = direct_edges.get(ordinal) else {
            *resolution_error.borrow_mut() = Some(format!(
                "Client IR contains an unexpected direct helper call `{path}` at ordinal {ordinal}"
            ));
            return None;
        };
        if path != edge.authored_path {
            *resolution_error.borrow_mut() = Some(format!(
                "Client IR direct helper call ordinal {ordinal} is `{path}`, but the relocation \
                 payload names `{}`",
                edge.authored_path,
            ));
            return None;
        }
        Some(ClientFunctionRef::Helper {
            edge: HelperCallEdgeId::new(edge.ordinal),
            authored_path: path.to_owned(),
            name: path.rsplit("::").next().unwrap_or(path).to_owned(),
        })
    };
    let options = ClientJsonParseOptions::new().with_unresolved_function_ref(&resolve);
    let resolved = parse_standalone_helper_body(definition, body_ir, &options)?;
    if let Some(reason) = resolution_error.into_inner() {
        return Err(invalid("helper", definition, reason));
    }
    if next_ordinal.get() != direct_edges.len() {
        return Err(invalid(
            "helper",
            definition,
            format!(
                "standalone helper Client IR resolved {} direct call(s), but the relocation \
                 payload contains {}",
                next_ordinal.get(),
                direct_edges.len(),
            ),
        ));
    }

    Ok((normalized, resolved))
}

fn compare_component_edges(
    definition: TransportDefinitionId,
    kind: &'static str,
    payload: &[TransportRecordId],
    linked: &[LinkedComponentEdge],
) -> Result<(), AbiAdapterError> {
    compare_edge_ids(
        definition,
        kind,
        payload,
        linked.iter().map(|edge| edge.source_edge),
    )
}

fn compare_helper_edges(
    definition: TransportDefinitionId,
    kind: &'static str,
    payload: &[TransportRecordId],
    linked: &[LinkedHelperEdge],
) -> Result<(), AbiAdapterError> {
    compare_edge_ids(
        definition,
        kind,
        payload,
        linked.iter().map(|edge| edge.source_edge),
    )
}

fn compare_edge_ids(
    definition: TransportDefinitionId,
    kind: &'static str,
    payload: &[TransportRecordId],
    linked: impl IntoIterator<Item = TransportRecordId>,
) -> Result<(), AbiAdapterError> {
    let mut payload_set = payload.to_vec();
    payload_set.sort();
    if payload_set.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid(
            kind,
            definition,
            "payload contains a duplicate source edge ID",
        ));
    }
    let mut linked_set = linked.into_iter().collect::<Vec<_>>();
    linked_set.sort();
    if payload_set != linked_set {
        return Err(AbiAdapterError::SourceEdgeSetMismatch {
            kind,
            definition,
            payload: payload_set.iter().map(hex_record).collect(),
            linked: linked_set.iter().map(hex_record).collect(),
        });
    }
    Ok(())
}

fn validate_definition_record(
    kind: &'static str,
    definition: TransportDefinitionId,
    object: &Map<String, Value>,
    field: &str,
) -> Result<(), AbiAdapterError> {
    let payload = parse_record_field(object, field, kind, definition)?;
    if payload != definition.record {
        return Err(invalid(
            kind,
            definition,
            format!(
                "`{field}` is {}, but linked definition record is {}",
                hex_record(&payload),
                hex_record(&definition.record)
            ),
        ));
    }
    Ok(())
}

fn validate_linked_semantic_hash(
    kind: &'static str,
    definition: TransportDefinitionId,
    root: &Map<String, Value>,
    linked: SemanticHash,
) -> Result<(), AbiAdapterError> {
    let payload = parse_sha256(
        required_str(root, "semantic_hash", kind, definition)?,
        kind,
        definition,
        "semantic_hash",
    )?;
    let payload = SemanticHash::from_bytes(payload);
    if payload != linked {
        return Err(AbiAdapterError::SemanticHashMismatch {
            kind,
            definition,
            payload,
            linked,
        });
    }
    Ok(())
}

fn validate_sha256_field(
    object: &Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<(), AbiAdapterError> {
    parse_sha256(
        required_str(object, field, kind, definition)?,
        kind,
        definition,
        field,
    )?;
    Ok(())
}

fn parse_record_field(
    object: &Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<TransportRecordId, AbiAdapterError> {
    parse_hex_32(
        required_str(object, field, kind, definition)?,
        kind,
        definition,
        field,
    )
}

fn parse_sha256(
    value: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
    field: &str,
) -> Result<[u8; 32], AbiAdapterError> {
    let value = value.strip_prefix("sha256:").ok_or_else(|| {
        invalid(
            kind,
            definition,
            format!("`{field}` must start with `sha256:`"),
        )
    })?;
    parse_hex_32(value, kind, definition, field)
}

fn parse_hex_32(
    value: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
    field: &str,
) -> Result<[u8; 32], AbiAdapterError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            kind,
            definition,
            format!("`{field}` must be exactly 64 lower-case hexadecimal characters"),
        ));
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16).map_err(|error| {
            invalid(
                kind,
                definition,
                format!("invalid hexadecimal `{field}`: {error}"),
            )
        })?;
    }
    Ok(output)
}

fn payload_object<'a>(
    kind: &'static str,
    definition: TransportDefinitionId,
    value: &'a Value,
) -> Result<&'a Map<String, Value>, AbiAdapterError> {
    value
        .as_object()
        .ok_or_else(|| invalid(kind, definition, "payload must be a JSON object"))
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<&'a Map<String, Value>, AbiAdapterError> {
    object
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(kind, definition, format!("missing object `{field}`")))
}

fn required_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<&'a Vec<Value>, AbiAdapterError> {
    object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(kind, definition, format!("missing array `{field}`")))
}

fn optional_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<Option<&'a Vec<Value>>, AbiAdapterError> {
    match object.get(field) {
        Some(value) => value
            .as_array()
            .map(Some)
            .ok_or_else(|| invalid(kind, definition, format!("`{field}` must be an array"))),
        None => Ok(None),
    }
}

fn required_str<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<&'a str, AbiAdapterError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(kind, definition, format!("missing string `{field}`")))
}

fn required_u64(
    object: &Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<u64, AbiAdapterError> {
    object.get(field).and_then(Value::as_u64).ok_or_else(|| {
        invalid(
            kind,
            definition,
            format!("missing unsigned integer `{field}`"),
        )
    })
}

fn required_bool(
    object: &Map<String, Value>,
    field: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<bool, AbiAdapterError> {
    object
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid(kind, definition, format!("missing boolean `{field}`")))
}

fn require_exact_str(
    object: &Map<String, Value>,
    field: &str,
    expected: &str,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<(), AbiAdapterError> {
    let actual = required_str(object, field, kind, definition)?;
    if actual == expected {
        Ok(())
    } else {
        Err(invalid(
            kind,
            definition,
            format!("unsupported `{field}` `{actual}`; expected `{expected}`"),
        ))
    }
}

fn require_exact_u64(
    object: &Map<String, Value>,
    field: &str,
    expected: u64,
    kind: &'static str,
    definition: TransportDefinitionId,
) -> Result<(), AbiAdapterError> {
    let actual = required_u64(object, field, kind, definition)?;
    if actual == expected {
        Ok(())
    } else {
        Err(invalid(
            kind,
            definition,
            format!("unsupported `{field}` {actual}; expected {expected}"),
        ))
    }
}

fn require_known_fields(
    label: &str,
    object: &Map<String, Value>,
    allowed: &[&str],
    definition: TransportDefinitionId,
) -> Result<(), AbiAdapterError> {
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(field.as_str()))
    {
        return Err(invalid(
            "descriptor",
            definition,
            format!(
                "{label} contains unknown field `{field}`; refusing to ignore possible semantic data"
            ),
        ));
    }
    Ok(())
}

fn invalid(
    kind: &'static str,
    definition: TransportDefinitionId,
    reason: impl Into<String>,
) -> AbiAdapterError {
    AbiAdapterError::InvalidPayload {
        kind,
        definition,
        reason: reason.into(),
    }
}

fn hex_record(record: &TransportRecordId) -> String {
    use std::fmt::Write as _;

    let mut result = String::with_capacity(64);
    for byte in record {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

/// Hash a legacy JSON value after recursively sorting all object keys.
///
/// JSON canonicalization remains at the transport boundary; the shared ABI
/// crate owns the exact compatibility material and domain separation used by
/// every compiler phase.
pub fn canonical_legacy_json_hash(domain: &str, value: &Value) -> SemanticHash {
    let bytes = canonical_json_bytes(value)
        .expect("a parsed serde_json::Value always serializes canonically");
    rsjs_ir::abi::canonical_legacy_json_bytes_hash(domain, &bytes)
}

#[cfg(test)]
mod tests;
