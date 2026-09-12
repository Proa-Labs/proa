// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Host-side extraction from caller-enumerated object and rlib artifacts.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use object::read::archive::ArchiveFile;
use object::{
    BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSymbol, RelocationEncoding,
    RelocationKind, RelocationTarget,
};
use thiserror::Error;

use crate::wire::{
    semantic_payload_hash, shared_state_schema_abi_hash, ComponentDefinitionAbi, PayloadKind,
    RecordKind, SemanticIdentityKind, TransportRecordId, COFF_COMPONENT_FIELD_SECTION,
    COFF_COMPONENT_PAYLOAD_SECTION, COFF_COMPONENT_SECTION, COFF_EDGE_SECTION,
    COFF_HELPER_EDGE_SECTION, COFF_HELPER_PAYLOAD_SECTION, COFF_HELPER_SECTION,
    COFF_SEMANTIC_IDENTITY_SECTION, COFF_SHARED_STATE_ACQUISITION_SECTION,
    COFF_SHARED_STATE_PAYLOAD_SECTION, COFF_SHARED_STATE_SECTION, COMPONENT_DEFINITION_KNOWN_FLAGS,
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
    SHARED_STATE_SCHEMA_FORMAT_MAGIC, SHARED_STATE_SCHEMA_FORMAT_VERSION,
    SHARED_STATE_SCHEMA_HEADER_SIZE, TRANSPORT_FORMAT_MAGIC, TRANSPORT_FORMAT_VERSION,
    TRANSPORT_RECORD_SIZE,
};

const HEADER_SIZE: usize = 32;
const CARGO_UNIT_ID_OFFSET: usize = 32;
const RECORD_ID_OFFSET: usize = 64;
const RELOCATION_OFFSET: usize = 96;
const PAYLOAD_RECORD_SIZE_OFFSET: usize = 16;
const PAYLOAD_SIZE_OFFSET: usize = 24;
const PAYLOAD_HASH_OFFSET: usize = 96;
const IDENTITY_RECORD_SIZE_OFFSET: usize = 16;
const IDENTITY_SIZE_OFFSET: usize = 24;
const SHARED_STATE_SCHEMA_RECORD_SIZE_OFFSET: usize = 16;
const SHARED_STATE_TYPE_KEY_SIZE_OFFSET: usize = 24;
const SHARED_STATE_SCHEMA_ABI_SIZE_OFFSET: usize = 32;
const SHARED_STATE_SCHEMA_ABI_HASH_OFFSET: usize = 40;

/// Exact position of one record in a Cargo-enumerated artifact.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactLocation {
    /// Exact artifact path supplied to the extractor.
    pub artifact_path: PathBuf,
    /// Archive member name for an rlib, if applicable.
    pub archive_member: Option<String>,
    /// Archive member ordinal, which disambiguates repeated member names.
    pub archive_member_index: Option<usize>,
    /// Target object section name.
    pub section: String,
    /// Record offset relative to the section.
    pub record_offset: u64,
}

impl std::fmt::Display for ArtifactLocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.artifact_path.display())?;
        if let Some(member) = &self.archive_member {
            write!(formatter, "({member})")?;
        }
        write!(formatter, ":{}+0x{:x}", self.section, self.record_offset)
    }
}

/// Identity obtained from an object relocation.
///
/// A symbol name here is the linker-visible target of the relocation, not an
/// identity reconstructed from Rust source tokens.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelocationIdentity {
    /// A linker symbol referenced by the relocation.
    Symbol(String),
    /// A same-object section-relative target.
    Section {
        /// Exact artifact/member containing the target section.
        artifact_path: PathBuf,
        /// Archive member name, when reading an rlib.
        archive_member: Option<String>,
        /// Archive member ordinal.
        archive_member_index: Option<usize>,
        /// Object section index.
        section_index: usize,
        /// Addend relative to the target section.
        offset: u64,
    },
}

impl std::fmt::Display for RelocationIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Symbol(symbol) => formatter.write_str(symbol),
            Self::Section {
                artifact_path,
                archive_member,
                section_index,
                offset,
                ..
            } => {
                write!(formatter, "{}", artifact_path.display())?;
                if let Some(member) = archive_member {
                    write!(formatter, "({member})")?;
                }
                write!(formatter, ":section#{section_index}+0x{offset:x}")
            }
        }
    }
}

/// Extracted component-definition record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentDefinition {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean the prototype producer had
    /// no orchestrator-assigned identity.
    pub cargo_unit_id: TransportRecordId,
    /// Descriptor payload record identifier; never the component identity.
    pub component_record_id: TransportRecordId,
    /// Target-independent render/origin projection read from the same fixed
    /// record that carries the defining witness relocation.
    pub component_abi: ComponentDefinitionAbi,
    /// Rust-resolved defining witness relocation.
    pub witness: RelocationIdentity,
}

/// Extracted direct-component edge record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentEdge {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean unassigned.
    pub cargo_unit_id: TransportRecordId,
    /// Source edge ID from the producer descriptor.
    pub source_edge_id: TransportRecordId,
    /// Relocation produced through `<ResolvedType as ComponentAbi>::WITNESS`.
    pub witness: RelocationIdentity,
}

/// Extracted rustc-backed component input-field witness record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentField {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean unassigned.
    pub cargo_unit_id: TransportRecordId,
    /// Deterministic component-field record identifier.
    pub field_record_id: TransportRecordId,
    /// Relocation to the field's defining component witness.
    pub witness: RelocationIdentity,
}

/// One edge joined to the exact defining record reached by its relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedComponentEdge {
    /// Source edge.
    pub edge: ComponentEdge,
    /// Defining component selected by the relocation target.
    pub definition: ComponentDefinition,
}

/// One field witness joined to the exact component definition reached by its
/// relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedComponentField {
    /// Rust-checked component field.
    pub field: ComponentField,
    /// Defining component selected by the relocation target.
    pub definition: ComponentDefinition,
}

/// Extracted client-helper-definition record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperDefinition {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean the prototype producer had
    /// no orchestrator-assigned identity.
    pub cargo_unit_id: TransportRecordId,
    /// Helper payload record identifier; never the helper identity.
    pub helper_record_id: TransportRecordId,
    /// Rust-resolved rewritten helper-function relocation.
    pub helper: RelocationIdentity,
}

/// Extracted client-helper source-edge record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperEdge {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean unassigned.
    pub cargo_unit_id: TransportRecordId,
    /// Source edge ID from the producer descriptor.
    pub source_edge_id: TransportRecordId,
    /// Relocation produced from the rustc-resolved helper path.
    pub helper: RelocationIdentity,
}

/// Extracted SharedState type-definition record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStateDefinition {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean unassigned.
    pub cargo_unit_id: TransportRecordId,
    /// Build-local schema payload record identifier.
    pub shared_state_record_id: TransportRecordId,
    /// Rust-resolved nominal type witness relocation.
    pub witness: RelocationIdentity,
}

/// Extracted component-local `rsjs::shared::<T>()` acquisition edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStateAcquisition {
    /// Record location.
    pub location: ArtifactLocation,
    /// Descriptor-pass Cargo unit. All zeroes mean unassigned.
    pub cargo_unit_id: TransportRecordId,
    /// Stable acquisition/source-edge identifier in the component descriptor.
    pub source_edge_id: TransportRecordId,
    /// Rust-resolved relocation obtained through the acquired state type.
    pub witness: RelocationIdentity,
}

/// One SharedState acquisition joined to the exact definition selected by its
/// rustc-resolved relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSharedStateAcquisition {
    /// Component-local acquisition edge.
    pub acquisition: SharedStateAcquisition,
    /// Defining SharedState type selected by the relocation target.
    pub definition: SharedStateDefinition,
}

/// Complete inline component descriptor joined by Cargo unit and defining
/// record ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentDescriptorPayload {
    /// Record location.
    pub location: ArtifactLocation,
    /// Exact descriptor-pass Cargo unit.
    pub cargo_unit_id: TransportRecordId,
    /// Component definition record this payload belongs to.
    pub component_record_id: TransportRecordId,
    /// Domain-separated digest validated during extraction.
    pub semantic_payload_hash: TransportRecordId,
    /// Complete deterministic descriptor JSON bytes.
    pub json: Vec<u8>,
}

/// Complete inline standalone-helper descriptor joined by Cargo unit and
/// defining record ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneHelperPayload {
    /// Record location.
    pub location: ArtifactLocation,
    /// Exact descriptor-pass Cargo unit.
    pub cargo_unit_id: TransportRecordId,
    /// Helper definition record this payload belongs to.
    pub helper_record_id: TransportRecordId,
    /// Domain-separated digest validated during extraction.
    pub semantic_payload_hash: TransportRecordId,
    /// Complete deterministic helper descriptor JSON bytes.
    pub json: Vec<u8>,
}

/// Exact public key and canonical wire-schema ABI joined to a SharedState
/// definition record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStateDefinitionPayload {
    /// Record location.
    pub location: ArtifactLocation,
    /// Exact descriptor-pass Cargo unit.
    pub cargo_unit_id: TransportRecordId,
    /// SharedState definition record this payload belongs to.
    pub shared_state_record_id: TransportRecordId,
    /// Domain-separated digest of the complete inner payload.
    pub semantic_payload_hash: TransportRecordId,
    /// Explicit public protocol key.
    pub type_key: String,
    /// Domain-separated digest of only the canonical schema ABI bytes.
    pub schema_abi_hash: TransportRecordId,
    /// Complete canonical wire-schema ABI bytes, preserved byte-for-byte.
    pub schema_abi: Vec<u8>,
}

/// Normalized package/crate/module/item identity emitted beside one exact
/// component or helper definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticIdentity {
    /// Record location in the caller-enumerated artifact.
    pub location: ArtifactLocation,
    /// Component or helper definition.
    pub kind: SemanticIdentityKind,
    /// Exact descriptor-pass Cargo unit.
    pub cargo_unit_id: TransportRecordId,
    /// Exact component/helper definition record.
    pub definition_record_id: TransportRecordId,
    /// Declared Cargo package namespace.
    pub package: String,
    /// Cargo/rustc crate name.
    pub crate_name: String,
    /// Module segments containing the defining macro expansion, excluding the
    /// crate-name prefix supplied by `module_path!()`.
    pub module_path: Vec<String>,
    /// Defining component/helper item name.
    pub item: String,
}

/// Component definition after its required inline payload has been joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedComponentDefinition {
    /// Rust-resolved definition record.
    pub definition: ComponentDefinition,
    /// Exact payload selected by `(CargoUnitId, component_record_id)`.
    pub payload: ComponentDescriptorPayload,
    /// Exact normalized identity selected by the same definition key.
    pub identity: SemanticIdentity,
}

/// Standalone helper definition after its required inline payload has been
/// joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedHelperDefinition {
    /// Rust-resolved definition record.
    pub definition: HelperDefinition,
    /// Exact payload selected by `(CargoUnitId, helper_record_id)`.
    pub payload: StandaloneHelperPayload,
    /// Exact normalized identity selected by the same definition key.
    pub identity: SemanticIdentity,
}

/// SharedState definition after its required canonical schema payload has been
/// joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSharedStateDefinition {
    /// Rust-resolved definition record.
    pub definition: SharedStateDefinition,
    /// Exact payload selected by `(CargoUnitId, shared_state_record_id)`.
    pub payload: SharedStateDefinitionPayload,
}

/// One helper edge joined to the exact definition reached by its relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedHelperEdge {
    /// Source edge.
    pub edge: HelperEdge,
    /// Defining helper selected by the relocation target.
    pub definition: HelperDefinition,
}

/// Records found in one exact object or rlib path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRecords {
    /// Exact input path.
    pub artifact_path: PathBuf,
    /// Object formats encountered in this artifact, in member order.
    pub object_formats: Vec<BinaryFormat>,
    /// Component definitions from dedicated sections.
    pub definitions: Vec<ComponentDefinition>,
    /// Component edges from dedicated sections.
    pub edges: Vec<ComponentEdge>,
    /// Component input-field witnesses from their dedicated section.
    pub component_fields: Vec<ComponentField>,
    /// Client-helper definitions from their dedicated section.
    pub helper_definitions: Vec<HelperDefinition>,
    /// Client-helper edges from their dedicated section.
    pub helper_edges: Vec<HelperEdge>,
    /// SharedState definitions from their dedicated section.
    pub shared_state_definitions: Vec<SharedStateDefinition>,
    /// Component-local SharedState acquisition edges.
    pub shared_state_acquisitions: Vec<SharedStateAcquisition>,
    /// Complete component descriptor payloads.
    pub component_payloads: Vec<ComponentDescriptorPayload>,
    /// Complete standalone-helper descriptor payloads.
    pub helper_payloads: Vec<StandaloneHelperPayload>,
    /// Complete SharedState schema payloads.
    pub shared_state_payloads: Vec<SharedStateDefinitionPayload>,
    /// Normalized semantic identities from their dedicated inline section.
    pub semantic_identities: Vec<SemanticIdentity>,
}

impl ArtifactRecords {
    /// Extract records from one caller-owned byte snapshot.
    ///
    /// `artifact_path` is retained as the exact identity used in record
    /// locations, but this function never opens or otherwise reads that path.
    /// This lets an orchestrator authenticate `data` and parse the very same
    /// bytes without a hash-then-reopen race.
    pub fn extract_exact_bytes(
        artifact_path: impl AsRef<Path>,
        data: &[u8],
    ) -> Result<Self, ExtractionError> {
        let path = artifact_path.as_ref().to_path_buf();
        let kind = object::FileKind::parse(data).map_err(|error| {
            ExtractionError::UnsupportedArtifact {
                path: path.clone(),
                kind: error.to_string(),
            }
        })?;

        let mut records = Self {
            artifact_path: path.clone(),
            object_formats: Vec::new(),
            definitions: Vec::new(),
            edges: Vec::new(),
            component_fields: Vec::new(),
            helper_definitions: Vec::new(),
            helper_edges: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
            component_payloads: Vec::new(),
            helper_payloads: Vec::new(),
            shared_state_payloads: Vec::new(),
            semantic_identities: Vec::new(),
        };

        if kind == object::FileKind::Archive {
            extract_archive(&path, data, &mut records)?;
        } else {
            extract_object(
                &path,
                None,
                None,
                data,
                format!("{}", path.display()),
                &mut records,
            )?;
        }

        sort_shared_state_artifact_records(&mut records);
        Ok(records)
    }
}

/// Combined extraction and relocation resolution for an explicit artifact set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedTransport {
    /// Per-input extraction, preserving caller order.
    pub artifacts: Vec<ArtifactRecords>,
    /// All definitions in caller/artifact order.
    pub definitions: Vec<ComponentDefinition>,
    /// All edges in caller/artifact order.
    pub edges: Vec<ComponentEdge>,
    /// Edges resolved against only the supplied artifacts.
    pub resolved_edges: Vec<ResolvedComponentEdge>,
    /// Component fields resolved against only the supplied artifacts.
    pub component_fields: Vec<ComponentField>,
    /// Component fields joined to their exact defining component witness.
    pub resolved_component_fields: Vec<ResolvedComponentField>,
    /// All client-helper definitions in caller/artifact order.
    pub helper_definitions: Vec<HelperDefinition>,
    /// All client-helper edges in caller/artifact order.
    pub helper_edges: Vec<HelperEdge>,
    /// Helper edges resolved against only the supplied artifacts.
    pub resolved_helper_edges: Vec<ResolvedHelperEdge>,
    /// All SharedState definitions in deterministic transport-key order.
    pub shared_state_definitions: Vec<SharedStateDefinition>,
    /// All component-local SharedState acquisitions in deterministic
    /// transport-key order.
    pub shared_state_acquisitions: Vec<SharedStateAcquisition>,
    /// Acquisitions resolved against only the supplied artifacts.
    pub resolved_shared_state_acquisitions: Vec<ResolvedSharedStateAcquisition>,
    /// Component definitions joined to exactly one inline payload.
    pub resolved_definitions: Vec<ResolvedComponentDefinition>,
    /// Helper definitions joined to exactly one inline payload.
    pub resolved_helper_definitions: Vec<ResolvedHelperDefinition>,
    /// SharedState definitions joined to exactly one canonical schema payload.
    pub resolved_shared_state_definitions: Vec<ResolvedSharedStateDefinition>,
    /// All normalized semantic identities in caller/artifact order.
    pub semantic_identities: Vec<SemanticIdentity>,
}

/// Object/rlib transport failure.
#[derive(Debug, Error)]
pub enum ExtractionError {
    /// The exact caller-supplied path could not be read.
    #[error("failed to read exact RSJS artifact `{path}`: {source}")]
    ReadArtifact {
        /// Artifact path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The top-level artifact was neither an object nor an archive.
    #[error("unsupported RSJS artifact `{path}` ({kind}); expected a relocatable object or rlib")]
    UnsupportedArtifact {
        /// Artifact path.
        path: PathBuf,
        /// Parsed or diagnostic kind.
        kind: String,
    },
    /// Thin archives require paths outside the explicitly supplied artifact.
    #[error(
        "thin archive `{path}` is unsupported: RSJS extraction will not follow archive-relative paths"
    )]
    ThinArchive {
        /// Archive path.
        path: PathBuf,
    },
    /// Archive structure could not be decoded.
    #[error("failed to parse exact RSJS archive `{path}`: {reason}")]
    ParseArchive {
        /// Archive path.
        path: PathBuf,
        /// Parser detail.
        reason: String,
    },
    /// An object member could not be parsed.
    #[error("failed to parse RSJS object at {location}: {reason}")]
    ParseObject {
        /// Object/member location.
        location: String,
        /// Parser detail.
        reason: String,
    },
    /// The format has no accepted section/relocation contract.
    #[error("unsupported RSJS object format `{format}` at {location}")]
    UnsupportedObjectFormat {
        /// Object/member location.
        location: String,
        /// Debug format name.
        format: String,
    },
    /// Executables and linked dylibs do not preserve the descriptor-pass
    /// relocation contract.
    #[error("unsupported RSJS object kind `{kind}` at {location}; expected a relocatable object")]
    UnsupportedObjectKind {
        /// Object/member location.
        location: String,
        /// Debug object kind.
        kind: String,
    },
    /// A dedicated transport section could not be read.
    #[error("failed to read RSJS transport section at {location}: {reason}")]
    ReadSection {
        /// Section location.
        location: String,
        /// Parser detail.
        reason: String,
    },
    /// Non-record bytes appeared in a dedicated section.
    #[error("invalid bytes in dedicated RSJS section at {location}: {reason}")]
    InvalidSection {
        /// Section location.
        location: String,
        /// Validation detail.
        reason: String,
    },
    /// A record failed fixed-layout validation.
    #[error("invalid RSJS transport record at {location}: {reason}")]
    InvalidRecord {
        /// Record location.
        location: ArtifactLocation,
        /// Validation detail.
        reason: String,
    },
    /// The pointer slot did not have exactly one relocation.
    #[error("RSJS record at {location} requires exactly one witness relocation; found {found}")]
    RelocationCount {
        /// Record location.
        location: ArtifactLocation,
        /// Number of relocations at the declared slot.
        found: usize,
    },
    /// Relocation semantics are outside this prototype's accepted contract.
    #[error("unsupported RSJS witness relocation at {location}: {reason}")]
    UnsupportedRelocation {
        /// Record location.
        location: ArtifactLocation,
        /// Relocation detail.
        reason: String,
    },
    /// An edge referenced no definition in the exact supplied artifacts.
    #[error(
        "unresolved RSJS component witness `{witness}` from {edge}; supply the exact defining rlib/object from Cargo JSON"
    )]
    UnresolvedWitness {
        /// Edge record location.
        edge: Box<ArtifactLocation>,
        /// Relocation identity.
        witness: RelocationIdentity,
    },
    /// Multiple supplied definitions claim one relocation target.
    #[error("ambiguous RSJS component witness `{witness}`; definitions: {definitions:?}")]
    AmbiguousWitness {
        /// Relocation identity.
        witness: RelocationIdentity,
        /// Conflicting record locations.
        definitions: Vec<ArtifactLocation>,
    },
    /// A component-field record referenced no component definition in the
    /// exact supplied artifacts.
    #[error(
        "unresolved RSJS component-field witness `{witness}` from {field}; supply the exact defining rlib/object from Cargo JSON"
    )]
    UnresolvedComponentFieldWitness {
        /// Component-field record location.
        field: Box<ArtifactLocation>,
        /// Relocation identity.
        witness: RelocationIdentity,
    },
    /// Multiple supplied component definitions claim the witness targeted by
    /// one field record.
    #[error("ambiguous RSJS component-field witness `{witness}`; definitions: {definitions:?}")]
    AmbiguousComponentFieldWitness {
        /// Relocation identity.
        witness: RelocationIdentity,
        /// Conflicting component-definition locations.
        definitions: Vec<ArtifactLocation>,
    },
    /// Multiple component-field object records reused one exact transport key.
    #[error(
        "duplicate RSJS component-field records for Cargo unit {cargo_unit_id}, field record {field_record_id}: {fields:?}"
    )]
    DuplicateComponentFieldRecordId {
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Field record as lower-case hexadecimal.
        field_record_id: String,
        /// Conflicting component-field locations.
        fields: Vec<ArtifactLocation>,
    },
    /// A field record targeted a component definition owned by another Cargo
    /// unit.
    #[error(
        "RSJS component-field record at {field} belongs to Cargo unit {field_cargo_unit_id} but its defining component at {definition} belongs to Cargo unit {definition_cargo_unit_id}"
    )]
    ComponentFieldCargoUnitMismatch {
        /// Component-field record location.
        field: Box<ArtifactLocation>,
        /// Field record Cargo unit as lower-case hexadecimal.
        field_cargo_unit_id: String,
        /// Component-definition location.
        definition: Box<ArtifactLocation>,
        /// Component-definition Cargo unit as lower-case hexadecimal.
        definition_cargo_unit_id: String,
    },
    /// A helper edge referenced no definition in the exact supplied artifacts.
    #[error(
        "unresolved RSJS client-helper function `{helper}` from {edge}; supply the exact defining rlib/object from Cargo JSON"
    )]
    UnresolvedHelper {
        /// Helper edge record location.
        edge: Box<ArtifactLocation>,
        /// Rust-resolved helper-function relocation identity.
        helper: RelocationIdentity,
    },
    /// Multiple supplied helper definitions claim one function relocation.
    #[error("ambiguous RSJS client-helper function `{helper}`; definitions: {definitions:?}")]
    AmbiguousHelper {
        /// Rust-resolved helper-function relocation identity.
        helper: RelocationIdentity,
        /// Conflicting record locations.
        definitions: Vec<ArtifactLocation>,
    },
    /// Multiple SharedState definitions reused one Rust type witness.
    #[error("ambiguous RSJS SharedState type witness `{witness}`; definitions: {definitions:?}")]
    AmbiguousSharedStateWitness {
        /// Rust-resolved SharedState type witness.
        witness: RelocationIdentity,
        /// Conflicting definition locations.
        definitions: Vec<ArtifactLocation>,
    },
    /// A SharedState definition reused a relocation owned by a different
    /// definition kind.
    #[error(
        "wrong-kind RSJS SharedState definition witness `{witness}` at {definition}; relocation is already claimed by {actual_kind} definition(s): {actual_definitions:?}"
    )]
    WrongKindSharedStateDefinitionWitness {
        /// Forged SharedState definition.
        definition: Box<ArtifactLocation>,
        /// Conflicting relocation identity.
        witness: Box<RelocationIdentity>,
        /// Existing definition kind.
        actual_kind: &'static str,
        /// Existing definitions at that relocation.
        actual_definitions: Vec<ArtifactLocation>,
    },
    /// A SharedState acquisition referenced no SharedState definition in the
    /// exact supplied artifacts.
    #[error(
        "unresolved RSJS SharedState type witness `{witness}` from {acquisition}; supply the exact defining rlib/object from Cargo JSON"
    )]
    UnresolvedSharedStateWitness {
        /// Acquisition record location.
        acquisition: Box<ArtifactLocation>,
        /// Rust-resolved type witness relocation.
        witness: RelocationIdentity,
    },
    /// A SharedState acquisition targeted a relocation belonging to a
    /// component or helper definition.
    #[error(
        "wrong-kind RSJS SharedState acquisition witness `{witness}` from {acquisition}; relocation resolves to {actual_kind} definition(s): {actual_definitions:?}"
    )]
    WrongKindSharedStateAcquisitionWitness {
        /// Acquisition record location.
        acquisition: Box<ArtifactLocation>,
        /// Rust-resolved type witness relocation.
        witness: Box<RelocationIdentity>,
        /// Resolved non-SharedState definition kind.
        actual_kind: &'static str,
        /// Definitions at that relocation.
        actual_definitions: Vec<ArtifactLocation>,
    },
    /// Multiple SharedState definition records reused one exact transport key.
    #[error(
        "duplicate RSJS SharedState definition records for Cargo unit {cargo_unit_id}, definition record {shared_state_record_id}: {definitions:?}"
    )]
    DuplicateSharedStateDefinitionRecordId {
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// SharedState definition record as lower-case hexadecimal.
        shared_state_record_id: String,
        /// Conflicting definitions.
        definitions: Vec<ArtifactLocation>,
    },
    /// Multiple SharedState acquisition records reused one exact transport key.
    #[error(
        "duplicate RSJS SharedState acquisition records for Cargo unit {cargo_unit_id}, source edge {source_edge_id}: {acquisitions:?}"
    )]
    DuplicateSharedStateAcquisitionRecordId {
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Acquisition/source edge as lower-case hexadecimal.
        source_edge_id: String,
        /// Conflicting acquisitions.
        acquisitions: Vec<ArtifactLocation>,
    },
    /// The hash embedded in a canonical SharedState schema payload did not
    /// match its schema ABI bytes.
    #[error(
        "RSJS SharedState schema ABI hash mismatch at {location}: recorded {recorded}, computed {computed}"
    )]
    SharedStateSchemaAbiHashMismatch {
        /// Payload record location.
        location: Box<ArtifactLocation>,
        /// Recorded lower-case hexadecimal digest.
        recorded: String,
        /// Recomputed lower-case hexadecimal digest.
        computed: String,
    },
    /// A payload's embedded digest did not match its inline bytes.
    #[error(
        "RSJS {kind} payload hash mismatch at {location}: recorded {recorded}, computed {computed}"
    )]
    PayloadHashMismatch {
        /// Payload record location.
        location: Box<ArtifactLocation>,
        /// Human-readable payload kind.
        kind: &'static str,
        /// Recorded lower-case hexadecimal digest.
        recorded: String,
        /// Recomputed lower-case hexadecimal digest.
        computed: String,
    },
    /// One defining record had no exact inline payload.
    #[error(
        "missing RSJS {kind} payload for Cargo unit {cargo_unit_id}, definition record {definition_record_id} at {definition}; descriptor sidecars are non-authoritative"
    )]
    MissingDefinitionPayload {
        /// Definition kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Defining relocation record.
        definition: Box<ArtifactLocation>,
    },
    /// More than one payload claimed the same `(CargoUnitId, definition ID)`.
    #[error(
        "duplicate RSJS {kind} payloads for Cargo unit {cargo_unit_id}, definition record {definition_record_id}: {payloads:?}"
    )]
    DuplicateDefinitionPayload {
        /// Payload kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Conflicting payload locations.
        payloads: Vec<ArtifactLocation>,
    },
    /// A payload did not have a matching definition in the exact artifact set.
    #[error(
        "orphan RSJS {kind} payload for Cargo unit {cargo_unit_id}, definition record {definition_record_id} at {payload}; supply the exact defining object/rlib from Cargo JSON"
    )]
    OrphanDefinitionPayload {
        /// Payload kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Orphan payload location.
        payload: Box<ArtifactLocation>,
    },
    /// Multiple definitions reused one build-local join key.
    #[error(
        "duplicate RSJS {kind} definition records for Cargo unit {cargo_unit_id}, definition record {definition_record_id}: {definitions:?}"
    )]
    DuplicateDefinitionRecordId {
        /// Definition kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Conflicting definition locations.
        definitions: Vec<ArtifactLocation>,
    },
    /// One defining record had no exact normalized identity record.
    #[error(
        "missing RSJS {kind} semantic identity for Cargo unit {cargo_unit_id}, definition record {definition_record_id} at {definition}; token paths and symbol demangling are non-authoritative"
    )]
    MissingSemanticIdentity {
        /// Definition kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Defining relocation record.
        definition: Box<ArtifactLocation>,
    },
    /// More than one identity claimed the same exact definition.
    #[error(
        "duplicate RSJS {kind} semantic identities for Cargo unit {cargo_unit_id}, definition record {definition_record_id}: {identities:?}"
    )]
    DuplicateSemanticIdentity {
        /// Definition kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Conflicting identity locations.
        identities: Vec<ArtifactLocation>,
    },
    /// An identity did not correspond to an exact supplied definition.
    #[error(
        "orphan RSJS {kind} semantic identity for Cargo unit {cargo_unit_id}, definition record {definition_record_id} at {identity}; supply only the exact Cargo-enumerated object/rlib set"
    )]
    OrphanSemanticIdentity {
        /// Identity kind.
        kind: &'static str,
        /// Cargo unit as lower-case hexadecimal.
        cargo_unit_id: String,
        /// Definition record as lower-case hexadecimal.
        definition_record_id: String,
        /// Orphan identity location.
        identity: Box<ArtifactLocation>,
    },
}

/// Extract records from one exact path.
///
/// This function never discovers neighboring files.
pub fn extract_exact_artifact(path: impl AsRef<Path>) -> Result<ArtifactRecords, ExtractionError> {
    let path = path.as_ref().to_path_buf();
    let data = fs::read(&path).map_err(|source| ExtractionError::ReadArtifact {
        path: path.clone(),
        source,
    })?;
    ArtifactRecords::extract_exact_bytes(path, &data)
}

/// Extract and resolve records from exactly the caller-supplied artifact list.
pub fn extract_exact_artifacts<I, P>(paths: I) -> Result<ExtractedTransport, ExtractionError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let artifacts = paths
        .into_iter()
        .map(extract_exact_artifact)
        .collect::<Result<Vec<_>, _>>()?;
    ExtractedTransport::resolve_exact_artifacts(artifacts)
}

impl ExtractedTransport {
    /// Resolve records already extracted from an exact caller-supplied
    /// artifact set.
    ///
    /// Artifact order and path identity are preserved. No artifact path is
    /// opened by this operation.
    pub fn resolve_exact_artifacts(
        artifacts: Vec<ArtifactRecords>,
    ) -> Result<Self, ExtractionError> {
        resolve_extracted_artifacts(artifacts)
    }
}

fn resolve_extracted_artifacts(
    artifacts: Vec<ArtifactRecords>,
) -> Result<ExtractedTransport, ExtractionError> {
    let definitions = artifacts
        .iter()
        .flat_map(|artifact| artifact.definitions.iter().cloned())
        .collect::<Vec<_>>();
    let edges = artifacts
        .iter()
        .flat_map(|artifact| artifact.edges.iter().cloned())
        .collect::<Vec<_>>();
    let component_fields = artifacts
        .iter()
        .flat_map(|artifact| artifact.component_fields.iter().cloned())
        .collect::<Vec<_>>();
    let helper_definitions = artifacts
        .iter()
        .flat_map(|artifact| artifact.helper_definitions.iter().cloned())
        .collect::<Vec<_>>();
    let helper_edges = artifacts
        .iter()
        .flat_map(|artifact| artifact.helper_edges.iter().cloned())
        .collect::<Vec<_>>();
    let mut shared_state_definitions = artifacts
        .iter()
        .flat_map(|artifact| artifact.shared_state_definitions.iter().cloned())
        .collect::<Vec<_>>();
    shared_state_definitions.sort_by(shared_state_definition_order);
    let mut shared_state_acquisitions = artifacts
        .iter()
        .flat_map(|artifact| artifact.shared_state_acquisitions.iter().cloned())
        .collect::<Vec<_>>();
    shared_state_acquisitions.sort_by(shared_state_acquisition_order);
    let component_payloads = artifacts
        .iter()
        .flat_map(|artifact| artifact.component_payloads.iter().cloned())
        .collect::<Vec<_>>();
    let helper_payloads = artifacts
        .iter()
        .flat_map(|artifact| artifact.helper_payloads.iter().cloned())
        .collect::<Vec<_>>();
    let shared_state_payloads = artifacts
        .iter()
        .flat_map(|artifact| artifact.shared_state_payloads.iter().cloned())
        .collect::<Vec<_>>();
    let semantic_identities = artifacts
        .iter()
        .flat_map(|artifact| artifact.semantic_identities.iter().cloned())
        .collect::<Vec<_>>();

    let mut definitions_by_witness =
        BTreeMap::<RelocationIdentity, Vec<ComponentDefinition>>::new();
    for definition in &definitions {
        definitions_by_witness
            .entry(definition.witness.clone())
            .or_default()
            .push(definition.clone());
    }

    let mut resolved_edges = Vec::with_capacity(edges.len());
    for edge in &edges {
        let Some(candidates) = definitions_by_witness.get(&edge.witness) else {
            return Err(ExtractionError::UnresolvedWitness {
                edge: Box::new(edge.location.clone()),
                witness: edge.witness.clone(),
            });
        };
        if candidates.len() != 1 {
            return Err(ExtractionError::AmbiguousWitness {
                witness: edge.witness.clone(),
                definitions: candidates
                    .iter()
                    .map(|candidate| candidate.location.clone())
                    .collect(),
            });
        }
        resolved_edges.push(ResolvedComponentEdge {
            edge: edge.clone(),
            definition: candidates[0].clone(),
        });
    }

    let resolved_component_fields =
        resolve_component_fields(&component_fields, &definitions_by_witness)?;

    let mut helper_definitions_by_function =
        BTreeMap::<RelocationIdentity, Vec<HelperDefinition>>::new();
    for definition in &helper_definitions {
        helper_definitions_by_function
            .entry(definition.helper.clone())
            .or_default()
            .push(definition.clone());
    }
    if let Some((helper, candidates)) = helper_definitions_by_function
        .iter()
        .find(|(_, candidates)| candidates.len() != 1)
    {
        return Err(ExtractionError::AmbiguousHelper {
            helper: helper.clone(),
            definitions: candidates
                .iter()
                .map(|candidate| candidate.location.clone())
                .collect(),
        });
    }

    let mut resolved_helper_edges = Vec::with_capacity(helper_edges.len());
    for edge in &helper_edges {
        let Some(candidates) = helper_definitions_by_function.get(&edge.helper) else {
            return Err(ExtractionError::UnresolvedHelper {
                edge: Box::new(edge.location.clone()),
                helper: edge.helper.clone(),
            });
        };
        if candidates.len() != 1 {
            return Err(ExtractionError::AmbiguousHelper {
                helper: edge.helper.clone(),
                definitions: candidates
                    .iter()
                    .map(|candidate| candidate.location.clone())
                    .collect(),
            });
        }
        resolved_helper_edges.push(ResolvedHelperEdge {
            edge: edge.clone(),
            definition: candidates[0].clone(),
        });
    }

    let resolved_shared_state_acquisitions = resolve_shared_state_acquisitions(
        &shared_state_definitions,
        &shared_state_acquisitions,
        &definitions_by_witness,
        &helper_definitions_by_function,
    )?;
    let resolved_definitions =
        join_component_payloads(&definitions, &component_payloads, &semantic_identities)?;
    let resolved_helper_definitions =
        join_helper_payloads(&helper_definitions, &helper_payloads, &semantic_identities)?;
    let resolved_shared_state_definitions =
        join_shared_state_payloads(&shared_state_definitions, &shared_state_payloads)?;

    Ok(ExtractedTransport {
        artifacts,
        definitions,
        edges,
        resolved_edges,
        component_fields,
        resolved_component_fields,
        helper_definitions,
        helper_edges,
        resolved_helper_edges,
        shared_state_definitions,
        shared_state_acquisitions,
        resolved_shared_state_acquisitions,
        resolved_definitions,
        resolved_helper_definitions,
        resolved_shared_state_definitions,
        semantic_identities,
    })
}

type DefinitionJoinKey = (TransportRecordId, TransportRecordId);

fn shared_state_definition_order(
    left: &SharedStateDefinition,
    right: &SharedStateDefinition,
) -> std::cmp::Ordering {
    left.cargo_unit_id
        .cmp(&right.cargo_unit_id)
        .then_with(|| {
            left.shared_state_record_id
                .cmp(&right.shared_state_record_id)
        })
        .then_with(|| left.location.cmp(&right.location))
}

fn shared_state_acquisition_order(
    left: &SharedStateAcquisition,
    right: &SharedStateAcquisition,
) -> std::cmp::Ordering {
    left.cargo_unit_id
        .cmp(&right.cargo_unit_id)
        .then_with(|| left.source_edge_id.cmp(&right.source_edge_id))
        .then_with(|| left.location.cmp(&right.location))
}

fn sort_shared_state_artifact_records(records: &mut ArtifactRecords) {
    records
        .shared_state_definitions
        .sort_by(shared_state_definition_order);
    records
        .shared_state_acquisitions
        .sort_by(shared_state_acquisition_order);
    records.shared_state_payloads.sort_by(|left, right| {
        left.cargo_unit_id
            .cmp(&right.cargo_unit_id)
            .then_with(|| {
                left.shared_state_record_id
                    .cmp(&right.shared_state_record_id)
            })
            .then_with(|| left.location.cmp(&right.location))
    });
}

fn resolve_shared_state_acquisitions(
    definitions: &[SharedStateDefinition],
    acquisitions: &[SharedStateAcquisition],
    component_definitions: &BTreeMap<RelocationIdentity, Vec<ComponentDefinition>>,
    helper_definitions: &BTreeMap<RelocationIdentity, Vec<HelperDefinition>>,
) -> Result<Vec<ResolvedSharedStateAcquisition>, ExtractionError> {
    let mut definitions_by_key = BTreeMap::<DefinitionJoinKey, Vec<&SharedStateDefinition>>::new();
    for definition in definitions {
        definitions_by_key
            .entry((definition.cargo_unit_id, definition.shared_state_record_id))
            .or_default()
            .push(definition);
    }
    if let Some(((cargo_unit_id, shared_state_record_id), candidates)) = definitions_by_key
        .iter()
        .find(|(_, candidates)| candidates.len() != 1)
    {
        return Err(ExtractionError::DuplicateSharedStateDefinitionRecordId {
            cargo_unit_id: id_hex(cargo_unit_id),
            shared_state_record_id: id_hex(shared_state_record_id),
            definitions: candidates
                .iter()
                .map(|candidate| candidate.location.clone())
                .collect(),
        });
    }

    let mut definitions_by_witness =
        BTreeMap::<RelocationIdentity, Vec<&SharedStateDefinition>>::new();
    for definition in definitions {
        definitions_by_witness
            .entry(definition.witness.clone())
            .or_default()
            .push(definition);
    }
    if let Some((witness, candidates)) = definitions_by_witness
        .iter()
        .find(|(_, candidates)| candidates.len() != 1)
    {
        return Err(ExtractionError::AmbiguousSharedStateWitness {
            witness: witness.clone(),
            definitions: candidates
                .iter()
                .map(|candidate| candidate.location.clone())
                .collect(),
        });
    }

    for definition in definitions {
        if let Some(conflicts) = component_definitions.get(&definition.witness) {
            return Err(ExtractionError::WrongKindSharedStateDefinitionWitness {
                definition: Box::new(definition.location.clone()),
                witness: Box::new(definition.witness.clone()),
                actual_kind: "component",
                actual_definitions: conflicts
                    .iter()
                    .map(|candidate| candidate.location.clone())
                    .collect(),
            });
        }
        if let Some(conflicts) = helper_definitions.get(&definition.witness) {
            return Err(ExtractionError::WrongKindSharedStateDefinitionWitness {
                definition: Box::new(definition.location.clone()),
                witness: Box::new(definition.witness.clone()),
                actual_kind: "standalone helper",
                actual_definitions: conflicts
                    .iter()
                    .map(|candidate| candidate.location.clone())
                    .collect(),
            });
        }
    }

    let mut acquisitions_by_key =
        BTreeMap::<DefinitionJoinKey, Vec<&SharedStateAcquisition>>::new();
    for acquisition in acquisitions {
        acquisitions_by_key
            .entry((acquisition.cargo_unit_id, acquisition.source_edge_id))
            .or_default()
            .push(acquisition);
    }
    if let Some(((cargo_unit_id, source_edge_id), candidates)) = acquisitions_by_key
        .iter()
        .find(|(_, candidates)| candidates.len() != 1)
    {
        return Err(ExtractionError::DuplicateSharedStateAcquisitionRecordId {
            cargo_unit_id: id_hex(cargo_unit_id),
            source_edge_id: id_hex(source_edge_id),
            acquisitions: candidates
                .iter()
                .map(|candidate| candidate.location.clone())
                .collect(),
        });
    }

    let mut resolved = Vec::with_capacity(acquisitions.len());
    for acquisition in acquisitions {
        let Some(candidates) = definitions_by_witness.get(&acquisition.witness) else {
            if let Some(conflicts) = component_definitions.get(&acquisition.witness) {
                return Err(ExtractionError::WrongKindSharedStateAcquisitionWitness {
                    acquisition: Box::new(acquisition.location.clone()),
                    witness: Box::new(acquisition.witness.clone()),
                    actual_kind: "component",
                    actual_definitions: conflicts
                        .iter()
                        .map(|candidate| candidate.location.clone())
                        .collect(),
                });
            }
            if let Some(conflicts) = helper_definitions.get(&acquisition.witness) {
                return Err(ExtractionError::WrongKindSharedStateAcquisitionWitness {
                    acquisition: Box::new(acquisition.location.clone()),
                    witness: Box::new(acquisition.witness.clone()),
                    actual_kind: "standalone helper",
                    actual_definitions: conflicts
                        .iter()
                        .map(|candidate| candidate.location.clone())
                        .collect(),
                });
            }
            return Err(ExtractionError::UnresolvedSharedStateWitness {
                acquisition: Box::new(acquisition.location.clone()),
                witness: acquisition.witness.clone(),
            });
        };
        debug_assert_eq!(candidates.len(), 1);
        resolved.push(ResolvedSharedStateAcquisition {
            acquisition: acquisition.clone(),
            definition: (*candidates[0]).clone(),
        });
    }
    Ok(resolved)
}

fn resolve_component_fields(
    fields: &[ComponentField],
    definitions_by_witness: &BTreeMap<RelocationIdentity, Vec<ComponentDefinition>>,
) -> Result<Vec<ResolvedComponentField>, ExtractionError> {
    let mut fields_by_key = BTreeMap::<DefinitionJoinKey, Vec<&ComponentField>>::new();
    for field in fields {
        fields_by_key
            .entry((field.cargo_unit_id, field.field_record_id))
            .or_default()
            .push(field);
    }
    if let Some(((cargo_unit_id, field_record_id), candidates)) = fields_by_key
        .iter()
        .find(|(_, candidates)| candidates.len() != 1)
    {
        return Err(ExtractionError::DuplicateComponentFieldRecordId {
            cargo_unit_id: id_hex(cargo_unit_id),
            field_record_id: id_hex(field_record_id),
            fields: candidates
                .iter()
                .map(|candidate| candidate.location.clone())
                .collect(),
        });
    }

    let mut resolved = Vec::with_capacity(fields.len());
    for field in fields {
        let Some(candidates) = definitions_by_witness.get(&field.witness) else {
            return Err(ExtractionError::UnresolvedComponentFieldWitness {
                field: Box::new(field.location.clone()),
                witness: field.witness.clone(),
            });
        };
        if candidates.len() != 1 {
            return Err(ExtractionError::AmbiguousComponentFieldWitness {
                witness: field.witness.clone(),
                definitions: candidates
                    .iter()
                    .map(|candidate| candidate.location.clone())
                    .collect(),
            });
        }
        let definition = &candidates[0];
        if field.cargo_unit_id != definition.cargo_unit_id {
            return Err(ExtractionError::ComponentFieldCargoUnitMismatch {
                field: Box::new(field.location.clone()),
                field_cargo_unit_id: id_hex(&field.cargo_unit_id),
                definition: Box::new(definition.location.clone()),
                definition_cargo_unit_id: id_hex(&definition.cargo_unit_id),
            });
        }
        resolved.push(ResolvedComponentField {
            field: field.clone(),
            definition: definition.clone(),
        });
    }
    Ok(resolved)
}

trait DefinitionLocation {
    fn location(&self) -> &ArtifactLocation;
}

impl DefinitionLocation for ComponentDefinition {
    fn location(&self) -> &ArtifactLocation {
        &self.location
    }
}

impl DefinitionLocation for HelperDefinition {
    fn location(&self) -> &ArtifactLocation {
        &self.location
    }
}

impl DefinitionLocation for SharedStateDefinition {
    fn location(&self) -> &ArtifactLocation {
        &self.location
    }
}

fn join_component_payloads(
    definitions: &[ComponentDefinition],
    payloads: &[ComponentDescriptorPayload],
    identities: &[SemanticIdentity],
) -> Result<Vec<ResolvedComponentDefinition>, ExtractionError> {
    let mut definitions_by_key = BTreeMap::<DefinitionJoinKey, Vec<&ComponentDefinition>>::new();
    for definition in definitions {
        definitions_by_key
            .entry((definition.cargo_unit_id, definition.component_record_id))
            .or_default()
            .push(definition);
    }
    reject_duplicate_definition_keys(
        "component",
        definitions_by_key.iter().map(|(key, definitions)| {
            (
                *key,
                definitions
                    .iter()
                    .map(|definition| definition.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;

    let mut payloads_by_key =
        BTreeMap::<DefinitionJoinKey, Vec<&ComponentDescriptorPayload>>::new();
    for payload in payloads {
        payloads_by_key
            .entry((payload.cargo_unit_id, payload.component_record_id))
            .or_default()
            .push(payload);
    }
    reject_duplicate_payload_keys(
        "component",
        payloads_by_key.iter().map(|(key, payloads)| {
            (
                *key,
                payloads
                    .iter()
                    .map(|payload| payload.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;
    reject_orphan_payloads(
        "component",
        &definitions_by_key,
        payloads_by_key
            .iter()
            .map(|(key, payloads)| (*key, payloads[0].location.clone())),
    )?;
    for definition in definitions {
        let key = (definition.cargo_unit_id, definition.component_record_id);
        if !payloads_by_key.contains_key(&key) {
            return Err(missing_payload_error(
                "component",
                key,
                definition.location.clone(),
            ));
        }
    }
    let identities_by_key = join_semantic_identities(
        SemanticIdentityKind::Component,
        &definitions_by_key,
        identities,
    )?;

    definitions
        .iter()
        .map(|definition| {
            let key = (definition.cargo_unit_id, definition.component_record_id);
            let payload = payloads_by_key[&key][0];
            Ok(ResolvedComponentDefinition {
                definition: definition.clone(),
                payload: payload.clone(),
                identity: identities_by_key[&key].clone(),
            })
        })
        .collect()
}

fn join_helper_payloads(
    definitions: &[HelperDefinition],
    payloads: &[StandaloneHelperPayload],
    identities: &[SemanticIdentity],
) -> Result<Vec<ResolvedHelperDefinition>, ExtractionError> {
    let mut definitions_by_key = BTreeMap::<DefinitionJoinKey, Vec<&HelperDefinition>>::new();
    for definition in definitions {
        definitions_by_key
            .entry((definition.cargo_unit_id, definition.helper_record_id))
            .or_default()
            .push(definition);
    }
    reject_duplicate_definition_keys(
        "standalone helper",
        definitions_by_key.iter().map(|(key, definitions)| {
            (
                *key,
                definitions
                    .iter()
                    .map(|definition| definition.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;

    let mut payloads_by_key = BTreeMap::<DefinitionJoinKey, Vec<&StandaloneHelperPayload>>::new();
    for payload in payloads {
        payloads_by_key
            .entry((payload.cargo_unit_id, payload.helper_record_id))
            .or_default()
            .push(payload);
    }
    reject_duplicate_payload_keys(
        "standalone helper",
        payloads_by_key.iter().map(|(key, payloads)| {
            (
                *key,
                payloads
                    .iter()
                    .map(|payload| payload.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;
    reject_orphan_payloads(
        "standalone helper",
        &definitions_by_key,
        payloads_by_key
            .iter()
            .map(|(key, payloads)| (*key, payloads[0].location.clone())),
    )?;
    for definition in definitions {
        let key = (definition.cargo_unit_id, definition.helper_record_id);
        if !payloads_by_key.contains_key(&key) {
            return Err(missing_payload_error(
                "standalone helper",
                key,
                definition.location.clone(),
            ));
        }
    }
    let identities_by_key = join_semantic_identities(
        SemanticIdentityKind::Helper,
        &definitions_by_key,
        identities,
    )?;

    definitions
        .iter()
        .map(|definition| {
            let key = (definition.cargo_unit_id, definition.helper_record_id);
            let payload = payloads_by_key[&key][0];
            Ok(ResolvedHelperDefinition {
                definition: definition.clone(),
                payload: payload.clone(),
                identity: identities_by_key[&key].clone(),
            })
        })
        .collect()
}

fn join_shared_state_payloads(
    definitions: &[SharedStateDefinition],
    payloads: &[SharedStateDefinitionPayload],
) -> Result<Vec<ResolvedSharedStateDefinition>, ExtractionError> {
    let mut definitions_by_key = BTreeMap::<DefinitionJoinKey, Vec<&SharedStateDefinition>>::new();
    for definition in definitions {
        definitions_by_key
            .entry((definition.cargo_unit_id, definition.shared_state_record_id))
            .or_default()
            .push(definition);
    }
    reject_duplicate_definition_keys(
        "SharedState",
        definitions_by_key.iter().map(|(key, definitions)| {
            (
                *key,
                definitions
                    .iter()
                    .map(|definition| definition.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;

    let mut payloads_by_key =
        BTreeMap::<DefinitionJoinKey, Vec<&SharedStateDefinitionPayload>>::new();
    for payload in payloads {
        payloads_by_key
            .entry((payload.cargo_unit_id, payload.shared_state_record_id))
            .or_default()
            .push(payload);
    }
    reject_duplicate_payload_keys(
        "SharedState schema",
        payloads_by_key.iter().map(|(key, payloads)| {
            (
                *key,
                payloads
                    .iter()
                    .map(|payload| payload.location.clone())
                    .collect::<Vec<_>>(),
            )
        }),
    )?;
    reject_orphan_payloads(
        "SharedState schema",
        &definitions_by_key,
        payloads_by_key
            .iter()
            .map(|(key, payloads)| (*key, payloads[0].location.clone())),
    )?;
    for definition in definitions {
        let key = (definition.cargo_unit_id, definition.shared_state_record_id);
        if !payloads_by_key.contains_key(&key) {
            return Err(missing_payload_error(
                "SharedState schema",
                key,
                definition.location.clone(),
            ));
        }
    }

    definitions
        .iter()
        .map(|definition| {
            let key = (definition.cargo_unit_id, definition.shared_state_record_id);
            Ok(ResolvedSharedStateDefinition {
                definition: definition.clone(),
                payload: payloads_by_key[&key][0].clone(),
            })
        })
        .collect()
}

fn reject_duplicate_definition_keys<I>(
    kind: &'static str,
    entries: I,
) -> Result<(), ExtractionError>
where
    I: IntoIterator<Item = (DefinitionJoinKey, Vec<ArtifactLocation>)>,
{
    for ((cargo_unit_id, definition_record_id), definitions) in entries {
        if definitions.len() > 1 {
            return Err(ExtractionError::DuplicateDefinitionRecordId {
                kind,
                cargo_unit_id: id_hex(&cargo_unit_id),
                definition_record_id: id_hex(&definition_record_id),
                definitions,
            });
        }
    }
    Ok(())
}

fn reject_duplicate_payload_keys<I>(kind: &'static str, entries: I) -> Result<(), ExtractionError>
where
    I: IntoIterator<Item = (DefinitionJoinKey, Vec<ArtifactLocation>)>,
{
    for ((cargo_unit_id, definition_record_id), payloads) in entries {
        if payloads.len() > 1 {
            return Err(ExtractionError::DuplicateDefinitionPayload {
                kind,
                cargo_unit_id: id_hex(&cargo_unit_id),
                definition_record_id: id_hex(&definition_record_id),
                payloads,
            });
        }
    }
    Ok(())
}

fn reject_orphan_payloads<T, I>(
    kind: &'static str,
    definitions: &BTreeMap<DefinitionJoinKey, Vec<&T>>,
    payloads: I,
) -> Result<(), ExtractionError>
where
    I: IntoIterator<Item = (DefinitionJoinKey, ArtifactLocation)>,
{
    for ((cargo_unit_id, definition_record_id), payload) in payloads {
        if !definitions.contains_key(&(cargo_unit_id, definition_record_id)) {
            return Err(ExtractionError::OrphanDefinitionPayload {
                kind,
                cargo_unit_id: id_hex(&cargo_unit_id),
                definition_record_id: id_hex(&definition_record_id),
                payload: Box::new(payload),
            });
        }
    }
    Ok(())
}

fn join_semantic_identities<T: DefinitionLocation>(
    expected_kind: SemanticIdentityKind,
    definitions: &BTreeMap<DefinitionJoinKey, Vec<&T>>,
    identities: &[SemanticIdentity],
) -> Result<BTreeMap<DefinitionJoinKey, SemanticIdentity>, ExtractionError> {
    let label = semantic_identity_kind_label(expected_kind);
    let mut by_key = BTreeMap::<DefinitionJoinKey, Vec<&SemanticIdentity>>::new();
    for identity in identities
        .iter()
        .filter(|identity| identity.kind == expected_kind)
    {
        by_key
            .entry((identity.cargo_unit_id, identity.definition_record_id))
            .or_default()
            .push(identity);
    }

    for (key, candidates) in &by_key {
        if candidates.len() != 1 {
            return Err(ExtractionError::DuplicateSemanticIdentity {
                kind: label,
                cargo_unit_id: id_hex(&key.0),
                definition_record_id: id_hex(&key.1),
                identities: candidates
                    .iter()
                    .map(|identity| identity.location.clone())
                    .collect(),
            });
        }
        if !definitions.contains_key(key) {
            return Err(ExtractionError::OrphanSemanticIdentity {
                kind: label,
                cargo_unit_id: id_hex(&key.0),
                definition_record_id: id_hex(&key.1),
                identity: Box::new(candidates[0].location.clone()),
            });
        }
    }

    let mut joined = BTreeMap::new();
    for (key, candidates) in definitions {
        let Some(identity) = by_key.get(key).and_then(|values| values.first()) else {
            return Err(ExtractionError::MissingSemanticIdentity {
                kind: label,
                cargo_unit_id: id_hex(&key.0),
                definition_record_id: id_hex(&key.1),
                definition: Box::new(
                    candidates
                        .first()
                        .expect("definition keys are never empty")
                        .location()
                        .clone(),
                ),
            });
        };
        joined.insert(*key, (*identity).clone());
    }
    Ok(joined)
}

fn missing_payload_error(
    kind: &'static str,
    (cargo_unit_id, definition_record_id): DefinitionJoinKey,
    definition: ArtifactLocation,
) -> ExtractionError {
    ExtractionError::MissingDefinitionPayload {
        kind,
        cargo_unit_id: id_hex(&cargo_unit_id),
        definition_record_id: id_hex(&definition_record_id),
        definition: Box::new(definition),
    }
}

fn id_hex(id: &TransportRecordId) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(id.len() * 2);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn extract_archive(
    path: &Path,
    data: &[u8],
    records: &mut ArtifactRecords,
) -> Result<(), ExtractionError> {
    let archive = ArchiveFile::parse(data).map_err(|error| ExtractionError::ParseArchive {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    if archive.is_thin() {
        return Err(ExtractionError::ThinArchive {
            path: path.to_path_buf(),
        });
    }

    for (member_index, member) in archive.members().enumerate() {
        let member = member.map_err(|error| ExtractionError::ParseArchive {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        let member_name = String::from_utf8_lossy(member.name()).into_owned();
        let member_data = member
            .data(data)
            .map_err(|error| ExtractionError::ParseArchive {
                path: path.to_path_buf(),
                reason: format!("member `{member_name}`: {error}"),
            })?;

        let member_kind = match object::FileKind::parse(member_data) {
            Ok(kind) => kind,
            Err(error) if member_name.ends_with(".o") || member_name.ends_with(".obj") => {
                return Err(ExtractionError::ParseObject {
                    location: format!("{}({member_name})", path.display()),
                    reason: error.to_string(),
                });
            }
            Err(_) => {
                // Rust rlibs also contain metadata members. They carry no
                // native object sections and are intentionally ignored.
                continue;
            }
        };
        if member_kind == object::FileKind::Archive {
            return Err(ExtractionError::UnsupportedArtifact {
                path: path.to_path_buf(),
                kind: format!("nested archive member `{member_name}`"),
            });
        }

        extract_object(
            path,
            Some(member_name.clone()),
            Some(member_index),
            member_data,
            format!("{}({member_name})", path.display()),
            records,
        )?;
    }
    Ok(())
}

fn extract_object(
    artifact_path: &Path,
    archive_member: Option<String>,
    archive_member_index: Option<usize>,
    data: &[u8],
    object_location: String,
    records: &mut ArtifactRecords,
) -> Result<(), ExtractionError> {
    let file = object::File::parse(data).map_err(|error| ExtractionError::ParseObject {
        location: object_location.clone(),
        reason: error.to_string(),
    })?;
    let format = file.format();
    if !matches!(
        format,
        BinaryFormat::MachO | BinaryFormat::Elf | BinaryFormat::Coff
    ) {
        return Err(ExtractionError::UnsupportedObjectFormat {
            location: object_location,
            format: format!("{format:?}"),
        });
    }
    if file.kind() != ObjectKind::Relocatable {
        return Err(ExtractionError::UnsupportedObjectKind {
            location: object_location,
            kind: format!("{:?}", file.kind()),
        });
    }
    records.object_formats.push(format);

    for section in file.sections() {
        let section_name = section
            .name()
            .map_err(|error| ExtractionError::ReadSection {
                location: object_location.clone(),
                reason: error.to_string(),
            })?;
        let segment_name =
            section
                .segment_name()
                .map_err(|error| ExtractionError::ReadSection {
                    location: object_location.clone(),
                    reason: error.to_string(),
                })?;
        let Some(section_kind) = transport_section_kind(format, segment_name, section_name) else {
            continue;
        };
        let section_data = section
            .data()
            .map_err(|error| ExtractionError::ReadSection {
                location: format!("{object_location}:{section_name}"),
                reason: error.to_string(),
            })?;
        let relocations = section.relocations().collect::<Vec<_>>();
        match section_kind {
            TransportSectionKind::Relocation(expected_kind) => {
                extract_section_records(
                    &file,
                    artifact_path,
                    archive_member.as_deref(),
                    archive_member_index,
                    section_name,
                    expected_kind,
                    section_data,
                    &relocations,
                    records,
                )?;
            }
            TransportSectionKind::Payload(expected_kind) => {
                extract_payload_section(
                    artifact_path,
                    archive_member.as_deref(),
                    archive_member_index,
                    section_name,
                    expected_kind,
                    section_data,
                    &relocations,
                    records,
                )?;
            }
            TransportSectionKind::SemanticIdentity => {
                extract_semantic_identity_section(
                    artifact_path,
                    archive_member.as_deref(),
                    archive_member_index,
                    section_name,
                    section_data,
                    &relocations,
                    records,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn extract_semantic_identity_section(
    artifact_path: &Path,
    archive_member: Option<&str>,
    archive_member_index: Option<usize>,
    section_name: &str,
    data: &[u8],
    relocations: &[(u64, object::Relocation)],
    records: &mut ArtifactRecords,
) -> Result<(), ExtractionError> {
    if !relocations.is_empty() {
        return Err(ExtractionError::InvalidSection {
            location: format!(
                "{}({}):{section_name}",
                artifact_path.display(),
                archive_member.unwrap_or("<object>")
            ),
            reason: format!(
                "inline semantic-identity sections must not contain relocations; found {}",
                relocations.len()
            ),
        });
    }

    let mut cursor = 0_usize;
    while cursor < data.len() {
        if data[cursor..].iter().all(|byte| *byte == 0) {
            break;
        }
        if data.len() - cursor < SEMANTIC_IDENTITY_HEADER_SIZE {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason: "truncated semantic-identity header".to_string(),
            });
        }
        if data[cursor..cursor + SEMANTIC_IDENTITY_FORMAT_MAGIC.len()]
            != SEMANTIC_IDENTITY_FORMAT_MAGIC
        {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason:
                    "dedicated semantic-identity section contains bytes outside the v1 record stream"
                        .to_string(),
            });
        }

        let location = ArtifactLocation {
            artifact_path: artifact_path.to_path_buf(),
            archive_member: archive_member.map(ToOwned::to_owned),
            archive_member_index,
            section: section_name.to_string(),
            record_offset: cursor as u64,
        };
        let header = decode_semantic_identity_header(
            &data[cursor..cursor + SEMANTIC_IDENTITY_HEADER_SIZE],
            &location,
        )?;
        let record_end = cursor
            .checked_add(header.record_size)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: format!(
                    "semantic-identity record size {} exceeds section size {}",
                    header.record_size,
                    data.len()
                ),
            })?;
        let identity_start = cursor + SEMANTIC_IDENTITY_HEADER_SIZE;
        let identity_end = identity_start
            .checked_add(header.identity_size)
            .filter(|end| *end == record_end)
            .ok_or_else(|| ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: "semantic-identity length does not match complete record size".to_string(),
            })?;
        let record = &data[cursor..record_end];
        let cargo_unit_id = read_id(record, CARGO_UNIT_ID_OFFSET, &location)?;
        let definition_record_id = read_id(record, RECORD_ID_OFFSET, &location)?;
        let (package, crate_name, module_path, item) =
            decode_semantic_identity_text(&data[identity_start..identity_end], &location)?;

        records.semantic_identities.push(SemanticIdentity {
            location,
            kind: header.kind,
            cargo_unit_id,
            definition_record_id,
            package,
            crate_name,
            module_path,
            item,
        });
        cursor = align_up(record_end, 8);
    }
    Ok(())
}

#[derive(Debug)]
struct DecodedSemanticIdentityHeader {
    kind: SemanticIdentityKind,
    record_size: usize,
    identity_size: usize,
}

fn decode_semantic_identity_header(
    bytes: &[u8],
    location: &ArtifactLocation,
) -> Result<DecodedSemanticIdentityHeader, ExtractionError> {
    if bytes[..8] != SEMANTIC_IDENTITY_FORMAT_MAGIC {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "wrong semantic-identity magic".to_string(),
        });
    }
    let version = read_u16(bytes, 8);
    if version != SEMANTIC_IDENTITY_FORMAT_VERSION {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "unsupported semantic-identity version {version}; expected {SEMANTIC_IDENTITY_FORMAT_VERSION}"
            ),
        });
    }
    let kind_raw = read_u16(bytes, 10);
    let kind = SemanticIdentityKind::from_wire(kind_raw).ok_or_else(|| {
        ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!("unknown semantic-identity kind {kind_raw}"),
        }
    })?;
    let header_size = read_u16(bytes, 12) as usize;
    let flags = read_u16(bytes, 14);
    let record_size =
        usize::try_from(read_u64(bytes, IDENTITY_RECORD_SIZE_OFFSET)).map_err(|_| {
            ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: "semantic-identity record size exceeds the host address space".to_string(),
            }
        })?;
    let identity_size = usize::try_from(read_u64(bytes, IDENTITY_SIZE_OFFSET)).map_err(|_| {
        ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "semantic-identity text length exceeds the host address space".to_string(),
        }
    })?;
    if header_size != SEMANTIC_IDENTITY_HEADER_SIZE
        || flags != 0
        || identity_size == 0
        || record_size != SEMANTIC_IDENTITY_HEADER_SIZE.saturating_add(identity_size)
    {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "semantic-identity layout mismatch: header={header_size}, record={record_size}, identity={identity_size}, flags={flags}"
            ),
        });
    }
    Ok(DecodedSemanticIdentityHeader {
        kind,
        record_size,
        identity_size,
    })
}

fn decode_semantic_identity_text(
    bytes: &[u8],
    location: &ArtifactLocation,
) -> Result<(String, String, Vec<String>, String), ExtractionError> {
    let text = std::str::from_utf8(bytes).map_err(|error| ExtractionError::InvalidRecord {
        location: location.clone(),
        reason: format!("semantic identity is not UTF-8: {error}"),
    })?;
    let fields = text.split('\0').collect::<Vec<_>>();
    if fields.len() != 4 {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "semantic identity requires exactly package, crate, module_path, and item fields; found {}",
                fields.len()
            ),
        });
    }
    let package = normalize_namespace_piece("package", fields[0], location)?;
    let crate_name = normalize_namespace_piece("crate", fields[1], location)?;
    let item = normalize_namespace_piece("item", fields[3], location)?;
    let full_module = fields[2]
        .split("::")
        .map(|segment| normalize_namespace_piece("module", segment, location))
        .collect::<Result<Vec<_>, _>>()?;
    let Some((module_crate, module_path)) = full_module.split_first() else {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "module_path!() identity must include its crate prefix".to_string(),
        });
    };
    if module_crate != &crate_name {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "module_path!() crate prefix `{module_crate}` does not match CARGO_CRATE_NAME `{crate_name}`"
            ),
        });
    }
    Ok((package, crate_name, module_path.to_vec(), item))
}

fn normalize_namespace_piece(
    field: &'static str,
    value: &str,
    location: &ArtifactLocation,
) -> Result<String, ExtractionError> {
    let value = value.strip_prefix("r#").unwrap_or(value);
    let normalized = !value.is_empty()
        && value.trim() == value
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("::")
        && !value.contains('\0');
    if !normalized {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!("semantic identity has invalid {field} namespace `{value}`"),
        });
    }
    Ok(value.to_string())
}

#[allow(clippy::too_many_arguments)]
fn extract_payload_section(
    artifact_path: &Path,
    archive_member: Option<&str>,
    archive_member_index: Option<usize>,
    section_name: &str,
    expected_kind: PayloadKind,
    data: &[u8],
    relocations: &[(u64, object::Relocation)],
    records: &mut ArtifactRecords,
) -> Result<(), ExtractionError> {
    if !relocations.is_empty() {
        return Err(ExtractionError::InvalidSection {
            location: format!(
                "{}({}):{section_name}",
                artifact_path.display(),
                archive_member.unwrap_or("<object>")
            ),
            reason: format!(
                "inline payload sections must not contain relocations; found {}",
                relocations.len()
            ),
        });
    }

    let mut cursor = 0_usize;
    while cursor < data.len() {
        if data[cursor..].iter().all(|byte| *byte == 0) {
            break;
        }
        if data.len() - cursor < PAYLOAD_HEADER_SIZE {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason: "truncated inline payload header".to_string(),
            });
        }
        if data[cursor..cursor + PAYLOAD_FORMAT_MAGIC.len()] != PAYLOAD_FORMAT_MAGIC {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason: "dedicated payload section contains bytes outside the v1 record stream"
                    .to_string(),
            });
        }

        let location = ArtifactLocation {
            artifact_path: artifact_path.to_path_buf(),
            archive_member: archive_member.map(ToOwned::to_owned),
            archive_member_index,
            section: section_name.to_string(),
            record_offset: cursor as u64,
        };
        let header = decode_payload_header(&data[cursor..cursor + PAYLOAD_HEADER_SIZE], &location)?;
        if header.kind != expected_kind {
            return Err(ExtractionError::InvalidRecord {
                location,
                reason: format!(
                    "section declares {expected_kind:?}, payload declares {:?}",
                    header.kind
                ),
            });
        }
        let record_end = cursor
            .checked_add(header.record_size)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: format!(
                    "payload record size {} exceeds section size {}",
                    header.record_size,
                    data.len()
                ),
            })?;
        let payload_start = cursor + PAYLOAD_HEADER_SIZE;
        let payload_end = payload_start
            .checked_add(header.payload_size)
            .filter(|end| *end == record_end)
            .ok_or_else(|| ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: "payload length does not match complete record size".to_string(),
            })?;
        if header.payload_size == 0 {
            return Err(ExtractionError::InvalidRecord {
                location,
                reason: "descriptor payload must not be empty".to_string(),
            });
        }

        let cargo_unit_id = read_id(&data[cursor..record_end], CARGO_UNIT_ID_OFFSET, &location)?;
        let definition_record_id = read_id(&data[cursor..record_end], RECORD_ID_OFFSET, &location)?;
        let recorded_hash = read_id(&data[cursor..record_end], PAYLOAD_HASH_OFFSET, &location)?;
        let payload = &data[payload_start..payload_end];
        let computed_hash = semantic_payload_hash(expected_kind, payload);
        if recorded_hash != computed_hash {
            return Err(ExtractionError::PayloadHashMismatch {
                location: Box::new(location),
                kind: payload_kind_label(expected_kind),
                recorded: id_hex(&recorded_hash),
                computed: id_hex(&computed_hash),
            });
        }

        match expected_kind {
            PayloadKind::ComponentDescriptor => {
                records.component_payloads.push(ComponentDescriptorPayload {
                    location,
                    cargo_unit_id,
                    component_record_id: definition_record_id,
                    semantic_payload_hash: recorded_hash,
                    json: payload.to_vec(),
                });
            }
            PayloadKind::StandaloneHelperDescriptor => {
                records.helper_payloads.push(StandaloneHelperPayload {
                    location,
                    cargo_unit_id,
                    helper_record_id: definition_record_id,
                    semantic_payload_hash: recorded_hash,
                    json: payload.to_vec(),
                });
            }
            PayloadKind::SharedStateDefinition => {
                let decoded = decode_shared_state_schema_payload(payload, &location)?;
                records
                    .shared_state_payloads
                    .push(SharedStateDefinitionPayload {
                        location,
                        cargo_unit_id,
                        shared_state_record_id: definition_record_id,
                        semantic_payload_hash: recorded_hash,
                        type_key: decoded.type_key,
                        schema_abi_hash: decoded.schema_abi_hash,
                        schema_abi: decoded.schema_abi,
                    });
            }
        }
        cursor = align_up(record_end, 8);
    }
    Ok(())
}

#[derive(Debug)]
struct DecodedSharedStateSchemaPayload {
    type_key: String,
    schema_abi_hash: TransportRecordId,
    schema_abi: Vec<u8>,
}

fn decode_shared_state_schema_payload(
    bytes: &[u8],
    location: &ArtifactLocation,
) -> Result<DecodedSharedStateSchemaPayload, ExtractionError> {
    if bytes.len() < SHARED_STATE_SCHEMA_HEADER_SIZE {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "truncated SharedState schema payload header: found {} bytes, need {SHARED_STATE_SCHEMA_HEADER_SIZE}",
                bytes.len()
            ),
        });
    }
    if bytes[..8] != SHARED_STATE_SCHEMA_FORMAT_MAGIC {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "wrong SharedState schema payload magic".to_string(),
        });
    }
    let version = read_u16(bytes, 8);
    if version != SHARED_STATE_SCHEMA_FORMAT_VERSION {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "unsupported SharedState schema payload version {version}; expected {SHARED_STATE_SCHEMA_FORMAT_VERSION}"
            ),
        });
    }
    let header_size = read_u16(bytes, 10) as usize;
    let flags = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed SharedState header"));
    let record_size = usize::try_from(read_u64(bytes, SHARED_STATE_SCHEMA_RECORD_SIZE_OFFSET))
        .map_err(|_| ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "SharedState schema payload size exceeds the host address space".to_string(),
        })?;
    let type_key_size = usize::try_from(read_u64(bytes, SHARED_STATE_TYPE_KEY_SIZE_OFFSET))
        .map_err(|_| ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "SharedState type_key length exceeds the host address space".to_string(),
        })?;
    let schema_abi_size = usize::try_from(read_u64(bytes, SHARED_STATE_SCHEMA_ABI_SIZE_OFFSET))
        .map_err(|_| ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "SharedState schema ABI length exceeds the host address space".to_string(),
        })?;
    let expected_record_size = SHARED_STATE_SCHEMA_HEADER_SIZE
        .checked_add(type_key_size)
        .and_then(|size| size.checked_add(schema_abi_size));
    if header_size != SHARED_STATE_SCHEMA_HEADER_SIZE
        || flags != 0
        || type_key_size == 0
        || schema_abi_size == 0
        || expected_record_size != Some(record_size)
        || record_size != bytes.len()
    {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "SharedState schema payload layout mismatch: header={header_size}, record={record_size}, type_key={type_key_size}, schema_abi={schema_abi_size}, flags={flags}, actual={}",
                bytes.len()
            ),
        });
    }

    let type_key_start = SHARED_STATE_SCHEMA_HEADER_SIZE;
    let type_key_end = type_key_start + type_key_size;
    let type_key = std::str::from_utf8(&bytes[type_key_start..type_key_end]).map_err(|error| {
        ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!("SharedState type_key is not UTF-8: {error}"),
        }
    })?;
    if type_key != type_key.trim()
        || type_key.chars().any(char::is_whitespace)
        || !type_key.contains('.')
    {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "SharedState type_key `{type_key}` must be non-empty, namespaced, and contain no whitespace"
            ),
        });
    }

    let recorded_hash = read_id(bytes, SHARED_STATE_SCHEMA_ABI_HASH_OFFSET, location)?;
    let schema_abi = &bytes[type_key_end..];
    let computed_hash = shared_state_schema_abi_hash(schema_abi);
    if recorded_hash != computed_hash {
        return Err(ExtractionError::SharedStateSchemaAbiHashMismatch {
            location: Box::new(location.clone()),
            recorded: id_hex(&recorded_hash),
            computed: id_hex(&computed_hash),
        });
    }

    Ok(DecodedSharedStateSchemaPayload {
        type_key: type_key.to_string(),
        schema_abi_hash: recorded_hash,
        schema_abi: schema_abi.to_vec(),
    })
}

#[derive(Debug)]
struct DecodedPayloadHeader {
    kind: PayloadKind,
    record_size: usize,
    payload_size: usize,
}

fn decode_payload_header(
    bytes: &[u8],
    location: &ArtifactLocation,
) -> Result<DecodedPayloadHeader, ExtractionError> {
    if bytes[..8] != PAYLOAD_FORMAT_MAGIC {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "wrong payload magic".to_string(),
        });
    }
    let version = read_u16(bytes, 8);
    if version != PAYLOAD_FORMAT_VERSION {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "unsupported payload version {version}; expected {PAYLOAD_FORMAT_VERSION}"
            ),
        });
    }
    let kind_raw = read_u16(bytes, 10);
    let kind = PayloadKind::from_wire(kind_raw).ok_or_else(|| ExtractionError::InvalidRecord {
        location: location.clone(),
        reason: format!("unknown payload kind {kind_raw}"),
    })?;
    let header_size = read_u16(bytes, 12) as usize;
    let flags = read_u16(bytes, 14);
    let record_size =
        usize::try_from(read_u64(bytes, PAYLOAD_RECORD_SIZE_OFFSET)).map_err(|_| {
            ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: "payload record size exceeds the host address space".to_string(),
            }
        })?;
    let payload_size = usize::try_from(read_u64(bytes, PAYLOAD_SIZE_OFFSET)).map_err(|_| {
        ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "payload byte length exceeds the host address space".to_string(),
        }
    })?;
    if header_size != PAYLOAD_HEADER_SIZE
        || flags != 0
        || record_size != PAYLOAD_HEADER_SIZE.saturating_add(payload_size)
    {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "payload layout mismatch: header={header_size}, record={record_size}, payload={payload_size}, flags={flags}"
            ),
        });
    }
    Ok(DecodedPayloadHeader {
        kind,
        record_size,
        payload_size,
    })
}

#[allow(clippy::too_many_arguments)]
fn extract_section_records<'data>(
    file: &object::File<'data>,
    artifact_path: &Path,
    archive_member: Option<&str>,
    archive_member_index: Option<usize>,
    section_name: &str,
    expected_kind: RecordKind,
    data: &[u8],
    relocations: &[(u64, object::Relocation)],
    records: &mut ArtifactRecords,
) -> Result<(), ExtractionError> {
    let mut cursor = 0_usize;
    while cursor < data.len() {
        if data[cursor..].iter().all(|byte| *byte == 0) {
            break;
        }
        if data.len() - cursor < HEADER_SIZE {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason: "truncated record header".to_string(),
            });
        }
        if data[cursor..cursor + TRANSPORT_FORMAT_MAGIC.len()] != TRANSPORT_FORMAT_MAGIC {
            return Err(ExtractionError::InvalidSection {
                location: format!(
                    "{}({}):{section_name}+0x{cursor:x}",
                    artifact_path.display(),
                    archive_member.unwrap_or("<object>")
                ),
                reason: format!(
                    "dedicated section contains bytes outside the v{TRANSPORT_FORMAT_VERSION} record stream"
                ),
            });
        }

        let location = ArtifactLocation {
            artifact_path: artifact_path.to_path_buf(),
            archive_member: archive_member.map(ToOwned::to_owned),
            archive_member_index,
            section: section_name.to_string(),
            record_offset: cursor as u64,
        };
        let header = decode_header(&data[cursor..cursor + HEADER_SIZE], &location)?;
        if header.kind != expected_kind {
            return Err(ExtractionError::InvalidRecord {
                location,
                reason: format!(
                    "section declares {expected_kind:?}, record declares {:?}",
                    header.kind
                ),
            });
        }
        let record_end = cursor
            .checked_add(header.record_size)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: format!(
                    "record size {} exceeds section size {}",
                    header.record_size,
                    data.len()
                ),
            })?;
        let record = &data[cursor..record_end];
        let cargo_unit_id = read_id(record, CARGO_UNIT_ID_OFFSET, &location)?;
        let record_id = read_id(record, RECORD_ID_OFFSET, &location)?;
        let relocation_offset = cursor as u64 + header.relocation_offset as u64;
        let matching = relocations
            .iter()
            .filter(|(offset, _)| *offset == relocation_offset)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(ExtractionError::RelocationCount {
                location,
                found: matching.len(),
            });
        }
        let witness = decode_relocation(
            file,
            artifact_path,
            archive_member,
            archive_member_index,
            &location,
            header.pointer_width,
            &matching[0].1,
            &record[header.relocation_offset..header.relocation_offset + 8],
        )?;

        match expected_kind {
            RecordKind::ComponentDefinition => {
                records.definitions.push(ComponentDefinition {
                    location,
                    cargo_unit_id,
                    component_record_id: record_id,
                    component_abi: ComponentDefinitionAbi::from_flags(header.flags)
                        .expect("component-definition flags were validated by decode_header"),
                    witness,
                });
            }
            RecordKind::ComponentEdge => {
                records.edges.push(ComponentEdge {
                    location,
                    cargo_unit_id,
                    source_edge_id: record_id,
                    witness,
                });
            }
            RecordKind::ComponentField => {
                records.component_fields.push(ComponentField {
                    location,
                    cargo_unit_id,
                    field_record_id: record_id,
                    witness,
                });
            }
            RecordKind::HelperDefinition => {
                records.helper_definitions.push(HelperDefinition {
                    location,
                    cargo_unit_id,
                    helper_record_id: record_id,
                    helper: witness,
                });
            }
            RecordKind::HelperEdge => {
                records.helper_edges.push(HelperEdge {
                    location,
                    cargo_unit_id,
                    source_edge_id: record_id,
                    helper: witness,
                });
            }
            RecordKind::SharedStateDefinition => {
                records
                    .shared_state_definitions
                    .push(SharedStateDefinition {
                        location,
                        cargo_unit_id,
                        shared_state_record_id: record_id,
                        witness,
                    });
            }
            RecordKind::SharedStateAcquisition => {
                records
                    .shared_state_acquisitions
                    .push(SharedStateAcquisition {
                        location,
                        cargo_unit_id,
                        source_edge_id: record_id,
                        witness,
                    });
            }
        }
        cursor = align_up(record_end, 8);
    }
    Ok(())
}

#[derive(Debug)]
struct DecodedHeader {
    kind: RecordKind,
    record_size: usize,
    relocation_offset: usize,
    pointer_width: u8,
    flags: u32,
}

fn decode_header(
    bytes: &[u8],
    location: &ArtifactLocation,
) -> Result<DecodedHeader, ExtractionError> {
    if bytes[..8] != TRANSPORT_FORMAT_MAGIC {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: "wrong transport magic".to_string(),
        });
    }
    let version = read_u16(bytes, 8);
    if version != TRANSPORT_FORMAT_VERSION {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "unsupported transport version {version}; expected {TRANSPORT_FORMAT_VERSION}"
            ),
        });
    }
    let kind_raw = read_u16(bytes, 10);
    let kind = RecordKind::from_wire(kind_raw).ok_or_else(|| ExtractionError::InvalidRecord {
        location: location.clone(),
        reason: format!("unknown record kind {kind_raw}"),
    })?;
    let header_size = read_u16(bytes, 12) as usize;
    let record_size = read_u16(bytes, 14) as usize;
    let relocation_offset = read_u16(bytes, 16) as usize;
    let pointer_width = bytes[18];
    let flags = u32::from_le_bytes(bytes[19..23].try_into().expect("fixed header slice"));
    if header_size != HEADER_SIZE
        || record_size != TRANSPORT_RECORD_SIZE as usize
        || relocation_offset != RELOCATION_OFFSET
        || !matches!(pointer_width, 4 | 8)
        || bytes[23..32] != [0; 9]
    {
        return Err(ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!(
                "layout mismatch: header={header_size}, record={record_size}, relocation={relocation_offset}, pointer={pointer_width}, flags={flags}"
            ),
        });
    }
    match kind {
        RecordKind::ComponentDefinition if flags & !COMPONENT_DEFINITION_KNOWN_FLAGS != 0 => {
            return Err(ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: format!(
                    "component definition uses unknown transport-v3 flags 0x{flags:08x}; \
                     known mask is 0x{COMPONENT_DEFINITION_KNOWN_FLAGS:08x}"
                ),
            });
        }
        RecordKind::ComponentDefinition => {}
        _ if flags != 0 => {
            return Err(ExtractionError::InvalidRecord {
                location: location.clone(),
                reason: format!(
                    "{kind:?} record illegally uses component-definition flags 0x{flags:08x}"
                ),
            });
        }
        _ => {}
    }
    Ok(DecodedHeader {
        kind,
        record_size,
        relocation_offset,
        pointer_width,
        flags,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_relocation<'data>(
    file: &object::File<'data>,
    artifact_path: &Path,
    archive_member: Option<&str>,
    archive_member_index: Option<usize>,
    location: &ArtifactLocation,
    pointer_width: u8,
    relocation: &object::Relocation,
    slot_bytes: &[u8],
) -> Result<RelocationIdentity, ExtractionError> {
    // `Object::is_64()` describes the container representation, not always
    // the target address width. In particular, object reports Windows COFF as
    // a 32-bit container even for x86_64. The relocation slot follows the
    // target architecture's pointer width.
    let object_pointer_width = file
        .architecture()
        .address_size()
        .map(object::AddressSize::bytes)
        .ok_or_else(|| ExtractionError::UnsupportedRelocation {
            location: location.clone(),
            reason: format!(
                "target architecture {:?} has no known pointer width",
                file.architecture()
            ),
        })?;
    if pointer_width != object_pointer_width
        || relocation.kind() != RelocationKind::Absolute
        || relocation.encoding() != RelocationEncoding::Generic
        || relocation.size() != pointer_width * 8
        || relocation.addend() != 0
        || slot_bytes.iter().any(|byte| *byte != 0)
    {
        return Err(ExtractionError::UnsupportedRelocation {
            location: location.clone(),
            reason: format!(
                "expected zero-addend absolute/generic {}-bit pointer relocation in a {}-bit object with a zeroed slot, got kind={:?}, encoding={:?}, size={}, addend={}, implicit_addend={}, slot={slot_bytes:02x?}",
                pointer_width * 8,
                object_pointer_width * 8,
                relocation.kind(),
                relocation.encoding(),
                relocation.size(),
                relocation.addend(),
                relocation.has_implicit_addend(),
            ),
        });
    }

    match relocation.target() {
        RelocationTarget::Symbol(index) => {
            let symbol = file.symbol_by_index(index).map_err(|error| {
                ExtractionError::UnsupportedRelocation {
                    location: location.clone(),
                    reason: format!("invalid symbol target: {error}"),
                }
            })?;
            let name = symbol
                .name()
                .map_err(|error| ExtractionError::UnsupportedRelocation {
                    location: location.clone(),
                    reason: format!("non-UTF-8 symbol target: {error}"),
                })?;
            if name.is_empty() {
                return Err(ExtractionError::UnsupportedRelocation {
                    location: location.clone(),
                    reason: "anonymous symbol target is not a stable artifact witness".to_string(),
                });
            }
            Ok(RelocationIdentity::Symbol(name.to_string()))
        }
        RelocationTarget::Section(index) => Ok(RelocationIdentity::Section {
            artifact_path: artifact_path.to_path_buf(),
            archive_member: archive_member.map(ToOwned::to_owned),
            archive_member_index,
            section_index: index.0,
            offset: 0,
        }),
        target => Err(ExtractionError::UnsupportedRelocation {
            location: location.clone(),
            reason: format!("unsupported relocation target {target:?}"),
        }),
    }
}

fn read_id(
    record: &[u8],
    offset: usize,
    location: &ArtifactLocation,
) -> Result<TransportRecordId, ExtractionError> {
    record
        .get(offset..offset + 32)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| ExtractionError::InvalidRecord {
            location: location.clone(),
            reason: format!("missing 32-byte identifier at offset {offset}"),
        })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("validated fixed header"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("validated fixed payload header"),
    )
}

fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportSectionKind {
    Relocation(RecordKind),
    Payload(PayloadKind),
    SemanticIdentity,
}

fn payload_kind_label(kind: PayloadKind) -> &'static str {
    match kind {
        PayloadKind::ComponentDescriptor => "component descriptor",
        PayloadKind::StandaloneHelperDescriptor => "standalone helper descriptor",
        PayloadKind::SharedStateDefinition => "SharedState definition",
    }
}

fn semantic_identity_kind_label(kind: SemanticIdentityKind) -> &'static str {
    match kind {
        SemanticIdentityKind::Component => "component",
        SemanticIdentityKind::Helper => "standalone helper",
    }
}

fn transport_section_kind(
    format: BinaryFormat,
    segment: Option<&str>,
    section: &str,
) -> Option<TransportSectionKind> {
    match format {
        BinaryFormat::MachO if segment == Some(MACHO_DATA_SEGMENT) => match section {
            MACHO_COMPONENT_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::ComponentDefinition,
            )),
            MACHO_EDGE_SECTION => Some(TransportSectionKind::Relocation(RecordKind::ComponentEdge)),
            MACHO_COMPONENT_FIELD_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::ComponentField))
            }
            MACHO_HELPER_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::HelperDefinition,
            )),
            MACHO_HELPER_EDGE_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::HelperEdge))
            }
            MACHO_SHARED_STATE_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateDefinition,
            )),
            MACHO_SHARED_STATE_ACQUISITION_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateAcquisition,
            )),
            MACHO_COMPONENT_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::ComponentDescriptor,
            )),
            MACHO_HELPER_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::StandaloneHelperDescriptor,
            )),
            MACHO_SHARED_STATE_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::SharedStateDefinition,
            )),
            MACHO_SEMANTIC_IDENTITY_SECTION => Some(TransportSectionKind::SemanticIdentity),
            _ => None,
        },
        BinaryFormat::Elf => match section {
            ELF_COMPONENT_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::ComponentDefinition,
            )),
            ELF_EDGE_SECTION => Some(TransportSectionKind::Relocation(RecordKind::ComponentEdge)),
            ELF_COMPONENT_FIELD_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::ComponentField))
            }
            ELF_HELPER_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::HelperDefinition,
            )),
            ELF_HELPER_EDGE_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::HelperEdge))
            }
            ELF_SHARED_STATE_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateDefinition,
            )),
            ELF_SHARED_STATE_ACQUISITION_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateAcquisition,
            )),
            ELF_COMPONENT_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::ComponentDescriptor,
            )),
            ELF_HELPER_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::StandaloneHelperDescriptor,
            )),
            ELF_SHARED_STATE_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::SharedStateDefinition,
            )),
            ELF_SEMANTIC_IDENTITY_SECTION => Some(TransportSectionKind::SemanticIdentity),
            _ => None,
        },
        BinaryFormat::Coff => match section {
            COFF_COMPONENT_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::ComponentDefinition,
            )),
            COFF_EDGE_SECTION => Some(TransportSectionKind::Relocation(RecordKind::ComponentEdge)),
            COFF_COMPONENT_FIELD_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::ComponentField))
            }
            COFF_HELPER_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::HelperDefinition,
            )),
            COFF_HELPER_EDGE_SECTION => {
                Some(TransportSectionKind::Relocation(RecordKind::HelperEdge))
            }
            COFF_SHARED_STATE_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateDefinition,
            )),
            COFF_SHARED_STATE_ACQUISITION_SECTION => Some(TransportSectionKind::Relocation(
                RecordKind::SharedStateAcquisition,
            )),
            COFF_COMPONENT_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::ComponentDescriptor,
            )),
            COFF_HELPER_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::StandaloneHelperDescriptor,
            )),
            COFF_SHARED_STATE_PAYLOAD_SECTION => Some(TransportSectionKind::Payload(
                PayloadKind::SharedStateDefinition,
            )),
            COFF_SEMANTIC_IDENTITY_SECTION => Some(TransportSectionKind::SemanticIdentity),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{
        TransportComponentOriginPermission, TransportComponentRenderKind, TRANSPORT_ID_BYTES,
    };

    fn empty_records(path: &str) -> ArtifactRecords {
        ArtifactRecords {
            artifact_path: PathBuf::from(path),
            object_formats: Vec::new(),
            definitions: Vec::new(),
            edges: Vec::new(),
            component_fields: Vec::new(),
            helper_definitions: Vec::new(),
            helper_edges: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
            component_payloads: Vec::new(),
            helper_payloads: Vec::new(),
            shared_state_payloads: Vec::new(),
            semantic_identities: Vec::new(),
        }
    }

    fn location(section: &str, offset: u64) -> ArtifactLocation {
        ArtifactLocation {
            artifact_path: PathBuf::from("fixture.o"),
            archive_member: None,
            archive_member_index: None,
            section: section.to_string(),
            record_offset: offset,
        }
    }

    fn shared_definition(
        cargo_byte: u8,
        record_byte: u8,
        witness: RelocationIdentity,
        offset: u64,
    ) -> SharedStateDefinition {
        SharedStateDefinition {
            location: location(MACHO_SHARED_STATE_SECTION, offset),
            cargo_unit_id: [cargo_byte; TRANSPORT_ID_BYTES],
            shared_state_record_id: [record_byte; TRANSPORT_ID_BYTES],
            witness,
        }
    }

    fn shared_acquisition(
        cargo_byte: u8,
        edge_byte: u8,
        witness: RelocationIdentity,
        offset: u64,
    ) -> SharedStateAcquisition {
        SharedStateAcquisition {
            location: location(MACHO_SHARED_STATE_ACQUISITION_SECTION, offset),
            cargo_unit_id: [cargo_byte; TRANSPORT_ID_BYTES],
            source_edge_id: [edge_byte; TRANSPORT_ID_BYTES],
            witness,
        }
    }

    fn relocation_record(kind: RecordKind, pointer_width: u8, id_byte: u8) -> Vec<u8> {
        let mut record = vec![0_u8; TRANSPORT_RECORD_SIZE as usize];
        record[..8].copy_from_slice(&TRANSPORT_FORMAT_MAGIC);
        record[8..10].copy_from_slice(&TRANSPORT_FORMAT_VERSION.to_le_bytes());
        record[10..12].copy_from_slice(&(kind as u16).to_le_bytes());
        record[12..14].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        record[14..16].copy_from_slice(&TRANSPORT_RECORD_SIZE.to_le_bytes());
        record[16..18].copy_from_slice(&(RELOCATION_OFFSET as u16).to_le_bytes());
        record[18] = pointer_width;
        if kind == RecordKind::ComponentDefinition {
            record[19..23].copy_from_slice(&COMPONENT_DEFINITION_KNOWN_FLAGS.to_le_bytes());
        }
        record[CARGO_UNIT_ID_OFFSET..RECORD_ID_OFFSET].fill(id_byte);
        record[RECORD_ID_OFFSET..RELOCATION_OFFSET].fill(id_byte.wrapping_add(1));
        record
    }

    fn relocation_section(format: BinaryFormat, kind: RecordKind) -> (Vec<u8>, &'static str) {
        let segment = if format == BinaryFormat::MachO {
            MACHO_DATA_SEGMENT.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        let section = match (format, kind) {
            (BinaryFormat::MachO, RecordKind::ComponentDefinition) => MACHO_COMPONENT_SECTION,
            (BinaryFormat::MachO, RecordKind::ComponentEdge) => MACHO_EDGE_SECTION,
            (BinaryFormat::MachO, RecordKind::ComponentField) => MACHO_COMPONENT_FIELD_SECTION,
            (BinaryFormat::MachO, RecordKind::HelperDefinition) => MACHO_HELPER_SECTION,
            (BinaryFormat::MachO, RecordKind::HelperEdge) => MACHO_HELPER_EDGE_SECTION,
            (BinaryFormat::MachO, RecordKind::SharedStateDefinition) => MACHO_SHARED_STATE_SECTION,
            (BinaryFormat::MachO, RecordKind::SharedStateAcquisition) => {
                MACHO_SHARED_STATE_ACQUISITION_SECTION
            }
            (BinaryFormat::Elf, RecordKind::ComponentDefinition) => ELF_COMPONENT_SECTION,
            (BinaryFormat::Elf, RecordKind::ComponentEdge) => ELF_EDGE_SECTION,
            (BinaryFormat::Elf, RecordKind::ComponentField) => ELF_COMPONENT_FIELD_SECTION,
            (BinaryFormat::Elf, RecordKind::HelperDefinition) => ELF_HELPER_SECTION,
            (BinaryFormat::Elf, RecordKind::HelperEdge) => ELF_HELPER_EDGE_SECTION,
            (BinaryFormat::Elf, RecordKind::SharedStateDefinition) => ELF_SHARED_STATE_SECTION,
            (BinaryFormat::Elf, RecordKind::SharedStateAcquisition) => {
                ELF_SHARED_STATE_ACQUISITION_SECTION
            }
            (BinaryFormat::Coff, RecordKind::ComponentDefinition) => COFF_COMPONENT_SECTION,
            (BinaryFormat::Coff, RecordKind::ComponentEdge) => COFF_EDGE_SECTION,
            (BinaryFormat::Coff, RecordKind::ComponentField) => COFF_COMPONENT_FIELD_SECTION,
            (BinaryFormat::Coff, RecordKind::HelperDefinition) => COFF_HELPER_SECTION,
            (BinaryFormat::Coff, RecordKind::HelperEdge) => COFF_HELPER_EDGE_SECTION,
            (BinaryFormat::Coff, RecordKind::SharedStateDefinition) => COFF_SHARED_STATE_SECTION,
            (BinaryFormat::Coff, RecordKind::SharedStateAcquisition) => {
                COFF_SHARED_STATE_ACQUISITION_SECTION
            }
            _ => unreachable!("unsupported fixture format"),
        };
        (segment, section)
    }

    #[test]
    fn relocation_records_extract_on_macho_elf_and_coff_at_32_and_64_bits() {
        for (format, architecture, endianness, pointer_width) in [
            (
                BinaryFormat::MachO,
                object::Architecture::I386,
                object::Endianness::Little,
                4,
            ),
            (
                BinaryFormat::MachO,
                object::Architecture::X86_64,
                object::Endianness::Little,
                8,
            ),
            (
                BinaryFormat::Elf,
                object::Architecture::I386,
                object::Endianness::Little,
                4,
            ),
            (
                BinaryFormat::Elf,
                object::Architecture::X86_64,
                object::Endianness::Little,
                8,
            ),
            (
                BinaryFormat::Elf,
                object::Architecture::PowerPc,
                object::Endianness::Big,
                4,
            ),
            (
                BinaryFormat::Elf,
                object::Architecture::PowerPc64,
                object::Endianness::Big,
                8,
            ),
            (
                BinaryFormat::Coff,
                object::Architecture::I386,
                object::Endianness::Little,
                4,
            ),
            (
                BinaryFormat::Coff,
                object::Architecture::X86_64,
                object::Endianness::Little,
                8,
            ),
        ] {
            let mut object = object::write::Object::new(format, architecture, endianness);
            let component_symbol = object.add_symbol(object::write::Symbol {
                name: b"__rsjs_fixture_component".to_vec(),
                value: 0,
                size: 0,
                kind: object::SymbolKind::Data,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: object::write::SymbolSection::Undefined,
                flags: object::SymbolFlags::None,
            });
            let helper_symbol = object.add_symbol(object::write::Symbol {
                name: b"__rsjs_fixture_helper".to_vec(),
                value: 0,
                size: 0,
                kind: object::SymbolKind::Text,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: object::write::SymbolSection::Undefined,
                flags: object::SymbolFlags::None,
            });
            let shared_state_symbol = object.add_symbol(object::write::Symbol {
                name: b"__rsjs_fixture_shared_state".to_vec(),
                value: 0,
                size: 0,
                kind: object::SymbolKind::Data,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: object::write::SymbolSection::Undefined,
                flags: object::SymbolFlags::None,
            });

            for (ordinal, kind) in [
                RecordKind::ComponentDefinition,
                RecordKind::ComponentEdge,
                RecordKind::ComponentField,
                RecordKind::HelperDefinition,
                RecordKind::HelperEdge,
                RecordKind::SharedStateDefinition,
                RecordKind::SharedStateAcquisition,
            ]
            .into_iter()
            .enumerate()
            {
                let (segment, section_name) = relocation_section(format, kind);
                let section = object.add_section(
                    segment,
                    section_name.as_bytes().to_vec(),
                    object::SectionKind::Data,
                );
                object.append_section_data(
                    section,
                    &relocation_record(kind, pointer_width, 0x10 + ordinal as u8 * 2),
                    8,
                );
                object
                    .add_relocation(
                        section,
                        object::write::Relocation {
                            offset: RELOCATION_OFFSET as u64,
                            symbol: match kind {
                                RecordKind::ComponentDefinition
                                | RecordKind::ComponentEdge
                                | RecordKind::ComponentField => component_symbol,
                                RecordKind::HelperDefinition | RecordKind::HelperEdge => {
                                    helper_symbol
                                }
                                RecordKind::SharedStateDefinition
                                | RecordKind::SharedStateAcquisition => shared_state_symbol,
                            },
                            addend: 0,
                            flags: object::RelocationFlags::Generic {
                                kind: RelocationKind::Absolute,
                                encoding: RelocationEncoding::Generic,
                                size: pointer_width * 8,
                            },
                        },
                    )
                    .unwrap();
            }

            let bytes = object.write().unwrap();
            let mut records = empty_records("relocation.o");
            extract_object(
                Path::new("relocation.o"),
                None,
                None,
                &bytes,
                "relocation.o".to_string(),
                &mut records,
            )
            .unwrap();

            let fixture = format!("{format:?}/{architecture:?}/{endianness:?}/{pointer_width}");
            assert_eq!(records.definitions.len(), 1, "{fixture}");
            assert_eq!(
                records.definitions[0].component_abi,
                ComponentDefinitionAbi::new(
                    TransportComponentRenderKind::Async,
                    TransportComponentOriginPermission::OriginCapable,
                ),
                "{fixture}"
            );
            assert_eq!(records.edges.len(), 1, "{fixture}");
            assert_eq!(records.component_fields.len(), 1, "{fixture}");
            assert_eq!(records.helper_definitions.len(), 1, "{fixture}");
            assert_eq!(records.helper_edges.len(), 1, "{fixture}");
            assert_eq!(records.shared_state_definitions.len(), 1, "{fixture}");
            assert_eq!(records.shared_state_acquisitions.len(), 1, "{fixture}");
            assert!(
                matches!(
                    &records.definitions[0].witness,
                    RelocationIdentity::Symbol(symbol)
                        if symbol.ends_with("__rsjs_fixture_component")
                ),
                "{fixture}: {:?}",
                records.definitions[0].witness
            );
            assert_eq!(
                records.edges[0].witness, records.definitions[0].witness,
                "{fixture}"
            );
            assert_eq!(
                records.component_fields[0].witness, records.definitions[0].witness,
                "{fixture}"
            );
            assert_eq!(
                records.component_fields[0].field_record_id, [0x15; TRANSPORT_ID_BYTES],
                "{fixture}"
            );
            assert!(
                matches!(
                    &records.helper_definitions[0].helper,
                    RelocationIdentity::Symbol(symbol)
                        if symbol.ends_with("__rsjs_fixture_helper")
                ),
                "{fixture}: {:?}",
                records.helper_definitions[0].helper
            );
            assert_eq!(
                records.helper_edges[0].helper, records.helper_definitions[0].helper,
                "{fixture}"
            );
            assert!(
                matches!(
                    &records.shared_state_definitions[0].witness,
                    RelocationIdentity::Symbol(symbol)
                        if symbol.ends_with("__rsjs_fixture_shared_state")
                ),
                "{fixture}: {:?}",
                records.shared_state_definitions[0].witness
            );
            assert_eq!(
                records.shared_state_acquisitions[0].witness,
                records.shared_state_definitions[0].witness,
                "{fixture}"
            );
        }
    }

    #[test]
    fn shared_state_definition_acquisition_and_schema_round_trip_through_object_extraction() {
        let format = BinaryFormat::Elf;
        let mut object = object::write::Object::new(
            format,
            object::Architecture::X86_64,
            object::Endianness::Little,
        );
        let witness = object.add_symbol(object::write::Symbol {
            name: b"__rsjs_shared_state_witness_WorkbenchState".to_vec(),
            value: 0,
            size: 0,
            kind: object::SymbolKind::Data,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: object::write::SymbolSection::Undefined,
            flags: object::SymbolFlags::None,
        });
        for (kind, cargo_byte) in [
            (RecordKind::SharedStateDefinition, 0x11),
            (RecordKind::SharedStateAcquisition, 0x33),
        ] {
            let (_, section_name) = relocation_section(format, kind);
            let section = object.add_section(
                Vec::new(),
                section_name.as_bytes().to_vec(),
                object::SectionKind::Data,
            );
            object.append_section_data(section, &relocation_record(kind, 8, cargo_byte), 8);
            object
                .add_relocation(
                    section,
                    object::write::Relocation {
                        offset: RELOCATION_OFFSET as u64,
                        symbol: witness,
                        addend: 0,
                        flags: object::RelocationFlags::Generic {
                            kind: RelocationKind::Absolute,
                            encoding: RelocationEncoding::Generic,
                            size: 64,
                        },
                    },
                )
                .unwrap();
        }
        let schema_abi = b"\x01object\0mode\0string\0selected_node\0option(string)";
        let payload_section = object.add_section(
            Vec::new(),
            ELF_SHARED_STATE_PAYLOAD_SECTION.as_bytes().to_vec(),
            object::SectionKind::Data,
        );
        object.append_section_data(
            payload_section,
            &crate::wire::encode_shared_state_definition_payload_record(
                [0x11; 32],
                [0x12; 32],
                "acme.workbench",
                schema_abi,
            ),
            8,
        );

        let directory = tempfile::tempdir().unwrap();
        let object_path = directory.path().join("shared-state.o");
        fs::write(&object_path, object.write().unwrap()).unwrap();
        let extracted = extract_exact_artifacts([&object_path]).unwrap();

        assert_eq!(extracted.shared_state_definitions.len(), 1);
        assert_eq!(extracted.shared_state_acquisitions.len(), 1);
        assert_eq!(extracted.resolved_shared_state_acquisitions.len(), 1);
        assert_eq!(extracted.resolved_shared_state_definitions.len(), 1);
        let definition = &extracted.shared_state_definitions[0];
        let acquisition = &extracted.shared_state_acquisitions[0];
        assert_eq!(definition.cargo_unit_id, [0x11; 32]);
        assert_eq!(definition.shared_state_record_id, [0x12; 32]);
        assert_eq!(acquisition.cargo_unit_id, [0x33; 32]);
        assert_eq!(acquisition.source_edge_id, [0x34; 32]);
        assert_eq!(acquisition.witness, definition.witness);
        assert_eq!(
            extracted.resolved_shared_state_acquisitions[0].definition,
            *definition
        );
        let resolved = &extracted.resolved_shared_state_definitions[0];
        assert_eq!(resolved.definition, *definition);
        assert_eq!(resolved.payload.type_key, "acme.workbench");
        assert_eq!(resolved.payload.schema_abi, schema_abi);
        assert_eq!(
            resolved.payload.schema_abi_hash,
            shared_state_schema_abi_hash(schema_abi)
        );
        let inner = crate::wire::encode_shared_state_schema_payload("acme.workbench", schema_abi);
        assert_eq!(
            resolved.payload.semantic_payload_hash,
            semantic_payload_hash(PayloadKind::SharedStateDefinition, &inner)
        );
    }

    #[test]
    fn shared_state_schema_payload_rejects_truncation_and_forged_inner_hash_or_lengths() {
        let location = location(MACHO_SHARED_STATE_PAYLOAD_SECTION, 0);
        let schema = b"\x01object\0mode\0string";
        let encoded = crate::wire::encode_shared_state_schema_payload("acme.workbench", schema);

        let truncated = decode_shared_state_schema_payload(
            &encoded[..SHARED_STATE_SCHEMA_HEADER_SIZE - 1],
            &location,
        )
        .unwrap_err();
        assert!(matches!(truncated, ExtractionError::InvalidRecord { .. }));
        assert!(truncated.to_string().contains("truncated SharedState"));

        let mut forged_hash = encoded.clone();
        *forged_hash.last_mut().unwrap() ^= 1;
        let forged_hash = decode_shared_state_schema_payload(&forged_hash, &location).unwrap_err();
        assert!(matches!(
            forged_hash,
            ExtractionError::SharedStateSchemaAbiHashMismatch { .. }
        ));

        let mut forged_length = encoded.clone();
        forged_length[32..40].copy_from_slice(&((schema.len() as u64) + 1).to_le_bytes());
        let forged_length =
            decode_shared_state_schema_payload(&forged_length, &location).unwrap_err();
        assert!(matches!(
            forged_length,
            ExtractionError::InvalidRecord { .. }
        ));
        assert!(forged_length.to_string().contains("layout mismatch"));

        let mut forged_type_key = encoded;
        let type_key_start = SHARED_STATE_SCHEMA_HEADER_SIZE;
        forged_type_key[type_key_start] = b' ';
        let forged_type_key =
            decode_shared_state_schema_payload(&forged_type_key, &location).unwrap_err();
        assert!(matches!(
            forged_type_key,
            ExtractionError::InvalidRecord { .. }
        ));
        assert!(forged_type_key.to_string().contains("type_key"));
    }

    #[test]
    fn shared_state_resolution_rejects_duplicate_definition_and_acquisition_ids() {
        let first_witness = RelocationIdentity::Symbol("first_state".to_string());
        let second_witness = RelocationIdentity::Symbol("second_state".to_string());
        let duplicate_definitions = [
            shared_definition(0x11, 0x22, first_witness.clone(), 0),
            shared_definition(0x11, 0x22, second_witness.clone(), 104),
        ];
        let duplicate_definition = resolve_shared_state_acquisitions(
            &duplicate_definitions,
            &[],
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            duplicate_definition,
            ExtractionError::DuplicateSharedStateDefinitionRecordId { .. }
        ));

        let definitions = [shared_definition(0x11, 0x22, first_witness.clone(), 0)];
        let duplicate_acquisitions = [
            shared_acquisition(0x33, 0x44, first_witness.clone(), 0),
            shared_acquisition(0x33, 0x44, first_witness, 104),
        ];
        let duplicate_acquisition = resolve_shared_state_acquisitions(
            &definitions,
            &duplicate_acquisitions,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            duplicate_acquisition,
            ExtractionError::DuplicateSharedStateAcquisitionRecordId { .. }
        ));
    }

    #[test]
    fn shared_state_resolution_rejects_unresolved_ambiguous_and_wrong_kind_relocations() {
        let witness = RelocationIdentity::Symbol("state_witness".to_string());
        let acquisition = shared_acquisition(0x33, 0x44, witness.clone(), 0);
        let unresolved = resolve_shared_state_acquisitions(
            &[],
            std::slice::from_ref(&acquisition),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            unresolved,
            ExtractionError::UnresolvedSharedStateWitness { .. }
        ));

        let ambiguous_definitions = [
            shared_definition(0x11, 0x21, witness.clone(), 0),
            shared_definition(0x11, 0x22, witness.clone(), 104),
        ];
        let ambiguous = resolve_shared_state_acquisitions(
            &ambiguous_definitions,
            std::slice::from_ref(&acquisition),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            ambiguous,
            ExtractionError::AmbiguousSharedStateWitness { .. }
        ));

        let component = ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, 0),
            cargo_unit_id: [0x55; 32],
            component_record_id: [0x66; 32],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: witness.clone(),
        };
        let component_definitions = BTreeMap::from([(witness.clone(), vec![component])]);
        let wrong_acquisition = resolve_shared_state_acquisitions(
            &[],
            std::slice::from_ref(&acquisition),
            &component_definitions,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            wrong_acquisition,
            ExtractionError::WrongKindSharedStateAcquisitionWitness { .. }
        ));

        let definition = shared_definition(0x11, 0x22, witness, 0);
        let wrong_definition = resolve_shared_state_acquisitions(
            std::slice::from_ref(&definition),
            &[],
            &component_definitions,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(
            wrong_definition,
            ExtractionError::WrongKindSharedStateDefinitionWitness { .. }
        ));

        let helper_witness = RelocationIdentity::Symbol("helper_witness".to_string());
        let helper = HelperDefinition {
            location: location(MACHO_HELPER_SECTION, 0),
            cargo_unit_id: [0x77; 32],
            helper_record_id: [0x88; 32],
            helper: helper_witness.clone(),
        };
        let helper_definitions = BTreeMap::from([(helper_witness.clone(), vec![helper])]);
        let helper_acquisition = shared_acquisition(0x33, 0x45, helper_witness.clone(), 104);
        let wrong_helper_acquisition = resolve_shared_state_acquisitions(
            &[],
            std::slice::from_ref(&helper_acquisition),
            &BTreeMap::new(),
            &helper_definitions,
        )
        .unwrap_err();
        assert!(matches!(
            wrong_helper_acquisition,
            ExtractionError::WrongKindSharedStateAcquisitionWitness {
                actual_kind: "standalone helper",
                ..
            }
        ));
        let helper_definition = shared_definition(0x11, 0x23, helper_witness, 104);
        let wrong_helper_definition = resolve_shared_state_acquisitions(
            std::slice::from_ref(&helper_definition),
            &[],
            &BTreeMap::new(),
            &helper_definitions,
        )
        .unwrap_err();
        assert!(matches!(
            wrong_helper_definition,
            ExtractionError::WrongKindSharedStateDefinitionWitness {
                actual_kind: "standalone helper",
                ..
            }
        ));
    }

    #[test]
    fn shared_state_records_use_deterministic_transport_key_order() {
        let first = RelocationIdentity::Symbol("first".to_string());
        let second = RelocationIdentity::Symbol("second".to_string());
        let mut records = empty_records("ordering.o");
        records
            .shared_state_definitions
            .push(shared_definition(0x22, 0x22, second.clone(), 0));
        records
            .shared_state_definitions
            .push(shared_definition(0x11, 0x33, first.clone(), 104));
        records
            .shared_state_definitions
            .push(shared_definition(0x11, 0x22, first.clone(), 208));
        records
            .shared_state_acquisitions
            .push(shared_acquisition(0x22, 0x22, second, 0));
        records
            .shared_state_acquisitions
            .push(shared_acquisition(0x11, 0x33, first.clone(), 104));
        records
            .shared_state_acquisitions
            .push(shared_acquisition(0x11, 0x22, first, 208));
        sort_shared_state_artifact_records(&mut records);

        let definition_keys = records
            .shared_state_definitions
            .iter()
            .map(|record| (record.cargo_unit_id[0], record.shared_state_record_id[0]))
            .collect::<Vec<_>>();
        assert_eq!(definition_keys, [(0x11, 0x22), (0x11, 0x33), (0x22, 0x22)]);
        let acquisition_keys = records
            .shared_state_acquisitions
            .iter()
            .map(|record| (record.cargo_unit_id[0], record.source_edge_id[0]))
            .collect::<Vec<_>>();
        assert_eq!(acquisition_keys, [(0x11, 0x22), (0x11, 0x33), (0x22, 0x22)]);
    }

    #[test]
    fn component_field_resolves_only_to_its_exact_defining_witness() {
        let witness = RelocationIdentity::Symbol("__rsjs_component_witness".to_string());
        let definition = ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, 0),
            cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
            component_record_id: [0x22; TRANSPORT_ID_BYTES],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: witness.clone(),
        };
        let field = ComponentField {
            location: location(MACHO_COMPONENT_FIELD_SECTION, 0),
            cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
            field_record_id: [0x33; TRANSPORT_ID_BYTES],
            witness: witness.clone(),
        };
        let definitions = BTreeMap::from([(witness, vec![definition.clone()])]);

        let resolved =
            resolve_component_fields(std::slice::from_ref(&field), &definitions).unwrap();
        assert_eq!(resolved, vec![ResolvedComponentField { field, definition }]);
    }

    #[test]
    fn component_field_resolution_rejects_duplicate_record_ids() {
        let witness = RelocationIdentity::Symbol("__rsjs_component_witness".to_string());
        let fields = [
            ComponentField {
                location: location(MACHO_COMPONENT_FIELD_SECTION, 0),
                cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
                field_record_id: [0x33; TRANSPORT_ID_BYTES],
                witness: witness.clone(),
            },
            ComponentField {
                location: location(MACHO_COMPONENT_FIELD_SECTION, 104),
                cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
                field_record_id: [0x33; TRANSPORT_ID_BYTES],
                witness,
            },
        ];

        let error = resolve_component_fields(&fields, &BTreeMap::new()).unwrap_err();
        assert!(matches!(
            error,
            ExtractionError::DuplicateComponentFieldRecordId { .. }
        ));
    }

    #[test]
    fn component_field_resolution_rejects_missing_or_ambiguous_definitions() {
        let witness = RelocationIdentity::Symbol("__rsjs_component_witness".to_string());
        let field = ComponentField {
            location: location(MACHO_COMPONENT_FIELD_SECTION, 0),
            cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
            field_record_id: [0x33; TRANSPORT_ID_BYTES],
            witness: witness.clone(),
        };

        let missing =
            resolve_component_fields(std::slice::from_ref(&field), &BTreeMap::new()).unwrap_err();
        assert!(matches!(
            missing,
            ExtractionError::UnresolvedComponentFieldWitness { .. }
        ));

        let definition = |offset| ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, offset),
            cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
            component_record_id: [offset as u8; TRANSPORT_ID_BYTES],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: witness.clone(),
        };
        let ambiguous = BTreeMap::from([(witness.clone(), vec![definition(0), definition(104)])]);
        let error = resolve_component_fields(std::slice::from_ref(&field), &ambiguous).unwrap_err();
        assert!(matches!(
            error,
            ExtractionError::AmbiguousComponentFieldWitness { .. }
        ));
    }

    #[test]
    fn component_field_resolution_rejects_a_foreign_cargo_unit() {
        let witness = RelocationIdentity::Symbol("__rsjs_component_witness".to_string());
        let field = ComponentField {
            location: location(MACHO_COMPONENT_FIELD_SECTION, 0),
            cargo_unit_id: [0x11; TRANSPORT_ID_BYTES],
            field_record_id: [0x33; TRANSPORT_ID_BYTES],
            witness: witness.clone(),
        };
        let definition = ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, 0),
            cargo_unit_id: [0x44; TRANSPORT_ID_BYTES],
            component_record_id: [0x22; TRANSPORT_ID_BYTES],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: witness.clone(),
        };
        let definitions = BTreeMap::from([(witness, vec![definition])]);

        let error =
            resolve_component_fields(std::slice::from_ref(&field), &definitions).unwrap_err();
        assert!(matches!(
            error,
            ExtractionError::ComponentFieldCargoUnitMismatch { .. }
        ));
    }

    #[test]
    fn exact_extractor_rejects_thin_archives_instead_of_following_paths() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("thin.rlib");
        fs::write(
            &archive_path,
            b"!<thin>\n/                                               4         `\n0000",
        )
        .unwrap();

        let error = extract_exact_artifact(&archive_path).unwrap_err();
        assert!(
            matches!(error, ExtractionError::ThinArchive { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn exact_byte_snapshot_is_parsed_and_resolved_without_reopening_its_path() {
        let object = object::write::Object::new(
            BinaryFormat::Elf,
            object::Architecture::X86_64,
            object::Endianness::Little,
        );
        let authenticated_bytes = object.write().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.o");
        fs::write(&path, b"different bytes on disk").unwrap();

        let records = ArtifactRecords::extract_exact_bytes(&path, &authenticated_bytes).unwrap();
        assert_eq!(records.artifact_path, path);
        assert_eq!(records.object_formats, vec![BinaryFormat::Elf]);

        let extracted = ExtractedTransport::resolve_exact_artifacts(vec![records]).unwrap();
        assert_eq!(extracted.artifacts.len(), 1);
        assert_eq!(extracted.artifacts[0].artifact_path, path);
        assert_eq!(fs::read(path).unwrap(), b"different bytes on disk");
    }

    #[test]
    fn header_decoder_rejects_unknown_versions() {
        let mut bytes = [0_u8; HEADER_SIZE];
        bytes[..8].copy_from_slice(&TRANSPORT_FORMAT_MAGIC);
        bytes[8..10].copy_from_slice(&99_u16.to_le_bytes());
        let location = ArtifactLocation {
            artifact_path: PathBuf::from("fixture.o"),
            archive_member: None,
            archive_member_index: None,
            section: MACHO_EDGE_SECTION.to_string(),
            record_offset: 0,
        };

        let error = decode_header(&bytes, &location).unwrap_err();
        assert!(matches!(error, ExtractionError::InvalidRecord { .. }));
        assert!(error
            .to_string()
            .contains("unsupported transport version 99"));
    }

    #[test]
    fn header_decoder_rejects_unknown_component_definition_flags() {
        let mut record = relocation_record(RecordKind::ComponentDefinition, 8, 0x11);
        record[19..23].copy_from_slice(&(1_u32 << 31).to_le_bytes());

        let error = decode_header(
            &record[..HEADER_SIZE],
            &location(MACHO_COMPONENT_SECTION, 0),
        )
        .unwrap_err();

        assert!(matches!(error, ExtractionError::InvalidRecord { .. }));
        assert!(
            error
                .to_string()
                .contains("unknown transport-v3 flags 0x80000000"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn header_decoder_rejects_component_definition_flags_on_other_records() {
        let mut record = relocation_record(RecordKind::ComponentEdge, 8, 0x11);
        record[19..23].copy_from_slice(&crate::wire::COMPONENT_DEFINITION_FLAG_ASYNC.to_le_bytes());

        let error =
            decode_header(&record[..HEADER_SIZE], &location(MACHO_EDGE_SECTION, 0)).unwrap_err();

        assert!(matches!(error, ExtractionError::InvalidRecord { .. }));
        assert!(
            error
                .to_string()
                .contains("illegally uses component-definition flags 0x00000001"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn extractor_rejects_a_relocation_that_disagrees_with_the_wire_pointer_width() {
        let mut record = vec![0_u8; TRANSPORT_RECORD_SIZE as usize];
        record[..8].copy_from_slice(&TRANSPORT_FORMAT_MAGIC);
        record[8..10].copy_from_slice(&TRANSPORT_FORMAT_VERSION.to_le_bytes());
        record[10..12].copy_from_slice(&(RecordKind::ComponentEdge as u16).to_le_bytes());
        record[12..14].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        record[14..16].copy_from_slice(&TRANSPORT_RECORD_SIZE.to_le_bytes());
        record[16..18].copy_from_slice(&(RELOCATION_OFFSET as u16).to_le_bytes());
        // Deliberately claim a 32-bit slot inside a 64-bit Mach-O object.
        record[18] = 4;
        record[RECORD_ID_OFFSET..RELOCATION_OFFSET].fill(0x44);

        let mut object = object::write::Object::new(
            BinaryFormat::MachO,
            object::Architecture::Aarch64,
            object::Endianness::Little,
        );
        let section = object.add_section(
            MACHO_DATA_SEGMENT.as_bytes().to_vec(),
            MACHO_EDGE_SECTION.as_bytes().to_vec(),
            object::SectionKind::Data,
        );
        object.append_section_data(section, &record, 8);
        let symbol = object.add_symbol(object::write::Symbol {
            name: b"__rsjs_test_witness".to_vec(),
            value: 0,
            size: 0,
            kind: object::SymbolKind::Data,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: object::write::SymbolSection::Undefined,
            flags: object::SymbolFlags::None,
        });
        object
            .add_relocation(
                section,
                object::write::Relocation {
                    offset: RELOCATION_OFFSET as u64,
                    symbol,
                    addend: 0,
                    flags: object::RelocationFlags::Generic {
                        kind: RelocationKind::Absolute,
                        encoding: RelocationEncoding::Generic,
                        size: 64,
                    },
                },
            )
            .unwrap();
        let bytes = object.write().unwrap();
        let mut records = ArtifactRecords {
            artifact_path: PathBuf::from("bad-pointer-width.o"),
            object_formats: Vec::new(),
            definitions: Vec::new(),
            edges: Vec::new(),
            component_fields: Vec::new(),
            helper_definitions: Vec::new(),
            helper_edges: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
            component_payloads: Vec::new(),
            helper_payloads: Vec::new(),
            shared_state_payloads: Vec::new(),
            semantic_identities: Vec::new(),
        };

        let error = extract_object(
            Path::new("bad-pointer-width.o"),
            None,
            None,
            &bytes,
            "bad-pointer-width.o".to_string(),
            &mut records,
        )
        .unwrap_err();
        assert!(
            matches!(error, ExtractionError::UnsupportedRelocation { .. }),
            "unexpected error: {error}"
        );
        assert!(error.to_string().contains("pointer relocation"));
    }

    #[test]
    fn extractor_rejects_an_object_format_without_a_transport_contract() {
        let object = object::write::Object::new(
            BinaryFormat::Xcoff,
            object::Architecture::PowerPc64,
            object::Endianness::Big,
        );
        let bytes = object.write().unwrap();
        let mut records = ArtifactRecords {
            artifact_path: PathBuf::from("unsupported.o"),
            object_formats: Vec::new(),
            definitions: Vec::new(),
            edges: Vec::new(),
            component_fields: Vec::new(),
            helper_definitions: Vec::new(),
            helper_edges: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
            component_payloads: Vec::new(),
            helper_payloads: Vec::new(),
            shared_state_payloads: Vec::new(),
            semantic_identities: Vec::new(),
        };

        let error = extract_object(
            Path::new("unsupported.o"),
            None,
            None,
            &bytes,
            "unsupported.o".to_string(),
            &mut records,
        )
        .unwrap_err();
        assert!(
            matches!(error, ExtractionError::UnsupportedObjectFormat { .. }),
            "unexpected error: {error}"
        );
        assert!(error.to_string().contains("Xcoff"));
    }

    #[test]
    fn inline_payload_sections_extract_on_macho_elf_and_coff() {
        for (format, architecture) in [
            (BinaryFormat::MachO, object::Architecture::Aarch64),
            (BinaryFormat::Elf, object::Architecture::Aarch64),
            (BinaryFormat::Coff, object::Architecture::X86_64),
        ] {
            let mut object =
                object::write::Object::new(format, architecture, object::Endianness::Little);
            let (segment, component_section, helper_section, shared_state_section) = match format {
                BinaryFormat::MachO => (
                    MACHO_DATA_SEGMENT.as_bytes().to_vec(),
                    MACHO_COMPONENT_PAYLOAD_SECTION,
                    MACHO_HELPER_PAYLOAD_SECTION,
                    MACHO_SHARED_STATE_PAYLOAD_SECTION,
                ),
                BinaryFormat::Elf => (
                    Vec::new(),
                    ELF_COMPONENT_PAYLOAD_SECTION,
                    ELF_HELPER_PAYLOAD_SECTION,
                    ELF_SHARED_STATE_PAYLOAD_SECTION,
                ),
                BinaryFormat::Coff => (
                    Vec::new(),
                    COFF_COMPONENT_PAYLOAD_SECTION,
                    COFF_HELPER_PAYLOAD_SECTION,
                    COFF_SHARED_STATE_PAYLOAD_SECTION,
                ),
                _ => unreachable!(),
            };
            let component = object.add_section(
                segment.clone(),
                component_section.as_bytes().to_vec(),
                object::SectionKind::Data,
            );
            let helper = object.add_section(
                segment.clone(),
                helper_section.as_bytes().to_vec(),
                object::SectionKind::Data,
            );
            let shared_state = object.add_section(
                segment,
                shared_state_section.as_bytes().to_vec(),
                object::SectionKind::Data,
            );
            object.append_section_data(
                component,
                &crate::wire::encode_payload_record(
                    PayloadKind::ComponentDescriptor,
                    [0x11; 32],
                    [0x22; 32],
                    br#"{"component":true}"#,
                ),
                8,
            );
            object.append_section_data(
                helper,
                &crate::wire::encode_payload_record(
                    PayloadKind::StandaloneHelperDescriptor,
                    [0x33; 32],
                    [0x44; 32],
                    br#"{"helper":true}"#,
                ),
                8,
            );
            object.append_section_data(
                shared_state,
                &crate::wire::encode_shared_state_definition_payload_record(
                    [0x55; 32],
                    [0x66; 32],
                    "acme.workbench",
                    b"\x01object\0mode\0string",
                ),
                8,
            );
            let bytes = object.write().unwrap();
            let mut records = empty_records("payload.o");

            extract_object(
                Path::new("payload.o"),
                None,
                None,
                &bytes,
                "payload.o".to_string(),
                &mut records,
            )
            .unwrap();
            assert_eq!(records.component_payloads.len(), 1, "{format:?}");
            assert_eq!(records.helper_payloads.len(), 1, "{format:?}");
            assert_eq!(records.shared_state_payloads.len(), 1, "{format:?}");
            assert_eq!(records.component_payloads[0].cargo_unit_id, [0x11; 32]);
            assert_eq!(
                records.component_payloads[0].component_record_id,
                [0x22; 32]
            );
            assert_eq!(records.helper_payloads[0].cargo_unit_id, [0x33; 32]);
            assert_eq!(records.helper_payloads[0].helper_record_id, [0x44; 32]);
            assert_eq!(records.shared_state_payloads[0].cargo_unit_id, [0x55; 32]);
            assert_eq!(
                records.shared_state_payloads[0].shared_state_record_id,
                [0x66; 32]
            );
            assert_eq!(records.shared_state_payloads[0].type_key, "acme.workbench");
            assert_eq!(
                records.shared_state_payloads[0].schema_abi,
                b"\x01object\0mode\0string"
            );
            assert_eq!(
                records.shared_state_payloads[0].schema_abi_hash,
                shared_state_schema_abi_hash(b"\x01object\0mode\0string")
            );
        }
    }

    #[test]
    fn semantic_identities_extract_on_macho_elf_and_coff() {
        for (format, architecture) in [
            (BinaryFormat::MachO, object::Architecture::Aarch64),
            (BinaryFormat::Elf, object::Architecture::Aarch64),
            (BinaryFormat::Coff, object::Architecture::X86_64),
        ] {
            let mut object =
                object::write::Object::new(format, architecture, object::Endianness::Little);
            let (segment, section_name) = match format {
                BinaryFormat::MachO => (
                    MACHO_DATA_SEGMENT.as_bytes().to_vec(),
                    MACHO_SEMANTIC_IDENTITY_SECTION,
                ),
                BinaryFormat::Elf => (Vec::new(), ELF_SEMANTIC_IDENTITY_SECTION),
                BinaryFormat::Coff => (Vec::new(), COFF_SEMANTIC_IDENTITY_SECTION),
                _ => unreachable!(),
            };
            let section = object.add_section(
                segment,
                section_name.as_bytes().to_vec(),
                object::SectionKind::Data,
            );
            object.append_section_data(
                section,
                &crate::wire::encode_semantic_identity_record(
                    SemanticIdentityKind::Component,
                    [0x11; 32],
                    [0x22; 32],
                    "acme-web\0acme_web\0acme_web::ui\0StatusBadge",
                ),
                8,
            );
            object.append_section_data(
                section,
                &crate::wire::encode_semantic_identity_record(
                    SemanticIdentityKind::Helper,
                    [0x33; 32],
                    [0x44; 32],
                    "acme-web\0acme_web\0acme_web::labels\0pluralize",
                ),
                8,
            );
            let bytes = object.write().unwrap();
            let mut records = empty_records("identity.o");

            extract_object(
                Path::new("identity.o"),
                None,
                None,
                &bytes,
                "identity.o".to_string(),
                &mut records,
            )
            .unwrap();

            assert_eq!(records.semantic_identities.len(), 2, "{format:?}");
            let component = &records.semantic_identities[0];
            assert_eq!(component.kind, SemanticIdentityKind::Component);
            assert_eq!(component.cargo_unit_id, [0x11; 32]);
            assert_eq!(component.definition_record_id, [0x22; 32]);
            assert_eq!(component.package, "acme-web");
            assert_eq!(component.crate_name, "acme_web");
            assert_eq!(component.module_path, ["ui"]);
            assert_eq!(component.item, "StatusBadge");
            let helper = &records.semantic_identities[1];
            assert_eq!(helper.kind, SemanticIdentityKind::Helper);
            assert_eq!(helper.module_path, ["labels"]);
            assert_eq!(helper.item, "pluralize");
        }
    }

    #[test]
    fn semantic_identity_crate_prefix_mismatch_fails_closed() {
        let encoded = crate::wire::encode_semantic_identity_record(
            SemanticIdentityKind::Component,
            [0x11; 32],
            [0x22; 32],
            "acme-web\0acme_web\0other_crate::ui\0StatusBadge",
        );
        let mut object = object::write::Object::new(
            BinaryFormat::MachO,
            object::Architecture::Aarch64,
            object::Endianness::Little,
        );
        let section = object.add_section(
            MACHO_DATA_SEGMENT.as_bytes().to_vec(),
            MACHO_SEMANTIC_IDENTITY_SECTION.as_bytes().to_vec(),
            object::SectionKind::Data,
        );
        object.append_section_data(section, &encoded, 8);
        let bytes = object.write().unwrap();
        let mut records = empty_records("wrong-prefix.o");

        let error = extract_object(
            Path::new("wrong-prefix.o"),
            None,
            None,
            &bytes,
            "wrong-prefix.o".to_string(),
            &mut records,
        )
        .unwrap_err();
        assert!(matches!(error, ExtractionError::InvalidRecord { .. }));
        assert!(error
            .to_string()
            .contains("does not match CARGO_CRATE_NAME"));
    }

    #[test]
    fn inline_payload_hash_corruption_fails_closed() {
        let mut encoded = crate::wire::encode_payload_record(
            PayloadKind::ComponentDescriptor,
            [0x11; 32],
            [0x22; 32],
            br#"{"component":true}"#,
        );
        *encoded.last_mut().unwrap() ^= 1;

        let mut object = object::write::Object::new(
            BinaryFormat::MachO,
            object::Architecture::Aarch64,
            object::Endianness::Little,
        );
        let section = object.add_section(
            MACHO_DATA_SEGMENT.as_bytes().to_vec(),
            MACHO_COMPONENT_PAYLOAD_SECTION.as_bytes().to_vec(),
            object::SectionKind::Data,
        );
        object.append_section_data(section, &encoded, 8);
        let bytes = object.write().unwrap();
        let mut records = empty_records("corrupt.o");

        let error = extract_object(
            Path::new("corrupt.o"),
            None,
            None,
            &bytes,
            "corrupt.o".to_string(),
            &mut records,
        )
        .unwrap_err();
        assert!(matches!(error, ExtractionError::PayloadHashMismatch { .. }));
    }

    #[test]
    fn definition_payload_join_rejects_missing_duplicate_and_wrong_unit_records() {
        let definition = ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, 0),
            cargo_unit_id: [0x11; 32],
            component_record_id: [0x22; 32],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: RelocationIdentity::Symbol("component".to_string()),
        };
        let payload = ComponentDescriptorPayload {
            location: location(MACHO_COMPONENT_PAYLOAD_SECTION, 0),
            cargo_unit_id: [0x11; 32],
            component_record_id: [0x22; 32],
            semantic_payload_hash: [0x33; 32],
            json: br#"{"component":true}"#.to_vec(),
        };

        let missing =
            join_component_payloads(std::slice::from_ref(&definition), &[], &[]).unwrap_err();
        assert!(matches!(
            missing,
            ExtractionError::MissingDefinitionPayload { .. }
        ));

        let duplicate = join_component_payloads(
            std::slice::from_ref(&definition),
            &[payload.clone(), payload.clone()],
            &[],
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            ExtractionError::DuplicateDefinitionPayload { .. }
        ));

        let mut wrong_unit = payload;
        wrong_unit.cargo_unit_id = [0x99; 32];
        let wrong_unit =
            join_component_payloads(std::slice::from_ref(&definition), &[wrong_unit], &[])
                .unwrap_err();
        assert!(matches!(
            wrong_unit,
            ExtractionError::OrphanDefinitionPayload { .. }
        ));
    }

    #[test]
    fn shared_state_payload_join_is_exact_and_fail_closed() {
        let witness = RelocationIdentity::Symbol("shared_state".to_string());
        let definition = shared_definition(0x11, 0x22, witness, 0);
        let schema_abi = b"\x01object\0mode\0string".to_vec();
        let payload = SharedStateDefinitionPayload {
            location: location(MACHO_SHARED_STATE_PAYLOAD_SECTION, 0),
            cargo_unit_id: [0x11; 32],
            shared_state_record_id: [0x22; 32],
            semantic_payload_hash: [0x33; 32],
            type_key: "acme.workbench".to_string(),
            schema_abi_hash: shared_state_schema_abi_hash(&schema_abi),
            schema_abi,
        };

        let missing =
            join_shared_state_payloads(std::slice::from_ref(&definition), &[]).unwrap_err();
        assert!(matches!(
            missing,
            ExtractionError::MissingDefinitionPayload { .. }
        ));

        let duplicate = join_shared_state_payloads(
            std::slice::from_ref(&definition),
            &[payload.clone(), payload.clone()],
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            ExtractionError::DuplicateDefinitionPayload { .. }
        ));

        let mut wrong_unit = payload.clone();
        wrong_unit.cargo_unit_id = [0x99; 32];
        let orphan = join_shared_state_payloads(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&wrong_unit),
        )
        .unwrap_err();
        assert!(matches!(
            orphan,
            ExtractionError::OrphanDefinitionPayload { .. }
        ));

        let resolved = join_shared_state_payloads(&[definition], &[payload]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].payload.type_key, "acme.workbench");
        assert_eq!(resolved[0].payload.schema_abi, b"\x01object\0mode\0string");
    }

    #[test]
    fn definition_identity_join_is_exact_and_fail_closed() {
        let definition = ComponentDefinition {
            location: location(MACHO_COMPONENT_SECTION, 0),
            cargo_unit_id: [0x11; 32],
            component_record_id: [0x22; 32],
            component_abi: ComponentDefinitionAbi::SYNC_PARTICIPANT,
            witness: RelocationIdentity::Symbol("component".to_string()),
        };
        let payload = ComponentDescriptorPayload {
            location: location(MACHO_COMPONENT_PAYLOAD_SECTION, 0),
            cargo_unit_id: [0x11; 32],
            component_record_id: [0x22; 32],
            semantic_payload_hash: [0x33; 32],
            json: br#"{"component":true}"#.to_vec(),
        };
        let identity = SemanticIdentity {
            location: location(MACHO_SEMANTIC_IDENTITY_SECTION, 0),
            kind: SemanticIdentityKind::Component,
            cargo_unit_id: [0x11; 32],
            definition_record_id: [0x22; 32],
            package: "acme-web".to_string(),
            crate_name: "acme_web".to_string(),
            module_path: vec!["ui".to_string()],
            item: "StatusBadge".to_string(),
        };

        let missing = join_component_payloads(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&payload),
            &[],
        )
        .unwrap_err();
        assert!(matches!(
            missing,
            ExtractionError::MissingSemanticIdentity { .. }
        ));

        let duplicate = join_component_payloads(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&payload),
            &[identity.clone(), identity.clone()],
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            ExtractionError::DuplicateSemanticIdentity { .. }
        ));

        let resolved = join_component_payloads(&[definition], &[payload], &[identity]).unwrap();
        assert_eq!(resolved[0].identity.item, "StatusBadge");
        assert_eq!(resolved[0].identity.module_path, ["ui"]);
    }
}
