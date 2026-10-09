// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Versioned public data model for the staged RSJS compiler ABI.
//!
//! This module intentionally contains no proc-macro or linker integration.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

/// Byte width of every content-addressed compiler ABI identifier.
pub const HASH_BYTES: usize = 32;

/// Magic prefix for a canonical RSJS descriptor envelope.
pub const DESCRIPTOR_MAGIC: [u8; 8] = *b"RSJSABI\0";

const CANONICAL_HASH_DOMAIN: &[u8] = b"proa.rsjs.compiler-abi.canonical.v1\0";

/// A value with a deterministic compiler-ABI representation.
///
/// Implementations must encode every behavior-affecting field and must not
/// encode process-local state, host paths, or timestamps into semantic values.
pub trait CanonicalEncode {
    /// Append the value's canonical representation.
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder);
}

/// Writer for the self-delimiting canonical compiler-ABI encoding.
#[derive(Debug, Default)]
pub struct CanonicalEncoder {
    bytes: Vec<u8>,
}

impl CanonicalEncoder {
    /// Create an empty encoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finish the encoding.
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// Start a fixed-schema record.
    pub fn record(&mut self, name: &str, field_count: u16) {
        self.raw_u8(0xf0);
        self.raw_string(name);
        self.raw_u16(field_count);
    }

    /// Append one numbered record field.
    pub fn field<T: CanonicalEncode + ?Sized>(&mut self, number: u16, value: &T) {
        self.raw_u8(0xf1);
        self.raw_u16(number);
        self.raw_prefixed(&canonical_bytes(value));
    }

    /// Start a tagged enum variant.
    pub fn variant(&mut self, name: &str, discriminant: u16, field_count: u16) {
        self.raw_u8(0xe0);
        self.raw_string(name);
        self.raw_u16(discriminant);
        self.raw_u16(field_count);
    }

    fn raw_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn raw_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn raw_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn raw_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn raw_prefixed(&mut self, value: &[u8]) {
        self.raw_u64(value.len() as u64);
        self.bytes.extend_from_slice(value);
    }

    fn raw_string(&mut self, value: &str) {
        self.raw_prefixed(value.as_bytes());
    }
}

/// Encode one ABI value into deterministic bytes.
pub fn canonical_bytes<T: CanonicalEncode + ?Sized>(value: &T) -> Vec<u8> {
    let mut encoder = CanonicalEncoder::new();
    value.encode_canonical(&mut encoder);
    encoder.finish()
}

impl<T: CanonicalEncode + ?Sized> CanonicalEncode for &T {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        (*self).encode_canonical(encoder);
    }
}

macro_rules! impl_canonical_uint {
    ($ty:ty, $tag:expr, $write:ident) => {
        impl CanonicalEncode for $ty {
            fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
                encoder.raw_u8($tag);
                encoder.$write(*self);
            }
        }
    };
}

impl_canonical_uint!(u8, 0x01, raw_u8);
impl_canonical_uint!(u16, 0x02, raw_u16);
impl_canonical_uint!(u32, 0x03, raw_u32);
impl_canonical_uint!(u64, 0x04, raw_u64);

impl CanonicalEncode for bool {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.raw_u8(0x05);
        encoder.raw_u8(u8::from(*self));
    }
}

impl CanonicalEncode for str {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.raw_u8(0x06);
        encoder.raw_string(self);
    }
}

impl CanonicalEncode for String {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        self.as_str().encode_canonical(encoder);
    }
}

impl<const N: usize> CanonicalEncode for [u8; N] {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.raw_u8(0x07);
        encoder.raw_prefixed(self);
    }
}

impl<T: CanonicalEncode> CanonicalEncode for Option<T> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.raw_u8(0x08);
        match self {
            Some(value) => {
                encoder.raw_u8(1);
                encoder.raw_prefixed(&canonical_bytes(value));
            }
            None => encoder.raw_u8(0),
        }
    }
}

impl<T: CanonicalEncode> CanonicalEncode for Vec<T> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.raw_u8(0x09);
        encoder.raw_u64(self.len() as u64);
        for value in self {
            encoder.raw_prefixed(&canonical_bytes(value));
        }
    }
}

impl<T: CanonicalEncode + Ord> CanonicalEncode for BTreeSet<T> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let mut entries = self.iter().map(canonical_bytes).collect::<Vec<_>>();
        entries.sort();

        encoder.raw_u8(0x0a);
        encoder.raw_u64(entries.len() as u64);
        for entry in entries {
            encoder.raw_prefixed(&entry);
        }
    }
}

impl<K, V> CanonicalEncode for BTreeMap<K, V>
where
    K: CanonicalEncode + Ord,
    V: CanonicalEncode,
{
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let mut entries = self
            .iter()
            .map(|(key, value)| (canonical_bytes(key), canonical_bytes(value)))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        encoder.raw_u8(0x0b);
        encoder.raw_u64(entries.len() as u64);
        for (key, value) in entries {
            encoder.raw_prefixed(&key);
            encoder.raw_prefixed(&value);
        }
    }
}

fn digest_canonical<T: CanonicalEncode + ?Sized>(domain: &[u8], value: &T) -> [u8; HASH_BYTES] {
    let bytes = canonical_bytes(value);
    let mut hasher = Sha256::new();
    hasher.update(CANONICAL_HASH_DOMAIN);
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}

fn fmt_digest(bytes: &[u8; HASH_BYTES], formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

macro_rules! define_digest_id {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; HASH_BYTES]);

        impl $name {
            /// Construct an identifier from an already validated digest.
            pub const fn from_bytes(bytes: [u8; HASH_BYTES]) -> Self {
                Self(bytes)
            }

            /// Return the digest bytes.
            pub const fn as_bytes(&self) -> &[u8; HASH_BYTES] {
                &self.0
            }

            /// Derive a domain-separated identifier from canonical input.
            pub fn derive<T: CanonicalEncode + ?Sized>(value: &T) -> Self {
                Self(digest_canonical($domain, value))
            }
        }

        impl CanonicalEncode for $name {
            fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
                encoder.record(stringify!($name), 1);
                encoder.field(1, &self.0);
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt_digest(&self.0, formatter)
            }
        }
    };
}

define_digest_id!(
    /// Identity of one pre-Cargo orchestrator invocation.
    InvocationId,
    b"invocation-id.v1"
);
define_digest_id!(
    /// Identity of one realized reusable descriptor-pass Cargo unit.
    CargoUnitId,
    b"cargo-unit-id.v1"
);
define_digest_id!(
    /// Identity of one realized root target's external link-unit manifest.
    LinkUnitId,
    b"link-unit-id.v1"
);
define_digest_id!(
    /// Content identity of one linked root plan.
    ///
    /// Build IDs are deliberately strong types:
    ///
    /// ```compile_fail
    /// use rsjs_ir::abi::{LinkUnitId, PlanId};
    ///
    /// fn consume_plan(_: PlanId) {}
    /// consume_plan(LinkUnitId::from_bytes([0; 32]));
    /// ```
    PlanId,
    b"plan-id.v1"
);
define_digest_id!(
    /// Identity of one plan-sensitive final compilation unit.
    FinalUnitId,
    b"final-unit-id.v1"
);
define_digest_id!(
    /// Hash of behavior-affecting descriptor content.
    SemanticHash,
    b"semantic-hash.v1"
);
define_digest_id!(
    /// Hash of optional diagnostic/source material.
    DebugHash,
    b"debug-hash.v1"
);

/// Hash a canonical value as behavior-affecting semantic content.
pub fn canonical_semantic_hash<T: CanonicalEncode + ?Sized>(value: &T) -> SemanticHash {
    SemanticHash::derive(value)
}

#[derive(Debug)]
struct LegacyJsonHashMaterial {
    domain: String,
    bytes: Vec<u8>,
}

/// Canonical-JSON compatibility domain for standalone-helper signatures.
pub const HELPER_SIGNATURE_JSON_HASH_DOMAIN: &str = "helper-signature.v1";
/// Canonical-JSON compatibility domain for transported standalone-helper IR.
pub const HELPER_CLIENT_IR_JSON_HASH_DOMAIN: &str = "helper-client-ir.v3";
/// Canonical-JSON compatibility domain for relocation-resolved helper codegen.
pub const HELPER_CODEGEN_IR_JSON_HASH_DOMAIN: &str = "helper-codegen-ir.v1";

impl CanonicalEncode for LegacyJsonHashMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        // This record name and field layout are a compatibility contract with
        // descriptor hashes produced before canonical JSON moved behind the
        // shared compiler ABI boundary.
        encoder.record("LegacyJsonHashMaterial", 2);
        encoder.field(1, &self.domain);
        encoder.field(2, &self.bytes);
    }
}

/// Hash already-canonical legacy JSON bytes using the shared semantic-hash
/// compatibility material.
///
/// This function intentionally does not parse or canonicalize JSON: callers
/// must first obtain bytes from the transport layer's canonical JSON writer.
/// Keeping only the byte-level hash contract here avoids an `rsjs_ir` -> JSON
/// transport dependency while ensuring all compiler phases use the same
/// record name, field numbering, and domain separation.
pub fn canonical_legacy_json_bytes_hash(domain: &str, canonical_json: &[u8]) -> SemanticHash {
    SemanticHash::derive(&LegacyJsonHashMaterial {
        domain: domain.to_owned(),
        bytes: canonical_json.to_vec(),
    })
}

#[derive(Debug)]
struct FinalUnitMaterial {
    unit_fingerprint: SemanticHash,
    plan_id: PlanId,
}

impl CanonicalEncode for FinalUnitMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("FinalUnitMaterial", 2);
        encoder.field(1, &self.unit_fingerprint);
        encoder.field(2, &self.plan_id);
    }
}

impl FinalUnitId {
    /// Derive `H(semantic unit fingerprint, PlanId)`.
    pub fn for_plan(unit_fingerprint: SemanticHash, plan_id: PlanId) -> Self {
        Self::derive(&FinalUnitMaterial {
            unit_fingerprint,
            plan_id,
        })
    }
}

macro_rules! define_local_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Construct a local canonical identifier.
            pub const fn new(index: u32) -> Self {
                Self(index)
            }

            /// Return its dense local index.
            pub const fn index(self) -> u32 {
                self.0
            }
        }

        impl CanonicalEncode for $name {
            fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
                encoder.record(stringify!($name), 1);
                encoder.field(1, &self.0);
            }
        }
    };
}

define_local_id!(
    /// Local value node in one symbolic descriptor graph.
    LocalValueId
);
define_local_id!(
    /// Local effect node in one symbolic descriptor graph.
    LocalEffectId
);
define_local_id!(
    /// Local control region in one symbolic descriptor graph.
    LocalControlRegionId
);
define_local_id!(
    /// Local typed dependency edge.
    DependencyEdgeId
);
define_local_id!(
    /// Direct component edge site.
    ComponentEdgeId
);
define_local_id!(
    /// Direct client-helper call edge site.
    HelperCallEdgeId
);
define_local_id!(
    /// Local prop-use site.
    PropSiteId
);
define_local_id!(
    /// Stable field identity inside one component descriptor.
    ComponentFieldId
);
define_local_id!(
    /// Compiler-generated hook used to cross a component field boundary.
    CrossingHookId
);
define_local_id!(
    /// Component-local SharedState schema acquisition.
    SharedStateRefId
);
define_local_id!(
    /// Compiler-recognized lexical SharedState provider site.
    ProviderSiteId
);
define_local_id!(
    /// Local linked owner site.
    OwnerSiteId
);
define_local_id!(
    /// Local unknown-span region site.
    RegionSiteId
);
define_local_id!(
    /// Local keyed-collection site.
    KeyedCollectionSiteId
);
define_local_id!(
    /// Stable source provenance identity.
    OriginId
);
define_local_id!(
    /// Build-local defining or edge witness record identity.
    WitnessRecordId
);

/// Major/minor version used for schema and protocol compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbiVersion {
    /// Breaking format generation.
    pub major: u16,
    /// Backward-compatible additive generation.
    pub minor: u16,
}

impl AbiVersion {
    /// Construct a schema/protocol version.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    fn is_supported_by(self, supported: Self) -> bool {
        self.major == supported.major && self.minor <= supported.minor
    }
}

impl CanonicalEncode for AbiVersion {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("AbiVersion", 2);
        encoder.field(1, &self.major);
        encoder.field(2, &self.minor);
    }
}

/// Full producer tool version carried for deterministic stale-build checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProducerVersion {
    /// Major tool release.
    pub major: u16,
    /// Minor tool release.
    pub minor: u16,
    /// Patch tool release.
    pub patch: u16,
}

impl ProducerVersion {
    /// Construct a producer version.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl CanonicalEncode for ProducerVersion {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ProducerVersion", 3);
        encoder.field(1, &self.major);
        encoder.field(2, &self.minor);
        encoder.field(3, &self.patch);
    }
}

/// Inclusive runtime protocol compatibility range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionRange {
    /// Oldest accepted protocol.
    pub min: AbiVersion,
    /// Newest accepted protocol.
    pub max: AbiVersion,
}

impl VersionRange {
    /// Construct an inclusive range.
    pub const fn new(min: AbiVersion, max: AbiVersion) -> Self {
        Self { min, max }
    }

    /// Construct a range accepting exactly one version.
    pub const fn exact(version: AbiVersion) -> Self {
        Self {
            min: version,
            max: version,
        }
    }

    /// Whether a version lies inside this inclusive range.
    pub const fn contains(self, version: AbiVersion) -> bool {
        (self.min.major < version.major
            || (self.min.major == version.major && self.min.minor <= version.minor))
            && (version.major < self.max.major
                || (version.major == self.max.major && version.minor <= self.max.minor))
    }

    fn is_valid(self) -> bool {
        self.min <= self.max
    }
}

impl CanonicalEncode for VersionRange {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("VersionRange", 2);
        encoder.field(1, &self.min);
        encoder.field(2, &self.max);
    }
}

/// Advertised descriptor/runtime capability bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilitySet(u64);

impl CapabilitySet {
    /// No optional capabilities.
    pub const NONE: Self = Self(0);
    /// Tolerant symbolic server graph support.
    pub const TOLERANT_SERVER_GRAPH: Self = Self(1 << 0);
    /// Classified dependency-edge graph support.
    pub const TYPED_DEPENDENCIES: Self = Self(1 << 1);
    /// Temporal prop modes.
    pub const TEMPORAL_PROPS: Self = Self(1 << 2);
    /// Origin and browser-effect taxonomy.
    pub const ORIGIN_EFFECT_TAXONOMY: Self = Self(1 << 3);
    /// Deterministic owner-partition inputs.
    pub const OWNER_PARTITION_INPUTS: Self = Self(1 << 4);
    /// Composable DOM span cardinality.
    pub const DOM_SPAN_CARDINALITY: Self = Self(1 << 5);
    /// Origin-free standalone helper descriptors.
    pub const ORIGIN_FREE_HELPERS: Self = Self(1 << 6);
    /// Typed component fields and their client crossing hooks.
    pub const COMPONENT_FIELD_SCHEMA: Self = Self(1 << 7);
    /// Direct component-edge field bindings.
    pub const DIRECT_COMPONENT_BINDINGS: Self = Self(1 << 8);
    /// Immutable server-slot contracts across client ownership boundaries.
    pub const STATIC_SLOT_CONTRACTS: Self = Self(1 << 9);
    /// Conditional-single direct component lifecycle metadata.
    pub const CONDITIONAL_COMPONENT_LIFECYCLE: Self = Self(1 << 10);
    /// Lexical SharedState provider topology.
    pub const SHARED_STATE_PROVIDER_TOPOLOGY: Self = Self(1 << 11);

    /// Construct from raw versioned bits.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Return the raw versioned bits.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Union two capability sets.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit in `required` is present.
    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    /// Bits required by `self` but missing from `supported`.
    pub const fn missing_from(self, supported: Self) -> Self {
        Self(self.0 & !supported.0)
    }
}

impl CanonicalEncode for CapabilitySet {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("CapabilitySet", 1);
        encoder.field(1, &self.0);
    }
}

/// Complete set of descriptor capabilities defined by schema v1.
pub const DESCRIPTOR_CAPABILITIES_V1: CapabilitySet = CapabilitySet::TOLERANT_SERVER_GRAPH
    .union(CapabilitySet::TYPED_DEPENDENCIES)
    .union(CapabilitySet::TEMPORAL_PROPS)
    .union(CapabilitySet::ORIGIN_EFFECT_TAXONOMY)
    .union(CapabilitySet::OWNER_PARTITION_INPUTS)
    .union(CapabilitySet::DOM_SPAN_CARDINALITY)
    .union(CapabilitySet::ORIGIN_FREE_HELPERS)
    .union(CapabilitySet::COMPONENT_FIELD_SCHEMA)
    .union(CapabilitySet::DIRECT_COMPONENT_BINDINGS)
    .union(CapabilitySet::STATIC_SLOT_CONTRACTS)
    .union(CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE)
    .union(CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY);

/// Runtime features required by the initial descriptor model.
pub const RUNTIME_CAPABILITIES_V1: CapabilitySet = CapabilitySet::TEMPORAL_PROPS
    .union(CapabilitySet::OWNER_PARTITION_INPUTS)
    .union(CapabilitySet::DOM_SPAN_CARDINALITY);

/// Frozen descriptor schema v1.
pub const DESCRIPTOR_SCHEMA_V1: AbiVersion = AbiVersion::new(1, 0);
/// Serialized Client IR generation used by raw component and helper payloads.
pub const CLIENT_IR_SCHEMA_VERSION: u32 = 3;
/// Client IR v3 ABI generation.
pub const CLIENT_IR_V3: AbiVersion = AbiVersion::new(3, 0);
/// Component semantic-key schema v1.
pub const COMPONENT_KEY_SCHEMA_V1: AbiVersion = AbiVersion::new(1, 0);
/// Helper semantic-key schema v1.
pub const HELPER_KEY_SCHEMA_V1: AbiVersion = AbiVersion::new(1, 0);
/// Analyzed-method extraction ABI v1.
pub const METHOD_EXTRACTION_ABI_V1: AbiVersion = AbiVersion::new(1, 0);
/// Site-registration ABI v1.
pub const SITE_REGISTRATION_ABI_V1: AbiVersion = AbiVersion::new(1, 0);
/// Standalone helper signature ABI v1.
pub const HELPER_SIGNATURE_ABI_V1: AbiVersion = AbiVersion::new(1, 0);
/// Client value codec ABI v1.
pub const CLIENT_CODEC_ABI_V1: AbiVersion = AbiVersion::new(1, 0);

/// Versioned ABI surface named by a compatibility diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AbiVersionField {
    /// Descriptor envelope schema.
    DescriptorSchema,
    /// Client IR schema.
    ClientIr,
    /// Component semantic-key schema.
    ComponentKeySchema,
    /// Helper semantic-key schema.
    HelperKeySchema,
    /// Render-method extraction ABI.
    MethodExtraction,
    /// Site-registration ABI.
    SiteRegistration,
    /// Standalone helper signature ABI.
    HelperSignature,
    /// Client codec ABI.
    ClientCodec,
}

/// Validation failure for a canonical ABI descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AbiValidationError {
    /// Envelope magic is not the frozen RSJS ABI magic.
    InvalidMagic {
        /// Bytes that were observed.
        actual: [u8; 8],
    },
    /// An inclusive protocol range has its bounds reversed.
    InvalidVersionRange {
        /// Invalid lower bound.
        min: AbiVersion,
        /// Invalid upper bound.
        max: AbiVersion,
    },
    /// A produced ABI generation is newer or from a different major.
    UnsupportedVersion {
        /// Surface being validated.
        field: AbiVersionField,
        /// Version carried by the descriptor.
        produced: AbiVersion,
        /// Newest compatible version understood by the consumer.
        supported: AbiVersion,
    },
    /// Descriptor capabilities are not understood by the consumer.
    UnsupportedDescriptorCapabilities {
        /// Unsupported required bits.
        missing: CapabilitySet,
    },
    /// Runtime capabilities are not available.
    UnsupportedRuntimeCapabilities {
        /// Unsupported required bits.
        missing: CapabilitySet,
    },
    /// Descriptor content uses capabilities omitted from its own header.
    UnderdeclaredDescriptorCapabilities {
        /// Body-required bits absent from `header.capabilities`.
        missing: CapabilitySet,
    },
    /// Descriptor content needs runtime behavior omitted from its own header.
    UnderdeclaredRuntimeCapabilities {
        /// Body-required bits absent from `header.runtime.required_capabilities`.
        missing: CapabilitySet,
    },
    /// The selected runtime protocol is outside the descriptor's range.
    RuntimeProtocolIncompatible {
        /// Descriptor's required inclusive range.
        required: VersionRange,
        /// Runtime protocol selected by the consumer.
        actual: AbiVersion,
    },
    /// Header kind and body kind disagree.
    DescriptorKindMismatch {
        /// Kind declared by the header.
        header: DescriptorKind,
        /// Kind represented by the body.
        body: DescriptorKind,
    },
    /// A normalized semantic-key namespace contains an invalid field.
    InvalidSemanticKey {
        /// Field name.
        field: &'static str,
        /// Invalid field value.
        value: String,
    },
    /// A component field name is not a normalized source identifier.
    InvalidComponentFieldName {
        /// Stable field ID.
        field: ComponentFieldId,
        /// Invalid source name.
        name: String,
    },
    /// Two component fields claim the same source name.
    DuplicateComponentFieldName {
        /// Duplicate source name.
        name: String,
    },
    /// A direct edge contains an invalid child-field selector.
    InvalidComponentEdgeFieldName {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Invalid source name.
        name: String,
    },
    /// A client-visible field has no generated crossing hook.
    MissingCrossingHook {
        /// Field missing the hook.
        field: ComponentFieldId,
    },
    /// A field refers to a hook that is not declared by the component.
    UnknownCrossingHook {
        /// Field using the hook.
        field: ComponentFieldId,
        /// Missing hook.
        hook: CrossingHookId,
    },
    /// A server-only field unexpectedly exposes a client crossing hook.
    UnexpectedCrossingHook {
        /// Server-only field.
        field: ComponentFieldId,
        /// Unexpected hook.
        hook: CrossingHookId,
    },
    /// A client-visible field has no concrete Rust type witness.
    MissingComponentFieldTypeWitness {
        /// Field missing its concrete type/signature witness.
        field: ComponentFieldId,
    },
    /// A value/callback/handle field has no child-local symbolic value.
    MissingComponentFieldClientValue {
        /// Field missing the graph value.
        field: ComponentFieldId,
    },
    /// A field that does not participate as a client value unexpectedly has one.
    UnexpectedComponentFieldClientValue {
        /// Field carrying the unexpected graph value.
        field: ComponentFieldId,
        /// Unexpected child-local symbolic value.
        value: LocalValueId,
    },
    /// A callback field omits its required/optional Rust container shape.
    MissingComponentCallbackShape {
        /// Callback field missing the shape.
        field: ComponentFieldId,
    },
    /// A non-callback field unexpectedly declares callback shape.
    UnexpectedComponentCallbackShape {
        /// Non-callback field carrying the shape.
        field: ComponentFieldId,
        /// Unexpected callback shape.
        shape: ComponentCallbackShape,
    },
    /// A field's child-local symbolic value is absent from the descriptor.
    UnknownComponentFieldClientValue {
        /// Field carrying the reference.
        field: ComponentFieldId,
        /// Missing child-local symbolic value.
        value: LocalValueId,
    },
    /// A value prop use disagrees with its field's canonical child value.
    PropUseFieldValueMismatch {
        /// Field selected by the prop use.
        field: ComponentFieldId,
        /// Canonical field value.
        field_value: LocalValueId,
        /// Value named by the prop use.
        prop_value: LocalValueId,
    },
    /// A prop-use site refers to a field absent from the descriptor.
    UnknownComponentField {
        /// Missing field.
        field: ComponentFieldId,
    },
    /// Only value fields may have temporal prop-use sites.
    PropUseOnNonValueField {
        /// Field used as a temporal prop.
        field: ComponentFieldId,
        /// Declared non-value role.
        role: ComponentFieldRole,
    },
    /// A `SeedOnce` use refers to a different prop-use site.
    InvalidSeedOnceSite {
        /// Site being validated.
        site: PropSiteId,
        /// Site named by `SeedOnce`.
        referenced: PropSiteId,
    },
    /// Only slot fields may advertise immutable server-slot support.
    StaticSlotOnNonSlotField {
        /// Invalid field.
        field: ComponentFieldId,
    },
    /// Callback and slot fields cannot declare a value codec.
    UnexpectedFieldCodec {
        /// Invalid field.
        field: ComponentFieldId,
    },
    /// A live value, callback, handle, or slot crosses an explicit island.
    LiveBindingAcrossIndependentIsland {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Child field name.
        field_name: String,
    },
    /// A SharedState provider topology record refers to a missing child edge.
    UnknownSharedStateProviderChildEdge {
        /// Provider topology site.
        site: ProviderSiteId,
        /// Missing direct child edge.
        child_edge: ComponentEdgeId,
    },
    /// A SharedState provider's topology-only child edge crosses an owner cut.
    SharedStateProviderChildMustBeComposed {
        /// Provider topology site.
        site: ProviderSiteId,
        /// Independent direct child edge.
        child_edge: ComponentEdgeId,
    },
    /// The initial provider topology ABI cannot represent dynamic providers.
    DynamicSharedStateProviderUnsupported {
        /// Provider topology site.
        site: ProviderSiteId,
        /// Conditional direct child edge.
        child_edge: ComponentEdgeId,
    },
    /// A provider topology-only child edge contains an owner-coupling binding.
    CoupledSharedStateProviderBinding {
        /// Provider topology site.
        site: ProviderSiteId,
        /// Provider child edge.
        child_edge: ComponentEdgeId,
        /// Child field whose binding requires owner coupling.
        field_name: String,
    },
    /// More than one provider topology record claims one direct child edge.
    DuplicateSharedStateProviderChildEdge {
        /// First provider site claiming the edge.
        first: ProviderSiteId,
        /// Later provider site claiming the edge.
        duplicate: ProviderSiteId,
        /// Direct child edge claimed by both sites.
        child_edge: ComponentEdgeId,
    },
    /// A component lifecycle control is unrelated to a structural DOM effect.
    LifecycleControlWithoutStructuralEffect {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed parent control.
        control: LocalControlRegionId,
    },
    /// A conditional component edge omits its control-to-component relation.
    MissingComponentLifecycleDependency {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed parent control.
        control: LocalControlRegionId,
    },
    /// Control-to-component dependencies disagree with lifecycle metadata.
    ComponentLifecycleDependencyMismatch {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed lifecycle control, or `None` for a static-single edge.
        expected_control: Option<LocalControlRegionId>,
        /// Source provenance required by the direct edge.
        expected_origin: OriginId,
        /// Actual control dependency sources and provenances.
        actual: Vec<(DependencyEndpoint, OriginId)>,
    },
    /// Predicate-to-control dependencies disagree with lifecycle metadata.
    LifecyclePredicateDependencyMismatch {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed lifecycle control.
        control: LocalControlRegionId,
        /// Canonical predicate value retained by the lifecycle record.
        expected_predicate: LocalValueId,
        /// Canonical provenance retained for the predicate dependency.
        expected_origin: OriginId,
        /// Every dependency entering the claimed control.
        actual: Vec<(
            DependencyEdgeId,
            DependencyEdgeKind,
            DependencyEndpoint,
            OriginId,
        )>,
    },
    /// A lifecycle predicate depends on a relation that cannot participate in
    /// the canonical browser value graph.
    UnsupportedLifecyclePredicateDependency {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed lifecycle control.
        control: LocalControlRegionId,
        /// Dependency that introduced the unsupported provenance path.
        dependency: DependencyEdgeId,
        /// Unsupported dependency class.
        kind: DependencyEdgeKind,
        /// Unsupported dependency source.
        from: DependencyEndpoint,
        /// Dependency target reached while traversing the predicate.
        to: DependencyEndpoint,
    },
    /// A lifecycle predicate reaches a local value/effect/component endpoint
    /// with no canonical producer.
    UnresolvedLifecyclePredicateSource {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed lifecycle control.
        control: LocalControlRegionId,
        /// Unresolved upstream endpoint.
        endpoint: DependencyEndpoint,
    },
    /// A lifecycle predicate has a closed, transportable graph but no local
    /// browser origin or live inbound value.
    LifecyclePredicateWithoutBrowserOrigin {
        /// Direct component edge.
        edge: ComponentEdgeId,
        /// Claimed lifecycle control.
        control: LocalControlRegionId,
        /// Canonical lifecycle predicate.
        predicate: LocalValueId,
    },
    /// Participant-only component contains a local client origin.
    OriginRequiresClientPermission {
        /// Origin declaration that requires permission.
        origin: OriginId,
        /// Classified origin kind.
        kind: OriginKind,
    },
    /// A hydration constraint contains an empty event or media query.
    InvalidHydrationConstraint {
        /// Human-readable reason.
        reason: &'static str,
    },
    /// DOM span plan has more than one canonical representation.
    NonCanonicalSpan {
        /// Human-readable reason.
        reason: &'static str,
    },
    /// A local identifier collection is not dense from zero.
    NonDenseLocalIds {
        /// Collection being validated.
        collection: &'static str,
        /// Required next identifier.
        expected: u32,
        /// Identifier that appeared instead.
        actual: u32,
    },
    /// An edge refers to an unknown source provenance ID.
    UnknownOrigin {
        /// Referenced origin.
        origin: OriginId,
    },
    /// A typed acquisition record points at an origin of the wrong class.
    OriginKindMismatch {
        /// Referenced origin.
        origin: OriginId,
        /// Origin class required by the acquisition record.
        expected: OriginKind,
        /// Origin class actually declared by the component.
        actual: OriginKind,
    },
    /// An edge endpoint refers to a missing local declaration.
    UnknownDependencyEndpoint {
        /// Missing endpoint.
        endpoint: DependencyEndpoint,
    },
    /// V1 helper descriptors must remain transitively origin-free.
    EffectfulHelperUnsupported,
    /// Stored semantic hash does not match canonical semantic content.
    SemanticHashMismatch {
        /// Recomputed hash.
        expected: SemanticHash,
        /// Stored hash.
        actual: SemanticHash,
    },
    /// Debug content exists without its hash.
    DebugHashMissing,
    /// A debug hash exists without debug content.
    UnexpectedDebugHash,
    /// Stored debug hash does not match canonical debug content.
    DebugHashMismatch {
        /// Recomputed hash.
        expected: DebugHash,
        /// Stored hash.
        actual: DebugHash,
    },
}

impl fmt::Display for AbiValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic { actual } => {
                write!(formatter, "invalid RSJS descriptor magic: {actual:?}")
            }
            Self::InvalidVersionRange { min, max } => {
                write!(formatter, "invalid protocol range {min:?}..={max:?}")
            }
            Self::UnsupportedVersion {
                field,
                produced,
                supported,
            } => write!(
                formatter,
                "unsupported {field:?} version {produced:?}; consumer supports {supported:?}"
            ),
            Self::UnsupportedDescriptorCapabilities { missing } => write!(
                formatter,
                "unsupported descriptor capability bits 0x{:x}",
                missing.bits()
            ),
            Self::UnsupportedRuntimeCapabilities { missing } => write!(
                formatter,
                "unsupported runtime capability bits 0x{:x}",
                missing.bits()
            ),
            Self::UnderdeclaredDescriptorCapabilities { missing } => write!(
                formatter,
                "descriptor body uses capability bits 0x{:x} omitted from its header",
                missing.bits()
            ),
            Self::UnderdeclaredRuntimeCapabilities { missing } => write!(
                formatter,
                "descriptor body needs runtime capability bits 0x{:x} omitted from its header",
                missing.bits()
            ),
            Self::RuntimeProtocolIncompatible { required, actual } => write!(
                formatter,
                "runtime protocol {actual:?} is outside {required:?}"
            ),
            Self::DescriptorKindMismatch { header, body } => {
                write!(
                    formatter,
                    "descriptor header kind {header:?} does not match body {body:?}"
                )
            }
            Self::InvalidSemanticKey { field, value } => {
                write!(formatter, "invalid semantic-key {field}: {value:?}")
            }
            Self::InvalidComponentFieldName { field, name } => write!(
                formatter,
                "component field {field:?} has invalid source name {name:?}"
            ),
            Self::DuplicateComponentFieldName { name } => {
                write!(formatter, "duplicate component field source name {name:?}")
            }
            Self::InvalidComponentEdgeFieldName { edge, name } => write!(
                formatter,
                "component edge {edge:?} has invalid child field name {name:?}"
            ),
            Self::MissingCrossingHook { field } => {
                write!(
                    formatter,
                    "client-visible component field {field:?} has no crossing hook"
                )
            }
            Self::UnknownCrossingHook { field, hook } => write!(
                formatter,
                "component field {field:?} refers to unknown crossing hook {hook:?}"
            ),
            Self::UnexpectedCrossingHook { field, hook } => write!(
                formatter,
                "server-only component field {field:?} unexpectedly exposes crossing hook {hook:?}"
            ),
            Self::MissingComponentFieldTypeWitness { field } => write!(
                formatter,
                "client-visible component field {field:?} has no concrete Rust type witness"
            ),
            Self::MissingComponentFieldClientValue { field } => write!(
                formatter,
                "client value/callback component field {field:?} has no child-local symbolic value"
            ),
            Self::UnexpectedComponentFieldClientValue { field, value } => write!(
                formatter,
                "component field {field:?} unexpectedly exposes child-local symbolic value {value:?}"
            ),
            Self::MissingComponentCallbackShape { field } => write!(
                formatter,
                "callback component field {field:?} has no required/optional callback shape"
            ),
            Self::UnexpectedComponentCallbackShape { field, shape } => write!(
                formatter,
                "non-callback component field {field:?} unexpectedly declares callback shape {shape:?}"
            ),
            Self::UnknownComponentFieldClientValue { field, value } => write!(
                formatter,
                "component field {field:?} refers to unknown child-local symbolic value {value:?}"
            ),
            Self::PropUseFieldValueMismatch {
                field,
                field_value,
                prop_value,
            } => write!(
                formatter,
                "prop use for component field {field:?} names value {prop_value:?}, but the field exposes {field_value:?}"
            ),
            Self::UnknownComponentField { field } => {
                write!(
                    formatter,
                    "prop use refers to unknown component field {field:?}"
                )
            }
            Self::PropUseOnNonValueField { field, role } => write!(
                formatter,
                "prop use refers to component field {field:?} with non-value role {role:?}"
            ),
            Self::InvalidSeedOnceSite { site, referenced } => write!(
                formatter,
                "prop use {site:?} has SeedOnce reference to different site {referenced:?}"
            ),
            Self::StaticSlotOnNonSlotField { field } => write!(
                formatter,
                "non-slot component field {field:?} advertises a static-slot contract"
            ),
            Self::UnexpectedFieldCodec { field } => write!(
                formatter,
                "callback or slot component field {field:?} unexpectedly declares a value codec"
            ),
            Self::LiveBindingAcrossIndependentIsland { edge, field_name } => write!(
                formatter,
                "component edge {edge:?} passes live field {field_name:?} across an independent island"
            ),
            Self::UnknownSharedStateProviderChildEdge { site, child_edge } => write!(
                formatter,
                "SharedState provider site {site:?} refers to missing child edge {child_edge:?}"
            ),
            Self::SharedStateProviderChildMustBeComposed { site, child_edge } => write!(
                formatter,
                "SharedState provider site {site:?} child edge {child_edge:?} must use composed ownership"
            ),
            Self::DynamicSharedStateProviderUnsupported { site, child_edge } => write!(
                formatter,
                "SharedState provider site {site:?} child edge {child_edge:?} has dynamic lifecycle metadata"
            ),
            Self::CoupledSharedStateProviderBinding {
                site,
                child_edge,
                field_name,
            } => write!(
                formatter,
                "SharedState provider site {site:?} child edge {child_edge:?} has owner-coupling field {field_name:?}"
            ),
            Self::DuplicateSharedStateProviderChildEdge {
                first,
                duplicate,
                child_edge,
            } => write!(
                formatter,
                "SharedState provider sites {first:?} and {duplicate:?} both claim child edge {child_edge:?}"
            ),
            Self::LifecycleControlWithoutStructuralEffect { edge, control } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} has no outgoing DomStructuralUpdate effect"
            ),
            Self::MissingComponentLifecycleDependency { edge, control } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} has no control dependency to the component"
            ),
            Self::ComponentLifecycleDependencyMismatch {
                edge,
                expected_control,
                expected_origin,
                actual,
            } => write!(
                formatter,
                "component edge {edge:?} expected exactly one lifecycle dependency from \
                 {expected_control:?} with origin {expected_origin:?}, found {actual:?}"
            ),
            Self::LifecyclePredicateDependencyMismatch {
                edge,
                control,
                expected_predicate,
                expected_origin,
                actual,
            } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} expected exactly one \
                 predicate dependency from value {expected_predicate:?} with origin \
                 {expected_origin:?}, found {actual:?}"
            ),
            Self::UnsupportedLifecyclePredicateDependency {
                edge,
                control,
                dependency,
                kind,
                from,
                to,
            } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} reaches unsupported \
                 predicate dependency {dependency:?}: {kind:?} {from:?} -> {to:?}"
            ),
            Self::UnresolvedLifecyclePredicateSource {
                edge,
                control,
                endpoint,
            } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} reaches predicate \
                 endpoint {endpoint:?} with no canonical producer"
            ),
            Self::LifecyclePredicateWithoutBrowserOrigin {
                edge,
                control,
                predicate,
            } => write!(
                formatter,
                "component edge {edge:?} lifecycle control {control:?} predicate \
                 {predicate:?} reaches no browser origin or live inbound value"
            ),
            Self::OriginRequiresClientPermission { origin, kind } => write!(
                formatter,
                "origin {origin:?} ({kind:?}) requires origin-capable permission"
            ),
            Self::InvalidHydrationConstraint { reason } => {
                write!(formatter, "invalid hydration constraint: {reason}")
            }
            Self::NonCanonicalSpan { reason } => {
                write!(formatter, "non-canonical DOM span: {reason}")
            }
            Self::NonDenseLocalIds {
                collection,
                expected,
                actual,
            } => write!(
                formatter,
                "{collection} IDs must be dense; expected {expected}, found {actual}"
            ),
            Self::UnknownOrigin { origin } => {
                write!(formatter, "dependency refers to unknown origin {origin:?}")
            }
            Self::OriginKindMismatch {
                origin,
                expected,
                actual,
            } => write!(
                formatter,
                "origin {origin:?} has kind {actual:?}, but this record requires {expected:?}"
            ),
            Self::UnknownDependencyEndpoint { endpoint } => {
                write!(
                    formatter,
                    "dependency refers to unknown endpoint {endpoint:?}"
                )
            }
            Self::EffectfulHelperUnsupported => {
                formatter.write_str("v1 helper descriptors must be OriginFree")
            }
            Self::SemanticHashMismatch { expected, actual } => write!(
                formatter,
                "descriptor semantic hash mismatch: expected {expected}, found {actual}"
            ),
            Self::DebugHashMissing => formatter.write_str("debug content is missing its hash"),
            Self::UnexpectedDebugHash => {
                formatter.write_str("debug hash exists without debug content")
            }
            Self::DebugHashMismatch { expected, actual } => write!(
                formatter,
                "descriptor debug hash mismatch: expected {expected}, found {actual}"
            ),
        }
    }
}

impl std::error::Error for AbiValidationError {}

/// Runtime protocol and feature requirements carried by a descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeCompatibility {
    /// Inclusive supported runtime protocol range.
    pub protocol: VersionRange,
    /// Runtime feature bits required by this descriptor.
    pub required_capabilities: CapabilitySet,
}

impl RuntimeCompatibility {
    /// Construct runtime compatibility requirements.
    pub const fn new(protocol: VersionRange, required_capabilities: CapabilitySet) -> Self {
        Self {
            protocol,
            required_capabilities,
        }
    }

    fn validate_shape(self) -> Result<(), AbiValidationError> {
        if self.protocol.is_valid() {
            Ok(())
        } else {
            Err(AbiValidationError::InvalidVersionRange {
                min: self.protocol.min,
                max: self.protocol.max,
            })
        }
    }
}

impl CanonicalEncode for RuntimeCompatibility {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("RuntimeCompatibility", 2);
        encoder.field(1, &self.protocol);
        encoder.field(2, &self.required_capabilities);
    }
}

/// Top-level descriptor body kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DescriptorKind {
    /// Component descriptor.
    Component,
    /// Standalone `#[rsjs::client]` helper descriptor.
    Helper,
}

impl CanonicalEncode for DescriptorKind {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Component => encoder.variant("DescriptorKind", 0, 0),
            Self::Helper => encoder.variant("DescriptorKind", 1, 0),
        }
    }
}

/// Common fixed header for component and helper descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DescriptorHeader {
    /// Fixed [`DESCRIPTOR_MAGIC`].
    pub magic: [u8; 8],
    /// Envelope schema generation.
    pub descriptor_schema: AbiVersion,
    /// Tool version that produced the descriptor.
    pub producer: ProducerVersion,
    /// Additive descriptor capabilities used by the payload.
    pub capabilities: CapabilitySet,
    /// Client IR generation used by the body.
    pub client_ir: AbiVersion,
    /// Runtime protocol/capability requirements.
    pub runtime: RuntimeCompatibility,
    /// Body kind.
    pub kind: DescriptorKind,
}

impl DescriptorHeader {
    /// Construct a v1 header.
    pub const fn v1(
        kind: DescriptorKind,
        producer: ProducerVersion,
        runtime: RuntimeCompatibility,
    ) -> Self {
        Self {
            magic: DESCRIPTOR_MAGIC,
            descriptor_schema: DESCRIPTOR_SCHEMA_V1,
            producer,
            capabilities: DESCRIPTOR_CAPABILITIES_V1,
            client_ir: CLIENT_IR_V3,
            runtime,
            kind,
        }
    }

    fn validate_shape(&self) -> Result<(), AbiValidationError> {
        if self.magic != DESCRIPTOR_MAGIC {
            return Err(AbiValidationError::InvalidMagic { actual: self.magic });
        }
        self.runtime.validate_shape()
    }

    /// Validate this header against one compiler/runtime support matrix.
    pub fn validate(&self, support: &AbiSupport) -> Result<(), AbiValidationError> {
        self.validate_shape()?;
        validate_version(
            AbiVersionField::DescriptorSchema,
            self.descriptor_schema,
            support.descriptor_schema,
        )?;
        validate_version(AbiVersionField::ClientIr, self.client_ir, support.client_ir)?;

        let missing = self
            .capabilities
            .missing_from(support.descriptor_capabilities);
        if missing != CapabilitySet::NONE {
            return Err(AbiValidationError::UnsupportedDescriptorCapabilities { missing });
        }

        let missing = self
            .runtime
            .required_capabilities
            .missing_from(support.runtime_capabilities);
        if missing != CapabilitySet::NONE {
            return Err(AbiValidationError::UnsupportedRuntimeCapabilities { missing });
        }

        if !self.runtime.protocol.contains(support.runtime_protocol) {
            return Err(AbiValidationError::RuntimeProtocolIncompatible {
                required: self.runtime.protocol,
                actual: support.runtime_protocol,
            });
        }

        Ok(())
    }
}

impl CanonicalEncode for DescriptorHeader {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DescriptorHeader", 7);
        encoder.field(1, &self.magic);
        encoder.field(2, &self.descriptor_schema);
        encoder.field(3, &self.producer);
        encoder.field(4, &self.capabilities);
        encoder.field(5, &self.client_ir);
        encoder.field(6, &self.runtime);
        encoder.field(7, &self.kind);
    }
}

/// Compiler/runtime versions and capabilities understood by a consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AbiSupport {
    /// Supported descriptor schema.
    pub descriptor_schema: AbiVersion,
    /// Supported Client IR schema.
    pub client_ir: AbiVersion,
    /// Supported component semantic-key schema.
    pub component_key_schema: AbiVersion,
    /// Supported helper semantic-key schema.
    pub helper_key_schema: AbiVersion,
    /// Supported render-method extraction ABI.
    pub method_extraction: AbiVersion,
    /// Supported site-registration ABI.
    pub site_registration: AbiVersion,
    /// Supported standalone helper signature ABI.
    pub helper_signature: AbiVersion,
    /// Supported client codec ABI.
    pub client_codec: AbiVersion,
    /// Descriptor capability bits understood by this consumer.
    pub descriptor_capabilities: CapabilitySet,
    /// Selected runtime protocol version.
    pub runtime_protocol: AbiVersion,
    /// Runtime capability bits available to linked output.
    pub runtime_capabilities: CapabilitySet,
}

impl AbiSupport {
    /// Construct the complete initial ABI support matrix.
    pub const fn v1(runtime_protocol: AbiVersion) -> Self {
        Self {
            descriptor_schema: DESCRIPTOR_SCHEMA_V1,
            client_ir: CLIENT_IR_V3,
            component_key_schema: COMPONENT_KEY_SCHEMA_V1,
            helper_key_schema: HELPER_KEY_SCHEMA_V1,
            method_extraction: METHOD_EXTRACTION_ABI_V1,
            site_registration: SITE_REGISTRATION_ABI_V1,
            helper_signature: HELPER_SIGNATURE_ABI_V1,
            client_codec: CLIENT_CODEC_ABI_V1,
            descriptor_capabilities: DESCRIPTOR_CAPABILITIES_V1,
            runtime_protocol,
            runtime_capabilities: RUNTIME_CAPABILITIES_V1,
        }
    }
}

fn validate_version(
    field: AbiVersionField,
    produced: AbiVersion,
    supported: AbiVersion,
) -> Result<(), AbiValidationError> {
    if produced.is_supported_by(supported) {
        Ok(())
    } else {
        Err(AbiValidationError::UnsupportedVersion {
            field,
            produced,
            supported,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SemanticPathKey {
    schema: AbiVersion,
    package: String,
    crate_name: String,
    module_path: Vec<String>,
    item: String,
}

impl SemanticPathKey {
    fn new<I, S>(
        schema: AbiVersion,
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Result<Self, AbiValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let key = Self {
            schema,
            package: package.into(),
            crate_name: crate_name.into(),
            module_path: module_path.into_iter().map(Into::into).collect(),
            item: item.into(),
        };
        key.validate()?;
        Ok(key)
    }

    fn validate(&self) -> Result<(), AbiValidationError> {
        validate_namespace_piece("package", &self.package)?;
        validate_namespace_piece("crate", &self.crate_name)?;
        for segment in &self.module_path {
            validate_namespace_piece("module", segment)?;
        }
        validate_namespace_piece("item", &self.item)
    }
}

impl CanonicalEncode for SemanticPathKey {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticPathKey", 5);
        encoder.field(1, &self.schema);
        encoder.field(2, &self.package);
        encoder.field(3, &self.crate_name);
        encoder.field(4, &self.module_path);
        encoder.field(5, &self.item);
    }
}

fn validate_namespace_piece(field: &'static str, value: &str) -> Result<(), AbiValidationError> {
    let normalized = !value.is_empty()
        && value.trim() == value
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("::");
    if normalized {
        Ok(())
    } else {
        Err(AbiValidationError::InvalidSemanticKey {
            field,
            value: value.to_string(),
        })
    }
}

fn validate_shared_state_type_key(
    field: &'static str,
    value: &str,
) -> Result<(), AbiValidationError> {
    validate_namespace_piece(field, value)?;
    if value.contains('.') && !value.chars().any(char::is_whitespace) {
        Ok(())
    } else {
        Err(AbiValidationError::InvalidSemanticKey {
            field,
            value: value.to_string(),
        })
    }
}

fn valid_component_field_name(value: &str) -> bool {
    let value = value.strip_prefix("r#").unwrap_or(value);
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && chars.all(|character| character == '_' || character.is_alphanumeric())
}

/// Stable component ABI identity independent of Cargo/object identities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentSemanticKey(SemanticPathKey);

impl ComponentSemanticKey {
    /// Construct a validated key with an explicit key-schema version.
    pub fn new<I, S>(
        schema: AbiVersion,
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Result<Self, AbiValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        SemanticPathKey::new(schema, package, crate_name, module_path, item).map(Self)
    }

    /// Construct a component key using [`COMPONENT_KEY_SCHEMA_V1`].
    pub fn v1<I, S>(
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Result<Self, AbiValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(
            COMPONENT_KEY_SCHEMA_V1,
            package,
            crate_name,
            module_path,
            item,
        )
    }

    /// Key-schema generation.
    pub const fn schema(&self) -> AbiVersion {
        self.0.schema
    }
}

impl CanonicalEncode for ComponentSemanticKey {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentSemanticKey", 1);
        encoder.field(1, &self.0);
    }
}

/// Stable helper ABI identity independent of Cargo/object identities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HelperSemanticKey(SemanticPathKey);

impl HelperSemanticKey {
    /// Construct a validated key with an explicit key-schema version.
    pub fn new<I, S>(
        schema: AbiVersion,
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Result<Self, AbiValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        SemanticPathKey::new(schema, package, crate_name, module_path, item).map(Self)
    }

    /// Construct a helper key using [`HELPER_KEY_SCHEMA_V1`].
    pub fn v1<I, S>(
        package: impl Into<String>,
        crate_name: impl Into<String>,
        module_path: I,
        item: impl Into<String>,
    ) -> Result<Self, AbiValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(HELPER_KEY_SCHEMA_V1, package, crate_name, module_path, item)
    }

    /// Key-schema generation.
    pub const fn schema(&self) -> AbiVersion {
        self.0.schema
    }
}

impl CanonicalEncode for HelperSemanticKey {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("HelperSemanticKey", 1);
        encoder.field(1, &self.0);
    }
}

/// Rust render trait implemented by an analyzed component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderKind {
    /// `WebRenderSync`.
    WebRenderSync,
    /// `WebRender`.
    WebRender,
}

/// Permission to originate browser behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginPermission {
    /// May participate in a graph driven by inbound live values.
    ParticipantOnly,
    /// May create browser behavior locally.
    OriginCapable,
}

/// Classified source of browser behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OriginKind {
    /// Provenance-only source that creates no browser capability.
    SourceOnly,
    /// Locally writable signal/cell.
    WritableSignal,
    /// New event-handler body.
    EventHandler,
    /// New explicit client effect.
    Effect,
    /// Timer or scheduled callback.
    Timer,
    /// Query acquisition.
    Query,
    /// Mutation acquisition.
    Mutation,
    /// SharedState acquisition.
    SharedState,
    /// Node-reference acquisition.
    NodeRef,
    /// Browser capability read.
    BrowserCapabilityRead,
    /// Browser capability write.
    BrowserCapabilityWrite,
}

impl OriginKind {
    /// Whether this classification requires `#[rsjs(component, client)]`.
    pub const fn requires_client_permission(self) -> bool {
        !matches!(self, Self::SourceOnly)
    }
}

/// Browser obligation/effect class retained by client reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EffectKind {
    /// Reactive text update.
    DomTextUpdate,
    /// Reactive attribute update.
    DomAttributeUpdate,
    /// Reactive DOM property update.
    DomPropertyUpdate,
    /// Reactive class update.
    DomClassUpdate,
    /// Reactive style update.
    DomStyleUpdate,
    /// Reactive structural/branch update.
    DomStructuralUpdate,
    /// Event-handler body.
    EventHandler,
    /// Explicit effect/subscription.
    Subscription,
    /// Browser API read.
    BrowserRead,
    /// Browser API write.
    BrowserWrite,
    /// Query observation.
    QueryObservation,
    /// Query refetch.
    QueryRefetch,
    /// Query invalidation.
    QueryInvalidation,
    /// Client mutation.
    Mutation,
    /// SharedState read.
    SharedStateRead,
    /// SharedState write.
    SharedStateWrite,
    /// Node-reference behavior.
    NodeReference,
    /// Server-to-browser serialization crossing.
    Serialization,
}

/// Verified transitive helper effect class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperEffectClass {
    /// Pure supported computation with no client origin.
    OriginFree,
    /// Reserved for a future capability-gated helper ABI.
    Originating,
}

/// Temporal semantics of an inbound live prop use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TemporalPropMode {
    /// Always observe the current linked value.
    LiveRead,
    /// Sample once to initialize locally owned state.
    SeedOnce(PropSiteId),
    /// Re-run an effect when the prop changes.
    EffectDependency,
    /// Rebind a query observer when the key/spec changes.
    QueryKeyDependency,
    /// Live provider initialization is forbidden.
    ForbiddenProviderSeed,
    /// Live independent-owner crossing is forbidden.
    ForbiddenBoundaryCrossing,
}

/// How one declared component field may participate in client composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ComponentFieldRole {
    /// Ordinary SSR input with no client-visible crossing.
    ServerOnly,
    /// Value prop that may be serialized or connected to a live value.
    Value,
    /// Compiler-owned typed callback slot.
    Callback,
    /// Renderable child/fragment slot.
    Slot,
    /// Framework client handle, such as a query or SharedState field handle.
    ClientHandle,
}

/// Rust-level shape of a compiler-owned callback component field.
///
/// Callback optionality affects both the accepted Rust field type and the
/// linked invocation contract, so it is part of the canonical semantic ABI
/// rather than transport-only validation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentCallbackShape {
    /// `rsjs::Handler<T>`.
    Required,
    /// `Option<rsjs::Handler<T>>`.
    Optional,
}

/// Client codec authority required by a component field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ComponentFieldCodec {
    /// The field is never serialized to the browser.
    None,
    /// The field requires the named `ClientEncode` schema.
    ClientEncode {
        /// Canonical codec/type schema hash.
        schema: SemanticHash,
    },
}

/// Whether a slot may cross an owner boundary as immutable server DOM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StaticSlotCapability {
    /// Static server-slot crossing is not supported.
    Unsupported,
    /// The child preserves the rendered DOM range for its entire lifetime.
    Immutable {
        /// Whether the range may contain an explicitly planned nested owner.
        permits_nested_owner: bool,
    },
}

/// Stable schema for one named component field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFieldSchema {
    /// Rust source-level field name used by direct struct literals.
    pub source_name: String,
    /// Client-composition role inferred by the component compiler.
    pub role: ComponentFieldRole,
    /// Rust-generated witness for the concrete field type/signature.
    ///
    /// Server-only fields deliberately carry no browser/linker type ABI. Rust
    /// still type-checks their authored struct literals and render use in both
    /// compilations. Client-visible value, callback, live-slot, and handle
    /// fields require a concrete witness. A dedicated immutable static-only
    /// slot carries no browser value and may omit one.
    pub type_witness: Option<WitnessRecordId>,
    /// Compiler-generated field crossing hook, when client-visible.
    pub crossing_hook: Option<CrossingHookId>,
    /// Child-local symbolic graph value supplied by this field.
    ///
    /// Value props, compiler-owned callbacks, and framework client handles
    /// carry one. Server-only and render-slot fields do not. In particular, a
    /// callback value is a live graph node resolved at invocation time; it is
    /// never a serialized closure seed.
    pub client_value: Option<LocalValueId>,
    /// Exact Rust callback container shape.
    ///
    /// This is present exactly when [`Self::role`] is
    /// [`ComponentFieldRole::Callback`].
    pub callback_shape: Option<ComponentCallbackShape>,
    /// Serialization contract, if this field may cross as browser data.
    pub codec: ComponentFieldCodec,
    /// Immutable server-slot contract.
    pub static_slot: StaticSlotCapability,
}

/// One use of an inbound component value in the symbolic client graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropUse {
    /// Component field supplying the value.
    pub field: ComponentFieldId,
    /// Symbolic value node receiving the linked input.
    pub value: LocalValueId,
    /// Normative temporal behavior at this use.
    pub temporal: TemporalPropMode,
    /// Stable source provenance for diagnostics.
    pub origin: OriginId,
}

/// One SharedState schema acquired by a component's browser closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStateReference {
    /// Public protocol key from `#[rsjs(type_key = "...")]`.
    pub type_key: String,
    /// Complete canonical `SharedState` wire-schema ABI hash.
    pub schema_abi: SemanticHash,
    /// Stable source provenance for conflict diagnostics.
    pub origin: OriginId,
}

/// One statically present lexical SharedState provider site.
///
/// The referenced direct component edge is topology-only: it lets the linker
/// see through the wrapperless provider intrinsic without creating an owner
/// coupling edge or changing the provider's `Exact(0)` DOM behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStateProviderSite {
    /// Direct component edge for the provider's child.
    pub child_edge: ComponentEdgeId,
    /// Public protocol key from `#[rsjs(type_key = "...")]`.
    pub type_key: String,
    /// Complete canonical `SharedState` wire-schema ABI hash.
    pub schema_abi: SemanticHash,
}

/// Ownership meaning of a direct component syntax edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentBoundary {
    /// Normal direct composition; live edges may fuse owners.
    Composed,
    /// Explicit `rsjs::island(...)` ownership cut.
    IndependentIsland,
}

/// Branch polarity of one conditional-single component occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentLifecycleBranch {
    /// The occurrence exists while the control predicate is true.
    Then,
    /// The occurrence exists while the control predicate is false.
    Else,
}

/// Parent structural control governing one conditional component occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentLifecycleControl {
    /// Canonical component-local conditional control region.
    pub control: LocalControlRegionId,
    /// Canonical predicate value that drives `control`.
    pub predicate: LocalValueId,
    /// Source provenance on the exact predicate-to-control dependency.
    pub predicate_origin: OriginId,
    /// Branch whose DOM range contains the component site.
    pub branch: ComponentLifecycleBranch,
}

/// Parent-side value supplied to one direct child field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ComponentBindingSource {
    /// Pure SSR input with no client representation.
    ServerOnly,
    /// Compiler-known callback value that performs no browser work, such as
    /// `rsjs::noop_handler()`.
    StaticCallback {
        /// Required/optional child callback container shape.
        shape: ComponentCallbackShape,
    },
    /// Static value serialized once as initial browser data.
    SerializedSeed {
        /// Symbolic parent value.
        value: LocalValueId,
        /// Codec schema used at this exact crossing.
        codec: SemanticHash,
    },
    /// Read-only live value edge inside a composed owner.
    LiveValue {
        /// Symbolic parent value.
        value: LocalValueId,
    },
    /// Compiler-owned callback edge resolved at invocation time.
    LiveCallback {
        /// Symbolic parent callback value.
        value: LocalValueId,
        /// Required/optional callback container shape at this crossing.
        shape: ComponentCallbackShape,
    },
    /// Framework client handle that requires owner coupling.
    ClientHandle {
        /// Symbolic parent handle value.
        value: LocalValueId,
    },
    /// Immutable server-rendered child range.
    StaticSlot {
        /// Exact or anchored-unknown range emitted by SSR.
        span: SpanCardinality,
    },
    /// Client-renderable slot controlled by a parent region.
    LiveSlot {
        /// Parent control region that owns the slot lifecycle.
        control: LocalControlRegionId,
    },
}

impl ComponentBindingSource {
    /// Whether this binding requires a live graph edge across the component site.
    pub const fn requires_owner_coupling(&self) -> bool {
        matches!(
            self,
            Self::LiveValue { .. }
                | Self::LiveCallback { .. }
                | Self::ClientHandle { .. }
                | Self::LiveSlot { .. }
        )
    }
}

/// One source-level field assignment at a direct component edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentFieldBinding {
    /// Parent-side binding classification.
    pub source: ComponentBindingSource,
    /// Stable source provenance.
    pub origin: OriginId,
}

/// Rust-resolved direct child component edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectComponentEdge {
    /// Relocation-bearing witness record for the resolved Rust child type.
    pub witness: WitnessRecordId,
    /// Normal composition or an explicit independent-owner cut.
    pub boundary: ComponentBoundary,
    /// Source field name to parent-side binding. The linker resolves names to
    /// the child's stable [`ComponentFieldId`] and crossing hook.
    pub bindings: BTreeMap<String, ComponentFieldBinding>,
    /// Parent-reactive conditional creation/removal region, if any.
    ///
    /// `None` is one unconditional static-single occurrence. Repeated and
    /// unknown multiplicities never enter the canonical descriptor.
    pub lifecycle_control: Option<ComponentLifecycleControl>,
    /// Stable source provenance for the component expression.
    pub origin: OriginId,
}

/// Constraint contributed to the final owner hydration strategy.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HydrationConstraint {
    /// No source-level constraint; a constraint-free owner defaults to eager.
    Unconstrained,
    /// Activate eagerly.
    Eager,
    /// Activate when visible.
    Visible,
    /// Activate during idle time.
    Idle,
    /// Activate on interaction, optionally restricted to one event.
    Interaction {
        /// Optional DOM event name.
        event: Option<String>,
    },
    /// Activate when a media query matches.
    Media {
        /// Media query.
        query: String,
    },
}

impl HydrationConstraint {
    fn validate(&self) -> Result<(), AbiValidationError> {
        match self {
            Self::Interaction { event: Some(event) } if event.trim().is_empty() => {
                Err(AbiValidationError::InvalidHydrationConstraint {
                    reason: "interaction event cannot be empty",
                })
            }
            Self::Media { query } if query.trim().is_empty() => {
                Err(AbiValidationError::InvalidHydrationConstraint {
                    reason: "media query cannot be empty",
                })
            }
            _ => Ok(()),
        }
    }
}

/// Kind of independently owned DOM boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerBoundaryKind {
    /// Explicit `rsjs::island(...)`.
    IndependentIsland,
    /// Separately planned nested owner.
    NestedOwner,
}

/// Composable DOM span/cardinality plan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpanCardinality {
    /// Exactly this many DOM nodes.
    Exact(u32),
    /// Unknown node count requiring region resynchronization.
    Unknown {
        /// Stable linked region site.
        region: RegionSiteId,
        /// Whether SSR emits an explicit anchor contract.
        anchored: bool,
    },
    /// Parent locators cannot traverse into this owner.
    OwnerBoundary {
        /// Stable linked owner site.
        owner: OwnerSiteId,
        /// Kind of ownership cut.
        kind: OwnerBoundaryKind,
    },
    /// Ordered composition of child plans.
    Sequence(Vec<SpanCardinality>),
}

impl SpanCardinality {
    fn validate(&self) -> Result<(), AbiValidationError> {
        match self {
            Self::Sequence(parts) if parts.len() < 2 => Err(AbiValidationError::NonCanonicalSpan {
                reason: "a sequence must contain at least two parts",
            }),
            Self::Sequence(parts) => {
                for part in parts {
                    if matches!(part, Self::Sequence(_)) {
                        return Err(AbiValidationError::NonCanonicalSpan {
                            reason: "nested sequences must be flattened",
                        });
                    }
                    part.validate()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Typed dependency relation in the symbolic graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DependencyEdgeKind {
    /// Value contribution.
    Data,
    /// Control/DOM-structure dependence.
    Control,
    /// Handler/effect capture.
    Capture,
    /// Prop/callback/query/shared/slot component crossing.
    Component,
    /// Rust-resolved `ComponentAbi` witness relation.
    ComponentIdentity,
    /// Rust-resolved annotated helper call.
    HelperCall,
    /// Live/lifecycle relation requiring owner fusion.
    OwnerCoupling,
    /// Explicit ownership cut.
    OwnerBoundary,
    /// Server-to-browser value crossing.
    Serialization,
    /// Explicit browser obligation.
    Effect,
}

/// Endpoint of one typed local dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DependencyEndpoint {
    /// Symbolic value node.
    Value(LocalValueId),
    /// Browser effect node.
    Effect(LocalEffectId),
    /// Control region.
    Control(LocalControlRegionId),
    /// Direct component edge.
    Component(ComponentEdgeId),
    /// Direct helper-call edge.
    Helper(HelperCallEdgeId),
    /// Provenance/origin node.
    Origin(OriginId),
}

/// One typed local dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DependencyEdge {
    /// Edge taxonomy.
    pub kind: DependencyEdgeKind,
    /// Dependency source.
    pub from: DependencyEndpoint,
    /// Dependent target.
    pub to: DependencyEndpoint,
    /// Stable source provenance.
    pub origin: OriginId,
}

impl DependencyEdge {
    /// Construct a dependency edge.
    pub const fn new(
        kind: DependencyEdgeKind,
        from: DependencyEndpoint,
        to: DependencyEndpoint,
        origin: OriginId,
    ) -> Self {
        Self {
            kind,
            from,
            to,
            origin,
        }
    }
}

macro_rules! impl_simple_enum_canonical {
    ($ty:ty, $name:literal, {$($variant:path => $tag:expr),+ $(,)?}) => {
        impl CanonicalEncode for $ty {
            fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
                match self {
                    $($variant => encoder.variant($name, $tag, 0),)+
                }
            }
        }
    };
}

impl_simple_enum_canonical!(RenderKind, "RenderKind", {
    RenderKind::WebRenderSync => 0,
    RenderKind::WebRender => 1,
});
impl_simple_enum_canonical!(OriginPermission, "OriginPermission", {
    OriginPermission::ParticipantOnly => 0,
    OriginPermission::OriginCapable => 1,
});
impl_simple_enum_canonical!(OriginKind, "OriginKind", {
    OriginKind::SourceOnly => 0,
    OriginKind::WritableSignal => 1,
    OriginKind::EventHandler => 2,
    OriginKind::Effect => 3,
    OriginKind::Timer => 4,
    OriginKind::Query => 5,
    OriginKind::Mutation => 6,
    OriginKind::SharedState => 7,
    OriginKind::NodeRef => 8,
    OriginKind::BrowserCapabilityRead => 9,
    OriginKind::BrowserCapabilityWrite => 10,
});
impl_simple_enum_canonical!(EffectKind, "EffectKind", {
    EffectKind::DomTextUpdate => 0,
    EffectKind::DomAttributeUpdate => 1,
    EffectKind::DomPropertyUpdate => 2,
    EffectKind::DomClassUpdate => 3,
    EffectKind::DomStyleUpdate => 4,
    EffectKind::DomStructuralUpdate => 5,
    EffectKind::EventHandler => 6,
    EffectKind::Subscription => 7,
    EffectKind::BrowserRead => 8,
    EffectKind::BrowserWrite => 9,
    EffectKind::QueryObservation => 10,
    EffectKind::QueryRefetch => 11,
    EffectKind::QueryInvalidation => 12,
    EffectKind::Mutation => 13,
    EffectKind::SharedStateRead => 14,
    EffectKind::SharedStateWrite => 15,
    EffectKind::NodeReference => 16,
    EffectKind::Serialization => 17,
});
impl_simple_enum_canonical!(HelperEffectClass, "HelperEffectClass", {
    HelperEffectClass::OriginFree => 0,
    HelperEffectClass::Originating => 1,
});
impl_simple_enum_canonical!(ComponentFieldRole, "ComponentFieldRole", {
    ComponentFieldRole::ServerOnly => 0,
    ComponentFieldRole::Value => 1,
    ComponentFieldRole::Callback => 2,
    ComponentFieldRole::Slot => 3,
    ComponentFieldRole::ClientHandle => 4,
});
impl_simple_enum_canonical!(ComponentCallbackShape, "ComponentCallbackShape", {
    ComponentCallbackShape::Required => 0,
    ComponentCallbackShape::Optional => 1,
});
impl_simple_enum_canonical!(ComponentBoundary, "ComponentBoundary", {
    ComponentBoundary::Composed => 0,
    ComponentBoundary::IndependentIsland => 1,
});
impl_simple_enum_canonical!(ComponentLifecycleBranch, "ComponentLifecycleBranch", {
    ComponentLifecycleBranch::Then => 0,
    ComponentLifecycleBranch::Else => 1,
});
impl_simple_enum_canonical!(OwnerBoundaryKind, "OwnerBoundaryKind", {
    OwnerBoundaryKind::IndependentIsland => 0,
    OwnerBoundaryKind::NestedOwner => 1,
});
impl_simple_enum_canonical!(DependencyEdgeKind, "DependencyEdgeKind", {
    DependencyEdgeKind::Data => 0,
    DependencyEdgeKind::Control => 1,
    DependencyEdgeKind::Capture => 2,
    DependencyEdgeKind::Component => 3,
    DependencyEdgeKind::ComponentIdentity => 4,
    DependencyEdgeKind::HelperCall => 5,
    DependencyEdgeKind::OwnerCoupling => 6,
    DependencyEdgeKind::OwnerBoundary => 7,
    DependencyEdgeKind::Serialization => 8,
    DependencyEdgeKind::Effect => 9,
});

impl CanonicalEncode for TemporalPropMode {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::LiveRead => encoder.variant("TemporalPropMode", 0, 0),
            Self::SeedOnce(site) => {
                encoder.variant("TemporalPropMode", 1, 1);
                encoder.field(1, site);
            }
            Self::EffectDependency => encoder.variant("TemporalPropMode", 2, 0),
            Self::QueryKeyDependency => encoder.variant("TemporalPropMode", 3, 0),
            Self::ForbiddenProviderSeed => encoder.variant("TemporalPropMode", 4, 0),
            Self::ForbiddenBoundaryCrossing => encoder.variant("TemporalPropMode", 5, 0),
        }
    }
}

impl CanonicalEncode for ComponentFieldCodec {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::None => encoder.variant("ComponentFieldCodec", 0, 0),
            Self::ClientEncode { schema } => {
                encoder.variant("ComponentFieldCodec", 1, 1);
                encoder.field(1, schema);
            }
        }
    }
}

impl CanonicalEncode for StaticSlotCapability {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Unsupported => encoder.variant("StaticSlotCapability", 0, 0),
            Self::Immutable {
                permits_nested_owner,
            } => {
                encoder.variant("StaticSlotCapability", 1, 1);
                encoder.field(1, permits_nested_owner);
            }
        }
    }
}

impl CanonicalEncode for ComponentFieldSchema {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentFieldSchema", 8);
        encoder.field(1, &self.source_name);
        encoder.field(2, &self.role);
        encoder.field(3, &self.type_witness);
        encoder.field(4, &self.crossing_hook);
        encoder.field(5, &self.client_value);
        encoder.field(6, &self.callback_shape);
        encoder.field(7, &self.codec);
        encoder.field(8, &self.static_slot);
    }
}

impl CanonicalEncode for PropUse {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("PropUse", 4);
        encoder.field(1, &self.field);
        encoder.field(2, &self.value);
        encoder.field(3, &self.temporal);
        encoder.field(4, &self.origin);
    }
}

impl CanonicalEncode for SharedStateReference {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SharedStateReference", 3);
        encoder.field(1, &self.type_key);
        encoder.field(2, &self.schema_abi);
        encoder.field(3, &self.origin);
    }
}

impl CanonicalEncode for SharedStateProviderSite {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SharedStateProviderSite", 3);
        encoder.field(1, &self.child_edge);
        encoder.field(2, &self.type_key);
        encoder.field(3, &self.schema_abi);
    }
}

impl CanonicalEncode for ComponentLifecycleControl {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentLifecycleControl", 4);
        encoder.field(1, &self.control);
        encoder.field(2, &self.predicate);
        encoder.field(3, &self.predicate_origin);
        encoder.field(4, &self.branch);
    }
}

impl CanonicalEncode for ComponentBindingSource {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::ServerOnly => encoder.variant("ComponentBindingSource", 0, 0),
            Self::StaticCallback { shape } => {
                encoder.variant("ComponentBindingSource", 7, 1);
                encoder.field(1, shape);
            }
            Self::SerializedSeed { value, codec } => {
                encoder.variant("ComponentBindingSource", 1, 2);
                encoder.field(1, value);
                encoder.field(2, codec);
            }
            Self::LiveValue { value } => {
                encoder.variant("ComponentBindingSource", 2, 1);
                encoder.field(1, value);
            }
            Self::LiveCallback { value, shape } => {
                encoder.variant("ComponentBindingSource", 3, 2);
                encoder.field(1, value);
                encoder.field(2, shape);
            }
            Self::ClientHandle { value } => {
                encoder.variant("ComponentBindingSource", 4, 1);
                encoder.field(1, value);
            }
            Self::StaticSlot { span } => {
                encoder.variant("ComponentBindingSource", 5, 1);
                encoder.field(1, span);
            }
            Self::LiveSlot { control } => {
                encoder.variant("ComponentBindingSource", 6, 1);
                encoder.field(1, control);
            }
        }
    }
}

impl CanonicalEncode for ComponentFieldBinding {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentFieldBinding", 2);
        encoder.field(1, &self.source);
        encoder.field(2, &self.origin);
    }
}

impl CanonicalEncode for DirectComponentEdge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DirectComponentEdge", 5);
        encoder.field(1, &self.witness);
        encoder.field(2, &self.boundary);
        encoder.field(3, &self.bindings);
        encoder.field(4, &self.lifecycle_control);
        encoder.field(5, &self.origin);
    }
}

impl CanonicalEncode for HydrationConstraint {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Unconstrained => encoder.variant("HydrationConstraint", 0, 0),
            Self::Eager => encoder.variant("HydrationConstraint", 1, 0),
            Self::Visible => encoder.variant("HydrationConstraint", 2, 0),
            Self::Idle => encoder.variant("HydrationConstraint", 3, 0),
            Self::Interaction { event } => {
                encoder.variant("HydrationConstraint", 4, 1);
                encoder.field(1, event);
            }
            Self::Media { query } => {
                encoder.variant("HydrationConstraint", 5, 1);
                encoder.field(1, query);
            }
        }
    }
}

impl CanonicalEncode for SpanCardinality {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Exact(count) => {
                encoder.variant("SpanCardinality", 0, 1);
                encoder.field(1, count);
            }
            Self::Unknown { region, anchored } => {
                encoder.variant("SpanCardinality", 1, 2);
                encoder.field(1, region);
                encoder.field(2, anchored);
            }
            Self::OwnerBoundary { owner, kind } => {
                encoder.variant("SpanCardinality", 2, 2);
                encoder.field(1, owner);
                encoder.field(2, kind);
            }
            Self::Sequence(parts) => {
                encoder.variant("SpanCardinality", 3, 1);
                encoder.field(1, parts);
            }
        }
    }
}

impl CanonicalEncode for DependencyEndpoint {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Value(id) => {
                encoder.variant("DependencyEndpoint", 0, 1);
                encoder.field(1, id);
            }
            Self::Effect(id) => {
                encoder.variant("DependencyEndpoint", 1, 1);
                encoder.field(1, id);
            }
            Self::Control(id) => {
                encoder.variant("DependencyEndpoint", 2, 1);
                encoder.field(1, id);
            }
            Self::Component(id) => {
                encoder.variant("DependencyEndpoint", 3, 1);
                encoder.field(1, id);
            }
            Self::Helper(id) => {
                encoder.variant("DependencyEndpoint", 4, 1);
                encoder.field(1, id);
            }
            Self::Origin(id) => {
                encoder.variant("DependencyEndpoint", 5, 1);
                encoder.field(1, id);
            }
        }
    }
}

impl CanonicalEncode for DependencyEdge {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DependencyEdge", 4);
        encoder.field(1, &self.kind);
        encoder.field(2, &self.from);
        encoder.field(3, &self.to);
        encoder.field(4, &self.origin);
    }
}

trait DenseLocalId {
    fn dense_index(&self) -> u32;
}

macro_rules! impl_dense_local_id {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl DenseLocalId for $ty {
                fn dense_index(&self) -> u32 {
                    self.index()
                }
            }
        )+
    };
}

impl_dense_local_id!(
    LocalValueId,
    LocalEffectId,
    LocalControlRegionId,
    DependencyEdgeId,
    ComponentEdgeId,
    HelperCallEdgeId,
    PropSiteId,
    ComponentFieldId,
    CrossingHookId,
    SharedStateRefId,
    ProviderSiteId,
    OwnerSiteId,
    RegionSiteId,
    KeyedCollectionSiteId,
    OriginId,
);

fn validate_dense<'a, I, T>(collection: &'static str, ids: I) -> Result<(), AbiValidationError>
where
    I: IntoIterator<Item = &'a T>,
    T: DenseLocalId + 'a,
{
    for (expected, id) in ids.into_iter().enumerate() {
        let expected = expected as u32;
        let actual = id.dense_index();
        if expected != actual {
            return Err(AbiValidationError::NonDenseLocalIds {
                collection,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

/// Initial symbolic component descriptor body.
///
/// This freezes the cross-stage semantic categories without integrating the
/// proc macro, object transport, or linker.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ComponentDescriptor {
    /// Stable normalized component identity.
    pub semantic_key: ComponentSemanticKey,
    /// Defining relocation witness record.
    pub defining_witness: WitnessRecordId,
    /// Authored Proa render trait.
    pub render_kind: RenderKind,
    /// Client-origin permission.
    pub origin_permission: OriginPermission,
    /// Render-method extraction ABI.
    pub method_extraction_abi: AbiVersion,
    /// Linked-site registration ABI.
    pub site_registration_abi: AbiVersion,
    /// Conservative standalone root eligibility.
    pub root_eligible: bool,
    /// Hash of the complete canonical client program input represented by this
    /// descriptor.
    ///
    /// The symbolic value/effect graph is sufficient for linking and owner
    /// partitioning, but not for cache identity: two handler bodies can have
    /// the same graph shape while performing different work. This hash binds
    /// the descriptor and final [`PlanId`] to that
    /// behavior without embedding target- or host-specific artifact paths.
    pub client_ir_hash: SemanticHash,
    /// Symbolic value declarations.
    pub values: BTreeSet<LocalValueId>,
    /// Browser effect declarations.
    pub effects: BTreeMap<LocalEffectId, EffectKind>,
    /// Symbolic control regions.
    pub control_regions: BTreeSet<LocalControlRegionId>,
    /// Stable provenance/origin classifications.
    pub origins: BTreeMap<OriginId, OriginKind>,
    /// Stable component field schema.
    pub fields: BTreeMap<ComponentFieldId, ComponentFieldSchema>,
    /// Compiler-generated client crossing hooks.
    pub crossing_hooks: BTreeSet<CrossingHookId>,
    /// Inbound field uses and their temporal semantics.
    pub prop_uses: BTreeMap<PropSiteId, PropUse>,
    /// Direct component edge relocation witnesses.
    pub direct_components: BTreeMap<ComponentEdgeId, DirectComponentEdge>,
    /// Direct helper-call relocation witnesses.
    pub direct_helpers: BTreeMap<HelperCallEdgeId, WitnessRecordId>,
    /// SharedState schema acquisitions reachable from this component.
    pub shared_state: BTreeMap<SharedStateRefId, SharedStateReference>,
    /// Statically present lexical SharedState provider topology.
    pub shared_state_providers: BTreeMap<ProviderSiteId, SharedStateProviderSite>,
    /// Typed symbolic dependency graph.
    pub dependencies: BTreeMap<DependencyEdgeId, DependencyEdge>,
    /// Explicit/linked owner sites.
    pub owner_sites: BTreeSet<OwnerSiteId>,
    /// Unknown-cardinality region sites.
    pub region_sites: BTreeSet<RegionSiteId>,
    /// Keyed-collection sites.
    pub keyed_collection_sites: BTreeSet<KeyedCollectionSiteId>,
    /// Composable DOM span plan.
    pub span: SpanCardinality,
    /// Constraint on the final linked owner strategy.
    pub hydration: HydrationConstraint,
}

impl ComponentDescriptor {
    /// Construct an empty v1 component descriptor.
    pub fn v1(
        semantic_key: ComponentSemanticKey,
        defining_witness: WitnessRecordId,
        render_kind: RenderKind,
        origin_permission: OriginPermission,
        span: SpanCardinality,
    ) -> Self {
        Self {
            semantic_key,
            defining_witness,
            render_kind,
            origin_permission,
            method_extraction_abi: METHOD_EXTRACTION_ABI_V1,
            site_registration_abi: SITE_REGISTRATION_ABI_V1,
            root_eligible: true,
            client_ir_hash: canonical_semantic_hash("rsjs.empty-component-client-ir.v1"),
            values: BTreeSet::new(),
            effects: BTreeMap::new(),
            control_regions: BTreeSet::new(),
            origins: BTreeMap::new(),
            fields: BTreeMap::new(),
            crossing_hooks: BTreeSet::new(),
            prop_uses: BTreeMap::new(),
            direct_components: BTreeMap::new(),
            direct_helpers: BTreeMap::new(),
            shared_state: BTreeMap::new(),
            shared_state_providers: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            owner_sites: BTreeSet::new(),
            region_sites: BTreeSet::new(),
            keyed_collection_sites: BTreeSet::new(),
            span,
            hydration: HydrationConstraint::Unconstrained,
        }
    }

    fn validate_shape(&self) -> Result<(), AbiValidationError> {
        self.semantic_key.0.validate()?;
        self.hydration.validate()?;
        self.span.validate()?;

        validate_dense("values", self.values.iter())?;
        validate_dense("effects", self.effects.keys())?;
        validate_dense("control_regions", self.control_regions.iter())?;
        validate_dense("origins", self.origins.keys())?;
        validate_dense("fields", self.fields.keys())?;
        validate_dense("crossing_hooks", self.crossing_hooks.iter())?;
        validate_dense("prop_uses", self.prop_uses.keys())?;
        validate_dense("direct_components", self.direct_components.keys())?;
        validate_dense("direct_helpers", self.direct_helpers.keys())?;
        validate_dense("shared_state", self.shared_state.keys())?;
        validate_dense("shared_state_providers", self.shared_state_providers.keys())?;
        validate_dense("dependencies", self.dependencies.keys())?;
        validate_dense("owner_sites", self.owner_sites.iter())?;
        validate_dense("region_sites", self.region_sites.iter())?;
        validate_dense("keyed_collection_sites", self.keyed_collection_sites.iter())?;

        if self.origin_permission == OriginPermission::ParticipantOnly {
            if let Some((&origin, &kind)) = self
                .origins
                .iter()
                .find(|(_, kind)| kind.requires_client_permission())
            {
                return Err(AbiValidationError::OriginRequiresClientPermission { origin, kind });
            }
        }

        self.validate_fields()?;
        self.validate_prop_uses()?;
        self.validate_shared_state_providers()?;
        self.validate_component_edges()?;
        for shared in self.shared_state.values() {
            validate_shared_state_type_key("shared-state type_key", &shared.type_key)?;
            match self.origins.get(&shared.origin).copied() {
                None => {
                    return Err(AbiValidationError::UnknownOrigin {
                        origin: shared.origin,
                    });
                }
                Some(OriginKind::SharedState) => {}
                Some(actual) => {
                    return Err(AbiValidationError::OriginKindMismatch {
                        origin: shared.origin,
                        expected: OriginKind::SharedState,
                        actual,
                    });
                }
            }
        }

        for edge in self.dependencies.values() {
            if !self.origins.contains_key(&edge.origin) {
                return Err(AbiValidationError::UnknownOrigin {
                    origin: edge.origin,
                });
            }
            self.validate_endpoint(edge.from)?;
            self.validate_endpoint(edge.to)?;
        }

        Ok(())
    }

    fn validate_endpoint(&self, endpoint: DependencyEndpoint) -> Result<(), AbiValidationError> {
        let exists = match endpoint {
            DependencyEndpoint::Value(id) => self.values.contains(&id),
            DependencyEndpoint::Effect(id) => self.effects.contains_key(&id),
            DependencyEndpoint::Control(id) => self.control_regions.contains(&id),
            DependencyEndpoint::Component(id) => self.direct_components.contains_key(&id),
            DependencyEndpoint::Helper(id) => self.direct_helpers.contains_key(&id),
            DependencyEndpoint::Origin(id) => self.origins.contains_key(&id),
        };
        if exists {
            Ok(())
        } else {
            Err(AbiValidationError::UnknownDependencyEndpoint { endpoint })
        }
    }

    fn validate_fields(&self) -> Result<(), AbiValidationError> {
        let mut names = BTreeSet::new();
        for (&field_id, field) in &self.fields {
            if !valid_component_field_name(&field.source_name) {
                return Err(AbiValidationError::InvalidComponentFieldName {
                    field: field_id,
                    name: field.source_name.clone(),
                });
            }
            if !names.insert(field.source_name.clone()) {
                return Err(AbiValidationError::DuplicateComponentFieldName {
                    name: field.source_name.clone(),
                });
            }

            // This is the dedicated static-only slot shape. Its value crosses
            // only through SSR as an immutable DOM range, so there is no
            // browser type or crossing hook to witness. Rust still checks the
            // concrete field assignment and the child renderer's bounds.
            let static_only_slot = field.role == ComponentFieldRole::Slot
                && matches!(field.static_slot, StaticSlotCapability::Immutable { .. })
                && field.type_witness.is_none()
                && field.crossing_hook.is_none();
            let client_visible = field.role != ComponentFieldRole::ServerOnly && !static_only_slot;
            if client_visible && field.type_witness.is_none() {
                return Err(AbiValidationError::MissingComponentFieldTypeWitness {
                    field: field_id,
                });
            }
            match (client_visible, field.crossing_hook) {
                (true, Some(hook)) if self.crossing_hooks.contains(&hook) => {}
                (true, Some(hook)) => {
                    return Err(AbiValidationError::UnknownCrossingHook {
                        field: field_id,
                        hook,
                    });
                }
                (true, None) => {
                    return Err(AbiValidationError::MissingCrossingHook { field: field_id });
                }
                (false, Some(hook)) => {
                    return Err(AbiValidationError::UnexpectedCrossingHook {
                        field: field_id,
                        hook,
                    });
                }
                (false, None) => {}
            }

            let expects_client_value = matches!(
                field.role,
                ComponentFieldRole::Value
                    | ComponentFieldRole::Callback
                    | ComponentFieldRole::ClientHandle
            );
            match (expects_client_value, field.client_value) {
                (true, Some(value)) if self.values.contains(&value) => {}
                (true, Some(value)) => {
                    return Err(AbiValidationError::UnknownComponentFieldClientValue {
                        field: field_id,
                        value,
                    });
                }
                (true, None) => {
                    return Err(AbiValidationError::MissingComponentFieldClientValue {
                        field: field_id,
                    });
                }
                (false, Some(value)) => {
                    return Err(AbiValidationError::UnexpectedComponentFieldClientValue {
                        field: field_id,
                        value,
                    });
                }
                (false, None) => {}
            }

            match (field.role, field.callback_shape) {
                (ComponentFieldRole::Callback, Some(_)) => {}
                (ComponentFieldRole::Callback, None) => {
                    return Err(AbiValidationError::MissingComponentCallbackShape {
                        field: field_id,
                    });
                }
                (_, Some(shape)) => {
                    return Err(AbiValidationError::UnexpectedComponentCallbackShape {
                        field: field_id,
                        shape,
                    });
                }
                (_, None) => {}
            }

            if field.role != ComponentFieldRole::Slot
                && field.static_slot != StaticSlotCapability::Unsupported
            {
                return Err(AbiValidationError::StaticSlotOnNonSlotField { field: field_id });
            }
            if matches!(
                field.role,
                ComponentFieldRole::Callback | ComponentFieldRole::Slot
            ) && field.codec != ComponentFieldCodec::None
            {
                return Err(AbiValidationError::UnexpectedFieldCodec { field: field_id });
            }
        }
        Ok(())
    }

    fn validate_prop_uses(&self) -> Result<(), AbiValidationError> {
        for (&site, prop_use) in &self.prop_uses {
            let field = self.fields.get(&prop_use.field).ok_or(
                AbiValidationError::UnknownComponentField {
                    field: prop_use.field,
                },
            )?;
            if field.role != ComponentFieldRole::Value {
                return Err(AbiValidationError::PropUseOnNonValueField {
                    field: prop_use.field,
                    role: field.role,
                });
            }
            if !self.values.contains(&prop_use.value) {
                return Err(AbiValidationError::UnknownDependencyEndpoint {
                    endpoint: DependencyEndpoint::Value(prop_use.value),
                });
            }
            let Some(field_value) = field.client_value else {
                return Err(AbiValidationError::MissingComponentFieldClientValue {
                    field: prop_use.field,
                });
            };
            if field_value != prop_use.value {
                return Err(AbiValidationError::PropUseFieldValueMismatch {
                    field: prop_use.field,
                    field_value,
                    prop_value: prop_use.value,
                });
            }
            if !self.origins.contains_key(&prop_use.origin) {
                return Err(AbiValidationError::UnknownOrigin {
                    origin: prop_use.origin,
                });
            }
            if let TemporalPropMode::SeedOnce(seed_site) = prop_use.temporal {
                if seed_site != site {
                    return Err(AbiValidationError::InvalidSeedOnceSite {
                        site,
                        referenced: seed_site,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_component_edges(&self) -> Result<(), AbiValidationError> {
        for (&edge_id, edge) in &self.direct_components {
            if !self.origins.contains_key(&edge.origin) {
                return Err(AbiValidationError::UnknownOrigin {
                    origin: edge.origin,
                });
            }
            let component_lifecycle_dependencies = self
                .dependencies
                .values()
                .filter(|dependency| {
                    dependency.kind == DependencyEdgeKind::Control
                        && dependency.to == DependencyEndpoint::Component(edge_id)
                })
                .map(|dependency| (dependency.from, dependency.origin))
                .collect::<Vec<_>>();
            if let Some(lifecycle) = edge.lifecycle_control {
                if !self.control_regions.contains(&lifecycle.control) {
                    return Err(AbiValidationError::UnknownDependencyEndpoint {
                        endpoint: DependencyEndpoint::Control(lifecycle.control),
                    });
                }
                let controls_structural_effect = self.dependencies.values().any(|dependency| {
                    dependency.kind == DependencyEdgeKind::Control
                        && dependency.from == DependencyEndpoint::Control(lifecycle.control)
                        && matches!(
                            dependency.to,
                            DependencyEndpoint::Effect(effect)
                                if self.effects.get(&effect)
                                    == Some(&EffectKind::DomStructuralUpdate)
                        )
                });
                if !controls_structural_effect {
                    return Err(
                        AbiValidationError::LifecycleControlWithoutStructuralEffect {
                            edge: edge_id,
                            control: lifecycle.control,
                        },
                    );
                }
                if component_lifecycle_dependencies.is_empty() {
                    return Err(AbiValidationError::MissingComponentLifecycleDependency {
                        edge: edge_id,
                        control: lifecycle.control,
                    });
                }
                if component_lifecycle_dependencies.as_slice()
                    != [(DependencyEndpoint::Control(lifecycle.control), edge.origin)]
                {
                    return Err(AbiValidationError::ComponentLifecycleDependencyMismatch {
                        edge: edge_id,
                        expected_control: Some(lifecycle.control),
                        expected_origin: edge.origin,
                        actual: component_lifecycle_dependencies,
                    });
                }
                self.validate_lifecycle_predicate(edge_id, lifecycle)?;
            } else if !component_lifecycle_dependencies.is_empty() {
                return Err(AbiValidationError::ComponentLifecycleDependencyMismatch {
                    edge: edge_id,
                    expected_control: None,
                    expected_origin: edge.origin,
                    actual: component_lifecycle_dependencies,
                });
            }

            for (field_name, binding) in &edge.bindings {
                if !valid_component_field_name(field_name) {
                    return Err(AbiValidationError::InvalidComponentEdgeFieldName {
                        edge: edge_id,
                        name: field_name.clone(),
                    });
                }
                if !self.origins.contains_key(&binding.origin) {
                    return Err(AbiValidationError::UnknownOrigin {
                        origin: binding.origin,
                    });
                }
                self.validate_binding_source(&binding.source)?;
                if edge.boundary == ComponentBoundary::IndependentIsland
                    && binding.source.requires_owner_coupling()
                {
                    return Err(AbiValidationError::LiveBindingAcrossIndependentIsland {
                        edge: edge_id,
                        field_name: field_name.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_shared_state_providers(&self) -> Result<(), AbiValidationError> {
        let mut child_edges = BTreeMap::new();
        for (&site, provider) in &self.shared_state_providers {
            validate_shared_state_type_key("shared-state provider type_key", &provider.type_key)?;

            let Some(child) = self.direct_components.get(&provider.child_edge) else {
                return Err(AbiValidationError::UnknownSharedStateProviderChildEdge {
                    site,
                    child_edge: provider.child_edge,
                });
            };
            if child.boundary != ComponentBoundary::Composed {
                return Err(AbiValidationError::SharedStateProviderChildMustBeComposed {
                    site,
                    child_edge: provider.child_edge,
                });
            }
            if child.lifecycle_control.is_some() {
                return Err(AbiValidationError::DynamicSharedStateProviderUnsupported {
                    site,
                    child_edge: provider.child_edge,
                });
            }
            if let Some((field_name, _)) = child
                .bindings
                .iter()
                .find(|(_, binding)| binding.source.requires_owner_coupling())
            {
                return Err(AbiValidationError::CoupledSharedStateProviderBinding {
                    site,
                    child_edge: provider.child_edge,
                    field_name: field_name.clone(),
                });
            }
            if let Some(first) = child_edges.insert(provider.child_edge, site) {
                return Err(AbiValidationError::DuplicateSharedStateProviderChildEdge {
                    first,
                    duplicate: site,
                    child_edge: provider.child_edge,
                });
            }
        }
        Ok(())
    }

    fn validate_lifecycle_predicate(
        &self,
        edge: ComponentEdgeId,
        lifecycle: ComponentLifecycleControl,
    ) -> Result<(), AbiValidationError> {
        if !self.values.contains(&lifecycle.predicate) {
            return Err(AbiValidationError::UnknownDependencyEndpoint {
                endpoint: DependencyEndpoint::Value(lifecycle.predicate),
            });
        }
        if !self.origins.contains_key(&lifecycle.predicate_origin) {
            return Err(AbiValidationError::UnknownOrigin {
                origin: lifecycle.predicate_origin,
            });
        }

        // `ConditionalSingle` is reserved for a client structural control.
        // Retaining the exact predicate value and dependency provenance makes
        // the lifecycle record independently checkable after transport and
        // semantic-plan persistence. Static/server-only conditionals must not
        // enter this shape.
        let predicate_dependencies = self
            .dependencies
            .iter()
            .filter(|(_, dependency)| {
                dependency.to == DependencyEndpoint::Control(lifecycle.control)
            })
            .map(|(&id, dependency)| (id, dependency.kind, dependency.from, dependency.origin))
            .collect::<Vec<_>>();
        let expected = [(
            DependencyEdgeKind::Control,
            DependencyEndpoint::Value(lifecycle.predicate),
            lifecycle.predicate_origin,
        )];
        if predicate_dependencies.len() != 1
            || predicate_dependencies
                .iter()
                .map(|(_, kind, from, origin)| (*kind, *from, *origin))
                .ne(expected)
        {
            return Err(AbiValidationError::LifecyclePredicateDependencyMismatch {
                edge,
                control: lifecycle.control,
                expected_predicate: lifecycle.predicate,
                expected_origin: lifecycle.predicate_origin,
                actual: predicate_dependencies,
            });
        }

        self.validate_lifecycle_predicate_provenance(edge, lifecycle)
    }

    fn validate_lifecycle_predicate_provenance(
        &self,
        edge: ComponentEdgeId,
        lifecycle: ComponentLifecycleControl,
    ) -> Result<(), AbiValidationError> {
        let live_inbound_values = self
            .prop_uses
            .values()
            .filter_map(|prop_use| {
                matches!(
                    prop_use.temporal,
                    TemporalPropMode::LiveRead
                        | TemporalPropMode::EffectDependency
                        | TemporalPropMode::QueryKeyDependency
                )
                .then_some(prop_use.value)
            })
            .collect::<HashSet<_>>();
        let mut pending = vec![DependencyEndpoint::Value(lifecycle.predicate)];
        let mut visited = HashSet::new();
        let mut reaches_browser_origin = false;

        while let Some(endpoint) = pending.pop() {
            if !visited.insert(endpoint) {
                continue;
            }
            self.validate_endpoint(endpoint)?;

            match endpoint {
                DependencyEndpoint::Origin(origin)
                    if self
                        .origins
                        .get(&origin)
                        .copied()
                        .is_some_and(OriginKind::requires_client_permission) =>
                {
                    reaches_browser_origin = true;
                }
                DependencyEndpoint::Value(value) if live_inbound_values.contains(&value) => {
                    reaches_browser_origin = true;
                }
                _ => {}
            }

            let incoming = self
                .dependencies
                .iter()
                .filter(|(_, dependency)| dependency.to == endpoint)
                .collect::<Vec<_>>();
            if incoming.is_empty() {
                let supported_leaf = match endpoint {
                    DependencyEndpoint::Origin(_) | DependencyEndpoint::Helper(_) => true,
                    DependencyEndpoint::Value(value) => live_inbound_values.contains(&value),
                    _ => false,
                };
                if !supported_leaf {
                    return Err(AbiValidationError::UnresolvedLifecyclePredicateSource {
                        edge,
                        control: lifecycle.control,
                        endpoint,
                    });
                }
                continue;
            }

            for (&dependency_id, dependency) in incoming {
                if !self.origins.contains_key(&dependency.origin) {
                    return Err(AbiValidationError::UnknownOrigin {
                        origin: dependency.origin,
                    });
                }
                if !self.lifecycle_predicate_dependency_is_transportable(dependency) {
                    return Err(
                        AbiValidationError::UnsupportedLifecyclePredicateDependency {
                            edge,
                            control: lifecycle.control,
                            dependency: dependency_id,
                            kind: dependency.kind,
                            from: dependency.from,
                            to: dependency.to,
                        },
                    );
                }
                pending.push(dependency.from);
            }
        }

        if !reaches_browser_origin {
            return Err(AbiValidationError::LifecyclePredicateWithoutBrowserOrigin {
                edge,
                control: lifecycle.control,
                predicate: lifecycle.predicate,
            });
        }
        Ok(())
    }

    fn lifecycle_predicate_dependency_is_transportable(&self, dependency: &DependencyEdge) -> bool {
        use DependencyEdgeKind::{Capture, Data, Effect, HelperCall};
        use DependencyEndpoint::{Effect as EffectEndpoint, Helper, Origin, Value};

        match (dependency.kind, dependency.from, dependency.to) {
            // Canonical expressions and signals carry a SourceOnly declaration
            // edge. A writable signal is the only client-originating value
            // declaration that may directly source a lifecycle predicate.
            (Data, Origin(origin), Value(_)) => matches!(
                self.origins.get(&origin),
                Some(OriginKind::SourceOnly | OriginKind::WritableSignal)
            ),
            // Ordinary expression/dataflow and origin-free helper results.
            (Data, Value(_), Value(_)) | (HelperCall, Helper(_), Value(_)) => true,
            // Only observations that actually return a browser value may feed
            // predicate dataflow. Handler bodies, mutations, writes, and DOM
            // effects are never value producers even if a forged descriptor
            // connects them to a LocalValueId.
            (Data, EffectEndpoint(effect), Value(_)) => {
                self.lifecycle_value_read_effect(effect).is_some()
            }
            // Value-producing reads may themselves be nested. Both sides must
            // remain reads so an effect/write chain cannot masquerade as
            // predicate provenance.
            (Data, EffectEndpoint(source), EffectEndpoint(target)) => {
                self.lifecycle_value_read_effect(source).is_some()
                    && self.lifecycle_value_read_effect(target).is_some()
            }
            // Captures and pure helpers are inputs to a value-producing read,
            // not independent proof that the read is browser-reactive.
            (Capture, Value(_), EffectEndpoint(effect))
            | (HelperCall, Helper(_), EffectEndpoint(effect)) => {
                self.lifecycle_value_read_effect(effect).is_some()
            }
            // The read's canonical acquisition edge must pair the exact origin
            // and effect classes. This prevents a Mutation/EventHandler origin
            // from being relabeled as a BrowserRead path.
            (Effect, Origin(origin), EffectEndpoint(effect)) => {
                matches!(
                    (
                        self.origins.get(&origin),
                        self.lifecycle_value_read_effect(effect)
                    ),
                    (
                        Some(OriginKind::BrowserCapabilityRead),
                        Some(EffectKind::BrowserRead)
                    ) | (Some(OriginKind::Query), Some(EffectKind::QueryObservation))
                        | (
                            Some(OriginKind::SharedState),
                            Some(EffectKind::SharedStateRead)
                        )
                )
            }
            _ => false,
        }
    }

    fn lifecycle_value_read_effect(&self, effect: LocalEffectId) -> Option<EffectKind> {
        self.effects.get(&effect).copied().filter(|kind| {
            matches!(
                kind,
                EffectKind::BrowserRead
                    | EffectKind::QueryObservation
                    | EffectKind::SharedStateRead
            )
        })
    }

    fn validate_binding_source(
        &self,
        source: &ComponentBindingSource,
    ) -> Result<(), AbiValidationError> {
        match source {
            ComponentBindingSource::ServerOnly | ComponentBindingSource::StaticCallback { .. } => {
                Ok(())
            }
            ComponentBindingSource::SerializedSeed { value, .. }
            | ComponentBindingSource::LiveValue { value }
            | ComponentBindingSource::LiveCallback { value, .. }
            | ComponentBindingSource::ClientHandle { value } => {
                if self.values.contains(value) {
                    Ok(())
                } else {
                    Err(AbiValidationError::UnknownDependencyEndpoint {
                        endpoint: DependencyEndpoint::Value(*value),
                    })
                }
            }
            ComponentBindingSource::StaticSlot { span } => span.validate(),
            ComponentBindingSource::LiveSlot { control } => {
                if self.control_regions.contains(control) {
                    Ok(())
                } else {
                    Err(AbiValidationError::UnknownDependencyEndpoint {
                        endpoint: DependencyEndpoint::Control(*control),
                    })
                }
            }
        }
    }

    fn validate_support(&self, support: &AbiSupport) -> Result<(), AbiValidationError> {
        validate_version(
            AbiVersionField::ComponentKeySchema,
            self.semantic_key.schema(),
            support.component_key_schema,
        )?;
        validate_version(
            AbiVersionField::MethodExtraction,
            self.method_extraction_abi,
            support.method_extraction,
        )?;
        validate_version(
            AbiVersionField::SiteRegistration,
            self.site_registration_abi,
            support.site_registration,
        )
    }
}

impl CanonicalEncode for ComponentDescriptor {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("ComponentDescriptor", 25);
        encoder.field(1, &self.semantic_key);
        encoder.field(2, &self.defining_witness);
        encoder.field(3, &self.render_kind);
        encoder.field(4, &self.origin_permission);
        encoder.field(5, &self.method_extraction_abi);
        encoder.field(6, &self.site_registration_abi);
        encoder.field(7, &self.root_eligible);
        encoder.field(8, &self.values);
        encoder.field(9, &self.effects);
        encoder.field(10, &self.control_regions);
        encoder.field(11, &self.origins);
        encoder.field(12, &self.fields);
        encoder.field(13, &self.crossing_hooks);
        encoder.field(14, &self.prop_uses);
        encoder.field(15, &self.direct_components);
        encoder.field(16, &self.direct_helpers);
        encoder.field(17, &self.shared_state);
        encoder.field(18, &self.dependencies);
        encoder.field(19, &self.owner_sites);
        encoder.field(20, &self.region_sites);
        encoder.field(21, &self.keyed_collection_sites);
        encoder.field(22, &self.span);
        encoder.field(23, &self.hydration);
        encoder.field(24, &self.client_ir_hash);
        encoder.field(25, &self.shared_state_providers);
    }
}

/// Initial standalone client-helper descriptor body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HelperDescriptor {
    /// Stable normalized function identity.
    pub semantic_key: HelperSemanticKey,
    /// Defining relocation witness record.
    pub defining_witness: WitnessRecordId,
    /// Supported function signature ABI.
    pub signature_abi: AbiVersion,
    /// Hash of the exact canonical parameter/result signature.
    ///
    /// The ABI version identifies how the signature is encoded; this hash
    /// identifies the concrete signature carried by this helper.
    pub signature_hash: SemanticHash,
    /// Supported argument/result codec ABI.
    pub codec_abi: AbiVersion,
    /// Verified transitive helper effect class.
    pub effect_class: HelperEffectClass,
    /// Hash of canonical Client IR payload.
    pub client_ir_hash: SemanticHash,
    /// Symbolic value declarations.
    pub values: BTreeSet<LocalValueId>,
    /// Symbolic control regions.
    pub control_regions: BTreeSet<LocalControlRegionId>,
    /// Source-only provenance records.
    pub origins: BTreeMap<OriginId, OriginKind>,
    /// Direct helper-call relocation witnesses.
    pub direct_helpers: BTreeMap<HelperCallEdgeId, WitnessRecordId>,
    /// Typed helper data/control/call dependencies.
    pub dependencies: BTreeMap<DependencyEdgeId, DependencyEdge>,
}

impl HelperDescriptor {
    /// Construct an empty origin-free v1 helper descriptor.
    pub fn v1(
        semantic_key: HelperSemanticKey,
        defining_witness: WitnessRecordId,
        signature_hash: SemanticHash,
        client_ir_hash: SemanticHash,
    ) -> Self {
        Self {
            semantic_key,
            defining_witness,
            signature_abi: HELPER_SIGNATURE_ABI_V1,
            signature_hash,
            codec_abi: CLIENT_CODEC_ABI_V1,
            effect_class: HelperEffectClass::OriginFree,
            client_ir_hash,
            values: BTreeSet::new(),
            control_regions: BTreeSet::new(),
            origins: BTreeMap::new(),
            direct_helpers: BTreeMap::new(),
            dependencies: BTreeMap::new(),
        }
    }

    fn validate_shape(&self) -> Result<(), AbiValidationError> {
        self.semantic_key.0.validate()?;
        if self.effect_class != HelperEffectClass::OriginFree
            || self
                .origins
                .values()
                .any(|kind| kind.requires_client_permission())
        {
            return Err(AbiValidationError::EffectfulHelperUnsupported);
        }

        validate_dense("helper.values", self.values.iter())?;
        validate_dense("helper.control_regions", self.control_regions.iter())?;
        validate_dense("helper.origins", self.origins.keys())?;
        validate_dense("helper.direct_helpers", self.direct_helpers.keys())?;
        validate_dense("helper.dependencies", self.dependencies.keys())?;

        for edge in self.dependencies.values() {
            if !self.origins.contains_key(&edge.origin) {
                return Err(AbiValidationError::UnknownOrigin {
                    origin: edge.origin,
                });
            }
            self.validate_endpoint(edge.from)?;
            self.validate_endpoint(edge.to)?;
        }
        Ok(())
    }

    fn validate_endpoint(&self, endpoint: DependencyEndpoint) -> Result<(), AbiValidationError> {
        let exists = match endpoint {
            DependencyEndpoint::Value(id) => self.values.contains(&id),
            DependencyEndpoint::Control(id) => self.control_regions.contains(&id),
            DependencyEndpoint::Helper(id) => self.direct_helpers.contains_key(&id),
            DependencyEndpoint::Origin(id) => self.origins.contains_key(&id),
            DependencyEndpoint::Effect(_) | DependencyEndpoint::Component(_) => false,
        };
        if exists {
            Ok(())
        } else {
            Err(AbiValidationError::UnknownDependencyEndpoint { endpoint })
        }
    }

    fn validate_support(&self, support: &AbiSupport) -> Result<(), AbiValidationError> {
        validate_version(
            AbiVersionField::HelperKeySchema,
            self.semantic_key.schema(),
            support.helper_key_schema,
        )?;
        validate_version(
            AbiVersionField::HelperSignature,
            self.signature_abi,
            support.helper_signature,
        )?;
        validate_version(
            AbiVersionField::ClientCodec,
            self.codec_abi,
            support.client_codec,
        )
    }
}

impl CanonicalEncode for HelperDescriptor {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("HelperDescriptor", 12);
        encoder.field(1, &self.semantic_key);
        encoder.field(2, &self.defining_witness);
        encoder.field(3, &self.signature_abi);
        encoder.field(4, &self.signature_hash);
        encoder.field(5, &self.codec_abi);
        encoder.field(6, &self.effect_class);
        encoder.field(7, &self.client_ir_hash);
        encoder.field(8, &self.values);
        encoder.field(9, &self.control_regions);
        encoder.field(10, &self.origins);
        encoder.field(11, &self.direct_helpers);
        encoder.field(12, &self.dependencies);
    }
}

/// Versioned component/helper descriptor payload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DescriptorBody {
    /// Component graph descriptor.
    Component(ComponentDescriptor),
    /// Standalone helper descriptor.
    Helper(HelperDescriptor),
}

impl DescriptorBody {
    /// Body kind.
    pub const fn kind(&self) -> DescriptorKind {
        match self {
            Self::Component(_) => DescriptorKind::Component,
            Self::Helper(_) => DescriptorKind::Helper,
        }
    }

    fn validate_shape(&self) -> Result<(), AbiValidationError> {
        match self {
            Self::Component(component) => component.validate_shape(),
            Self::Helper(helper) => helper.validate_shape(),
        }
    }

    fn validate_support(&self, support: &AbiSupport) -> Result<(), AbiValidationError> {
        match self {
            Self::Component(component) => component.validate_support(support),
            Self::Helper(helper) => helper.validate_support(support),
        }
    }

    fn required_descriptor_capabilities(&self) -> CapabilitySet {
        match self {
            Self::Component(component) => {
                // These two contracts are intrinsic to the canonical
                // component body: every component carries classified origins
                // and one explicit DOM-span plan, even when both are empty or
                // exact in the simplest case.
                let mut required = CapabilitySet::ORIGIN_EFFECT_TAXONOMY
                    .union(CapabilitySet::DOM_SPAN_CARDINALITY);
                if !component.dependencies.is_empty() {
                    required = required.union(CapabilitySet::TYPED_DEPENDENCIES);
                }
                if !component.fields.is_empty() || !component.crossing_hooks.is_empty() {
                    required = required.union(CapabilitySet::COMPONENT_FIELD_SCHEMA);
                }
                if !component.prop_uses.is_empty() {
                    required = required.union(CapabilitySet::TEMPORAL_PROPS);
                }
                if !component.direct_components.is_empty() {
                    required = required
                        .union(CapabilitySet::DIRECT_COMPONENT_BINDINGS)
                        .union(CapabilitySet::OWNER_PARTITION_INPUTS);
                }
                if component
                    .direct_components
                    .values()
                    .any(|edge| edge.lifecycle_control.is_some())
                {
                    required = required.union(CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE);
                }
                if !component.shared_state_providers.is_empty() {
                    required = required.union(CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY);
                }
                if !component.owner_sites.is_empty() {
                    required = required.union(CapabilitySet::OWNER_PARTITION_INPUTS);
                }
                if !component.direct_helpers.is_empty() {
                    required = required.union(CapabilitySet::ORIGIN_FREE_HELPERS);
                }
                if component.fields.values().any(|field| {
                    matches!(field.static_slot, StaticSlotCapability::Immutable { .. })
                }) || component.direct_components.values().any(|edge| {
                    edge.bindings.values().any(|binding| {
                        matches!(binding.source, ComponentBindingSource::StaticSlot { .. })
                    })
                }) {
                    required = required.union(CapabilitySet::STATIC_SLOT_CONTRACTS);
                }
                required
            }
            Self::Helper(helper) => {
                let mut required =
                    CapabilitySet::ORIGIN_EFFECT_TAXONOMY.union(CapabilitySet::ORIGIN_FREE_HELPERS);
                if !helper.dependencies.is_empty() {
                    required = required.union(CapabilitySet::TYPED_DEPENDENCIES);
                }
                required
            }
        }
    }

    fn required_runtime_capabilities(&self) -> CapabilitySet {
        let Self::Component(component) = self else {
            return CapabilitySet::NONE;
        };

        let mut required = CapabilitySet::NONE;
        if !component.prop_uses.is_empty() {
            required = required.union(CapabilitySet::TEMPORAL_PROPS);
        }
        if !component.owner_sites.is_empty()
            || !component.direct_components.is_empty()
            || span_contains_owner_boundary(&component.span)
        {
            required = required.union(CapabilitySet::OWNER_PARTITION_INPUTS);
        }
        if !component.region_sites.is_empty() || span_needs_runtime_planning(&component.span) {
            required = required.union(CapabilitySet::DOM_SPAN_CARDINALITY);
        }
        required
    }
}

fn span_contains_owner_boundary(span: &SpanCardinality) -> bool {
    match span {
        SpanCardinality::OwnerBoundary { .. } => true,
        SpanCardinality::Sequence(parts) => parts.iter().any(span_contains_owner_boundary),
        SpanCardinality::Exact(_) | SpanCardinality::Unknown { .. } => false,
    }
}

fn span_needs_runtime_planning(span: &SpanCardinality) -> bool {
    match span {
        SpanCardinality::Exact(_) => false,
        SpanCardinality::Unknown { .. } | SpanCardinality::OwnerBoundary { .. } => true,
        SpanCardinality::Sequence(parts) => parts.iter().any(span_needs_runtime_planning),
    }
}

impl CanonicalEncode for DescriptorBody {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Component(component) => {
                encoder.variant("DescriptorBody", 0, 1);
                encoder.field(1, component);
            }
            Self::Helper(helper) => {
                encoder.variant("DescriptorBody", 1, 1);
                encoder.field(1, helper);
            }
        }
    }
}

/// Crate-relative source location used only for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSourceLocation {
    /// Normalized crate-relative source path.
    pub crate_relative_path: String,
    /// Start byte within that source file.
    pub start_byte: u32,
    /// End byte within that source file.
    pub end_byte: u32,
    /// One-based line.
    pub line: u32,
    /// One-based column.
    pub column: u32,
    /// Human-facing source label.
    pub label: String,
}

impl CanonicalEncode for DebugSourceLocation {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DebugSourceLocation", 6);
        encoder.field(1, &self.crate_relative_path);
        encoder.field(2, &self.start_byte);
        encoder.field(3, &self.end_byte);
        encoder.field(4, &self.line);
        encoder.field(5, &self.column);
        encoder.field(6, &self.label);
    }
}

/// Optional non-semantic source/debug material.
///
/// Host paths and generation timestamps may appear here because the complete
/// debug block is excluded from [`DescriptorEnvelope::semantic_hash`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DescriptorDebugInfo {
    /// Source location by stable origin.
    pub origins: BTreeMap<OriginId, DebugSourceLocation>,
    /// Optional host-specific paths retained only for local diagnostics.
    pub host_paths: BTreeSet<String>,
    /// Optional wall-clock generation time retained only for diagnostics.
    pub generated_at_unix_millis: Option<u64>,
}

impl CanonicalEncode for DescriptorDebugInfo {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DescriptorDebugInfo", 3);
        encoder.field(1, &self.origins);
        encoder.field(2, &self.host_paths);
        encoder.field(3, &self.generated_at_unix_millis);
    }
}

#[derive(Debug)]
struct SemanticDescriptorMaterial<'a> {
    header: &'a DescriptorHeader,
    body: &'a DescriptorBody,
}

impl CanonicalEncode for SemanticDescriptorMaterial<'_> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("SemanticDescriptorMaterial", 2);
        encoder.field(1, self.header);
        encoder.field(2, self.body);
    }
}

#[derive(Debug)]
struct DebugDescriptorMaterial<'a> {
    semantic_hash: SemanticHash,
    debug: &'a DescriptorDebugInfo,
}

impl CanonicalEncode for DebugDescriptorMaterial<'_> {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DebugDescriptorMaterial", 2);
        encoder.field(1, &self.semantic_hash);
        encoder.field(2, self.debug);
    }
}

/// Canonical descriptor envelope with separate semantic and debug identities.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DescriptorEnvelope {
    /// Common version/capability header.
    pub header: DescriptorHeader,
    /// Component or helper body.
    pub body: DescriptorBody,
    /// Hash over header and body only.
    pub semantic_hash: SemanticHash,
    /// Optional source/debug material excluded from semantic identity.
    pub debug: Option<DescriptorDebugInfo>,
    /// Hash over semantic identity and debug material, when present.
    pub debug_hash: Option<DebugHash>,
}

impl DescriptorEnvelope {
    /// Construct and hash a descriptor envelope.
    pub fn new(
        header: DescriptorHeader,
        body: DescriptorBody,
        debug: Option<DescriptorDebugInfo>,
    ) -> Result<Self, AbiValidationError> {
        let mut envelope = Self {
            header,
            body,
            semantic_hash: SemanticHash::from_bytes([0; HASH_BYTES]),
            debug,
            debug_hash: None,
        };
        envelope.validate_shape()?;
        envelope.refresh_hashes();
        Ok(envelope)
    }

    /// Canonical bytes used for semantic identity.
    pub fn semantic_bytes(&self) -> Vec<u8> {
        canonical_bytes(&SemanticDescriptorMaterial {
            header: &self.header,
            body: &self.body,
        })
    }

    /// Recompute semantic and optional debug hashes after editing public fields.
    pub fn refresh_hashes(&mut self) {
        self.semantic_hash = SemanticHash::derive(&SemanticDescriptorMaterial {
            header: &self.header,
            body: &self.body,
        });
        self.debug_hash = self.debug.as_ref().map(|debug| {
            DebugHash::derive(&DebugDescriptorMaterial {
                semantic_hash: self.semantic_hash,
                debug,
            })
        });
    }

    fn validate_shape(&self) -> Result<(), AbiValidationError> {
        self.header.validate_shape()?;
        let body_kind = self.body.kind();
        if self.header.kind != body_kind {
            return Err(AbiValidationError::DescriptorKindMismatch {
                header: self.header.kind,
                body: body_kind,
            });
        }
        self.body.validate_shape()?;

        let missing = self
            .body
            .required_descriptor_capabilities()
            .missing_from(self.header.capabilities);
        if missing != CapabilitySet::NONE {
            return Err(AbiValidationError::UnderdeclaredDescriptorCapabilities { missing });
        }

        let missing = self
            .body
            .required_runtime_capabilities()
            .missing_from(self.header.runtime.required_capabilities);
        if missing != CapabilitySet::NONE {
            return Err(AbiValidationError::UnderdeclaredRuntimeCapabilities { missing });
        }

        Ok(())
    }

    /// Validate shape, versions, capabilities, protocol range, and stored
    /// hashes against one consumer support matrix.
    pub fn validate(&self, support: &AbiSupport) -> Result<(), AbiValidationError> {
        self.validate_shape()?;
        self.header.validate(support)?;
        self.body.validate_support(support)?;

        let expected = SemanticHash::derive(&SemanticDescriptorMaterial {
            header: &self.header,
            body: &self.body,
        });
        if expected != self.semantic_hash {
            return Err(AbiValidationError::SemanticHashMismatch {
                expected,
                actual: self.semantic_hash,
            });
        }

        match (&self.debug, self.debug_hash) {
            (Some(debug), Some(actual)) => {
                let expected = DebugHash::derive(&DebugDescriptorMaterial {
                    semantic_hash: self.semantic_hash,
                    debug,
                });
                if expected != actual {
                    return Err(AbiValidationError::DebugHashMismatch { expected, actual });
                }
            }
            (Some(_), None) => return Err(AbiValidationError::DebugHashMissing),
            (None, Some(_)) => return Err(AbiValidationError::UnexpectedDebugHash),
            (None, None) => {}
        }

        Ok(())
    }
}

impl CanonicalEncode for DescriptorEnvelope {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DescriptorEnvelope", 5);
        encoder.field(1, &self.header);
        encoder.field(2, &self.body);
        encoder.field(3, &self.semantic_hash);
        encoder.field(4, &self.debug);
        encoder.field(5, &self.debug_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    const RUNTIME_V1: AbiVersion = AbiVersion::new(1, 0);

    #[test]
    fn legacy_canonical_json_byte_hash_is_pinned_and_domain_separated() {
        let hash = canonical_legacy_json_bytes_hash("fixture", br#"{"a":1}"#);
        assert_eq!(
            hash.to_string(),
            "a28fa1d9b30f2f383216e88d459a0e2f10c4b0145251358b59d8f58cd0d1bb2a"
        );
        assert_ne!(
            hash,
            canonical_legacy_json_bytes_hash("different", br#"{"a":1}"#)
        );
        assert_ne!(
            hash,
            canonical_legacy_json_bytes_hash("fixture", br#"{"a":2}"#)
        );
    }

    fn support() -> AbiSupport {
        AbiSupport::v1(RUNTIME_V1)
    }

    fn header(kind: DescriptorKind) -> DescriptorHeader {
        DescriptorHeader::v1(
            kind,
            ProducerVersion::new(0, 2, 0),
            RuntimeCompatibility::new(
                VersionRange::new(RUNTIME_V1, AbiVersion::new(1, 2)),
                RUNTIME_CAPABILITIES_V1,
            ),
        )
    }

    fn component(reverse_insertion: bool) -> ComponentDescriptor {
        let key = ComponentSemanticKey::v1("acme-web", "acme_web", ["ui"], "Counter")
            .expect("fixture key should be valid");
        let mut component = ComponentDescriptor::v1(
            key,
            WitnessRecordId::new(7),
            RenderKind::WebRenderSync,
            OriginPermission::ParticipantOnly,
            SpanCardinality::Sequence(vec![
                SpanCardinality::Exact(1),
                SpanCardinality::Unknown {
                    region: RegionSiteId::new(0),
                    anchored: true,
                },
            ]),
        );
        component.hydration = HydrationConstraint::Visible;

        let value_ids = if reverse_insertion { [1, 0] } else { [0, 1] };
        for id in value_ids {
            component.values.insert(LocalValueId::new(id));
        }
        component
            .effects
            .insert(LocalEffectId::new(0), EffectKind::DomTextUpdate);
        component
            .control_regions
            .insert(LocalControlRegionId::new(0));

        let origin_ids = if reverse_insertion { [1, 0] } else { [0, 1] };
        for id in origin_ids {
            component
                .origins
                .insert(OriginId::new(id), OriginKind::SourceOnly);
        }

        let field_schemas = [
            (
                ComponentFieldId::new(0),
                ComponentFieldSchema {
                    source_name: "count".to_string(),
                    role: ComponentFieldRole::Value,
                    type_witness: Some(WitnessRecordId::new(20)),
                    crossing_hook: Some(CrossingHookId::new(0)),
                    client_value: Some(LocalValueId::new(0)),
                    callback_shape: None,
                    codec: ComponentFieldCodec::ClientEncode {
                        schema: canonical_semantic_hash("u32"),
                    },
                    static_slot: StaticSlotCapability::Unsupported,
                },
            ),
            (
                ComponentFieldId::new(1),
                ComponentFieldSchema {
                    source_name: "label".to_string(),
                    role: ComponentFieldRole::Value,
                    type_witness: Some(WitnessRecordId::new(21)),
                    crossing_hook: Some(CrossingHookId::new(1)),
                    client_value: Some(LocalValueId::new(1)),
                    callback_shape: None,
                    codec: ComponentFieldCodec::ClientEncode {
                        schema: canonical_semantic_hash("String"),
                    },
                    static_slot: StaticSlotCapability::Unsupported,
                },
            ),
        ];
        for index in if reverse_insertion {
            [1_usize, 0]
        } else {
            [0, 1]
        } {
            let (field, schema) = field_schemas[index].clone();
            component.fields.insert(field, schema);
            component
                .crossing_hooks
                .insert(CrossingHookId::new(index as u32));
        }

        let prop_uses = [
            PropUse {
                field: ComponentFieldId::new(0),
                value: LocalValueId::new(0),
                temporal: TemporalPropMode::LiveRead,
                origin: OriginId::new(0),
            },
            PropUse {
                field: ComponentFieldId::new(1),
                value: LocalValueId::new(1),
                temporal: TemporalPropMode::SeedOnce(PropSiteId::new(1)),
                origin: OriginId::new(1),
            },
        ];
        for index in if reverse_insertion {
            [1_usize, 0]
        } else {
            [0, 1]
        } {
            component
                .prop_uses
                .insert(PropSiteId::new(index as u32), prop_uses[index]);
        }

        let mut bindings = BTreeMap::new();
        bindings.insert(
            "label".to_string(),
            ComponentFieldBinding {
                source: ComponentBindingSource::LiveValue {
                    value: LocalValueId::new(1),
                },
                origin: OriginId::new(1),
            },
        );
        component.direct_components.insert(
            ComponentEdgeId::new(0),
            DirectComponentEdge {
                witness: WitnessRecordId::new(11),
                boundary: ComponentBoundary::Composed,
                bindings,
                lifecycle_control: None,
                origin: OriginId::new(1),
            },
        );
        component
            .direct_helpers
            .insert(HelperCallEdgeId::new(0), WitnessRecordId::new(12));

        let dependencies = [
            DependencyEdge::new(
                DependencyEdgeKind::Data,
                DependencyEndpoint::Value(LocalValueId::new(0)),
                DependencyEndpoint::Effect(LocalEffectId::new(0)),
                OriginId::new(0),
            ),
            DependencyEdge::new(
                DependencyEdgeKind::ComponentIdentity,
                DependencyEndpoint::Component(ComponentEdgeId::new(0)),
                DependencyEndpoint::Effect(LocalEffectId::new(0)),
                OriginId::new(1),
            ),
            DependencyEdge::new(
                DependencyEdgeKind::HelperCall,
                DependencyEndpoint::Helper(HelperCallEdgeId::new(0)),
                DependencyEndpoint::Value(LocalValueId::new(1)),
                OriginId::new(1),
            ),
        ];
        for index in if reverse_insertion {
            [2_usize, 1, 0]
        } else {
            [0, 1, 2]
        } {
            component
                .dependencies
                .insert(DependencyEdgeId::new(index as u32), dependencies[index]);
        }

        component.owner_sites.insert(OwnerSiteId::new(0));
        component.region_sites.insert(RegionSiteId::new(0));
        component
            .keyed_collection_sites
            .insert(KeyedCollectionSiteId::new(0));
        component
    }

    fn component_with_shared_state_provider() -> ComponentDescriptor {
        let mut descriptor = component(false);
        descriptor
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .bindings
            .get_mut("label")
            .unwrap()
            .source = ComponentBindingSource::ServerOnly;
        descriptor.shared_state_providers.insert(
            ProviderSiteId::new(0),
            SharedStateProviderSite {
                child_edge: ComponentEdgeId::new(0),
                type_key: "acme.workbench".to_owned(),
                schema_abi: canonical_semantic_hash("workbench-v1"),
            },
        );
        descriptor
    }

    fn component_envelope(reverse_insertion: bool) -> DescriptorEnvelope {
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(component(reverse_insertion)),
            None,
        )
        .expect("fixture descriptor should be valid")
    }

    fn debug_info(path: &str, host_path: &str, timestamp: u64) -> DescriptorDebugInfo {
        let mut debug = DescriptorDebugInfo::default();
        debug.origins.insert(
            OriginId::new(0),
            DebugSourceLocation {
                crate_relative_path: path.to_string(),
                start_byte: 10,
                end_byte: 20,
                line: 4,
                column: 8,
                label: "count.get()".to_string(),
            },
        );
        debug.host_paths.insert(host_path.to_string());
        debug.generated_at_unix_millis = Some(timestamp);
        debug
    }

    #[test]
    fn canonical_maps_ignore_insertion_order() {
        let forward = component_envelope(false);
        let reverse = component_envelope(true);

        assert_eq!(forward.semantic_bytes(), reverse.semantic_bytes());
        assert_eq!(forward.semantic_hash, reverse.semantic_hash);
        // The v1 support/default capability set advertises the complete
        // current descriptor contract, so adding provider topology changes
        // this descriptor-header fixture even when the provider map is empty.
        assert_eq!(
            forward.semantic_hash.to_string(),
            "f4c50cbea4a5d7418534950b19355c915c6fcaaf167da7213593a9dac251c010"
        );
        forward.validate(&support()).unwrap();
        reverse.validate(&support()).unwrap();
    }

    #[test]
    fn shared_state_provider_topology_is_canonical_and_hash_sensitive() {
        let descriptor = component_with_shared_state_provider();
        let envelope = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(descriptor.clone()),
            None,
        )
        .expect("one static composed provider child should validate");
        envelope
            .validate(&support())
            .expect("v1 support advertises provider topology");

        let mut changed_key = descriptor.clone();
        changed_key
            .shared_state_providers
            .get_mut(&ProviderSiteId::new(0))
            .unwrap()
            .type_key = "acme.editor".to_owned();
        let changed_key = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(changed_key),
            None,
        )
        .unwrap();
        assert_ne!(envelope.semantic_hash, changed_key.semantic_hash);

        let mut changed_schema = descriptor;
        changed_schema
            .shared_state_providers
            .get_mut(&ProviderSiteId::new(0))
            .unwrap()
            .schema_abi = canonical_semantic_hash("workbench-v2");
        let changed_schema = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(changed_schema),
            None,
        )
        .unwrap();
        assert_ne!(envelope.semantic_hash, changed_schema.semantic_hash);

        let mut underdeclared_header = header(DescriptorKind::Component);
        underdeclared_header.capabilities = CapabilitySet::from_bits(
            underdeclared_header.capabilities.bits()
                & !CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY.bits(),
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                underdeclared_header,
                DescriptorBody::Component(component_with_shared_state_provider()),
                None,
            ),
            Err(AbiValidationError::UnderdeclaredDescriptorCapabilities { missing })
                if missing == CapabilitySet::SHARED_STATE_PROVIDER_TOPOLOGY
        ));
    }

    #[test]
    fn shared_state_provider_topology_fails_closed_on_invalid_shape() {
        let assert_invalid = |descriptor, expected| {
            let error = DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(descriptor),
                None,
            )
            .unwrap_err();
            assert_eq!(error, expected);
        };

        let mut non_dense = component_with_shared_state_provider();
        let provider = non_dense
            .shared_state_providers
            .remove(&ProviderSiteId::new(0))
            .unwrap();
        non_dense
            .shared_state_providers
            .insert(ProviderSiteId::new(1), provider);
        assert_invalid(
            non_dense,
            AbiValidationError::NonDenseLocalIds {
                collection: "shared_state_providers",
                expected: 0,
                actual: 1,
            },
        );

        let mut unnamespaced = component_with_shared_state_provider();
        unnamespaced
            .shared_state_providers
            .get_mut(&ProviderSiteId::new(0))
            .unwrap()
            .type_key = "workbench".to_owned();
        assert_invalid(
            unnamespaced,
            AbiValidationError::InvalidSemanticKey {
                field: "shared-state provider type_key",
                value: "workbench".to_owned(),
            },
        );

        let mut missing_child = component_with_shared_state_provider();
        missing_child
            .shared_state_providers
            .get_mut(&ProviderSiteId::new(0))
            .unwrap()
            .child_edge = ComponentEdgeId::new(1);
        assert_invalid(
            missing_child,
            AbiValidationError::UnknownSharedStateProviderChildEdge {
                site: ProviderSiteId::new(0),
                child_edge: ComponentEdgeId::new(1),
            },
        );

        let mut independent_child = component_with_shared_state_provider();
        independent_child
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .boundary = ComponentBoundary::IndependentIsland;
        assert_invalid(
            independent_child,
            AbiValidationError::SharedStateProviderChildMustBeComposed {
                site: ProviderSiteId::new(0),
                child_edge: ComponentEdgeId::new(0),
            },
        );

        let mut dynamic_child = component_with_shared_state_provider();
        dynamic_child
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control = Some(ComponentLifecycleControl {
            control: LocalControlRegionId::new(0),
            predicate: LocalValueId::new(0),
            predicate_origin: OriginId::new(0),
            branch: ComponentLifecycleBranch::Then,
        });
        assert_invalid(
            dynamic_child,
            AbiValidationError::DynamicSharedStateProviderUnsupported {
                site: ProviderSiteId::new(0),
                child_edge: ComponentEdgeId::new(0),
            },
        );

        let mut coupled_child = component_with_shared_state_provider();
        coupled_child
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .bindings
            .get_mut("label")
            .unwrap()
            .source = ComponentBindingSource::LiveValue {
            value: LocalValueId::new(1),
        };
        assert_invalid(
            coupled_child,
            AbiValidationError::CoupledSharedStateProviderBinding {
                site: ProviderSiteId::new(0),
                child_edge: ComponentEdgeId::new(0),
                field_name: "label".to_owned(),
            },
        );

        let mut duplicate_child = component_with_shared_state_provider();
        duplicate_child.shared_state_providers.insert(
            ProviderSiteId::new(1),
            SharedStateProviderSite {
                child_edge: ComponentEdgeId::new(0),
                type_key: "acme.workbench".to_owned(),
                schema_abi: canonical_semantic_hash("workbench-v1"),
            },
        );
        assert_invalid(
            duplicate_child,
            AbiValidationError::DuplicateSharedStateProviderChildEdge {
                first: ProviderSiteId::new(0),
                duplicate: ProviderSiteId::new(1),
                child_edge: ComponentEdgeId::new(0),
            },
        );
    }

    #[test]
    fn behavior_fields_are_hash_sensitive() {
        let base = component(false);
        let base_hash = component_envelope(false).semantic_hash;

        let mut variants = Vec::new();

        let mut changed = base.clone();
        changed.render_kind = RenderKind::WebRender;
        variants.push(changed);

        let mut changed = base.clone();
        changed.origin_permission = OriginPermission::OriginCapable;
        variants.push(changed);

        let mut changed = base.clone();
        changed.client_ir_hash = canonical_semantic_hash("different-client-program");
        variants.push(changed);

        let mut changed = base.clone();
        changed
            .prop_uses
            .get_mut(&PropSiteId::new(0))
            .unwrap()
            .temporal = TemporalPropMode::EffectDependency;
        variants.push(changed);

        let mut changed = base.clone();
        changed
            .dependencies
            .get_mut(&DependencyEdgeId::new(0))
            .unwrap()
            .kind = DependencyEdgeKind::Control;
        variants.push(changed);

        let mut changed = base.clone();
        changed.span = SpanCardinality::Exact(3);
        variants.push(changed);

        let mut changed = base;
        changed.hydration = HydrationConstraint::Eager;
        variants.push(changed);

        for body in variants {
            let envelope = DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(body),
                None,
            )
            .unwrap();
            assert_ne!(base_hash, envelope.semantic_hash);
        }

        let mut changed_header = component_envelope(false);
        changed_header.header.producer.patch += 1;
        changed_header.refresh_hashes();
        assert_ne!(base_hash, changed_header.semantic_hash);
    }

    #[test]
    fn source_debug_and_host_state_do_not_change_semantic_hash() {
        let first = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(component(false)),
            Some(debug_info(
                "src/ui/counter.rs",
                "/opt/acme/src/ui/counter.rs",
                1_000,
            )),
        )
        .unwrap();
        let second = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(component(false)),
            Some(debug_info(
                "src/components/counter.rs",
                "C:\\work\\acme\\src\\components\\counter.rs",
                9_000,
            )),
        )
        .unwrap();

        assert_eq!(first.semantic_bytes(), second.semantic_bytes());
        assert_eq!(first.semantic_hash, second.semantic_hash);
        assert_ne!(first.debug_hash, second.debug_hash);
        assert_ne!(canonical_bytes(&first), canonical_bytes(&second));
        first.validate(&support()).unwrap();
        second.validate(&support()).unwrap();
    }

    #[test]
    fn versions_capabilities_and_runtime_ranges_fail_closed() {
        let valid = component_envelope(false);
        valid.validate(&support()).unwrap();

        let mut schema_mismatch = valid.clone();
        schema_mismatch.header.descriptor_schema = AbiVersion::new(2, 0);
        schema_mismatch.refresh_hashes();
        assert!(matches!(
            schema_mismatch.validate(&support()),
            Err(AbiValidationError::UnsupportedVersion {
                field: AbiVersionField::DescriptorSchema,
                ..
            })
        ));

        for client_ir in [AbiVersion::new(2, 0), AbiVersion::new(4, 0)] {
            let mut client_ir_mismatch = valid.clone();
            client_ir_mismatch.header.client_ir = client_ir;
            client_ir_mismatch.refresh_hashes();
            assert!(matches!(
                client_ir_mismatch.validate(&support()),
                Err(AbiValidationError::UnsupportedVersion {
                    field: AbiVersionField::ClientIr,
                    produced,
                    supported: CLIENT_IR_V3,
                }) if produced == client_ir
            ));
        }

        let mut invalid_range = valid.clone();
        invalid_range.header.runtime.protocol =
            VersionRange::new(AbiVersion::new(2, 0), AbiVersion::new(1, 0));
        invalid_range.refresh_hashes();
        assert!(matches!(
            invalid_range.validate(&support()),
            Err(AbiValidationError::InvalidVersionRange { .. })
        ));

        let mut runtime_mismatch = valid.clone();
        runtime_mismatch.header.runtime.protocol = VersionRange::exact(AbiVersion::new(2, 0));
        runtime_mismatch.refresh_hashes();
        assert!(matches!(
            runtime_mismatch.validate(&support()),
            Err(AbiValidationError::RuntimeProtocolIncompatible { .. })
        ));

        let mut capability_mismatch = valid.clone();
        capability_mismatch.header.capabilities = capability_mismatch
            .header
            .capabilities
            .union(CapabilitySet::from_bits(1 << 63));
        capability_mismatch.refresh_hashes();
        assert!(matches!(
            capability_mismatch.validate(&support()),
            Err(AbiValidationError::UnsupportedDescriptorCapabilities { .. })
        ));

        let mut runtime_capability_mismatch = valid;
        runtime_capability_mismatch
            .header
            .runtime
            .required_capabilities = runtime_capability_mismatch
            .header
            .runtime
            .required_capabilities
            .union(CapabilitySet::from_bits(1 << 62));
        runtime_capability_mismatch.refresh_hashes();
        assert!(matches!(
            runtime_capability_mismatch.validate(&support()),
            Err(AbiValidationError::UnsupportedRuntimeCapabilities { .. })
        ));
    }

    #[test]
    fn descriptor_bodies_cannot_underdeclare_their_capability_contracts() {
        let valid = component_envelope(false);

        let mut descriptor_underdeclared = valid.clone();
        descriptor_underdeclared.header.capabilities = CapabilitySet::from_bits(
            DESCRIPTOR_CAPABILITIES_V1.bits() & !CapabilitySet::TYPED_DEPENDENCIES.bits(),
        );
        assert!(matches!(
            descriptor_underdeclared.validate(&support()),
            Err(AbiValidationError::UnderdeclaredDescriptorCapabilities { missing })
                if missing == CapabilitySet::TYPED_DEPENDENCIES
        ));

        let mut runtime_underdeclared = valid;
        runtime_underdeclared.header.runtime.required_capabilities = CapabilitySet::from_bits(
            RUNTIME_CAPABILITIES_V1.bits() & !CapabilitySet::DOM_SPAN_CARDINALITY.bits(),
        );
        assert!(matches!(
            runtime_underdeclared.validate(&support()),
            Err(AbiValidationError::UnderdeclaredRuntimeCapabilities { missing })
                if missing == CapabilitySet::DOM_SPAN_CARDINALITY
        ));

        let helper = HelperDescriptor::v1(
            HelperSemanticKey::v1("acme", "acme", ["helpers"], "identity").unwrap(),
            WitnessRecordId::new(0),
            canonical_semantic_hash("signature"),
            canonical_semantic_hash("body"),
        );
        let mut helper_header = header(DescriptorKind::Helper);
        helper_header.capabilities = CapabilitySet::ORIGIN_EFFECT_TAXONOMY;
        assert!(matches!(
            DescriptorEnvelope::new(
                helper_header,
                DescriptorBody::Helper(helper),
                None,
            ),
            Err(AbiValidationError::UnderdeclaredDescriptorCapabilities { missing })
                if missing == CapabilitySet::ORIGIN_FREE_HELPERS
        ));
    }

    #[test]
    fn version_range_is_inclusive_across_major_boundaries() {
        let range = VersionRange::new(AbiVersion::new(1, 2), AbiVersion::new(2, 1));
        assert!(range.contains(AbiVersion::new(1, 2)));
        assert!(range.contains(AbiVersion::new(1, 9)));
        assert!(range.contains(AbiVersion::new(2, 0)));
        assert!(range.contains(AbiVersion::new(2, 1)));
        assert!(!range.contains(AbiVersion::new(1, 1)));
        assert!(!range.contains(AbiVersion::new(2, 2)));
    }

    #[test]
    fn stored_hashes_are_validated_and_excluded_from_their_own_input() {
        let mut envelope = component_envelope(false);
        let semantic_bytes = envelope.semantic_bytes();
        envelope.semantic_hash = SemanticHash::from_bytes([0x55; HASH_BYTES]);

        assert_eq!(semantic_bytes, envelope.semantic_bytes());
        assert!(matches!(
            envelope.validate(&support()),
            Err(AbiValidationError::SemanticHashMismatch { .. })
        ));

        let mut with_debug = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(component(false)),
            Some(debug_info("src/ui.rs", "/tmp/ui.rs", 10)),
        )
        .unwrap();
        with_debug.debug_hash = Some(DebugHash::from_bytes([0x44; HASH_BYTES]));
        assert!(matches!(
            with_debug.validate(&support()),
            Err(AbiValidationError::DebugHashMismatch { .. })
        ));
    }

    #[test]
    fn canonical_shape_validation_rejects_non_dense_and_dangling_graphs() {
        let mut non_dense = component(false);
        non_dense.values.remove(&LocalValueId::new(0));
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(non_dense),
                None,
            ),
            Err(AbiValidationError::NonDenseLocalIds {
                collection: "values",
                ..
            })
        ));

        let mut dangling = component(false);
        dangling
            .dependencies
            .get_mut(&DependencyEdgeId::new(0))
            .unwrap()
            .to = DependencyEndpoint::Value(LocalValueId::new(99));
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(dangling),
                None,
            ),
            Err(AbiValidationError::UnknownDependencyEndpoint { .. })
        ));

        let mut nested_span = component(false);
        nested_span.span = SpanCardinality::Sequence(vec![
            SpanCardinality::Exact(1),
            SpanCardinality::Sequence(vec![SpanCardinality::Exact(1), SpanCardinality::Exact(1)]),
        ]);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(nested_span),
                None,
            ),
            Err(AbiValidationError::NonCanonicalSpan { .. })
        ));
    }

    #[test]
    fn structural_controls_are_canonical_and_fail_closed() {
        let key = ComponentSemanticKey::v1("acme", "acme", ["ui"], "Disclosure")
            .expect("fixture key should be valid");
        let mut controlled = ComponentDescriptor::v1(
            key,
            WitnessRecordId::new(0),
            RenderKind::WebRenderSync,
            OriginPermission::ParticipantOnly,
            SpanCardinality::Exact(1),
        );
        let value = LocalValueId::new(0);
        let control = LocalControlRegionId::new(0);
        let effect = LocalEffectId::new(0);
        let origin = OriginId::new(0);
        controlled.values.insert(value);
        controlled.control_regions.insert(control);
        controlled
            .effects
            .insert(effect, EffectKind::DomStructuralUpdate);
        controlled.origins.insert(origin, OriginKind::SourceOnly);
        controlled.dependencies.extend([
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

        let valid = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(controlled.clone()),
            None,
        )
        .expect("a closed structural-control graph should validate");
        valid
            .validate(&support())
            .expect("the consumer supports canonical control dependencies");

        controlled.control_regions.clear();
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(controlled),
                None,
            ),
            Err(AbiValidationError::UnknownDependencyEndpoint {
                endpoint: DependencyEndpoint::Control(id),
            }) if id == control
        ));
    }

    fn conditional_component_descriptor() -> ComponentDescriptor {
        let mut descriptor = component(false);
        let control = LocalControlRegionId::new(0);
        let effect = LocalEffectId::new(1);
        let edge = ComponentEdgeId::new(0);
        let edge_origin = descriptor.direct_components[&edge].origin;
        let predicate = LocalValueId::new(2);
        let signal_origin = OriginId::new(2);
        let predicate_origin = OriginId::new(3);
        descriptor.origin_permission = OriginPermission::OriginCapable;
        descriptor.values.insert(predicate);
        descriptor
            .origins
            .insert(signal_origin, OriginKind::WritableSignal);
        descriptor
            .origins
            .insert(predicate_origin, OriginKind::SourceOnly);
        descriptor
            .effects
            .insert(effect, EffectKind::DomStructuralUpdate);
        descriptor.dependencies.extend([
            (
                DependencyEdgeId::new(3),
                DependencyEdge::new(
                    DependencyEdgeKind::Control,
                    DependencyEndpoint::Value(predicate),
                    DependencyEndpoint::Control(control),
                    predicate_origin,
                ),
            ),
            (
                DependencyEdgeId::new(4),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Origin(signal_origin),
                    DependencyEndpoint::Value(predicate),
                    signal_origin,
                ),
            ),
            (
                DependencyEdgeId::new(5),
                DependencyEdge::new(
                    DependencyEdgeKind::Effect,
                    DependencyEndpoint::Origin(predicate_origin),
                    DependencyEndpoint::Effect(effect),
                    predicate_origin,
                ),
            ),
            (
                DependencyEdgeId::new(6),
                DependencyEdge::new(
                    DependencyEdgeKind::Control,
                    DependencyEndpoint::Control(control),
                    DependencyEndpoint::Effect(effect),
                    predicate_origin,
                ),
            ),
            (
                DependencyEdgeId::new(7),
                DependencyEdge::new(
                    DependencyEdgeKind::Control,
                    DependencyEndpoint::Control(control),
                    DependencyEndpoint::Component(edge),
                    edge_origin,
                ),
            ),
        ]);
        descriptor
            .direct_components
            .get_mut(&edge)
            .unwrap()
            .lifecycle_control = Some(ComponentLifecycleControl {
            control,
            predicate,
            predicate_origin,
            branch: ComponentLifecycleBranch::Then,
        });
        descriptor
    }

    #[test]
    fn conditional_component_lifecycle_requires_the_structural_control_graph() {
        let descriptor = conditional_component_descriptor();
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(descriptor.clone()),
            None,
        )
        .expect("a conditional-single edge tied to its structural control should validate");

        let mut unrelated = descriptor;
        let unrelated_control = LocalControlRegionId::new(1);
        unrelated.control_regions.insert(unrelated_control);
        unrelated
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control = Some(ComponentLifecycleControl {
            control: unrelated_control,
            predicate: LocalValueId::new(2),
            predicate_origin: OriginId::new(3),
            branch: ComponentLifecycleBranch::Else,
        });
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(unrelated),
                None,
            ),
            Err(AbiValidationError::LifecycleControlWithoutStructuralEffect {
                edge,
                control,
            }) if edge == ComponentEdgeId::new(0) && control == unrelated_control
        ));

        let mut missing_component_dependency = conditional_component_descriptor();
        missing_component_dependency
            .dependencies
            .remove(&DependencyEdgeId::new(7));
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(missing_component_dependency),
                None,
            ),
            Err(AbiValidationError::MissingComponentLifecycleDependency {
                edge,
                control,
            }) if edge == ComponentEdgeId::new(0)
                && control == LocalControlRegionId::new(0)
        ));

        let mut static_with_control_dependency = conditional_component_descriptor();
        static_with_control_dependency
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control = None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(static_with_control_dependency),
                None,
            ),
            Err(AbiValidationError::ComponentLifecycleDependencyMismatch {
                edge,
                expected_control: None,
                ..
            }) if edge == ComponentEdgeId::new(0)
        ));

        let mut extra_control_dependency = conditional_component_descriptor();
        let extra_control = LocalControlRegionId::new(1);
        let edge_origin =
            extra_control_dependency.direct_components[&ComponentEdgeId::new(0)].origin;
        extra_control_dependency
            .control_regions
            .insert(extra_control);
        extra_control_dependency.dependencies.insert(
            DependencyEdgeId::new(8),
            DependencyEdge::new(
                DependencyEdgeKind::Control,
                DependencyEndpoint::Control(extra_control),
                DependencyEndpoint::Component(ComponentEdgeId::new(0)),
                edge_origin,
            ),
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(extra_control_dependency),
                None,
            ),
            Err(AbiValidationError::ComponentLifecycleDependencyMismatch {
                edge,
                expected_control: Some(control),
                actual,
                ..
            }) if edge == ComponentEdgeId::new(0)
                && control == LocalControlRegionId::new(0)
                && actual.len() == 2
        ));
    }

    #[test]
    fn conditional_component_lifecycle_requires_exact_predicate_dependency() {
        let edge = ComponentEdgeId::new(0);
        let control = LocalControlRegionId::new(0);

        let mut wrong_predicate = conditional_component_descriptor();
        wrong_predicate
            .direct_components
            .get_mut(&edge)
            .unwrap()
            .lifecycle_control
            .as_mut()
            .unwrap()
            .predicate = LocalValueId::new(0);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(wrong_predicate),
                None,
            ),
            Err(AbiValidationError::LifecyclePredicateDependencyMismatch {
                edge: actual_edge,
                control: actual_control,
                expected_predicate,
                ..
            }) if actual_edge == edge
                && actual_control == control
                && expected_predicate == LocalValueId::new(0)
        ));

        let mut wrong_origin = conditional_component_descriptor();
        wrong_origin
            .direct_components
            .get_mut(&edge)
            .unwrap()
            .lifecycle_control
            .as_mut()
            .unwrap()
            .predicate_origin = OriginId::new(2);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(wrong_origin),
                None,
            ),
            Err(AbiValidationError::LifecyclePredicateDependencyMismatch {
                edge: actual_edge,
                control: actual_control,
                expected_origin,
                ..
            }) if actual_edge == edge
                && actual_control == control
                && expected_origin == OriginId::new(2)
        ));

        let mut duplicate = conditional_component_descriptor();
        duplicate.dependencies.insert(
            DependencyEdgeId::new(8),
            DependencyEdge::new(
                DependencyEdgeKind::Control,
                DependencyEndpoint::Value(LocalValueId::new(0)),
                DependencyEndpoint::Control(control),
                OriginId::new(0),
            ),
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(duplicate),
                None,
            ),
            Err(AbiValidationError::LifecyclePredicateDependencyMismatch {
                edge: actual_edge,
                control: actual_control,
                actual,
                ..
            }) if actual_edge == edge && actual_control == control && actual.len() == 2
        ));
    }

    #[test]
    fn conditional_component_lifecycle_accepts_multi_signal_memo_predicate() {
        let mut descriptor = conditional_component_descriptor();
        let second_signal = LocalValueId::new(3);
        let memo = LocalValueId::new(4);
        let second_signal_origin = OriginId::new(4);
        let memo_origin = OriginId::new(5);
        descriptor.values.extend([second_signal, memo]);
        descriptor
            .origins
            .insert(second_signal_origin, OriginKind::WritableSignal);
        descriptor
            .origins
            .insert(memo_origin, OriginKind::SourceOnly);
        descriptor.dependencies.extend([
            (
                DependencyEdgeId::new(8),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Origin(second_signal_origin),
                    DependencyEndpoint::Value(second_signal),
                    second_signal_origin,
                ),
            ),
            (
                DependencyEdgeId::new(9),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Origin(memo_origin),
                    DependencyEndpoint::Value(memo),
                    memo_origin,
                ),
            ),
            (
                DependencyEdgeId::new(10),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Value(LocalValueId::new(2)),
                    DependencyEndpoint::Value(memo),
                    memo_origin,
                ),
            ),
            (
                DependencyEdgeId::new(11),
                DependencyEdge::new(
                    DependencyEdgeKind::Data,
                    DependencyEndpoint::Value(second_signal),
                    DependencyEndpoint::Value(memo),
                    memo_origin,
                ),
            ),
        ]);
        descriptor
            .dependencies
            .get_mut(&DependencyEdgeId::new(3))
            .unwrap()
            .from = DependencyEndpoint::Value(memo);
        descriptor
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control
            .as_mut()
            .unwrap()
            .predicate = memo;

        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(descriptor),
            None,
        )
        .expect("every memo predicate dependency reaches a browser signal origin");
    }

    #[test]
    fn conditional_component_lifecycle_accepts_live_inbound_participant_predicate() {
        let mut descriptor = conditional_component_descriptor();
        descriptor.origin_permission = OriginPermission::ParticipantOnly;
        descriptor
            .origins
            .insert(OriginId::new(2), OriginKind::SourceOnly);
        descriptor
            .dependencies
            .get_mut(&DependencyEdgeId::new(3))
            .unwrap()
            .from = DependencyEndpoint::Value(LocalValueId::new(0));
        descriptor
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control
            .as_mut()
            .unwrap()
            .predicate = LocalValueId::new(0);

        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(descriptor),
            None,
        )
        .expect("a participant conditional may be driven by an inbound LiveRead");
    }

    #[test]
    fn conditional_component_lifecycle_rejects_origin_free_or_opaque_predicates() {
        let mut no_browser_origin = conditional_component_descriptor();
        no_browser_origin
            .origins
            .insert(OriginId::new(2), OriginKind::SourceOnly);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(no_browser_origin),
                None,
            ),
            Err(AbiValidationError::LifecyclePredicateWithoutBrowserOrigin {
                edge,
                control,
                predicate,
            }) if edge == ComponentEdgeId::new(0)
                && control == LocalControlRegionId::new(0)
                && predicate == LocalValueId::new(2)
        ));

        let mut opaque = conditional_component_descriptor();
        opaque.dependencies.insert(
            DependencyEdgeId::new(8),
            DependencyEdge::new(
                DependencyEdgeKind::OwnerBoundary,
                DependencyEndpoint::Value(LocalValueId::new(0)),
                DependencyEndpoint::Value(LocalValueId::new(2)),
                OriginId::new(0),
            ),
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(opaque),
                None,
            ),
            Err(AbiValidationError::UnsupportedLifecyclePredicateDependency {
                edge,
                control,
                dependency,
                kind: DependencyEdgeKind::OwnerBoundary,
                ..
            }) if edge == ComponentEdgeId::new(0)
                && control == LocalControlRegionId::new(0)
                && dependency == DependencyEdgeId::new(8)
        ));
    }

    #[test]
    fn conditional_component_lifecycle_rejects_non_value_effect_provenance() {
        let mut descriptor = conditional_component_descriptor();
        let mutation_effect = LocalEffectId::new(2);
        let mutation_origin = OriginId::new(4);
        descriptor
            .effects
            .insert(mutation_effect, EffectKind::Mutation);
        descriptor
            .origins
            .insert(mutation_origin, OriginKind::Mutation);
        let forged_value_edge = descriptor
            .dependencies
            .get_mut(&DependencyEdgeId::new(4))
            .expect("fixture has its writable-signal predicate source");
        forged_value_edge.from = DependencyEndpoint::Effect(mutation_effect);
        forged_value_edge.origin = mutation_origin;
        descriptor.dependencies.insert(
            DependencyEdgeId::new(8),
            DependencyEdge::new(
                DependencyEdgeKind::Effect,
                DependencyEndpoint::Origin(mutation_origin),
                DependencyEndpoint::Effect(mutation_effect),
                mutation_origin,
            ),
        );

        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(descriptor),
                None,
            ),
            Err(AbiValidationError::UnsupportedLifecyclePredicateDependency {
                edge,
                control,
                dependency,
                kind: DependencyEdgeKind::Data,
                from: DependencyEndpoint::Effect(effect),
                to: DependencyEndpoint::Value(predicate),
            }) if edge == ComponentEdgeId::new(0)
                && control == LocalControlRegionId::new(0)
                && dependency == DependencyEdgeId::new(4)
                && effect == mutation_effect
                && predicate == LocalValueId::new(2)
        ));
    }

    #[test]
    fn conditional_component_lifecycle_cannot_underdeclare_its_capability() {
        let mut descriptor_header = header(DescriptorKind::Component);
        descriptor_header.capabilities = CapabilitySet::from_bits(
            descriptor_header.capabilities.bits()
                & !CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE.bits(),
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                descriptor_header,
                DescriptorBody::Component(conditional_component_descriptor()),
                None,
            ),
            Err(AbiValidationError::UnderdeclaredDescriptorCapabilities { missing })
                if missing == CapabilitySet::CONDITIONAL_COMPONENT_LIFECYCLE
        ));
    }

    #[test]
    fn conditional_component_branch_polarity_is_semantic_identity() {
        let then = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(conditional_component_descriptor()),
            None,
        )
        .unwrap();
        let mut else_descriptor = conditional_component_descriptor();
        else_descriptor
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .lifecycle_control
            .as_mut()
            .unwrap()
            .branch = ComponentLifecycleBranch::Else;
        let else_branch = DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(else_descriptor),
            None,
        )
        .unwrap();

        assert_ne!(then.semantic_bytes(), else_branch.semantic_bytes());
        assert_ne!(then.semantic_hash, else_branch.semantic_hash);
    }

    #[test]
    fn static_component_edge_none_retains_its_legacy_canonical_bytes_and_hash() {
        struct LegacyStaticDirectComponentEdge<'a> {
            edge: &'a DirectComponentEdge,
        }

        impl CanonicalEncode for LegacyStaticDirectComponentEdge<'_> {
            fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
                encoder.record("DirectComponentEdge", 5);
                encoder.field(1, &self.edge.witness);
                encoder.field(2, &self.edge.boundary);
                encoder.field(3, &self.edge.bindings);
                encoder.field(4, &Option::<LocalControlRegionId>::None);
                encoder.field(5, &self.edge.origin);
            }
        }

        let descriptor = component(false);
        let edge = &descriptor.direct_components[&ComponentEdgeId::new(0)];
        assert_eq!(edge.lifecycle_control, None);
        let legacy = LegacyStaticDirectComponentEdge { edge };
        assert_eq!(canonical_bytes(edge), canonical_bytes(&legacy));
        assert_eq!(
            canonical_semantic_hash(edge),
            canonical_semantic_hash(&legacy)
        );
    }

    #[test]
    fn component_field_and_edge_contracts_fail_closed() {
        let mut missing_type_witness = component(false);
        missing_type_witness
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap()
            .type_witness = None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(missing_type_witness),
                None,
            ),
            Err(AbiValidationError::MissingComponentFieldTypeWitness { .. })
        ));

        let mut missing_hook = component(false);
        missing_hook
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap()
            .crossing_hook = None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(missing_hook),
                None,
            ),
            Err(AbiValidationError::MissingCrossingHook { .. })
        ));

        let mut callback_without_shape = component(false);
        let callback_field = callback_without_shape
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap();
        callback_field.role = ComponentFieldRole::Callback;
        callback_field.codec = ComponentFieldCodec::None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(callback_without_shape),
                None,
            ),
            Err(AbiValidationError::MissingComponentCallbackShape { field })
                if field == ComponentFieldId::new(0)
        ));

        let mut value_with_callback_shape = component(false);
        value_with_callback_shape
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap()
            .callback_shape = Some(ComponentCallbackShape::Optional);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(value_with_callback_shape),
                None,
            ),
            Err(AbiValidationError::UnexpectedComponentCallbackShape { field, shape })
                if field == ComponentFieldId::new(0)
                    && shape == ComponentCallbackShape::Optional
        ));

        let mut server_only = ComponentDescriptor::v1(
            ComponentSemanticKey::v1("acme", "acme", ["ui"], "StaticLabel").unwrap(),
            WitnessRecordId::new(0),
            RenderKind::WebRenderSync,
            OriginPermission::ParticipantOnly,
            SpanCardinality::Exact(1),
        );
        server_only.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "label".to_owned(),
                role: ComponentFieldRole::ServerOnly,
                type_witness: None,
                crossing_hook: None,
                client_value: None,
                callback_shape: None,
                codec: ComponentFieldCodec::None,
                static_slot: StaticSlotCapability::Unsupported,
            },
        );
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(server_only),
            None,
        )
        .expect("server-only fields have no browser/linker type ABI");

        let mut static_only_slot = ComponentDescriptor::v1(
            ComponentSemanticKey::v1("acme", "acme", ["ui"], "Disclosure").unwrap(),
            WitnessRecordId::new(0),
            RenderKind::WebRenderSync,
            OriginPermission::OriginCapable,
            SpanCardinality::Exact(1),
        );
        static_only_slot.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "children".to_owned(),
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
        DescriptorEnvelope::new(
            header(DescriptorKind::Component),
            DescriptorBody::Component(static_only_slot),
            None,
        )
        .expect("an immutable SSR-only slot has no browser crossing ABI");

        let mut live_slot_without_witness = ComponentDescriptor::v1(
            ComponentSemanticKey::v1("acme", "acme", ["ui"], "LiveSlot").unwrap(),
            WitnessRecordId::new(0),
            RenderKind::WebRenderSync,
            OriginPermission::OriginCapable,
            SpanCardinality::Exact(1),
        );
        live_slot_without_witness.fields.insert(
            ComponentFieldId::new(0),
            ComponentFieldSchema {
                source_name: "children".to_owned(),
                role: ComponentFieldRole::Slot,
                type_witness: None,
                crossing_hook: None,
                client_value: None,
                callback_shape: None,
                codec: ComponentFieldCodec::None,
                static_slot: StaticSlotCapability::Unsupported,
            },
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(live_slot_without_witness),
                None,
            ),
            Err(AbiValidationError::MissingComponentFieldTypeWitness { .. })
        ));

        let mut duplicate_name = component(false);
        duplicate_name
            .fields
            .get_mut(&ComponentFieldId::new(1))
            .unwrap()
            .source_name = "count".to_string();
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(duplicate_name),
                None,
            ),
            Err(AbiValidationError::DuplicateComponentFieldName { .. })
        ));

        let mut wrong_seed_site = component(false);
        wrong_seed_site
            .prop_uses
            .get_mut(&PropSiteId::new(1))
            .unwrap()
            .temporal = TemporalPropMode::SeedOnce(PropSiteId::new(0));
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(wrong_seed_site),
                None,
            ),
            Err(AbiValidationError::InvalidSeedOnceSite { .. })
        ));

        let mut callback_prop_use = component(false);
        let callback_field = callback_prop_use
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap();
        callback_field.role = ComponentFieldRole::Callback;
        callback_field.callback_shape = Some(ComponentCallbackShape::Required);
        callback_field.codec = ComponentFieldCodec::None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(callback_prop_use),
                None,
            ),
            Err(AbiValidationError::PropUseOnNonValueField { .. })
        ));

        let mut callback_without_value = component(false);
        let callback_field = callback_without_value
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap();
        callback_field.role = ComponentFieldRole::Callback;
        callback_field.callback_shape = Some(ComponentCallbackShape::Required);
        callback_field.codec = ComponentFieldCodec::None;
        callback_field.client_value = None;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(callback_without_value),
                None,
            ),
            Err(AbiValidationError::MissingComponentFieldClientValue {
                field
            }) if field == ComponentFieldId::new(0)
        ));

        let mut mismatched_prop_value = component(false);
        mismatched_prop_value
            .fields
            .get_mut(&ComponentFieldId::new(0))
            .unwrap()
            .client_value = Some(LocalValueId::new(1));
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(mismatched_prop_value),
                None,
            ),
            Err(AbiValidationError::PropUseFieldValueMismatch {
                field,
                field_value,
                prop_value,
            }) if field == ComponentFieldId::new(0)
                && field_value == LocalValueId::new(1)
                && prop_value == LocalValueId::new(0)
        ));

        let mut invalid_shared_key = component(false);
        invalid_shared_key.shared_state.insert(
            SharedStateRefId::new(0),
            SharedStateReference {
                type_key: " acme.workbench".to_string(),
                schema_abi: canonical_semantic_hash("workbench-v1"),
                origin: OriginId::new(0),
            },
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(invalid_shared_key),
                None,
            ),
            Err(AbiValidationError::InvalidSemanticKey {
                field: "shared-state type_key",
                ..
            })
        ));

        let mut mismatched_shared_origin = component(false);
        mismatched_shared_origin.shared_state.insert(
            SharedStateRefId::new(0),
            SharedStateReference {
                type_key: "acme.workbench".to_string(),
                schema_abi: canonical_semantic_hash("workbench-v1"),
                origin: OriginId::new(0),
            },
        );
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(mismatched_shared_origin),
                None,
            ),
            Err(AbiValidationError::OriginKindMismatch {
                origin,
                expected: OriginKind::SharedState,
                actual: OriginKind::SourceOnly,
            }) if origin == OriginId::new(0)
        ));

        let mut live_island = component(false);
        live_island
            .direct_components
            .get_mut(&ComponentEdgeId::new(0))
            .unwrap()
            .boundary = ComponentBoundary::IndependentIsland;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(live_island),
                None,
            ),
            Err(AbiValidationError::LiveBindingAcrossIndependentIsland { .. })
        ));
    }

    #[test]
    fn callback_optionality_is_part_of_the_canonical_semantic_hash() {
        let envelope = |shape| {
            let mut descriptor = ComponentDescriptor::v1(
                ComponentSemanticKey::v1("acme", "acme", ["ui"], "CallbackProbe").unwrap(),
                WitnessRecordId::new(0),
                RenderKind::WebRenderSync,
                OriginPermission::ParticipantOnly,
                SpanCardinality::Exact(1),
            );
            descriptor.values.insert(LocalValueId::new(0));
            descriptor.crossing_hooks.insert(CrossingHookId::new(0));
            descriptor.fields.insert(
                ComponentFieldId::new(0),
                ComponentFieldSchema {
                    source_name: "on_change".to_owned(),
                    role: ComponentFieldRole::Callback,
                    type_witness: Some(WitnessRecordId::new(1)),
                    crossing_hook: Some(CrossingHookId::new(0)),
                    client_value: Some(LocalValueId::new(0)),
                    callback_shape: Some(shape),
                    codec: ComponentFieldCodec::None,
                    static_slot: StaticSlotCapability::Unsupported,
                },
            );
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(descriptor),
                None,
            )
            .unwrap()
        };

        let required = envelope(ComponentCallbackShape::Required);
        let optional = envelope(ComponentCallbackShape::Optional);
        assert_ne!(
            required.semantic_hash, optional.semantic_hash,
            "Handler<T> and Option<Handler<T>> must not share a semantic component ABI"
        );
    }

    #[test]
    fn independent_island_accepts_only_static_or_serialized_inputs() {
        let allowed_sources = [
            ComponentBindingSource::ServerOnly,
            ComponentBindingSource::SerializedSeed {
                value: LocalValueId::new(0),
                codec: canonical_semantic_hash("u32"),
            },
            ComponentBindingSource::StaticSlot {
                span: SpanCardinality::Unknown {
                    region: RegionSiteId::new(0),
                    anchored: true,
                },
            },
        ];

        for source in allowed_sources {
            let mut descriptor = component(false);
            let edge = descriptor
                .direct_components
                .get_mut(&ComponentEdgeId::new(0))
                .unwrap();
            edge.boundary = ComponentBoundary::IndependentIsland;
            edge.bindings.get_mut("label").unwrap().source = source;
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(descriptor),
                None,
            )
            .expect("static and serialized inputs may cross an independent boundary");
        }
    }

    #[test]
    fn participant_only_and_helper_origin_rules_are_validated() {
        let mut participant = component(false);
        participant
            .origins
            .insert(OriginId::new(1), OriginKind::WritableSignal);
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Component),
                DescriptorBody::Component(participant),
                None,
            ),
            Err(AbiValidationError::OriginRequiresClientPermission { .. })
        ));

        let helper_key =
            HelperSemanticKey::v1("acme-web", "acme_web", ["helpers"], "pluralize").unwrap();
        let mut helper = HelperDescriptor::v1(
            helper_key,
            WitnessRecordId::new(3),
            canonical_semantic_hash("pluralize-signature"),
            canonical_semantic_hash("pluralize-ir"),
        );
        helper.effect_class = HelperEffectClass::Originating;
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Helper),
                DescriptorBody::Helper(helper),
                None,
            ),
            Err(AbiValidationError::EffectfulHelperUnsupported)
        ));
    }

    #[test]
    fn helper_signature_is_part_of_semantic_identity() {
        let helper_key =
            HelperSemanticKey::v1("acme-web", "acme_web", ["helpers"], "pluralize").unwrap();
        let first = HelperDescriptor::v1(
            helper_key.clone(),
            WitnessRecordId::new(0),
            canonical_semantic_hash("fn(u32) -> String"),
            canonical_semantic_hash("pluralize-ir"),
        );
        let second = HelperDescriptor::v1(
            helper_key,
            WitnessRecordId::new(0),
            canonical_semantic_hash("fn(u64) -> String"),
            canonical_semantic_hash("pluralize-ir"),
        );
        let first = DescriptorEnvelope::new(
            header(DescriptorKind::Helper),
            DescriptorBody::Helper(first),
            None,
        )
        .unwrap();
        let second = DescriptorEnvelope::new(
            header(DescriptorKind::Helper),
            DescriptorBody::Helper(second),
            None,
        )
        .unwrap();

        assert_ne!(first.semantic_hash, second.semantic_hash);
    }

    #[test]
    fn header_and_body_kind_must_match() {
        assert!(matches!(
            DescriptorEnvelope::new(
                header(DescriptorKind::Helper),
                DescriptorBody::Component(component(false)),
                None,
            ),
            Err(AbiValidationError::DescriptorKindMismatch { .. })
        ));
    }

    #[test]
    fn strong_ids_are_distinct_and_domain_separated() {
        assert_ne!(TypeId::of::<InvocationId>(), TypeId::of::<CargoUnitId>());
        assert_ne!(TypeId::of::<CargoUnitId>(), TypeId::of::<LinkUnitId>());
        assert_ne!(TypeId::of::<LinkUnitId>(), TypeId::of::<PlanId>());
        assert_ne!(TypeId::of::<PlanId>(), TypeId::of::<FinalUnitId>());
        assert_ne!(
            TypeId::of::<ComponentEdgeId>(),
            TypeId::of::<HelperCallEdgeId>()
        );
        assert_ne!(TypeId::of::<ComponentFieldId>(), TypeId::of::<PropSiteId>());
        assert_ne!(
            TypeId::of::<ComponentFieldId>(),
            TypeId::of::<CrossingHookId>()
        );
        assert_ne!(TypeId::of::<OwnerSiteId>(), TypeId::of::<RegionSiteId>());

        let invocation = InvocationId::derive("same material");
        let cargo_unit = CargoUnitId::derive("same material");
        let link_unit = LinkUnitId::derive("same material");
        let plan = PlanId::derive("same material");
        assert_ne!(invocation.as_bytes(), cargo_unit.as_bytes());
        assert_ne!(cargo_unit.as_bytes(), link_unit.as_bytes());
        assert_ne!(link_unit.as_bytes(), plan.as_bytes());

        let fingerprint = canonical_semantic_hash("unit");
        let first = FinalUnitId::for_plan(fingerprint, PlanId::derive("plan-a"));
        let second = FinalUnitId::for_plan(fingerprint, PlanId::derive("plan-b"));
        assert_ne!(first, second);
    }
}
