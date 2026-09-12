// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic identities for the two-compilation RSJS build pipeline.
//!
//! These types separate semantic build identity from host-local artifact
//! locations. Cargo supplies the realized unit graph and exact artifact paths;
//! the linker validates those paths against content identities without making
//! an absolute path part of [`InvocationId`], [`CargoUnitId`], or
//! [`LinkUnitId`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::abi::{
    AbiVersion, CanonicalEncode, CanonicalEncoder, CargoUnitId, DebugHash, InvocationId,
    LinkUnitId, PlanId, ProducerVersion, SemanticHash,
};

/// Tool and protocol versions that affect descriptor compilation or linking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildToolchain {
    /// Cargo version selected for the invocation.
    pub cargo: String,
    /// Rust compiler version selected for the invocation.
    pub rustc: String,
    /// `cargo-proa` producer version.
    pub proa: ProducerVersion,
    /// Compiler ABI understood by this invocation.
    pub compiler_abi: AbiVersion,
    /// Fixed object-section transport version.
    pub transport_version: u16,
}

impl CanonicalEncode for BuildToolchain {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("BuildToolchain", 5);
        encoder.field(1, &self.cargo);
        encoder.field(2, &self.rustc);
        encoder.field(3, &self.proa);
        encoder.field(4, &self.compiler_abi);
        encoder.field(5, &self.transport_version);
    }
}

/// One root target selected before Cargo realizes its unit graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestedRootTarget {
    /// Cargo package selector or package ID.
    pub package: String,
    /// Cargo target kind (`bin`, `example`, `test`, `bench`, or `lib`).
    pub target_kind: String,
    /// Cargo target name, when explicitly selected.
    pub target_name: Option<String>,
}

impl CanonicalEncode for RequestedRootTarget {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("RequestedRootTarget", 3);
        encoder.field(1, &self.package);
        encoder.field(2, &self.target_kind);
        encoder.field(3, &self.target_name);
    }
}

/// Hash-only representation of one declared environment input.
///
/// Environment values may contain credentials and therefore do not belong in
/// an external link manifest. The orchestrator hashes the exact value before
/// constructing this record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredEnvironmentInput {
    /// Environment variable name.
    pub name: String,
    /// Domain-separated hash of the exact value, including an explicit
    /// distinction between unset and the empty string.
    pub value_hash: SemanticHash,
}

impl CanonicalEncode for DeclaredEnvironmentInput {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DeclaredEnvironmentInput", 2);
        encoder.field(1, &self.name);
        encoder.field(2, &self.value_hash);
    }
}

/// Inputs known before the descriptor Cargo invocation starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationSpec {
    /// Selected root targets. Source order is not semantic.
    pub roots: BTreeSet<RequestedRootTarget>,
    /// Target triple selected by Cargo/rustc.
    pub target_triple: Option<String>,
    /// Enabled Cargo features after CLI normalization.
    pub features: BTreeSet<String>,
    /// Cargo profile name.
    pub profile: String,
    /// Semantic Cargo configuration overrides.
    pub cargo_config: BTreeMap<String, String>,
    /// Whether dependency resolution is locked.
    pub locked: bool,
    /// Whether network access is disabled.
    pub offline: bool,
    /// Other normalized Cargo selection arguments not represented above.
    pub selection_args: Vec<String>,
    /// Declared environment inputs, keyed by name to forbid duplicates.
    pub environment: BTreeMap<String, DeclaredEnvironmentInput>,
    /// Toolchain and ABI versions.
    pub toolchain: BuildToolchain,
}

impl InvocationSpec {
    /// Validate and derive the pre-Cargo invocation identity.
    pub fn invocation_id(&self) -> Result<InvocationId, BuildIdentityError> {
        self.validate()?;
        Ok(InvocationId::derive(self))
    }

    /// Reject ambiguous or non-normalized invocation material.
    pub fn validate(&self) -> Result<(), BuildIdentityError> {
        if self.roots.is_empty() {
            return Err(BuildIdentityError::NoRootTargets);
        }
        require_nonempty("profile", &self.profile)?;
        for root in &self.roots {
            require_nonempty("root package", &root.package)?;
            require_nonempty("root target kind", &root.target_kind)?;
            if let Some(name) = &root.target_name {
                require_nonempty("root target name", name)?;
            }
        }
        for feature in &self.features {
            require_nonempty("feature", feature)?;
        }
        for (name, input) in &self.environment {
            require_nonempty("environment name", name)?;
            if input.name != *name {
                return Err(BuildIdentityError::EnvironmentKeyMismatch {
                    map_key: name.clone(),
                    record_name: input.name.clone(),
                });
            }
        }
        Ok(())
    }
}

impl CanonicalEncode for InvocationSpec {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("InvocationSpec", 10);
        encoder.field(1, &self.roots);
        encoder.field(2, &self.target_triple);
        encoder.field(3, &self.features);
        encoder.field(4, &self.profile);
        encoder.field(5, &self.cargo_config);
        encoder.field(6, &self.locked);
        encoder.field(7, &self.offline);
        encoder.field(8, &self.selection_args);
        encoder.field(9, &self.environment);
        encoder.field(10, &self.toolchain);
    }
}

/// Descriptor-pass codegen changes permitted solely to retain transport
/// records for extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorCodegenPolicy {
    /// LTO policy used by the descriptor pass.
    pub lto: String,
    /// Strip policy used by the descriptor pass.
    pub strip: String,
    /// Number of descriptor-pass codegen units.
    pub codegen_units: Option<u32>,
    /// Whether dead-code linking is requested to retain records.
    pub link_dead_code: bool,
}

impl CanonicalEncode for DescriptorCodegenPolicy {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("DescriptorCodegenPolicy", 4);
        encoder.field(1, &self.lto);
        encoder.field(2, &self.strip);
        encoder.field(3, &self.codegen_units);
        encoder.field(4, &self.link_dead_code);
    }
}

/// One realized dependency of a descriptor-pass rustc unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoUnitDependency {
    /// `--extern` name visible to the consuming unit.
    pub extern_name: String,
    /// Identity assigned when the dependency unit ran.
    pub unit: CargoUnitId,
    /// Digest of the exact dependency artifact bytes consumed by rustc.
    pub artifact: SemanticHash,
}

impl CanonicalEncode for CargoUnitDependency {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("CargoUnitDependency", 3);
        encoder.field(1, &self.extern_name);
        encoder.field(2, &self.unit);
        encoder.field(3, &self.artifact);
    }
}

/// Semantic fingerprint material available when one descriptor-pass rustc
/// unit executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoUnitSpec {
    /// Cargo package ID.
    pub package_id: String,
    /// Target name and kind selected by Cargo.
    pub target_name: String,
    /// Cargo target kind.
    pub target_kind: String,
    /// Rust crate name.
    pub crate_name: String,
    /// Rust crate types in normalized order.
    pub crate_types: BTreeSet<String>,
    /// Effective target triple.
    pub target_triple: String,
    /// Effective profile.
    pub profile: String,
    /// Realized `--cfg` values.
    pub cfgs: BTreeSet<String>,
    /// Hash of normalized behavior-affecting rustc inputs not represented by
    /// another field.
    pub rustc_inputs: SemanticHash,
    /// Hash of the realized build-script environment.
    pub build_script_environment: SemanticHash,
    /// Exact dependency artifact identities.
    pub dependencies: BTreeSet<CargoUnitDependency>,
    /// Documented descriptor-pass retention policy.
    pub descriptor_codegen: DescriptorCodegenPolicy,
}

impl CargoUnitSpec {
    /// Validate and derive the reusable descriptor unit identity.
    pub fn cargo_unit_id(&self) -> Result<CargoUnitId, BuildIdentityError> {
        self.validate()?;
        Ok(CargoUnitId::derive(self))
    }

    /// Reject missing or contradictory unit identity material.
    pub fn validate(&self) -> Result<(), BuildIdentityError> {
        require_nonempty("package id", &self.package_id)?;
        require_nonempty("target name", &self.target_name)?;
        require_nonempty("target kind", &self.target_kind)?;
        require_nonempty("crate name", &self.crate_name)?;
        require_nonempty("target triple", &self.target_triple)?;
        require_nonempty("profile", &self.profile)?;
        if self.crate_types.is_empty() {
            return Err(BuildIdentityError::NoCrateTypes);
        }
        for crate_type in &self.crate_types {
            require_nonempty("crate type", crate_type)?;
        }
        for dependency in &self.dependencies {
            require_nonempty("dependency extern name", &dependency.extern_name)?;
        }
        require_nonempty("descriptor LTO policy", &self.descriptor_codegen.lto)?;
        require_nonempty("descriptor strip policy", &self.descriptor_codegen.strip)?;
        Ok(())
    }
}

impl CanonicalEncode for CargoUnitSpec {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("CargoUnitSpec", 12);
        encoder.field(1, &self.package_id);
        encoder.field(2, &self.target_name);
        encoder.field(3, &self.target_kind);
        encoder.field(4, &self.crate_name);
        encoder.field(5, &self.crate_types);
        encoder.field(6, &self.target_triple);
        encoder.field(7, &self.profile);
        encoder.field(8, &self.cfgs);
        encoder.field(9, &self.rustc_inputs);
        encoder.field(10, &self.build_script_environment);
        encoder.field(11, &self.dependencies);
        encoder.field(12, &self.descriptor_codegen);
    }
}

/// Semantic role of one Cargo-enumerated artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ArtifactRole {
    /// Selected executable root.
    RootExecutable,
    /// Selected test executable root.
    RootTest,
    /// Selected benchmark executable root.
    RootBenchmark,
    /// Selected example executable root.
    RootExample,
    /// Selected library root for check/build workflows without an executable.
    RootLibrary,
    /// Rust library dependency.
    RustLibrary,
    /// Rust metadata dependency consumed through an exact `.rmeta`.
    RustMetadata,
    /// Procedural macro dependency.
    ProcMacro,
    /// Native object input.
    Object,
    /// Descriptor-pass object containing RSJS transport records.
    DescriptorObject,
    /// Native static library input.
    StaticLibrary,
    /// Native dynamic library input.
    DynamicLibrary,
}

impl ArtifactRole {
    /// Whether this role is a selected root artifact.
    pub const fn is_root(self) -> bool {
        matches!(
            self,
            Self::RootExecutable
                | Self::RootTest
                | Self::RootBenchmark
                | Self::RootExample
                | Self::RootLibrary
        )
    }
}

impl CanonicalEncode for ArtifactRole {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        let (name, discriminant) = match self {
            Self::RootExecutable => ("RootExecutable", 1),
            Self::RootTest => ("RootTest", 2),
            Self::RootBenchmark => ("RootBenchmark", 3),
            Self::RootExample => ("RootExample", 4),
            Self::RootLibrary => ("RootLibrary", 10),
            Self::RustLibrary => ("RustLibrary", 5),
            Self::RustMetadata => ("RustMetadata", 11),
            Self::ProcMacro => ("ProcMacro", 6),
            Self::Object => ("Object", 7),
            Self::DescriptorObject => ("DescriptorObject", 12),
            Self::StaticLibrary => ("StaticLibrary", 8),
            Self::DynamicLibrary => ("DynamicLibrary", 9),
        };
        encoder.variant(name, discriminant, 0);
    }
}

/// One exact artifact enumerated by Cargo for a root link unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkArtifact {
    /// Unit that produced the artifact.
    pub unit: CargoUnitId,
    /// Link/extraction role.
    pub role: ArtifactRole,
    /// Digest of the exact artifact bytes.
    pub content: SemanticHash,
    /// Host-local path supplied by the same Cargo invocation.
    ///
    /// This is validated and serialized by the orchestrator but intentionally
    /// excluded from canonical identity. Moving an otherwise byte-identical
    /// target directory must not change a link plan.
    pub path: String,
}

impl LinkArtifact {
    fn encode_semantic(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkArtifactSemantic", 3);
        encoder.field(1, &self.unit);
        encoder.field(2, &self.role);
        encoder.field(3, &self.content);
    }
}

impl CanonicalEncode for LinkArtifact {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        self.encode_semantic(encoder);
    }
}

/// One realized unit dependency in a root-specific link graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkUnitDependency {
    /// Consuming unit.
    pub from: CargoUnitId,
    /// Dependency unit.
    pub to: CargoUnitId,
    /// Cargo/rustc edge label.
    pub extern_name: String,
}

impl CanonicalEncode for LinkUnitDependency {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkUnitDependency", 3);
        encoder.field(1, &self.from);
        encoder.field(2, &self.to);
        encoder.field(3, &self.extern_name);
    }
}

/// External, root-specific manifest produced from Cargo JSON messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkUnitManifest {
    /// Invocation that realized this graph.
    pub invocation: InvocationId,
    /// Selected root unit.
    pub root_unit: CargoUnitId,
    /// Exact ordered artifacts extracted/linked for this root.
    pub artifacts: Vec<LinkArtifact>,
    /// Realized unit dependency edges.
    pub dependencies: BTreeSet<LinkUnitDependency>,
}

impl LinkUnitManifest {
    /// Validate and derive the root-specific link identity.
    pub fn link_unit_id(&self) -> Result<LinkUnitId, BuildIdentityError> {
        self.validate()?;
        Ok(LinkUnitId::derive(self))
    }

    /// Validate exact root membership, artifact uniqueness, and graph shape.
    pub fn validate(&self) -> Result<(), BuildIdentityError> {
        if self.artifacts.is_empty() {
            return Err(BuildIdentityError::NoArtifacts);
        }

        let units = self
            .artifacts
            .iter()
            .map(|artifact| artifact.unit)
            .collect::<BTreeSet<_>>();
        if !units.contains(&self.root_unit) {
            return Err(BuildIdentityError::MissingRootUnit(self.root_unit));
        }

        let mut root_count = 0;
        let mut semantic_artifacts = BTreeSet::new();
        for artifact in &self.artifacts {
            require_nonempty("artifact path", &artifact.path)?;
            if artifact.role.is_root() {
                root_count += 1;
                if artifact.unit != self.root_unit {
                    return Err(BuildIdentityError::RootArtifactUnitMismatch {
                        expected: self.root_unit,
                        actual: artifact.unit,
                    });
                }
            }
            if !semantic_artifacts.insert((artifact.unit, artifact.role, artifact.content)) {
                return Err(BuildIdentityError::DuplicateArtifact {
                    unit: artifact.unit,
                    role: artifact.role,
                });
            }
        }
        if root_count != 1 {
            return Err(BuildIdentityError::RootArtifactCount(root_count));
        }

        for edge in &self.dependencies {
            require_nonempty("dependency edge name", &edge.extern_name)?;
            if !units.contains(&edge.from) {
                return Err(BuildIdentityError::UnknownDependencyUnit(edge.from));
            }
            if !units.contains(&edge.to) {
                return Err(BuildIdentityError::UnknownDependencyUnit(edge.to));
            }
            if edge.from == edge.to {
                return Err(BuildIdentityError::SelfDependency(edge.from));
            }
        }
        validate_acyclic(&units, &self.dependencies)
    }
}

impl CanonicalEncode for LinkUnitManifest {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkUnitManifest", 4);
        encoder.field(1, &self.invocation);
        encoder.field(2, &self.root_unit);
        encoder.field(3, &self.artifacts);
        encoder.field(4, &self.dependencies);
    }
}

/// Semantic material hashed into one root's final linked plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPlanMaterial {
    /// Exact pre-link root identity.
    pub link_unit: LinkUnitId,
    /// Ordered complete component descriptor hashes.
    pub component_descriptors: Vec<SemanticHash>,
    /// Ordered complete helper descriptor hashes.
    pub helper_descriptors: Vec<SemanticHash>,
    /// Hash of the canonical owner/code/serialization plan payload with its
    /// build-ID slots omitted.
    pub payload: SemanticHash,
}

impl LinkPlanMaterial {
    /// Derive the content-addressed final plan identity.
    pub fn plan_id(&self) -> PlanId {
        PlanId::derive(self)
    }
}

impl CanonicalEncode for LinkPlanMaterial {
    fn encode_canonical(&self, encoder: &mut CanonicalEncoder) {
        encoder.record("LinkPlanMaterial", 4);
        encoder.field(1, &self.link_unit);
        encoder.field(2, &self.component_descriptors);
        encoder.field(3, &self.helper_descriptors);
        encoder.field(4, &self.payload);
    }
}

/// Optional non-semantic provenance attached to an external manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildDebugProvenance {
    /// Hash of diagnostic/source provenance.
    pub debug_hash: DebugHash,
    /// Human-readable invocation label.
    pub label: String,
}

fn require_nonempty(field: &'static str, value: &str) -> Result<(), BuildIdentityError> {
    if value.trim().is_empty() {
        Err(BuildIdentityError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn validate_acyclic(
    units: &BTreeSet<CargoUnitId>,
    dependencies: &BTreeSet<LinkUnitDependency>,
) -> Result<(), BuildIdentityError> {
    let mut indegree = units
        .iter()
        .copied()
        .map(|unit| (unit, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<CargoUnitId, Vec<CargoUnitId>>::new();
    for edge in dependencies {
        // A consumer points at its dependency. Reversing the edge here is not
        // semantically important; Kahn's algorithm only needs consistency.
        outgoing.entry(edge.from).or_default().push(edge.to);
        *indegree
            .get_mut(&edge.to)
            .expect("manifest validation checked dependency membership") += 1;
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(unit, count)| (*count == 0).then_some(*unit))
        .collect::<BTreeSet<_>>();
    let mut visited = 0;
    while let Some(unit) = ready.pop_first() {
        visited += 1;
        if let Some(next) = outgoing.get(&unit) {
            for dependency in next {
                let count = indegree
                    .get_mut(dependency)
                    .expect("manifest validation checked dependency membership");
                *count -= 1;
                if *count == 0 {
                    ready.insert(*dependency);
                }
            }
        }
    }
    if visited == units.len() {
        Ok(())
    } else {
        Err(BuildIdentityError::DependencyCycle)
    }
}

/// Fail-closed build identity validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuildIdentityError {
    /// No root target was requested.
    NoRootTargets,
    /// A required normalized field is empty.
    EmptyField(&'static str),
    /// Declared environment map key and record disagree.
    EnvironmentKeyMismatch {
        /// Map key.
        map_key: String,
        /// Name stored in the record.
        record_name: String,
    },
    /// A realized unit declares no crate type.
    NoCrateTypes,
    /// Cargo emitted no artifacts for the selected root.
    NoArtifacts,
    /// The selected root unit has no artifact.
    MissingRootUnit(CargoUnitId),
    /// Root-role artifact belongs to another unit.
    RootArtifactUnitMismatch {
        /// Selected root.
        expected: CargoUnitId,
        /// Artifact producer.
        actual: CargoUnitId,
    },
    /// A root manifest must contain exactly one selected root artifact.
    RootArtifactCount(usize),
    /// Duplicate semantic artifact identity.
    DuplicateArtifact {
        /// Artifact producer.
        unit: CargoUnitId,
        /// Artifact role.
        role: ArtifactRole,
    },
    /// Dependency references a unit absent from the exact artifact set.
    UnknownDependencyUnit(CargoUnitId),
    /// Unit graph contains a self-edge.
    SelfDependency(CargoUnitId),
    /// Unit graph contains a cycle.
    DependencyCycle,
}

impl fmt::Display for BuildIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRootTargets => write!(formatter, "no Cargo root target was selected"),
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::EnvironmentKeyMismatch {
                map_key,
                record_name,
            } => write!(
                formatter,
                "declared environment key `{map_key}` contains record `{record_name}`"
            ),
            Self::NoCrateTypes => write!(formatter, "realized Cargo unit has no crate type"),
            Self::NoArtifacts => write!(formatter, "root link manifest contains no artifacts"),
            Self::MissingRootUnit(unit) => {
                write!(formatter, "root unit {unit} has no enumerated artifact")
            }
            Self::RootArtifactUnitMismatch { expected, actual } => write!(
                formatter,
                "root artifact belongs to unit {actual}, expected selected root {expected}"
            ),
            Self::RootArtifactCount(count) => write!(
                formatter,
                "root link manifest must contain exactly one root artifact, found {count}"
            ),
            Self::DuplicateArtifact { unit, role } => {
                write!(formatter, "duplicate {role:?} artifact for unit {unit}")
            }
            Self::UnknownDependencyUnit(unit) => {
                write!(formatter, "dependency references unknown unit {unit}")
            }
            Self::SelfDependency(unit) => write!(formatter, "unit {unit} depends on itself"),
            Self::DependencyCycle => write!(formatter, "Cargo unit graph contains a cycle"),
        }
    }
}

impl std::error::Error for BuildIdentityError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::canonical_bytes;

    fn semantic(value: &str) -> SemanticHash {
        SemanticHash::derive(value)
    }

    fn toolchain() -> BuildToolchain {
        BuildToolchain {
            cargo: "cargo 1.90.0".into(),
            rustc: "rustc 1.90.0".into(),
            proa: ProducerVersion::new(0, 1, 0),
            compiler_abi: AbiVersion::new(1, 0),
            transport_version: 1,
        }
    }

    fn invocation() -> InvocationSpec {
        InvocationSpec {
            roots: BTreeSet::from([RequestedRootTarget {
                package: "app".into(),
                target_kind: "bin".into(),
                target_name: Some("app".into()),
            }]),
            target_triple: Some("aarch64-apple-darwin".into()),
            features: BTreeSet::from(["hydration".into()]),
            profile: "dev".into(),
            cargo_config: BTreeMap::new(),
            locked: true,
            offline: false,
            selection_args: vec!["--workspace".into()],
            environment: BTreeMap::new(),
            toolchain: toolchain(),
        }
    }

    fn unit(name: &str) -> CargoUnitId {
        CargoUnitId::derive(name)
    }

    fn manifest() -> LinkUnitManifest {
        let root = unit("app");
        let dependency = unit("ui");
        LinkUnitManifest {
            invocation: invocation().invocation_id().unwrap(),
            root_unit: root,
            artifacts: vec![
                LinkArtifact {
                    unit: root,
                    role: ArtifactRole::RootExecutable,
                    content: semantic("app-bytes"),
                    path: "/tmp/target/debug/app".into(),
                },
                LinkArtifact {
                    unit: dependency,
                    role: ArtifactRole::RustLibrary,
                    content: semantic("ui-bytes"),
                    path: "/tmp/target/debug/deps/libui.rlib".into(),
                },
            ],
            dependencies: BTreeSet::from([LinkUnitDependency {
                from: root,
                to: dependency,
                extern_name: "ui".into(),
            }]),
        }
    }

    #[test]
    fn invocation_is_stable_across_set_and_map_insertion_order() {
        let left = invocation();
        let mut right = invocation();
        right.features = ["hydration".to_string()].into_iter().rev().collect();
        assert_eq!(
            left.invocation_id().unwrap(),
            right.invocation_id().unwrap()
        );
    }

    #[test]
    fn unit_identity_includes_realized_dependency_artifacts() {
        let base = CargoUnitSpec {
            package_id: "ui 0.1.0".into(),
            target_name: "ui".into(),
            target_kind: "lib".into(),
            crate_name: "ui".into(),
            crate_types: BTreeSet::from(["rlib".into()]),
            target_triple: "aarch64-apple-darwin".into(),
            profile: "dev".into(),
            cfgs: BTreeSet::from(["feature=\"hydration\"".into()]),
            rustc_inputs: semantic("rustc-inputs"),
            build_script_environment: semantic("build-env"),
            dependencies: BTreeSet::from([CargoUnitDependency {
                extern_name: "proa".into(),
                unit: unit("proa-a"),
                artifact: semantic("proa-a-bytes"),
            }]),
            descriptor_codegen: DescriptorCodegenPolicy {
                lto: "off".into(),
                strip: "none".into(),
                codegen_units: Some(2),
                link_dead_code: true,
            },
        };
        let mut changed = base.clone();
        changed.dependencies = BTreeSet::from([CargoUnitDependency {
            extern_name: "proa".into(),
            unit: unit("proa-b"),
            artifact: semantic("proa-b-bytes"),
        }]);
        assert_ne!(
            base.cargo_unit_id().unwrap(),
            changed.cargo_unit_id().unwrap()
        );
    }

    #[test]
    fn host_artifact_path_is_not_semantic_identity() {
        let left = manifest();
        let mut right = left.clone();
        right.artifacts[0].path = "/different/target/debug/app".into();
        right.artifacts[1].path = "/different/target/debug/deps/libui.rlib".into();

        assert_eq!(left.link_unit_id().unwrap(), right.link_unit_id().unwrap());
        assert_eq!(canonical_bytes(&left), canonical_bytes(&right));
    }

    #[test]
    fn artifact_order_is_root_manifest_identity() {
        let left = manifest();
        let mut right = left.clone();
        right.artifacts.reverse();
        assert_ne!(left.link_unit_id().unwrap(), right.link_unit_id().unwrap());
    }

    #[test]
    fn root_and_dependency_graph_fail_closed() {
        let mut missing_root = manifest();
        missing_root.root_unit = unit("other");
        assert!(matches!(
            missing_root.validate(),
            Err(BuildIdentityError::MissingRootUnit(_))
        ));

        let mut cycle = manifest();
        cycle.dependencies.insert(LinkUnitDependency {
            from: unit("ui"),
            to: unit("app"),
            extern_name: "app".into(),
        });
        assert_eq!(cycle.validate(), Err(BuildIdentityError::DependencyCycle));
    }

    #[test]
    fn plan_identity_binds_root_descriptors_helpers_and_payload() {
        let link_unit = manifest().link_unit_id().unwrap();
        let base = LinkPlanMaterial {
            link_unit,
            component_descriptors: vec![semantic("component")],
            helper_descriptors: vec![semantic("helper")],
            payload: semantic("payload"),
        };
        let mut changed = base.clone();
        changed.payload = semantic("other-payload");
        assert_ne!(base.plan_id(), changed.plan_id());
    }
}
