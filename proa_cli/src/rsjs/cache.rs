// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Explicit, coordinate-addressed reuse for descriptor-phase Cargo builds.
//!
//! Cargo still owns freshness. This cache never discovers outputs by walking a
//! target directory: every reuse decision starts from the exact
//! `compiler-artifact` messages emitted by the current Cargo invocation. Each
//! cached unit's rustc-observed source/environment inputs and exact
//! `--extern` bytes are validated before a descriptor object is admitted into
//! the linked invocation. Package-local files that are not inputs to that unit
//! remain under Cargo and build-script freshness tracking, so editing docs,
//! styles, or other unrelated assets does not invalidate the entire descriptor
//! target. Cached descriptor objects remain in their immutable,
//! content-addressed cache slots under the retained cache lease instead of
//! being copied into fresh staging. Cargo's `fresh` bit is authoritative for
//! other target outputs; the cache deliberately does not reread every emitted
//! file.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2_accelerated::{Digest, Sha256};
use thiserror::Error;

use crate::rsjs::build::wrapper_control_environment;
use crate::rsjs::build::{
    acquire_descriptor_target_lease, classify_dependency_artifact, descriptor_input_link_hashes,
    descriptor_package_source_closure_hash, descriptor_staging_marker_is_valid,
    descriptor_target_dirty_journal_path, descriptor_target_marker_is_valid,
    join_cargo_enumerated_descriptor_units, join_descriptor_units_to_cargo_artifacts,
    managed_paths_overlap, normalize_managed_path_identity, prepare_descriptor_target_directory,
    run_descriptor_cargo_allow_empty, run_descriptor_cargo_allow_empty_resuming,
    uplifted_root_lib_hashed_rlib, validate_descriptor_managed_paths,
    validate_descriptor_observed_inputs, CargoCompilerArtifact, DescriptorCargoOutput,
    DescriptorCargoRequest, DescriptorCargoRun, DescriptorCargoRunError, DescriptorCargoUnitId,
    DescriptorInputHashes, DescriptorObjectSnapshot, DescriptorSidecarError, DescriptorTargetLease,
    DescriptorUnitSidecar, UnitArtifactJoinError, ValidatedDescriptorUnit,
    DESCRIPTOR_OBJECT_HASH_DOMAIN,
};
#[cfg(test)]
use crate::rsjs::build::{
    descriptor_environment_dependency_value_hash, descriptor_input_content_hash,
    descriptor_object_content_hash, descriptor_out_dir_dependency_content_hash,
    descriptor_source_dependency_content_hash, DescriptorEnvironmentDependency,
    DescriptorOutDirDependency, DescriptorSourceDependency,
};

pub(crate) const DESCRIPTOR_CACHE_SCHEMA_VERSION: u16 = 5;
const ARTIFACT_COORDINATE_DOMAIN: &[u8] = b"proa.rsjs.descriptor-cache-coordinate.v2";
const ARTIFACT_SET_COORDINATE_DOMAIN: &[u8] = b"proa.rsjs.descriptor-cache-coordinate-set.v2";
const CACHED_SIDECAR_HASH_DOMAIN: &[u8] = b"proa.rsjs.descriptor-cache-sidecar.v1";
static CACHE_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

/// Run Cargo's descriptor compilation and reuse exact units from an explicit
/// persistent cache when Cargo reports artifacts as fresh.
///
/// `request.staging_directory` remains fresh and invocation-local during
/// ordinary builds. Crash recovery instead resumes only the exact
/// marker-owned staging root named by the target journal and gives each retry
/// a fresh child writer directory. The current Cargo JSON stream authorizes
/// every cache hit, whose object is then validated in place and retained under
/// the returned cache lease.
pub fn run_descriptor_cargo_cached(
    request: &DescriptorCargoRequest,
    cache_directory: &Path,
) -> Result<CachedDescriptorCargoLease, DescriptorCacheError> {
    run_descriptor_cargo_cached_leased(request, cache_directory)
}

/// Cache/descriptor-target lease retained by the linked pipeline until every
/// shared target byte has been consumed into an immutable plan.
#[must_use = "descriptor cache leases retain filesystem locks until consumed or dropped"]
pub struct CachedDescriptorCargoLease {
    output: DescriptorCargoOutput,
    // Fields drop in declaration order. Release the cache lock before the
    // outer target lock, reversing acquisition order.
    _cache_lock: CacheGenerationLock,
    _target_lock: DescriptorTargetLease,
}

impl fmt::Debug for CachedDescriptorCargoLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachedDescriptorCargoLease")
            .field("artifacts", &self.output.cargo_artifacts.len())
            .field("units", &self.output.units.len())
            .finish_non_exhaustive()
    }
}

impl crate::rsjs::build::descriptor_run_sealed::Sealed for CachedDescriptorCargoLease {
    fn output(&self) -> &DescriptorCargoOutput {
        &self.output
    }

    fn into_output(self) -> DescriptorCargoOutput {
        self.output
    }
}

impl DescriptorCargoRun for CachedDescriptorCargoLease {}

pub(crate) fn run_descriptor_cargo_cached_leased(
    request: &DescriptorCargoRequest,
    cache_directory: &Path,
) -> Result<CachedDescriptorCargoLease, DescriptorCacheError> {
    validate_descriptor_managed_paths(request).map_err(DescriptorCacheError::DescriptorCargo)?;
    let cache_directory = validate_cache_directories(
        cache_directory,
        &request.staging_directory,
        &request.target_directory,
        &request.source_exclusion_directory,
        &request.working_directory,
    )?;
    let target_lock = acquire_descriptor_target_lease(&request.target_directory)
        .map_err(DescriptorCacheError::DescriptorCargo)?;
    ensure_plain_directory(&cache_directory)?;
    let cache_lock = CacheGenerationLock::acquire_path(&cache_directory.join("generation.lock"))?;
    create_cache_layout(&cache_directory)?;
    prepare_descriptor_target_directory(&request.target_directory)
        .map_err(DescriptorCacheError::DescriptorCargo)?;
    let interrupted_staging = interrupted_cache_generation_staging(&cache_directory, request)?;
    let mut active_request = request.clone();
    if let Some(staging) = &interrupted_staging {
        active_request.staging_directory = staging.clone();
        eprintln!(
            "recovering interrupted RSJS descriptor cache transaction from `{}` while preserving Cargo target `{}`",
            staging.display(),
            request.target_directory.display(),
        );
    } else {
        mark_cache_generation_dirty(request)?;
    }

    let result = match run_descriptor_cargo_cached_once(
        &active_request,
        &cache_directory,
        false,
        interrupted_staging.is_some(),
    ) {
        Ok(output) => Ok(output),
        Err(error)
            if recoverable_cache_authority_error(&error)
                || (interrupted_staging.is_some()
                    && recoverable_interrupted_generation_error(&error)) =>
        {
            eprintln!(
                "RSJS descriptor cache recovery could not prove exact reuse ({error}); rebuilding its marker-owned Cargo target once"
            );
            reset_descriptor_request_owned_state(
                &active_request,
                &active_request.staging_directory,
            )?;
            mark_cache_generation_dirty(request)?;
            run_descriptor_cargo_cached_once(request, &cache_directory, true, false)
        }
        Err(error) => Err(error),
    };
    match result {
        Ok(output) => {
            clear_cache_generation_dirty(request)?;
            Ok(CachedDescriptorCargoLease {
                output,
                _cache_lock: cache_lock,
                _target_lock: target_lock,
            })
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirtyCacheGeneration {
    schema_version: u16,
    target_directory: PathBuf,
    staging_directory: PathBuf,
}

fn interrupted_cache_generation_staging(
    cache_directory: &Path,
    request: &DescriptorCargoRequest,
) -> Result<Option<PathBuf>, DescriptorCacheError> {
    let path = descriptor_target_dirty_journal_path(&request.target_directory)
        .map_err(DescriptorCacheError::DescriptorCargo)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(DescriptorCacheError::ReadCacheFile { path, source }),
    };
    if !metadata.file_type().is_file() {
        return Err(DescriptorCacheError::InvalidDirtyMarker(path));
    }
    let bytes = fs::read(&path).map_err(|source| DescriptorCacheError::ReadCacheFile {
        path: path.clone(),
        source,
    })?;
    let marker: DirtyCacheGeneration =
        serde_json::from_slice(&bytes).map_err(|source| DescriptorCacheError::ParseCacheFile {
            path: path.clone(),
            source,
        })?;
    if marker.schema_version != DESCRIPTOR_CACHE_SCHEMA_VERSION {
        return Err(DescriptorCacheError::UnsupportedCacheSchema {
            path,
            actual: marker.schema_version,
            expected: DESCRIPTOR_CACHE_SCHEMA_VERSION,
        });
    }
    let expected = normalize_managed_path(&request.target_directory)?;
    let actual = normalize_managed_path(&marker.target_directory)?;
    if actual != expected {
        return Err(DescriptorCacheError::DirtyTargetMismatch { expected, actual });
    }
    validate_cache_directories(
        cache_directory,
        &marker.staging_directory,
        &request.target_directory,
        &request.source_exclusion_directory,
        &request.working_directory,
    )?;
    let normalized_staging = normalize_managed_path(&marker.staging_directory)?;
    let normalized_exclusion = normalize_managed_path(&request.source_exclusion_directory)?;
    let relative = normalized_staging
        .strip_prefix(&normalized_exclusion)
        .map_err(
            |_| DescriptorCacheError::GeneratedStateOutsideSourceExclusion {
                kind: "staging",
                path: normalized_staging.clone(),
                exclusion: normalized_exclusion,
            },
        )?;
    let request_spelling = request.source_exclusion_directory.join(relative);
    let rebound = normalize_managed_path(&request_spelling)?;
    if rebound != normalized_staging {
        return Err(DescriptorCacheError::DirtyStagingMismatch {
            expected: normalized_staging,
            actual: rebound,
        });
    }
    Ok(Some(request_spelling))
}

fn mark_cache_generation_dirty(
    request: &DescriptorCargoRequest,
) -> Result<(), DescriptorCacheError> {
    let marker = DirtyCacheGeneration {
        schema_version: DESCRIPTOR_CACHE_SCHEMA_VERSION,
        target_directory: normalize_managed_path(&request.target_directory)?,
        staging_directory: normalize_managed_path(&request.staging_directory)?,
    };
    let bytes =
        serde_json::to_vec_pretty(&marker).map_err(DescriptorCacheError::SerializeCacheRecord)?;
    let path = descriptor_target_dirty_journal_path(&request.target_directory)
        .map_err(DescriptorCacheError::DescriptorCargo)?;
    atomic_write_replaceable(&path, &bytes)
}

fn clear_cache_generation_dirty(
    request: &DescriptorCargoRequest,
) -> Result<(), DescriptorCacheError> {
    let path = descriptor_target_dirty_journal_path(&request.target_directory)
        .map_err(DescriptorCacheError::DescriptorCargo)?;
    fs::remove_file(&path).map_err(|source| DescriptorCacheError::WriteCacheFile {
        path: path.clone(),
        source,
    })?;
    let parent = path
        .parent()
        .expect("descriptor target journals always have a parent");
    sync_directory(parent).map_err(|source| DescriptorCacheError::WriteCacheFile {
        path: parent.to_path_buf(),
        source,
    })
}

fn run_descriptor_cargo_cached_once(
    request: &DescriptorCargoRequest,
    cache_directory: &Path,
    require_complete_rebuild: bool,
    resume_interrupted_staging: bool,
) -> Result<DescriptorCargoOutput, DescriptorCacheError> {
    let mut timing = super::timing::StageTimer::new();
    let mut fresh = if resume_interrupted_staging {
        run_descriptor_cargo_allow_empty_resuming(request)
    } else {
        run_descriptor_cargo_allow_empty(request)
    }
    .map_err(DescriptorCacheError::DescriptorCargo)?;
    if require_complete_rebuild && fresh.cargo_artifacts.iter().any(|artifact| artifact.fresh) {
        return Err(DescriptorCacheError::RecoveryDidNotFullyRebuild);
    }
    if resume_interrupted_staging {
        retain_current_interrupted_units(&mut fresh, request)?;
    }
    timing.mark("descriptor Cargo and sidecars");
    let evidence = collect_artifact_evidence(&fresh.cargo_artifacts)?;
    let artifact_set_coordinate = artifact_set_coordinate(&evidence);
    if fresh.units.is_empty() {
        reject_rebuilt_artifact_without_wrapper_units(&evidence)?;
    }

    let fresh_joined = join_cargo_enumerated_descriptor_units(&fresh.units, &fresh.cargo_artifacts)
        .map_err(DescriptorCacheError::ArtifactJoin)?;
    let mut fresh_by_artifact = BTreeMap::<String, DescriptorCargoUnitId>::new();
    for (unit, artifact) in &fresh_joined {
        let coordinate = coordinate_for_artifact(artifact, &evidence)?;
        if let Some(previous) = fresh_by_artifact.insert(coordinate.clone(), *unit) {
            return Err(DescriptorCacheError::MultipleFreshUnitsForArtifact {
                coordinate,
                first: previous,
                second: *unit,
            });
        }
    }

    let fresh_ids = fresh
        .units
        .iter()
        .map(|unit| unit.unit)
        .collect::<BTreeSet<_>>();
    let mut cached_generations = BTreeMap::new();
    let mut expected_artifact_by_cached_unit = BTreeMap::new();

    if fresh.units.is_empty() && !require_complete_rebuild {
        let snapshot = load_snapshot(cache_directory, &artifact_set_coordinate, &evidence)?;
        for generation in snapshot.units {
            insert_cached_generation(&mut cached_generations, generation)?;
        }
    } else {
        for artifact in &evidence {
            if fresh_by_artifact.contains_key(&artifact.coordinate) {
                continue;
            }

            match load_artifact_index(cache_directory, &artifact.coordinate) {
                Ok(index) => {
                    if !artifact.artifact.fresh && !index.units.is_empty() {
                        return Err(DescriptorCacheError::RebuiltArtifactMissingFreshUnit {
                            coordinate: artifact.coordinate.clone(),
                        });
                    }
                    for generation in index.units {
                        let unit = generation.unit(Path::new("artifact-index"))?;
                        if let Some(previous) = expected_artifact_by_cached_unit
                            .insert(unit, artifact.coordinate.clone())
                        {
                            if previous != artifact.coordinate {
                                return Err(
                                    DescriptorCacheError::CachedUnitClaimedByMultipleArtifacts {
                                        unit,
                                        first: previous,
                                        second: artifact.coordinate.clone(),
                                    },
                                );
                            }
                        }
                        insert_cached_generation(&mut cached_generations, generation)?;
                    }
                }
                Err(DescriptorCacheError::MissingArtifactIndex { .. })
                    if !artifact.artifact.fresh =>
                {
                    // A freshly rebuilt Cargo artifact with no joined wrapper
                    // unit is a confirmed negative entry (for example a Cargo
                    // unit with no distinct compiler-artifact/wrapper pair).
                    // It is persisted below, never guessed on a future fresh
                    // invocation.
                }
                Err(error) => return Err(error),
            }
        }
    }

    timing.mark("cache artifact indexes");
    cached_generations.retain(|unit, _| !fresh_ids.contains(unit));
    let cached_ids = cached_generations.keys().copied().collect::<BTreeSet<_>>();
    let fresh_generations = persist_fresh_units(cache_directory, &fresh.units)?;
    let mut generations = cached_generations;
    for (_, generation) in fresh_generations {
        insert_cached_generation(&mut generations, generation)?;
    }
    // Both rebuilt and reused units are now immutable cache generations. This
    // makes every path in the returned output independent of nonce staging and
    // lets successful invocations remove staging before releasing the lease.
    timing.mark("cache persist rebuilt objects");
    let (units, objects) = load_cached_units_with_objects(cache_directory, &generations)?;
    timing.mark("cache load and hash objects");
    let units_by_id = units
        .iter()
        .map(|unit| (unit.unit, unit))
        .collect::<BTreeMap<_, _>>();
    for expected in fresh_ids.iter().chain(cached_ids.iter()) {
        if !units_by_id.contains_key(expected) {
            return Err(DescriptorCacheError::MissingMergedUnit(*expected));
        }
    }

    let validation_ids = if require_complete_rebuild || resume_interrupted_staging {
        units_by_id.keys().copied().collect::<BTreeSet<_>>()
    } else {
        cached_ids.clone()
    };
    let input_hashes = validate_cached_unit_inputs(
        &units_by_id,
        &validation_ids,
        &request.target_directory,
        &fresh.cargo_artifacts,
    )?;
    timing.mark("cache validate observed inputs and externs");
    validate_cached_artifact_joins(
        &units_by_id,
        &cached_ids,
        &expected_artifact_by_cached_unit,
        &fresh.cargo_artifacts,
        &evidence,
        fresh.units.is_empty(),
    )?;

    if let Some(unit) = units_by_id
        .keys()
        .find(|unit| !generations.contains_key(unit))
    {
        return Err(DescriptorCacheError::MissingCachedGeneration(*unit));
    }
    timing.mark("cache validate artifact joins");
    persist_artifact_indexes(
        cache_directory,
        &units,
        &generations,
        &fresh.cargo_artifacts,
        &evidence,
    )?;
    persist_snapshot(
        cache_directory,
        &artifact_set_coordinate,
        &evidence,
        units.iter().map(|unit| generations[&unit.unit].clone()),
    )?;
    remove_completed_staging(&request.staging_directory)?;

    timing.mark("cache publish indexes and cleanup");
    Ok(DescriptorCargoOutput {
        cargo_artifacts: fresh.cargo_artifacts,
        units,
        objects,
        input_hashes,
    })
}

fn retain_current_interrupted_units(
    output: &mut DescriptorCargoOutput,
    request: &DescriptorCargoRequest,
) -> Result<(), DescriptorCacheError> {
    let candidates = std::mem::take(&mut output.units);
    let candidate_count = candidates.len();
    let mut retained = BTreeMap::<DescriptorCargoUnitId, ValidatedDescriptorUnit>::new();
    let mut stale = 0usize;
    for unit in candidates {
        if interrupted_unit_matches_current_inputs(
            &unit,
            &request.target_directory,
            &request.source_exclusion_directory,
            &output.cargo_artifacts,
        )? {
            if let Some(previous) = retained.get(&unit.unit) {
                if previous.sidecar != unit.sidecar {
                    return Err(DescriptorCacheError::ConflictingInterruptedUnitCandidates(
                        unit.unit,
                    ));
                }
            } else {
                retained.insert(unit.unit, unit);
            }
        } else {
            stale += 1;
        }
    }
    if stale != 0 {
        eprintln!(
            "discarded {stale} stale descriptor recovery candidate{} after current-input validation",
            if stale == 1 { "" } else { "s" },
        );
    }
    let duplicate_count = candidate_count.saturating_sub(stale + retained.len());
    if duplicate_count != 0 {
        eprintln!(
            "deduplicated {duplicate_count} exact descriptor recovery candidate{}",
            if duplicate_count == 1 { "" } else { "s" },
        );
    }
    output.units = retained.into_values().collect();
    Ok(())
}

fn interrupted_unit_matches_current_inputs(
    unit: &ValidatedDescriptorUnit,
    descriptor_target: &Path,
    source_exclusion: &Path,
    cargo_artifacts: &[CargoCompilerArtifact],
) -> Result<bool, DescriptorCacheError> {
    if !interrupted_unit_matches_local_inputs(unit, descriptor_target, source_exclusion) {
        return Ok(false);
    }

    let units_by_id = BTreeMap::from([(unit.unit, unit)]);
    let unit_ids = BTreeSet::from([unit.unit]);
    match validate_cached_dependency_artifacts(&units_by_id, &unit_ids, cargo_artifacts) {
        Ok(_) => {}
        Err(
            DescriptorCacheError::CachedDependencyNotEnumerated { .. }
            | DescriptorCacheError::StaleCachedDependency { .. },
        ) => return Ok(false),
        Err(error) => return Err(error),
    }
    match join_descriptor_units_to_cargo_artifacts(std::slice::from_ref(unit), cargo_artifacts) {
        Ok(_) => Ok(true),
        Err(UnitArtifactJoinError::MissingArtifact(_)) => Err(
            DescriptorCacheError::InterruptedUnitMissingArtifact(unit.unit),
        ),
        Err(error) => Err(DescriptorCacheError::ArtifactJoin(error)),
    }
}

fn interrupted_unit_matches_local_inputs(
    unit: &ValidatedDescriptorUnit,
    descriptor_target: &Path,
    source_exclusion: &Path,
) -> bool {
    let Some(manifest) = unit.sidecar.cargo_manifest_path.as_deref() else {
        return false;
    };
    let Some(package_root) = manifest.parent() else {
        return false;
    };
    let current_closure =
        match descriptor_package_source_closure_hash(package_root, source_exclusion) {
            Ok(hash) => hash,
            Err(_) => return false,
        };
    if current_closure != unit.sidecar.package_source_closure_hash {
        return false;
    }
    validate_descriptor_observed_inputs(descriptor_target, &unit.sidecar).is_ok()
}

fn reject_rebuilt_artifact_without_wrapper_units(
    evidence: &[ArtifactEvidence],
) -> Result<(), DescriptorCacheError> {
    if let Some(artifact) = evidence.iter().find(|artifact| !artifact.artifact.fresh) {
        return Err(DescriptorCacheError::RebuiltArtifactMissingFreshUnit {
            coordinate: artifact.coordinate.clone(),
        });
    }
    Ok(())
}

fn insert_cached_generation(
    generations: &mut BTreeMap<DescriptorCargoUnitId, CachedUnitGeneration>,
    generation: CachedUnitGeneration,
) -> Result<(), DescriptorCacheError> {
    let unit = generation.unit(Path::new("cache-generation"))?;
    if let Some(previous) = generations.insert(unit, generation.clone()) {
        if previous != generation {
            return Err(DescriptorCacheError::ConflictingCachedGeneration {
                unit,
                first: cached_generation_label(&previous),
                second: cached_generation_label(&generation),
            });
        }
    }
    Ok(())
}

fn validate_cache_directories(
    cache_directory: &Path,
    staging_directory: &Path,
    target_directory: &Path,
    source_exclusion_directory: &Path,
    working_directory: &Path,
) -> Result<PathBuf, DescriptorCacheError> {
    if !cache_directory.is_absolute() {
        return Err(DescriptorCacheError::RelativeCacheDirectory(
            cache_directory.to_path_buf(),
        ));
    }
    for (name, path) in [
        ("staging", staging_directory),
        ("target", target_directory),
        ("source exclusion", source_exclusion_directory),
        ("working directory", working_directory),
    ] {
        if !path.is_absolute() {
            return Err(DescriptorCacheError::RelativeManagedDirectory {
                kind: name,
                path: path.to_path_buf(),
            });
        }
    }
    let cache = normalize_managed_path(cache_directory)?;
    let staging = normalize_managed_path(staging_directory)?;
    let target = normalize_managed_path(target_directory)?;
    let source_exclusion = normalize_managed_path(source_exclusion_directory)?;
    let working = normalize_managed_path(working_directory)?;

    for (first_kind, first, second_kind, second) in [
        ("cache", &cache, "staging", &staging),
        ("cache", &cache, "target", &target),
        ("staging", &staging, "target", &target),
    ] {
        if managed_paths_overlap(first, second) {
            return Err(DescriptorCacheError::OverlappingManagedDirectories {
                first_kind,
                first: first.clone(),
                second_kind,
                second: second.clone(),
            });
        }
    }
    for (kind, managed) in [("staging", &staging), ("target", &target)] {
        if working == *managed || working.starts_with(managed) {
            return Err(DescriptorCacheError::UnsafeManagedDirectory {
                kind,
                managed: managed.clone(),
                working: working.clone(),
            });
        }
    }
    if target == source_exclusion || !target.starts_with(&source_exclusion) {
        return Err(DescriptorCacheError::TargetOutsideSourceExclusion {
            target,
            exclusion: source_exclusion,
        });
    }
    if source_exclusion == working || working.starts_with(&source_exclusion) {
        return Err(DescriptorCacheError::UnsafeSourceExclusion {
            exclusion: source_exclusion,
            working,
        });
    }
    if source_exclusion.starts_with(&working) {
        for (kind, managed) in [
            ("cache", &cache),
            ("staging", &staging),
            ("target", &target),
        ] {
            if !managed.starts_with(&source_exclusion) {
                return Err(DescriptorCacheError::GeneratedStateOutsideSourceExclusion {
                    kind,
                    path: managed.clone(),
                    exclusion: source_exclusion.clone(),
                });
            }
        }
    }
    Ok(cache)
}

fn normalize_managed_path(path: &Path) -> Result<PathBuf, DescriptorCacheError> {
    normalize_managed_path_identity(path).map_err(|source| {
        DescriptorCacheError::NormalizeManagedDirectory {
            path: path.to_path_buf(),
            source,
        }
    })
}

struct CacheGenerationLock {
    _file: File,
}

impl CacheGenerationLock {
    fn acquire_path(path: &Path) -> Result<Self, DescriptorCacheError> {
        let parent = path
            .parent()
            .ok_or_else(|| DescriptorCacheError::CachePathWithoutParent(path.to_path_buf()))?;
        ensure_plain_directory(parent)?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf()));
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(DescriptorCacheError::OpenCacheLock {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|source| DescriptorCacheError::OpenCacheLock {
                path: path.to_path_buf(),
                source,
            })?;
        file.lock()
            .map_err(|source| DescriptorCacheError::AcquireCacheLock {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self { _file: file })
    }
}

fn recoverable_cache_authority_error(error: &DescriptorCacheError) -> bool {
    matches!(
        error,
        DescriptorCacheError::MissingArtifactSetSnapshot { .. }
            | DescriptorCacheError::MissingArtifactIndex { .. }
            | DescriptorCacheError::MissingCachedUnitFile { .. }
            | DescriptorCacheError::CachedOutDirDependencyMismatch { .. }
    )
}

fn recoverable_interrupted_generation_error(error: &DescriptorCacheError) -> bool {
    matches!(
        error,
        DescriptorCacheError::MultipleFreshUnitsForArtifact { .. }
            | DescriptorCacheError::RebuiltArtifactMissingFreshUnit { .. }
            | DescriptorCacheError::ConflictingInterruptedUnitCandidates(_)
            | DescriptorCacheError::InterruptedUnitMissingArtifact(_)
            | DescriptorCacheError::ArtifactJoin(_)
    )
}

fn reset_descriptor_request_owned_state(
    request: &DescriptorCargoRequest,
    staging_directory: &Path,
) -> Result<(), DescriptorCacheError> {
    let marker_valid =
        descriptor_target_marker_is_valid(&request.target_directory).map_err(|source| {
            DescriptorCacheError::ReadDescriptorTargetMarker {
                path: request.target_directory.clone(),
                source,
            }
        })?;
    if !marker_valid {
        return Err(DescriptorCacheError::UnownedRecoveryTarget(
            request.target_directory.clone(),
        ));
    }
    let staging_exists = fs::symlink_metadata(staging_directory)
        .map(|metadata| metadata.file_type().is_dir())
        .or_else(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(source)
            }
        })
        .map_err(|source| DescriptorCacheError::ReadDescriptorStagingMarker {
            path: staging_directory.to_path_buf(),
            source,
        })?;
    if staging_exists {
        let staging_owned =
            descriptor_staging_marker_is_valid(staging_directory).map_err(|source| {
                DescriptorCacheError::ReadDescriptorStagingMarker {
                    path: staging_directory.to_path_buf(),
                    source,
                }
            })?;
        if !staging_owned {
            return Err(DescriptorCacheError::UnownedRecoveryStaging(
                staging_directory.to_path_buf(),
            ));
        }
        remove_exact_owned_directory(staging_directory, "staging")?;
    }
    remove_exact_owned_directory(&request.target_directory, "target")
}

fn remove_completed_staging(staging_directory: &Path) -> Result<(), DescriptorCacheError> {
    let staging_owned =
        descriptor_staging_marker_is_valid(staging_directory).map_err(|source| {
            DescriptorCacheError::ReadDescriptorStagingMarker {
                path: staging_directory.to_path_buf(),
                source,
            }
        })?;
    if !staging_owned {
        return Err(DescriptorCacheError::UnownedRecoveryStaging(
            staging_directory.to_path_buf(),
        ));
    }
    remove_exact_owned_directory(staging_directory, "completed staging")
}

fn remove_exact_owned_directory(
    path: &Path,
    kind: &'static str,
) -> Result<(), DescriptorCacheError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(DescriptorCacheError::RemoveRecoveryDirectory {
                kind,
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(DescriptorCacheError::UnsafeRecoveryDirectory {
            kind,
            path: path.to_path_buf(),
        });
    }
    fs::remove_dir_all(path).map_err(|source| DescriptorCacheError::RemoveRecoveryDirectory {
        kind,
        path: path.to_path_buf(),
        source,
    })
}

fn create_cache_layout(cache_directory: &Path) -> Result<(), DescriptorCacheError> {
    for relative in ["units", "objects", "artifacts", "snapshots"] {
        let path = cache_directory.join(relative);
        ensure_plain_directory(&path)?;
    }
    Ok(())
}

fn ensure_plain_directory(path: &Path) -> Result<(), DescriptorCacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => return Ok(()),
        Ok(_) => return Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DescriptorCacheError::CreateCacheDirectory {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    fs::create_dir_all(path).map_err(|source| DescriptorCacheError::CreateCacheDirectory {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        DescriptorCacheError::CreateCacheDirectory {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf()))
    }
}

fn require_plain_directory(path: &Path) -> Result<(), DescriptorCacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf())),
        Err(source) => Err(DescriptorCacheError::ReadCacheFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[derive(Clone, Debug)]
struct ArtifactEvidence {
    artifact: CargoCompilerArtifact,
    coordinate: String,
}

fn collect_artifact_evidence(
    artifacts: &[CargoCompilerArtifact],
) -> Result<Vec<ArtifactEvidence>, DescriptorCacheError> {
    let mut evidence = Vec::with_capacity(artifacts.len());
    let mut seen = BTreeSet::new();
    for artifact in artifacts {
        let mut hash = CacheHasher::new(ARTIFACT_COORDINATE_DOMAIN);
        hash.field("package_id", artifact.package_id.as_bytes());
        hash.path("manifest_path", &artifact.manifest_path)?;
        hash.field("target_name", artifact.target.name.as_bytes());
        hash.set(
            "target_kind",
            artifact.target.kind.iter().map(String::as_bytes),
        );
        hash.set(
            "crate_types",
            artifact.target.crate_types.iter().map(String::as_bytes),
        );
        hash.path("source_path", &artifact.target.src_path)?;
        hash.field("edition", artifact.target.edition.as_bytes());
        hash.field("profile", &canonical_json(&artifact.profile));
        hash.set("features", artifact.features.iter().map(String::as_bytes));

        hash.field(
            "filename_count",
            &(artifact.filenames.len() as u64).to_be_bytes(),
        );
        for path in &artifact.filenames {
            hash.path("filename", path)?;
        }
        match &artifact.executable {
            Some(path) => {
                hash.field("executable_present", b"1");
                hash.path("executable", path)?;
            }
            None => hash.field("executable_present", b"0"),
        }

        let coordinate = hash.finish();
        if !seen.insert(coordinate.clone()) {
            return Err(DescriptorCacheError::DuplicateArtifactCoordinate(
                coordinate,
            ));
        }
        evidence.push(ArtifactEvidence {
            artifact: artifact.clone(),
            coordinate,
        });
    }
    evidence.sort_by(|left, right| left.coordinate.cmp(&right.coordinate));
    Ok(evidence)
}

fn artifact_set_coordinate(evidence: &[ArtifactEvidence]) -> String {
    let mut hash = CacheHasher::new(ARTIFACT_SET_COORDINATE_DOMAIN);
    hash.field("artifact_count", &(evidence.len() as u64).to_be_bytes());
    for artifact in evidence {
        hash.field("artifact", artifact.coordinate.as_bytes());
    }
    hash.finish()
}

fn coordinate_for_artifact(
    artifact: &CargoCompilerArtifact,
    evidence: &[ArtifactEvidence],
) -> Result<String, DescriptorCacheError> {
    let candidates = evidence
        .iter()
        .filter(|candidate| candidate.artifact == *artifact)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [candidate] => Ok(candidate.coordinate.clone()),
        [] => Err(DescriptorCacheError::ArtifactMissingFromCurrentEvidence),
        _ => Err(DescriptorCacheError::AmbiguousCurrentArtifactEvidence),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactCacheIndex {
    schema_version: u16,
    artifact_coordinate: String,
    units: Vec<CachedUnitGeneration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactSetSnapshot {
    schema_version: u16,
    artifact_set_coordinate: String,
    artifact_coordinates: Vec<String>,
    units: Vec<CachedUnitGeneration>,
}

/// Immutable cache generation selected by an artifact/snapshot pointer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct CachedUnitGeneration {
    cargo_unit_id: String,
    descriptor_object_hash: String,
    sidecar_hash: String,
}

impl CachedUnitGeneration {
    fn unit(&self, path: &Path) -> Result<DescriptorCargoUnitId, DescriptorCacheError> {
        DescriptorCargoUnitId::from_hex(&self.cargo_unit_id).map_err(|source| {
            DescriptorCacheError::InvalidCachedUnitId {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    fn validate_hashes(&self, path: &Path) -> Result<(), DescriptorCacheError> {
        validate_cache_hash(&self.descriptor_object_hash).map_err(|reason| {
            DescriptorCacheError::InvalidCachedGenerationHash {
                path: path.to_path_buf(),
                field: "descriptor_object_hash",
                value: self.descriptor_object_hash.clone(),
                reason,
            }
        })?;
        validate_cache_hash(&self.sidecar_hash).map_err(|reason| {
            DescriptorCacheError::InvalidCachedGenerationHash {
                path: path.to_path_buf(),
                field: "sidecar_hash",
                value: self.sidecar_hash.clone(),
                reason,
            }
        })
    }
}

fn load_artifact_index(
    cache_directory: &Path,
    coordinate: &str,
) -> Result<ParsedArtifactIndex, DescriptorCacheError> {
    let path = cache_directory
        .join("artifacts")
        .join(format!("{coordinate}.json"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(DescriptorCacheError::UnsafeCacheEntry(path));
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(DescriptorCacheError::MissingArtifactIndex {
                coordinate: coordinate.to_string(),
            });
        }
        Err(source) => return Err(DescriptorCacheError::ReadCacheFile { path, source }),
    }
    let bytes = fs::read(&path).map_err(|source| DescriptorCacheError::ReadCacheFile {
        path: path.clone(),
        source,
    })?;
    let index: ArtifactCacheIndex =
        serde_json::from_slice(&bytes).map_err(|source| DescriptorCacheError::ParseCacheFile {
            path: path.clone(),
            source,
        })?;
    if index.schema_version != DESCRIPTOR_CACHE_SCHEMA_VERSION {
        return Err(DescriptorCacheError::UnsupportedCacheSchema {
            path,
            actual: index.schema_version,
            expected: DESCRIPTOR_CACHE_SCHEMA_VERSION,
        });
    }
    if index.artifact_coordinate != coordinate {
        return Err(DescriptorCacheError::CacheCoordinateMismatch {
            path,
            expected: coordinate.to_string(),
            actual: index.artifact_coordinate,
        });
    }
    Ok(ParsedArtifactIndex {
        units: parse_unique_generations(&path, index.units)?,
    })
}

struct ParsedArtifactIndex {
    units: Vec<CachedUnitGeneration>,
}

fn load_snapshot(
    cache_directory: &Path,
    coordinate: &str,
    evidence: &[ArtifactEvidence],
) -> Result<ParsedSnapshot, DescriptorCacheError> {
    let path = cache_directory
        .join("snapshots")
        .join(format!("{coordinate}.json"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(DescriptorCacheError::UnsafeCacheEntry(path));
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(DescriptorCacheError::MissingArtifactSetSnapshot {
                coordinate: coordinate.to_string(),
            });
        }
        Err(source) => return Err(DescriptorCacheError::ReadCacheFile { path, source }),
    }
    let bytes = fs::read(&path).map_err(|source| DescriptorCacheError::ReadCacheFile {
        path: path.clone(),
        source,
    })?;
    let snapshot: ArtifactSetSnapshot =
        serde_json::from_slice(&bytes).map_err(|source| DescriptorCacheError::ParseCacheFile {
            path: path.clone(),
            source,
        })?;
    if snapshot.schema_version != DESCRIPTOR_CACHE_SCHEMA_VERSION {
        return Err(DescriptorCacheError::UnsupportedCacheSchema {
            path,
            actual: snapshot.schema_version,
            expected: DESCRIPTOR_CACHE_SCHEMA_VERSION,
        });
    }
    if snapshot.artifact_set_coordinate != coordinate {
        return Err(DescriptorCacheError::CacheCoordinateMismatch {
            path,
            expected: coordinate.to_string(),
            actual: snapshot.artifact_set_coordinate,
        });
    }
    let expected_artifacts = evidence
        .iter()
        .map(|artifact| artifact.coordinate.clone())
        .collect::<Vec<_>>();
    if snapshot.artifact_coordinates != expected_artifacts {
        return Err(DescriptorCacheError::SnapshotArtifactSetMismatch(path));
    }
    Ok(ParsedSnapshot {
        units: parse_unique_generations(&path, snapshot.units)?,
    })
}

struct ParsedSnapshot {
    units: Vec<CachedUnitGeneration>,
}

fn parse_unique_generations(
    path: &Path,
    encoded: Vec<CachedUnitGeneration>,
) -> Result<Vec<CachedUnitGeneration>, DescriptorCacheError> {
    let mut units = Vec::with_capacity(encoded.len());
    let mut seen = BTreeSet::new();
    for generation in encoded {
        generation.validate_hashes(path)?;
        let unit = generation.unit(path)?;
        if !seen.insert(unit) {
            return Err(DescriptorCacheError::DuplicateCachedUnit {
                path: path.to_path_buf(),
                unit,
            });
        }
        units.push(generation);
    }
    Ok(units)
}

fn validate_cache_hash(value: &str) -> Result<(), String> {
    if value.len() != 64 {
        return Err(format!(
            "expected 64 lowercase hexadecimal characters, got {}",
            value.len()
        ));
    }
    if let Some((offset, byte)) = value
        .bytes()
        .enumerate()
        .find(|(_, byte)| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "invalid byte `{}` at offset {offset}",
            char::from(byte)
        ));
    }
    Ok(())
}

#[cfg(test)]
fn load_cached_units_direct(
    cache_directory: &Path,
    units: &BTreeMap<DescriptorCargoUnitId, CachedUnitGeneration>,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorCacheError> {
    load_cached_units_with_objects(cache_directory, units).map(|(units, _)| units)
}

fn load_cached_units_with_objects(
    cache_directory: &Path,
    units: &BTreeMap<DescriptorCargoUnitId, CachedUnitGeneration>,
) -> Result<
    (
        Vec<ValidatedDescriptorUnit>,
        BTreeMap<PathBuf, DescriptorObjectSnapshot>,
    ),
    DescriptorCacheError,
> {
    let mut cached = Vec::with_capacity(units.len());
    for (unit, generation) in units {
        let sidecar_source = cached_sidecar_path(cache_directory, generation);
        let object_source = cached_object_path(cache_directory, generation);
        require_plain_directory(sidecar_source.parent().ok_or_else(|| {
            DescriptorCacheError::CachePathWithoutParent(sidecar_source.clone())
        })?)?;
        require_plain_directory(
            object_source.parent().ok_or_else(|| {
                DescriptorCacheError::CachePathWithoutParent(object_source.clone())
            })?,
        )?;
        require_file(&sidecar_source, *unit, CacheUnitFileKind::Sidecar)?;
        require_file(&object_source, *unit, CacheUnitFileKind::Object)?;
        let sidecar_bytes =
            fs::read(&sidecar_source).map_err(|source| DescriptorCacheError::ReadCacheFile {
                path: sidecar_source.clone(),
                source,
            })?;
        let actual_sidecar_hash = cached_sidecar_hash(&sidecar_bytes);
        if actual_sidecar_hash != generation.sidecar_hash {
            return Err(DescriptorCacheError::CachedSidecarHashMismatch {
                unit: *unit,
                expected: generation.sidecar_hash.clone(),
                actual: actual_sidecar_hash,
            });
        }
        let sidecar: DescriptorUnitSidecar =
            serde_json::from_slice(&sidecar_bytes).map_err(|source| {
                DescriptorCacheError::ParseCacheFile {
                    path: sidecar_source.clone(),
                    source,
                }
            })?;
        if sidecar.schema_version != crate::rsjs::build::DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION {
            return Err(DescriptorCacheError::CachedSidecarSchemaMismatch {
                path: sidecar_source,
                actual: sidecar.schema_version,
                expected: crate::rsjs::build::DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
            });
        }
        if sidecar.cargo_unit_id != unit.to_string() {
            return Err(DescriptorCacheError::CachedSidecarUnitMismatch {
                unit: *unit,
                actual: sidecar.cargo_unit_id,
            });
        }
        let expected_object = format!("objects/{unit}.o");
        if sidecar.descriptor_object != expected_object {
            return Err(DescriptorCacheError::CachedSidecarObjectPathMismatch {
                unit: *unit,
                actual: sidecar.descriptor_object,
                expected: expected_object,
            });
        }
        if sidecar.descriptor_object_hash != generation.descriptor_object_hash {
            return Err(DescriptorCacheError::CachedGenerationObjectHashMismatch {
                unit: *unit,
                pointer: generation.descriptor_object_hash.clone(),
                sidecar: sidecar.descriptor_object_hash,
            });
        }
        validate_cached_sidecar_semantics(*unit, &sidecar, &sidecar_source)?;
        cached.push(CachedUnitCandidate {
            unit: *unit,
            sidecar,
            sidecar_path: sidecar_source,
            descriptor_object: object_source,
            expected_object_hash: generation.descriptor_object_hash.clone(),
        });
    }

    // Hash the exact content-addressed cache objects under the retained cache
    // lease. Rayon preserves indexed collection order, so I/O failures and
    // mismatches are still reported deterministically by Cargo-unit ID below.
    let actual_hashes = cached
        .par_iter()
        .map(|unit| DescriptorObjectSnapshot::read(&unit.descriptor_object))
        .collect::<Vec<_>>();
    let mut validated = Vec::with_capacity(cached.len());
    let mut objects = BTreeMap::new();
    for (unit, snapshot) in cached.into_iter().zip(actual_hashes) {
        let snapshot = snapshot.map_err(|source| DescriptorCacheError::ReadCacheSource {
            path: unit.descriptor_object.clone(),
            source,
        })?;
        let actual_hash = encode_digest(&snapshot.descriptor_hash);
        if actual_hash != unit.expected_object_hash {
            return Err(DescriptorCacheError::DescriptorObjectHashMismatch {
                unit: unit.unit,
                expected: unit.expected_object_hash,
                actual: actual_hash,
            });
        }
        // Recheck the entry kind after the byte read. This rejects a
        // non-cooperating replacement with a directory or symlink before the
        // exact cache path is published to the linker.
        require_file(
            &unit.descriptor_object,
            unit.unit,
            CacheUnitFileKind::Object,
        )?;
        objects.insert(unit.descriptor_object.clone(), snapshot);
        validated.push(ValidatedDescriptorUnit {
            unit: unit.unit,
            sidecar: unit.sidecar,
            sidecar_path: unit.sidecar_path,
            descriptor_object: unit.descriptor_object,
        });
    }
    Ok((validated, objects))
}

struct CachedUnitCandidate {
    unit: DescriptorCargoUnitId,
    sidecar: DescriptorUnitSidecar,
    sidecar_path: PathBuf,
    descriptor_object: PathBuf,
    expected_object_hash: String,
}

fn validate_cached_sidecar_semantics(
    unit: DescriptorCargoUnitId,
    sidecar: &DescriptorUnitSidecar,
    path: &Path,
) -> Result<(), DescriptorCacheError> {
    let invalid = |reason| DescriptorCacheError::InvalidCachedSidecar {
        unit,
        path: path.to_path_buf(),
        reason,
    };

    if sidecar.normalized_rustc_inputs_hash != unit.to_string() {
        return Err(invalid(format!(
            "normalized rustc input hash `{}` does not equal the selected unit",
            sidecar.normalized_rustc_inputs_hash
        )));
    }
    for (name, value) in [
        (
            "logical environment hash",
            sidecar.logical_environment_hash.as_str(),
        ),
        (
            "realized environment hash",
            sidecar.realized_environment_hash.as_str(),
        ),
        (
            "package source-closure hash",
            sidecar.package_source_closure_hash.as_str(),
        ),
        (
            "descriptor object hash",
            sidecar.descriptor_object_hash.as_str(),
        ),
    ] {
        validate_cache_hash(value)
            .map_err(|reason| invalid(format!("invalid {name} `{value}`: {reason}")))?;
    }
    validate_cached_relative_path(&sidecar.dep_info_path)
        .map_err(|reason| invalid(format!("invalid target-relative dep-info path: {reason}")))?;
    if !sidecar.dep_info_working_directory.is_absolute() {
        return Err(invalid(
            "dep-info working directory is relative".to_string(),
        ));
    }
    match &sidecar.out_dir {
        Some(out_dir) => validate_cached_relative_path(out_dir)
            .map_err(|reason| invalid(format!("invalid OUT_DIR path `{out_dir}`: {reason}")))?,
        None if !sidecar.out_dir_dependencies.is_empty() => {
            return Err(invalid(
                "OUT_DIR dependencies are present without an OUT_DIR coordinate".to_string(),
            ));
        }
        None => {}
    }

    let mut previous_out_dir = None::<&str>;
    for dependency in &sidecar.out_dir_dependencies {
        validate_cached_relative_path(&dependency.relative_path).map_err(|reason| {
            invalid(format!(
                "invalid OUT_DIR dependency path `{}`: {reason}",
                dependency.relative_path
            ))
        })?;
        validate_cache_hash(&dependency.content_hash).map_err(|reason| {
            invalid(format!(
                "invalid OUT_DIR dependency hash for `{}`: {reason}",
                dependency.relative_path
            ))
        })?;
        if previous_out_dir.is_some_and(|previous| previous >= dependency.relative_path.as_str()) {
            return Err(invalid(
                "OUT_DIR dependencies are not strictly sorted and unique".to_string(),
            ));
        }
        previous_out_dir = Some(&dependency.relative_path);
    }

    let mut previous_source = None::<&Path>;
    for dependency in &sidecar.source_dependencies {
        if !dependency.absolute_path.is_absolute() {
            return Err(invalid(format!(
                "observed source path `{}` is relative",
                dependency.absolute_path.display()
            )));
        }
        validate_cache_hash(&dependency.content_hash).map_err(|reason| {
            invalid(format!(
                "invalid observed source hash for `{}`: {reason}",
                dependency.absolute_path.display()
            ))
        })?;
        if previous_source.is_some_and(|previous| previous >= dependency.absolute_path.as_path()) {
            return Err(invalid(
                "observed source dependencies are not strictly sorted and unique".to_string(),
            ));
        }
        previous_source = Some(&dependency.absolute_path);
    }

    let mut previous_environment = None::<&str>;
    for dependency in &sidecar.environment_dependencies {
        if dependency.name.is_empty()
            || dependency.name.bytes().any(|byte| byte == 0)
            || wrapper_control_environment(&dependency.name)
        {
            return Err(invalid(format!(
                "invalid observed environment dependency name `{}`",
                dependency.name
            )));
        }
        if let Some(value_hash) = &dependency.value_hash {
            validate_cache_hash(value_hash).map_err(|reason| {
                invalid(format!(
                    "invalid observed environment value hash for `{}`: {reason}",
                    dependency.name
                ))
            })?;
        }
        if previous_environment.is_some_and(|previous| previous >= dependency.name.as_str()) {
            return Err(invalid(
                "environment dependencies are not strictly sorted and unique".to_string(),
            ));
        }
        previous_environment = Some(&dependency.name);
    }
    if sidecar.out_dir.is_none()
        && sidecar
            .environment_dependencies
            .iter()
            .any(|dependency| dependency.name == "OUT_DIR" && dependency.value_hash.is_some())
    {
        return Err(invalid(
            "present OUT_DIR environment dependency has no OUT_DIR coordinate".to_string(),
        ));
    }

    for (extern_name, artifact) in &sidecar.dependency_artifacts {
        if extern_name.is_empty() {
            return Err(invalid("dependency name is empty".to_string()));
        }
        if !artifact.path.is_absolute() {
            return Err(invalid(format!(
                "dependency `{extern_name}` path `{}` is relative",
                artifact.path.display()
            )));
        }
        validate_cache_hash(&artifact.content_hash).map_err(|reason| {
            invalid(format!(
                "invalid dependency hash for `{extern_name}`: {reason}"
            ))
        })?;
    }
    if let Some(extra_filename) = &sidecar.cargo_extra_filename {
        validate_cached_extra_filename(extra_filename).map_err(|reason| {
            invalid(format!(
                "invalid Cargo extra-filename `{extra_filename}`: {reason}"
            ))
        })?;
    }
    let codegen = &sidecar.descriptor_codegen;
    if !codegen.is_supported() {
        return Err(invalid(
            "descriptor retention/codegen policy is incompatible".to_string(),
        ));
    }
    Ok(())
}

fn validate_cached_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path is empty".to_string());
    }
    for component in path.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(format!("invalid path component `{component}`"));
        }
        if component.contains('\\')
            || component.bytes().any(|byte| byte == 0)
            || component
                .chars()
                .any(|character| character.is_whitespace() && character != ' ')
        {
            return Err(format!("non-portable path component `{component}`"));
        }
    }
    Ok(())
}

fn validate_cached_extra_filename(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("the discriminator is empty".to_string());
    }
    if value.contains(['/', '\\']) {
        return Err("path separators are not allowed".to_string());
    }
    if let Some((offset, character)) = value
        .char_indices()
        .find(|(_, character)| character.is_control())
    {
        return Err(format!(
            "control character U+{:04X} at byte offset {offset} is not allowed",
            u32::from(character)
        ));
    }
    Ok(())
}

fn cached_sidecar_path(cache: &Path, generation: &CachedUnitGeneration) -> PathBuf {
    cache
        .join("units")
        .join(&generation.cargo_unit_id)
        .join(format!("{}.json", generation.sidecar_hash))
}

fn cached_object_path(cache: &Path, generation: &CachedUnitGeneration) -> PathBuf {
    cache
        .join("objects")
        .join(&generation.cargo_unit_id)
        .join(format!("{}.o", generation.descriptor_object_hash))
}

fn cached_sidecar_hash(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(CACHED_SIDECAR_HASH_DOMAIN);
    hash.update(bytes);
    encode_digest(&hash.finalize().into())
}

fn require_file(
    path: &Path,
    unit: DescriptorCargoUnitId,
    kind: CacheUnitFileKind,
) -> Result<(), DescriptorCacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(DescriptorCacheError::MissingCachedUnitFile {
                unit,
                kind,
                path: path.to_path_buf(),
            })
        }
        Err(source) => Err(DescriptorCacheError::ReadCacheFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_cached_unit_inputs(
    units_by_id: &BTreeMap<DescriptorCargoUnitId, &ValidatedDescriptorUnit>,
    cached_ids: &BTreeSet<DescriptorCargoUnitId>,
    descriptor_target: &Path,
    cargo_artifacts: &[CargoCompilerArtifact],
) -> Result<DescriptorInputHashes, DescriptorCacheError> {
    let mut timing = super::timing::StageTimer::new();
    validate_cached_observed_inputs(units_by_id, cached_ids, descriptor_target)?;
    timing.mark("cache validate observed source generated and environment inputs");
    let hashes = validate_cached_dependency_artifacts(units_by_id, cached_ids, cargo_artifacts)?;
    timing.mark("cache validate dependency artifact bytes");
    Ok(hashes)
}

fn validate_cached_observed_inputs(
    units_by_id: &BTreeMap<DescriptorCargoUnitId, &ValidatedDescriptorUnit>,
    cached_ids: &BTreeSet<DescriptorCargoUnitId>,
    descriptor_target: &Path,
) -> Result<(), DescriptorCacheError> {
    let units = cached_ids
        .iter()
        .map(|unit_id| {
            units_by_id
                .get(unit_id)
                .copied()
                .ok_or(DescriptorCacheError::MissingMergedUnit(*unit_id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Each unit validates its full source/generated/environment input set.
    // Collect before propagating errors so diagnostics keep canonical unit-ID
    // order regardless of worker scheduling, as object/extern hashing does.
    let validations = units
        .par_iter()
        .map(|unit| {
            validate_descriptor_observed_inputs(descriptor_target, &unit.sidecar).map_err(
                |source| DescriptorCacheError::CachedOutDirDependencyMismatch {
                    unit: unit.unit,
                    source,
                },
            )
        })
        .collect::<Vec<_>>();
    for validation in validations {
        validation?;
    }
    Ok(())
}

fn validate_cached_dependency_artifacts(
    units_by_id: &BTreeMap<DescriptorCargoUnitId, &ValidatedDescriptorUnit>,
    cached_ids: &BTreeSet<DescriptorCargoUnitId>,
    cargo_artifacts: &[CargoCompilerArtifact],
) -> Result<DescriptorInputHashes, DescriptorCacheError> {
    let mut enumerated = BTreeMap::<PathBuf, BTreeSet<usize>>::new();
    for (artifact_index, artifact) in cargo_artifacts.iter().enumerate() {
        for path in &artifact.filenames {
            if classify_dependency_artifact(artifact, path).is_some() {
                enumerated
                    .entry(path.clone())
                    .or_default()
                    .insert(artifact_index);
            }
        }
        // A root package's lib is reported at its uplifted destination while
        // same-package dependents link the hashed `deps/` rlib the message
        // never lists; the content hash of that exact file is still verified
        // below.
        if let Some(path) = uplifted_root_lib_hashed_rlib(artifact) {
            enumerated.entry(path).or_default().insert(artifact_index);
        }
    }

    let mut expected_by_path = BTreeMap::<PathBuf, (String, DescriptorCargoUnitId, String)>::new();
    for unit_id in cached_ids {
        let unit = units_by_id
            .get(unit_id)
            .ok_or(DescriptorCacheError::MissingMergedUnit(*unit_id))?;
        for (extern_name, artifact) in &unit.sidecar.dependency_artifacts {
            match expected_by_path.entry(artifact.path.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((artifact.content_hash.clone(), *unit_id, extern_name.clone()));
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get().0 != artifact.content_hash =>
                {
                    return Err(DescriptorCacheError::ConflictingSidecarArtifactEvidence {
                        path: artifact.path.clone(),
                        first: entry.get().0.clone(),
                        second: artifact.content_hash.clone(),
                        unit: *unit_id,
                        extern_name: extern_name.clone(),
                    });
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
    }

    let expectations = expected_by_path.into_iter().collect::<Vec<_>>();
    for (path, (_, unit, extern_name)) in &expectations {
        let matches = enumerated.get(path).map_or(0, BTreeSet::len);
        match matches {
            0 => {
                return Err(DescriptorCacheError::CachedDependencyNotEnumerated {
                    unit: *unit,
                    extern_name: extern_name.clone(),
                    path: path.clone(),
                });
            }
            1 => {}
            matches => {
                return Err(DescriptorCacheError::AmbiguousCachedDependencyArtifact {
                    unit: *unit,
                    extern_name: extern_name.clone(),
                    path: path.clone(),
                    matches,
                });
            }
        }
    }
    let actuals = expectations
        .par_iter()
        .map(|(path, _)| descriptor_input_link_hashes(path))
        .collect::<Vec<_>>();
    let mut hashes = BTreeMap::new();
    for ((path, (expected, unit, extern_name)), actual) in expectations.into_iter().zip(actuals) {
        let actual = actual.map_err(|source| DescriptorCacheError::HashCurrentInput {
            path: path.clone(),
            source,
        })?;
        let actual_hash = encode_digest(&actual.0);
        if actual_hash != expected {
            return Err(DescriptorCacheError::StaleCachedDependency {
                unit,
                extern_name,
                path,
                expected,
                actual: actual_hash,
            });
        }
        hashes.insert(path, actual);
    }
    Ok(hashes)
}

fn validate_cached_artifact_joins(
    units_by_id: &BTreeMap<DescriptorCargoUnitId, &ValidatedDescriptorUnit>,
    cached_ids: &BTreeSet<DescriptorCargoUnitId>,
    expected_artifact_by_cached_unit: &BTreeMap<DescriptorCargoUnitId, String>,
    cargo_artifacts: &[CargoCompilerArtifact],
    evidence: &[ArtifactEvidence],
    exact_snapshot: bool,
) -> Result<(), DescriptorCacheError> {
    for unit_id in cached_ids {
        let unit = *units_by_id
            .get(unit_id)
            .ok_or(DescriptorCacheError::MissingMergedUnit(*unit_id))?;
        match join_descriptor_units_to_cargo_artifacts(std::slice::from_ref(unit), cargo_artifacts)
        {
            Ok(joined) => {
                let artifact = joined
                    .get(unit_id)
                    .expect("one-unit join returns the requested unit");
                let actual = coordinate_for_artifact(artifact, evidence)?;
                if let Some(expected) = expected_artifact_by_cached_unit.get(unit_id) {
                    if *expected != actual {
                        return Err(DescriptorCacheError::CachedArtifactJoinMismatch {
                            unit: *unit_id,
                            expected: expected.clone(),
                            actual,
                        });
                    }
                }
            }
            Err(UnitArtifactJoinError::MissingArtifact(_)) if exact_snapshot => {
                // Some rustc units have no distinct Cargo compiler-artifact.
                // They are reusable only through a snapshot authorized by the
                // exact complete artifact set from the current Cargo run.
            }
            Err(source) => return Err(DescriptorCacheError::ArtifactJoin(source)),
        }
    }
    Ok(())
}

fn persist_fresh_units(
    cache_directory: &Path,
    units: &[ValidatedDescriptorUnit],
) -> Result<BTreeMap<DescriptorCargoUnitId, CachedUnitGeneration>, DescriptorCacheError> {
    // The caller holds the cache lease. Each generation has its own paths,
    // unique temporary names, and atomic verified publication, so independent
    // compiler units can publish concurrently. Await every result before any
    // indexes become visible and report errors in the original unit order.
    let publications = units
        .par_iter()
        .map(|unit| -> Result<_, DescriptorCacheError> {
            let sidecar_bytes = fs::read(&unit.sidecar_path).map_err(|source| {
                DescriptorCacheError::ReadCacheSource {
                    path: unit.sidecar_path.clone(),
                    source,
                }
            })?;
            let generation = CachedUnitGeneration {
                cargo_unit_id: unit.unit.to_string(),
                descriptor_object_hash: unit.sidecar.descriptor_object_hash.clone(),
                sidecar_hash: cached_sidecar_hash(&sidecar_bytes),
            };
            generation.validate_hashes(&unit.sidecar_path)?;
            atomic_copy_descriptor_object_verified(
                &unit.descriptor_object,
                &cached_object_path(cache_directory, &generation),
                unit.unit,
                &generation.descriptor_object_hash,
            )?;
            atomic_write_immutable_bytes(
                &cached_sidecar_path(cache_directory, &generation),
                &sidecar_bytes,
            )?;
            Ok((unit.unit, generation))
        })
        .collect::<Vec<_>>();
    let mut generations = BTreeMap::new();
    for publication in publications {
        let (unit, generation) = publication?;
        if let Some(previous) = generations.insert(unit, generation.clone()) {
            return Err(DescriptorCacheError::ConflictingCachedGeneration {
                unit,
                first: cached_generation_label(&previous),
                second: cached_generation_label(&generation),
            });
        }
    }
    Ok(generations)
}

fn cached_generation_label(generation: &CachedUnitGeneration) -> String {
    format!(
        "object={},sidecar={}",
        generation.descriptor_object_hash, generation.sidecar_hash
    )
}

fn atomic_copy_descriptor_object_verified(
    source: &Path,
    destination: &Path,
    unit: DescriptorCargoUnitId,
    expected: &str,
) -> Result<(), DescriptorCacheError> {
    let _profile_span = super::timing::ProfileSpan::new("atomic_copy_descriptor_object_verified");
    use super::timing::{FileProfile, Work};
    let mut profile = FileProfile::new("publish descriptor object", source, 1);
    let parent = destination
        .parent()
        .ok_or_else(|| DescriptorCacheError::CachePathWithoutParent(destination.to_path_buf()))?;
    ensure_plain_directory(parent)?;
    let nonce = CACHE_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| DescriptorCacheError::InvalidCacheFilename(destination.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let mut input = profile
        .measure(Work::Read, || File::open(source))
        .map_err(|source_error| DescriptorCacheError::ReadCacheSource {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    let mut hash = Sha256::new();
    hash.update(DESCRIPTOR_OBJECT_HASH_DOMAIN);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = profile
            .measure(Work::Read, || input.read(&mut buffer))
            .map_err(|source_error| DescriptorCacheError::ReadCacheSource {
                path: source.to_path_buf(),
                source: source_error,
            })?;
        if read == 0 {
            break;
        }
        profile.bytes(read);
        profile.measure(Work::Hash, || hash.update(&buffer[..read]));
        profile
            .measure(Work::Write, || output.write_all(&buffer[..read]))
            .map_err(|source| DescriptorCacheError::WriteCacheFile {
                path: temporary.clone(),
                source,
            })?;
    }
    profile
        .measure(Work::Sync, || output.sync_all())
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    drop(output);
    let actual = encode_digest(&hash.finalize().into());
    if actual != expected {
        let _ = fs::remove_file(&temporary);
        return Err(DescriptorCacheError::DescriptorObjectHashMismatch {
            unit,
            expected: expected.to_string(),
            actual,
        });
    }
    install_immutable(&temporary, destination)
}

fn persist_artifact_indexes(
    cache_directory: &Path,
    units: &[ValidatedDescriptorUnit],
    generations: &BTreeMap<DescriptorCargoUnitId, CachedUnitGeneration>,
    cargo_artifacts: &[CargoCompilerArtifact],
    evidence: &[ArtifactEvidence],
) -> Result<(), DescriptorCacheError> {
    let joined = join_cargo_enumerated_descriptor_units(units, cargo_artifacts)
        .map_err(DescriptorCacheError::ArtifactJoin)?;
    let mut units_by_artifact = BTreeMap::<String, Vec<CachedUnitGeneration>>::new();
    for (unit, artifact) in joined {
        let coordinate = coordinate_for_artifact(&artifact, evidence)?;
        let generation = generations
            .get(&unit)
            .ok_or(DescriptorCacheError::MissingCachedGeneration(unit))?
            .clone();
        units_by_artifact
            .entry(coordinate)
            .or_default()
            .push(generation);
    }
    let mut indexes = Vec::with_capacity(evidence.len());
    for artifact in evidence {
        let mut units = units_by_artifact
            .remove(&artifact.coordinate)
            .unwrap_or_default();
        units.sort();
        let index = ArtifactCacheIndex {
            schema_version: DESCRIPTOR_CACHE_SCHEMA_VERSION,
            artifact_coordinate: artifact.coordinate.clone(),
            units,
        };
        let bytes = serde_json::to_vec_pretty(&index)
            .map_err(DescriptorCacheError::SerializeCacheRecord)?;
        indexes.push((
            cache_directory
                .join("artifacts")
                .join(format!("{}.json", artifact.coordinate)),
            bytes,
        ));
    }
    // Artifact index paths are independent. Publish them under the same lease,
    // then commit the invocation snapshot only after all results succeed.
    let publications = indexes
        .par_iter()
        .map(|(path, bytes)| atomic_write_replaceable(path, bytes))
        .collect::<Vec<_>>();
    for publication in publications {
        publication?;
    }
    Ok(())
}

fn persist_snapshot(
    cache_directory: &Path,
    artifact_set_coordinate: &str,
    evidence: &[ArtifactEvidence],
    units: impl IntoIterator<Item = CachedUnitGeneration>,
) -> Result<(), DescriptorCacheError> {
    let snapshot = ArtifactSetSnapshot {
        schema_version: DESCRIPTOR_CACHE_SCHEMA_VERSION,
        artifact_set_coordinate: artifact_set_coordinate.to_string(),
        artifact_coordinates: evidence
            .iter()
            .map(|artifact| artifact.coordinate.clone())
            .collect(),
        units: units.into_iter().collect(),
    };
    let bytes =
        serde_json::to_vec_pretty(&snapshot).map_err(DescriptorCacheError::SerializeCacheRecord)?;
    atomic_write_replaceable(
        &cache_directory
            .join("snapshots")
            .join(format!("{artifact_set_coordinate}.json")),
        &bytes,
    )
}

fn atomic_write_immutable_bytes(
    destination: &Path,
    bytes: &[u8],
) -> Result<(), DescriptorCacheError> {
    let _profile_span = super::timing::ProfileSpan::new("atomic_write_immutable_bytes");
    let parent = destination
        .parent()
        .ok_or_else(|| DescriptorCacheError::CachePathWithoutParent(destination.to_path_buf()))?;
    ensure_plain_directory(parent)?;
    let nonce = CACHE_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| DescriptorCacheError::InvalidCacheFilename(destination.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    drop(output);
    install_immutable(&temporary, destination)
}

fn atomic_write_replaceable(path: &Path, bytes: &[u8]) -> Result<(), DescriptorCacheError> {
    let _profile_span = super::timing::ProfileSpan::new("atomic_write_replaceable");
    let parent = path
        .parent()
        .ok_or_else(|| DescriptorCacheError::CachePathWithoutParent(path.to_path_buf()))?;
    ensure_plain_directory(parent)?;
    // The caller retains the generation lease. These pointers were already
    // atomically published and flushed when their contents last changed; an
    // identical pointer needs neither replacement nor another durability flush.
    // Compare the complete on-disk representation, including its final newline.
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(DescriptorCacheError::UnsafeCacheEntry(path.to_path_buf()));
        }
        Ok(metadata) if metadata.len() == bytes.len() as u64 + 1 => {
            let existing =
                fs::read(path).map_err(|source| DescriptorCacheError::ReadCacheFile {
                    path: path.to_path_buf(),
                    source,
                })?;
            if existing.strip_suffix(b"\n") == Some(bytes) {
                return Ok(());
            }
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DescriptorCacheError::ReadCacheFile {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    let nonce = CACHE_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| DescriptorCacheError::InvalidCacheFilename(path.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| DescriptorCacheError::WriteCacheFile {
            path: temporary.clone(),
            source,
        })?;
    drop(file);

    // Artifact coordinates deliberately exclude mutable source/output bytes,
    // so their unit-set indexes and complete snapshots are atomic pointers to
    // the latest Cargo-authorized generation rather than immutable blobs.
    if let Err(source) = replace_file_atomic(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(DescriptorCacheError::WriteCacheFile {
            path: path.to_path_buf(),
            source,
        });
    }
    sync_directory(parent).map_err(|source| DescriptorCacheError::WriteCacheFile {
        path: parent.to_path_buf(),
        source,
    })
}

#[cfg(not(windows))]
fn replace_file_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_file_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVE_FILE_REPLACE_EXISTING, MOVE_FILE_WRITE_THROUGH,
    };

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVE_FILE_REPLACE_EXISTING | MOVE_FILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    let _profile_span = super::timing::ProfileSpan::new("sync_directory");
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    // MoveFileExW uses MOVE_FILE_WRITE_THROUGH above. Other non-Unix targets
    // have no portable directory-sync primitive in std.
    Ok(())
}

fn install_immutable(temporary: &Path, destination: &Path) -> Result<(), DescriptorCacheError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            let _ = fs::remove_file(temporary);
            return Err(DescriptorCacheError::UnsafeCacheEntry(
                destination.to_path_buf(),
            ));
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            let _ = fs::remove_file(temporary);
            return Err(DescriptorCacheError::ReadCacheFile {
                path: destination.to_path_buf(),
                source,
            });
        }
    }
    match fs::hard_link(temporary, destination) {
        Ok(()) => {
            fs::remove_file(temporary).map_err(|source| DescriptorCacheError::WriteCacheFile {
                path: temporary.to_path_buf(),
                source,
            })?;
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let equal = files_equal(temporary, destination)?;
            let _ = fs::remove_file(temporary);
            if equal {
                Ok(())
            } else {
                Err(DescriptorCacheError::ConflictingCacheEntry(
                    destination.to_path_buf(),
                ))
            }
        }
        Err(source) => Err(DescriptorCacheError::WriteCacheFile {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn files_equal(left: &Path, right: &Path) -> Result<bool, DescriptorCacheError> {
    let _profile_span = super::timing::ProfileSpan::new("files_equal");
    let left_metadata =
        fs::metadata(left).map_err(|source| DescriptorCacheError::ReadCacheSource {
            path: left.to_path_buf(),
            source,
        })?;
    let right_metadata =
        fs::metadata(right).map_err(|source| DescriptorCacheError::ReadCacheSource {
            path: right.to_path_buf(),
            source,
        })?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left_file =
        File::open(left).map_err(|source| DescriptorCacheError::ReadCacheSource {
            path: left.to_path_buf(),
            source,
        })?;
    let mut right_file =
        File::open(right).map_err(|source| DescriptorCacheError::ReadCacheSource {
            path: right.to_path_buf(),
            source,
        })?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left_file.read(&mut left_buffer).map_err(|source| {
            DescriptorCacheError::ReadCacheSource {
                path: left.to_path_buf(),
                source,
            }
        })?;
        let right_read = right_file.read(&mut right_buffer).map_err(|source| {
            DescriptorCacheError::ReadCacheSource {
                path: right.to_path_buf(),
                source,
            }
        })?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn canonical_json(value: &Value) -> Vec<u8> {
    fn write(value: &Value, output: &mut Vec<u8>) {
        match value {
            Value::Null => output.extend_from_slice(b"n"),
            Value::Bool(value) => {
                output.extend_from_slice(if *value { b"t" } else { b"f" });
            }
            Value::Number(value) => {
                output.extend_from_slice(b"d");
                output.extend_from_slice(value.to_string().as_bytes());
                output.push(0);
            }
            Value::String(value) => {
                output.extend_from_slice(b"s");
                output.extend_from_slice(&(value.len() as u64).to_be_bytes());
                output.extend_from_slice(value.as_bytes());
            }
            Value::Array(values) => {
                output.extend_from_slice(b"a");
                output.extend_from_slice(&(values.len() as u64).to_be_bytes());
                for value in values {
                    write(value, output);
                }
            }
            Value::Object(values) => {
                output.extend_from_slice(b"o");
                output.extend_from_slice(&(values.len() as u64).to_be_bytes());
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort();
                for key in keys {
                    output.extend_from_slice(&(key.len() as u64).to_be_bytes());
                    output.extend_from_slice(key.as_bytes());
                    write(&values[key], output);
                }
            }
        }
    }

    let mut output = Vec::new();
    write(value, &mut output);
    output
}

struct CacheHasher(Sha256);

impl CacheHasher {
    fn new(domain: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update((domain.len() as u64).to_be_bytes());
        hash.update(domain);
        Self(hash)
    }

    fn field(&mut self, name: &str, value: &[u8]) {
        self.0.update((name.len() as u64).to_be_bytes());
        self.0.update(name.as_bytes());
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    fn set<'a>(&mut self, name: &str, values: impl IntoIterator<Item = &'a [u8]>) {
        let values = values.into_iter().collect::<Vec<_>>();
        self.field(&format!("{name}:len"), &(values.len() as u64).to_be_bytes());
        for value in values {
            self.field(name, value);
        }
    }

    fn path(&mut self, name: &str, path: &Path) -> Result<(), DescriptorCacheError> {
        let path = path
            .to_str()
            .ok_or_else(|| DescriptorCacheError::NonUtf8CargoPath(path.to_path_buf()))?;
        self.field(name, path.as_bytes());
        Ok(())
    }

    fn finish(self) -> String {
        encode_digest(&self.0.finalize().into())
    }
}

fn encode_digest(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Kind of immutable per-unit cache file.
#[derive(Clone, Copy, Debug)]
pub enum CacheUnitFileKind {
    /// JSON unit sidecar.
    Sidecar,
    /// Exact descriptor object.
    Object,
}

impl std::fmt::Display for CacheUnitFileKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sidecar => formatter.write_str("sidecar"),
            Self::Object => formatter.write_str("object"),
        }
    }
}

/// Fail-closed descriptor-cache error.
#[derive(Debug, Error)]
pub enum DescriptorCacheError {
    /// Cache identity must not depend on the caller's current directory.
    #[error("descriptor cache directory must be absolute: `{0}`")]
    RelativeCacheDirectory(PathBuf),
    /// Every managed directory must use an absolute deletion-safe identity.
    #[error("descriptor {kind} directory must be absolute: `{path}`")]
    RelativeManagedDirectory {
        /// Directory role.
        kind: &'static str,
        /// Rejected path.
        path: PathBuf,
    },
    /// Symlinks and lexical aliases are resolved before overlap checks.
    #[error("failed to normalize descriptor managed directory `{path}`: {source}")]
    NormalizeManagedDirectory {
        /// Caller path.
        path: PathBuf,
        /// Canonicalization failure.
        #[source]
        source: io::Error,
    },
    /// Cache, staging, and descriptor target are exact disjoint roots.
    #[error("descriptor {first_kind} `{first}` and {second_kind} `{second}` must not overlap")]
    OverlappingManagedDirectories {
        /// First role.
        first_kind: &'static str,
        /// First normalized path.
        first: PathBuf,
        /// Second role.
        second_kind: &'static str,
        /// Second normalized path.
        second: PathBuf,
    },
    /// Recovery roots may never contain the Cargo working directory.
    #[error(
        "descriptor {kind} recovery root `{managed}` would contain working directory `{working}`"
    )]
    UnsafeManagedDirectory {
        /// Managed role.
        kind: &'static str,
        /// Candidate deletion root.
        managed: PathBuf,
        /// Cargo working directory.
        working: PathBuf,
    },
    /// The exact descriptor target must be within the phase-invariant generated root.
    #[error("descriptor target `{target}` is outside source exclusion `{exclusion}`")]
    TargetOutsideSourceExclusion {
        /// Exact target.
        target: PathBuf,
        /// Exact generated root.
        exclusion: PathBuf,
    },
    /// Excluding an ancestor of the package would erase actual source inputs.
    #[error(
        "descriptor source exclusion `{exclusion}` would contain working directory `{working}`"
    )]
    UnsafeSourceExclusion {
        /// Exact generated root.
        exclusion: PathBuf,
        /// Cargo working directory.
        working: PathBuf,
    },
    /// In-package cache state must live beneath the one exact excluded subtree.
    #[error("descriptor {kind} `{path}` must be inside in-package source exclusion `{exclusion}`")]
    GeneratedStateOutsideSourceExclusion {
        /// Managed role.
        kind: &'static str,
        /// Managed path.
        path: PathBuf,
        /// Exact excluded subtree.
        exclusion: PathBuf,
    },
    /// Persistent cache layout could not be created.
    #[error("failed to create descriptor cache directory `{path}`: {source}")]
    CreateCacheDirectory {
        /// Directory.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Cache and lock namespaces must not be redirected by symlinks/special files.
    #[error("descriptor cache entry is not a plain regular file/directory: `{0}`")]
    UnsafeCacheEntry(PathBuf),
    /// Per-cache generation lock could not be opened.
    #[error("failed to open descriptor cache lock `{path}`: {source}")]
    OpenCacheLock {
        /// Exact lock path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Another process owns this cache generation until the OS lock releases.
    #[error("failed to acquire descriptor cache lock `{path}`: {source}")]
    AcquireCacheLock {
        /// Exact lock path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// The crash marker itself may not be redirected through a symlink.
    #[error("descriptor cache dirty marker is not a regular file: `{0}`")]
    InvalidDirtyMarker(PathBuf),
    /// A dirty cache cannot authorize deletion of another target namespace.
    #[error(
        "descriptor cache dirty marker belongs to target `{actual}`, not requested target `{expected}`"
    )]
    DirtyTargetMismatch {
        /// Current request target.
        expected: PathBuf,
        /// Interrupted target.
        actual: PathBuf,
    },
    /// Rebinding a normalized journal path to the request's lexical source
    /// exclusion must preserve its exact filesystem identity.
    #[error(
        "descriptor cache dirty staging path resolves to `{actual}`, not journaled `{expected}`"
    )]
    DirtyStagingMismatch {
        /// Journaled canonical staging identity.
        expected: PathBuf,
        /// Request-spelled staging identity.
        actual: PathBuf,
    },
    /// Recovery requires a Cargo run in the freshly recreated target.
    #[error("descriptor cache recovery did not force every Cargo artifact to rebuild")]
    RecoveryDidNotFullyRebuild,
    /// Recovery deletion is allowed only for a marker-proven target.
    #[error("refusing to reset unowned descriptor target `{0}`")]
    UnownedRecoveryTarget(PathBuf),
    /// Staging deletion requires its own runner-written marker.
    #[error("refusing to reset unowned descriptor staging directory `{0}`")]
    UnownedRecoveryStaging(PathBuf),
    /// Target marker inspection failed.
    #[error("failed to read descriptor target ownership marker in `{path}`: {source}")]
    ReadDescriptorTargetMarker {
        /// Exact target.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Staging ownership marker inspection failed.
    #[error("failed to read descriptor staging ownership marker in `{path}`: {source}")]
    ReadDescriptorStagingMarker {
        /// Exact staging root.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A recovery path was replaced with a file or symlink.
    #[error("refusing to remove non-directory descriptor {kind} recovery path `{path}`")]
    UnsafeRecoveryDirectory {
        /// Managed role.
        kind: &'static str,
        /// Exact path.
        path: PathBuf,
    },
    /// Exact request-owned recovery state could not be removed.
    #[error("failed to remove descriptor {kind} recovery directory `{path}`: {source}")]
    RemoveRecoveryDirectory {
        /// Managed role.
        kind: &'static str,
        /// Exact path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Descriptor Cargo run failed.
    #[error(transparent)]
    DescriptorCargo(DescriptorCargoRunError),
    /// A current Cargo-enumerated input could not be hashed.
    #[error("failed to hash current Cargo input `{path}`: {source}")]
    HashCurrentInput {
        /// Exact current path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Two descriptor rustc invocations observed different bytes at one exact
    /// Cargo artifact path.
    #[error(
        "descriptor sidecars disagree about Cargo artifact `{path}`: unit {unit} extern `{extern_name}` reports `{second}`, but another unit reports `{first}`"
    )]
    ConflictingSidecarArtifactEvidence {
        /// Exact host-local Cargo path.
        path: PathBuf,
        /// First wrapper-time descriptor-domain hash.
        first: String,
        /// Conflicting wrapper-time descriptor-domain hash.
        second: String,
        /// Unit contributing the conflict.
        unit: DescriptorCargoUnitId,
        /// Extern contributing the conflict.
        extern_name: String,
    },
    /// Cargo paths originate in JSON and must remain exact UTF-8 strings.
    #[error("current Cargo artifact path is not UTF-8: `{0}`")]
    NonUtf8CargoPath(PathBuf),
    /// Two current artifacts collapsed to one deterministic Cargo coordinate.
    #[error("current Cargo artifacts have duplicate coordinate `{0}`")]
    DuplicateArtifactCoordinate(String),
    /// A joined artifact unexpectedly disappeared from current evidence.
    #[error("joined Cargo artifact is absent from current artifact evidence")]
    ArtifactMissingFromCurrentEvidence,
    /// Current Cargo messages contained indistinguishable artifacts.
    #[error("joined Cargo artifact has ambiguous current evidence")]
    AmbiguousCurrentArtifactEvidence,
    /// Multiple fresh units cannot own one Cargo artifact.
    #[error("current artifact `{coordinate}` joined fresh units {first} and {second}")]
    MultipleFreshUnitsForArtifact {
        /// Current artifact coordinate.
        coordinate: String,
        /// First fresh unit.
        first: DescriptorCargoUnitId,
        /// Second fresh unit.
        second: DescriptorCargoUnitId,
    },
    /// No-op recovery requires the exact prior complete artifact-set snapshot.
    #[error("descriptor cache has no exact snapshot for current artifact set `{coordinate}`")]
    MissingArtifactSetSnapshot {
        /// Exact current artifact-set coordinate.
        coordinate: String,
    },
    /// A fresh Cargo artifact has no explicit positive or negative cache index.
    #[error("descriptor cache has no index for fresh Cargo artifact `{coordinate}`")]
    MissingArtifactIndex {
        /// Exact current artifact coordinate.
        coordinate: String,
    },
    /// A rebuilt artifact emitted no fresh wrapper unit.
    #[error("rebuilt Cargo artifact `{coordinate}` emitted no fresh descriptor wrapper unit")]
    RebuiltArtifactMissingFreshUnit {
        /// Exact artifact coordinate.
        coordinate: String,
    },
    /// Interrupted staging is authoritative only when its unit joins an exact
    /// artifact from the recovery Cargo invocation.
    #[error("interrupted descriptor recovery unit {0} has no exact current Cargo artifact")]
    InterruptedUnitMissingArtifact(DescriptorCargoUnitId),
    /// Two recovery attempts claimed the same unit with different validated
    /// sidecar content, so neither is preferred implicitly.
    #[error("interrupted descriptor recovery has conflicting candidates for unit {0}")]
    ConflictingInterruptedUnitCandidates(DescriptorCargoUnitId),
    /// Corrupt cache JSON.
    #[error("failed to parse descriptor cache file `{path}`: {source}")]
    ParseCacheFile {
        /// Cache record.
        path: PathBuf,
        /// JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// Cache file read failed.
    #[error("failed to read descriptor cache file `{path}`: {source}")]
    ReadCacheFile {
        /// Cache record.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Cache schema mismatch.
    #[error("descriptor cache file `{path}` uses schema {actual}; expected {expected}")]
    UnsupportedCacheSchema {
        /// Cache record.
        path: PathBuf,
        /// Actual schema.
        actual: u16,
        /// Required schema.
        expected: u16,
    },
    /// A cached wrapper sidecar uses another wrapper schema.
    #[error("cached descriptor sidecar `{path}` uses schema {actual}; expected {expected}")]
    CachedSidecarSchemaMismatch {
        /// Cached sidecar.
        path: PathBuf,
        /// Actual schema.
        actual: u16,
        /// Required schema.
        expected: u16,
    },
    /// Cache filename/unit selection and sidecar identity diverged.
    #[error("cached descriptor sidecar for unit {unit} reports unit `{actual}`")]
    CachedSidecarUnitMismatch {
        /// Cache-selected unit.
        unit: DescriptorCargoUnitId,
        /// Sidecar unit text.
        actual: String,
    },
    /// A cached sidecar points outside its exact content-addressed object slot.
    #[error(
        "cached descriptor sidecar for unit {unit} reports object `{actual}`; expected `{expected}`"
    )]
    CachedSidecarObjectPathMismatch {
        /// Cache-selected unit.
        unit: DescriptorCargoUnitId,
        /// Sidecar path.
        actual: String,
        /// Required path.
        expected: String,
    },
    /// Coordinate-addressed filename and record diverged.
    #[error("descriptor cache file `{path}` reports coordinate `{actual}`; expected `{expected}`")]
    CacheCoordinateMismatch {
        /// Cache record.
        path: PathBuf,
        /// Filename-derived identity.
        expected: String,
        /// Payload identity.
        actual: String,
    },
    /// Exact snapshot lists another artifact set.
    #[error("descriptor cache snapshot `{0}` does not list the current artifact set")]
    SnapshotArtifactSetMismatch(PathBuf),
    /// Unit ID text in cache is malformed.
    #[error("descriptor cache file `{path}` has an invalid unit ID: {source}")]
    InvalidCachedUnitId {
        /// Cache record.
        path: PathBuf,
        /// ID parser.
        #[source]
        source: crate::rsjs::build::WrapperError,
    },
    /// Generation pointers contain only exact lowercase content hashes.
    #[error("descriptor cache file `{path}` has invalid {field} `{value}`: {reason}")]
    InvalidCachedGenerationHash {
        /// Pointer record.
        path: PathBuf,
        /// Hash field.
        field: &'static str,
        /// Rejected value.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cache record repeats a unit.
    #[error("descriptor cache file `{path}` repeats unit {unit}")]
    DuplicateCachedUnit {
        /// Cache record.
        path: PathBuf,
        /// Duplicate.
        unit: DescriptorCargoUnitId,
    },
    /// One unit cannot be authorized by two current artifacts.
    #[error("cached unit {unit} is claimed by current artifacts `{first}` and `{second}`")]
    CachedUnitClaimedByMultipleArtifacts {
        /// Unit.
        unit: DescriptorCargoUnitId,
        /// First artifact.
        first: String,
        /// Second artifact.
        second: String,
    },
    /// Explicitly indexed cache bytes are gone.
    #[error("cached unit {unit} {kind} is missing at `{path}`")]
    MissingCachedUnitFile {
        /// Unit.
        unit: DescriptorCargoUnitId,
        /// File role.
        kind: CacheUnitFileKind,
        /// Exact cache path.
        path: PathBuf,
    },
    /// Fresh and cached staging did not contain an authorized unit.
    #[error("merged descriptor staging omitted authorized unit {0}")]
    MissingMergedUnit(DescriptorCargoUnitId),
    /// One materialized unit cannot have two verified object hashes.
    #[error(
        "descriptor unit {unit} has conflicting prevalidated object hashes `{first}` and `{second}`"
    )]
    ConflictingPrevalidatedObjectHash {
        /// Materialized unit.
        unit: DescriptorCargoUnitId,
        /// First verified hash.
        first: String,
        /// Conflicting verified hash.
        second: String,
    },
    /// One logical Cargo unit cannot select two cache generations at once.
    #[error("descriptor unit {unit} has conflicting cache generations `{first}` and `{second}`")]
    ConflictingCachedGeneration {
        /// Logical unit.
        unit: DescriptorCargoUnitId,
        /// First exact generation.
        first: String,
        /// Second exact generation.
        second: String,
    },
    /// Every merged sidecar must have a committed immutable generation.
    #[error("descriptor unit {0} has no committed cache generation")]
    MissingCachedGeneration(DescriptorCargoUnitId),
    /// Pointer and sidecar must agree on the immutable object blob.
    #[error(
        "cached descriptor unit {unit} pointer requires object hash `{pointer}`, but sidecar reports `{sidecar}`"
    )]
    CachedGenerationObjectHashMismatch {
        /// Logical unit.
        unit: DescriptorCargoUnitId,
        /// Pointer hash.
        pointer: String,
        /// Sidecar hash.
        sidecar: String,
    },
    /// Immutable sidecar bytes must match their generation address.
    #[error(
        "cached descriptor sidecar for unit {unit} has hash `{actual}`; expected `{expected}`"
    )]
    CachedSidecarHashMismatch {
        /// Logical unit.
        unit: DescriptorCargoUnitId,
        /// Generation address.
        expected: String,
        /// Actual exact bytes.
        actual: String,
    },
    /// A content-addressed sidecar failed the wrapper protocol's semantic
    /// validation before its cache object was admitted to the linker.
    #[error("cached descriptor sidecar for unit {unit} at `{path}` is invalid: {reason}")]
    InvalidCachedSidecar {
        /// Logical unit.
        unit: DescriptorCargoUnitId,
        /// Exact immutable sidecar path.
        path: PathBuf,
        /// Value-redacted validation detail.
        reason: String,
    },
    /// A cached unit's rustc-declared generated-source inputs changed or are
    /// no longer safe beneath the persistent descriptor target.
    #[error("cached descriptor unit {unit} OUT_DIR dependency validation failed: {source}")]
    CachedOutDirDependencyMismatch {
        /// Cached unit.
        unit: DescriptorCargoUnitId,
        /// Exact generated-input validation failure.
        #[source]
        source: crate::rsjs::build::WrapperError,
    },
    /// Cached unit points at a dependency absent from current Cargo artifacts.
    #[error(
        "cached descriptor unit {unit} extern `{extern_name}` path `{path}` is absent from current Cargo artifacts"
    )]
    CachedDependencyNotEnumerated {
        /// Cached unit.
        unit: DescriptorCargoUnitId,
        /// Extern.
        extern_name: String,
        /// Exact wrapper-time path.
        path: PathBuf,
    },
    /// One exact dependency path must belong to one Cargo artifact message.
    #[error(
        "cached descriptor unit {unit} extern `{extern_name}` path `{path}` matches {matches} current Cargo artifacts"
    )]
    AmbiguousCachedDependencyArtifact {
        /// Cached unit.
        unit: DescriptorCargoUnitId,
        /// Extern.
        extern_name: String,
        /// Exact wrapper-time path.
        path: PathBuf,
        /// Matching Cargo messages.
        matches: usize,
    },
    /// Cached dependency bytes changed after the wrapper observed them.
    #[error(
        "cached descriptor unit {unit} extern `{extern_name}` path `{path}` has hash `{actual}`; expected `{expected}`"
    )]
    StaleCachedDependency {
        /// Unit.
        unit: DescriptorCargoUnitId,
        /// Extern.
        extern_name: String,
        /// Exact wrapper-time path.
        path: PathBuf,
        /// Wrapper-time exact input hash.
        expected: String,
        /// Current exact input hash.
        actual: String,
    },
    /// Artifact-specific index did not join back to that exact current artifact.
    #[error(
        "cached unit {unit} joined current artifact `{actual}` instead of indexed artifact `{expected}`"
    )]
    CachedArtifactJoinMismatch {
        /// Unit.
        unit: DescriptorCargoUnitId,
        /// Index expectation.
        expected: String,
        /// Actual current join.
        actual: String,
    },
    /// Merged sidecars/objects failed existing strict validation.
    #[error(transparent)]
    MergedSidecars(DescriptorSidecarError),
    /// Sidecar/Cargo join failed.
    #[error(transparent)]
    ArtifactJoin(UnitArtifactJoinError),
    /// Cache record serialization failed.
    #[error("failed to serialize descriptor cache record: {0}")]
    SerializeCacheRecord(serde_json::Error),
    /// Cached object's exact bytes disagree with its sidecar.
    #[error("descriptor object for unit {unit} has hash `{actual}`; expected `{expected}`")]
    DescriptorObjectHashMismatch {
        /// Unit.
        unit: DescriptorCargoUnitId,
        /// Sidecar.
        expected: String,
        /// Current bytes.
        actual: String,
    },
    /// Cache path has no parent.
    #[error("descriptor cache path has no parent: `{0}`")]
    CachePathWithoutParent(PathBuf),
    /// Cache filename cannot support an atomic temporary sibling.
    #[error("descriptor cache path has an invalid filename: `{0}`")]
    InvalidCacheFilename(PathBuf),
    /// Source bytes for an atomic cache copy could not be read.
    #[error("failed to read descriptor cache source `{path}`: {source}")]
    ReadCacheSource {
        /// Source.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Atomic cache write failed.
    #[error("failed to atomically write descriptor cache file `{path}`: {source}")]
    WriteCacheFile {
        /// Destination/temporary.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Immutable content address already contains different bytes.
    #[error("descriptor cache content address already contains different bytes: `{0}`")]
    ConflictingCacheEntry(PathBuf),
}

#[cfg(test)]
mod tests;
