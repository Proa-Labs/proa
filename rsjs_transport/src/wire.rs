// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Target-independent fixed relocation records and inline payload records.

use sha2::{Digest, Sha256};

use crate::canonical_json_bytes;

/// Magic bytes at the front of every RSJS object transport record.
pub const TRANSPORT_FORMAT_MAGIC: [u8; 8] = *b"RSJSTRN\0";

/// Version of the fixed record layout in this module.
pub const TRANSPORT_FORMAT_VERSION: u16 = 3;

/// Magic bytes at the front of every inline descriptor payload.
pub const PAYLOAD_FORMAT_MAGIC: [u8; 8] = *b"RSJSPAY\0";

/// Version of the inline descriptor payload envelope.
pub const PAYLOAD_FORMAT_VERSION: u16 = 1;

/// Byte width of the fixed prefix before an inline payload.
pub const PAYLOAD_HEADER_SIZE: usize = 128;

/// Magic bytes at the front of the canonical SharedState schema payload.
pub const SHARED_STATE_SCHEMA_FORMAT_MAGIC: [u8; 8] = *b"RSJSSCH\0";

/// Version of the canonical SharedState schema payload envelope.
pub const SHARED_STATE_SCHEMA_FORMAT_VERSION: u16 = 1;

/// Byte width of the fixed SharedState schema payload prefix.
pub const SHARED_STATE_SCHEMA_HEADER_SIZE: usize = 72;

/// Magic bytes at the front of the canonical SharedState ABI bytes carried
/// inside [`SHARED_STATE_SCHEMA_FORMAT_MAGIC`].
///
/// The outer schema payload joins a `type_key` to an opaque byte string. This
/// inner format makes the current producer's byte string independently
/// versioned and lets the semantic linker distinguish a complete closed schema
/// from one that still needs recursive nested-codec witnesses.
pub const SHARED_STATE_CANONICAL_ABI_MAGIC: [u8; 8] = *b"RSJSCAB\0";

/// Version of the canonical SharedState ABI encoding.
pub const SHARED_STATE_CANONICAL_ABI_VERSION: u16 = 1;

/// Byte width of the canonical SharedState ABI header.
pub const SHARED_STATE_CANONICAL_ABI_HEADER_SIZE: usize = 32;

/// Byte width of one resolved SharedState field header.
pub const SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE: usize = 24;

/// Byte width of one recursive codec header.
pub const SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE: usize = 8;

/// Runtime SharedState ABI represented by the v1 canonical schema producer.
///
/// `rsjs::SHARED_STATE_ABI_VERSION` aliases this value so the derive's
/// host-produced schema bytes and target-side runtime metadata cannot drift.
pub const SHARED_STATE_RUNTIME_ABI_VERSION: u32 = 1;

/// Magic bytes at the front of every inline semantic-identity record.
pub const SEMANTIC_IDENTITY_FORMAT_MAGIC: [u8; 8] = *b"RSJSIDN\0";

/// Version of the semantic-identity record and text encoding.
pub const SEMANTIC_IDENTITY_FORMAT_VERSION: u16 = 1;

/// Byte width of the fixed prefix before semantic-identity text.
pub const SEMANTIC_IDENTITY_HEADER_SIZE: usize = 96;

/// Byte width of Cargo unit, definition, field, and source-edge identifiers.
pub const TRANSPORT_ID_BYTES: usize = 32;

/// Placeholder used until the descriptor-pass orchestrator supplies a
/// realized `CargoUnitId`.
///
/// The all-zero value is never a valid assigned unit identity. Extraction
/// preserves it so the prototype cannot accidentally claim orchestrator-level
/// freshness.
pub const UNASSIGNED_CARGO_UNIT_ID: TransportRecordId = [0; TRANSPORT_ID_BYTES];

/// Fixed-size transport identifier.
pub type TransportRecordId = [u8; TRANSPORT_ID_BYTES];

/// Mach-O segment containing descriptor-pass records.
pub const MACHO_DATA_SEGMENT: &str = "__DATA";
/// Mach-O component-definition section.
pub const MACHO_COMPONENT_SECTION: &str = "__rsjs_comp";
/// Mach-O component-edge section.
pub const MACHO_EDGE_SECTION: &str = "__rsjs_edge";
/// Mach-O component-field witness section.
pub const MACHO_COMPONENT_FIELD_SECTION: &str = "__rsjs_field";
/// Mach-O client-helper-definition section.
pub const MACHO_HELPER_SECTION: &str = "__rsjs_help";
/// Mach-O client-helper-edge section.
pub const MACHO_HELPER_EDGE_SECTION: &str = "__rsjs_hedge";
/// Mach-O component-descriptor payload section.
pub const MACHO_COMPONENT_PAYLOAD_SECTION: &str = "__rsjs_cpay";
/// Mach-O standalone-helper payload section.
pub const MACHO_HELPER_PAYLOAD_SECTION: &str = "__rsjs_hpay";
/// Mach-O SharedState-definition section.
pub const MACHO_SHARED_STATE_SECTION: &str = "__rsjs_state";
/// Mach-O component-local SharedState-acquisition section.
pub const MACHO_SHARED_STATE_ACQUISITION_SECTION: &str = "__rsjs_sedge";
/// Mach-O canonical SharedState-schema payload section.
pub const MACHO_SHARED_STATE_PAYLOAD_SECTION: &str = "__rsjs_spay";
/// Mach-O normalized semantic-identity section.
pub const MACHO_SEMANTIC_IDENTITY_SECTION: &str = "__rsjs_ident";
/// ELF component-definition section.
pub const ELF_COMPONENT_SECTION: &str = ".rsjs.component";
/// ELF component-edge section.
pub const ELF_EDGE_SECTION: &str = ".rsjs.edge";
/// ELF component-field witness section.
pub const ELF_COMPONENT_FIELD_SECTION: &str = ".rsjs.component_field";
/// ELF client-helper-definition section.
pub const ELF_HELPER_SECTION: &str = ".rsjs.helper";
/// ELF client-helper-edge section.
pub const ELF_HELPER_EDGE_SECTION: &str = ".rsjs.helper_edge";
/// ELF component-descriptor payload section.
pub const ELF_COMPONENT_PAYLOAD_SECTION: &str = ".rsjs.component_payload";
/// ELF standalone-helper payload section.
pub const ELF_HELPER_PAYLOAD_SECTION: &str = ".rsjs.helper_payload";
/// ELF SharedState-definition section.
pub const ELF_SHARED_STATE_SECTION: &str = ".rsjs.shared_state";
/// ELF component-local SharedState-acquisition section.
pub const ELF_SHARED_STATE_ACQUISITION_SECTION: &str = ".rsjs.shared_acquisition";
/// ELF canonical SharedState-schema payload section.
pub const ELF_SHARED_STATE_PAYLOAD_SECTION: &str = ".rsjs.shared_state_payload";
/// ELF normalized semantic-identity section.
pub const ELF_SEMANTIC_IDENTITY_SECTION: &str = ".rsjs.identity";
/// COFF component-definition section.
pub const COFF_COMPONENT_SECTION: &str = ".rsjs$C";
/// COFF component-edge section.
pub const COFF_EDGE_SECTION: &str = ".rsjs$E";
/// COFF component-field witness section.
pub const COFF_COMPONENT_FIELD_SECTION: &str = ".rsjs$F";
/// COFF client-helper-definition section.
pub const COFF_HELPER_SECTION: &str = ".rsjs$H";
/// COFF client-helper-edge section.
pub const COFF_HELPER_EDGE_SECTION: &str = ".rsjs$I";
/// COFF component-descriptor payload section.
pub const COFF_COMPONENT_PAYLOAD_SECTION: &str = ".rsjs$P";
/// COFF standalone-helper payload section.
pub const COFF_HELPER_PAYLOAD_SECTION: &str = ".rsjs$Q";
/// COFF SharedState-definition section.
pub const COFF_SHARED_STATE_SECTION: &str = ".rsjs$J";
/// COFF component-local SharedState-acquisition section.
pub const COFF_SHARED_STATE_ACQUISITION_SECTION: &str = ".rsjs$K";
/// COFF canonical SharedState-schema payload section.
pub const COFF_SHARED_STATE_PAYLOAD_SECTION: &str = ".rsjs$R";
/// COFF normalized semantic-identity section.
pub const COFF_SEMANTIC_IDENTITY_SECTION: &str = ".rsjs$S";

/// Definition kind carried by one semantic-identity record.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticIdentityKind {
    /// Identity of a `#[rsjs(component...)]` definition.
    Component = 1,
    /// Identity of a standalone `#[rsjs::client]` helper definition.
    Helper = 2,
}

impl SemanticIdentityKind {
    #[cfg(any(feature = "extract", test))]
    pub(crate) fn from_wire(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Component),
            2 => Some(Self::Helper),
            _ => None,
        }
    }
}

/// Fixed prefix for one inline semantic-identity record.
///
/// The bytes after this header use the v1 text encoding
/// `package\0crate\0module_path\0item`. `module_path` is the exact output of
/// `module_path!()` at the defining macro expansion. Keeping the values inline
/// avoids decoding Rust data layout or demangling a target symbol.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct SemanticIdentityHeader {
    /// [`SEMANTIC_IDENTITY_FORMAT_MAGIC`].
    pub magic: [u8; 8],
    /// [`SEMANTIC_IDENTITY_FORMAT_VERSION`], little endian.
    pub format_version_le: [u8; 2],
    /// [`SemanticIdentityKind`] discriminant, little endian.
    pub identity_kind_le: [u8; 2],
    /// Header byte width, little endian.
    pub header_size_le: [u8; 2],
    /// Reserved capability flags; zero in v1.
    pub flags_le: [u8; 2],
    /// Complete unpadded record byte width, little endian.
    pub record_size_le: [u8; 8],
    /// Semantic-identity text byte length, little endian.
    pub identity_size_le: [u8; 8],
    /// Exact descriptor-pass Cargo unit.
    pub cargo_unit_id: TransportRecordId,
    /// Exact component/helper definition record.
    pub definition_record_id: TransportRecordId,
}

impl SemanticIdentityHeader {
    const fn new(kind: SemanticIdentityKind, identity_size: usize) -> Self {
        let record_size = SEMANTIC_IDENTITY_HEADER_SIZE
            .checked_add(identity_size)
            .expect("RSJS semantic identity record size overflow");
        Self {
            magic: SEMANTIC_IDENTITY_FORMAT_MAGIC,
            format_version_le: SEMANTIC_IDENTITY_FORMAT_VERSION.to_le_bytes(),
            identity_kind_le: (kind as u16).to_le_bytes(),
            header_size_le: (SEMANTIC_IDENTITY_HEADER_SIZE as u16).to_le_bytes(),
            flags_le: 0_u16.to_le_bytes(),
            record_size_le: (record_size as u64).to_le_bytes(),
            identity_size_le: (identity_size as u64).to_le_bytes(),
            cargo_unit_id: [0; TRANSPORT_ID_BYTES],
            definition_record_id: [0; TRANSPORT_ID_BYTES],
        }
    }

    /// Decode the semantic-identity format version.
    pub const fn format_version(&self) -> u16 {
        u16::from_le_bytes(self.format_version_le)
    }

    /// Decode the identity-kind discriminant.
    pub const fn identity_kind_raw(&self) -> u16 {
        u16::from_le_bytes(self.identity_kind_le)
    }

    /// Decode the fixed header width.
    pub const fn header_size(&self) -> u16 {
        u16::from_le_bytes(self.header_size_le)
    }

    /// Decode the complete unpadded record width.
    pub const fn record_size(&self) -> u64 {
        u64::from_le_bytes(self.record_size_le)
    }

    /// Decode the inline identity byte length.
    pub const fn identity_size(&self) -> u64 {
        u64::from_le_bytes(self.identity_size_le)
    }
}

/// Inline, relocation-free normalized semantic identity.
///
/// `N` is supplied from the generated `concat!(env!(...), module_path!(), ...)`
/// string. The object extractor reads only the explicit wire header and bytes;
/// it never interprets this Rust struct's native field layout.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct SemanticIdentityRecord<const N: usize> {
    /// Versioned, target-independent header.
    pub header: SemanticIdentityHeader,
    /// V1 NUL-delimited semantic namespace.
    pub identity: [u8; N],
}

impl<const N: usize> SemanticIdentityRecord<N> {
    /// Construct one semantic-identity record.
    pub const fn new(
        kind: SemanticIdentityKind,
        cargo_unit_id: TransportRecordId,
        definition_record_id: TransportRecordId,
        identity: [u8; N],
    ) -> Self {
        assert!(N != 0, "RSJS semantic identity must not be empty");
        let mut header = SemanticIdentityHeader::new(kind, N);
        header.cargo_unit_id = cargo_unit_id;
        header.definition_record_id = definition_record_id;
        Self { header, identity }
    }
}

/// Copy a generated semantic namespace into a fixed-size inline record.
///
/// This is intentionally `const` so `module_path!()` and Cargo's `env!`
/// values remain target object bytes rather than host proc-macro guesses.
pub const fn encode_semantic_identity<const N: usize>(identity: &str) -> [u8; N] {
    let source = identity.as_bytes();
    assert!(
        source.len() == N,
        "RSJS semantic identity length does not match its record"
    );
    let mut encoded = [0_u8; N];
    let mut index = 0;
    while index < N {
        encoded[index] = source[index];
        index += 1;
    }
    encoded
}

/// Encode one semantic-identity record for host-side fixtures and tooling.
///
/// Target code normally emits [`SemanticIdentityRecord`] directly so
/// `module_path!()` remains available at the definition site.
pub fn encode_semantic_identity_record(
    kind: SemanticIdentityKind,
    cargo_unit_id: TransportRecordId,
    definition_record_id: TransportRecordId,
    identity: &str,
) -> Vec<u8> {
    let record_size = SEMANTIC_IDENTITY_HEADER_SIZE
        .checked_add(identity.len())
        .expect("RSJS semantic identity record size overflow");
    let record_size =
        u64::try_from(record_size).expect("RSJS semantic identity does not fit the wire format");
    let identity_size =
        u64::try_from(identity.len()).expect("RSJS semantic identity does not fit the wire format");
    let mut encoded = Vec::with_capacity(record_size as usize);
    encoded.extend_from_slice(&SEMANTIC_IDENTITY_FORMAT_MAGIC);
    encoded.extend_from_slice(&SEMANTIC_IDENTITY_FORMAT_VERSION.to_le_bytes());
    encoded.extend_from_slice(&(kind as u16).to_le_bytes());
    encoded.extend_from_slice(&(SEMANTIC_IDENTITY_HEADER_SIZE as u16).to_le_bytes());
    encoded.extend_from_slice(&0_u16.to_le_bytes());
    encoded.extend_from_slice(&record_size.to_le_bytes());
    encoded.extend_from_slice(&identity_size.to_le_bytes());
    encoded.extend_from_slice(&cargo_unit_id);
    encoded.extend_from_slice(&definition_record_id);
    encoded.extend_from_slice(identity.as_bytes());
    debug_assert_eq!(encoded.len(), record_size as usize);
    encoded
}

/// Kind tag stored in an inline payload header.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PayloadKind {
    /// Complete JSON emitted for one `#[rsjs(component...)]` descriptor.
    ComponentDescriptor = 1,
    /// Complete JSON emitted for one standalone `#[rsjs::client]` helper.
    StandaloneHelperDescriptor = 2,
    /// Exact public key and complete canonical wire-schema ABI for one
    /// SharedState type.
    SharedStateDefinition = 3,
}

impl PayloadKind {
    #[cfg(any(feature = "extract", test))]
    pub(crate) fn from_wire(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::ComponentDescriptor),
            2 => Some(Self::StandaloneHelperDescriptor),
            3 => Some(Self::SharedStateDefinition),
            _ => None,
        }
    }
}

/// An aligned inline byte record emitted directly into an object section.
///
/// The payload is never interpreted or executed by target code. The wrapper's
/// only purpose is to keep consecutive variable-size records eight-byte
/// aligned in object sections on every supported target.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct PayloadRecord<const N: usize> {
    /// Versioned envelope followed by the complete descriptor bytes.
    pub bytes: [u8; N],
}

impl<const N: usize> PayloadRecord<N> {
    /// Construct an inline payload from bytes produced by
    /// [`encode_payload_record`].
    pub const fn new(bytes: [u8; N]) -> Self {
        Self { bytes }
    }
}

/// Client-visible role encoded into a component-field witness identity.
///
/// This is transport metadata, not a Rust nominal type identity. Rustc checks
/// the actual field projection and callback shape in generated code.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportComponentFieldRole {
    /// Inbound live value.
    Value = 1,
    /// Compiler-owned callback resolved at invocation time.
    Callback = 2,
    /// Required `NodeRef<T>` handle alias.
    NodeRef = 3,
    /// Exact `self.<field>` projection whose role is resolved through the
    /// rustc-linked child field.
    DeferredProjection = 4,
}

/// Required/optional shape encoded for callback-field witness identity.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportCallbackShape {
    /// Value fields do not have a callback shape.
    NotCallback = 0,
    /// `Handler<T>`.
    Required = 1,
    /// `Option<Handler<T>>`.
    Optional = 2,
}

/// Derive the exact object-record ID for one compiler-discovered component
/// field.
///
/// Both the producer and linker call this function. The component input
/// fingerprint scopes dense field/crossing IDs to one exact analyzed
/// definition; role and callback shape prevent a stale record from validating
/// after a behavior-bearing schema change.
pub fn component_field_record_id(
    component_input_fingerprint: &str,
    field_id: u32,
    crossing_hook_id: u32,
    role: TransportComponentFieldRole,
    callback_shape: TransportCallbackShape,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.component-field.v1\0");
    hasher.update(component_input_fingerprint.as_bytes());
    hasher.update(field_id.to_le_bytes());
    hasher.update(crossing_hook_id.to_le_bytes());
    hasher.update([role as u8, callback_shape as u8]);
    hasher.finalize().into()
}

/// Derive the defining object-record ID for one analyzed component payload.
///
/// This binds every fingerprint-scoped field and edge claim back to the exact
/// rustc-resolved parent definition instead of merely proving that the claims
/// are self-consistent with one another.
pub fn component_record_id(component_input_fingerprint: &str) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.component-record.v1\0");
    hasher.update(component_input_fingerprint.as_bytes());
    hasher.finalize().into()
}

/// Authored ownership boundary committed by a direct component source edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectComponentBoundary {
    /// The child participates in the enclosing compiler-owned owner graph.
    Composed,
    /// The child starts an independent island owner.
    ExplicitIsland,
}

/// Branch polarity committed by a conditional direct component source edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectComponentBranch {
    /// The site is in the conditional's `then` branch.
    Then,
    /// The site is in the conditional's `else` branch.
    Else,
}

/// Exact lifecycle topology committed by a direct component source edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectComponentLifecycle {
    /// One unconditional occurrence.
    StaticSingle,
    /// One occurrence controlled by one canonical conditional.
    ConditionalSingle {
        /// Canonical parent-local conditional control.
        control: u32,
        /// Producer-local expression that supplies the control predicate.
        predicate_expression: u32,
        /// Domain-separated commitment to the analyzed component graph that
        /// supplies the predicate's complete upstream provenance.
        ///
        /// The current v1 commitment conservatively covers the full component
        /// descriptor graph. That is a strict superset of the predicate
        /// closure and avoids trusting a payload-recomputed closure boundary.
        predicate_graph: TransportRecordId,
        /// Exact branch polarity.
        branch: TransportDirectComponentBranch,
    },
    /// An occurrence inside a server template loop.
    Repeated,
    /// A currently unsupported occurrence under nested conditionals.
    UnknownNestedConditionals,
}

/// Exact analyzer provenance for a live-value component field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectLiveValueSource {
    /// A direct signal `.get()`.
    DirectSignalRead,
    /// A compiler-proven alias of one exact signal read.
    ClientAlias,
    /// A live inbound component field reused as one exact signal.
    InboundField,
}

/// Exact static callback spelling accepted by the analyzer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectStaticCallbackForm {
    /// Literal `None` for an optional callback.
    None,
    /// Required `rsjs::noop_handler()`.
    NoopHandler,
    /// Optional `Some(rsjs::noop_handler())`.
    SomeNoopHandler,
}

/// Exact live callback spelling accepted by the analyzer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectLiveCallbackForm {
    /// A required payload handler.
    Handler,
    /// An optional `Some(payload_handler)`.
    SomeHandler,
}

/// Behavior-bearing field binding committed by a direct component edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportDirectComponentBinding {
    /// No compiler-owned client dependency is visible at this field.
    ServerOnly,
    /// One exact parent signal remains live in the child.
    LiveValue {
        /// Parent-local legacy signal ID.
        signal: u32,
        /// Exact analyzer provenance for the signal.
        source: TransportDirectLiveValueSource,
    },
    /// A static inert callback.
    StaticCallback {
        /// Exact accepted callback form.
        form: TransportDirectStaticCallbackForm,
    },
    /// A compiler-owned live callback.
    LiveCallback {
        /// Exact parent prelude binding containing the payload handler.
        handler_prelude_local: u32,
        /// Required versus optional callback form.
        form: TransportDirectLiveCallbackForm,
    },
    /// One exact parent node-ref object aliases a required child field.
    NodeRefHandle {
        /// Parent-local legacy node-ref ID.
        node_ref: u32,
    },
    /// Type-ambiguous inbound projection. The linked child field selects
    /// exactly one of the committed value/callback, signal, or NodeRef paths.
    DeferredProjection {
        /// Parent inbound field identity.
        field: u32,
        /// Parent-local synthetic current-value signal.
        signal: u32,
        /// Parent-local exact NodeRef identity.
        node_ref: u32,
    },
    /// One immutable, exactly-one-node server slot across an island boundary.
    StaticSlotExactOne,
    /// An unproven binding retained only for a focused compatibility gap.
    Unsupported,
}

/// One ordered field entry in a direct component edge commitment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransportDirectComponentField<'a> {
    /// Exact Rust field name.
    pub name: &'a str,
    /// Exact analyzer binding classification and source identity.
    pub binding: TransportDirectComponentBinding,
}

/// Complete behavior-bearing metadata authenticated by a direct edge record.
///
/// Diagnostic-only path spelling and source coordinates are deliberately
/// excluded. The rustc relocation authenticates the child type itself; this
/// tuple authenticates every parent-side fact that can change canonical owner
/// topology, coupling, or browser behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransportDirectComponentEdge<'a> {
    /// Composed versus independent-island ownership.
    pub boundary: TransportDirectComponentBoundary,
    /// Exact schema view node representing this occurrence.
    pub site_node: u32,
    /// Exact multiplicity/control topology.
    pub lifecycle: TransportDirectComponentLifecycle,
    /// Whether the source struct literal contains `..rest`.
    pub has_rest: bool,
    /// Dense source-ordered field names and binding classifications.
    pub fields: &'a [TransportDirectComponentField<'a>],
}

/// Behavior-bearing topology authenticated for one static
/// `SharedStateProvider` source site.
///
/// The provider's state type is authenticated separately by the
/// [`SharedStateAcquisitionRecord`] relocation emitted with this source-edge
/// identifier. This tuple binds that relocation to the exact wrapperless
/// server node and to the ordinary rustc-resolved component edge that renders
/// the provider's direct child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransportSharedStateProviderSite {
    /// Exact schema view node containing the complete provider expression.
    pub site_node: u32,
    /// Dense source-local direct-component edge for `children`.
    pub child_edge_ordinal: u32,
    /// Authenticated source-edge identifier of that child component edge.
    pub child_source_edge_id: TransportRecordId,
}

/// Commit one conditional component predicate and its analyzed provenance.
///
/// `component_descriptor` is the producer's exact descriptor object, excluding
/// transport claims and the outer payload hash. Serialization lives in this
/// shared transport crate so macro and adapter cannot disagree about byte
/// ordering. Hashing the complete descriptor is intentionally conservative:
/// it authenticates every possible upstream predicate producer until a
/// separately versioned canonical closure encoding is available.
pub fn component_lifecycle_predicate_graph_commitment(
    component_descriptor: &serde_json::Map<String, serde_json::Value>,
    control: u32,
    predicate_expression: u32,
) -> TransportRecordId {
    let descriptor_bytes =
        canonical_json_bytes(&serde_json::Value::Object(component_descriptor.clone()))
            .expect("a serde_json component descriptor object always serializes");
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.lifecycle-predicate-graph.v1\0");
    hasher.update(control.to_le_bytes());
    hasher.update(predicate_expression.to_le_bytes());
    hash_sized_bytes(&mut hasher, &descriptor_bytes);
    hasher.finalize().into()
}

/// Derive the exact source-edge record for one direct component syntax edge.
///
/// The input fingerprint and dense ordinal scope the relation to one analyzed
/// parent. `edge` commits the complete behavior-bearing parent-side tuple, so
/// a payload cannot move a live binding, island cut, lifecycle, or view site
/// onto another valid rustc relocation while retaining that relocation's ID.
pub fn component_source_edge_id(
    component_input_fingerprint: &str,
    local_ordinal: u32,
    edge: &TransportDirectComponentEdge<'_>,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.component-edge.v3\0");
    hash_sized_bytes(&mut hasher, component_input_fingerprint.as_bytes());
    hasher.update(local_ordinal.to_le_bytes());
    hasher.update([match edge.boundary {
        TransportDirectComponentBoundary::Composed => 0,
        TransportDirectComponentBoundary::ExplicitIsland => 1,
    }]);
    hasher.update(edge.site_node.to_le_bytes());
    match edge.lifecycle {
        TransportDirectComponentLifecycle::StaticSingle => hasher.update([0]),
        TransportDirectComponentLifecycle::ConditionalSingle {
            control,
            predicate_expression,
            predicate_graph,
            branch,
        } => {
            hasher.update([1]);
            hasher.update(control.to_le_bytes());
            hasher.update(predicate_expression.to_le_bytes());
            hasher.update(predicate_graph);
            hasher.update([match branch {
                TransportDirectComponentBranch::Then => 0,
                TransportDirectComponentBranch::Else => 1,
            }]);
        }
        TransportDirectComponentLifecycle::Repeated => hasher.update([2]),
        TransportDirectComponentLifecycle::UnknownNestedConditionals => hasher.update([3]),
    }
    hasher.update([u8::from(edge.has_rest)]);
    hasher.update(
        u64::try_from(edge.fields.len())
            .expect("direct component field count must fit u64")
            .to_le_bytes(),
    );
    for field in edge.fields {
        hash_sized_bytes(&mut hasher, field.name.as_bytes());
        match field.binding {
            TransportDirectComponentBinding::ServerOnly => hasher.update([0]),
            TransportDirectComponentBinding::LiveValue { signal, source } => {
                hasher.update([1]);
                hasher.update(signal.to_le_bytes());
                hasher.update([match source {
                    TransportDirectLiveValueSource::DirectSignalRead => 0,
                    TransportDirectLiveValueSource::ClientAlias => 1,
                    TransportDirectLiveValueSource::InboundField => 2,
                }]);
            }
            TransportDirectComponentBinding::StaticCallback { form } => {
                hasher.update([2]);
                hasher.update([match form {
                    TransportDirectStaticCallbackForm::None => 0,
                    TransportDirectStaticCallbackForm::NoopHandler => 1,
                    TransportDirectStaticCallbackForm::SomeNoopHandler => 2,
                }]);
            }
            TransportDirectComponentBinding::LiveCallback {
                handler_prelude_local,
                form,
            } => {
                hasher.update([3]);
                hasher.update(handler_prelude_local.to_le_bytes());
                hasher.update([match form {
                    TransportDirectLiveCallbackForm::Handler => 0,
                    TransportDirectLiveCallbackForm::SomeHandler => 1,
                }]);
            }
            TransportDirectComponentBinding::NodeRefHandle { node_ref } => {
                // Keep all existing binding discriminants stable.
                hasher.update([6]);
                hasher.update(node_ref.to_le_bytes());
            }
            TransportDirectComponentBinding::DeferredProjection {
                field,
                signal,
                node_ref,
            } => {
                hasher.update([7]);
                hasher.update(field.to_le_bytes());
                hasher.update(signal.to_le_bytes());
                hasher.update(node_ref.to_le_bytes());
            }
            TransportDirectComponentBinding::StaticSlotExactOne => hasher.update([4]),
            TransportDirectComponentBinding::Unsupported => hasher.update([5]),
        }
    }
    hasher.finalize().into()
}

/// Derive the provider-specific source edge for one static
/// `SharedStateProvider` topology declaration.
///
/// Provider declarations and `rsjs::shared::<T>()` acquisitions intentionally
/// share the same fixed relocation record layout, but never the same source
/// edge domain. Committing the exact child edge and whole-expression server
/// node prevents a valid SharedState witness relocation from being moved to a
/// different provider occurrence or descendant.
pub fn shared_state_provider_source_edge_id(
    component_input_fingerprint: &str,
    provider_site_ordinal: u32,
    site: &TransportSharedStateProviderSite,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.shared-state-provider-site.v1\0");
    hash_sized_bytes(&mut hasher, component_input_fingerprint.as_bytes());
    hasher.update(provider_site_ordinal.to_le_bytes());
    hasher.update(site.site_node.to_le_bytes());
    hasher.update(site.child_edge_ordinal.to_le_bytes());
    hasher.update(site.child_source_edge_id);
    hasher.finalize().into()
}

fn hash_sized_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(
        u64::try_from(bytes.len())
            .expect("transport commitment input length must fit u64")
            .to_le_bytes(),
    );
    hasher.update(bytes);
}

/// Derive the source-edge record for one component-local client-helper call.
///
/// This relation is ordinal-bearing protocol data, not an unordered claim:
/// recomputation prevents two valid relocation IDs from being swapped across
/// source call sites.
pub fn component_helper_source_edge_id(
    component_input_fingerprint: &str,
    local_ordinal: u32,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.client-helper-edge.v1\0");
    hasher.update(component_input_fingerprint.as_bytes());
    hasher.update(local_ordinal.to_le_bytes());
    hasher.finalize().into()
}

/// Derive the source-edge record for one direct call in a standalone helper.
pub fn helper_direct_call_source_edge_id(
    helper_record_id: TransportRecordId,
    ordinal: u32,
) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.client-helper-direct-call.v1\0");
    hasher.update(helper_record_id);
    hasher.update(u64::from(ordinal).to_le_bytes());
    hasher.finalize().into()
}

/// Compute the domain-separated semantic content hash stored in a payload
/// envelope.
///
/// Producers must make `payload` a deterministic canonical encoding. The
/// transport validates this digest without needing to execute target code or
/// understand a producer's higher-level descriptor schema.
pub fn semantic_payload_hash(kind: PayloadKind, payload: &[u8]) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.semantic-payload.v1\0");
    hasher.update((kind as u16).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().into()
}

/// Compute the domain-separated hash of a complete canonical SharedState
/// wire-schema ABI.
///
/// The transport deliberately treats the ABI bytes as opaque. A future
/// producer must emit the canonical bytes directly; extraction never follows
/// or decodes target-native `ClientPropsMeta` pointers.
pub fn shared_state_schema_abi_hash(schema_abi: &[u8]) -> TransportRecordId {
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.transport.shared-state-schema-abi.v1\0");
    hasher.update(schema_abi);
    hasher.finalize().into()
}

/// Encode the deterministic payload carried by one SharedState definition.
///
/// Layout (all integer fields little-endian):
///
/// ```text
/// 0..8      SharedState schema magic
/// 8..10     schema payload version
/// 10..12    header size
/// 12..16    flags (zero in v1)
/// 16..24    complete unpadded payload size
/// 24..32    UTF-8 type_key byte length
/// 32..40    canonical schema ABI byte length
/// 40..72    domain-separated schema ABI hash
/// 72..      type_key bytes followed by exact canonical schema ABI bytes
/// ```
///
/// The public key and schema are length-delimited rather than serialized
/// through a target-native Rust value. The caller is responsible for supplying
/// the complete canonical schema ABI in deterministic field order.
pub fn encode_shared_state_schema_payload(type_key: &str, schema_abi: &[u8]) -> Vec<u8> {
    assert!(
        !type_key.is_empty()
            && type_key == type_key.trim()
            && !type_key.chars().any(char::is_whitespace)
            && type_key.contains('.'),
        "SharedState type_key must be non-empty, namespaced, and contain no whitespace"
    );
    assert!(
        !schema_abi.is_empty(),
        "SharedState canonical schema ABI must not be empty"
    );
    let payload_size = SHARED_STATE_SCHEMA_HEADER_SIZE
        .checked_add(type_key.len())
        .and_then(|size| size.checked_add(schema_abi.len()))
        .expect("RSJS SharedState schema payload size overflow");
    let payload_size = u64::try_from(payload_size)
        .expect("RSJS SharedState schema payload does not fit the wire format");
    let type_key_size =
        u64::try_from(type_key.len()).expect("RSJS SharedState type_key does not fit wire format");
    let schema_abi_size = u64::try_from(schema_abi.len())
        .expect("RSJS SharedState schema ABI does not fit wire format");
    let schema_abi_hash = shared_state_schema_abi_hash(schema_abi);

    let mut encoded = Vec::with_capacity(payload_size as usize);
    encoded.extend_from_slice(&SHARED_STATE_SCHEMA_FORMAT_MAGIC);
    encoded.extend_from_slice(&SHARED_STATE_SCHEMA_FORMAT_VERSION.to_le_bytes());
    encoded.extend_from_slice(&(SHARED_STATE_SCHEMA_HEADER_SIZE as u16).to_le_bytes());
    encoded.extend_from_slice(&0_u32.to_le_bytes());
    encoded.extend_from_slice(&payload_size.to_le_bytes());
    encoded.extend_from_slice(&type_key_size.to_le_bytes());
    encoded.extend_from_slice(&schema_abi_size.to_le_bytes());
    encoded.extend_from_slice(&schema_abi_hash);
    encoded.extend_from_slice(type_key.as_bytes());
    encoded.extend_from_slice(schema_abi);
    debug_assert_eq!(encoded.len(), payload_size as usize);
    encoded
}

/// Encode one complete outer payload record for a SharedState definition.
pub fn encode_shared_state_definition_payload_record(
    cargo_unit_id: TransportRecordId,
    shared_state_record_id: TransportRecordId,
    type_key: &str,
    schema_abi: &[u8],
) -> Vec<u8> {
    let payload = encode_shared_state_schema_payload(type_key, schema_abi);
    encode_payload_record(
        PayloadKind::SharedStateDefinition,
        cargo_unit_id,
        shared_state_record_id,
        &payload,
    )
}

/// Whether canonical SharedState ABI bytes contain a complete schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SharedStateSchemaAbiResolution {
    /// Every recursive field codec is represented exactly.
    Resolved,
    /// A producer encountered a schema shape whose recursive ABI cannot yet be
    /// represented soundly.
    Unresolved(SharedStateSchemaUnresolvedReason),
}

/// Typed reason a SharedState definition is intentionally unavailable to the
/// canonical linker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SharedStateSchemaUnresolvedReason {
    /// A field reaches another `ClientProps`-derived codec. Rust path tokens
    /// are not a schema identity, so v1 records no path and waits for a
    /// relocation-backed recursive codec witness.
    NestedClientProps,
}

/// Equality semantics of one independently reactive SharedState field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SharedStateSchemaEquality {
    /// JavaScript `Object.is`.
    ObjectIs,
}

/// Closed recursive client-codec vocabulary in canonical SharedState ABI v1.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SharedStateSchemaCodec {
    Bool,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    String,
    Option(Box<Self>),
    Vec(Box<Self>),
}

/// One top-level field in a resolved canonical SharedState schema.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SharedStateSchemaField {
    /// Dense declaration-order field identifier.
    pub id: u32,
    /// Authored field name.
    pub name: String,
    /// Browser equality semantics.
    pub equality: SharedStateSchemaEquality,
    /// Complete recursive codec shape.
    pub codec: SharedStateSchemaCodec,
}

/// Decoded producer-owned canonical SharedState schema ABI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStateSchemaAbi {
    /// Runtime protocol version covered by these bytes.
    pub runtime_abi_version: u32,
    /// Complete schema or an explicit fail-closed reason.
    pub resolution: SharedStateSchemaAbiResolution,
    /// Declaration-ordered fields. Empty for an unresolved schema.
    pub fields: Vec<SharedStateSchemaField>,
}

/// Error while constructing or decoding canonical SharedState schema ABI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStateSchemaAbiError {
    reason: String,
}

impl SharedStateSchemaAbiError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl core::fmt::Display for SharedStateSchemaAbiError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for SharedStateSchemaAbiError {}

const SHARED_STATE_SCHEMA_RESOLVED: u8 = 1;
const SHARED_STATE_SCHEMA_UNRESOLVED: u8 = 2;
const SHARED_STATE_SCHEMA_REASON_NONE: u8 = 0;
const SHARED_STATE_SCHEMA_REASON_NESTED_CLIENT_PROPS: u8 = 1;

const SHARED_STATE_SCHEMA_EQUALITY_OBJECT_IS: u8 = 1;

const SHARED_STATE_CODEC_BOOL: u8 = 1;
const SHARED_STATE_CODEC_I32: u8 = 2;
const SHARED_STATE_CODEC_U32: u8 = 3;
const SHARED_STATE_CODEC_I64: u8 = 4;
const SHARED_STATE_CODEC_U64: u8 = 5;
const SHARED_STATE_CODEC_F32: u8 = 6;
const SHARED_STATE_CODEC_F64: u8 = 7;
const SHARED_STATE_CODEC_STRING: u8 = 8;
const SHARED_STATE_CODEC_OPTION: u8 = 9;
const SHARED_STATE_CODEC_VEC: u8 = 10;
const SHARED_STATE_CODEC_MAX_DEPTH: usize = 64;

/// Encode a complete closed SharedState schema.
///
/// Field identifiers must be dense and declaration ordered. Names must be
/// unique and non-empty. These constraints make byte equality sufficient for
/// schema equality without a later normalization pass.
pub fn encode_resolved_shared_state_schema_abi(
    runtime_abi_version: u32,
    fields: &[SharedStateSchemaField],
) -> Result<Vec<u8>, SharedStateSchemaAbiError> {
    if fields.is_empty() {
        return Err(SharedStateSchemaAbiError::new(
            "a resolved SharedState schema must contain at least one field",
        ));
    }

    let mut body = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (ordinal, field) in fields.iter().enumerate() {
        let expected_id = u32::try_from(ordinal).map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field count exceeds the v1 ABI")
        })?;
        if field.id != expected_id {
            return Err(SharedStateSchemaAbiError::new(format!(
                "SharedState field ids must be dense in declaration order: expected {expected_id}, found {}",
                field.id
            )));
        }
        if field.name.is_empty() {
            return Err(SharedStateSchemaAbiError::new(
                "SharedState field names must not be empty",
            ));
        }
        if !names.insert(field.name.as_str()) {
            return Err(SharedStateSchemaAbiError::new(format!(
                "duplicate SharedState field name `{}`",
                field.name
            )));
        }

        let codec = encode_shared_state_codec(&field.codec, 0)?;
        let field_size = SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE
            .checked_add(field.name.len())
            .and_then(|size| size.checked_add(codec.len()))
            .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState field ABI size overflow"))?;
        let field_size = u64::try_from(field_size).map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field ABI exceeds the wire format")
        })?;
        let name_size = u32::try_from(field.name.len()).map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field name exceeds the wire format")
        })?;
        let codec_size = u32::try_from(codec.len()).map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field codec exceeds the wire format")
        })?;

        body.extend_from_slice(&field.id.to_le_bytes());
        body.push(match field.equality {
            SharedStateSchemaEquality::ObjectIs => SHARED_STATE_SCHEMA_EQUALITY_OBJECT_IS,
        });
        body.push(0);
        body.extend_from_slice(&(SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE as u16).to_le_bytes());
        body.extend_from_slice(&field_size.to_le_bytes());
        body.extend_from_slice(&name_size.to_le_bytes());
        body.extend_from_slice(&codec_size.to_le_bytes());
        body.extend_from_slice(field.name.as_bytes());
        body.extend_from_slice(&codec);
    }

    encode_shared_state_schema_abi_header(
        runtime_abi_version,
        SHARED_STATE_SCHEMA_RESOLVED,
        SHARED_STATE_SCHEMA_REASON_NONE,
        fields.len(),
        &body,
    )
}

/// Encode an intentionally unresolved SharedState definition.
///
/// No Rust path or partial field schema is included. The typed reason is enough
/// for the linker to reject the definition without accidentally treating
/// source spelling as protocol identity.
pub fn encode_unresolved_shared_state_schema_abi(
    runtime_abi_version: u32,
    reason: SharedStateSchemaUnresolvedReason,
) -> Vec<u8> {
    let reason = match reason {
        SharedStateSchemaUnresolvedReason::NestedClientProps => {
            SHARED_STATE_SCHEMA_REASON_NESTED_CLIENT_PROPS
        }
    };
    encode_shared_state_schema_abi_header(
        runtime_abi_version,
        SHARED_STATE_SCHEMA_UNRESOLVED,
        reason,
        0,
        &[],
    )
    .expect("the fixed unresolved SharedState ABI header is valid")
}

fn encode_shared_state_schema_abi_header(
    runtime_abi_version: u32,
    resolution: u8,
    reason: u8,
    field_count: usize,
    body: &[u8],
) -> Result<Vec<u8>, SharedStateSchemaAbiError> {
    let record_size = SHARED_STATE_CANONICAL_ABI_HEADER_SIZE
        .checked_add(body.len())
        .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState schema ABI size overflow"))?;
    let record_size = u64::try_from(record_size).map_err(|_| {
        SharedStateSchemaAbiError::new("SharedState schema ABI exceeds the wire format")
    })?;
    let field_count = u32::try_from(field_count).map_err(|_| {
        SharedStateSchemaAbiError::new("SharedState field count exceeds the wire format")
    })?;

    let mut encoded = Vec::with_capacity(record_size as usize);
    encoded.extend_from_slice(&SHARED_STATE_CANONICAL_ABI_MAGIC);
    encoded.extend_from_slice(&SHARED_STATE_CANONICAL_ABI_VERSION.to_le_bytes());
    encoded.extend_from_slice(&(SHARED_STATE_CANONICAL_ABI_HEADER_SIZE as u16).to_le_bytes());
    encoded.push(resolution);
    encoded.push(reason);
    encoded.extend_from_slice(&0_u16.to_le_bytes());
    encoded.extend_from_slice(&runtime_abi_version.to_le_bytes());
    encoded.extend_from_slice(&field_count.to_le_bytes());
    encoded.extend_from_slice(&record_size.to_le_bytes());
    encoded.extend_from_slice(body);
    debug_assert_eq!(encoded.len(), record_size as usize);
    Ok(encoded)
}

fn encode_shared_state_codec(
    codec: &SharedStateSchemaCodec,
    depth: usize,
) -> Result<Vec<u8>, SharedStateSchemaAbiError> {
    if depth >= SHARED_STATE_CODEC_MAX_DEPTH {
        return Err(SharedStateSchemaAbiError::new(format!(
            "SharedState codec nesting exceeds the v1 limit of {SHARED_STATE_CODEC_MAX_DEPTH}"
        )));
    }
    let (tag, child) = match codec {
        SharedStateSchemaCodec::Bool => (SHARED_STATE_CODEC_BOOL, None),
        SharedStateSchemaCodec::I32 => (SHARED_STATE_CODEC_I32, None),
        SharedStateSchemaCodec::U32 => (SHARED_STATE_CODEC_U32, None),
        SharedStateSchemaCodec::I64 => (SHARED_STATE_CODEC_I64, None),
        SharedStateSchemaCodec::U64 => (SHARED_STATE_CODEC_U64, None),
        SharedStateSchemaCodec::F32 => (SHARED_STATE_CODEC_F32, None),
        SharedStateSchemaCodec::F64 => (SHARED_STATE_CODEC_F64, None),
        SharedStateSchemaCodec::String => (SHARED_STATE_CODEC_STRING, None),
        SharedStateSchemaCodec::Option(inner) => (
            SHARED_STATE_CODEC_OPTION,
            Some(encode_shared_state_codec(inner, depth + 1)?),
        ),
        SharedStateSchemaCodec::Vec(inner) => (
            SHARED_STATE_CODEC_VEC,
            Some(encode_shared_state_codec(inner, depth + 1)?),
        ),
    };
    let child = child.as_deref().unwrap_or_default();
    let record_size = SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE
        .checked_add(child.len())
        .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState codec ABI size overflow"))?;
    let record_size = u32::try_from(record_size).map_err(|_| {
        SharedStateSchemaAbiError::new("SharedState codec ABI exceeds the wire format")
    })?;
    let mut encoded = Vec::with_capacity(record_size as usize);
    encoded.push(tag);
    encoded.push(0);
    encoded.extend_from_slice(&(SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE as u16).to_le_bytes());
    encoded.extend_from_slice(&record_size.to_le_bytes());
    encoded.extend_from_slice(child);
    Ok(encoded)
}

/// Decode and validate canonical SharedState schema ABI bytes.
///
/// A semantic linker should call this before grouping definitions by
/// `type_key`. [`SharedStateSchemaAbiResolution::Unresolved`] is a deliberate
/// hard stop, not a schema that may be linked by comparing its bytes.
pub fn decode_shared_state_schema_abi(
    bytes: &[u8],
) -> Result<SharedStateSchemaAbi, SharedStateSchemaAbiError> {
    if bytes.len() < SHARED_STATE_CANONICAL_ABI_HEADER_SIZE {
        return Err(SharedStateSchemaAbiError::new(format!(
            "truncated canonical SharedState ABI header: found {} bytes, need {SHARED_STATE_CANONICAL_ABI_HEADER_SIZE}",
            bytes.len()
        )));
    }
    if bytes[..8] != SHARED_STATE_CANONICAL_ABI_MAGIC {
        return Err(SharedStateSchemaAbiError::new(
            "wrong canonical SharedState ABI magic",
        ));
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("fixed ABI version"));
    if version != SHARED_STATE_CANONICAL_ABI_VERSION {
        return Err(SharedStateSchemaAbiError::new(format!(
            "unsupported canonical SharedState ABI version {version}; expected {SHARED_STATE_CANONICAL_ABI_VERSION}"
        )));
    }
    let header_size =
        u16::from_le_bytes(bytes[10..12].try_into().expect("fixed ABI header")) as usize;
    if header_size != SHARED_STATE_CANONICAL_ABI_HEADER_SIZE {
        return Err(SharedStateSchemaAbiError::new(format!(
            "canonical SharedState ABI header size {header_size} does not match {SHARED_STATE_CANONICAL_ABI_HEADER_SIZE}"
        )));
    }
    if u16::from_le_bytes(bytes[14..16].try_into().expect("fixed ABI flags")) != 0 {
        return Err(SharedStateSchemaAbiError::new(
            "canonical SharedState ABI flags must be zero in v1",
        ));
    }
    let runtime_abi_version =
        u32::from_le_bytes(bytes[16..20].try_into().expect("fixed runtime ABI"));
    let field_count =
        u32::from_le_bytes(bytes[20..24].try_into().expect("fixed field count")) as usize;
    let record_size = usize::try_from(u64::from_le_bytes(
        bytes[24..32].try_into().expect("fixed ABI size"),
    ))
    .map_err(|_| {
        SharedStateSchemaAbiError::new(
            "canonical SharedState ABI record size exceeds the host address space",
        )
    })?;
    if record_size != bytes.len() {
        return Err(SharedStateSchemaAbiError::new(format!(
            "canonical SharedState ABI record size {record_size} does not match {} bytes",
            bytes.len()
        )));
    }

    let resolution = match (bytes[12], bytes[13]) {
        (SHARED_STATE_SCHEMA_RESOLVED, SHARED_STATE_SCHEMA_REASON_NONE) => {
            SharedStateSchemaAbiResolution::Resolved
        }
        (SHARED_STATE_SCHEMA_UNRESOLVED, SHARED_STATE_SCHEMA_REASON_NESTED_CLIENT_PROPS) => {
            SharedStateSchemaAbiResolution::Unresolved(
                SharedStateSchemaUnresolvedReason::NestedClientProps,
            )
        }
        (resolution, reason) => {
            return Err(SharedStateSchemaAbiError::new(format!(
                "unknown canonical SharedState ABI resolution/reason pair {resolution}/{reason}"
            )));
        }
    };

    if let SharedStateSchemaAbiResolution::Unresolved(_) = resolution {
        if field_count != 0 || record_size != SHARED_STATE_CANONICAL_ABI_HEADER_SIZE {
            return Err(SharedStateSchemaAbiError::new(
                "an unresolved canonical SharedState ABI must not contain partial field data",
            ));
        }
        return Ok(SharedStateSchemaAbi {
            runtime_abi_version,
            resolution,
            fields: Vec::new(),
        });
    }
    if field_count == 0 {
        return Err(SharedStateSchemaAbiError::new(
            "a resolved canonical SharedState ABI must contain at least one field",
        ));
    }
    let maximum_field_count = (bytes.len() - SHARED_STATE_CANONICAL_ABI_HEADER_SIZE)
        / SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE;
    if field_count > maximum_field_count {
        return Err(SharedStateSchemaAbiError::new(format!(
            "canonical SharedState ABI field count {field_count} exceeds the maximum {maximum_field_count} possible in this record"
        )));
    }

    let mut fields = Vec::with_capacity(field_count);
    let mut names = std::collections::BTreeSet::new();
    let mut cursor = SHARED_STATE_CANONICAL_ABI_HEADER_SIZE;
    for ordinal in 0..field_count {
        let header_end = cursor
            .checked_add(SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE)
            .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState field offset overflow"))?;
        if header_end > bytes.len() {
            return Err(SharedStateSchemaAbiError::new(format!(
                "truncated canonical SharedState field {ordinal} header"
            )));
        }
        let id = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().expect("field id"));
        let expected_id = u32::try_from(ordinal).map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field count exceeds the v1 ABI")
        })?;
        if id != expected_id {
            return Err(SharedStateSchemaAbiError::new(format!(
                "SharedState field ids must be dense in declaration order: expected {expected_id}, found {id}"
            )));
        }
        let equality = match bytes[cursor + 4] {
            SHARED_STATE_SCHEMA_EQUALITY_OBJECT_IS => SharedStateSchemaEquality::ObjectIs,
            value => {
                return Err(SharedStateSchemaAbiError::new(format!(
                    "unknown SharedState field equality tag {value}"
                )));
            }
        };
        if bytes[cursor + 5] != 0 {
            return Err(SharedStateSchemaAbiError::new(
                "SharedState field reserved byte must be zero",
            ));
        }
        let field_header_size = u16::from_le_bytes(
            bytes[cursor + 6..cursor + 8]
                .try_into()
                .expect("field header"),
        ) as usize;
        if field_header_size != SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE {
            return Err(SharedStateSchemaAbiError::new(format!(
                "SharedState field header size {field_header_size} does not match {SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE}"
            )));
        }
        let field_size = usize::try_from(u64::from_le_bytes(
            bytes[cursor + 8..cursor + 16]
                .try_into()
                .expect("field size"),
        ))
        .map_err(|_| {
            SharedStateSchemaAbiError::new("SharedState field size exceeds the host address space")
        })?;
        let name_size = u32::from_le_bytes(
            bytes[cursor + 16..cursor + 20]
                .try_into()
                .expect("field name size"),
        ) as usize;
        let codec_size = u32::from_le_bytes(
            bytes[cursor + 20..cursor + 24]
                .try_into()
                .expect("codec size"),
        ) as usize;
        let expected_field_size = SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE
            .checked_add(name_size)
            .and_then(|size| size.checked_add(codec_size))
            .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState field size overflow"))?;
        if field_size != expected_field_size {
            return Err(SharedStateSchemaAbiError::new(format!(
                "SharedState field {id} layout mismatch: record={field_size}, name={name_size}, codec={codec_size}"
            )));
        }
        let field_end = cursor
            .checked_add(field_size)
            .ok_or_else(|| SharedStateSchemaAbiError::new("SharedState field offset overflow"))?;
        if field_end > bytes.len() {
            return Err(SharedStateSchemaAbiError::new(format!(
                "truncated canonical SharedState field {id}"
            )));
        }
        let name_start = header_end;
        let name_end = name_start + name_size;
        let name = core::str::from_utf8(&bytes[name_start..name_end])
            .map_err(|error| {
                SharedStateSchemaAbiError::new(format!(
                    "SharedState field {id} name is not UTF-8: {error}"
                ))
            })?
            .to_string();
        if name.is_empty() {
            return Err(SharedStateSchemaAbiError::new(
                "SharedState field names must not be empty",
            ));
        }
        if !names.insert(name.clone()) {
            return Err(SharedStateSchemaAbiError::new(format!(
                "duplicate SharedState field name `{name}`"
            )));
        }
        let codec = decode_shared_state_codec(&bytes[name_end..field_end], 0)?;
        fields.push(SharedStateSchemaField {
            id,
            name,
            equality,
            codec,
        });
        cursor = field_end;
    }
    if cursor != bytes.len() {
        return Err(SharedStateSchemaAbiError::new(format!(
            "canonical SharedState ABI has {} trailing bytes",
            bytes.len() - cursor
        )));
    }

    Ok(SharedStateSchemaAbi {
        runtime_abi_version,
        resolution,
        fields,
    })
}

fn decode_shared_state_codec(
    bytes: &[u8],
    depth: usize,
) -> Result<SharedStateSchemaCodec, SharedStateSchemaAbiError> {
    if depth >= SHARED_STATE_CODEC_MAX_DEPTH {
        return Err(SharedStateSchemaAbiError::new(format!(
            "SharedState codec nesting exceeds the v1 limit of {SHARED_STATE_CODEC_MAX_DEPTH}"
        )));
    }
    if bytes.len() < SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE {
        return Err(SharedStateSchemaAbiError::new(
            "truncated canonical SharedState codec header",
        ));
    }
    if bytes[1] != 0 {
        return Err(SharedStateSchemaAbiError::new(
            "SharedState codec flags must be zero in v1",
        ));
    }
    let header_size =
        u16::from_le_bytes(bytes[2..4].try_into().expect("codec header size")) as usize;
    if header_size != SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE {
        return Err(SharedStateSchemaAbiError::new(format!(
            "SharedState codec header size {header_size} does not match {SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE}"
        )));
    }
    let record_size =
        u32::from_le_bytes(bytes[4..8].try_into().expect("codec record size")) as usize;
    if record_size != bytes.len() {
        return Err(SharedStateSchemaAbiError::new(format!(
            "SharedState codec record size {record_size} does not match {} bytes",
            bytes.len()
        )));
    }
    let child = &bytes[SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE..];
    let primitive = |codec| {
        if child.is_empty() {
            Ok(codec)
        } else {
            Err(SharedStateSchemaAbiError::new(
                "primitive SharedState codec must not contain a child codec",
            ))
        }
    };
    match bytes[0] {
        SHARED_STATE_CODEC_BOOL => primitive(SharedStateSchemaCodec::Bool),
        SHARED_STATE_CODEC_I32 => primitive(SharedStateSchemaCodec::I32),
        SHARED_STATE_CODEC_U32 => primitive(SharedStateSchemaCodec::U32),
        SHARED_STATE_CODEC_I64 => primitive(SharedStateSchemaCodec::I64),
        SHARED_STATE_CODEC_U64 => primitive(SharedStateSchemaCodec::U64),
        SHARED_STATE_CODEC_F32 => primitive(SharedStateSchemaCodec::F32),
        SHARED_STATE_CODEC_F64 => primitive(SharedStateSchemaCodec::F64),
        SHARED_STATE_CODEC_STRING => primitive(SharedStateSchemaCodec::String),
        SHARED_STATE_CODEC_OPTION => Ok(SharedStateSchemaCodec::Option(Box::new(
            decode_shared_state_codec(child, depth + 1)?,
        ))),
        SHARED_STATE_CODEC_VEC => Ok(SharedStateSchemaCodec::Vec(Box::new(
            decode_shared_state_codec(child, depth + 1)?,
        ))),
        tag => Err(SharedStateSchemaAbiError::new(format!(
            "unknown SharedState codec tag {tag}"
        ))),
    }
}

/// Encode one complete inline descriptor payload.
///
/// Layout (all integer fields little-endian):
///
/// ```text
/// 0..8      magic
/// 8..10     payload format version
/// 10..12    payload kind
/// 12..14    header size
/// 14..16    flags (zero in v1)
/// 16..24    complete unpadded record size
/// 24..32    payload byte length
/// 32..64    CargoUnitId
/// 64..96    defining component/helper record ID
/// 96..128   domain-separated semantic payload hash
/// 128..     inline payload bytes
/// ```
pub fn encode_payload_record(
    kind: PayloadKind,
    cargo_unit_id: TransportRecordId,
    definition_record_id: TransportRecordId,
    payload: &[u8],
) -> Vec<u8> {
    let record_size = PAYLOAD_HEADER_SIZE
        .checked_add(payload.len())
        .expect("RSJS payload record size overflow");
    let record_size =
        u64::try_from(record_size).expect("RSJS payload record does not fit the wire format");
    let payload_size =
        u64::try_from(payload.len()).expect("RSJS payload does not fit the wire format");
    let semantic_hash = semantic_payload_hash(kind, payload);

    let mut encoded = Vec::with_capacity(record_size as usize);
    encoded.extend_from_slice(&PAYLOAD_FORMAT_MAGIC);
    encoded.extend_from_slice(&PAYLOAD_FORMAT_VERSION.to_le_bytes());
    encoded.extend_from_slice(&(kind as u16).to_le_bytes());
    encoded.extend_from_slice(&(PAYLOAD_HEADER_SIZE as u16).to_le_bytes());
    encoded.extend_from_slice(&0_u16.to_le_bytes());
    encoded.extend_from_slice(&record_size.to_le_bytes());
    encoded.extend_from_slice(&payload_size.to_le_bytes());
    encoded.extend_from_slice(&cargo_unit_id);
    encoded.extend_from_slice(&definition_record_id);
    encoded.extend_from_slice(&semantic_hash);
    encoded.extend_from_slice(payload);
    debug_assert_eq!(encoded.len(), record_size as usize);
    encoded
}

/// Kind tag stored in [`RecordHeader`].
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// One descriptor-bearing component and its defining witness.
    ComponentDefinition = 1,
    /// One source edge whose relocation names a component witness.
    ComponentEdge = 2,
    /// One client helper and a relocation to its rewritten Rust function.
    HelperDefinition = 3,
    /// One source helper edge whose relocation names the resolved Rust function.
    HelperEdge = 4,
    /// One component input field and a relocation to its defining component
    /// witness.
    ComponentField = 5,
    /// One exact SharedState schema and a relocation to its Rust type witness.
    SharedStateDefinition = 6,
    /// One component-local SharedState acquisition whose relocation names the
    /// resolved Rust type witness.
    SharedStateAcquisition = 7,
}

impl RecordKind {
    #[cfg(any(feature = "extract", test))]
    pub(crate) fn from_wire(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::ComponentDefinition),
            2 => Some(Self::ComponentEdge),
            3 => Some(Self::HelperDefinition),
            4 => Some(Self::HelperEdge),
            5 => Some(Self::ComponentField),
            6 => Some(Self::SharedStateDefinition),
            7 => Some(Self::SharedStateAcquisition),
            _ => None,
        }
    }
}

/// Component-definition flag indicating an asynchronous `WebRender` surface.
pub const COMPONENT_DEFINITION_FLAG_ASYNC: u32 = 1 << 0;

/// Component-definition flag granting permission to originate browser behavior.
pub const COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE: u32 = 1 << 1;

/// Every component-definition flag understood by transport format v3.
pub const COMPONENT_DEFINITION_KNOWN_FLAGS: u32 =
    COMPONENT_DEFINITION_FLAG_ASYNC | COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE;

/// Rust-checked server render surface projected into a component definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportComponentRenderKind {
    /// The component implements `WebRenderSync`.
    Sync,
    /// The component implements `WebRender`.
    Async,
}

/// Compiler-owned permission projected into a component definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportComponentOriginPermission {
    /// The component may only participate in client behavior owned elsewhere.
    ParticipantOnly,
    /// The component may originate browser-owned behavior.
    OriginCapable,
}

/// Target-independent projection of the defining `ComponentAbiWitness`.
///
/// The projection travels in the flags of the same fixed record whose
/// relocation names that witness. It therefore survives host extraction
/// without decoding target-native Rust struct layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComponentDefinitionAbi {
    /// Synchronous or asynchronous server render surface.
    pub render_kind: TransportComponentRenderKind,
    /// Participant-only or origin-capable client permission.
    pub origin_permission: TransportComponentOriginPermission,
}

impl ComponentDefinitionAbi {
    /// The default ABI of a static synchronous component.
    pub const SYNC_PARTICIPANT: Self = Self {
        render_kind: TransportComponentRenderKind::Sync,
        origin_permission: TransportComponentOriginPermission::ParticipantOnly,
    };

    /// Construct a typed component-definition projection.
    pub const fn new(
        render_kind: TransportComponentRenderKind,
        origin_permission: TransportComponentOriginPermission,
    ) -> Self {
        Self {
            render_kind,
            origin_permission,
        }
    }

    /// Encode this projection into transport-v3 component-definition flags.
    pub const fn flags(self) -> u32 {
        let render = match self.render_kind {
            TransportComponentRenderKind::Sync => 0,
            TransportComponentRenderKind::Async => COMPONENT_DEFINITION_FLAG_ASYNC,
        };
        let permission = match self.origin_permission {
            TransportComponentOriginPermission::ParticipantOnly => 0,
            TransportComponentOriginPermission::OriginCapable => {
                COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE
            }
        };
        render | permission
    }

    /// Decode transport-v3 component-definition flags.
    ///
    /// Unknown bits fail closed instead of silently changing the component
    /// contract under a newer producer.
    pub const fn from_flags(flags: u32) -> Option<Self> {
        if flags & !COMPONENT_DEFINITION_KNOWN_FLAGS != 0 {
            return None;
        }
        let render_kind = if flags & COMPONENT_DEFINITION_FLAG_ASYNC != 0 {
            TransportComponentRenderKind::Async
        } else {
            TransportComponentRenderKind::Sync
        };
        let origin_permission = if flags & COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE != 0 {
            TransportComponentOriginPermission::OriginCapable
        } else {
            TransportComponentOriginPermission::ParticipantOnly
        };
        Some(Self::new(render_kind, origin_permission))
    }
}

impl Default for ComponentDefinitionAbi {
    fn default() -> Self {
        Self::SYNC_PARTICIPANT
    }
}

/// The fixed 32-byte prefix shared by all transport records.
///
/// Integer fields are stored as little-endian byte arrays so a host extractor
/// can inspect cross-target artifacts without executing target code or
/// interpreting native-endian Rust integers.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct RecordHeader {
    /// [`TRANSPORT_FORMAT_MAGIC`].
    pub magic: [u8; 8],
    /// [`TRANSPORT_FORMAT_VERSION`], little endian.
    pub format_version_le: [u8; 2],
    /// [`RecordKind`] discriminant, little endian.
    pub record_kind_le: [u8; 2],
    /// Header byte width, little endian.
    pub header_size_le: [u8; 2],
    /// Complete record byte width, little endian.
    pub record_size_le: [u8; 2],
    /// Offset of the relocation-bearing pointer slot, little endian.
    pub relocation_offset_le: [u8; 2],
    /// Target pointer width in bytes.
    pub pointer_width: u8,
    /// Versioned flags. In v3 only component definitions may use bits, and
    /// those bits encode [`ComponentDefinitionAbi`].
    pub flags_le: [u8; 4],
    /// Must be zero in format v3.
    pub reserved: [u8; 9],
}

impl RecordHeader {
    const fn new(kind: RecordKind, relocation_offset: u16) -> Self {
        Self::new_with_flags(kind, relocation_offset, 0)
    }

    const fn new_with_flags(kind: RecordKind, relocation_offset: u16, flags: u32) -> Self {
        Self {
            magic: TRANSPORT_FORMAT_MAGIC,
            format_version_le: TRANSPORT_FORMAT_VERSION.to_le_bytes(),
            record_kind_le: (kind as u16).to_le_bytes(),
            header_size_le: (core::mem::size_of::<Self>() as u16).to_le_bytes(),
            record_size_le: TRANSPORT_RECORD_SIZE.to_le_bytes(),
            relocation_offset_le: relocation_offset.to_le_bytes(),
            pointer_width: core::mem::size_of::<usize>() as u8,
            flags_le: flags.to_le_bytes(),
            reserved: [0; 9],
        }
    }

    /// Decode the transport format version.
    pub const fn format_version(&self) -> u16 {
        u16::from_le_bytes(self.format_version_le)
    }

    /// Decode the record-kind discriminant.
    pub const fn record_kind_raw(&self) -> u16 {
        u16::from_le_bytes(self.record_kind_le)
    }

    /// Decode the header byte width.
    pub const fn header_size(&self) -> u16 {
        u16::from_le_bytes(self.header_size_le)
    }

    /// Decode the complete record byte width.
    pub const fn record_size(&self) -> u16 {
        u16::from_le_bytes(self.record_size_le)
    }

    /// Decode the pointer-relocation offset.
    pub const fn relocation_offset(&self) -> u16 {
        u16::from_le_bytes(self.relocation_offset_le)
    }

    /// Decode the capability flags.
    pub const fn flags(&self) -> u32 {
        u32::from_le_bytes(self.flags_le)
    }
}

/// One fixed eight-byte slot whose first target-pointer-width bytes carry an
/// object relocation.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RelocationSlot<T: 'static> {
    /// Rust-resolved relocation target.
    pub target: &'static T,
    /// Keeps the slot eight bytes wide on 32-bit targets.
    #[cfg(target_pointer_width = "32")]
    pub padding: [u8; 4],
}

impl<T> RelocationSlot<T> {
    /// Construct a relocation slot from a Rust-resolved static reference.
    pub const fn new(target: &'static T) -> Self {
        Self {
            target,
            #[cfg(target_pointer_width = "32")]
            padding: [0; 4],
        }
    }
}

/// One fixed eight-byte slot whose first target-pointer-width bytes relocate
/// directly to a rewritten Rust client-helper function.
///
/// The pointer is never dereferenced by target code. It exists only so an
/// object extractor can follow rustc's resolved function identity.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FunctionRelocationSlot {
    /// Rust-resolved helper function address.
    pub target: *const (),
    /// Keeps the slot eight bytes wide on 32-bit targets.
    #[cfg(target_pointer_width = "32")]
    pub padding: [u8; 4],
}

// SAFETY: the slot is immutable transport data and no code in this crate
// dereferences or mutates the stored address. Sharing it only shares the
// object relocation bytes.
unsafe impl Sync for FunctionRelocationSlot {}

impl FunctionRelocationSlot {
    /// Construct a relocation slot from a Rust-resolved function address.
    pub const fn new(target: *const ()) -> Self {
        Self {
            target,
            #[cfg(target_pointer_width = "32")]
            padding: [0; 4],
        }
    }
}

/// Fixed definition record emitted beside one `ComponentAbi` witness.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct ComponentDefinitionRecord<T: 'static> {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Build-local descriptor record identifier. This is payload metadata, not
    /// Rust component identity.
    pub component_record_id: TransportRecordId,
    /// The ABI projection is encoded in [`RecordHeader::flags`], keeping this
    /// fixed record the same size as every other relocation record.
    /// Relocation to the defining `ComponentAbi::WITNESS`.
    pub witness: RelocationSlot<T>,
}

impl<T> ComponentDefinitionRecord<T> {
    /// Construct a component-definition transport record.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        component_record_id: TransportRecordId,
        component_abi: ComponentDefinitionAbi,
        witness: &'static T,
    ) -> Self {
        Self {
            header: RecordHeader::new_with_flags(
                RecordKind::ComponentDefinition,
                core::mem::offset_of!(Self, witness) as u16
                    + core::mem::offset_of!(RelocationSlot<T>, target) as u16,
                component_abi.flags(),
            ),
            cargo_unit_id,
            component_record_id,
            witness: RelocationSlot::new(witness),
        }
    }
}

/// Fixed edge record emitted at a direct component source edge.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct ComponentEdgeRecord<T: 'static> {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Stable source edge identifier inside the producer descriptor.
    pub source_edge_id: TransportRecordId,
    /// Relocation obtained through `<ResolvedType as ComponentAbi>::WITNESS`.
    pub witness: RelocationSlot<T>,
}

impl<T> ComponentEdgeRecord<T> {
    /// Construct an edge from a rustc-resolved component witness reference.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        source_edge_id: TransportRecordId,
        witness: &'static T,
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::ComponentEdge,
                core::mem::offset_of!(Self, witness) as u16
                    + core::mem::offset_of!(RelocationSlot<T>, target) as u16,
            ),
            cargo_unit_id,
            source_edge_id,
            witness: RelocationSlot::new(witness),
        }
    }
}

/// Fixed record emitted for one rustc-checked component input field.
///
/// The record does not attempt to encode nominal Rust type identity. Its
/// relocation points to the same `ComponentAbi::WITNESS` as the defining
/// component record, while `field_record_id` joins later compiler metadata to
/// this exact object record.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct ComponentFieldRecord<T: 'static> {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Deterministic identifier for this component input field record.
    pub field_record_id: TransportRecordId,
    /// Relocation to the defining `ComponentAbi::WITNESS`.
    pub witness: RelocationSlot<T>,
}

impl<T> ComponentFieldRecord<T> {
    /// Construct a component-field witness transport record.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        field_record_id: TransportRecordId,
        witness: &'static T,
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::ComponentField,
                core::mem::offset_of!(Self, witness) as u16
                    + core::mem::offset_of!(RelocationSlot<T>, target) as u16,
            ),
            cargo_unit_id,
            field_record_id,
            witness: RelocationSlot::new(witness),
        }
    }
}

/// Fixed definition record emitted beside one rewritten `#[rsjs::client]`
/// helper function.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct HelperDefinitionRecord {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Build-local helper payload identifier. This is metadata, not helper
    /// identity.
    pub helper_record_id: TransportRecordId,
    /// Direct relocation to the rewritten Rust helper function.
    pub helper: FunctionRelocationSlot,
}

impl HelperDefinitionRecord {
    /// Construct a client-helper-definition transport record.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        helper_record_id: TransportRecordId,
        helper: *const (),
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::HelperDefinition,
                core::mem::offset_of!(Self, helper) as u16
                    + core::mem::offset_of!(FunctionRelocationSlot, target) as u16,
            ),
            cargo_unit_id,
            helper_record_id,
            helper: FunctionRelocationSlot::new(helper),
        }
    }
}

/// Fixed source-edge record for one rustc-resolved client-helper call.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct HelperEdgeRecord {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Stable source edge identifier inside the producer descriptor.
    pub source_edge_id: TransportRecordId,
    /// Direct relocation to the Rust helper selected by name resolution.
    pub helper: FunctionRelocationSlot,
}

impl HelperEdgeRecord {
    /// Construct an edge from a rustc-resolved helper function address.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        source_edge_id: TransportRecordId,
        helper: *const (),
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::HelperEdge,
                core::mem::offset_of!(Self, helper) as u16
                    + core::mem::offset_of!(FunctionRelocationSlot, target) as u16,
            ),
            cargo_unit_id,
            source_edge_id,
            helper: FunctionRelocationSlot::new(helper),
        }
    }
}

/// Fixed definition record emitted beside one compiler-generated SharedState
/// type witness.
///
/// The witness target is the Rust-resolved nominal type identity. The
/// `shared_state_record_id` joins this fixed record to an inline
/// [`PayloadKind::SharedStateDefinition`] record containing the public
/// `type_key` and complete canonical schema ABI bytes.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct SharedStateDefinitionRecord<T: 'static> {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Build-local SharedState definition/payload record identifier.
    pub shared_state_record_id: TransportRecordId,
    /// Relocation to the defining SharedState type witness.
    pub witness: RelocationSlot<T>,
}

impl<T> SharedStateDefinitionRecord<T> {
    /// Construct a SharedState-definition transport record.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        shared_state_record_id: TransportRecordId,
        witness: &'static T,
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::SharedStateDefinition,
                core::mem::offset_of!(Self, witness) as u16
                    + core::mem::offset_of!(RelocationSlot<T>, target) as u16,
            ),
            cargo_unit_id,
            shared_state_record_id,
            witness: RelocationSlot::new(witness),
        }
    }
}

/// Fixed component-local acquisition edge for `rsjs::shared::<T>()`.
///
/// The producer obtains `witness` through the acquired Rust type. Following
/// this relocation is the only accepted way to associate the acquisition with
/// a SharedState definition; source path tokens are not identity.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct SharedStateAcquisitionRecord<T: 'static> {
    /// Versioned transport envelope.
    pub header: RecordHeader,
    /// Realized descriptor-pass Cargo unit, or
    /// [`UNASSIGNED_CARGO_UNIT_ID`] in this prototype.
    pub cargo_unit_id: TransportRecordId,
    /// Stable source edge identifier inside the acquiring component.
    pub source_edge_id: TransportRecordId,
    /// Rust-resolved relocation to the acquired SharedState type witness.
    pub witness: RelocationSlot<T>,
}

impl<T> SharedStateAcquisitionRecord<T> {
    /// Construct a SharedState acquisition from its rustc-resolved witness.
    pub const fn new(
        cargo_unit_id: TransportRecordId,
        source_edge_id: TransportRecordId,
        witness: &'static T,
    ) -> Self {
        Self {
            header: RecordHeader::new(
                RecordKind::SharedStateAcquisition,
                core::mem::offset_of!(Self, witness) as u16
                    + core::mem::offset_of!(RelocationSlot<T>, target) as u16,
            ),
            cargo_unit_id,
            source_edge_id,
            witness: RelocationSlot::new(witness),
        }
    }
}

/// Every v3 record has this exact byte width on 32- and 64-bit targets.
pub const TRANSPORT_RECORD_SIZE: u16 = core::mem::size_of::<ComponentEdgeRecord<()>>() as u16;

const _: () = assert!(core::mem::size_of::<RecordHeader>() == 32);
const _: () =
    assert!(core::mem::size_of::<SemanticIdentityHeader>() == SEMANTIC_IDENTITY_HEADER_SIZE);
const _: () = assert!(core::mem::offset_of!(SemanticIdentityRecord<1>, identity) == 96);
const _: () = assert!(core::mem::size_of::<RelocationSlot<()>>() == 8);
const _: () = assert!(core::mem::size_of::<FunctionRelocationSlot>() == 8);
const _: () = assert!(
    core::mem::size_of::<ComponentDefinitionRecord<()>>()
        == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(
    core::mem::size_of::<HelperDefinitionRecord>()
        == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(
    core::mem::size_of::<ComponentFieldRecord<()>>()
        == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(
    core::mem::size_of::<HelperEdgeRecord>() == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(
    core::mem::size_of::<SharedStateDefinitionRecord<()>>()
        == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(
    core::mem::size_of::<SharedStateAcquisitionRecord<()>>()
        == core::mem::size_of::<ComponentEdgeRecord<()>>()
);
const _: () = assert!(TRANSPORT_RECORD_SIZE == 104);

#[cfg(test)]
mod tests {
    use super::*;

    static TARGET: u8 = 7;

    fn helper(value: u32) -> u32 {
        value
    }

    #[test]
    fn v3_layout_is_fixed_and_self_describing() {
        let definition = ComponentDefinitionRecord::new(
            [1; 32],
            [2; 32],
            ComponentDefinitionAbi::SYNC_PARTICIPANT,
            &TARGET as &'static u8,
        );
        let edge = ComponentEdgeRecord::new([3; 32], [4; 32], &TARGET as &'static u8);
        let helper_definition = HelperDefinitionRecord::new([5; 32], [6; 32], helper as *const ());
        let helper_edge = HelperEdgeRecord::new([7; 32], [8; 32], helper as *const ());
        let field = ComponentFieldRecord::new([9; 32], [10; 32], &TARGET as &'static u8);
        let shared_definition =
            SharedStateDefinitionRecord::new([11; 32], [12; 32], &TARGET as &'static u8);
        let shared_acquisition =
            SharedStateAcquisitionRecord::new([13; 32], [14; 32], &TARGET as &'static u8);

        for header in [
            definition.header,
            edge.header,
            helper_definition.header,
            helper_edge.header,
            field.header,
            shared_definition.header,
            shared_acquisition.header,
        ] {
            assert_eq!(header.magic, TRANSPORT_FORMAT_MAGIC);
            assert_eq!(header.format_version(), TRANSPORT_FORMAT_VERSION);
            assert_eq!(header.header_size(), 32);
            assert_eq!(header.record_size(), TRANSPORT_RECORD_SIZE);
            assert_eq!(header.relocation_offset(), 96);
            assert_eq!(header.pointer_width, core::mem::size_of::<usize>() as u8);
            assert_eq!(header.flags(), 0);
            assert_eq!(header.reserved, [0; 9]);
        }
        assert_eq!(
            definition.header.record_kind_raw(),
            RecordKind::ComponentDefinition as u16
        );
        assert_eq!(
            edge.header.record_kind_raw(),
            RecordKind::ComponentEdge as u16
        );
        assert_eq!(
            helper_definition.header.record_kind_raw(),
            RecordKind::HelperDefinition as u16
        );
        assert_eq!(
            helper_edge.header.record_kind_raw(),
            RecordKind::HelperEdge as u16
        );
        assert_eq!(
            field.header.record_kind_raw(),
            RecordKind::ComponentField as u16
        );
        assert_eq!(
            shared_definition.header.record_kind_raw(),
            RecordKind::SharedStateDefinition as u16
        );
        assert_eq!(
            shared_acquisition.header.record_kind_raw(),
            RecordKind::SharedStateAcquisition as u16
        );
        assert_eq!(field.cargo_unit_id, [9; 32]);
        assert_eq!(field.field_record_id, [10; 32]);
    }

    #[test]
    fn shared_state_schema_payload_is_exact_deterministic_hash_material() {
        let schema = b"\x01object\0mode\0string\0selected_node\0option(string)";
        let first = encode_shared_state_schema_payload("acme.workbench", schema);
        let second = encode_shared_state_schema_payload("acme.workbench", schema);
        assert_eq!(first, second);
        assert_eq!(&first[..8], &SHARED_STATE_SCHEMA_FORMAT_MAGIC);
        assert_eq!(
            u16::from_le_bytes(first[8..10].try_into().unwrap()),
            SHARED_STATE_SCHEMA_FORMAT_VERSION
        );
        assert_eq!(
            u16::from_le_bytes(first[10..12].try_into().unwrap()) as usize,
            SHARED_STATE_SCHEMA_HEADER_SIZE
        );
        assert_eq!(
            u64::from_le_bytes(first[16..24].try_into().unwrap()) as usize,
            first.len()
        );
        assert_eq!(
            u64::from_le_bytes(first[24..32].try_into().unwrap()) as usize,
            "acme.workbench".len()
        );
        assert_eq!(
            u64::from_le_bytes(first[32..40].try_into().unwrap()) as usize,
            schema.len()
        );
        assert_eq!(&first[40..72], &shared_state_schema_abi_hash(schema));
        assert_eq!(
            &first[SHARED_STATE_SCHEMA_HEADER_SIZE
                ..SHARED_STATE_SCHEMA_HEADER_SIZE + "acme.workbench".len()],
            b"acme.workbench"
        );
        assert_eq!(
            &first[SHARED_STATE_SCHEMA_HEADER_SIZE + "acme.workbench".len()..],
            schema
        );

        let other_key = encode_shared_state_schema_payload("acme.other", schema);
        let other_schema =
            encode_shared_state_schema_payload("acme.workbench", b"\x01object\0mode\0bool");
        assert_ne!(
            semantic_payload_hash(PayloadKind::SharedStateDefinition, &first),
            semantic_payload_hash(PayloadKind::SharedStateDefinition, &other_key)
        );
        assert_ne!(
            semantic_payload_hash(PayloadKind::SharedStateDefinition, &first),
            semantic_payload_hash(PayloadKind::SharedStateDefinition, &other_schema)
        );
        assert_ne!(
            shared_state_schema_abi_hash(schema),
            shared_state_schema_abi_hash(b"\x01object\0mode\0bool")
        );
    }

    #[test]
    fn canonical_shared_state_schema_round_trips_closed_recursive_codecs() {
        let fields = vec![
            SharedStateSchemaField {
                id: 0,
                name: "enabled".to_string(),
                equality: SharedStateSchemaEquality::ObjectIs,
                codec: SharedStateSchemaCodec::Bool,
            },
            SharedStateSchemaField {
                id: 1,
                name: "labels".to_string(),
                equality: SharedStateSchemaEquality::ObjectIs,
                codec: SharedStateSchemaCodec::Option(Box::new(SharedStateSchemaCodec::Vec(
                    Box::new(SharedStateSchemaCodec::String),
                ))),
            },
            SharedStateSchemaField {
                id: 2,
                name: "total".to_string(),
                equality: SharedStateSchemaEquality::ObjectIs,
                codec: SharedStateSchemaCodec::I64,
            },
        ];
        let encoded =
            encode_resolved_shared_state_schema_abi(SHARED_STATE_RUNTIME_ABI_VERSION, &fields)
                .unwrap();
        assert_eq!(&encoded[..8], &SHARED_STATE_CANONICAL_ABI_MAGIC);
        assert_eq!(
            decode_shared_state_schema_abi(&encoded).unwrap(),
            SharedStateSchemaAbi {
                runtime_abi_version: SHARED_STATE_RUNTIME_ABI_VERSION,
                resolution: SharedStateSchemaAbiResolution::Resolved,
                fields,
            }
        );
    }

    #[test]
    fn unresolved_nested_schema_is_typed_and_contains_no_rust_path() {
        let encoded = encode_unresolved_shared_state_schema_abi(
            SHARED_STATE_RUNTIME_ABI_VERSION,
            SharedStateSchemaUnresolvedReason::NestedClientProps,
        );
        assert_eq!(encoded.len(), SHARED_STATE_CANONICAL_ABI_HEADER_SIZE);
        assert!(!encoded
            .windows(b"crate::models::Nested".len())
            .any(|window| window == b"crate::models::Nested"));
        assert_eq!(
            decode_shared_state_schema_abi(&encoded).unwrap(),
            SharedStateSchemaAbi {
                runtime_abi_version: SHARED_STATE_RUNTIME_ABI_VERSION,
                resolution: SharedStateSchemaAbiResolution::Unresolved(
                    SharedStateSchemaUnresolvedReason::NestedClientProps,
                ),
                fields: Vec::new(),
            }
        );
    }

    #[test]
    fn canonical_schema_rejects_forged_huge_field_count_before_allocation() {
        let mut encoded = encode_resolved_shared_state_schema_abi(
            SHARED_STATE_RUNTIME_ABI_VERSION,
            &[SharedStateSchemaField {
                id: 0,
                name: "value".to_string(),
                equality: SharedStateSchemaEquality::ObjectIs,
                codec: SharedStateSchemaCodec::U32,
            }],
        )
        .unwrap();
        encoded[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        let error = decode_shared_state_schema_abi(&encoded).unwrap_err();
        assert!(error.to_string().contains("field count"));
        assert!(error.to_string().contains("exceeds the maximum"));
    }

    #[test]
    fn canonical_schema_rejects_excessive_nested_codec_depth() {
        fn raw_codec(tag: u8, child: &[u8]) -> Vec<u8> {
            let size = SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE + child.len();
            let mut encoded = Vec::with_capacity(size);
            encoded.push(tag);
            encoded.push(0);
            encoded.extend_from_slice(
                &(SHARED_STATE_CANONICAL_CODEC_HEADER_SIZE as u16).to_le_bytes(),
            );
            encoded.extend_from_slice(&(size as u32).to_le_bytes());
            encoded.extend_from_slice(child);
            encoded
        }

        let mut codec = raw_codec(SHARED_STATE_CODEC_STRING, &[]);
        for _ in 0..SHARED_STATE_CODEC_MAX_DEPTH {
            codec = raw_codec(SHARED_STATE_CODEC_VEC, &codec);
        }
        let name = b"value";
        let field_size = SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE + name.len() + codec.len();
        let mut body = Vec::with_capacity(field_size);
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.push(SHARED_STATE_SCHEMA_EQUALITY_OBJECT_IS);
        body.push(0);
        body.extend_from_slice(&(SHARED_STATE_CANONICAL_FIELD_HEADER_SIZE as u16).to_le_bytes());
        body.extend_from_slice(&(field_size as u64).to_le_bytes());
        body.extend_from_slice(&(name.len() as u32).to_le_bytes());
        body.extend_from_slice(&(codec.len() as u32).to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(&codec);
        let encoded = encode_shared_state_schema_abi_header(
            SHARED_STATE_RUNTIME_ABI_VERSION,
            SHARED_STATE_SCHEMA_RESOLVED,
            SHARED_STATE_SCHEMA_REASON_NONE,
            1,
            &body,
        )
        .unwrap();

        let error = decode_shared_state_schema_abi(&encoded).unwrap_err();
        assert!(error.to_string().contains("nesting exceeds"));

        let mut model = SharedStateSchemaCodec::String;
        for _ in 0..SHARED_STATE_CODEC_MAX_DEPTH {
            model = SharedStateSchemaCodec::Vec(Box::new(model));
        }
        let error = encode_resolved_shared_state_schema_abi(
            SHARED_STATE_RUNTIME_ABI_VERSION,
            &[SharedStateSchemaField {
                id: 0,
                name: "value".to_string(),
                equality: SharedStateSchemaEquality::ObjectIs,
                codec: model,
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("nesting exceeds"));
    }

    #[test]
    fn component_definition_flags_round_trip_every_v3_abi_combination() {
        for (render_kind, origin_permission, expected_flags) in [
            (
                TransportComponentRenderKind::Sync,
                TransportComponentOriginPermission::ParticipantOnly,
                0,
            ),
            (
                TransportComponentRenderKind::Async,
                TransportComponentOriginPermission::ParticipantOnly,
                COMPONENT_DEFINITION_FLAG_ASYNC,
            ),
            (
                TransportComponentRenderKind::Sync,
                TransportComponentOriginPermission::OriginCapable,
                COMPONENT_DEFINITION_FLAG_ORIGIN_CAPABLE,
            ),
            (
                TransportComponentRenderKind::Async,
                TransportComponentOriginPermission::OriginCapable,
                COMPONENT_DEFINITION_KNOWN_FLAGS,
            ),
        ] {
            let abi = ComponentDefinitionAbi::new(render_kind, origin_permission);
            let definition =
                ComponentDefinitionRecord::new([1; 32], [2; 32], abi, &TARGET as &'static u8);
            assert_eq!(definition.header.flags(), expected_flags);
            assert_eq!(
                ComponentDefinitionAbi::from_flags(expected_flags),
                Some(abi)
            );
        }

        assert_eq!(
            ComponentDefinitionAbi::from_flags(1 << 31),
            None,
            "unknown component ABI bits must fail closed"
        );
    }

    #[test]
    fn component_field_ids_cover_role_shape_and_crossing() {
        let value = component_field_record_id(
            "input-fingerprint",
            2,
            3,
            TransportComponentFieldRole::Value,
            TransportCallbackShape::NotCallback,
        );
        assert_eq!(
            value,
            component_field_record_id(
                "input-fingerprint",
                2,
                3,
                TransportComponentFieldRole::Value,
                TransportCallbackShape::NotCallback,
            )
        );
        assert_ne!(
            value,
            component_field_record_id(
                "input-fingerprint",
                2,
                4,
                TransportComponentFieldRole::Value,
                TransportCallbackShape::NotCallback,
            )
        );
        assert_ne!(
            value,
            component_field_record_id(
                "input-fingerprint",
                2,
                3,
                TransportComponentFieldRole::Callback,
                TransportCallbackShape::Required,
            )
        );
        assert_ne!(
            value,
            component_field_record_id(
                "input-fingerprint",
                2,
                3,
                TransportComponentFieldRole::NodeRef,
                TransportCallbackShape::NotCallback,
            ),
            "a required NodeRef<T> handle is not an ordinary value field"
        );
        assert_ne!(
            component_field_record_id(
                "input-fingerprint",
                2,
                3,
                TransportComponentFieldRole::Callback,
                TransportCallbackShape::Required,
            ),
            component_field_record_id(
                "input-fingerprint",
                2,
                3,
                TransportComponentFieldRole::Callback,
                TransportCallbackShape::Optional,
            )
        );
    }

    #[test]
    fn lifecycle_predicate_graph_commitment_is_json_key_order_stable() {
        let forward: serde_json::Value = serde_json::from_str(
            r#"{"bindings":{"conditionals":[{"condition":4,"parent":0}]},"exprs":[{"id":4}]}"#,
        )
        .unwrap();
        let reverse: serde_json::Value = serde_json::from_str(
            r#"{"exprs":[{"id":4}],"bindings":{"conditionals":[{"parent":0,"condition":4}]}}"#,
        )
        .unwrap();
        assert_eq!(
            component_lifecycle_predicate_graph_commitment(forward.as_object().unwrap(), 0, 4,),
            component_lifecycle_predicate_graph_commitment(reverse.as_object().unwrap(), 0, 4,),
            "macro and adapter descriptor parsing must produce one deterministic commitment"
        );
    }

    #[test]
    fn source_edge_ids_bind_their_definition_scope_relation_and_ordinal() {
        let predicate_descriptor = serde_json::json!({"exprs": [{"id": 4, "signal": 3}]});
        let predicate_descriptor = predicate_descriptor
            .as_object()
            .expect("fixture descriptor is an object");
        let predicate_graph =
            component_lifecycle_predicate_graph_commitment(predicate_descriptor, 2, 4);
        let fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::LiveValue {
                signal: 3,
                source: TransportDirectLiveValueSource::DirectSignalRead,
            },
        }];
        let edge = TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::Composed,
            site_node: 11,
            lifecycle: TransportDirectComponentLifecycle::ConditionalSingle {
                control: 2,
                predicate_expression: 4,
                predicate_graph,
                branch: TransportDirectComponentBranch::Then,
            },
            has_rest: false,
            fields: &fields,
        };
        assert_eq!(
            component_record_id("sha256:fixture"),
            component_record_id("sha256:fixture")
        );
        assert_ne!(
            component_record_id("sha256:fixture"),
            component_record_id("sha256:other-fixture")
        );

        let component = component_source_edge_id("sha256:fixture", 7, &edge);
        assert_eq!(
            component,
            component_source_edge_id("sha256:fixture", 7, &edge),
            "source-edge derivation must be deterministic"
        );
        assert_ne!(
            component,
            component_source_edge_id("sha256:fixture", 8, &edge)
        );
        assert_ne!(
            component,
            component_source_edge_id("sha256:other-fixture", 7, &edge)
        );
        assert_ne!(
            component,
            component_helper_source_edge_id("sha256:fixture", 7),
            "component and helper relations must be domain-separated"
        );

        let island_edge = TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::ExplicitIsland,
            ..edge
        };
        assert_ne!(
            component,
            component_source_edge_id("sha256:fixture", 7, &island_edge),
            "ownership boundary is behavior-bearing"
        );
        let moved_edge = TransportDirectComponentEdge {
            site_node: 12,
            ..edge
        };
        assert_ne!(
            component,
            component_source_edge_id("sha256:fixture", 7, &moved_edge),
            "exact view site is behavior-bearing"
        );
        let static_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::ServerOnly,
        }];
        let static_edge = TransportDirectComponentEdge {
            fields: &static_fields,
            ..edge
        };
        assert_ne!(
            component,
            component_source_edge_id("sha256:fixture", 7, &static_edge),
            "field binding semantics are behavior-bearing"
        );
        let aliased_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::LiveValue {
                signal: 3,
                source: TransportDirectLiveValueSource::ClientAlias,
            },
        }];
        assert_ne!(
            component,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &aliased_fields,
                    ..edge
                },
            ),
            "live-value provenance is behavior-bearing"
        );
        let static_callback_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::StaticCallback {
                form: TransportDirectStaticCallbackForm::None,
            },
        }];
        let some_static_callback_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::StaticCallback {
                form: TransportDirectStaticCallbackForm::SomeNoopHandler,
            },
        }];
        assert_ne!(
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &static_callback_fields,
                    ..edge
                },
            ),
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &some_static_callback_fields,
                    ..edge
                },
            ),
            "static callback form is behavior-bearing"
        );
        for lifecycle in [
            TransportDirectComponentLifecycle::ConditionalSingle {
                control: 3,
                predicate_expression: 4,
                predicate_graph: component_lifecycle_predicate_graph_commitment(
                    predicate_descriptor,
                    3,
                    4,
                ),
                branch: TransportDirectComponentBranch::Then,
            },
            TransportDirectComponentLifecycle::ConditionalSingle {
                control: 2,
                predicate_expression: 4,
                predicate_graph,
                branch: TransportDirectComponentBranch::Else,
            },
            TransportDirectComponentLifecycle::ConditionalSingle {
                control: 2,
                predicate_expression: 5,
                predicate_graph: component_lifecycle_predicate_graph_commitment(
                    predicate_descriptor,
                    2,
                    5,
                ),
                branch: TransportDirectComponentBranch::Then,
            },
            TransportDirectComponentLifecycle::ConditionalSingle {
                control: 2,
                predicate_expression: 4,
                predicate_graph: component_lifecycle_predicate_graph_commitment(
                    serde_json::json!({"exprs": [{"id": 4, "signal": 9}]})
                        .as_object()
                        .expect("fixture descriptor is an object"),
                    2,
                    4,
                ),
                branch: TransportDirectComponentBranch::Then,
            },
            TransportDirectComponentLifecycle::Repeated,
            TransportDirectComponentLifecycle::UnknownNestedConditionals,
        ] {
            assert_ne!(
                component,
                component_source_edge_id(
                    "sha256:fixture",
                    7,
                    &TransportDirectComponentEdge { lifecycle, ..edge },
                ),
                "control, predicate identity/provenance, branch, and lifecycle kind are behavior-bearing"
            );
        }
        assert_ne!(
            component,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    has_rest: true,
                    ..edge
                },
            ),
            "struct-rest presence is behavior-bearing"
        );
        let renamed_fields = [TransportDirectComponentField {
            name: "other_label",
            binding: fields[0].binding,
        }];
        assert_ne!(
            component,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &renamed_fields,
                    ..edge
                },
            ),
            "field names are behavior-bearing"
        );
        let callback_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::LiveCallback {
                handler_prelude_local: 3,
                form: TransportDirectLiveCallbackForm::Handler,
            },
        }];
        let callback = component_source_edge_id(
            "sha256:fixture",
            7,
            &TransportDirectComponentEdge {
                fields: &callback_fields,
                ..edge
            },
        );
        let optional_callback_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::LiveCallback {
                handler_prelude_local: 3,
                form: TransportDirectLiveCallbackForm::SomeHandler,
            },
        }];
        assert_ne!(
            callback,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &optional_callback_fields,
                    ..edge
                },
            ),
            "required versus optional live callback form is behavior-bearing"
        );
        let other_callback_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::LiveCallback {
                handler_prelude_local: 4,
                form: TransportDirectLiveCallbackForm::Handler,
            },
        }];
        assert_ne!(
            callback,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &other_callback_fields,
                    ..edge
                },
            ),
            "the exact callback source binding is behavior-bearing"
        );
        let node_ref_fields = [TransportDirectComponentField {
            name: "target",
            binding: TransportDirectComponentBinding::NodeRefHandle { node_ref: 3 },
        }];
        let node_ref = component_source_edge_id(
            "sha256:fixture",
            7,
            &TransportDirectComponentEdge {
                fields: &node_ref_fields,
                ..edge
            },
        );
        let other_node_ref_fields = [TransportDirectComponentField {
            name: "target",
            binding: TransportDirectComponentBinding::NodeRefHandle { node_ref: 4 },
        }];
        assert_ne!(
            node_ref,
            component_source_edge_id(
                "sha256:fixture",
                7,
                &TransportDirectComponentEdge {
                    fields: &other_node_ref_fields,
                    ..edge
                },
            ),
            "the exact parent node-ref identity is behavior-bearing"
        );

        let helper = helper_direct_call_source_edge_id([0x11; 32], 7);
        assert_ne!(helper, helper_direct_call_source_edge_id([0x11; 32], 8));
        assert_ne!(helper, helper_direct_call_source_edge_id([0x12; 32], 7));
    }

    #[test]
    fn shared_state_provider_source_edges_bind_exact_static_topology() {
        let child_fields = [TransportDirectComponentField {
            name: "label",
            binding: TransportDirectComponentBinding::ServerOnly,
        }];
        let child = TransportDirectComponentEdge {
            boundary: TransportDirectComponentBoundary::Composed,
            site_node: 11,
            lifecycle: TransportDirectComponentLifecycle::StaticSingle,
            has_rest: false,
            fields: &child_fields,
        };
        let child_source_edge_id = component_source_edge_id("sha256:provider", 3, &child);
        let site = TransportSharedStateProviderSite {
            site_node: 11,
            child_edge_ordinal: 3,
            child_source_edge_id,
        };
        let provider = shared_state_provider_source_edge_id("sha256:provider", 2, &site);

        assert_eq!(
            provider,
            shared_state_provider_source_edge_id("sha256:provider", 2, &site),
            "provider source-edge derivation must be deterministic"
        );
        assert_ne!(
            provider,
            shared_state_provider_source_edge_id("sha256:other", 2, &site),
            "the defining component fingerprint scopes provider topology"
        );
        assert_ne!(
            provider,
            shared_state_provider_source_edge_id("sha256:provider", 1, &site),
            "the dense provider-site ordinal is behavior-bearing"
        );
        assert_ne!(
            provider,
            shared_state_provider_source_edge_id(
                "sha256:provider",
                2,
                &TransportSharedStateProviderSite {
                    site_node: 12,
                    ..site
                },
            ),
            "the whole provider-expression server node is behavior-bearing"
        );
        assert_ne!(
            provider,
            shared_state_provider_source_edge_id(
                "sha256:provider",
                2,
                &TransportSharedStateProviderSite {
                    child_edge_ordinal: 4,
                    ..site
                },
            ),
            "the topology-only child edge ordinal is behavior-bearing"
        );
        let other_child_source_edge_id = component_source_edge_id("sha256:provider", 4, &child);
        assert_ne!(
            provider,
            shared_state_provider_source_edge_id(
                "sha256:provider",
                2,
                &TransportSharedStateProviderSite {
                    child_source_edge_id: other_child_source_edge_id,
                    ..site
                },
            ),
            "the authenticated child component relation is behavior-bearing"
        );
        assert_ne!(
            provider,
            component_source_edge_id("sha256:provider", 2, &child),
            "provider topology and component composition are separate domains"
        );
        assert_ne!(
            provider,
            component_helper_source_edge_id("sha256:provider", 2),
            "provider topology and helper calls are separate domains"
        );
    }

    #[test]
    fn inline_payload_is_deterministic_and_self_describing() {
        let payload = br#"{"format":"fixture","value":7}"#;
        let encoded = encode_payload_record(
            PayloadKind::ComponentDescriptor,
            [0x11; 32],
            [0x22; 32],
            payload,
        );

        assert_eq!(&encoded[..8], &PAYLOAD_FORMAT_MAGIC);
        assert_eq!(
            u16::from_le_bytes(encoded[8..10].try_into().unwrap()),
            PAYLOAD_FORMAT_VERSION
        );
        assert_eq!(
            u16::from_le_bytes(encoded[10..12].try_into().unwrap()),
            PayloadKind::ComponentDescriptor as u16
        );
        assert_eq!(
            u16::from_le_bytes(encoded[12..14].try_into().unwrap()) as usize,
            PAYLOAD_HEADER_SIZE
        );
        assert_eq!(
            u64::from_le_bytes(encoded[16..24].try_into().unwrap()) as usize,
            PAYLOAD_HEADER_SIZE + payload.len()
        );
        assert_eq!(
            u64::from_le_bytes(encoded[24..32].try_into().unwrap()) as usize,
            payload.len()
        );
        assert_eq!(&encoded[32..64], &[0x11; 32]);
        assert_eq!(&encoded[64..96], &[0x22; 32]);
        assert_eq!(
            &encoded[96..128],
            &semantic_payload_hash(PayloadKind::ComponentDescriptor, payload)
        );
        assert_eq!(&encoded[PAYLOAD_HEADER_SIZE..], payload);
        assert_eq!(
            encoded,
            encode_payload_record(
                PayloadKind::ComponentDescriptor,
                [0x11; 32],
                [0x22; 32],
                payload,
            )
        );
        assert_eq!(core::mem::offset_of!(PayloadRecord<3>, bytes), 0);
        assert_eq!(core::mem::align_of::<PayloadRecord<3>>(), 8);
        assert_eq!(core::mem::size_of::<PayloadRecord<3>>(), 8);
    }

    #[test]
    fn semantic_identity_is_inline_and_target_independent() {
        const TEXT: &str = "acme-web\0acme_web\0acme_web::ui\0StatusBadge";
        let record = SemanticIdentityRecord::<{ TEXT.len() }>::new(
            SemanticIdentityKind::Component,
            [0x11; 32],
            [0x22; 32],
            encode_semantic_identity::<{ TEXT.len() }>(TEXT),
        );

        assert_eq!(record.header.magic, SEMANTIC_IDENTITY_FORMAT_MAGIC);
        assert_eq!(
            record.header.format_version(),
            SEMANTIC_IDENTITY_FORMAT_VERSION
        );
        assert_eq!(
            record.header.identity_kind_raw(),
            SemanticIdentityKind::Component as u16
        );
        assert_eq!(
            record.header.header_size() as usize,
            SEMANTIC_IDENTITY_HEADER_SIZE
        );
        assert_eq!(
            record.header.record_size() as usize,
            SEMANTIC_IDENTITY_HEADER_SIZE + TEXT.len()
        );
        assert_eq!(record.header.identity_size() as usize, TEXT.len());
        assert_eq!(record.header.cargo_unit_id, [0x11; 32]);
        assert_eq!(record.header.definition_record_id, [0x22; 32]);
        assert_eq!(&record.identity, TEXT.as_bytes());
        assert_eq!(core::mem::align_of_val(&record), 8);
    }
}
