// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end orchestration for one exact RSJS Cargo root.
//!
//! This module is deliberately small glue over the independently tested
//! descriptor runner, exact-artifact transport linker, canonical semantic
//! linker, and plan-sensitive final runner. It does not discover artifacts in
//! a target directory or infer a root from a source path. The caller supplies
//! one exact Cargo artifact selector, and every later identity comes from
//! Cargo messages, validated wrapper sidecars, or rustc relocations.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use rsjs_ir::abi::{AbiVersion, InvocationId, ProducerVersion, SemanticHash};
use rsjs_ir::build::ArtifactRole;
use thiserror::Error;

use crate::rsjs::abi_adapter::{
    adapt_transport_plan, AbiAdapterContext, AbiAdapterError, AdaptedTransportAbi,
};
use crate::rsjs::build::{
    build_link_unit_manifest, descriptor_run_sealed, join_cargo_enumerated_descriptor_units,
    managed_paths_overlap, normalize_managed_path_identity, remove_successful_descriptor_staging,
    run_descriptor_cargo, validate_descriptor_managed_paths, CargoCompilerArtifact,
    DescriptorCargoOutput, DescriptorCargoReport, DescriptorCargoRequest, DescriptorCargoRun,
    DescriptorCargoRunError, DescriptorCargoUnitId, LinkManifestAssemblyError,
    UnitArtifactJoinError, ValidatedDescriptorUnit,
};
use crate::rsjs::cache::{run_descriptor_cargo_cached, DescriptorCacheError};
#[cfg(test)]
use crate::rsjs::final_compile::FinalCargoArtifactReport;
use crate::rsjs::final_compile::{
    ensure_plain_managed_directory, ensure_plain_managed_file, run_final_cargo_leased,
    snapshot_final_artifact, FinalCargoError, FinalCargoOutput, FinalCargoRequest,
    ManagedPathError,
};
use crate::rsjs::link::{link_transport_manifest, TransportLinkError, TransportLinkPlan};
use crate::rsjs::semantic_link::{
    link_semantic_plan, write_content_addressed_semantic_plan, SemanticLinkError, SemanticLinkPlan,
    SemanticPlanWriteError,
};

/// Exact Cargo root selected from the descriptor invocation's JSON messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorRootSelector {
    /// Exact Cargo package ID from a `compiler-artifact` message.
    pub package_id: String,
    /// Exact target name.
    pub target_name: String,
    /// Exact normalized Cargo target-kind set.
    pub target_kinds: BTreeSet<String>,
    /// Link-manifest role for this selected root.
    pub role: ArtifactRole,
    /// Exact Cargo-enumerated root filename.
    ///
    /// This may be omitted only when Cargo reported exactly one usable
    /// executable or filename for the selected artifact.
    pub artifact_path: Option<PathBuf>,
}

/// One complete descriptor, link, and final-Cargo request.
#[derive(Clone, Debug)]
pub struct LinkedCargoRequest {
    /// Exact descriptor-pass Cargo request.
    pub descriptor: DescriptorCargoRequest,
    /// Root selector applied only to that invocation's Cargo JSON messages.
    pub root: DescriptorRootSelector,
    /// Pre-Cargo invocation identity.
    pub invocation: InvocationId,
    /// Compiler producer version recorded in canonical descriptor envelopes.
    pub producer: ProducerVersion,
    /// Runtime protocol selected for canonical compatibility checks.
    pub runtime_protocol: AbiVersion,
    /// Absolute content-addressed semantic-plan directory.
    pub plan_directory: PathBuf,
    /// Absolute parent for invocation- and wrapper-policy-scoped final target
    /// directories.
    pub final_target_root: PathBuf,
    /// Cargo arguments for final compilation, without runner-owned
    /// `--message-format` or `--target-dir`.
    pub final_cargo_args: Vec<OsString>,
    /// Exact additional environment for final Cargo.
    pub final_environment: BTreeMap<OsString, OsString>,
}

/// Successful output of every stage for one selected root.
#[derive(Clone, Debug)]
pub struct LinkedCargoOutput {
    /// Path-free descriptor Cargo diagnostics.
    pub descriptor: DescriptorCargoReport,
    /// Exact selected descriptor unit.
    pub root_unit: DescriptorCargoUnitId,
    /// Relocation-resolved transport graph.
    pub transport: TransportLinkPlan,
    /// Canonical descriptors produced by the compatibility bridge.
    pub adapted: AdaptedTransportAbi,
    /// Canonical semantic owner/helper plan.
    pub semantic: SemanticLinkPlan,
    /// Exact content-addressed semantic-plan path.
    pub semantic_plan_path: PathBuf,
    /// Plan-sensitive final Cargo output.
    pub final_cargo: FinalCargoOutput,
    /// Exact final-phase Cargo artifact attesting the selected descriptor root.
    pub final_root_artifact: CargoCompilerArtifact,
}

/// Run descriptor compilation, exact linking, semantic planning, and final
/// Cargo compilation for one root.
pub fn run_linked_cargo(
    request: &LinkedCargoRequest,
) -> Result<LinkedCargoOutput, LinkedCargoError> {
    validate_request(request)?;
    prepare_linked_managed_directories(request)?;
    let lease =
        run_descriptor_cargo(&request.descriptor).map_err(LinkedCargoError::DescriptorCargo)?;
    let linked = link_descriptor_transport(lease, request.invocation, &request.root)?;
    let prepared = prepare_linked_cargo(request, linked)?;
    let output = finish_prepared_linked_cargo(request, prepared)?;
    remove_successful_descriptor_staging(&request.descriptor)
        .map_err(LinkedCargoError::DescriptorStagingCleanup)?;
    Ok(output)
}

/// Run the exact linked pipeline while reusing descriptor objects admitted by
/// the explicit persistent cache.
///
/// Cargo remains the freshness authority. The cache is used only after the
/// current Cargo JSON stream authorizes unchanged artifacts, and every cached
/// sidecar/object hash is revalidated before linking.
pub fn run_linked_cargo_cached(
    request: &LinkedCargoRequest,
    cache_directory: &std::path::Path,
) -> Result<LinkedCargoOutput, LinkedCargoError> {
    run_linked_cargo_cached_with_progress(request, cache_directory, |_| {})
}

pub(crate) enum LinkedCargoStage {
    Descriptor,
    Link,
    Final,
}

pub(crate) fn run_linked_cargo_cached_with_progress(
    request: &LinkedCargoRequest,
    cache_directory: &Path,
    mut progress: impl FnMut(LinkedCargoStage),
) -> Result<LinkedCargoOutput, LinkedCargoError> {
    let managed = validate_request(request)?;
    validate_cache_against_linked_paths(cache_directory, &managed)?;
    prepare_linked_managed_directories(request)?;
    progress(LinkedCargoStage::Descriptor);
    let lease = run_descriptor_cargo_cached(&request.descriptor, cache_directory)
        .map_err(LinkedCargoError::DescriptorCache)?;
    progress(LinkedCargoStage::Link);
    let linked = link_descriptor_transport(lease, request.invocation, &request.root)?;
    let prepared = prepare_linked_cargo(request, linked)?;
    progress(LinkedCargoStage::Final);
    finish_prepared_linked_cargo(request, prepared)
}

fn validate_cache_against_linked_paths(
    cache_directory: &Path,
    managed: &ValidatedLinkedManagedPaths,
) -> Result<(), LinkedCargoError> {
    let cache = normalize_linked_managed_path("descriptor cache", cache_directory)?;
    for (kind, path) in [
        ("semantic plans", &managed.plan),
        ("final targets", &managed.final_target),
    ] {
        if managed_paths_overlap(&cache, path) {
            return Err(LinkedCargoError::ManagedDirectoryOverlap {
                first_kind: "descriptor cache",
                first: cache.clone(),
                second_kind: kind,
                second: path.clone(),
            });
        }
    }
    Ok(())
}

struct PreparedLinkedCargo {
    descriptor: DescriptorCargoReport,
    selected: SelectedDescriptorRoot,
    transport: TransportLinkPlan,
    adapted: AdaptedTransportAbi,
    semantic: SemanticLinkPlan,
    semantic_plan_path: PathBuf,
    final_descriptor_units: Vec<ValidatedDescriptorUnit>,
}

/// Descriptor artifacts fully hashed and transport-linked while their target
/// and optional cache generation remained exclusively leased.
#[derive(Debug)]
pub struct LinkedDescriptorTransport {
    /// Path-free descriptor diagnostics.
    pub descriptor: DescriptorCargoReport,
    /// Exact selected descriptor unit.
    pub root_unit: DescriptorCargoUnitId,
    /// Relocation-resolved transport graph, independent of target paths.
    pub transport: TransportLinkPlan,
    selected: SelectedDescriptorRoot,
    final_descriptor_units: Vec<ValidatedDescriptorUnit>,
}

/// Consume a cached or uncached descriptor run and materialize every mutable
/// target byte needed by the transport linker before releasing its leases.
pub fn link_descriptor_transport<R: DescriptorCargoRun>(
    run: R,
    invocation: InvocationId,
    selector: &DescriptorRootSelector,
) -> Result<LinkedDescriptorTransport, LinkedCargoError> {
    let mut timing = super::timing::StageTimer::new();
    let linked = {
        let descriptor = descriptor_run_sealed::Sealed::output(&run);
        let selected = select_descriptor_root(descriptor, selector)?;
        let manifest = build_link_unit_manifest(
            invocation,
            selected.unit,
            &selected.artifact,
            selector.role,
            descriptor,
        )
        .map_err(LinkedCargoError::LinkManifest)?;
        timing.mark("transport manifest and root selection");
        let transport = link_transport_manifest(&manifest, &descriptor.units, &descriptor.objects)
            .map_err(LinkedCargoError::TransportLink)?;
        timing.mark("transport graph");
        let cargo_enumerated_units =
            join_cargo_enumerated_descriptor_units(&descriptor.units, &descriptor.cargo_artifacts)
                .map_err(LinkedCargoError::JoinDescriptorArtifacts)?;
        let final_descriptor_units = descriptor
            .units
            .iter()
            .filter(|unit| cargo_enumerated_units.contains_key(&unit.unit))
            .cloned()
            .collect();

        LinkedDescriptorTransport {
            descriptor: DescriptorCargoReport::from_output(descriptor),
            root_unit: selected.unit,
            transport,
            selected,
            final_descriptor_units,
        }
    };

    // `linked` owns only extracted/path-free material plus private final-map
    // inputs. Releasing the lease here keeps semantic-plan writes and final
    // Cargo outside both descriptor and cache critical sections.
    drop(descriptor_run_sealed::Sealed::into_output(run));
    timing.mark("transport final-unit selection");
    Ok(linked)
}

fn prepare_linked_cargo(
    request: &LinkedCargoRequest,
    linked: LinkedDescriptorTransport,
) -> Result<PreparedLinkedCargo, LinkedCargoError> {
    let LinkedDescriptorTransport {
        descriptor,
        selected,
        transport,
        final_descriptor_units,
        ..
    } = linked;
    let mut timing = super::timing::StageTimer::new();
    let abi = AbiAdapterContext::from_transport_plan(
        request.producer,
        request.runtime_protocol,
        &transport,
    )
    .map_err(LinkedCargoError::AbiAdapter)?;
    let adapted = adapt_transport_plan(&abi, &transport).map_err(LinkedCargoError::AbiAdapter)?;
    timing.mark("ABI adaptation");
    let semantic = link_semantic_plan(transport.link_unit, &adapted)
        .map_err(LinkedCargoError::SemanticLink)?;
    timing.mark("semantic planning");
    let semantic_plan_path =
        write_content_addressed_semantic_plan(&request.plan_directory, &semantic)
            .map_err(LinkedCargoError::WriteSemanticPlan)?;
    timing.mark("semantic plan publication");
    Ok(PreparedLinkedCargo {
        descriptor,
        selected,
        transport,
        adapted,
        semantic,
        semantic_plan_path,
        final_descriptor_units,
    })
}

fn finish_prepared_linked_cargo(
    request: &LinkedCargoRequest,
    prepared: PreparedLinkedCargo,
) -> Result<LinkedCargoOutput, LinkedCargoError> {
    let PreparedLinkedCargo {
        descriptor,
        selected,
        transport,
        adapted,
        semantic,
        semantic_plan_path,
        final_descriptor_units,
    } = prepared;

    let final_request = FinalCargoRequest {
        invocation: request.invocation,
        cargo: request.descriptor.cargo.clone(),
        wrapper: request.descriptor.wrapper.clone(),
        working_directory: request.descriptor.working_directory.clone(),
        source_exclusion_directory: request.descriptor.source_exclusion_directory.clone(),
        target_root: request.final_target_root.clone(),
        plan_path: semantic_plan_path.clone(),
        cargo_args: request.final_cargo_args.clone(),
        // validate_request proves both phase maps are byte-for-byte equal;
        // keep one frozen source of truth for all caller-supplied variables.
        environment: request.descriptor.environment.clone(),
        root_unit_fingerprint: SemanticHash::derive(&selected.unit.as_abi_id()),
        descriptor_units: final_descriptor_units,
    };
    let final_cargo_run =
        run_final_cargo_leased(&final_request, &semantic).map_err(LinkedCargoError::FinalCargo)?;
    let final_root_artifact = attest_final_root(
        final_cargo_run.output(),
        final_cargo_run.cargo_artifacts(),
        &request.root,
        &selected.cargo_artifact,
    )?;
    let final_root_artifact = snapshot_final_artifact(
        &request.descriptor.source_exclusion_directory,
        &request
            .final_target_root
            .join("rsjs-final-artifact-snapshots"),
        &final_cargo_run.output().target_directory,
        &final_root_artifact,
    )
    .map_err(LinkedCargoError::FinalCargo)?;
    let final_cargo = final_cargo_run.into_output();
    // The selected root now points only at immutable content-addressed files,
    // so long-running programs never retain the reusable target lease and a
    // later build cannot replace the artifact between attestation and spawn.
    execute_requested_final_root(request, &final_root_artifact)?;

    Ok(LinkedCargoOutput {
        descriptor,
        root_unit: selected.unit,
        transport,
        adapted,
        semantic,
        semantic_plan_path,
        final_cargo,
        final_root_artifact,
    })
}

#[derive(Clone, Debug)]
struct SelectedDescriptorRoot {
    unit: DescriptorCargoUnitId,
    artifact: PathBuf,
    cargo_artifact: CargoCompilerArtifact,
}

fn select_descriptor_root(
    output: &DescriptorCargoOutput,
    selector: &DescriptorRootSelector,
) -> Result<SelectedDescriptorRoot, LinkedCargoError> {
    validate_selector(selector)?;
    let joined = join_cargo_enumerated_descriptor_units(&output.units, &output.cargo_artifacts)
        .map_err(LinkedCargoError::JoinDescriptorArtifacts)?;
    let mut matches = joined
        .into_iter()
        .filter(|(_, artifact)| root_artifact_matches(artifact, selector))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));

    let (unit, artifact) = match matches.as_slice() {
        [] => {
            return Err(LinkedCargoError::MissingRoot {
                package_id: selector.package_id.clone(),
                target_name: selector.target_name.clone(),
            });
        }
        [(unit, artifact)] => (*unit, artifact),
        _ => {
            return Err(LinkedCargoError::AmbiguousRoot {
                package_id: selector.package_id.clone(),
                target_name: selector.target_name.clone(),
                matches: matches.len(),
            });
        }
    };
    let artifact_path = select_root_path(artifact, selector)?;
    Ok(SelectedDescriptorRoot {
        unit,
        artifact: artifact_path,
        cargo_artifact: artifact.clone(),
    })
}

fn root_artifact_matches(
    artifact: &CargoCompilerArtifact,
    selector: &DescriptorRootSelector,
) -> bool {
    artifact.package_id == selector.package_id
        && artifact.target.name == selector.target_name
        && artifact.target.kind == selector.target_kinds
}

fn select_root_path(
    artifact: &CargoCompilerArtifact,
    selector: &DescriptorRootSelector,
) -> Result<PathBuf, LinkedCargoError> {
    if let Some(requested) = &selector.artifact_path {
        if !requested.is_absolute() {
            return Err(LinkedCargoError::RelativeRootArtifact(requested.clone()));
        }
        if artifact.executable.as_ref() == Some(requested) || artifact.filenames.contains(requested)
        {
            return Ok(requested.clone());
        }
        return Err(LinkedCargoError::RootArtifactNotEnumerated(
            requested.clone(),
        ));
    }

    let mut candidates = artifact.filenames.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(executable) = &artifact.executable {
        candidates.insert(executable.clone());
    }
    match candidates.len() {
        0 => Err(LinkedCargoError::RootHasNoArtifact),
        1 => Ok(candidates
            .into_iter()
            .next()
            .expect("one candidate was counted")),
        count => Err(LinkedCargoError::AmbiguousRootArtifact { matches: count }),
    }
}

fn attest_final_root(
    output: &FinalCargoOutput,
    cargo_artifacts: &[CargoCompilerArtifact],
    selector: &DescriptorRootSelector,
    descriptor_root: &CargoCompilerArtifact,
) -> Result<CargoCompilerArtifact, LinkedCargoError> {
    let mut matches = cargo_artifacts
        .iter()
        .filter(|artifact| root_artifact_matches(artifact, selector))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.manifest_path
            .cmp(&right.manifest_path)
            .then_with(|| left.target.src_path.cmp(&right.target.src_path))
    });
    let final_root = match matches.as_slice() {
        [] => {
            return Err(LinkedCargoError::MissingFinalRoot {
                package_id: selector.package_id.clone(),
                target_name: selector.target_name.clone(),
            });
        }
        [artifact] => *artifact,
        artifacts => {
            return Err(LinkedCargoError::AmbiguousFinalRoot {
                package_id: selector.package_id.clone(),
                target_name: selector.target_name.clone(),
                matches: artifacts.len(),
            });
        }
    };

    if final_root.manifest_path != descriptor_root.manifest_path
        || final_root.target != descriptor_root.target
        || final_root.profile != descriptor_root.profile
        || final_root.features != descriptor_root.features
    {
        return Err(LinkedCargoError::FinalRootCoordinateMismatch {
            package_id: selector.package_id.clone(),
            target_name: selector.target_name.clone(),
        });
    }

    let materialized = match (
        descriptor_root.executable.as_ref(),
        final_root.executable.as_ref(),
    ) {
        (Some(_), Some(executable)) => executable,
        (Some(_), None) => {
            return Err(LinkedCargoError::MissingFinalRootExecutable {
                package_id: selector.package_id.clone(),
                target_name: selector.target_name.clone(),
            });
        }
        (None, _) => final_root
            .filenames
            .iter()
            .next()
            .ok_or(LinkedCargoError::FinalRootHasNoArtifact)?,
    };
    ensure_plain_managed_file(&output.target_directory, materialized)
        .map_err(LinkedCargoError::ManagedPath)?;
    let metadata = fs::symlink_metadata(materialized).map_err(|source| {
        LinkedCargoError::InspectFinalRootArtifact {
            path: materialized.clone(),
            source,
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(LinkedCargoError::FinalRootArtifactNotFile(
            materialized.clone(),
        ));
    }

    Ok(final_root.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalExecution<'a> {
    None,
    Run(&'a [OsString]),
    Test(&'a [OsString]),
}

fn requested_final_execution(arguments: &[OsString]) -> FinalExecution<'_> {
    let (compile_arguments, runtime_arguments) = split_cargo_runtime_arguments(arguments);
    let runtime_arguments = runtime_arguments.unwrap_or_default();
    match compile_arguments.first().map(OsString::as_os_str) {
        Some(subcommand) if subcommand == OsStr::new("run") => {
            FinalExecution::Run(runtime_arguments)
        }
        Some(subcommand) if subcommand == OsStr::new("test") => {
            FinalExecution::Test(runtime_arguments)
        }
        _ => FinalExecution::None,
    }
}

fn execute_requested_final_root(
    request: &LinkedCargoRequest,
    artifact: &CargoCompilerArtifact,
) -> Result<(), LinkedCargoError> {
    let execution = requested_final_execution(&request.final_cargo_args);
    let (runtime_arguments, working_directory) = match execution {
        FinalExecution::None => return Ok(()),
        FinalExecution::Run(arguments) => {
            (arguments, request.descriptor.working_directory.as_path())
        }
        FinalExecution::Test(arguments) => {
            let working_directory = artifact.manifest_path.parent().ok_or_else(|| {
                LinkedCargoError::FinalRootManifestWithoutParent(artifact.manifest_path.clone())
            })?;
            (arguments, working_directory)
        }
    };
    let executable = artifact.executable.as_ref().ok_or_else(|| {
        LinkedCargoError::FinalRootNotHostExecutable {
            package_id: artifact.package_id.clone(),
            target_name: artifact.target.name.clone(),
        }
    })?;

    let status = Command::new(executable)
        .current_dir(working_directory)
        .args(runtime_arguments)
        .envs(&request.final_environment)
        // `status` inherits the caller's raw stdin/stdout/stderr. The direct
        // child also inherits the normal local environment; only explicit
        // request variables override it, matching the Cargo invocation.
        .status()
        .map_err(|source| LinkedCargoError::ExecuteFinalRoot {
            path: executable.clone(),
            source,
        })?;
    if !status.success() {
        return Err(LinkedCargoError::FinalRootExited {
            path: executable.clone(),
            status,
        });
    }
    Ok(())
}

fn validate_phase_cargo_arguments(
    descriptor: &[OsString],
    final_args: &[OsString],
) -> Result<(), LinkedCargoError> {
    let (descriptor_compile, descriptor_runtime) = split_cargo_runtime_arguments(descriptor);
    if descriptor_runtime.is_some() {
        return Err(LinkedCargoError::DescriptorRuntimeArguments);
    }
    let (final_compile, final_runtime) = split_cargo_runtime_arguments(final_args);
    let descriptor_subcommand = descriptor_compile
        .first()
        .ok_or(LinkedCargoError::MissingDescriptorCargoArguments)?;
    let final_subcommand = final_compile
        .first()
        .ok_or(LinkedCargoError::MissingFinalCargoArguments)?;

    let supported_pair = (
        descriptor_subcommand.as_os_str(),
        final_subcommand.as_os_str(),
    );
    if !matches!(
        supported_pair,
        (descriptor, final_phase)
            if (descriptor == OsStr::new("build")
                && (final_phase == OsStr::new("build")
                    || final_phase == OsStr::new("run")))
                || (descriptor == OsStr::new("check")
                    && final_phase == OsStr::new("check"))
                || (descriptor == OsStr::new("test")
                    && final_phase == OsStr::new("test"))
    ) {
        return Err(LinkedCargoError::UnsupportedPhaseSubcommands {
            descriptor: descriptor_subcommand.clone(),
            final_phase: final_subcommand.clone(),
        });
    }
    if final_runtime.is_some()
        && final_subcommand != OsStr::new("run")
        && final_subcommand != OsStr::new("test")
    {
        return Err(LinkedCargoError::UnsupportedFinalRuntimeArguments(
            final_subcommand.clone(),
        ));
    }

    let mut normalized_descriptor = descriptor_compile.to_vec();
    let mut normalized_final = final_compile.to_vec();
    normalized_descriptor[0] = OsString::from("<linked-phase>");
    normalized_final[0] = OsString::from("<linked-phase>");

    let descriptor_no_run = normalized_descriptor
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == OsStr::new("--no-run")).then_some(index))
        .collect::<Vec<_>>();
    let final_has_no_run = normalized_final
        .iter()
        .any(|argument| argument == OsStr::new("--no-run"));
    if descriptor_subcommand == OsStr::new("test") {
        if descriptor_no_run.len() != 1 || final_has_no_run {
            return Err(LinkedCargoError::InvalidTestPhaseNoRun);
        }
        normalized_descriptor.remove(descriptor_no_run[0]);
    } else if !descriptor_no_run.is_empty() || final_has_no_run {
        return Err(LinkedCargoError::InvalidTestPhaseNoRun);
    }

    if normalized_descriptor != normalized_final {
        return Err(LinkedCargoError::PhaseCargoArgumentsMismatch {
            descriptor: normalized_descriptor,
            final_phase: normalized_final,
        });
    }
    Ok(())
}

fn split_cargo_runtime_arguments(arguments: &[OsString]) -> (&[OsString], Option<&[OsString]>) {
    match arguments
        .iter()
        .position(|argument| argument == OsStr::new("--"))
    {
        Some(index) => (&arguments[..index], Some(&arguments[index + 1..])),
        None => (arguments, None),
    }
}

pub(crate) struct ValidatedLinkedManagedPaths {
    plan: PathBuf,
    final_target: PathBuf,
}

pub(crate) fn validate_request(
    request: &LinkedCargoRequest,
) -> Result<ValidatedLinkedManagedPaths, LinkedCargoError> {
    if !request.plan_directory.is_absolute() {
        return Err(LinkedCargoError::RelativePlanDirectory(
            request.plan_directory.clone(),
        ));
    }
    if !request.final_target_root.is_absolute() {
        return Err(LinkedCargoError::RelativeFinalTargetRoot(
            request.final_target_root.clone(),
        ));
    }
    if request.final_cargo_args.is_empty() {
        return Err(LinkedCargoError::MissingFinalCargoArguments);
    }
    validate_phase_cargo_arguments(&request.descriptor.cargo_args, &request.final_cargo_args)?;
    if request.descriptor.environment != request.final_environment {
        let names = request
            .descriptor
            .environment
            .keys()
            .chain(request.final_environment.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|name| {
                request.descriptor.environment.get(*name) != request.final_environment.get(*name)
            })
            .cloned()
            .collect();
        return Err(LinkedCargoError::PhaseEnvironmentMismatch { names });
    }
    let descriptor = validate_descriptor_managed_paths(&request.descriptor)
        .map_err(LinkedCargoError::DescriptorManagedPaths)?;
    let plan = normalize_linked_managed_path("semantic plans", &request.plan_directory)?;
    let final_target = normalize_linked_managed_path("final targets", &request.final_target_root)?;
    let managed_roots = [
        ("descriptor staging", &descriptor.staging),
        ("descriptor target", &descriptor.target),
        ("semantic plans", &plan),
        ("final targets", &final_target),
    ];
    for (index, (first_kind, first)) in managed_roots.iter().enumerate() {
        for (second_kind, second) in &managed_roots[index + 1..] {
            if managed_paths_overlap(first, second) {
                return Err(LinkedCargoError::ManagedDirectoryOverlap {
                    first_kind,
                    first: (*first).clone(),
                    second_kind,
                    second: (*second).clone(),
                });
            }
        }
    }
    Ok(ValidatedLinkedManagedPaths { plan, final_target })
}

fn prepare_linked_managed_directories(
    request: &LinkedCargoRequest,
) -> Result<(), LinkedCargoError> {
    ensure_plain_managed_directory(
        &request.descriptor.source_exclusion_directory,
        &request.plan_directory,
    )
    .map_err(LinkedCargoError::ManagedPath)?;
    ensure_plain_managed_directory(
        &request.descriptor.source_exclusion_directory,
        &request.final_target_root,
    )
    .map_err(LinkedCargoError::ManagedPath)?;
    Ok(())
}

fn normalize_linked_managed_path(
    kind: &'static str,
    path: &Path,
) -> Result<PathBuf, LinkedCargoError> {
    normalize_managed_path_identity(path).map_err(|source| {
        LinkedCargoError::NormalizeManagedDirectory {
            kind,
            path: path.to_path_buf(),
            source,
        }
    })
}

fn validate_selector(selector: &DescriptorRootSelector) -> Result<(), LinkedCargoError> {
    if selector.package_id.is_empty() {
        return Err(LinkedCargoError::EmptyRootSelector("package_id"));
    }
    if selector.target_name.is_empty() {
        return Err(LinkedCargoError::EmptyRootSelector("target_name"));
    }
    if selector.target_kinds.is_empty() || selector.target_kinds.iter().any(String::is_empty) {
        return Err(LinkedCargoError::EmptyRootSelector("target_kinds"));
    }
    if !selector.role.is_root() {
        return Err(LinkedCargoError::NonRootRole(selector.role));
    }
    Ok(())
}

/// End-to-end orchestration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LinkedCargoError {
    /// Descriptor Cargo must receive an explicit subcommand/selection too.
    #[error("RSJS descriptor Cargo arguments are empty")]
    MissingDescriptorCargoArguments,
    /// Plan storage must be cwd-independent.
    #[error("RSJS semantic-plan directory must be absolute: `{0}`")]
    RelativePlanDirectory(PathBuf),
    /// Final artifacts live in an invocation- and wrapper-policy-scoped
    /// absolute namespace.
    #[error("RSJS final target root must be absolute: `{0}`")]
    RelativeFinalTargetRoot(PathBuf),
    /// Final Cargo must receive an explicit subcommand/selection.
    #[error("RSJS final Cargo arguments are empty")]
    MissingFinalCargoArguments,
    /// Descriptor compilation may never execute a selected program.
    #[error("descriptor Cargo arguments may not contain a runtime `-- ...` suffix")]
    DescriptorRuntimeArguments,
    /// Only the lifecycle's known phase pairs preserve one Cargo unit model.
    #[error(
        "unsupported linked Cargo phase pair: descriptor {descriptor:?}, final {final_phase:?}"
    )]
    UnsupportedPhaseSubcommands {
        /// Descriptor-pass Cargo subcommand.
        descriptor: OsString,
        /// Final-pass Cargo subcommand.
        final_phase: OsString,
    },
    /// Runtime arguments are meaningful only for final run/test invocations.
    #[error("final Cargo subcommand {0:?} may not contain runtime arguments after `--`")]
    UnsupportedFinalRuntimeArguments(OsString),
    /// Tests compile without execution in the descriptor phase and execute
    /// only in the final phase.
    #[error(
        "linked Cargo test requires exactly one descriptor-only `--no-run` and none in the final compile arguments"
    )]
    InvalidTestPhaseNoRun,
    /// After the controlled phase rewrite, every Cargo selection/config input
    /// must be byte-for-byte identical.
    #[error(
        "descriptor and final Cargo selection/config arguments diverge after phase normalization: descriptor {descriptor:?}, final {final_phase:?}"
    )]
    PhaseCargoArgumentsMismatch {
        /// Normalized descriptor arguments.
        descriptor: Vec<OsString>,
        /// Normalized final arguments.
        final_phase: Vec<OsString>,
    },
    /// Caller-supplied compilation environment must be frozen across phases.
    #[error(
        "descriptor and final Cargo environments differ for variable names {names:?}; use one identical environment map for both phases"
    )]
    PhaseEnvironmentMismatch {
        /// Names only; potentially secret values are never included.
        names: Vec<OsString>,
    },
    /// Every generated namespace is a separate identity/deletion boundary.
    #[error(
        "RSJS managed {first_kind} directory `{first}` overlaps {second_kind} directory `{second}`"
    )]
    ManagedDirectoryOverlap {
        /// First managed namespace role.
        first_kind: &'static str,
        /// First managed namespace root.
        first: PathBuf,
        /// Second managed namespace role.
        second_kind: &'static str,
        /// Second managed namespace root.
        second: PathBuf,
    },
    /// Managed output confinement or plain-entry validation failed.
    #[error(transparent)]
    ManagedPath(#[from] ManagedPathError),
    /// Descriptor path relationships must be validated before any linked
    /// namespace is created.
    #[error("invalid descriptor managed paths: {0}")]
    DescriptorManagedPaths(#[source] DescriptorCargoRunError),
    /// Cross-subsystem comparisons use one canonical managed-path identity.
    #[error("failed to normalize RSJS managed {kind} directory `{path}`: {source}")]
    NormalizeManagedDirectory {
        /// Managed namespace role.
        kind: &'static str,
        /// Caller-supplied path.
        path: PathBuf,
        /// Canonicalization failure.
        #[source]
        source: std::io::Error,
    },
    /// Root selection fields are exact and nonempty.
    #[error("RSJS root selector field `{0}` must not be empty")]
    EmptyRootSelector(&'static str),
    /// A dependency role cannot be selected as a root.
    #[error("RSJS root selector uses non-root artifact role {0:?}")]
    NonRootRole(ArtifactRole),
    /// Descriptor Cargo failed.
    #[error("RSJS descriptor compilation failed: {0}")]
    DescriptorCargo(#[source] DescriptorCargoRunError),
    /// A fully successful uncached run could not remove only its exact,
    /// marker-owned invocation staging tree.
    #[error("failed to clean successful RSJS descriptor staging: {0}")]
    DescriptorStagingCleanup(#[source] DescriptorCargoRunError),
    /// Persistent descriptor reuse failed closed.
    #[error("RSJS descriptor cache validation failed: {0}")]
    DescriptorCache(#[source] DescriptorCacheError),
    /// Wrapper sidecars could not be joined to this invocation's Cargo
    /// messages.
    #[error("failed to join RSJS descriptor units to Cargo artifacts: {0}")]
    JoinDescriptorArtifacts(#[source] UnitArtifactJoinError),
    /// The exact selected Cargo root did not exist.
    #[error(
        "descriptor Cargo emitted no exact root for package `{package_id}` target `{target_name}`"
    )]
    MissingRoot {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
    },
    /// More than one unit matched an exact root selector.
    #[error(
        "descriptor Cargo emitted {matches} roots for package `{package_id}` target `{target_name}`"
    )]
    AmbiguousRoot {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
        /// Candidate count.
        matches: usize,
    },
    /// Final Cargo must emit the exact root selected from descriptor Cargo.
    #[error(
        "final Cargo emitted no exact root for descriptor package `{package_id}` target `{target_name}`"
    )]
    MissingFinalRoot {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
    },
    /// One exact root selection must remain unique in the final phase.
    #[error(
        "final Cargo emitted {matches} exact roots for descriptor package `{package_id}` target `{target_name}`"
    )]
    AmbiguousFinalRoot {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
        /// Candidate count.
        matches: usize,
    },
    /// Profile, features, manifest, and target metadata are part of root
    /// identity across phases.
    #[error(
        "final Cargo root coordinate differs from descriptor root for package `{package_id}` target `{target_name}`"
    )]
    FinalRootCoordinateMismatch {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
    },
    /// Executable roots must remain executable roots in the final phase.
    #[error(
        "final Cargo root for package `{package_id}` target `{target_name}` has no executable"
    )]
    MissingFinalRootExecutable {
        /// Exact package ID.
        package_id: String,
        /// Exact target name.
        target_name: String,
    },
    /// A final check root still needs one Cargo-enumerated materialized file.
    #[error("selected final RSJS root has no Cargo-enumerated artifact")]
    FinalRootHasNoArtifact,
    /// Final root output inspection failed.
    #[error("failed to inspect selected final root artifact `{path}`: {source}")]
    InspectFinalRootArtifact {
        /// Cargo-enumerated artifact path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// Cargo-enumerated final root material must be an ordinary file.
    #[error("selected final root artifact is not a plain regular file: `{0}`")]
    FinalRootArtifactNotFile(PathBuf),
    /// Run/test requests require a directly host-executable attested root.
    #[error(
        "attested final root for package `{package_id}` target `{target_name}` is not directly executable on this host"
    )]
    FinalRootNotHostExecutable {
        /// Exact Cargo package ID.
        package_id: String,
        /// Exact Cargo target name.
        target_name: String,
    },
    /// Cargo manifest paths must identify the test package directory.
    #[error("attested final root manifest has no parent directory: `{0}`")]
    FinalRootManifestWithoutParent(PathBuf),
    /// The attested executable could not be launched directly.
    #[error(
        "failed to execute attested final root `{path}` directly on this host: {source}; cargo-proa will not rerun Cargo target selection"
    )]
    ExecuteFinalRoot {
        /// Attested executable or test harness.
        path: PathBuf,
        /// Host spawn failure.
        #[source]
        source: std::io::Error,
    },
    /// User programs and test harnesses propagate a nonzero result.
    #[error("attested final root `{path}` exited unsuccessfully with {status}")]
    FinalRootExited {
        /// Attested executable or test harness.
        path: PathBuf,
        /// Exact host exit status.
        status: ExitStatus,
    },
    /// Caller-supplied artifact paths must be exact absolute Cargo paths.
    #[error("selected RSJS root artifact must be absolute: `{0}`")]
    RelativeRootArtifact(PathBuf),
    /// Caller-supplied root path was not in the matching Cargo message.
    #[error("selected RSJS root artifact was not enumerated by Cargo: `{0}`")]
    RootArtifactNotEnumerated(PathBuf),
    /// Cargo published no usable root artifact.
    #[error("selected RSJS root has no Cargo-enumerated artifact")]
    RootHasNoArtifact,
    /// Automatic root-filename selection is intentionally fail-closed.
    #[error(
        "selected RSJS root has {matches} Cargo-enumerated filenames; supply `artifact_path` exactly"
    )]
    AmbiguousRootArtifact {
        /// Candidate count.
        matches: usize,
    },
    /// Exact external root graph assembly failed.
    #[error("failed to assemble RSJS link manifest: {0}")]
    LinkManifest(#[source] LinkManifestAssemblyError),
    /// Object extraction/relocation resolution failed.
    #[error("failed to resolve RSJS transport records: {0}")]
    TransportLink(#[source] TransportLinkError),
    /// Legacy producer data could not become canonical ABI.
    #[error("failed to adapt RSJS descriptors: {0}")]
    AbiAdapter(#[source] AbiAdapterError),
    /// Canonical graph/owner linking failed.
    #[error("failed to link canonical RSJS plan: {0}")]
    SemanticLink(#[source] SemanticLinkError),
    /// Semantic plan persistence failed.
    #[error("failed to persist canonical RSJS plan: {0}")]
    WriteSemanticPlan(#[source] SemanticPlanWriteError),
    /// Plan-sensitive final compilation failed.
    #[error("RSJS final compilation failed")]
    FinalCargo(#[source] FinalCargoError),
}

#[cfg(test)]
mod tests;
