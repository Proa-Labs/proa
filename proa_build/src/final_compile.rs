// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Plan-sensitive final Cargo compilation and execution for RSJS roots.
//!
//! The final compiler is intentionally separate from descriptor compilation.
//! A content-addressed plan is validated before Cargo starts, its [`PlanId`] is
//! supplied to the final rustc wrapper. Artifacts share a wrapper-policy target
//! namespace; generated RSJS witnesses expose the exact plan/map fingerprints
//! to Cargo so only plan-sensitive units are invalidated.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use object::{Architecture, BinaryFormat, Object as _};
use rsjs_ir::abi::{FinalUnitId, InvocationId, LinkUnitId, PlanId, SemanticHash};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::build::{
    acquire_descriptor_target_lease, atomic_write_exact, final_unit_tracking_inputs,
    scrub_wrapper_control_environment, wrapper_control_environment,
    write_content_addressed_final_unit_map, CargoArtifactCollector, CargoArtifactTarget,
    CargoCompilerArtifact, CargoMessageError, DescriptorCargoRunError, DescriptorTargetLease,
    FinalUnitMapError, ValidatedDescriptorUnit, COMPONENT_DESCRIPTOR_DIRS_ENV,
    FINAL_CARGO_UNIT_STATUS_ENV, FINAL_TARGET_DIR_ENV, FINAL_UNIT_MAP_HASH_ENV,
    FINAL_UNIT_MAP_PATH_ENV, FINAL_UNIT_PLAN_DIRECTORY_ENV, FINAL_UNIT_PLAN_ENV_PREFIX,
    FINAL_WRAPPER_POLICY_VERSION, SOURCE_EXCLUSION_DIR_ENV, WRAPPER_MODE_ENV,
    WRAPPER_MODE_FINAL_V2,
};
pub use crate::build::{
    FINAL_PHASE_ENV, FINAL_PLAN_FILE_HASH_ENV, FINAL_PLAN_ID_ENV, FINAL_PLAN_PATH_ENV,
};
use crate::link::{transport_plan_json, TransportLinkPlan};
use crate::semantic_link::{
    semantic_plan_json, SemanticLinkPlan, SEMANTIC_PLAN_FILE_SUFFIX, SEMANTIC_PLAN_FORMAT,
};

const PLAN_FILE_FORMAT: &str = "proa.rsjs.resolved_transport_plan";
const TRANSPORT_PLAN_FILE_SUFFIX: &str = "transport-plan.json";
static FINAL_SNAPSHOT_NONCE: AtomicU64 = AtomicU64::new(0);
/// Linked-plan variants accepted by the final Cargo runner.
pub trait FinalCargoPlan {
    /// Strong plan identity used for Cargo invalidation.
    fn final_plan_id(&self) -> PlanId;
    /// Exact root graph identity.
    fn final_link_unit(&self) -> LinkUnitId;
    /// External JSON format discriminator.
    fn final_plan_format(&self) -> &'static str;
    /// Content-addressed filename suffix.
    fn final_plan_file_suffix(&self) -> &'static str;
    /// Complete deterministic JSON value expected at the linked-plan path.
    fn final_plan_json(&self) -> Result<Value, String>;
}

impl FinalCargoPlan for TransportLinkPlan {
    fn final_plan_id(&self) -> PlanId {
        self.plan_id
    }

    fn final_link_unit(&self) -> LinkUnitId {
        self.link_unit
    }

    fn final_plan_format(&self) -> &'static str {
        PLAN_FILE_FORMAT
    }

    fn final_plan_file_suffix(&self) -> &'static str {
        TRANSPORT_PLAN_FILE_SUFFIX
    }

    fn final_plan_json(&self) -> Result<Value, String> {
        Ok(transport_plan_json(self))
    }
}

impl FinalCargoPlan for SemanticLinkPlan {
    fn final_plan_id(&self) -> PlanId {
        self.plan_id
    }

    fn final_link_unit(&self) -> LinkUnitId {
        self.link_unit
    }

    fn final_plan_format(&self) -> &'static str {
        SEMANTIC_PLAN_FORMAT
    }

    fn final_plan_file_suffix(&self) -> &'static str {
        SEMANTIC_PLAN_FILE_SUFFIX
    }

    fn final_plan_json(&self) -> Result<Value, String> {
        semantic_plan_json(self).map_err(|error| error.to_string())
    }
}

/// One root-specific final Cargo invocation.
#[derive(Clone, Debug)]
pub struct FinalCargoRequest {
    /// Stable pre-Cargo invocation identity separating roots, features,
    /// profiles, and target selections while remaining unchanged by sources.
    pub invocation: InvocationId,
    /// Cargo executable.
    pub cargo: OsString,
    /// Absolute `cargo-proa`/`proa` executable used as `RUSTC_WRAPPER`.
    pub wrapper: PathBuf,
    /// Absolute Cargo working directory.
    pub working_directory: PathBuf,
    /// Exact generated subtree excluded from both descriptor and final package
    /// source-closure identities.
    pub source_exclusion_directory: PathBuf,
    /// Absolute parent for wrapper-policy-scoped final target directories.
    pub target_root: PathBuf,
    /// Exact content-addressed plan written by the linker.
    pub plan_path: PathBuf,
    /// Cargo arguments including subcommand and target selection.
    ///
    /// The runner owns `--message-format` and `--target-dir`.
    pub cargo_args: Vec<OsString>,
    /// Additional exact build environment.
    pub environment: BTreeMap<OsString, OsString>,
    /// Semantic fingerprint of the root descriptor unit before plan
    /// specialization.
    pub root_unit_fingerprint: SemanticHash,
    /// Freshly validated descriptor units authorized for final identity
    /// restoration.
    pub descriptor_units: Vec<ValidatedDescriptorUnit>,
}

/// Path-free report for one final Cargo artifact.
///
/// Cargo's materialized artifact paths live in a reusable target and may be
/// replaced by a later build. Only the leased runner sees those raw paths;
/// durable linked outputs expose an immutable snapshot of the selected root.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalCargoArtifactReport {
    /// Cargo package ID, exactly as Cargo emitted it.
    pub package_id: String,
    /// Target identity reported by Cargo.
    pub target: CargoArtifactTarget,
    /// Profile object reported by Cargo.
    pub profile: Value,
    /// Enabled features, normalized for exact matching.
    pub features: BTreeSet<String>,
    /// Whether Cargo reused the artifact.
    pub fresh: bool,
}

impl From<&CargoCompilerArtifact> for FinalCargoArtifactReport {
    fn from(artifact: &CargoCompilerArtifact) -> Self {
        Self {
            package_id: artifact.package_id.clone(),
            target: artifact.target.clone(),
            profile: artifact.profile.clone(),
            features: artifact.features.clone(),
            fresh: artifact.fresh,
        }
    }
}

/// Successful plan-sensitive Cargo output.
#[derive(Clone, Debug)]
pub struct FinalCargoOutput {
    /// Plan consumed by this compilation.
    pub plan_id: PlanId,
    /// Root unit specialized by `plan_id`.
    pub final_unit_id: FinalUnitId,
    /// Stable wrapper-policy-scoped target directory.
    pub target_directory: PathBuf,
    /// Content-addressed phase-invariant unit map consumed by the wrapper.
    pub unit_map_path: PathBuf,
    /// SHA-256 of the exact unit-map bytes.
    pub unit_map_hash: String,
    /// Tagged `sha256:<hex>` digest of the exact linked-plan bytes.
    pub plan_file_hash: String,
    /// Path-free reports from strictly parsed Cargo artifact messages.
    pub cargo_artifacts: Vec<FinalCargoArtifactReport>,
}

/// One final Cargo result whose stable target remains exclusively leased.
///
/// The linked pipeline retains this lease through root attestation, launch
/// dependency validation, and immutable snapshot publication. This prevents a
/// concurrent build for the same invocation from replacing stable target
/// paths while any of those operations still reads them. Once the selected
/// root is represented only by a supported content-addressed snapshot, the
/// pipeline consumes this value and releases the lease before execution.
#[must_use = "final Cargo runs retain their target lease until consumed"]
pub(crate) struct FinalCargoRun {
    output: FinalCargoOutput,
    cargo_artifacts: Vec<CargoCompilerArtifact>,
    _target_lease: DescriptorTargetLease,
}

impl FinalCargoRun {
    pub(crate) fn output(&self) -> &FinalCargoOutput {
        &self.output
    }

    pub(crate) fn cargo_artifacts(&self) -> &[CargoCompilerArtifact] {
        &self.cargo_artifacts
    }

    pub(crate) fn into_output(self) -> FinalCargoOutput {
        self.output
    }
}

/// Validate the plan and run one final, compile-only Cargo invocation.
///
/// An original `cargo run` request is compiled with `cargo build`; an original
/// `cargo test` request is compiled with `cargo test --no-run`. Runtime
/// arguments are never passed to Cargo here. Before optional execution, the
/// linked pipeline attests the exact resulting root, validates its launch
/// dependencies, and publishes a supported immutable snapshot.
pub fn run_final_cargo<P: FinalCargoPlan>(
    request: &FinalCargoRequest,
    plan: &P,
) -> Result<FinalCargoOutput, FinalCargoError> {
    run_final_cargo_leased(request, plan).map(FinalCargoRun::into_output)
}

/// Run final Cargo while retaining exclusive authority over its stable target.
///
/// Callers that inspect materialized target paths must keep the returned lease
/// alive until inspection finishes or the selected root has passed launch
/// dependency validation and been published into immutable storage. The linked
/// pipeline uses the latter path so optional execution does not retain the
/// reusable target lease.
pub(crate) fn run_final_cargo_leased<P: FinalCargoPlan>(
    request: &FinalCargoRequest,
    plan: &P,
) -> Result<FinalCargoRun, FinalCargoError> {
    let mut timing = super::timing::StageTimer::new();
    validate_request(request)?;
    let (plan_file_hash, plan_bytes) = validate_plan_file(&request.plan_path, plan)?;
    let plan_id = plan.final_plan_id();
    let unit_map_directory = request.target_root.join("rsjs-final-unit-maps");
    ensure_plain_managed_directory(&request.source_exclusion_directory, &unit_map_directory)?;
    let unit_map = write_content_addressed_final_unit_map(
        &unit_map_directory,
        plan_id,
        &plan_file_hash,
        &request.descriptor_units,
    )
    .map_err(FinalCargoError::UnitMap)?;
    ensure_plain_managed_file(&request.source_exclusion_directory, &unit_map.path)?;

    timing.mark("final request validation and unit map");
    let target_directory = final_target_directory(&request.target_root, request.invocation);
    ensure_plain_managed_directory(&request.source_exclusion_directory, &target_directory)?;
    let target_lease =
        acquire_descriptor_target_lease(&target_directory).map_err(FinalCargoError::TargetLease)?;

    let mut environment = request.environment.clone();
    environment.extend(
        final_unit_tracking_inputs(&request.descriptor_units).map_err(FinalCargoError::UnitMap)?,
    );
    if plan.final_plan_format() == SEMANTIC_PLAN_FORMAT {
        environment.extend(publish_unit_projections(request, plan_id, &plan_bytes)?);
    }
    inject_plan_environment(
        &mut environment,
        request,
        plan,
        &plan_file_hash,
        &unit_map.path,
        &unit_map.content_hash,
    )?;
    let cargo_args = compile_only_cargo_arguments(&request.cargo_args)?;
    let mut command = Command::new(&request.cargo);
    command
        .current_dir(&request.working_directory)
        .args(&cargo_args)
        .arg("--target-dir")
        .arg(&target_directory)
        .arg("--message-format=json-render-diagnostics");
    scrub_wrapper_control_environment(&mut command);
    command
        .envs(&environment)
        .env("RUSTC_WRAPPER", &request.wrapper)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env(WRAPPER_MODE_ENV, WRAPPER_MODE_FINAL_V2)
        .env(
            SOURCE_EXCLUSION_DIR_ENV,
            &request.source_exclusion_directory,
        )
        .env(FINAL_TARGET_DIR_ENV, &target_directory)
        .env_remove("RSJS_DESCRIPTOR_PHASE")
        .env_remove("RSJS_CARGO_UNIT_ID")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    timing.mark("final Cargo setup");
    let mut child = command
        .spawn()
        .map_err(|source| FinalCargoError::SpawnCargo {
            program: PathBuf::from(&request.cargo),
            source,
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or(FinalCargoError::MissingCargoStdout)?;
    let mut collector = CargoArtifactCollector::default();
    let mut stdout = BufReader::new(stdout);
    let mut cargo_line_number = 0_usize;
    loop {
        let mut bytes = Vec::new();
        let read = match stdout.read_until(b'\n', &mut bytes) {
            Ok(read) => read,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FinalCargoError::ReadCargoStdout(source));
            }
        };
        if read == 0 {
            break;
        }
        cargo_line_number += 1;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let line = match std::str::from_utf8(&bytes) {
            Ok(line) => line,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FinalCargoError::NonUtf8CargoProtocol {
                    line: cargo_line_number,
                    source,
                });
            }
        };
        forward_rendered_cargo_diagnostic(line);
        if let Err(source) = collector.ingest_line(line) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FinalCargoError::CargoProtocol(source));
        }
    }
    let status = child.wait().map_err(FinalCargoError::WaitForCargo)?;
    if !status.success() {
        return Err(FinalCargoError::CargoFailed(status));
    }
    let cargo_artifacts = collector.finish().map_err(FinalCargoError::CargoProtocol)?;
    if cargo_artifacts.is_empty() {
        return Err(FinalCargoError::NoCargoArtifacts);
    }
    let cargo_artifact_reports = cargo_artifacts
        .iter()
        .map(FinalCargoArtifactReport::from)
        .collect();

    timing.mark("final Cargo execution");
    Ok(FinalCargoRun {
        output: FinalCargoOutput {
            plan_id,
            final_unit_id: FinalUnitId::for_plan(request.root_unit_fingerprint, plan_id),
            target_directory,
            unit_map_path: unit_map.path,
            unit_map_hash: unit_map.content_hash,
            plan_file_hash,
            cargo_artifacts: cargo_artifact_reports,
        },
        cargo_artifacts,
        _target_lease: target_lease,
    })
}

fn validate_request(request: &FinalCargoRequest) -> Result<(), FinalCargoError> {
    if !request.wrapper.is_absolute() {
        return Err(FinalCargoError::RelativeWrapper(request.wrapper.clone()));
    }
    if !request.working_directory.is_absolute() {
        return Err(FinalCargoError::RelativeWorkingDirectory(
            request.working_directory.clone(),
        ));
    }
    if !request.target_root.is_absolute() {
        return Err(FinalCargoError::RelativeTargetRoot(
            request.target_root.clone(),
        ));
    }
    if !request.source_exclusion_directory.is_absolute() {
        return Err(FinalCargoError::RelativeSourceExclusionDirectory(
            request.source_exclusion_directory.clone(),
        ));
    }
    if !request.plan_path.is_absolute() {
        return Err(FinalCargoError::RelativePlanPath(request.plan_path.clone()));
    }
    if request.cargo_args.is_empty() {
        return Err(FinalCargoError::MissingCargoArguments);
    }
    if env::var_os(COMPONENT_DESCRIPTOR_DIRS_ENV).is_some()
        || request
            .environment
            .contains_key(OsStr::new(COMPONENT_DESCRIPTOR_DIRS_ENV))
    {
        return Err(FinalCargoError::UnsupportedEnvironment(
            COMPONENT_DESCRIPTOR_DIRS_ENV,
        ));
    }
    let cargo_args = compile_only_cargo_arguments(&request.cargo_args)?;
    for argument in cargo_args {
        let argument = argument.to_string_lossy();
        if argument == "--target-dir"
            || argument.starts_with("--target-dir=")
            || argument == "--message-format"
            || argument.starts_with("--message-format=")
        {
            return Err(FinalCargoError::ReservedCargoArgument(
                argument.into_owned(),
            ));
        }
    }
    if let Some(name) = request
        .environment
        .keys()
        .find(|name| name.to_str().is_some_and(wrapper_control_environment))
    {
        return Err(FinalCargoError::ReservedEnvironment(
            name.to_string_lossy().into_owned(),
        ));
    }
    for reserved in [
        OsStr::new(FINAL_PHASE_ENV),
        OsStr::new(FINAL_PLAN_PATH_ENV),
        OsStr::new(FINAL_PLAN_ID_ENV),
        OsStr::new(FINAL_PLAN_FILE_HASH_ENV),
        OsStr::new(FINAL_UNIT_MAP_PATH_ENV),
        OsStr::new(FINAL_UNIT_MAP_HASH_ENV),
        OsStr::new(FINAL_CARGO_UNIT_STATUS_ENV),
        OsStr::new(WRAPPER_MODE_ENV),
        OsStr::new(SOURCE_EXCLUSION_DIR_ENV),
        OsStr::new(FINAL_TARGET_DIR_ENV),
        OsStr::new("RSJS_DESCRIPTOR_PHASE"),
        OsStr::new("RSJS_CARGO_UNIT_ID"),
    ] {
        if request.environment.contains_key(reserved) {
            return Err(FinalCargoError::ReservedEnvironment(
                reserved.to_string_lossy().into_owned(),
            ));
        }
    }
    if env::var_os("RUSTC_WORKSPACE_WRAPPER").is_some()
        || request
            .environment
            .contains_key(OsStr::new("RUSTC_WORKSPACE_WRAPPER"))
    {
        return Err(FinalCargoError::ExistingWorkspaceWrapper);
    }
    if env::var_os("RUSTC_WRAPPER").is_some()
        || request
            .environment
            .contains_key(OsStr::new("RUSTC_WRAPPER"))
    {
        return Err(FinalCargoError::ExistingRustcWrapper);
    }
    ensure_plain_managed_directory(&request.source_exclusion_directory, &request.target_root)?;
    ensure_plain_managed_file(&request.source_exclusion_directory, &request.plan_path)?;
    Ok(())
}

/// Failure to confine a final-build path beneath its trusted generated-output
/// boundary using only ordinary directories and regular files.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManagedPathError {
    /// Managed paths must have an unambiguous lexical spelling.
    #[error("managed path contains `.` or `..`: `{0}`")]
    NonNormalPath(PathBuf),
    /// A managed output escaped the source-exclusion boundary.
    #[error("managed path `{path}` is outside source exclusion `{exclusion}`")]
    OutsideSourceExclusion {
        /// Rejected output path.
        path: PathBuf,
        /// Trusted generated-output boundary.
        exclusion: PathBuf,
    },
    /// A directory component could not be created.
    #[error("failed to create managed directory `{path}`: {source}")]
    CreateDirectory {
        /// Directory being created.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A managed path could not be inspected without following symlinks.
    #[error("failed to inspect managed path `{path}`: {source}")]
    Inspect {
        /// Path being inspected.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Directory traversal encountered a symlink or non-directory.
    #[error("managed directory component is not a plain directory: `{0}`")]
    NonDirectory(PathBuf),
    /// A content-addressed input/output was a symlink or non-file.
    #[error("managed file is not a plain regular file: `{0}`")]
    NonFile(PathBuf),
    /// A file path had no containing directory.
    #[error("managed file has no parent directory: `{0}`")]
    FileWithoutParent(PathBuf),
}

/// Create and validate `directory` one plain component at a time beneath the
/// trusted source-exclusion boundary.
///
/// The boundary itself is the trust anchor. Every descendant is inspected via
/// `symlink_metadata`, so a pre-existing symlink cannot redirect a unit-map or
/// final-target write outside that boundary.
pub(crate) fn ensure_plain_managed_directory(
    source_exclusion: &Path,
    directory: &Path,
) -> Result<(), ManagedPathError> {
    validate_normal_absolute_path(source_exclusion)?;
    validate_normal_absolute_path(directory)?;
    let relative = directory.strip_prefix(source_exclusion).map_err(|_| {
        ManagedPathError::OutsideSourceExclusion {
            path: directory.to_path_buf(),
            exclusion: source_exclusion.to_path_buf(),
        }
    })?;

    fs::create_dir_all(source_exclusion).map_err(|source| ManagedPathError::CreateDirectory {
        path: source_exclusion.to_path_buf(),
        source,
    })?;
    require_plain_directory(source_exclusion)?;

    let mut current = source_exclusion.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(ManagedPathError::NonNormalPath(directory.to_path_buf()));
        };
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(ManagedPathError::CreateDirectory {
                    path: current,
                    source,
                });
            }
        }
        require_plain_directory(&current)?;
    }
    Ok(())
}

pub(crate) fn ensure_plain_managed_file(
    source_exclusion: &Path,
    file: &Path,
) -> Result<(), ManagedPathError> {
    validate_normal_absolute_path(file)?;
    let parent = file
        .parent()
        .ok_or_else(|| ManagedPathError::FileWithoutParent(file.to_path_buf()))?;
    ensure_plain_managed_directory(source_exclusion, parent)?;
    let metadata = fs::symlink_metadata(file).map_err(|source| ManagedPathError::Inspect {
        path: file.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(ManagedPathError::NonFile(file.to_path_buf()));
    }
    Ok(())
}

fn validate_normal_absolute_path(path: &Path) -> Result<(), ManagedPathError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(ManagedPathError::NonNormalPath(path.to_path_buf()));
    }
    Ok(())
}

fn require_plain_directory(path: &Path) -> Result<(), ManagedPathError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ManagedPathError::Inspect {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_dir() {
        return Err(ManagedPathError::NonDirectory(path.to_path_buf()));
    }
    Ok(())
}

fn split_runtime_arguments(arguments: &[OsString]) -> (&[OsString], Option<&[OsString]>) {
    match arguments
        .iter()
        .position(|argument| argument == OsStr::new("--"))
    {
        Some(index) => (&arguments[..index], Some(&arguments[index + 1..])),
        None => (arguments, None),
    }
}

fn compile_only_cargo_arguments(arguments: &[OsString]) -> Result<Vec<OsString>, FinalCargoError> {
    let (compile_arguments, runtime_arguments) = split_runtime_arguments(arguments);
    let mut compile_arguments = compile_arguments.to_vec();
    let subcommand = compile_arguments
        .first()
        .cloned()
        .ok_or(FinalCargoError::MissingCargoArguments)?;
    match subcommand.as_os_str() {
        value if value == OsStr::new("run") => {
            compile_arguments[0] = OsString::from("build");
        }
        value if value == OsStr::new("test") => {
            if !compile_arguments
                .iter()
                .any(|argument| argument == OsStr::new("--no-run"))
            {
                compile_arguments.push(OsString::from("--no-run"));
            }
        }
        value if value == OsStr::new("build") || value == OsStr::new("check") => {
            if runtime_arguments.is_some() {
                return Err(FinalCargoError::RuntimeArgumentsForCompileOnlySubcommand(
                    subcommand.clone(),
                ));
            }
        }
        _ => {
            return Err(FinalCargoError::UnsupportedCargoSubcommand(
                subcommand.clone(),
            ));
        }
    }
    Ok(compile_arguments)
}

fn validate_plan_file<P: FinalCargoPlan>(
    path: &Path,
    plan: &P,
) -> Result<(String, Vec<u8>), FinalCargoError> {
    let bytes = fs::read(path).map_err(|source| FinalCargoError::ReadPlan {
        path: path.to_path_buf(),
        source,
    })?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|source| FinalCargoError::ParsePlan {
            path: path.to_path_buf(),
            source,
        })?;
    let format = value.get("format").and_then(Value::as_str);
    if format != Some(plan.final_plan_format()) {
        return Err(FinalCargoError::PlanFormat {
            actual: format.unwrap_or_default().to_string(),
        });
    }
    let file_plan_id = value.get("plan_id").and_then(Value::as_str);
    let expected_plan_id = plan.final_plan_id().to_string();
    if file_plan_id != Some(expected_plan_id.as_str()) {
        return Err(FinalCargoError::PlanIdMismatch {
            expected: plan.final_plan_id(),
            actual: file_plan_id.unwrap_or_default().to_string(),
        });
    }
    let file_link_unit = value.get("link_unit_id").and_then(Value::as_str);
    let expected_link_unit = plan.final_link_unit().to_string();
    if file_link_unit != Some(expected_link_unit.as_str()) {
        return Err(FinalCargoError::LinkUnitMismatch {
            expected: expected_link_unit,
            actual: file_link_unit.unwrap_or_default().to_string(),
        });
    }

    let expected_name = format!("{}.{}", plan.final_plan_id(), plan.final_plan_file_suffix());
    if path.file_name() != Some(OsStr::new(&expected_name)) {
        return Err(FinalCargoError::PlanFilename {
            path: path.to_path_buf(),
            expected: expected_name,
        });
    }
    let expected_value = plan
        .final_plan_json()
        .map_err(FinalCargoError::SerializeExpectedPlan)?;
    if value != expected_value {
        return Err(FinalCargoError::PlanContentMismatch);
    }
    Ok((exact_sha256_hex(&bytes), bytes))
}

fn publish_unit_projections(
    request: &FinalCargoRequest,
    plan_id: PlanId,
    plan_bytes: &[u8],
) -> Result<BTreeMap<OsString, OsString>, FinalCargoError> {
    let directory = request.target_root.join("rsjs-final-unit-plans");
    ensure_plain_managed_directory(&request.source_exclusion_directory, &directory)?;
    let mut environment = BTreeMap::new();
    environment.insert(
        OsString::from(FINAL_UNIT_PLAN_DIRECTORY_ENV),
        directory.as_os_str().to_owned(),
    );
    let projections = rsjs_ir::final_plan::unit_projection_bytes_for_units(
        request
            .descriptor_units
            .iter()
            .map(|unit| *unit.unit.as_bytes()),
        *plan_id.as_bytes(),
        plan_bytes,
    )
    .map_err(FinalCargoError::UnitProjection)?;
    for unit in &request.descriptor_units {
        let unit_id = unit.unit;
        let bytes = &projections[unit_id.as_bytes()];
        let hash = hex_sha256(&Sha256::digest(bytes).into());
        let path = directory.join(format!(
            "{hash}.{}",
            rsjs_ir::final_plan::UNIT_PROJECTION_FILE_SUFFIX
        ));
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                ensure_plain_managed_file(&request.source_exclusion_directory, &path)?;
                let existing = fs::read(&path).map_err(|source| FinalCargoError::ReadPlan {
                    path: path.clone(),
                    source,
                })?;
                if &existing != bytes {
                    return Err(FinalCargoError::UnitProjection(format!(
                        "conflicting content-addressed unit projection `{}`",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                atomic_write_exact(&path, bytes).map_err(FinalCargoError::UnitMap)?;
                ensure_plain_managed_file(&request.source_exclusion_directory, &path)?;
            }
            Err(source) => return Err(FinalCargoError::ReadPlan { path, source }),
        }
        environment.insert(
            OsString::from(format!("{FINAL_UNIT_PLAN_ENV_PREFIX}{unit_id}")),
            OsString::from(hash),
        );
    }
    Ok(environment)
}

fn inject_plan_environment<P: FinalCargoPlan>(
    environment: &mut BTreeMap<OsString, OsString>,
    request: &FinalCargoRequest,
    plan: &P,
    plan_file_hash: &str,
    unit_map_path: &Path,
    unit_map_hash: &str,
) -> Result<(), FinalCargoError> {
    let plan_path = request
        .plan_path
        .to_str()
        .ok_or_else(|| FinalCargoError::NonUtf8PlanPath(request.plan_path.clone()))?;
    environment.insert(OsString::from(FINAL_PHASE_ENV), OsString::from("1"));
    environment.insert(
        OsString::from(FINAL_PLAN_PATH_ENV),
        OsString::from(plan_path),
    );
    environment.insert(
        OsString::from(FINAL_PLAN_ID_ENV),
        OsString::from(plan.final_plan_id().to_string()),
    );
    environment.insert(
        OsString::from(FINAL_PLAN_FILE_HASH_ENV),
        OsString::from(plan_file_hash),
    );
    environment.insert(
        OsString::from(FINAL_UNIT_MAP_PATH_ENV),
        unit_map_path.as_os_str().to_owned(),
    );
    environment.insert(
        OsString::from(FINAL_UNIT_MAP_HASH_ENV),
        OsString::from(unit_map_hash),
    );

    Ok(())
}

fn exact_sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    format!("sha256:{output}")
}

fn final_target_directory(target_root: &Path, invocation: InvocationId) -> PathBuf {
    if cfg!(windows) {
        return target_root
            .join(format!("w{FINAL_WRAPPER_POLICY_VERSION}"))
            .join(invocation.to_string());
    }
    target_root
        .join("rsjs-final")
        .join(format!("wrapper-v{FINAL_WRAPPER_POLICY_VERSION}"))
        .join(invocation.to_string())
}

/// Publish every materialized path for one selected root into immutable,
/// content-addressed storage while the reusable Cargo target is still leased.
pub(crate) fn snapshot_final_artifact(
    source_exclusion: &Path,
    snapshot_root: &Path,
    target_directory: &Path,
    artifact: &CargoCompilerArtifact,
) -> Result<CargoCompilerArtifact, FinalCargoError> {
    ensure_plain_managed_directory(source_exclusion, snapshot_root)?;

    let mut snapshots = BTreeMap::new();
    if let Some(executable) = &artifact.executable {
        ensure_plain_managed_file(target_directory, executable)?;
        snapshots.insert(
            executable.clone(),
            snapshot_final_file(
                source_exclusion,
                snapshot_root,
                executable,
                Some(target_directory),
            )?,
        );
    }
    for source in &artifact.filenames {
        if artifact.executable.as_ref() == Some(source) {
            continue;
        }
        ensure_plain_managed_file(target_directory, source)?;
        snapshots.insert(
            source.clone(),
            snapshot_final_file(source_exclusion, snapshot_root, source, None)?,
        );
    }

    let mut snapshot = artifact.clone();
    snapshot.filenames = artifact
        .filenames
        .iter()
        .map(|source| {
            snapshots
                .get(source)
                .cloned()
                .expect("every final filename was snapshotted")
        })
        .collect();
    snapshot.executable = artifact.executable.as_ref().map(|source| {
        snapshots
            .get(source)
            .cloned()
            .expect("the final executable was snapshotted")
    });
    Ok(snapshot)
}

fn snapshot_final_file(
    source_exclusion: &Path,
    snapshot_root: &Path,
    source: &Path,
    launch_target_directory: Option<&Path>,
) -> Result<PathBuf, FinalCargoError> {
    let metadata = fs::symlink_metadata(source).map_err(|source_error| {
        FinalCargoError::SnapshotArtifactIo {
            path: source.to_path_buf(),
            source: source_error,
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(FinalCargoError::SnapshotArtifactNotFile(
            source.to_path_buf(),
        ));
    }
    let file_name = source
        .file_name()
        .ok_or_else(|| FinalCargoError::SnapshotArtifactWithoutFilename(source.to_path_buf()))?;
    let permissions = metadata.permissions();
    let permission_identity = snapshot_permission_identity(&permissions);

    let nonce = FINAL_SNAPSHOT_NONCE.fetch_add(1, Ordering::Relaxed);
    let temporary = snapshot_root.join(format!(".snapshot.{}.{}.tmp", std::process::id(), nonce));
    let mut input =
        File::open(source).map_err(|source_error| FinalCargoError::SnapshotArtifactIo {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source_error| FinalCargoError::SnapshotArtifactIo {
            path: temporary.clone(),
            source: source_error,
        })?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let copy_result = (|| -> io::Result<()> {
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
            output.write_all(&buffer[..read])?;
        }
        output.sync_all()?;
        drop(output);
        fs::set_permissions(&temporary, permissions)?;
        Ok(())
    })();
    if let Err(source_error) = copy_result {
        let _ = fs::remove_file(&temporary);
        return Err(FinalCargoError::SnapshotArtifactIo {
            path: temporary,
            source: source_error,
        });
    }

    if let Some(target_directory) = launch_target_directory {
        if let Err(error) = validate_executable_snapshot_contract(&temporary, target_directory) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }

    let digest: [u8; 32] = hash.finalize().into();
    let identity = format!("{}-{permission_identity}", hex_sha256(&digest));
    let destination_directory = snapshot_root.join(identity);
    ensure_plain_managed_directory(source_exclusion, &destination_directory)?;
    let destination = destination_directory.join(file_name);
    match fs::hard_link(&temporary, &destination) {
        Ok(()) => {
            fs::remove_file(&temporary).map_err(|source_error| {
                FinalCargoError::SnapshotArtifactIo {
                    path: temporary,
                    source: source_error,
                }
            })?;
        }
        Err(source_error) if source_error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            let (existing_digest, existing_permissions) = snapshot_file_identity(&destination)?;
            if existing_digest != digest || existing_permissions != permission_identity {
                return Err(FinalCargoError::ConflictingSnapshotArtifact(destination));
            }
        }
        Err(source_error) => {
            let _ = fs::remove_file(&temporary);
            return Err(FinalCargoError::SnapshotArtifactIo {
                path: destination,
                source: source_error,
            });
        }
    }
    ensure_plain_managed_file(source_exclusion, &destination)?;
    Ok(destination)
}

/// Publish the exact wrapper bytes into the generated-output CAS before Cargo
/// is allowed to reopen an executable path.
///
/// This uses the same lstat, one-open-file copy, relocation/dependency check,
/// atomic publication, and existing-entry verification contract as final
/// executable snapshots. The returned path is content addressed and can
/// therefore be bound into the linked invocation identity without trusting the
/// caller's mutable pathname.
pub fn snapshot_wrapper_executable(
    source_exclusion: &Path,
    snapshot_root: &Path,
    source: &Path,
) -> Result<PathBuf, FinalCargoError> {
    ensure_plain_managed_directory(source_exclusion, snapshot_root)?;
    // A warm wrapper snapshot is already immutable. Verify fresh source and
    // destination bytes/mode before reusing it, rather than writing and
    // syncing another identical temporary executable on every dev cycle.
    let (digest, permissions) = snapshot_file_identity(source)?;
    let file_name = source
        .file_name()
        .ok_or_else(|| FinalCargoError::SnapshotArtifactWithoutFilename(source.to_path_buf()))?;
    let directory = snapshot_root.join(format!("{}-{permissions}", hex_sha256(&digest)));
    ensure_plain_managed_directory(source_exclusion, &directory)?;
    let destination = directory.join(file_name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            ensure_plain_managed_file(source_exclusion, &destination)?;
            let (existing_digest, existing_permissions) = snapshot_file_identity(&destination)?;
            if existing_digest != digest || existing_permissions != permissions {
                return Err(FinalCargoError::ConflictingSnapshotArtifact(destination));
            }
            validate_executable_snapshot_contract(&destination, source_exclusion)?;
            return Ok(destination);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(FinalCargoError::SnapshotArtifactIo {
                path: destination,
                source,
            });
        }
    }
    // A miss retains the one-open-file copy and atomic publication contract.
    // Its own digest is authoritative if the source changed after this probe.
    snapshot_final_file(
        source_exclusion,
        snapshot_root,
        source,
        Some(source_exclusion),
    )
}

#[derive(Debug, Default)]
struct ExecutableSnapshotDependencies {
    libraries: BTreeSet<Vec<u8>>,
    loader_paths: BTreeSet<Vec<u8>>,
    interpreters: BTreeSet<Vec<u8>>,
}

fn validate_executable_snapshot_contract(
    executable: &Path,
    target_directory: &Path,
) -> Result<(), FinalCargoError> {
    let bytes = fs::read(executable).map_err(|source| FinalCargoError::SnapshotArtifactIo {
        path: executable.to_path_buf(),
        source,
    })?;
    let file = object::File::parse(bytes.as_slice()).map_err(|source| {
        FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: format!("the executable format could not be parsed: {source}"),
        }
    })?;
    let format = file.format();
    validate_snapshot_host_identity(executable, format, file.architecture())?;

    let dependencies = executable_snapshot_dependencies(&file).map_err(|source| {
        FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: format!("the runtime dependency table could not be validated: {source}"),
        }
    })?;
    if let Some(loader_path) = dependencies.loader_paths.into_iter().next() {
        let loader_path = dependency_name(&loader_path).ok_or_else(|| {
            FinalCargoError::UnsupportedSnapshotExecutable {
                path: executable.to_path_buf(),
                reason: "a loader search path is not valid UTF-8".to_string(),
            }
        })?;
        if loader_path.is_empty() || loader_path_is_relative(loader_path) {
            return Err(FinalCargoError::UnsupportedSnapshotDependency {
                path: executable.to_path_buf(),
                dependency: loader_path.to_string(),
                reason:
                    "it uses a relative loader search path whose meaning changes after relocation",
            });
        }
        return Err(FinalCargoError::UnsupportedSnapshotDependency {
            path: executable.to_path_buf(),
            dependency: loader_path.to_string(),
            reason: "custom loader search paths are outside the system-only snapshot contract",
        });
    }

    for interpreter in dependencies.interpreters {
        let interpreter = dependency_name(&interpreter).ok_or_else(|| {
            FinalCargoError::UnsupportedSnapshotExecutable {
                path: executable.to_path_buf(),
                reason: "the program interpreter or dynamic loader is not valid UTF-8".to_string(),
            }
        })?;
        validate_snapshot_interpreter(executable, target_directory, interpreter)?;
    }

    for dependency in dependencies.libraries {
        let dependency = dependency_name(&dependency).ok_or_else(|| {
            FinalCargoError::UnsupportedSnapshotExecutable {
                path: executable.to_path_buf(),
                reason: "a dynamic-library dependency name is not valid UTF-8".to_string(),
            }
        })?;
        validate_snapshot_dependency(format, executable, target_directory, dependency)?;
    }
    Ok(())
}

fn validate_snapshot_host_identity(
    executable: &Path,
    format: BinaryFormat,
    architecture: Architecture,
) -> Result<(), FinalCargoError> {
    if format != host_executable_format() {
        return Err(FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: format!(
                "the {format:?} executable format is not the current host's supported launch format ({:?})",
                host_executable_format()
            ),
        });
    }
    let Some(host_architecture) = host_architecture() else {
        return Err(FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: format!(
                "the current host architecture `{}` has no immutable launch-snapshot contract",
                std::env::consts::ARCH
            ),
        });
    };
    if architecture != host_architecture {
        return Err(FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: format!(
                "the {architecture:?} executable architecture does not match the current host architecture {host_architecture:?}"
            ),
        });
    }
    Ok(())
}

fn executable_snapshot_dependencies(
    file: &object::File<'_>,
) -> Result<ExecutableSnapshotDependencies, String> {
    match file {
        object::File::Elf32(file) => elf_snapshot_dependencies(file),
        object::File::Elf64(file) => elf_snapshot_dependencies(file),
        object::File::MachO32(file) => macho_snapshot_dependencies(file),
        object::File::MachO64(file) => macho_snapshot_dependencies(file),
        object::File::Pe32(file) => pe_snapshot_dependencies(file),
        object::File::Pe64(file) => pe_snapshot_dependencies(file),
        _ => {
            let mut dependencies = ExecutableSnapshotDependencies::default();
            for import in file.imports().map_err(|error| error.to_string())? {
                if import.library().is_empty() {
                    return Err(
                        "an imported symbol has no authoritative library identity".to_string()
                    );
                }
                dependencies.libraries.insert(import.library().to_vec());
            }
            Ok(dependencies)
        }
    }
}

fn elf_snapshot_dependencies<'data, Elf>(
    file: &object::read::elf::ElfFile<'data, Elf>,
) -> Result<ExecutableSnapshotDependencies, String>
where
    Elf: object::read::elf::FileHeader,
{
    use object::read::elf::{Dyn as _, ProgramHeader as _};

    let mut dependencies = ExecutableSnapshotDependencies::default();
    let endian = file.endian();
    let data = file.data();
    let mut has_dynamic_segment = false;
    for segment in file.elf_program_headers() {
        has_dynamic_segment |= segment.p_type(endian) == object::elf::PT_DYNAMIC;
        if let Some(interpreter) = segment
            .interpreter(endian, data)
            .map_err(|error| error.to_string())?
        {
            dependencies.interpreters.insert(interpreter.to_vec());
        }
    }
    let sections = file.elf_section_table();
    let Some((dynamic, strings_index)) = sections
        .dynamic(endian, data)
        .map_err(|error| error.to_string())?
    else {
        if has_dynamic_segment {
            return Err(
                "ELF has a PT_DYNAMIC segment but no authoritative SHT_DYNAMIC table".to_string(),
            );
        }
        return Ok(dependencies);
    };
    let strings = sections
        .strings(endian, data, strings_index)
        .map_err(|error| error.to_string())?;
    for entry in dynamic {
        match entry.tag32(endian) {
            Some(object::elf::DT_NEEDED) => {
                dependencies.libraries.insert(
                    entry
                        .string(endian, strings)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            Some(object::elf::DT_RPATH | object::elf::DT_RUNPATH) => {
                dependencies.loader_paths.insert(
                    entry
                        .string(endian, strings)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }
    Ok(dependencies)
}

/// Read library descriptors, rather than symbol imports, so ordinal-only and
/// delay-loaded DLLs receive the same dependency validation as named imports.
fn pe_snapshot_dependencies<'data, Pe>(
    file: &object::read::pe::PeFile<'data, Pe>,
) -> Result<ExecutableSnapshotDependencies, String>
where
    Pe: object::read::pe::ImageNtHeaders,
{
    use object::LittleEndian as LE;

    let mut dependencies = ExecutableSnapshotDependencies::default();
    if let Some(table) = file.import_table().map_err(|error| error.to_string())? {
        let mut descriptors = table.descriptors().map_err(|error| error.to_string())?;
        while let Some(descriptor) = descriptors.next().map_err(|error| error.to_string())? {
            let name = table
                .name(descriptor.name.get(LE))
                .map_err(|error| error.to_string())?;
            dependencies.libraries.insert(name.to_vec());
        }
    }
    let sections = file.section_table();
    if let Some(table) = file
        .data_directories()
        .delay_load_import_table(file.data(), &sections)
        .map_err(|error| error.to_string())?
    {
        let mut descriptors = table.descriptors().map_err(|error| error.to_string())?;
        while let Some(descriptor) = descriptors.next().map_err(|error| error.to_string())? {
            // PE/COFF defines bit zero as RvaBased; all other bits are reserved.
            // Do not treat legacy virtual addresses as RVAs. object 0.37's
            // IMAGE_DELAYLOAD_RVA_BASED constant incorrectly uses bit 31.
            if descriptor.attributes.get(LE) != 1 {
                return Err("PE delay-load imports require RVA-based descriptors with no reserved attributes".to_string());
            }
            let name = table
                .name(descriptor.dll_name_rva.get(LE))
                .map_err(|error| error.to_string())?;
            dependencies.libraries.insert(name.to_vec());
        }
    }
    Ok(dependencies)
}

fn macho_snapshot_dependencies<'data, Mach>(
    file: &object::read::macho::MachOFile<'data, Mach>,
) -> Result<ExecutableSnapshotDependencies, String>
where
    Mach: object::read::macho::MachHeader,
{
    let mut dependencies = ExecutableSnapshotDependencies::default();
    let endian = file.endian();
    let mut commands = file
        .macho_load_commands()
        .map_err(|error| error.to_string())?;
    while let Some(command) = commands.next().map_err(|error| error.to_string())? {
        match command.variant().map_err(|error| error.to_string())? {
            object::read::macho::LoadCommandVariant::Dylib(dylib) => {
                dependencies.libraries.insert(
                    command
                        .string(endian, dylib.dylib.name)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            object::read::macho::LoadCommandVariant::Rpath(rpath) => {
                dependencies.loader_paths.insert(
                    command
                        .string(endian, rpath.path)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            object::read::macho::LoadCommandVariant::LoadDylinker(dylinker) => {
                dependencies.interpreters.insert(
                    command
                        .string(endian, dylinker.name)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            object::read::macho::LoadCommandVariant::DyldEnvironment(environment) => {
                dependencies.loader_paths.insert(
                    command
                        .string(endian, environment.name)
                        .map_err(|error| error.to_string())?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }
    Ok(dependencies)
}

fn host_executable_format() -> BinaryFormat {
    #[cfg(target_os = "windows")]
    {
        BinaryFormat::Pe
    }
    #[cfg(target_os = "macos")]
    {
        BinaryFormat::MachO
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        BinaryFormat::Elf
    }
}

fn host_architecture() -> Option<Architecture> {
    match std::env::consts::ARCH {
        "aarch64" => Some(Architecture::Aarch64),
        "arm" => Some(Architecture::Arm),
        "loongarch64" => Some(Architecture::LoongArch64),
        "mips" => Some(Architecture::Mips),
        "mips64" => Some(Architecture::Mips64),
        "powerpc" => Some(Architecture::PowerPc),
        "powerpc64" => Some(Architecture::PowerPc64),
        "riscv32" => Some(Architecture::Riscv32),
        "riscv64" => Some(Architecture::Riscv64),
        "s390x" => Some(Architecture::S390x),
        "sparc64" => Some(Architecture::Sparc64),
        "x86" => Some(Architecture::I386),
        "x86_64" => Some(Architecture::X86_64),
        _ => None,
    }
}

fn dependency_name(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

fn validate_snapshot_dependency(
    format: BinaryFormat,
    executable: &Path,
    target_directory: &Path,
    dependency: &str,
) -> Result<(), FinalCargoError> {
    if dependency.is_empty() {
        return Err(FinalCargoError::UnsupportedSnapshotExecutable {
            path: executable.to_path_buf(),
            reason: "an imported dynamic library has an empty identity".to_string(),
        });
    }
    if rust_dynamic_dependency(dependency) {
        return Err(FinalCargoError::UnsupportedSnapshotDependency {
            path: executable.to_path_buf(),
            dependency: dependency.to_string(),
            reason: "Rust dynamic libraries are not copied into immutable launch snapshots",
        });
    }
    if loader_path_is_relative(dependency) {
        return Err(FinalCargoError::UnsupportedSnapshotDependency {
            path: executable.to_path_buf(),
            dependency: dependency.to_string(),
            reason: "it is relative to the original executable or loader search path",
        });
    }

    let dependency_path = Path::new(dependency);
    if dependency_path.is_absolute() {
        if dependency_path.starts_with(target_directory) {
            return Err(FinalCargoError::UnsupportedSnapshotDependency {
                path: executable.to_path_buf(),
                dependency: dependency.to_string(),
                reason: "it points into the mutable final Cargo target",
            });
        }
        if platform_system_dependency(format, dependency) {
            return Ok(());
        }
        return Err(FinalCargoError::UnsupportedSnapshotDependency {
            path: executable.to_path_buf(),
            dependency: dependency.to_string(),
            reason: "non-system absolute dynamic libraries are outside the snapshot contract",
        });
    }

    if platform_system_dependency(format, dependency) {
        if format == BinaryFormat::Pe {
            if let Some(directory) = executable.parent() {
                let adjacent = directory.join(dependency);
                match fs::symlink_metadata(&adjacent) {
                    Ok(_) => {
                        return Err(FinalCargoError::UnsupportedSnapshotDependency {
                            path: executable.to_path_buf(),
                            dependency: dependency.to_string(),
                            reason: "an app-local DLL beside the executable is not copied into the relocated snapshot",
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(FinalCargoError::SnapshotArtifactIo {
                            path: adjacent,
                            source,
                        });
                    }
                }
            }
        }
        return Ok(());
    }
    Err(FinalCargoError::UnsupportedSnapshotDependency {
        path: executable.to_path_buf(),
        dependency: dependency.to_string(),
        reason: "non-system dynamic libraries are not copied into immutable launch snapshots",
    })
}

fn validate_snapshot_interpreter(
    executable: &Path,
    target_directory: &Path,
    interpreter: &str,
) -> Result<(), FinalCargoError> {
    let interpreter_path = Path::new(interpreter);
    let reason = if !interpreter_path.is_absolute() {
        Some("the program interpreter or dynamic-loader path is relative")
    } else if interpreter_path.starts_with(target_directory) {
        Some("the program interpreter or dynamic loader points into the mutable final Cargo target")
    } else if !platform_system_interpreter(interpreter) {
        Some("the program interpreter or dynamic loader is outside the host system-loader contract")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(FinalCargoError::UnsupportedSnapshotDependency {
            path: executable.to_path_buf(),
            dependency: interpreter.to_string(),
            reason,
        });
    }
    Ok(())
}

fn platform_system_interpreter(interpreter: &str) -> bool {
    interpreter.starts_with("/lib/")
        || interpreter.starts_with("/lib64/")
        || interpreter.starts_with("/usr/lib/")
        || interpreter.starts_with("/usr/lib64/")
        || matches!(interpreter, "/system/bin/linker" | "/system/bin/linker64")
}

fn loader_path_is_relative(path: &str) -> bool {
    path.starts_with('@')
        || path.contains("$ORIGIN")
        || path.contains("${ORIGIN}")
        || path.split(':').any(|entry| {
            entry.is_empty()
                || entry.starts_with('@')
                || entry.contains("$ORIGIN")
                || entry.contains("${ORIGIN}")
                || (entry.contains('/') && !Path::new(entry).is_absolute())
        })
}

fn rust_dynamic_dependency(dependency: &str) -> bool {
    let name = dependency
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(dependency)
        .to_ascii_lowercase();
    [
        "libstd-",
        "std-",
        "libtest-",
        "test-",
        "libpanic_",
        "panic_",
        "librustc_",
        "rustc_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        && [".so", ".dylib", ".dll"]
            .iter()
            .any(|suffix| name.contains(suffix))
}

fn platform_system_dependency(format: BinaryFormat, dependency: &str) -> bool {
    match format {
        BinaryFormat::MachO => {
            dependency.starts_with("/usr/lib/")
                || dependency.starts_with("/System/Library/")
                || dependency.starts_with("/System/DriverKit/")
        }
        BinaryFormat::Elf => matches!(
            dependency,
            "ld-linux-x86-64.so.2"
                | "libc.so"
                | "libc.so.6"
                | "libdl.so"
                | "libdl.so.2"
                | "libgcc_s.so.1"
                | "libm.so"
                | "libm.so.6"
                | "libpthread.so"
                | "libpthread.so.0"
                | "libresolv.so"
                | "libresolv.so.2"
                | "librt.so"
                | "librt.so.1"
                | "libthr.so.3"
                | "libutil.so"
                | "libutil.so.1"
                | "libutil.so.9"
        ),
        BinaryFormat::Pe => {
            let dependency = dependency.to_ascii_lowercase();
            // API-set contracts and system DLLs are basenames. A prefix must
            // never admit an app-relative path or an alternate data stream.
            if dependency.contains(['/', '\\', ':']) || !dependency.ends_with(".dll") {
                return false;
            }
            dependency.starts_with("api-ms-win-")
                || dependency.starts_with("ext-ms-win-")
                || matches!(
                    dependency.as_str(),
                    "advapi32.dll"
                        | "bcrypt.dll"
                        | "bcryptprimitives.dll"
                        | "crypt32.dll"
                        | "gdi32.dll"
                        | "iphlpapi.dll"
                        | "kernel32.dll"
                        | "msvcrt.dll"
                        | "ncrypt.dll"
                        | "ntdll.dll"
                        | "ole32.dll"
                        | "secur32.dll"
                        | "shell32.dll"
                        | "ucrtbase.dll"
                        | "user32.dll"
                        | "userenv.dll"
                        | "vcruntime140.dll"
                        | "vcruntime140_1.dll"
                        | "ws2_32.dll"
                )
        }
        _ => false,
    }
}

fn snapshot_file_identity(path: &Path) -> Result<([u8; 32], String), FinalCargoError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| FinalCargoError::SnapshotArtifactIo {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.file_type().is_file() {
        return Err(FinalCargoError::SnapshotArtifactNotFile(path.to_path_buf()));
    }
    let mut file = File::open(path).map_err(|source| FinalCargoError::SnapshotArtifactIo {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read =
            file.read(&mut buffer)
                .map_err(|source| FinalCargoError::SnapshotArtifactIo {
                    path: path.to_path_buf(),
                    source,
                })?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok((
        hash.finalize().into(),
        snapshot_permission_identity(&metadata.permissions()),
    ))
}

fn hex_sha256(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(unix)]
fn snapshot_permission_identity(permissions: &fs::Permissions) -> String {
    use std::os::unix::fs::PermissionsExt as _;
    format!("mode-{:o}", permissions.mode())
}

#[cfg(not(unix))]
fn snapshot_permission_identity(permissions: &fs::Permissions) -> String {
    format!("readonly-{}", u8::from(permissions.readonly()))
}

fn forward_rendered_cargo_diagnostic(line: &str) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
        return;
    }
    if let Some(rendered) = value
        .get("message")
        .and_then(|message| message.get("rendered"))
        .and_then(Value::as_str)
    {
        eprint!("{rendered}");
    }
}

/// Final-compilation validation or Cargo failure.
#[derive(Debug, Error)]
pub enum FinalCargoError {
    /// Managed outputs must remain beneath the generated-output boundary and
    /// may not traverse symlinks or special filesystem entries.
    #[error(transparent)]
    ManagedPath(#[from] ManagedPathError),
    /// Stable final targets are serialized through the shared target lease.
    #[error("failed to acquire the final Cargo target lease: {0}")]
    TargetLease(DescriptorCargoRunError),
    /// A selected final root path had no filename for snapshot publication.
    #[error("selected final Cargo artifact has no filename: `{0}`")]
    SnapshotArtifactWithoutFilename(PathBuf),
    /// Only plain regular files can become immutable launch artifacts.
    #[error("selected final Cargo artifact is not a plain regular file: `{0}`")]
    SnapshotArtifactNotFile(PathBuf),
    /// Immutable launch artifact publication or validation failed.
    #[error("failed to publish final Cargo artifact snapshot `{path}`: {source}")]
    SnapshotArtifactIo {
        /// Source, temporary, or destination path involved.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A content-addressed snapshot path contained different bytes or mode.
    #[error("content-addressed final Cargo artifact conflicts with `{0}`")]
    ConflictingSnapshotArtifact(PathBuf),
    /// Executable parsing or host-format validation could not prove that a
    /// relocated snapshot remains directly launchable.
    #[error("final executable `{path}` cannot become an immutable launch snapshot: {reason}")]
    UnsupportedSnapshotExecutable {
        /// Exact temporary snapshot bytes that were inspected.
        path: PathBuf,
        /// Focused fail-closed reason.
        reason: String,
    },
    /// A runtime library or loader path would retain mutable or
    /// relocation-sensitive state outside the selected snapshot.
    #[error(
        "final executable `{path}` cannot become an immutable launch snapshot because dependency `{dependency}` is unsupported: {reason}"
    )]
    UnsupportedSnapshotDependency {
        /// Exact temporary snapshot bytes that were inspected.
        path: PathBuf,
        /// Imported library or loader-search path.
        dependency: String,
        /// Focused contract violation.
        reason: &'static str,
    },
    /// Wrapper path must be invocation-stable.
    #[error("final Cargo wrapper path must be absolute: `{0}`")]
    RelativeWrapper(PathBuf),
    /// Cargo cwd must be explicit.
    #[error("final Cargo working directory must be absolute: `{0}`")]
    RelativeWorkingDirectory(PathBuf),
    /// Cross-phase package-source exclusion must be exact.
    #[error("final Cargo source exclusion directory must be absolute: `{0}`")]
    RelativeSourceExclusionDirectory(PathBuf),
    /// Target root must be explicit.
    #[error("final Cargo target root must be absolute: `{0}`")]
    RelativeTargetRoot(PathBuf),
    /// Plan path must be exact.
    #[error("final RSJS plan path must be absolute: `{0}`")]
    RelativePlanPath(PathBuf),
    /// Cargo invocation is empty.
    #[error("final Cargo invocation has no arguments")]
    MissingCargoArguments,
    /// Only known compile-only rewrites are accepted by the final runner.
    #[error("final Cargo subcommand {0:?} is unsupported; expected build, check, run, or test")]
    UnsupportedCargoSubcommand(OsString),
    /// Runtime suffixes have no meaning for already compile-only subcommands.
    #[error("final Cargo subcommand {0:?} may not contain runtime arguments after `--`")]
    RuntimeArgumentsForCompileOnlySubcommand(OsString),
    /// Runner-owned Cargo argument was supplied.
    #[error("final Cargo argument `{0}` is reserved")]
    ReservedCargoArgument(String),
    /// Runner-owned environment was supplied.
    #[error("final Cargo environment `{0}` is reserved")]
    ReservedEnvironment(String),
    /// Proc-macro filesystem fallbacks are not part of exact build identity.
    #[error("final Cargo environment `{0}` is unsupported by the exact linked pipeline")]
    UnsupportedEnvironment(&'static str),
    /// Existing wrappers are not represented by the current identity model.
    #[error("RUSTC_WORKSPACE_WRAPPER is set; unset both Cargo rustc wrapper variables before invoking cargo-proa")]
    ExistingWorkspaceWrapper,
    /// An inherited wrapper is outside final-target/cache identity.
    #[error(
        "RUSTC_WRAPPER is set; unset it before invoking cargo-proa so final compilation cannot diverge from descriptor cache identity"
    )]
    ExistingRustcWrapper,
    /// Plan bytes could not be read.
    #[error("failed to read final RSJS plan `{path}`: {source}")]
    ReadPlan {
        /// Exact path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Plan JSON is invalid.
    #[error("failed to parse final RSJS plan `{path}`: {source}")]
    ParsePlan {
        /// Exact path.
        path: PathBuf,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Wrong external plan format.
    #[error("unsupported final RSJS plan format `{actual}`")]
    PlanFormat {
        /// Rejected format.
        actual: String,
    },
    /// File names another PlanId.
    #[error("final plan ID mismatch: expected {expected}, found `{actual}`")]
    PlanIdMismatch {
        /// Expected ID.
        expected: PlanId,
        /// File ID.
        actual: String,
    },
    /// File names another root graph.
    #[error("final link-unit ID mismatch: expected {expected}, found `{actual}`")]
    LinkUnitMismatch {
        /// Expected ID.
        expected: String,
        /// File ID.
        actual: String,
    },
    /// Content-addressed filename disagrees with PlanId.
    #[error("final plan `{path}` must be named `{expected}`")]
    PlanFilename {
        /// Actual path.
        path: PathBuf,
        /// Required filename.
        expected: String,
    },
    /// The exact file and in-memory plan disagree despite sharing their
    /// top-level identifiers.
    #[error("final RSJS plan file does not match the complete in-memory linked plan")]
    PlanContentMismatch,
    /// The in-memory plan could not be deterministically serialized for exact
    /// comparison.
    #[error("failed to serialize the expected final RSJS plan: {0}")]
    SerializeExpectedPlan(String),
    /// A consumed unit selection was invalid or its immutable publication conflicted.
    #[error("failed to prepare final RSJS unit projection: {0}")]
    UnitProjection(String),
    /// Descriptor-to-final identity map construction failed.
    #[error(transparent)]
    UnitMap(FinalUnitMapError),
    /// Plan path cannot enter rustc's environment.
    #[error("final plan path is not UTF-8: `{0}`")]
    NonUtf8PlanPath(PathBuf),
    /// Cargo spawn failed.
    #[error("failed to run final Cargo `{program}`: {source}")]
    SpawnCargo {
        /// Cargo program.
        program: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Cargo stdout pipe was unavailable.
    #[error("final Cargo process has no piped stdout")]
    MissingCargoStdout,
    /// Cargo stdout failed.
    #[error("failed to read final Cargo stdout: {0}")]
    ReadCargoStdout(io::Error),
    /// Cargo's compile-only machine-readable protocol must remain UTF-8.
    #[error("final Cargo protocol line {line} is not UTF-8: {source}")]
    NonUtf8CargoProtocol {
        /// One-based Cargo protocol line.
        line: usize,
        /// UTF-8 validation error.
        #[source]
        source: std::str::Utf8Error,
    },
    /// Cargo wait failed.
    #[error("failed to wait for final Cargo: {0}")]
    WaitForCargo(io::Error),
    /// Cargo compilation failed.
    #[error("final Cargo stopped after the compiler diagnostic above ({0})")]
    CargoFailed(ExitStatus),
    /// Cargo JSON protocol failed.
    #[error(transparent)]
    CargoProtocol(CargoMessageError),
    /// Successful Cargo invocation emitted no artifacts.
    #[error("final Cargo compilation emitted no artifacts")]
    NoCargoArtifacts,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsjs_ir::abi::{InvocationId, LinkUnitId};
    use rsjs_ir::build::LinkPlanMaterial;

    fn plan() -> TransportLinkPlan {
        let material = LinkPlanMaterial {
            link_unit: LinkUnitId::derive("final-link-unit"),
            component_descriptors: Vec::new(),
            helper_descriptors: Vec::new(),
            payload: SemanticHash::derive("empty-final-payload"),
        };
        TransportLinkPlan {
            link_unit: material.link_unit,
            plan_id: material.plan_id(),
            material,
            components: Vec::new(),
            helpers: Vec::new(),
            shared_state_definitions: Vec::new(),
            shared_state_acquisitions: Vec::new(),
        }
    }

    #[test]
    fn plan_environment_preserves_inherited_rustflags_byte_for_byte() {
        let plan = plan();
        let request = FinalCargoRequest {
            invocation: InvocationId::derive("fixture-final-cargo-request"),
            cargo: OsString::from("cargo"),
            wrapper: PathBuf::from("/fixture/cargo-proa"),
            working_directory: PathBuf::from("/fixture"),
            source_exclusion_directory: PathBuf::from("/fixture/target"),
            target_root: PathBuf::from("/fixture/target"),
            plan_path: PathBuf::from("/fixture/plan.json"),
            cargo_args: vec![OsString::from("build")],
            environment: BTreeMap::new(),
            root_unit_fingerprint: SemanticHash::derive("fixture"),
            descriptor_units: Vec::new(),
        };
        let plain = OsString::from("--cfg custom_cfg -C opt-level=1");
        let encoded = OsString::from("-Copt-level=2\u{1f}-Ctarget-cpu=native");
        let mut environment = BTreeMap::from([
            (OsString::from("RUSTFLAGS"), plain.clone()),
            (OsString::from("CARGO_ENCODED_RUSTFLAGS"), encoded.clone()),
        ]);

        inject_plan_environment(
            &mut environment,
            &request,
            &plan,
            &format!("sha256:{}", "1".repeat(64)),
            Path::new("/fixture/unit-map.json"),
            &"2".repeat(64),
        )
        .unwrap();

        assert_eq!(environment.get(OsStr::new("RUSTFLAGS")), Some(&plain));
        assert_eq!(
            environment.get(OsStr::new("CARGO_ENCODED_RUSTFLAGS")),
            Some(&encoded)
        );
    }

    #[test]
    fn final_target_is_stable_within_one_wrapper_policy() {
        let root = Path::new("/fixture/final-target-root");
        let invocation = InvocationId::derive("stable-final-target");
        let expected = if cfg!(windows) {
            root.join(format!("w{FINAL_WRAPPER_POLICY_VERSION}"))
                .join(invocation.to_string())
        } else {
            root.join("rsjs-final")
                .join(format!("wrapper-v{FINAL_WRAPPER_POLICY_VERSION}"))
                .join(invocation.to_string())
        };
        assert_eq!(final_target_directory(root, invocation), expected);
    }

    #[test]
    fn snapshot_final_artifact_preserves_immutable_generations() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_exclusion = temp.path().join("generated");
        let target_directory = source_exclusion.join("reusable-target");
        let snapshot_root = source_exclusion.join("snapshots");
        fs::create_dir_all(&target_directory).unwrap();

        let source = target_directory.join("fixture-artifact");
        fs::write(&source, b"artifact-a\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&source, fs::Permissions::from_mode(0o751)).unwrap();
        }

        let artifact = CargoCompilerArtifact {
            package_id: "path+file:///fixture#snapshot_fixture@0.0.0".to_string(),
            manifest_path: temp.path().join("Cargo.toml"),
            target: CargoArtifactTarget {
                name: "snapshot_fixture".to_string(),
                kind: BTreeSet::from(["rlib".to_string()]),
                crate_types: BTreeSet::from(["rlib".to_string()]),
                src_path: temp.path().join("src/main.rs"),
                edition: "2021".to_string(),
            },
            profile: serde_json::json!({"opt_level":"0","test":false}),
            features: BTreeSet::new(),
            filenames: BTreeSet::from([source.clone()]),
            executable: None,
            fresh: false,
        };

        let snapshot_a = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap();
        let path_a = snapshot_a.filenames.iter().next().unwrap();
        assert_eq!(fs::read(path_a).unwrap(), b"artifact-a\n");

        fs::write(&source, b"artifact-b\n").unwrap();
        let snapshot_b = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap();
        let path_b = snapshot_b.filenames.iter().next().unwrap();
        assert_ne!(path_a, path_b);
        assert_eq!(fs::read(path_a).unwrap(), b"artifact-a\n");
        assert_eq!(fs::read(path_b).unwrap(), b"artifact-b\n");

        let snapshot_b_again = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap();
        assert_eq!(snapshot_b_again.filenames, snapshot_b.filenames);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(path_a).unwrap().permissions().mode() & 0o777,
                0o751
            );
            assert_eq!(
                fs::metadata(path_b).unwrap().permissions().mode() & 0o777,
                0o751
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_pe_snapshot_of_current_executable_can_launch() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_exclusion = temp.path().join("generated");
        let target_directory = source_exclusion.join("target");
        let snapshot_root = source_exclusion.join("snapshots");
        fs::create_dir_all(&target_directory).unwrap();
        let executable = target_directory.join("fixture.exe");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let artifact = snapshot_fixture_artifact(temp.path(), executable.clone());
        let snapshot = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap();
        let published = snapshot.executable.unwrap();
        assert_ne!(published, executable);
        let output = Command::new(published).arg("--list").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains("windows_pe_snapshot_of_current_executable_can_launch"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn ordinary_host_executable_can_be_snapshotted() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_exclusion = temp.path().join("generated");
        let target_directory = source_exclusion.join("reusable-target");
        let snapshot_root = source_exclusion.join("snapshots");
        fs::create_dir_all(&target_directory).unwrap();
        let executable = compile_snapshot_fixture(&target_directory, false);
        let artifact = snapshot_fixture_artifact(temp.path(), executable.clone());

        let snapshot = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap();
        let published = snapshot.executable.unwrap();
        assert_ne!(published, executable);
        assert!(published.is_file());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn dynamic_rust_executable_is_rejected_before_snapshot_publication() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_exclusion = temp.path().join("generated");
        let target_directory = source_exclusion.join("reusable-target");
        let snapshot_root = source_exclusion.join("snapshots");
        fs::create_dir_all(&target_directory).unwrap();
        let executable = compile_snapshot_fixture(&target_directory, true);
        let artifact = snapshot_fixture_artifact(temp.path(), executable);

        let error = snapshot_final_artifact(
            &source_exclusion,
            &snapshot_root,
            &target_directory,
            &artifact,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FinalCargoError::UnsupportedSnapshotDependency {
                dependency,
                reason,
                ..
            } if dependency.to_ascii_lowercase().contains("libstd")
                || reason.contains("relative loader search path")
        ));
        assert_eq!(fs::read_dir(&snapshot_root).unwrap().count(), 0);
    }

    #[test]
    fn relative_loader_dependency_is_rejected_explicitly() {
        let executable = Path::new("/fixture/target/app");
        let target = Path::new("/fixture/target");
        assert!(matches!(
            validate_snapshot_dependency(
                BinaryFormat::MachO,
                executable,
                target,
                "@rpath/libfixture.dylib",
            ),
            Err(FinalCargoError::UnsupportedSnapshotDependency {
                dependency,
                reason,
                ..
            }) if dependency == "@rpath/libfixture.dylib" && reason.contains("relative")
        ));
    }

    #[test]
    fn windows_pe_snapshots_read_regular_and_delayed_dlls_for_both_bitnesses() {
        for is_64 in [false, true] {
            let (bytes, _) = windows_pe_fixture(is_64, "KERNEL32.dll", "bcryptprimitives.dll");
            let file = object::File::parse(bytes.as_slice()).unwrap();
            let dependencies = executable_snapshot_dependencies(&file).unwrap();
            assert_eq!(
                dependencies.libraries,
                BTreeSet::from([b"KERNEL32.dll".to_vec(), b"bcryptprimitives.dll".to_vec()])
            );
            for dependency in &dependencies.libraries {
                validate_snapshot_dependency(
                    BinaryFormat::Pe,
                    Path::new("fixture.exe"),
                    Path::new("target"),
                    dependency_name(dependency).unwrap(),
                )
                .unwrap();
            }
        }
        for runtime in ["msvcrt.dll", "vcruntime140.dll", "VCRUNTIME140_1.DLL"] {
            assert!(platform_system_dependency(BinaryFormat::Pe, runtime));
        }
    }

    #[test]
    fn windows_pe_snapshot_does_not_silently_drop_an_app_local_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("app.exe");
        fs::write(temp.path().join("vcruntime140.dll"), b"app-local runtime").unwrap();
        assert!(matches!(
            validate_snapshot_dependency(BinaryFormat::Pe, &executable, temp.path(), "vcruntime140.dll"),
            Err(FinalCargoError::UnsupportedSnapshotDependency { reason, .. })
                if reason.contains("app-local DLL")
        ));
    }

    #[test]
    fn windows_pe_snapshots_reject_non_system_dlls_in_either_import_table() {
        for is_64 in [false, true] {
            for unsupported in [
                "plugin.dll",
                "libstd-fixture.dll",
                ".\\kernel32.dll",
                "api-ms-win-core-fixture\\plugin.dll",
                "api-ms-win-core-fixture.dll:stream",
                "api-ms-win-core-fixture",
            ] {
                for (normal, delayed) in
                    [(unsupported, "kernel32.dll"), ("kernel32.dll", unsupported)]
                {
                    let (bytes, _) = windows_pe_fixture(is_64, normal, delayed);
                    let file = object::File::parse(bytes.as_slice()).unwrap();
                    let dependencies = executable_snapshot_dependencies(&file).unwrap();
                    assert!(dependencies.libraries.iter().any(|dependency| {
                        validate_snapshot_dependency(
                            BinaryFormat::Pe,
                            Path::new("fixture.exe"),
                            Path::new("target"),
                            dependency_name(dependency).unwrap(),
                        )
                        .is_err()
                    }));
                }
            }
        }
    }

    #[test]
    fn windows_pe_snapshots_reject_malformed_or_legacy_delay_descriptors() {
        for is_64 in [false, true] {
            for attributes in [0_u32, 2, 0x8000_0000] {
                let (mut bytes, section) = windows_pe_fixture(is_64, "kernel32.dll", "user32.dll");
                bytes[section + 64..section + 68].copy_from_slice(&attributes.to_le_bytes());
                let file = object::File::parse(bytes.as_slice()).unwrap();
                assert!(executable_snapshot_dependencies(&file)
                    .unwrap_err()
                    .contains("RVA-based"));
            }
            for name_offset in [12, 68] {
                let (mut bytes, section) = windows_pe_fixture(is_64, "kernel32.dll", "user32.dll");
                bytes[section + name_offset..section + name_offset + 4]
                    .copy_from_slice(&u32::MAX.to_le_bytes());
                let file = object::File::parse(bytes.as_slice()).unwrap();
                assert!(executable_snapshot_dependencies(&file).is_err());
            }
        }
    }

    // These are real PE32/PE32+ headers and import descriptors. No Windows
    // compiler or execution is needed to test the production object reader.
    fn windows_pe_fixture(is_64: bool, normal: &str, delayed: &str) -> (Vec<u8>, usize) {
        use object::write::pe::{NtHeaders, Writer};
        let mut bytes = Vec::new();
        let mut writer = Writer::new(is_64, 0x1000, 0x200, &mut bytes);
        writer.reserve_dos_header();
        writer.reserve_nt_headers(16);
        writer.reserve_section_headers(1);
        let section = writer.reserve_rdata_section(512);
        writer.set_data_directory(
            object::pe::IMAGE_DIRECTORY_ENTRY_IMPORT,
            section.virtual_address,
            40,
        );
        writer.set_data_directory(
            object::pe::IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT,
            section.virtual_address + 64,
            64,
        );
        let mut data = vec![0; 512];
        data[12..16].copy_from_slice(&(section.virtual_address + 160).to_le_bytes());
        data[64..68].copy_from_slice(&1_u32.to_le_bytes());
        data[68..72].copy_from_slice(&(section.virtual_address + 256).to_le_bytes());
        data[160..160 + normal.len()].copy_from_slice(normal.as_bytes());
        data[256..256 + delayed.len()].copy_from_slice(delayed.as_bytes());
        writer.write_empty_dos_header().unwrap();
        writer.write_nt_headers(NtHeaders {
            machine: if is_64 {
                object::pe::IMAGE_FILE_MACHINE_AMD64
            } else {
                object::pe::IMAGE_FILE_MACHINE_I386
            },
            time_date_stamp: 0,
            characteristics: object::pe::IMAGE_FILE_EXECUTABLE_IMAGE,
            major_linker_version: 0,
            minor_linker_version: 0,
            address_of_entry_point: 0,
            image_base: 0x400000,
            major_operating_system_version: 6,
            minor_operating_system_version: 0,
            major_image_version: 0,
            minor_image_version: 0,
            major_subsystem_version: 6,
            minor_subsystem_version: 0,
            subsystem: object::pe::IMAGE_SUBSYSTEM_WINDOWS_CUI,
            dll_characteristics: 0,
            size_of_stack_reserve: 0x100000,
            size_of_stack_commit: 0x1000,
            size_of_heap_reserve: 0x100000,
            size_of_heap_commit: 0x1000,
        });
        writer.write_section_headers();
        writer.write_section(section.file_offset, &data);
        (bytes, section.file_offset as usize)
    }

    #[test]
    fn elf_system_loader_soname_is_an_allowed_runtime_dependency() {
        assert!(validate_snapshot_dependency(
            BinaryFormat::Elf,
            Path::new("/fixture/target/app"),
            Path::new("/fixture/target"),
            "ld-linux-x86-64.so.2",
        )
        .is_ok());
    }

    #[test]
    fn executable_architecture_must_match_the_host() {
        let host = host_architecture().expect("the test host must have a supported architecture");
        let mismatch = if host == Architecture::X86_64 {
            Architecture::Aarch64
        } else {
            Architecture::X86_64
        };
        assert!(matches!(
            validate_snapshot_host_identity(
                Path::new("/fixture/target/app"),
                host_executable_format(),
                mismatch,
            ),
            Err(FinalCargoError::UnsupportedSnapshotExecutable { reason, .. })
                if reason.contains("does not match the current host architecture")
        ));
    }

    #[test]
    fn interpreter_inside_mutable_target_is_rejected() {
        let executable = Path::new("/fixture/target/app");
        let target = Path::new("/fixture/target");
        assert!(matches!(
            validate_snapshot_interpreter(executable, target, "/fixture/target/ld.so"),
            Err(FinalCargoError::UnsupportedSnapshotDependency {
                dependency,
                reason,
                ..
            }) if dependency == "/fixture/target/ld.so" && reason.contains("mutable final Cargo target")
        ));
    }

    #[cfg(not(target_os = "windows"))]
    fn compile_snapshot_fixture(target_directory: &Path, prefer_dynamic: bool) -> PathBuf {
        let source = target_directory.join(if prefer_dynamic {
            "dynamic-fixture.rs"
        } else {
            "static-fixture.rs"
        });
        fs::write(&source, "fn main() {}\n").unwrap();
        let executable = target_directory.join(if prefer_dynamic {
            "dynamic-fixture"
        } else {
            "static-fixture"
        });
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
        let mut command = Command::new(rustc);
        command.arg(&source).arg("-o").arg(&executable);
        if prefer_dynamic {
            command.arg("-Cprefer-dynamic");
        }
        let status = command.status().unwrap();
        assert!(status.success(), "fixture rustc failed with {status}");
        executable
    }

    fn snapshot_fixture_artifact(root: &Path, executable: PathBuf) -> CargoCompilerArtifact {
        CargoCompilerArtifact {
            package_id: "path+file:///fixture#snapshot_fixture@0.0.0".to_string(),
            manifest_path: root.join("Cargo.toml"),
            target: CargoArtifactTarget {
                name: "snapshot_fixture".to_string(),
                kind: BTreeSet::from(["bin".to_string()]),
                crate_types: BTreeSet::from(["bin".to_string()]),
                src_path: root.join("src/main.rs"),
                edition: "2021".to_string(),
            },
            profile: serde_json::json!({"opt_level":"0","test":false}),
            features: BTreeSet::new(),
            filenames: BTreeSet::from([executable.clone()]),
            executable: Some(executable),
            fresh: false,
        }
    }

    #[test]
    fn final_cargo_rewrites_execution_requests_to_compile_only_arguments() {
        let arguments = vec![
            OsString::from("test"),
            OsString::from("-p"),
            OsString::from("demo"),
            OsString::from("--test"),
            OsString::from("http"),
            OsString::from("--"),
            OsString::from("--nocapture"),
            OsString::from("--target-dir"),
        ];
        assert_eq!(
            compile_only_cargo_arguments(&arguments).unwrap(),
            [
                OsString::from("test"),
                OsString::from("-p"),
                OsString::from("demo"),
                OsString::from("--test"),
                OsString::from("http"),
                OsString::from("--no-run"),
            ]
        );
        assert_eq!(
            compile_only_cargo_arguments(
                &["run", "-p", "demo", "--bin", "server", "--", "--probe",].map(OsString::from)
            )
            .unwrap(),
            [
                OsString::from("build"),
                OsString::from("-p"),
                OsString::from("demo"),
                OsString::from("--bin"),
                OsString::from("server"),
            ]
        );
        assert!(matches!(
            compile_only_cargo_arguments(&["build", "--", "ignored"].map(OsString::from)),
            Err(FinalCargoError::RuntimeArgumentsForCompileOnlySubcommand(subcommand))
                if subcommand == OsStr::new("build")
        ));
    }

    #[test]
    fn final_unit_identity_changes_with_the_plan() {
        let fingerprint = SemanticHash::derive("root-unit");
        let first = FinalUnitId::for_plan(fingerprint, plan().plan_id);
        let second = FinalUnitId::for_plan(
            fingerprint,
            PlanId::derive(&InvocationId::derive("other-plan")),
        );
        assert_ne!(first, second);
    }

    #[test]
    fn managed_final_paths_must_remain_beneath_source_exclusion() {
        let temp = tempfile::TempDir::new().unwrap();
        let exclusion = temp.path().join("generated");
        let outside = temp.path().join("outside/final");

        assert!(matches!(
            ensure_plain_managed_directory(&exclusion, &outside),
            Err(ManagedPathError::OutsideSourceExclusion { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn managed_final_paths_reject_symlinked_directories_and_plan_files() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::TempDir::new().unwrap();
        let exclusion = temp.path().join("generated");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        ensure_plain_managed_directory(&exclusion, &exclusion).unwrap();

        let redirected_target = exclusion.join("final-target");
        symlink(&outside, &redirected_target).unwrap();
        assert!(matches!(
            ensure_plain_managed_directory(&exclusion, &redirected_target),
            Err(ManagedPathError::NonDirectory(path)) if path == redirected_target
        ));

        let plans = exclusion.join("plans");
        ensure_plain_managed_directory(&exclusion, &plans).unwrap();
        let real_plan = outside.join("plan.json");
        fs::write(&real_plan, b"{}").unwrap();
        let redirected_plan = plans.join("plan.json");
        symlink(&real_plan, &redirected_plan).unwrap();
        assert!(matches!(
            ensure_plain_managed_file(&exclusion, &redirected_plan),
            Err(ManagedPathError::NonFile(path)) if path == redirected_plan
        ));
    }

    #[test]
    fn same_ids_cannot_authorize_different_plan_content() {
        let plan = plan();
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp
            .path()
            .join(format!("{}.transport-plan.json", plan.plan_id));
        let mut value = transport_plan_json(&plan);
        value["payload_hash"] = Value::String(format!("sha256:{}", "ff".repeat(32)));
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        assert!(matches!(
            validate_plan_file(&path, &plan),
            Err(FinalCargoError::PlanContentMismatch)
        ));
    }
}
